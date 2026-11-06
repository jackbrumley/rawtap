//! Rendering: dispatch, and the setup screen. The monitor screen is in ui_monitor.rs.

use crate::app::{App, Screen, Setup};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

pub const DIM: Style = Style::new().fg(Color::DarkGray);

pub fn draw(f: &mut Frame, app: &mut App) {
    match app.screen {
        Screen::Setup => draw_setup(f, app),
        Screen::Monitor => {
            if let Some(m) = app.monitor.as_mut() {
                crate::ui_monitor::draw(f, m);
            }
        }
    }
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

// ---------------------------------------------------------------- setup

fn draw_setup(f: &mut Frame, app: &App) {
    let fields = Setup::fields(app.saved.mode, app.setup.options_open);
    let ports_rows = if app.saved.mode == crate::config::ConnMode::Serial { app.setup.ports.len().max(1) + 2 } else { 0 };
    let notes_rows = app.notes.len() + app.setup.error.as_ref().map_or(0, |e| e.lines().count() + 2) + 1;
    let h = (fields.len() + ports_rows + notes_rows + 8 + 5) as u16;
    // Centred horizontally but pinned near the top, so the box doesn't jump when the
    // number of fields changes between Serial / TCP / Listen.
    let full = f.area();
    let w = 80.min(full.width);
    let top = if full.height >= h + 2 { 1 } else { 0 };
    let area = Rect {
        x: full.x + (full.width - w) / 2,
        y: full.y + top,
        width: w,
        height: h.min(full.height - top),
    };
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(" RawTap ", Style::new().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" raw serial / TCP monitor "),
        ]))
        .title_bottom(Line::from(Span::styled(" ↑↓ move  ←→ change  type to edit  Enter connect  F5 rescan  Esc quit ", DIM)))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![Line::from("")];
    let mut cursor = None;
    for (i, fld) in fields.iter().enumerate() {
        let focused = i == app.setup.focus;
        let value = app.field_value(*fld);
        let label_style = if focused { Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::new() };
        let marker = if focused { " ▶ " } else { "   " };
        let mut spans = vec![
            Span::styled(marker, Style::new().fg(Color::Cyan)),
            Span::styled(format!("{:<14}", fld.label()), label_style),
        ];
        let val_style = if focused { Style::new().fg(Color::Black).bg(Color::Cyan) } else { Style::new().fg(Color::White) };
        let cyclable = !fld.is_text() || matches!(fld, crate::app::Field::SerialPort | crate::app::Field::Baud);
        let cyclable = cyclable && *fld != crate::app::Field::Options;
        if focused && cyclable {
            spans.push(Span::styled("◀ ", DIM));
        }
        spans.push(Span::styled(format!(" {value} "), val_style));
        if focused && cyclable {
            spans.push(Span::styled(" ▶", DIM));
        }
        if focused && fld.is_text() {
            let x = inner.x + 3 + 14 + if cyclable { 2 } else { 0 } + 1 + value.chars().count() as u16;
            cursor = Some((x, inner.y + lines.len() as u16));
        }
        if *fld == crate::app::Field::SerialPort {
            if let Some(p) = app.setup.ports.iter().find(|p| p.name.eq_ignore_ascii_case(value.trim())) {
                spans.push(Span::styled(format!("  {}", p.desc), DIM));
            } else if !value.trim().is_empty() {
                spans.push(Span::styled("  (not detected right now)", Style::new().fg(Color::Yellow)));
            }
        }
        lines.push(Line::from(spans));
    }

    if app.saved.mode == crate::config::ConnMode::Serial {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("   Detected ports (refreshes every 2 s)", DIM)));
        if app.setup.ports.is_empty() {
            lines.push(Line::from(Span::styled("     none found. Plug in the device or USB adapter.", Style::new().fg(Color::Yellow))));
        }
        for p in &app.setup.ports {
            lines.push(Line::from(vec![
                Span::styled(format!("     {:<8}", p.name), Style::new().fg(Color::White)),
                Span::styled(p.desc.clone(), DIM),
            ]));
        }
    }

    // What the highlighted field does.
    if let Some(fld) = fields.get(app.setup.focus) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(format!("   {}", "─".repeat(inner.width.saturating_sub(6) as usize)), DIM)));
        // Always two rows, so the box doesn't change height as the selection moves.
        let mut rows = wrap_words(fld.description(), inner.width.saturating_sub(6) as usize);
        rows.resize(2, String::new());
        for r in rows.into_iter().take(2) {
            lines.push(Line::from(Span::styled(format!("   {r}"), Style::new().fg(Color::Gray))));
        }
    }

    lines.push(Line::from(""));
    if app.setup.connecting {
        lines.push(Line::from(Span::styled("   Connecting…", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    } else {
        lines.push(Line::from(vec![
            Span::raw("   Press "),
            Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)),
            Span::raw(" to connect"),
        ]));
    }
    if let Some(e) = &app.setup.error {
        lines.push(Line::from(""));
        for l in e.lines() {
            lines.push(Line::from(Span::styled(format!("   {l}"), Style::new().fg(Color::LightRed))));
        }
    }
    for n in &app.notes {
        lines.push(Line::from(Span::styled(format!("   {n}"), DIM)));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    if let Some(c) = cursor {
        f.set_cursor_position(c);
    }
}

/// Greedy word wrap.
pub fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut rows = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            rows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        rows.push(cur);
    }
    rows
}
