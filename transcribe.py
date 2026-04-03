#!/usr/bin/env python3
"""
Transcribe a WAV file using faster-whisper on GPU.
Usage: transcribe.py <wav_file>
Prints transcribed text to stdout.
"""
import sys
from faster_whisper import WhisperModel

def main():
    if len(sys.argv) < 2:
        print("Usage: transcribe.py <wav_file>", file=sys.stderr)
        sys.exit(1)

    wav_path = sys.argv[1]

    # Use GPU (cuda), float16, medium model — fast + accurate
    # Model downloads to ~/.cache/huggingface on first run
    model = WhisperModel("medium", device="cuda", compute_type="float16")

    segments, info = model.transcribe(wav_path, beam_size=5, language="en")

    text = " ".join(seg.text.strip() for seg in segments)
    print(text)

if __name__ == "__main__":
    main()
