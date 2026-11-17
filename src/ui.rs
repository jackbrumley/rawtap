//! Rendering: dispatch, and the setup screen. The monitor screen is in ui_monitor.rs.

mod connection_panel;
mod favourites;

use crate::app::setup::{ListRow, Setup};
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
    if let Some(dialog) = &app.favourite_dialog {
        let over = match app.screen {
            Screen::Setup => setup_layout(app, f.area()).area,
            Screen::Monitor => f.area(),
        };
        favourites::draw_dialog(f, dialog, &app.saved, over);
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

/// Where the start screen's box goes, its wrapped notes and errors, and how
/// many detected ports fit.
struct SetupLayout {
    area: Rect,
    error_rows: Vec<String>,
    note_rows: Vec<String>,
    /// Detected ports listed in the connection panel; the rest are summed up
    /// in one "… and N more" row. The list is the only part that gives way
    /// in a short window, so the keys, errors and notes under it always show.
    ports_shown: usize,
}

/// The list's rows: "New connection", a gap, the "Favourites" heading, then
/// the favourites (or a line saying how to save one).
fn list_rows(app: &App) -> usize {
    3 + app.saved.favourites.len().max(1)
}

fn setup_layout(app: &App, full: Rect) -> SetupLayout {
    // The panel has a description of the highlighted field (blank, rule,
    // two rows); the list's keys say enough on their own.
    let (body_rows, ports_rows, description_rows) = match &app.setup.panel {
        Some(panel) => {
            let (fields, ports) = connection_panel::rows(panel, app.setup.ports.len());
            (fields, ports, 4)
        }
        None => (list_rows(app), 0, 0),
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
    // Rows a wrapped line really takes: one longer than the box (a path with
    // no space to break at) wraps again when drawn.
    let inner_width = w.saturating_sub(2).max(1) as usize;
    let rows_for = |row: &String| (row.chars().count() + 3).div_ceil(inner_width).max(1);
    let notes_rows = note_rows.iter().map(rows_for).sum::<usize>()
        + if error_rows.is_empty() {
            0
        } else {
            1 + error_rows.iter().map(rows_for).sum::<usize>()
        };
    // Exactly what `draw_setup` draws: the borders, the blank top row, the
    // rows and port list, the panel's description, the keys (blank, one
    // row), then errors and notes.
    let h = (2 + 1 + body_rows + ports_rows + description_rows + 2 + notes_rows) as u16;
    let top = if full.height >= h + 2 { 1 } else { 0 };
    // Too tall: list fewer ports, and one row saying how many more there are.
    let excess = h.saturating_sub(full.height - top) as usize;
    let ports = app.setup.ports.len();
    let ports_shown = if excess > 0 && ports_rows > 0 && ports > 1 {
        ports.saturating_sub(excess + 1)
    } else {
        ports
    };
    let saved = if ports_shown < ports {
        ports - ports_shown - 1
    } else {
        0
    };
    let area = Rect {
        x: full.x + (full.width - w) / 2,
        y: full.y + top,
        width: w,
        height: (h - saved as u16).min(full.height - top),
    };
    SetupLayout {
        area,
        error_rows,
        note_rows,
        ports_shown,
    }
}

fn draw_setup(f: &mut Frame, app: &App) {
    let SetupLayout {
        area,
        error_rows,
        note_rows,
        ports_shown,
    } = setup_layout(app, f.area());
    f.render_widget(Clear, area);
    let heading = match &app.setup.panel {
        None => " raw serial / TCP monitor ".to_string(),
        Some(panel) => match panel.favourite {
            None => " New connection ".to_string(),
            Some(_) => " Edit favourite ".to_string(),
        },
    };
    let block = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " RawTap ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(heading),
        ]))
        .title_bottom(Line::from(Span::styled(
            match app.setup.panel {
                None => " ↑↓ move  Ctrl+Q quit ",
                Some(_) => " ↑↓ move  ←→ change  F5 rescan  Esc back  Ctrl+Q quit ",
            },
            DIM,
        )))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![Line::from("")];
    let mut cursor = None;
    // What the highlighted field does (in the panel), and the keys for it.
    let (description, actions): (Option<String>, &[(&str, &str)]) = match &app.setup.panel {
        Some(panel) => {
            cursor = connection_panel::draw_rows(app, panel, inner, ports_shown, &mut lines);
            let field = panel.fields()[panel.focus.min(panel.fields().len() - 1)];
            let actions: &[(&str, &str)] = match panel.favourite {
                None => &[
                    ("Enter", "connect"),
                    ("Ctrl+S", "save as favourite"),
                    ("Esc", "back"),
                ],
                Some(_) => &[
                    ("Enter", "connect"),
                    ("Ctrl+S", "save changes"),
                    ("Esc", "back"),
                ],
            };
            (Some(field.description().to_string()), actions)
        }
        None => (None, draw_list(app, inner.width, &mut lines)),
    };

    if let Some(description) = description {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("   {}", "─".repeat(inner.width.saturating_sub(6) as usize)),
            DIM,
        )));
        // Always two rows, so the box doesn't change height as the selection moves.
        let mut wrapped = wrap_words(&description, inner.width.saturating_sub(6) as usize);
        wrapped.resize(2, String::new());
        for r in wrapped.into_iter().take(2) {
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
        let mut spans = vec![Span::raw("  ")];
        for (key, what) in actions {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!(" {key} "),
                Style::new().fg(Color::Black).bg(Color::Green),
            ));
            spans.push(Span::raw(format!(" {what}  ")));
        }
        lines.push(Line::from(spans));
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

/// The list: "New connection", then the favourites. Returns the
/// highlighted row's keys.
fn draw_list(
    app: &App,
    width: u16,
    lines: &mut Vec<Line<'static>>,
) -> &'static [(&'static str, &'static str)] {
    let rows = Setup::list_rows(&app.saved);
    let focus = app.setup.focus.min(rows.len() - 1);
    lines.push(favourites::new_connection_row(focus == 0));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("   Favourites", DIM)));
    if app.saved.favourites.is_empty() {
        lines.push(Line::from(Span::styled(
            "     None yet. Open New connection, fill it in and press Ctrl+S.",
            DIM,
        )));
    }
    for i in 0..app.saved.favourites.len() {
        lines.push(favourites::row(&app.saved, i, focus == i + 1, width));
    }
    match rows[focus] {
        ListRow::New => &[("Enter", "new connection")],
        ListRow::Favourite(_) => &[("Enter", "connect"), ("E", "edit"), ("Del", "remove")],
    }
}

/// Cuts `s` to `max` characters, ending in "…" when it's cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::setup::PortInfo;
    use ratatui::{Terminal, backend::TestBackend};

    fn screen(width: u16, height: u16, ports: usize) -> String {
        let dir =
            std::env::temp_dir().join(format!("rawtap_ui_ports_{}_{height}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = crate::paths::Paths::at(dir.clone());
        let (cfg, notes) = crate::config::load(&paths).unwrap();
        let mut saved = crate::config::Saved::default();
        saved.connection.mode = crate::config::ConnMode::Serial;
        let mut app = App::new(cfg, paths, saved.clone(), notes, true);
        // The ports are listed in a serial connection's panel.
        app.setup.panel = Some(crate::app::setup::Panel::new(
            None,
            String::new(),
            saved.connection,
        ));
        app.setup.ports = (1..=ports)
            .map(|n| PortInfo {
                name: format!("COM{n}"),
                desc: format!("USB Serial Device (COM{n})"),
            })
            .collect();
        let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
        t.draw(|f| draw(f, &mut app)).unwrap();
        let buf = t.backend().buffer();
        let text = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        let _ = std::fs::remove_dir_all(&dir);
        text
    }

    /// The default console window (120×30) with many ports: the list gives
    /// way, and the keys under it still show.
    #[test]
    fn many_ports_in_a_short_window_keep_the_keys_visible() {
        let text = screen(120, 30, 20);
        println!("{text}");
        assert!(
            text.contains("Enter"),
            "keys cut off:
{text}"
        );
        assert!(
            text.contains("save as favourite"),
            "keys cut off:
{text}"
        );
        assert!(
            text.contains("more. ←→ on Serial port"),
            "no summary row:
{text}"
        );
        assert!(text.contains("COM1 "));
        assert!(!text.contains("COM20"));
    }

    #[test]
    fn every_port_is_listed_when_there_is_room() {
        let text = screen(120, 60, 20);
        assert!(text.contains("COM20"));
        assert!(!text.contains("more. ←→"));
    }
}
