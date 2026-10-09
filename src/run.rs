//! Run targets (stored per project in `<state_dir>/targets.json`) and the herdr requests that start them.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// One process of a project: a command and the directory (relative to the run root; empty is the root) to run it in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub project_key: String,
    pub name: String,
    pub command: String,
    pub cwd: String,
    /// Edited by hand in `targets.json` only.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One tab per target.
    Tabs,
    /// One tab, targets side by side.
    Split,
}

/// One pane to start: its herdr label, directory, environment and command.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub label: String,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// A new tab whose root pane runs `launch`.
    Tab { label: String, launch: Launch },
    /// A split right of the previous step's pane; the previous pane keeps `ratio` of the width.
    Split { ratio: f32, launch: Launch },
}

/// herdr label of a target's pane; also how Jarvis recognizes a running target.
pub fn pane_label(project: &str, target: &str) -> String {
    format!("{project}:{target}")
}

pub fn plan(project: &str, root: &Path, targets: &[Target], layout: Layout) -> Vec<Step> {
    let n = targets.len();
    (targets.iter().enumerate())
        .map(|(i, t)| {
            let dir = if t.cwd.is_empty() {
                root.to_path_buf()
            } else {
                root.join(&t.cwd)
            };
            let launch = Launch {
                label: pane_label(project, &t.name),
                cwd: dir.to_string_lossy().into_owned(),
                env: t.env.clone(),
                command: t.command.clone(),
            };
            match (layout, i) {
                (Layout::Tabs, _) => Step::Tab {
                    label: t.name.clone(),
                    launch,
                },
                (Layout::Split, 0) => Step::Tab {
                    label: project.to_string(),
                    launch,
                },
                // The previous pane holds the n - i + 1 panes still to place; it keeps one share.
                (Layout::Split, _) => Step::Split {
                    ratio: 1.0 / (n - i + 1) as f32,
                    launch,
                },
            }
        })
        .collect()
}

/// Sends the steps in order through `send` and returns the first new pane. Stops at the first error;
/// panes created before it stay.
pub fn execute(
    workspace_id: Option<&str>,
    steps: &[Step],
    mut send: impl FnMut(&str, Value) -> Result<Value>,
) -> Result<Option<String>> {
    let mut first = None;
    let mut prev: Option<String> = None;
    for step in steps {
        let (method, params, launch) = match step {
            Step::Tab { label, launch } => (
                "tab.create",
                json!({"workspace_id": workspace_id, "label": label, "cwd": launch.cwd, "env": launch.env}),
                launch,
            ),
            Step::Split { ratio, launch } => (
                "pane.split",
                json!({"target_pane_id": prev, "direction": "right", "ratio": ratio,
                       "cwd": launch.cwd, "env": launch.env}),
                launch,
            ),
        };
        let result = send(method, params)?;
        let pane = (result.get("root_pane").or_else(|| result.get("pane")))
            .and_then(|p| p["pane_id"].as_str())
            .ok_or_else(|| anyhow!("{method}: herdr returned no pane"))?
            .to_string();
        send(
            "pane.rename",
            json!({"pane_id": pane, "label": launch.label}),
        )?;
        send(
            "pane.send_input",
            json!({"pane_id": pane, "text": launch.command, "keys": ["Enter"]}),
        )?;
        first.get_or_insert_with(|| pane.clone());
        prev = Some(pane);
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<Target> {
        let target = |name: &str, command: &str, cwd: &str| Target {
            project_key: "/home/u/alpha".into(),
            name: name.into(),
            command: command.into(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
        };
        let mut api = target("api", "cargo run", "services/api");
        api.env.insert("PORT".into(), "3001".into());
        vec![api, target("web", "pnpm dev", "")]
    }

    #[test]
    fn plan_tabs_gives_each_target_its_own_tab() {
        let root = Path::new("/r");
        let steps = plan("alpha", root, &targets(), Layout::Tabs);
        assert_eq!(
            steps,
            vec![
                Step::Tab {
                    label: "api".into(),
                    launch: Launch {
                        label: "alpha:api".into(),
                        cwd: root.join("services/api").to_string_lossy().into_owned(),
                        env: targets()[0].env.clone(),
                        command: "cargo run".into(),
                    },
                },
                Step::Tab {
                    label: "web".into(),
                    launch: Launch {
                        label: "alpha:web".into(),
                        cwd: "/r".into(),
                        env: BTreeMap::new(),
                        command: "pnpm dev".into(),
                    },
                },
            ]
        );
    }

    #[test]
    fn plan_split_keeps_equal_widths() {
        let mut three = targets();
        three.push(three[1].clone());
        let steps = plan("alpha", Path::new("/r"), &three, Layout::Split);
        assert!(matches!(&steps[0], Step::Tab { label, .. } if label == "alpha"));
        let ratios: Vec<f32> = steps[1..]
            .iter()
            .map(|s| match s {
                Step::Split { ratio, .. } => *ratio,
                Step::Tab { .. } => panic!("expected a split"),
            })
            .collect();
        assert_eq!(ratios, vec![1.0 / 3.0, 1.0 / 2.0]);
    }

    #[test]
    fn execute_creates_splits_names_and_starts_each_pane() {
        let steps = plan("alpha", Path::new("/r"), &targets(), Layout::Split);
        let mut calls: Vec<(String, Value)> = Vec::new();
        let first = execute(Some("w1"), &steps, |method, params| {
            calls.push((method.to_string(), params));
            Ok(match method {
                "tab.create" => json!({"type": "tab_created", "root_pane": {"pane_id": "w1:p9"}}),
                "pane.split" => json!({"type": "pane_info", "pane": {"pane_id": "w1:p10"}}),
                _ => json!({"type": "ok"}),
            })
        })
        .unwrap();
        assert_eq!(first.as_deref(), Some("w1:p9"));
        let methods: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            [
                "tab.create",
                "pane.rename",
                "pane.send_input",
                "pane.split",
                "pane.rename",
                "pane.send_input"
            ]
        );
        assert_eq!(calls[0].1["workspace_id"], "w1");
        assert_eq!(calls[0].1["env"]["PORT"], "3001");
        assert_eq!(
            calls[1].1,
            json!({"pane_id": "w1:p9", "label": "alpha:api"})
        );
        assert_eq!(
            calls[2].1,
            json!({"pane_id": "w1:p9", "text": "cargo run", "keys": ["Enter"]})
        );
        assert_eq!(calls[3].1["target_pane_id"], "w1:p9");
        assert_eq!(calls[3].1["direction"], "right");
        assert_eq!(calls[5].1["pane_id"], "w1:p10");
    }

    #[test]
    fn execute_stops_at_the_first_failure() {
        let steps = plan("alpha", Path::new("/r"), &targets(), Layout::Tabs);
        let mut sent = 0;
        let err = execute(None, &steps, |_, _| {
            sent += 1;
            Err(anyhow!("herdr not_found: no workspace"))
        })
        .unwrap_err();
        assert!(err.to_string().contains("no workspace"));
        assert_eq!(sent, 1);
    }
}
