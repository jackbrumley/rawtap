//! Drawing the two panes: the raw log and the live terminal.

use super::*;

// ---------------------------------------------------------------- raw log

pub(super) fn wrap_tokens(
    prefix: String,
    tokens: Vec<String>,
    sep: &str,
    style: Style,
    width: usize,
) -> Vec<Line<'static>> {
    let indent = prefix.chars().count();
    let avail = width.saturating_sub(indent).max(8);
    let mut rows: Vec<String> = Vec::new();
    let mut cur = String::new();
    for t in tokens {
        let add = if cur.is_empty() {
            t.len()
        } else {
            sep.len() + t.len()
        };
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
            let p = if i == 0 {
                prefix.clone()
            } else {
                " ".repeat(indent)
            };
            Line::from(vec![Span::styled(p, DIM), Span::styled(r, style)])
        })
        .collect()
}

/// Screen rows for one entry at the given width.
pub(crate) fn entry_lines(e: &Entry, width: u16, fmt: LogFormat) -> Vec<Line<'static>> {
    let w = width as usize;
    let ts = e.time.format("%H:%M:%S%.3f").to_string();
    match &e.kind {
        EntryKind::Rx(d) | EntryKind::Tx(d) => {
            let rx = matches!(e.kind, EntryKind::Rx(_));
            let dir = if rx { "RX" } else { "TX" };
            let (hex_style, text_style) = if rx {
                (Style::new().fg(Color::Cyan), Style::new().fg(Color::White))
            } else {
                (
                    Style::new().fg(Color::Yellow),
                    Style::new().fg(Color::LightYellow),
                )
            };
            if fmt == LogFormat::Compact {
                return vec![compact_line(&ts, dir, d, w, hex_style, text_style)];
            }
            let mut out = Vec::new();
            if fmt != LogFormat::Text {
                let toks = d.iter().map(|b| format!("{b:02X}")).collect();
                out.extend(wrap_tokens(
                    format!("{ts} {dir} HEX  "),
                    toks,
                    " ",
                    hex_style,
                    w,
                ));
            }
            if fmt != LogFormat::Hex {
                let toks = d.iter().map(|&b| codec::byte_text(b)).collect();
                out.extend(wrap_tokens(
                    format!("{ts} {dir} TEXT "),
                    toks,
                    "",
                    text_style,
                    w,
                ));
            }
            out
        }
        EntryKind::TxHidden(n) => vec![Line::from(vec![
            Span::styled(format!("{ts} TX "), DIM),
            Span::styled(
                format!("<hidden: {n} bytes>"),
                Style::new().fg(Color::Magenta),
            ),
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
                return vec![Line::from(vec![
                    Span::styled(prefix, DIM),
                    Span::styled(body, style),
                ])];
            }
            let mut out = Vec::new();
            for line in m.lines() {
                let toks = line.split(' ').map(str::to_string).collect();
                let prefix = if out.is_empty() {
                    format!("{ts} {mark}")
                } else {
                    " ".repeat(ts.len() + 3)
                };
                out.extend(wrap_tokens(prefix, toks, " ", style, w));
            }
            out
        }
    }
}

/// One row: "12:25:31.749 RX   14 73 68 6F 77 20 72 … │ show r…"
pub(super) fn compact_line(
    ts: &str,
    dir: &str,
    d: &[u8],
    w: usize,
    hex_style: Style,
    text_style: Style,
) -> Line<'static> {
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

pub(super) fn draw_log(f: &mut Frame, m: &mut Monitor, area: Rect) {
    let fmt = m.log_format();
    let mut block = pane_block(m, Pane::Log).title_top(Line::from(vec![
        Span::raw(" Hex "),
        Span::styled(format!("· {} (F8) ", fmt.label()), DIM),
    ]));
    if !m.follow {
        block = block.title_top(
            Line::from(Span::styled(
                " ⏸ paused · F9 or Ctrl+End to follow ",
                Style::new().fg(Color::Yellow),
            ))
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
    if let (Some(s), Some((x, y))) = (m.selection, m.drag_pos)
        && s.dragging
        && s.pane == Pane::Log
    {
        let head = m.pos_at(Pane::Log, x, y);
        let moved = s.moved || head != s.anchor;
        m.selection = Some(crate::monitor::Selection { head, moved, ..s });
    }
    f.render_widget(Paragraph::new(rows), inner);
    highlight_log(f, m, inner);
}

// ---------------------------------------------------------------- terminal

pub(super) fn vt_color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub(super) fn draw_term(f: &mut Frame, m: &mut Monitor, area: Rect) {
    let inner = Block::bordered().inner(area);
    m.resize_terminal(inner.height, inner.width);
    m.term_rows = inner.height;
    m.term_inner = inner;
    let abs_top = m.term.abs_top();
    if let (Some(s), Some((x, y))) = (m.selection, m.drag_pos)
        && s.dragging
        && s.pane == Pane::Terminal
    {
        let head = m.pos_at(Pane::Terminal, x, y);
        let moved = s.moved || head != s.anchor;
        m.selection = Some(crate::monitor::Selection { head, moved, ..s });
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
            Line::from(Span::styled(
                " LIVE KEYS · F4 to leave ",
                Style::new().fg(Color::Black).bg(Color::Magenta),
            ))
            .right_aligned(),
        );
    }
    f.render_widget(block, area);

    if m.rx_bytes == 0 && back == 0 {
        let hint = vec![
            Line::from(Span::styled(
                "Device output appears here, as it would in PuTTY.",
                DIM,
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Type a command below and press Enter, or press F4 for live keys",
                DIM,
            )),
            Line::from(Span::styled(
                "(Tab completion, ?, space at --More--, Ctrl+C, arrow keys).",
                DIM,
            )),
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
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let mut st = Style::new()
                .fg(vt_color(cell.fgcolor()))
                .bg(vt_color(cell.bgcolor()));
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
                cur.push_str(cell.contents());
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
    if m.live
        && m.secret.is_none()
        && !m.show_help
        && m.menu.is_none()
        && back == 0
        && !screen.hide_cursor()
    {
        let (r, c) = screen.cursor_position();
        let x = inner.x + c.min(inner.width.saturating_sub(1));
        let y = inner.y + r.min(inner.height.saturating_sub(1));
        f.set_cursor_position((x, y));
    }
}
