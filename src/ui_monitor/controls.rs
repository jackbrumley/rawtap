//! Drawing the controls: the send box, the footer and the help screen.

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
            let remember = m.secret_use == crate::monitor::password::SecretUse::Remember;
            let (badge_text, right, placeholder) = if remember {
                (
                    " REMEMBER PASSWORD ".to_string(),
                    format!(
                        " kept in {} · Enter saves · Esc cancels ",
                        crate::secrets::store_name()
                    ),
                    "Type the password to remember and press Enter",
                )
            } else {
                (
                    format!(" HIDDEN SEND + {le} "),
                    " not logged · Enter sends · Esc cancels ".to_string(),
                    "Type the password and press Enter",
                )
            };
            (
                Line::from(vec![
                    Span::raw(" "),
                    badge(badge_text, Color::Red),
                    Span::raw(" "),
                ]),
                Line::from(Span::styled(right, DIM)).right_aligned(),
                "•".repeat(s.len()),
                placeholder,
                Some(s.len()),
            )
        } else if let Some(name) = &m.log_name {
            (
                Line::from(vec![
                    Span::raw(" "),
                    badge(" NAME THIS SESSION LOG ".into(), Color::Cyan),
                    Span::raw(" "),
                ]),
                Line::from(Span::styled(" Enter renames · Esc keeps its name ", DIM))
                    .right_aligned(),
                name.clone(),
                "Type a name, e.g. core switch config",
                Some(name.chars().count()),
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
            // At a password prompt the box hides what's typed, and the
            // send is logged as <hidden> and kept out of history.
            if m.at_password_prompt() {
                t.push(badge(" PASSWORD · not recorded ".into(), Color::Red));
                (
                    Line::from(t),
                    Line::from(""),
                    "•".repeat(m.input.len()),
                    "Type the password and press Enter",
                    Some(m.cursor),
                )
            } else {
                (
                    Line::from(t),
                    preview,
                    m.input_text(),
                    placeholder,
                    Some(m.cursor),
                )
            }
        };

    let border = if m.secret.is_some() {
        Color::Red
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
pub(super) fn draw_footer(f: &mut Frame, m: &mut Monitor, area: Rect) {
    let key = |k: &str| {
        Span::styled(
            format!(" {k} "),
            Style::new().fg(Color::Black).bg(Color::Gray),
        )
    };
    let txt = |t: &str| Span::raw(format!(" {t}   "));

    // In key order: the menu, then the view switch (the key, what it does,
    // then the current view, which a click changes too; its place is kept
    // for `Monitor::on_mouse`), so a narrow window never hides them.
    let mut spans = vec![
        Span::raw(" "),
        key("F1"),
        txt("Menu"),
        key("F3"),
        Span::raw(" View "),
    ];
    let x = area.x + spans.iter().map(|s| s.width() as u16).sum::<u16>();
    let view = format!(" {} ", m.layout.label());
    m.footer_view = Some(Rect {
        x,
        y: area.y,
        width: view.chars().count() as u16,
        height: 1,
    });
    spans.push(Span::styled(
        view,
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("   "));
    spans.extend(if m.live {
        [key("F4"), txt("Leave live keys")]
    } else {
        [key("F4"), txt("Live keys")]
    });
    if m.layout != crate::config::Layout::Terminal {
        spans.push(key("F8"));
        spans.push(Span::raw(" Hex "));
        spans.push(Span::styled(
            format!("{}   ", m.log_format().label()),
            Style::new().fg(Color::White),
        ));
    }
    spans.extend(if m.live {
        vec![Span::styled(
            "F1 then Ctrl+Q quits · other keys go to the device",
            DIM,
        )]
    } else {
        vec![
            key("Ctrl+N"),
            txt("New connection"),
            key("Ctrl+Q"),
            txt("Quit"),
        ]
    });
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
        l("  F1  menu"),
        l("  F2  line ending          F3  view (Terminal/Split/Hex)   F4  live keys on/off"),
        l("  F5  pane PgUp/Dn scrolls F6  hidden send         F7  break"),
        l("  F8  hex format           F9  pause Hex pane      F10 clear screen"),
        l("  Ctrl+T char delay   Tab HEX/ASCII   ↑↓ history   Esc clear box / cancel macro"),
        l("  Ctrl+R reconnect    Ctrl+N new connection        Ctrl+Q quit"),
        l("  PgUp/PgDn scroll    Ctrl+Home/End top/bottom     Ctrl+←→ / Ctrl+↑↓ resize panes"),
        l(
            "  Mouse: wheel scrolls the pane under it · drag selects (drag past the edge, or use the",
        ),
        l("         wheel, to keep going) · Ctrl+click extends"),
        l(
            "         A selection is copied when you let go · Alt+A (or Ctrl+A in line mode) selects",
        ),
        l("         and copies the whole pane · Ctrl+C or right-click clears the highlight."),
        l("         Right-click with nothing selected pastes (asks first if several lines)."),
        l("         The Hex pane copies whole entries in full (hex + text)."),
        l("  In live keys every key goes to the device, but the F1 menu still works: F1, then"),
        l("  Ctrl+Q quits or Ctrl+N starts a new connection."),
        l("  Macros: Alt+0..9, Shift+F1..F12, F11, F12 (set in settings.json)"),
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
            "  rawtap telnet -H 10.0.0.1  ·  rawtap ssh admin@10.0.0.1  ·  rawtap ports  (list serial ports)",
        ),
        l("  --no-log  skip log files"),
        l(""),
        Line::from(Span::styled("  Any key closes this.", crate::ui::POPUP_DIM)),
    ];
    let block = Block::bordered()
        .title(" Keyboard shortcuts & send syntax ")
        .border_style(Style::new().fg(Color::Cyan));
    f.render_widget(Paragraph::new(text).block(block), area);
}
