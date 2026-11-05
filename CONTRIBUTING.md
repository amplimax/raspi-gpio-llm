# Mantenere il progetto aggiornato

Il repository contiene il software corrente, i test, la documentazione e gli
strumenti necessari a ricostruirla. Il servizio hardware non viene aggiornato
automaticamente da un push: compilazione e riavvio sul Raspberry restano operazioni
esplicite, dopo i controlli.

## Prima di modificare

1. Aggiorna il checkout con `git pull --ff-only`.
2. Leggi il [manuale tecnico](docs/manuale-software.html) e individua il modulo
   responsabile. Non aggirare il validatore o aggiungere un secondo proprietario GPIO.
3. Conserva CLI, protocolli e pin di stato riservato, salvo un cambiamento concordato.

## Controlli

```bash
cargo test --locked
cargo build --release --locked
python3 -m unittest discover -s voice -p 'test_*.py' -v
python3 -m unittest discover -s tests -p 'test_*.py' -v
python3 scripts/genera-manuale.py
git diff --check
```

Su Windows i test puri funzionano; la verifica del renderer systemd viene saltata.
Linux CI e Raspberry eseguono anche quel caso. I test non sostituiscono la prova
del microfono, del modello e dei LED reali.

Quando cambia il codice o il testo del manuale, rigenera anche l'HTML. La data di
edizione è esplicita (`--edition YYYY-MM-DD`) e rimane stabile fino a una revisione
editoriale: la build è riproducibile e CI può verificare che il file pubblicato sia
aggiornato. Aggiorna anche le spiegazioni, non soltanto lo snapshot incorporato.

Per il controllo browser opzionale:

```bash
python3 -m pip install -r requirements-dev.txt
python3 -m playwright install chromium
python3 scripts/verifica-manuale.py
```

Screenshot temporanei e report locali non sono versionati. GitHub Actions esegue
test Rust/Python, compilazione e controllo del manuale ad ogni push su main e PR.
Dependabot propone aggiornamenti mensili; non è abilitato il merge automatico.
Una modifica di Vosk o modello va collaudata anche sull'hardware ARM64.

## Cosa includere nei commit

- Sorgenti e test attivi; Cargo.toml e Cargo.lock.
- Documentazione corrente, HTML generato, immagini e prompt necessari alla manutenzione.
- Modifiche coerenti e un messaggio che descriva il risultato.

Escludi ambienti virtuali, modelli scaricati, target di compilazione, registrazioni,
log, backup, credenziali e appunti privati. Non ripubblicare documentazione storica
che contraddice l'assetto corrente. L'installer autonomo usa gli helper attivi in
`scripts/service_support.py`; l'installer oneshot precedente è stato ritirato.

Controlla `git status` e `git diff --cached` prima del commit. Pubblica solo i file
pertinenti. Evita force push e riscritture della cronologia condivisa.
