//! The live terminal pane (a vt100 emulator), keystroke → bytes for live-keys mode, and a
//! plain-text cleaner used for transcripts and macro captures.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

const SCROLLBACK: usize = 10_000;

fn nz(v: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(v.max(1)).unwrap()
}

pub struct Term {
    parser: vt100::Parser,
    last_rx: u8,
}

impl Term {
    pub fn new() -> Term {
        Term {
            parser: vt100::Parser::new(nz(24), nz(80), SCROLLBACK),
            last_rx: 0,
        }
    }

    /// Feed received bytes. A bare LF also returns the cursor to column 0, so devices that
    /// send only LF don't staircase across the screen.
    pub fn feed(&mut self, data: &[u8]) {
        let mut buf = Vec::with_capacity(data.len() + 8);
        for &b in data {
            if b == b'\n' && self.last_rx != b'\r' {
                buf.push(b'\r');
            }
            buf.push(b);
            self.last_rx = b;
        }
        self.parser.process(&buf);
    }

    /// (rows, cols)
    pub fn size(&self) -> (u16, u16) {
        let (r, c) = self.parser.screen().size();
        (r.get(), c.get())
    }

    /// Resize to the pane. When it gets shorter, vt100 keeps the cursor line on screen and
    /// pushes the rows above into the scrollback.
    /// Returns true when the size changed.
    pub fn resize(&mut self, rows: u16, cols: u16) -> bool {
        if self.size() == (rows.max(1), cols.max(1)) {
            return false;
        }
        self.parser.set_size(nz(rows), nz(cols));
        true
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let cur = self.parser.screen().scrollback() as isize;
        self.scroll_to((cur + delta).max(0) as usize);
    }

    /// Scroll back `rows` from the live view (clamped to what's in the scrollback).
    pub fn scroll_to(&mut self, rows: usize) {
        let max = self.parser.screen().scrollback_len();
        self.parser.screen_mut().set_scrollback(rows.min(max));
    }

    pub fn clear(&mut self) {
        let (r, c) = self.size();
        self.parser = vt100::Parser::new(nz(r), nz(c), SCROLLBACK);
        self.last_rx = 0;
    }

    /// Absolute row (counting from the oldest scrollback line) of the top visible row.
    /// Selections use absolute rows so they survive scrolling.
    pub fn abs_top(&self) -> usize {
        let s = self.parser.screen();
        s.scrollback_len() - s.scrollback()
    }

    /// Text from absolute (row, col) to (row, col), inclusive, across scrollback.
    pub fn text_between(&mut self, start: (usize, u16), end: (usize, u16)) -> String {
        let saved = self.parser.screen().scrollback();
        let sb = self.parser.screen().scrollback_len();
        let (rows, cols) = self.size();
        let mut out = String::new();
        for r in start.0..=end.0 {
            // Scroll so absolute row r is the top row (or as close as possible), then read it.
            let off = sb.saturating_sub(r);
            self.parser.screen_mut().set_scrollback(off);
            let vr = (r - (sb - off)) as u16;
            if vr >= rows {
                break;
            }
            let from = if r == start.0 { start.1 } else { 0 };
            let to = if r == end.0 {
                (end.1 + 1).min(cols)
            } else {
                cols
            };
            let screen = self.parser.screen();
            out.push_str(screen.contents_between(vr, from, vr, to).trim_end());
            if r != end.0 && !screen.row_wrapped(vr) {
                out.push('\n');
            }
        }
        self.parser.screen_mut().set_scrollback(saved);
        out
    }

    /// Text on the cursor's row up to the cursor, e.g. "Password: ".
    pub fn text_before_cursor(&self) -> String {
        let s = self.parser.screen();
        let (r, c) = s.cursor_position();
        s.contents_between(r, 0, r, c)
    }

    /// True when the device looks like it's asking for a password, so live keystrokes are
    /// kept out of the log.
    pub fn at_password_prompt(&self) -> bool {
        asks_for_secret(&self.text_before_cursor())
    }

    /// True at a login's password prompt (not a key passphrase or a PIN):
    /// where a remembered password goes.
    pub fn at_login_password_prompt(&self) -> bool {
        asks_for_login_password(&self.text_before_cursor())
    }
}

/// Bytes to send for a keystroke in live-keys mode (xterm/PuTTY conventions).
pub fn key_bytes(k: &KeyEvent, app_cursor: bool, enter: &[u8]) -> Option<Vec<u8>> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let csi = |s: &str| Some(format!("\x1b[{s}").into_bytes());
    let arrow = |c: char| {
        if ctrl {
            Some(format!("\x1b[1;5{c}").into_bytes())
        } else if app_cursor {
            Some(format!("\x1bO{c}").into_bytes())
        } else {
            Some(format!("\x1b[{c}").into_bytes())
        }
    };
    match k.code {
        KeyCode::Char(c) => {
            // AltGr arrives as Ctrl+Alt: treat it as the plain character it produced.
            if ctrl && !alt {
                let b = match c {
                    'a'..='z' => c as u8 - b'a' + 1,
                    'A'..='Z' => c as u8 - b'A' + 1,
                    '@' | ' ' | '2' => 0x00,
                    '[' | '3' => 0x1B,
                    '\\' | '4' => 0x1C,
                    ']' | '5' => 0x1D,
                    '^' | '6' => 0x1E,
                    '_' | '-' | '7' => 0x1F,
                    _ => return None,
                };
                return Some(vec![b]);
            }
            let mut v = if alt && !ctrl { vec![0x1B] } else { Vec::new() };
            let mut buf = [0u8; 4];
            v.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            Some(v)
        }
        KeyCode::Enter => Some(if enter.is_empty() {
            b"\r".to_vec()
        } else {
            enter.to_vec()
        }),
        KeyCode::Backspace => Some(vec![0x7F]),
        KeyCode::Tab => Some(vec![0x09]),
        KeyCode::BackTab => csi("Z"),
        KeyCode::Esc => Some(vec![0x1B]),
        KeyCode::Up => arrow('A'),
        KeyCode::Down => arrow('B'),
        KeyCode::Right => arrow('C'),
        KeyCode::Left => arrow('D'),
        KeyCode::Home => csi("1~"),
        KeyCode::Insert => csi("2~"),
        KeyCode::Delete => csi("3~"),
        KeyCode::End => csi("4~"),
        _ => None,
    }
}

// ---------------------------------------------------------------- plain text

enum EscapeState {
    Normal,
    Esc,
    Csi(String),
    Osc,
    OscEsc,
}

/// Turns a terminal byte stream into plain text lines: applies CR, backspace and
/// "erase line", drops colour/cursor escape codes.
pub struct TextCleaner {
    line: Vec<char>,
    col: usize,
    state: EscapeState,
    utf8: Vec<u8>,
}

impl Default for TextCleaner {
    fn default() -> Self {
        TextCleaner {
            line: Vec::new(),
            col: 0,
            state: EscapeState::Normal,
            utf8: Vec::new(),
        }
    }
}

impl TextCleaner {
    /// Process bytes; completed lines are appended to `out` with a trailing '\n'.
    pub fn feed(&mut self, data: &[u8], out: &mut String) {
        for &b in data {
            match std::mem::replace(&mut self.state, EscapeState::Normal) {
                EscapeState::Normal => self.normal(b, out),
                EscapeState::Esc => match b {
                    b'[' => self.state = EscapeState::Csi(String::new()),
                    b']' => self.state = EscapeState::Osc,
                    _ => {}
                },
                EscapeState::Csi(mut params) => {
                    if (0x40..=0x7E).contains(&b) {
                        self.csi(&params, b);
                    } else {
                        params.push(b as char);
                        self.state = EscapeState::Csi(params);
                    }
                }
                EscapeState::Osc => match b {
                    0x07 => {}
                    0x1B => self.state = EscapeState::OscEsc,
                    _ => self.state = EscapeState::Osc,
                },
                EscapeState::OscEsc => {
                    if b != b'\\' {
                        self.state = EscapeState::Osc;
                    }
                }
            }
        }
    }

    fn normal(&mut self, b: u8, out: &mut String) {
        if b >= 0x80 || !self.utf8.is_empty() {
            self.utf8.push(b);
            match std::str::from_utf8(&self.utf8) {
                Ok(s) => {
                    let chars: Vec<char> = s.chars().collect();
                    self.utf8.clear();
                    for c in chars {
                        self.put(c);
                    }
                }
                Err(e) if e.error_len().is_some() || self.utf8.len() >= 4 => {
                    self.utf8.clear();
                    self.put('\u{FFFD}');
                }
                Err(_) => {}
            }
            return;
        }
        match b {
            0x1B => self.state = EscapeState::Esc,
            b'\r' => self.col = 0,
            b'\n' => {
                let s: String = self.line.iter().collect();
                out.push_str(s.trim_end());
                out.push('\n');
                self.line.clear();
                self.col = 0;
            }
            0x08 => self.col = self.col.saturating_sub(1),
            b'\t' => {
                let next = (self.col / 8 + 1) * 8;
                while self.col < next {
                    self.put(' ');
                }
            }
            0x20..=0x7E => self.put(b as char),
            _ => {}
        }
    }

    fn put(&mut self, c: char) {
        while self.line.len() < self.col {
            self.line.push(' ');
        }
        if self.col < self.line.len() {
            self.line[self.col] = c;
        } else {
            self.line.push(c);
        }
        self.col += 1;
    }

    fn csi(&mut self, params: &str, fin: u8) {
        let n = params
            .trim_start_matches('?')
            .split(';')
            .next()
            .and_then(|p| p.parse::<usize>().ok());
        match fin {
            b'K' => match n.unwrap_or(0) {
                0 => self.line.truncate(self.col),
                1 => {
                    for i in 0..self.col.min(self.line.len()) {
                        self.line[i] = ' ';
                    }
                }
                _ => self.line.clear(),
            },
            b'D' => self.col = self.col.saturating_sub(n.unwrap_or(1).max(1)),
            b'C' => self.col += n.unwrap_or(1).max(1),
            b'G' => self.col = n.unwrap_or(1).max(1) - 1,
            _ => {}
        }
    }

    /// The current, not yet finished line (e.g. a prompt).
    pub fn partial(&self) -> String {
        self.line.iter().collect::<String>().trim_end().to_string()
    }
}

/// True when a prompt line asks for a password or another secret, e.g.
/// "Password:" or "admin@switch's password: ".
pub fn asks_for_secret(prompt: &str) -> bool {
    let t = prompt.trim_end().to_lowercase();
    t.ends_with(':')
        && ["password", "passcode", "passphrase", "secret", "pin"]
            .iter()
            .any(|w| t.contains(w))
}

/// A login asking for its password: "Password:", "admin@host's password:",
/// "Passcode:". Not a key's passphrase, a PIN or a secret.
pub fn asks_for_login_password(prompt: &str) -> bool {
    let t = prompt.trim_end().to_lowercase();
    t.ends_with(':') && (t.contains("password") || t.contains("passcode"))
}

/// Plain-text copy of everything the device printed, next to the hex log.
pub struct Transcript {
    file: File,
    cleaner: TextCleaner,
    pub path: PathBuf,
}

impl Transcript {
    pub fn create(log_path: &Path) -> std::io::Result<Transcript> {
        let path = log_path.with_extension("txt");
        let file = File::create(&path)?;
        Ok(Transcript {
            file,
            cleaner: TextCleaner::default(),
            path,
        })
    }

    pub fn feed(&mut self, data: &[u8]) -> std::io::Result<()> {
        let mut out = String::new();
        self.cleaner.feed(data, &mut out);
        if out.is_empty() {
            return Ok(());
        }
        self.file.write_all(out.as_bytes())
    }
}

impl Drop for Transcript {
    fn drop(&mut self) {
        let p = self.cleaner.partial();
        if !p.is_empty()
            && let Err(error) = writeln!(self.file, "{p}")
        {
            log::warn!("Could not finish {}: {error}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(data: &[u8]) -> String {
        let mut c = TextCleaner::default();
        let mut out = String::new();
        c.feed(data, &mut out);
        out + &c.partial()
    }

    #[test]
    fn cleaner_handles_crlf_backspace_and_escapes() {
        assert_eq!(clean(b"hello\r\nworld\r\n"), "hello\nworld\n");
        assert_eq!(clean(b"abc\x08\x08XY\r\n"), "aXY\n");
        assert_eq!(clean(b"\x1b[1;32mgreen\x1b[0m\r\n"), "green\n");
        // Cisco --More-- erase: backspaces over it then overwrite with spaces.
        assert_eq!(
            clean(b" --More-- \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08          \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08next line\r\n"),
            "next line\n"
        );
        assert_eq!(clean(b"progress 10%\rprogress 99%\r\n"), "progress 99%\n");
        assert_eq!(clean(b"abcdef\x1b[3D\x1b[K\r\n"), "abc\n");
        assert_eq!(clean("caf\u{e9}\n".as_bytes()), "caf\u{e9}\n");
        assert_eq!(clean(b"Switch#"), "Switch#");
    }

    #[test]
    fn term_implicit_cr_and_password_prompt() {
        let mut t = Term::new();
        t.feed(b"one\ntwo\n");
        assert!(t.screen().contents().starts_with("one\ntwo"));
        t.feed(b"User Access Verification\r\n\r\nPassword: ");
        assert!(t.at_password_prompt());
        t.feed(b"\r\nSwitch>");
        assert!(!t.at_password_prompt());
    }

    #[test]
    fn secret_prompts_are_recognised() {
        assert!(asks_for_secret("Password:"));
        assert!(asks_for_secret("admin@switch's password: "));
        assert!(asks_for_secret(
            "Enter passphrase for key '/home/j/.ssh/id_ed25519':"
        ));
        assert!(!asks_for_secret("Username:"));
        assert!(!asks_for_secret("password reset done"));
    }

    /// A long prompt wraps in a narrow pane: the cursor's screen row no longer
    /// shows "password", but the device's current line still does, which is
    /// why the monitor checks both.
    #[test]
    fn a_wrapped_prompt_is_still_a_password_prompt_on_the_device_line() {
        let prompt = b"admin@a-long-hostname.example.com's password: ";
        let mut t = Term::new();
        t.resize(24, 10); // the cursor row is just "word: "
        t.feed(prompt);
        assert!(!t.at_password_prompt());
        let mut cleaner = TextCleaner::default();
        let mut out = String::new();
        cleaner.feed(prompt, &mut out);
        assert!(asks_for_secret(&cleaner.partial()));
    }

    #[test]
    fn shrinking_keeps_the_newest_lines() {
        let mut t = Term::new();
        t.resize(26, 80);
        for i in 1..=30 {
            t.feed(format!("line {i}\r\n").as_bytes());
        }
        t.feed(b"Switch#");
        t.resize(10, 80);
        let c = t.screen().contents();
        assert!(c.trim_end().ends_with("line 30\nSwitch#"), "{c}");
        t.feed(b"enable");
        assert!(t.screen().contents().trim_end().ends_with("Switch#enable"));
    }

    #[test]
    fn keys_encode() {
        let k = |code, m| KeyEvent::new(code, m);
        assert_eq!(
            key_bytes(&k(KeyCode::Char('c'), KeyModifiers::CONTROL), false, b"\r"),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes(&k(KeyCode::Char('z'), KeyModifiers::CONTROL), false, b"\r"),
            Some(vec![26])
        );
        assert_eq!(
            key_bytes(&k(KeyCode::Up, KeyModifiers::NONE), false, b"\r"),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(&k(KeyCode::Up, KeyModifiers::NONE), true, b"\r"),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_bytes(&k(KeyCode::Enter, KeyModifiers::NONE), false, b""),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_bytes(&k(KeyCode::Char('?'), KeyModifiers::NONE), false, b"\r"),
            Some(b"?".to_vec())
        );
        // AltGr+q on a German layout
        assert_eq!(
            key_bytes(
                &k(
                    KeyCode::Char('@'),
                    KeyModifiers::CONTROL | KeyModifiers::ALT
                ),
                false,
                b"\r"
            ),
            Some(b"@".to_vec())
        );
    }
}
