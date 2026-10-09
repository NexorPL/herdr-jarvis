//! Popups drawn over the current screen: forms, the delete popup and the run picker.

use super::app::{App, Confirm, Form, Picker};
use super::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

/// A bordered box centered in `area`, sized to its lines.
pub fn popup(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, pal: &Palette) {
    let w = 84.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .title(title.to_string())
                .border_style(Style::new().fg(pal.accent)),
        ),
        rect,
    );
}

pub fn form(f: &mut Frame, area: Rect, app: &App, form: &Form) {
    let pal = &app.palette;
    let mut lines: Vec<Line<'static>> = (form.fields.iter().enumerate())
        .map(|(i, (label, value))| {
            let focused = i == form.focus;
            let style = if focused {
                Style::new().fg(pal.accent).bold()
            } else {
                Style::new().fg(pal.dim)
            };
            Line::from(vec![
                Span::styled(format!("{label:<12} "), style),
                Span::raw(value.clone()),
                Span::styled(if focused { "▏" } else { "" }, Style::new().fg(pal.accent)),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::new().fg(pal.alert),
        )));
    }
    lines.push(Line::from(Span::styled(
        "Tab next field · Enter save · Esc cancel",
        Style::new().fg(pal.dim),
    )));
    popup(f, area, &form.title, lines, pal);
}

pub fn picker(f: &mut Frame, area: Rect, app: &App, p: &Picker) {
    let pal = &app.palette;
    let targets = app.picker_targets();
    let mut lines: Vec<Line<'static>> = (targets.iter().enumerate())
        .map(|(i, (_, t))| {
            let mark = if p.chosen[i] { "[x]" } else { "[ ]" };
            let mut spans = vec![
                Span::styled(
                    if i == p.cursor { "› " } else { "  " },
                    Style::new().fg(pal.accent),
                ),
                Span::raw(format!("{mark} {:<16}", t.name)),
                Span::styled(format!(" {}", t.command), Style::new().fg(pal.dim)),
            ];
            if app.running_pane(&p.ctx.project, &t.name).is_some() {
                spans.push(Span::styled("  ● running", Style::new().fg(pal.ok)));
            }
            Line::from(spans)
        })
        .collect();
    if targets.is_empty() {
        lines.push(Line::styled(
            "no targets yet · n to add",
            Style::new().fg(pal.dim),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        if p.choosing_layout {
            "t one tab per target · s side by side in one tab · Esc back"
        } else {
            "Space select · a all · Enter run · n new · e edit · d delete · Esc close"
        },
        Style::new().fg(pal.accent),
    )));
    popup(f, area, &format!(" run · {} ", p.ctx.project), lines, pal);
}

pub fn confirm(f: &mut Frame, area: Rect, app: &App, c: &Confirm) {
    let pal = &app.palette;
    let button = |label: &'static str, on: bool| {
        if on {
            Span::styled(label, Style::new().fg(pal.accent).bold().reversed())
        } else {
            Span::styled(label, Style::new().fg(pal.dim))
        }
    };
    let lines = vec![
        Line::from(format!("Delete \"{}\"?", c.name)).centered(),
        Line::raw(""),
        Line::from(vec![
            button("[ Yes ]", c.yes),
            Span::raw("   "),
            button("[ No ]", !c.yes),
        ])
        .centered(),
    ];
    popup(f, area, " delete ", lines, pal);
}
