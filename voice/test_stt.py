"""Checks at the audio/text boundary; no microphone or Vosk model required."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import wave

spec = importlib.util.spec_from_file_location("stt", Path(__file__).with_name("stt.py"))
stt = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stt)


class AudioBoundaryTests(unittest.TestCase):
    def test_one_second_forms_and_time_articles_are_contextual(self):
        for spoken in ["un secondo", "uno secondo", "un secondi", "uno secondi", "una secondo", "una secondi", "1 secondo", "1 secondi"]:
            with self.subTest(spoken=spoken):
                normalized = stt.normalize_command("fai lampeggiare un led sul pin diciassette ogni " + spoken)
                self.assertEqual(normalized, "fai lampeggiare un led sul pin 17 ogni 1 " + spoken.split()[-1])
        self.assertEqual(stt.normalize_command("pin diciassette ogni un secondo e mezzo"), "pin 17 ogni 1.5 secondo")
        self.assertEqual(stt.normalize_command("pin diciassette ogni due secondi e mezzo"), "pin 17 ogni 2.5 secondi")
        self.assertEqual(stt.normalize_command("pin diciassette ogni un quarto di secondo"), "pin 17 ogni 0.25 secondi")
        self.assertEqual(stt.normalize_command("pin diciassette ogni uno virgola zero cinque secondi"), "pin 17 ogni 1.05 secondi")
        self.assertEqual(stt.normalize_command("pin diciassette ogni meno un secondo"), "pin 17 ogni -1 secondo")

    def test_spoken_decimals_preserve_the_fraction_and_other_words(self):
        for source, expected in [
            ("intervallo zero virgola venticinque secondi", "intervallo 0.25 secondi"),
            ("zero virgola zero cinque", "0.05"),
            ("zero virgola due cinque", "0.25"),
            ("un secondo e zero virgola cinque", "un secondo e 0.5"),
            ("uno virgola zero cinque", "1.05"),
            ("zero virgola ventitré secondi", "0.23 secondi"),
            ("zero virgola centoventicinque", "0.125"),
            ("zero virgola cento venticinque", "0.125"),
            ("zero virgola cento venti cinque", "0.125"),
            ("zero virgola uno due cinque", "0.125"),
            ("0,25 secondi", "0.25 secondi"),
            ("pin diciassette ogni due secondi", "pin diciassette ogni due secondi"),
            ("zero virgola zero venticinque", "zero virgola zero venticinque"),
            ("zero virgola cento", "0.100"),
        ]:
            with self.subTest(source=source):
                self.assertEqual(stt.normalize_decimals(source), expected)

    def test_command_numbers_include_pin_and_spoken_fractions(self):
        for source, expected in [
            ("pin diciassette ogni zero virgola cinque secondi", "pin 17 ogni 0.5 secondi"),
            ("pin diciassette ogni zero virgola venticinque secondi", "pin 17 ogni 0.25 secondi"),
            ("pin diciassette intervallo un ottavo di secondo", "pin 17 intervallo 0.125 secondi"),
            ("pin diciassette intervallo un quarto di secondo", "pin 17 intervallo 0.25 secondi"),
            ("pin diciassette intervallo mezzo secondo", "pin 17 intervallo 0.5 secondi"),
            ("pin diciassette intervallo tre quarti di secondo", "pin 17 intervallo 0.75 secondi"),
            ("pin ventisette ogni due secondi", "pin 27 ogni 2 secondi"),
        ]:
            with self.subTest(source=source):
                self.assertEqual(stt.normalize_command(source), expected)

    def test_pcm_measurements_and_silence(self):
        stats = stt.audio_stats(b"\0\0" * 16000)
        self.assertEqual(stats["audio_s"], 1.0)
        self.assertEqual(stats["clipped_pct"], 0.0)
        self.assertLess(stats["rms_dbfs"], -90)
        self.assertGreater(stt.audio_stats(b"\xff\x7f" * 16)["clipped_pct"], 99)

    def test_rejects_failed_and_truncated_recordings(self):
        for result in (
            subprocess.CompletedProcess([], 1, b"", b"device busy"),
            subprocess.CompletedProcess([], 0, b"\0\0", b""),
            subprocess.CompletedProcess([], 0, b"\0\0" * 16000, b"overrun!!!"),
        ):
            with self.subTest(result=result.returncode, diagnostic=result.stderr):
                with patch.object(stt.subprocess, "run", return_value=result):
                    with self.assertRaises(RuntimeError):
                        stt.record("plughw:CARD=Talk,DEV=0", 1)

    def test_rejects_stereo_or_wrong_sample_rate(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.wav"
            for channels, rate in [(2, 16000), (1, 48000)]:
                with wave.open(str(path), "wb") as output:
                    output.setnchannels(channels)
                    output.setsampwidth(2)
                    output.setframerate(rate)
                    output.writeframes(b"\0\0" * channels * 100)
                with self.assertRaises(ValueError):
                    stt.read_wav(path)

    def test_keeps_all_final_segments_in_order_and_ignores_partial_results(self):
        class Recognizer:
            def __init__(self, *_args):
                self.index = 0

            def AcceptWaveform(self, _data):
                self.index += 1
                return self.index == 1

            def Result(self):
                return json.dumps({"text": "fai lampeggiare il led"})

            def FinalResult(self):
                return json.dumps({"text": "sul pin diciassette ogni due secondi"})

            def PartialResult(self):
                raise AssertionError("Partial results must never be used")

        with patch.dict(sys.modules, {"vosk": types.SimpleNamespace(KaldiRecognizer=Recognizer)}):
            result = stt.transcribe(object(), b"\0\0" * 8000)
        self.assertEqual(result["text"], "fai lampeggiare il led sul pin diciassette ogni due secondi")


if __name__ == "__main__":
    unittest.main()
