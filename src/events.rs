//! Timeline records: derived by diffing consecutive snapshots, stored as daily JSONL files.

use crate::herdr::{AgentStatus, Pane, Snapshot};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Status,
    AgentAppeared,
    AgentGone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub ts: DateTime<Utc>,
    pub kind: Kind,
    pub pane_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<AgentStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<AgentStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

fn record(
    ts: DateTime<Utc>,
    kind: Kind,
    a: &Pane,
    from: Option<AgentStatus>,
    to: Option<AgentStatus>,
) -> Record {
    Record {
        ts,
        kind,
        pane_id: a.pane_id.clone(),
        workspace_id: a.workspace_id.clone(),
        cwd: a.cwd.clone(),
        agent: a.agent.clone(),
        from,
        to,
        session_id: a.session_id().map(str::to_string),
        title: a.terminal_title_stripped.clone(),
    }
}

/// Records implied by going from `prev` to `next`: status changes and appeared agents in `next`
/// order, then agents that disappeared.
pub fn diff(prev: &Snapshot, next: &Snapshot, now: DateTime<Utc>) -> Vec<Record> {
    let before: HashMap<&str, &Pane> = prev
        .agents
        .iter()
        .map(|a| (a.pane_id.as_str(), a))
        .collect();
    let after: HashMap<&str, &Pane> = next
        .agents
        .iter()
        .map(|a| (a.pane_id.as_str(), a))
        .collect();
    let mut out = Vec::new();
    for a in &next.agents {
        match before.get(a.pane_id.as_str()) {
            None => out.push(record(
                now,
                Kind::AgentAppeared,
                a,
                None,
                Some(a.agent_status),
            )),
            Some(b) if b.agent_status != a.agent_status => out.push(record(
                now,
                Kind::Status,
                a,
                Some(b.agent_status),
                Some(a.agent_status),
            )),
            Some(_) => {}
        }
    }
    for b in &prev.agents {
        if !after.contains_key(b.pane_id.as_str()) {
            out.push(record(now, Kind::AgentGone, b, Some(b.agent_status), None));
        }
    }
    out
}

/// For records sorted by time: how long since the previous record of the same pane.
pub fn durations(records: &[Record]) -> Vec<Option<chrono::Duration>> {
    let mut last: HashMap<&str, DateTime<Utc>> = HashMap::new();
    records
        .iter()
        .map(|r| {
            let d = last.get(r.pane_id.as_str()).map(|t| r.ts - *t);
            last.insert(r.pane_id.as_str(), r.ts);
            d
        })
        .collect()
}

/// Daily files `events-YYYY-MM-DD.jsonl` (UTC days) in one directory.
pub struct Log {
    dir: PathBuf,
}

impl Log {
    pub fn new(dir: PathBuf) -> Log {
        Log { dir }
    }

    fn path(&self, day: NaiveDate) -> PathBuf {
        self.dir.join(format!("events-{day}.jsonl"))
    }

    pub fn append(&self, records: &[Record]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        for r in records {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.path(r.ts.date_naive()))?;
            writeln!(f, "{}", serde_json::to_string(r)?)?;
        }
        Ok(())
    }

    /// Records with `ts >= since`, oldest first; unreadable lines are skipped.
    pub fn read_since(&self, since: DateTime<Utc>, now: DateTime<Utc>) -> Vec<Record> {
        let mut out = Vec::new();
        let mut day = since.date_naive();
        while day <= now.date_naive() {
            if let Ok(text) = std::fs::read_to_string(self.path(day)) {
                out.extend(
                    text.lines()
                        .filter_map(|l| serde_json::from_str::<Record>(l).ok())
                        .filter(|r| r.ts >= since),
                );
            }
            let Some(next) = day.succ_opt() else { break };
            day = next;
        }
        out.sort_by_key(|r| r.ts);
        out
    }

    /// Deletes event files older than `retention_days`; other files are left alone.
    pub fn prune(&self, retention_days: u32, today: NaiveDate) {
        let Some(cutoff) = today.checked_sub_days(chrono::Days::new(retention_days.into())) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(date) = name
                .strip_prefix("events-")
                .and_then(|n| n.strip_suffix(".jsonl"))
            else {
                continue;
            };
            if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok_and(|d| d < cutoff) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::{AgentSession, AgentStatus::*};

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn agent(id: &str, status: AgentStatus) -> Pane {
        Pane {
            pane_id: id.into(),
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            cwd: Some("/home/u/alpha".into()),
            agent_status: status,
            agent: Some("claude".into()),
            agent_session: Some(AgentSession {
                value: format!("sess-{id}"),
            }),
            terminal_title_stripped: Some("Fix parser".into()),
            focused: false,
        }
    }

    fn snap(agents: Vec<Pane>) -> Snapshot {
        Snapshot {
            protocol: 22,
            agents,
            ..Default::default()
        }
    }

    fn rec(t: &str, pane: &str, kind: Kind) -> Record {
        Record {
            ts: ts(t),
            kind,
            pane_id: pane.into(),
            workspace_id: "w1".into(),
            cwd: None,
            agent: None,
            from: None,
            to: None,
            session_id: None,
            title: None,
        }
    }

    #[test]
    fn diff_reports_status_changes_appear_and_gone() {
        let now = ts("2026-10-08T12:00:00Z");
        let prev = snap(vec![agent("p1", Working), agent("p2", Idle)]);
        let next = snap(vec![agent("p1", Done), agent("p3", Working)]);
        let recs = diff(&prev, &next, now);
        assert_eq!(recs.len(), 3);
        assert_eq!(
            (
                recs[0].kind,
                recs[0].pane_id.as_str(),
                recs[0].from,
                recs[0].to
            ),
            (Kind::Status, "p1", Some(Working), Some(Done))
        );
        assert_eq!(
            (recs[1].kind, recs[1].pane_id.as_str()),
            (Kind::AgentAppeared, "p3")
        );
        assert_eq!(
            (recs[2].kind, recs[2].pane_id.as_str(), recs[2].to),
            (Kind::AgentGone, "p2", None)
        );
        assert_eq!(recs[0].session_id.as_deref(), Some("sess-p1"));
        assert_eq!(recs[0].cwd.as_deref(), Some("/home/u/alpha"));
    }

    #[test]
    fn diff_of_identical_snapshots_is_empty() {
        let s = snap(vec![agent("p1", Working)]);
        assert!(diff(&s, &s, ts("2026-10-08T12:00:00Z")).is_empty());
    }

    #[test]
    fn log_round_trips_across_days() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::new(dir.path().to_path_buf());
        let a = rec("2026-10-07T23:59:00Z", "p1", Kind::AgentAppeared);
        let b = rec("2026-10-08T00:01:00Z", "p1", Kind::Status);
        log.append(&[b.clone(), a.clone()]).unwrap();
        assert!(dir.path().join("events-2026-10-07.jsonl").exists());
        assert!(dir.path().join("events-2026-10-08.jsonl").exists());
        let back = log.read_since(ts("2026-10-07T00:00:00Z"), ts("2026-10-08T12:00:00Z"));
        assert_eq!(back, vec![a, b.clone()]);
        let recent = log.read_since(ts("2026-10-08T00:00:00Z"), ts("2026-10-08T12:00:00Z"));
        assert_eq!(recent, vec![b]);
    }

    #[test]
    fn read_skips_corrupt_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::new(dir.path().to_path_buf());
        log.append(&[rec("2026-10-08T10:00:00Z", "p1", Kind::Status)])
            .unwrap();
        let path = dir.path().join("events-2026-10-08.jsonl");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{broken\n");
        std::fs::write(&path, text).unwrap();
        assert_eq!(
            log.read_since(ts("2026-10-08T00:00:00Z"), ts("2026-10-08T12:00:00Z"))
                .len(),
            1
        );
    }

    #[test]
    fn prune_removes_old_files_only() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "events-2026-09-01.jsonl",
            "events-2026-10-07.jsonl",
            "jarvis.log",
            "events-garbage.jsonl",
        ] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        Log::new(dir.path().to_path_buf()).prune(30, "2026-10-08".parse().unwrap());
        assert!(!dir.path().join("events-2026-09-01.jsonl").exists());
        assert!(dir.path().join("events-2026-10-07.jsonl").exists());
        assert!(dir.path().join("jarvis.log").exists());
        assert!(dir.path().join("events-garbage.jsonl").exists());
    }

    #[test]
    fn durations_measure_time_since_previous_record_of_same_pane() {
        let recs = vec![
            rec("2026-10-08T10:00:00Z", "p1", Kind::Status),
            rec("2026-10-08T10:01:00Z", "p2", Kind::Status),
            rec("2026-10-08T10:04:12Z", "p1", Kind::Status),
        ];
        let d = durations(&recs);
        assert_eq!(d[0], None);
        assert_eq!(d[1], None);
        assert_eq!(d[2], Some(chrono::Duration::seconds(252)));
    }
}
