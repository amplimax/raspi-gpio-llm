//! Tipi di comando: dal JSON grezzo dell'LLM all'azione validata.

use std::{fmt, time::Duration};

use serde::Deserialize;

/// Comando grezzo prodotto dall'LLM. Non fidato: puo' avere campi mancanti
/// o valori fuori range. Si trasforma in [`Action`] solo tramite il validatore.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RawCommand {
    Blink {
        pin: Option<u8>,
        interval_s: Option<f64>,
    },
    On {
        pin: Option<u8>,
    },
    Off {
        pin: Option<u8>,
    },
    Stop {
        pin: Option<u8>,
    },
}

/// Azione validata, pronta per l'esecutore.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Alterna lo stato del pin ogni `half_period`.
    Blink {
        pin: u8,
        half_period: Duration,
    },
    On {
        pin: u8,
    },
    /// Ferma qualsiasi attivita' sul pin e lo porta a livello basso.
    Halt {
        pin: u8,
    },
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Blink { pin, half_period } => write!(
                f,
                "GPIO {pin}: lampeggio, cambio stato ogni {:.2} s",
                half_period.as_secs_f64()
            ),
            Action::On { pin } => write!(f, "GPIO {pin}: acceso"),
            Action::Halt { pin } => write!(f, "GPIO {pin}: spento"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blink_completo() {
        let c: RawCommand =
            serde_json::from_str(r#"{"action":"blink","pin":17,"interval_s":2}"#).unwrap();
        assert_eq!(
            c,
            RawCommand::Blink {
                pin: Some(17),
                interval_s: Some(2.0)
            }
        );
    }

    #[test]
    fn campi_null_o_assenti_diventano_none() {
        let c: RawCommand =
            serde_json::from_str(r#"{"action":"blink","pin":null,"interval_s":null}"#).unwrap();
        assert_eq!(
            c,
            RawCommand::Blink {
                pin: None,
                interval_s: None
            }
        );
        let c: RawCommand = serde_json::from_str(r#"{"action":"on"}"#).unwrap();
        assert_eq!(c, RawCommand::On { pin: None });
    }

    #[test]
    fn azione_sconosciuta_rifiutata() {
        assert!(
            serde_json::from_str::<RawCommand>(r#"{"action":"format_disk","pin":17}"#).is_err()
        );
    }

    #[test]
    fn pin_fuori_da_u8_rifiutato() {
        assert!(serde_json::from_str::<RawCommand>(r#"{"action":"on","pin":300}"#).is_err());
        assert!(serde_json::from_str::<RawCommand>(r#"{"action":"on","pin":-1}"#).is_err());
    }

    #[test]
    fn display_azioni() {
        let a = Action::Blink {
            pin: 17,
            half_period: Duration::from_millis(1500),
        };
        assert_eq!(
            a.to_string(),
            "GPIO 17: lampeggio, cambio stato ogni 1.50 s"
        );
        assert_eq!(Action::On { pin: 27 }.to_string(), "GPIO 27: acceso");
        assert_eq!(Action::Halt { pin: 22 }.to_string(), "GPIO 22: spento");
    }
}
