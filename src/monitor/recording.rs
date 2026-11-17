//! The session log: turning it on and off, naming it, and opening the log,
//! its transcript and their folder (F1 → Session log, or a click on the
//! header's ● REC badge).

use super::*;

/// The longest name a session log can be given.
const MAX_NAME: usize = 60;

impl Monitor {
    /// F1 → Log to file: starts or stops this session's log. The choice is
    /// remembered for the connection.
    pub(super) fn toggle_logging(&mut self) {
        self.flush_pending();
        if self.logger.is_some() {
            self.push(EntryKind::Info("Logging stopped for this session".into()));
            self.logger = None;
            self.transcript = None;
            self.profile.logging = Some(false);
            self.set_status("Logging off for this connection", false);
            return;
        }
        let Some(dir) = self.log_dir.clone() else {
            self.set_status("Logging is disabled (started with --no-log)", true);
            return;
        };
        match Logger::create(&dir, self.settings.kind(), &self.settings.describe()) {
            Ok(l) => {
                self.transcript = match Transcript::create(&l.path) {
                    Ok(t) => Some(t),
                    Err(error) => {
                        self.report_error(format!("Could not create the transcript: {error}"));
                        None
                    }
                };
                let p = l.path.display().to_string();
                self.logger = Some(l);
                self.profile.logging = Some(true);
                self.push(EntryKind::Info(format!("Logging to {p}")));
                self.set_status("Logging on for this connection", false);
            }
            Err(e) => self.set_status(format!("Couldn't start logging: {e}"), true),
        }
    }

    /// F1 → Open session log: the hex log, in the desktop's text app.
    pub(super) fn open_session_log(&mut self) {
        match self.logger.as_ref().map(|l| l.path.clone()) {
            Some(path) => self.open_reporting(&path, "the session log"),
            None => self.not_logging(),
        }
    }

    /// F1 → Open transcript: the plain text the device printed.
    pub(super) fn open_transcript(&mut self) {
        match self.transcript.as_ref().map(|t| t.path.clone()) {
            Some(path) => self.open_reporting(&path, "the transcript"),
            None => self.not_logging(),
        }
    }

    /// F1 → Open session logs folder, or a click on ● REC: every session's
    /// logs, in the file manager.
    pub fn open_logs_folder(&mut self) {
        let Some(dir) = self.log_dir.clone() else {
            self.set_status("Logging is disabled (started with --no-log)", true);
            return;
        };
        // Opened before the first log is written, it may not exist yet.
        if let Err(error) = std::fs::create_dir_all(&dir) {
            self.report_error(format!("Could not create {}: {error}", dir.display()));
            return;
        }
        self.open_reporting(&dir, "the session logs folder");
    }

    /// F1 → Copy session log path, for a command line or a ticket.
    pub(super) fn copy_log_path(&mut self) {
        let Some(path) = self.logger.as_ref().map(|l| l.path.display().to_string()) else {
            return self.not_logging();
        };
        match crate::clip::set(&path) {
            Ok(()) => self.set_status(format!("Copied {path}"), false),
            Err(error) => self.report_error(error),
        }
    }

    /// F1 → Name this session log: the send box asks for the name.
    pub(super) fn start_naming_log(&mut self) {
        if self.logger.is_none() {
            return self.not_logging();
        }
        self.log_name = Some(String::new());
    }

    /// Keys while the name is being typed: Enter renames, Esc keeps the
    /// name the log has.
    pub(super) fn log_name_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let Some(name) = self.log_name.as_mut() else {
            return;
        };
        match k.code {
            KeyCode::Enter => {
                let name = self.log_name.take().unwrap_or_default();
                self.name_log(&name);
            }
            KeyCode::Esc => {
                self.log_name = None;
                self.set_status("The session log keeps its name", false);
            }
            KeyCode::Backspace => {
                name.pop();
            }
            KeyCode::Char(c) if (!ctrl || alt) && name.chars().count() < MAX_NAME => {
                name.push(c);
            }
            _ => {}
        }
    }

    /// Renames the log and its transcript; the name keeps letters, digits,
    /// '-' and '_', and spaces become '-'.
    fn name_log(&mut self, typed: &str) {
        let name = file_name_part(typed);
        if name.is_empty() {
            self.set_status("The session log keeps its name", false);
            return;
        }
        let Some(logger) = self.logger.as_mut() else {
            return self.not_logging();
        };
        if let Err(error) = logger.rename(&name) {
            return self.report_error(format!("Could not rename the session log: {error}"));
        }
        let path = logger.path.clone();
        if let Some(transcript) = self.transcript.as_mut()
            && let Err(error) = transcript.rename_with(&path)
        {
            return self.report_error(format!("Could not rename the transcript: {error}"));
        }
        log::info!("Session log renamed to {}", path.display());
        self.set_status(format!("Session log: {}", path.display()), false);
    }

    fn open_reporting(&mut self, path: &std::path::Path, what: &str) {
        match crate::opener::open(path) {
            Ok(()) => self.set_status(format!("Opened {what}"), false),
            Err(error) => self.report_error(error),
        }
    }

    fn not_logging(&mut self) {
        self.set_status(
            "This session isn't logged. F1 → Session log → Log to file starts it.",
            true,
        );
    }
}

/// What a typed name becomes in a file name.
fn file_name_part(typed: &str) -> String {
    typed
        .trim()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::file_name_part;

    #[test]
    fn a_typed_name_is_safe_in_a_file_name() {
        assert_eq!(
            file_name_part(" Core switch: config/backup "),
            "Core-switch-configbackup"
        );
        assert_eq!(file_name_part("../.."), "");
    }
}
