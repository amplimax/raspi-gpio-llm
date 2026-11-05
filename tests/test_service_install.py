"""Contracts of the installer; no sudo, model, microphone or GPIO access."""
import os
from pathlib import Path
import runpy
import unittest

ROOT = Path(__file__).resolve().parent.parent
SUPPORT = runpy.run_path(str(ROOT / 'scripts/service_support.py'))


class ServiceInstallTests(unittest.TestCase):
    def test_unit_values_escape_specifiers_and_reject_line_injection(self):
        quote = SUPPORT['unit_quote']
        self.assertEqual(quote('a%name"\\b'), '"a%%name\\"\\\\b"')
        for bad in ['device\nExecStart=other', 'device\rmore', 'device\0more']:
            with self.subTest(bad=repr(bad)), self.assertRaises(ValueError):
                quote(bad)

    def test_installer_only_accepts_the_local_chat_endpoint(self):
        parse = SUPPORT['parse_local_url']
        self.assertEqual(parse(SUPPORT['DEFAULT_URL'])[1], 11434)
        for url in ['https://localhost/v1/chat/completions', 'http://example.com/v1/chat/completions',
                    'http://user:secret@localhost/v1/chat/completions', 'http://localhost/api/chat',
                    'http://localhost/v1/chat/completions?extra=1']:
            with self.subTest(url=url), self.assertRaises(ValueError):
                parse(url)

    def test_unit_context_rejects_relative_paths_and_invalid_names(self):
        validate = SUPPORT['validate_unit_inputs']
        good = (ROOT, 'operator', SUPPORT['DEFAULT_URL'], 'ollama.service')
        validate(*good)
        for path, user, service in [(Path('relative'), 'operator', 'ollama.service'),
                                    (ROOT, 'user\nOther', 'ollama.service'),
                                    (ROOT, 'operator', 'ollama.service\nOther')]:
            with self.subTest(path=str(path), user=user, service=service), self.assertRaises(ValueError):
                validate(path, user, good[2], service)

    @unittest.skipUnless(os.name == 'posix', 'installer renderer requires Linux/Unix')
    def test_autonomous_unit_remains_identical_to_the_verified_service(self):
        installer = runpy.run_path(str(ROOT / 'install-autonomous-service.py'))
        actual = installer['render_unit'](Path('/home/max/gpio-llm'), 'max', SUPPORT['DEFAULT_URL'], 'ollama.service')
        expected = '\n'.join(line for line in (ROOT / 'scripts/manuale/servizio-verificato.txt')
                             .read_text(encoding='utf-8').splitlines() if not line.startswith('#'))
        self.assertEqual(actual.strip(), expected.strip())


if __name__ == '__main__':
    unittest.main()
