# voice-input

GPU-accelerated push-to-talk speech-to-text for Linux terminals. Captures mic input, transcribes locally via [faster-whisper](https://github.com/SYSTRAN/faster-whisper), and outputs text to stdout, clipboard, or types it directly into the active window.

Built for and tested on: Ubuntu/Debian, PipeWire audio, NVIDIA GPU, UC03 USB headset.

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
voice-input --print    # transcribe → stdout
voice-input --clip     # transcribe → clipboard (Ctrl+Shift+V to paste)
voice-input            # transcribe → xdotool types into active window
```

Press **Enter** to stop recording early. Auto-stops at 65 seconds.  
Low beep = recording started. High beep = stopped, transcribing.

---

## Requirements

### Hardware
- USB headset or microphone (tested: UC03 USB)
- NVIDIA GPU with CUDA (tested: RTX 3060 12GB, RTX 3080 10GB)

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

### CUDA
- Requires `libcublas.so.12` — **not** included in standard CUDA 11 installs
- If you have [Ollama](https://ollama.com) installed, it bundles this at `/usr/lib/ollama/`
- The script sets `LD_LIBRARY_PATH=/usr/lib/ollama` automatically
- Alternatively: install CUDA 12 toolkit

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
| `voice-input.sh` | Main script — records, dispatches to transcribe.py, outputs text |
| `transcribe.py` | Loads faster-whisper medium model on GPU, transcribes WAV to stdout |
| `requirements.txt` | Frozen Python dependencies |
| `STATUS.md` | Debug history, build notes, lessons learned |

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

Default model: `faster-whisper medium` (~1.5 GB, float16 on GPU).

To use a larger/smaller model, edit `transcribe.py`:
```python
model = WhisperModel("large-v3", device="cuda", compute_type="float16")  # more accurate
model = WhisperModel("small",    device="cuda", compute_type="float16")  # faster
model = WhisperModel("medium",   device="cpu",  compute_type="int8")     # no GPU
```

---

## Suggested Workflow with Claude Code

Open a split terminal:
- **Left pane:** Claude Code session
- **Right pane:** `voice-input --clip`

Speak → press Enter → `Ctrl+Shift+V` into the Claude Code prompt.
