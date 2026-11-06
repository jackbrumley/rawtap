//! Rendering for the monitor screen: header, raw log pane, terminal pane, send box, footer.

use crate::codec::{self, InputMode};
use crate::config::{Layout as PaneLayout, LogFormat};
use crate::link::LinkSettings;
use crate::menu::{self, Kind};
use crate::monitor::{Entry, EntryKind, Monitor, Pane};
use crate::ui::{DIM, centered};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// Below this width the split view stacks the panes (terminal on top, log strip below).
const SIDE_BY_SIDE_MIN: u16 = 130;

pub fn draw(f: &mut Frame, m: &mut Monitor) {
    let [header, body, input, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_header(f, m, header);
    m.term_inner = Rect::default();
    m.log_inner = Rect::default();
    let (log_area, term_area) = split_body(m, body);
    if let Some(a) = term_area {
        draw_term(f, m, a);
    }
    if let Some(a) = log_area {
        draw_log(f, m, a);
    }
    draw_input(f, m, input);
    draw_footer(f, m, footer);
    if m.menu.is_some() || m.show_help || m.paste_confirm.is_some() {
        dim(f);
    }
    if m.menu.is_some() {
        draw_menu(f, m);
    }
    if m.paste_confirm.is_some() {
        draw_paste_confirm(f, m);
    }
    if m.show_help {
        draw_help(f);
    }
}

/// Grey out everything already drawn, so a pop-up on top is the only thing in focus.
fn dim(f: &mut Frame) {
    let area = f.area();
    f.buffer_mut().set_style(
        area,
        Style::new().fg(Color::DarkGray).bg(Color::Reset).remove_modifier(Modifier::all()),
    );
}

/// Paint the terminal selection: `abs_top` is the absolute row shown at the top of the pane.
fn highlight_term(f: &mut Frame, m: &Monitor, inner: Rect, abs_top: usize) {
    let Some(s) = m.selection.filter(|s| s.pane == Pane::Terminal && !s.is_empty()) else { return };
    let ((r1, c1), (r2, c2)) = s.ordered();
    let style = Style::new().fg(Color::Black).bg(Color::LightBlue);
    let buf = f.buffer_mut();
    for vr in 0..inner.height {
        let r = abs_top + vr as usize;
        if r < r1 || r > r2 {
            continue;
        }
        let from = if r == r1 { c1 } else { 0 };
        let to = if r == r2 { c2 } else { inner.width.saturating_sub(1) };
        for c in from..=to.min(inner.width.saturating_sub(1)) {
            buf[(inner.x + c, inner.y + vr)].set_style(style);
        }
    }
}

/// Paint whole raw-log rows whose entry is inside the selection.
fn highlight_log(f: &mut Frame, m: &Monitor, inner: Rect) {
    let Some(s) = m.selection.filter(|s| s.pane == Pane::Log && !s.is_empty()) else { return };
    let ((lo, _), (hi, _)) = s.ordered();
    let style = Style::new().fg(Color::Black).bg(Color::LightBlue);
    let buf = f.buffer_mut();
    for (vr, id) in m.log_row_ids.iter().enumerate() {
        if (*id as usize) < lo || (*id as usize) > hi || vr as u16 >= inner.height {
            continue;
        }
        for c in 0..inner.width {
            buf[(inner.x + c, inner.y + vr as u16)].set_style(style);
        }
    }
}

fn draw_paste_confirm(f: &mut Frame, m: &Monitor) {
    let Some(lines) = &m.paste_confirm else { return };
    let full = f.area();
    let shown = lines.len().min(10);
    let h = (shown as u16 + 9).min(full.height);
    let w = 76.min(full.width);
    let area = Rect { x: full.x + (full.width - w) / 2, y: full.y + (full.height - h) / 3, width: w, height: h };
    f.render_widget(Clear, area);
    let le = m.profile.line_ending.label();
    let mut text = vec![
        Line::from(Span::styled(
            format!(" Paste {} lines to {}?", lines.len(), m.settings.describe()),
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            match m.profile.char_delay_ms {
                0 => format!(" Sent line by line with {le}. Garbled? Set a char delay (F1)."),
                ms => format!(" Each line is sent with {le}, {ms} ms between characters."),
            },
            DIM,
        )),
        Line::from(""),
    ];
    let iw = w.saturating_sub(6) as usize;
    for l in lines.iter().take(shown) {
        let t: String = l.chars().take(iw).collect();
        text.push(Line::from(Span::styled(format!("  │ {t}"), Style::new().fg(Color::White))));
    }
    if lines.len() > shown {
        text.push(Line::from(Span::styled(format!("  │ … and {} more", lines.len() - shown), DIM)));
    }
    let block = Block::bordered()
        .title(Line::from(Span::styled(" Confirm paste ", Style::new().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))))
        .title_bottom(Line::from(vec![
            Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)),
            Span::raw(" send   "),
            Span::styled(" Esc ", Style::new().fg(Color::Black).bg(Color::Gray)),
            Span::raw(" cancel "),
        ]))
        .border_style(Style::new().fg(Color::Yellow));
    f.render_widget(Paragraph::new(text).block(block), area);
}

fn split_body(m: &mut Monitor, body: Rect) -> (Option<Rect>, Option<Rect>) {
    match m.profile.layout {
        PaneLayout::Log => (Some(body), None),
        PaneLayout::Terminal => (None, Some(body)),
        PaneLayout::Split if body.width >= SIDE_BY_SIDE_MIN => {
            m.side_by_side = true;
            let auto = (body.width * 60 / 100).max(82);
            let want = if m.profile.term_cols == 0 { auto } else { m.profile.term_cols };
            let tc = want.clamp(40, body.width.saturating_sub(30));
            m.cur_term_cols = tc;
            let [l, t] = Layout::horizontal([Constraint::Min(0), Constraint::Length(tc)]).areas(body);
            (Some(l), Some(t))
        }
        PaneLayout::Split => {
            m.side_by_side = false;
            let auto = (body.height / 4).clamp(5, 10);
            let want = if m.profile.log_rows == 0 { auto } else { m.profile.log_rows };
            let lr = want.clamp(3, body.height.saturating_sub(6).max(3));
            m.cur_log_rows = lr;
            let [t, l] = Layout::vertical([Constraint::Min(0), Constraint::Length(lr)]).areas(body);
            (Some(l), Some(t))
        }
    }
}

fn pane_block(m: &Monitor, pane: Pane) -> Block<'static> {
    let focused = m.profile.layout != PaneLayout::Split || m.focus == pane;
    let color = if !m.connected {
        Color::DarkGray
    } else if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    Block::bordered().border_style(Style::new().fg(color))
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn draw_header(f: &mut Frame, m: &Monitor, area: Rect) {
    let (state, state_style) = match (&m.settings, m.connected, &m.peer) {
        (_, false, _) => ("○ DISCONNECTED".to_string(), Style::new().fg(Color::White).bg(Color::Red)),
        (LinkSettings::TcpListen { .. }, true, None) => {
            ("◌ WAITING FOR CLIENT".to_string(), Style::new().fg(Color::Black).bg(Color::Yellow))
        }
        (LinkSettings::TcpListen { .. }, true, Some(p)) => {
            (format!("● CLIENT {p}"), Style::new().fg(Color::Black).bg(Color::Green))
        }
        _ => ("● CONNECTED".to_string(), Style::new().fg(Color::Black).bg(Color::Green)),
    };
    let mut spans = vec![
        Span::styled(" RawTap ", Style::new().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(format!(" {} ", m.settings.describe())),
        Span::styled(format!(" {state} "), state_style),
        Span::raw(format!("  RX {} B  TX {} B", thousands(m.rx_bytes), thousands(m.tx_bytes))),
    ];
    if m.live {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(" LIVE KEYS ", Style::new().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)));
    }
    if let Some(l) = &m.logger {
        if let Some(name) = l.path.file_name() {
            spans.push(Span::styled(format!("  log {}", name.to_string_lossy()), DIM));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ---------------------------------------------------------------- raw log

fn wrap_tokens(prefix: String, tokens: Vec<String>, sep: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    let indent = prefix.chars().count();
    let avail = width.saturating_sub(indent).max(8);
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    for t in tokens {
        let add = if cur.is_empty() { t.len() } else { sep.len() + t.len() };
        if !cur.is_empty() && cur.len() + add > avail {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str(sep);
        }
        cur.push_str(&t);
    }
    if !cur.is_empty() || rows.is_empty() {
        rows.push(cur);
    }
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let p = if i == 0 { prefix.clone() } else { " ".repeat(indent) };
            Line::from(vec![Span::styled(p, DIM), Span::styled(r, style)])
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Screen rows for one entry at the given width.
pub fn entry_lines(e: &Entry, width: u16, fmt: LogFormat) -> Vec<Line<'static>> {
    let w = width as usize;
    let ts = e.time.format("%H:%M:%S%.3f").to_string();
    match &e.kind {
        EntryKind::Rx(d) | EntryKind::Tx(d) => {
            let rx = matches!(e.kind, EntryKind::Rx(_));
            let dir = if rx { "RX" } else { "TX" };
            let (hex_style, text_style) = if rx {
                (Style::new().fg(Color::Cyan), Style::new().fg(Color::White))
            } else {
                (Style::new().fg(Color::Yellow), Style::new().fg(Color::LightYellow))
            };
            if fmt == LogFormat::Compact {
                return vec![compact_line(&ts, dir, d, w, hex_style, text_style)];
            }
            let mut out = Vec::new();
            if fmt != LogFormat::Text {
                let toks = d.iter().map(|b| format!("{b:02X}")).collect();
                out.extend(wrap_tokens(format!("{ts} {dir} HEX  "), toks, " ", hex_style, w));
            }
            if fmt != LogFormat::Hex {
                let toks = d.iter().map(|&b| codec::byte_text(b)).collect();
                out.extend(wrap_tokens(format!("{ts} {dir} TEXT "), toks, "", text_style, w));
            }
            out
        }
        EntryKind::TxHidden(n) => vec![Line::from(vec![
            Span::styled(format!("{ts} TX "), DIM),
            Span::styled(format!("<hidden: {n} bytes>"), Style::new().fg(Color::Magenta)),
        ])],
        EntryKind::Info(m) | EntryKind::Good(m) | EntryKind::Error(m) => {
            let (mark, style) = match e.kind {
                EntryKind::Good(_) => ("✔ ", Style::new().fg(Color::Green)),
                EntryKind::Error(_) => ("✘ ", Style::new().fg(Color::LightRed)),
                _ => ("· ", Style::new().fg(Color::Gray)),
            };
            if fmt == LogFormat::Compact {
                let prefix = format!("{ts} {mark}");
                let first = m.lines().collect::<Vec<_>>().join(" ");
                let body = truncate(&first, w.saturating_sub(prefix.chars().count()).max(1));
                return vec![Line::from(vec![Span::styled(prefix, DIM), Span::styled(body, style)])];
            }
            let mut out = Vec::new();
            for line in m.lines() {
                let toks = line.split(' ').map(str::to_string).collect();
                let prefix = if out.is_empty() { format!("{ts} {mark}") } else { " ".repeat(ts.len() + 3) };
                out.extend(wrap_tokens(prefix, toks, " ", style, w));
            }
            out
        }
    }
}

/// One row: "12:25:31.749 RX   14 73 68 6F 77 20 72 … │ show r…"
fn compact_line(ts: &str, dir: &str, d: &[u8], w: usize, hex_style: Style, text_style: Style) -> Line<'static> {
    let prefix = format!("{ts} {dir} {:>4} ", d.len());
    let avail = w.saturating_sub(prefix.len());
    let hex_w = avail * 11 / 20;
    let mut hex = String::new();
    let mut shown = 0;
    for b in d {
        if hex.len() + 3 > hex_w.saturating_sub(1) {
            break;
        }
        if !hex.is_empty() {
            hex.push(' ');
        }
        hex.push_str(&format!("{b:02X}"));
        shown += 1;
    }
    if shown < d.len() {
        hex.push('…');
    }
    let hex = format!("{hex:<hex_w$}");
    let text_w = avail.saturating_sub(hex_w + 3);
    let mut text = String::new();
    for &b in d {
        let t = codec::byte_text(b);
        if text.chars().count() + t.len() > text_w.saturating_sub(1) {
            text.push('…');
            break;
        }
        text.push_str(&t);
    }
    Line::from(vec![
        Span::styled(prefix, DIM),
        Span::styled(hex, hex_style),
        Span::styled(" │ ", DIM),
        Span::styled(text, text_style),
    ])
}

fn draw_log(f: &mut Frame, m: &mut Monitor, area: Rect) {
    let fmt = m.log_format();
    let mut block = pane_block(m, Pane::Log).title_top(Line::from(vec![
        Span::raw(" Raw log "),
        Span::styled(format!("· {} (F8) ", fmt.label()), DIM),
    ]));
    if !m.follow {
        block = block.title_top(
            Line::from(Span::styled(" ⏸ paused · F9 or Ctrl+End to follow ", Style::new().fg(Color::Yellow)))
                .right_aligned(),
        );
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (w, h) = (inner.width, inner.height as usize);
    m.log_width = w;
    m.log_height = inner.height;
    m.log_inner = inner;

    // Clamp scroll to what exists.
    let mut cum = 0usize;
    for e in m.entries.iter_mut().rev() {
        cum += e.line_count(w, fmt);
        if cum >= m.scroll + h {
            break;
        }
    }
    if cum < m.scroll + h {
        m.scroll = cum.saturating_sub(h);
    }

    // Build only the visible rows, walking up from the bottom.
    let (top, bottom) = (m.scroll + h, m.scroll);
    let mut rows: Vec<Line> = Vec::with_capacity(h);
    let mut ids: Vec<u64> = Vec::with_capacity(h);
    let mut cum = 0usize;
    for e in m.entries.iter_mut().rev() {
        if cum >= top {
            break;
        }
        let c = e.line_count(w, fmt);
        if cum + c > bottom {
            let lines = entry_lines(e, w, fmt);
            for (j, line) in lines.into_iter().enumerate().rev() {
                let off = cum + (c - 1 - j);
                if off >= bottom && off < top {
                    rows.push(line);
                    ids.push(e.id);
                }
            }
        }
        cum += c;
    }
    rows.reverse();
    ids.reverse();
    m.log_row_ids = ids;
    // While dragging (including auto-scroll and the wheel), follow the mouse with the selection.
    if let (Some(s), Some((x, y))) = (m.selection, m.drag_pos) {
        if s.dragging && s.pane == Pane::Log {
            let head = m.pos_at(Pane::Log, x, y);
            let moved = s.moved || head != s.anchor;
            m.selection = Some(crate::monitor::Selection { head, moved, ..s });
        }
    }
    f.render_widget(Paragraph::new(rows), inner);
    highlight_log(f, m, inner);
}

// ---------------------------------------------------------------- terminal

fn vt_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn draw_term(f: &mut Frame, m: &mut Monitor, area: Rect) {
    let inner = Block::bordered().inner(area);
    m.term.resize(inner.height, inner.width);
    m.term_rows = inner.height;
    m.term_inner = inner;
    let abs_top = m.term.abs_top();
    if let (Some(s), Some((x, y))) = (m.selection, m.drag_pos) {
        if s.dragging && s.pane == Pane::Terminal {
            let head = m.pos_at(Pane::Terminal, x, y);
            let moved = s.moved || head != s.anchor;
            m.selection = Some(crate::monitor::Selection { head, moved, ..s });
        }
    }
    let screen = m.term.screen();
    let back = screen.scrollback();

    let mut block = pane_block(m, Pane::Terminal).title_top(Line::from(vec![
        Span::raw(" Terminal "),
        Span::styled(format!("{}×{} ", inner.width, inner.height), DIM),
    ]));
    if back > 0 {
        block = block.title_top(
            Line::from(Span::styled(
                format!(" ↑ {back} lines back · Ctrl+End for live "),
                Style::new().fg(Color::Yellow),
            ))
            .right_aligned(),
        );
    } else if m.live {
        block = block.title_top(
            Line::from(Span::styled(" LIVE KEYS · F4 to leave ", Style::new().fg(Color::Black).bg(Color::Magenta)))
                .right_aligned(),
        );
    }
    f.render_widget(block, area);

    if m.rx_bytes == 0 && back == 0 {
        let hint = vec![
            Line::from(Span::styled("Device output appears here, as it would in PuTTY.", DIM)),
            Line::from(""),
            Line::from(Span::styled("Type a command below and press Enter, or press F4 for live keys", DIM)),
            Line::from(Span::styled("(Tab completion, ?, space at --More--, Ctrl+C, arrow keys).", DIM)),
            Line::from(Span::styled("F6 sends a password without logging it.", DIM)),
        ];
        f.render_widget(Paragraph::new(hint), inner);
        return;
    }

    let mut lines = Vec::with_capacity(inner.height as usize);
    for row in 0..inner.height {
        let mut spans: Vec<Span> = Vec::new();
        let mut cur_style: Option<Style> = None;
        let mut cur = String::new();
        for col in 0..inner.width {
            let Some(cell) = screen.cell(row, col) else { continue };
            if cell.is_wide_continuation() {
                continue;
            }
            let mut st = Style::new().fg(vt_color(cell.fgcolor())).bg(vt_color(cell.bgcolor()));
            if cell.bold() {
                st = st.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                st = st.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                st = st.add_modifier(Modifier::UNDERLINED);
            }
            if cell.inverse() {
                st = st.add_modifier(Modifier::REVERSED);
            }
            if cur_style != Some(st) {
                if let Some(s) = cur_style {
                    spans.push(Span::styled(std::mem::take(&mut cur), s));
                }
                cur_style = Some(st);
            }
            if cell.has_contents() {
                cur.push_str(&cell.contents());
            } else {
                cur.push(' ');
            }
        }
        if let Some(s) = cur_style {
            spans.push(Span::styled(cur, s));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
    highlight_term(f, m, inner, abs_top);

    let screen = m.term.screen();
    if m.live && m.secret.is_none() && !m.show_help && m.menu.is_none() && back == 0 && !screen.hide_cursor() {
        let (r, c) = screen.cursor_position();
        let x = inner.x + c.min(inner.width.saturating_sub(1));
        let y = inner.y + r.min(inner.height.saturating_sub(1));
        f.set_cursor_position((x, y));
    }
}

// ---------------------------------------------------------------- send box

fn draw_input(f: &mut Frame, m: &Monitor, area: Rect) {
    let le = m.profile.line_ending.label();
    let badge = |t: String, bg: Color| Span::styled(t, Style::new().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD));

    // (title, right-hand title, text, placeholder when empty, cursor column)
    let (title, right, body, placeholder, cursor): (Line, Line, String, &str, Option<usize>) = if let Some(s) = &m.secret {
        (
            Line::from(vec![Span::raw(" "), badge(format!(" HIDDEN SEND + {le} "), Color::Red), Span::raw(" ")]),
            Line::from(Span::styled(" not logged · Enter sends · Esc cancels ", DIM)).right_aligned(),
            "•".repeat(s.len()),
            "Type the password and press Enter",
            Some(s.len()),
        )
    } else if m.live {
        (
            Line::from(vec![Span::raw(" "), badge(" LIVE KEYS ".into(), Color::Magenta), Span::raw(format!(" Enter sends {le} "))]),
            Line::from(""),
            String::new(),
            "Every key goes straight to the device. F4 to stop.",
            None,
        )
    } else {
        let mode_bg = match m.profile.input_mode {
            InputMode::Hex => Color::Magenta,
            InputMode::Ascii => Color::Cyan,
        };
        let mut t = vec![Span::raw(" Send "), badge(format!(" {} ", m.profile.input_mode.label()), mode_bg)];
        if m.profile.input_mode == InputMode::Ascii {
            t.push(Span::raw(format!(" + {le} ")));
        } else {
            t.push(Span::raw(" "));
        }
        if m.profile.char_delay_ms > 0 {
            t.push(Span::styled(format!("· {} ms/char ", m.profile.char_delay_ms), Style::new().fg(Color::Yellow)));
        }
        let preview = if m.input.is_empty() {
            Line::from("")
        } else {
            match m.preview() {
                Ok(b) if b.is_empty() => Line::from(""),
                Ok(b) => {
                    let shown: Vec<u8> = b.iter().take(16).copied().collect();
                    let more = if b.len() > 16 { " …" } else { "" };
                    let plural = if b.len() == 1 { "" } else { "s" };
                    Line::from(Span::styled(
                        format!(" {} byte{plural}: {}{more} ", b.len(), codec::hex_string(&shown)),
                        Style::new().fg(Color::Green),
                    ))
                }
                Err(e) => Line::from(Span::styled(format!(" ✘ {e} "), Style::new().fg(Color::LightRed))),
            }
        }
        .right_aligned();
        let placeholder = match m.profile.input_mode {
            InputMode::Hex => "Type hex bytes (e.g. 02 41 30 03) and press Enter",
            InputMode::Ascii => "Type a command and press Enter",
        };
        (Line::from(t), preview, m.input_text(), placeholder, Some(m.cursor))
    };

    let border = if m.secret.is_some() {
        Color::Red
    } else if m.live {
        Color::Magenta
    } else if m.connected {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let block = Block::bordered().border_style(Style::new().fg(border)).title_top(title).title_top(right);
    let inner = block.inner(area);
    if body.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(placeholder, DIM)).block(block), area);
    } else {
        let iw = inner.width.max(1) as usize;
        let chars: Vec<char> = body.chars().collect();
        let start = (cursor.unwrap_or(0) + 1).saturating_sub(iw);
        let shown: String = chars.iter().skip(start).take(iw).collect();
        f.render_widget(Paragraph::new(shown).block(block), area);
    }
    if let Some(c) = cursor {
        if !m.show_help && m.menu.is_none() && m.paste_confirm.is_none() {
            let iw = inner.width.max(1) as usize;
            let start = (c + 1).saturating_sub(iw);
            f.set_cursor_position((inner.x + (c - start) as u16, inner.y));
        }
    }
}

/// One calm line: the few keys you need, plus the latest status message on the right.
fn draw_footer(f: &mut Frame, m: &Monitor, area: Rect) {
    let key = |k: &str| Span::styled(format!(" {k} "), Style::new().fg(Color::Black).bg(Color::Gray));
    let txt = |t: &str| Span::raw(format!(" {t}   "));
    let spans = if m.live {
        vec![
            key("F1"), txt("Menu"), key("F4"), txt("Stop live keys"), key("Ctrl+Shift+Q"), txt("Quit"),
            Span::styled("Other keys go to the device", DIM),
        ]
    } else {
        vec![
            key("F1"), txt("Menu"), key("F4"), txt("Live keys"), key("Ctrl+N"), txt("New connection"),
            key("Ctrl+Q"), txt("Quit"),
        ]
    };
    let left_w: u16 = spans.iter().map(|s| s.width() as u16).sum();
    let [l, r] = Layout::horizontal([Constraint::Length(left_w.min(area.width)), Constraint::Min(0)]).areas(area);
    f.render_widget(Paragraph::new(Line::from(spans)), l);
    if let Some((msg, err)) = m.status_text() {
        let style = if err { Style::new().fg(Color::LightRed) } else { Style::new().fg(Color::Green) };
        f.render_widget(Paragraph::new(Line::from(Span::styled(format!("{msg} "), style)).right_aligned()), r);
    }
}

fn draw_menu(f: &mut Frame, m: &Monitor) {
    let Some(menu) = &m.menu else { return };
    let items = menu::visible(m, &menu.filter);
    let sel = menu.selected.min(items.len().saturating_sub(1));

    // Rows: section headers (when not filtering) + items.
    enum Row<'a> {
        Header(&'a str),
        Item(usize),
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut last = "";
    for (i, it) in items.iter().enumerate() {
        if menu.filter.trim().is_empty() && it.section != last {
            if !rows.is_empty() {
                rows.push(Row::Header(""));
            }
            rows.push(Row::Header(it.section));
            last = it.section;
        }
        rows.push(Row::Item(i));
    }

    let full = f.area();
    let w = 84.min(full.width.saturating_sub(2)).max(30).min(full.width);
    let desc_rows = 3u16;
    let want_h = rows.len() as u16 + 4 + desc_rows + 2;
    let h = want_h.min(full.height.saturating_sub(2)).max(10.min(full.height));
    let area = Rect { x: full.x + (full.width.saturating_sub(w)) / 2, y: full.y + 1.min(full.height.saturating_sub(h)), width: w, height: h };
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(" RawTap menu ", Style::new().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {} ", m.profile_key), DIM),
        ]))
        .title_bottom(Line::from(Span::styled(" ↑↓ move   ←→ change   Enter select   type to search   Esc close ", DIM)))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [search, _gap, list, sep, desc] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(desc_rows),
    ])
    .areas(inner);

    // Search box
    let search_line = if menu.filter.is_empty() {
        Line::from(vec![Span::styled(" › ", Style::new().fg(Color::Cyan)), Span::styled("type to search…", DIM)])
    } else {
        Line::from(vec![Span::styled(" › ", Style::new().fg(Color::Cyan)), Span::raw(menu.filter.clone())])
    };
    f.render_widget(Paragraph::new(search_line), search);
    f.set_cursor_position((search.x + 3 + menu.filter.chars().count() as u16, search.y));

    // List, scrolled so the selection is visible.
    let sel_row = rows.iter().position(|r| matches!(r, Row::Item(i) if *i == sel)).unwrap_or(0);
    let lh = list.height as usize;
    let offset = if sel_row + 1 > lh { sel_row + 1 - lh } else { 0 };
    let lw = list.width as usize;
    let key_w = 9usize;
    let val_w = 16usize;
    let label_w = lw.saturating_sub(key_w + val_w + 3);
    let mut lines = Vec::new();
    for row in rows.iter().skip(offset).take(lh) {
        match row {
            Row::Header(t) => lines.push(Line::from(Span::styled(format!(" {t}"), DIM.add_modifier(Modifier::BOLD)))),
            Row::Item(i) => {
                let it = &items[*i];
                let selected = *i == sel;
                let value = match (&it.value, selected, it.kind) {
                    (Some(v), true, Kind::Setting(_)) => format!("◀ {v} ▶"),
                    (Some(v), _, _) => v.clone(),
                    _ => String::new(),
                };
                let label: String = it.label.chars().take(label_w).collect();
                let text = format!(" {label:<label_w$} {value:<val_w$} {:>key_w$} ", it.key);
                let style = if selected {
                    Style::new().fg(Color::Black).bg(Color::Cyan)
                } else if it.value.is_some() {
                    Style::new().fg(Color::White)
                } else {
                    Style::new()
                };
                if selected {
                    lines.push(Line::from(Span::styled(text, style)));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {label:<label_w$} "), style),
                        Span::styled(format!("{value:<val_w$} "), Style::new().fg(Color::Yellow)),
                        Span::styled(format!("{:>key_w$} ", it.key), DIM),
                    ]));
                }
            }
        }
    }
    if items.is_empty() {
        lines.push(Line::from(Span::styled("  Nothing matches. Backspace or Esc to clear the search.", DIM)));
    }
    f.render_widget(Paragraph::new(lines), list);

    f.render_widget(Paragraph::new(Span::styled("─".repeat(sep.width as usize), DIM)), sep);
    if let Some(it) = items.get(sel) {
        let hint = match it.kind {
            Kind::Setting(_) => "  (←→ to change)",
            Kind::Cmd(_) => "  (Enter)",
        };
        let rows = crate::ui::wrap_words(&format!("{}{hint}", it.desc), desc.width.saturating_sub(2) as usize);
        let text: Vec<Line> = rows
            .into_iter()
            .map(|r| Line::from(Span::styled(format!(" {r}"), Style::new().fg(Color::Gray))))
            .collect();
        f.render_widget(Paragraph::new(text), desc);
    }
}

fn draw_help(f: &mut Frame) {
    let area = centered(f.area(), 96, 38);
    f.render_widget(Clear, area);
    let h = |s: &'static str| Line::from(Span::styled(s, Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    let l = |s: &'static str| Line::from(s);
    let text = vec![
        h("Everything is in the F1 menu. These shortcuts are for when you know what you want:"),
        l(""),
        l("  F1 / Ctrl+P  menu (Ctrl+P only in line mode: in live keys it goes to the device)"),
        l("  F2  line ending          F3  layout              F4  live keys on/off"),
        l("  F5  pane PgUp/Dn scrolls F6  hidden send         F7  serial break"),
        l("  F8  raw log format       F9  pause raw log       F10 clear screen"),
        l("  Ctrl+T char delay   Tab HEX/ASCII   ↑↓ history   Esc clear box / cancel macro"),
        l("  Ctrl+R reconnect    Ctrl+N new connection        Ctrl+Q quit"),
        l("  PgUp/PgDn scroll    Ctrl+Home/End top/bottom     Ctrl+←→ / Ctrl+↑↓ resize panes"),
        l("  Mouse: wheel scrolls the pane under it · drag selects (drag past the edge, or use the"),
        l("         wheel, to keep going) · Ctrl+click extends · Ctrl+Shift+C copies"),
        l("         Raw log copies whole entries in full (hex + text), whatever the view."),
        l("  Ctrl+Shift+Q quits from anywhere, including live keys."),
        l("         Ctrl+Shift+V or right-click pastes (asks first if several lines) · Shift+drag = console select"),
        l("  Macros: Alt+0..9, Shift+F1..F12, F11, F12 (set in rawtap_config.json)"),
        l(""),
        h("Send box"),
        l("  HEX    02 41 03   024103   0x02,0x41,0x03   02:41:03   (sent exactly as typed)"),
        l("  ASCII  text + line ending.  \\r \\n \\t \\0 \\e \\\\ \\xHH   and tokens <STX> <CR> <0x1B> ..."),
        l("         so a received TEXT line can be pasted back.  \\< sends a literal '<'."),
        l(""),
        h("Live keys (F4)"),
        l("  Every key goes straight to the device: Tab, ?, space at --More--, Ctrl+C, Ctrl+Z, arrows."),
        l("  F-keys still work. Keys typed at a 'Password:' prompt are logged as <hidden>."),
        l(""),
        h("Files"),
        l("  logs\\*.log  hex log, both directions     logs\\*.txt  plain-text transcript"),
        l("  captures\\   macro captures               rawtap_config.json  macros, image extraction"),
        l(""),
        h("Command line"),
        l("  rawtap serial -p COM3 -b 9600  ·  rawtap tcp -H 10.0.0.5 -p 2101  ·  rawtap listen -p 9000"),
        l("  rawtap ports  (list COM ports)  ·   --no-log  skip log files"),
        l(""),
        Line::from(Span::styled("  Any key closes this.", DIM)),
    ];
    let block = Block::bordered().title(" Keyboard shortcuts & send syntax ").border_style(Style::new().fg(Color::Cyan));
    f.render_widget(Paragraph::new(text).block(block), area);
}
