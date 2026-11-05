"""Continuous local wake-word recognition and one-command listening sessions."""

import json
import math
import os
import selectors
import subprocess
import sys
import tempfile
import time

from stt import SAMPLE_RATE, audio_stats, emit, normalize_command


def is_wake(result, confidence):
    words = result.get("result", [])
    return (
        result.get("text", "").strip().lower() == "ascolta"
        and len(words) == 1
        and words[0].get("word") == "ascolta"
        and words[0].get("conf", 0) >= confidence
    )


class WakeSession:
    """The microphone stays open, but audio is ignored during LED/LLM work."""

    def __init__(self, model, confidence=0.85, timeout=12, output=emit, recognizer=None):
        if not math.isfinite(confidence) or not 0 <= confidence <= 1:
            raise ValueError("GPIO_WAKE_CONFIDENCE deve essere tra 0 e 1")
        if not math.isfinite(timeout) or not 1 <= timeout <= 30:
            raise ValueError("GPIO_VOICE_COMMAND_TIMEOUT deve essere tra 1 e 30 secondi")
        if recognizer is None:
            from vosk import KaldiRecognizer
            recognizer = KaldiRecognizer
        self.factory = recognizer
        self.model = model
        if hasattr(model, "vosk_model_find_word") and model.vosk_model_find_word("ascolta") < 0:
            raise ValueError("il modello italiano non contiene la parola ASCOLTA")
        self.wake = recognizer(model, SAMPLE_RATE, json.dumps(["ascolta", "[unk]"]))
        self.wake.SetWords(True)
        self.confidence = confidence
        self.timeout = timeout
        self.output = output
        self.state = "paused"
        self.command = None
        self.audio = bytearray()
        self.started = 0
        self.stt_s = 0

    def control(self, command, now=None):
        now = time.monotonic() if now is None else now
        if command == "arm" and self.state in ("paused", "busy", "waiting"):
            self.wake.Reset()
            self.command = None
            self.audio.clear()
            self.state = "wake"
            self.output({"event": "armed"})
        elif command == "listen" and self.state == "waiting":
            self.command = self.factory(self.model, SAMPLE_RATE)
            self.audio.clear()
            self.started = now
            self.stt_s = 0
            self.state = "command"
            self.output({"event": "listening"})
        else:
            raise ValueError(f"Controllo {command!r} non ammesso nello stato {self.state}")

    def tick(self, now=None):
        now = time.monotonic() if now is None else now
        if self.state == "command" and now - self.started >= self.timeout:
            # A partial sentence at the deadline is never promoted to a command.
            self.state = "busy"
            self.audio.clear()
            self.output({"event": "timeout", "error": "nessun comando completo entro il tempo di ascolto"})

    def feed(self, pcm, now=None):
        self.tick(now)
        if self.state == "wake":
            if self.wake.AcceptWaveform(pcm):
                result = json.loads(self.wake.Result())
                if is_wake(result, self.confidence):
                    self.state = "waiting"
                    self.output({"event": "wake", "confidence": result["result"][0]["conf"]})
        elif self.state == "command":
            self.audio.extend(pcm)
            if len(self.audio) > self.timeout * SAMPLE_RATE * 2:
                self.state = "busy"
                self.audio.clear()
                self.output({"event": "timeout", "error": "comando troppo lungo"})
                return
            started = time.monotonic()
            complete = self.command.AcceptWaveform(pcm)
            self.stt_s += time.monotonic() - started
            if complete:
                text = json.loads(self.command.Result()).get("text", "").strip()
                if text:
                    self.state = "busy"
                    self.output({
                        "event": "transcript", "text": text,
                        "llm_text": normalize_command(text),
                        "stt_s": round(self.stt_s, 3),
                        **audio_stats(self.audio),
                    })
                    self.audio.clear()


def run(model, device):
    session = WakeSession(
        model,
        confidence=float(os.environ.get("GPIO_WAKE_CONFIDENCE", "0.85")),
        timeout=float(os.environ.get("GPIO_VOICE_COMMAND_TIMEOUT", "12")),
    )
    capture = None
    with tempfile.TemporaryFile() as diagnostics, selectors.DefaultSelector() as selector:
        try:
            capture = subprocess.Popen(
                ["arecord", "-q", "-D", device, "-t", "raw", "-f", "S16_LE", "-r", str(SAMPLE_RATE), "-c", "1"],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=diagnostics,
            )
            selector.register(capture.stdout, selectors.EVENT_READ, "audio")
            selector.register(sys.stdin.buffer, selectors.EVENT_READ, "control")
            controls = bytearray()
            ready = False
            last_audio = time.monotonic()
            while True:
                for key, _mask in selector.select(timeout=0.5):
                    if key.data == "audio":
                        pcm = os.read(capture.stdout.fileno(), 3200)
                        if not pcm:
                            raise RuntimeError("il flusso del microfono è terminato")
                        if len(pcm) % 2:
                            raise RuntimeError("campione PCM incompleto")
                        last_audio = time.monotonic()
                        if not ready:
                            if capture.poll() is not None or os.fstat(diagnostics.fileno()).st_size:
                                raise RuntimeError("apertura microfono non riuscita; controllare ALSA")
                            emit({"event": "ready"})
                            ready = True
                        session.feed(pcm)
                    else:
                        data = os.read(sys.stdin.fileno(), 4096)
                        if not data:
                            return
                        controls.extend(data)
                        if len(controls) > 8192:
                            raise RuntimeError("controllo STT troppo lungo")
                        while b"\n" in controls:
                            line, _separator, rest = controls.partition(b"\n")
                            controls = bytearray(rest)
                            request = json.loads(line)
                            if request == {"command": "quit"}:
                                return
                            if set(request) != {"command"}:
                                raise ValueError("controllo STT non valido")
                            session.control(request["command"])
                session.tick()
                size = os.fstat(diagnostics.fileno()).st_size
                if size:
                    raise RuntimeError("ALSA: " + os.pread(diagnostics.fileno(), min(size, 4096), 0).decode(errors="replace").strip())
                if time.monotonic() - last_audio > 5:
                    raise RuntimeError("nessun campione dal microfono da oltre 5 secondi")
        finally:
            if capture is not None:
                capture.terminate()
                try:
                    capture.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    capture.kill()
                    capture.wait()
                if capture.stdout:
                    capture.stdout.close()
