# Ricostruire la PoC dal repository

Git contiene i sorgenti attivi, i lock delle dipendenze, l'installer, la
documentazione e le immagini. Non contiene il sistema operativo, i pacchetti di
sistema, i pesi dei modelli o dati locali. Questa separazione è intenzionale:
questi elementi sono ricreati o scaricati, non copiati da una vecchia cartella.

## 1. Prerequisiti della macchina

Raspberry Pi 4 con Linux ARM64; ambiente verificato in
[ambiente-collaudato.json](ambiente-collaudato.json). Predisponi:

- Python 3.13 con venv/pip, Git, arecord/ALSA e una toolchain Rust/Cargo stabile.
- Ollama locale come servizio `ollama.service` su `127.0.0.1:11434`.
- Modello `qwen3:0.6b`, con il digest della versione collaudata.
- Accesso dell'utente ad audio e GPIO; microfono e due LED cablati come nella guida.

Su Debian gli strumenti audio e venv provengono dai pacchetti `alsa-utils` e
`python3-venv`. Rust e Ollama sono prerequisiti esterni: i riferimenti per
l'installazione sono [Rust](https://www.rust-lang.org/tools/install) e
[Ollama Linux](https://docs.ollama.com/linux). La preparazione non esegue script
di installazione di sistema in modo nascosto.

```bash
ollama pull qwen3:0.6b
git clone https://github.com/amplimax/raspi-gpio-llm.git
cd raspi-gpio-llm
python3 scripts/prepara-raspberry.py
```

Lo script è destinato a un clone pulito e rifiuta una cartella con un servizio
già configurato. Verifica il digest LLM senza modificarlo, crea il venv da
`voice/requirements-lock.txt`, scarica Vosk dal sito ufficiale e controlla
SHA-256 prima dell'estrazione, carica il modello nativo, esegue i 15 test Python
e i 40 Rust e compila il binario release. Non apre GPIO/microfono e non installa
né riavvia servizi. L'archivio Vosk temporaneo viene rimosso dopo l'estrazione.

I tag remoti possono cambiare: se il digest LLM non coincide, lo script si ferma
anziché dichiarare identico un modello diverso. Il file di riferimento riporta
il digest esatto del collaudo. Conservare una copia dei pesi fuori da Git è una
possibile strategia di backup; il solo repository non rende disponibili file
che un fornitore dovesse rimuovere in futuro.

## 2. Installare nella directory definitiva

Compila nella cartella da cui il servizio funzionerà: Rust incorpora alcuni
percorsi del progetto alla compilazione. Non spostare poi il solo binario.

```bash
arecord -l
python3 install-autonomous-service.py --print-unit
python3 install-autonomous-service.py --install
systemctl status gpio-llm-ready.service --no-pager
journalctl -u gpio-llm-ready.service -b -n 60 --no-pager
```

Se il dispositivo non è `plughw:CARD=Talk,DEV=0`, imposta `GPIO_VOICE_DEVICE`
prima dell'installer. L'installer usa l'utente e la directory effettivi; non
richiede che abbiano i nomi del vecchio collaudo. Sudo viene richiesto nel terminale.

## 3. Collaudare

Verifica pronto al boot, ASCOLTA, luce fissa durante cattura, cinque impulsi
prima di on/off/blink e due impulsi per un rifiuto. Prova 0.5, 0.25, 0.125, un
secondo e un secondo e mezzo, confrontando i log. La CLI deve continuare a
funzionare e `esci` deve chiudere soltanto il client.

## 4. Aggiornamenti successivi e pulizia

Per la cartella già operativa usa `git pull --ff-only`, test/build e restart
come nel README, non lo script per clone pulito. Aggiorna lock, documentazione,
HTML e file di ambiente quando cambia lo stato collaudato. Le copie PC e
Raspberry devono puntare allo stesso commit prima di dichiararle allineate.

Le versioni superate vanno recuperate dalla cronologia Git, non mantenute come
copie `old`, `v2`, `backup` accanto ai sorgenti correnti. Rimuovi registrazioni,
log di prove concluse e cartelle di verifica temporanee. Conserva invece venv,
modello Vosk estratto e binario release richiesti dall'istanza operativa.
Non eliminare il lock di un processo attivo o il marker `.autonomous-mode`.
Non usare `git clean -fdx` sulla cartella operativa: cancellerebbe anche risorse
ignorate ma indispensabili al servizio.
