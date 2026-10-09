//! UI state and key handling. Pure: no terminal, no herdr; side effects are requested via `action`.

use super::theme::Palette;
use crate::config::{Animation, Config};
use crate::herdr::AgentStatus;
use crate::ideas::Idea;
use crate::model::{AgentRow, EventRow, Model, Project, ThreadRow};
use crate::pricing::Pricing;
use crate::run::{self, Layout, Step, Target};
use chrono::{DateTime, Duration, Local, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::path::PathBuf;

/// Boot animation length in 33 ms ticks (~0.8 s).
pub const BOOT_TICKS: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Agents,
    Threads,
    Timeline,
    Usage,
    Ideas,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Agents,
        Tab::Threads,
        Tab::Timeline,
        Tab::Usage,
        Tab::Ideas,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Agents => "Agents",
            Tab::Threads => "Threads",
            Tab::Timeline => "Timeline",
            Tab::Usage => "Usage",
            Tab::Ideas => "Ideas",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Core,
    Project { key: String, tab: Tab },
    Global(Tab),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Hour,
    Today,
    Week,
}

impl Range {
    fn next(self) -> Range {
        match self {
            Range::Hour => Range::Today,
            Range::Today => Range::Week,
            Range::Week => Range::Hour,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Range::Hour => "1h",
            Range::Today => "today",
            Range::Week => "7d",
        }
    }

    pub fn since(self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Range::Hour => now - Duration::hours(1),
            Range::Week => now - Duration::days(7),
            Range::Today => now
                .with_timezone(&Local)
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|d| d.and_local_timezone(Local).earliest())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(now - Duration::days(1)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    FocusPane(String),
    Copy(String),
    /// `ideas` changed; write them to disk.
    SaveIdeas,
    /// `targets` changed; write them to disk.
    SaveTargets,
    /// Send these steps to herdr, then focus the first new pane.
    Run {
        workspace_id: Option<String>,
        steps: Vec<Step>,
    },
}

/// What a form saves on Enter. `editing` indexes the list it edits; `None` adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormKind {
    Idea {
        editing: Option<usize>,
        project_key: String,
        project_name: String,
    },
    Target {
        editing: Option<usize>,
        project_key: String,
    },
}

/// A popup of labelled single-line fields; while open it takes every key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub title: String,
    pub kind: FormKind,
    pub fields: Vec<(&'static str, String)>,
    pub focus: usize,
}

/// What the delete popup removes, by index into its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doomed {
    Idea(usize),
    Target(usize),
}

/// The delete popup; while open it takes every key. `yes` is the selected button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub what: Doomed,
    pub name: String,
    pub yes: bool,
}

/// Where `x` starts targets: the project (key and name), the agent's workspace and the run root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    pub project_key: String,
    pub project: String,
    pub workspace_id: Option<String>,
    pub root: PathBuf,
}

/// The run-target popup; while open it takes every key. Lists `App::picker_targets`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub ctx: RunContext,
    /// One per row of `App::picker_targets`.
    pub chosen: Vec<bool>,
    pub cursor: usize,
    pub choosing_layout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Project(usize),
    /// Overflow node with the number of projects it stands for.
    More(usize),
}

/// Angle of node `i` of `n`: first at the top, then clockwise.
pub fn node_angle(i: usize, n: usize) -> f64 {
    FRAC_PI_2 - TAU * i as f64 / n.max(1) as f64
}

/// The node nearest to `from` in direction (`dx`, `dy`), preferring well-aligned ones.
pub fn nearest(n: usize, from: usize, dx: f64, dy: f64) -> Option<usize> {
    let pos = |i: usize| {
        let a = node_angle(i, n);
        (a.cos(), a.sin())
    };
    let (fx, fy) = pos(from);
    (0..n)
        .filter(|&j| j != from)
        .filter_map(|j| {
            let (x, y) = pos(j);
            let (vx, vy) = (x - fx, y - fy);
            let dot = vx * dx + vy * dy;
            let d2 = vx * vx + vy * vy;
            (dot > 1e-9).then(|| (j, d2.powf(1.5) / (dot * dot)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(j, _)| j)
}

pub struct App {
    pub screen: Screen,
    pub model: Model,
    pub pricing: Pricing,
    pub palette: Palette,
    pub now: DateTime<Utc>,
    pub tick: u64,
    pub boot_done: bool,
    pub animation: Animation,
    pub max_branches: usize,
    pub compact: bool,
    pub selected_node: usize,
    pub selected_row: usize,
    pub search: String,
    pub search_editing: bool,
    pub state_filter: Option<AgentStatus>,
    pub range: Range,
    pub project_filter: Option<String>,
    pub show_help: bool,
    pub status: Option<String>,
    pub offline: Option<String>,
    pub fatal: Option<String>,
    pub collector_running: bool,
    pub quit: bool,
    pub refresh: bool,
    pub action: Option<Action>,
    /// Whether herdr's focus is on Jarvis's own pane.
    pub looking: bool,
    /// When the focus last left Jarvis; blocked/done changes after it are new to you.
    pub last_left: DateTime<Utc>,
    pub ideas: Vec<Idea>,
    pub form: Option<Form>,
    pub confirm: Option<Confirm>,
    pub picker: Option<Picker>,
    pub targets: Vec<Target>,
}

impl App {
    pub fn new(config: &Config, pricing: Pricing) -> App {
        App {
            screen: Screen::Core,
            model: Model::default(),
            pricing,
            palette: Palette::detect(),
            now: Utc::now(),
            tick: 0,
            boot_done: config.animation == Animation::Off,
            animation: config.animation,
            max_branches: config.max_branches.max(2),
            compact: false,
            selected_node: 0,
            selected_row: 0,
            search: String::new(),
            search_editing: false,
            state_filter: None,
            range: Range::Today,
            project_filter: None,
            show_help: false,
            status: None,
            offline: None,
            fatal: None,
            collector_running: true,
            quit: false,
            refresh: false,
            action: None,
            looking: false,
            last_left: Utc::now(),
            ideas: Vec::new(),
            form: None,
            confirm: None,
            picker: None,
            targets: Vec::new(),
        }
    }

    /// Blocked or done since the focus last left Jarvis, so not seen yet.
    pub fn is_new(&self, a: &AgentRow) -> bool {
        matches!(
            a.pane.agent_status,
            AgentStatus::Blocked | AgentStatus::Done
        ) && a.since.is_some_and(|t| t > self.last_left)
    }

    pub fn new_in(&self, p: &Project) -> usize {
        p.agents.iter().filter(|a| self.is_new(a)).count()
    }

    /// Leaving Jarvis marks everything it showed as seen.
    pub fn set_looking(&mut self, looking: bool) {
        if self.looking && !looking {
            self.last_left = self.now;
        }
        self.looking = looking;
    }

    /// Label for Jarvis's tab: every blocked agent, plus done ones not seen yet.
    pub fn tab_label(&self) -> String {
        let new_done = (self.model.projects.iter().flat_map(|p| &p.agents))
            .filter(|a| a.pane.agent_status == AgentStatus::Done && self.is_new(a))
            .count();
        let mut label = String::from("Jarvis");
        if self.model.totals.blocked > 0 {
            label += &format!(" ▲{}", self.model.totals.blocked);
        }
        if new_done > 0 {
            label += &format!(" ✓{new_done}");
        }
        label
    }

    /// Core-view nodes: one per project, the overflow folded into a final `More` node.
    pub fn nodes(&self) -> Vec<Node> {
        let n = self.model.projects.len();
        if n <= self.max_branches {
            return (0..n).map(Node::Project).collect();
        }
        let shown = self.max_branches - 1;
        (0..shown)
            .map(Node::Project)
            .chain([Node::More(n - shown)])
            .collect()
    }

    pub fn boot_progress(&self) -> f64 {
        if self.boot_done {
            1.0
        } else {
            (self.tick as f64 / BOOT_TICKS as f64).min(1.0)
        }
    }

    /// Animation clock; frozen at 0 when animation is off.
    pub fn anim_tick(&self) -> u64 {
        if self.animation == Animation::Off {
            0
        } else {
            self.tick
        }
    }

    pub fn animating(&self) -> bool {
        !self.boot_done
            || (self.animation != Animation::Off && self.screen == Screen::Core && !self.compact)
    }

    pub fn tab(&self) -> Tab {
        match &self.screen {
            Screen::Project { tab, .. } | Screen::Global(tab) => *tab,
            Screen::Core => Tab::Agents,
        }
    }

    /// Project key the list views are limited to.
    pub fn scope(&self) -> Option<&str> {
        match &self.screen {
            Screen::Project { key, .. } => Some(key),
            Screen::Global(_) => self.project_filter.as_deref(),
            Screen::Core => None,
        }
    }

    pub fn today(&self) -> String {
        crate::transcripts::claude::local_day(self.now)
    }

    pub fn project_name(&self, key: &str) -> String {
        self.model
            .projects
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| key.to_string())
    }

    pub fn visible_agents(&self) -> Vec<(&Project, &AgentRow)> {
        let scope = self.scope();
        self.model
            .projects
            .iter()
            .filter(|p| scope.is_none_or(|k| p.key == k))
            .flat_map(|p| p.agents.iter().map(move |a| (p, a)))
            .filter(|(_, a)| self.state_filter.is_none_or(|s| a.pane.agent_status == s))
            .collect()
    }

    pub fn visible_threads(&self) -> Vec<&ThreadRow> {
        let scope = self.scope();
        let q = self.search.to_lowercase();
        self.model
            .threads
            .iter()
            .filter(|t| scope.is_none_or(|k| t.project_key == k))
            .filter(|t| {
                q.is_empty()
                    || t.thread
                        .title
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || t.project_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn visible_events(&self) -> Vec<&EventRow> {
        let scope = self.scope();
        let since = self.range.since(self.now);
        self.model
            .events
            .iter()
            .filter(|e| e.record.ts >= since)
            .filter(|e| scope.is_none_or(|k| e.project_key == k))
            .filter(|e| self.state_filter.is_none_or(|s| e.record.to == Some(s)))
            .collect()
    }

    /// Ideas in scope that match the search, with their index into `ideas`.
    pub fn visible_ideas(&self) -> Vec<(usize, &Idea)> {
        let scope = self.scope();
        let q = self.search.to_lowercase();
        self.ideas
            .iter()
            .enumerate()
            .filter(|(_, i)| scope.is_none_or(|k| i.project_key == k))
            .filter(|(_, i)| {
                q.is_empty()
                    || i.name.to_lowercase().contains(&q)
                    || i.description.to_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn row_count(&self) -> usize {
        match self.tab() {
            Tab::Agents => self.visible_agents().len(),
            Tab::Threads => self.visible_threads().len(),
            Tab::Timeline => self.visible_events().len(),
            Tab::Usage => 0,
            Tab::Ideas => self.visible_ideas().len(),
        }
    }

    fn node_count(&self) -> usize {
        if self.compact {
            self.model.projects.len()
        } else {
            self.nodes().len()
        }
    }

    /// Keeps selections valid after the model changed.
    pub fn clamp(&mut self) {
        self.selected_node = self.selected_node.min(self.node_count().saturating_sub(1));
        if self.screen != Screen::Core {
            self.selected_row = self.selected_row.min(self.row_count().saturating_sub(1));
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.fatal.is_some() {
            self.quit = true;
            return;
        }
        if !self.boot_done {
            self.boot_done = true;
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }
        if self.form.is_some() {
            self.on_form_key(key);
            return;
        }
        if self.confirm.is_some() {
            self.on_confirm_key(key);
            return;
        }
        if self.picker.is_some() {
            self.on_picker_key(key);
            return;
        }
        if self.search_editing {
            self.on_search_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('r') => self.refresh = true,
            KeyCode::Char('A') => self.goto(Screen::Global(Tab::Agents)),
            KeyCode::Char('T') => self.goto(Screen::Global(Tab::Threads)),
            KeyCode::Char('L') => self.goto(Screen::Global(Tab::Timeline)),
            KeyCode::Char('U') => self.goto(Screen::Global(Tab::Usage)),
            KeyCode::Char('I') => self.goto(Screen::Global(Tab::Ideas)),
            _ if self.screen == Screen::Core => self.on_core_key(key),
            _ => self.on_list_key(key),
        }
    }

    fn goto(&mut self, screen: Screen) {
        self.screen = screen;
        self.selected_row = 0;
        self.search.clear();
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.search_editing = false,
            KeyCode::Esc => {
                self.search_editing = false;
                self.search.clear();
            }
            KeyCode::Backspace => {
                self.search.pop();
            }
            KeyCode::Char(c) => self.search.push(c),
            _ => {}
        }
        self.selected_row = 0;
    }

    fn on_core_key(&mut self, key: KeyEvent) {
        let dir = match key.code {
            KeyCode::Left | KeyCode::Char('h') => Some((-1.0, 0.0)),
            KeyCode::Right | KeyCode::Char('l') => Some((1.0, 0.0)),
            KeyCode::Up | KeyCode::Char('k') => Some((0.0, 1.0)),
            KeyCode::Down | KeyCode::Char('j') => Some((0.0, -1.0)),
            _ => None,
        };
        if let Some((dx, dy)) = dir {
            let n = self.node_count();
            if n == 0 {
                return;
            }
            self.selected_node = if self.compact {
                if dy > 0.0 || dx < 0.0 {
                    self.selected_node.saturating_sub(1)
                } else {
                    (self.selected_node + 1).min(n - 1)
                }
            } else {
                nearest(n, self.selected_node, dx, dy).unwrap_or(self.selected_node)
            };
            return;
        }
        match key.code {
            KeyCode::Enter => self.open_selected(),
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.node_count() {
                    self.selected_node = i;
                }
            }
            _ => {}
        }
    }

    fn open_selected(&mut self) {
        let project = if self.compact {
            Some(self.selected_node)
        } else {
            match self.nodes().get(self.selected_node) {
                Some(Node::Project(i)) => Some(*i),
                Some(Node::More(_)) => return self.goto(Screen::Global(Tab::Agents)),
                None => None,
            }
        };
        if let Some(p) = project.and_then(|i| self.model.projects.get(i)) {
            let key = p.key.clone();
            self.goto(Screen::Project {
                key,
                tab: Tab::Agents,
            });
        }
    }

    fn set_tab(&mut self, tab: Tab) {
        match &mut self.screen {
            Screen::Project { tab: t, .. } => *t = tab,
            Screen::Global(t) => *t = tab,
            Screen::Core => {}
        }
        self.selected_row = 0;
    }

    fn on_list_key(&mut self, key: KeyEvent) {
        let tab_index = Tab::ALL.iter().position(|t| *t == self.tab()).unwrap_or(0);
        match key.code {
            KeyCode::Esc if !self.search.is_empty() => self.search.clear(),
            KeyCode::Esc => self.goto(Screen::Core),
            KeyCode::Tab => self.set_tab(Tab::ALL[(tab_index + 1) % Tab::ALL.len()]),
            KeyCode::BackTab => {
                self.set_tab(Tab::ALL[(tab_index + Tab::ALL.len() - 1) % Tab::ALL.len()])
            }
            KeyCode::Char(c @ '1'..='5') => self.set_tab(Tab::ALL[c as usize - '1' as usize]),
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_row = self.selected_row.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_row = (self.selected_row + 1).min(self.row_count().saturating_sub(1))
            }
            KeyCode::Enter => self.activate_row(),
            KeyCode::Char('/') => self.search_editing = true,
            KeyCode::Char('s') => {
                self.state_filter = match self.state_filter {
                    None => Some(AgentStatus::Blocked),
                    Some(AgentStatus::Blocked) => Some(AgentStatus::Done),
                    Some(AgentStatus::Done) => Some(AgentStatus::Working),
                    Some(AgentStatus::Working) => Some(AgentStatus::Idle),
                    Some(_) => None,
                };
                self.selected_row = 0;
            }
            KeyCode::Char('w') => self.range = self.range.next(),
            KeyCode::Char('f') if matches!(self.screen, Screen::Global(_)) => {
                let keys: Vec<&String> = self.model.projects.iter().map(|p| &p.key).collect();
                let next = match &self.project_filter {
                    None => keys.first().map(|k| k.to_string()),
                    Some(cur) => keys
                        .iter()
                        .position(|k| *k == cur)
                        .and_then(|i| keys.get(i + 1))
                        .map(|k| k.to_string()),
                };
                self.project_filter = next;
                self.selected_row = 0;
            }
            KeyCode::Char('a') if self.tab() == Tab::Ideas => self.add_idea(),
            KeyCode::Char('e') if self.tab() == Tab::Ideas => self.edit_idea(),
            KeyCode::Char('d') if self.tab() == Tab::Ideas => {
                let target = (self.visible_ideas().get(self.selected_row))
                    .map(|(i, idea)| (*i, idea.name.clone()));
                if let Some((i, name)) = target {
                    self.confirm = Some(Confirm {
                        what: Doomed::Idea(i),
                        name,
                        yes: false,
                    });
                }
            }
            KeyCode::Char('x') => match self.run_context() {
                Some(ctx) => {
                    let n = (self.targets.iter())
                        .filter(|t| t.project_key == ctx.project_key)
                        .count();
                    self.picker = Some(Picker {
                        ctx,
                        chosen: vec![false; n],
                        cursor: 0,
                        choosing_layout: false,
                    });
                }
                None => {
                    self.status = Some("x runs targets: open a project or select an agent".into())
                }
            },
            _ => {}
        }
    }

    fn activate_row(&mut self) {
        let row = self.selected_row;
        match self.tab() {
            Tab::Agents => {
                if let Some(id) = self
                    .visible_agents()
                    .get(row)
                    .map(|(_, a)| a.pane.pane_id.clone())
                {
                    self.action = Some(Action::FocusPane(id));
                }
            }
            Tab::Threads => {
                let target = self
                    .visible_threads()
                    .get(row)
                    .map(|t| (t.live_pane.clone(), t.thread.session_id.clone()));
                match target {
                    Some((Some(pane), _)) => self.action = Some(Action::FocusPane(pane)),
                    Some((None, session)) => {
                        let cmd = format!("claude --resume {session}");
                        self.status = Some(format!("{cmd}  (copied)"));
                        self.action = Some(Action::Copy(cmd));
                    }
                    None => {}
                }
            }
            Tab::Ideas => self.edit_idea(),
            Tab::Timeline | Tab::Usage => {}
        }
    }

    /// Pane of a target that is already running, found by its herdr label.
    pub fn running_pane(&self, project: &str, target: &str) -> Option<&String> {
        self.model
            .pane_labels
            .get(&run::pane_label(project, target))
    }

    /// The selected agent (Agents tab) or the open project, with the agent's worktree preferred as root.
    fn run_context(&self) -> Option<RunContext> {
        let row = if self.tab() == Tab::Agents {
            self.visible_agents().get(self.selected_row).copied()
        } else {
            None
        };
        let (project, agent) = match (&self.screen, row) {
            (_, Some((p, a))) => (p, Some(a)),
            (Screen::Project { key, .. }, None) => {
                (self.model.projects.iter().find(|p| &p.key == key)?, None)
            }
            _ => return None,
        };
        let root = (agent.and_then(|a| a.worktree.clone())).unwrap_or_else(|| project.root.clone());
        Some(RunContext {
            project_key: project.key.clone(),
            project: project.name.clone(),
            workspace_id: agent
                .or(project.agents.first())
                .map(|a| a.pane.workspace_id.clone()),
            root: PathBuf::from(root),
        })
    }

    /// The open picker's targets: those of its project, with their index into `targets`.
    pub fn picker_targets(&self) -> Vec<(usize, &Target)> {
        let Some(p) = &self.picker else {
            return Vec::new();
        };
        (self.targets.iter().enumerate())
            .filter(|(_, t)| t.project_key == p.ctx.project_key)
            .collect()
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let last = self.picker_targets().len().saturating_sub(1);
        let Some(p) = self.picker.as_mut() else {
            return;
        };
        match (p.choosing_layout, key.code) {
            (true, KeyCode::Char('t')) => self.start_targets(Layout::Tabs),
            (true, KeyCode::Char('s')) => self.start_targets(Layout::Split),
            (true, KeyCode::Esc) => p.choosing_layout = false,
            (true, _) => {}
            (false, KeyCode::Esc) => self.picker = None,
            (false, KeyCode::Up | KeyCode::Char('k')) => p.cursor = p.cursor.saturating_sub(1),
            (false, KeyCode::Down | KeyCode::Char('j')) => p.cursor = (p.cursor + 1).min(last),
            (false, KeyCode::Char(' ')) => {
                if let Some(c) = p.chosen.get_mut(p.cursor) {
                    *c = !*c;
                }
            }
            (false, KeyCode::Char('a')) => {
                let all = p.chosen.iter().all(|c| *c);
                p.chosen.fill(!all);
            }
            (false, KeyCode::Char('n')) => self.target_form(false),
            (false, KeyCode::Char('e')) => self.target_form(true),
            (false, KeyCode::Char('d')) => {
                let cursor = p.cursor;
                if let Some((i, t)) = self.picker_targets().get(cursor) {
                    self.confirm = Some(Confirm {
                        what: Doomed::Target(*i),
                        name: t.name.clone(),
                        yes: false,
                    });
                }
            }
            (false, KeyCode::Enter) => match p.chosen.iter().filter(|c| **c).count() {
                0 => {}
                1 => self.start_targets(Layout::Tabs),
                _ => p.choosing_layout = true,
            },
            (false, _) => {}
        }
    }

    /// Starts the chosen targets that are not running yet; focuses a running one when none is left.
    fn start_targets(&mut self, layout: Layout) {
        let Some(p) = self.picker.as_ref() else {
            return;
        };
        let ctx = p.ctx.clone();
        let (running, to_start): (Vec<Target>, Vec<Target>) = (self.picker_targets().into_iter())
            .zip(&p.chosen)
            .filter(|(_, chosen)| **chosen)
            .map(|((_, t), _)| t.clone())
            .partition(|t| self.running_pane(&ctx.project, &t.name).is_some());
        self.picker = None;
        if to_start.is_empty() {
            self.action = (running.first())
                .and_then(|t| self.running_pane(&ctx.project, &t.name))
                .cloned()
                .map(Action::FocusPane);
            return;
        }
        if !running.is_empty() {
            let names: Vec<&str> = running.iter().map(|t| t.name.as_str()).collect();
            self.status = Some(format!("already running: {}", names.join(", ")));
        }
        self.action = Some(Action::Run {
            steps: run::plan(&ctx.project, &ctx.root, &to_start, layout),
            workspace_id: ctx.workspace_id,
        });
    }

    /// The target form over the picker: a new target, or (`edit`) the one under the cursor.
    fn target_form(&mut self, edit: bool) {
        let Some(p) = &self.picker else {
            return;
        };
        let (editing, t) = match (edit, self.picker_targets().get(p.cursor)) {
            (false, _) => (
                None,
                Target {
                    project_key: p.ctx.project_key.clone(),
                    ..Target::default()
                },
            ),
            (true, Some((i, t))) => (Some(*i), (*t).clone()),
            (true, None) => return,
        };
        let verb = if edit { "edit" } else { "new" };
        self.form = Some(Form {
            title: format!(" {verb} target · {} ", p.ctx.project),
            kind: FormKind::Target {
                editing,
                project_key: t.project_key,
            },
            fields: vec![("name", t.name), ("command", t.command), ("cwd", t.cwd)],
            focus: 0,
        });
    }

    fn on_confirm_key(&mut self, key: KeyEvent) {
        let Some(c) = self.confirm.as_mut() else {
            return;
        };
        let delete = match key.code {
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('h' | 'l') => {
                c.yes = !c.yes;
                return;
            }
            KeyCode::Enter => c.yes,
            KeyCode::Char('y') => true,
            KeyCode::Char('n') | KeyCode::Esc => false,
            _ => return,
        };
        let Some(c) = self.confirm.take() else {
            return;
        };
        if !delete {
            return;
        }
        match c.what {
            Doomed::Idea(i) if i < self.ideas.len() => {
                self.ideas.remove(i);
                self.action = Some(Action::SaveIdeas);
                self.clamp();
            }
            Doomed::Target(i) if i < self.targets.len() => {
                self.targets.remove(i);
                if let Some(p) = self.picker.as_mut() {
                    if p.cursor < p.chosen.len() {
                        p.chosen.remove(p.cursor);
                    }
                    p.cursor = p.cursor.min(p.chosen.len().saturating_sub(1));
                }
                self.action = Some(Action::SaveTargets);
            }
            _ => {}
        }
    }

    fn add_idea(&mut self) {
        let Some(key) = self.scope().map(str::to_string) else {
            self.status = Some("pick a project with f to add an idea here".into());
            return;
        };
        let project_name = self.project_name(&key);
        self.form = Some(Form {
            title: format!(" new idea · {project_name} "),
            kind: FormKind::Idea {
                editing: None,
                project_key: key,
                project_name,
            },
            fields: vec![("name", String::new()), ("description", String::new())],
            focus: 0,
        });
    }

    fn edit_idea(&mut self) {
        let target =
            (self.visible_ideas().get(self.selected_row)).map(|(i, idea)| (*i, (*idea).clone()));
        if let Some((i, idea)) = target {
            self.form = Some(Form {
                title: format!(" edit idea · {} ", idea.project_name),
                kind: FormKind::Idea {
                    editing: Some(i),
                    project_key: idea.project_key,
                    project_name: idea.project_name,
                },
                fields: vec![("name", idea.name), ("description", idea.description)],
                focus: 0,
            });
        }
    }

    fn on_form_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.form = None;
                self.status = None;
            }
            KeyCode::Enter => self.submit_form(),
            code => {
                let Some(form) = self.form.as_mut() else {
                    return;
                };
                let n = form.fields.len();
                match code {
                    KeyCode::Tab => form.focus = (form.focus + 1) % n,
                    KeyCode::BackTab => form.focus = (form.focus + n - 1) % n,
                    KeyCode::Backspace => {
                        form.fields[form.focus].1.pop();
                    }
                    KeyCode::Char(c) => form.fields[form.focus].1.push(c),
                    _ => {}
                }
            }
        }
    }

    fn submit_form(&mut self) {
        let Some(form) = self.form.take() else {
            return;
        };
        let v: Vec<String> = (form.fields.iter())
            .map(|(_, s)| s.trim().to_string())
            .collect();
        let (required, refusal) = match form.kind {
            FormKind::Idea { .. } => (1, "an idea needs a name"),
            FormKind::Target { .. } => (2, "a target needs a name and a command"),
        };
        if v[..required].iter().any(String::is_empty) {
            self.status = Some(refusal.into());
            self.form = Some(form);
            return;
        }
        self.status = None;
        match form.kind {
            FormKind::Idea {
                editing,
                project_key,
                project_name,
            } => {
                let idea = Idea {
                    name: v[0].clone(),
                    description: v[1].clone(),
                    project_key,
                    project_name,
                };
                match editing {
                    Some(i) if i < self.ideas.len() => self.ideas[i] = idea,
                    _ => self.ideas.push(idea),
                }
                self.action = Some(Action::SaveIdeas);
            }
            FormKind::Target {
                editing,
                project_key,
            } => {
                let i = match editing {
                    Some(i) if i < self.targets.len() => i,
                    _ => {
                        self.targets.push(Target {
                            project_key,
                            ..Target::default()
                        });
                        if let Some(p) = self.picker.as_mut() {
                            p.chosen.push(false);
                        }
                        self.targets.len() - 1
                    }
                };
                let t = &mut self.targets[i];
                (t.name, t.command, t.cwd) = (v[0].clone(), v[1].clone(), v[2].clone());
                self.action = Some(Action::SaveTargets);
            }
        }
    }
}

#[cfg(test)]
pub fn sample_app() -> App {
    let mut app = App::new(&Config::default(), Pricing::new(Default::default()));
    app.model = crate::model::testkit::model();
    app.now = crate::model::testkit::ts("2026-10-08T12:00:00Z");
    app.last_left = app.now;
    app.boot_done = true;
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ideas::Idea;
    use crate::run::{self, Layout, Target};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn chars(app: &mut App, s: &str) {
        for c in s.chars() {
            key(app, KeyCode::Char(c));
        }
    }

    /// Beta's blocked agent and alpha's agent (made done), changed at `blocked_at` and `done_at`.
    fn with_changes(blocked_at: &str, done_at: &str) -> App {
        let mut app = sample_app();
        for p in &mut app.model.projects {
            for a in &mut p.agents {
                if a.pane.agent_status == AgentStatus::Blocked {
                    a.since = Some(crate::model::testkit::ts(blocked_at));
                } else {
                    a.pane.agent_status = AgentStatus::Done;
                    a.since = Some(crate::model::testkit::ts(done_at));
                }
            }
        }
        app
    }

    fn agent(app: &App, status: AgentStatus) -> &AgentRow {
        app.model
            .projects
            .iter()
            .flat_map(|p| &p.agents)
            .find(|a| a.pane.agent_status == status)
            .unwrap()
    }

    #[test]
    fn changes_after_you_left_are_new_until_you_leave_again() {
        let mut app = with_changes("2026-10-08T11:00:00Z", "2026-10-08T11:30:00Z");
        app.last_left = crate::model::testkit::ts("2026-10-08T11:15:00Z");
        assert!(!app.is_new(agent(&app, AgentStatus::Blocked)));
        assert!(app.is_new(agent(&app, AgentStatus::Done)));
        app.set_looking(true);
        assert!(app.is_new(agent(&app, AgentStatus::Done)));
        app.set_looking(false);
        assert_eq!(app.last_left, app.now);
        assert!(!app.is_new(agent(&app, AgentStatus::Done)));
    }

    #[test]
    fn tab_label_counts_all_blocked_and_new_done() {
        let mut app = with_changes("2026-10-08T11:00:00Z", "2026-10-08T11:30:00Z");
        app.model.totals.blocked = 1;
        app.last_left = crate::model::testkit::ts("2026-10-08T10:00:00Z");
        assert_eq!(app.tab_label(), "Jarvis ▲1 ✓1");
        app.last_left = app.now;
        assert_eq!(app.tab_label(), "Jarvis ▲1");
        app.model.totals.blocked = 0;
        assert_eq!(app.tab_label(), "Jarvis");
    }

    #[test]
    fn first_key_only_skips_boot() {
        let mut app = sample_app();
        app.boot_done = false;
        key(&mut app, KeyCode::Char('q'));
        assert!(app.boot_done);
        assert!(!app.quit);
    }

    #[test]
    fn enter_opens_most_urgent_project() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Enter);
        let beta = app.model.projects[0].key.clone();
        assert_eq!(
            app.screen,
            Screen::Project {
                key: beta,
                tab: Tab::Agents
            }
        );
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.action, Some(Action::FocusPane("w2:p1".into())));
    }

    #[test]
    fn digits_select_and_letters_open_global_views() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('3'));
        assert_eq!(app.selected_node, 2);
        key(&mut app, KeyCode::Char('9'));
        assert_eq!(app.selected_node, 2);
        key(&mut app, KeyCode::Char('T'));
        assert_eq!(app.screen, Screen::Global(Tab::Threads));
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Core);
        // Jarvis stays open as a tab: Esc on the core does nothing, only q quits.
        key(&mut app, KeyCode::Esc);
        assert!(!app.quit);
        key(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
    }

    #[test]
    fn overflow_goes_into_more_node() {
        let mut app = sample_app();
        app.max_branches = 2;
        let nodes = app.nodes();
        assert!(matches!(nodes[..], [Node::Project(0), Node::More(2)]));
        key(&mut app, KeyCode::Char('2'));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Global(Tab::Agents));
    }

    #[test]
    fn nearest_follows_direction() {
        // 4 nodes: 0 top, 1 right, 2 bottom, 3 left
        assert_eq!(nearest(4, 0, 1.0, 0.0), Some(1));
        assert_eq!(nearest(4, 0, 0.0, -1.0), Some(2));
        assert_eq!(nearest(4, 0, -1.0, 0.0), Some(3));
        assert_eq!(nearest(4, 0, 0.0, 1.0), None);
        assert_eq!(nearest(1, 0, 1.0, 0.0), None);
    }

    #[test]
    fn compact_moves_vertically() {
        let mut app = sample_app();
        app.compact = true;
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Down);
        assert_eq!(app.selected_node, 2);
        key(&mut app, KeyCode::Up);
        assert_eq!(app.selected_node, 1);
    }

    #[test]
    fn tabs_search_and_resume() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('A'));
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.tab(), Tab::Threads);
        assert_eq!(app.visible_threads().len(), 2);
        key(&mut app, KeyCode::Char('/'));
        chars(&mut app, "beta");
        key(&mut app, KeyCode::Enter);
        assert!(!app.search_editing);
        assert_eq!(app.visible_threads().len(), 1);
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            app.action,
            Some(Action::Copy("claude --resume sess-old".into()))
        );
        assert!(app
            .status
            .as_deref()
            .unwrap()
            .contains("claude --resume sess-old"));
        key(&mut app, KeyCode::Esc);
        assert!(app.search.is_empty());
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Core);
    }

    #[test]
    fn timeline_filters_by_state_range_and_project() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('L'));
        app.range = Range::Hour;
        assert_eq!(app.visible_events().len(), 2);
        key(&mut app, KeyCode::Char('s'));
        assert_eq!(app.state_filter, Some(AgentStatus::Blocked));
        assert_eq!(app.visible_events().len(), 1);
        key(&mut app, KeyCode::Char('s'));
        key(&mut app, KeyCode::Char('s'));
        key(&mut app, KeyCode::Char('s'));
        key(&mut app, KeyCode::Char('s'));
        assert_eq!(app.state_filter, None);
        key(&mut app, KeyCode::Char('f'));
        assert_eq!(
            app.project_filter.as_deref(),
            Some(app.model.projects[0].key.as_str())
        );
        assert_eq!(app.visible_events().len(), 1);
    }

    #[test]
    fn clamp_keeps_selection_in_range() {
        let mut app = sample_app();
        app.selected_node = 7;
        app.screen = Screen::Global(Tab::Threads);
        app.selected_row = 9;
        app.clamp();
        assert_eq!(app.selected_node, 2);
        assert_eq!(app.selected_row, 1);
    }

    #[test]
    fn help_and_fatal_swallow_keys() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('?'));
        assert!(app.show_help);
        key(&mut app, KeyCode::Char('T'));
        assert!(!app.show_help);
        assert_eq!(app.screen, Screen::Core);
        app.fatal = Some("protocol".into());
        key(&mut app, KeyCode::Char('x'));
        assert!(app.quit);
    }

    fn idea(project: &App, i: usize, name: &str) -> Idea {
        let p = &project.model.projects[i];
        Idea {
            project_key: p.key.clone(),
            project_name: p.name.clone(),
            name: name.into(),
            description: String::new(),
        }
    }

    #[test]
    fn ideas_tab_is_reachable_like_the_others() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('I'));
        assert_eq!(app.screen, Screen::Global(Tab::Ideas));
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.tab(), Tab::Agents);
        key(&mut app, KeyCode::BackTab);
        assert_eq!(app.tab(), Tab::Ideas);
        key(&mut app, KeyCode::Char('1'));
        key(&mut app, KeyCode::Char('5'));
        assert_eq!(app.tab(), Tab::Ideas);
    }

    #[test]
    fn add_edit_and_delete_an_idea_in_a_project() {
        let mut app = sample_app();
        let beta = app.model.projects[0].key.clone();
        app.screen = Screen::Project {
            key: beta.clone(),
            tab: Tab::Ideas,
        };
        key(&mut app, KeyCode::Char('a'));
        chars(&mut app, "Cache q?5");
        key(&mut app, KeyCode::Tab);
        chars(&mut app, "keep 24h");
        key(&mut app, KeyCode::Enter);
        assert!(app.form.is_none());
        assert!(!app.quit);
        assert_eq!(app.action.take(), Some(Action::SaveIdeas));
        assert_eq!(
            app.ideas,
            vec![Idea {
                project_key: beta,
                project_name: "beta".into(),
                name: "Cache q?5".into(),
                description: "keep 24h".into(),
            }]
        );

        key(&mut app, KeyCode::Char('e'));
        for _ in 0..4 {
            key(&mut app, KeyCode::Backspace);
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.ideas[0].name, "Cache");
        assert_eq!(app.ideas[0].description, "keep 24h");
        assert_eq!(app.action.take(), Some(Action::SaveIdeas));

        key(&mut app, KeyCode::Char('d'));
        assert_eq!(
            app.confirm,
            Some(Confirm {
                what: Doomed::Idea(0),
                name: "Cache".into(),
                yes: false,
            })
        );
        key(&mut app, KeyCode::Char('n'));
        assert_eq!(app.confirm, None);
        assert_eq!(app.ideas.len(), 1);
        assert_eq!(app.action, None);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        assert!(app.ideas.is_empty());
        assert_eq!(app.action, Some(Action::SaveIdeas));
    }

    #[test]
    fn delete_popup_defaults_to_no() {
        let mut app = sample_app();
        app.ideas = vec![idea(&app, 0, "keep"), idea(&app, 0, "drop")];
        app.screen = Screen::Project {
            key: app.model.projects[0].key.clone(),
            tab: Tab::Ideas,
        };
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('q'));
        assert!(!app.quit, "the popup takes every key");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.confirm, None, "Enter on the default No cancels");
        assert_eq!(app.ideas.len(), 2);

        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.confirm, None);
        assert!(matches!(app.screen, Screen::Project { .. }));
        assert_eq!(app.ideas.len(), 2);

        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Right);
        assert!(app.confirm.as_ref().unwrap().yes);
        key(&mut app, KeyCode::Char('h'));
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.ideas, vec![idea(&app, 0, "keep")]);
        assert_eq!(app.action, Some(Action::SaveIdeas));
        assert_eq!(app.selected_row, 0);
    }

    #[test]
    fn form_refuses_an_empty_name_and_esc_cancels() {
        let mut app = sample_app();
        app.screen = Screen::Project {
            key: app.model.projects[0].key.clone(),
            tab: Tab::Ideas,
        };
        key(&mut app, KeyCode::Char('a'));
        chars(&mut app, "  ");
        key(&mut app, KeyCode::Enter);
        assert!(app.form.is_some());
        assert_eq!(app.status.as_deref(), Some("an idea needs a name"));
        key(&mut app, KeyCode::Esc);
        assert!(app.form.is_none());
        assert_eq!(app.status, None);
        assert!(app.ideas.is_empty());
        assert!(matches!(app.screen, Screen::Project { .. }));
    }

    #[test]
    fn form_backspace_removes_whole_characters() {
        let mut app = sample_app();
        app.screen = Screen::Project {
            key: app.model.projects[0].key.clone(),
            tab: Tab::Ideas,
        };
        key(&mut app, KeyCode::Char('a'));
        chars(&mut app, "Zażółć");
        key(&mut app, KeyCode::Backspace);
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.form.as_ref().unwrap().fields[0].1, "Zażó");
    }

    #[test]
    fn global_ideas_need_a_filter_to_add_and_list_every_project() {
        let mut app = sample_app();
        app.ideas = vec![idea(&app, 0, "beta idea"), idea(&app, 1, "alpha idea")];
        key(&mut app, KeyCode::Char('I'));
        assert_eq!(app.visible_ideas().len(), 2);
        key(&mut app, KeyCode::Char('a'));
        assert!(app.form.is_none());
        assert!(app
            .status
            .as_deref()
            .unwrap()
            .contains("pick a project with f"));
        key(&mut app, KeyCode::Char('f'));
        assert_eq!(app.visible_ideas().len(), 1);
        key(&mut app, KeyCode::Char('a'));
        assert_eq!(app.form.as_ref().unwrap().title, " new idea · beta ");
    }

    #[test]
    fn edit_and_delete_follow_the_filtered_row() {
        let mut app = sample_app();
        app.ideas = vec![idea(&app, 0, "first"), idea(&app, 1, "second")];
        key(&mut app, KeyCode::Char('I'));
        key(&mut app, KeyCode::Char('/'));
        chars(&mut app, "second");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.form.as_ref().unwrap().kind,
            FormKind::Idea {
                editing: Some(1),
                ..
            }
        ));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(app.ideas, vec![idea(&app, 0, "first")]);
    }

    /// alpha's targets, as `alpha_picker` puts them first in `App::targets`.
    fn run_targets(app: &App) -> Vec<Target> {
        let target = |name: &str, command: &str| Target {
            project_key: app.model.projects[1].key.clone(),
            name: name.into(),
            command: command.into(),
            cwd: String::new(),
            env: Default::default(),
        };
        vec![target("api", "cargo run"), target("web", "pnpm dev")]
    }

    /// alpha's project screen, Agents tab, its only agent selected.
    fn alpha_agents(app: &mut App) {
        app.screen = Screen::Project {
            key: app.model.projects[1].key.clone(),
            tab: Tab::Agents,
        };
    }

    /// Opens alpha's picker with alpha's two targets and one of beta's.
    fn alpha_picker(app: &mut App) -> RunContext {
        app.targets = run_targets(app);
        app.targets.push(Target {
            project_key: app.model.projects[0].key.clone(),
            name: "beta-api".into(),
            command: "make".into(),
            cwd: String::new(),
            env: Default::default(),
        });
        alpha_agents(app);
        key(app, KeyCode::Char('x'));
        app.picker.as_ref().expect("x opens the picker").ctx.clone()
    }

    fn names(app: &App) -> Vec<String> {
        app.picker_targets()
            .iter()
            .map(|(_, t)| t.name.clone())
            .collect()
    }

    #[test]
    fn x_opens_the_picker_for_the_selected_agents_worktree() {
        let mut app = sample_app();
        app.model.projects[1].agents[0].worktree = Some("/home/u/alpha-feat".into());
        alpha_agents(&mut app);
        key(&mut app, KeyCode::Char('x'));
        assert_eq!(
            app.picker.as_ref().unwrap().ctx,
            RunContext {
                project_key: app.model.projects[1].key.clone(),
                project: "alpha".into(),
                workspace_id: Some("w1".into()),
                root: PathBuf::from("/home/u/alpha-feat"),
            }
        );
    }

    #[test]
    fn x_on_a_project_tab_without_agent_rows_uses_the_project_root() {
        let mut app = sample_app();
        app.screen = Screen::Project {
            key: app.model.projects[1].key.clone(),
            tab: Tab::Threads,
        };
        key(&mut app, KeyCode::Char('x'));
        let ctx = &app.picker.as_ref().unwrap().ctx;
        assert_eq!(ctx.root, PathBuf::from(&app.model.projects[1].root));
        assert_eq!(ctx.workspace_id.as_deref(), Some("w1"));
    }

    #[test]
    fn x_needs_a_project_or_an_agent() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('T'));
        key(&mut app, KeyCode::Char('x'));
        assert!(app.picker.is_none());
        assert!(app.status.as_deref().unwrap().contains("open a project"));
    }

    #[test]
    fn x_opens_an_empty_picker_for_a_project_without_targets() {
        let mut app = sample_app();
        alpha_agents(&mut app);
        key(&mut app, KeyCode::Char('x'));
        assert!(app.picker_targets().is_empty());
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Char('e'));
        key(&mut app, KeyCode::Char('d'));
        assert!(app.picker.is_some());
        assert!(app.form.is_none() && app.confirm.is_none());
        assert_eq!(app.action, None);
    }

    #[test]
    fn picker_lists_only_the_projects_targets() {
        let mut app = sample_app();
        alpha_picker(&mut app);
        assert_eq!(names(&app), ["api", "web"]);
        key(&mut app, KeyCode::Esc);
        app.screen = Screen::Project {
            key: app.model.projects[0].key.clone(),
            tab: Tab::Agents,
        };
        key(&mut app, KeyCode::Char('x'));
        assert_eq!(names(&app), ["beta-api"]);
    }

    #[test]
    fn picker_adds_edits_and_deletes_targets() {
        let mut app = sample_app();
        alpha_agents(&mut app);
        key(&mut app, KeyCode::Char('x'));
        key(&mut app, KeyCode::Char('n'));
        chars(&mut app, "api");
        key(&mut app, KeyCode::Tab);
        chars(&mut app, "cargo run");
        key(&mut app, KeyCode::Tab);
        chars(&mut app, "services/api");
        key(&mut app, KeyCode::Enter);
        assert!(app.form.is_none());
        assert!(app.picker.is_some(), "Enter returns to the picker");
        assert_eq!(app.action.take(), Some(Action::SaveTargets));
        assert_eq!(
            app.targets,
            vec![Target {
                project_key: app.model.projects[1].key.clone(),
                name: "api".into(),
                command: "cargo run".into(),
                cwd: "services/api".into(),
                env: Default::default(),
            }]
        );
        assert_eq!(app.picker.as_ref().unwrap().chosen, [false]);

        app.targets[0].env.insert("PORT".into(), "3001".into());
        key(&mut app, KeyCode::Char('e'));
        key(&mut app, KeyCode::BackTab);
        key(&mut app, KeyCode::BackTab);
        for _ in 0..3 {
            key(&mut app, KeyCode::Backspace);
        }
        chars(&mut app, "test");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.action.take(), Some(Action::SaveTargets));
        assert_eq!(app.targets[0].command, "cargo test");
        assert_eq!(app.targets[0].cwd, "services/api");
        assert_eq!(app.targets[0].env["PORT"], "3001", "env survives an edit");

        key(&mut app, KeyCode::Char('d'));
        assert_eq!(
            app.confirm,
            Some(Confirm {
                what: Doomed::Target(0),
                name: "api".into(),
                yes: false,
            })
        );
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.targets.len(), 1);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        assert!(app.targets.is_empty());
        assert_eq!(app.action, Some(Action::SaveTargets));
        assert!(app.picker.as_ref().unwrap().chosen.is_empty());
    }

    #[test]
    fn target_form_needs_a_name_and_a_command() {
        let mut app = sample_app();
        alpha_agents(&mut app);
        key(&mut app, KeyCode::Char('x'));
        key(&mut app, KeyCode::Char('n'));
        chars(&mut app, "api");
        key(&mut app, KeyCode::Enter);
        assert!(app.form.is_some());
        assert_eq!(
            app.status.as_deref(),
            Some("a target needs a name and a command")
        );
        key(&mut app, KeyCode::Esc);
        assert!(app.form.is_none());
        assert!(app.picker.is_some());
        assert!(app.targets.is_empty());
    }

    #[test]
    fn deleting_a_target_keeps_the_other_choices() {
        let mut app = sample_app();
        alpha_picker(&mut app);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Up);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(names(&app), ["web"]);
        assert_eq!(app.picker.as_ref().unwrap().chosen, [true]);
        assert_eq!(app.targets.len(), 2, "beta's target stays");
    }

    #[test]
    fn one_chosen_target_starts_in_its_own_tab() {
        let mut app = sample_app();
        let ctx = alpha_picker(&mut app);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Enter);
        assert!(app.picker.is_none());
        assert_eq!(
            app.action,
            Some(Action::Run {
                workspace_id: Some("w1".into()),
                steps: run::plan("alpha", &ctx.root, &run_targets(&app)[1..], Layout::Tabs),
            })
        );
    }

    #[test]
    fn several_targets_ask_for_the_layout() {
        let mut app = sample_app();
        let ctx = alpha_picker(&mut app);
        key(&mut app, KeyCode::Enter);
        assert!(app.action.is_none(), "nothing chosen, nothing happens");
        key(&mut app, KeyCode::Char('a'));
        key(&mut app, KeyCode::Enter);
        assert!(app.picker.as_ref().unwrap().choosing_layout);
        key(&mut app, KeyCode::Esc);
        assert!(!app.picker.as_ref().unwrap().choosing_layout);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('s'));
        assert_eq!(
            app.action,
            Some(Action::Run {
                workspace_id: Some("w1".into()),
                steps: run::plan("alpha", &ctx.root, &run_targets(&app), Layout::Split),
            })
        );
    }

    #[test]
    fn running_targets_are_focused_or_skipped() {
        let mut app = sample_app();
        app.model
            .pane_labels
            .insert("alpha:api".into(), "w1:p7".into());
        alpha_picker(&mut app);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.action.take(), Some(Action::FocusPane("w1:p7".into())));

        let ctx = alpha_picker(&mut app);
        key(&mut app, KeyCode::Char('a'));
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('t'));
        assert_eq!(
            app.action,
            Some(Action::Run {
                workspace_id: Some("w1".into()),
                steps: run::plan("alpha", &ctx.root, &run_targets(&app)[1..], Layout::Tabs),
            })
        );
        assert_eq!(app.status.as_deref(), Some("already running: api"));
    }

    #[test]
    fn picker_takes_every_key_and_esc_closes_it() {
        let mut app = sample_app();
        alpha_picker(&mut app);
        key(&mut app, KeyCode::Char('q'));
        key(&mut app, KeyCode::Char('T'));
        assert!(!app.quit);
        assert!(matches!(app.screen, Screen::Project { .. }));
        key(&mut app, KeyCode::Esc);
        assert!(app.picker.is_none());
        assert!(matches!(app.screen, Screen::Project { .. }));
    }
}
