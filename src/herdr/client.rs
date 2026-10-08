use super::transport;
use super::types::Snapshot;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Socket protocol this build speaks.
pub const PROTOCOL: u32 = 22;

/// herdr speaks a different protocol; callers stop instead of retrying.
#[derive(Debug)]
pub struct ProtocolMismatch(pub String);

impl std::fmt::Display for ProtocolMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolMismatch {}

/// Sends one request on its own connection and returns the response's `result`.
pub fn request(path: &Path, method: &str, params: Value) -> Result<Value> {
    let mut stream =
        transport::connect(path).with_context(|| format!("connect {}", path.display()))?;
    let line = json!({"id": "jarvis", "method": method, "params": params}).to_string() + "\n";
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response)?;
    parse_response(&response)
}

pub fn parse_response(line: &str) -> Result<Value> {
    let mut v: Value = serde_json::from_str(line).context("invalid herdr response")?;
    if let Some(err) = v.get("error") {
        bail!(
            "herdr {}: {}",
            err["code"].as_str().unwrap_or("error"),
            err["message"].as_str().unwrap_or("")
        );
    }
    v.get_mut("result")
        .map(Value::take)
        .ok_or_else(|| anyhow!("herdr response without result"))
}

pub fn parse_snapshot(result: Value) -> Result<Snapshot> {
    let snap: Snapshot =
        serde_json::from_value(result.get("snapshot").cloned().unwrap_or(Value::Null))
            .context("invalid herdr snapshot")?;
    if snap.protocol != PROTOCOL {
        return Err(ProtocolMismatch(format!(
            "herdr {} speaks protocol {}; Jarvis needs protocol {PROTOCOL}",
            snap.version, snap.protocol
        ))
        .into());
    }
    Ok(snap)
}

pub fn snapshot(path: &Path) -> Result<Snapshot> {
    parse_snapshot(request(path, "session.snapshot", json!({}))?)
}

pub fn focus_pane(path: &Path, pane_id: &str) -> Result<()> {
    request(path, "pane.focus", json!({"pane_id": pane_id})).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::AgentStatus;

    const FIXTURE: &str = include_str!("../../tests/fixtures/snapshot.json");

    #[test]
    fn parses_recorded_snapshot() {
        let snap = parse_snapshot(parse_response(FIXTURE).unwrap()).unwrap();
        assert_eq!(snap.protocol, 22);
        assert_eq!(snap.workspaces.len(), 2);
        assert_eq!(snap.panes.len(), 3);
        assert_eq!(snap.agents.len(), 2);
        let a = &snap.agents[0];
        assert_eq!(a.session_id(), Some("sess-a"));
        assert_eq!(a.title(), "Fix parser");
        assert_eq!(snap.agents[1].agent_status, AgentStatus::Blocked);
        assert_eq!(snap.panes[1].agent, None);
    }

    #[test]
    fn error_response_becomes_error() {
        let err = parse_response(r#"{"id":"x","error":{"code":"not_found","message":"no pane"}}"#)
            .unwrap_err();
        assert!(err.to_string().contains("not_found"));
    }

    #[test]
    fn other_protocol_is_rejected() {
        let result = serde_json::json!({"snapshot": {"version": "1.2.0", "protocol": 23}});
        let err = parse_snapshot(result).unwrap_err();
        assert!(err.downcast_ref::<ProtocolMismatch>().is_some());
    }

    #[test]
    fn unknown_status_maps_to_unknown() {
        let s: AgentStatus = serde_json::from_str(r#""napping""#).unwrap();
        assert_eq!(s, AgentStatus::Unknown);
    }

    #[test]
    fn status_order_is_urgency() {
        assert!(AgentStatus::Blocked < AgentStatus::Done);
        assert!(AgentStatus::Done < AgentStatus::Working);
        assert!(AgentStatus::Working < AgentStatus::Idle);
    }

    #[test]
    #[ignore = "needs a running herdr"]
    fn live_snapshot() {
        let snap = snapshot(&crate::herdr::socket_path()).unwrap();
        assert_eq!(snap.protocol, PROTOCOL);
    }
}
