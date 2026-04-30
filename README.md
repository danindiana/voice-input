# voice-input

<p align="center">
  <img src="voice-input-logo.png" alt="voice-input logo" width="400"/>
</p>

Push-to-talk speech-to-text for Linux terminals. Captures mic input, transcribes locally via [faster-whisper](https://github.com/SYSTRAN/faster-whisper), and outputs text to stdout, clipboard, or types it directly into the active window. Uses GPU when available, falls back to CPU automatically.

Built for and tested on: Ubuntu/Debian, PipeWire audio, NVIDIA GPU (optional), UC03 USB headset.

---

## Quickstart

```bash
# 1. Install system dependencies
sudo apt install sox pipewire-audio-client-libraries xdotool xclip

# 2. Install Python dependencies (in your venv)
pip install -r requirements.txt

# 3. Edit voice-input.sh — set VENV to your Python venv path

# 4. Make executable and put on PATH
chmod +x voice-input.sh
sudo ln -sf "$(pwd)/voice-input.sh" /usr/local/bin/voice-input

# 5. Run
voice-input                     # transcribe → animated display, exit clean (default)
voice-input --clip              # transcribe → clipboard (Ctrl+Shift+V to paste)
voice-input --type              # transcribe → xdotool types into active window
voice-input --print             # transcribe → stdout (animated)
voice-input --print --no-fancy  # transcribe → stdout (plain)
```

Press **Enter** to stop recording early. Auto-stops at 65 seconds.  
Low beep = recording started. High beep = stopped, transcribing.

---

## Requirements

### Hardware
- USB headset or microphone (tested: UC03 USB)
- NVIDIA GPU with CUDA — optional (tested: RTX 3060 12GB, RTX 3080 10GB); falls back to CPU if unavailable

### System packages
| Package | Purpose |
|---------|---------|
| `parec` | PipeWire/PulseAudio capture (part of `pipewire-audio-client-libraries`) |
| `sox` | Raw PCM → WAV conversion, tone generation |
| `xdotool` | Types transcribed text into active window |
| `xclip` | Clipboard output mode |
| `paplay` | Plays audio feedback beeps |

### Python
- Python 3.8+
- See `requirements.txt` — install with `pip install -r requirements.txt`
- Whisper model (~1.5 GB) downloads automatically on first run to `~/.cache/huggingface/`

### CUDA (optional)
- GPU mode requires `libcublas.so.12` — **not** included in standard CUDA 11 installs
- If you have [Ollama](https://ollama.com) installed, it bundles this at `/usr/lib/ollama/`
- The script sets `LD_LIBRARY_PATH=/usr/lib/ollama` automatically
- Alternatively: install CUDA 12 toolkit
- If CUDA init fails for any reason (GPU absent, OOM, driver issue, contention), `transcribe.py` automatically falls back to CPU with `int8` quantization and logs a warning to stderr

---

## Configuration

Edit the variables at the top of `voice-input.sh`:

| Variable | Default | Description |
|----------|---------|-------------|
| `VENV` | `/home/jeb/programs/python_programs/venv` | Path to Python venv |
| `MIC_SOURCE` | `alsa_input.usb-UC03_UC03-00.mono-fallback` | PipeWire source name |
| `AUDIO_SINK` | `alsa_output.usb-UC03_UC03-00.analog-stereo` | PipeWire sink for beeps |
| `MAX_SECONDS` | `65` | Maximum recording window |

Find your mic's PipeWire source name:
```bash
pactl list sources short
```

---

## Files

| File | Purpose |
|------|---------|
| `voice-input.sh` | Main script — records, dispatches, outputs text; routes `--ambient` to Rust TUI |
| `transcribe.py` | faster-whisper transcription (plain, fancy-animated, dual, timed) |
| `ambient.py` | Continuous mic capture + transcription worker; emits JSON to Rust TUI |
| `voice-ambient/` | Rust/Ratatui TUI binary for ambient mode |
| `requirements.txt` | Frozen Python dependencies |
| `STATUS.md` | Debug history, build notes, lessons learned |

---

## Fancy Animated Output

By default, `--print` mode uses an animated word-by-word renderer powered by faster-whisper's `word_timestamps=True`:

- Each word prints a placeholder (`___`) as soon as the model starts yielding that segment
- Scrambled characters cycle in-place (more frames = lower confidence)
- Word snaps to final text, colored by confidence:
  - **Bright white** — ≥ 92% confidence
  - Normal — ≥ 75%
  - **Yellow** — ≥ 50%
  - **Red** — < 50%
- Dim superscript timestamp beside each word: `better¹²·⁹ˢ`

Pass `--no-fancy` to get plain text output instead:
```bash
voice-input --print --no-fancy
```

**Implementation note:** the animation uses `\033[{N}D` (cursor-left by N columns) to overwrite in place. `\033[s/u` (cursor save/restore) fails silently in many terminals (tmux, some VTE-based), causing frames to print sequentially instead of in-place.

---

## Build Issues & Lessons Learned

### 1. UC03 native sample rate is 32 kHz — not 16 kHz

Recording at `--rate=16000` caused PipeWire's resampler to produce near-silence (RMS ~0.0005, needed 344× boost). At native `--rate=32000` the signal is healthy.

**Rule:** always check your device's native rate with `pactl list sources` before hardcoding a sample rate. faster-whisper handles 32 kHz input natively.

### 2. libcublas.so.12 missing from system path

faster-whisper (via ctranslate2) requires `libcublas.so.12`. Ubuntu packages typically ship `.so.11`. Ollama bundles `.so.12` at `/usr/lib/ollama/`. The script exports `LD_LIBRARY_PATH=/usr/lib/ollama` to resolve this without a full CUDA 12 install.

### 3. Symlink breaks SCRIPT_DIR resolution

`/usr/local/bin/voice-input` is a symlink. Using `dirname "${BASH_SOURCE[0]}"` resolved to `/usr/local/bin/`, causing `transcribe.py` to not be found. Fixed with:
```bash
SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
```

### 4. WAV header corruption when killing a sox pipeline

Originally: `parec | sox ... output.wav &` — killing the background job left the WAV header incomplete. Fixed by writing raw PCM to a temp file with parec, then converting with sox after parec exits cleanly.

### 5. Default PipeWire source pointed at motherboard monitor

On first headset plug-in, system default source was `alsa_output.pci-0000_10_00.4.analog-stereo.monitor` (a loopback monitor of the onboard audio output — not a mic). Set correct defaults:

```bash
pactl set-default-sink alsa_output.usb-UC03_UC03-00.analog-stereo
pactl set-default-source alsa_input.usb-UC03_UC03-00.mono-fallback
```

Persisted via WirePlumber config:
```
~/.config/wireplumber/wireplumber.conf.d/51-default-audio.conf
```

### 6. `read` exits immediately when stdin is not a TTY

`read -r _` returned instantly when the script's stdin was not a terminal (e.g. piped or backgrounded). Fixed by reading from `/dev/tty` explicitly:
```bash
read -r _ < /dev/tty || true
```

Combined with a USR1 signal from a background timer to unblock `read` on timeout.

---

## Whisper Model

Default model: `faster-whisper medium` (~1.5 GB).

Device selection is automatic: GPU (`float16`) is tried first; CPU (`int8`) is used if GPU init fails.

To switch model size, edit the `load_model()` call in `transcribe.py`:
```python
model = load_model("large-v3")  # more accurate, ~3 GB
model = load_model("small")     # faster, ~500 MB
model = load_model("medium")    # default
```

---

## Ambient Mode

`voice-input --ambient` opens a full-screen TUI that keeps the microphone open and transcribes continuously until you press `q`, `Esc`, or `Ctrl-C`.

```
┌● REC  voice-input : ambient  00:05:23──────────────────────────┐
│                                                                   │
│ waveform                                                          │
│▁▂▄▆▇▇▆▄▃▂▁▁▁▂▃▄▅▆▇▆▅▄▃▂▁▁▁▁▂▃▄▅▅▄▃▂▁▁▁▁▁▁▁▁▂▃▄▅▆▇▇▆▅▄▃▂▁▁▁▁│
│████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░         │
├───────────────────────────────────────────────────────────────────┤
│ transcript                                                        │
│ 14:32:01 hello world how are you doing today                      │
│ 14:32:08 i think we should implement this feature                 │
│ 14:32:16 that way we can test it properly                         │
├───────────────────────────────────────────────────────────────────┤
│ Recording  │  words: 47  utt: 8  │  DB: off    q / Ctrl-C to stop│
└───────────────────────────────────────────────────────────────────┘
```

**Visual cues:**
- Waveform sparkline scrolls left as audio arrives (the "waterfall")
- Level gauge shifts green → yellow → red by amplitude
- Transcript lines fade cyan → white → gray → dark-gray as they age
- REC indicator blinks red while recording

**SQLite logging** (`--db <path>`):

```bash
voice-input --ambient --db ~/transcripts.db
```

Creates (or appends to) an SQLite database:

```sql
sessions   (id, started_at, ended_at)
utterances (id, session_id, recorded_at, text, word_count)
```

Query example:
```bash
sqlite3 ~/transcripts.db "SELECT recorded_at, text FROM utterances ORDER BY id DESC LIMIT 10;"
```

### Building the ambient binary

Requires Rust / Cargo (install via https://rustup.rs if needed):

```bash
cd voice-ambient
cargo build --release
# binary: voice-ambient/target/release/voice-ambient (~3 MB, statically linked SQLite)
```

The binary is auto-discovered by `voice-input.sh` at that path. To put it on PATH manually:
```bash
sudo ln -sf "$(pwd)/voice-ambient/target/release/voice-ambient" /usr/local/bin/voice-ambient
```

---

## Suggested Workflow with Claude Code

Open a split terminal:
- **Left pane:** Claude Code session
- **Right pane:** `voice-input --clip`

Speak → press Enter → `Ctrl+Shift+V` into the Claude Code prompt.

For longer sessions where you want a transcript log:
```bash
voice-input --ambient --db ~/session-$(date +%Y%m%d).db
```
