# voice-input STATUS

**Date:** 2026-04-02
**State:** Working — confirmed end-to-end

## What This Is

Push-to-talk speech-to-text for the terminal / Claude Code sessions on worlock.
Captures mic from UC03 USB headset → faster-whisper medium (GPU) → typed, clipboard, or printed text.

Confirmed working output:
> "And if you want to buy me flowers just go ahead now and if you want to talk for hours just go ahead"

## Files

| File | Purpose |
|------|---------|
| `voice-input.sh` | Main script. Records via parec, transcribes, outputs text. |
| `transcribe.py` | Python: loads faster-whisper medium on GPU, prints transcript. |
| `requirements.txt` | Frozen Python deps. |
| `README.md` | Full setup, quickstart, build notes. |
| `/usr/local/bin/voice-input` | Symlink → voice-input.sh (on PATH) |

## Usage

```bash
voice-input --print             # transcribe → stdout (animated fancy mode)
voice-input --print --no-fancy  # transcribe → stdout (plain text)
voice-input --clip              # transcribe → clipboard
voice-input                     # transcribe → xdotool types into active window
```

Press Enter to stop early. Auto-stops at 65s. Low beep = start, high beep = stop.

## Hardware

- **Headset:** UC03 USB (Bus 005 Device 005, vendor e4b7:0812)
- **ALSA card:** 3
- **PipeWire source:** `alsa_input.usb-UC03_UC03-00.mono-fallback`
- **PipeWire sink:** `alsa_output.usb-UC03_UC03-00.analog-stereo`
- **Native sample rate:** 32000 Hz mono — do not change to 16kHz

## Python Environment

```
/home/jeb/programs/python_programs/venv  (Python 3.13)
faster-whisper==1.2.1
ctranslate2==4.7.1
av==17.0.0
flatbuffers==25.12.19
onnxruntime==1.24.4
```

## Audio Config

Default source/sink persisted in WirePlumber:
`~/.config/wireplumber/wireplumber.conf.d/51-default-audio.conf`

ALSA mic gain: 127/127 (+23.81 dB) — set during debug, may want to normalize
PulseAudio source volume: 150% — set during debug, may want to normalize

```bash
amixer -c 3 sset Mic 100
pactl set-source-volume alsa_input.usb-UC03_UC03-00.mono-fallback 100%
```

## Debug Timeline (2026-04-02)

| Issue | Symptom | Root Cause | Fix |
|-------|---------|------------|-----|
| Silent recordings | RMS ~0.0005, 344× boost needed | Forced 16kHz on 32kHz native device | `--rate=32000` |
| CUDA error | `libcublas.so.12 not found` | System only has .so.11 | `LD_LIBRARY_PATH=/usr/lib/ollama` |
| No transcription | Empty TEXT variable | Symlink broke SCRIPT_DIR; transcribe.py not found | `readlink -f` in SCRIPT_DIR |
| WAV corruption | Empty/broken WAV file | Killing `parec\|sox` pipeline left incomplete WAV header | Write raw to file, convert after |
| Wrong default source | Silent recordings | Default source was motherboard loopback monitor | `pactl set-default-source` + WirePlumber config |
| Enter not stopping | Script hung 65s | `read` exits on non-TTY stdin | `read < /dev/tty` + USR1 signal timer |
| Fancy animation broken | Frames print sequentially, no in-place overwrite | `\033[s/u` cursor save/restore not supported in terminal | Switch to `\033[{N}D` cursor-left |
| `--no-fancy` output silent | `--print --no-fancy` produced nothing | `print)` case missing from output dispatch | Restored `print)` case in voice-input.sh |

## Session Log

### 2026-05-01 (Thu May 1 02:00 CDT)

**Change: Tier 3 complete — whisper-rs replaces Python entirely**

- **`src/lib.rs`** (new): exposes `pub mod whisper_infer`
- **`src/whisper_infer.rs`** (new): `load_ctx()` (GPU→CPU fallback via `WhisperContextParameters::use_gpu`), `transcribe_i16()` (32 kHz i16 → rubato sinc resample → 16 kHz f32 → whisper.cpp), `default_model_path()` (`~/.cache/whisper/ggml-<model>.bin`)
- **`.cargo/config.toml`** (new): `CMAKE_CUDA_COMPILER=/usr/local/cuda-13.0/bin/nvcc`; rustflags linker search `/usr/local/cuda-13.0/lib64`
- **`Cargo.toml`**: `whisper-rs = { version = "0.14", features = ["cuda"] }` — enables GGML_CUDA cmake flag + links cublas/cudart/cublasLt
- **`src/main.rs`** (voice-ambient): removed Python subprocess spawn + JSON IPC; `spawn_audio()` sends `Vec<i16>` chunks via `sync_channel(2)` to new `spawn_whisper()` inference thread (loads model, transcribes, writes DB/transcript, sends `Update::Utterance`)
- **`src/bin/voice_input.rs`**: records directly to `Vec<i16>`; calls `whisper_infer::load_ctx()` + `transcribe_i16()` inline; no temp WAV files; `run_ambient()` passes `--model-path` to voice-ambient binary
- **Runtime**: CUDA 13 libs (`libcublas.so.13`) in system ldconfig — no `LD_LIBRARY_PATH` needed
- **Model**: `~/.cache/whisper/ggml-large-v3.bin` (3095033483 bytes) — GGML format, different from HuggingFace safetensors
- **Symlink**: `/usr/local/bin/voice-input` → `voice-ambient/target/release/voice-input`

Confirmed end-to-end: binary captures audio → whisper-rs CUDA inference → text output. Python/faster-whisper no longer invoked.

---

### 2026-04-30 (Thu Apr 30 11:25 CDT, session 3)

**Change: default model → large-v3; --model flag; session doc**

After confirming large-v3 works correctly, bumped the default in both `ambient.py` and `transcribe.py` from `"medium"` to `"large-v3"`. Added `--model <name>` flag to `voice-input.sh` for per-invocation override without needing to set an env var. Session document written to `sessions/2026-04-30_112540.md`.

Use `--model medium` to revert to lighter/faster transcription when GPU is under load.

---

### 2026-04-30 (Thu Apr 30, session 2)

**Changes: auto-save transcripts, --submit flag, configurable model**

- **Ambient transcript auto-save** (`voice-ambient/src/main.rs`): each session now creates a timestamped plain-text file at `~/.local/share/voice-input/transcripts/YYYY-MM-DD_HH-MM-SS.txt`. Each utterance is appended as `[HH:MM:SS] text`. The TUI footer shows `SAVE: <filename>`. Pass `--no-save` (or set via `voice-input --ambient --no-save`) to disable.
- **`--submit` flag** (`voice-input.sh`): `voice-input --type --submit` sends `xdotool key Return` after typing — useful for auto-submitting voice input into Claude Code or other prompts.
- **Configurable model** (`ambient.py`, `transcribe.py`): both scripts now read `VOICE_WHISPER_MODEL` env var (default: `medium`). Run `VOICE_WHISPER_MODEL=large-v3 voice-input --ambient` to test large-v3 accuracy.

---

### 2026-04-30 (Thu Apr 30 09:31 CDT)

**Change: GPU → CPU fallback in `transcribe.py`**

Added `load_model()` function that first attempts `device="cuda"` with `compute_type="float16"`.
If that raises any exception (CUDA not available, OOM, driver issue, GPU locked by another process),
it catches it, emits a stderr warning, and loads the model on CPU with `compute_type="int8"`.

Motivation: the dual-GPU setup on worlock is a contention resource shared with Ollama and other
workloads. `transcribe.py` previously hard-crashed if CUDA init failed. Now it degrades gracefully
— transcription still works on CPU, just slower.

Stdout path (`voice-input.sh --print`): stderr device messages are suppressed by `2>/dev/null`
in the shell script, so they don't contaminate captured `$TEXT`.

---

## Next Steps

- [ ] Global hotkey via `xbindkeys` — launch without second terminal
- [ ] Normalize mic gain after confirming no clipping
- [x] **Tier 3**: whisper-rs CUDA inference — `ggml-large-v3.bin` + whisper.cpp statically compiled; Python eliminated entirely
- [x] Auto-submit mode — `voice-input --type --submit` sends Return after xdotool types
- [x] Configurable model — set `VOICE_WHISPER_MODEL=large-v3` env var to try large-v3
- [x] Ambient transcript auto-save — each session writes to `~/.local/share/voice-input/transcripts/YYYY-MM-DD_HH-MM-SS.txt` by default; use `--no-save` to disable; footer shows `SAVE: <filename>`
- [x] Default model upgraded to `large-v3`; use `--model medium` to opt back to lighter/faster
- [x] **Tier 1**: `voice-input` Rust binary replaces `voice-input.sh` (cpal/hound/rodio/enigo/arboard); device: `"pipewire"` ALSA virtual
- [x] **Tier 2**: ambient audio in Rust — cpal replaces parec, hound replaces sox; `ambient_infer.py` is inference-only daemon
