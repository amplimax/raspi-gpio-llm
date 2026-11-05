# Funzionamento autonomo con ASCOLTA

Per studiare software, moduli e codice da zero: [manuale tecnico HTML offline](manuale-software.html).

Schema da presentazione dell'architettura corrente (PNG 16:9, verificato il
5 novembre 2026): [Dalla voce al GPIO](architettura-poc-attuale.png).
Mostra Vosk `vosk-model-small-it-0.22`, Ollama con `qwen3:0.6b`, il backend
Rust e i due LED separati per attuazione e stato.
È disponibile anche la [variante per presentazione](architettura-poc-attuale-presentazione.png)
con lo stesso schema, senza marca del microfono, numeri dei pin e porta Ollama.

Il Raspberry avvia un unico backend Rust al boot. Il backend verifica il
modello, carica Vosk, apre il microfono e conferma il pronto con cinque
lampeggi rapidi sul LED di stato BCM23. Poi riceve sia comandi dalla CLI sia eventi vocali.

## Uso quotidiano

Per la voce non servono tastiera, SSH o terminale:

1. Dire **ASCOLTA** da sola e fare una breve pausa.
2. Attendere che il LED di stato si accenda fisso: la finestra di ascolto è aperta.
3. Pronunciare il comando, per esempio «Accendi il LED sul pin diciassette».
4. Fare una pausa: Vosk riconosce la fine della frase e il LED di stato si
   spegne. Il comando viene interpretato e validato; cinque lampeggi rapidi
   confermano che è valido. **L'attuazione comincia soltanto dopo l'ultimo
   lampeggio.** Due lampeggi segnalano un comando rifiutato o un errore.
5. Il sistema torna ad attendere ASCOLTA.

La CLI conserva avvio, banner, prompt e messaggi:

```bash
./target/release/gpio-llm
```

```text
Scrivi un comando ("esci" per terminare).
> Accendi il GPIO 17
OK: GPIO 17: acceso
> esci
```

`esci` chiude quella CLI; il microfono continua a funzionare. Più client
possono collegarsi, ma le azioni vengono eseguite in ordine da un solo backend.
La CLI non apre direttamente i GPIO quando è abilitato il servizio autonomo.

## Segnalazione del LED

Il LED di attuazione rimane su **BCM17, pin fisico 11**. Il secondo LED,
preferibilmente verde, è dedicato a stato e notifiche su **BCM23, pin fisico 16**.

Con porte USB/Ethernet a destra e USB-C sotto, contare da sinistra su ciascuna fila:

| Fila del connettore | Contatto nella fila | Pin fisico | Collegamento |
|---|---:|---:|---|
| Inferiore, verso il centro della scheda | 5° | 9 | GND del LED attuatore esistente |
| Inferiore, verso il centro della scheda | 6° | 11 | BCM17 → resistenza → anodo LED attuatore |
| Superiore, verso il bordo della scheda | **7°** | **14** | **GND → catodo LED di stato** |
| Superiore, verso il bordo della scheda | **8°** | **16** | **BCM23 → resistenza 330 Ω → anodo LED di stato** |

Ogni LED ha la propria resistenza. L'anodo è normalmente il terminale lungo;
il catodo è normalmente quello corto, sul lato piatto della capsula. Collegare
a Raspberry spento. [Pinout ufficiale Raspberry Pi](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#pin-layout).

Il nuovo LED di stato produce:

| Circostanza | Segnale |
|---|---|
| Warmup e microfono pronti | Cinque impulsi di 100 ms acceso + 100 ms spento |
| ASCOLTA riconosciuto, cattura del comando aperta | Acceso fisso per tutta la finestra di ascolto |
| Fine frase o fine della finestra | Spento; resta spento durante l'elaborazione |
| Comando vocale riconosciuto e validato | Cinque impulsi rapidi, poi attuazione e LED di stato spento |
| Comando non riconosciuto, rifiutato, fallito o scaduto | Due impulsi rapidi, poi spento |

Ogni sequenza di impulsi è preceduta da 200 ms spento, così i lampeggi si
distinguono dalla precedente luce fissa anche quando l'elaborazione è
istantanea. All'interno di ogni sequenza restano 100 ms acceso + 100 ms spento.
Accensione e spegnimento della luce fissa seguono gli eventi effettivi del
processo audio prima della coda di esecuzione: il LED si spegne a fine ascolto
anche se il backend sta ancora elaborando una richiesta CLI.

Le notifiche usano esclusivamente BCM23: il LED attuatore su BCM17 non viene
spento, sospeso o riavviato dai segnali. Il suo eventuale lampeggio prosegue
indipendentemente. Gli impulsi di stato sono generati da Rust e terminano
sempre a livello basso. BCM23 è riservato e non accetta comandi CLI o vocali,
neppure aggiungendolo a una policy personalizzata.

Il self-test del modello continua a verificare comandi su BCM17 senza
eseguirli. Il segnale fisico di pronto è invece prodotto sul LED dedicato BCM23.

Una frase vuota, incompleta, non valida o scaduta riceve i due lampeggi di
errore. La sequenza è interpretazione → controllo dei numeri → validazione →
cinque lampeggi completi → GPIO. Se l'esecutore fallisce dopo la conferma del
comando valido, vengono emessi anche i due lampeggi di errore. Dopo un timeout
serve pronunciare di nuovo ASCOLTA.

## Installazione del servizio

Dopo compilazione e collaudo, come utente `max` sul Raspberry:

```bash
cd /home/max/gpio-llm
python3 install-autonomous-service.py --print-unit
python3 install-autonomous-service.py --install
```

Lo script chiede la password sudo nel terminale del Raspberry per aggiornare
systemd. Riutilizza **gpio-llm-ready.service**, sostituendo il vecchio oneshot
con il backend persistente. Conserva una copia dell'unità precedente e la
ripristina se l'installazione o l'avvio falliscono. Chiudere prima eventuali
istanze manuali che controllano microfono/GPIO. L'istanza temporanea di collaudo
predisposta dall'agente viene identificata e fermata dall'installer dopo
l'autenticazione sudo.

La nuova unità è `Type=notify`: systemd riceve READY solo dopo warmup,
microfono verificato, segnale LED di pronto, attivazione del riconoscitore e apertura
del collegamento CLI. L'unità avvia anche Ollama tramite la dipendenza
esistente. Se microfono o backend falliscono, il servizio viene ritentato
dopo cinque secondi; il pronto viene nuovamente verificato.

Il file `.autonomous-mode` nel progetto mantiene la CLI collegata al
servizio anche quando questo è temporaneamente fermo: evita che il terminale
avvii un secondo controllore GPIO. Inoltre un lock di processo esclude
istanze concorrenti del backend o delle modalità manuali.

```bash
systemctl status gpio-llm-ready.service --no-pager
journalctl -u gpio-llm-ready.service -b --no-pager
sudo systemctl restart gpio-llm-ready.service
```

La vecchia modalità manuale `--voice` resta disponibile quando il backend è
fermo; per l'uso autonomo non occorre lanciare `voice/run.sh`.

## Audio e configurazione

L'audio viene letto come flusso continuo PCM mono 16 bit a 16 kHz da
`plughw:CARD=Talk,DEV=0`. Si usano eventi di lettura del flusso ALSA, senza
registrazioni periodiche su disco. In attesa viene riconosciuto il vocabolario
ristretto `ascolta`/`[unk]`; soltanto una frase finale corrispondente ad ASCOLTA
e con confidenza sufficiente avvia la sessione comando.

Durante i lampeggi di esito e l'inferenza LLM, il flusso continua a essere drenato
ma l'audio viene ignorato, così non si accumulano frasi da eseguire più tardi.
Il comando successivo al segnale usa il riconoscitore italiano completo;
viene inviato all'LLM solo a fine frase. I decimali dettati continuano a essere
normalizzati. L'audio non viene salvato e non viene inviato al cloud.

Prima dell'LLM, la modalità autonoma controlla che la frase vocale contenga
sia un riferimento al LED/GPIO sia un verbo di controllo (accendere, spegnere,
lampeggiare, fermare, ecc.). Una conversazione generica che non soddisfa questi
requisiti viene scartata con due lampeggi, senza chiamare il modello. Questo
prefiltro non cambia la CLI testuale e non sostituisce la validazione del
comando proposto dal modello.

Un comando di lampeggio con pin e intervallo numericamente univoci può essere
accettato anche se Vosk trascrive male le parole «LED sul pin», per esempio
«letto super in 17 intervallo 0.25 secondi».

### Numeri vocali

La normalizzazione produce numeri letterali sia per il GPIO sia per l'intervallo:

| Forma pronunciata | Valore in secondi |
|---|---:|
| un secondo; uno secondo; un secondi; uno secondi | 1 |
| un secondo e mezzo | 1.5 |
| zero virgola cinque; mezzo secondo | 0.5 |
| zero virgola venticinque; un quarto di secondo | 0.25 |
| zero virgola centoventicinque; zero virgola cento venticinque; un ottavo di secondo | 0.125 |
| zero virgola zero cinque | 0.05, rifiutato dalla policy minima di 0.1 s |

Sono gestiti numeri italiani fino a 999, frazioni comuni e decimali con virgola
o punto. Le cifre separate come «uno due cinque» dopo «zero virgola» danno
0.125; sequenze ambigue rimangono tali e vengono rifiutate.
«Un» viene convertito in 1 soltanto davanti a secondo/secondi o
millisecondo/millisecondi; in «un LED» mantiene il valore di articolo.

Nel percorso vocale autonomo Rust ricava pin e intervallo dal testo
normalizzato e impone questi valori alla proposta numerica del modello,
prima di applicare il normale validatore. Il modello continua a interpretare
l'azione. Non può così cambiare un 17 pronunciato in 27, né sostituire 0.125
con 0.5. Se pin o intervallo mancano nella frase, non vengono inventati.
I controlli valgono anche per le risposte recuperate dalla cache.

Questo vincolo conserva i numeri della **trascrizione**: se il riconoscitore
audio trascrive il numero stesso in modo errato, non può ricostruire la voce
originale. Il journal mostra sempre frase originale e numeri normalizzati.
La CLI testuale conserva interfaccia e percorso precedenti.

| Variabile del servizio | Default | Effetto |
|---|---|---|
| `GPIO_VOICE_DEVICE` | `plughw:CARD=Talk,DEV=0` | Ingresso ALSA |
| `GPIO_VOICE_MODEL` | Modello italiano piccolo del progetto | Directory Vosk |
| `GPIO_WAKE_CONFIDENCE` | `0.85` | Confidenza minima della parola ASCOLTA |
| `GPIO_VOICE_COMMAND_TIMEOUT` | `12` | Secondi massimi per una frase completa dopo il segnale |

Il timeout non promuove una trascrizione parziale a comando: una frase troppo
lunga viene scartata. La voce di un'altra persona o una riproduzione audio
possono comunque attivare il sistema: ASCOLTA non è autenticazione vocale.

Le variabili presenti quando viene eseguito l'installer vengono salvate
nell'unità se appartengono all'elenco supportato. Le impostazioni della policy
GPIO e l'endpoint LLM vengono conservati come configurazione esplicita del
servizio; systemd non legge `.bashrc`.

## Collegamento interno e verifiche

La CLI usa un socket Unix privato `/run/gpio-llm/control.sock` con permessi
600 in una directory accessibile al solo utente del servizio. Non c'è una
nuova porta TCP. Richiesta a righe JSON: `{"text":"spegni LED sul pin 17"}`;
risposta: `{"message":"OK: GPIO 17: spento","executed":true}`.
Le richieste sono limitate a 2048 byte di testo e 8192 byte per messaggio;
non sono ammessi newline nel comando o campi extra. La coda è limitata;
un comando che attende oltre 30 secondi viene rifiutato prima dell'esecuzione.

```bash
.venv-voice/bin/python -m unittest discover -s voice -p 'test_*.py' -v
cargo test --locked
cargo build --release --locked
```

I test verificano il consenso alla cattura dopo il segnale, l'assenza di
comandi senza ASCOLTA, il rifiuto dei parziali al timeout, decimali, frame
locali non validi e scadenza della coda. I collaudi reali devono verificare
anche la voce dell'utente, il LED e un avvio a freddo.

Riferimento per il vocabolario ristretto e i risultati finali:
[esempio ufficiale Vosk](https://github.com/alphacep/vosk-api/blob/master/python/example/test_words.py).
