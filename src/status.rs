//! Dedicated notification output. It never acquires an actuation pin.

use std::{sync::Mutex, thread, time::Duration};
use anyhow::{Context, Result};
use rppal::gpio::{Gpio, OutputPin};

pub const PIN: u8 = 23;

pub struct StatusLed {
    output: OutputPin,
}

impl StatusLed {
    pub fn new() -> Result<Self> {
        let output = Gpio::new().context("GPIO per il LED di stato")?
            .get(PIN).context("BCM23 riservato al LED di stato non disponibile")?
            .into_output_low();
        Ok(Self { output })
    }

    pub fn signal(&mut self) {
        self.blink(5);
    }

    fn blink(&mut self, count: usize) {
        self.output.set_low();
        // Separate feedback from a preceding solid listening light, including
        // timeouts and cached commands with no perceptible processing delay.
        thread::sleep(Duration::from_millis(200));
        for _ in 0..count {
            self.output.set_high();
            thread::sleep(Duration::from_millis(100));
            self.output.set_low();
            thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn listening(&mut self) { self.output.set_high(); }

    pub fn failure(&mut self) { self.blink(2); }

    pub fn clear(&mut self) { self.output.set_low(); }
}

impl Drop for StatusLed {
    fn drop(&mut self) { self.clear(); }
}

pub type SharedStatus = Mutex<Option<StatusLed>>;

pub fn signal(led: &SharedStatus) -> Result<()> {
    let mut led = led.lock().unwrap_or_else(|e| e.into_inner());
    led.as_mut().context("LED di stato non inizializzato")?.signal();
    Ok(())
}

pub fn listening(led: &SharedStatus) -> Result<()> {
    let mut led = led.lock().unwrap_or_else(|e| e.into_inner());
    led.as_mut().context("LED di stato non inizializzato")?.listening();
    Ok(())
}

pub fn failure(led: &SharedStatus) -> Result<()> {
    let mut led = led.lock().unwrap_or_else(|e| e.into_inner());
    led.as_mut().context("LED di stato non inizializzato")?.failure();
    Ok(())
}

pub fn clear(led: &SharedStatus) {
    if let Some(led) = led.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        led.clear();
    }
}
