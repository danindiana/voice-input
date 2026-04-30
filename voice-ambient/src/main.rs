use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline},
    Frame, Terminal,
};
use serde::Deserialize;

// ── Messages from Python ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Msg {
    Status { msg: String },
    Level { rms: f64 },
    Utterance { text: String, ts: String },
    Done,
}

// ── App state ─────────────────────────────────────────────────────────────────

struct Utterance {
    timestamp: String,
    text: String,
    born: Instant,
}

struct App {
    utterances: VecDeque<Utterance>,
    level_history: Vec<u64>,  // 0-100, fed to Sparkline
    current_rms: f64,
    status_msg: String,
    word_count: u64,
    utt_count: u64,
    start: Instant,
    blink: bool,
    last_blink: Instant,
    db_path: Option<String>,
    transcript_path: Option<String>,
}

impl App {
    fn new(db_path: Option<String>, transcript_path: Option<String>) -> Self {
        App {
            utterances: VecDeque::new(),
            level_history: vec![0u64; 100],
            current_rms: 0.0,
            status_msg: "Initializing...".to_string(),
            word_count: 0,
            utt_count: 0,
            start: Instant::now(),
            blink: true,
            last_blink: Instant::now(),
            db_path,
            transcript_path,
        }
    }

    fn push_level(&mut self, rms: f64) {
        self.current_rms = rms;
        self.level_history.push((rms * 100.0) as u64);
        if self.level_history.len() > 100 {
            self.level_history.remove(0);
        }
    }

    fn push_utterance(&mut self, ts: String, text: String) {
        self.word_count += text.split_whitespace().count() as u64;
        self.utt_count += 1;
        // ISO 8601 → local HH:MM:SS display (chars 11-19 of UTC string)
        let ts_display = ts.get(11..19).unwrap_or(&ts).to_string();
        self.utterances.push_back(Utterance {
            timestamp: ts_display,
            text,
            born: Instant::now(),
        });
        while self.utterances.len() > 500 {
            self.utterances.pop_front();
        }
    }

    fn elapsed_str(&self) -> String {
        let s = self.start.elapsed().as_secs();
        format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

// ── Update channel ────────────────────────────────────────────────────────────

enum Update {
    Status(String),
    Level(f64),
    Utterance { ts: String, text: String },
    Quit,
}

// ── SQLite helpers ────────────────────────────────────────────────────────────

fn open_db(path: &str) -> rusqlite::Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
             id         INTEGER PRIMARY KEY AUTOINCREMENT,
             started_at TEXT NOT NULL,
             ended_at   TEXT
         );
         CREATE TABLE IF NOT EXISTS utterances (
             id          INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id  INTEGER NOT NULL,
             recorded_at TEXT NOT NULL,
             text        TEXT NOT NULL,
             word_count  INTEGER NOT NULL
         );",
    )?;
    Ok(conn)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── Reader thread ─────────────────────────────────────────────────────────────

fn spawn_reader(stdout: std::process::ChildStdout, tx: Sender<Update>, db_path: Option<String>, transcript_path: Option<String>) {
    thread::spawn(move || {
        let conn = db_path.as_deref().and_then(|p| open_db(p).ok());
        let mut transcript_file = transcript_path.as_deref().and_then(|p| {
            fs::OpenOptions::new().create(true).append(true).open(p).ok()
        });

        let session_id: i64 = conn.as_ref().map_or(0, |c| {
            let _ = c.execute(
                "INSERT INTO sessions (started_at) VALUES (?1)",
                rusqlite::params![now_iso()],
            );
            c.last_insert_rowid()
        });

        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let msg: Msg = match serde_json::from_str(&line) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let update = match msg {
                Msg::Status { msg } => Update::Status(msg),
                Msg::Level { rms } => Update::Level(rms),
                Msg::Utterance { text, ts } => {
                    if let Some(ref c) = conn {
                        let wc = text.split_whitespace().count() as i64;
                        let _ = c.execute(
                            "INSERT INTO utterances \
                             (session_id, recorded_at, text, word_count) \
                             VALUES (?1, ?2, ?3, ?4)",
                            rusqlite::params![session_id, ts, text, wc],
                        );
                    }
                    if let Some(ref mut f) = transcript_file {
                        let ts_display = ts.get(11..19).unwrap_or(&ts);
                        let _ = writeln!(f, "[{}] {}", ts_display, text);
                    }
                    Update::Utterance { ts, text }
                }
                Msg::Done => {
                    if let Some(ref c) = conn {
                        let _ = c.execute(
                            "UPDATE sessions SET ended_at = ?1 WHERE id = ?2",
                            rusqlite::params![now_iso(), session_id],
                        );
                    }
                    let _ = tx.send(Update::Quit);
                    return;
                }
            };
            if tx.send(update).is_err() {
                break;
            }
        }
        let _ = tx.send(Update::Quit);
    });
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header: REC + title + elapsed
            Constraint::Length(4), // audio: sparkline waveform + level gauge
            Constraint::Min(4),    // transcript waterfall
            Constraint::Length(3), // footer: status / stats / db
        ])
        .split(area);

    draw_header(frame, app, chunks[0]);
    draw_audio(frame, app, chunks[1]);
    draw_transcript(frame, app, chunks[2]);
    draw_footer(frame, app, chunks[3]);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (rec, rec_color) = if app.blink {
        ("● REC", Color::Red)
    } else {
        ("○ REC", Color::DarkGray)
    };
    let line = Line::from(vec![
        Span::styled(rec, Style::default().fg(rec_color).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(
            "voice-input : ambient",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(app.elapsed_str(), Style::default().fg(Color::White)),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

fn draw_audio(frame: &mut Frame, app: &App, area: Rect) {
    // Split into sparkline (top 2 rows) + gauge (bottom 2 rows)
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2)])
        .split(area);

    // Scrolling waveform — the "waterfall": oldest sample on left, newest on right
    frame.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::TOP)
                    .title(" waveform "),
            )
            .data(&app.level_history)
            .style(Style::default().fg(Color::Green)),
        rows[0],
    );

    // Current level gauge with colour zones
    let pct = (app.current_rms * 100.0).min(100.0) as u16;
    let bar_color = if pct > 75 {
        Color::Red
    } else if pct > 40 {
        Color::Yellow
    } else {
        Color::Green
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM))
            .gauge_style(Style::default().fg(bar_color).bg(Color::Black))
            .percent(pct),
        rows[1],
    );
}

fn draw_transcript(frame: &mut Frame, app: &App, area: Rect) {
    let height = area.height.saturating_sub(2) as usize;
    let now = Instant::now();

    // Show the last `height` utterances; newest at bottom, oldest at top.
    // Age-based fade: cyan → white → gray → dark-gray as utterances recede.
    let items: Vec<ListItem> = app
        .utterances
        .iter()
        .rev()
        .take(height)
        .rev()
        .map(|u| {
            let age = now.duration_since(u.born).as_secs();
            let color = match age {
                0..=4   => Color::Cyan,
                5..=19  => Color::White,
                20..=59 => Color::Gray,
                _       => Color::DarkGray,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", u.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(u.text.as_str(), Style::default().fg(color)),
            ]))
        })
        .collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" transcript "),
        ),
        area,
    );
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let db_span = match &app.db_path {
        Some(p) => Span::styled(
            format!("DB: {}  ", p),
            Style::default().fg(Color::Cyan),
        ),
        None => Span::styled("DB: off  ", Style::default().fg(Color::DarkGray)),
    };
    let save_span = match &app.transcript_path {
        Some(p) => {
            let name = std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p.as_str());
            Span::styled(format!("SAVE: {}  ", name), Style::default().fg(Color::Green))
        }
        None => Span::styled("SAVE: off  ", Style::default().fg(Color::DarkGray)),
    };
    let line = Line::from(vec![
        Span::styled(app.status_msg.as_str(), Style::default().fg(Color::Green)),
        Span::raw("  │  "),
        Span::styled(
            format!("words: {}  utt: {}", app.word_count, app.utt_count),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  │  "),
        db_span,
        Span::raw("  │  "),
        save_span,
        Span::styled("  q / Ctrl-C to stop", Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    // ── Parse CLI args ────────────────────────────────────────────────────────
    let args: Vec<String> = std::env::args().collect();
    let get_arg = |flag: &str| -> Option<String> {
        args.windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].clone())
    };

    let script   = get_arg("--script").unwrap_or_default();
    let python   = get_arg("--python").unwrap_or_default();
    let db_path  = get_arg("--db");
    let no_save  = args.contains(&"--no-save".to_string());

    let transcript_path: Option<String> = if no_save {
        None
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        let dir  = format!("{}/.local/share/voice-input/transcripts", home);
        let _    = fs::create_dir_all(&dir);
        let ts   = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        Some(format!("{}/{}.txt", dir, ts))
    };

    // ── Spawn Python subprocess ───────────────────────────────────────────────
    let mut child: Child = Command::new(&python)
        .arg(&script)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let child_stdout = child.stdout.take().unwrap();

    // ── Start reader thread ───────────────────────────────────────────────────
    let (tx, rx): (Sender<Update>, Receiver<Update>) = mpsc::channel();
    spawn_reader(child_stdout, tx, db_path.clone(), transcript_path.clone());

    // ── Terminal setup ────────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut app = App::new(db_path, transcript_path);

    // ── Render loop ───────────────────────────────────────────────────────────
    'main: loop {
        // Drain all pending updates from reader thread
        loop {
            match rx.try_recv() {
                Ok(Update::Status(msg))           => app.status_msg = msg,
                Ok(Update::Level(rms))             => app.push_level(rms),
                Ok(Update::Utterance { ts, text }) => app.push_utterance(ts, text),
                Ok(Update::Quit)                   => break 'main,
                Err(_)                             => break,
            }
        }

        // Toggle REC blink every 500 ms
        if app.last_blink.elapsed() > Duration::from_millis(500) {
            app.blink = !app.blink;
            app.last_blink = Instant::now();
        }

        terminal.draw(|f| ui(f, &app))?;

        // Keyboard: q, Esc, or Ctrl-C quit
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(k) = event::read()? {
                match (k.code, k.modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    child.kill().ok();
    child.wait().ok();

    Ok(())
}
