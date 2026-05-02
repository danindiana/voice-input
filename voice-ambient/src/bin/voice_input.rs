/// voice-input — push-to-talk speech-to-text using in-process whisper-rs inference.
///
/// Records from the PipeWire ALSA virtual device (UC03 USB headset at 32 kHz mono),
/// resamples 32 kHz → 16 kHz, and transcribes with whisper.cpp via whisper-rs.
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arboard::Clipboard;
use clap::{Parser, ValueEnum};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::bounded;
use enigo::{Enigo, Keyboard, Settings};
use rodio::buffer::SamplesBuffer;
use rodio::OutputStream;
use voice_ambient::whisper_infer;

// ── Constants ────────────────────────────────────────────────────────────────

const MAX_RECORD_SECS: u64 = 65;
const SAMPLE_RATE: u32 = 32_000;
const BEEP_LOW_HZ: f32 = 480.0;
const BEEP_HIGH_HZ: f32 = 880.0;
const BEEP_DURATION_SECS: f32 = 0.15;
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

    /// Override GGML model file path (default: ~/.cache/whisper/ggml-<model>.bin)
    #[arg(long)]
    model_path: Option<PathBuf>,

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

impl Args {
    fn resolved_model_path(&self) -> PathBuf {
        self.model_path
            .clone()
            .unwrap_or_else(|| whisper_infer::default_model_path(&self.model))
    }
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

    let samples = record_samples().expect("audio capture failed");

    // Beep: high tone → recording stopped
    play_beep(BEEP_HIGH_HZ, BEEP_DURATION_SECS);

    eprintln!("[voice-input] Transcribing ({} samples at {} Hz)…", samples.len(), SAMPLE_RATE);

    let ctx = if args.model_path.is_some() {
        whisper_infer::load_ctx(args.resolved_model_path().to_str().unwrap_or(""))
    } else {
        whisper_infer::load_ctx_with_fallback(&args.model)
    };

    let text = whisper_infer::transcribe_i16(&ctx, &samples)
        .unwrap_or_default();
    let text = text.trim().to_string();

    if text.is_empty() {
        eprintln!("[voice-input] No transcription");
        return;
    }

    match args.mode {
        Mode::Print => {
            println!("{}", text);
        }
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
        Mode::Ambient => unreachable!(),
    }
}

// ── Audio recording ───────────────────────────────────────────────────────────

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

fn record_samples() -> Result<Vec<i16>, Box<dyn std::error::Error>> {
    let device = find_pipewire_device();
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let samples_writer = samples.clone();

    let (tx_stop, rx_stop) = bounded::<()>(1);

    // Timer thread
    let tx_timer = tx_stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(MAX_RECORD_SECS));
        let _ = tx_timer.send(());
    });

    // Enter-key thread
    let tx_enter = tx_stop.clone();
    std::thread::spawn(move || {
        if let Ok(mut tty) = fs::File::open("/dev/tty") {
            let mut buf = [0u8; 1];
            let _ = tty.read(&mut buf);
            let _ = tx_enter.send(());
        }
    });

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

    let _ = rx_stop.recv();
    drop(stream);

    let captured = samples.lock().unwrap().clone();
    let secs = captured.len() as f32 / SAMPLE_RATE as f32;
    eprintln!("[voice-input] Captured {:.1}s ({} samples)", secs, captured.len());

    Ok(captured)
}

// ── Beep synthesis ────────────────────────────────────────────────────────────

fn play_beep(freq_hz: f32, duration_secs: f32) {
    let sample_rate = 44_100u32;
    let n = (sample_rate as f32 * duration_secs) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let envelope = {
                let fade = (sample_rate as f32 * 0.01) as usize;
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

    let model_path = args.resolved_model_path();

    let mut cmd = std::process::Command::new(&ambient_bin);
    cmd.arg("--model").arg(&args.model)
       .arg("--model-path").arg(&model_path)
       .env("VOICE_WHISPER_MODEL", &args.model);

    if let Some(db) = &args.db {
        cmd.arg("--db").arg(db);
    }
    if args.no_save {
        cmd.arg("--no-save");
    }

    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    eprintln!("[voice-input] exec failed: {err}");
    std::process::exit(1);
}
