//! The monitor screen's state: raw log, live terminal, send box and macros. The panes,
//! mouse, copy and paste are in `view`; commands, menu and keys are in `keys`.

mod keys;
mod view;

use crate::codec;
use crate::config::{HistoryItem, Layout, LogFormat, Profile};
use crate::extract::{ExtractEvent, Extractor};
use crate::link::{Link, LinkEvent, LinkSettings};
use crate::logger::Logger;
use crate::macros::{LoadedMacro, MacroRun, Poll};
use crate::menu::{self, Cmd, Kind, Menu, Setting};
use crate::term::TextCleaner;
use crate::term::{self, Term, Transcript};
use chrono::{DateTime, Local};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const MAX_ENTRIES: usize = 20_000;
const MAX_HISTORY: usize = 200;
/// Bytes arriving closer together than this are shown as one log entry.
const COALESCE_IDLE: Duration = Duration::from_millis(30);
/// How long ssh may print nothing before the raw log says so.
const SSH_SILENCE_NOTE: Duration = Duration::from_secs(10);
const COALESCE_MAX: usize = 1024;
const BREAK_LEN: Duration = Duration::from_millis(300);

// ---------------------------------------------------------------- log entries

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Rx,
    Tx,
}

pub enum EntryKind {
    Rx(Vec<u8>),
    Tx(Vec<u8>),
    /// A send kept out of the log (password). Only the length is kept.
    TxHidden(usize),
    Info(String),
    Good(String),
    Error(String),
}

pub struct Entry {
    pub id: u64,
    pub time: DateTime<Local>,
    pub kind: EntryKind,
    cache: Option<(u16, LogFormat, usize)>,
}

impl Entry {
    pub fn new(kind: EntryKind) -> Entry {
        Entry {
            id: 0,
            time: Local::now(),
            kind,
            cache: None,
        }
    }

    /// Number of screen rows this entry takes at a given width (cached).
    pub fn line_count(&mut self, width: u16, fmt: LogFormat) -> usize {
        if let Some((w, f, n)) = self.cache
            && w == width
            && f == fmt
        {
            return n;
        }
        let n = crate::ui_monitor::panes::entry_lines(self, width, fmt).len();
        self.cache = Some((width, fmt, n));
        n
    }
}

struct Pending {
    dir: Dir,
    hidden: bool,
    data: Vec<u8>,
    time: DateTime<Local>,
    last: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Log,
    Terminal,
}

/// A mouse selection inside one pane. Positions are (row, col) where row is an absolute
/// scrollback row for the terminal, or an entry id for the raw log (col unused there), so a
/// selection stays put while you scroll or new output arrives.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub pane: Pane,
    pub anchor: (usize, u16),
    pub head: (usize, u16),
    pub dragging: bool,
    /// False for a plain click (which shouldn't select anything).
    pub moved: bool,
}

impl Selection {
    /// Start and end in reading order.
    pub fn ordered(&self) -> ((usize, u16), (usize, u16)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
    pub fn is_empty(&self) -> bool {
        !self.moved || (self.pane == Pane::Terminal && self.anchor == self.head)
    }
}

const WHEEL_LINES: usize = 3;
const MAX_PLAIN: usize = 4 * 1024 * 1024;

pub enum Action {
    None,
    Quit,
    Reconnect,
    NewConnection,
    CheckForUpdates,
    UpdateNow,
    SaveFavourite,
}

// ---------------------------------------------------------------- monitor

pub struct Monitor {
    pub settings: LinkSettings,
    pub profile_key: String,
    pub profile: Profile,
    /// The view this session shows (Terminal / Split / Hex).
    pub layout: Layout,
    /// The user changed the view in this session: it becomes the "last used"
    /// view. A favourite's fixed view isn't, unless the user picks it.
    pub view_chosen: bool,
    /// Where the footer drew each view word, for clicks.
    pub footer_views: Vec<(Rect, Layout)>,
    pub link: Option<Link>,
    pub logger: Option<Logger>,
    pub transcript: Option<Transcript>,
    extractor: Extractor,

    // Raw log pane
    pub entries: VecDeque<Entry>,
    /// Rows scrolled up from the bottom.
    pub scroll: usize,
    pub follow: bool,
    pending: Option<Pending>,

    // Terminal pane
    pub term: Term,
    pub focus: Pane,
    pub live: bool,

    // Send box
    pub input: Vec<char>,
    pub cursor: usize,
    pub history: Vec<HistoryItem>,
    hist_pos: Option<usize>,
    draft: Vec<char>,
    /// When an SSH session started, until ssh first prints something: a long
    /// silence gets a note (see `tick`).
    silent_since: Option<Instant>,
    /// Hidden (password) send in progress.
    pub secret: Option<Vec<char>>,

    pub macros: Vec<LoadedMacro>,
    pub run: Option<MacroRun>,
    capture_dir: PathBuf,

    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub status: Option<(String, bool, Instant)>,
    pub show_help: bool,
    pub menu: Option<Menu>,
    /// Where to write logs if logging is switched on mid-session (None with --no-log).
    pub log_dir: Option<PathBuf>,
    pub selection: Option<Selection>,
    /// Multi-line paste waiting for confirmation.
    pub paste_confirm: Option<Vec<String>>,
    /// Plain text of everything received, for "copy all output".
    plain: TextCleaner,
    plain_text: String,
    /// Pane areas and log text from the last draw (for the mouse).
    pub term_inner: Rect,
    pub log_inner: Rect,
    /// Entry id shown on each visible raw-log row.
    pub log_row_ids: Vec<u64>,
    /// Where the mouse is while dragging a selection (the draw resolves it to a position).
    pub drag_pos: Option<(u16, u16)>,
    last_autoscroll: Instant,
    next_id: u64,
    pub connected: bool,
    pub peer: Option<String>,

    // Filled in by the last draw.
    pub log_width: u16,
    pub log_height: u16,
    pub term_rows: u16,
    pub side_by_side: bool,
    pub cur_term_cols: u16,
    pub cur_log_rows: u16,
}

impl Monitor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: LinkSettings,
        profile_key: String,
        profile: Profile,
        layout: Layout,
        history: Vec<HistoryItem>,
        macros: Vec<LoadedMacro>,
        extractor: Extractor,
        capture_dir: PathBuf,
    ) -> Monitor {
        let focus = if layout == Layout::Hex {
            Pane::Log
        } else {
            Pane::Terminal
        };
        Monitor {
            settings,
            profile_key,
            profile,
            layout,
            view_chosen: false,
            footer_views: Vec::new(),
            link: None,
            logger: None,
            transcript: None,
            extractor,
            entries: VecDeque::new(),
            scroll: 0,
            follow: true,
            pending: None,
            term: Term::new(),
            focus,
            live: false,
            input: Vec::new(),
            cursor: 0,
            history,
            hist_pos: None,
            draft: Vec::new(),
            silent_since: None,
            secret: None,
            macros,
            run: None,
            capture_dir,
            rx_bytes: 0,
            tx_bytes: 0,
            status: None,
            show_help: false,
            menu: None,
            log_dir: None,
            selection: None,
            paste_confirm: None,
            plain: TextCleaner::default(),
            plain_text: String::new(),
            term_inner: Rect::default(),
            log_inner: Rect::default(),
            log_row_ids: Vec::new(),
            drag_pos: None,
            last_autoscroll: Instant::now(),
            next_id: 0,
            connected: false,
            peer: None,
            log_width: 0,
            log_height: 0,
            term_rows: 0,
            side_by_side: true,
            cur_term_cols: 0,
            cur_log_rows: 0,
        }
    }

    pub fn log_format(&self) -> LogFormat {
        match self.layout {
            Layout::Hex => self.profile.full_log,
            _ => self.profile.split_log,
        }
    }

    fn set_log_format(&mut self, f: LogFormat) {
        match self.layout {
            Layout::Hex => self.profile.full_log = f,
            _ => self.profile.split_log = f,
        }
        self.follow = true;
        self.scroll = 0;
    }

    pub fn attach(&mut self, link: Link, logger: Option<Logger>, note: Option<String>) {
        link.set_pacing(self.profile.char_delay_ms);
        // Telnet reports this size once the device asks for it.
        let (rows, cols) = self.term.size();
        if let Err(error) = link.resize(cols, rows) {
            log::warn!("Could not set the window size: {error}");
        }
        self.link = Some(link);
        let mut transcript_note = None;
        self.transcript = logger
            .as_ref()
            .and_then(|l| match Transcript::create(&l.path) {
                Ok(t) => Some(t),
                Err(error) => {
                    transcript_note = Some(format!("Could not create the transcript: {error}"));
                    None
                }
            });
        self.logger = logger;
        self.connected = true;
        self.peer = None;
        let msg = match &self.settings {
            LinkSettings::TcpListen { .. } => {
                format!(
                    "Listening: {}. Waiting for a client…",
                    self.settings.describe()
                )
            }
            // ssh is still logging in; the link notes when the server answered.
            LinkSettings::Ssh { .. } => format!("Starting {}", self.settings.describe()),
            _ => format!("Connected: {}", self.settings.describe()),
        };
        self.silent_since = matches!(self.settings, LinkSettings::Ssh { .. }).then(Instant::now);
        self.push(EntryKind::Good(msg));
        if let Some(l) = &self.logger {
            let p = l.path.display().to_string();
            self.push(EntryKind::Info(format!("Logging to {p}")));
        }
        if let Some(t) = &self.transcript {
            let p = t.path.display().to_string();
            self.push(EntryKind::Info(format!("Plain-text transcript: {p}")));
        }
        for n in note.into_iter().chain(transcript_note) {
            log::error!("{n}");
            self.push(EntryKind::Error(n));
        }
    }

    pub fn detach(&mut self, why: &str) {
        self.flush_pending();
        self.run = None;
        self.silent_since = None;
        if self.link.is_some() || self.logger.is_some() {
            self.push(EntryKind::Info(why.to_string()));
        }
        self.link = None;
        self.logger = None;
        self.transcript = None;
        self.connected = false;
        self.peer = None;
    }

    pub fn push(&mut self, kind: EntryKind) {
        let mut e = Entry::new(kind);
        self.next_id += 1;
        e.id = self.next_id;
        let failure = self.write_log(&e);
        self.push_entry(&mut e);
        self.entries.push_back(e);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
        if let Some(error) = failure {
            self.report_error(error);
        }
    }

    /// Writes an entry to the traffic log. On failure (disk full, drive gone)
    /// logging stops and the reason is returned, so it's reported once
    /// instead of failing on every entry.
    fn write_log(&mut self, e: &Entry) -> Option<String> {
        let logger = self.logger.as_mut()?;
        let error = logger.entry(e).err()?;
        let message = format!(
            "Logging stopped: could not write {}: {error}",
            logger.path.display()
        );
        self.logger = None;
        self.transcript = None;
        Some(message)
    }

    /// The device is asking for a password: what's typed or pasted now is
    /// logged as `<hidden>`. Checked on the screen's cursor row and on the
    /// device's current unfinished line, which doesn't depend on the pane's
    /// width, so a long prompt wrapped in a narrow pane still counts.
    pub fn at_password_prompt(&self) -> bool {
        self.term.at_password_prompt() || term::asks_for_secret(&self.plain.partial())
    }

    /// The terminal pane is drawn at this size: resize the emulator and tell the
    /// connection (Telnet reports it to the device).
    pub fn resize_terminal(&mut self, rows: u16, cols: u16) {
        if !self.term.resize(rows, cols) {
            return;
        }
        let (rows, cols) = self.term.size();
        let result = self.link.as_ref().map_or(Ok(()), |l| l.resize(cols, rows));
        if let Err(error) = result {
            self.report_error(format!("Could not send the window size: {error}"));
        }
    }

    /// Shows an error in the raw log and on the status line, and writes it to
    /// the session log.
    pub fn report_error(&mut self, message: String) {
        log::error!("{message}");
        self.set_status(message.clone(), true);
        self.push(EntryKind::Error(message));
    }

    fn push_entry(&mut self, e: &mut Entry) {
        if !self.follow && self.log_width > 0 {
            // Keep the view still while scrolled up.
            self.scroll += e.line_count(self.log_width, self.log_format());
        }
    }

    /// Group traffic into entries: a new entry starts on a change of direction, after a
    /// quiet gap, after a line feed, or every COALESCE_MAX bytes.
    fn traffic(&mut self, dir: Dir, hidden: bool, data: &[u8]) {
        let mut rest = data;
        while !rest.is_empty() {
            if self
                .pending
                .as_ref()
                .is_some_and(|p| p.dir != dir || p.hidden != hidden)
            {
                self.flush_pending();
            }
            let p = self.pending.get_or_insert_with(|| Pending {
                dir,
                hidden,
                data: Vec::new(),
                time: Local::now(),
                last: Instant::now(),
            });
            let room = COALESCE_MAX - p.data.len();
            let take = match rest.iter().take(room).position(|&b| b == b'\n') {
                Some(i) => i + 1,
                None => rest.len().min(room),
            };
            p.data.extend_from_slice(&rest[..take]);
            p.last = Instant::now();
            let full = p.data.len() >= COALESCE_MAX || p.data.last() == Some(&b'\n');
            rest = &rest[take..];
            if full {
                self.flush_pending();
            }
        }
    }

    fn flush_pending(&mut self) {
        let Some(p) = self.pending.take() else { return };
        let kind = match (p.dir, p.hidden) {
            (Dir::Rx, _) => EntryKind::Rx(p.data),
            (Dir::Tx, false) => EntryKind::Tx(p.data),
            (Dir::Tx, true) => EntryKind::TxHidden(p.data.len()),
        };
        self.next_id += 1;
        let mut e = Entry {
            id: self.next_id,
            time: p.time,
            kind,
            cache: None,
        };
        let failure = self.write_log(&e);
        self.push_entry(&mut e);
        self.entries.push_back(e);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
        if let Some(error) = failure {
            self.report_error(error);
        }
    }

    pub(crate) fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status = Some((msg.into(), is_error, Instant::now()));
    }

    pub fn status_text(&self) -> Option<(String, bool)> {
        if let Some(r) = &self.run {
            let (i, n) = r.progress();
            return Some((
                format!("Running macro '{}' (step {i}/{n}). Esc cancels.", r.label),
                false,
            ));
        }
        self.status
            .as_ref()
            .filter(|(_, _, t)| t.elapsed() < Duration::from_secs(4))
            .map(|(m, e, _)| (m.clone(), *e))
    }

    pub fn input_text(&self) -> String {
        self.input.iter().collect()
    }

    /// What Enter would send right now.
    pub fn preview(&self) -> Result<Vec<u8>, String> {
        codec::encode(
            &self.input_text(),
            self.profile.input_mode,
            self.profile.line_ending,
        )
    }

    /// Called every frame.
    pub fn tick(&mut self) {
        self.autoscroll();
        if self
            .silent_since
            .is_some_and(|t| t.elapsed() >= SSH_SILENCE_NOTE)
        {
            self.silent_since = None;
            self.push(EntryKind::Info(
                "ssh hasn't printed anything for 10 s. It may still be logging in (slow name lookups or Kerberos/GSSAPI can take a while). Ctrl+C stops it.".into(),
            ));
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.last.elapsed() >= COALESCE_IDLE)
        {
            self.flush_pending();
        }
        while let Some(run) = self.run.as_mut() {
            match run.poll() {
                Poll::Pending => break,
                Poll::Send(b) => {
                    if !self.send_bytes(b, false) {
                        self.run = None;
                        self.push(EntryKind::Error("Macro stopped: send failed".into()));
                        break;
                    }
                }
                Poll::Done { capture } => {
                    let label = run.label.clone();
                    self.run = None;
                    self.finish_macro(&label, capture);
                    break;
                }
                Poll::Failed(e) => {
                    let label = run.label.clone();
                    self.run = None;
                    self.push(EntryKind::Error(format!("Macro '{label}' stopped: {e}")));
                    break;
                }
            }
        }
    }

    fn finish_macro(&mut self, label: &str, capture: Option<String>) {
        let Some(text) = capture else {
            self.set_status(format!("Macro '{label}' done"), false);
            return;
        };
        let safe: String = label
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let name = format!("{safe}_{}.txt", Local::now().format("%Y%m%d_%H%M%S"));
        let path = self.capture_dir.join(name);
        let result =
            std::fs::create_dir_all(&self.capture_dir).and_then(|_| std::fs::write(&path, &text));
        match result {
            Ok(()) => self.push(EntryKind::Good(format!(
                "Macro '{label}' done. Saved {} lines to {}",
                text.lines().count(),
                path.display()
            ))),
            Err(e) => self.push(EntryKind::Error(format!(
                "Macro '{label}': couldn't save capture: {e}"
            ))),
        }
    }

    pub fn on_link_event(&mut self, ev: LinkEvent) {
        // A normal end (logged out) is a note; anything else is an error.
        let normal_end = matches!(ev, LinkEvent::Ended(_));
        match ev {
            LinkEvent::Data(d) => {
                self.silent_since = None;
                self.rx_bytes += d.len() as u64;
                self.term.feed(&d);
                self.plain.feed(&d, &mut self.plain_text);
                if self.plain_text.len() > MAX_PLAIN {
                    let mut cut = self.plain_text.len() - MAX_PLAIN / 2;
                    while !self.plain_text.is_char_boundary(cut) {
                        cut += 1;
                    }
                    self.plain_text.drain(..cut);
                }
                if let Some(t) = self.transcript.as_mut()
                    && let Err(error) = t.feed(&d)
                {
                    let message = format!(
                        "Transcript stopped: could not write {}: {error}",
                        t.path.display()
                    );
                    self.transcript = None;
                    self.report_error(message);
                }
                if let Some(r) = self.run.as_mut() {
                    r.on_rx(&d);
                }
                let found = self.extractor.feed(&d);
                self.traffic(Dir::Rx, false, &d);
                if !found.is_empty() {
                    self.flush_pending();
                }
                for f in found {
                    match f {
                        ExtractEvent::Saved { path, len } => self.push(EntryKind::Good(format!(
                            "Extracted image: {} ({len} bytes)",
                            path.display()
                        ))),
                        ExtractEvent::Failed(m) => self.push(EntryKind::Error(m)),
                        ExtractEvent::Skipped(m) => self.push(EntryKind::Info(m)),
                    }
                }
            }
            LinkEvent::Closed(m) | LinkEvent::Ended(m) => {
                self.silent_since = None;
                self.flush_pending();
                self.link = None;
                self.connected = false;
                self.peer = None;
                self.live = false;
                self.run = None;
                if normal_end {
                    log::info!("{m}");
                    self.push(EntryKind::Info(m));
                } else {
                    log::warn!("{m}");
                    self.push(EntryKind::Error(m));
                }
                self.push(EntryKind::Info(
                    "Ctrl+R reconnects, Ctrl+N picks a new connection.".into(),
                ));
            }
            LinkEvent::PeerConnected(a) => {
                self.flush_pending();
                self.push(EntryKind::Good(format!("Client connected from {a}")));
                self.peer = Some(a);
            }
            LinkEvent::PeerDisconnected(m) => {
                self.flush_pending();
                self.peer = None;
                self.push(EntryKind::Info(m));
            }
            LinkEvent::SendError(m) => {
                self.flush_pending();
                self.push(EntryKind::Error(format!("Send failed: {m}")));
            }
            LinkEvent::Info(m) => {
                self.flush_pending();
                log::info!("{m}");
                self.push(EntryKind::Info(m));
            }
        }
    }

    /// Send one complete message (line mode, macros, hidden send) as its own log entry.
    fn send_bytes(&mut self, data: Vec<u8>, hidden: bool) -> bool {
        let Some(link) = self.link.as_ref() else {
            self.set_status("Not connected. Ctrl+R to reconnect.", true);
            return false;
        };
        match link.send(&data) {
            Ok(()) => {
                self.tx_bytes += data.len() as u64;
                self.flush_pending();
                self.traffic(Dir::Tx, hidden, &data);
                self.flush_pending();
                self.term.scroll_to(0);
                true
            }
            Err(e) => {
                self.push(EntryKind::Error(format!("Send failed: {e}")));
                false
            }
        }
    }

    /// Live keys: send immediately, grouped into the log like received data.
    fn live_send(&mut self, data: Vec<u8>) {
        let Some(link) = self.link.as_ref() else {
            self.set_status("Not connected. Ctrl+R to reconnect.", true);
            return;
        };
        self.term.scroll_to(0);
        self.selection = None;
        let hidden = self.at_password_prompt();
        match link.send(&data) {
            Ok(()) => {
                self.tx_bytes += data.len() as u64;
                self.traffic(Dir::Tx, hidden, &data);
            }
            Err(e) => self.push(EntryKind::Error(format!("Send failed: {e}"))),
        }
    }

    fn submit(&mut self) {
        let text = self.input_text();
        let mode = self.profile.input_mode;
        match self.preview() {
            Err(e) => self.set_status(e, true),
            Ok(bytes) if bytes.is_empty() => self.set_status("Nothing to send", true),
            Ok(bytes) => {
                if self.send_bytes(bytes, false) {
                    if !text.is_empty() {
                        let dup = self
                            .history
                            .last()
                            .is_some_and(|h| h.text == text && h.mode == mode);
                        if !dup {
                            self.history.push(HistoryItem { mode, text });
                            if self.history.len() > MAX_HISTORY {
                                self.history.remove(0);
                            }
                        }
                    }
                    self.input.clear();
                    self.cursor = 0;
                    self.hist_pos = None;
                }
            }
        }
    }

    fn submit_secret(&mut self) {
        let Some(chars) = self.secret.take() else {
            return;
        };
        let mut bytes: Vec<u8> = chars.iter().collect::<String>().into_bytes();
        bytes.extend_from_slice(self.profile.line_ending.bytes());
        if bytes.is_empty() {
            return;
        }
        if self.send_bytes(bytes, true) {
            self.set_status("Hidden send done (not logged, not in history)", false);
        }
    }

    fn start_macro(&mut self, i: usize) {
        if self.run.is_some() {
            self.set_status("A macro is already running. Esc cancels it.", true);
            return;
        }
        match MacroRun::new(&self.macros[i], self.profile.line_ending) {
            Ok(r) => {
                let (_, n) = r.progress();
                if n > 1 {
                    let msg = format!("Macro '{}' started", r.label);
                    self.push(EntryKind::Info(msg));
                }
                self.run = Some(r);
                self.tick();
            }
            Err(e) => {
                let msg = format!("Macro {} is invalid: {e}", self.macros[i].key.label());
                self.set_status(msg, true);
            }
        }
    }

    fn load_history(&mut self, i: usize) {
        let item = &self.history[i];
        self.input = item.text.chars().collect();
        self.profile.input_mode = item.mode;
        self.cursor = self.input.len();
        self.hist_pos = Some(i);
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let i = match self.hist_pos {
            None => {
                self.draft = self.input.clone();
                self.history.len() - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.load_history(i);
    }

    fn history_next(&mut self) {
        let Some(p) = self.hist_pos else { return };
        if p + 1 < self.history.len() {
            self.load_history(p + 1);
        } else {
            self.hist_pos = None;
            self.input = std::mem::take(&mut self.draft);
            self.cursor = self.input.len();
        }
    }
}
