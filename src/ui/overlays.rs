//! Popups drawn over the current screen: forms, the delete popup, the run picker and the answer popup.

use super::app::{Answer, App, Confirm, Form, FormKind, Picker};
use super::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

/// A bordered box centered in `area`, sized to its lines.
pub fn popup(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, pal: &Palette) {
    popup_wide(f, area, 84, title, lines, pal)
}

fn popup_wide(
    f: &mut Frame,
    area: Rect,
    width: u16,
    title: &str,
    lines: Vec<Line<'static>>,
    pal: &Palette,
) {
    let w = width.min(area.width);
    let text = Paragraph::new(lines).wrap(Wrap { trim: false });
    // Sized to the wrapped rows, not the lines: a long value wraps onto several.
    let rows = text.line_count(w.saturating_sub(2)) as u16;
    let h = rows.saturating_add(2).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    // Taller than the screen: the end shows, where the cursor and the hints are.
    let scroll = rows.saturating_sub(h.saturating_sub(2));
    f.render_widget(Clear, rect);
    f.render_widget(
        text.scroll((scroll, 0)).block(
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
        .flat_map(|(i, (label, value))| {
            let focused = i == form.focus;
            let style = if focused {
                Style::new().fg(pal.accent).bold()
            } else {
                Style::new().fg(pal.dim)
            };
            field_lines(label, value, focused.then_some(form.cursor), style)
        })
        .collect();
    // A prompt can be long: nearly the screen's width and at least 8 rows to write in.
    let prompt = matches!(form.kind, FormKind::Prompt { .. });
    let width = if prompt {
        area.width.saturating_sub(4)
    } else {
        84
    };
    if prompt {
        let rows = (Paragraph::new(lines.clone()).wrap(Wrap { trim: false }))
            .line_count(width.saturating_sub(2));
        lines.extend(vec![Line::raw(""); 8usize.saturating_sub(rows)]);
    }
    lines.push(Line::raw(""));
    if let Some(e) = &form.error {
        lines.push(Line::from(Span::styled(
            e.clone(),
            Style::new().fg(pal.alert),
        )));
    }
    let hint = match form.kind {
        FormKind::Prompt { .. } => "Enter send · Shift+Enter new line · Esc cancel",
        _ => "Tab next field · Enter save · Esc cancel",
    };
    lines.push(Line::from(Span::styled(hint, Style::new().fg(pal.dim))));
    popup_wide(f, area, width, &form.title, lines, pal);
}

/// A labelled field, a row per line of its value. With a `cursor` (a char index, past the end means
/// at the end) the char under it is drawn reversed, a reversed space at a line's end.
fn field_lines(
    label: &str,
    value: &str,
    cursor: Option<usize>,
    style: Style,
) -> Vec<Line<'static>> {
    let chars: Vec<char> = value.chars().collect();
    let cursor = cursor.map(|c| c.min(chars.len()));
    let mut lines = Vec::new();
    let mut spans = vec![Span::styled(format!("{label:<12} "), style)];
    let mut run = String::new();
    let flush = |run: &mut String, spans: &mut Vec<Span<'static>>| {
        if !run.is_empty() {
            spans.push(Span::raw(std::mem::take(run)));
        }
    };
    for i in 0..=chars.len() {
        let c = chars.get(i).copied();
        if cursor == Some(i) {
            flush(&mut run, &mut spans);
            let under = match c {
                Some('\n') | None => ' ',
                Some(c) => c,
            };
            spans.push(Span::styled(under.to_string(), Style::new().reversed()));
            // A char under the cursor is drawn; a line break or the end still has to happen.
            if !matches!(c, Some('\n') | None) {
                continue;
            }
        }
        match c {
            Some('\n') => {
                flush(&mut run, &mut spans);
                lines.push(Line::from(std::mem::take(&mut spans)));
            }
            Some(c) => run.push(c),
            None => {}
        }
    }
    flush(&mut run, &mut spans);
    lines.push(Line::from(spans));
    lines
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

/// The blocked agent's screen, bottom lines first to go when it does not fit, with its live state.
pub fn answer(f: &mut Frame, area: Rect, app: &App, a: &Answer) {
    let pal = &app.palette;
    let screen: Vec<&str> = a.screen.trim_end().lines().collect();
    let room = (area.height as usize).saturating_sub(5);
    let mut lines: Vec<Line<'static>> = screen[screen.len().saturating_sub(room)..]
        .iter()
        .map(|l| Line::raw(l.to_string()))
        .collect();
    if screen.is_empty() {
        lines.push(Line::styled("reading the agent…", Style::new().fg(pal.dim)));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "keys go to the agent: 1-9 ↑↓ Enter Tab, typing · Esc close",
        Style::new().fg(pal.accent),
    ));
    let state = (app.model.projects.iter().flat_map(|p| &p.agents))
        .find(|r| r.pane.pane_id == a.pane_id)
        .map_or("gone".into(), |r| {
            format!("{:?}", r.pane.agent_status).to_lowercase()
        });
    let width = area.width.saturating_sub(4);
    popup_wide(
        f,
        area,
        width,
        &format!(" answer · {} · {state} ", a.agent),
        lines,
        pal,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{sample_app, FormKind};
    use crate::ui::render;

    fn with_prompt(text: String) -> App {
        let mut app = sample_app();
        app.form = Some(Form {
            title: " prompt · alpha ".into(),
            kind: FormKind::Prompt {
                pane_id: "w1:p1".into(),
                agent: "alpha".into(),
            },
            fields: vec![("prompt", text)],
            focus: 0,
            cursor: usize::MAX,
            error: None,
        });
        app
    }

    #[test]
    fn long_prompt_keeps_its_end_and_hint_in_view() {
        let words: String = (0..60).map(|i| format!("word{i:02} ")).collect();
        let app = with_prompt(words + "END");
        let out = render(120, 30, |f| crate::ui::draw(f, &app));
        assert!(out.contains("END"));
        assert!(out.contains("Shift+Enter new line · Esc cancel"));
        // Nearly the whole width, not the 84 columns of other popups.
        let top = out.lines().find(|l| l.contains("prompt · alpha")).unwrap();
        assert_eq!(top.find('┌'), Some(2));
        // Longer than the screen: the end, where the cursor is, stays in view.
        let app = with_prompt("x".repeat(3000) + "END");
        let out = render(120, 20, |f| crate::ui::draw(f, &app));
        assert!(out.contains("END") && out.contains("Shift+Enter new line · Esc cancel"));
    }

    #[test]
    fn prompt_lines_show_on_rows_of_their_own() {
        let app = with_prompt("first line\nsecond line\n".into());
        let out = render(120, 30, |f| crate::ui::draw(f, &app));
        let row = |s: &str| out.lines().position(|l| l.contains(s)).unwrap();
        assert_eq!(row("second line"), row("first line") + 1);
        assert!(out.contains("Enter send · Shift+Enter new line · Esc cancel"));
        // A cursor on a line break is a reversed space at the end of its line.
        let lines = field_lines("prompt", "first line\nsecond line", Some(10), Style::new());
        assert_eq!(lines.len(), 2);
        let last = lines[0].spans.last().unwrap();
        assert_eq!(
            (last.content.as_ref(), last.style),
            (" ", Style::new().reversed())
        );
    }

    #[test]
    fn short_prompt_gets_a_roomy_box() {
        let app = with_prompt("hi".into());
        let out = render(120, 30, |f| crate::ui::draw(f, &app));
        let rows = out.lines().filter(|l| l.find('│') == Some(2)).count();
        assert!(rows >= 10, "{out}");
    }
}
