//! Pure aggregation of herdr state, threads and timeline into what the UI shows.

use crate::events::{self, Record};
use crate::herdr::{AgentStatus, Pane, Snapshot};
use crate::pricing::{Cost, Pricing, Usage};
use crate::projects::{ProjectRef, Resolver};
use crate::transcripts::Thread;
use chrono::{DateTime, Local, Utc};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub blocked: usize,
    pub done: usize,
    pub working: usize,
    pub idle: usize,
}

impl Counts {
    pub fn add(&mut self, s: AgentStatus) {
        match s {
            AgentStatus::Blocked => self.blocked += 1,
            AgentStatus::Done => self.done += 1,
            AgentStatus::Working => self.working += 1,
            AgentStatus::Idle | AgentStatus::Unknown => self.idle += 1,
        }
    }

    pub fn active(&self) -> usize {
        self.blocked + self.done + self.working
    }

    pub fn total(&self) -> usize {
        self.active() + self.idle
    }
}

#[derive(Debug, Clone)]
pub struct AgentRow {
    pub pane: Pane,
    pub workspace: String,
    pub worktree: Option<String>,
    /// Index into `Model::threads`.
    pub thread: Option<usize>,
    /// Time of the latest timeline record for this pane.
    pub since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub key: String,
    pub name: String,
    pub root: String,
    pub branch: Option<String>,
    pub agents: Vec<AgentRow>,
    pub counts: Counts,
    pub last_activity: Option<DateTime<Utc>>,
}

impl Project {
    /// Most urgent agent state; `Unknown` for a project without agents.
    pub fn urgency(&self) -> AgentStatus {
        self.agents
            .iter()
            .map(|a| a.pane.agent_status)
            .min()
            .unwrap_or(AgentStatus::Unknown)
    }
}

#[derive(Debug, Clone)]
pub struct ThreadRow {
    pub thread: Thread,
    pub project_key: String,
    pub project_name: String,
    pub live_pane: Option<String>,
    pub usage: Usage,
    pub cost: Cost,
}

#[derive(Debug, Clone)]
pub struct EventRow {
    pub record: Record,
    pub project_key: String,
    pub project_name: String,
    pub duration: Option<chrono::Duration>,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub projects: Vec<Project>,
    pub threads: Vec<ThreadRow>,
    pub events: Vec<EventRow>,
    pub totals: Counts,
    pub today: Usage,
    pub today_cost: Cost,
}

fn entry<'a>(projects: &'a mut HashMap<String, Project>, p: &ProjectRef) -> &'a mut Project {
    projects.entry(p.key.clone()).or_insert_with(|| Project {
        key: p.key.clone(),
        name: p.name.clone(),
        root: p.root.clone(),
        branch: None,
        agents: Vec::new(),
        counts: Counts::default(),
        last_activity: None,
    })
}

pub fn build(
    snap: &Snapshot,
    threads: &[Thread],
    records: &[Record],
    pricing: &Pricing,
    resolver: &mut Resolver,
    today: &str,
) -> Model {
    let mut thread_rows: Vec<ThreadRow> = threads
        .iter()
        .map(|t| {
            let p = resolver.resolve(t.cwd.as_deref().unwrap_or(""));
            let mut usage = Usage::default();
            let mut cost = Cost::default();
            for models in t.usage.values() {
                for (model, u) in models {
                    usage += *u;
                    cost.add(pricing.cost(model, u));
                }
            }
            ThreadRow {
                live_pane: snap
                    .agents
                    .iter()
                    .find(|a| a.session_id() == Some(t.session_id.as_str()))
                    .map(|a| a.pane_id.clone()),
                project_key: p.key,
                project_name: p.name,
                thread: t.clone(),
                usage,
                cost,
            }
        })
        .collect();
    thread_rows.sort_by_key(|r| Reverse(r.thread.last_ts));

    let mut sorted = records.to_vec();
    sorted.sort_by_key(|r| r.ts);
    let durs = events::durations(&sorted);
    let mut event_rows: Vec<EventRow> = sorted
        .into_iter()
        .zip(durs)
        .map(|(record, duration)| {
            let p = resolver.resolve(record.cwd.as_deref().unwrap_or(""));
            EventRow {
                project_key: p.key,
                project_name: p.name,
                record,
                duration,
            }
        })
        .collect();
    event_rows.reverse();

    let mut projects: HashMap<String, Project> = HashMap::new();
    for pane in &snap.panes {
        if let Some(cwd) = &pane.cwd {
            entry(&mut projects, &resolver.resolve(cwd));
        }
    }
    for a in &snap.agents {
        let p = resolver.resolve(a.cwd.as_deref().unwrap_or(""));
        let workspace = snap
            .workspaces
            .iter()
            .find(|w| w.workspace_id == a.workspace_id)
            .and_then(|w| w.label.clone())
            .unwrap_or_else(|| a.workspace_id.clone());
        let thread = thread_rows
            .iter()
            .position(|r| Some(r.thread.session_id.as_str()) == a.session_id());
        let since = event_rows
            .iter()
            .find(|e| e.record.pane_id == a.pane_id)
            .map(|e| e.record.ts);
        let project = entry(&mut projects, &p);
        project.counts.add(a.agent_status);
        project.agents.push(AgentRow {
            pane: a.clone(),
            workspace,
            worktree: p.worktree.clone(),
            thread,
            since,
        });
    }
    for r in &thread_rows {
        if let Some(project) = projects.get_mut(&r.project_key) {
            project.last_activity = project.last_activity.max(r.thread.last_ts);
            if project.branch.is_none() {
                project.branch = r.thread.branch.clone();
            }
        }
    }
    for e in &event_rows {
        if let Some(project) = projects.get_mut(&e.project_key) {
            project.last_activity = project.last_activity.max(Some(e.record.ts));
        }
    }
    let mut projects: Vec<Project> = projects.into_values().collect();
    for p in &mut projects {
        p.agents.sort_by(|a, b| {
            (a.pane.agent_status, &a.pane.pane_id).cmp(&(b.pane.agent_status, &b.pane.pane_id))
        });
    }
    projects.sort_by(|a, b| {
        (a.urgency(), Reverse(a.last_activity), &a.name).cmp(&(
            b.urgency(),
            Reverse(b.last_activity),
            &b.name,
        ))
    });

    let mut totals = Counts::default();
    for a in &snap.agents {
        totals.add(a.agent_status);
    }
    let today_row = usage(
        &thread_rows,
        pricing,
        None,
        Some(&[today.to_string()]),
        GroupBy::Day,
    )
    .into_iter()
    .next();
    Model {
        projects,
        threads: thread_rows,
        events: event_rows,
        totals,
        today: today_row.as_ref().map(|r| r.usage).unwrap_or_default(),
        today_cost: today_row.map(|r| r.cost).unwrap_or_default(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Day,
    Project,
    Model,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub label: String,
    pub usage: Usage,
    pub cost: Cost,
}

/// Sums thread usage per day, project or model; optionally limited to one project and some days.
pub fn usage(
    rows: &[ThreadRow],
    pricing: &Pricing,
    project: Option<&str>,
    days: Option<&[String]>,
    by: GroupBy,
) -> Vec<UsageRow> {
    let mut acc: BTreeMap<String, UsageRow> = BTreeMap::new();
    for r in rows
        .iter()
        .filter(|r| project.is_none_or(|k| r.project_key == k))
    {
        for (day, models) in &r.thread.usage {
            if days.is_some_and(|d| !d.contains(day)) {
                continue;
            }
            for (model, u) in models {
                let (key, label) = match by {
                    GroupBy::Day => (day.clone(), day.clone()),
                    GroupBy::Project => (r.project_key.clone(), r.project_name.clone()),
                    GroupBy::Model => (model.clone(), model.clone()),
                };
                let row = acc.entry(key).or_insert_with(|| UsageRow {
                    label,
                    usage: Usage::default(),
                    cost: Cost::default(),
                });
                row.usage += *u;
                row.cost.add(pricing.cost(model, u));
            }
        }
    }
    acc.into_values().collect()
}

/// The last `n` local days ending today, oldest first, as `YYYY-MM-DD`.
pub fn last_days(now: DateTime<Utc>, n: usize) -> Vec<String> {
    let today = now.with_timezone(&Local).date_naive();
    (0..n as u64)
        .rev()
        .filter_map(|i| today.checked_sub_days(chrono::Days::new(i)))
        .map(|d| d.format("%Y-%m-%d").to_string())
        .collect()
}

#[cfg(test)]
pub mod testkit {
    use super::*;
    use crate::events::Kind;
    use crate::herdr::{AgentSession, Workspace};
    use std::collections::HashMap;

    pub fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    pub fn pane(id: &str, ws: &str, cwd: &str, status: AgentStatus, session: Option<&str>) -> Pane {
        Pane {
            pane_id: id.into(),
            workspace_id: ws.into(),
            tab_id: format!("{ws}:t1"),
            cwd: Some(cwd.into()),
            agent_status: status,
            agent: Some("claude".into()),
            agent_session: session.map(|s| AgentSession { value: s.into() }),
            terminal_title_stripped: Some(format!("title {id}")),
            focused: false,
        }
    }

    pub fn snapshot() -> Snapshot {
        let alpha = pane(
            "w1:p1",
            "w1",
            "/home/u/alpha",
            AgentStatus::Working,
            Some("sess-a"),
        );
        let beta = pane("w2:p1", "w2", "/home/u/beta", AgentStatus::Blocked, None);
        let mut shell = pane("w3:p1", "w3", "/home/u/gamma", AgentStatus::Unknown, None);
        shell.agent = None;
        Snapshot {
            version: "0.9.3".into(),
            protocol: 22,
            workspaces: vec![
                Workspace {
                    workspace_id: "w1".into(),
                    label: Some("alpha-ws".into()),
                    number: 1,
                },
                Workspace {
                    workspace_id: "w2".into(),
                    label: None,
                    number: 2,
                },
            ],
            panes: vec![alpha.clone(), beta.clone(), shell],
            agents: vec![alpha, beta],
        }
    }

    pub fn threads() -> Vec<Thread> {
        let mut a = Thread {
            session_id: "sess-a".into(),
            title: Some("Parser fix".into()),
            cwd: Some("/home/u/alpha".into()),
            branch: Some("main".into()),
            turns: 3,
            first_ts: Some(ts("2026-10-08T09:00:00Z")),
            last_ts: Some(ts("2026-10-08T11:50:00Z")),
            last_prompt: Some("add tests".into()),
            last_reply: Some("Done.".into()),
            ..Default::default()
        };
        // claude-opus-5-5: 1M input tokens = $4.00
        a.usage.entry("2026-10-08".into()).or_default().insert(
            "claude-opus-5-5".into(),
            Usage {
                input: 1_000_000,
                ..Default::default()
            },
        );
        let mut b = Thread {
            session_id: "sess-old".into(),
            title: Some("Old beta work".into()),
            cwd: Some("/home/u/beta".into()),
            turns: 1,
            first_ts: Some(ts("2026-10-07T08:00:00Z")),
            last_ts: Some(ts("2026-10-07T09:00:00Z")),
            ..Default::default()
        };
        // claude-sonnet-5: 100k output tokens = $1.00
        b.usage.entry("2026-10-07".into()).or_default().insert(
            "claude-sonnet-5".into(),
            Usage {
                output: 100_000,
                ..Default::default()
            },
        );
        vec![a, b]
    }

    pub fn records() -> Vec<Record> {
        let base = |t: &str, pane: &str, ws: &str, cwd: &str, from, to| Record {
            ts: ts(t),
            kind: Kind::Status,
            pane_id: pane.into(),
            workspace_id: ws.into(),
            cwd: Some(cwd.into()),
            agent: Some("claude".into()),
            from: Some(from),
            to: Some(to),
            session_id: None,
            title: None,
        };
        vec![
            base(
                "2026-10-08T11:00:00Z",
                "w1:p1",
                "w1",
                "/home/u/alpha",
                AgentStatus::Idle,
                AgentStatus::Working,
            ),
            base(
                "2026-10-08T11:30:00Z",
                "w2:p1",
                "w2",
                "/home/u/beta",
                AgentStatus::Working,
                AgentStatus::Blocked,
            ),
        ]
    }

    pub fn model() -> Model {
        build(
            &snapshot(),
            &threads(),
            &records(),
            &Pricing::new(HashMap::new()),
            &mut Resolver::default(),
            "2026-10-08",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn projects_sort_by_urgency_then_activity() {
        let m = model();
        let names: Vec<&str> = m.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["beta", "alpha", "gamma"]);
        assert_eq!(m.projects[2].counts.total(), 0);
    }

    #[test]
    fn agents_link_threads_workspaces_and_events() {
        let m = model();
        let alpha = &m.projects[1];
        let a = &alpha.agents[0];
        assert_eq!(a.workspace, "alpha-ws");
        assert_eq!(a.thread, Some(0));
        assert_eq!(a.since, Some(ts("2026-10-08T11:00:00Z")));
        assert_eq!(alpha.branch.as_deref(), Some("main"));
        assert_eq!(alpha.last_activity, Some(ts("2026-10-08T11:50:00Z")));
        assert_eq!(m.projects[0].agents[0].workspace, "w2");
    }

    #[test]
    fn threads_newest_first_with_live_pane() {
        let m = model();
        assert_eq!(m.threads[0].thread.session_id, "sess-a");
        assert_eq!(m.threads[0].live_pane.as_deref(), Some("w1:p1"));
        assert_eq!(m.threads[1].live_pane, None);
        assert!((m.threads[0].cost.usd - 4.0).abs() < 1e-9);
    }

    #[test]
    fn totals_and_today() {
        let m = model();
        assert_eq!(
            m.totals,
            Counts {
                blocked: 1,
                done: 0,
                working: 1,
                idle: 0
            }
        );
        assert_eq!(m.today.input, 1_000_000);
        assert!((m.today_cost.usd - 4.0).abs() < 1e-9);
    }

    #[test]
    fn events_newest_first_with_projects() {
        let m = model();
        assert_eq!(m.events[0].project_name, "beta");
        assert_eq!(m.events[1].project_name, "alpha");
        assert_eq!(m.events[0].duration, None);
    }

    #[test]
    fn usage_groups_and_filters() {
        let m = model();
        let pricing = Pricing::new(HashMap::new());
        let by_model = usage(&m.threads, &pricing, None, None, GroupBy::Model);
        assert_eq!(by_model.len(), 2);
        assert_eq!(by_model[0].label, "claude-opus-5-5");
        assert!((by_model[1].cost.usd - 1.0).abs() < 1e-9);
        let beta_key = m.projects[0].key.clone();
        let beta = usage(
            &m.threads,
            &pricing,
            Some(&beta_key),
            None,
            GroupBy::Project,
        );
        assert_eq!(beta.len(), 1);
        assert_eq!(beta[0].label, "beta");
        let today = usage(
            &m.threads,
            &pricing,
            None,
            Some(&["2026-10-08".to_string()]),
            GroupBy::Day,
        );
        assert_eq!(today.len(), 1);
    }

    #[test]
    fn last_days_end_today() {
        let now = ts("2026-10-08T12:00:00Z");
        let days = last_days(now, 3);
        assert_eq!(days.len(), 3);
        assert_eq!(days[2], crate::transcripts::claude::local_day(now));
        assert!(days[0] < days[1]);
    }

    #[test]
    fn build_tolerates_missing_cwd_and_label() {
        let mut snap = snapshot();
        snap.agents[0].cwd = None;
        snap.workspaces.clear();
        let mut threads = threads();
        threads[1].cwd = None;
        let m = build(
            &snap,
            &threads,
            &[],
            &Pricing::new(HashMap::new()),
            &mut Resolver::default(),
            "2026-10-08",
        );
        assert!(m.projects.iter().any(|p| p.name == "(unknown)"));
        assert!(m
            .projects
            .iter()
            .flat_map(|p| &p.agents)
            .all(|a| a.workspace.starts_with('w')));
    }
}
