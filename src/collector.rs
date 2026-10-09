//! `jarvis collect` records the timeline while the overlay is closed.
//! herdr's `[[startup]]` hook is one-shot, so `ensure-collector` spawns it detached and exits.

use crate::config::Config;
use crate::herdr::{self, WatchMsg};
use crate::log::log;
use crate::{events, paths};
use serde_json::json;
use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const GIVE_UP_AFTER: Duration = Duration::from_secs(600);

pub fn run() -> anyhow::Result<()> {
    let state = paths::state_dir();
    std::fs::create_dir_all(&state)?;
    let Some(_lock) = lock_with_retry(&state.join("collector.lock"))? else {
        return Ok(()); // another collector is running
    };
    let config = Config::load(&paths::config_dir());
    let store = events::Log::new(state);
    store.prune(config.retention_days, chrono::Utc::now().date_naive());
    let mut prev: Option<herdr::Snapshot> = None;
    let mut offline_since: Option<Instant> = None;
    for msg in herdr::watcher::spawn(herdr::socket_path()) {
        match msg {
            WatchMsg::Snapshot(next) => {
                offline_since = None;
                if let Some(p) = &prev {
                    let records = events::diff(p, &next, chrono::Utc::now());
                    if let Err(e) = store.append(&records) {
                        log(format!("collector: append failed: {e}"));
                    }
                }
                prev = Some(next);
            }
            WatchMsg::Offline(reason) => {
                if offline_since.get_or_insert_with(Instant::now).elapsed() > GIVE_UP_AFTER {
                    log(format!(
                        "collector: herdr unreachable for 10 minutes ({reason}); exiting"
                    ));
                    break;
                }
            }
            WatchMsg::Incompatible(reason) => {
                log(format!("collector: {reason}"));
                break;
            }
        }
    }
    Ok(())
}

/// `jarvis ensure-collector`, run by herdr's `[[startup]]`: the collector, then the Jarvis tab.
pub fn ensure_running() -> anyhow::Result<()> {
    ensure_collector()?;
    ensure_tui(&paths::state_dir())
}

/// Starts a detached collector unless one already holds the lock.
pub fn ensure_collector() -> anyhow::Result<()> {
    let state = paths::state_dir();
    std::fs::create_dir_all(&state)?;
    if !is_running(&state.join("collector.lock")) {
        spawn_detached(&["collect"])?;
    }
    Ok(())
}

/// herdr restores a session's layout but not plugin processes, so after a restart the "Jarvis" tab
/// holds a plain shell. Unless a Jarvis is running, replace those leftovers with a live one, in the
/// same workspace and without taking the focus.
fn ensure_tui(state: &Path) -> anyhow::Result<()> {
    if is_running(&state.join("tui.lock")) {
        return Ok(());
    }
    let socket = herdr::socket_path();
    let snap = herdr::client::snapshot(&socket)?;
    let stale = stale_jarvis_panes(&snap);
    for pane in &stale {
        herdr::client::request(&socket, "pane.close", json!({"pane_id": pane.pane_id}))?;
    }
    let mut open = json!({"plugin_id": "jarvis", "entrypoint": "core", "focus": false});
    if let Some(pane) = stale.first() {
        open["workspace_id"] = json!(pane.workspace_id);
    }
    herdr::client::request(&socket, "plugin.pane.open", open)?;
    // herdr activates the new tab shortly after the request returns, even with focus=false; give
    // the focus back from a detached process once that has happened.
    if let Some(focused) = snap
        .focused_pane_id
        .as_deref()
        .filter(|f| stale.iter().all(|p| p.pane_id != *f))
    {
        spawn_detached(&["focus", focused])?;
    }
    Ok(())
}

/// Panes left over from an earlier Jarvis: herdr labels plugin panes with the manifest title.
pub fn stale_jarvis_panes(snap: &herdr::Snapshot) -> Vec<&herdr::Pane> {
    snap.panes
        .iter()
        .filter(|p| p.label.as_deref() == Some("Jarvis"))
        .collect()
}

/// Holds an exclusive lock for the life of the returned file; `None` when another process holds it.
pub fn try_lock(path: &Path) -> std::io::Result<Option<File>> {
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

/// A starting collector retries for ~1 s, so an instant `is_running` probe from the TUI cannot
/// make it give up; a real running collector still holds the lock after that.
fn lock_with_retry(path: &Path) -> std::io::Result<Option<File>> {
    for _ in 0..10 {
        if let Some(lock) = try_lock(path)? {
            return Ok(Some(lock));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(None)
}

/// Probes by taking the lock for an instant; `lock_with_retry` makes that safe for a starting collector.
pub fn is_running(lock: &Path) -> bool {
    matches!(try_lock(lock), Ok(None))
}

/// Runs this executable with `args`, detached from the caller's terminal and process group.
pub fn spawn_detached(args: &[&str]) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let command = || {
        let mut c = Command::new(&exe);
        // Not the plugin root as cwd: a long-lived process would keep that directory in use.
        c.args(args)
            .current_dir(paths::state_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // Breaking away from herdr's job object keeps the collector alive; not every job allows it.
        if command()
            .creation_flags(base | CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
            .is_err()
        {
            command().creation_flags(base).spawn()?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command().process_group(0).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_exclusive_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collector.lock");
        let held = try_lock(&path).unwrap();
        assert!(held.is_some());
        assert!(try_lock(&path).unwrap().is_none());
        assert!(is_running(&path));
        drop(held);
        assert!(!is_running(&path));
        assert!(try_lock(&path).unwrap().is_some());
    }

    #[test]
    fn restored_jarvis_panes_are_stale() {
        let snap: herdr::Snapshot = serde_json::from_value(serde_json::json!({
            "protocol": 22,
            "panes": [
                {"pane_id": "w8:pA", "workspace_id": "w8", "tab_id": "w8:t4", "label": "Jarvis", "agent_status": "unknown"},
                {"pane_id": "w8:pD", "workspace_id": "w8", "tab_id": "w8:t1", "label": "Sidebar", "agent_status": "unknown"},
                {"pane_id": "w8:p1", "workspace_id": "w8", "tab_id": "w8:t1", "agent_status": "working"}
            ]
        }))
        .unwrap();
        let stale = stale_jarvis_panes(&snap);
        assert_eq!(stale.len(), 1);
        assert_eq!(
            (stale[0].pane_id.as_str(), stale[0].workspace_id.as_str()),
            ("w8:pA", "w8")
        );
    }

    #[test]
    fn startup_lock_survives_a_brief_probe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collector.lock");
        let probe = try_lock(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(probe);
        });
        assert!(lock_with_retry(&path).unwrap().is_some());
        release.join().unwrap();
    }
}
