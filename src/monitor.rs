//! The monitor screen's state: raw log, live terminal, send box, macros, and key handling.

use crate::codec;
use crate::config::{HistoryItem, Layout, LogFormat, Profile};
use crate::extract::{ExtractEvent, Extractor};
use crate::link::{Link, LinkEvent, LinkSettings};
use crate::logger::Logger;
use crate::macros::{LoadedMacro, MacroRun, Poll};
use crate::menu::{self, Cmd, Kind, Menu, Setting};
use crate::term::{self, Term, Transcript};
use chrono::{DateTime, Local};
use crate::term::TextCleaner;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const MAX_ENTRIES: usize = 20_000;
const MAX_HISTORY: usize = 200;
/// Bytes arriving closer together than this are shown as one log entry.
const COALESCE_IDLE: Duration = Duration::from_millis(30);
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
        Entry { id: 0, time: Local::now(), kind, cache: None }
    }

    /// Number of screen rows this entry takes at a given width (cached).
    pub fn line_count(&mut self, width: u16, fmt: LogFormat) -> usize {
        if let Some((w, f, n)) = self.cache {
            if w == width && f == fmt {
                return n;
            }
        }
        let n = crate::ui_monitor::entry_lines(self, width, fmt).len();
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
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
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
}

// ---------------------------------------------------------------- monitor

pub struct Monitor {
    pub settings: LinkSettings,
    pub profile_key: String,
    pub profile: Profile,
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
        history: Vec<HistoryItem>,
        macros: Vec<LoadedMacro>,
        extractor: Extractor,
        capture_dir: PathBuf,
    ) -> Monitor {
        let focus = if profile.layout == Layout::Log { Pane::Log } else { Pane::Terminal };
        Monitor {
            settings,
            profile_key,
            profile,
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
        match self.profile.layout {
            Layout::Log => self.profile.full_log,
            _ => self.profile.split_log,
        }
    }

    fn set_log_format(&mut self, f: LogFormat) {
        match self.profile.layout {
            Layout::Log => self.profile.full_log = f,
            _ => self.profile.split_log = f,
        }
        self.follow = true;
        self.scroll = 0;
    }

    pub fn attach(&mut self, link: Link, logger: Option<Logger>, note: Option<String>) {
        link.set_pacing(self.profile.char_delay_ms);
        self.link = Some(link);
        self.transcript = logger.as_ref().and_then(|l| Transcript::create(&l.path).ok());
        self.logger = logger;
        self.connected = true;
        self.peer = None;
        let msg = match &self.settings {
            LinkSettings::TcpListen { .. } => {
                format!("Listening: {}. Waiting for a client…", self.settings.describe())
            }
            _ => format!("Connected: {}", self.settings.describe()),
        };
        self.push(EntryKind::Good(msg));
        if let Some(l) = &self.logger {
            let p = l.path.display().to_string();
            self.push(EntryKind::Info(format!("Logging to {p}")));
        }
        if let Some(t) = &self.transcript {
            let p = t.path.display().to_string();
            self.push(EntryKind::Info(format!("Plain-text transcript: {p}")));
        }
        if let Some(n) = note {
            self.push(EntryKind::Error(n));
        }
    }

    pub fn detach(&mut self, why: &str) {
        self.flush_pending();
        self.run = None;
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
        if let Some(l) = self.logger.as_mut() {
            l.entry(&e);
        }
        self.push_entry(&mut e);
        self.entries.push_back(e);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
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
            if self.pending.as_ref().is_some_and(|p| p.dir != dir || p.hidden != hidden) {
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
        let mut e = Entry { id: self.next_id, time: p.time, kind, cache: None };
        if let Some(l) = self.logger.as_mut() {
            l.entry(&e);
        }
        self.push_entry(&mut e);
        self.entries.push_back(e);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
    }

    fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status = Some((msg.into(), is_error, Instant::now()));
    }

    pub fn status_text(&self) -> Option<(String, bool)> {
        if let Some(r) = &self.run {
            let (i, n) = r.progress();
            return Some((format!("Running macro '{}' (step {i}/{n}). Esc cancels.", r.label), false));
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
        codec::encode(&self.input_text(), self.profile.input_mode, self.profile.line_ending)
    }

    /// Called every frame.
    pub fn tick(&mut self) {
        self.autoscroll();
        if self.pending.as_ref().is_some_and(|p| p.last.elapsed() >= COALESCE_IDLE) {
            self.flush_pending();
        }
        loop {
            let Some(run) = self.run.as_mut() else { break };
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
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        let name = format!("{safe}_{}.txt", Local::now().format("%Y%m%d_%H%M%S"));
        let path = self.capture_dir.join(name);
        let result = std::fs::create_dir_all(&self.capture_dir).and_then(|_| std::fs::write(&path, &text));
        match result {
            Ok(()) => self.push(EntryKind::Good(format!(
                "Macro '{label}' done. Saved {} lines to {}",
                text.lines().count(),
                path.display()
            ))),
            Err(e) => self.push(EntryKind::Error(format!("Macro '{label}': couldn't save capture: {e}"))),
        }
    }

    pub fn on_link_event(&mut self, ev: LinkEvent) {
        match ev {
            LinkEvent::Data(d) => {
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
                if let Some(t) = self.transcript.as_mut() {
                    t.feed(&d);
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
            LinkEvent::Closed(m) => {
                self.flush_pending();
                self.link = None;
                self.connected = false;
                self.peer = None;
                self.live = false;
                self.run = None;
                self.push(EntryKind::Error(m));
                self.push(EntryKind::Info("Ctrl+R reconnects, Ctrl+N picks a new connection.".into()));
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
        let hidden = self.term.at_password_prompt();
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
                        let dup = self.history.last().is_some_and(|h| h.text == text && h.mode == mode);
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
        let Some(chars) = self.secret.take() else { return };
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

    fn scroll_pane(&mut self, pages: isize) {
        match self.focus {
            Pane::Terminal => {
                let page = (self.term_rows as isize - 1).max(1);
                self.term.scroll_by(pages * page);
            }
            Pane::Log => {
                let page = (self.log_height as usize).saturating_sub(1).max(1);
                if pages > 0 {
                    self.follow = false;
                    self.scroll += page * pages as usize;
                } else {
                    self.scroll = self.scroll.saturating_sub(page * (-pages) as usize);
                    if self.scroll == 0 {
                        self.follow = true;
                    }
                }
            }
        }
    }

    fn scroll_end(&mut self, top: bool) {
        match (self.focus, top) {
            (Pane::Terminal, true) => self.term.scroll_to(usize::MAX),
            (Pane::Terminal, false) => self.term.scroll_to(0),
            (Pane::Log, true) => {
                self.follow = false;
                self.scroll = usize::MAX / 4; // clamped when drawn
            }
            (Pane::Log, false) => {
                self.follow = true;
                self.scroll = 0;
            }
        }
    }

    fn set_layout(&mut self, l: Layout) {
        self.profile.layout = l;
        match l {
            Layout::Log => {
                self.focus = Pane::Log;
                if self.live {
                    self.live = false;
                    self.set_status("Live keys off: no terminal pane in log-only layout", false);
                    return;
                }
            }
            Layout::Terminal => self.focus = Pane::Terminal,
            Layout::Split => {}
        }
        self.follow = true;
        self.scroll = 0;
        self.set_status(format!("Layout: {}", l.label()), false);
    }

    fn copy_selection(&mut self) {
        let Some(s) = self.selection.filter(|s| !s.is_empty()) else {
            self.set_status("Nothing selected. Drag with the mouse to select.", true);
            return;
        };
        let (start, end) = s.ordered();
        let (text, what) = match s.pane {
            Pane::Terminal => {
                let t = self.term.text_between(start, end);
                let n = t.lines().count().max(1);
                (t, format!("{n} line{}", if n == 1 { "" } else { "s" }))
            }
            Pane::Log => {
                // Whole entries, in full, in log-file form, whatever the on-screen format.
                let (lo, hi) = (start.0 as u64, end.0 as u64);
                let picked: Vec<String> = self
                    .entries
                    .iter()
                    .filter(|e| e.id >= lo && e.id <= hi)
                    .map(crate::logger::format_entry)
                    .collect();
                let n = picked.len();
                (picked.concat(), format!("{n} log entr{}", if n == 1 { "y" } else { "ies" }))
            }
        };
        match crate::clip::set(&text) {
            Ok(()) => {
                self.set_status(format!("Copied {what}"), false);
                self.selection = None;
            }
            Err(e) => self.set_status(e, true),
        }
    }

    fn paste_clipboard(&mut self) {
        match crate::clip::get() {
            Ok(t) if !t.is_empty() => self.on_paste(t),
            Ok(_) => self.set_status("Clipboard is empty", true),
            Err(e) => self.set_status(e, true),
        }
    }

    /// Pasted text, from Ctrl+Shift+V, right-click, or the console typing a paste in.
    pub fn on_paste(&mut self, text: String) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let ends_with_newline = text.ends_with('\n');
        let lines: Vec<String> = text.trim_end_matches('\n').split('\n').map(str::to_string).collect();
        if let Some(menu) = self.menu.as_mut() {
            menu.filter.push_str(lines[0].trim());
            menu.selected = 0;
            return;
        }
        if let Some(secret) = self.secret.as_mut() {
            secret.extend(lines[0].chars());
            return;
        }
        if lines.len() > 1 {
            self.paste_confirm = Some(lines);
            return;
        }
        let line = &lines[0];
        if self.live {
            let mut b = line.as_bytes().to_vec();
            if ends_with_newline {
                b.extend_from_slice(self.profile.line_ending.bytes());
            }
            self.live_send(b);
        } else {
            for c in line.chars() {
                self.input.insert(self.cursor, c);
                self.cursor += 1;
            }
            self.hist_pos = None;
            if ends_with_newline {
                self.submit();
            }
        }
    }

    fn send_confirmed_paste(&mut self) {
        let Some(lines) = self.paste_confirm.take() else { return };
        let n = lines.len();
        for l in lines {
            let mut b = l.into_bytes();
            b.extend_from_slice(self.profile.line_ending.bytes());
            if !self.send_bytes(b, false) {
                return;
            }
        }
        let pace = match self.profile.char_delay_ms {
            0 => String::new(),
            ms => format!(" at {ms} ms/char"),
        };
        self.set_status(format!("Pasted {n} lines{pace}"), false);
    }

    fn pane_at(&self, x: u16, y: u16) -> Option<Pane> {
        let hit = |r: Rect| r.width > 0 && x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
        if hit(self.term_inner) {
            Some(Pane::Terminal)
        } else if hit(self.log_inner) {
            Some(Pane::Log)
        } else {
            None
        }
    }

    fn pane_rect(&self, p: Pane) -> Rect {
        match p {
            Pane::Terminal => self.term_inner,
            Pane::Log => self.log_inner,
        }
    }

    /// Screen point → selection position in a pane, clamped to the pane.
    pub fn pos_at(&mut self, pane: Pane, x: u16, y: u16) -> (usize, u16) {
        let r = self.pane_rect(pane);
        let row = y.saturating_sub(r.y).min(r.height.saturating_sub(1));
        let col = x.saturating_sub(r.x).min(r.width.saturating_sub(1));
        match pane {
            Pane::Terminal => (self.term.abs_top() + row as usize, col),
            Pane::Log => {
                let id = self.log_row_ids.get(row as usize).or(self.log_row_ids.last()).copied().unwrap_or(0);
                (id as usize, 0)
            }
        }
    }

    fn scroll_lines(&mut self, pane: Pane, up: bool, n: usize) {
        match pane {
            Pane::Terminal => self.term.scroll_by(if up { n as isize } else { -(n as isize) }),
            Pane::Log => {
                if up {
                    self.follow = false;
                    self.scroll += n;
                } else {
                    self.scroll = self.scroll.saturating_sub(n);
                    if self.scroll == 0 {
                        self.follow = true;
                    }
                }
            }
        }
    }

    /// While dragging above or below a pane, keep scrolling it so the selection grows.
    fn autoscroll(&mut self) {
        let (Some(s), Some((_, y))) = (self.selection, self.drag_pos) else { return };
        if !s.dragging || self.last_autoscroll.elapsed() < Duration::from_millis(40) {
            return;
        }
        let r = self.pane_rect(s.pane);
        if r.height == 0 {
            return;
        }
        if y < r.y {
            self.scroll_lines(s.pane, true, 1 + (r.y - y) as usize / 2);
        } else if y >= r.y + r.height {
            self.scroll_lines(s.pane, false, 1 + (y - (r.y + r.height)) as usize / 2);
        } else {
            return;
        }
        self.last_autoscroll = Instant::now();
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        if self.show_help || self.paste_confirm.is_some() {
            return;
        }
        if self.menu.is_some() {
            let code = match ev.kind {
                MouseEventKind::ScrollUp => KeyCode::Up,
                MouseEventKind::ScrollDown => KeyCode::Down,
                _ => return,
            };
            self.menu_key(KeyEvent::new(code, KeyModifiers::NONE));
            return;
        }
        let (x, y) = (ev.column, ev.row);
        let extend = ev.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        match ev.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                // While dragging, the wheel scrolls the pane being selected and the selection follows.
                let pane = match self.selection {
                    Some(s) if s.dragging => s.pane,
                    _ => self.pane_at(x, y).unwrap_or(self.focus),
                };
                self.scroll_lines(pane, ev.kind == MouseEventKind::ScrollUp, WHEEL_LINES);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(p) = self.pane_at(x, y) else {
                    self.selection = None;
                    return;
                };
                if self.profile.layout == Layout::Split {
                    self.focus = p;
                }
                let pos = self.pos_at(p, x, y);
                self.drag_pos = Some((x, y));
                match self.selection {
                    // Ctrl+click (or Shift+click where the console passes it) extends the selection.
                    Some(s) if extend && s.pane == p => {
                        self.selection = Some(Selection { head: pos, dragging: true, moved: true, ..s });
                    }
                    _ => {
                        self.selection = Some(Selection { pane: p, anchor: pos, head: pos, dragging: true, moved: false });
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(s) = self.selection.filter(|s| s.dragging) {
                    self.drag_pos = Some((x, y));
                    let head = self.pos_at(s.pane, x, y);
                    let moved = s.moved || head != s.anchor;
                    self.selection = Some(Selection { head, moved, ..s });
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.drag_pos = None;
                if let Some(s) = self.selection.as_mut() {
                    s.dragging = false;
                    if s.is_empty() {
                        self.selection = None;
                    }
                }
            }
            // Windows Terminal style: right-click copies a selection, otherwise pastes.
            MouseEventKind::Down(MouseButton::Right) => {
                if self.selection.is_some_and(|s| !s.is_empty()) {
                    self.copy_selection();
                } else {
                    self.paste_clipboard();
                }
            }
            _ => {}
        }
    }

    /// Run a command from a shortcut key or the menu.
    pub fn run_cmd(&mut self, c: Cmd) -> Action {
        match c {
            Cmd::HiddenSend => self.secret = Some(Vec::new()),
            Cmd::Break => match self.link.as_ref().map(|l| l.send_break(BREAK_LEN)) {
                Some(Ok(())) => self.push(EntryKind::Info(format!("Sent break ({} ms)", BREAK_LEN.as_millis()))),
                Some(Err(e)) => self.set_status(format!("Break failed: {e}"), true),
                None => self.set_status("Not connected", true),
            },
            Cmd::Macro(i) => self.start_macro(i),
            Cmd::ToggleLive => {
                self.live = !self.live;
                if self.live {
                    if self.profile.layout == Layout::Log {
                        self.profile.layout = Layout::Split;
                    }
                    self.focus = Pane::Terminal;
                    self.term.scroll_to(0);
                    self.set_status("Live keys on", false);
                } else {
                    self.set_status("Line mode", false);
                }
            }
            Cmd::PauseLog => {
                self.follow = !self.follow;
                if self.follow {
                    self.scroll = 0;
                }
                self.set_status(if self.follow { "Raw log following" } else { "Raw log paused" }, false);
            }
            Cmd::Clear => {
                self.entries.clear();
                self.scroll = 0;
                self.follow = true;
                self.term.clear();
            }
            Cmd::FocusPane => {
                if self.profile.layout == Layout::Split {
                    self.focus = match self.focus {
                        Pane::Log => Pane::Terminal,
                        Pane::Terminal => Pane::Log,
                    };
                    let name = if self.focus == Pane::Log { "raw log" } else { "terminal" };
                    self.set_status(format!("PgUp/PgDn now scroll the {name}"), false);
                }
            }
            Cmd::Shortcuts => self.show_help = true,
            Cmd::Copy => self.copy_selection(),
            Cmd::Paste => self.paste_clipboard(),
            Cmd::CopyAll => {
                let mut text = self.plain_text.clone();
                text.push_str(&self.plain.partial());
                let lines = text.lines().count();
                match crate::clip::set(&text) {
                    Ok(()) => self.set_status(format!("Copied all terminal output ({lines} lines)"), false),
                    Err(e) => self.set_status(e, true),
                }
            }
            Cmd::OpenTranscript => match &self.transcript {
                Some(t) => {
                    let _ = std::process::Command::new("notepad.exe").arg(&t.path).spawn();
                    self.set_status("Opened the transcript in Notepad", false);
                }
                None => self.set_status("No transcript: logging is off for this session", true),
            },
            Cmd::Reconnect => return Action::Reconnect,
            Cmd::NewConnection => return Action::NewConnection,
            Cmd::Quit => return Action::Quit,
        }
        Action::None
    }

    /// Change a per-connection setting one step (dir = +1 / -1).
    pub fn change_setting(&mut self, s: Setting, dir: i32) {
        match s {
            Setting::LineEnding => {
                use crate::codec::LineEnding as L;
                self.profile.line_ending = menu::step(&[L::None, L::Cr, L::Lf, L::Crlf], self.profile.line_ending, dir);
                self.set_status(format!("Line ending: {}", self.profile.line_ending.label()), false);
            }
            Setting::CharDelay => {
                let v = menu::step(&menu::CHAR_DELAYS, self.profile.char_delay_ms, dir);
                self.profile.char_delay_ms = v;
                if let Some(l) = &self.link {
                    l.set_pacing(v);
                }
                let msg = match v {
                    0 => "Char delay: off".to_string(),
                    ms => format!("Char delay: {ms} ms"),
                };
                self.set_status(msg, false);
            }
            Setting::InputMode => {
                self.profile.input_mode = menu::step(&menu::input_modes(), self.profile.input_mode, dir);
                self.hist_pos = None;
            }
            Setting::Layout => {
                let l = menu::step(&[Layout::Split, Layout::Log, Layout::Terminal], self.profile.layout, dir);
                self.set_layout(l);
            }
            Setting::LogFormat => {
                use crate::config::LogFormat as F;
                let f = menu::step(&[F::Compact, F::Both, F::Hex, F::Text], self.log_format(), dir);
                self.set_log_format(f);
                self.set_status(format!("Raw log format: {}", f.label()), false);
            }
            Setting::Logging => self.toggle_logging(),
            // On/off switches: either direction flips them.
            Setting::LiveKeys => {
                self.run_cmd(Cmd::ToggleLive);
            }
            Setting::ScrollPane => {
                self.run_cmd(Cmd::FocusPane);
            }
            Setting::LogFollow => {
                self.run_cmd(Cmd::PauseLog);
            }
        }
    }

    fn toggle_logging(&mut self) {
        self.flush_pending();
        if self.logger.is_some() {
            self.push(EntryKind::Info("Logging stopped for this session".into()));
            self.logger = None;
            self.transcript = None;
            self.set_status("Logging off", false);
            return;
        }
        let Some(dir) = self.log_dir.clone() else {
            self.set_status("Logging is disabled (started with --no-log)", true);
            return;
        };
        match Logger::create(&dir, self.settings.kind(), &self.settings.describe()) {
            Ok(l) => {
                self.transcript = Transcript::create(&l.path).ok();
                let p = l.path.display().to_string();
                self.logger = Some(l);
                self.push(EntryKind::Info(format!("Logging to {p}")));
                self.set_status("Logging on", false);
            }
            Err(e) => self.set_status(format!("Couldn't start logging: {e}"), true),
        }
    }

    fn menu_key(&mut self, k: KeyEvent) -> Action {
        let Some(mut menu) = self.menu.take() else { return Action::None };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let items = menu::visible(self, &menu.filter);
        let n = items.len();
        let sel = menu.selected.min(n.saturating_sub(1));
        match k.code {
            KeyCode::Esc => {
                if menu.filter.is_empty() {
                    return Action::None; // closed
                }
                menu.filter.clear();
                menu.selected = 0;
            }
            KeyCode::F(1) => return Action::None,
            KeyCode::Char('p') if ctrl && !alt => return Action::None,
            KeyCode::Char('q') if ctrl && !alt => return Action::Quit,
            KeyCode::Up if n > 0 => menu.selected = (sel + n - 1) % n,
            KeyCode::Down | KeyCode::Tab if n > 0 => menu.selected = (sel + 1) % n,
            KeyCode::BackTab if n > 0 => menu.selected = (sel + n - 1) % n,
            KeyCode::PageUp => menu.selected = sel.saturating_sub(8),
            KeyCode::PageDown => menu.selected = (sel + 8).min(n.saturating_sub(1)),
            KeyCode::Home => menu.selected = 0,
            KeyCode::End => menu.selected = n.saturating_sub(1),
            KeyCode::Left | KeyCode::Right => {
                if let Some(Kind::Setting(s)) = items.get(sel).map(|i| i.kind) {
                    self.change_setting(s, if k.code == KeyCode::Left { -1 } else { 1 });
                }
            }
            KeyCode::Enter => match items.get(sel).map(|i| i.kind) {
                Some(Kind::Setting(s)) => self.change_setting(s, 1),
                Some(Kind::Cmd(c)) => return self.run_cmd(c), // menu closes
                None => {}
            },
            KeyCode::Backspace => {
                menu.filter.pop();
                menu.selected = 0;
            }
            KeyCode::Char(c) if !ctrl || alt => {
                menu.filter.push(c);
                menu.selected = 0;
            }
            _ => {}
        }
        let n = menu::visible(self, &menu.filter).len();
        menu.selected = menu.selected.min(n.saturating_sub(1));
        self.menu = Some(menu);
        Action::None
    }

    pub fn on_key(&mut self, k: KeyEvent) -> Action {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);

        if self.show_help {
            self.show_help = false;
            return Action::None;
        }
        if self.paste_confirm.is_some() {
            match k.code {
                KeyCode::Enter => self.send_confirmed_paste(),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.paste_confirm = None;
                    self.set_status("Paste cancelled", false);
                }
                _ => {}
            }
            return Action::None;
        }
        // Terminal-style copy / paste (also works in live keys: Ctrl+C alone still goes to the device).
        if ctrl && k.modifiers.contains(KeyModifiers::SHIFT) {
            match k.code {
                KeyCode::Char(c) if c.eq_ignore_ascii_case(&'c') => {
                    self.copy_selection();
                    return Action::None;
                }
                KeyCode::Char(c) if c.eq_ignore_ascii_case(&'v') => {
                    self.paste_clipboard();
                    return Action::None;
                }
                KeyCode::Char(c) if c.eq_ignore_ascii_case(&'q') => return Action::Quit,
                _ => {}
            }
        }
        if self.menu.is_some() {
            return self.menu_key(k);
        }

        // Hidden send captures every key until Enter / Esc.
        if let Some(secret) = self.secret.as_mut() {
            match k.code {
                KeyCode::Enter => self.submit_secret(),
                KeyCode::Esc | KeyCode::F(6) => {
                    self.secret = None;
                    self.set_status("Hidden send cancelled", false);
                }
                KeyCode::Backspace => {
                    secret.pop();
                }
                KeyCode::Char(c) if !ctrl || alt => secret.push(c),
                _ => {}
            }
            return Action::None;
        }

        if let Some(i) = self.macros.iter().position(|m| m.key.matches(&k)) {
            self.start_macro(i);
            return Action::None;
        }

        // Shortcuts that work in both line and live mode (all are also in the F1 menu).
        match k.code {
            KeyCode::F(1) => self.menu = Some(Menu::default()),
            KeyCode::F(2) => self.change_setting(Setting::LineEnding, 1),
            KeyCode::F(3) => self.change_setting(Setting::Layout, 1),
            KeyCode::F(4) => return self.run_cmd(Cmd::ToggleLive),
            KeyCode::F(5) => return self.run_cmd(Cmd::FocusPane),
            KeyCode::F(6) => return self.run_cmd(Cmd::HiddenSend),
            KeyCode::F(7) => return self.run_cmd(Cmd::Break),
            KeyCode::F(8) => self.change_setting(Setting::LogFormat, 1),
            KeyCode::F(9) => return self.run_cmd(Cmd::PauseLog),
            KeyCode::F(10) => return self.run_cmd(Cmd::Clear),
            KeyCode::PageUp => self.scroll_pane(1),
            KeyCode::PageDown => self.scroll_pane(-1),
            KeyCode::Home if ctrl => self.scroll_end(true),
            KeyCode::End if ctrl => self.scroll_end(false),
            _ => {
                if self.live {
                    let app_cursor = self.term.screen().application_cursor();
                    if let Some(b) = term::key_bytes(&k, app_cursor, self.profile.line_ending.bytes()) {
                        self.live_send(b);
                    }
                    return Action::None;
                }
                return self.line_key(k);
            }
        }
        Action::None
    }

    fn line_key(&mut self, k: KeyEvent) -> Action {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Char('q') if ctrl && !alt => return Action::Quit,
            KeyCode::Char('c') if ctrl && !alt => {
                if self.selection.is_some_and(|s| !s.is_empty()) {
                    self.copy_selection();
                } else {
                    self.set_status("Ctrl+Q quits · Ctrl+Shift+C copies a selection", false);
                }
            }
            KeyCode::Char('r') if ctrl && !alt => return Action::Reconnect,
            KeyCode::Char('n') if ctrl && !alt => return Action::NewConnection,
            KeyCode::Char('t') if ctrl && !alt => self.change_setting(Setting::CharDelay, 1),
            KeyCode::Char('p') if ctrl && !alt => self.menu = Some(Menu::default()),
            KeyCode::Tab | KeyCode::BackTab => self.change_setting(Setting::InputMode, 1),
            KeyCode::Enter => self.submit(),
            KeyCode::Esc => {
                if self.run.take().is_some() {
                    self.push(EntryKind::Info("Macro cancelled".into()));
                } else {
                    self.input.clear();
                    self.cursor = 0;
                    self.hist_pos = None;
                }
            }
            // Divider: Ctrl+←/→ widens/narrows the terminal, Ctrl+↑/↓ grows/shrinks the log.
            KeyCode::Left if ctrl => self.profile.term_cols = self.cur_term_cols.saturating_add(4),
            KeyCode::Right if ctrl => self.profile.term_cols = self.cur_term_cols.saturating_sub(4).max(20),
            KeyCode::Up if ctrl => self.profile.log_rows = self.cur_log_rows.saturating_add(1),
            KeyCode::Down if ctrl => self.profile.log_rows = self.cur_log_rows.saturating_sub(1).max(3),
            KeyCode::Up => self.history_prev(),
            KeyCode::Down => self.history_next(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.input.remove(self.cursor);
                    self.hist_pos = None;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len() {
                    self.input.remove(self.cursor);
                    self.hist_pos = None;
                }
            }
            // AltGr arrives as Ctrl+Alt on Windows, so allow that for typing.
            KeyCode::Char(c) if !ctrl || alt => {
                self.input.insert(self.cursor, c);
                self.cursor += 1;
                self.hist_pos = None;
            }
            _ => {}
        }
        Action::None
    }
}
