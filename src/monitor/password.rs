//! Remembered passwords and SSH key login, from the session's side.
//!
//! A remembered password is read from the OS keyring (`crate::secrets`) on a
//! background thread each time a connection opens, and sent once, hidden, at
//! that connection's first login password prompt. A wrong password isn't
//! sent again: the next prompt is the user's. Saving and forgetting also run
//! off the draw loop, since the keyring may ask to be unlocked.
//!
//! "Set up key login" (SSH only) makes a key with ssh-keygen when there isn't
//! one, then types a command into the logged-in remote shell that adds the
//! public key to `~/.ssh/authorized_keys`, so the next login needs no password.

use super::{EntryKind, Monitor};
use crate::link::LinkSettings;
use crate::secrets;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// What the hidden input box is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SecretUse {
    /// F6: send it now.
    #[default]
    Send,
    /// F1 → Remember password: keep it in the keyring.
    Remember,
}

#[derive(Default)]
pub struct Remembered {
    /// The keyring read started when the connection opened.
    reading: Option<Receiver<Result<Option<String>, String>>>,
    /// Held until the first login password prompt, then dropped.
    password: Option<String>,
    /// Whether the keyring has a password for this connection; `None` until
    /// the read finishes, or when there's no keyring.
    pub stored: Option<bool>,
    sent: bool,
    /// A save or forget running in the background: its outcome for the
    /// status line, and what `stored` becomes when it succeeds.
    change: Option<(Receiver<Result<String, String>>, bool)>,
}

impl Monitor {
    /// A connection opened: read its remembered password, if there's a
    /// keyring.
    pub(super) fn load_password(&mut self) {
        self.remembered = Remembered {
            reading: secrets::read_in_background(&self.profile_key),
            ..Remembered::default()
        };
    }

    /// Every frame: collect keyring results, and send the remembered password
    /// at the first login prompt.
    pub(super) fn poll_password(&mut self) {
        if let Some(reading) = &self.remembered.reading {
            match reading.try_recv() {
                Ok(Ok(password)) => {
                    self.remembered.stored = Some(password.is_some());
                    self.remembered.password = password;
                    self.remembered.reading = None;
                }
                Ok(Err(error)) => {
                    self.remembered.reading = None;
                    self.report_error(format!("Could not read the remembered password: {error}"));
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.remembered.reading = None,
            }
        }
        if let Some((change, stored)) = &self.remembered.change {
            let stored = *stored;
            match change.try_recv() {
                Ok(Ok(message)) => {
                    self.remembered.change = None;
                    self.remembered.stored = Some(stored);
                    self.set_status(message, false);
                }
                Ok(Err(error)) => {
                    self.remembered.change = None;
                    self.report_error(error);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.remembered.change = None,
            }
        }
        if !self.remembered.sent
            && self.remembered.password.is_some()
            && self.link.is_some()
            && self.secret.is_none()
            && self.at_login_password_prompt()
            && let Some(password) = self.remembered.password.take()
        {
            self.remembered.sent = true;
            let mut bytes = password.into_bytes();
            bytes.extend_from_slice(self.profile.line_ending.bytes());
            if self.send_bytes(bytes, true) {
                self.push(EntryKind::Info(
                    "Sent the remembered password (hidden). If it's wrong, type it at the next prompt, then F1 → Forget password.".into(),
                ));
            }
        }
    }

    fn at_login_password_prompt(&self) -> bool {
        self.term.at_login_password_prompt()
            || crate::term::asks_for_login_password(&self.plain.partial())
    }

    /// The menu offers Remember password (`Some(false)`) or Forget password
    /// (`Some(true)`); `None`: neither (no keyring, or still reading).
    pub fn password_stored(&self) -> Option<bool> {
        if self.remembered.change.is_some() {
            return None;
        }
        self.remembered.stored
    }

    pub(super) fn remember_password(&mut self, password: String) {
        if password.is_empty() {
            self.set_status("Nothing to remember: the password was empty", true);
            return;
        }
        let account = self.profile_key.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = secrets::remember(&account, &password)
                .map(|()| {
                    format!(
                        "Password remembered in {}. RawTap sends it at the first password prompt of each connection.",
                        secrets::store_name()
                    )
                })
                .map_err(|e| format!("Could not remember the password: {e}"));
            // Nobody listening means the session closed; the save still happened.
            let _ = tx.send(outcome);
        });
        self.remembered.change = Some((rx, true));
        self.set_status("Saving the password…", false);
    }

    pub(super) fn forget_password(&mut self) {
        let account = self.profile_key.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = secrets::forget(&account)
                .map(|()| format!("Forgot the password for {account}."))
                .map_err(|e| format!("Could not forget the password: {e}"));
            // Nobody listening means the session closed; the delete still happened.
            let _ = tx.send(outcome);
        });
        self.remembered.password = None;
        self.remembered.change = Some((rx, false));
        self.set_status("Forgetting the password…", false);
    }

    /// F1 → Set up key login (SSH sessions, once logged in).
    pub(super) fn set_up_key_login(&mut self) {
        let LinkSettings::Ssh { host, user, .. } = &self.settings else {
            self.set_status("Key login is for SSH connections", true);
            return;
        };
        let comment = match user.as_str() {
            "" => format!("RawTap for {host}"),
            user => format!("RawTap for {user}@{host}"),
        };
        let key = match crate::ssh_key::public_key_or_create(&comment) {
            Ok((key, created)) => {
                if let Some(path) = created {
                    self.push(EntryKind::Info(format!(
                        "Made a new SSH key with no passphrase: {}. Add one any time with ssh-keygen -p.",
                        path.display()
                    )));
                }
                key
            }
            Err(error) => return self.report_error(error),
        };
        let command = match crate::ssh_key::authorize_command(&key) {
            Ok(command) => command,
            Err(error) => return self.report_error(error),
        };
        let mut bytes = command.into_bytes();
        bytes.push(b'\r');
        if self.send_bytes(bytes, false) {
            self.push(EntryKind::Info(
                "Adding your public key to ~/.ssh/authorized_keys on the server. Next time, ssh logs in with the key.".into(),
            ));
        }
    }
}
