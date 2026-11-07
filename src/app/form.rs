//! The start screen's form: keys, typing into fields, cycling choices and
//! what each field shows. Moving onto a favourite fills the form.

use super::App;
use super::setup::{
    BAUDS, DATA_BITS, FLOWS, Field, PARITIES, Row, STOP_BITS, Setup, connection_profile_key,
    cycle_in,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    pub(super) fn setup_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // While connecting the form waits: Esc cancels, Ctrl+Q still quits.
        if self.connecting.is_some() {
            match k.code {
                KeyCode::Esc => self.cancel_connect(),
                KeyCode::Char('q') if ctrl => self.quit = true,
                _ => {}
            }
            return;
        }
        let n = Setup::rows(&self.saved, self.setup.options_open).len();
        let Some(row) = self.focused_row() else {
            return;
        };
        let field = match row {
            Row::Field(field) => Some(field),
            Row::Favourite(_) => None,
        };
        match (k.code, row) {
            (KeyCode::Char('q'), _) if ctrl => self.quit = true,
            (KeyCode::Char('u'), _) if ctrl => self.request_update(),
            (KeyCode::Char('s'), _) if ctrl => self.save_form_as_favourite(),
            (KeyCode::Up, _) => self.move_focus((self.setup.focus + n - 1) % n),
            (KeyCode::Down, _) => self.move_focus((self.setup.focus + 1) % n),
            (KeyCode::Left, Row::Field(field)) => self.cycle(field, -1),
            (KeyCode::Right, Row::Field(field)) => self.cycle(field, 1),
            (KeyCode::Enter, Row::Field(Field::Options)) => self.cycle(Field::Options, 1),
            (KeyCode::Enter, _) => self.request_connect(),
            (KeyCode::F(2), Row::Favourite(i)) => self.edit_favourite(i),
            (KeyCode::Delete, Row::Favourite(i)) => self.ask_remove_favourite(i),
            (KeyCode::F(5), _) => self.rescan_ports(),
            (KeyCode::Backspace, _) => {
                if let Some(t) = field.and_then(|f| self.text_mut(f)) {
                    t.pop();
                }
            }
            (KeyCode::Char(c), _) if !ctrl || k.modifiers.contains(KeyModifiers::ALT) => {
                if let Some(t) = field.and_then(|f| self.text_mut(f)) {
                    t.push(c);
                }
            }
            _ => {}
        }
    }

    /// The start screen's highlighted row (focus clamped to the rows there are).
    pub fn focused_row(&mut self) -> Option<Row> {
        let rows = Setup::rows(&self.saved, self.setup.options_open);
        self.setup.focus = self.setup.focus.min(rows.len().saturating_sub(1));
        rows.get(self.setup.focus).copied()
    }

    /// Moving onto a favourite puts its connection on the form, so Enter
    /// connects to it and the form below shows what it is.
    fn move_focus(&mut self, focus: usize) {
        self.setup.focus = focus;
        if let Some(Row::Favourite(i)) = self.focused_row() {
            self.saved.connection = self.saved.favourites[i].connection.clone();
            self.setup.error = None;
        }
    }

    pub(super) fn text_mut(&mut self, f: Field) -> Option<&mut String> {
        let s = &mut self.saved;
        Some(match f {
            Field::SerialPort => &mut s.connection.serial_port,
            Field::Baud => &mut s.connection.baud,
            Field::Host => &mut s.connection.tcp_host,
            Field::TcpPort => &mut s.connection.tcp_port,
            Field::Bind => &mut s.connection.listen_bind,
            Field::ListenPort => &mut s.connection.listen_port,
            Field::TelnetHost => &mut s.connection.telnet_host,
            Field::TelnetPort => &mut s.connection.telnet_port,
            Field::SshHost => &mut s.connection.ssh_host,
            Field::SshUser => &mut s.connection.ssh_user,
            Field::SshPort => &mut s.connection.ssh_port,
            _ => return None,
        })
    }

    fn cycle(&mut self, f: Field, dir: i32) {
        let s = &mut self.saved;
        match f {
            Field::Mode => s.connection.mode = s.connection.mode.cycle(dir),
            Field::SerialPort => {
                let names: Vec<&str> = self.setup.ports.iter().map(|p| p.name.as_str()).collect();
                if !names.is_empty() {
                    s.connection.serial_port = cycle_in(&names, &s.connection.serial_port, dir);
                }
            }
            Field::Baud => s.connection.baud = cycle_in(BAUDS, &s.connection.baud, dir),
            Field::DataBits => {
                s.connection.data_bits = cycle_in(DATA_BITS, &s.connection.data_bits, dir)
            }
            Field::Parity => s.connection.parity = cycle_in(PARITIES, &s.connection.parity, dir),
            Field::StopBits => {
                s.connection.stop_bits = cycle_in(STOP_BITS, &s.connection.stop_bits, dir)
            }
            Field::Flow => s.connection.flow = cycle_in(FLOWS, &s.connection.flow, dir),
            Field::Logging => s.logging = !s.logging,
            Field::Options => self.setup.options_open = !self.setup.options_open,
            // The view is one setting for every connection, not per profile.
            Field::OptLineEnding | Field::OptCharDelay | Field::OptSendMode | Field::OptLayout => {
                use crate::codec::LineEnding as L;
                let key = connection_profile_key(&s.connection);
                let p = s.profiles.entry(key).or_default();
                match f {
                    Field::OptLineEnding => {
                        p.line_ending =
                            crate::menu::step(&[L::None, L::Cr, L::Lf, L::Crlf], p.line_ending, dir)
                    }
                    Field::OptCharDelay => {
                        p.char_delay_ms =
                            crate::menu::step(&crate::menu::CHAR_DELAYS, p.char_delay_ms, dir)
                    }
                    Field::OptLayout => {
                        p.view = crate::menu::step(&crate::config::VIEW_CHOICES, p.view, dir)
                    }
                    _ => {
                        p.input_mode =
                            crate::menu::step(&crate::menu::input_modes(), p.input_mode, dir)
                    }
                }
            }
            Field::Host
            | Field::TcpPort
            | Field::Bind
            | Field::ListenPort
            | Field::TelnetHost
            | Field::TelnetPort
            | Field::SshHost
            | Field::SshUser
            | Field::SshPort => {}
        }
    }

    /// The view a new session opens in: the connection's own, or the last
    /// one used.
    pub(crate) fn view_for_connection(&self) -> crate::config::Layout {
        self.form_profile().view.unwrap_or(self.saved.view)
    }

    /// The remembered profile for what's typed in the form (defaults if none yet).
    pub fn form_profile(&self) -> crate::config::Profile {
        self.saved
            .profiles
            .get(&connection_profile_key(&self.saved.connection))
            .cloned()
            .unwrap_or_default()
    }

    pub fn field_value(&self, f: Field) -> String {
        let s = &self.saved;
        match f {
            Field::Mode => s.connection.mode.label().to_string(),
            Field::SerialPort => s.connection.serial_port.clone(),
            Field::Baud => s.connection.baud.clone(),
            Field::DataBits => s.connection.data_bits.clone(),
            Field::Parity => s.connection.parity.clone(),
            Field::StopBits => s.connection.stop_bits.clone(),
            Field::Flow => s.connection.flow.clone(),
            Field::Host => s.connection.tcp_host.clone(),
            Field::TcpPort => s.connection.tcp_port.clone(),
            Field::Bind => s.connection.listen_bind.clone(),
            Field::ListenPort => s.connection.listen_port.clone(),
            Field::TelnetHost => s.connection.telnet_host.clone(),
            Field::TelnetPort => s.connection.telnet_port.clone(),
            Field::SshHost => s.connection.ssh_host.clone(),
            Field::SshUser => s.connection.ssh_user.clone(),
            Field::SshPort => s.connection.ssh_port.clone(),
            Field::Logging => {
                if s.logging {
                    "Yes".into()
                } else {
                    "No".into()
                }
            }
            Field::Options => {
                let p = self.form_profile();
                let delay = if p.char_delay_ms == 0 {
                    "no delay".to_string()
                } else {
                    format!("{} ms delay", p.char_delay_ms)
                };
                let arrow = if self.setup.options_open {
                    "▾"
                } else {
                    "▸"
                };
                format!(
                    "{arrow} {} · {delay} · {} · {}",
                    p.line_ending.label(),
                    p.input_mode.label(),
                    crate::config::view_choice_label(p.view)
                )
            }
            Field::OptLineEnding => self.form_profile().line_ending.label().into(),
            Field::OptCharDelay => match self.form_profile().char_delay_ms {
                0 => "off".into(),
                ms => format!("{ms} ms"),
            },
            Field::OptSendMode => self.form_profile().input_mode.label().into(),
            Field::OptLayout => crate::config::view_choice_label(self.form_profile().view).into(),
        }
    }
}
