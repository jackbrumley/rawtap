//! Drawing favourites: their rows on the start screen, and the dialog that
//! saves, edits or removes one.

use super::{DIM, dim, truncate, wrap_words};
use crate::app::favourites::{self, FavouriteDialog, FavouriteForm};
use crate::config::Saved;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

const NAME_WIDTH: usize = 24;

/// One favourite on the start screen: name, what it connects to, and its
/// view when it's fixed.
pub(super) fn row(saved: &Saved, i: usize, focused: bool, width: u16) -> Line<'static> {
    let f = &saved.favourites[i];
    let marker = if focused { " ▶ " } else { "   " };
    let name_style = if focused {
        Style::new().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::new().fg(Color::White)
    };
    let name = truncate(&f.name, NAME_WIDTH);
    let mut spans = vec![
        Span::styled(marker, Style::new().fg(Color::Cyan)),
        Span::styled(format!(" {name:<NAME_WIDTH$} "), name_style),
        Span::raw("  "),
    ];
    // What it connects to, unless the name already says it.
    let target = favourites::describe(&f.connection);
    if !target.eq_ignore_ascii_case(f.name.trim()) {
        let room = (width as usize).saturating_sub(3 + NAME_WIDTH + 2 + 2);
        spans.push(Span::styled(truncate(&target, room), DIM));
    }
    Line::from(spans)
}

pub(super) fn draw_dialog(f: &mut Frame, dialog: &FavouriteDialog, saved: &Saved, over: Rect) {
    dim(f);
    let full = f.area();
    let w = 64.min(full.width.saturating_sub(2)).max(30.min(full.width));
    let text_width = w.saturating_sub(6) as usize;
    let (title, hint, lines, cursor) = match dialog {
        FavouriteDialog::Edit(form) => {
            let title = if form.index.is_some() {
                " Edit favourite "
            } else {
                " Save as favourite "
            };
            let (lines, cursor) = edit_lines(form, text_width);
            (title, " Enter save  Esc cancel ", lines, cursor)
        }
        FavouriteDialog::Remove(i) => {
            let Some(fav) = saved.favourites.get(*i) else {
                return;
            };
            let mut lines = vec![Line::from("")];
            let question = format!(
                "Remove \"{}\" ({}) from your favourites?",
                fav.name,
                favourites::describe(&fav.connection)
            );
            for r in wrap_words(&question, text_width) {
                lines.push(Line::from(format!("  {r}")));
            }
            lines.push(Line::from(""));
            (
                " Remove favourite ",
                " Enter remove  Esc keep ",
                lines,
                None,
            )
        }
    };

    let h = (lines.len() as u16 + 2).min(full.height);
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
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )))
        .title_bottom(Line::from(Span::styled(hint, DIM)))
        .border_style(Style::new().fg(Color::Cyan));
    f.render_widget(Paragraph::new(lines).block(block), area);
    if let Some((cx, cy)) = cursor {
        let cx = (area.x + 1 + cx).min(area.x + area.width.saturating_sub(2));
        f.set_cursor_position((cx, area.y + 1 + cy));
    }
}

/// The edit dialog's rows, and where the cursor goes (inside the border).
fn edit_lines(form: &FavouriteForm, text_width: usize) -> (Vec<Line<'static>>, Option<(u16, u16)>) {
    let mut lines = vec![Line::from("")];
    let name_row = lines.len() as u16;
    let name_room = text_width.saturating_sub(7);
    // The end of a long name, so what's being typed stays in sight.
    let shown: String = {
        let count = form.name.chars().count();
        form.name
            .chars()
            .skip(count.saturating_sub(name_room))
            .collect()
    };
    lines.push(Line::from(vec![
        Span::styled(
            "  Name ",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {shown:<name_room$} "),
            Style::new().fg(Color::Black).bg(Color::Cyan),
        ),
    ]));
    lines.push(Line::from(""));
    for r in wrap_words(&favourites::describe(&form.connection), text_width) {
        lines.push(Line::from(Span::styled(format!("  {r}"), DIM)));
    }
    if let Some(error) = &form.error {
        lines.push(Line::from(""));
        for r in wrap_words(error, text_width) {
            lines.push(Line::from(Span::styled(
                format!("  {r}"),
                Style::new().fg(Color::Yellow),
            )));
        }
    }
    lines.push(Line::from(""));
    let cursor = Some((7 + 1 + shown.chars().count() as u16, name_row));
    (lines, cursor)
}
