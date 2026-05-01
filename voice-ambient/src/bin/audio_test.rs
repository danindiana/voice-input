/// Validates that cpal can capture from the UC03 USB headset at 32 kHz mono.
/// Usage: audio-test [device-substring] [seconds]
/// Defaults: device="UC03", seconds=5
/// Output: /tmp/voice-cpal-test.wav
use std::io::BufWriter;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Via ALSA, cpal sees a "pipewire" virtual device that routes to the PipeWire default
    // source (set to UC03 via WirePlumber). Matching on hardware name won't work here.
    let device_hint = args.get(1).map(|s| s.as_str()).unwrap_or("pipewire");
    let capture_secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let out_path = "/tmp/voice-cpal-test.wav";

    let host = cpal::default_host();
    eprintln!("Host: {}", host.id().name());

    // List all input devices
    let devices: Vec<_> = host
        .input_devices()
        .expect("can't enumerate input devices")
        .collect();

    eprintln!("--- Input devices ({}) ---", devices.len());
    for d in &devices {
        let name = d.name().unwrap_or_else(|_| "<unknown>".into());
        eprintln!("  {name}");
        if let Ok(configs) = d.supported_input_configs() {
            for c in configs {
                eprintln!(
                    "    ch={} fmt={:?} rate={}-{}",
                    c.channels(),
                    c.sample_format(),
                    c.min_sample_rate().0,
                    c.max_sample_rate().0,
                );
            }
        }
    }
    eprintln!("---");

    // Pick the first device whose name contains device_hint (case-insensitive)
    let device = devices
        .into_iter()
        .find(|d| {
            d.name()
                .map(|n| n.to_lowercase().contains(&device_hint.to_lowercase()))
                .unwrap_or(false)
        })
        .expect(&format!("No device matching '{}'", device_hint));

    let chosen_name = device.name().unwrap_or_else(|_| "<unknown>".into());
    eprintln!("Selected: {chosen_name}");

    // Request 32 kHz mono i16 — UC03 native rate
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(32000),
        buffer_size: cpal::BufferSize::Default,
    };

    let samples: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    let samples_writer = samples.clone();
    let err_fn = |e| eprintln!("stream error: {e}");

    let stream = device
        .build_input_stream(
            &config,
            move |data: &[i16], _| {
                samples_writer.lock().unwrap().extend_from_slice(data);
            },
            err_fn,
            None,
        )
        .expect("failed to build input stream");

    eprintln!(
        "Recording {} s at 32000 Hz mono from '{chosen_name}'…",
        capture_secs
    );
    stream.play().expect("failed to start stream");
    std::thread::sleep(Duration::from_secs(capture_secs));
    drop(stream);

    let captured = samples.lock().unwrap();
    eprintln!("Captured {} samples ({:.2} s)", captured.len(), captured.len() as f32 / 32000.0);

    // Write WAV
    let f = std::fs::File::create(out_path).expect("can't create output file");
    let mut writer = hound::WavWriter::new(
        BufWriter::new(f),
        hound::WavSpec {
            channels: 1,
            sample_rate: 32000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .expect("can't create WavWriter");

    for &s in captured.iter() {
        writer.write_sample(s).expect("write_sample failed");
    }
    writer.finalize().expect("finalize failed");

    // Compute RMS
    let rms = {
        let sum_sq: f64 = captured.iter().map(|&s| (s as f64).powi(2)).sum();
        (sum_sq / captured.len() as f64).sqrt() / 32768.0
    };
    eprintln!("RMS level: {:.4} (should be >0.01 when speaking)", rms);
    eprintln!("WAV written to {out_path}");
    eprintln!("Verify with: python3 -c \"import sys; sys.path.insert(0,'/home/jeb/programs/python_programs/venv/lib/python3.13/site-packages'); from faster_whisper import WhisperModel; m=WhisperModel('large-v3',device='cuda',compute_type='float16'); segs,_=m.transcribe('{out_path}',beam_size=5); print(' '.join(s.text for s in segs))\"");
}
