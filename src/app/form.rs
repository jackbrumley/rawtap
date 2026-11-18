//! The start screen's keys: the list (New connection, then the favourites)
//! and the connection panel (typing into fields, cycling choices and what
//! each field shows). Moving through the list never changes anything.

use super::App;
use super::setup::{
    BAUDS, DATA_BITS, FLOWS, Field, ListRow, PARITIES, Panel, STOP_BITS, Setup,
    connection_profile_key, cycle_in,
};
use crate::config::Connection;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

impl App {
    pub(super) fn setup_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // While connecting the screen waits: Esc cancels, Ctrl+Q still quits.
        if self.connecting.is_some() {
            match k.code {
                KeyCode::Esc => self.cancel_connect(),
                KeyCode::Char('q') if ctrl => self.quit = true,
                _ => {}
            }
            return;
        }
        match k.code {
            KeyCode::Char('q') if ctrl => self.quit = true,
            KeyCode::Char('u') if ctrl => self.request_update(),
            KeyCode::F(5) => self.rescan_ports(),
            _ if self.setup.panel.is_some() => self.panel_key(k),
            _ => self.list_key(k),
        }
    }

    fn list_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let n = Setup::list_rows(&self.saved).len();
        let row = self.focused_list_row();
        match (k.code, row) {
            (KeyCode::Up, _) => self.setup.focus = (self.setup.focus + n - 1) % n,
            (KeyCode::Down, _) => self.setup.focus = (self.setup.focus + 1) % n,
            (KeyCode::Enter, ListRow::New) => self.open_new_connection(),
            (KeyCode::Enter, ListRow::Favourite(i)) => self.connect_favourite(i),
            (KeyCode::Char('e'), ListRow::Favourite(i)) if !ctrl => self.edit_favourite(i),
            (KeyCode::F(2), ListRow::Favourite(i)) => self.edit_favourite(i),
            (KeyCode::Delete, ListRow::Favourite(i)) => self.ask_remove_favourite(i),
            (KeyCode::Enter, ListRow::Recent(i)) => self.connect_recent(i),
            (KeyCode::Char('e'), ListRow::Recent(i)) if !ctrl => self.edit_recent(i),
            (KeyCode::F(2), ListRow::Recent(i)) => self.edit_recent(i),
            (KeyCode::Delete, ListRow::Recent(i)) => self.forget_recent(i),
            _ => {}
        }
    }

    fn panel_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(panel) = self.setup.panel.as_mut() else {
            return;
        };
        let field = panel.focused_field();
        let n = panel.fields().len();
        if k.code != KeyCode::Esc {
            panel.discard_armed = false;
        }
        let typed = !ctrl || k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Esc => self.close_panel(),
            KeyCode::Char('s') if ctrl => self.save_panel(),
            KeyCode::Up => panel.move_focus((panel.focus + n - 1) % n),
            KeyCode::Down => panel.move_focus((panel.focus + 1) % n),
            KeyCode::Left if field.cycles() || field == Field::Options => self.cycle(field, -1),
            KeyCode::Right if field.cycles() || field == Field::Options => self.cycle(field, 1),
            KeyCode::Enter if field == Field::Options => self.cycle(Field::Options, 1),
            KeyCode::Enter => self.connect_panel(),
            KeyCode::Char(_) if !typed => {}
            code => panel.edit_text(field, code),
        }
    }

    /// A click on the start screen's list: New connection opens the panel,
    /// a favourite or recent connection connects.
    pub(super) fn setup_click(&mut self, me: MouseEvent) {
        if me.kind != MouseEventKind::Down(MouseButton::Left)
            || self.setup.panel.is_some()
            || self.connecting.is_some()
        {
            return;
        }
        let Some(row) = self
            .setup
            .list_hits
            .iter()
            .find(|(r, _)| r.contains((me.column, me.row).into()))
            .map(|(_, row)| *row)
        else {
            return;
        };
        if let Some(i) = Setup::list_rows(&self.saved).iter().position(|r| *r == row) {
            self.setup.focus = i;
        }
        match row {
            ListRow::New => self.open_new_connection(),
            ListRow::Favourite(i) => self.connect_favourite(i),
            ListRow::Recent(i) => self.connect_recent(i),
        }
    }

    /// The list's highlighted row (focus clamped to the rows there are).
    pub fn focused_list_row(&mut self) -> ListRow {
        let rows = Setup::list_rows(&self.saved);
        self.setup.focus = self.setup.focus.min(rows.len() - 1);
        rows[self.setup.focus]
    }

    /// Enter on "New connection": an empty panel. Only the connection type
    /// is the last one used; nothing else is copied, so a new connection is
    /// never a favourite in disguise.
    pub(super) fn open_new_connection(&mut self) {
        let mut connection = Connection {
            mode: self.saved.connection.mode,
            ..Connection::default()
        };
        if connection.serial_port.trim().is_empty()
            && let Some(p) = self.setup.ports.first()
        {
            connection.serial_port = p.name.clone();
        }
        self.setup.error = None;
        self.setup.panel = Some(Panel::new(None, String::new(), connection));
    }

    /// Enter in the panel: connect to what's in it. Nothing is saved as a
    /// favourite; the panel stays open if the connection fails.
    fn connect_panel(&mut self) {
        if let Some(panel) = &self.setup.panel {
            self.saved.connection = panel.connection.clone();
            self.request_connect();
        }
    }

    /// Esc in the panel: back to the list. Unsaved changes need a second Esc.
    fn close_panel(&mut self) {
        let Some(panel) = self.setup.panel.as_mut() else {
            return;
        };
        if panel.has_changes() && !panel.discard_armed {
            panel.discard_armed = true;
            self.setup.error = Some(
                "There are unsaved changes. Esc again throws them away; Ctrl+S saves them.".into(),
            );
            return;
        }
        self.setup.panel = None;
        self.setup.error = None;
    }

    fn cycle(&mut self, f: Field, dir: i32) {
        let Some(panel) = self.setup.panel.as_mut() else {
            return;
        };
        let c = &mut panel.connection;
        match f {
            Field::Mode => c.mode = c.mode.cycle(dir),
            Field::SerialPort => {
                let names: Vec<&str> = self.setup.ports.iter().map(|p| p.name.as_str()).collect();
                if !names.is_empty() {
                    c.serial_port = cycle_in(&names, &c.serial_port, dir);
                }
            }
            Field::Baud => c.baud = cycle_in(BAUDS, &c.baud, dir),
            Field::DataBits => c.data_bits = cycle_in(DATA_BITS, &c.data_bits, dir),
            Field::Parity => c.parity = cycle_in(PARITIES, &c.parity, dir),
            Field::StopBits => c.stop_bits = cycle_in(STOP_BITS, &c.stop_bits, dir),
            Field::Flow => c.flow = cycle_in(FLOWS, &c.flow, dir),
            Field::Logging => {
                let every = self.saved.logging;
                let p = self
                    .saved
                    .profiles
                    .entry(connection_profile_key(c))
                    .or_default();
                p.logging = Some(!p.logging.unwrap_or(every));
            }
            Field::Options => panel.options_open = !panel.options_open,
            // The view is one setting for every connection, not per profile.
            Field::OptLineEnding | Field::OptCharDelay | Field::OptSendMode | Field::OptLayout => {
                use crate::codec::LineEnding as L;
                let key = connection_profile_key(c);
                let p = self.saved.profiles.entry(key).or_default();
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
            Field::Name
            | Field::Host
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
        self.profile_for(&self.saved.connection)
            .view
            .unwrap_or(self.saved.view)
    }

    /// Whether a connection's sessions are logged: its own choice, else the
    /// setting for every connection.
    pub(crate) fn logs_connection(&self, connection: &Connection) -> bool {
        self.profile_for(connection)
            .logging
            .unwrap_or(self.saved.logging)
    }

    /// The remembered profile for a connection (defaults if none yet).
    pub fn profile_for(&self, connection: &Connection) -> crate::config::Profile {
        self.saved
            .profiles
            .get(&connection_profile_key(connection))
            .cloned()
            .unwrap_or_default()
    }

    /// What a field of the panel shows.
    pub fn field_value(&self, panel: &Panel, f: Field) -> String {
        let c = &panel.connection;
        match f {
            Field::Name => panel.name.clone(),
            Field::Mode => c.mode.label().to_string(),
            Field::SerialPort => c.serial_port.clone(),
            Field::Baud => c.baud.clone(),
            Field::DataBits => c.data_bits.clone(),
            Field::Parity => c.parity.clone(),
            Field::StopBits => c.stop_bits.clone(),
            Field::Flow => c.flow.clone(),
            Field::Host => c.tcp_host.clone(),
            Field::TcpPort => c.tcp_port.clone(),
            Field::Bind => c.listen_bind.clone(),
            Field::ListenPort => c.listen_port.clone(),
            Field::TelnetHost => c.telnet_host.clone(),
            Field::TelnetPort => c.telnet_port.clone(),
            Field::SshHost => c.ssh_host.clone(),
            Field::SshUser => c.ssh_user.clone(),
            Field::SshPort => c.ssh_port.clone(),
            Field::Logging => {
                if self.logs_connection(c) {
                    "Yes".into()
                } else {
                    "No".into()
                }
            }
            Field::Options => {
                let p = self.profile_for(c);
                let delay = if p.char_delay_ms == 0 {
                    "no delay".to_string()
                } else {
                    format!("{} ms delay", p.char_delay_ms)
                };
                let arrow = if panel.options_open { "▼" } else { "►" };
                let log = if self.logs_connection(c) {
                    "logged"
                } else {
                    "not logged"
                };
                format!(
                    "{arrow} {} · {delay} · {} · {} · {log}",
                    p.line_ending.label(),
                    p.input_mode.label(),
                    crate::config::view_choice_label(p.view)
                )
            }
            Field::OptLineEnding => self.profile_for(c).line_ending.label().into(),
            Field::OptCharDelay => match self.profile_for(c).char_delay_ms {
                0 => "off".into(),
                ms => format!("{ms} ms"),
            },
            Field::OptSendMode => self.profile_for(c).input_mode.label().into(),
            Field::OptLayout => crate::config::view_choice_label(self.profile_for(c).view).into(),
        }
    }
}
