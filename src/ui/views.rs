//! The list tabs, shared by the project drill-down and the global views.

use super::app::{App, Screen, Tab};
use super::theme::{self, Palette};
use crate::herdr::AgentStatus;
use crate::ideas::Status as IdeaStatus;
use crate::model::{self, GroupBy, UsageRow};
use crate::transcripts::SubAgent;
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
        Tab::Ideas => ideas(f, body, app, pal),
        Tab::Live => live(f, body, app, pal),
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

/// Keys as a bar: each key in the accent color, its action dimmed.
pub fn key_bar(keys: &[(&str, &str)], pal: &Palette) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (k, what) in keys {
        spans.push(Span::styled(
            format!(" {k}"),
            Style::new().fg(pal.accent).bold(),
        ));
        spans.push(Span::styled(format!(" {what} "), Style::new().fg(pal.dim)));
    }
    spans
}

/// The panels screen: every agent's card and the keys, nothing else.
pub fn draw_panels(f: &mut Frame, area: Rect, app: &App) {
    let pal = &app.palette;
    let [body, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    live(f, body, app, pal);
    let keys = [
        ("←↑↓→", "select"),
        ("Enter", "type into agent"),
        ("g", "go to agent"),
        ("p", "prompt/answer"),
        ("R", "rename"),
        ("x", "run"),
        ("N", "new agent"),
        ("o", "options"),
        ("Esc v", "core"),
        ("?", "help"),
    ];
    f.render_widget(Paragraph::new(Line::from(key_bar(&keys, pal))), footer);
}

fn footer_line(app: &App, pal: &Palette) -> Line<'static> {
    let tab = app.tab();
    let mut keys = vec![("↑↓", "move"), ("Enter", "open"), ("Tab", "views")];
    if tab == Tab::Live {
        keys[1] = ("Enter", "type into agent");
        keys.push(("g", "go to agent"));
    }
    if tab.lists_agents() {
        keys.extend([("p", "prompt/answer"), ("R", "rename")]);
    }
    if matches!(app.screen, Screen::Project { .. }) || tab.lists_agents() {
        keys.push(("x", "run"));
    }
    if tab == Tab::Ideas {
        keys.extend([
            ("a", "add"),
            ("e", "edit"),
            ("d", "delete"),
            ("Space", "status"),
            ("s", "filter"),
            ("c", "done"),
        ]);
    }
    keys.push(("/", "search"));
    if tab.has_state() {
        keys.push(("s", "state"));
    }
    if tab == Tab::Timeline {
        keys.push(("w", "range"));
    }
    if matches!(app.screen, Screen::Global(_)) {
        keys.push(("f", "project"));
    }
    keys.extend([
        ("N", "new agent"),
        ("o", "options"),
        ("v", "view"),
        ("?", "help"),
    ]);
    let mut spans = key_bar(&keys, pal);
    if app.search_editing || !app.search.is_empty() {
        spans.push(Span::styled(
            format!("   /{}", app.search),
            Style::new().fg(pal.accent),
        ));
    }
    if let Some(s) = app.state_filter.filter(|_| app.tab().has_state()) {
        spans.push(Span::styled(
            format!("   state: {}", theme::word(s)),
            Style::new().fg(pal.warn),
        ));
    }
    if let Some(s) = app.idea_filter.filter(|_| app.tab() == Tab::Ideas) {
        spans.push(Span::styled(
            format!("   status: {}", s.label()),
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

/// One card per agent with the last lines of its screen. The cards share the whole body: the
/// column count giving the roomiest cards wins, a short last row stretches, and only when cards
/// would get smaller than `MIN_W`×`MIN_H` does the grid keep that size and scroll to the selection.
fn live(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let rows = app.visible_agents();
    if rows.is_empty() {
        return empty(f, area, "no agents here", pal);
    }
    let (cols, fit) = grid(rows.len(), area.width, area.height);
    app.grid_cols.set(cols);
    let first = (app.selected_row / cols).saturating_sub(fit - 1) * cols;
    let shown = &rows[first..rows.len().min(first + cols * fit)];
    let row_count = shown.len().div_ceil(cols);
    let lines =
        Layout::vertical(vec![Constraint::Ratio(1, row_count as u32); row_count]).split(area);
    for (r, line) in lines.iter().enumerate() {
        let cards = &shown[r * cols..shown.len().min((r + 1) * cols)];
        let cells = Layout::horizontal(vec![Constraint::Ratio(1, cards.len() as u32); cards.len()])
            .split(*line);
        for (c, ((p, a), rect)) in cards.iter().zip(cells.iter()).enumerate() {
            let selected = first + r * cols + c == app.selected_row;
            // Running subagents get cards of their own beside the agent's screen, or below it when
            // the card is narrow; not while you type into it, and not when either part would
            // drop below the minimum card size.
            let typing = app.typing.as_deref() == Some(a.pane.pane_id.as_str());
            let subs = app.running_subagents_of(a);
            let split = [Constraint::Percentage(60), Constraint::Percentage(40)];
            let parts = match (typing || subs.is_empty(), rect.width, rect.height) {
                (true, ..) => None,
                (false, w, _) if w >= 2 * MIN_W => Some(Layout::horizontal(split).areas(*rect)),
                (false, _, h) if h >= 2 * MIN_H => Some(Layout::vertical(split).areas(*rect)),
                _ => None,
            };
            let main = match parts {
                Some([main, side]) => {
                    subagent_cards(f, side, app, pal, &subs);
                    main
                }
                None => *rect,
            };
            card(f, main, app, pal, p, a, selected);
        }
    }
}

const MIN_W: u16 = 40;
const MIN_H: u16 = 7;
/// Smallest subagent card: its borders and two log lines.
const SUB_H: u16 = 4;

/// A subagent's own tokens and estimated cost, if it used any.
fn own_usage(app: &App, s: &SubAgent) -> Option<String> {
    let mut cost = crate::pricing::Cost::default();
    let mut tokens = 0;
    for (model, u) in &s.usage {
        tokens += u.total();
        cost.add(app.pricing.cost(model, u));
    }
    (tokens > 0).then(|| format!("{} tok · {}", theme::tokens(tokens), theme::cost(&cost)))
}

/// Stacked cards of running subagents with the tail of their logs; those that do not fit fold into
/// a last line.
fn subagent_cards(f: &mut Frame, area: Rect, app: &App, pal: &Palette, subs: &[&SubAgent]) {
    let fit = ((area.height / SUB_H) as usize).max(1);
    let shown = match subs.len() > fit {
        true => (fit - 1).max(1),
        false => subs.len(),
    };
    let more = subs.len() - shown;
    let mut heights = vec![Constraint::Ratio(1, shown as u32); shown];
    if more > 0 {
        heights.push(Constraint::Length(1));
    }
    let rects = Layout::vertical(heights).split(area);
    for (s, rect) in subs.iter().zip(rects.iter()) {
        let title = format!(
            " ● {} · {} · {} ",
            s.agent_type,
            s.description,
            theme::ago(s.last_ts, app.now)
        );
        let mut block = Block::bordered()
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::new().fg(pal.dim))
            .title(Span::styled(
                theme::truncate(&title, rect.width.saturating_sub(4) as usize),
                Style::new().fg(pal.accent),
            ));
        // Its own tokens and cost on the bottom border, so a long description cannot cut them.
        if let Some(usage) = own_usage(app, s) {
            block = block.title_bottom(Line::from(format!(" {usage} ")).right_aligned());
        }
        let inner = rect.height.saturating_sub(2) as usize;
        let log = &s.activity[s.activity.len().saturating_sub(inner)..];
        let lines: Vec<Line> = log.iter().map(|l| Line::from(l.as_str())).collect();
        f.render_widget(Paragraph::new(lines).block(block), *rect);
    }
    if more > 0 {
        f.render_widget(
            Paragraph::new(format!(" +{more} more")).style(Style::new().fg(pal.dim)),
            rects[shown],
        );
    }
}

/// Columns and rows of cards for `n` agents in `w`×`h` cells.
fn grid(n: usize, w: u16, h: u16) -> (usize, usize) {
    let max_cols = ((w / MIN_W).max(1) as usize).min(n);
    let max_rows = (h / MIN_H).max(1) as usize;
    // Cards about 2.5 times as wide as tall (in cells) read best; take the most room at that shape.
    let score = |cols: usize| {
        let rows = n.div_ceil(cols);
        let (cw, ch) = (w as f64 / cols as f64, h as f64 / rows as f64);
        // Empty slots in the last row count against a layout.
        (cw / 2.5).min(ch) * n as f64 / (rows * cols) as f64
    };
    match (1..=max_cols)
        .filter(|c| n.div_ceil(*c) <= max_rows)
        .max_by(|a, b| score(*a).total_cmp(&score(*b)))
    {
        Some(cols) => (cols, n.div_ceil(cols)),
        None => (max_cols, max_rows),
    }
}

fn card(
    f: &mut Frame,
    rect: Rect,
    app: &App,
    pal: &Palette,
    p: &model::Project,
    a: &model::AgentRow,
    selected: bool,
) {
    let s = a.pane.agent_status;
    let color = theme::status_color(s, pal);
    let title = format!(
        " {} {} · {} · {} ",
        theme::glyph(s),
        p.name,
        a.pane.agent.as_deref().unwrap_or("agent"),
        a.pane.title()
    );
    // While you type into it the card shows the whole screen, input box included.
    let typing = app.typing.as_deref() == Some(a.pane.pane_id.as_str());
    let title = match typing {
        true => format!(" ⌨ typing · Esc leaves ·{title}"),
        false => title,
    };
    let block = Block::bordered()
        .border_type(if typing {
            ratatui::widgets::BorderType::Double
        } else if selected {
            ratatui::widgets::BorderType::Thick
        } else {
            ratatui::widgets::BorderType::Rounded
        })
        .border_style(Style::new().fg(if selected { pal.accent } else { color }))
        .title(Span::styled(
            theme::truncate(&title, rect.width.saturating_sub(4) as usize),
            Style::new().fg(color).bold(),
        ));
    let inner = rect.height.saturating_sub(2) as usize;
    let screen = app
        .screens
        .get(&a.pane.pane_id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let lines = activity(screen, typing || s == AgentStatus::Blocked);
    let tail = lines[lines.len().saturating_sub(inner)..].to_vec();
    f.render_widget(Paragraph::new(tail).block(block), rect);
}

/// An agent screen in its own colors, without its input box and the hints under it, which take
/// the last lines of every screen; a blocked agent keeps them, its question is down there.
/// Blank lines are kept single, like the gaps between the agent's blocks.
/// ponytail: the input box is found by its horizontal rules in the last six lines; agents that
/// draw it differently show it in their card.
fn activity(screen: &[Line<'static>], blocked: bool) -> Vec<Line<'static>> {
    let mut lines: Vec<(String, Line<'static>)> = Vec::new();
    for l in screen.iter().cloned() {
        let text = super::ansi::plain(&l).trim_end().to_string();
        let blank = text.trim().is_empty();
        if blank && lines.last().is_none_or(|(t, _)| t.trim().is_empty()) {
            continue;
        }
        lines.push((text, l));
    }
    let rule = |l: &str| {
        let l = l.trim();
        l.chars().count() > 10 && l.chars().all(|c| "─━═╭╮╰╯│ ".contains(c))
    };
    if !blocked {
        let tail = lines.len().saturating_sub(6);
        if let Some(cut) = (tail..lines.len()).find(|&i| rule(&lines[i].0)) {
            lines.truncate(cut);
        }
    }
    while lines.last().is_some_and(|(t, _)| t.trim().is_empty()) {
        lines.pop();
    }
    lines.into_iter().map(|(_, l)| l).collect()
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
    // Each agent row is followed by its subagents (running, or finished in the last 10 minutes);
    // those rows are not selectable, so the selection maps to the agent rows only.
    let mut body: Vec<Row> = Vec::new();
    let mut selected = 0;
    for (i, (p, a)) in rows.iter().enumerate() {
        if i == app.selected_row {
            selected = body.len();
        }
        body.push({
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
                Cell::from(Line::from(vec![
                    Span::styled(
                        if app.is_new(a) { "new " } else { "" },
                        Style::new().fg(theme::status_color(s, pal)).bold(),
                    ),
                    Span::raw(theme::truncate(&title, 48)),
                ])),
                Cell::from(theme::ago(a.since, app.now)),
            ];
            if global {
                cells.insert(1, Cell::from(p.name.clone()));
            }
            Row::new(cells)
        });
        let subs: Vec<_> = (app.subagents(a).into_iter())
            .filter(|(sub, running)| {
                *running
                    || sub
                        .last_ts
                        .is_some_and(|t| app.now - t < chrono::Duration::minutes(10))
            })
            .collect();
        for (n, (sub, running)) in subs.iter().enumerate() {
            let branch = if n + 1 == subs.len() { "└" } else { "├" };
            let (glyph, color) = if *running {
                ("●", pal.accent)
            } else {
                ("✓", pal.dim)
            };
            let mut cells = vec![
                Cell::from(Line::from(vec![
                    Span::styled(format!(" {branch} "), Style::new().fg(pal.dim)),
                    Span::styled(glyph, Style::new().fg(color)),
                ])),
                Cell::from(""),
                Cell::from(Span::styled(
                    theme::truncate(&sub.agent_type, 8),
                    Style::new().fg(pal.dim),
                )),
                Cell::from(theme::truncate(&sub.description, 48)),
                Cell::from(theme::ago(sub.last_ts, app.now)),
            ];
            if global {
                cells.insert(1, Cell::from(""));
            }
            body.push(Row::new(cells).style(Style::new().fg(pal.dim)));
        }
    }
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
    f.render_stateful_widget(table, list, &mut table_state(selected));

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
    let subs = app.subagents(a);
    if !subs.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("subagents", Style::new().fg(pal.dim)));
        for (sub, running) in subs.iter().take(8) {
            let (glyph, color) = if *running {
                ("●", pal.accent)
            } else {
                ("✓", pal.dim)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::new().fg(color)),
                Span::raw(format!("{} ", theme::truncate(&sub.description, 40))),
                Span::styled(
                    format!("{} · {}", sub.agent_type, theme::ago(sub.last_ts, app.now)),
                    Style::new().fg(pal.dim),
                ),
                Span::styled(
                    own_usage(app, sub).map_or(String::new(), |u| format!(" · {u}")),
                    Style::new().fg(pal.dim),
                ),
            ]));
        }
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
            "no agent threads"
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
                Cell::from(t.thread.agent.clone()),
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
        Constraint::Length(7),
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
                "", "title", "agent", "project", "branch", "last", "turns", "tokens", "cost",
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

fn ideas(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let rows = app.visible_ideas();
    let folded = app.folded_done();
    if rows.is_empty() {
        let msg = if folded > 0 {
            format!("all {folded} ideas here are done · c to show")
        } else if let Some(s) = app.idea_filter {
            format!("no {} ideas · s changes the filter", s.label())
        } else if !app.search.is_empty() {
            "no ideas match the search".into()
        } else if app.scope().is_none() {
            "no ideas yet · f picks a project, then a adds".into()
        } else {
            "no ideas yet · a to add".into()
        };
        return empty(f, area, &msg, pal);
    }
    let [list, fold, preview] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(u16::from(folded > 0)),
        Constraint::Length(5),
    ])
    .areas(area);
    f.render_widget(
        Paragraph::new(format!("── ✓ {folded} done · c to show ──"))
            .style(Style::new().fg(pal.dim)),
        fold,
    );
    let global = matches!(app.screen, Screen::Global(_));
    let body: Vec<Row> = rows
        .iter()
        .map(|(_, i)| {
            let (color, name) = match i.status {
                IdeaStatus::Todo => (pal.dim, Style::new().bold()),
                IdeaStatus::Doing => (pal.warn, Style::new().bold()),
                IdeaStatus::Done => (pal.ok, Style::new().fg(pal.dim)),
            };
            let mut cells = vec![
                Cell::from(Span::styled(i.status.label(), Style::new().fg(color))),
                Cell::from(Span::styled(i.name.clone(), name)),
            ];
            if global {
                cells.push(Cell::from(Span::styled(
                    i.project_name.clone(),
                    Style::new().fg(pal.dim),
                )));
            }
            cells.push(Cell::from(i.description.clone()));
            Row::new(cells)
        })
        .collect();
    let mut widths = vec![Constraint::Length(8), Constraint::Length(28)];
    let mut header = vec!["status", "idea"];
    if global {
        widths.push(Constraint::Length(18));
        header.push("project");
    }
    widths.push(Constraint::Min(10));
    header.push("description");
    let table = Table::new(body, widths)
        .header(Row::new(header).style(Style::new().fg(pal.dim)))
        .row_highlight_style(Style::new().bg(pal.highlight));
    f.render_stateful_widget(table, list, &mut table_state(app.selected_row));
    if let Some((_, i)) = rows.get(app.selected_row) {
        f.render_widget(
            Paragraph::new(i.description.clone())
                .wrap(Wrap { trim: true })
                .block(
                    Block::new()
                        .borders(Borders::TOP)
                        .border_style(Style::new().fg(pal.dim)),
                ),
            preview,
        );
    }
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
        return empty(f, area, "no agent usage in the last 14 days", pal);
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
        assert!(!out.contains("subagents"));
        let out = screen(&crate::ui::app::with_subagents(app), 120, 30);
        assert!(out.contains("subagents"));
        assert!(out.contains("● Find the lexer Explore"));
        assert!(out.contains("✓ Old survey"));
        // And as tree rows under their agent in the list.
        assert!(out.contains(" ├ ●") && out.contains("Explore  Find the lexer"));
        assert!(out.contains(" └ ✓") && out.contains("Explore  Old survey"));
    }

    #[test]
    fn new_agents_are_marked() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Agents);
        app.last_left = crate::model::testkit::ts("2000-01-01T00:00:00Z");
        app.model.projects[0].agents[0].since = Some(app.now);
        assert!(screen(&app, 140, 30).contains("new title w2:p1"));
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
    fn ideas_show_name_description_and_project_globally() {
        let mut app = sample_app();
        app.ideas = vec![crate::ideas::Idea {
            project_key: app.model.projects[1].key.clone(),
            project_name: "alpha".into(),
            name: "Pricing cache".into(),
            description: "keep prices for 24h".into(),
            status: crate::ideas::Status::Doing,
        }];
        app.screen = Screen::Global(Tab::Ideas);
        let out = screen(&app, 120, 30);
        assert!(out.contains("Pricing cache"));
        assert!(out.contains("◐ doing"));
        assert!(out.contains("alpha"));
        assert!(out.contains("keep prices for 24h"));
        assert!(out.contains("a add"));
    }

    #[test]
    fn empty_ideas_say_why() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Ideas);
        assert!(screen(&app, 120, 30).contains("no ideas yet · f picks a project, then a adds"));
        app.ideas = vec![crate::ideas::Idea {
            project_key: app.model.projects[1].key.clone(),
            project_name: "alpha".into(),
            name: "Next".into(),
            description: String::new(),
            status: crate::ideas::Status::Todo,
        }];
        app.idea_filter = Some(crate::ideas::Status::Doing);
        assert!(screen(&app, 120, 30).contains("no ◐ doing ideas · s changes the filter"));
    }

    #[test]
    fn done_ideas_fold_into_one_line() {
        let mut app = sample_app();
        let done = crate::ideas::Idea {
            project_key: app.model.projects[1].key.clone(),
            project_name: "alpha".into(),
            name: "Shipped".into(),
            description: String::new(),
            status: crate::ideas::Status::Done,
        };
        app.ideas = vec![done.clone()];
        app.screen = Screen::Global(Tab::Ideas);
        assert!(screen(&app, 120, 30).contains("all 1 ideas here are done · c to show"));
        app.ideas.push(crate::ideas::Idea {
            name: "Next".into(),
            status: crate::ideas::Status::Todo,
            ..done
        });
        let out = screen(&app, 120, 30);
        assert!(out.contains("Next"));
        assert!(!out.contains("Shipped"));
        assert!(out.contains("── ✓ 1 done · c to show ──"));
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

    #[test]
    fn live_cards_show_each_agent_screen() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Live);
        let pane = app.visible_agents()[0].1.pane.pane_id.clone();
        app.screens.insert(
            pane,
            crate::ui::ansi::lines("old line\n\n> fix it\n● Update(src/lib.rs)\n"),
        );
        let out = screen(&app, 120, 30);
        assert!(out.contains("● Update(src/lib.rs)"));
        assert!(out.contains("> fix it"));
        // A card per agent, the selected one with a thick border.
        assert!(out.contains("┏"));
        assert_eq!(out.matches("╭").count(), app.visible_agents().len() - 1);
    }

    #[test]
    fn live_card_splits_off_its_running_subagents() {
        let mut app = crate::ui::app::with_subagents(sample_app());
        let t = (app.model.threads.iter_mut())
            .find(|t| t.thread.session_id == "sess-a")
            .unwrap();
        t.thread.subagents[0].activity =
            vec!["Grep  ChangeScene".into(), "Read  src/lexer.rs".into()];
        let usage = crate::pricing::Usage {
            input: 1_000_000,
            output: 100_000,
            ..Default::default()
        };
        t.thread.subagents[0].usage = [("claude-sonnet-4-5".to_string(), usage)].into();
        // Its own tokens and cost: in the agent's side panel, and in its Live card's title.
        app.screen = Screen::Global(Tab::Agents);
        app.selected_row = (app.visible_agents().iter())
            .position(|(_, a)| a.pane.pane_id == "w1:p1")
            .unwrap();
        let out = screen(&app, 200, 40);
        let own = |out: &str| {
            (out.lines()
                .find(|l| l.contains("Find the lexer") && l.contains("Explore ·")))
            .is_some_and(|l| l.contains("1.1M tok · ≈$"))
        };
        assert!(own(&out), "{out}");
        app.selected_row = 0;
        app.screen = Screen::Global(Tab::Live);
        let title = |out: &str| -> String {
            (out.lines().find(|l| l.contains("Explore · Find the lexer")))
                .unwrap_or_default()
                .to_string()
        };
        // Wide: the subagent cards stand beside the agent's screen.
        let out = screen(&app, 200, 40);
        assert!(out.contains("Grep  ChangeScene"));
        assert!(out.contains("Read  src/lexer.rs"));
        assert!(!out.contains("Old survey"));
        assert!(title(&out).contains(" alpha · claude"));
        // Its tokens and cost on the card's bottom border, clear of the description.
        let bottom = (out.lines())
            .find(|l| l.contains("1.1M tok · ≈$"))
            .unwrap_or_default();
        assert!(bottom.contains('╰') && !bottom.contains("Find the lexer"));
        // Narrow: below it.
        let out = screen(&app, 70, 40);
        assert!(out.contains("Read  src/lexer.rs"));
        assert!(title(&out).starts_with('╭'));
        // Too small to split, or typed into: the agent keeps its whole card.
        app.selected_row = 1;
        let out = screen(&app, 70, 14);
        assert!(out.contains("alpha · claude") && !out.contains("Find the lexer"));
        app.typing = Some(app.visible_agents()[1].1.pane.pane_id.clone());
        assert!(!screen(&app, 200, 40).contains("Find the lexer"));
    }

    #[test]
    fn activity_drops_the_input_box() {
        let rule = "─".repeat(40);
        let screen = format!(
            "● Update(a.rs)
  ⎿  done

{rule}
❯ 
{rule}
  ⏵⏵ auto mode on
"
        );
        let plain = |screen: &str, blocked| -> Vec<String> {
            (activity(&crate::ui::ansi::lines(screen), blocked).iter())
                .map(|l| crate::ui::ansi::plain(l).trim_end().to_string())
                .collect()
        };
        assert_eq!(plain(&screen, false), ["● Update(a.rs)", "  ⎿  done"]);
        assert_eq!(plain(&screen, true).len(), 7);
        assert_eq!(plain("\n\njust\n\n\ntext\n\n", false), ["just", "", "text"]);
    }

    #[test]
    fn cards_fill_the_screen() {
        // Few agents: big cards, no empty rows.
        assert_eq!(grid(1, 200, 50), (1, 1));
        assert_eq!(grid(2, 200, 50), (2, 1));
        assert_eq!(grid(4, 200, 50), (2, 2));
        assert_eq!(grid(9, 200, 50), (3, 3));
        // Too many to fit at the minimum size: as many as fit, the rest scrolls.
        assert_eq!(grid(40, 200, 50), (5, 7));
        assert_eq!(grid(3, 30, 5), (1, 1));
    }
}
