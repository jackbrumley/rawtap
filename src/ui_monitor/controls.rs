//! Drawing the controls: the send box, the footer, the F1 menu and the help screen.

use super::*;

// ---------------------------------------------------------------- send box

pub(super) fn draw_input(f: &mut Frame, m: &Monitor, area: Rect) {
    let le = m.profile.line_ending.label();
    let badge = |t: String, bg: Color| {
        Span::styled(
            t,
            Style::new()
                .fg(Color::Black)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        )
    };

    // (title, right-hand title, text, placeholder when empty, cursor column)
    let (title, right, body, placeholder, cursor): (Line, Line, String, &str, Option<usize>) =
        if let Some(s) = &m.secret {
            (
                Line::from(vec![
                    Span::raw(" "),
                    badge(format!(" HIDDEN SEND + {le} "), Color::Red),
                    Span::raw(" "),
                ]),
                Line::from(Span::styled(
                    " not logged · Enter sends · Esc cancels ",
                    DIM,
                ))
                .right_aligned(),
                "•".repeat(s.len()),
                "Type the password and press Enter",
                Some(s.len()),
            )
        } else if m.live {
            (
                Line::from(vec![
                    Span::raw(" "),
                    badge(" LIVE KEYS ".into(), Color::Magenta),
                    Span::raw(format!(" Enter sends {le} ")),
                ]),
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
            let mut t = vec![
                Span::raw(" Send "),
                badge(format!(" {} ", m.profile.input_mode.label()), mode_bg),
            ];
            if m.profile.input_mode == InputMode::Ascii {
                t.push(Span::raw(format!(" + {le} ")));
            } else {
                t.push(Span::raw(" "));
            }
            if m.profile.char_delay_ms > 0 {
                t.push(Span::styled(
                    format!("· {} ms/char ", m.profile.char_delay_ms),
                    Style::new().fg(Color::Yellow),
                ));
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
                            format!(
                                " {} byte{plural}: {}{more} ",
                                b.len(),
                                codec::hex_string(&shown)
                            ),
                            Style::new().fg(Color::Green),
                        ))
                    }
                    Err(e) => Line::from(Span::styled(
                        format!(" ✘ {e} "),
                        Style::new().fg(Color::LightRed),
                    )),
                }
            }
            .right_aligned();
            let placeholder = match m.profile.input_mode {
                InputMode::Hex => "Type hex bytes (e.g. 02 41 30 03) and press Enter",
                InputMode::Ascii => "Type a command and press Enter",
            };
            (
                Line::from(t),
                preview,
                m.input_text(),
                placeholder,
                Some(m.cursor),
            )
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
    let block = Block::bordered()
        .border_style(Style::new().fg(border))
        .title_top(title)
        .title_top(right);
    let inner = block.inner(area);
    if body.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(placeholder, DIM)).block(block),
            area,
        );
    } else {
        let iw = inner.width.max(1) as usize;
        let chars: Vec<char> = body.chars().collect();
        let start = (cursor.unwrap_or(0) + 1).saturating_sub(iw);
        let shown: String = chars.iter().skip(start).take(iw).collect();
        f.render_widget(Paragraph::new(shown).block(block), area);
    }
    if let Some(c) = cursor
        && !m.show_help
        && m.menu.is_none()
        && m.paste_confirm.is_none()
    {
        let iw = inner.width.max(1) as usize;
        let start = (c + 1).saturating_sub(iw);
        f.set_cursor_position((inner.x + (c - start) as u16, inner.y));
    }
}

/// One calm line: the few keys you need, plus the latest status message on the right.
pub(super) fn draw_footer(f: &mut Frame, m: &Monitor, area: Rect) {
    let key = |k: &str| {
        Span::styled(
            format!(" {k} "),
            Style::new().fg(Color::Black).bg(Color::Gray),
        )
    };
    let txt = |t: &str| Span::raw(format!(" {t}   "));
    let spans = if m.live {
        vec![
            key("F1"),
            txt("Menu"),
            key("F4"),
            txt("Stop live keys"),
            key("Ctrl+Shift+Q"),
            txt("Quit"),
            Span::styled("Other keys go to the device", DIM),
        ]
    } else {
        vec![
            key("F1"),
            txt("Menu"),
            key("F4"),
            txt("Live keys"),
            key("Ctrl+N"),
            txt("New connection"),
            key("Ctrl+Q"),
            txt("Quit"),
        ]
    };
    let left_w: u16 = spans.iter().map(|s| s.width() as u16).sum();
    let [l, r] = Layout::horizontal([
        Constraint::Length(left_w.min(area.width)),
        Constraint::Min(0),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(Line::from(spans)), l);
    if let Some((msg, err)) = m.status_text() {
        let style = if err {
            Style::new().fg(Color::LightRed)
        } else {
            Style::new().fg(Color::Green)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("{msg} "), style)).right_aligned()),
            r,
        );
    }
}

pub(super) fn draw_menu(f: &mut Frame, m: &Monitor) {
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
    let h = want_h
        .min(full.height.saturating_sub(2))
        .max(10.min(full.height));
    let area = Rect {
        x: full.x + (full.width.saturating_sub(w)) / 2,
        y: full.y + 1.min(full.height.saturating_sub(h)),
        width: w,
        height: h,
    };
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " RawTap menu ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {} ", m.profile_key), DIM),
        ]))
        .title_bottom(Line::from(Span::styled(
            " ↑↓ move   ←→ change   Enter select   type to search   Esc close ",
            DIM,
        )))
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
        Line::from(vec![
            Span::styled(" › ", Style::new().fg(Color::Cyan)),
            Span::styled("type to search…", DIM),
        ])
    } else {
        Line::from(vec![
            Span::styled(" › ", Style::new().fg(Color::Cyan)),
            Span::raw(menu.filter.clone()),
        ])
    };
    f.render_widget(Paragraph::new(search_line), search);
    f.set_cursor_position((search.x + 3 + menu.filter.chars().count() as u16, search.y));

    // List, scrolled so the selection is visible.
    let sel_row = rows
        .iter()
        .position(|r| matches!(r, Row::Item(i) if *i == sel))
        .unwrap_or(0);
    let lh = list.height as usize;
    let offset = (sel_row + 1).saturating_sub(lh);
    let lw = list.width as usize;
    let key_w = 9usize;
    let val_w = 16usize;
    let label_w = lw.saturating_sub(key_w + val_w + 3);
    let mut lines = Vec::new();
    for row in rows.iter().skip(offset).take(lh) {
        match row {
            Row::Header(t) => lines.push(Line::from(Span::styled(
                format!(" {t}"),
                DIM.add_modifier(Modifier::BOLD),
            ))),
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
        lines.push(Line::from(Span::styled(
            "  Nothing matches. Backspace or Esc to clear the search.",
            DIM,
        )));
    }
    f.render_widget(Paragraph::new(lines), list);

    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(sep.width as usize), DIM)),
        sep,
    );
    if let Some(it) = items.get(sel) {
        let hint = match it.kind {
            Kind::Setting(_) => "  (←→ to change)",
            Kind::Cmd(_) => "  (Enter)",
        };
        let rows = crate::ui::wrap_words(
            &format!("{}{hint}", it.desc),
            desc.width.saturating_sub(2) as usize,
        );
        let text: Vec<Line> = rows
            .into_iter()
            .map(|r| Line::from(Span::styled(format!(" {r}"), Style::new().fg(Color::Gray))))
            .collect();
        f.render_widget(Paragraph::new(text), desc);
    }
}

pub(super) fn draw_help(f: &mut Frame) {
    let area = centered(f.area(), 96, 38);
    f.render_widget(Clear, area);
    let h = |s: &'static str| {
        Line::from(Span::styled(
            s,
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
    };
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
        l(
            "  Mouse: wheel scrolls the pane under it · drag selects (drag past the edge, or use the",
        ),
        l("         wheel, to keep going) · Ctrl+click extends · Ctrl+Shift+C copies"),
        l("         Raw log copies whole entries in full (hex + text), whatever the view."),
        l("  Ctrl+Shift+Q quits from anywhere, including live keys."),
        l(
            "         Ctrl+Shift+V or right-click pastes (asks first if several lines) · Shift+drag = console select",
        ),
        l("  Macros: Alt+0..9, Shift+F1..F12, F11, F12 (set in rawtap_config.json)"),
        l(""),
        h("Send box"),
        l("  HEX    02 41 03   024103   0x02,0x41,0x03   02:41:03   (sent exactly as typed)"),
        l(
            "  ASCII  text + line ending.  \\r \\n \\t \\0 \\e \\\\ \\xHH   and tokens <STX> <CR> <0x1B> ...",
        ),
        l("         so a received TEXT line can be pasted back.  \\< sends a literal '<'."),
        l(""),
        h("Live keys (F4)"),
        l(
            "  Every key goes straight to the device: Tab, ?, space at --More--, Ctrl+C, Ctrl+Z, arrows.",
        ),
        l("  F-keys still work. Keys typed at a 'Password:' prompt are logged as <hidden>."),
        l(""),
        h("Files (in ~/.config/rawtap)"),
        l("  logs/*.log  hex log, both directions     logs/*.txt  plain-text transcript"),
        l("  captures/   macro captures               settings.json  macros, image extraction"),
        l(""),
        h("Command line"),
        l(
            "  rawtap serial -p COM3 -b 9600  ·  rawtap tcp -H 10.0.0.5 -p 2101  ·  rawtap listen -p 9000",
        ),
        l(
            "  rawtap telnet -H 10.0.0.1  ·  rawtap ports  (list serial ports)  ·  --no-log  skip log files",
        ),
        l(""),
        Line::from(Span::styled("  Any key closes this.", DIM)),
    ];
    let block = Block::bordered()
        .title(" Keyboard shortcuts & send syntax ")
        .border_style(Style::new().fg(Color::Cyan));
    f.render_widget(Paragraph::new(text).block(block), area);
}
