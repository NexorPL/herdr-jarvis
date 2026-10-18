//! Status bar and the core view: arc-reactor, project branches, node boxes, event feed, boot, compact list.
//!
//! Canvas units: x = one cell column, y = half a cell row, so circles look round on ~1:2 cells.

use super::app::{node_angle, App, Node};
use super::theme;
use crate::config::Animation;
use crate::events::Kind;
use ratatui::prelude::*;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine, Points};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use std::f64::consts::TAU;

pub fn status_bar(f: &mut Frame, area: Rect, app: &App) {
    let pal = &app.palette;
    let c = app.model.totals;
    let mut spans = vec![
        Span::styled(
            " JARVIS ",
            Style::new().fg(Color::Black).bg(pal.accent).bold(),
        ),
        // Up front, so narrow terminals (compact mode) still see it; the reason follows at the end.
        Span::styled(
            if app.offline.is_some() {
                " OFFLINE "
            } else {
                ""
            },
            Style::new().fg(Color::Black).bg(pal.alert).bold(),
        ),
        Span::raw("  "),
        Span::styled(
            format!("● {} working", c.working),
            Style::new().fg(pal.accent),
        ),
        Span::raw("  "),
        Span::styled(
            format!("▲ {} blocked", c.blocked),
            Style::new().fg(if c.blocked > 0 { pal.alert } else { pal.dim }),
        ),
        Span::raw("  "),
        Span::styled(format!("✓ {} done", c.done), Style::new().fg(pal.warn)),
        Span::raw("  "),
        Span::styled(format!("○ {} idle", c.idle), Style::new().fg(pal.dim)),
        Span::raw("   "),
        Span::styled(
            format!(
                "today {} · {} tok",
                theme::cost(&app.model.today_cost),
                theme::tokens(app.model.today.total())
            ),
            Style::new().fg(pal.text),
        ),
    ];
    if let Some(msg) = app.offline.as_ref().or(app.status.as_ref()) {
        let color = if app.offline.is_some() {
            pal.alert
        } else {
            pal.warn
        };
        spans.push(Span::raw("   "));
        spans.push(Span::styled(msg.clone(), Style::new().fg(color)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    let clock = app
        .now
        .with_timezone(&chrono::Local)
        .format("%H:%M ")
        .to_string();
    f.render_widget(
        Paragraph::new(clock)
            .alignment(Alignment::Right)
            .style(Style::new().fg(pal.dim)),
        area,
    );
}

pub fn draw_core(f: &mut Frame, area: Rect, app: &App) {
    let [main, keys] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    key_line(f, keys, app);
    if app.compact {
        return compact(f, main, app);
    }
    let [field, feed_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(main);
    reactor(f, field, app);
    feed(f, feed_area, app);
}

/// What the core takes: the keys that matter here, the rest is behind `?`.
fn key_line(f: &mut Frame, area: Rect, app: &App) {
    let keys = [
        ("←↑↓→", "select"),
        ("Enter", "open"),
        ("N", "new agent"),
        ("o", "options"),
        ("v", "panels"),
        ("A T L U I G", "all-project views"),
        ("?", "help"),
        ("q", "quit"),
    ];
    let spans = super::views::key_bar(&keys, &app.palette);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn node_pos(i: usize, n: usize, w: f64, h: f64) -> (f64, f64) {
    let a = node_angle(i, n);
    (a.cos() * w * 0.36, a.sin() * h * 0.72)
}

fn to_cell(area: Rect, (x, y): (f64, f64)) -> (u16, u16) {
    let col = area.x as f64 + area.width as f64 / 2.0 + x;
    let row = area.y as f64 + area.height as f64 / 2.0 - y / 2.0;
    (col.round().max(0.0) as u16, row.round().max(0.0) as u16)
}

/// Dashed ring of radius `r`: `segments` lit arcs, rotated by `rotation` radians.
fn ring_points(r: f64, rotation: f64, segments: u32) -> Vec<(f64, f64)> {
    let seg = TAU / (2 * segments) as f64;
    (0..240)
        .map(|k| k as f64 * TAU / 240.0)
        .filter(|a| ((a + rotation) / seg).floor().rem_euclid(2.0) == 0.0)
        .map(|a| (a.cos() * r, a.sin() * r))
        .collect()
}

fn node_working(node: &Node, app: &App) -> bool {
    matches!(node, Node::Project(i) if app.model.projects[*i].counts.working > 0)
}

/// Node box size in cells for `lines` of text: a fifth of the field's width and at most a quarter
/// of its height, so eight nodes around the reactor do not overlap; never below 24×4.
fn box_size(field: Rect, lines: usize) -> (u16, u16) {
    let w = (field.width / 5).clamp(24, 44);
    let h = (lines as u16)
        .saturating_add(2)
        .clamp(4, (field.height / 4).max(4));
    (w, h)
}

/// Twigs for `n` running subagents: short lines fanned outward from the node box at (`x`, `y`),
/// whose half size in canvas units is `half`.
fn twigs(x: f64, y: f64, n: usize, half: (f64, f64)) -> Vec<((f64, f64), (f64, f64))> {
    let base = y.atan2(x);
    let spread = 0.35;
    (0..n)
        .map(|k| {
            let a = base + spread * (k as f64 - (n - 1) as f64 / 2.0);
            let (dx, dy) = (a.cos(), a.sin());
            // Leave the box where the ray from its center crosses its edge.
            let edge = (half.0 / dx.abs().max(1e-6)).min(half.1 / dy.abs().max(1e-6));
            let start = (x + dx * (edge + 1.0), y + dy * (edge + 1.0));
            (start, (start.0 + dx * 6.0, start.1 + dy * 6.0))
        })
        .collect()
}

fn reactor(f: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let pal = &app.palette;
    let nodes = app.nodes();
    let progress = app.boot_progress();
    let tick = app.anim_tick();
    let full = app.animation == Animation::Full;
    let (w, h) = (area.width as f64, area.height as f64);
    let r = (w / 2.0).min(h) * 0.22 * progress.max(0.1);
    let totals = app.model.totals;
    let ring = if totals.blocked > 0 {
        if full && (tick / 10) % 2 == 1 {
            pal.alert_dim
        } else {
            pal.alert
        }
    } else if totals.done > 0 {
        pal.warn
    } else {
        pal.accent
    };
    let speed = if full {
        2.0 + 2.0 * totals.working as f64
    } else {
        1.0
    };
    let rotation = (tick as f64 * speed).to_radians();
    let positions: Vec<(f64, f64)> = (0..nodes.len())
        .map(|i| node_pos(i, nodes.len(), w, h))
        .collect();
    let inner = box_size(area, 0).0 as usize - 2;
    let contents: Vec<NodeContent> = (nodes.iter().enumerate())
        .map(|(i, node)| node_content(app, i, node, inner))
        .collect();
    let sizes: Vec<(u16, u16)> = (contents.iter())
        .map(|c| box_size(area, c.lines.len()))
        .collect();
    let show_nodes = progress >= 0.6;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-w / 2.0, w / 2.0])
        .y_bounds([-h, h])
        .paint(|ctx| {
            if show_nodes {
                for (i, &(x, y)) in positions.iter().enumerate() {
                    let len = x.hypot(y).max(1e-6);
                    let (sx, sy) = (x / len * r * 1.15, y / len * r * 1.15);
                    ctx.draw(&CanvasLine {
                        x1: sx,
                        y1: sy,
                        x2: x,
                        y2: y,
                        color: pal.dim,
                    });
                    if full && node_working(&nodes[i], app) {
                        let t = (tick % 40) as f64 / 40.0;
                        ctx.draw(&Points {
                            coords: &[(sx + (x - sx) * t, sy + (y - sy) * t)],
                            color: pal.accent,
                        });
                    }
                    let subs = match nodes[i] {
                        Node::Project(p) => app.running_subagents(&app.model.projects[p]),
                        Node::More(_) => 0,
                    };
                    // Box rows are two canvas units high.
                    let half = (sizes[i].0 as f64 / 2.0, sizes[i].1 as f64);
                    for (k, ((x1, y1), (x2, y2))) in twigs(x, y, subs, half).into_iter().enumerate()
                    {
                        ctx.draw(&CanvasLine {
                            x1,
                            y1,
                            x2,
                            y2,
                            color: pal.dim,
                        });
                        ctx.draw(&Circle {
                            x: x2,
                            y: y2,
                            radius: 0.6,
                            color: pal.accent,
                        });
                        if full {
                            let t = ((tick + 9 * k as u64) % 30) as f64 / 30.0;
                            ctx.draw(&Points {
                                coords: &[(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t)],
                                color: pal.accent,
                            });
                        }
                    }
                }
            }
            ctx.draw(&Points {
                coords: &ring_points(r, rotation, 12),
                color: ring,
            });
            ctx.draw(&Points {
                coords: &ring_points(r * 0.72, -rotation * 1.5, 8),
                color: ring,
            });
            ctx.draw(&Circle {
                x: 0.0,
                y: 0.0,
                radius: r * 0.38,
                color: ring,
            });
            ctx.draw(&Circle {
                x: 0.0,
                y: 0.0,
                radius: r * 0.2,
                color: pal.text,
            });
            if progress < 1.0 {
                let y = h - 2.0 * h * progress;
                ctx.draw(&CanvasLine {
                    x1: -w / 2.0,
                    y1: y,
                    x2: w / 2.0,
                    y2: y,
                    color: pal.accent,
                });
            }
        });
    f.render_widget(canvas, area);

    let label = if progress < 1.0 {
        "J.A.R.V.I.S. ONLINE".to_string()
    } else if app.model.projects.is_empty() {
        "no herdr panes yet".to_string()
    } else {
        format!("{} active", totals.active())
    };
    let (cx, cy) = to_cell(area, (0.0, -(r + 3.0)));
    let lw = label.chars().count() as u16;
    let rect = Rect::new(
        cx.saturating_sub(lw / 2),
        cy.min(area.bottom().saturating_sub(1)),
        lw,
        1,
    )
    .intersection(area);
    f.render_widget(
        Paragraph::new(Span::styled(label, Style::new().fg(pal.accent).bold())),
        rect,
    );

    if show_nodes {
        for (i, content) in contents.into_iter().enumerate() {
            let center = to_cell(area, positions[i]);
            node_box(f, area, center, sizes[i], i, content, app);
        }
    }
}

struct NodeContent {
    title: String,
    lines: Vec<Line<'static>>,
    color: Color,
}

/// A project node: its counts, then a row per agent followed by its running subagents, each fitted
/// to `width` cells. The overflow node lists the projects it stands for.
fn node_content(app: &App, i: usize, node: &Node, width: usize) -> NodeContent {
    let pal = &app.palette;
    let dim = Style::new().fg(pal.dim);
    match node {
        Node::Project(p) => {
            let p = &app.model.projects[*p];
            let mut lines = vec![Line::from(theme::truncate(&project_counts(app, p), width))];
            for a in &p.agents {
                let s = a.pane.agent_status;
                let ago = theme::ago(a.since, app.now);
                let agent = a.pane.agent.as_deref().unwrap_or_default();
                let left = theme::truncate(
                    format!("{agent} {}", a.pane.title()).trim(),
                    width.saturating_sub(ago.chars().count() + 3),
                );
                let pad = width.saturating_sub(left.chars().count() + ago.chars().count() + 2);
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", theme::glyph(s)),
                        Style::new().fg(theme::status_color(s, pal)),
                    ),
                    Span::raw(left),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(ago, dim),
                ]));
                let running: Vec<_> = (app.subagents(a).into_iter())
                    .filter(|(_, running)| *running)
                    .collect();
                for (n, (sub, _)) in running.iter().enumerate() {
                    let branch = if n + 1 == running.len() { "└" } else { "├" };
                    let text = format!("{} {}", sub.agent_type, sub.description);
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {branch} "), dim),
                        Span::styled("● ", Style::new().fg(pal.accent)),
                        Span::styled(theme::truncate(&text, width.saturating_sub(6)), dim),
                    ]));
                }
            }
            NodeContent {
                title: format!("{} {}", i + 1, p.name),
                lines,
                color: theme::status_color(p.urgency(), pal),
            }
        }
        Node::More(n) => {
            let rest: Vec<&str> = (app.model.projects.iter().skip(i))
                .map(|p| p.name.as_str())
                .collect();
            NodeContent {
                title: format!("{} +{n} more", i + 1),
                lines: vec![Line::from(theme::truncate(&rest.join(", "), width))],
                color: pal.dim,
            }
        }
    }
}

fn node_box(
    f: &mut Frame,
    area: Rect,
    (cx, cy): (u16, u16),
    (w, h): (u16, u16),
    i: usize,
    content: NodeContent,
    app: &App,
) {
    let pal = &app.palette;
    let x = cx
        .saturating_sub(w / 2)
        .max(area.x)
        .min(area.right().saturating_sub(w));
    let y = cy
        .saturating_sub(h / 2)
        .max(area.y)
        .min(area.bottom().saturating_sub(h));
    let rect = Rect::new(x, y, w, h).intersection(area);
    if rect.is_empty() {
        return;
    }
    let selected = i == app.selected_node;
    let NodeContent {
        title,
        mut lines,
        color,
    } = content;
    let pulse_off =
        app.animation == Animation::Full && color == pal.alert && (app.anim_tick() / 8) % 2 == 1;
    let border = if selected {
        pal.accent
    } else if pulse_off {
        pal.alert_dim
    } else {
        color
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(if selected {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(Style::new().fg(border))
        .title(Span::styled(
            theme::truncate(&title, w as usize - 4),
            Style::new().fg(pal.text).bold(),
        ));
    // What does not fit folds into a last row.
    let rows = h as usize - 2;
    if lines.len() > rows {
        let hidden = lines.len() - (rows - 1);
        lines.truncate(rows - 1);
        lines.push(Line::styled(
            format!("… +{hidden}"),
            Style::new().fg(pal.dim),
        ));
    }
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

/// Agent counts, how many changed to blocked/done since you last left Jarvis, and running subagents.
fn project_counts(app: &App, p: &crate::model::Project) -> String {
    let mut counts = theme::counts(&p.counts);
    match app.new_in(p) {
        0 => {}
        n => counts += &format!(" · {n} new"),
    }
    match app.running_subagents(p) {
        0 => {}
        n => counts += &format!(" · {n} sub"),
    }
    counts
}

fn feed(f: &mut Frame, area: Rect, app: &App) {
    let pal = &app.palette;
    let dim = Style::new().fg(pal.dim);
    let mut spans = vec![Span::styled(" ▸ ", Style::new().fg(pal.accent))];
    if app.model.events.is_empty() {
        spans.push(Span::styled("no events yet", dim));
    }
    for (k, e) in app.model.events.iter().take(6).enumerate() {
        if k > 0 {
            spans.push(Span::styled(" · ", dim));
        }
        spans.push(Span::styled(theme::stamp(e.record.ts, app.now), dim));
        spans.push(Span::raw(format!(" {} ", e.project_name)));
        let color = e
            .record
            .to
            .map(|s| theme::status_color(s, pal))
            .unwrap_or(pal.dim);
        spans.push(Span::styled(
            theme::transition(&e.record),
            Style::new().fg(color),
        ));
        if let (Kind::Status, Some(d)) = (e.record.kind, e.duration) {
            spans.push(Span::styled(format!(" ({})", theme::duration(d)), dim));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn compact(f: &mut Frame, area: Rect, app: &App) {
    let pal = &app.palette;
    if app.model.projects.is_empty() {
        f.render_widget(
            Paragraph::new("no herdr panes yet").style(Style::new().fg(pal.dim)),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .model
        .projects
        .iter()
        .map(|p| {
            let s = p.urgency();
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", theme::glyph(s)),
                    Style::new().fg(theme::status_color(s, pal)),
                ),
                Span::styled(
                    format!("{:<20}", theme::truncate(&p.name, 20)),
                    Style::new().bold(),
                ),
                Span::raw(format!(" {:<16}", project_counts(app, p))),
                Span::styled(
                    theme::ago(p.last_activity, app.now),
                    Style::new().fg(pal.dim),
                ),
            ]))
        })
        .collect();
    f.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::new().bg(pal.highlight))
            .highlight_symbol("▸ "),
        area,
        &mut ListState::default().with_selected(Some(app.selected_node)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::sample_app;
    use crate::ui::render;

    fn core(app: &App, w: u16, h: u16) -> String {
        render(w, h, |f| {
            let area = f.area();
            draw_core(f, area, app)
        })
    }

    #[test]
    fn core_shows_numbered_project_nodes_and_count() {
        let app = sample_app();
        let out = core(&app, 100, 30);
        assert!(out.contains("1 beta"));
        assert!(out.contains("2 alpha"));
        assert!(out.contains("3 gamma"));
        assert!(out.contains("2 active"));
        assert!(out.contains("▲1"));
    }

    #[test]
    fn nodes_and_compact_list_count_new_agents() {
        let mut app = sample_app();
        app.last_left = crate::model::testkit::ts("2000-01-01T00:00:00Z");
        app.model.projects[0].agents[0].since = Some(app.now);
        assert!(core(&app, 100, 30).contains("▲1 · 1 new"));
        app.compact = true;
        assert!(core(&app, 60, 20).contains("▲1 · 1 new"));
    }

    #[test]
    fn running_subagents_count_on_their_node() {
        let mut app = crate::ui::app::with_subagents(sample_app());
        assert!(core(&app, 100, 30).contains("●1 · 1 sub"));
        // Silent for 15 minutes but its parent still works: a long tool call, still running.
        app.now += chrono::Duration::minutes(16);
        assert!(core(&app, 100, 30).contains("1 sub"));
        // Parent idle and the subagent silent: no longer running.
        for a in app
            .model
            .projects
            .iter_mut()
            .flat_map(|p| p.agents.iter_mut())
        {
            a.pane.agent_status = crate::herdr::AgentStatus::Idle;
        }
        assert!(!core(&app, 100, 30).contains("sub"));
    }

    #[test]
    fn node_box_lists_agents_and_running_subagents() {
        let app = crate::ui::app::with_subagents(sample_app());
        let out = core(&app, 200, 50);
        assert!(out.contains("● claude title w1:p1"));
        assert!(out.contains("● Explore Find the lexer"));
        assert!(!out.contains("Old survey"));
    }

    #[test]
    fn node_box_grows_with_the_field_and_folds_what_does_not_fit() {
        let mut app = sample_app();
        let alpha = &mut app.model.projects[1];
        let agent = alpha.agents[0].clone();
        for n in 2..10 {
            let mut a = agent.clone();
            a.pane.pane_id = format!("w1:p{n}");
            a.pane.terminal_title_stripped = Some(format!("title w1:p{n}"));
            alpha.agents.push(a);
        }
        // A 200×28 field: boxes 40 wide and up to 7 rows, so the counts, 3 agents and the fold.
        let out = core(&app, 200, 30);
        assert!(out.contains("title w1:p3"));
        assert!(!out.contains("title w1:p4"));
        assert!(out.contains("… +6"));
        assert_eq!(box_size(Rect::new(0, 0, 200, 46), 20), (40, 11));
        assert_eq!(box_size(Rect::new(0, 0, 100, 28), 2), (24, 4));
    }

    #[test]
    fn twigs_leave_the_node_box_outward() {
        let half = (20.0, 6.0);
        for (x, y) in [(40.0, 0.0), (0.0, 20.0), (-30.0, -15.0)] {
            for ((x1, y1), (x2, y2)) in twigs(x, y, 3, half) {
                assert!((x1 - x).abs() > half.0 || (y1 - y).abs() > half.1);
                assert!(x2.hypot(y2) > x1.hypot(y1));
            }
        }
    }

    #[test]
    fn core_frame_is_stable() {
        let mut app = sample_app();
        app.model.events.clear();
        insta::assert_snapshot!(core(&app, 100, 30));
    }

    #[test]
    fn boot_shows_banner_before_nodes() {
        let mut app = sample_app();
        app.boot_done = false;
        app.tick = 7;
        let out = core(&app, 100, 30);
        assert!(out.contains("J.A.R.V.I.S. ONLINE"));
        assert!(!out.contains("1 beta"));
    }

    #[test]
    fn compact_lists_projects() {
        let mut app = sample_app();
        app.compact = true;
        let out = core(&app, 60, 20);
        assert!(out.contains("▸ ▲ beta"));
        assert!(out.contains("alpha"));
    }

    #[test]
    fn empty_model_says_so() {
        let mut app = sample_app();
        app.model = Default::default();
        assert!(core(&app, 100, 30).contains("no herdr panes yet"));
        app.compact = true;
        assert!(core(&app, 60, 20).contains("no herdr panes yet"));
    }

    #[test]
    fn core_survives_tiny_area() {
        let mut app = sample_app();
        core(&app, 8, 3);
        core(&app, 1, 1);
        app.compact = true;
        core(&app, 8, 3);
    }

    #[test]
    fn status_bar_shows_counts_and_offline() {
        let mut app = sample_app();
        app.offline = Some("herdr offline: connect failed".into());
        let out = render(160, 1, |f| {
            let area = f.area();
            status_bar(f, area, &app)
        });
        assert!(out.contains("JARVIS"));
        assert!(out.contains("▲ 1 blocked"));
        assert!(out.contains("today ≈$4.00"));
        assert!(out.contains("herdr offline"));
    }

    #[test]
    fn offline_badge_visible_on_narrow_terminals() {
        let mut app = sample_app();
        app.offline = Some("herdr offline: connect failed".into());
        let out = render(60, 1, |f| {
            let area = f.area();
            status_bar(f, area, &app)
        });
        assert!(out.contains("JARVIS  OFFLINE"));
    }
}
