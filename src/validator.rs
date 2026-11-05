//! Validatore: unico punto in cui un `RawCommand` diventa un `Action`.
//! Codice puro e deterministico: nessuna dipendenza da hardware o rete.

use std::time::Duration;

use thiserror::Error;

use crate::command::{Action, RawCommand};

/// Regole di sicurezza applicate ai comandi.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Pin GPIO (numerazione BCM) che possono essere comandati.
    pub allowed_pins: Vec<u8>,
    pub min_interval_s: f64,
    pub max_interval_s: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed_pins: vec![17, 27, 22],
            min_interval_s: 0.1,
            max_interval_s: 60.0,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("Quale GPIO devo usare?")]
    MissingPin,
    #[error("GPIO {0} non consentito")]
    PinNotAllowed(u8),
    #[error("Con che intervallo (in secondi)?")]
    MissingInterval,
    #[error("Intervallo fuori limite ({min}-{max} s)")]
    IntervalOutOfRange { min: f64, max: f64 },
}

fn check_pin(pin: Option<u8>, policy: &Policy) -> Result<u8, ValidationError> {
    let pin = pin.ok_or(ValidationError::MissingPin)?;
    if pin != crate::status::PIN && policy.allowed_pins.contains(&pin) {
        Ok(pin)
    } else {
        Err(ValidationError::PinNotAllowed(pin))
    }
}

pub fn validate(cmd: RawCommand, policy: &Policy) -> Result<Action, ValidationError> {
    match cmd {
        RawCommand::Blink { pin, interval_s } => {
            let pin = check_pin(pin, policy)?;
            let secs = interval_s.ok_or(ValidationError::MissingInterval)?;
            // `contains` e' false per NaN: anche NaN viene rifiutato.
            if !(policy.min_interval_s..=policy.max_interval_s).contains(&secs) {
                return Err(ValidationError::IntervalOutOfRange {
                    min: policy.min_interval_s,
                    max: policy.max_interval_s,
                });
            }
            Ok(Action::Blink {
                pin,
                half_period: Duration::from_secs_f64(secs),
            })
        }
        RawCommand::On { pin } => Ok(Action::On {
            pin: check_pin(pin, policy)?,
        }),
        RawCommand::Off { pin } | RawCommand::Stop { pin } => Ok(Action::Halt {
            pin: check_pin(pin, policy)?,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blink(pin: Option<u8>, interval_s: Option<f64>) -> RawCommand {
        RawCommand::Blink { pin, interval_s }
    }

    #[test]
    fn blink_valido() {
        let a = validate(blink(Some(17), Some(2.0)), &Policy::default()).unwrap();
        assert_eq!(
            a,
            Action::Blink {
                pin: 17,
                half_period: Duration::from_secs(2)
            }
        );
    }

    #[test]
    fn pin_non_in_whitelist() {
        let e = validate(RawCommand::On { pin: Some(2) }, &Policy::default()).unwrap_err();
        assert_eq!(e, ValidationError::PinNotAllowed(2));
    }

    #[test]
    fn status_led_is_reserved_even_if_a_custom_policy_whitelists_it() {
        let policy = Policy { allowed_pins: vec![17, 23], ..Policy::default() };
        for command in [
            RawCommand::On { pin: Some(23) },
            RawCommand::Off { pin: Some(23) },
            RawCommand::Stop { pin: Some(23) },
            RawCommand::Blink { pin: Some(23), interval_s: Some(0.5) },
        ] {
            assert_eq!(validate(command, &policy), Err(ValidationError::PinNotAllowed(23)));
        }
        assert!(validate(RawCommand::On { pin: Some(17) }, &policy).is_ok());
    }

    #[test]
    fn pin_mancante() {
        let e = validate(blink(None, Some(1.0)), &Policy::default()).unwrap_err();
        assert_eq!(e, ValidationError::MissingPin);
    }

    #[test]
    fn intervallo_mancante() {
        let e = validate(blink(Some(17), None), &Policy::default()).unwrap_err();
        assert_eq!(e, ValidationError::MissingInterval);
    }

    #[test]
    fn intervallo_fuori_limite() {
        let p = Policy::default();
        for v in [0.0001, 0.0, -1.0, 10_000.0, f64::NAN, f64::INFINITY] {
            let e = validate(blink(Some(17), Some(v)), &p).unwrap_err();
            assert!(
                matches!(e, ValidationError::IntervalOutOfRange { .. }),
                "v={v}"
            );
        }
    }

    #[test]
    fn estremi_inclusi() {
        let p = Policy::default();
        assert!(validate(blink(Some(17), Some(p.min_interval_s)), &p).is_ok());
        assert!(validate(blink(Some(17), Some(p.max_interval_s)), &p).is_ok());
    }

    #[test]
    fn off_e_stop_diventano_halt() {
        let p = Policy::default();
        let expected = Action::Halt { pin: 27 };
        assert_eq!(
            validate(RawCommand::Off { pin: Some(27) }, &p).unwrap(),
            expected
        );
        assert_eq!(
            validate(RawCommand::Stop { pin: Some(27) }, &p).unwrap(),
            expected
        );
    }

    #[test]
    fn policy_personalizzata() {
        let p = Policy {
            allowed_pins: vec![5],
            min_interval_s: 1.0,
            max_interval_s: 2.0,
        };
        assert!(validate(RawCommand::On { pin: Some(5) }, &p).is_ok());
        assert_eq!(
            validate(RawCommand::On { pin: Some(17) }, &p).unwrap_err(),
            ValidationError::PinNotAllowed(17)
        );
        assert!(validate(blink(Some(5), Some(0.5)), &p).is_err());
    }
}
