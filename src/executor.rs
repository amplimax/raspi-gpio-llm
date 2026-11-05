//! Esecutore GPIO: un thread per pin, interrompibile. Accetta solo `Action` validate.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use log::debug;
use rppal::gpio::Gpio;
use thiserror::Error;

use crate::command::Action;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("GPIO non disponibile su questo sistema: {0}")]
    Init(rppal::gpio::Error),
    #[error("GPIO {pin} non accessibile: {cause}")]
    Pin { pin: u8, cause: rppal::gpio::Error },
}

#[derive(Clone, Copy)]
enum Mode {
    Blink(Duration),
    On,
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

pub struct Executor {
    gpio: Gpio,
    workers: HashMap<u8, Worker>,
}

/// Dorme per `total`, ma esce subito se `stop` viene impostato.
fn sleep_unless_stopped(total: Duration, stop: &AtomicBool) {
    let step = Duration::from_millis(50);
    let mut left = total;
    while left > Duration::ZERO && !stop.load(Ordering::Relaxed) {
        let s = left.min(step);
        thread::sleep(s);
        left -= s;
    }
}

impl Executor {
    pub fn new() -> Result<Self, ExecError> {
        Ok(Self {
            gpio: Gpio::new().map_err(ExecError::Init)?,
            workers: HashMap::new(),
        })
    }

    pub fn run(&mut self, action: &Action) -> Result<(), ExecError> {
        match *action {
            Action::Blink { pin, half_period } => self.start(pin, Mode::Blink(half_period)),
            Action::On { pin } => self.start(pin, Mode::On),
            Action::Halt { pin } => {
                self.halt(pin);
                Ok(())
            }
        }
    }

    /// Ferma tutti i worker e porta i pin a livello basso.
    pub fn halt_all(&mut self) {
        let pins: Vec<u8> = self.workers.keys().copied().collect();
        for pin in pins {
            self.halt(pin);
        }
    }

    fn halt(&mut self, pin: u8) {
        if let Some(w) = self.workers.remove(&pin) {
            w.stop.store(true, Ordering::Relaxed);
            let _ = w.handle.join();
            debug!("GPIO {pin}: worker fermato");
        }
    }

    fn start(&mut self, pin: u8, mode: Mode) -> Result<(), ExecError> {
        // Un solo worker per pin: ferma quello precedente prima di riprendere il pin.
        self.halt(pin);

        let mut out = self
            .gpio
            .get(pin)
            .map_err(|cause| ExecError::Pin { pin, cause })?
            .into_output_low();

        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            match mode {
                Mode::Blink(half) => {
                    while !flag.load(Ordering::Relaxed) {
                        out.toggle();
                        sleep_unless_stopped(half, &flag);
                    }
                }
                Mode::On => {
                    out.set_high();
                    while !flag.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
            out.set_low();
        });

        debug!("GPIO {pin}: worker avviato");
        self.workers.insert(pin, Worker { stop, handle });
        Ok(())
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.halt_all();
    }
}
