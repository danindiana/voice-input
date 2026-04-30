#!/usr/bin/env bash
# voice-input.sh — push-to-talk for terminal / Claude Code
#
# Usage:
#   ./voice-input.sh            # record, show animated transcript, exit clean (default)
#   ./voice-input.sh --clip     # copy transcript to clipboard
#   ./voice-input.sh --type     # type transcript into active window (xdotool)
#   ./voice-input.sh --print    # print to stdout only (for scripting)
#   ./voice-input.sh --ambient  # continuous ambient mode (Ratatui TUI)
#   ./voice-input.sh --ambient --db /path/to/db.sqlite   # + SQLite logging
#
# Max recording window (push-to-talk modes): 65 seconds (press Enter to stop early)
# Audio feedback: low beep = recording started, high beep = stopped
#
# Requires: parec, sox, faster-whisper (venv), xdotool, xclip
# Ambient also requires: voice-ambient binary (cd voice-ambient && cargo build --release)

set -euo pipefail

# libcublas.so.12 lives with Ollama's bundled CUDA libs
export LD_LIBRARY_PATH="/usr/lib/ollama${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

VENV="/home/jeb/programs/python_programs/venv"
SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"
TRANSCRIBE="$SCRIPT_DIR/transcribe.py"
MIC_SOURCE="alsa_input.usb-UC03_UC03-00.mono-fallback"
AUDIO_SINK="alsa_output.usb-UC03_UC03-00.analog-stereo"
MAX_SECONDS=65
DB_PATH=""
TMPRAW=""
TMPWAV=""

for arg in "$@"; do
    if [[ "$arg" == "--help" || "$arg" == "-h" ]]; then
        cat <<'EOF'
voice-input — push-to-talk speech-to-text

USAGE
  voice-input [OPTIONS]

OUTPUT MODES (mutually exclusive; default: clean exit)
  (none)        Show animated transcript on the terminal, then exit cleanly.
                Nothing is typed, copied, or written anywhere.
  --clip        Copy transcribed text to the clipboard (X11 primary selection).
                Paste with Ctrl+Shift+V (terminal) or Ctrl+V (GUI apps).
  --print       Print two lines to stdout (after any animation):
                  Line 1: each word with its superscript timestamp  (hello¹·²ˢ world²·⁴ˢ)
                  Line 2: plain text only                           (hello world)
                Useful for scripting, logging, or piping.
  --type        Type transcribed text into the active window via xdotool.
                Switch to the target window before pressing Enter to stop.
                Use with care — types into whatever has focus.
  --ambient     Continuous push-to-listen mode. Leaves the microphone open and
                transcribes indefinitely. Renders a full-screen Ratatui TUI with:
                  • Scrolling waveform sparkline (audio waterfall)
                  • Colour-coded level gauge (green / yellow / red)
                  • Live transcript — new utterances appear at bottom, older
                    lines fade from cyan → white → gray → dark-gray over time
                  • Stats bar (words, utterances, elapsed, DB path)
                Press q, Esc, or Ctrl-C to stop.
                Requires: voice-ambient binary (see Build section in README).
  --db <path>   Used with --ambient. Write every utterance to a SQLite database
                at <path>. Creates the file if it does not exist.
                Schema: sessions(id, started_at, ended_at)
                        utterances(id, session_id, recorded_at, text, word_count)

DISPLAY OPTIONS
  --fancy       Animate each word as it is transcribed: scrambled characters
                resolve to the final word, colored by confidence.
                  bright white  ≥92% confidence
                  normal        ≥75%
                  yellow        ≥50%
                  red           <50%
                Superscript timestamps show when each word was spoken.
                This is the default when --print is active.
  --no-fancy    Skip animation; print plain text only. Faster for scripting.

OTHER
  --help, -h    Show this help and exit.

RECORDING
  Press Enter to stop recording early.
  Auto-stops after 65 seconds.
  Low beep  = recording started.
  High beep = recording stopped, transcription in progress.

TRANSCRIPTION
  Uses faster-whisper (medium model) locally — no cloud API.
  Tries GPU (CUDA float16) first; falls back to CPU (int8) if GPU is
  unavailable or occupied by another process.

EXAMPLES
  voice-input                              # speak, see animated transcript, exit clean
  voice-input --clip                       # speak, copy to clipboard, paste anywhere
  voice-input --type                       # speak, type into active window (xdotool)
  voice-input --print                      # speak, animate, then print timed + plain lines
  voice-input --print --no-fancy           # speak, print timed + plain lines (no animation)
  voice-input --print | tail -1            # extract plain-text line only
  voice-input --print | head -1            # extract timed line only
  voice-input --ambient                    # continuous TUI transcription, no logging
  voice-input --ambient --db ~/notes.db   # continuous TUI transcription + SQLite log

HARDWARE (this machine)
  Mic source : alsa_input.usb-UC03_UC03-00.mono-fallback
  Audio sink : alsa_output.usb-UC03_UC03-00.analog-stereo
  Sample rate: 32000 Hz mono (UC03 native — do not change)
EOF
        exit 0
    fi
done

MODE="default"  # default | print | clip | type | ambient
FANCY="--fancy"
_ndb=false
for arg in "$@"; do
    if [[ "$_ndb" == true ]]; then DB_PATH="$arg"; _ndb=false; continue; fi
    case "$arg" in
        --clip)     MODE="clip"    ;;
        --print)    MODE="print"   ;;
        --type)     MODE="type"    ;;
        --ambient)  MODE="ambient" ;;
        --no-fancy) FANCY=""       ;;
        --db)       _ndb=true      ;;
        --db=*)     DB_PATH="${arg#--db=}" ;;
    esac
done

# Ambient mode: hand off entirely to the Rust TUI binary (no push-to-talk recording)
if [[ "$MODE" == "ambient" ]]; then
    BINARY="$SCRIPT_DIR/voice-ambient/target/release/voice-ambient"
    if [[ ! -x "$BINARY" ]]; then
        echo "[voice-input] ambient binary not found. Build it first:" >&2
        echo "  cd $SCRIPT_DIR/voice-ambient && cargo build --release" >&2
        exit 1
    fi
    ARGS=(--script "$SCRIPT_DIR/ambient.py" --python "$VENV/bin/python3")
    [[ -n "$DB_PATH" ]] && ARGS+=(--db "$DB_PATH")
    exec "$BINARY" "${ARGS[@]}"
fi

# Push-to-talk modes: create temp files and register cleanup
TMPRAW=$(mktemp /tmp/voice-XXXXXX.raw)
TMPWAV=$(mktemp /tmp/voice-XXXXXX.wav)
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
# default: fancy animation → exit clean (nothing captured or dispatched)
# print:   fancy animation (if on) → dual output: timed line + plain line → exit
# clip/type: fancy animation (if on) → plain capture → dispatch

if [[ -n "$FANCY" ]]; then
    "$VENV/bin/python3" "$TRANSCRIBE" "$TMPWAV" --fancy 2>/dev/null
    if [[ "$MODE" == "default" ]]; then
        exit 0
    fi
    if [[ "$MODE" == "print" ]]; then
        "$VENV/bin/python3" "$TRANSCRIBE" "$TMPWAV" --dual 2>/dev/null
        exit 0
    fi
    # clip/type need plain text
    TEXT=$("$VENV/bin/python3" "$TRANSCRIBE" "$TMPWAV" 2>/dev/null)
else
    if [[ "$MODE" == "default" ]]; then
        exit 0
    fi
    if [[ "$MODE" == "print" ]]; then
        "$VENV/bin/python3" "$TRANSCRIBE" "$TMPWAV" --dual 2>/dev/null
        exit 0
    fi
    TEXT=$("$VENV/bin/python3" "$TRANSCRIBE" "$TMPWAV" 2>/dev/null)
fi

if [[ -z "${TEXT:-}" ]]; then
    echo "[voice-input] No speech detected." >&2
    exit 0
fi

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
esac
