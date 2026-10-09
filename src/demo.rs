//! `jarvis demo`: made-up projects for screenshots and recordings; no herdr, no real data.

use crate::events::{Kind, Record};
use crate::herdr::{AgentStatus, Snapshot};
use crate::ideas::{Idea, Status};
use crate::pricing::Usage;
use crate::projects::normalize;
use crate::run::Target;
use crate::transcripts::Thread;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;

/// (workspace, cwd, agent, status, title, minutes since its last change)
const AGENTS: [(&str, &str, &str, &str, &str, i64); 9] = [
    (
        "d1",
        "/home/me/shop",
        "claude",
        "blocked",
        "Fix flaky cart test",
        3,
    ),
    (
        "d1",
        "/home/me/shop",
        "claude",
        "working",
        "Retry failed payments",
        12,
    ),
    (
        "d2",
        "/home/me/api-gateway",
        "claude",
        "done",
        "Rate limit per API key",
        6,
    ),
    (
        "d2",
        "/home/me/api-gateway",
        "codex",
        "working",
        "Trace slow requests",
        25,
    ),
    (
        "d3",
        "/home/me/docs-site",
        "claude",
        "working",
        "Rewrite getting started",
        8,
    ),
    (
        "d4",
        "/home/me/mobile-app",
        "claude",
        "idle",
        "Dark mode for settings",
        95,
    ),
    (
        "d5",
        "/home/me/infra",
        "codex",
        "working",
        "Split prod Terraform state",
        40,
    ),
    (
        "d6",
        "/home/me/ml-pipeline",
        "claude",
        "done",
        "Speed up feature extraction",
        50,
    ),
    (
        "d7",
        "/home/me/blog",
        "opencode",
        "idle",
        "Draft release post",
        180,
    ),
];

const MODELS: [&str; 3] = [
    "claude-opus-5-5",
    "claude-sonnet-5",
    "claude-haiku-4-5-20251001",
];

fn status(s: &str) -> AgentStatus {
    serde_json::from_value(json!(s)).unwrap_or(AgentStatus::Unknown)
}

pub fn snapshot() -> Snapshot {
    let agents: Vec<_> = (AGENTS.iter().enumerate())
        .map(|(i, (ws, cwd, agent, status, title, _))| {
            json!({
                "pane_id": format!("{ws}:p{i}"), "workspace_id": ws, "tab_id": format!("{ws}:t1"),
                "cwd": cwd, "agent": agent, "agent_status": status, "terminal_title_stripped": title,
                "agent_session": (*agent == "claude").then(|| json!({"value": format!("demo-{i}")})),
            })
        })
        .collect();
    let workspaces: Vec<_> = (1..=7)
        .map(|n| json!({"workspace_id": format!("d{n}"), "number": n}))
        .collect();
    serde_json::from_value(json!({
        "version": "0.9.3", "protocol": crate::herdr::client::PROTOCOL,
        "workspaces": workspaces, "panes": agents, "agents": agents,
    }))
    .expect("demo snapshot")
}

/// One Claude thread per Claude agent, with a week of usage.
pub fn threads(now: DateTime<Utc>) -> Vec<Thread> {
    (AGENTS.iter().enumerate())
        .filter(|(_, a)| a.2 == "claude")
        .map(|(i, (_, cwd, _, _, title, ago))| {
            let mut t = Thread {
                session_id: format!("demo-{i}"),
                title: Some(title.to_string()),
                cwd: Some(cwd.to_string()),
                branch: Some("main".into()),
                turns: 4 + i as u32 * 3,
                first_ts: Some(now - Duration::hours(30 + i as i64)),
                last_ts: Some(now - Duration::minutes(*ago)),
                ..Default::default()
            };
            for day in 0..7u64 {
                let k = (i as u64 + 1) * (7 - day);
                t.usage
                    .entry(crate::transcripts::claude::local_day(
                        now - Duration::days(day as i64),
                    ))
                    .or_default()
                    .insert(
                        MODELS[(i + day as usize) % 3].into(),
                        Usage {
                            input: 9_000 * k,
                            output: 14_000 * k,
                            cache_read: 380_000 * k,
                            cache_write_5m: 30_000 * k,
                            ..Default::default()
                        },
                    );
            }
            t
        })
        .collect()
}

/// Each agent went working at some point and reached its current state `ago` minutes back.
pub fn records(now: DateTime<Utc>) -> Vec<Record> {
    (AGENTS.iter().enumerate())
        .flat_map(|(i, (ws, cwd, agent, to, _, ago))| {
            let rec = |mins: i64, from: &str, to: &str| Record {
                ts: now - Duration::minutes(mins),
                kind: Kind::Status,
                pane_id: format!("{ws}:p{i}"),
                workspace_id: ws.to_string(),
                cwd: Some(cwd.to_string()),
                agent: Some(agent.to_string()),
                from: Some(status(from)),
                to: Some(status(to)),
                session_id: None,
                title: None,
            };
            [rec(ago + 20, "idle", "working"), rec(*ago, "working", to)]
        })
        .filter(|r| r.from != r.to)
        .collect()
}

pub fn ideas() -> Vec<Idea> {
    let idea = |cwd: &str, name: &str, description: &str, status| Idea {
        project_key: normalize(cwd),
        project_name: cwd.rsplit('/').next().unwrap_or(cwd).into(),
        name: name.into(),
        description: description.into(),
        status,
    };
    vec![
        idea(
            "/home/me/shop",
            "Gift cards",
            "redeem at checkout, balance in account",
            Status::Doing,
        ),
        idea(
            "/home/me/shop",
            "Saved carts",
            "keep the cart across devices",
            Status::Todo,
        ),
        idea(
            "/home/me/shop",
            "Order export",
            "CSV for accounting",
            Status::Done,
        ),
        idea(
            "/home/me/docs-site",
            "Search",
            "full-text search over all pages",
            Status::Todo,
        ),
        idea(
            "/home/me/api-gateway",
            "OpenAPI from routes",
            "",
            Status::Todo,
        ),
    ]
}

pub fn targets() -> Vec<Target> {
    let target = |name: &str, command: &str| Target {
        project_key: normalize("/home/me/shop"),
        name: name.into(),
        command: command.into(),
        ..Target::default()
    };
    vec![
        target("backend", "npm run dev"),
        target("frontend", "npm run web"),
        target("worker", "npm run queue"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_data_builds_every_project() {
        let now = Utc::now();
        let m = crate::model::build(
            &snapshot(),
            &threads(now),
            &records(now),
            &crate::pricing::Pricing::new(Default::default()),
            &mut crate::projects::Resolver::default(),
            &crate::transcripts::claude::local_day(now),
        );
        assert_eq!(m.projects.len(), 7);
        assert_eq!(m.projects[0].name, "shop");
        assert!(m.totals.blocked == 1 && m.threads.len() == 6);
    }
}
