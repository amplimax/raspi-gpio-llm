//! Shared command interpretation, validation and execution.

use std::sync::Mutex;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use crate::{command::{Action, RawCommand}, config::Config, executor::Executor, llm::LlmClient, validator::validate};

#[derive(Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub message: String,
    pub executed: bool,
}

impl Outcome {
    pub fn rejected(message: impl Into<String>) -> Self {
        Self { message: message.into(), executed: false }
    }
}

fn interpret(text: &str, llm: &LlmClient) -> Result<RawCommand, Outcome> {
    let raw = match llm.ask(text) {
        Ok(raw) => raw,
        Err(e) => {
            error!("LLM: {e}");
            return Err(Outcome::rejected(format!("Errore LLM: {e}")));
        }
    };
    info!("LLM -> {raw:?}");
    Ok(raw)
}

fn validated(raw: RawCommand, cfg: &Config) -> Result<Action, Outcome> {
    let action = match validate(raw, &cfg.policy) {
        Ok(action) => action,
        Err(e) => {
            warn!("comando rifiutato: {e}");
            return Err(Outcome::rejected(format!("Rifiutato: {e}")));
        }
    };
    Ok(action)
}

pub fn prepare(text: &str, cfg: &Config, llm: &LlmClient) -> Result<Action, Outcome> {
    validated(interpret(text, llm)?, cfg)
}

pub fn prepare_voice(text: &str, cfg: &Config, llm: &LlmClient) -> Result<Action, Outcome> {
    let numbers = crate::voice_numbers::extract(text).map_err(|error| {
        warn!("numeri vocali rifiutati: {error}");
        Outcome::rejected(format!("Rifiutato: {error}"))
    })?;
    let raw = interpret(text, llm)?;
    let constrained = numbers.constrain(raw.clone());
    if constrained != raw {
        info!("Numeri LLM corretti dai valori della frase: {constrained:?}");
    }
    validated(constrained, cfg)
}

pub fn apply(action: &Action, exec: &Mutex<Executor>) -> Outcome {
    match crate::lock(exec).run(action) {
        Ok(()) => {
            info!("eseguito: {action}");
            Outcome { message: format!("OK: {action}"), executed: true }
        }
        Err(e) => {
            error!("esecutore: {e}");
            Outcome::rejected(format!("Errore GPIO: {e}"))
        }
    }
}

pub fn execute(text: &str, cfg: &Config, llm: &LlmClient, exec: &Mutex<Executor>) -> Outcome {
    match prepare(text, cfg, llm) {
        Ok(action) => apply(&action, exec),
        Err(outcome) => outcome,
    }
}

/// A valid action can reach GPIO only after the complete acknowledgement.
pub fn execute_after_signal(
    prepared: Result<Action, Outcome>,
    acknowledge: impl FnOnce() -> anyhow::Result<()>,
    actuate: impl FnOnce(&Action) -> Outcome,
) -> anyhow::Result<Outcome> {
    match prepared {
        Ok(action) => { acknowledge()?; Ok(actuate(&action)) },
        Err(outcome) => Ok(outcome),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn valid_command_waits_for_the_whole_signal_before_actuation() {
        let events = RefCell::new(Vec::new());
        let result = execute_after_signal(Ok(Action::On { pin: 17 }), || {
            events.borrow_mut().push("signal start");
            events.borrow_mut().push("signal end");
            Ok(())
        }, |_| {
            events.borrow_mut().push("actuation");
            Outcome { message: "OK".into(), executed: true }
        }).unwrap();
        assert!(result.executed);
        assert_eq!(*events.borrow(), ["signal start", "signal end", "actuation"]);
    }

    #[test]
    fn rejected_command_or_failed_signal_cannot_actuate() {
        let result = execute_after_signal(Err(Outcome::rejected("missing pin")),
            || panic!("no success acknowledgement for a rejection"),
            |_| panic!("no actuation for a rejection")).unwrap();
        assert!(!result.executed);
        let actuated = Cell::new(false);
        assert!(execute_after_signal(Ok(Action::On { pin: 17 }),
            || anyhow::bail!("status unavailable"), |_| {
                actuated.set(true);
                Outcome { message: "OK".into(), executed: true }
            }).is_err());
        assert!(!actuated.get());
    }
}
