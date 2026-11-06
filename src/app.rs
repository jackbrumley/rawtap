//! Application state: the setup screen, switching to the monitor, and saved state.

pub mod identity;

use crate::config::{self, Config, ConnMode, Saved};
use crate::extract::Extractor;
use crate::link::{Link, LinkEvent, LinkSettings};
use crate::logger::Logger;
use crate::monitor::{Action, EntryKind, Monitor};
use crate::paths::Paths;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

pub const BAUDS: &[&str] = &[
    "300", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "230400", "460800",
    "921600",
];
pub const DATA_BITS: &[&str] = &["5", "6", "7", "8"];
pub const PARITIES: &[&str] = &["None", "Odd", "Even"];
pub const STOP_BITS: &[&str] = &["1", "2"];
pub const FLOWS: &[&str] = &["None", "Software", "Hardware"];

// ---------------------------------------------------------------- setup screen

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Mode,
    SerialPort,
    Baud,
    DataBits,
    Parity,
    StopBits,
    Flow,
    Host,
    TcpPort,
    Bind,
    ListenPort,
    Logging,
    Options,
    OptLineEnding,
    OptCharDelay,
    OptSendMode,
    OptLayout,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Mode => "Connection",
            Field::SerialPort => "Serial port",
            Field::Baud => "Baud rate",
            Field::DataBits => "Data bits",
            Field::Parity => "Parity",
            Field::StopBits => "Stop bits",
            Field::Flow => "Flow control",
            Field::Host => "Host / IP",
            Field::TcpPort => "TCP port",
            Field::Bind => "Listen on",
            Field::ListenPort => "TCP port",
            Field::Logging => "Log to file",
            Field::Options => "Options",
            Field::OptLineEnding => "  Line ending",
            Field::OptCharDelay => "  Char delay",
            Field::OptSendMode => "  Send mode",
            Field::OptLayout => "  Layout",
        }
    }

    /// Shown under the form for the highlighted field.
    pub fn description(self) -> &'static str {
        match self {
            Field::Mode => {
                "Serial: a serial port (USB adapter or console cable). TCP client: connect out to an IP and port. TCP server: wait for a device to connect to this computer."
            }
            Field::SerialPort => {
                "The serial port to open. ←→ cycles through the ports detected on this computer, or type a name such as COM3 (Windows) or /dev/ttyUSB0 (Linux)."
            }
            Field::Baud => {
                "Speed of the serial line; it must match the device. Switch consoles are usually 9600 or 115200. ←→ for common speeds, or type one."
            }
            Field::DataBits | Field::Parity | Field::StopBits => {
                "Almost always 8 data bits, no parity, 1 stop bit (\"8N1\"). Only change these if the device's manual says so."
            }
            Field::Flow => {
                "Usually None. Hardware uses the RTS/CTS wires; Software uses XON/XOFF characters."
            }
            Field::Host => {
                "Host name or IP address of the device or terminal server. Note: raw TCP, not telnet."
            }
            Field::TcpPort => {
                "Port number on that host, e.g. a terminal server's raw TCP port for the device."
            }
            Field::Bind => {
                "Which address on this computer to listen on. 0.0.0.0 means all network adapters."
            }
            Field::ListenPort => {
                "Port devices will connect to on this computer. A firewall may need to allow it."
            }
            Field::Logging => {
                "Write a hex log and a plain-text transcript of the session to the logs folder."
            }
            Field::Options => {
                "Settings remembered for this port or host. ←→ or Enter to show / hide them. They can also be changed any time from the F1 menu."
            }
            Field::OptLineEnding => crate::menu::line_ending_desc(),
            Field::OptCharDelay => crate::menu::char_delay_desc(),
            Field::OptSendMode => crate::menu::send_mode_desc(),
            Field::OptLayout => crate::menu::layout_desc(),
        }
    }
    /// Text fields accept typing; the rest are cycled with ←/→.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            Field::SerialPort
                | Field::Baud
                | Field::Host
                | Field::TcpPort
                | Field::Bind
                | Field::ListenPort
        )
    }
}

pub struct PortInfo {
    pub name: String,
    pub desc: String,
}

pub struct Setup {
    pub focus: usize,
    pub ports: Vec<PortInfo>,
    pub error: Option<String>,
    pub connecting: bool,
    pub options_open: bool,
    last_scan: Option<Instant>,
}

impl Setup {
    pub fn fields(mode: ConnMode, options_open: bool) -> Vec<Field> {
        let mut f = vec![Field::Mode];
        match mode {
            ConnMode::Serial => f.extend([
                Field::SerialPort,
                Field::Baud,
                Field::DataBits,
                Field::Parity,
                Field::StopBits,
                Field::Flow,
            ]),
            ConnMode::Tcp => f.extend([Field::Host, Field::TcpPort]),
            ConnMode::Listen => f.extend([Field::Bind, Field::ListenPort]),
        }
        f.push(Field::Logging);
        f.push(Field::Options);
        if options_open {
            f.extend([
                Field::OptLineEnding,
                Field::OptCharDelay,
                Field::OptSendMode,
                Field::OptLayout,
            ]);
        }
        f
    }
}

pub fn scan_ports() -> Vec<PortInfo> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let desc = match p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    let name = u
                        .product
                        .or(u.manufacturer)
                        .unwrap_or_else(|| "USB serial".into());
                    format!("{name} (USB {:04X}:{:04X})", u.vid, u.pid)
                }
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                serialport::SerialPortType::PciPort => "Built-in / PCI".into(),
                serialport::SerialPortType::Unknown => String::new(),
            };
            PortInfo {
                name: p.port_name,
                desc,
            }
        })
        .collect();
    let num = |n: &str| {
        n.trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    };
    ports.sort_by(|a, b| num(&a.name).cmp(&num(&b.name)).then(a.name.cmp(&b.name)));
    ports
}

fn cycle_in(list: &[&str], current: &str, dir: i32) -> String {
    let i = list.iter().position(|v| v.eq_ignore_ascii_case(current));
    let n = list.len() as i32;
    let next = match i {
        Some(i) => (i as i32 + dir).rem_euclid(n),
        None => {
            if dir > 0 {
                0
            } else {
                n - 1
            }
        }
    };
    list[next as usize].to_string()
}

/// The profile key for what's currently typed in the start screen (matches `profile_key`).
pub fn form_profile_key(s: &Saved) -> String {
    let port = |p: &str| {
        p.trim()
            .parse::<u16>()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| p.trim().to_string())
    };
    match s.mode {
        ConnMode::Serial => format!("serial:{}", serial_key(s.serial_port.trim())),
        ConnMode::Tcp => format!("tcp:{}:{}", s.tcp_host.trim(), port(&s.tcp_port)),
        ConnMode::Listen => format!("listen:{}", port(&s.listen_port)),
    }
}

/// Key for remembering view settings per connection, e.g. "serial:COM22".
pub fn profile_key(s: &LinkSettings) -> String {
    match s {
        LinkSettings::Serial { port, .. } => format!("serial:{}", serial_key(port)),
        LinkSettings::TcpClient { host, port } => format!("tcp:{host}:{port}"),
        LinkSettings::TcpListen { port, .. } => format!("listen:{port}"),
    }
}

/// Windows port names are case-insensitive (com3 is COM3); Linux device paths
/// are case-sensitive and kept as they are.
fn serial_key(port: &str) -> String {
    if cfg!(windows) {
        port.to_uppercase()
    } else {
        port.to_string()
    }
}

pub fn settings_from_saved(s: &Saved) -> Result<LinkSettings, String> {
    let port_num = |v: &str, what: &str| -> Result<u16, String> {
        match v.trim().parse::<u16>() {
            Ok(p) if p > 0 => Ok(p),
            _ => Err(format!("{what} must be a number from 1 to 65535")),
        }
    };
    match s.mode {
        ConnMode::Serial => {
            let port = s.serial_port.trim().to_string();
            if port.is_empty() {
                return Err("Pick a serial port (←/→ cycles through the detected ones).".into());
            }
            let baud = match s.baud.trim().parse::<u32>() {
                Ok(b) if b > 0 => b,
                _ => return Err("Baud rate must be a number, e.g. 9600".into()),
            };
            let data_bits = match s.data_bits.as_str() {
                "5" => DataBits::Five,
                "6" => DataBits::Six,
                "7" => DataBits::Seven,
                _ => DataBits::Eight,
            };
            let parity = match s.parity.to_ascii_lowercase().as_str() {
                "odd" | "o" => Parity::Odd,
                "even" | "e" => Parity::Even,
                _ => Parity::None,
            };
            let stop_bits = if s.stop_bits == "2" {
                StopBits::Two
            } else {
                StopBits::One
            };
            let flow = match s.flow.to_ascii_lowercase().as_str() {
                "software" | "xonxoff" => FlowControl::Software,
                "hardware" | "rtscts" => FlowControl::Hardware,
                _ => FlowControl::None,
            };
            Ok(LinkSettings::Serial {
                port,
                baud,
                data_bits,
                parity,
                stop_bits,
                flow,
            })
        }
        ConnMode::Tcp => {
            let host = s.tcp_host.trim().to_string();
            if host.is_empty() {
                return Err("Enter a host name or IP address.".into());
            }
            Ok(LinkSettings::TcpClient {
                host,
                port: port_num(&s.tcp_port, "TCP port")?,
            })
        }
        ConnMode::Listen => {
            let bind = match s.listen_bind.trim() {
                "" => "0.0.0.0".to_string(),
                b => b.to_string(),
            };
            Ok(LinkSettings::TcpListen {
                bind,
                port: port_num(&s.listen_port, "TCP port")?,
            })
        }
    }
}

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
                    m.push(EntryKind::Error(e));
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
            Field::Host | Field::TcpPort | Field::Bind | Field::ListenPort => {}
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
