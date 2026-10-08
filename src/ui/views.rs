//! The four list tabs, shared by the project drill-down and the global views.

use super::app::{App, Screen, Tab};
use super::theme::{self, Palette};
use crate::model::{self, GroupBy, UsageRow};
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, Borders, Cell, Paragraph, Row, Sparkline, Table, TableState, Tabs, Wrap,
};

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let pal = &app.palette;
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(header_line(app, pal)), header);
    let tab = app.tab();
    let selected = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
    f.render_widget(
        Tabs::new(Tab::ALL.iter().map(|t| t.title()))
            .select(selected)
            .style(Style::new().fg(pal.dim))
            .highlight_style(Style::new().fg(pal.accent).bold()),
        tabs,
    );
    match tab {
        Tab::Agents => agents(f, body, app, pal),
        Tab::Threads => threads(f, body, app, pal),
        Tab::Timeline => timeline(f, body, app, pal),
        Tab::Usage => usage(f, body, app, pal),
    }
    f.render_widget(Paragraph::new(footer_line(app, pal)), footer);
}

fn header_line(app: &App, pal: &Palette) -> Line<'static> {
    let mut spans = vec![Span::styled(
        " JARVIS › ",
        Style::new().fg(pal.accent).bold(),
    )];
    match &app.screen {
        Screen::Project { key, .. } => match app.model.projects.iter().find(|p| &p.key == key) {
            Some(p) => {
                spans.push(Span::styled(p.name.clone(), Style::new().bold()));
                let branch = p
                    .branch
                    .as_ref()
                    .map(|b| format!(" · {b}"))
                    .unwrap_or_default();
                spans.push(Span::styled(
                    format!("  ({}{branch})", p.root),
                    Style::new().fg(pal.dim),
                ));
            }
            None => spans.push(Span::raw(key.clone())),
        },
        _ => {
            spans.push(Span::styled("all projects", Style::new().bold()));
            if let Some(k) = &app.project_filter {
                spans.push(Span::styled(
                    format!("  filter: {}", app.project_name(k)),
                    Style::new().fg(pal.dim),
                ));
            }
        }
    }
    spans.push(Span::styled(
        "    Esc = back to core",
        Style::new().fg(pal.dim),
    ));
    Line::from(spans)
}

fn footer_line(app: &App, pal: &Palette) -> Line<'static> {
    let mut hint = String::from(" ↑↓ move · Enter open · Tab views · / search · s state");
    if app.tab() == Tab::Timeline {
        hint.push_str(" · w range");
    }
    if matches!(app.screen, Screen::Global(_)) {
        hint.push_str(" · f project");
    }
    hint.push_str(" · ? help");
    let mut spans = vec![Span::styled(hint, Style::new().fg(pal.dim))];
    if app.search_editing || !app.search.is_empty() {
        spans.push(Span::styled(
            format!("   /{}", app.search),
            Style::new().fg(pal.accent),
        ));
    }
    if let Some(s) = app.state_filter {
        spans.push(Span::styled(
            format!("   state: {}", theme::word(s)),
            Style::new().fg(pal.warn),
        ));
    }
    if app.tab() == Tab::Timeline {
        spans.push(Span::styled(
            format!("   range: {}", app.range.label()),
            Style::new().fg(pal.warn),
        ));
    }
    Line::from(spans)
}

fn empty(f: &mut Frame, area: Rect, msg: &str, pal: &Palette) {
    f.render_widget(
        Paragraph::new(msg.to_string())
            .style(Style::new().fg(pal.dim))
            .alignment(Alignment::Center),
        area,
    );
}

fn table_state(selected: usize) -> TableState {
    TableState::default().with_selected(Some(selected))
}

fn agents(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let rows = app.visible_agents();
    if rows.is_empty() {
        return empty(f, area, "no agents here", pal);
    }
    let [list, side] =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area);
    let global = matches!(app.screen, Screen::Global(_));
    let body: Vec<Row> = rows
        .iter()
        .map(|(p, a)| {
            let s = a.pane.agent_status;
            let title = match &a.worktree {
                Some(_) => format!("⎇ {}", a.pane.title()),
                None => a.pane.title().to_string(),
            };
            let mut cells = vec![
                Cell::from(Span::styled(
                    format!("{} {}", theme::glyph(s), theme::word(s)),
                    Style::new().fg(theme::status_color(s, pal)),
                )),
                Cell::from(a.pane.pane_id.clone()),
                Cell::from(a.pane.agent.clone().unwrap_or_default()),
                Cell::from(theme::truncate(&title, 48)),
                Cell::from(theme::ago(a.since, app.now)),
            ];
            if global {
                cells.insert(1, Cell::from(p.name.clone()));
            }
            Row::new(cells)
        })
        .collect();
    let mut widths = vec![
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Min(10),
        Constraint::Length(5),
    ];
    let mut header = vec!["state", "pane", "agent", "title", "since"];
    if global {
        widths.insert(1, Constraint::Length(18));
        header.insert(1, "project");
    }
    let table = Table::new(body, widths)
        .header(Row::new(header).style(Style::new().fg(pal.dim)))
        .row_highlight_style(Style::new().bg(pal.highlight));
    f.render_stateful_widget(table, list, &mut table_state(app.selected_row));

    let Some((_, a)) = rows.get(app.selected_row) else {
        return;
    };
    let thread = a.thread.and_then(|i| app.model.threads.get(i));
    let mut lines = vec![
        Line::from(format!("workspace  {}", a.workspace)),
        Line::from(format!(
            "worktree   {}",
            a.worktree.as_deref().unwrap_or("—")
        )),
        Line::from(format!(
            "thread     {}",
            thread
                .and_then(|t| t.thread.title.as_deref())
                .unwrap_or("—")
        )),
        Line::from(format!(
            "branch     {}",
            thread
                .and_then(|t| t.thread.branch.as_deref())
                .unwrap_or("—")
        )),
    ];
    if let Some(t) = thread {
        let today = app.today();
        let day = model::usage(
            std::slice::from_ref(t),
            &app.pricing,
            None,
            Some(std::slice::from_ref(&today)),
            GroupBy::Day,
        );
        let (u, c) = day.first().map(|r| (r.usage, r.cost)).unwrap_or_default();
        lines.push(Line::from(format!(
            "today      {} tok · {}",
            theme::tokens(u.total()),
            theme::cost(&c)
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled("recent", Style::new().fg(pal.dim)));
    for e in app
        .model
        .events
        .iter()
        .filter(|e| e.record.pane_id == a.pane.pane_id)
        .take(5)
    {
        lines.push(Line::from(format!(
            "{}  {}",
            theme::stamp(e.record.ts, app.now),
            theme::transition(&e.record)
        )));
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::new()
                .borders(Borders::LEFT)
                .border_style(Style::new().fg(pal.dim)),
        ),
        side,
    );
}

fn threads(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let rows = app.visible_threads();
    if rows.is_empty() {
        let msg = if app.search.is_empty() {
            "no Claude Code threads"
        } else {
            "no threads match the search"
        };
        return empty(f, area, msg, pal);
    }
    let [list, preview] = Layout::vertical([Constraint::Min(0), Constraint::Length(6)]).areas(area);
    let body: Vec<Row> = rows
        .iter()
        .map(|t| {
            Row::new(vec![
                Cell::from(Span::styled(
                    t.live_pane
                        .as_ref()
                        .map(|p| format!("LIVE {p}"))
                        .unwrap_or_default(),
                    Style::new().fg(pal.ok).bold(),
                )),
                Cell::from(theme::truncate(
                    t.thread.title.as_deref().unwrap_or("(untitled)"),
                    44,
                )),
                Cell::from(t.project_name.clone()),
                Cell::from(t.thread.branch.clone().unwrap_or_default()),
                Cell::from(theme::ago(t.thread.last_ts, app.now)),
                Cell::from(t.thread.turns.to_string()),
                Cell::from(theme::tokens(t.usage.total())),
                Cell::from(theme::cost(&t.cost)),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(11),
        Constraint::Min(16),
        Constraint::Length(18),
        Constraint::Length(14),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(7),
        Constraint::Length(10),
    ];
    let table = Table::new(body, widths)
        .header(
            Row::new(vec![
                "", "title", "project", "branch", "last", "turns", "tokens", "cost",
            ])
            .style(Style::new().fg(pal.dim)),
        )
        .row_highlight_style(Style::new().bg(pal.highlight));
    f.render_stateful_widget(table, list, &mut table_state(app.selected_row));
    if let Some(t) = rows.get(app.selected_row) {
        let text = vec![
            Line::from(vec![
                Span::styled("› ", Style::new().fg(pal.accent)),
                Span::raw(t.thread.last_prompt.clone().unwrap_or_default()),
            ]),
            Line::from(vec![
                Span::styled("‹ ", Style::new().fg(pal.ok)),
                Span::raw(t.thread.last_reply.clone().unwrap_or_default()),
            ]),
        ];
        f.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: true }).block(
                Block::new()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(pal.dim)),
            ),
            preview,
        );
    }
}

fn timeline(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let mut area = area;
    if !app.collector_running {
        let [warn, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        f.render_widget(
            Paragraph::new(Span::styled(
                "collector not running: changes are not being recorded",
                Style::new().fg(pal.warn),
            )),
            warn,
        );
        area = rest;
    }
    let rows = app.visible_events();
    if rows.is_empty() {
        return empty(
            f,
            area,
            &format!("no events in range ({})", app.range.label()),
            pal,
        );
    }
    let body: Vec<Row> = rows
        .iter()
        .map(|e| {
            let color = e
                .record
                .to
                .map(|s| theme::status_color(s, pal))
                .unwrap_or(pal.dim);
            Row::new(vec![
                Cell::from(theme::stamp(e.record.ts, app.now)),
                Cell::from(e.project_name.clone()),
                Cell::from(format!(
                    "{} {}",
                    e.record.agent.as_deref().unwrap_or("agent"),
                    e.record.pane_id
                )),
                Cell::from(Span::styled(
                    theme::transition(&e.record),
                    Style::new().fg(color),
                )),
                Cell::from(e.duration.map(theme::duration).unwrap_or_default()),
            ])
        })
        .collect();
    let widths = [
        Constraint::Length(11),
        Constraint::Length(18),
        Constraint::Length(16),
        Constraint::Min(20),
        Constraint::Length(9),
    ];
    let table = Table::new(body, widths)
        .header(
            Row::new(vec!["time", "project", "pane", "change", "took"])
                .style(Style::new().fg(pal.dim)),
        )
        .row_highlight_style(Style::new().bg(pal.highlight));
    f.render_stateful_widget(table, area, &mut table_state(app.selected_row));
}

fn usage(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let days = model::last_days(app.now, 14);
    let scope = app.scope();
    let daily = model::usage(
        &app.model.threads,
        &app.pricing,
        scope,
        Some(&days),
        GroupBy::Day,
    );
    if daily.is_empty() {
        return empty(f, area, "no Claude Code usage in the last 14 days", pal);
    }
    let by_cost = |mut rows: Vec<UsageRow>| {
        rows.sort_by(|a, b| b.cost.usd.total_cmp(&a.cost.usd));
        rows
    };
    let by_project = by_cost(model::usage(
        &app.model.threads,
        &app.pricing,
        scope,
        Some(&days),
        GroupBy::Project,
    ));
    let by_model = by_cost(model::usage(
        &app.model.threads,
        &app.pricing,
        scope,
        Some(&days),
        GroupBy::Model,
    ));
    let [spark, tables] = Layout::vertical([Constraint::Length(4), Constraint::Min(0)]).areas(area);
    let values: Vec<u64> = days
        .iter()
        .map(|d| {
            daily
                .iter()
                .find(|r| &r.label == d)
                .map(|r| (r.cost.usd * 100.0).round() as u64)
                .unwrap_or(0)
        })
        .collect();
    f.render_widget(
        Sparkline::default()
            .block(Block::new().title(format!(
                " cost per day, {} → {} ",
                days[0],
                days[days.len() - 1]
            )))
            .data(values)
            .style(Style::new().fg(pal.accent)),
        spark,
    );
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(tables);
    let [top, bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(right);
    let newest_first: Vec<UsageRow> = daily.into_iter().rev().collect();
    usage_table(f, left, "day", &newest_first, pal);
    usage_table(f, top, "project", &by_project, pal);
    usage_table(f, bottom, "model", &by_model, pal);
}

fn usage_table(f: &mut Frame, area: Rect, label: &'static str, rows: &[UsageRow], pal: &Palette) {
    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(theme::truncate(&r.label, 22)),
                Cell::from(theme::tokens(r.usage.input)),
                Cell::from(theme::tokens(r.usage.output)),
                Cell::from(theme::tokens(r.usage.cache_read)),
                Cell::from(theme::tokens(
                    r.usage.cache_write_5m + r.usage.cache_write_1h,
                )),
                Cell::from(theme::cost(&r.cost)),
            ])
        })
        .collect();
    let widths = [
        Constraint::Min(10),
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    f.render_widget(
        Table::new(body, widths).header(
            Row::new(vec![label, "input", "output", "cache r", "cache w", "cost"])
                .style(Style::new().fg(pal.dim)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::sample_app;
    use crate::ui::render;

    fn screen(app: &App, w: u16, h: u16) -> String {
        render(w, h, |f| {
            let area = f.area();
            draw(f, area, app)
        })
    }

    #[test]
    fn project_agents_show_state_and_side_panel() {
        let mut app = sample_app();
        app.screen = Screen::Project {
            key: app.model.projects[1].key.clone(),
            tab: Tab::Agents,
        };
        let out = screen(&app, 120, 30);
        assert!(out.contains("JARVIS › alpha"));
        assert!(out.contains("● working"));
        assert!(out.contains("w1:p1"));
        assert!(out.contains("workspace  alpha-ws"));
        assert!(out.contains("Parser fix"));
    }

    #[test]
    fn global_threads_show_live_badge_and_cost() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Threads);
        let out = screen(&app, 140, 30);
        assert!(out.contains("LIVE w1:p1"));
        assert!(out.contains("Old beta work"));
        assert!(out.contains("≈$4.00"));
        assert!(out.contains("› add tests"));
    }

    #[test]
    fn timeline_shows_transitions_and_collector_warning() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Timeline);
        app.collector_running = false;
        let out = screen(&app, 120, 30);
        assert!(out.contains("working → blocked"));
        assert!(out.contains("collector not running"));
    }

    #[test]
    fn usage_shows_models_and_days() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Usage);
        let out = screen(&app, 140, 40);
        assert!(out.contains("claude-opus-5-5"));
        assert!(out.contains("2026-10-08"));
        assert!(out.contains("≈$4.00"));
    }

    #[test]
    fn empty_model_shows_empty_states() {
        let mut app = sample_app();
        app.model = Default::default();
        for tab in Tab::ALL {
            app.screen = Screen::Global(tab);
            let out = screen(&app, 100, 20);
            assert!(
                out.contains("no "),
                "tab {tab:?} should show an empty state"
            );
        }
    }

    #[test]
    fn views_survive_tiny_area() {
        let mut app = sample_app();
        for tab in Tab::ALL {
            app.screen = Screen::Global(tab);
            screen(&app, 8, 3);
            screen(&app, 1, 1);
        }
    }
}
