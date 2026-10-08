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
    if app.compact {
        return compact(f, area, app);
    }
    let [field, feed_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    reactor(f, field, app);
    feed(f, feed_area, app);
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
        for (i, node) in nodes.iter().enumerate() {
            node_box(f, area, to_cell(area, positions[i]), i, node, app, tick);
        }
    }
}

fn node_box(
    f: &mut Frame,
    area: Rect,
    (cx, cy): (u16, u16),
    i: usize,
    node: &Node,
    app: &App,
    tick: u64,
) {
    let pal = &app.palette;
    let (w, h) = (24u16, 4u16);
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
    let (title, lines, color) = match node {
        Node::Project(p) => {
            let p = &app.model.projects[*p];
            let first = p
                .agents
                .first()
                .map(|a| a.pane.title().to_string())
                .unwrap_or_default();
            (
                format!("{} {}", i + 1, p.name),
                vec![
                    theme::counts(&p.counts),
                    format!("{} · {first}", theme::ago(p.last_activity, app.now)),
                ],
                theme::status_color(p.urgency(), pal),
            )
        }
        Node::More(n) => {
            let rest: Vec<&str> = app
                .model
                .projects
                .iter()
                .skip(i)
                .map(|p| p.name.as_str())
                .collect();
            (
                format!("{} +{n} more", i + 1),
                vec![rest.join(", ")],
                pal.dim,
            )
        }
    };
    let pulse_off = app.animation == Animation::Full && color == pal.alert && (tick / 8) % 2 == 1;
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
    let body: Vec<Line> = lines
        .into_iter()
        .map(|l| Line::from(theme::truncate(&l, w as usize - 2)))
        .collect();
    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(body).block(block), rect);
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
                Span::raw(format!(" {:<16}", theme::counts(&p.counts))),
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
}
