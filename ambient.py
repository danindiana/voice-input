#!/usr/bin/env python3
"""
ambient.py — continuous microphone transcription for voice-input --ambient mode.

Emits JSON lines to stdout (read by the voice-ambient Rust TUI):
  {"type":"status",    "msg":"..."}
  {"type":"level",     "rms":0.0-1.0}
  {"type":"utterance", "text":"...", "timed":"...", "words":[...], "ts":"ISO8601"}
  {"type":"done"}

Audio is captured in 100 ms slices (for VU level) and accumulated into
5-second chunks that are queued to a transcription worker thread, so the
capture loop never blocks on model inference.
"""
import os, sys, json, math, struct, signal, subprocess, tempfile, threading, time
from queue import Queue, Full
from datetime import datetime, timezone

os.environ["LD_LIBRARY_PATH"] = (
    "/usr/lib/ollama"
    + (":" + os.environ["LD_LIBRARY_PATH"] if "LD_LIBRARY_PATH" in os.environ else "")
)

from faster_whisper import WhisperModel

RATE        = 32000
BYTES_S     = 2                              # signed 16-bit mono
LEVEL_SECS  = 0.10                           # VU update interval
CHUNK_SECS  = 5                              # transcription window
LEVEL_BYTES = int(RATE * BYTES_S * LEVEL_SECS)
CHUNK_BYTES = RATE * BYTES_S * CHUNK_SECS

MIC_SOURCE  = os.getenv("VOICE_MIC_SOURCE", "alsa_input.usb-UC03_UC03-00.mono-fallback")
_SUP        = str.maketrans("0123456789.", "⁰¹²³⁴⁵⁶⁷⁸⁹·")


def emit(obj: dict) -> None:
    print(json.dumps(obj), flush=True)


def rms_norm(data: bytes) -> float:
    n = len(data) // 2
    if n == 0:
        return 0.0
    samples = struct.unpack(f"<{n}h", data[: n * 2])
    return min(1.0, math.sqrt(sum(s * s for s in samples) / n) / 32768.0)


def load_model() -> WhisperModel:
    try:
        model_size = os.getenv("VOICE_WHISPER_MODEL", "medium")
        m = WhisperModel(model_size, device="cuda", compute_type="float16")
        emit({"type": "status", "msg": f"GPU (cuda/float16) model={model_size}"})
        return m
    except Exception as e:
        model_size = os.getenv("VOICE_WHISPER_MODEL", "medium")
        emit({"type": "status", "msg": f"CPU fallback ({type(e).__name__}) model={model_size}"})
        return WhisperModel(model_size, device="cpu", compute_type="int8")


def transcription_worker(q: Queue, model: WhisperModel) -> None:
    """Drain raw-PCM chunks from q, transcribe each, emit utterance JSON."""
    while True:
        raw = q.get()
        if raw is None:
            break
        wav = None
        try:
            with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
                wav = f.name
            proc = subprocess.run(
                ["sox", "-t", "raw", "-r", str(RATE), "-e", "signed",
                 "-b", "16", "-c", "1", "-", wav],
                input=raw, capture_output=True,
            )
            if proc.returncode != 0:
                continue
            segs, _ = model.transcribe(
                wav, beam_size=5, language="en",
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
            emit({"type": "status", "msg": f"trans error: {e}"})
        finally:
            if wav:
                try:
                    os.unlink(wav)
                except OSError:
                    pass
        q.task_done()


def main() -> None:
    model = load_model()
    q: Queue = Queue(maxsize=4)

    worker = threading.Thread(target=transcription_worker, args=(q, model), daemon=True)
    worker.start()

    parec = subprocess.Popen(
        ["parec", f"--device={MIC_SOURCE}", "--format=s16le",
         f"--rate={RATE}", "--channels=1", "--raw"],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    )
    emit({"type": "status", "msg": "Recording"})

    buf      = bytearray()
    lvl_buf  = bytearray()
    last_lvl = time.monotonic()

    def stop(*_) -> None:
        parec.kill()
        q.put(None)
        emit({"type": "done"})
        sys.exit(0)

    signal.signal(signal.SIGINT,  stop)
    signal.signal(signal.SIGTERM, stop)

    try:
        while True:
            chunk = parec.stdout.read(LEVEL_BYTES)
            if not chunk:
                break
            buf.extend(chunk)
            lvl_buf.extend(chunk)

            now = time.monotonic()
            if now - last_lvl >= LEVEL_SECS:
                emit({"type": "level", "rms": rms_norm(bytes(lvl_buf))})
                lvl_buf.clear()
                last_lvl = now

            if len(buf) >= CHUNK_BYTES:
                raw = bytes(buf)
                buf.clear()
                try:
                    q.put_nowait(raw)
                except Full:
                    emit({"type": "status", "msg": "Queue full — skipping chunk"})
    except (KeyboardInterrupt, BrokenPipeError):
        pass
    finally:
        stop()


if __name__ == "__main__":
    main()
