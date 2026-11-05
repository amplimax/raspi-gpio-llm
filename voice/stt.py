#!/usr/bin/env python3
"""Local Italian speech input; only finalized text leaves this process."""

import argparse
from array import array
import json
import math
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time
import wave

ROOT = Path(__file__).resolve().parent.parent
SAMPLE_RATE = 16000


def italian_numbers():
    names = "zero uno due tre quattro cinque sei sette otto nove dieci undici dodici tredici quattordici quindici sedici diciassette diciotto diciannove".split()
    values = {word: value for value, word in enumerate(names)}
    for tens, name in enumerate(["venti", "trenta", "quaranta", "cinquanta", "sessanta", "settanta", "ottanta", "novanta"], start=2):
        values[name] = tens * 10
        for unit in range(1, 10):
            prefix = name[:-1] if unit in (1, 8) else name
            word = prefix + names[unit]
            values[word] = tens * 10 + unit
            if unit == 3:
                values[word[:-1] + "é"] = tens * 10 + unit
    small = list(values.items())
    for hundred in range(1, 10):
        prefix = "cento" if hundred == 1 else names[hundred] + "cento"
        values[prefix] = hundred * 100
        for word, value in small:
            if value:
                stem = prefix[:-1] if value == 8 or 80 <= value <= 89 else prefix
                values[stem + word] = hundred * 100 + value
    return values


NUMBER_WORDS = italian_numbers()
NUMBER_TOKEN = "(?:" + "|".join(sorted(NUMBER_WORDS, key=len, reverse=True)) + r"|\d+)"
WORD_NUMBER = re.compile(r"(?<!\w)(" + "|".join(sorted(NUMBER_WORDS, key=len, reverse=True)) + r")(?!\w)", re.IGNORECASE)
SPOKEN_DECIMAL = re.compile(
    rf"(?<!\w)({NUMBER_TOKEN})\s+virgola\s+({NUMBER_TOKEN}(?:\s+{NUMBER_TOKEN})*)(?!\w)",
    re.IGNORECASE,
)


def cardinal_value(values):
    if len(values) == 1:
        return values[0]
    if len(values) == 2 and values[0] in range(20, 100, 10) and 1 <= values[1] <= 9:
        return sum(values)
    if values and values[0] in range(100, 1000, 100):
        tail = cardinal_value(values[1:])
        if tail is not None and 0 < tail < 100:
            return values[0] + tail
    return None


def normalize_decimals(text):
    """Preserve spoken decimal digits before handing text to the small LLM."""
    def replace(match):
        whole_word, fractional_words = match.groups()
        whole = str(NUMBER_WORDS[whole_word.lower()]) if not whole_word.isdigit() else whole_word
        fractions = fractional_words.lower().split()
        digits = [str(NUMBER_WORDS[word]) if not word.isdigit() else word for word in fractions]
        if len(digits) == 1 or all(len(digit) == 1 for digit in digits):
            fractional = "".join(digits)
        else:
            # Vosk may write "cento venticinque" or "centoventicinque".
            value = cardinal_value([int(digit) for digit in digits])
            if value is None:
                return match.group(0)
            fractional = str(value)
        return whole + "." + fractional

    text = SPOKEN_DECIMAL.sub(replace, text)
    return re.sub(r"(?<!\w)(\d+),(\d+)(?!\w)", r"\1.\2", text)


FRACTION_DENOMINATORS = {
    "mezzo": 2, "quarto": 4, "quarti": 4, "ottavo": 8, "ottavi": 8,
    "decimo": 10, "decimi": 10, "centesimo": 100, "centesimi": 100,
    "millesimo": 1000, "millesimi": 1000,
}
SPOKEN_FRACTION = re.compile(
    rf"(?<!\w)(?:({NUMBER_TOKEN}|un|una)\s+)?({'|'.join(FRACTION_DENOMINATORS)})\s+(?:di\s+)?second[oi]\b",
    re.IGNORECASE,
)


def normalize_command(text):
    """Expose literal source numbers to Rust instead of letting the LLM guess."""
    def fraction(match):
        numerator, name = match.groups()
        numerator = (numerator or "un").lower()
        value = 1 if numerator in ("un", "una") else (int(numerator) if numerator.isdigit() else NUMBER_WORDS[numerator])
        seconds = value / FRACTION_DENOMINATORS[name.lower()]
        return format(seconds, ".12g") + " secondi"

    text = SPOKEN_FRACTION.sub(fraction, text)
    text = normalize_decimals(text)
    text = WORD_NUMBER.sub(lambda match: str(NUMBER_WORDS[match.group(0).lower()]), text)
    # "un" is a number before a time unit, but remains an article in "un LED".
    text = re.sub(r"\b(?:un|una)\s+((?:milli)?second[oi])\b", r"1 \1", text, flags=re.IGNORECASE)
    text = re.sub(
        r"(?<!\w)(\d+(?:\.\d+)?)\s+((?:milli)?second[oi])\s+e\s+(?:un\s+)?mezzo\b",
        lambda match: format(float(match.group(1)) + 0.5, ".12g") + " " + match.group(2),
        text, flags=re.IGNORECASE,
    )
    text = re.sub(r"\bmeno\s+(\d+(?:\.\d+)?)", r"-\1", text, flags=re.IGNORECASE)
    return re.sub(r"(\d)(?=(?:milli)?second[oi]\b)", r"\1 ", text, flags=re.IGNORECASE)


def audio_stats(pcm):
    if not pcm or len(pcm) % 2:
        raise ValueError("Registrazione vuota o PCM 16 bit incompleto")
    samples = array("h", pcm)
    if sys.byteorder != "little":
        samples.byteswap()
    peak = max(abs(value) for value in samples)
    rms = math.sqrt(sum(value * value for value in samples) / len(samples))
    return {
        "audio_s": len(samples) / SAMPLE_RATE,
        "peak_dbfs": round(20 * math.log10(max(peak, 1) / 32768), 1),
        "rms_dbfs": round(20 * math.log10(max(rms, 1) / 32768), 1),
        "clipped_pct": round(100 * sum(abs(value) >= 32767 for value in samples) / len(samples), 3),
    }


def read_wav(path):
    with wave.open(str(path), "rb") as source:
        if (source.getnchannels(), source.getsampwidth(), source.getframerate(), source.getcomptype()) != (1, 2, SAMPLE_RATE, "NONE"):
            raise ValueError("Serve un WAV PCM mono, 16 bit, 16000 Hz")
        return source.readframes(source.getnframes())


def save_wav(path, pcm):
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        output.writeframes(pcm)


def record(device, seconds):
    print(f"PARLA ORA: registrazione di {seconds} secondi...", file=sys.stderr, flush=True)
    result = subprocess.run(
        ["arecord", "-q", "-D", device, "-t", "raw", "-f", "S16_LE", "-r", str(SAMPLE_RATE), "-c", "1", "-d", str(seconds)],
        capture_output=True,
        timeout=seconds + 10,
        check=False,
    )
    if result.returncode:
        raise RuntimeError("Microfono: " + result.stderr.decode(errors="replace").strip())
    if result.stderr.strip():
        # Do not interpret a recording that ALSA reports as corrupted/overrun.
        raise RuntimeError("Acquisizione audio: " + result.stderr.decode(errors="replace").strip())
    expected = seconds * SAMPLE_RATE * 2
    if len(result.stdout) != expected:
        raise RuntimeError(f"Registrazione incompleta: {len(result.stdout)} byte, attesi {expected}")
    print("Registrazione terminata; trascrizione in corso...", file=sys.stderr, flush=True)
    return result.stdout


def transcribe(model, pcm):
    from vosk import KaldiRecognizer

    started = time.monotonic()
    recognizer = KaldiRecognizer(model, SAMPLE_RATE)
    parts = []
    for offset in range(0, len(pcm), 8000):
        if recognizer.AcceptWaveform(pcm[offset:offset + 8000]):
            parts.append(json.loads(recognizer.Result()).get("text", ""))
    parts.append(json.loads(recognizer.FinalResult()).get("text", ""))
    text = " ".join(part for part in parts if part).strip()
    return {
        "text": text,
        "llm_text": normalize_command(text),
        "stt_s": round(time.monotonic() - started, 3),
        **audio_stats(pcm),
    }


def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=Path(os.environ.get("GPIO_VOICE_MODEL", ROOT / "models/vosk-model-small-it-0.22")))
    parser.add_argument("--device", default=os.environ.get("GPIO_VOICE_DEVICE", "plughw:CARD=Talk,DEV=0"))
    parser.add_argument("--seconds", type=int, default=int(os.environ.get("GPIO_VOICE_SECONDS", "6")))
    parser.add_argument("--wav", type=Path, help="Trascrive un WAV esistente senza usare il microfono")
    parser.add_argument("--save", type=Path, help="Salva il WAV della prova e il risultato JSON accanto ad esso")
    parser.add_argument("--worker", action="store_true", help="Protocollo JSON a righe per il processo Rust")
    parser.add_argument("--autonomous", action="store_true", help="Ascolto continuo della parola ASCOLTA")
    args = parser.parse_args()
    if not 1 <= args.seconds <= 30:
        parser.error("--seconds deve essere tra 1 e 30")
    if args.worker and (args.wav or args.save):
        parser.error("--worker non ammette --wav o --save")
    if args.autonomous and (args.worker or args.wav or args.save):
        parser.error("--autonomous non ammette --worker, --wav o --save")

    from vosk import Model, SetLogLevel

    SetLogLevel(-1)
    if not args.model.is_dir():
        raise FileNotFoundError(f"Modello italiano assente: {args.model}")
    print("Caricamento modello vocale italiano...", file=sys.stderr, flush=True)
    started = time.monotonic()
    model = Model(str(args.model))
    # During native model construction, keep normal OS termination semantics:
    # Vosk cannot safely free a partially constructed Python Model object.
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    print(f"STT pronto in {time.monotonic() - started:.2f} s.", file=sys.stderr, flush=True)

    if args.autonomous:
        from autonomous import run
        run(model, args.device)
        return

    if args.worker:
        emit({"ready": True})
        for line in sys.stdin:
            try:
                request = json.loads(line)
                if request == {"command": "quit"}:
                    return
                if request != {"command": "listen"}:
                    raise ValueError("Richiesta di ascolto non valida")
                emit(transcribe(model, record(args.device, args.seconds)))
            except (ValueError, OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                emit({"error": str(error)})
        return

    pcm = read_wav(args.wav) if args.wav else record(args.device, args.seconds)
    if args.save:
        save_wav(args.save, pcm)
    result = transcribe(model, pcm)
    if args.save:
        args.save.with_suffix(".json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    emit(result)


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
    except (ValueError, OSError, RuntimeError, subprocess.TimeoutExpired, ImportError) as error:
        print(f"Errore STT: {error}", file=sys.stderr)
        sys.exit(1)
