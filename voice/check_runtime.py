"""Exercise the live Rust/Python boundary, including microphone failure."""
from pathlib import Path
import os
import signal
import subprocess
import time

root = Path(__file__).resolve().parent.parent
binary = str(root / 'target/release/gpio-llm')
env = dict(os.environ, GPIO_LLM_URL='http://127.0.0.1:11434/v1/chat/completions')

def worker_pids():
    active = subprocess.run(['pgrep', '-f', str(root / 'voice/stt.py')], text=True, capture_output=True)
    assert active.returncode in (0, 1), active.stderr
    return set(active.stdout.split())

existing_workers = worker_pids()

def run_case(name, commands, extra_env=None):
    result = subprocess.run([binary, '--voice'], input=commands, text=True, capture_output=True,
                            env=dict(env, **(extra_env or {})), timeout=30)
    print(name, 'exit=', result.returncode)
    print(result.stdout)
    print(result.stderr)
    assert result.returncode == 0, name
    assert 'Voce italiana pronta' in result.stdout, name
    assert 'LLM ->' not in result.stderr, 'Unexpected LLM call'
    return result

run_case('Startup e uscita normale', 'esci\n')
result = run_case('Microfono non disponibile: nessun comando eseguito', '\nesci\n',
                  {'GPIO_VOICE_DEVICE': 'hw:CARD=DoesNotExist,DEV=0'})
assert 'Registrazione rifiutata' in result.stderr
assert 'Trascrizione:' not in result.stdout

# While the worker loads/waits, SIGTERM must stop both the main program and STT.
process = subprocess.Popen([binary, '--voice'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, text=True, env=env)
try:
    time.sleep(2)
    process.send_signal(signal.SIGTERM)
    stdout, stderr = process.communicate(timeout=10)
    print('SIGTERM durante la sessione vocale: exit=', process.returncode)
    print(stdout, stderr)
    assert process.returncode == 130
finally:
    if process.poll() is None:
        process.kill()
        process.wait()

new_workers = worker_pids() - existing_workers
assert not new_workers, 'Worker STT del test rimasto attivo: ' + ', '.join(new_workers)
print('OK: avvio, uscita, errore audio e terminazione verificati; nessun worker residuo.')
