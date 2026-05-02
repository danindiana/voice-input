use std::path::PathBuf;
use std::process::Command;

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// Chunks below this energy level are assumed silent and skipped.
const VAD_RMS_THRESHOLD: f32 = 0.005;

// Minimum free VRAM (MiB) required to attempt GPU inference.
// large-v3 needs ~3.1 GB weights + ~700 MB buffers ≈ 3800 MB.
const MIN_VRAM_MB: u64 = 3_800;

// Approximate minimum free VRAM (MiB) required per model size.
const MODEL_VRAM_MB: &[(&str, u64)] = &[
    ("large-v3", 3_800),
    ("large-v2", 3_800),
    ("large",    3_800),
    ("medium",   2_000),
    ("small",      700),
    ("base",       350),
    ("tiny",       200),
];

// Ordered fallback chain: when VRAM is insufficient for the requested model,
// try these smaller models in sequence before giving up and using CPU.
const FALLBACK_CHAIN: &[&str] = &["large-v3", "medium", "small", "base"];

/// Construct the default GGML model path from a model name.
/// `"large-v3"` → `~/.cache/whisper/ggml-large-v3.bin`
pub fn default_model_path(model_name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.cache/whisper/ggml-{}.bin", home, model_name))
}

/// Query free VRAM for a specific CUDA device index via nvidia-smi.
/// Returns None if nvidia-smi is unavailable or the query fails.
fn free_vram_mb(device: i32) -> Option<u64> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.free",
            "--format=csv,noheader,nounits",
            &format!("--id={}", device),
        ])
        .output()
        .ok()?;
    String::from_utf8(out.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Load a WhisperContext from a GGML .bin file.
///
/// Device selection (in priority order):
///   1. `WHISPER_NO_GPU=1`       → CPU only, no GPU attempted
///   2. `WHISPER_GPU_DEVICE=N`   → try device N, then CPU
///   3. Default                  → try device 1 (RTX 3080 SM 8.6) first,
///                                  then device 0, then CPU
///
/// Device 1 is preferred because device 0 is an RTX 5080 (SM 12.0 / Blackwell)
/// and the current whisper.cpp build may not yet include SM 12.0 kernels —
/// that combination produces `abort()` at first inference. Device 1 (RTX 3080,
/// SM 8.6) is confirmed working with the current build.
/// Rebuild with CMAKE_CUDA_ARCHITECTURES=86;120 to enable both GPUs.
///
/// VRAM check: if the target device has less than MIN_VRAM_MB free, GPU is
/// skipped and CPU is used instead to avoid OOM mid-inference.
pub fn load_ctx(model_path: &str) -> WhisperContext {
    let force_cpu = std::env::var("WHISPER_NO_GPU")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if force_cpu {
        eprintln!("[whisper] WHISPER_NO_GPU set — loading on CPU");
        return load_cpu(model_path);
    }

    let device_override: Option<i32> = std::env::var("WHISPER_GPU_DEVICE")
        .ok()
        .and_then(|s| s.parse().ok());

    // Devices to try in order.
    let devices: Vec<i32> = match device_override {
        Some(d) => vec![d],
        None    => vec![1, 0],  // RTX 3080 first, then RTX 5080
    };

    for dev in devices {
        // VRAM gate — skip devices that can't fit the model.
        match free_vram_mb(dev) {
            Some(free) if free < MIN_VRAM_MB => {
                eprintln!(
                    "[whisper] GPU device {} has only {} MiB free (need {}), skipping",
                    dev, free, MIN_VRAM_MB
                );
                continue;
            }
            None => {
                eprintln!("[whisper] could not query VRAM for device {}, skipping", dev);
                continue;
            }
            Some(free) => {
                eprintln!("[whisper] GPU device {} has {} MiB free — attempting load", dev, free);
            }
        }

        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);
        params.gpu_device(dev);

        match WhisperContext::new_with_params(model_path, params) {
            Ok(ctx) => {
                eprintln!("[whisper] loaded on GPU device {}", dev);
                return ctx;
            }
            Err(e) => {
                eprintln!("[whisper] GPU device {} load failed: {}", dev, e);
            }
        }
    }

    eprintln!("[whisper] all GPU options exhausted — falling back to CPU");
    load_cpu(model_path)
}

fn load_cpu(model_path: &str) -> WhisperContext {
    eprintln!("[whisper] loading on CPU (this will be slow for large-v3)");
    let mut p = WhisperContextParameters::default();
    p.use_gpu(false);
    WhisperContext::new_with_params(model_path, p)
        .expect("failed to load whisper model on CPU")
}

/// Transcribe a buffer of 32 kHz mono i16 samples.
/// Returns `None` if the chunk is silent or produces no text.
pub fn transcribe_i16(ctx: &WhisperContext, samples_32k: &[i16]) -> Option<String> {
    let samples_16k = resample_32k_to_16k(samples_32k);
    if samples_16k.is_empty() {
        return None;
    }

    // Energy-based VAD gate
    let rms: f32 = (samples_16k.iter().map(|&s| s * s).sum::<f32>() / samples_16k.len() as f32)
        .sqrt();
    if rms < VAD_RMS_THRESHOLD {
        return None;
    }

    let mut state = ctx.create_state().ok()?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_context(true);

    state.full(params, &samples_16k).ok()?;

    let n_segs = state.full_n_segments().ok()?;
    let mut parts: Vec<String> = Vec::new();
    for i in 0..n_segs {
        if let Ok(text) = state.full_get_segment_text(i) {
            let t = text.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
    }

    let text = parts.join(" ");
    if text.is_empty() { None } else { Some(text) }
}

fn vram_for_model(name: &str) -> u64 {
    MODEL_VRAM_MB.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, mb)| *mb)
        .unwrap_or(MIN_VRAM_MB)
}

fn fallback_chain_from(model_name: &str) -> &'static [&'static str] {
    let start = FALLBACK_CHAIN.iter().position(|&n| n == model_name).unwrap_or(0);
    &FALLBACK_CHAIN[start..]
}

/// Load a WhisperContext, automatically falling back to smaller models when VRAM is tight.
///
/// Tries the requested model_name first; if no GPU has enough free VRAM, steps down
/// through medium → small → base (skipping sizes not present on disk).
/// Falls back to CPU with the first available model file as last resort.
///
/// Device selection and env var overrides (WHISPER_NO_GPU, WHISPER_GPU_DEVICE)
/// behave identically to `load_ctx`.
pub fn load_ctx_with_fallback(model_name: &str) -> WhisperContext {
    let force_cpu = std::env::var("WHISPER_NO_GPU")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let device_override: Option<i32> = std::env::var("WHISPER_GPU_DEVICE")
        .ok()
        .and_then(|s| s.parse().ok());

    let devices: Vec<i32> = match device_override {
        Some(d) => vec![d],
        None    => vec![1, 0],
    };

    for &candidate in fallback_chain_from(model_name) {
        let path = default_model_path(candidate);
        if !path.exists() {
            continue;
        }
        let path_str = path.to_string_lossy().into_owned();
        let needed   = vram_for_model(candidate);

        if force_cpu {
            if candidate == model_name {
                eprintln!("[whisper] WHISPER_NO_GPU set — loading {} on CPU", candidate);
            } else {
                eprintln!("[whisper] WHISPER_NO_GPU set — using {} on CPU (fell back from {})", candidate, model_name);
            }
            return load_cpu(&path_str);
        }

        for &dev in &devices {
            match free_vram_mb(dev) {
                Some(free) if free >= needed => {
                    eprintln!("[whisper] GPU {} has {} MiB free (need {} for {})", dev, free, needed, candidate);
                    let mut params = WhisperContextParameters::default();
                    params.use_gpu(true);
                    params.gpu_device(dev);
                    match WhisperContext::new_with_params(&path_str, params) {
                        Ok(ctx) => {
                            if candidate != model_name {
                                eprintln!("[whisper] loaded {} on GPU {} (fell back from {})", candidate, dev, model_name);
                            } else {
                                eprintln!("[whisper] loaded {} on GPU {}", candidate, dev);
                            }
                            return ctx;
                        }
                        Err(e) => eprintln!("[whisper] GPU {} failed for {}: {}", dev, candidate, e),
                    }
                }
                Some(free) => eprintln!(
                    "[whisper] GPU {} has {} MiB free (need {} for {}), skipping",
                    dev, free, needed, candidate
                ),
                None => eprintln!("[whisper] could not query VRAM for GPU {}", dev),
            }
        }
    }

    // Last resort: CPU with first available model file in fallback chain
    for &candidate in fallback_chain_from(model_name) {
        let path = default_model_path(candidate);
        if path.exists() {
            eprintln!("[whisper] all GPU options exhausted — loading {} on CPU", candidate);
            if candidate != model_name {
                eprintln!("[whisper] (fell back from {})", model_name);
            }
            return load_cpu(&path.to_string_lossy().into_owned());
        }
    }

    eprintln!("[whisper] no model file found for {} or any fallback", model_name);
    load_cpu(&default_model_path(model_name).to_string_lossy().into_owned())
}

/// Resample 32 kHz mono i16 → 16 kHz mono f32 using sinc interpolation.
fn resample_32k_to_16k(samples_i16: &[i16]) -> Vec<f32> {
    let n = samples_i16.len();
    if n == 0 {
        return Vec::new();
    }
    let samples_f32: Vec<f32> = samples_i16.iter().map(|&s| s as f32 / 32768.0).collect();

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };

    match SincFixedIn::<f32>::new(0.5, 2.0, params, n, 1) {
        Ok(mut resampler) => match resampler.process(&[samples_f32], None) {
            Ok(mut out) => out.pop().unwrap_or_default(),
            Err(e) => {
                eprintln!("[whisper] resample error: {e}");
                Vec::new()
            }
        },
        Err(e) => {
            eprintln!("[whisper] resampler init error: {e}");
            Vec::new()
        }
    }
}
