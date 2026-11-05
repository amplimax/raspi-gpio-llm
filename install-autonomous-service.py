#!/usr/bin/env python3
"""Migrate the existing boot unit to standalone wake-word and CLI operation."""

import argparse
from datetime import datetime
import fcntl
import os
from pathlib import Path
import pwd
import runpy
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent
SUPPORT = runpy.run_path(str(ROOT / "scripts/service_support.py"), run_name="service_helpers")
UNIT_NAME = SUPPORT["UNIT_NAME"]
UNIT_PATH = Path("/etc/systemd/system") / UNIT_NAME


def render_unit(root, user, url, ollama_service, environment=None):
    SUPPORT["validate_unit_inputs"](root, user, url, ollama_service)
    quote = SUPPORT["unit_quote"]
    values = {"GPIO_LLM_URL": url, "RUST_LOG": "info", "GPIO_LLM_SOCKET": "/run/gpio-llm/control.sock"}
    allowed = (*SUPPORT["POLICY_KEYS"], "GPIO_VOICE_DEVICE", "GPIO_VOICE_MODEL", "GPIO_WAKE_CONFIDENCE", "GPIO_VOICE_COMMAND_TIMEOUT")
    values.update({key: value for key, value in (environment or {}).items() if key in allowed})
    return "\n".join([
        "[Unit]",
        "Description=GPIO LLM autonomo: warmup, ASCOLTA e CLI",
        "Wants=" + ollama_service,
        "After=sound.target " + ollama_service,
        "StartLimitIntervalSec=0",
        "",
        "[Service]",
        "Type=notify",
        "NotifyAccess=main",
        "User=" + user,
        "WorkingDirectory=" + str(root).replace("%", "%%"),
        "RuntimeDirectory=gpio-llm",
        "RuntimeDirectoryMode=0700",
        "UMask=0077",
        *["Environment=" + quote(key + "=" + value) for key, value in values.items()],
        "ExecStart=" + quote(str(root / "target/release/gpio-llm")) + " --daemon",
        "TimeoutStartSec=600",
        "TimeoutStopSec=15",
        "KillMode=control-group",
        "SuccessExitStatus=130",
        "Restart=on-failure",
        "RestartSec=5",
        "StandardInput=null",
        "StandardOutput=journal",
        "StandardError=journal",
        "SyslogIdentifier=gpio-llm",
        "",
        "[Install]",
        "WantedBy=multi-user.target",
        "",
    ])


def sudo(*arguments, check=True):
    return subprocess.run(["sudo", *arguments], check=check)


def test_daemon():
    """Identify only this project's temporary daemon, never an arbitrary PID."""
    try:
        pid = int((ROOT / "run/autonomous-test.pid").read_text())
        proc = Path("/proc") / str(pid)
        arguments = (proc / "cmdline").read_bytes().split(b"\0")
        environment = (proc / "environ").read_bytes().split(b"\0")
        expected_socket = ("GPIO_LLM_SOCKET=" + str(ROOT / "run/control-test.sock")).encode()
        if (proc.stat().st_uid == os.getuid()
                and arguments[0] == str(ROOT / "target/release/gpio-llm").encode()
                and b"--daemon" in arguments and expected_socket in environment):
            return pid
    except (OSError, ValueError, IndexError):
        pass
    return None


def stop_test_daemon(pid):
    print("Chiudo il backend temporaneo di collaudo prima di avviare il servizio.", flush=True)
    if test_daemon() != pid:
        raise RuntimeError("L'identità del processo di collaudo è cambiata: nessun processo fermato.")
    os.kill(pid, signal.SIGTERM)
    deadline = time.monotonic() + 10
    with (ROOT / ".gpio-llm.lock").open("a") as lock:
        while True:
            try:
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise RuntimeError("Il backend di prova non ha rilasciato i GPIO.")
                time.sleep(0.1)
    (ROOT / "run/autonomous-test.pid").unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--print-unit", action="store_true")
    action.add_argument("--install", action="store_true")
    parser.add_argument("--url", default=os.environ.get("GPIO_LLM_URL", SUPPORT["DEFAULT_URL"]))
    parser.add_argument("--ollama-service", default="ollama.service")
    args = parser.parse_args()
    if os.geteuid() == 0:
        raise RuntimeError("Eseguire come utente del progetto senza sudo davanti a python3; sarà richiesta la password solo per systemd.")
    user = pwd.getpwuid(os.getuid()).pw_name
    unit = render_unit(ROOT, user, args.url, args.ollama_service, os.environ)
    if args.print_unit:
        print(unit, end="")
        return
    binary = ROOT / "target/release/gpio-llm"
    help_text = subprocess.check_output([str(binary), "--help"], text=True)
    if "--daemon" not in help_text:
        raise RuntimeError("Compilare prima la versione con --daemon.")
    for path in [ROOT / ".venv-voice/bin/python", ROOT / "voice/autonomous.py", ROOT / "models/vosk-model-small-it-0.22"]:
        if not path.exists():
            raise RuntimeError(f"Componente vocale mancante: {path}")
    temporary_pid = test_daemon()
    if not (ROOT / ".autonomous-mode").exists():
        with (ROOT / ".gpio-llm.lock").open("a") as lock:
            try:
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                if temporary_pid is None:
                    raise RuntimeError("Chiudere l'istanza manuale prima di installare il servizio.")
    sudo("-v")
    version = SUPPORT["verify_ollama"](args.url, args.ollama_service)
    if temporary_pid is not None:
        stop_test_daemon(temporary_pid)
    print(f"Ollama {version} verificato. Installo il servizio autonomo per {user}.", flush=True)
    backup = ROOT / "backups" / ("service-before-autonomous-" + datetime.now().strftime("%Y%m%d-%H%M%S-%f"))
    backup.mkdir(parents=True)
    previous = UNIT_PATH.read_bytes() if UNIT_PATH.exists() else None
    marker = ROOT / ".autonomous-mode"
    previous_marker = marker.read_bytes() if marker.exists() else None
    enabled = subprocess.run(["systemctl", "is-enabled", UNIT_NAME], capture_output=True).returncode == 0
    active = subprocess.run(["systemctl", "is-active", UNIT_NAME], capture_output=True).returncode == 0
    previous_file = backup / UNIT_NAME
    if previous is not None:
        previous_file.write_bytes(previous)
    try:
        with tempfile.TemporaryDirectory(prefix="gpio-llm-autonomous-") as directory:
            rendered = Path(directory) / UNIT_NAME
            rendered.write_text(unit, encoding="utf-8")
            subprocess.run(["systemd-analyze", "verify", str(rendered)], check=True)
            if previous is not None:
                sudo("systemctl", "stop", UNIT_NAME)
            sudo("install", "-m", "644", str(rendered), str(UNIT_PATH))
        sudo("systemctl", "daemon-reload")
        marker.write_text("CLI collegata al servizio autonomo gpio-llm-ready.service\n", encoding="utf-8")
        sudo("systemctl", "enable", UNIT_NAME)
        print("Warmup e apertura microfono: attendere i cinque lampeggi di pronto...", flush=True)
        sudo("systemctl", "start", UNIT_NAME)
        subprocess.run(["systemctl", "is-active", "--quiet", UNIT_NAME], check=True)
    except BaseException:
        print("Installazione non riuscita: ripristino la configurazione precedente.", file=sys.stderr, flush=True)
        sudo("systemctl", "stop", UNIT_NAME, check=False)
        if previous is not None:
            sudo("install", "-m", "644", str(previous_file), str(UNIT_PATH))
        else:
            sudo("rm", "-f", str(UNIT_PATH))
        if previous_marker is None:
            marker.unlink(missing_ok=True)
        else:
            marker.write_bytes(previous_marker)
        sudo("systemctl", "daemon-reload", check=False)
        if previous is not None:
            sudo("systemctl", "enable" if enabled else "disable", UNIT_NAME, check=False)
            if active:
                sudo("systemctl", "start", UNIT_NAME, check=False)
        raise
    print("PRONTO: avvio automatico abilitato. Dire ASCOLTA, attendere il lampeggio e pronunciare il comando.")
    print("CLI invariata: ./target/release/gpio-llm ; esci chiude solo il terminale.")
    print(f"Backup della configurazione: {backup}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Errore: {error}", file=sys.stderr)
        print("Diagnostica: systemctl status gpio-llm-ready.service --no-pager; journalctl -u gpio-llm-ready.service -b --no-pager", file=sys.stderr)
        sys.exit(1)
