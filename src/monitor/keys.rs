//! The monitor's commands and keys: the F1 menu, shortcut keys, per-connection
//! settings and line-mode editing.

use super::*;

impl Monitor {
    /// Run a command from a shortcut key or the menu.
    pub fn run_cmd(&mut self, c: Cmd) -> Action {
        match c {
            Cmd::HiddenSend => self.secret = Some(Vec::new()),
            Cmd::Break => match self.link.as_ref().map(|l| l.send_break(BREAK_LEN)) {
                Some(Ok(())) => self.push(EntryKind::Info(format!(
                    "Sent break ({} ms)",
                    BREAK_LEN.as_millis()
                ))),
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
                self.set_status(
                    if self.follow {
                        "Raw log following"
                    } else {
                        "Raw log paused"
                    },
                    false,
                );
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
                    let name = if self.focus == Pane::Log {
                        "raw log"
                    } else {
                        "terminal"
                    };
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
                    Ok(()) => self
                        .set_status(format!("Copied all terminal output ({lines} lines)"), false),
                    Err(e) => self.set_status(e, true),
                }
            }
            Cmd::OpenTranscript => match &self.transcript {
                Some(t) => {
                    let path = t.path.clone();
                    match crate::opener::open(&path) {
                        Ok(()) => self.set_status("Opened the transcript", false),
                        Err(error) => self.report_error(error),
                    }
                }
                None => self.set_status("No transcript: logging is off for this session", true),
            },
            Cmd::CopySessionLog => match crate::session_log::path() {
                Some(path) => match std::fs::read_to_string(path) {
                    Ok(text) => match crate::clip::set(&text) {
                        Ok(()) => self.set_status("Copied the session log", false),
                        Err(error) => self.report_error(error),
                    },
                    Err(error) => {
                        self.report_error(format!("Could not read {}: {error}", path.display()))
                    }
                },
                None => self.report_error("The session log isn't running".into()),
            },
            Cmd::OpenLogFolder => match crate::session_log::path().and_then(|p| p.parent()) {
                Some(dir) => match crate::opener::open(dir) {
                    Ok(()) => self.set_status("Opened the log folder", false),
                    Err(error) => self.report_error(error),
                },
                None => self.report_error("The session log isn't running".into()),
            },
            Cmd::CopySystemInfo => {
                let summary = crate::system_info::SystemInfo::collect().summary();
                match crate::clip::set(&summary) {
                    Ok(()) => self.set_status("Copied the system info", false),
                    Err(error) => self.report_error(error),
                }
            }
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
                self.profile.line_ending = menu::step(
                    &[L::None, L::Cr, L::Lf, L::Crlf],
                    self.profile.line_ending,
                    dir,
                );
                self.set_status(
                    format!("Line ending: {}", self.profile.line_ending.label()),
                    false,
                );
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
                self.profile.input_mode =
                    menu::step(&menu::input_modes(), self.profile.input_mode, dir);
                self.hist_pos = None;
            }
            Setting::Layout => {
                let l = menu::step(
                    &[Layout::Split, Layout::Log, Layout::Terminal],
                    self.profile.layout,
                    dir,
                );
                self.set_layout(l);
            }
            Setting::LogFormat => {
                use crate::config::LogFormat as F;
                let f = menu::step(
                    &[F::Compact, F::Both, F::Hex, F::Text],
                    self.log_format(),
                    dir,
                );
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
            Setting::LogLevel => {
                let level = menu::step(&crate::session_log::LEVELS, log::max_level(), dir);
                crate::session_log::set_level(level);
                self.set_status(
                    format!("Log level: {}", crate::session_log::level_name(level)),
                    false,
                );
            }
        }
    }

    pub(super) fn toggle_logging(&mut self) {
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
                self.transcript = match Transcript::create(&l.path) {
                    Ok(t) => Some(t),
                    Err(error) => {
                        self.report_error(format!("Could not create the transcript: {error}"));
                        None
                    }
                };
                let p = l.path.display().to_string();
                self.logger = Some(l);
                self.push(EntryKind::Info(format!("Logging to {p}")));
                self.set_status("Logging on", false);
            }
            Err(e) => self.set_status(format!("Couldn't start logging: {e}"), true),
        }
    }

    pub(super) fn menu_key(&mut self, k: KeyEvent) -> Action {
        let Some(mut menu) = self.menu.take() else {
            return Action::None;
        };
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
                    if let Some(b) =
                        term::key_bytes(&k, app_cursor, self.profile.line_ending.bytes())
                    {
                        self.live_send(b);
                    }
                    return Action::None;
                }
                return self.line_key(k);
            }
        }
        Action::None
    }

    pub(super) fn line_key(&mut self, k: KeyEvent) -> Action {
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
            KeyCode::Right if ctrl => {
                self.profile.term_cols = self.cur_term_cols.saturating_sub(4).max(20)
            }
            KeyCode::Up if ctrl => self.profile.log_rows = self.cur_log_rows.saturating_add(1),
            KeyCode::Down if ctrl => {
                self.profile.log_rows = self.cur_log_rows.saturating_sub(1).max(3)
            }
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
