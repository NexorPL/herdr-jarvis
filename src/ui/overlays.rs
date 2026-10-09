//! Popups drawn over the current screen: the idea form and the run picker.

use super::app::{App, IdeaForm};
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

pub fn idea_form(f: &mut Frame, area: Rect, app: &App, form: &IdeaForm) {
    let pal = &app.palette;
    let field = |label: &str, value: &str, focused: bool| {
        let style = if focused {
            Style::new().fg(pal.accent).bold()
        } else {
            Style::new().fg(pal.dim)
        };
        Line::from(vec![
            Span::styled(format!("{label:<12} "), style),
            Span::raw(value.to_string()),
            Span::styled(if focused { "▏" } else { "" }, Style::new().fg(pal.accent)),
        ])
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("{:<12} {}", "project", form.project_name),
            Style::new().fg(pal.dim),
        )),
        Line::raw(""),
        field("name", &form.name, !form.on_description),
        field("description", &form.description, form.on_description),
        Line::raw(""),
        Line::from(Span::styled(
            "Tab switch field · Enter save · Esc cancel",
            Style::new().fg(pal.dim),
        )),
    ];
    let title = if form.editing.is_some() {
        " edit idea "
    } else {
        " new idea "
    };
    popup(f, area, title, lines, pal);
}
