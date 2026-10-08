use crate::events::{Kind, Record};
use crate::herdr::AgentStatus;
use crate::model::Counts;
use crate::pricing::Cost;
use chrono::{DateTime, Local, Utc};
use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub alert: Color,
    pub alert_dim: Color,
    pub dim: Color,
    pub text: Color,
    pub highlight: Color,
}

impl Palette {
    /// Truecolor HUD palette when the terminal advertises it, else the 16-colour fallback.
    pub fn detect() -> Palette {
        let truecolor = std::env::var("COLORTERM")
            .is_ok_and(|v| v.contains("truecolor") || v.contains("24bit"));
        if truecolor {
            Palette {
                accent: Color::Rgb(0, 229, 255),
                ok: Color::Rgb(80, 250, 123),
                warn: Color::Rgb(255, 176, 0),
                alert: Color::Rgb(255, 59, 48),
                alert_dim: Color::Rgb(120, 20, 20),
                dim: Color::Rgb(110, 120, 135),
                text: Color::Rgb(220, 235, 245),
                highlight: Color::Rgb(20, 45, 60),
            }
        } else {
            Palette {
                accent: Color::Cyan,
                ok: Color::Green,
                warn: Color::Yellow,
                alert: Color::Red,
                alert_dim: Color::DarkGray,
                dim: Color::DarkGray,
                text: Color::White,
                highlight: Color::DarkGray,
            }
        }
    }
}

pub fn glyph(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Blocked => "▲",
        AgentStatus::Done => "✓",
        AgentStatus::Working => "●",
        AgentStatus::Idle => "○",
        AgentStatus::Unknown => "·",
    }
}

pub fn word(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle => "idle",
        AgentStatus::Unknown => "unknown",
    }
}

pub fn status_color(s: AgentStatus, pal: &Palette) -> Color {
    match s {
        AgentStatus::Blocked => pal.alert,
        AgentStatus::Done => pal.warn,
        AgentStatus::Working => pal.accent,
        AgentStatus::Idle | AgentStatus::Unknown => pal.dim,
    }
}

/// Shortens to `max` characters (not bytes), ending with `…` when cut.
/// ponytail: counts chars, not display width; wide CJK glyphs may overflow by a cell.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn tokens(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}

pub fn cost(c: &Cost) -> String {
    if c.usd == 0.0 && c.partial {
        return "? $".into();
    }
    format!("≈${:.2}{}", c.usd, if c.partial { "+?" } else { "" })
}

pub fn ago(ts: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(ts) = ts else { return "—".into() };
    match (now - ts).num_seconds().max(0) {
        s if s < 60 => "now".into(),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

pub fn duration(d: chrono::Duration) -> String {
    let s = d.num_seconds().max(0);
    if s >= 3_600 {
        format!("{}h{:02}m", s / 3_600, s % 3_600 / 60)
    } else {
        format!("{}m{:02}s", s / 60, s % 60)
    }
}

/// `HH:MM` for today, `MM-DD HH:MM` otherwise, in local time.
pub fn stamp(ts: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = ts.with_timezone(&Local);
    if local.date_naive() == now.with_timezone(&Local).date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%m-%d %H:%M").to_string()
    }
}

pub fn transition(r: &Record) -> String {
    let w = |s: Option<AgentStatus>| s.map(word).unwrap_or("?");
    match r.kind {
        Kind::Status => format!("{} → {}", w(r.from), w(r.to)),
        Kind::AgentAppeared => format!("started ({})", w(r.to)),
        Kind::AgentGone => "ended".into(),
    }
}

pub fn counts(c: &Counts) -> String {
    let parts: Vec<String> = [
        ("▲", c.blocked),
        ("✓", c.done),
        ("●", c.working),
        ("○", c.idle),
    ]
    .iter()
    .filter(|(_, n)| *n > 0)
    .map(|(g, n)| format!("{g}{n}"))
    .collect();
    if parts.is_empty() {
        "no agents".into()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::testkit::ts;

    #[test]
    fn truncate_counts_chars() {
        assert_eq!(truncate("Zażółć gęślą jaźń", 50), "Zażółć gęślą jaźń");
        assert_eq!(truncate("Zażółć gęślą jaźń", 10), "Zażółć gę…");
        assert_eq!(truncate("łódź", 1), "…");
    }

    #[test]
    fn formats_tokens_costs_and_times() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(3_100_000), "3.1M");
        assert_eq!(tokens(12_400), "12.4k");
        assert_eq!(
            cost(&Cost {
                usd: 12.4,
                partial: false
            }),
            "≈$12.40"
        );
        assert_eq!(
            cost(&Cost {
                usd: 1.0,
                partial: true
            }),
            "≈$1.00+?"
        );
        assert_eq!(
            cost(&Cost {
                usd: 0.0,
                partial: true
            }),
            "? $"
        );
        let now = ts("2026-10-08T12:00:00Z");
        assert_eq!(ago(Some(ts("2026-10-08T11:57:00Z")), now), "3m");
        assert_eq!(ago(Some(ts("2026-10-08T09:00:00Z")), now), "3h");
        assert_eq!(ago(None, now), "—");
        assert_eq!(duration(chrono::Duration::seconds(252)), "4m12s");
        assert_eq!(duration(chrono::Duration::seconds(3_900)), "1h05m");
    }

    #[test]
    fn counts_show_non_zero_states() {
        let c = Counts {
            blocked: 1,
            done: 0,
            working: 2,
            idle: 3,
        };
        assert_eq!(counts(&c), "▲1 ●2 ○3");
        assert_eq!(counts(&Counts::default()), "no agents");
    }
}
