use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::process::{Child, Command, Stdio};
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
use serde::Deserialize;

// ── Audio constants ───────────────────────────────────────────────────────────

const SAMPLE_RATE: u32 = 32_000;
const CHUNK_SAMPLES: usize = SAMPLE_RATE as usize * 5;   // 5-second transcription window
const LEVEL_INTERVAL: Duration = Duration::from_millis(100);
const PIPEWIRE_DEVICE: &str = "pipewire";

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

fn spawn_reader(
    stdout: std::process::ChildStdout,
    tx: Sender<Update>,
    db_path: Option<String>,
    transcript_path: Option<String>,
) {
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

// ── Audio capture (Tier 2) ────────────────────────────────────────────────────

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

fn write_chunk_wav(samples: &[i16]) -> io::Result<std::path::PathBuf> {
    let tmp = tempfile::NamedTempFile::new()?;
    let f = tmp.reopen()?;
    let mut writer = hound::WavWriter::new(
        BufWriter::new(f),
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    for &s in samples {
        writer
            .write_sample(s)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    }
    writer
        .finalize()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    // keep() prevents auto-delete on drop; Python deletes after transcription
    let (_, path) = tmp
        .keep()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    Ok(path)
}

/// Starts cpal audio capture and a processing thread.
/// The processing thread:
///   - Emits Level updates to the TUI every 100 ms
///   - Accumulates 5-second chunks, writes each as a WAV temp file,
///     and sends the path to Python's stdin for transcription.
/// Returns the cpal Stream (must stay alive for the duration of recording).
fn spawn_audio(
    tx: Sender<Update>,
    python_stdin: std::process::ChildStdin,
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
        let mut stdin_writer = BufWriter::new(python_stdin);
        let mut chunk_buf: Vec<i16> = Vec::new();
        let mut level_last = Instant::now();

        while running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(20));

            // Drain new samples collected by the cpal callback
            let new_samples: Vec<i16> = {
                let mut guard = shared.lock().unwrap();
                std::mem::take(&mut *guard)
            };

            if !new_samples.is_empty() {
                chunk_buf.extend_from_slice(&new_samples);

                // Level update every ~100 ms
                if level_last.elapsed() >= LEVEL_INTERVAL {
                    let rms = rms_f64(&new_samples);
                    let _ = tx.send(Update::Level(rms));
                    level_last = Instant::now();
                }
            }

            // Drain completed 5-second chunks → WAV file → send path to Python
            while chunk_buf.len() >= CHUNK_SAMPLES {
                let chunk: Vec<i16> = chunk_buf.drain(..CHUNK_SAMPLES).collect();
                match write_chunk_wav(&chunk) {
                    Ok(path) => {
                        if writeln!(stdin_writer, "{}", path.display()).is_err()
                            || stdin_writer.flush().is_err()
                        {
                            return; // Python stdin closed
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Update::Status(format!("chunk error: {e}")));
                    }
                }
            }
        }
        // Close Python's stdin gracefully → Python's for-loop exits → emits done
        drop(stdin_writer);
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
            .style(Style::default().fg(Color::Green)),
        rows[0],
    );

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
    let model    = get_arg("--model")
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

    // ── Spawn Python inference daemon ─────────────────────────────────────────
    // stdin: Rust sends WAV file paths (one per line)
    // stdout: Python emits JSON (status, utterance, done)
    let mut child: Child = Command::new(&python)
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("VOICE_WHISPER_MODEL", &model)
        .env("LD_LIBRARY_PATH", "/usr/lib/ollama")
        .spawn()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let child_stdin  = child.stdin.take().unwrap();
    let child_stdout = child.stdout.take().unwrap();

    // ── Reader thread: parse Python JSON → Update channel ────────────────────
    let (tx, rx): (Sender<Update>, Receiver<Update>) = mpsc::channel();
    spawn_reader(child_stdout, tx.clone(), db_path.clone(), transcript_path.clone());

    // ── Audio capture: cpal → RMS → 5s WAV chunks → Python stdin ─────────────
    let running = Arc::new(AtomicBool::new(true));
    let _stream = spawn_audio(tx, child_stdin, running.clone());

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
                    _ => {}
                }
            }
        }
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    // Signal audio thread to stop (closes Python stdin → Python emits done + exits)
    running.store(false, Ordering::Relaxed);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    child.kill().ok();
    child.wait().ok();

    Ok(())
}
