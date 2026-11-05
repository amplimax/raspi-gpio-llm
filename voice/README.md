# Voce: uso e diagnostica

Per l'uso quotidiano è attivo il [servizio autonomo con ASCOLTA](../docs/ascolto-autonomo.md).
Per capire protocolli e sorgenti consulta il [manuale tecnico offline](../docs/manuale-software.html).

## Percorso attuale

Rust avvia `.venv-voice/bin/python -u voice/stt.py --autonomous`. Python carica
Vosk una volta e delega ad `autonomous.py`: microfono aperto, parola ASCOLTA,
sessione per un comando completo, esito e riarmo. Durante elaborazione ed esito
l'audio è drenato e ignorato.

L'acquisizione richiede PCM mono 16 bit a 16 kHz tramite ALSA. Il nome del
dispositivo predefinito è `plughw:CARD=Talk,DEV=0`; adatta la configurazione
alla scheda elencata da `arecord -l`. I modelli sono file separati in `models/`,
non vengono installati automaticamente dal pacchetto pip.

| Variabile | Default | Uso |
|---|---|---|
| `GPIO_VOICE_DEVICE` | `plughw:CARD=Talk,DEV=0` | Dispositivo audio |
| `GPIO_VOICE_MODEL` | `models/vosk-model-small-it-0.22` nel progetto | Modello Vosk |
| `GPIO_WAKE_CONFIDENCE` | `0.85` | Soglia della parola ASCOLTA |
| `GPIO_VOICE_COMMAND_TIMEOUT` | `12` s | Limite della frase autonoma |
| `GPIO_VOICE_SECONDS` | `6` s | Durata della prova manuale, non dell'autonomo |
| `GPIO_VOICE_HOME` | Root del progetto alla compilazione | Percorso cercato da Rust |

L'installer persiste le variabili autonome supportate. Cambiare una variabile
in una shell non modifica l'ambiente del servizio già in esecuzione.

## Prove manuali, a servizio autonomo fermo

Queste operazioni interrompono l'uso autonomo e le azioni attive. Arresta il
servizio prima di aprire un altro registratore o un altro controllore GPIO:

```bash
sudo systemctl stop gpio-llm-ready.service
sh voice/run.sh
```

Premi Invio e parla dopo PARLA ORA. La registrazione dura sei secondi di default;
il modello resta caricato fra le prove. Scrivi `esci` per chiudere. Il launcher
usa Ollama su localhost:11434 se non è impostato un altro endpoint.

Per isolare cattura e trascrizione senza chiamare LLM o GPIO:

```bash
.venv-voice/bin/python voice/stt.py --seconds 8 --save recordings/prova.wav
.venv-voice/bin/python voice/stt.py --wav recordings/prova.wav
```

`--save` crea WAV e JSON diagnostici; il funzionamento autonomo normale mantiene
solo la sessione in memoria. Il WAV deve essere PCM mono 16 bit, 16000 Hz.
Una registrazione incompleta o con diagnostiche ALSA viene rifiutata.

Puoi verificare anche il confine Rust/Python in modalità manuale con
`.venv-voice/bin/python voice/check_runtime.py`: richiede Linux e il binario,
esercita avvio, microfono assente e SIGTERM, senza inviare comandi all'LLM.

Al termine delle prove ripristina l'uso autonomo:

```bash
sudo systemctl start gpio-llm-ready.service
```

## Test senza microfono

```bash
python3 -m unittest discover -s voice -p 'test_*.py' -v
```

I test coprono normalizzazione, errori di cattura, risultati finali e macchina
a stati con riconoscitore finto. L'accuratezza acustica va verificata sulla scheda.
