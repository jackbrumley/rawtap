//! Drawing the F1 menu: the top level (the sections, then Keyboard
//! shortcuts and Quit), an open
//! section's items, or search results from every section.

use super::*;
use crate::menu::{Item, Row};
use crate::ui::POPUP_DIM;

/// Width of a section's name on the top level.
const SECTION_W: usize = 18;

pub(super) fn draw_menu(f: &mut Frame, m: &Monitor) {
    let Some(menu) = &m.menu else { return };
    let rows = menu::visible(m, menu);
    let sel = menu.selected.min(rows.len().saturating_sub(1));
    let searching = !menu.filter.trim().is_empty();
    let top_level = !searching && menu.section.is_none();

    let full = f.area();
    let w = 84.min(full.width.saturating_sub(2)).max(30).min(full.width);
    let desc_rows = 3u16;
    // The top level has a gap above its pinned items.
    let gap = u16::from(top_level);
    let want_h = rows.len() as u16 + gap + 4 + desc_rows + 2;
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
    let mut title = vec![Span::styled(
        " RawTap menu ",
        Style::new()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some(open) = menu.section.filter(|_| !searching) {
        title.push(Span::styled(
            format!(" › {open} "),
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    }
    title.push(Span::styled(
        format!(" {} ", m.profile_key),
        Style::new().fg(Color::White),
    ));
    let keys = if searching {
        " ↑↓ move  ←→ change  Enter select  Esc clear search "
    } else if menu.section.is_some() {
        " ↑↓ move  ←→ change  Enter select  type to search  Esc back "
    } else {
        " ↑↓ move  Enter open  type to search  Ctrl+Q quit  Esc close "
    };
    let block = Block::bordered()
        .title(Line::from(title))
        .title_bottom(Line::from(Span::styled(keys, Style::new().fg(Color::Cyan))))
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

    let search_line = if menu.filter.is_empty() {
        Line::from(vec![
            Span::styled(" › ", Style::new().fg(Color::Cyan)),
            // A placeholder, like an empty search box.
            Span::styled("Search", DIM),
        ])
    } else {
        Line::from(vec![
            Span::styled(" › ", Style::new().fg(Color::Cyan)),
            Span::raw(menu.filter.clone()),
        ])
    };
    f.render_widget(Paragraph::new(search_line), search);
    f.set_cursor_position((search.x + 3 + menu.filter.chars().count() as u16, search.y));

    // Every line of the list, and the one the selection is on.
    let lw = list.width as usize;
    let mut lines = Vec::new();
    let mut sel_line = 0;
    for (i, row) in rows.iter().enumerate() {
        let selected = i == sel;
        if selected {
            sel_line = lines.len();
        }
        match row {
            Row::Section { name, hint } => lines.push(section_line(name, hint, selected, lw)),
            Row::Item(item) => {
                // A gap between the sections and the pinned items.
                if top_level && matches!(rows.get(i.wrapping_sub(1)), Some(Row::Section { .. })) {
                    lines.push(Line::from(""));
                }
                lines.push(item_line(item, searching, selected, lw));
            }
        }
    }
    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Nothing matches. Backspace or Esc to clear the search.",
            POPUP_DIM,
        )));
    }
    // Scrolled so the selection is visible.
    let offset = (sel_line + 1).saturating_sub(list.height as usize);
    let lines: Vec<Line> = lines.into_iter().skip(offset).collect();
    f.render_widget(Paragraph::new(lines), list);

    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(sep.width as usize), POPUP_DIM)),
        sep,
    );
    let description = match rows.get(sel) {
        Some(Row::Section { hint, .. }) => format!("{hint}.  (Enter opens it)"),
        Some(Row::Item(item)) => {
            let hint = match item.kind {
                Kind::Setting(_) => "  (←→ to change)",
                Kind::Cmd(_) => "  (Enter)",
            };
            format!("{}{hint}", item.desc)
        }
        None => String::new(),
    };
    let text: Vec<Line> =
        crate::ui::wrap_words(&description, desc.width.saturating_sub(2) as usize)
            .into_iter()
            .map(|r| Line::from(Span::styled(format!(" {r}"), Style::new().fg(Color::Gray))))
            .collect();
    f.render_widget(Paragraph::new(text), desc);
}

/// A section on the top level: its name, then what's in it.
fn section_line(name: &str, hint: &str, selected: bool, width: usize) -> Line<'static> {
    let hint = truncate(hint, width.saturating_sub(SECTION_W + 6));
    if selected {
        let text = format!(" {name:<SECTION_W$} ►  {hint}");
        return Line::from(Span::styled(
            format!("{text:<width$}"),
            Style::new().fg(Color::Black).bg(Color::Cyan),
        ));
    }
    Line::from(vec![
        Span::styled(
            format!(" {name:<SECTION_W$} "),
            Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled("►  ", Style::new().fg(Color::Cyan)),
        Span::styled(hint, POPUP_DIM),
    ])
}

/// An item: its name (with its section when searching), its value and
/// its key.
fn item_line(item: &Item, searching: bool, selected: bool, width: usize) -> Line<'static> {
    let key_w = 20usize;
    let val_w = 16usize;
    let label_w = width.saturating_sub(key_w + val_w + 3);
    let value = match (&item.value, selected, item.kind) {
        (Some(v), true, Kind::Setting(_)) => format!("◄ {v} ►"),
        (Some(v), _, _) => v.clone(),
        _ => String::new(),
    };
    let label = if searching {
        format!("{} › {}", item.section, item.label)
    } else {
        item.label.clone()
    };
    let label = truncate(&label, label_w);
    if selected {
        return Line::from(Span::styled(
            format!(" {label:<label_w$} {value:<val_w$} {:>key_w$} ", item.key),
            Style::new().fg(Color::Black).bg(Color::Cyan),
        ));
    }
    let style = if item.value.is_some() {
        Style::new().fg(Color::White)
    } else {
        Style::new()
    };
    Line::from(vec![
        Span::styled(format!(" {label:<label_w$} "), style),
        Span::styled(format!("{value:<val_w$} "), Style::new().fg(Color::Yellow)),
        Span::styled(format!("{:>key_w$} ", item.key), POPUP_DIM),
    ])
}
