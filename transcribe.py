#!/usr/bin/env python3
"""
Transcribe a WAV file using faster-whisper on GPU.

Usage:
    transcribe.py <wav_file>           # plain text to stdout
    transcribe.py <wav_file> --fancy   # animated word-by-word with probability render
"""
import sys
import time
import random
import string

from faster_whisper import WhisperModel

# Characters used during scramble animation
SCRAMBLE = string.ascii_lowercase + string.digits + "!?#@$%&*"

# Unicode superscript map for timestamps
_SUP_MAP = str.maketrans("0123456789.", "⁰¹²³⁴⁵⁶⁷⁸⁹·")

def sup_time(t: float) -> tuple:
    """Return (raw_str, ansi_str) for a timestamp, e.g. ('¹²·³ˢ', '\033[2m¹²·³ˢ\033[0m')"""
    raw = f"{t:.1f}ˢ".translate(_SUP_MAP)
    return raw, f"\033[2m{raw}\033[0m"

def word_color(prob: float) -> str:
    if prob >= 0.92:
        return "\033[97m"    # bright white — high confidence
    elif prob >= 0.75:
        return "\033[0m"     # normal
    elif prob >= 0.50:
        return "\033[33m"    # yellow — uncertain
    else:
        return "\033[31m"    # red — low confidence

def animate_word(word: str, prob: float, start: float) -> None:
    """
    Print a single word with scramble-to-resolve animation.
    Uses \033[{N}D (cursor-left) to overwrite in place — compatible with
    terminals that don't implement cursor save/restore (\033[s/u).
    Number of scramble frames scales with (1 - probability).
    """
    frames   = max(1, int((1.0 - prob) * 10))
    ts_raw, ts_ansi = sup_time(start)
    color    = word_color(prob)
    width    = len(word)
    # Step back width + len(ts_raw) + 1 space to overwrite in place
    back     = width + len(ts_raw) + 1

    for _ in range(frames):
        scrambled = ''.join(random.choice(SCRAMBLE) for _ in range(width))
        sys.stdout.write(f"\033[2m{scrambled}\033[0m{ts_ansi} \033[{back}D")
        sys.stdout.flush()
        time.sleep(0.035)

    # Final: write real word then advance cursor (no cursor-left)
    sys.stdout.write(f"{color}{word}\033[0m{ts_ansi} ")
    sys.stdout.flush()

def fancy_transcribe(wav_path: str, model: WhisperModel) -> None:
    """Stream words to terminal with per-word probability animation."""
    segments, _ = model.transcribe(
        wav_path,
        beam_size=5,
        language="en",
        word_timestamps=True,
    )

    col = 0  # track approximate column for soft line wrapping
    for segment in segments:
        for word in (segment.words or []):
            w = word.word.strip()
            if not w:
                continue

            # Soft wrap at ~100 cols
            if col + len(w) + 12 > 100:
                sys.stdout.write("\n")
                col = 0

            # Print placeholder underscores at word width so line doesn't jump
            ts_raw, ts_ansi = sup_time(word.start)
            marker = f"\033[2m{'_' * len(w)}\033[0m{ts_ansi} "
            sys.stdout.write(marker)
            sys.stdout.flush()

            animate_word(w, word.probability, word.start)
            col += len(w) + len(ts_raw) + 1

    sys.stdout.write("\n")

def plain_transcribe(wav_path: str, model: WhisperModel) -> None:
    segments, _ = model.transcribe(wav_path, beam_size=5, language="en")
    print(" ".join(seg.text.strip() for seg in segments))

def main() -> None:
    fancy   = "--fancy" in sys.argv
    args    = [a for a in sys.argv[1:] if not a.startswith("--")]

    if not args:
        print("Usage: transcribe.py <wav_file> [--fancy]", file=sys.stderr)
        sys.exit(1)

    model = WhisperModel("medium", device="cuda", compute_type="float16")

    if fancy:
        fancy_transcribe(args[0], model)
    else:
        plain_transcribe(args[0], model)

if __name__ == "__main__":
    main()
