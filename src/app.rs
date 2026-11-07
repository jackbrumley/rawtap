//! Application state: the setup screen, switching to the monitor, and saved state.

pub mod connect;
pub mod favourites;
mod form;
pub mod identity;
pub mod setup;
pub mod update_prompt;

use setup::{Row, Setup, connection_settings, profile_key, scan_ports};

use crate::config::{self, Config, Saved};
use crate::extract::Extractor;
use crate::link::{Link, LinkEvent, LinkSettings};
use crate::logger::Logger;
use crate::monitor::{Action, EntryKind, Monitor};
use crate::paths::Paths;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::sync::mpsc::{self, Receiver};
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
    /// A connection being opened in the background (spinner, Esc cancels).
    pub connecting: Option<connect::Connecting>,
    /// Set when the user chose to update: the main loop then quits, restores
    /// the terminal and runs the updater.
    pub update_requested: bool,
    /// A running update check, and whether the user asked for it (then the
    /// result is reported even when there's nothing new).
    update_check: Option<(Receiver<Result<crate::update::UpdateCheck, String>>, bool)>,
    /// The "Update Available" dialog, when it's on screen.
    pub update_prompt: Option<update_prompt::UpdatePrompt>,
    /// Saving, editing or removing a favourite.
    pub favourite_dialog: Option<favourites::FavouriteDialog>,
    /// A check result waiting to be shown as the dialog once nothing else
    /// needs the keyboard (see `update_prompt::can_show_prompt`).
    pending_prompt: Option<crate::update::UpdateCheck>,
    /// The dialog opens by itself once a run; after that only on request.
    prompt_shown: bool,
    /// Keys arriving faster than anyone can type are a paste from the console; collect them.
    paste_buf: Option<String>,
    paste_deadline: Option<Instant>,
    /// The current connection's events (each connection has its own channel).
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
        // No connection yet: a channel nobody sends on.
        let (_, rx) = mpsc::channel();
        let mut app = App {
            screen: Screen::Setup,
            setup: Setup {
                focus: 0,
                ports: Vec::new(),
                error: None,
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
            connecting: None,
            update_requested: false,
            update_check: None,
            update_prompt: None,
            favourite_dialog: None,
            pending_prompt: None,
            prompt_shown: false,
            paste_buf: None,
            paste_deadline: None,
            rx,
        };
        app.rescan_ports();
        app.home_focus();
        app
    }

    /// Enter on the start screen (or a connection given on the command line):
    /// check the settings, then open the connection in the background.
    pub fn request_connect(&mut self) {
        if self.connecting.is_some() {
            return;
        }
        self.setup.error = None;
        match connection_settings(&self.saved.connection) {
            Ok(settings) => self.start_connect(settings, false),
            Err(error) => self.setup.error = Some(error),
        }
    }

    fn rescan_ports(&mut self) {
        self.setup.ports = scan_ports();
        self.setup.last_scan = Some(Instant::now());
        if self.saved.connection.serial_port.trim().is_empty()
            && let Some(p) = self.setup.ports.first()
        {
            self.saved.connection.serial_port = p.name.clone();
        }
    }

    /// Handle everything that arrived since the last frame. A burst of typed characters
    /// (the console's own paste) is treated as one paste.
    pub fn on_events(&mut self, batch: Vec<Event>) {
        // Dialogs take keys one by one: a burst is never a paste into them.
        if self.update_prompt.is_some() || self.favourite_dialog.is_some() {
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
        if self.favourite_dialog.is_some() {
            return self.favourite_dialog_paste(&text);
        }
        match self.screen {
            Screen::Monitor => {
                if let Some(m) = self.monitor.as_mut() {
                    m.on_paste(text);
                }
            }
            Screen::Setup => {
                if let Some(Row::Field(f)) = self.focused_row()
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
        self.poll_connect();
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

    /// A connection from the start screen opened: show the monitor.
    fn open_monitor(&mut self, settings: LinkSettings, link: Link) {
        self.close_monitor();
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
            self.view_for_connection(),
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
        if self.connecting.is_some() {
            return;
        }
        let Some(m) = self.monitor.as_mut() else {
            return;
        };
        m.detach("Disconnected. Reconnecting…");
        let settings = m.settings.clone();
        self.start_connect(settings, true);
    }

    /// Ctrl+R's connection opened: attach it to the monitor that's showing.
    fn reattach(&mut self, settings: &LinkSettings, link: Link) {
        let (logger, note) = self.make_logger(settings);
        if let Some(m) = self.monitor.as_mut() {
            m.attach(link, logger, note);
        }
    }

    fn close_monitor(&mut self) {
        if let Some(mut m) = self.monitor.take() {
            self.saved.history = m.history.clone();
            self.saved
                .profiles
                .insert(m.profile_key.clone(), m.profile.clone());
            if m.view_chosen {
                self.saved.view = m.layout;
            }
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
            if m.view_chosen {
                self.saved.view = m.layout;
            }
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
        if self.favourite_dialog.is_some() {
            match ev {
                Event::Key(k) if k.kind != KeyEventKind::Release => self.favourite_dialog_key(k),
                Event::Paste(text) => self.favourite_dialog_paste(&text),
                _ => {}
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
                // Esc during Ctrl+R's reconnect cancels it.
                if self.connecting.is_some() && k.code == KeyCode::Esc {
                    self.cancel_connect();
                    return;
                }
                let Some(m) = self.monitor.as_mut() else {
                    return;
                };
                match m.on_key(k) {
                    Action::None => {}
                    Action::Quit => self.quit = true,
                    Action::Reconnect => self.reconnect(),
                    Action::CheckForUpdates => self.start_update_check(true),
                    Action::UpdateNow => self.request_update(),
                    Action::SaveFavourite => self.save_session_as_favourite(),
                    Action::NewConnection => {
                        self.close_monitor();
                        self.store_state_reporting();
                        self.screen = Screen::Setup;
                        self.rescan_ports();
                        self.home_focus();
                        crate::set_title("RawTap");
                    }
                }
            }
        }
    }
}
