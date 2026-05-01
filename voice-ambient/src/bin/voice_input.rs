/// voice-input — Rust replacement for voice-input.sh
///
/// Push-to-talk speech-to-text using cpal (audio), hound (WAV), rodio (beeps),
/// arboard (clipboard), enigo (xdotool replacement), and faster-whisper (Python).
///
/// Device: PipeWire ALSA "pipewire" virtual device → UC03 USB headset at 32 kHz mono.
use std::env;
use std::fs;
use std::io::{BufWriter, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arboard::Clipboard;
use clap::{Parser, ValueEnum};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::bounded;
use enigo::{Enigo, Keyboard, Settings};
use rodio::buffer::SamplesBuffer;
use rodio::OutputStream;
use tempfile::NamedTempFile;

// ── Constants ────────────────────────────────────────────────────────────────

const MAX_RECORD_SECS: u64 = 65;
const SAMPLE_RATE: u32 = 32_000;
const BEEP_LOW_HZ: f32 = 480.0;
const BEEP_HIGH_HZ: f32 = 880.0;
const BEEP_DURATION_SECS: f32 = 0.15;

// The PipeWire ALSA virtual device — routes to WirePlumber default source (UC03).
const PIPEWIRE_DEVICE: &str = "pipewire";

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum Mode {
    /// Type transcript into active window (default)
    Type,
    /// Print transcript to stdout
    Print,
    /// Copy transcript to clipboard
    Clip,
    /// Launch continuous ambient mode TUI
    Ambient,
}

#[derive(Parser, Debug)]
#[command(name = "voice-input", about = "Push-to-talk speech-to-text")]
struct Args {
    #[arg(long, default_value = "type")]
    mode: Mode,

    /// Whisper model name (e.g. large-v3, medium)
    #[arg(long, default_value = "large-v3")]
    model: String,

    /// Disable fancy animated output for --mode=print
    #[arg(long)]
    no_fancy: bool,

    /// After --mode=type, send Return keystroke (auto-submit)
    #[arg(long)]
    submit: bool,

    /// [ambient] disable transcript auto-save
    #[arg(long)]
    no_save: bool,

    /// [ambient] SQLite DB path for session logging
    #[arg(long)]
    db: Option<PathBuf>,
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    if args.mode == Mode::Ambient {
        run_ambient(&args);
        return;
    }

    // Beep: low tone → recording starts
    play_beep(BEEP_LOW_HZ, BEEP_DURATION_SECS);

    eprintln!("[voice-input] Recording… press Enter to stop (auto-stop at {}s)", MAX_RECORD_SECS);

    // Capture audio
    let wav_file = record_to_wav().expect("audio capture failed");

    // Beep: high tone → recording stopped
    play_beep(BEEP_HIGH_HZ, BEEP_DURATION_SECS);

    // For --mode=print: transcription already printed to stdout by subprocess; we're done.
    if args.mode == Mode::Print {
        let _ = transcribe(&wav_file.path().to_path_buf(), &args);
        return;
    }

    // For --clip and --type: capture stdout as plain text
    let text = transcribe(&wav_file.path().to_path_buf(), &args).unwrap_or_default();
    let text = text.trim().to_string();

    if text.is_empty() {
        eprintln!("[voice-input] No transcription");
        return;
    }

    match args.mode {
        Mode::Type => {
            let mut enigo = Enigo::new(&Settings::default()).expect("enigo init failed");
            enigo.text(&text).expect("enigo text failed");
            if args.submit {
                use enigo::Key;
                enigo.key(Key::Return, enigo::Direction::Click).expect("enigo Return failed");
            }
        }
        Mode::Clip => {
            let mut cb = Clipboard::new().expect("clipboard init failed");
            cb.set_text(&text).expect("clipboard write failed");
            eprintln!("[voice-input] Copied {} chars to clipboard", text.len());
        }
        Mode::Print | Mode::Ambient => unreachable!(),
    }
}

// ── Audio recording ───────────────────────────────────────────────────────────

/// Suppress ALSA/JACK probe noise for the duration of f().
/// Saves fd 2, redirects to /dev/null, runs f(), then restores.
fn with_quiet_stderr<F: FnOnce() -> T, T>(f: F) -> T {
    unsafe {
        let null = libc::open(b"/dev/null\0".as_ptr() as *const libc::c_char, libc::O_WRONLY);
        let saved = libc::dup(2);
        libc::dup2(null, 2);
        libc::close(null);
        let result = f();
        libc::dup2(saved, 2);
        libc::close(saved);
        result
    }
}

fn find_pipewire_device() -> cpal::Device {
    // Both default_host() and input_devices() generate ALSA/JACK noise — suppress all of it.
    with_quiet_stderr(|| {
        let host = cpal::default_host();
        host.input_devices()
            .expect("cannot enumerate input devices")
            .find(|d| d.name().map(|n| n == PIPEWIRE_DEVICE).unwrap_or(false))
            .unwrap_or_else(|| {
                host.default_input_device().expect("no default input device")
            })
    })
}

fn record_to_wav() -> Result<NamedTempFile, Box<dyn std::error::Error>> {
    let device = find_pipewire_device();
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let samples_writer = samples.clone();

    // Channel: either timer or Enter press stops recording
    let (tx_stop, rx_stop) = bounded::<()>(1);

    // Timer thread
    let tx_timer = tx_stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(MAX_RECORD_SECS));
        let _ = tx_timer.send(());
    });

    // Enter-key thread (reads from /dev/tty)
    let tx_enter = tx_stop.clone();
    std::thread::spawn(move || {
        if let Ok(mut tty) = fs::File::open("/dev/tty") {
            let mut buf = [0u8; 1];
            let _ = tty.read(&mut buf); // block until any byte (Enter)
            let _ = tx_enter.send(());
        }
    });

    // Build and start cpal stream (stream open may also generate ALSA noise)
    let stream = with_quiet_stderr(|| {
        device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                samples_writer.lock().unwrap().extend_from_slice(data);
            },
            |e| eprintln!("[voice-input] stream error: {e}"),
            None,
        )
    })?;
    with_quiet_stderr(|| stream.play())?;

    // Block until stop signal
    let _ = rx_stop.recv();
    drop(stream);

    // Encode to WAV in a temp file
    let captured = samples.lock().unwrap();
    let secs = captured.len() as f32 / SAMPLE_RATE as f32;
    eprintln!("[voice-input] Captured {:.1}s ({} samples)", secs, captured.len());

    let tmp = NamedTempFile::new()?;
    let f = tmp.reopen()?;
    let mut writer = hound::WavWriter::new(
        BufWriter::new(f),
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for &s in captured.iter() {
        writer.write_sample(s)?;
    }
    writer.finalize()?;

    Ok(tmp)
}

// ── Transcription ─────────────────────────────────────────────────────────────

fn locate_transcribe_py() -> PathBuf {
    // Look relative to this binary's path, then fall back to PATH
    let exe = env::current_exe().unwrap_or_default();
    let project_root = exe
        .parent() // target/release or target/debug
        .and_then(|p| p.parent()) // target
        .and_then(|p| p.parent()); // project root

    if let Some(root) = project_root {
        let candidate = root.parent().unwrap_or(root).join("transcribe.py");
        if candidate.exists() {
            return candidate;
        }
        // voice-ambient/target/… → project root is one more up
        let candidate2 = root
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("transcribe.py"));
        if let Some(c) = candidate2 {
            if c.exists() {
                return c;
            }
        }
    }
    PathBuf::from("transcribe.py")
}

fn locate_python() -> PathBuf {
    let venv = PathBuf::from("/home/jeb/programs/python_programs/venv/bin/python3");
    if venv.exists() {
        return venv;
    }
    PathBuf::from("python3")
}

fn transcribe(wav: &PathBuf, args: &Args) -> Result<String, Box<dyn std::error::Error>> {
    let script = locate_transcribe_py();
    let python = locate_python();

    // Choose output mode for transcribe.py
    let mode_flag = if args.mode == Mode::Print && !args.no_fancy {
        "--fancy"
    } else if args.mode == Mode::Print {
        "--dual"
    } else {
        // --clip and --type both need plain text
        ""
    };

    let mut cmd = Command::new(&python);
    cmd.arg(&script)
        .arg(wav)
        .env("LD_LIBRARY_PATH", "/usr/lib/ollama")
        .env("VOICE_WHISPER_MODEL", &args.model);

    if !mode_flag.is_empty() {
        cmd.arg(mode_flag);
    }

    // For --print with fancy/dual: inherit stdout so animation renders directly
    if args.mode == Mode::Print {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::null());
        let status = cmd.status()?;
        if !status.success() {
            eprintln!("[voice-input] transcribe.py exited with {}", status);
        }
        return Ok(String::new()); // output already printed
    }

    // For --clip and --type: capture stdout as plain text
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let output = cmd.output()?;
    if !output.status.success() {
        eprintln!("[voice-input] transcribe.py exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ── Beep synthesis ────────────────────────────────────────────────────────────

fn play_beep(freq_hz: f32, duration_secs: f32) {
    let sample_rate = 44_100u32;
    let n = (sample_rate as f32 * duration_secs) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            // Sine wave with 10ms fade-in/out to avoid clicks
            let envelope = {
                let fade = (sample_rate as f32 * 0.01) as usize; // 10ms
                if i < fade {
                    i as f32 / fade as f32
                } else if i > n - fade {
                    (n - i) as f32 / fade as f32
                } else {
                    1.0
                }
            };
            (2.0 * std::f32::consts::PI * freq_hz * t).sin() * 0.4 * envelope
        })
        .collect();

    // OutputStream::try_default() also generates ALSA/JACK noise
    with_quiet_stderr(|| {
        if let Ok((_stream, handle)) = OutputStream::try_default() {
            if let Ok(sink) = rodio::Sink::try_new(&handle) {
                sink.append(SamplesBuffer::new(1, sample_rate, samples));
                sink.sleep_until_end();
            }
        }
    });
}

// ── Ambient mode ──────────────────────────────────────────────────────────────

fn run_ambient(args: &Args) {
    // Locate the voice-ambient binary (built alongside this one)
    let exe = env::current_exe().unwrap_or_default();
    let bin_dir = exe.parent().unwrap_or(exe.as_path());
    let ambient_bin = bin_dir.join("voice-ambient");

    if !ambient_bin.exists() {
        eprintln!(
            "[voice-input] voice-ambient binary not found at {}",
            ambient_bin.display()
        );
        eprintln!("[voice-input] Build it with: cargo build --bin voice-ambient --release");
        std::process::exit(1);
    }

    let script = locate_transcribe_py()
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("ambient.py");
    let python = locate_python();

    let mut cmd = Command::new(&ambient_bin);
    cmd.arg("--script").arg(&script)
        .arg("--python").arg(&python)
        .env("VOICE_WHISPER_MODEL", &args.model);

    if let Some(db) = &args.db {
        cmd.arg("--db").arg(db);
    }
    if args.no_save {
        cmd.arg("--no-save");
    }

    // exec-replace this process with voice-ambient
    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    eprintln!("[voice-input] exec failed: {err}");
    std::process::exit(1);
}
