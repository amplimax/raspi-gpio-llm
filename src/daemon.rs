//! One owner of the model and GPIO, accepting CLI and wake-word events.

use std::{
    os::unix::net::UnixDatagram,
    sync::{mpsc::{self, SyncSender}, Arc, Mutex},
    time::{Duration, Instant},
};
use anyhow::{bail, ensure, Context, Result};
use log::{info, warn};
use crate::{
    config::Config, controller::{self, Outcome}, executor::Executor, ipc,
    llm::LlmClient, startup, status, voice::{AutonomousEvent, AutonomousInput, SharedWorker},
};

pub enum Event {
    Cli { text: String, reply: SyncSender<Outcome>, queued: Instant },
    Voice { event: Result<AutonomousEvent>, queued: Instant },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase { Wake, Command }

fn ready_notify() -> Result<()> {
    if let Some(path) = std::env::var_os("NOTIFY_SOCKET") {
        let socket = UnixDatagram::unbound()?;
        let message = b"READY=1\nSTATUS=Pronto: CLI e parola ASCOLTA in ascolto";
        #[cfg(target_os = "linux")]
        {
            use std::os::{linux::net::SocketAddrExt, unix::ffi::OsStrExt};
            let bytes = path.as_os_str().as_bytes();
            if bytes.first() == Some(&b'@') {
                let address = std::os::unix::net::SocketAddr::from_abstract_name(&bytes[1..])?;
                socket.send_to_addr(message, &address)?;
                return Ok(());
            }
        }
        socket.send_to(message, path)?;
    }
    Ok(())
}

fn expired(queued: Instant) -> bool { queued.elapsed() > Duration::from_secs(30) }

fn capture_light(event: &Result<AutonomousEvent>) -> Option<bool> {
    match event {
        Ok(AutonomousEvent::Listening) => Some(true),
        Ok(AutonomousEvent::Transcript { .. } | AutonomousEvent::Timeout { .. }) | Err(_) => Some(false),
        _ => None,
    }
}

pub fn run(cfg: &Config, llm: &LlmClient, exec: &Mutex<Executor>, worker: &SharedWorker, status_led: &Arc<status::SharedStatus>) -> Result<()> {
    startup::warmup(llm, &cfg.policy)?;
    let (sender, receiver) = mpsc::sync_channel(32);
    let voice_sender = sender.clone();
    let capture_status = Arc::clone(status_led);
    let mut voice = AutonomousInput::start(worker, move |event| {
        // Update the capture light before enqueueing: a CLI inference may keep
        // the dispatcher busy after the microphone's listening window has ended.
        let event = match capture_light(&event) {
            Some(true) => match status::listening(&capture_status) {
                Ok(()) => event,
                Err(error) => Err(error),
            },
            Some(false) => { status::clear(&capture_status); event },
            None => event,
        };
        let _ = voice_sender.send(Event::Voice { event, queued: Instant::now() });
    })?;

    // The microphone is open and healthy before the existing ready signal.
    status::signal(status_led)?;
    voice.command("arm")?;
    match receiver.recv_timeout(Duration::from_secs(15)).context("attivazione dell'ascolto")? {
        Event::Voice { event, .. } => ensure!(matches!(event?, AutonomousEvent::Armed), "conferma di ascolto non valida"),
        _ => bail!("evento inatteso durante l'avvio"),
    }
    let _socket = ipc::serve(sender)?;
    ready_notify().context("notifica di pronto a systemd")?;
    info!("PRONTO: warmup e microfono verificati, 5 lampeggi di stato su BCM23; CLI e ASCOLTA attivi");
    let mut phase = Phase::Wake;

    while let Ok(event) = receiver.recv() {
        match event {
            Event::Cli { text, reply, queued } => {
                let result = if expired(queued) {
                    Outcome::rejected("Errore LLM: comando scaduto in coda, riprova")
                } else {
                    info!("CLI: {text}");
                    controller::execute(&text, cfg, llm, exec)
                };
                let _ = reply.send(result);
            }
            Event::Voice { event, queued } => {
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        status::failure(status_led)?;
                        return Err(error);
                    }
                };
                match event {
                    AutonomousEvent::Wake { confidence } if phase == Phase::Wake => {
                        if expired(queued) {
                            warn!("attivazione vocale scaduta in coda; torno ad ASCOLTA");
                            status::failure(status_led)?;
                            voice.command("arm")?;
                            continue;
                        }
                        info!("ASCOLTA riconosciuto (confidenza {confidence:.2}); apro la finestra del comando");
                        phase = Phase::Command;
                        voice.command("listen")?;
                    }
                    AutonomousEvent::Listening if phase == Phase::Command => {
                        info!("PARLA ORA: LED di stato acceso per tutta la finestra di ascolto");
                    }
                    AutonomousEvent::Transcript { text, llm_text, audio_s, stt_s } if phase == Phase::Command => {
                        info!("Trascrizione: {text}; audio={audio_s:.2} s; STT={stt_s:.2} s");
                        if text != llm_text { info!("Numeri normalizzati: {llm_text}"); }
                        if text.trim().is_empty() || llm_text.trim().is_empty() {
                            warn!("comando non riconosciuto: trascrizione vuota");
                            status::failure(status_led)?;
                        } else if !crate::voice_intent::is_control_request(&llm_text) {
                            warn!("Rifiutato: frase vocale non riconosciuta come comando LED/GPIO; nessuna richiesta al modello o attuazione");
                            status::failure(status_led)?;
                        } else if expired(queued) {
                            warn!("comando vocale scaduto in coda: nessuna esecuzione");
                            status::failure(status_led)?;
                        } else {
                            let prepared = controller::prepare_voice(&llm_text, cfg, llm);
                            let result = controller::execute_after_signal(prepared, || {
                                status::signal(status_led)?;
                                info!("comando vocale validato: 5 lampeggi completati prima dell'attuazione");
                                Ok(())
                            }, |action| controller::apply(action, exec))?;
                            info!("{}", result.message);
                            if !result.executed {
                                status::failure(status_led)?;
                                info!("comando vocale non eseguito: 2 lampeggi sul LED di stato BCM23");
                            }
                        }
                        phase = Phase::Wake;
                        voice.command("arm")?;
                    }
                    AutonomousEvent::Timeout { error } if phase == Phase::Command => {
                        warn!("ascolto terminato senza esecuzione: {error}");
                        status::failure(status_led)?;
                        phase = Phase::Wake;
                        voice.command("arm")?;
                    }
                    AutonomousEvent::Armed if phase == Phase::Wake => {
                        info!("in attesa della parola ASCOLTA");
                    }
                    other => bail!("evento STT inatteso nello stato {phase:?}: {other:?}"),
                }
            }
        }
    }
    bail!("canale dei comandi terminato")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_queued_commands_expire_instead_of_executing_later() {
        assert!(expired(Instant::now() - Duration::from_secs(31)));
        assert!(!expired(Instant::now()));
    }

    #[test]
    fn capture_led_tracks_actual_audio_events_including_failure() {
        assert_eq!(capture_light(&Ok(AutonomousEvent::Wake { confidence: 1.0 })), None);
        assert_eq!(capture_light(&Ok(AutonomousEvent::Listening)), Some(true));
        assert_eq!(capture_light(&Ok(AutonomousEvent::Transcript {
            text: "spegni pin 17".into(), llm_text: "spegni pin 17".into(), audio_s: 2.0, stt_s: 0.2,
        })), Some(false));
        assert_eq!(capture_light(&Ok(AutonomousEvent::Timeout { error: "silenzio".into() })), Some(false));
        assert_eq!(capture_light(&Err(anyhow::anyhow!("microfono scollegato"))), Some(false));
        assert_eq!(capture_light(&Ok(AutonomousEvent::Armed)), None);
    }
}
