use std::path::PathBuf;

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

// Chunks below this energy level are assumed silent and skipped.
const VAD_RMS_THRESHOLD: f32 = 0.005;

/// Construct the default GGML model path from a model name.
/// `"large-v3"` → `~/.cache/whisper/ggml-large-v3.bin`
pub fn default_model_path(model_name: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.cache/whisper/ggml-{}.bin", home, model_name))
}

/// Load a WhisperContext from a GGML .bin file.
/// Tries GPU first; falls back to CPU on failure.
pub fn load_ctx(model_path: &str) -> WhisperContext {
    let mut params = WhisperContextParameters::default();
    params.use_gpu(true);
    WhisperContext::new_with_params(model_path, params).unwrap_or_else(|e| {
        eprintln!("[whisper] GPU init failed ({e}), falling back to CPU");
        let mut p = WhisperContextParameters::default();
        p.use_gpu(false);
        WhisperContext::new_with_params(model_path, p).expect("failed to load whisper model on CPU")
    })
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
