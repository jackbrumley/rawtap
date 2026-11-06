//! The monitor's panes: scrolling, layout, mouse selection, copy and paste.

use super::*;

impl Monitor {
    pub(super) fn scroll_pane(&mut self, pages: isize) {
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

    pub(super) fn scroll_end(&mut self, top: bool) {
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

    pub(super) fn set_layout(&mut self, l: Layout) {
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

    pub(super) fn copy_selection(&mut self) {
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
                (
                    picked.concat(),
                    format!("{n} log entr{}", if n == 1 { "y" } else { "ies" }),
                )
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

    pub(super) fn paste_clipboard(&mut self) {
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
        let lines: Vec<String> = text
            .trim_end_matches('\n')
            .split('\n')
            .map(str::to_string)
            .collect();
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

    pub(super) fn send_confirmed_paste(&mut self) {
        let Some(lines) = self.paste_confirm.take() else {
            return;
        };
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

    pub(super) fn pane_at(&self, x: u16, y: u16) -> Option<Pane> {
        let hit = |r: Rect| {
            r.width > 0 && x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
        };
        if hit(self.term_inner) {
            Some(Pane::Terminal)
        } else if hit(self.log_inner) {
            Some(Pane::Log)
        } else {
            None
        }
    }

    pub(super) fn pane_rect(&self, p: Pane) -> Rect {
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
                let id = self
                    .log_row_ids
                    .get(row as usize)
                    .or(self.log_row_ids.last())
                    .copied()
                    .unwrap_or(0);
                (id as usize, 0)
            }
        }
    }

    pub(super) fn scroll_lines(&mut self, pane: Pane, up: bool, n: usize) {
        match pane {
            Pane::Terminal => self
                .term
                .scroll_by(if up { n as isize } else { -(n as isize) }),
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
    pub(super) fn autoscroll(&mut self) {
        let (Some(s), Some((_, y))) = (self.selection, self.drag_pos) else {
            return;
        };
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
        let extend = ev
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::SHIFT);
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
                        self.selection = Some(Selection {
                            head: pos,
                            dragging: true,
                            moved: true,
                            ..s
                        });
                    }
                    _ => {
                        self.selection = Some(Selection {
                            pane: p,
                            anchor: pos,
                            head: pos,
                            dragging: true,
                            moved: false,
                        });
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
}
