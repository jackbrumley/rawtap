//! Favourites: named connections listed at the top of the start screen.
//!
//! A favourite keeps any kind of connection; its view and other options are
//! the connection's own (the start screen's Options). They're saved with Ctrl+S on the start screen or F1 → "Save as favourite…" in a
//! session, edited with F2, removed with Del, and opened from the command
//! line with `rawtap open "<name>"`. They live in state.json.

use super::{App, Screen};
use crate::config::{Connection, Favourite, Saved};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a favourite connects to, e.g. "SSH admin@10.0.0.5:22".
pub fn describe(connection: &Connection) -> String {
    match super::setup::connection_settings(connection) {
        Ok(settings) => settings.describe(),
        Err(_) => format!("{} (incomplete)", connection.mode.label()),
    }
}

pub enum FavouriteDialog {
    Edit(Box<FavouriteForm>),
    /// Del on a favourite: Enter removes it, Esc keeps it.
    Remove(usize),
}

pub struct FavouriteForm {
    /// The favourite being edited; `None` saves a new one.
    pub index: Option<usize>,
    pub connection: Connection,
    pub name: String,
    pub error: Option<String>,
}

/// The favourite with this exact connection, if any.
pub fn find(saved: &Saved, connection: &Connection) -> Option<usize> {
    saved
        .favourites
        .iter()
        .position(|f| &f.connection == connection)
}

/// The favourite called `name` (any case).
pub fn find_by_name(saved: &Saved, name: &str) -> Option<usize> {
    saved
        .favourites
        .iter()
        .position(|f| f.name.eq_ignore_ascii_case(name.trim()))
}

/// `rawtap open "<name>"`: puts the favourite's connection on the start
/// screen's form, or says which names there are.
pub fn open_by_name(saved: &mut Saved, name: &str) -> Result<(), String> {
    if let Some(i) = find_by_name(saved, name) {
        saved.connection = saved.favourites[i].connection.clone();
        return Ok(());
    }
    if saved.favourites.is_empty() {
        return Err(format!(
            "There's no favourite called \"{name}\": there are no favourites yet. Save one on the start screen with Ctrl+S."
        ));
    }
    let names: Vec<String> = saved
        .favourites
        .iter()
        .map(|f| format!("\"{}\"", f.name))
        .collect();
    Err(format!(
        "There's no favourite called \"{name}\". Favourites: {}.",
        names.join(", ")
    ))
}

impl App {
    /// Ctrl+S on the start screen: save what's on the form. A connection
    /// that's already a favourite opens that favourite instead.
    pub(super) fn save_form_as_favourite(&mut self) {
        let connection = self.saved.connection.clone();
        if let Err(error) = super::setup::connection_settings(&connection) {
            self.setup.error = Some(format!("Can't save this as a favourite yet: {error}"));
            return;
        }
        self.open_favourite_form(connection);
    }

    /// F1 → "Save as favourite…": the session's connection.
    pub(crate) fn save_session_as_favourite(&mut self) {
        self.open_favourite_form(self.saved.connection.clone());
    }

    fn open_favourite_form(&mut self, connection: Connection) {
        let form = match find(&self.saved, &connection) {
            Some(i) => {
                let existing = &self.saved.favourites[i];
                FavouriteForm {
                    index: Some(i),
                    name: existing.name.clone(),
                    connection,
                    error: Some(format!(
                        "This connection is already saved as \"{}\".",
                        existing.name
                    )),
                }
            }
            None => FavouriteForm {
                index: None,
                name: describe(&connection),
                connection,
                error: None,
            },
        };
        self.favourite_dialog = Some(FavouriteDialog::Edit(Box::new(form)));
    }

    /// F2 on a favourite.
    pub(super) fn edit_favourite(&mut self, i: usize) {
        let Some(f) = self.saved.favourites.get(i) else {
            return;
        };
        self.favourite_dialog = Some(FavouriteDialog::Edit(Box::new(FavouriteForm {
            index: Some(i),
            connection: f.connection.clone(),
            name: f.name.clone(),
            error: None,
        })));
    }

    /// Del on a favourite.
    pub(super) fn ask_remove_favourite(&mut self, i: usize) {
        if i < self.saved.favourites.len() {
            self.favourite_dialog = Some(FavouriteDialog::Remove(i));
        }
    }

    /// Keys while the dialog is open. Nothing reaches the screen behind it.
    pub(super) fn favourite_dialog_key(&mut self, k: KeyEvent) {
        let Some(dialog) = self.favourite_dialog.as_mut() else {
            return;
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match dialog {
            FavouriteDialog::Remove(i) => {
                let i = *i;
                match k.code {
                    KeyCode::Enter => self.remove_favourite(i),
                    KeyCode::Esc => self.favourite_dialog = None,
                    _ => {}
                }
            }
            FavouriteDialog::Edit(form) => match k.code {
                KeyCode::Esc => self.favourite_dialog = None,
                KeyCode::Enter => self.save_favourite_form(),
                KeyCode::Backspace => {
                    form.name.pop();
                }
                KeyCode::Char(c) if !ctrl || k.modifiers.contains(KeyModifiers::ALT) => {
                    form.name.push(c);
                }
                _ => {}
            },
        }
    }

    /// A paste while the dialog is open goes into the name.
    pub(super) fn favourite_dialog_paste(&mut self, text: &str) {
        if let Some(FavouriteDialog::Edit(form)) = self.favourite_dialog.as_mut() {
            form.name.push_str(text.lines().next().unwrap_or("").trim());
        }
    }

    fn save_favourite_form(&mut self) {
        let Some(FavouriteDialog::Edit(form)) = self.favourite_dialog.as_mut() else {
            return;
        };
        let name = form.name.trim().to_string();
        if name.is_empty() {
            form.error = Some("Give the favourite a name.".into());
            return;
        }
        if let Some(other) = find_by_name(&self.saved, &name).filter(|&i| Some(i) != form.index) {
            form.error = Some(format!(
                "There's already a favourite called \"{}\".",
                self.saved.favourites[other].name
            ));
            return;
        }
        let saved = Favourite {
            name: name.clone(),
            connection: form.connection.clone(),
        };
        let index = match form.index {
            Some(i) => {
                self.saved.favourites[i] = saved;
                i
            }
            None => {
                self.saved.favourites.push(saved);
                self.saved.favourites.len() - 1
            }
        };
        self.favourite_dialog = None;
        log::info!("Saved favourite \"{name}\"");
        match self.screen {
            Screen::Setup => self.setup.focus = index,
            Screen::Monitor => {
                if let Some(m) = self.monitor.as_mut() {
                    m.set_status(format!("Saved favourite \"{name}\""), false);
                }
            }
        }
        self.store_state_reporting();
    }

    fn remove_favourite(&mut self, i: usize) {
        self.favourite_dialog = None;
        if i >= self.saved.favourites.len() {
            return;
        }
        let removed = self.saved.favourites.remove(i);
        log::info!("Removed favourite \"{}\"", removed.name);
        self.setup.focus = self.setup.focus.min(self.saved.favourites.len());
        self.store_state_reporting();
    }

    /// Where the start screen's highlight starts: on the favourite that
    /// matches the last connection, else the form's first field.
    pub(super) fn home_focus(&mut self) {
        self.setup.focus =
            find(&self.saved, &self.saved.connection).unwrap_or(self.saved.favourites.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConnMode, Layout};

    fn ssh(host: &str) -> Connection {
        Connection {
            mode: ConnMode::Ssh,
            ssh_host: host.into(),
            ..Default::default()
        }
    }

    #[test]
    fn open_by_name_ignores_case_and_lists_names_when_missing() {
        let mut saved = Saved {
            favourites: vec![Favourite {
                name: "Core switch".into(),
                connection: ssh("10.0.0.5"),
            }],
            ..Default::default()
        };
        open_by_name(&mut saved, "core SWITCH").unwrap();
        assert_eq!(saved.connection.ssh_host, "10.0.0.5");
        let error = open_by_name(&mut saved, "printer").unwrap_err();
        assert!(error.contains("\"Core switch\""), "{error}");
    }

    #[test]
    fn state_json_from_before_favourites_still_loads() {
        let old = r#"{"mode":"tcp","tcp_host":"10.1.1.1","tcp_port":"4001","view":"terminal"}"#;
        let saved: Saved = serde_json::from_str(old).unwrap();
        assert_eq!(saved.connection.mode, ConnMode::Tcp);
        assert_eq!(saved.connection.tcp_host, "10.1.1.1");
        assert_eq!(saved.connection.baud, "9600");
        assert_eq!(saved.view, Layout::Terminal);
        assert!(saved.favourites.is_empty());
        let round = serde_json::to_string(&saved).unwrap();
        assert!(round.contains(r#""tcp_host":"10.1.1.1""#), "{round}");
    }
}
