//! Manual speech input from a persistent local Vosk worker.
//! Only a finalized transcription reaches the normal validation/execution path.

use std::{
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::{bail, ensure, Context, Result};
use log::warn;
use serde::Deserialize;

pub type SharedWorker = Arc<Mutex<Option<Child>>>;

fn spawn_worker(worker: &SharedWorker, flag: &str) -> Result<(ChildStdin, ChildStdout)> {
    let root = std::env::var_os("GPIO_VOICE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let python = root.join(".venv-voice/bin/python");
    let script = root.join("voice/stt.py");
    ensure!(python.is_file() && script.is_file(), "ingresso vocale non installato in {}; consulta voice/README.md", root.display());
    let mut slot = worker.lock().unwrap_or_else(|e| e.into_inner());
    ensure!(slot.is_none(), "un processo vocale è già attivo");
    let mut child = Command::new(python).arg("-u").arg(script).arg(flag)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())
        .spawn().context("avvio del riconoscitore vocale")?;
    let input = child.stdin.take().expect("stdin pipe");
    let output = child.stdout.take().expect("stdout pipe");
    *slot = Some(child);
    Ok((input, output))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AutonomousEvent {
    Ready,
    Armed,
    Listening,
    Wake { confidence: f64 },
    Transcript { text: String, llm_text: String, audio_s: f64, stt_s: f64 },
    Timeout { error: String },
}

pub struct AutonomousInput {
    worker: SharedWorker,
    input: ChildStdin,
}

impl AutonomousInput {
    pub fn start(worker: &SharedWorker, mut receive: impl FnMut(Result<AutonomousEvent>) + Send + 'static) -> Result<Self> {
        let (input, output) = spawn_worker(worker, "--autonomous")?;
        let voice = Self { worker: Arc::clone(worker), input };
        let mut reader = BufReader::new(output);
        let mut line = String::new();
        ensure!(reader.read_line(&mut line)? > 0, "il microfono non è pronto; controlla i messaggi STT");
        ensure!(matches!(serde_json::from_str::<AutonomousEvent>(&line)?, AutonomousEvent::Ready), "risposta iniziale del microfono non valida");
        thread::spawn(move || loop {
            let mut line = String::new();
            let event = (|| -> Result<AutonomousEvent> {
                ensure!(reader.read_line(&mut line)? > 0, "il processo di ascolto è terminato");
                serde_json::from_str(&line).context("evento vocale non valido")
            })();
            let failed = event.is_err();
            receive(event);
            if failed { break; }
        });
        Ok(voice)
    }

    pub fn command(&mut self, command: &str) -> Result<()> {
        serde_json::to_writer(&mut self.input, &serde_json::json!({"command": command}))?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        Ok(())
    }
}

impl Drop for AutonomousInput {
    fn drop(&mut self) { stop_worker(&self.worker); }
}

/// Terminate the worker on normal exit and Ctrl+C/SIGTERM. SIGTERM lets Python
/// stop and reap an active arecord before exiting; force termination is bounded.
pub fn stop_worker(worker: &SharedWorker) {
    let child = worker.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(mut child) = child {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        let _ = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        for _ in 0..50 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

struct VoiceInput {
    worker: SharedWorker,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(default)]
    ready: bool,
    text: Option<String>,
    llm_text: Option<String>,
    error: Option<String>,
    audio_s: Option<f64>,
    stt_s: Option<f64>,
    peak_dbfs: Option<f64>,
    clipped_pct: Option<f64>,
}

fn parse_reply(line: &str) -> Result<Reply> {
    let reply: Reply = serde_json::from_str(line).context("risposta STT non valida")?;
    if let Some(error) = &reply.error {
        bail!("STT: {error}");
    }
    Ok(reply)
}

impl VoiceInput {
    fn start(worker: &SharedWorker) -> Result<Self> {
        let (input, output) = spawn_worker(worker, "--worker")?;
        let mut voice = Self { worker: Arc::clone(worker), input, output: BufReader::new(output) };
        ensure!(voice.read_reply()?.ready, "il riconoscitore vocale non è pronto");
        Ok(voice)
    }

    fn read_reply(&mut self) -> Result<Reply> {
        let mut line = String::new();
        ensure!(self.output.read_line(&mut line)? != 0, "il processo STT è terminato; controlla i messaggi precedenti");
        parse_reply(&line)
    }

    fn listen(&mut self) -> Result<Reply> {
        self.input.write_all(b"{\"command\":\"listen\"}\n")?;
        self.input.flush()?;
        let reply = self.read_reply()?;
        ensure!(reply.text.is_some(), "trascrizione finale assente nella risposta STT");
        Ok(reply)
    }
}

impl Drop for VoiceInput {
    fn drop(&mut self) {
        stop_worker(&self.worker);
    }
}

pub fn repl(worker: &SharedWorker, mut process: impl FnMut(&str)) -> Result<()> {
    let mut voice = VoiceInput::start(worker)?;
    println!("Voce italiana pronta. Premi Invio per registrare; scrivi esci per terminare.");
    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("Invio = ascolta > ");
        io::stdout().flush()?;
        line.clear();
        if stdin.lock().read_line(&mut line)? == 0 || line.trim().eq_ignore_ascii_case("esci") {
            return Ok(());
        }
        if !line.trim().is_empty() {
            println!("Premi solo Invio per iniziare la registrazione, oppure scrivi esci.");
            continue;
        }
        let reply = match voice.listen() {
            Ok(reply) => reply,
            Err(error) => {
                // Invalid/failed audio never reaches the LLM or GPIO. A broken
                // worker ends this mode; ALSA capture errors allow another try.
                let running = worker.lock().unwrap_or_else(|e| e.into_inner())
                    .as_mut().map(|child| child.try_wait()).transpose()?;
                if !matches!(running, Some(None)) {
                    return Err(error);
                }
                eprintln!("Registrazione rifiutata: {error:#}");
                continue;
            }
        };
        println!("Audio: {:.1} s; STT: {:.2} s; picco: {:.1} dBFS",
            reply.audio_s.unwrap_or_default(), reply.stt_s.unwrap_or_default(), reply.peak_dbfs.unwrap_or(-96.0));
        if reply.clipped_pct.unwrap_or_default() > 0.1 {
            warn!("audio saturo: riduci il guadagno del microfono");
        }
        let text = reply.text.as_deref().unwrap_or_default().trim();
        if text.is_empty() {
            println!("Nessuna frase riconosciuta; riprova parlando dopo PARLA ORA.");
            continue;
        }
        println!("Trascrizione: {text}");
        let llm_text = reply.llm_text.as_deref().unwrap_or(text).trim();
        ensure!(!llm_text.is_empty(), "testo normalizzato assente");
        if llm_text != text {
            println!("Numeri normalizzati: {llm_text}");
        }
        process(llm_text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_partial_text_are_not_valid_final_transcripts() {
        assert!(parse_reply(r#"{"error":"device scollegato","text":"accendi pin 17"}"#).is_err());
        assert!(parse_reply("testo libero fuori protocollo").is_err());
        assert!(parse_reply(r#"{"partial":"accendi"}"#).unwrap().text.is_none());
    }

    #[test]
    fn empty_speech_is_distinct_from_a_failed_capture() {
        let reply = parse_reply(r#"{"text":"","audio_s":6.0,"stt_s":0.2}"#).unwrap();
        assert_eq!(reply.text.as_deref(), Some(""));
        assert!(!reply.ready);
    }
}
