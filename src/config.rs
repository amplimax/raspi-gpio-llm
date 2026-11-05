//! Configurazione da variabili d'ambiente, con valori di default sicuri.
//!
//! GPIO_LLM_URL          URL di llama-server (default: http://127.0.0.1:8080/v1/chat/completions)
//! GPIO_ALLOWED_PINS     pin BCM ammessi, separati da virgola (default: 17,27,22)
//! GPIO_MIN_INTERVAL_S   intervallo minimo in secondi (default: 0.1)
//! GPIO_MAX_INTERVAL_S   intervallo massimo in secondi (default: 60)

use anyhow::{ensure, Context, Result};

use crate::validator::Policy;

const DEFAULT_LLM_URL: &str = "http://127.0.0.1:8080/v1/chat/completions";

#[derive(Debug, Clone)]
pub struct Config {
    pub llm_url: String,
    pub policy: Policy,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Costruisce la configurazione da una funzione di lookup (testabile senza toccare l'ambiente).
    pub fn from_lookup<F: Fn(&str) -> Option<String>>(get: F) -> Result<Self> {
        let mut policy = Policy::default();

        if let Some(v) = get("GPIO_ALLOWED_PINS") {
            let pins: Result<Vec<u8>> = v
                .split(',')
                .map(|s| {
                    s.trim()
                        .parse::<u8>()
                        .with_context(|| format!("GPIO_ALLOWED_PINS: pin non valido {s:?}"))
                })
                .collect();
            policy.allowed_pins = pins?;
        }
        if let Some(v) = get("GPIO_MIN_INTERVAL_S") {
            policy.min_interval_s = v.trim().parse().context("GPIO_MIN_INTERVAL_S non valido")?;
        }
        if let Some(v) = get("GPIO_MAX_INTERVAL_S") {
            policy.max_interval_s = v.trim().parse().context("GPIO_MAX_INTERVAL_S non valido")?;
        }

        ensure!(!policy.allowed_pins.is_empty(), "nessun pin ammesso");
        ensure!(!policy.allowed_pins.contains(&crate::status::PIN),
            "BCM23 è riservato al LED di stato e non può essere incluso in GPIO_ALLOWED_PINS");
        ensure!(
            policy.min_interval_s > 0.0 && policy.min_interval_s <= policy.max_interval_s,
            "intervalli incoerenti: serve 0 < min <= max"
        );

        Ok(Self {
            llm_url: get("GPIO_LLM_URL").unwrap_or_else(|| DEFAULT_LLM_URL.to_owned()),
            policy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn default() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.policy, Policy::default());
        assert_eq!(c.llm_url, DEFAULT_LLM_URL);
    }

    #[test]
    fn override_valori() {
        let c = cfg(&[
            ("GPIO_ALLOWED_PINS", "5, 6"),
            ("GPIO_MIN_INTERVAL_S", "0.5"),
            ("GPIO_MAX_INTERVAL_S", "10"),
            ("GPIO_LLM_URL", "http://localhost:9000/v1/chat/completions"),
        ])
        .unwrap();
        assert_eq!(c.policy.allowed_pins, vec![5, 6]);
        assert_eq!(c.policy.min_interval_s, 0.5);
        assert_eq!(c.policy.max_interval_s, 10.0);
        assert!(c.llm_url.ends_with(":9000/v1/chat/completions"));
    }

    #[test]
    fn valori_non_validi() {
        assert!(cfg(&[("GPIO_ALLOWED_PINS", "17,abc")]).is_err());
        assert!(cfg(&[("GPIO_ALLOWED_PINS", "")]).is_err());
        assert!(cfg(&[("GPIO_ALLOWED_PINS", "17,23")]).is_err());
        assert!(cfg(&[("GPIO_MIN_INTERVAL_S", "5"), ("GPIO_MAX_INTERVAL_S", "1")]).is_err());
        assert!(cfg(&[("GPIO_MIN_INTERVAL_S", "0")]).is_err());
    }
}
