"""Shared, standard-library helpers for the autonomous systemd installer.

This module validates configuration and inspects the existing local Ollama
service. It neither installs a unit nor starts/stops the application on import.
"""

import json
from pathlib import Path
import re
import subprocess
import urllib.request
from urllib.parse import urlsplit

UNIT_NAME = "gpio-llm-ready.service"
DEFAULT_URL = "http://127.0.0.1:11434/v1/chat/completions"
POLICY_KEYS = ("GPIO_ALLOWED_PINS", "GPIO_MIN_INTERVAL_S", "GPIO_MAX_INTERVAL_S")


def unit_quote(value):
    if any(char in value for char in "\n\r\0"):
        raise ValueError("un valore della configurazione contiene caratteri non ammessi")
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%") + '"'


def parse_local_url(url):
    parsed = urlsplit(url)
    if (parsed.scheme != "http" or parsed.hostname not in ("localhost", "127.0.0.1")
            or parsed.username or parsed.password or parsed.query or parsed.fragment
            or parsed.path.rstrip("/") != "/v1/chat/completions"):
        raise ValueError("questo installer richiede un endpoint Ollama HTTP locale /v1/chat/completions")
    return parsed, parsed.port or 80


def validate_unit_inputs(project_dir, user, url, ollama_service):
    parse_local_url(url)
    if not project_dir.is_absolute():
        raise ValueError("la directory del progetto deve essere assoluta")
    unit_quote(str(project_dir))
    if not re.fullmatch(r"[A-Za-z0-9_.@:-]+\.service", ollama_service):
        raise ValueError("nome del servizio Ollama non valido")
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.-]*\$?", user):
        raise ValueError("nome utente non supportato dall'installer")


def run_capture(command):
    return subprocess.run(command, check=True, text=True, capture_output=True).stdout.strip()


def parent_pid(pid):
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return int(fields[1])
    except (OSError, ValueError, IndexError):
        return 0


def belongs_to_service(pid, main_pid):
    visited = set()
    while pid > 0 and pid not in visited:
        if pid == main_pid:
            return True
        visited.add(pid)
        pid = parent_pid(pid)
    return False


def verify_ollama(url, service):
    parsed, port = parse_local_url(url)
    run_capture(["systemctl", "cat", service])
    main_pid = int(run_capture(["systemctl", "show", service, "--property=MainPID", "--value"]) or "0")
    if main_pid <= 0:
        raise RuntimeError(f"{service} non e' attivo. Verificare quale servizio gestisce l'istanza Ollama funzionante.")
    listeners = run_capture(["sudo", "ss", "-H", "-ltnp", f"sport = :{port}"])
    pids = {int(value) for value in re.findall(r"pid=(\d+)", listeners)}
    if not any(belongs_to_service(pid, main_pid) for pid in pids):
        raise RuntimeError(
            f"La porta {port} non risulta gestita da {service} (MainPID {main_pid}). "
            "Non installo dipendenze verso un'istanza diversa. "
            "Controllare systemctl status ollama e sudo ss -ltnp; "
            "per un servizio personalizzato usare --ollama-service NOME.service."
        )
    with urllib.request.urlopen(f"{parsed.scheme}://{parsed.netloc}/api/version", timeout=5) as response:
        return json.load(response).get("version", "non indicata")
