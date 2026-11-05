#!/usr/bin/env python3
"""Prepare a clean Raspberry checkout without installing or restarting a service.

Prerequisites: Linux ARM64, Python venv/pip, arecord, Rust/Cargo and a running
local Ollama with the reference qwen3:0.6b model. Run from a fresh clone:
python3 scripts/prepara-raspberry.py
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def run(*args):
    print('+ ' + ' '.join(map(str, args)), flush=True)
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True)


def main():
    if sys.platform != 'linux' or platform.machine() not in ('aarch64', 'arm64'):
        raise RuntimeError('Questa preparazione è verificata su Linux ARM64/Raspberry Pi.')
    if (ROOT / '.autonomous-mode').exists():
        raise RuntimeError('Usa un clone pulito: questa cartella contiene già un servizio configurato. Per aggiornare segui README/CONTRIBUTING.')
    if not shutil.which('arecord'):
        raise RuntimeError('Manca arecord: installa ALSA (alsa-utils) prima della preparazione.')
    cargo = shutil.which('cargo') or str(Path.home() / '.cargo/bin/cargo')
    if not Path(cargo).is_file():
        raise RuntimeError('Manca Cargo: installa la toolchain Rust stabile per ARM64.')
    reference = json.loads((ROOT / 'docs/ambiente-collaudato.json').read_text(encoding='utf-8'))
    with urllib.request.urlopen('http://127.0.0.1:11434/api/tags', timeout=15) as response:
        models = json.load(response)['models']
    model = next((item for item in models if item['name'] == reference['llm']['name']), None)
    if model is None:
        raise RuntimeError('Predisponi Ollama locale ed esegui ollama pull ' + reference['llm']['name'])
    if model['digest'].removeprefix('sha256:') != reference['llm']['digest']:
        raise RuntimeError('Il tag LLM non coincide con il digest collaudato: verifica il modello prima di ricostruire questo stato.')
    print('Modello LLM locale: digest collaudato verificato.', flush=True)
    python = ROOT / '.venv-voice/bin/python'
    if not python.is_file():
        run(sys.executable, '-m', 'venv', ROOT / '.venv-voice')
    run(python, '-m', 'pip', 'install', '-r', ROOT / reference['python_lock'])

    model_root = ROOT / 'models'
    target = model_root / reference['stt']['name']
    if not target.is_dir():
        model_root.mkdir(exist_ok=True)
        temporary = model_root / (reference['stt']['name'] + '.download')
        staging = model_root / '.extracting'
        if staging.exists():
            raise RuntimeError('Estrazione precedente incompleta: controlla models/.extracting prima di riprovare.')
        try:
            print('Download del modello Vosk ufficiale...', flush=True)
            with urllib.request.urlopen(reference['stt']['url'], timeout=120) as source, temporary.open('wb') as output:
                shutil.copyfileobj(source, output)
            with temporary.open('rb') as stream:
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            if digest != reference['stt']['archive_sha256']:
                raise RuntimeError('Checksum Vosk diverso da quello collaudato; archivio non estratto.')
            staging.mkdir()
            with zipfile.ZipFile(temporary) as archive:
                base = staging.resolve()
                for member in archive.infolist():
                    if not (base / member.filename).resolve().is_relative_to(base):
                        raise RuntimeError('Percorso non valido nell’archivio Vosk.')
                archive.extractall(staging)
            (staging / reference['stt']['name']).rename(target)
            staging.rmdir()
        finally:
            temporary.unlink(missing_ok=True)
    run(python, '-c', 'from vosk import Model,SetLogLevel; import sys; SetLogLevel(-1); Model(sys.argv[1]); print("Modello Vosk caricato correttamente.")', target)
    run(python, '-m', 'unittest', 'discover', '-s', 'voice', '-p', 'test_*.py', '-v')
    run(python, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_*.py', '-v')
    # Bound compiler parallelism on a 4 GB Pi while the PoC or other services run.
    os.environ.setdefault('CARGO_BUILD_JOBS', '2')
    run(cargo, 'test', '--locked')
    run(cargo, 'build', '--release', '--locked')
    print('PREPARAZIONE COMPLETATA. Nessun GPIO aperto, nessuna unit installata o riavviata.', flush=True)
    print('Per installare sulla destinazione definitiva: python3 install-autonomous-service.py --print-unit, poi --install.', flush=True)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        print('Preparazione interrotta: ' + str(error), file=sys.stderr)
        sys.exit(1)
