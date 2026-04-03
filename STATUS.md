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

## Next Steps

- [ ] Global hotkey via `xbindkeys` — launch without second terminal
- [ ] Try `large-v3` model for better accuracy
- [ ] Normalize mic gain after confirming no clipping
- [ ] Auto-submit mode (append `\n` via xdotool)
