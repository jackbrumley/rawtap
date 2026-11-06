//! Rendering for the monitor screen: header, raw log pane, terminal pane, send box, footer.

mod controls;
pub(crate) mod panes;

use controls::{draw_footer, draw_help, draw_input, draw_menu};
use panes::{draw_log, draw_term};

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
        Style::new()
            .fg(Color::DarkGray)
            .bg(Color::Reset)
            .remove_modifier(Modifier::all()),
    );
}

/// Paint the terminal selection: `abs_top` is the absolute row shown at the top of the pane.
fn highlight_term(f: &mut Frame, m: &Monitor, inner: Rect, abs_top: usize) {
    let Some(s) = m
        .selection
        .filter(|s| s.pane == Pane::Terminal && !s.is_empty())
    else {
        return;
    };
    let ((r1, c1), (r2, c2)) = s.ordered();
    let style = Style::new().fg(Color::Black).bg(Color::LightBlue);
    let buf = f.buffer_mut();
    for vr in 0..inner.height {
        let r = abs_top + vr as usize;
        if r < r1 || r > r2 {
            continue;
        }
        let from = if r == r1 { c1 } else { 0 };
        let to = if r == r2 {
            c2
        } else {
            inner.width.saturating_sub(1)
        };
        for c in from..=to.min(inner.width.saturating_sub(1)) {
            buf[(inner.x + c, inner.y + vr)].set_style(style);
        }
    }
}

/// Paint whole raw-log rows whose entry is inside the selection.
fn highlight_log(f: &mut Frame, m: &Monitor, inner: Rect) {
    let Some(s) = m.selection.filter(|s| s.pane == Pane::Log && !s.is_empty()) else {
        return;
    };
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
    let Some(lines) = &m.paste_confirm else {
        return;
    };
    let full = f.area();
    let shown = lines.len().min(10);
    let h = (shown as u16 + 9).min(full.height);
    let w = 76.min(full.width);
    let area = Rect {
        x: full.x + (full.width - w) / 2,
        y: full.y + (full.height - h) / 3,
        width: w,
        height: h,
    };
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
        text.push(Line::from(Span::styled(
            format!("  │ {t}"),
            Style::new().fg(Color::White),
        )));
    }
    if lines.len() > shown {
        text.push(Line::from(Span::styled(
            format!("  │ … and {} more", lines.len() - shown),
            DIM,
        )));
    }
    let block = Block::bordered()
        .title(Line::from(Span::styled(
            " Confirm paste ",
            Style::new()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )))
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
            let want = if m.profile.term_cols == 0 {
                auto
            } else {
                m.profile.term_cols
            };
            let tc = want.clamp(40, body.width.saturating_sub(30));
            m.cur_term_cols = tc;
            let [l, t] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(tc)]).areas(body);
            (Some(l), Some(t))
        }
        PaneLayout::Split => {
            m.side_by_side = false;
            let auto = (body.height / 4).clamp(5, 10);
            let want = if m.profile.log_rows == 0 {
                auto
            } else {
                m.profile.log_rows
            };
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
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn draw_header(f: &mut Frame, m: &Monitor, area: Rect) {
    let (state, state_style) = match (&m.settings, m.connected, &m.peer) {
        (_, false, _) => (
            "○ DISCONNECTED".to_string(),
            Style::new().fg(Color::White).bg(Color::Red),
        ),
        (LinkSettings::TcpListen { .. }, true, None) => (
            "◌ WAITING FOR CLIENT".to_string(),
            Style::new().fg(Color::Black).bg(Color::Yellow),
        ),
        (LinkSettings::TcpListen { .. }, true, Some(p)) => (
            format!("● CLIENT {p}"),
            Style::new().fg(Color::Black).bg(Color::Green),
        ),
        _ => (
            "● CONNECTED".to_string(),
            Style::new().fg(Color::Black).bg(Color::Green),
        ),
    };
    let mut spans = vec![
        Span::styled(
            " RawTap ",
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {} ", m.settings.describe())),
        Span::styled(format!(" {state} "), state_style),
        Span::raw(format!(
            "  RX {} B  TX {} B",
            thousands(m.rx_bytes),
            thousands(m.tx_bytes)
        )),
    ];
    if m.live {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            " LIVE KEYS ",
            Style::new()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(l) = &m.logger
        && let Some(name) = l.path.file_name()
    {
        spans.push(Span::styled(
            format!("  log {}", name.to_string_lossy()),
            DIM,
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
