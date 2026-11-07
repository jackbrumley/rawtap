//! Application state: the setup screen, switching to the monitor, and saved state.

pub mod identity;
pub mod setup;
pub mod update_prompt;

use setup::{
    BAUDS, DATA_BITS, FLOWS, Field, PARITIES, STOP_BITS, Setup, cycle_in, form_profile_key,
    profile_key, scan_ports, settings_from_saved,
};

use crate::config::{self, Config, Saved};
use crate::extract::Extractor;
use crate::link::{Link, LinkEvent, LinkSettings};
use crate::logger::Logger;
use crate::monitor::{Action, EntryKind, Monitor};
use crate::paths::Paths;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- app

#[derive(PartialEq, Eq)]
pub enum Screen {
    Setup,
    Monitor,
}

pub struct App {
    pub screen: Screen,
    pub setup: Setup,
    pub monitor: Option<Monitor>,
    pub config: Config,
    pub paths: Paths,
    pub saved: Saved,
    pub notes: Vec<String>,
    pub no_log: bool,
    pub quit: bool,
    pub pending_connect: bool,
    /// Set when the user chose to update: the main loop then quits, restores
    /// the terminal and runs the updater.
    pub update_requested: bool,
    /// A running update check, and whether the user asked for it (then the
    /// result is reported even when there's nothing new).
    update_check: Option<(Receiver<Result<crate::update::UpdateCheck, String>>, bool)>,
    /// The "Update Available" dialog, when it's on screen.
    pub update_prompt: Option<update_prompt::UpdatePrompt>,
    /// A check result waiting to be shown as the dialog once nothing else
    /// needs the keyboard (see `update_prompt::can_show_prompt`).
    pending_prompt: Option<crate::update::UpdateCheck>,
    /// The dialog opens by itself once a run; after that only on request.
    prompt_shown: bool,
    /// Keys arriving faster than anyone can type are a paste from the console; collect them.
    paste_buf: Option<String>,
    paste_deadline: Option<Instant>,
    tx: Sender<LinkEvent>,
    rx: Receiver<LinkEvent>,
}

impl App {
    pub fn new(
        config: Config,
        paths: Paths,
        saved: Saved,
        notes: Vec<String>,
        no_log: bool,
    ) -> App {
        let (tx, rx) = mpsc::channel();
        let mut app = App {
            screen: Screen::Setup,
            setup: Setup {
                focus: 0,
                ports: Vec::new(),
                error: None,
                connecting: false,
                options_open: false,
                last_scan: None,
            },
            monitor: None,
            config,
            paths,
            saved,
            notes,
            no_log,
            quit: false,
            pending_connect: false,
            update_requested: false,
            update_check: None,
            update_prompt: None,
            pending_prompt: None,
            prompt_shown: false,
            paste_buf: None,
            paste_deadline: None,
            tx,
            rx,
        };
        app.rescan_ports();
        app
    }

    pub fn request_connect(&mut self) {
        self.pending_connect = true;
        self.setup.connecting = true;
        self.setup.error = None;
    }

    fn rescan_ports(&mut self) {
        self.setup.ports = scan_ports();
        self.setup.last_scan = Some(Instant::now());
        if self.saved.serial_port.trim().is_empty()
            && let Some(p) = self.setup.ports.first()
        {
            self.saved.serial_port = p.name.clone();
        }
    }

    /// Handle everything that arrived since the last frame. A burst of typed characters
    /// (Windows Terminal's Ctrl+Shift+V / right-click paste) is treated as one paste.
    pub fn on_events(&mut self, batch: Vec<Event>) {
        // The update dialog takes keys one by one: a burst is never a paste into it.
        if self.update_prompt.is_some() {
            for e in batch {
                self.on_event(e);
            }
            return;
        }
        fn typed(e: &Event) -> Option<char> {
            let Event::Key(k) = e else { return None };
            if k.kind == KeyEventKind::Release {
                return None;
            }
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            let alt = k.modifiers.contains(KeyModifiers::ALT);
            match k.code {
                KeyCode::Char(c) if !ctrl || alt => Some(c),
                KeyCode::Enter => Some('\n'),
                KeyCode::Tab => Some('\t'),
                _ => None,
            }
        }
        let is_release = |e: &Event| matches!(e, Event::Key(k) if k.kind == KeyEventKind::Release);
        let n = batch.iter().filter(|e| typed(e).is_some()).count();
        let continuing = self.paste_buf.is_some()
            && n > 0
            && batch.iter().all(|e| typed(e).is_some() || is_release(e));
        if n >= 4 || continuing {
            let buf = self.paste_buf.get_or_insert_with(String::new);
            let mut others = Vec::new();
            for e in batch {
                match typed(&e) {
                    Some(c) => buf.push(c),
                    None if is_release(&e) => {}
                    None => others.push(e),
                }
            }
            self.paste_deadline = Some(Instant::now() + Duration::from_millis(60));
            for e in others {
                self.on_event(e);
            }
            return;
        }
        for e in batch {
            self.on_event(e);
        }
    }

    fn on_paste(&mut self, text: String) {
        match self.screen {
            Screen::Monitor => {
                if let Some(m) = self.monitor.as_mut() {
                    m.on_paste(text);
                }
            }
            Screen::Setup => {
                let fields = Setup::fields(self.saved.mode, self.setup.options_open);
                if let Some(f) = fields.get(self.setup.focus).copied()
                    && let Some(t) = self.text_mut(f)
                {
                    t.push_str(text.lines().next().unwrap_or("").trim());
                }
            }
        }
    }

    /// Called every frame.
    pub fn tick(&mut self) {
        self.poll_update_check();
        self.show_pending_prompt();
        if self.paste_deadline.is_some_and(|d| Instant::now() >= d) {
            self.paste_deadline = None;
            if let Some(t) = self.paste_buf.take() {
                self.on_paste(t);
            }
        }
        if self.pending_connect {
            self.pending_connect = false;
            self.connect_from_setup();
        }
        if self.screen == Screen::Setup
            && self
                .setup
                .last_scan
                .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.rescan_ports();
        }
        while let Ok(ev) = self.rx.try_recv() {
            if let Some(m) = self.monitor.as_mut() {
                m.on_link_event(ev);
            }
        }
        if let Some(m) = self.monitor.as_mut() {
            m.tick();
        }
    }

    fn make_logger(&self, settings: &LinkSettings) -> (Option<Logger>, Option<String>) {
        if self.no_log || !self.saved.logging {
            return (None, None);
        }
        let dir = self
            .paths
            .resolve(self.config.log_directory.as_deref().unwrap_or("logs"));
        match Logger::create(&dir, settings.kind(), &settings.describe()) {
            Ok(l) => (Some(l), None),
            Err(e) => (
                None,
                Some(format!(
                    "Could not create log file in {}: {e}",
                    dir.display()
                )),
            ),
        }
    }

    fn connect_from_setup(&mut self) {
        self.setup.connecting = false;
        let settings = match settings_from_saved(&self.saved) {
            Ok(s) => s,
            Err(e) => {
                self.setup.error = Some(e);
                self.screen = Screen::Setup;
                return;
            }
        };
        self.close_monitor();
        let link = match Link::open(&settings, self.tx.clone()) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("Could not connect to {}: {e}", settings.describe());
                self.setup.error = Some(e);
                self.screen = Screen::Setup;
                return;
            }
        };
        let (logger, log_note) = self.make_logger(&settings);
        let (extractor, extract_note) =
            Extractor::new(self.config.image_extraction.as_ref(), &self.paths);
        let key = profile_key(&settings);
        let profile = self.saved.profiles.get(&key).cloned().unwrap_or_default();
        let (macros, macro_notes) = crate::macros::load(&self.config.macros);
        let capture_dir = self.paths.resolve(
            self.config
                .capture_directory
                .as_deref()
                .unwrap_or("captures"),
        );
        let mut m = Monitor::new(
            settings.clone(),
            key,
            profile,
            self.saved.history.clone(),
            macros,
            extractor,
            capture_dir,
        );
        // ssh asks its own questions (host keys, passwords): keys go straight to it.
        if matches!(settings, LinkSettings::Ssh { .. }) {
            m.run_cmd(crate::menu::Cmd::ToggleLive);
        }
        if !self.no_log {
            m.log_dir = Some(
                self.paths
                    .resolve(self.config.log_directory.as_deref().unwrap_or("logs")),
            );
        }
        for n in self.notes.drain(..) {
            m.push(EntryKind::Info(n));
        }
        m.attach(link, logger, log_note);
        m.push(EntryKind::Info(extract_note));
        for n in macro_notes {
            m.push(EntryKind::Error(n));
        }
        m.push(EntryKind::Info(
            "F1 opens the menu: settings, macros, hidden send, live keys and more.".into(),
        ));
        self.monitor = Some(m);
        self.screen = Screen::Monitor;
        self.setup.error = None;
        self.store_state_reporting();
        crate::set_title(&format!("RawTap - {}", settings.describe()));
    }

    fn reconnect(&mut self) {
        let Some(m) = self.monitor.as_mut() else {
            return;
        };
        m.detach("Disconnected. Reconnecting…");
        while self.rx.try_recv().is_ok() {}
        let settings = m.settings.clone();
        match Link::open(&settings, self.tx.clone()) {
            Ok(link) => {
                let (logger, note) = self.make_logger(&settings);
                if let Some(m) = self.monitor.as_mut() {
                    m.attach(link, logger, note);
                }
            }
            Err(e) => {
                if let Some(m) = self.monitor.as_mut() {
                    m.report_error(e);
                }
            }
        }
    }

    fn close_monitor(&mut self) {
        if let Some(mut m) = self.monitor.take() {
            self.saved.history = m.history.clone();
            self.saved
                .profiles
                .insert(m.profile_key.clone(), m.profile.clone());
            m.detach("Session closed.");
        }
        while self.rx.try_recv().is_ok() {}
    }

    /// Saves `state.json`: remembered connections, per-connection settings,
    /// history and the log level.
    pub fn store_state(&mut self) -> Result<(), String> {
        if let Some(m) = &self.monitor {
            self.saved.history = m.history.clone();
            self.saved
                .profiles
                .insert(m.profile_key.clone(), m.profile.clone());
        }
        self.saved.log_level = crate::session_log::level_name(log::max_level()).into();
        config::store_saved(&self.paths, &self.saved).inspect_err(|error| log::error!("{error}"))
    }

    /// Saves state while the TUI runs, showing a failure where the user is
    /// looking.
    fn store_state_reporting(&mut self) {
        if let Err(error) = self.store_state() {
            match self.monitor.as_mut() {
                Some(m) => m.report_error(error),
                None => self.setup.error = Some(error),
            }
        }
    }

    /// Saves state and closes the connection. A failed save is returned so
    /// `main` can print it once the terminal is back to normal.
    pub fn shutdown(&mut self) -> Result<(), String> {
        let saved = self.store_state();
        self.close_monitor();
        saved
    }

    pub fn on_event(&mut self, ev: Event) {
        if self.update_prompt.is_some() {
            if let Event::Key(k) = ev
                && k.kind != KeyEventKind::Release
            {
                self.update_prompt_key(k);
            }
            return;
        }
        let k = match ev {
            Event::Key(k) => k,
            Event::Paste(t) => return self.on_paste(t),
            Event::Mouse(me) => {
                if self.screen == Screen::Monitor
                    && let Some(m) = self.monitor.as_mut()
                {
                    m.on_mouse(me);
                }
                return;
            }
            _ => return,
        };
        if k.kind == KeyEventKind::Release {
            return;
        }
        match self.screen {
            Screen::Setup => self.setup_key(k),
            Screen::Monitor => {
                let Some(m) = self.monitor.as_mut() else {
                    return;
                };
                match m.on_key(k) {
                    Action::None => {}
                    Action::Quit => self.quit = true,
                    Action::Reconnect => self.reconnect(),
                    Action::CheckForUpdates => self.start_update_check(true),
                    Action::UpdateNow => self.request_update(),
                    Action::NewConnection => {
                        self.close_monitor();
                        self.store_state_reporting();
                        self.screen = Screen::Setup;
                        self.setup.focus = 0;
                        self.rescan_ports();
                        crate::set_title("RawTap");
                    }
                }
            }
        }
    }

    fn setup_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let fields = Setup::fields(self.saved.mode, self.setup.options_open);
        let n = fields.len();
        self.setup.focus = self.setup.focus.min(n - 1);
        let field = fields[self.setup.focus];
        match k.code {
            KeyCode::Char('q') | KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('u') if ctrl => self.request_update(),
            KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::BackTab => self.setup.focus = (self.setup.focus + n - 1) % n,
            KeyCode::Down | KeyCode::Tab => self.setup.focus = (self.setup.focus + 1) % n,
            KeyCode::Left => self.cycle(field, -1),
            KeyCode::Right => self.cycle(field, 1),
            KeyCode::Enter if field == Field::Options => self.cycle(field, 1),
            KeyCode::Enter => self.request_connect(),
            KeyCode::F(5) => self.rescan_ports(),
            KeyCode::Backspace => {
                if let Some(t) = self.text_mut(field) {
                    t.pop();
                }
            }
            KeyCode::Char(' ') if !field.is_text() => self.cycle(field, 1),
            KeyCode::Char(c) if !ctrl || k.modifiers.contains(KeyModifiers::ALT) => {
                if let Some(t) = self.text_mut(field) {
                    t.push(c);
                }
            }
            _ => {}
        }
    }

    fn text_mut(&mut self, f: Field) -> Option<&mut String> {
        let s = &mut self.saved;
        Some(match f {
            Field::SerialPort => &mut s.serial_port,
            Field::Baud => &mut s.baud,
            Field::Host => &mut s.tcp_host,
            Field::TcpPort => &mut s.tcp_port,
            Field::Bind => &mut s.listen_bind,
            Field::ListenPort => &mut s.listen_port,
            Field::TelnetHost => &mut s.telnet_host,
            Field::TelnetPort => &mut s.telnet_port,
            Field::SshHost => &mut s.ssh_host,
            Field::SshUser => &mut s.ssh_user,
            Field::SshPort => &mut s.ssh_port,
            _ => return None,
        })
    }

    fn cycle(&mut self, f: Field, dir: i32) {
        let s = &mut self.saved;
        match f {
            Field::Mode => s.mode = s.mode.cycle(dir),
            Field::SerialPort => {
                let names: Vec<&str> = self.setup.ports.iter().map(|p| p.name.as_str()).collect();
                if !names.is_empty() {
                    s.serial_port = cycle_in(&names, &s.serial_port, dir);
                }
            }
            Field::Baud => s.baud = cycle_in(BAUDS, &s.baud, dir),
            Field::DataBits => s.data_bits = cycle_in(DATA_BITS, &s.data_bits, dir),
            Field::Parity => s.parity = cycle_in(PARITIES, &s.parity, dir),
            Field::StopBits => s.stop_bits = cycle_in(STOP_BITS, &s.stop_bits, dir),
            Field::Flow => s.flow = cycle_in(FLOWS, &s.flow, dir),
            Field::Logging => s.logging = !s.logging,
            Field::Options => self.setup.options_open = !self.setup.options_open,
            Field::OptLineEnding | Field::OptCharDelay | Field::OptSendMode | Field::OptLayout => {
                use crate::codec::LineEnding as L;
                use crate::config::Layout as Ly;
                let key = form_profile_key(s);
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
                    Field::OptSendMode => {
                        p.input_mode =
                            crate::menu::step(&crate::menu::input_modes(), p.input_mode, dir)
                    }
                    _ => {
                        p.layout =
                            crate::menu::step(&[Ly::Split, Ly::Log, Ly::Terminal], p.layout, dir)
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

    /// The remembered profile for what's typed in the form (defaults if none yet).
    pub fn form_profile(&self) -> crate::config::Profile {
        self.saved
            .profiles
            .get(&form_profile_key(&self.saved))
            .cloned()
            .unwrap_or_default()
    }

    pub fn field_value(&self, f: Field) -> String {
        let s = &self.saved;
        match f {
            Field::Mode => s.mode.label().to_string(),
            Field::SerialPort => s.serial_port.clone(),
            Field::Baud => s.baud.clone(),
            Field::DataBits => s.data_bits.clone(),
            Field::Parity => s.parity.clone(),
            Field::StopBits => s.stop_bits.clone(),
            Field::Flow => s.flow.clone(),
            Field::Host => s.tcp_host.clone(),
            Field::TcpPort => s.tcp_port.clone(),
            Field::Bind => s.listen_bind.clone(),
            Field::ListenPort => s.listen_port.clone(),
            Field::TelnetHost => s.telnet_host.clone(),
            Field::TelnetPort => s.telnet_port.clone(),
            Field::SshHost => s.ssh_host.clone(),
            Field::SshUser => s.ssh_user.clone(),
            Field::SshPort => s.ssh_port.clone(),
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
                    p.layout.label()
                )
            }
            Field::OptLineEnding => self.form_profile().line_ending.label().into(),
            Field::OptCharDelay => match self.form_profile().char_delay_ms {
                0 => "off".into(),
                ms => format!("{ms} ms"),
            },
            Field::OptSendMode => self.form_profile().input_mode.label().into(),
            Field::OptLayout => self.form_profile().layout.label().into(),
        }
    }
}
