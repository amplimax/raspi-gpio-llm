#!/usr/bin/env python3
"""Build the standalone offline software manual using only Python's standard library.

Run from any directory: python scripts/genera-manuale.py
The HTML embeds figures, stylesheet, scripts and an exact source snapshot.
It does not contact the Raspberry or run application/hardware code.
"""
from base64 import b64encode
from argparse import ArgumentParser
from datetime import date
from hashlib import sha256
from html import escape
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
PARTS = ROOT / 'scripts/manuale'
OUT = ROOT / 'docs/manuale-software.html'


def build(edition):
    paths = sorted((ROOT / 'src').glob('*.rs'))
    paths += sorted((ROOT / 'voice').glob('*.py'))
    paths += sorted((ROOT / 'tests').glob('test_*.py'))
    paths += [ROOT / p for p in ['voice/run.sh', 'voice/requirements.txt', 'Cargo.toml', 'Cargo.lock',
                                'install-autonomous-service.py', 'scripts/service_support.py']]
    sources = {}
    fingerprint = sha256()
    for path in paths:
        name = path.relative_to(ROOT).as_posix()
        raw = path.read_bytes()
        text = raw.decode('utf-8').replace('\r\n', '\n')
        sources[name] = {'text': text, 'sha256': sha256(raw).hexdigest(), 'lines': len(text.splitlines())}
        fingerprint.update(name.encode() + b'\0' + raw + b'\0')
    sources['deploy/gpio-llm-ready.service (snapshot verificato)'] = {
        'text': (PARTS / 'servizio-verificato.txt').read_text(encoding='utf-8'),
        'sha256': sha256((PARTS / 'servizio-verificato.txt').read_bytes()).hexdigest(),
        'lines': len((PARTS / 'servizio-verificato.txt').read_text(encoding='utf-8').splitlines()),
    }
    body = (PARTS / 'capitoli.html').read_text(encoding='utf-8')
    body += (PARTS / 'moduli.html').read_text(encoding='utf-8')
    body += (PARTS / 'riferimenti.html').read_text(encoding='utf-8')
    link_count = 0

    def link(match):
        nonlocal link_count
        filename, symbol, label = match.groups()
        if filename not in sources:
            raise ValueError('Source missing: ' + filename)
        lines = sources[filename]['text'].splitlines()
        line = 1
        if symbol:
            matches = [i + 1 for i, text in enumerate(lines) if symbol in text]
            if not matches:
                raise ValueError(f'Symbol missing: {filename}: {symbol}')
            line = matches[0]
        link_count += 1
        return (f'<a class="source-link" href="#codice" data-file="{escape(filename, quote=True)}" '
                f'data-line="{line}" title="Apri {escape(filename, quote=True)}:{line}">{label}'
                f'<span aria-hidden="true"> ↗</span></a>')

    body = re.sub(r'<code-link file="([^"]+)"(?: symbol="([^"]+)")?>(.*?)</code-link>', link, body, flags=re.S)

    def figure(match):
        filename, alt, caption = match.groups()
        raw = (ROOT / 'docs' / filename).read_bytes()
        uri = 'data:image/png;base64,' + b64encode(raw).decode('ascii')
        return (f'<figure class="diagram"><button class="figure-button" type="button" '
                f'aria-label="Ingrandisci: {escape(alt, quote=True)}"><img src="{uri}" '
                f'alt="{escape(alt, quote=True)}" loading="lazy" decoding="async"></button>'
                f'<figcaption>{caption}<span>Seleziona l’immagine per ingrandirla</span></figcaption></figure>')

    body = re.sub(r'<manual-figure file="([^"]+)" alt="([^"]+)">(.*?)</manual-figure>', figure, body, flags=re.S)
    tests = []
    for filename, source in sources.items():
        if filename.endswith('.rs'):
            for match in re.finditer(r'#\s*\[test\]\s*fn\s+(\w+)', source['text']):
                tests.append((filename, match.group(1), source['text'][:match.start()].count('\n') + 1))
        elif Path(filename).name.startswith('test_') and filename.endswith('.py'):
            for match in re.finditer(r'^\s*def (test_\w+)\(', source['text'], re.M):
                tests.append((filename, match.group(1), source['text'][:match.start()].count('\n') + 1))
    rows = ''.join(f'<tr><td>{escape(f)}</td><td><a class="source-link" href="#codice" data-file="{escape(f)}" data-line="{n}">{escape(t)}</a></td></tr>' for f, t, n in tests)
    body = body.replace('@@TEST_TABLE@@', rows)
    titles = re.findall(r'<section id="([^"]+)" data-title="([^"]+)"', body)
    nav = ''.join(f'<a href="#{identifier}"><span>{i:02d}</span>{escape(title)}</a>' for i, (identifier, title) in enumerate(titles, 1))
    total_lines = sum(s['lines'] for s in sources.values())
    rust_tests = sum(1 for f, _, _ in tests if f.endswith('.rs'))
    python_tests = len(tests) - rust_tests
    replacements = {
        '@@STYLE@@': (PARTS / 'stile.css').read_text(encoding='utf-8'),
        '@@SCRIPT@@': (PARTS / 'interazioni.js').read_text(encoding='utf-8'),
        '@@BODY@@': body,
        '@@NAV@@': nav,
        '@@SOURCES@@': json.dumps(sources, ensure_ascii=False).replace('<', '\\u003c'),
        '@@SOURCE_COUNT@@': str(len(sources)),
        '@@LINE_COUNT@@': f'{total_lines:,}'.replace(',', '.'),
        '@@FINGERPRINT@@': fingerprint.hexdigest(),
        '@@RUST_TESTS@@': str(rust_tests),
        '@@PYTHON_TESTS@@': str(python_tests),
        '@@BUILD_DATE@@': edition,
    }
    html = (PARTS / 'pagina.html').read_text(encoding='utf-8')
    # BODY contains additional metadata placeholders, resolved on subsequent passes.
    for _ in range(2):
        for key, value in replacements.items():
            html = html.replace(key, value)
    unresolved = re.findall(r'@@[A-Z_]+@@', html)
    if unresolved:
        raise ValueError('Unresolved placeholders: ' + ', '.join(unresolved))
    OUT.write_text(html, encoding='utf-8', newline='\n')
    print(json.dumps({'file': str(OUT), 'bytes': OUT.stat().st_size,
                      'chapters': len(titles), 'source_files': len(sources), 'source_lines': total_lines,
                      'source_links': link_count, 'rust_tests_indexed': rust_tests,
                      'python_tests_indexed': python_tests, 'source_fingerprint': fingerprint.hexdigest()}, indent=2))


if __name__ == '__main__':
    parser = ArgumentParser(description=__doc__)
    parser.add_argument('--edition', default='2026-11-05', help='Editorial edition date, YYYY-MM-DD; stable for reproducible builds')
    args = parser.parse_args()
    build(date.fromisoformat(args.edition).isoformat())
