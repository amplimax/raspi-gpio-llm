//! gpio-llm: comando in linguaggio naturale -> LLM locale -> validazione -> GPIO.
//!
//! L'LLM produce solo un JSON (`RawCommand`); il validatore lo trasforma in `Action`
//! oppure lo rifiuta; solo le `Action` raggiungono l'esecutore.

mod command;
mod controller;
mod config;
mod executor;
mod llm;
mod startup;
mod status;
mod validator;
mod voice;
mod voice_intent;
mod voice_numbers;
mod ownership;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod ipc;

use std::{
    io::{self, BufRead, Write},
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::{bail, Context, Result};
use log::{info, warn};

use crate::{config::Config, executor::Executor, llm::LlmClient};

fn lock(exec: &Mutex<Executor>) -> MutexGuard<'_, Executor> {
    exec.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn process(text: &str, cfg: &Config, llm: &LlmClient, exec: &Mutex<Executor>) {
    println!("{}", controller::execute(text, cfg, llm, exec).message);
}

fn repl(mut process: impl FnMut(&str)) -> Result<()> {
    println!("Scrivi un comando (\"esci\" per terminare).");

    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("> ");
        io::stdout().flush()?;

        line.clear();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        if text.eq_ignore_ascii_case("esci") {
            break;
        }
        process(text);
    }
    Ok(())
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (warmup_only, voice_mode, daemon_mode) = match args.as_slice() {
        [] => (false, false, false),
        [flag] if flag == "--warmup-only" => (true, false, false),
        [flag] if flag == "--voice" => (false, true, false),
        [flag] if flag == "--daemon" => (false, false, true),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("Uso: gpio-llm [--warmup-only | --voice | --daemon]");
            println!("Senza argomenti: CLI interattiva.");
            println!("--warmup-only: prova il modello, segnala con 5 lampeggi e termina.");
            println!("--voice: premi Invio per acquisire e interpretare una frase italiana.");
            println!("--daemon: warmup e ascolto autonomo di ASCOLTA e della CLI.");
            return Ok(());
        }
        _ => bail!("argomenti non riconosciuti; usa --help"),
    };

    let cfg = Config::from_env().context("configurazione non valida")?;
    info!("policy: {:?}", cfg.policy);
    info!("llm: {}", cfg.llm_url);

    #[cfg(unix)]
    if !warmup_only && !voice_mode && !daemon_mode && ipc::use_service() {
        return repl(|text| match ipc::request(text) {
            Ok(result) => println!("{}", result.message),
            Err(error) => println!("Errore LLM: {error:#}"),
        });
    }

    let _hardware_owner = ownership::acquire()?;

    let llm = LlmClient::new(cfg.llm_url.clone());
    let exec = Arc::new(Mutex::new(Executor::new()?));
    let status_led = Arc::new(Mutex::new(if warmup_only || daemon_mode {
        Some(status::StatusLed::new()?)
    } else { None }));
    let voice_worker: voice::SharedWorker = Arc::new(Mutex::new(None));
    let shutdown = Arc::new(Mutex::new(()));

    // Ctrl+C / SIGTERM: porta i pin a livello basso prima di uscire.
    {
        let exec = Arc::clone(&exec);
        let voice_worker = Arc::clone(&voice_worker);
        let shutdown = Arc::clone(&shutdown);
        let status_led = Arc::clone(&status_led);
        ctrlc::set_handler(move || {
            let _shutdown = shutdown.lock().unwrap_or_else(|e| e.into_inner());
            warn!("terminazione richiesta: spengo i GPIO");
            lock(&exec).halt_all();
            status::clear(&status_led);
            voice::stop_worker(&voice_worker);
            std::process::exit(130);
        })
        .context("impossibile installare il gestore di terminazione")?;
    }

    let result = if warmup_only {
        startup::warmup(&llm, &cfg.policy).and_then(|()| {
            status::signal(&status_led)?;
            info!("PRONTO: warmup verificato, 5 lampeggi di stato completati su BCM{}", status::PIN);
            Ok(())
        })
    } else if daemon_mode {
        #[cfg(unix)]
        { daemon::run(&cfg, &llm, &exec, &voice_worker, &status_led) }
        #[cfg(not(unix))]
        { bail!("la modalità autonoma richiede Linux") }
    } else if voice_mode {
        voice::repl(&voice_worker, |text| process(text, &cfg, &llm, &exec))
    } else {
        repl(|text| process(text, &cfg, &llm, &exec))
    };
    // Stopping STT closes its output pipe. Wait for an active signal handler
    // to finish instead of racing that EOF to a premature error exit.
    let _shutdown = shutdown.lock().unwrap_or_else(|e| e.into_inner());
    lock(&exec).halt_all();
    status::clear(&status_led);
    result
}
