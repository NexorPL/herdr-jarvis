pub mod app;
pub mod hud;
pub mod overlays;
pub mod theme;
pub mod views;

use crate::config::Config;
use crate::events::{self, Record};
use crate::herdr::{self, Snapshot, WatchMsg};
use crate::log::log;
use crate::pricing::Pricing;
use crate::projects::Resolver;
use crate::transcripts::claude::ClaudeSource;
use crate::transcripts::Thread;
use crate::{collector, ideas, model, paths};
use app::{Action, App, Screen, BOOT_TICKS};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::prelude::*;
use ratatui::DefaultTerminal;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub fn draw(f: &mut Frame, app: &App) {
    let [top, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(f.area());
    hud::status_bar(f, top, app);
    if let Some(msg) = &app.fatal {
        return centered(
            f,
            body,
            " Jarvis cannot run ",
            &[msg.as_str(), "", "press any key to close"],
            app,
        );
    }
    match &app.screen {
        Screen::Core => hud::draw_core(f, body, app),
        Screen::Project { .. } | Screen::Global(_) => views::draw(f, body, app),
    }
    if let Some(form) = &app.form {
        overlays::idea_form(f, body, app, form);
    }
    if app.show_help {
        centered(
            f,
            body,
            " help ",
            &[
                "Core:   arrows/hjkl move · 1-9 select · Enter open project",
                "        A agents · T threads · L timeline · U usage · I ideas (all projects)",
                "Lists:  ↑↓/jk move · Enter jump to pane / resume thread · Tab or 1-5 views",
                "        / search · s state filter · w time range · f project filter",
                "Ideas:  a add · e or Enter edit · d delete",
                "        Esc back · r refresh · q quit · ? this help",
            ],
            app,
        );
    }
}

fn centered(f: &mut Frame, area: Rect, title: &str, lines: &[&str], app: &App) {
    let lines = lines.iter().map(|l| Line::from(l.to_string())).collect();
    overlays::popup(f, area, title, lines, &app.palette);
}

#[derive(Default)]
struct SourceUpdate {
    threads: Vec<Thread>,
    records: Vec<Record>,
    collector_running: bool,
}

/// Transcripts and the event log are read off the UI thread, so a large first index never blocks drawing.
fn spawn_sources(
    mut claude: Option<ClaudeSource>,
    store: events::Log,
    lock: PathBuf,
) -> Receiver<SourceUpdate> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let read = |threads: Vec<Thread>| {
            let now = chrono::Utc::now();
            SourceUpdate {
                threads,
                records: store.read_since(now - chrono::Duration::days(7), now),
                collector_running: collector::is_running(&lock),
            }
        };
        // Timeline and collector status first: the first transcript index can take a while.
        if tx.send(read(Vec::new())).is_err() {
            return;
        }
        loop {
            let threads = claude.as_mut().map(|c| c.refresh()).unwrap_or_default();
            if tx.send(read(threads)).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    rx
}

/// One Jarvis per state dir: the first holds `tui.lock` for its lifetime and records its pane;
/// any later one gets that pane id instead.
fn claim_instance(state: &Path, my_pane: &str) -> std::io::Result<Result<File, String>> {
    match collector::try_lock(&state.join("tui.lock"))? {
        Some(lock) => {
            std::fs::write(state.join("tui.pane"), my_pane)?;
            Ok(Ok(lock))
        }
        None => Ok(Err(
            std::fs::read_to_string(state.join("tui.pane")).unwrap_or_default()
        )),
    }
}

pub fn run() -> anyhow::Result<()> {
    let config = Config::load(&paths::config_dir());
    if let Err(e) = collector::ensure_collector() {
        log(format!("tui: could not start collector: {e}"));
    }
    let state = paths::state_dir();
    std::fs::create_dir_all(&state)?;
    let my_pane = std::env::var("HERDR_PANE_ID").unwrap_or_default();
    let _instance = match claim_instance(&state, &my_pane)? {
        Ok(lock) => lock,
        // Jarvis is already open: jump to it. herdr closes this new tab when we exit, so focus
        // from a detached process once that is done.
        Err(pane) => {
            if !pane.is_empty() {
                collector::spawn_detached(&["focus", &pane])?;
            }
            return Ok(());
        }
    };
    let watch = herdr::watcher::spawn(herdr::socket_path());
    let claude = config
        .claude_dir()
        .map(|dir| ClaudeSource::new(&dir, state.join("claude-index-v2.json")));
    let sources = spawn_sources(
        claude,
        events::Log::new(state.clone()),
        state.join("collector.lock"),
    );
    let mut app = App::new(&config, Pricing::new(config.pricing.clone()));
    let ideas_path = state.join("ideas.json");
    match ideas::load(&ideas_path) {
        Ok(list) => app.ideas = list,
        Err(e) => app.status = Some(e),
    }
    let mut terminal = ratatui::init();
    let me = Me {
        pane: my_pane,
        tab: std::env::var("HERDR_TAB_ID").unwrap_or_default(),
    };
    let result = event_loop(&mut terminal, &mut app, &watch, &sources, &me, &ideas_path);
    ratatui::restore();
    result
}

/// Jarvis's own pane and tab in herdr.
struct Me {
    pane: String,
    tab: String,
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    watch: &Receiver<WatchMsg>,
    sources: &Receiver<SourceUpdate>,
    me: &Me,
    ideas_path: &Path,
) -> anyhow::Result<()> {
    let start = Instant::now();
    let mut tab_label = String::new();
    let mut snap = Snapshot::default();
    let mut update = SourceUpdate::default();
    let mut resolver = Resolver::default();
    let mut dirty = true;
    while !app.quit {
        for msg in watch.try_iter() {
            match msg {
                WatchMsg::Snapshot(s) => {
                    snap = s;
                    app.offline = None;
                    dirty = true;
                }
                WatchMsg::Offline(reason) => app.offline = Some(format!("herdr offline: {reason}")),
                WatchMsg::Incompatible(reason) => app.fatal = Some(reason),
            }
        }
        if let Some(u) = sources.try_iter().last() {
            app.collector_running = u.collector_running;
            update = u;
            dirty = true;
        }
        app.now = chrono::Utc::now();
        app.tick = (start.elapsed().as_millis() / 33) as u64;
        if !app.boot_done && app.tick >= BOOT_TICKS {
            app.boot_done = true;
        }
        if dirty || std::mem::take(&mut app.refresh) {
            let today = app.today();
            app.model = model::build(
                &snap,
                &update.threads,
                &update.records,
                &app.pricing,
                &mut resolver,
                &today,
            );
            app.clamp();
            dirty = false;
        }
        app.set_looking(snap.focused_pane_id.as_deref() == Some(me.pane.as_str()));
        let label = app.tab_label();
        if !me.tab.is_empty() && label != tab_label {
            let params = serde_json::json!({"tab_id": me.tab, "label": label});
            if let Err(e) = herdr::client::request(&herdr::socket_path(), "tab.rename", params) {
                app.status = Some(format!("could not rename the Jarvis tab: {e}"));
            }
            // Not retried on failure: one attempt per label change.
            tab_label = label;
        }
        let size = terminal.size()?;
        app.compact = size.width < 90 || size.height < 28;
        terminal.draw(|f| draw(f, app))?;
        let timeout = Duration::from_millis(if app.animating() { 33 } else { 250 });
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }
        match app.action.take() {
            // Jarvis stays open in its own tab; the agent's pane takes the focus.
            Some(Action::FocusPane(id)) => {
                if let Err(e) = herdr::focus_pane(&herdr::socket_path(), &id) {
                    app.status = Some(format!("could not focus {id}: {e}"));
                }
            }
            Some(Action::Copy(text)) => copy_to_clipboard(&text),
            Some(Action::SaveIdeas) => {
                if let Err(e) = ideas::save(ideas_path, &app.ideas) {
                    app.status = Some(format!("could not save ideas: {e}"));
                }
            }
            None => {}
        }
    }
    Ok(())
}

/// OSC 52 clipboard write; terminals without support ignore it and the status bar still shows the text.
fn copy_to_clipboard(text: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn render(w: u16, h: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(draw).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{sample_app, Screen, Tab};

    #[test]
    fn draws_empty_model() {
        let mut app = sample_app();
        app.model = Default::default();
        assert!(render(100, 30, |f| draw(f, &app)).contains("no herdr panes yet"));
    }

    #[test]
    fn draws_help_and_fatal() {
        let mut app = sample_app();
        app.show_help = true;
        assert!(render(100, 30, |f| draw(f, &app)).contains("help"));
        app.show_help = false;
        app.fatal = Some("herdr 2.0 speaks protocol 30".into());
        assert!(render(100, 30, |f| draw(f, &app)).contains("protocol 30"));
    }

    #[test]
    fn draws_list_screens() {
        let mut app = sample_app();
        app.screen = Screen::Global(Tab::Threads);
        assert!(render(120, 30, |f| draw(f, &app)).contains("Parser fix"));
    }

    #[test]
    fn draws_the_idea_form_over_the_view() {
        let mut app = sample_app();
        app.screen = Screen::Project {
            key: app.model.projects[0].key.clone(),
            tab: Tab::Ideas,
        };
        app.form = Some(crate::ui::app::IdeaForm {
            editing: None,
            project_key: app.model.projects[0].key.clone(),
            project_name: "beta".into(),
            name: "Export".into(),
            description: "usage to CSV".into(),
            on_description: true,
        });
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("new idea"));
        assert!(out.contains("Export"));
        assert!(out.contains("usage to CSV"));
        assert!(out.contains("Enter save"));
    }

    #[test]
    fn base64_encodes() {
        assert_eq!(base64(b"claude"), "Y2xhdWRl");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
    }

    #[test]
    fn second_jarvis_gets_the_first_ones_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let first = claim_instance(tmp.path(), "w8:p3").unwrap();
        assert!(first.is_ok());
        assert_eq!(
            claim_instance(tmp.path(), "w9:p1").unwrap().unwrap_err(),
            "w8:p3"
        );
        drop(first);
        assert!(claim_instance(tmp.path(), "w9:p1").unwrap().is_ok());
    }

    #[test]
    fn sources_report_timeline_and_collector_before_the_transcript_index() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("claude/projects/-home-u-alpha");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("sess-a.jsonl"),
            include_str!("../../tests/fixtures/claude-session.jsonl"),
        )
        .unwrap();
        let lock = tmp.path().join("collector.lock");
        let _held = collector::try_lock(&lock).unwrap();
        let claude = ClaudeSource::new(&tmp.path().join("claude"), tmp.path().join("index.json"));
        let rx = spawn_sources(
            Some(claude),
            events::Log::new(tmp.path().to_path_buf()),
            lock,
        );
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.threads.is_empty());
        assert!(first.collector_running);
        let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(second.threads.len(), 1);
    }
}
