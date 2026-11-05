//! Preparazione del modello per il servizio di avvio.
//! Le risposte del warmup vengono validate ma non raggiungono l'esecutore.

use std::{thread, time::{Duration, Instant}};

use anyhow::{ensure, Context, Result};
use log::{info, warn};

use crate::{
    command::{Action, RawCommand},
    llm::{LlmClient, LlmError},
    validator::{validate, Policy},
};

pub const PROBE_PIN: u8 = 17;
const SERVER_WAIT: Duration = Duration::from_secs(300);
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

fn warmup_once<F>(policy: &Policy, mut request: F) -> Result<()>
where
    F: FnMut(&str) -> std::result::Result<RawCommand, LlmError>,
{
    ensure!(policy.allowed_pins.contains(&PROBE_PIN),
        "BCM{PROBE_PIN} deve essere ammesso dalla policy per il self-test di attuazione");
    let probes = [
        ("accendi LED sul pin 17", Action::On { pin: PROBE_PIN }),
        ("spegni LED sul pin 17", Action::Halt { pin: PROBE_PIN }),
    ];
    for (text, expected) in probes {
        info!("warmup: interpreto {text:?} senza attuarlo");
        let raw = request(text).with_context(|| format!("warmup LLM: {text}"))?;
        let action = validate(raw, policy).context("warmup: risposta non ammessa dalla policy")?;
        ensure!(action == expected,
            "warmup: interpretazione errata per {text:?}: attesa {expected:?}, ricevuta {action:?}");
    }
    Ok(())
}

pub fn warmup(llm: &LlmClient, policy: &Policy) -> Result<()> {
    let started = Instant::now();
    loop {
        match warmup_once(policy, |text| llm.ask_uncached(text)) {
            Ok(()) => {
                info!("warmup del modello completato e verificato in {:.2} s", started.elapsed().as_secs_f64());
                return Ok(());
            }
            Err(err) => {
                // Attende il servizio se non raggiungibile o temporaneamente occupato.
                // Errori di parsing, policy e interpretazione fanno fallire subito il test.
                let retryable = matches!(err.downcast_ref::<LlmError>(), Some(LlmError::Http(_)));
                if !retryable || started.elapsed() >= SERVER_WAIT {
                    return Err(err).context("preparazione fallita: nessuna segnalazione di pronto");
                }
                warn!("warmup: server non pronto ({err:#}); nuovo tentativo tra 2 s");
                thread::sleep(RETRY_INTERVAL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn pronto_richiede_due_risposte_corrette() {
        let mut responses = VecDeque::from([
            RawCommand::On { pin: Some(17) },
            RawCommand::Off { pin: Some(17) },
        ]);
        let mut texts = Vec::new();
        warmup_once(&Policy::default(), |text| {
            texts.push(text.to_owned());
            Ok(responses.pop_front().unwrap())
        }).unwrap();
        assert!(responses.is_empty());
        assert_eq!(texts, ["accendi LED sul pin 17", "spegni LED sul pin 17"]);
    }

    #[test]
    fn accensione_al_posto_dello_spegnimento_non_da_pronto() {
        let err = warmup_once(&Policy::default(), |_| {
            Ok(RawCommand::On { pin: Some(17) })
        }).unwrap_err();
        assert!(err.to_string().contains("interpretazione errata"));
    }

    #[test]
    fn pin_diverso_anche_se_ammesso_non_da_pronto() {
        assert!(warmup_once(&Policy::default(), |_| {
            Ok(RawCommand::On { pin: Some(27) })
        }).is_err());
    }

    #[test]
    fn policy_senza_pin_di_prova_non_chiama_il_modello() {
        let policy = Policy { allowed_pins: vec![27], ..Policy::default() };
        assert!(warmup_once(&policy, |_| panic!("il modello non deve essere chiamato")).is_err());
    }

    #[test]
    fn errore_http_non_da_pronto() {
        let err = warmup_once(&Policy::default(), |_| {
            Err(LlmError::Http("connessione rifiutata".to_owned()))
        }).unwrap_err();
        assert!(matches!(err.downcast_ref::<LlmError>(), Some(LlmError::Http(_))));
    }
}
