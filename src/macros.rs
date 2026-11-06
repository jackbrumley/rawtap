//! Macro keys and the step runner (send / wait for text / wait for quiet / delay, with capture).

use crate::codec::{self, InputMode, LineEnding};
use crate::config::Macro;
use crate::term::TextCleaner;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyCombo {
    code: KeyCode,
    mods: KeyModifiers,
}

impl KeyCombo {
    pub fn parse(s: &str) -> Result<KeyCombo, String> {
        let mut mods = KeyModifiers::NONE;
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (last, rest) = parts.split_last().ok_or("empty key")?;
        for m in rest {
            match m.to_ascii_lowercase().as_str() {
                "alt" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
                other => return Err(format!("unknown modifier '{other}'")),
            }
        }
        let up = last.to_ascii_uppercase();
        let code = if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
            if !(1..=12).contains(&n) {
                return Err(format!("no such key F{n}"));
            }
            if mods.is_empty() && n <= 10 {
                return Err(format!("F{n} is a built-in key; use F11, F12, Shift+F{n} or Alt+0..9"));
            }
            KeyCode::F(n)
        } else if last.chars().count() == 1 && !mods.is_empty() {
            let c = last.chars().next().unwrap();
            if mods.contains(KeyModifiers::CONTROL) && !mods.contains(KeyModifiers::ALT) {
                return Err("Ctrl+<key> macros would clash with editing and live keys; use Alt".into());
            }
            KeyCode::Char(c.to_ascii_lowercase())
        } else {
            return Err(format!("can't use '{s}' as a macro key"));
        };
        Ok(KeyCombo { code, mods })
    }

    pub fn matches(&self, k: &KeyEvent) -> bool {
        let code = match k.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            c => c,
        };
        // Shift on a letter is reported in the char itself; ignore it for char keys.
        let mut mods = k.modifiers & (KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::CONTROL);
        if matches!(code, KeyCode::Char(_)) {
            mods.remove(KeyModifiers::SHIFT);
        }
        code == self.code && mods == self.mods
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            s.push_str("Ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            s.push_str("Alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            s.push_str("Shift+");
        }
        match self.code {
            KeyCode::F(n) => s.push_str(&format!("F{n}")),
            KeyCode::Char(c) => s.push(c.to_ascii_uppercase()),
            _ => {}
        }
        s
    }
}

pub struct LoadedMacro {
    pub key: KeyCombo,
    pub label: String,
    pub def: Macro,
}

/// Validate macro definitions. Returns usable macros and warnings for the rest.
pub fn load(defs: &[Macro]) -> (Vec<LoadedMacro>, Vec<String>) {
    let mut out: Vec<LoadedMacro> = Vec::new();
    let mut warnings = Vec::new();
    for d in defs {
        let key = match KeyCombo::parse(&d.key) {
            Ok(k) => k,
            Err(e) => {
                warnings.push(format!("Macro '{}' skipped: {e}", d.key));
                continue;
            }
        };
        if out.iter().any(|m| m.key == key) {
            warnings.push(format!("Macro '{}' skipped: key used twice", d.key));
            continue;
        }
        if d.data.is_none() && d.steps.is_empty() {
            warnings.push(format!("Macro '{}' skipped: needs \"data\" or \"steps\"", d.key));
            continue;
        }
        let label = if !d.label.is_empty() {
            d.label.clone()
        } else {
            d.data.clone().unwrap_or_else(|| format!("{} steps", d.steps.len()))
        };
        out.push(LoadedMacro { key, label, def: d.clone() });
    }
    (out, warnings)
}

enum Action {
    Send(Vec<u8>),
    Delay(Duration),
    Wait { text: String, timeout: Duration },
    WaitIdle { idle: Duration, timeout: Duration },
}

pub enum Poll {
    Pending,
    Send(Vec<u8>),
    Done { capture: Option<String> },
    Failed(String),
}

pub struct MacroRun {
    pub label: String,
    actions: Vec<Action>,
    idx: usize,
    step_start: Instant,
    last_rx: Instant,
    /// Output since the last send, for `wait`.
    seen: TextCleaner,
    seen_text: String,
    capture: Option<(TextCleaner, String)>,
}

impl MacroRun {
    /// Build a run. `current_ending` is the F2 setting, used when the macro doesn't say.
    pub fn new(m: &LoadedMacro, current_ending: LineEnding) -> Result<MacroRun, String> {
        let d = &m.def;
        let mut actions = Vec::new();
        let ascii = |text: &str, le: Option<LineEnding>| -> Result<Vec<u8>, String> {
            codec::encode(text, InputMode::Ascii, le.or(d.line_ending).unwrap_or(current_ending))
        };
        if let Some(data) = &d.data {
            let bytes = match d.mode {
                InputMode::Hex => codec::parse_hex(data)?,
                InputMode::Ascii => ascii(data, None)?,
            };
            actions.push(Action::Send(bytes));
        }
        for s in &d.steps {
            let timeout = Duration::from_millis(s.timeout_ms.unwrap_or(10_000));
            if let Some(ms) = s.delay_ms {
                actions.push(Action::Delay(Duration::from_millis(ms)));
            }
            if let Some(t) = &s.send {
                actions.push(Action::Send(ascii(t, s.line_ending)?));
            }
            if let Some(h) = &s.send_hex {
                actions.push(Action::Send(codec::parse_hex(h)?));
            }
            if let Some(w) = &s.wait {
                actions.push(Action::Wait { text: w.clone(), timeout });
            }
            if let Some(ms) = s.wait_idle_ms {
                actions.push(Action::WaitIdle { idle: Duration::from_millis(ms), timeout });
            }
        }
        let now = Instant::now();
        Ok(MacroRun {
            label: m.label.clone(),
            actions,
            idx: 0,
            step_start: now,
            last_rx: now,
            seen: TextCleaner::default(),
            seen_text: String::new(),
            capture: d.capture.then(|| (TextCleaner::default(), String::new())),
        })
    }

    pub fn progress(&self) -> (usize, usize) {
        ((self.idx + 1).min(self.actions.len()), self.actions.len())
    }

    pub fn on_rx(&mut self, data: &[u8]) {
        self.last_rx = Instant::now();
        self.seen.feed(data, &mut self.seen_text);
        if let Some((c, out)) = self.capture.as_mut() {
            c.feed(data, out);
        }
    }

    fn advance(&mut self) {
        self.idx += 1;
        self.step_start = Instant::now();
    }

    pub fn poll(&mut self) -> Poll {
        let Some(a) = self.actions.get(self.idx) else {
            let capture = self.capture.take().map(|(c, mut out)| {
                out.push_str(&c.partial());
                out
            });
            return Poll::Done { capture };
        };
        match a {
            Action::Send(b) => {
                let b = b.clone();
                self.seen = TextCleaner::default();
                self.seen_text.clear();
                self.advance();
                self.last_rx = Instant::now();
                Poll::Send(b)
            }
            Action::Delay(d) => {
                if self.step_start.elapsed() >= *d {
                    self.advance();
                    return self.poll();
                }
                Poll::Pending
            }
            Action::Wait { text, timeout } => {
                if self.seen_text.contains(text.as_str()) || self.seen.partial().contains(text.as_str()) {
                    self.advance();
                    return self.poll();
                }
                if self.step_start.elapsed() > *timeout {
                    return Poll::Failed(format!("timed out waiting for '{text}'"));
                }
                Poll::Pending
            }
            Action::WaitIdle { idle, timeout } => {
                if self.last_rx.elapsed() >= *idle {
                    self.advance();
                    return self.poll();
                }
                if self.step_start.elapsed() > *timeout {
                    return Poll::Failed("timed out waiting for the device to go quiet".into());
                }
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Step;

    #[test]
    fn key_parsing() {
        assert!(KeyCombo::parse("F5").is_err());
        assert!(KeyCombo::parse("F11").is_ok());
        let k = KeyCombo::parse("Alt+1").unwrap();
        assert!(k.matches(&KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT)));
        assert!(!k.matches(&KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)));
        let k = KeyCombo::parse("shift+f5").unwrap();
        assert!(k.matches(&KeyEvent::new(KeyCode::F(5), KeyModifiers::SHIFT)));
        assert_eq!(k.label(), "Shift+F5");
        assert!(KeyCombo::parse("Ctrl+x").is_err());
    }

    #[test]
    fn run_waits_for_prompt_then_captures() {
        let def = Macro {
            key: "Alt+9".into(),
            capture: true,
            steps: vec![
                Step { send: Some("terminal length 0".into()), wait: Some("#".into()), ..Default::default() },
                Step { send: Some("show run".into()), wait: Some("Switch#".into()), ..Default::default() },
            ],
            ..Default::default()
        };
        let (loaded, warn) = load(&[def]);
        assert!(warn.is_empty());
        let mut run = MacroRun::new(&loaded[0], LineEnding::Cr).unwrap();
        // The old prompt is on screen before we start; it must not satisfy the first wait.
        run.on_rx(b"Switch#");
        let Poll::Send(b) = run.poll() else { panic!() };
        assert_eq!(b, b"terminal length 0\r");
        assert!(matches!(run.poll(), Poll::Pending));
        run.on_rx(b"terminal length 0\r\nSwitch#");
        let Poll::Send(b) = run.poll() else { panic!() };
        assert_eq!(b, b"show run\r");
        run.on_rx(b"show run\r\nhostname Switch\r\n");
        assert!(matches!(run.poll(), Poll::Pending));
        run.on_rx(b"end\r\nSwitch#");
        let Poll::Done { capture: Some(c) } = run.poll() else { panic!() };
        assert!(c.contains("hostname Switch\nend\nSwitch#"));
    }
}
