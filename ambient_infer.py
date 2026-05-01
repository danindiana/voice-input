#!/usr/bin/env python3
"""
ambient_infer.py — inference-only daemon for voice-input ambient mode (Tier 2).

Reads WAV file paths from stdin (one per line, written by the Rust TUI).
For each path: transcribes the WAV, emits JSON utterance to stdout.
The Rust side handles all audio capture, RMS metering, and WAV encoding.

JSON protocol (stdout):
  {"type":"status",    "msg":"..."}
  {"type":"utterance", "text":"...", "timed":"...", "words":[...], "ts":"ISO8601"}
  {"type":"done"}
"""
import os, sys, json, signal
from datetime import datetime, timezone

os.environ["LD_LIBRARY_PATH"] = (
    "/usr/lib/ollama"
    + (":" + os.environ["LD_LIBRARY_PATH"] if "LD_LIBRARY_PATH" in os.environ else "")
)

from faster_whisper import WhisperModel

_SUP = str.maketrans("0123456789.", "⁰¹²³⁴⁵⁶⁷⁸⁹·")


def emit(obj: dict) -> None:
    print(json.dumps(obj), flush=True)


def load_model() -> WhisperModel:
    model_size = os.getenv("VOICE_WHISPER_MODEL", "large-v3")
    try:
        m = WhisperModel(model_size, device="cuda", compute_type="float16")
        emit({"type": "status", "msg": f"GPU (cuda/float16) model={model_size}"})
        return m
    except Exception as e:
        emit({"type": "status", "msg": f"CPU fallback ({type(e).__name__}) model={model_size}"})
        return WhisperModel(model_size, device="cpu", compute_type="int8")


def transcribe_wav(wav_path: str, model: WhisperModel) -> None:
    try:
        segs, _ = model.transcribe(
            wav_path, beam_size=5, language="en",
            word_timestamps=True, vad_filter=True,
        )
        plain, timed, words = [], [], []
        for seg in segs:
            for w in seg.words or []:
                t = w.word.strip()
                if not t:
                    continue
                ts = f"{w.start:.1f}ˢ".translate(_SUP)
                plain.append(t)
                timed.append(f"{t}{ts}")
                words.append({
                    "word": t, "start": w.start,
                    "end": w.end, "prob": w.probability,
                })
        if plain:
            emit({
                "type":  "utterance",
                "text":  " ".join(plain),
                "timed": " ".join(timed),
                "words": words,
                "ts":    datetime.now(timezone.utc).isoformat(),
            })
    except Exception as e:
        emit({"type": "status", "msg": f"infer error: {e}"})
    finally:
        try:
            os.unlink(wav_path)
        except OSError:
            pass


def main() -> None:
    model = load_model()
    emit({"type": "status", "msg": "Recording"})

    def stop(*_) -> None:
        emit({"type": "done"})
        sys.exit(0)

    signal.signal(signal.SIGINT,  stop)
    signal.signal(signal.SIGTERM, stop)

    try:
        for line in sys.stdin:
            wav_path = line.strip()
            if wav_path:
                transcribe_wav(wav_path, model)
    except (KeyboardInterrupt, BrokenPipeError):
        pass
    finally:
        stop()


if __name__ == "__main__":
    main()
