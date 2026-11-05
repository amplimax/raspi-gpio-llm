//! Exclusive hardware ownership across daemon, legacy CLI and manual voice.

use std::{fs::{File, OpenOptions}, path::PathBuf};
use anyhow::{Context, Result};

pub fn acquire() -> Result<File> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".gpio-llm.lock");
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(path).context("lock di controllo GPIO")?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // The open File owns this lock until process exit, including signals.
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            return Err(std::io::Error::last_os_error()).context(
                "GPIO già gestiti da un'altra istanza; usa la CLI del servizio o chiudi l'istanza manuale"
            );
        }
    }
    Ok(file)
}
