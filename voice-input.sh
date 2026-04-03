#!/usr/bin/env bash
# voice-input.sh — push-to-talk for terminal / Claude Code
#
# Usage:
#   ./voice-input.sh            # record until Enter, then type into active window
#   ./voice-input.sh --clip     # same, but only copy to clipboard (no auto-type)
#   ./voice-input.sh --print    # print to stdout only (for scripting)
#
# Max recording window: 65 seconds (press Enter to stop early)
# Audio feedback: low beep = recording started, high beep = stopped
#
# Requires: parec, sox, faster-whisper (venv), xdotool, xclip

set -euo pipefail

# libcublas.so.12 lives with Ollama's bundled CUDA libs
export LD_LIBRARY_PATH="/usr/lib/ollama${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

VENV="/home/jeb/programs/python_programs/venv"
SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
TRANSCRIBE="$SCRIPT_DIR/transcribe.py"
MIC_SOURCE="alsa_input.usb-UC03_UC03-00.mono-fallback"
AUDIO_SINK="alsa_output.usb-UC03_UC03-00.analog-stereo"
TMPRAW=$(mktemp /tmp/voice-XXXXXX.raw)
TMPWAV=$(mktemp /tmp/voice-XXXXXX.wav)
MAX_SECONDS=65

MODE="type"   # type | clip | print
if [[ "${1:-}" == "--clip" ]];  then MODE="clip";  fi
if [[ "${1:-}" == "--print" ]]; then MODE="print"; fi

cleanup() { rm -f "$TMPRAW" "$TMPWAV"; }
trap cleanup EXIT

# Play a tone through the headset: beep <freq> <duration_sec>
beep() {
    local freq="${1:-440}" dur="${2:-0.12}"
    sox -n -t wav - synth "$dur" sine "$freq" 2>/dev/null \
        | paplay --device="$AUDIO_SINK" 2>/dev/null &
}

# --- Record ---
echo "[voice-input] Recording (max ${MAX_SECONDS}s)... press Enter to stop early." >&2
beep 480 0.15   # low tone = start

# Write raw PCM to file — avoids WAV header race when killed
parec --device="$MIC_SOURCE" --format=s16le --rate=32000 --channels=1 \
      --raw > "$TMPRAW" &
PAREC_PID=$!
MAIN_PID=$$

# Timer: after MAX_SECONDS, kill parec and wake main script via USR1
( sleep "$MAX_SECONDS"
  kill "$PAREC_PID" 2>/dev/null
  kill -USR1 "$MAIN_PID" 2>/dev/null
) &
TIMER_PID=$!

# USR1 interrupts read so the timeout path also unblocks
trap 'true' USR1

# Block until Enter (or USR1 from timer)
read -r _ < /dev/tty || true

# Either path: clean up both timer and parec
trap - USR1
kill "$TIMER_PID" 2>/dev/null || true
kill "$PAREC_PID" 2>/dev/null || true
wait "$PAREC_PID" 2>/dev/null || true

beep 880 0.15   # high tone = stopped

# Convert raw PCM → WAV now that recording is cleanly done
sox -t raw -r 32000 -e signed -b 16 -c 1 "$TMPRAW" "$TMPWAV"

echo "[voice-input] Transcribing..." >&2

# --- Transcribe ---
TEXT=$("$VENV/bin/python3" "$TRANSCRIBE" "$TMPWAV" 2>/dev/null)

if [[ -z "$TEXT" ]]; then
    echo "[voice-input] No speech detected." >&2
    exit 0
fi

echo "[voice-input] Got: $TEXT" >&2

# --- Output ---
case "$MODE" in
    type)
        sleep 0.1
        xdotool type --clearmodifiers --delay 20 "$TEXT"
        ;;
    clip)
        echo -n "$TEXT" | xclip -selection clipboard
        echo "[voice-input] Copied to clipboard." >&2
        ;;
    print)
        echo "$TEXT"
        ;;
esac
