//! Client LLM per API chat compatibili OpenAI, con schema JSON e cache delle interpretazioni.

use std::{collections::HashMap, sync::Mutex, time::{Duration, Instant}};

use serde_json::{
    json,
    Value
};
use thiserror::Error;

use crate::command::RawCommand;

const SYSTEM_PROMPT: &str = r#"Traduci un comando italiano per un LED in JSON.
Scegli action in base al significato del comando:
- accendi, attiva: on;
- spegni, disattiva: off;
- ferma, interrompi il lampeggio: stop;
- lampeggia, fai lampeggiare: blink.
Rispondi con action e pin. Aggiungi interval_s solo per blink.
Produci JSON compatto su una sola riga, senza spazi o ritorni a capo.
pin e' il numero GPIO indicato dall'utente, come intero, oppure null se manca.
Per blink, interval_s e' l'intervallo in secondi indicato dall'utente, oppure null se manca.
Per on, off e stop ometti interval_s.
Non inventare valori mancanti e non copiare il pin dagli esempi.

Esempio: accendi LED su pin 17
{"action":"on","pin":17}
Esempio: spegni LED su pin 17
{"action":"off","pin":17}
Esempio: ferma LED su pin 17
{"action":"stop","pin":17}
Esempio: fai lampeggiare LED su pin 27 ogni 2 secondi
{"action":"blink","pin":27,"interval_s":2}"#;

 # [derive(Debug, Error)]
pub enum LlmError {
     # [error("llama-server non raggiungibile o errore HTTP: {0}")]
    Http(String),
     # [error("risposta dell'LLM vuota o in formato inatteso")]
    EmptyResponse,
     # [error("JSON non valido dall'LLM ({reason}): {content}")]
    InvalidJson {
        content: String,
        reason: String
    },
}

pub struct LlmClient {
    agent: ureq::Agent,
    url: String,
    // Solo interpretazioni: validazione ed esecuzione restano a carico del chiamante.
    cache: Mutex<HashMap<String, RawCommand>>,
}

const MAX_CACHED_COMMANDS: usize = 128;

fn command_schema()->Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["blink", "on", "off", "stop"]
            },
            "pin": {
                "type": ["integer", "null"]
            },
            "interval_s": {
                "type": ["number", "null"]
            }
        },
        "required": ["action", "pin"],
        "additionalProperties": false
    })
}

fn request_body(text:  & str)->Value {
    json!({
		"model": "qwen3:0.6b",
        "messages": [{
                "role": "system",
                "content": SYSTEM_PROMPT
            },
            // /no_think: modalita' non-thinking di Qwen3
            {
                "role": "user",
                "content": format!("{text} /no_think")
            }
        ],
        "temperature": 0,
        "reasoning_effort": "none",
        "stream": false,
        "keep_alive": -1,
        "max_tokens": 256,
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "gpio_command",
                "strict": true,
                "schema": command_schema()
            }
        }
    })
}

fn http_error(e: ureq::Error)->LlmError {
    match e {
        ureq::Error::Status(code, resp) => LlmError::Http(format!(
                "HTTP {code}: {}",
                resp.into_string().unwrap_or_default())),
        other => LlmError::Http(other.to_string()),
    }
}

impl LlmClient {
    pub fn new(url: impl Into < String > )->Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout(Duration::from_secs(120))
            .build();
        Self {
            agent,
            url: url.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn ask( & self, text:  & str)->Result < RawCommand,
    LlmError > {
        let key = text.trim();
        if let Some(raw) = self.cache.lock().ok().and_then(|cache| cache.get(key).cloned()) {
            log::info!("LLM: interpretazione da cache; nessuna nuova inferenza");
            return Ok(raw);
        }

        let raw = self.ask_uncached(text)?;
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= MAX_CACHED_COMMANDS {
                if let Some(old_key) = cache.keys().next().cloned() {
                    cache.remove(&old_key);
                }
            }
            cache.insert(key.to_owned(), raw.clone());
        }
        Ok(raw)
    }

    /// Inferenza reale, senza leggere o scrivere la cache delle frasi.
    /// Usata dal warmup per verificare il server e riscaldare il prompt.
    pub fn ask_uncached(&self, text: &str) -> Result<RawCommand, LlmError> {
        let started = Instant::now();
        let resp: Value = self
            .agent
            .post( & self.url)
            .send_json(request_body(text))
            .map_err(http_error) ?
            .into_json()
            .map_err( | e | LlmError::Http(e.to_string())) ? ;

        let elapsed = started.elapsed().as_secs_f64();
        let input_tokens = resp
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "n/d".to_owned());
        let output_tokens = resp
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "n/d".to_owned());
        log::info!(
            "Richiesta LLM: {:.2} s; token input={}; token output={}",
            elapsed,
            input_tokens,
            output_tokens
        );
        if let (Some(prompt_ms), Some(generation_ms)) = (
            resp.pointer("/timings/prompt_ms").and_then(Value::as_f64),
            resp.pointer("/timings/predicted_ms").and_then(Value::as_f64),
        ) {
            log::info!(
                "Tempi server LLM: prompt={:.2} s; generazione={:.2} s",
                prompt_ms / 1000.0,
                generation_ms / 1000.0
            );
        }

        let content = resp
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or(LlmError::EmptyResponse) ? ;

        log::debug!("LLM JSON: {}", content);
        serde_json::from_str(content).map_err( | e | LlmError::InvalidJson {
            content: content.to_owned(),
            reason: e.to_string(),
        })
    }
}

 # [cfg(test)]
mod tests {
    use super:: * ;

    #[test]
    fn cache_distingue_le_frasi_e_non_memorizza_risposte_non_valide() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1/chat/completions", listener.local_addr().unwrap());
        let on = "accendi LED sul pin 17";
        let off = "spegni LED sul pin 17";
        let retry = "disattiva LED sul pin 17";
        let server = thread::spawn(move || {
            let exchanges = [
                (on, r#"{"action":"on","pin":17}"#),
                (off, r#"{"action":"off","pin":17}"#),
                (retry, r#"{"action":"off","pin":"17"}"#),
                (retry, r#"{"action":"off","pin":17}"#),
                (off, r#"{"action":"off","pin":17}"#),
            ];
            for (expected_text, content) in exchanges {
                let deadline = Instant::now() + Duration::from_secs(5);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "richiesta HTTP attesa ma assente");
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => panic!("accept: {e}"),
                    }
                };
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut content_length = 0;
                loop {
                    line.clear();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" { break; }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            content_length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                }
                assert!(content_length > 0);
                let mut bytes = vec![0; content_length];
                reader.read_exact(&mut bytes).unwrap();
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    request["messages"][1]["content"],
                    format!("{expected_text} /no_think")
                );
                let response = json!({
                    "choices": [{"message": {"content": content}}]
                }).to_string();
                write!(reader.get_mut(),
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(), response
                ).unwrap();
            }
        });

        let client = LlmClient::new(url);
        assert_eq!(client.ask(on).unwrap(), RawCommand::On { pin: Some(17) });
        // Nessuna seconda richiesta HTTP per la stessa frase (spazi esterni esclusi).
        assert_eq!(client.ask(&format!("  {on}  ")).unwrap(), RawCommand::On { pin: Some(17) });
        assert_eq!(client.ask(off).unwrap(), RawCommand::Off { pin: Some(17) });
        assert!(matches!(client.ask(retry), Err(LlmError::InvalidJson { .. })));
        // Una risposta non valida non entra in cache: il secondo tentativo usa HTTP.
        assert_eq!(client.ask(retry).unwrap(), RawCommand::Off { pin: Some(17) });
        assert_eq!(client.ask(retry).unwrap(), RawCommand::Off { pin: Some(17) });
        // Il warmup deve raggiungere il server anche se la frase e' gia' in cache.
        assert_eq!(client.ask_uncached(off).unwrap(), RawCommand::Off { pin: Some(17) });
        server.join().unwrap();
    }

    #[test]
    fn json_breve_compatibile_con_validazione_e_lampeggio() {
        use crate::command::Action;
        use crate::validator::{validate, Policy, ValidationError};

        let policy = Policy::default();
        for (content, expected) in [
            (r#"{"action":"on","pin":17}"#, Action::On { pin: 17 }),
            (r#"{"action":"off","pin":17}"#, Action::Halt { pin: 17 }),
            (r#"{"action":"stop","pin":17}"#, Action::Halt { pin: 17 }),
            (r#"{"action":"blink","pin":17,"interval_s":2}"#,
                Action::Blink { pin: 17, half_period: Duration::from_secs(2) }),
        ] {
            let raw = serde_json::from_str::<RawCommand>(content).unwrap();
            assert_eq!(validate(raw, &policy).unwrap(), expected);
        }

        let missing_interval = serde_json::from_str::<RawCommand>(
            r#"{"action":"blink","pin":17}"#
        ).unwrap();
        assert_eq!(validate(missing_interval, &policy), Err(ValidationError::MissingInterval));
        let missing_pin = serde_json::from_str::<RawCommand>(
            r#"{"action":"off","pin":null}"#
        ).unwrap();
        assert_eq!(validate(missing_pin, &policy), Err(ValidationError::MissingPin));
        let forbidden_pin = serde_json::from_str::<RawCommand>(
            r#"{"action":"on","pin":5}"#
        ).unwrap();
        assert_eq!(validate(forbidden_pin, &policy), Err(ValidationError::PinNotAllowed(5)));
    }

     # [test]
    fn azioni_dello_schema_coincidono_con_rawcommand() {
        let schema = command_schema();
        let actions = schema["properties"]["action"]["enum"].as_array().unwrap();
        assert_eq!(actions.len(), 4);
        for a in actions {
            let v = json!({
                    "action": a,
                    "pin": 17,
                    "interval_s": 1.0
                });
            assert!(
                serde_json::from_value:: < RawCommand > (v).is_ok(),
                "azione nello schema non gestita da RawCommand: {a}");
        }
    }

     # [test]
    fn richiesta_contiene_schema_e_testo_utente() {
        let b = request_body("accendi il led 17");
        assert_eq!(b["temperature"], 0);
        assert!(b["messages"][1]["content"]
            .as_str()
            .unwrap()
            .starts_with("accendi il led 17"));
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(
            b["response_format"]["json_schema"]["schema"],
            command_schema()
        );
    }
}
