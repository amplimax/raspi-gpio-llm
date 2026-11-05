# Manutenzione di raspi-gpio-llm

Il repository pubblico `https://github.com/amplimax/raspi-gpio-llm.git` è la fonte
dei sorgenti e della documentazione corrente. Mantieni coerenti checkout PC,
checkout Raspberry e GitHub quando completi modifiche richieste dall'utente.

## Regole di lavoro

- Conserva la CLI testuale, l'unico backend proprietario dei GPIO e la
  separazione attuazione/stato. Non cambiare questi contratti incidentalmente.
- Prima di eliminare un file verifica se è una dipendenza attiva. Gli helper
  dell'installer sono in `scripts/service_support.py`; non ripristinare il vecchio
  installer oneshot o documenti che descrivono architetture superate.
- Per modifiche autorizzate: esegui i controlli pertinenti, aggiorna spiegazioni
  e manuale HTML, esamina il diff e mantieni GitHub aggiornato con commit chiari.
  Non includere cambiamenti estranei o riscrivere la cronologia condivisa.
- Allinea il Raspberry tramite Git senza sovrascrivere modifiche locali inattese.
  Se non è raggiungibile, dichiara l'allineamento ancora da completare; non
  affermare che una copia sia aggiornata solo perché il push è riuscito.
- Un nuovo binario o file Python sul disco non prova che il processo attivo lo
  stia usando: serve un riavvio gestito quando cambia il runtime. Non riavviare
  il servizio per sole modifiche alla documentazione o agli strumenti di sviluppo.

## Pulizia e riproducibilità

- Non conservare varianti vecchie dei sorgenti o della documentazione nelle
  cartelle di lavoro: usa la cronologia Git per recuperarle.
- Non versionare modelli, venv, target, registrazioni, log, backup o note private.
  Elimina le prove temporanee concluse. Le note di ripresa locali devono descrivere
  lo stato corrente, senza accumulare copie obsolete del manuale.
- Sul Raspberry preserva il modello estratto, il venv e il binario necessari al
  servizio. Non cancellare il lock di un processo attivo, il marker autonomo o
  il socket runtime; non usare `git clean -fdx` sulla cartella operativa.
- Per una verifica di ricostruzione usa un clone pulito e
  `python3 scripts/prepara-raspberry.py`, senza fermare l'istanza operativa.
  Installa la unit soltanto nella destinazione definitiva.
- Mantieni `Cargo.lock`, `voice/requirements-lock.txt` e
  `docs/ambiente-collaudato.json` coerenti dopo un nuovo collaudo.

## Documentazione e controlli

Il manuale offline è `docs/manuale-software.html`; i template sono in
`scripts/manuale/`. Rigenera con `python scripts/genera-manuale.py` dopo modifiche
al codice o ai testi. La data di edizione è esplicita e la build riproducibile.

Leggi `CONTRIBUTING.md` per test Rust/Python, verifica browser opzionale e CI.
I test automatici non dimostrano l'esito elettrico né l'accuratezza del parlato:
per variazioni di modelli, audio, GPIO o sequenze LED serve anche il collaudo reale.
