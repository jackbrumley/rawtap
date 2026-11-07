//! Rendering: dispatch, and the setup screen. The monitor screen is in ui_monitor.rs.

use crate::app::setup::Setup;
use crate::app::update_prompt::UpdatePrompt;
use crate::app::{App, Screen};
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
    if let Some(prompt) = &app.update_prompt {
        // Over the start screen's box (pinned near the top), so the dialog
        // sits on what it's about; the monitor fills the window.
        let over = match app.screen {
            Screen::Setup => setup_layout(app, f.area()).area,
            Screen::Monitor => f.area(),
        };
        draw_update_prompt(f, prompt, over);
    }
}

/// Greys out everything behind a dialog (the F1 menu, the update dialog).
pub fn dim(f: &mut Frame) {
    let area = f.area();
    f.buffer_mut().set_style(
        area,
        Style::new()
            .fg(Color::DarkGray)
            .bg(Color::Reset)
            .remove_modifier(Modifier::all()),
    );
}

/// The "Update Available" dialog, worded as in Voquill.
fn draw_update_prompt(f: &mut Frame, prompt: &UpdatePrompt, over: Rect) {
    dim(f);
    let check = &prompt.check;
    let available = check.update_available;
    let (title, mut body) = if available {
        (
            " Update Available ",
            vec![
                format!(
                    "A newer RawTap version is available. Current: v{} -> Latest: v{}.",
                    check.current_version, check.latest_version
                ),
                String::new(),
                "Press \"Update Now\" to automatically download and install the latest release. RawTap will restart after updating.".to_string(),
            ],
        )
    } else {
        (
            " RawTap is Up to Date ",
            vec![format!(
                "You are on the latest version (v{}).",
                check.current_version
            )],
        )
    };
    if let Some(error) = &prompt.error {
        body.push(String::new());
        body.push(error.clone());
    }

    let full = f.area();
    let w = 72.min(full.width.saturating_sub(2)).max(30.min(full.width));
    let text_width = w.saturating_sub(6) as usize;
    let mut lines = vec![Line::from("")];
    for paragraph in &body {
        if paragraph.is_empty() {
            lines.push(Line::from(""));
        }
        for row in wrap_words(paragraph, text_width) {
            let style = if prompt.error.as_ref() == Some(paragraph) {
                Style::new().fg(Color::LightRed)
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(format!("  {row}"), style)));
        }
    }
    lines.push(Line::from(""));
    let mut buttons = vec![Span::raw("  ")];
    for (i, button) in prompt.buttons().iter().enumerate() {
        if i > 0 {
            buttons.push(Span::raw("   "));
        }
        let style = if *button == prompt.selected {
            Style::new()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::White).bg(Color::DarkGray)
        };
        buttons.push(Span::styled(
            format!(" {} ", button.label(available)),
            style,
        ));
    }
    lines.push(Line::from(buttons));
    lines.push(Line::from(""));

    let h = (lines.len() as u16 + 2).min(full.height);
    // Centred on `over` at full size (never squeezed into a smaller box),
    // then kept on screen.
    let x = (over.x + over.width.saturating_sub(w) / 2).min(full.x + full.width.saturating_sub(w));
    let y =
        (over.y + over.height.saturating_sub(h) / 2).min(full.y + full.height.saturating_sub(h));
    let area = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(Span::styled(
            title,
            Style::new()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )))
        .title_bottom(Line::from(Span::styled(
            " ←→ choose  Enter select  Esc later ",
            DIM,
        )))
        .border_style(Style::new().fg(Color::Green));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

// ---------------------------------------------------------------- setup

/// Where the start screen's box goes, and its wrapped notes and errors.
struct SetupLayout {
    area: Rect,
    error_rows: Vec<String>,
    note_rows: Vec<String>,
}

fn setup_layout(app: &App, full: Rect) -> SetupLayout {
    let fields = Setup::fields(app.saved.mode, app.setup.options_open);
    let ports_rows = if app.saved.mode == crate::config::ConnMode::Serial {
        app.setup.ports.len().max(1) + 2
    } else {
        0
    };
    // Centred horizontally but pinned near the top, so the box doesn't jump when the
    // number of fields changes between Serial / TCP / Listen.
    let w = 80.min(full.width);
    // Notes and errors are wrapped here, indented on every row: the inner
    // width less the borders and the three-space indent.
    let text_width = w.saturating_sub(6) as usize;
    let error_rows: Vec<String> = app.setup.error.as_ref().map_or(Vec::new(), |e| {
        e.lines().flat_map(|l| wrap_words(l, text_width)).collect()
    });
    let note_rows: Vec<String> = app
        .notes
        .iter()
        .flat_map(|n| wrap_words(n, text_width))
        .collect();
    let notes_rows = note_rows.len()
        + if error_rows.is_empty() {
            0
        } else {
            error_rows.len() + 2
        }
        + 1;
    let h = (fields.len() + ports_rows + notes_rows + 8 + 5) as u16;
    let top = if full.height >= h + 2 { 1 } else { 0 };
    let area = Rect {
        x: full.x + (full.width - w) / 2,
        y: full.y + top,
        width: w,
        height: h.min(full.height - top),
    };
    SetupLayout {
        area,
        error_rows,
        note_rows,
    }
}

fn draw_setup(f: &mut Frame, app: &App) {
    let fields = Setup::fields(app.saved.mode, app.setup.options_open);
    let SetupLayout {
        area,
        error_rows,
        note_rows,
    } = setup_layout(app, f.area());
    f.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " RawTap ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" raw serial / TCP monitor "),
        ]))
        .title_bottom(Line::from(Span::styled(
            " ↑↓ move  ←→ change  type to edit  Enter connect  F5 rescan  Esc quit ",
            DIM,
        )))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![Line::from("")];
    let mut cursor = None;
    for (i, fld) in fields.iter().enumerate() {
        let focused = i == app.setup.focus;
        let value = app.field_value(*fld);
        let label_style = if focused {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let marker = if focused { " ▶ " } else { "   " };
        let mut spans = vec![
            Span::styled(marker, Style::new().fg(Color::Cyan)),
            Span::styled(format!("{:<14}", fld.label()), label_style),
        ];
        let val_style = if focused {
            Style::new().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::new().fg(Color::White)
        };
        let cyclable = !fld.is_text()
            || matches!(
                fld,
                crate::app::setup::Field::SerialPort | crate::app::setup::Field::Baud
            );
        let cyclable = cyclable && *fld != crate::app::setup::Field::Options;
        if focused && cyclable {
            spans.push(Span::styled("◀ ", DIM));
        }
        spans.push(Span::styled(format!(" {value} "), val_style));
        if focused && cyclable {
            spans.push(Span::styled(" ▶", DIM));
        }
        if focused && fld.is_text() {
            let x =
                inner.x + 3 + 14 + if cyclable { 2 } else { 0 } + 1 + value.chars().count() as u16;
            cursor = Some((x, inner.y + lines.len() as u16));
        }
        if *fld == crate::app::setup::Field::SerialPort {
            if let Some(p) = app
                .setup
                .ports
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(value.trim()))
            {
                spans.push(Span::styled(format!("  {}", p.desc), DIM));
            } else if !value.trim().is_empty() {
                spans.push(Span::styled(
                    "  (not detected right now)",
                    Style::new().fg(Color::Yellow),
                ));
            }
        }
        lines.push(Line::from(spans));
    }

    if app.saved.mode == crate::config::ConnMode::Serial {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "   Detected ports (refreshes every 2 s)",
            DIM,
        )));
        if app.setup.ports.is_empty() {
            lines.push(Line::from(Span::styled(
                "     none found. Plug in the device or USB adapter.",
                Style::new().fg(Color::Yellow),
            )));
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
        lines.push(Line::from(Span::styled(
            format!("   {}", "─".repeat(inner.width.saturating_sub(6) as usize)),
            DIM,
        )));
        // Always two rows, so the box doesn't change height as the selection moves.
        let mut rows = wrap_words(fld.description(), inner.width.saturating_sub(6) as usize);
        rows.resize(2, String::new());
        for r in rows.into_iter().take(2) {
            lines.push(Line::from(Span::styled(
                format!("   {r}"),
                Style::new().fg(Color::Gray),
            )));
        }
    }

    lines.push(Line::from(""));
    if let Some(attempt) = app.connecting.as_ref().filter(|a| !a.reconnect) {
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "   {} Connecting to {}…",
                    crate::app::connect::spinner(attempt.started),
                    attempt.settings.describe()
                ),
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   Esc cancels", DIM),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("   Press "),
            Span::styled(" Enter ", Style::new().fg(Color::Black).bg(Color::Green)),
            Span::raw(" to connect"),
        ]));
    }
    if !error_rows.is_empty() {
        lines.push(Line::from(""));
        for row in &error_rows {
            lines.push(Line::from(Span::styled(
                format!("   {row}"),
                Style::new().fg(Color::LightRed),
            )));
        }
    }
    for row in &note_rows {
        lines.push(Line::from(Span::styled(format!("   {row}"), DIM)));
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
