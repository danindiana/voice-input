# voice-input

```
██╗   ██╗ ██████╗ ██╗ ██████╗███████╗
██║   ██║██╔═══██╗██║██╔════╝██╔════╝
██║   ██║██║   ██║██║██║     █████╗  
╚██╗ ██╔╝██║   ██║██║██║     ██╔══╝  
 ╚████╔╝ ╚██████╔╝██║╚██████╗███████╗
  ╚═══╝   ╚═════╝ ╚═╝ ╚═════╝╚══════╝
██╗███╗   ██╗██████╗ ██╗   ██╗████████╗
██║████╗  ██║██╔══██╗██║   ██║╚══██╔══╝
██║██╔██╗ ██║██████╔╝██║   ██║   ██║   
██║██║╚██╗██║██╔═══╝ ██║   ██║   ██║   
██║██║ ╚████║██║     ╚██████╔╝   ██║   
╚═╝╚═╝  ╚═══╝╚═╝      ╚═════╝   ╚═╝   
  speak  ▸  whisper.cpp + cuda  ▸  type  ·  pure rust  ·  no python
```

<p align="center">
  <img src="voice-input-logo.png" alt="voice-input logo" width="400"/>
</p>

Push-to-talk speech-to-text for Linux terminals. Captures mic input, transcribes locally via [whisper-rs](https://github.com/tazz4843/whisper-rs) (whisper.cpp bindings with CUDA), and outputs text by typing into the active window, copying to clipboard, printing to stdout, or running a continuous ambient TUI.

Pure Rust — no Python, no subprocess, no venv. One binary.

Built for and tested on: Ubuntu/Debian, PipeWire audio, NVIDIA GPU (optional), UC03 USB headset.

---

## Quickstart

**New here? Run the setup wizard first:**
```bash
voice-wizard
```
It checks your system, walks you through the four modes, and builds the exact command to run.

**Or jump straight in:**
```bash
voice-input --mode type     # speak → transcribe → typed into active window
voice-input --mode clip     # speak → transcribe → clipboard (Ctrl+Shift+V to paste)
voice-input --mode print    # speak → transcribe → stdout
voice-input --mode ambient  # speak continuously → live TUI + transcript log
```

Press **Enter** to stop recording. Auto-stops at 65 seconds.
Low beep (480 Hz) = recording started. High beep (880 Hz) = stopped, transcribing.

---

## Requirements

### Hardware
- USB headset or microphone (tested: UC03 USB, native rate 32 kHz)
- NVIDIA GPU with CUDA — optional; whisper-rs falls back to CPU automatically

### System packages
```bash
sudo apt install libxdo-dev
```

| Package | Purpose |
|---------|---------|
| `libxdo-dev` | Build-time dep for enigo (X11 keyboard simulation) |

> **Not needed:** sox, parec, xdotool, xclip, paplay, Python, pip — all replaced by Rust crates.

### Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Whisper model (GGML format)
```bash
mkdir -p ~/.cache/whisper
# Download ggml-large-v3.bin (~3.1 GB) — one-time setup
# From https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin
wget -P ~/.cache/whisper/ https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin
```

> **Note:** This is GGML format — different from HuggingFace safetensors used by faster-whisper.
> Other sizes: `ggml-medium.bin`, `ggml-small.bin`, `ggml-base.bin`

### CUDA (optional)
Requires CUDA 13.0:
```bash
ls /usr/local/cuda-13.0/bin/nvcc   # confirm nvcc present
```
If absent, whisper-rs compiles and runs on CPU with no code changes.

---

## Build

```bash
cd ~/Documents/claude_creations/voice-input/voice-ambient
cargo build --release
```

Outputs (in `target/release/`):

| Binary | Purpose |
|--------|---------|
| `voice-input` | Push-to-talk CLI — all four modes |
| `voice-ambient` | Continuous ambient TUI (spawned by `--mode ambient`) |
| `voice-wizard` | 5-page interactive setup wizard |
| `audio-test` | Device enumeration and capture validation |

**Put binaries on PATH (symlinks):**
```bash
sudo ln -sf "$(pwd)/target/release/voice-input"   /usr/local/bin/voice-input
sudo ln -sf "$(pwd)/target/release/voice-ambient"  /usr/local/bin/voice-ambient
sudo ln -sf "$(pwd)/target/release/voice-wizard"   /usr/local/bin/voice-wizard
```

---

## Modes

### `--mode type` (default)
Records speech, transcribes, then uses X11 keyboard simulation to type the text into whichever window has focus.

```bash
voice-input --mode type
voice-input --mode type --submit   # also sends Return after typing (auto-submit)
```

### `--mode clip`
Records speech, transcribes, copies result to X11 clipboard.
```bash
voice-input --mode clip
# then Ctrl+Shift+V to paste
```

### `--mode print`
Records speech, transcribes, prints to stdout.
```bash
voice-input --mode print
text=$(voice-input --mode print)   # capture in a variable
```

### `--mode ambient`
Spawns the `voice-ambient` TUI — keeps the mic open and transcribes continuously.
```bash
voice-input --mode ambient
voice-input --mode ambient --db ~/transcripts.db   # with SQLite logging
voice-input --mode ambient --no-save               # disable plain-text auto-save
```

---

## Model Selection

Default model: `large-v3`. Override per-invocation:
```bash
voice-input --mode print --model medium     # faster, ~1.5 GB
voice-input --mode print --model small      # very fast, ~500 MB
voice-input --mode print --model-path /path/to/custom.bin
```

Or set a persistent default:
```bash
export VOICE_WHISPER_MODEL=medium
```

Model path convention: `~/.cache/whisper/ggml-<model>.bin`

GPU → CPU fallback is automatic. If CUDA init fails for any reason (GPU absent, OOM, driver contention), whisper-rs retries on CPU.

---

## Ambient Mode

`voice-input --mode ambient` opens a full-screen TUI:

```
┌● REC  voice-input : ambient  00:05:23──────────────────────────┐
│▁▂▄▆▇▇▆▄▃▂▁▁▁▂▃▄▅▆▇▆▅▄▃▂▁▁▁▁▂▃▄▅▅▄▃▂▁▁▁▁▁▁▁▁▂▃▄▅▆▇▇▆▅▄▃▂▁▁▁▁│
│████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░         │
├───────────────────────────────────────────────────────────────────┤
│ 14:32:01 hello world how are you doing today                      │
│ 14:32:08 i think we should implement this feature                 │
│ 14:32:16 that way we can test it properly                         │
├───────────────────────────────────────────────────────────────────┤
│ Recording  │  words: 47  utt: 8  │  DB: off  │  q / Ctrl-C: stop │
└───────────────────────────────────────────────────────────────────┘
```

- Waveform sparkline + RMS level gauge (green → yellow → red)
- Transcript lines fade cyan → white → gray → dark-gray as they age
- REC indicator blinks red while recording; 5-second transcription chunks

**Plain-text auto-save** (on by default):
```
~/.local/share/voice-input/transcripts/YYYY-MM-DD_HH-MM-SS.txt
```
Each line: `[HH:MM:SS] transcribed text`

**SQLite session logging** (`--db <path>`):
```bash
voice-input --mode ambient --db ~/transcripts.db
```
Schema:
```sql
sessions   (id, started_at, ended_at)
utterances (id, session_id, recorded_at, text, word_count)
```
Query:
```bash
sqlite3 ~/transcripts.db "SELECT recorded_at, text FROM utterances ORDER BY id DESC LIMIT 10;"
```

---

## Setup Wizard

`voice-wizard` is an interactive 5-page ratatui TUI that guides first-time setup:

1. **Welcome** — program overview and mode descriptions
2. **System Check** — GPU/CUDA, whisper model, audio device, binary health
3. **Mode Select** — pick type / print / clip / ambient with arrow keys
4. **Options** — toggle per-mode flags; live command preview
5. **Launch** — copy command to clipboard or exec directly

```bash
voice-wizard
```

Navigation: `→`/Enter = next, `←` = back, `↑`/`↓` = select, `Space` = toggle option, `q` = quit.

---

## Directory MOTD

`cd`ing into the project directory auto-displays a status banner (binary health, model, mic, quick-ref).

This is wired via a `chpwd` hook in `~/.zshrc` — fires on any `cd` into a directory containing `.motd`.

```bash
cd ~/Documents/claude_creations/voice-input
# → banner displays automatically
```

---

## File Layout

```
voice-input/
├── README.md
├── STATUS.md
├── .motd                          ← directory banner (chpwd hook in ~/.zshrc)
├── voice-ambient/                 ← Rust workspace
│   ├── Cargo.toml
│   ├── .cargo/config.toml         ← CUDA 13.0 build config
│   └── src/
│       ├── lib.rs                 ← exports whisper_infer module
│       ├── whisper_infer.rs       ← whisper.cpp wrapper (load, resample, VAD, infer)
│       ├── main.rs                ← voice-ambient TUI binary
│       └── bin/
│           ├── voice_input.rs     ← voice-input CLI binary
│           ├── voice_wizard.rs    ← voice-wizard setup wizard
│           └── audio_test.rs      ← cpal device validation tool
├── sessions/                      ← timestamped session docs
├── diagrams/                      ← architecture diagrams
└── motd/                          ← legacy system motd fragment (superseded by .motd)
```

---

## Hardware Notes

- **UC03 USB headset** native sample rate: **32 kHz mono** — do not force 16 kHz (causes near-silence)
- **PipeWire source:** `alsa_input.usb-UC03_UC03-00.mono-fallback`
- **cpal device name:** `"pipewire"` (PipeWire's ALSA virtual device — routes to WirePlumber default source)
- **Mic gain:** ALSA 127/127 (+23.81 dB) — set during early debug; normalize if clipping observed:
  ```bash
  amixer -c 3 sset Mic 100
  pactl set-source-volume alsa_input.usb-UC03_UC03-00.mono-fallback 100%
  ```
- **WirePlumber default source/sink** persisted at:
  `~/.config/wireplumber/wireplumber.conf.d/51-default-audio.conf`

### Validate audio capture
```bash
cd ~/Documents/claude_creations/voice-input/voice-ambient
./target/release/audio-test
# captures 5 s from "pipewire" device → /tmp/voice-cpal-test.wav
# prints RMS level (should be > 0.01 during speech)
```

---

## Suggested Workflow with Claude Code

Split terminal:
- **Left pane:** Claude Code session
- **Right pane:** `voice-input --mode type --submit`

Speak → press Enter → transcription is typed directly into the Claude Code prompt and submitted.

For transcript logging alongside a long session:
```bash
voice-input --mode ambient --db ~/session-$(date +%Y%m%d).db
```
