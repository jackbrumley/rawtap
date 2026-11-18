//! Drawing the connection panel: a new connection or a favourite being
//! edited, with its fields and, for a serial connection, the detected ports.

use super::DIM;
use crate::app::App;
use crate::app::setup::{Field, Panel};
use crate::config::ConnMode;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Width of the field labels.
const LABEL_WIDTH: usize = 20;

/// The panel's rows: how many the fields take, and how many the detected
/// ports would take with every port listed.
pub(super) fn rows(panel: &Panel, ports: usize) -> (usize, usize) {
    // A blank row parts the name from the connection's fields.
    let fields = panel.fields().len() + 1;
    let ports_rows = if panel.connection.mode == ConnMode::Serial {
        ports.max(1) + 2
    } else {
        0
    };
    (fields, ports_rows)
}

/// Adds the panel's rows to `lines`; returns where the cursor goes when a
/// text field is highlighted.
pub(super) fn draw_rows(
    app: &App,
    panel: &Panel,
    inner: Rect,
    ports_shown: usize,
    lines: &mut Vec<Line<'static>>,
) -> Option<(u16, u16)> {
    let mut cursor = None;
    for (i, field) in panel.fields().into_iter().enumerate() {
        let focused = i == panel.focus;
        let value = app.field_value(panel, field);
        let label_style = if focused {
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let marker = if focused { " ► " } else { "   " };
        let mut spans = vec![
            Span::styled(marker, Style::new().fg(Color::Cyan)),
            Span::styled(format!("{:<LABEL_WIDTH$}", field.label()), label_style),
        ];
        let val_style = if focused {
            Style::new().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::new().fg(Color::White)
        };
        let cyclable = field.cycles() && field != Field::Options;
        if focused && cyclable {
            spans.push(Span::styled("◄ ", DIM));
        }
        if field == Field::Name && value.is_empty() && !focused {
            spans.push(Span::styled(" only needed to save it as a favourite", DIM));
        } else {
            spans.push(Span::styled(format!(" {value} "), val_style));
        }
        if focused && cyclable {
            spans.push(Span::styled(" ►", DIM));
        }
        if focused && field.is_text() {
            let x = inner.x
                + 3
                + LABEL_WIDTH as u16
                + if cyclable { 2 } else { 0 }
                + 1
                + panel.cursor_in(&value) as u16;
            cursor = Some((x, inner.y + lines.len() as u16));
        }
        if field == Field::SerialPort {
            spans.extend(port_note(app, &value));
        }
        lines.push(Line::from(spans));
        if field == Field::Name {
            lines.push(Line::from(""));
        }
    }
    if panel.connection.mode == ConnMode::Serial {
        detected_ports(app, ports_shown, lines);
    }
    cursor
}

/// After the serial port: what it is, or that it isn't plugged in.
fn port_note(app: &App, value: &str) -> Option<Span<'static>> {
    if let Some(p) = app
        .setup
        .ports
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(value.trim()))
    {
        Some(Span::styled(format!("  {}", p.desc), DIM))
    } else if !value.trim().is_empty() {
        Some(Span::styled(
            "  (not detected right now)",
            Style::new().fg(Color::Yellow),
        ))
    } else {
        None
    }
}

fn detected_ports(app: &App, shown: usize, lines: &mut Vec<Line<'static>>) {
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
    for p in app.setup.ports.iter().take(shown) {
        lines.push(Line::from(vec![
            Span::styled(format!("     {:<8}", p.name), Style::new().fg(Color::White)),
            Span::styled(p.desc.clone(), DIM),
        ]));
    }
    let hidden = app.setup.ports.len() - shown;
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("     … and {hidden} more. ←→ on Serial port goes through them all."),
            DIM,
        )));
    }
}
