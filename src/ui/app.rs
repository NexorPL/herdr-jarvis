//! UI state and key handling. Pure: no terminal, no herdr; side effects are requested via `action`.

use super::theme::Palette;
use crate::config::{Animation, Config};
use crate::herdr::AgentStatus;
use crate::model::{AgentRow, EventRow, Model, Project, ThreadRow};
use crate::pricing::Pricing;
use chrono::{DateTime, Duration, Local, Utc};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::f64::consts::{FRAC_PI_2, TAU};

/// Boot animation length in 33 ms ticks (~0.8 s).
pub const BOOT_TICKS: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Agents,
    Threads,
    Timeline,
    Usage,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Agents, Tab::Threads, Tab::Timeline, Tab::Usage];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Agents => "Agents",
            Tab::Threads => "Threads",
            Tab::Timeline => "Timeline",
            Tab::Usage => "Usage",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    FocusPane(String),
    Copy(String),
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
        }
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

    pub fn row_count(&self) -> usize {
        match self.tab() {
            Tab::Agents => self.visible_agents().len(),
            Tab::Threads => self.visible_threads().len(),
            Tab::Timeline => self.visible_events().len(),
            Tab::Usage => 0,
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
            KeyCode::Esc => self.quit = true,
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
            KeyCode::Tab => self.set_tab(Tab::ALL[(tab_index + 1) % 4]),
            KeyCode::BackTab => self.set_tab(Tab::ALL[(tab_index + 3) % 4]),
            KeyCode::Char(c @ '1'..='4') => self.set_tab(Tab::ALL[c as usize - '1' as usize]),
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
            Tab::Timeline | Tab::Usage => {}
        }
    }
}

#[cfg(test)]
pub fn sample_app() -> App {
    let mut app = App::new(&Config::default(), Pricing::new(Default::default()));
    app.model = crate::model::testkit::model();
    app.now = crate::model::testkit::ts("2026-10-08T12:00:00Z");
    app.boot_done = true;
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn chars(app: &mut App, s: &str) {
        for c in s.chars() {
            key(app, KeyCode::Char(c));
        }
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
        key(&mut app, KeyCode::Esc);
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
}
