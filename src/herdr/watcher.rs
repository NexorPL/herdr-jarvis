//! Keeps one `events.subscribe` connection and turns any event into a fresh, debounced snapshot.
//! A changed pane set means a new connection: herdr 0.9.3 does not acknowledge a second
//! subscribe on the same connection, and `pane.agent_status_changed` needs one entry per pane.

use super::client;
use super::transport;
use super::Snapshot;
use anyhow::bail;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

pub enum WatchMsg {
    Snapshot(Snapshot),
    /// herdr is unreachable; the watcher keeps retrying.
    Offline(String),
    /// herdr speaks another protocol; the watcher has stopped.
    Incompatible(String),
}

const GLOBAL_SUBSCRIPTIONS: &[&str] = &[
    "pane.updated",
    "pane.created",
    "pane.closed",
    "pane.exited",
    "pane.agent_detected",
    // Jarvis shows what changed since the focus last left it.
    "pane.focused",
    "tab.focused",
    "workspace.focused",
    "workspace.created",
    "workspace.closed",
    "worktree.created",
    "worktree.removed",
];

/// Events (normalized names) after which the subscription set must be rebuilt.
const STRUCTURAL: &[&str] = &[
    "pane_created",
    "pane_closed",
    "pane_exited",
    "pane_agent_detected",
    "workspace_created",
    "workspace_closed",
    "worktree_created",
    "worktree_removed",
];

const DEBOUNCE: Duration = Duration::from_millis(150);
const RETRY: Duration = Duration::from_secs(5);

/// The global subscriptions plus one agent-status subscription per agent pane.
pub fn subscriptions(snap: &Snapshot) -> Vec<Value> {
    let mut subs: Vec<Value> = GLOBAL_SUBSCRIPTIONS
        .iter()
        .map(|t| json!({"type": t}))
        .collect();
    subs.extend(
        snap.agents
            .iter()
            .map(|a| json!({"type": "pane.agent_status_changed", "pane_id": a.pane_id})),
    );
    subs
}

/// Event name of one subscription line with `.` normalized to `_`; `None` for acks and garbage.
pub fn event_name(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    Some(v.get("event")?.as_str()?.replace('.', "_"))
}

pub fn is_structural(event: &str) -> bool {
    STRUCTURAL.contains(&event)
}

/// Starts the watcher thread. It sends a snapshot on connect and after every change.
pub fn spawn(path: PathBuf) -> Receiver<WatchMsg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || run(&path, &tx));
    rx
}

fn run(path: &Path, tx: &Sender<WatchMsg>) {
    loop {
        let snap = match client::snapshot(path) {
            Ok(s) => s,
            Err(e) if e.downcast_ref::<client::ProtocolMismatch>().is_some() => {
                let _ = tx.send(WatchMsg::Incompatible(e.to_string()));
                return;
            }
            Err(e) => {
                if tx.send(WatchMsg::Offline(e.to_string())).is_err() {
                    return;
                }
                std::thread::sleep(RETRY);
                continue;
            }
        };
        let subs = subscriptions(&snap);
        if tx.send(WatchMsg::Snapshot(snap)).is_err() {
            return;
        }
        let events = match subscribe(path, subs) {
            Ok(rx) => rx,
            Err(e) => {
                if tx.send(WatchMsg::Offline(e.to_string())).is_err() {
                    return;
                }
                std::thread::sleep(RETRY);
                continue;
            }
        };
        // Re-snapshot on every burst of events until the pane set changes or the stream ends.
        while let Ok(first) = events.recv() {
            let mut structural = is_structural(&first);
            std::thread::sleep(DEBOUNCE);
            for ev in events.try_iter() {
                structural |= is_structural(&ev);
            }
            if structural {
                break;
            }
            match client::snapshot(path) {
                Ok(s) => {
                    if tx.send(WatchMsg::Snapshot(s)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
    }
}

/// Opens a subscription connection and forwards event names from a reader thread.
fn subscribe(path: &Path, subs: Vec<Value>) -> anyhow::Result<Receiver<String>> {
    let mut stream = transport::connect(path)?;
    let line =
        json!({"id": "jarvis-sub", "method": "events.subscribe", "params": {"subscriptions": subs}})
            .to_string() + "\n";
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut ack = String::new();
    loop {
        ack.clear();
        if reader.read_line(&mut ack)? == 0 {
            bail!("herdr closed the subscription");
        }
        if event_name(&ack).is_none() {
            break;
        }
    }
    client::parse_response(&ack)?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // ponytail: a replaced reader exits on its next line, when send fails; pane.updated keeps
        // lines flowing every few seconds, so no cross-platform cancellation of blocking reads is needed.
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some(ev) = event_name(&line) {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        }
    });
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::client::{parse_response, parse_snapshot};
    use std::time::Duration;

    fn fixture() -> Snapshot {
        parse_snapshot(parse_response(include_str!("../../tests/fixtures/snapshot.json")).unwrap())
            .unwrap()
    }

    #[test]
    fn subscribes_globally_and_per_agent_pane() {
        let subs = subscriptions(&fixture());
        assert_eq!(subs.len(), GLOBAL_SUBSCRIPTIONS.len() + 2);
        assert!(subs.contains(&json!({"type": "pane.updated"})));
        assert!(subs.contains(&json!({"type": "pane.focused"})));
        assert!(subs.contains(&json!({"type": "pane.agent_status_changed", "pane_id": "w2:p1"})));
    }

    #[test]
    fn reads_event_names() {
        assert_eq!(
            event_name(r#"{"event":"pane_updated","data":{}}"#).as_deref(),
            Some("pane_updated")
        );
        assert_eq!(
            event_name(r#"{"event":"pane.agent_status_changed","data":{}}"#).as_deref(),
            Some("pane_agent_status_changed")
        );
        assert_eq!(
            event_name(r#"{"id":"jarvis-sub","result":{"type":"subscription_started"}}"#),
            None
        );
        assert_eq!(event_name("not json"), None);
    }

    #[test]
    fn structural_events_change_the_pane_set() {
        assert!(is_structural("pane_created"));
        assert!(is_structural("pane_agent_detected"));
        assert!(!is_structural("pane_updated"));
        assert!(!is_structural("pane_agent_status_changed"));
    }

    #[test]
    fn watcher_reports_offline() {
        let rx = spawn(PathBuf::from("/nonexistent/jarvis-test/herdr.sock"));
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            WatchMsg::Offline(_) => {}
            _ => panic!("expected Offline"),
        }
    }

    #[test]
    #[ignore = "needs a running herdr"]
    fn live_watcher_sends_snapshot() {
        let rx = spawn(crate::herdr::socket_path());
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            WatchMsg::Snapshot(_)
        ));
    }
}
