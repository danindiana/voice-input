# voice-input STATUS

**Last updated:** 2026-05-01
**State:** ✅ Stable — all Rust, no Python, wizard added, docs current

---

## What This Is

Push-to-talk speech-to-text for the terminal / Claude Code sessions on worlock.
UC03 USB headset → cpal (PipeWire) → whisper-rs CUDA (ggml-large-v3) → typed, clipboard, stdout, or ambient TUI.

No Python. No subprocesses. One `cargo build --release`.

---

## Current Binaries

| Binary | Path | Status |
|--------|------|--------|
| `voice-input` | `/usr/local/bin/voice-input` → `voice-ambient/target/release/voice-input` | ✅ working |
| `voice-ambient` | `/usr/local/bin/voice-ambient` → `voice-ambient/target/release/voice-ambient` | ✅ working |
| `voice-wizard` | `/usr/local/bin/voice-wizard` → `voice-ambient/target/release/voice-wizard` | ✅ working |
| `audio-test` | `voice-ambient/target/release/audio-test` | ✅ working |

---

## Audio

| Setting | Value |
|---------|-------|
| Device (cpal) | `"pipewire"` ALSA virtual → WirePlumber default source |
| PipeWire source | `alsa_input.usb-UC03_UC03-00.mono-fallback` |
| Native sample rate | 32 000 Hz mono i16 |
| Mic gain (ALSA) | 127/127 (+23.81 dB) — set during debug, may want to normalize |
| Source volume (PA) | 150% — set during debug, may want to normalize |
| WirePlumber config | `~/.config/wireplumber/wireplumber.conf.d/51-default-audio.conf` |

---

## Model

| Setting | Value |
|---------|-------|
| Default model | `large-v3` |
| Model path | `~/.cache/whisper/ggml-large-v3.bin` |
| File size | 3 095 033 483 bytes (~2.9 GB) |
| Format | GGML (whisper.cpp native — NOT HuggingFace safetensors) |
| CUDA | whisper-rs built with `features = ["cuda"]`; GPU → CPU fallback automatic |
| CUDA version | 13.0 (`/usr/local/cuda-13.0/`) |

---

## Open Items

- [ ] Global hotkey via `xbindkeys` — launch `voice-input --mode type` without a second terminal
- [ ] Normalize mic gain (127/127 = +23.81 dB) once confirmed no clipping at 100%
- [ ] Add `--model-path` option to voice-wizard Options page

---

## Session Log

### 2026-05-01 — wizard, motd, docs, rename

- **voice-wizard** (`src/bin/voice_wizard.rs`): 5-page ratatui TUI setup wizard. System checks (GPU, model, audio, binaries), mode selection, per-mode option toggles, live command preview, clipboard copy + exec launch. Binary symlinked to `/usr/local/bin/voice-wizard`.
- **`.motd`** (project root): bash script displaying binary health, model status, UC03 mic presence, and quick-ref commands. Fires automatically on `cd` via `chpwd()` hook added to `~/.zshrc`.
- **Folder renamed**: `2026-04-02_voice-input/` → `voice-input/` (under `claude_creations/`). All three symlinks updated.
- **voice-ambient symlink** added to `/usr/local/bin/` (was missing).
- **Docs rewritten**: README.md and STATUS.md updated to reflect current Rust-only state.
- Commits: `f494590` (wizard), docs/motd/rename not yet committed (see below).

### 2026-05-01 — Tier 3 complete: whisper-rs CUDA replaces Python

- `src/lib.rs` + `src/whisper_infer.rs`: `load_ctx()` (GPU→CPU fallback), `transcribe_i16()` (32 kHz i16 → rubato sinc resample 16 kHz f32 → whisper.cpp), `default_model_path()`
- `.cargo/config.toml`: `CMAKE_CUDA_COMPILER=/usr/local/cuda-13.0/bin/nvcc`; rustflags `-L /usr/local/cuda-13.0/lib64`
- `Cargo.toml`: `whisper-rs = { version = "0.14", features = ["cuda"] }`
- `src/main.rs`: removed Python subprocess + JSON IPC; `spawn_whisper()` thread handles inference inline
- `src/bin/voice_input.rs`: records to `Vec<i16>`; calls `whisper_infer::load_ctx()` + `transcribe_i16()` directly
- CUDA 13 libs in system ldconfig — no `LD_LIBRARY_PATH` needed at runtime
- Model: `~/.cache/whisper/ggml-large-v3.bin` (3095033483 bytes)
- Commit: `a99eb50`

### 2026-05-01 (early) — Tier 1 + 2: full Rust audio pipeline

- Tier 1 (`voice-input` binary): replaces `voice-input.sh`; cpal/hound/rodio/arboard/enigo; `--mode type/print/clip/ambient`; crossbeam-channel stop mechanism; quiet_stderr fd redirect
- Tier 2 (`voice-ambient` main.rs): replaces `ambient.py` audio loop; cpal replaces parec; hound replaces sox; RMS metering + 5-second chunk batching in Rust
- cpal device: `"pipewire"` ALSA virtual (not the raw ALSA source name)
- Commits: `558a7db` (Tier 1), `50be1c1` (Tier 2)

### 2026-04-30 — default model → large-v3; --model flag; --submit; transcript auto-save

- Default model bumped to `large-v3` in ambient.py + transcribe.py
- `--model <name>` flag added to voice-input.sh for per-invocation override
- `--submit` flag: sends Return keystroke after xdotool typing
- Ambient transcript auto-save to `~/.local/share/voice-input/transcripts/YYYY-MM-DD_HH-MM-SS.txt`
- `VOICE_WHISPER_MODEL` env var support
- Commits: `d70b13f`, `fbb86f4`

### 2026-04-30 — GPU→CPU fallback in transcribe.py

- `load_model()` tries `device="cuda"` first; catches any exception and retries `device="cpu"` with `int8`
- Motivation: dual-GPU contention with Ollama; previously hard-crashed on CUDA init failure

### 2026-04-02 — initial working state

- Confirmed end-to-end: UC03 → parec (32 kHz) → sox → faster-whisper medium (GPU) → typed/clipboard/stdout
- Debug timeline (all issues resolved):

| Issue | Root Cause | Fix |
|-------|------------|-----|
| Silent recordings | Forced 16 kHz on 32 kHz native device | `--rate=32000` |
| `libcublas.so.12` missing | System only had .so.11 | `LD_LIBRARY_PATH=/usr/lib/ollama` (later superseded by CUDA 13 install) |
| SCRIPT_DIR broken via symlink | `dirname "${BASH_SOURCE[0]}"` resolved to symlink dir | `readlink -f` |
| WAV corruption | Killing `parec\|sox` pipeline left incomplete header | Write raw PCM first, convert after |
| Wrong default source | Default was motherboard loopback monitor | `pactl set-default-source` + WirePlumber config |
| Enter not stopping | `read` exits on non-TTY stdin | `read < /dev/tty` + USR1 timer |
| Fancy animation broken | `\033[s/u` not supported in tmux/VTE | Switched to `\033[{N}D` cursor-left |
