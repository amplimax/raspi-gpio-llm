//! Private Unix socket used by the unchanged text CLI. No TCP listener.

use std::{
    fs, io::{BufRead, BufReader, Read, Write}, os::unix::{fs::{FileTypeExt, PermissionsExt}, net::{UnixListener, UnixStream}},
    path::{Path, PathBuf}, sync::{atomic::{AtomicUsize, Ordering}, mpsc::{self, SyncSender}, Arc}, thread,
    time::{Duration, Instant},
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use crate::{controller::Outcome, daemon::Event};

const MAX_FRAME: usize = 8192;
pub const DEFAULT_SOCKET: &str = "/run/gpio-llm/control.sock";

pub fn socket_path() -> PathBuf {
    std::env::var_os("GPIO_LLM_SOCKET").map(PathBuf::from).unwrap_or_else(|| DEFAULT_SOCKET.into())
}

pub fn use_service() -> bool {
    std::env::var_os("GPIO_LLM_SOCKET").is_some()
        || Path::new(env!("CARGO_MANIFEST_DIR")).join(".autonomous-mode").exists()
        || socket_path().exists()
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request { text: String }

fn read_frame(reader: impl Read) -> Result<String> {
    let mut frame = String::new();
    let count = BufReader::new(reader.take((MAX_FRAME + 1) as u64)).read_line(&mut frame)?;
    ensure!(count > 0 && count <= MAX_FRAME && frame.ends_with('\n'), "messaggio locale incompleto o troppo lungo");
    Ok(frame)
}

fn parse_request(frame: &str) -> Result<Request> {
    let request: Request = serde_json::from_str(frame).context("richiesta locale non valida")?;
    ensure!(!request.text.trim().is_empty() && request.text.len() <= 2048
        && !request.text.contains(['\n', '\r', '\0']), "testo comando non valido");
    Ok(request)
}

pub fn request(text: &str) -> Result<Outcome> {
    let mut stream = UnixStream::connect(socket_path())
        .context("servizio autonomo non disponibile; controlla gpio-llm-ready.service")?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.set_read_timeout(Some(Duration::from_secs(180)))?;
    serde_json::to_writer(&mut stream, &Request { text: text.to_owned() })?;
    stream.write_all(b"\n")?;
    serde_json::from_str(&read_frame(&stream)?).context("risposta del servizio non valida")
}

pub struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) { let _ = fs::remove_file(&self.0); }
}

fn handle(mut stream: UnixStream, sender: &SyncSender<Event>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let outcome = match read_frame(&stream).and_then(|frame| parse_request(&frame)) {
        Ok(request) => {
            let (reply, response) = mpsc::sync_channel(1);
            match sender.try_send(Event::Cli { text: request.text, reply, queued: Instant::now() }) {
                Ok(()) => response.recv_timeout(Duration::from_secs(160))
                    .unwrap_or_else(|_| Outcome::rejected("Errore LLM: servizio non disponibile o tempo di attesa scaduto")),
                Err(_) => Outcome::rejected("Errore LLM: servizio occupato, riprova"),
            }
        }
        Err(error) => Outcome::rejected(format!("Rifiutato: {error}")),
    };
    serde_json::to_writer(&mut stream, &outcome)?;
    stream.write_all(b"\n")?;
    Ok(())
}

pub fn serve(sender: SyncSender<Event>) -> Result<SocketGuard> {
    let path = socket_path();
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.file_type().is_socket() { bail!("il percorso del socket contiene un file diverso: {}", path.display()); }
        // Called only after acquiring the exclusive hardware lock.
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("socket CLI {}; verificare la directory runtime", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let clients = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            if clients.fetch_add(1, Ordering::AcqRel) >= 16 {
                clients.fetch_sub(1, Ordering::AcqRel);
                drop(stream);
                continue;
            }
            let clients = Arc::clone(&clients);
            let sender = sender.clone();
            thread::spawn(move || {
                if let Err(error) = handle(stream, &sender) { log::debug!("client CLI: {error:#}"); }
                clients.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
    Ok(SocketGuard(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_frames_never_become_commands() {
        assert!(read_frame(&b"{\"text\":\"accendi\"}"[..]).is_err());
        assert!(read_frame(&vec![b'x'; MAX_FRAME + 1][..]).is_err());
        for frame in [r#"{"text":""}"#, r#"{"text":"accendi\nspegni"}"#, r#"{"text":"accendi","pin":17}"#] {
            assert!(parse_request(frame).is_err());
        }
        assert_eq!(parse_request(r#"{"text":"spegni pin 17"}"#).unwrap().text, "spegni pin 17");
    }
    #[test]
    fn cli_wire_reply_preserves_the_existing_message() {
        let response = Outcome { message: "OK: GPIO 17: acceso".into(), executed: true };
        let decoded: Outcome = serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
        assert_eq!(decoded.message, "OK: GPIO 17: acceso");
        assert!(decoded.executed);
    }
}
