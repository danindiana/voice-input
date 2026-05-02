use std::collections::VecDeque;
use std::fs;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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

use voice_ambient::theme::{self, THEMES};
use voice_ambient::whisper_infer;

// ── Audio constants ───────────────────────────────────────────────────────────

const SAMPLE_RATE: u32 = 32_000;
const CHUNK_SAMPLES: usize = SAMPLE_RATE as usize * 5;   // 5-second transcription window
const LEVEL_INTERVAL: Duration = Duration::from_millis(100);
const PIPEWIRE_DEVICE: &str = "pipewire";

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
    theme_idx: usize,
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
            theme_idx: theme::load_theme_idx(),
        }
    }

    fn theme(&self) -> &'static voice_ambient::theme::Theme {
        &THEMES[self.theme_idx]
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

// ── Whisper inference thread ──────────────────────────────────────────────────

fn spawn_whisper(
    model_name: String,
    chunk_rx: std::sync::mpsc::Receiver<Vec<i16>>,
    tx: Sender<Update>,
    db_path: Option<String>,
    transcript_path: Option<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let _ = tx.send(Update::Status(format!("Loading whisper model: {}", model_name)));

        let ctx = whisper_infer::load_ctx_with_fallback(&model_name);
        let _ = tx.send(Update::Status("Recording".to_string()));

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

        for chunk in chunk_rx {
            if let Some(text) = whisper_infer::transcribe_i16(&ctx, &chunk) {
                let ts = now_iso();
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
                if tx.send(Update::Utterance { ts, text }).is_err() {
                    break;
                }
            }
        }

        if let Some(ref c) = conn {
            let _ = c.execute(
                "UPDATE sessions SET ended_at = ?1 WHERE id = ?2",
                rusqlite::params![now_iso(), session_id],
            );
        }

        let _ = tx.send(Update::Quit);
    })
}

// ── Audio capture ─────────────────────────────────────────────────────────────

fn quiet_stderr<F: FnOnce() -> T, T>(f: F) -> T {
    unsafe {
        let null = libc::open(b"/dev/null\0".as_ptr() as *const libc::c_char, libc::O_WRONLY);
        let saved = libc::dup(2);
        libc::dup2(null, 2);
        libc::close(null);
        let r = f();
        libc::dup2(saved, 2);
        libc::close(saved);
        r
    }
}

fn find_pipewire_device() -> cpal::Device {
    quiet_stderr(|| {
        let host = cpal::default_host();
        host.input_devices()
            .expect("cannot enumerate input devices")
            .find(|d| d.name().map(|n| n == PIPEWIRE_DEVICE).unwrap_or(false))
            .unwrap_or_else(|| host.default_input_device().expect("no default input device"))
    })
}

fn rms_f64(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    ((sum_sq / samples.len() as f64).sqrt() / 32768.0).min(1.0)
}

/// Starts cpal audio capture and a processing thread.
/// The processing thread emits Level updates every 100 ms and sends 5-second
/// chunks of i16 samples to `chunk_tx` for in-process whisper transcription.
fn spawn_audio(
    tx: Sender<Update>,
    chunk_tx: std::sync::mpsc::SyncSender<Vec<i16>>,
    running: Arc<AtomicBool>,
) -> cpal::Stream {
    let shared: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let shared_cb = shared.clone();

    let stream = quiet_stderr(|| {
        let device = find_pipewire_device();
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };
        device
            .build_input_stream(
                &config,
                move |data: &[i16], _| {
                    shared_cb.lock().unwrap().extend_from_slice(data);
                },
                |e| eprintln!("[voice-ambient] audio error: {e}"),
                None,
            )
            .expect("failed to build audio stream")
    });
    quiet_stderr(|| stream.play().expect("failed to start audio stream"));

    thread::spawn(move || {
        let mut chunk_buf: Vec<i16> = Vec::new();
        let mut level_last = Instant::now();

        while running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(20));

            let new_samples: Vec<i16> = {
                let mut guard = shared.lock().unwrap();
                std::mem::take(&mut *guard)
            };

            if !new_samples.is_empty() {
                chunk_buf.extend_from_slice(&new_samples);

                if level_last.elapsed() >= LEVEL_INTERVAL {
                    let _ = tx.send(Update::Level(rms_f64(&new_samples)));
                    level_last = Instant::now();
                }
            }

            // Send completed 5-second chunks to whisper thread
            while chunk_buf.len() >= CHUNK_SAMPLES {
                let chunk: Vec<i16> = chunk_buf.drain(..CHUNK_SAMPLES).collect();
                if chunk_tx.send(chunk).is_err() {
                    return;
                }
            }
        }
        // chunk_tx drops here → whisper thread's recv loop ends
    });

    stream
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
    let t = app.theme();
    let (rec, rec_color) = if app.blink {
        ("● REC", t.rec_on)
    } else {
        ("○ REC", t.dim)
    };
    let line = Line::from(vec![
        Span::styled(rec, Style::default().fg(rec_color).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(
            "voice-input : ambient",
            Style::default().fg(t.primary).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(app.elapsed_str(), Style::default().fg(t.text)),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.primary)),
        ),
        area,
    );
}

fn draw_audio(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.theme();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2)])
        .split(area);

    frame.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT | Borders::TOP)
                    .title(" waveform "),
            )
            .data(&app.level_history)
            .style(Style::default().fg(t.success)),
        rows[0],
    );

    // Gauge colors remain semantic (green=low, yellow=medium, red=high audio level)
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
    let t = app.theme();
    let height = area.height.saturating_sub(2) as usize;
    let now = Instant::now();

    let items: Vec<ListItem> = app
        .utterances
        .iter()
        .rev()
        .take(height)
        .rev()
        .map(|u| {
            let age = now.duration_since(u.born).as_secs();
            // Newest text uses theme primary, aging through text → gray → dim
            let color = match age {
                0..=4   => t.primary,
                5..=19  => t.text,
                20..=59 => Color::Gray,
                _       => t.dim,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", u.timestamp),
                    Style::default().fg(t.dim),
                ),
                Span::styled(u.text.as_str(), Style::default().fg(color)),
            ]))
        })
        .collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.primary))
                .title(" transcript "),
        ),
        area,
    );
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let t = app.theme();
    let db_span = match &app.db_path {
        Some(p) => Span::styled(
            format!("DB: {}  ", p),
            Style::default().fg(t.primary),
        ),
        None => Span::styled("DB: off  ", Style::default().fg(t.dim)),
    };
    let save_span = match &app.transcript_path {
        Some(p) => {
            let name = std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p.as_str());
            Span::styled(format!("SAVE: {}  ", name), Style::default().fg(t.success))
        }
        None => Span::styled("SAVE: off  ", Style::default().fg(t.dim)),
    };
    let line = Line::from(vec![
        Span::styled(app.status_msg.as_str(), Style::default().fg(t.success)),
        Span::raw("  │  "),
        Span::styled(
            format!("words: {}  utt: {}", app.word_count, app.utt_count),
            Style::default().fg(t.secondary),
        ),
        Span::raw("  │  "),
        db_span,
        Span::raw("  │  "),
        save_span,
        Span::styled("  t: [", Style::default().fg(t.dim)),
        Span::styled(t.name, Style::default().fg(t.primary).add_modifier(Modifier::BOLD)),
        Span::styled("]  ", Style::default().fg(t.dim)),
        Span::styled("q / Ctrl-C: stop", Style::default().fg(t.dim)),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(t.primary)),
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

    let db_path  = get_arg("--db");
    let no_save  = args.contains(&"--no-save".to_string());
    let model_name = get_arg("--model")
        .or_else(|| std::env::var("VOICE_WHISPER_MODEL").ok())
        .unwrap_or_else(|| "large-v3".to_string());

    let transcript_path: Option<String> = if no_save {
        None
    } else {
        let home = std::env::var("HOME").unwrap_or_default();
        let dir  = format!("{}/.local/share/voice-input/transcripts", home);
        let _    = fs::create_dir_all(&dir);
        let ts   = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        Some(format!("{}/{}.txt", dir, ts))
    };

    // ── Channels ──────────────────────────────────────────────────────────────
    let (tx, rx): (Sender<Update>, Receiver<Update>) = mpsc::channel();
    // Bounded chunk channel: audio → whisper. Buffer=2 to absorb one chunk queued
    // while the previous is being processed.
    let (chunk_tx, chunk_rx) = std::sync::mpsc::sync_channel::<Vec<i16>>(2);

    // ── Whisper inference thread ──────────────────────────────────────────────
    let whisper_handle = spawn_whisper(
        model_name,
        chunk_rx,
        tx.clone(),
        db_path.clone(),
        transcript_path.clone(),
    );

    // ── Audio capture ─────────────────────────────────────────────────────────
    let running = Arc::new(AtomicBool::new(true));
    let _stream = spawn_audio(tx, chunk_tx, running.clone());

    // ── Terminal setup ────────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut app = App::new(db_path, transcript_path);

    // ── Render loop ───────────────────────────────────────────────────────────
    'main: loop {
        loop {
            match rx.try_recv() {
                Ok(Update::Status(msg))           => app.status_msg = msg,
                Ok(Update::Level(rms))             => app.push_level(rms),
                Ok(Update::Utterance { ts, text }) => app.push_utterance(ts, text),
                Ok(Update::Quit)                   => break 'main,
                Err(_)                             => break,
            }
        }

        if app.last_blink.elapsed() > Duration::from_millis(500) {
            app.blink = !app.blink;
            app.last_blink = Instant::now();
        }

        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(k) = event::read()? {
                match (k.code, k.modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Char('t'), _) => {
                        app.theme_idx = (app.theme_idx + 1) % THEMES.len();
                        theme::save_theme_idx(app.theme_idx);
                    }
                    _ => {}
                }
            }
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    running.store(false, Ordering::Relaxed);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Wait for whisper thread to finish current transcription and close DB cleanly.
    let _ = whisper_handle.join();

    Ok(())
}
