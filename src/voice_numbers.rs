//! Literal numbers extracted from the normalized transcript, independently of the LLM.

use crate::command::RawCommand;

#[derive(Debug, Default, PartialEq)]
pub struct Numbers {
    pub pin: Option<u8>,
    pub interval_s: Option<f64>,
}

impl Numbers {
    pub fn constrain(&self, raw: RawCommand) -> RawCommand {
        match raw {
            RawCommand::Blink { .. } => RawCommand::Blink { pin: self.pin, interval_s: self.interval_s },
            RawCommand::On { .. } => RawCommand::On { pin: self.pin },
            RawCommand::Off { .. } => RawCommand::Off { pin: self.pin },
            RawCommand::Stop { .. } => RawCommand::Stop { pin: self.pin },
        }
    }
}

fn unit(word: &str) -> Option<f64> {
    match word {
        "secondo" | "secondi" | "s" | "sec" => Some(1.0),
        "millisecondo" | "millisecondi" | "ms" => Some(0.001),
        _ => None,
    }
}

fn one_value(values: &[f64]) -> Result<Option<f64>, String> {
    let Some(first) = values.first().copied() else { return Ok(None) };
    if values.iter().any(|value| (value - first).abs() > 1e-9) {
        return Err("più valori numerici diversi per lo stesso parametro; pronuncia un comando alla volta".into());
    }
    Ok(Some(first))
}

pub fn extract(text: &str) -> Result<Numbers, String> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '+'))
        .filter(|word| !word.is_empty()).collect();
    if words.contains(&"virgola") {
        return Err("numero decimale incompleto o ambiguo; ripeti il valore".into());
    }
    let mut intervals = Vec::new();
    let mut explicit_pins = Vec::new();
    let mut candidates = Vec::new();
    for (index, word) in words.iter().enumerate() {
        if let Some(pin) = ["gpio", "pin", "led", "lead"].iter()
            .find_map(|prefix| word.strip_prefix(prefix).filter(|rest| !rest.is_empty()).and_then(|rest| rest.parse::<u16>().ok())) {
            explicit_pins.push(f64::from(pin));
            continue;
        }
        let Ok(value) = word.parse::<f64>() else { continue };
        if !value.is_finite() {
            return Err("numero non finito".into());
        }
        let next = words.get(index + 1).copied().unwrap_or("");
        let previous = words.get(index.wrapping_sub(1)).copied().unwrap_or("");
        let explicit_pin = matches!(previous, "pin" | "gpio" | "led" | "lead")
            || (previous == "numero" && index >= 2 && matches!(words[index - 2], "pin" | "gpio"));
        if explicit_pin {
            explicit_pins.push(value);
        } else if let Some(scale) = unit(next) {
            intervals.push(value * scale);
        } else if matches!(previous, "intervallo" | "ogni") {
            intervals.push(value);
        } else if matches!(next, "volte" | "lampeggi" | "minuti" | "minuto" | "ore" | "ora") {
            // A count or an unsupported time unit is not a GPIO number.
            return Err("conteggi o unità temporali non supportati; indica il GPIO e l'intervallo in secondi".into());
        } else if word.contains('.') {
            intervals.push(value);
        } else {
            candidates.push(value);
        }
    }
    let pin_value = if explicit_pins.is_empty() { one_value(&candidates)? } else {
        if !candidates.is_empty() {
            return Err("numeri aggiuntivi non associati a pin o intervallo".into());
        }
        one_value(&explicit_pins)?
    };
    let pin = match pin_value {
        Some(value) if value.fract() == 0.0 && (0.0..=255.0).contains(&value) => Some(value as u8),
        Some(_) => return Err("numero GPIO non valido".into()),
        None => None,
    };
    Ok(Numbers { pin, interval_s: one_value(&intervals)? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::{validate, Policy, ValidationError};

    #[test]
    fn source_pin_and_decimal_override_both_observed_model_errors() {
        let facts = extract("fai lampeggiare led su pin 17 intervallo 0.125 secondi").unwrap();
        let raw = RawCommand::Blink { pin: Some(27), interval_s: Some(0.5) };
        let fixed = facts.constrain(raw);
        assert_eq!(fixed, RawCommand::Blink { pin: Some(17), interval_s: Some(0.125) });
        assert!(validate(fixed, &Policy::default()).is_ok());
    }

    #[test]
    fn values_survive_a_misrecognized_led_or_pin_label() {
        let facts = extract("fai lampeggiare letto super in 17 intervallo 0.25 secondi").unwrap();
        assert_eq!(facts, Numbers { pin: Some(17), interval_s: Some(0.25) });
        assert_eq!(extract("ogni 0.5 secondi lampeggia led 17").unwrap().pin, Some(17));
        assert_eq!(extract("lampeggia pin17 ogni 500 millisecondi").unwrap().interval_s, Some(0.5));
    }

    #[test]
    fn missing_information_is_never_filled_in_from_the_model() {
        let raw = RawCommand::Blink { pin: Some(17), interval_s: Some(0.5) };
        let no_pin = extract("lampeggia led ogni 0.5 secondi").unwrap().constrain(raw.clone());
        assert_eq!(validate(no_pin, &Policy::default()), Err(ValidationError::MissingPin));
        let no_interval = extract("lampeggia led sul pin 17").unwrap().constrain(raw);
        assert_eq!(validate(no_interval, &Policy::default()), Err(ValidationError::MissingInterval));
    }

    #[test]
    fn ambiguous_or_unfinished_numbers_are_rejected() {
        for phrase in [
            "pin 17 ogni 0.5 secondi oppure 0.25 secondi",
            "pin 17 e pin 27 ogni 0.5 secondi",
            "pin 17 intervallo 0 virgola 0 25 secondi",
            "lampeggia 17 volte ogni 0.5 secondi",
        ] { assert!(extract(phrase).is_err(), "{phrase}"); }
        assert_eq!(extract("pin 17 ogni -0.5 secondi").unwrap().interval_s, Some(-0.5));
        assert!(extract("pin -17 ogni 0.5 secondi").is_err());
    }
}
