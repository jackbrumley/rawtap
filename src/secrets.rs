//! Remembered passwords, kept only in the operating system's keyring: the
//! Secret Service on Linux (GNOME Keyring, KWallet) and Windows Credential
//! Manager. Never in RawTap's own files.
//!
//! Each password is stored under the app ID with the connection's profile
//! key (`ssh:admin@10.0.0.5:22`, `serial:COM3`...) as the account. The
//! keyring can be slow (a D-Bus service starting, an unlock prompt), so
//! everything here runs off the draw loop: `init` checks availability on a
//! background thread, and the monitor reads on its own thread.

use crate::app::identity::APP_ID;
use keyring_core::{Entry, Error};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Availability {
    Checking,
    Available,
    /// Why there's no keyring (logged once; the menu then offers nothing).
    Unavailable(String),
}

static AVAILABILITY: Mutex<Availability> = Mutex::new(Availability::Checking);

/// The keyring's name, for messages.
pub fn store_name() -> &'static str {
    if cfg!(windows) {
        "Windows Credential Manager"
    } else {
        "your keyring"
    }
}

pub fn availability() -> Availability {
    AVAILABILITY
        .lock()
        .map(|a| a.clone())
        .unwrap_or_else(|_| Availability::Unavailable("internal error".into()))
}

fn set_availability(a: Availability) {
    if let Ok(mut guard) = AVAILABILITY.lock() {
        *guard = a;
    }
}

/// Opens the platform's keyring and checks it answers, in the background.
pub fn init() {
    std::thread::spawn(|| {
        let result = open_store().and_then(|()| probe());
        match result {
            Ok(()) => {
                log::info!(
                    "Keyring available: {}",
                    if cfg!(windows) {
                        "Windows Credential Manager"
                    } else {
                        "Secret Service"
                    }
                );
                set_availability(Availability::Available);
            }
            Err(reason) => {
                log::warn!("No keyring, so Remember password is off: {reason}");
                set_availability(Availability::Unavailable(reason));
            }
        }
    });
}

#[cfg(target_os = "linux")]
fn open_store() -> Result<(), String> {
    let store = zbus_secret_service_keyring_store::Store::new().map_err(describe)?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(windows)]
fn open_store() -> Result<(), String> {
    let store = windows_native_keyring_store::Store::new().map_err(describe)?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn open_store() -> Result<(), String> {
    Err("not supported on this platform".into())
}

/// Reads an entry that doesn't exist: "no entry" means the keyring works.
fn probe() -> Result<(), String> {
    match entry("rawtap-availability-check")?.get_password() {
        Ok(_) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(describe(error)),
    }
}

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(APP_ID, account).map_err(describe)
}

fn describe(error: Error) -> String {
    match error {
        Error::NoStorageAccess(e) => format!("the keyring can't be reached ({e})"),
        Error::PlatformFailure(e) => format!("the keyring failed ({e})"),
        other => other.to_string(),
    }
}

pub fn remember(account: &str, password: &str) -> Result<(), String> {
    entry(account)?.set_password(password).map_err(describe)
}

/// The remembered password, or `None` when there isn't one.
pub fn read(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(describe(error)),
    }
}

/// Removes the remembered password. Forgetting one that isn't there is fine.
pub fn forget(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(describe(error)),
    }
}

/// `read` on a background thread; `None` when there's no keyring.
pub fn read_in_background(account: &str) -> Option<Receiver<Result<Option<String>, String>>> {
    if availability() != Availability::Available {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let account = account.to_string();
    std::thread::spawn(move || {
        // Nobody listening means the session closed first; nothing to do.
        let _ = tx.send(read(&account));
    });
    Some(rx)
}

/// Tests share one in-memory keyring.
#[cfg(test)]
pub fn use_test_store() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        set_availability(Availability::Available);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_read_and_forget() {
        use_test_store();
        let account = "ssh:test@secrets-unit:22";
        assert_eq!(read(account).unwrap(), None);
        remember(account, "hunter2").unwrap();
        assert_eq!(read(account).unwrap().as_deref(), Some("hunter2"));
        forget(account).unwrap();
        assert_eq!(read(account).unwrap(), None);
        forget(account).unwrap();
    }
}
