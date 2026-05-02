use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use arboard::Clipboard;
use cpal::traits::{DeviceTrait, HostTrait};
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
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Page {
    Welcome,
    SystemCheck,
    ModeSelect,
    Options,
    Launch,
}

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Type,
    Print,
    Clip,
    Ambient,
}

impl Mode {
    fn as_str(&self) -> &'static str {
        match self {
            Mode::Type    => "type",
            Mode::Print   => "print",
            Mode::Clip    => "clip",
            Mode::Ambient => "ambient",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
struct CheckResult {
    label: &'static str,
    status: CheckStatus,
    detail: String,
}

const MODELS: &[&str] = &["large-v3", "large-v2", "medium", "small", "base"];

struct App {
    page: Page,
    checks: Vec<CheckResult>,
    mode_cursor: usize,
    selected_mode: Option<Mode>,
    opt_submit: bool,
    opt_no_save: bool,
    opt_db_enabled: bool,
    opt_model_idx: usize,
    opt_cursor: usize,
    launch_msg: Option<String>,
    quit: bool,
}

impl App {
    fn new(checks: Vec<CheckResult>) -> Self {
        App {
            page: Page::Welcome,
            checks,
            mode_cursor: 0,
            selected_mode: None,
            opt_submit: false,
            opt_no_save: false,
            opt_db_enabled: false,
            opt_model_idx: 0,
            opt_cursor: 0,
            launch_msg: None,
            quit: false,
        }
    }

    fn model(&self) -> &str {
        MODELS[self.opt_model_idx]
    }
}

// ── quiet_stderr ──────────────────────────────────────────────────────────────

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

// ── System checks ─────────────────────────────────────────────────────────────

fn run_checks() -> Vec<CheckResult> {
    let mut results = Vec::new();

    // 1. GPU / CUDA
    {
        let (status, detail) = if std::path::Path::new("/dev/nvidia0").exists() {
            (CheckStatus::Pass, "/dev/nvidia0 present".to_string())
        } else {
            let ok = Command::new("nvidia-smi")
                .arg("--query-gpu=name")
                .arg("--format=csv,noheader")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                (CheckStatus::Pass, "nvidia-smi reports GPU".to_string())
            } else {
                (CheckStatus::Warn, "no GPU — whisper-rs will fall back to CPU".to_string())
            }
        };
        results.push(CheckResult { label: "GPU / CUDA", status, detail });
    }

    // 2. Whisper model
    {
        let home = std::env::var("HOME").unwrap_or_default();
        let path = PathBuf::from(&home).join(".cache/whisper/ggml-large-v3.bin");
        let (status, detail) = if path.exists() {
            (CheckStatus::Pass, path.display().to_string())
        } else {
            (CheckStatus::Fail, format!("not found: {}", path.display()))
        };
        results.push(CheckResult { label: "Whisper model", status, detail });
    }

    // 3. Audio device
    {
        let found = quiet_stderr(|| {
            let host = cpal::default_host();
            host.input_devices()
                .ok()
                .and_then(|mut devs| {
                    devs.find(|d| {
                        d.name()
                            .map(|n| n.to_lowercase().contains("pipewire"))
                            .unwrap_or(false)
                    })
                })
                .map(|d| d.name().unwrap_or_default())
        });
        let (status, detail) = if let Some(name) = found {
            (CheckStatus::Pass, format!("found: \"{}\"", name))
        } else {
            let any = quiet_stderr(|| {
                cpal::default_host().default_input_device().is_some()
            });
            if any {
                (CheckStatus::Warn, "pipewire not found; will use default input device".to_string())
            } else {
                (CheckStatus::Fail, "no audio input devices found".to_string())
            }
        };
        results.push(CheckResult { label: "Audio device", status, detail });
    }

    // 4. voice-input binary
    {
        let (status, detail) = find_binary("voice-input");
        results.push(CheckResult { label: "voice-input binary", status, detail });
    }

    // 5. voice-ambient binary
    {
        let (status, detail) = find_binary("voice-ambient");
        results.push(CheckResult { label: "voice-ambient binary", status, detail });
    }

    results
}

fn find_binary(name: &str) -> (CheckStatus, String) {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return (CheckStatus::Pass, candidate.display().to_string());
            }
        }
    }
    let found = Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());
    match found {
        Some(p) => (CheckStatus::Pass, p),
        None    => (CheckStatus::Warn, format!("{} not found in PATH", name)),
    }
}

// ── Command assembly ──────────────────────────────────────────────────────────

fn assemble_command(app: &App) -> String {
    let mode = match &app.selected_mode {
        Some(m) => m,
        None    => return "voice-input".to_string(),
    };
    let mut parts = vec![
        "voice-input".to_string(),
        "--mode".to_string(),
        mode.as_str().to_string(),
    ];
    if app.model() != "large-v3" {
        parts.push("--model".to_string());
        parts.push(app.model().to_string());
    }
    if *mode == Mode::Type && app.opt_submit {
        parts.push("--submit".to_string());
    }
    if *mode == Mode::Ambient {
        if app.opt_no_save {
            parts.push("--no-save".to_string());
        }
        if app.opt_db_enabled {
            let home = std::env::var("HOME").unwrap_or_default();
            parts.push("--db".to_string());
            parts.push(format!("{}/.local/share/voice-input/sessions.db", home));
        }
    }
    parts.join(" ")
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(area);

    draw_breadcrumb(frame, app, outer[0]);
    match &app.page {
        Page::Welcome     => draw_welcome(frame, outer[1]),
        Page::SystemCheck => draw_system_check(frame, app, outer[1]),
        Page::ModeSelect  => draw_mode_select(frame, app, outer[1]),
        Page::Options     => draw_options(frame, app, outer[1]),
        Page::Launch      => draw_launch(frame, app, outer[1]),
    }
    draw_global_footer(frame, outer[2]);
}

fn draw_breadcrumb(frame: &mut Frame, app: &App, area: Rect) {
    let steps: &[(&str, &Page)] = &[
        ("1/5 Welcome",      &Page::Welcome),
        ("2/5 System Check", &Page::SystemCheck),
        ("3/5 Mode Select",  &Page::ModeSelect),
        ("4/5 Options",      &Page::Options),
        ("5/5 Launch",       &Page::Launch),
    ];
    let page_order = [
        &Page::Welcome,
        &Page::SystemCheck,
        &Page::ModeSelect,
        &Page::Options,
        &Page::Launch,
    ];
    let current_idx = page_order.iter().position(|p| **p == app.page).unwrap_or(0);

    let mut spans: Vec<Span> = Vec::new();
    for (i, (label, _)) in steps.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" → ", Style::default().fg(Color::DarkGray)));
        }
        let style = if i == current_idx {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if i < current_idx {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(*label, style));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

fn draw_global_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            "  q/Esc: quit at any time",
            Style::default().fg(Color::DarkGray),
        )),
        area,
    );
}

fn draw_welcome(frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "  voice-input  ·  setup wizard",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Push-to-talk speech recognition for Linux terminals.",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                "  Powered by whisper-rs (whisper.cpp) with CUDA GPU acceleration.",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" voice-input wizard "),
        ),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  This wizard ", Style::default().fg(Color::White)),
                Span::styled("checks prerequisites", Style::default().fg(Color::Cyan)),
                Span::styled(", helps you ", Style::default().fg(Color::White)),
                Span::styled("pick a mode", Style::default().fg(Color::Cyan)),
                Span::styled(", and ", Style::default().fg(Color::White)),
                Span::styled("builds the command", Style::default().fg(Color::Cyan)),
                Span::styled(" to run.", Style::default().fg(Color::White)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Modes:  ", Style::default().fg(Color::DarkGray)),
                Span::styled("type", Style::default().fg(Color::Yellow)),
                Span::styled(" (→ active window)   ", Style::default().fg(Color::DarkGray)),
                Span::styled("print", Style::default().fg(Color::Yellow)),
                Span::styled(" (→ stdout)   ", Style::default().fg(Color::DarkGray)),
                Span::styled("clip", Style::default().fg(Color::Yellow)),
                Span::styled(" (→ clipboard)   ", Style::default().fg(Color::DarkGray)),
                Span::styled("ambient", Style::default().fg(Color::Yellow)),
                Span::styled(" (continuous TUI)", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Audio:  ", Style::default().fg(Color::DarkGray)),
                Span::styled("press Enter to stop recording", Style::default().fg(Color::White)),
                Span::styled("  ·  max 65 s  ·  low beep = start, high beep = done", Style::default().fg(Color::DarkGray)),
            ]),
        ]),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  → / Enter: continue   q: quit",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[2],
    );
}

fn draw_system_check(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  System Prerequisites",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );

    let items: Vec<ListItem> = app.checks.iter().map(|c| {
        let (glyph, glyph_color) = match c.status {
            CheckStatus::Pass => ("[OK]", Color::Green),
            CheckStatus::Warn => ("[!!]", Color::Yellow),
            CheckStatus::Fail => ("[XX]", Color::Red),
        };
        ListItem::new(Line::from(vec![
            Span::styled(
                format!("  {} ", glyph),
                Style::default().fg(glyph_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<24}", c.label),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                c.detail.as_str(),
                Style::default().fg(Color::DarkGray),
            ),
        ]))
    }).collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" checks "),
        ),
        chunks[1],
    );

    let pass = app.checks.iter().filter(|c| c.status == CheckStatus::Pass).count();
    let warn = app.checks.iter().filter(|c| c.status == CheckStatus::Warn).count();
    let fail = app.checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{} passed  ", pass), Style::default().fg(Color::Green)),
                Span::styled(format!("{} warning  ", warn), Style::default().fg(Color::Yellow)),
                Span::styled(format!("{} failed", fail), Style::default().fg(Color::Red)),
            ]),
            Line::from(Span::styled(
                "  ← back   → continue (warnings are non-fatal)",
                Style::default().fg(Color::DarkGray),
            )),
        ]),
        chunks[2],
    );
}

fn draw_mode_select(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  Select a mode:",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );

    let mode_defs: &[(&str, &str)] = &[
        ("type   ", "record speech → transcribe → type into active window (X11)"),
        ("print  ", "record speech → transcribe → print to stdout"),
        ("clip   ", "record speech → transcribe → copy to clipboard"),
        ("ambient", "continuous ambient TUI  ·  live transcript  ·  SQLite logging"),
    ];

    let items: Vec<ListItem> = mode_defs.iter().enumerate().map(|(i, (name, desc))| {
        let selected = i == app.mode_cursor;
        let (fg, bg) = if selected {
            (Color::Black, Color::Cyan)
        } else {
            (Color::White, Color::Reset)
        };
        let indicator = if selected { "▶ " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::styled(
                format!("{}  {:<8}  —  ", indicator, name),
                Style::default().fg(fg).bg(bg),
            ),
            Span::styled(
                *desc,
                Style::default().fg(if selected { Color::Black } else { Color::DarkGray }).bg(bg),
            ),
        ]))
    }).collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" modes "),
        ),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  ↑/↓: select   Enter / →: confirm   ← back",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[2],
    );
}

#[allow(unused_assignments)]
fn build_option_items(app: &App, mode: &Mode) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut idx = 0usize;

    macro_rules! push_row {
        ($flag:expr, $val:expr) => {{
            let focused = app.opt_cursor == idx;
            let style = if focused {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let indicator = if focused { "▶ " } else { "  " };
            items.push(ListItem::new(Line::from(vec![Span::styled(
                format!("{}  {:<14}  {}", indicator, $flag, $val),
                style,
            )])));
            idx += 1;
        }};
    }

    match mode {
        Mode::Type => {
            push_row!("--submit", if app.opt_submit { "[x] send Return after typing" } else { "[ ] do not send Return" });
            push_row!("--model", format!("[{}]  Space to cycle: large-v3 → large-v2 → medium → small → base", app.model()));
        }
        Mode::Print | Mode::Clip => {
            push_row!("--model", format!("[{}]  Space to cycle: large-v3 → large-v2 → medium → small → base", app.model()));
        }
        Mode::Ambient => {
            push_row!("--no-save", if app.opt_no_save { "[x] transcript auto-save disabled" } else { "[ ] transcript auto-saved to ~/.local/share/voice-input/" });
            push_row!("--db", if app.opt_db_enabled { "[x] SQLite session logging on" } else { "[ ] SQLite session logging off" });
            push_row!("--model", format!("[{}]  Space to cycle: large-v3 → large-v2 → medium → small → base", app.model()));
        }
    }

    items
}

fn draw_options(frame: &mut Frame, app: &App, area: Rect) {
    let mode = match &app.selected_mode {
        Some(m) => m,
        None    => return,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "  Options for mode: ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(mode.as_str(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ])),
        chunks[0],
    );

    let items = build_option_items(app, mode);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" options "),
        ),
        chunks[1],
    );

    let cmd = assemble_command(app);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", cmd),
            Style::default().fg(Color::Yellow),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" command preview "),
        ),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  ↑/↓: move   Space: toggle / cycle model   → / Enter: continue   ← back",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[3],
    );
}

fn draw_launch(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "  ✓ Ready!  ",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Copy the command below or launch it directly.",
                Style::default().fg(Color::White),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        ),
        chunks[1],
    );

    let cmd = assemble_command(app);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", cmd),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" command "),
        ),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[C] Copy to clipboard   ", Style::default().fg(Color::Cyan)),
            Span::styled("[L] Launch now   ", Style::default().fg(Color::Green)),
            Span::styled("[←] Back   ", Style::default().fg(Color::White)),
            Span::styled("[Q] Quit", Style::default().fg(Color::DarkGray)),
        ])),
        chunks[4],
    );

    if let Some(msg) = &app.launch_msg {
        let is_err = msg.contains("error") || msg.contains("Error") || msg.contains("fail");
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("  {}", msg),
                Style::default().fg(if is_err { Color::Red } else { Color::Green }),
            )),
            chunks[5],
        );
    }

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  ← back   q: quit",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[6],
    );
}

// ── Actions ───────────────────────────────────────────────────────────────────

fn do_copy(app: &mut App) {
    let cmd = assemble_command(app);
    match Clipboard::new() {
        Ok(mut clip) => match clip.set_text(&cmd) {
            Ok(_)  => app.launch_msg = Some("Copied to clipboard!".to_string()),
            Err(e) => app.launch_msg = Some(format!("Clipboard error: {}", e)),
        },
        Err(e) => app.launch_msg = Some(format!("Clipboard error: {}", e)),
    }
}

fn do_launch(app: &App) {
    use std::os::unix::process::CommandExt;
    let cmd = assemble_command(app);
    // Restore terminal before exec so the launched program has a clean TTY
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    // Replace this process — does not return on success
    let err = Command::new("sh").args(["-c", &cmd]).exec();
    eprintln!("launch failed: {}", err);
    std::process::exit(1);
}

// ── Event handling ────────────────────────────────────────────────────────────

fn option_count(mode: &Mode) -> usize {
    match mode {
        Mode::Type    => 2,
        Mode::Print   => 1,
        Mode::Clip    => 1,
        Mode::Ambient => 3,
    }
}

fn advance_page(app: &mut App) {
    match app.page {
        Page::Welcome     => app.page = Page::SystemCheck,
        Page::SystemCheck => app.page = Page::ModeSelect,
        Page::ModeSelect  => {
            app.selected_mode = Some(match app.mode_cursor {
                0 => Mode::Type,
                1 => Mode::Print,
                2 => Mode::Clip,
                _ => Mode::Ambient,
            });
            app.opt_cursor = 0;
            app.page = Page::Options;
        }
        Page::Options => app.page = Page::Launch,
        Page::Launch  => app.quit = true,
    }
}

fn retreat_page(app: &mut App) {
    match app.page {
        Page::Welcome     => {}
        Page::SystemCheck => app.page = Page::Welcome,
        Page::ModeSelect  => app.page = Page::SystemCheck,
        Page::Options     => app.page = Page::ModeSelect,
        Page::Launch      => app.page = Page::Options,
    }
}

fn toggle_current_option(app: &mut App) {
    let mode = match app.selected_mode.clone() {
        Some(m) => m,
        None    => return,
    };
    match mode {
        Mode::Type => match app.opt_cursor {
            0 => app.opt_submit = !app.opt_submit,
            _ => app.opt_model_idx = (app.opt_model_idx + 1) % MODELS.len(),
        },
        Mode::Print | Mode::Clip => {
            app.opt_model_idx = (app.opt_model_idx + 1) % MODELS.len();
        }
        Mode::Ambient => match app.opt_cursor {
            0 => app.opt_no_save = !app.opt_no_save,
            1 => app.opt_db_enabled = !app.opt_db_enabled,
            _ => app.opt_model_idx = (app.opt_model_idx + 1) % MODELS.len(),
        },
    }
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    // Ctrl+C always quits
    if code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL) {
        app.quit = true;
        return;
    }

    let page = app.page.clone();
    match page {
        Page::Launch => match code {
            KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
            KeyCode::Char('c') => do_copy(app),
            KeyCode::Char('l') => do_launch(app),
            KeyCode::Left      => retreat_page(app),
            KeyCode::Right | KeyCode::Enter => {}  // already at last page
            _ => {}
        },
        Page::ModeSelect => match code {
            KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
            KeyCode::Up   => { if app.mode_cursor > 0 { app.mode_cursor -= 1; } }
            KeyCode::Down => { if app.mode_cursor < 3 { app.mode_cursor += 1; } }
            KeyCode::Right | KeyCode::Enter => advance_page(app),
            KeyCode::Left => retreat_page(app),
            _ => {}
        },
        Page::Options => {
            let max = app.selected_mode.as_ref()
                .map(|m| option_count(m).saturating_sub(1))
                .unwrap_or(0);
            match code {
                KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
                KeyCode::Up   => { if app.opt_cursor > 0 { app.opt_cursor -= 1; } }
                KeyCode::Down => { if app.opt_cursor < max { app.opt_cursor += 1; } }
                KeyCode::Char(' ') => toggle_current_option(app),
                KeyCode::Right | KeyCode::Enter => advance_page(app),
                KeyCode::Left => retreat_page(app),
                _ => {}
            }
        },
        _ => match code {
            KeyCode::Char('q') | KeyCode::Esc => app.quit = true,
            KeyCode::Right | KeyCode::Enter => advance_page(app),
            KeyCode::Left => retreat_page(app),
            _ => {}
        },
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    // Run checks before raw mode so any cpal/libc stderr goes to the terminal cleanly
    let checks = run_checks();

    // Restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut app = App::new(checks);

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(k) = event::read()? {
                handle_key(&mut app, k.code, k.modifiers);
            }
        }

        if app.quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
