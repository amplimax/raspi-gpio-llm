"""Wake-word gating, acknowledgements and truncated-command rejection."""

import unittest

from autonomous import WakeSession, is_wake


def wake_result(confidence=0.99):
    return {"text": "ascolta", "result": [{"word": "ascolta", "conf": confidence}]}


class FakeRecognizer:
    def __init__(self, *_args):
        self.results = []

    def SetWords(self, _value):
        pass

    def Reset(self):
        self.results.clear()

    def AcceptWaveform(self, _pcm):
        return bool(self.results)

    def Result(self):
        import json
        return json.dumps(self.results.pop(0))

    def FinalResult(self):
        raise AssertionError("A partial command must not be finalized at the timeout")


class AutonomousTests(unittest.TestCase):
    def make_session(self):
        events = []
        session = WakeSession(object(), output=events.append, recognizer=FakeRecognizer, timeout=3)
        return session, events

    def test_wake_requires_the_exact_word_and_sufficient_confidence(self):
        self.assertTrue(is_wake(wake_result(), 0.85))
        self.assertFalse(is_wake(wake_result(0.4), 0.85))
        self.assertFalse(is_wake({"text": "ascolta"}, 0.85))
        self.assertFalse(is_wake({"text": "non ascolta", "result": [{"word": "ascolta", "conf": 1}]}, 0.85))
        self.assertFalse(is_wake({"text": "ascolta", "result": [{"word": "[unk]", "conf": 1}]}, 0.85))

    def test_no_command_can_escape_before_wake_and_led_acknowledgement(self):
        session, events = self.make_session()
        with self.assertRaises(ValueError):
            session.control("listen", now=0)
        session.control("arm", now=0)
        session.wake.results.append(wake_result())
        session.feed(b"\0\0" * 1600, now=0.1)
        self.assertEqual(events[-1]["event"], "wake")
        self.assertEqual(session.state, "waiting")
        session.feed(b"\0\0" * 1600, now=0.2)
        self.assertEqual([event["event"] for event in events], ["armed", "wake"])

        session.control("listen", now=1)
        session.command.results.append({"text": "lampeggia pin diciassette ogni zero virgola venticinque secondi"})
        session.feed(b"\0\0" * 1600, now=1.1)
        transcript = events[-1]
        self.assertEqual(transcript["event"], "transcript")
        self.assertIn("0.25", transcript["llm_text"])
        self.assertEqual(session.state, "busy")
        count = len(events)
        session.feed(b"\0\0" * 1600, now=1.2)
        self.assertEqual(len(events), count)
        session.control("arm", now=2)
        self.assertEqual(session.state, "wake")

    def test_silence_and_partial_speech_expire_without_a_command(self):
        session, events = self.make_session()
        session.control("arm", now=0)
        session.wake.results.append(wake_result())
        session.feed(b"\0\0" * 1600, now=0)
        session.control("listen", now=1)
        session.feed(b"\0\0" * 1600, now=1.5)
        session.tick(now=4.1)
        session.tick(now=5)
        self.assertEqual(events[-1]["event"], "timeout")
        self.assertEqual(sum(event["event"] == "timeout" for event in events), 1)
        self.assertFalse(any(event["event"] == "transcript" for event in events))

    def test_non_wake_speech_does_not_enter_command_mode(self):
        session, events = self.make_session()
        session.control("arm", now=0)
        for result in [{"text": "[unk]"}, wake_result(0.2), {"text": "accendi led"}]:
            session.wake.results.append(result)
            session.feed(b"\0\0" * 1600, now=1)
        self.assertEqual(session.state, "wake")
        self.assertEqual([event["event"] for event in events], ["armed"])


if __name__ == "__main__":
    unittest.main()
