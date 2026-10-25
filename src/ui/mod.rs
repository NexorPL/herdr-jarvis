mod ansi;
pub mod app;
pub mod hud;
mod line_edit;
pub mod overlays;
pub mod theme;
pub mod views;

use crate::config::Config;
use crate::events::{self, Record};
use crate::herdr::{self, Snapshot, WatchMsg};
use crate::log::log;
use crate::pricing::Pricing;
use crate::projects::Resolver;
use crate::transcripts::Source;
use crate::transcripts::Thread;
use crate::{collector, demo, model, paths, run, store};
use app::{Action, App, Input, Screen, Tab, BOOT_TICKS};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::DefaultTerminal;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
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
        Screen::Panels => views::draw_panels(f, body, app),
    }
    if let Some(p) = &app.picker {
        overlays::picker(f, body, app, p);
    }
    if let Some(form) = &app.form {
        overlays::form(f, body, app, form);
    }
    if let Some(c) = &app.confirm {
        overlays::confirm(f, body, app, c);
    }
    if let Some(a) = &app.answer {
        overlays::answer(f, body, app, a);
    }
    if app.show_help {
        centered(
            f,
            body,
            " help ",
            &[
                "Core:   arrows/hjkl move · 1-9 select · Enter open project",
                "        A agents · T threads · L timeline · U usage · I ideas · G live (all projects)",
                "Lists:  ↑↓/jk move · Enter jump to pane / resume thread · Tab or 1-6 views",
                "        p prompt the selected agent, or answer it when blocked (Agents, Live)",
                "        / search · s state filter · w time range · f project filter",
                "Ideas:  a add · e or Enter edit · d delete (y or Enter on Yes confirms)",
                "        Space todo → doing → done · s status filter · c show done",
                "Run:    x run targets (project screen, agent rows) · n new · e edit · d delete",
                "Agents: N new agent (folder, name) · R rename its workspace · o options",
                "View:   v switch the home screen: animated core or agent panels",
                "Cards:  Enter type into the agent (Esc stops) · g go to its pane",
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
    mut sources: Vec<Source>,
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
            let threads = sources.iter_mut().flat_map(Source::refresh).collect();
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
    let sources = spawn_sources(
        crate::transcripts::sources(config.claude_dir(), &state),
        events::Log::new(state.clone()),
        state.join("collector.lock"),
    );
    let mut app = App::new(&config, Pricing::new(config.pricing.clone()));
    let mut errors = Vec::new();
    match store::load(&state.join("ideas.json")) {
        Ok(list) => app.ideas = list,
        Err(e) => errors.push(e),
    }
    match store::load(&state.join("targets.json")) {
        Ok(list) => app.targets = list,
        Err(e) => errors.push(e),
    }
    // A one-item list, so it shares the store's backup of an unreadable file.
    match store::load(&state.join("options.json")) {
        Ok(list) => app.options = list.into_iter().next().unwrap_or_default(),
        Err(e) => errors.push(e),
    }
    app.screen = app.home();
    if !errors.is_empty() {
        app.status = Some(errors.join(" · "));
    }
    let mut terminal = ratatui::init();
    let me = Me {
        pane: my_pane,
        tab: std::env::var("HERDR_TAB_ID").unwrap_or_default(),
        demo: false,
    };
    let result = event_loop(&mut terminal, &mut app, &watch, &sources, &me, &state);
    ratatui::restore();
    result
}

/// The TUI on `demo` data, for screenshots: no herdr, no collector, state in a temp dir.
pub fn demo() -> anyhow::Result<()> {
    let config = Config::default();
    let state = std::env::temp_dir().join("jarvis-demo");
    std::fs::create_dir_all(&state)?;
    let now = chrono::Utc::now();
    let (snap_tx, watch) = mpsc::channel();
    snap_tx.send(WatchMsg::Snapshot(demo::snapshot()))?;
    let (source_tx, sources) = mpsc::channel();
    source_tx.send(SourceUpdate {
        threads: demo::threads(now),
        records: demo::records(now),
        collector_running: true,
    })?;
    let mut app = App::new(&config, Pricing::new(config.pricing.clone()));
    app.ideas = demo::ideas();
    app.targets = demo::targets();
    let me = Me {
        pane: String::new(),
        tab: String::new(),
        demo: true,
    };
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &watch, &sources, &me, &state);
    ratatui::restore();
    result
}

/// Jarvis's own pane and tab in herdr.
struct Me {
    pane: String,
    tab: String,
    /// `jarvis demo`: actions that would reach herdr are dropped.
    demo: bool,
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    watch: &Receiver<WatchMsg>,
    sources: &Receiver<SourceUpdate>,
    me: &Me,
    state: &Path,
) -> anyhow::Result<()> {
    let start = Instant::now();
    let mut tab_label = String::new();
    let mut snap = Snapshot::default();
    let mut update = SourceUpdate::default();
    let mut resolver = Resolver::default();
    let mut dirty = true;
    let mut screen_read = Instant::now();
    // Results of agent starts and failed keys, which run in threads of their own.
    let (started_tx, started) = mpsc::channel::<String>();
    let read_plan = Arc::new(Mutex::new(ReadPlan::default()));
    let typed = Arc::new(AtomicBool::new(false));
    let screens = match me.demo {
        true => mpsc::channel().1,
        false => spawn_reader(read_plan.clone(), typed.clone()),
    };
    let keys = spawn_sender(started_tx.clone(), typed.clone());
    while !app.quit {
        if let Some(msg) = started.try_iter().last() {
            app.status = Some(msg);
        }
        // Cards show each agent's screen. herdr is read off the UI thread, so typing never waits
        // on it; the demo makes its screens up.
        let live = app.screen != Screen::Core && app.tab() == Tab::Live;
        if me.demo {
            if live && app.screens.is_empty() {
                app.screens = (app.visible_agents().into_iter())
                    .map(|(_, a)| {
                        let text = demo::screen(a.pane.title(), a.pane.agent_status);
                        (a.pane.pane_id.clone(), ansi::lines(&text))
                    })
                    .collect();
            }
        } else {
            if let Ok(mut plan) = read_plan.lock() {
                plan.live = match live {
                    true => (app.visible_agents().into_iter())
                        .map(|(_, a)| a.pane.pane_id.clone())
                        .collect(),
                    false => Vec::new(),
                };
                plan.typing = app.typing.clone();
            }
            app.screens.extend(
                screens
                    .try_iter()
                    .map(|(pane, text)| (pane, ansi::lines(&text))),
            );
        }
        // The answer popup shows the agent's screen: read on open, after input and every 500 ms.
        if let Some(a) = app.answer.as_mut() {
            if a.screen.is_empty() || screen_read.elapsed() >= Duration::from_millis(500) {
                a.screen = if me.demo {
                    demo::question().into()
                } else {
                    read_screen(&a.pane_id, "text")
                        .unwrap_or_else(|e| format!("could not read the agent: {e}"))
                };
                screen_read = Instant::now();
            }
        }
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
            let selected = app.selection();
            app.model = model::build(
                &snap,
                &update.threads,
                &update.records,
                &app.pricing,
                &mut resolver,
                &today,
            );
            app.reselect(selected);
            dirty = false;
            // Forget the screens of closed panes, and stop typing into one.
            let open: std::collections::HashSet<&str> = (app.model.projects.iter())
                .flat_map(|p| &p.agents)
                .map(|a| a.pane.pane_id.as_str())
                .collect();
            app.screens.retain(|pane, _| open.contains(pane.as_str()));
            if app.typing.as_deref().is_some_and(|p| !open.contains(p)) {
                app.typing = None;
                app.status = Some("the agent you were typing into is gone".into());
            }
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
        // Typing into a card redraws as fast as animating, so its screen follows the keys.
        let fast = app.animating() || app.typing.is_some();
        let timeout = Duration::from_millis(if fast { 33 } else { 250 });
        if event::poll(timeout)? {
            // Every key already waiting is handled before the next draw; keys typed into an agent
            // go straight to the sender thread, other actions are handled below one at a time.
            loop {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        let text_input = app.form.is_some() || app.search_editing;
                        let next = || next_press(Duration::from_millis(50));
                        for key in pasted_line_breaks(key, text_input, next)? {
                            app.on_key(key);
                        }
                    }
                }
                if !me.demo && send_typed(&mut app.action, &keys) {
                    // The answer popup shows the agent's reaction on the next pass.
                    screen_read = (Instant::now().checked_sub(Duration::from_millis(400)))
                        .unwrap_or_else(Instant::now);
                }
                if app.action.is_some() || app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if me.demo
            && matches!(
                app.action,
                Some(
                    Action::FocusPane(_)
                        | Action::Prompt { .. }
                        | Action::Input { .. }
                        | Action::Run { .. }
                        | Action::Spawn { .. }
                        | Action::RenameWorkspace { .. }
                )
            )
        {
            app.action = None;
            app.status = Some("demo: nothing is sent to herdr".into());
        }
        match app.action.take() {
            // Jarvis stays open in its own tab; the agent's pane takes the focus.
            Some(Action::FocusPane(id)) => {
                if let Err(e) = herdr::focus_pane(&herdr::socket_path(), &id) {
                    app.status = Some(format!("could not focus {id}: {e}"));
                }
            }
            Some(Action::Copy(text)) => copy_to_clipboard(&text),
            // Only in the demo, which drops it above; elsewhere the sender thread has it.
            Some(Action::Input { .. }) => {}
            Some(Action::Prompt {
                pane_id,
                agent,
                text,
            }) => {
                let params = serde_json::json!({"target": pane_id, "text": text});
                app.status = Some(
                    match herdr::client::request(&herdr::socket_path(), "agent.prompt", params) {
                        Ok(_) => format!("sent to {agent}"),
                        Err(e) => format!("could not prompt {agent}: {e}"),
                    },
                );
            }
            Some(Action::SaveIdeas) => {
                if let Err(e) = store::save(&state.join("ideas.json"), &app.ideas) {
                    app.status = Some(format!("could not save ideas: {e}"));
                }
            }
            Some(Action::SaveOptions) => {
                let options = [app.options.clone()];
                if let Err(e) = store::save(&state.join("options.json"), &options) {
                    app.status = Some(format!("could not save options: {e}"));
                }
            }
            Some(Action::Spawn { agent, cwd, name }) => {
                app.status = Some(format!("starting {agent} in a new workspace {name}…"));
                let tx = started_tx.clone();
                std::thread::spawn(move || {
                    let msg = match spawn_agent(&agent, &cwd, &name) {
                        Ok(()) => format!("{agent} started in workspace {name}"),
                        Err(e) => format!("could not start {agent}: {e}"),
                    };
                    let _ = tx.send(msg);
                });
            }
            Some(Action::RenameWorkspace {
                workspace_id,
                label,
            }) => {
                let params = serde_json::json!({"workspace_id": workspace_id, "label": label});
                if let Err(e) =
                    herdr::client::request(&herdr::socket_path(), "workspace.rename", params)
                {
                    app.status = Some(format!("could not rename the workspace: {e}"));
                }
            }
            Some(Action::SaveTargets) => {
                if let Err(e) = store::save(&state.join("targets.json"), &app.targets) {
                    app.status = Some(format!("could not save run targets: {e}"));
                }
            }
            Some(Action::Run {
                workspace_id,
                steps,
            }) => {
                let sock = herdr::socket_path();
                let send = |method: &str, params| herdr::client::request(&sock, method, params);
                match run::execute(workspace_id.as_deref(), &steps, send) {
                    Ok(Some(pane)) => {
                        if let Err(e) = herdr::focus_pane(&sock, &pane) {
                            app.status = Some(format!("could not focus {pane}: {e}"));
                        }
                    }
                    Ok(None) => {}
                    Err(e) => app.status = Some(format!("run: {e}")),
                }
            }
            None => {}
        }
    }
    Ok(())
}

/// Pastes arrive as keys (crossterm reports no paste events on Windows): a line feed as Ctrl+Enter,
/// a carriage return as a plain Enter, which would submit a form halfway and send the rest of the
/// paste to Jarvis's shortcuts. In text input, an Enter that another key follows within the window
/// of `next` is pasted: it becomes a line break, one with the line feed of a CR LF pair. Nobody
/// types the next key that fast, so a typed Enter still submits.
fn pasted_line_breaks(
    mut key: KeyEvent,
    text_input: bool,
    mut next: impl FnMut() -> std::io::Result<Option<KeyEvent>>,
) -> std::io::Result<Vec<KeyEvent>> {
    let line_break = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
    let mut keys = Vec::new();
    while text_input && key.code == KeyCode::Enter && key.modifiers.is_empty() {
        let Some(following) = next()? else { break };
        if !app::is_line_break(following) {
            keys.push(line_break);
        }
        key = following;
    }
    keys.push(key);
    Ok(keys)
}

/// The next key press within `within`, skipping releases and other events.
fn next_press(within: Duration) -> std::io::Result<Option<KeyEvent>> {
    let deadline = Instant::now() + within;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if !event::poll(left)? {
            break;
        }
        if let Event::Key(k) = event::read()? {
            if k.kind == KeyEventKind::Press {
                return Ok(Some(k));
            }
        }
    }
    Ok(None)
}

/// Which agent screens the reader thread keeps fresh.
#[derive(Default)]
struct ReadPlan {
    /// The cards on screen, read once a second.
    live: Vec<String>,
    /// The card being typed into, read every 80 ms and right after keys go out.
    typing: Option<String>,
}

fn spawn_reader(plan: Arc<Mutex<ReadPlan>>, typed: Arc<AtomicBool>) -> Receiver<(String, String)> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut all_read, mut typing_read) =
            (Instant::now() - Duration::from_secs(5), Instant::now());
        loop {
            std::thread::sleep(Duration::from_millis(40));
            let (live, typing) = match plan.lock() {
                Ok(p) => (p.live.clone(), p.typing.clone()),
                Err(_) => return,
            };
            let read = |pane: &str| {
                let text = read_screen(pane, "ansi")
                    .unwrap_or_else(|e| format!("could not read the agent: {e}"));
                tx.send((pane.to_string(), text)).is_ok()
            };
            if let Some(pane) = &typing {
                if typed.swap(false, Ordering::Relaxed)
                    || typing_read.elapsed() >= Duration::from_millis(80)
                {
                    if !read(pane) {
                        return;
                    }
                    typing_read = Instant::now();
                }
            }
            if !live.is_empty() && all_read.elapsed() >= Duration::from_secs(1) {
                for pane in live.iter().filter(|p| Some(*p) != typing.as_ref()) {
                    if !read(pane) {
                        return;
                    }
                    // Keys typed meanwhile show without waiting for the rest of the cards.
                    if let Some(t) = typing
                        .as_deref()
                        .filter(|_| typed.swap(false, Ordering::Relaxed))
                    {
                        if !read(t) {
                            return;
                        }
                    }
                }
                all_read = Instant::now();
            }
        }
    });
    rx
}

/// Sends keys to agents in order; text typed in a burst goes out as one `pane.send_text`.
fn spawn_sender(
    status: mpsc::Sender<String>,
    typed: Arc<AtomicBool>,
) -> mpsc::Sender<(String, Input)> {
    let (tx, rx) = mpsc::channel::<(String, Input)>();
    std::thread::spawn(move || {
        let sock = herdr::socket_path();
        while let Ok(first) = rx.recv() {
            let batch = coalesce(std::iter::once(first).chain(rx.try_iter()));
            for (pane_id, input) in batch {
                let sent = match input {
                    Input::Key(k) => herdr::client::request(
                        &sock,
                        "agent.send_keys",
                        serde_json::json!({"target": pane_id, "keys": [k]}),
                    ),
                    Input::Text(t) => herdr::client::request(
                        &sock,
                        "pane.send_text",
                        serde_json::json!({"pane_id": pane_id, "text": t}),
                    ),
                };
                if let Err(e) = sent {
                    let _ = status.send(format!("could not type into the agent: {e}"));
                }
            }
            typed.store(true, Ordering::Relaxed);
        }
    });
    tx
}

/// Hands a key typed into an agent to the sender thread; any other action stays for the loop.
fn send_typed(action: &mut Option<Action>, keys: &mpsc::Sender<(String, Input)>) -> bool {
    match action.take() {
        Some(Action::Input { pane_id, input }) => {
            let _ = keys.send((pane_id, input));
            true
        }
        other => {
            *action = other;
            false
        }
    }
}

/// Consecutive text for the same pane joined into one input.
fn coalesce(inputs: impl Iterator<Item = (String, Input)>) -> Vec<(String, Input)> {
    let mut batch: Vec<(String, Input)> = Vec::new();
    for (pane, input) in inputs {
        match (batch.last_mut(), &input) {
            (Some((p, Input::Text(t))), Input::Text(more)) if *p == pane => t.push_str(more),
            _ => batch.push((pane, input)),
        }
    }
    batch
}

/// A new herdr workspace in `cwd` labelled `name`, with `agent` started in its first pane; herdr
/// answers once the agent is ready for input.
fn spawn_agent(agent: &str, cwd: &str, name: &str) -> anyhow::Result<()> {
    let sock = herdr::socket_path();
    let params = serde_json::json!({"cwd": cwd, "label": name, "focus": false});
    let created = herdr::client::request(&sock, "workspace.create", params)?;
    let pane = created["root_pane"]["pane_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("herdr returned no pane for the new workspace"))?;
    let params =
        serde_json::json!({"name": name, "kind": agent, "pane_id": pane, "timeout_ms": 60000});
    herdr::client::request(&sock, "agent.start", params)?;
    Ok(())
}

/// The visible terminal text of the agent in `pane_id`.
fn read_screen(pane_id: &str, format: &str) -> anyhow::Result<String> {
    let params = serde_json::json!({"target": pane_id, "source": "visible", "format": format});
    let result = herdr::client::request(&herdr::socket_path(), "agent.read", params)?;
    Ok(result["read"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string())
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
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn pasted_carriage_returns_become_line_breaks() {
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        let (enter, lf) = (
            k(KeyCode::Enter),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
        );
        let run = |first, text_input, following: Vec<KeyEvent>| {
            let mut following = following.into_iter();
            pasted_line_breaks(first, text_input, || Ok(following.next())).unwrap()
        };
        // Typed: nothing follows within the window, so Enter submits.
        assert_eq!(run(enter, true, vec![]), [enter]);
        // CR LF: one line break. A lone CR before more text, or before another CR: each one.
        assert_eq!(run(enter, true, vec![lf]), [lf]);
        let x = k(KeyCode::Char('x'));
        assert_eq!(run(enter, true, vec![x]), [lf, x]);
        assert_eq!(run(enter, true, vec![enter, x]), [lf, lf, x]);
        // Outside text input, and for other keys, nothing waits.
        assert_eq!(run(enter, false, vec![x]), [enter]);
        assert_eq!(run(x, true, vec![enter]), [x]);
    }

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
        app.form = Some(crate::ui::app::Form {
            title: " new idea · beta ".into(),
            kind: crate::ui::app::FormKind::Idea {
                editing: None,
                project_key: app.model.projects[0].key.clone(),
                project_name: "beta".into(),
            },
            fields: vec![
                ("name", "Export".into()),
                ("description", "usage to CSV".into()),
            ],
            focus: 1,
            cursor: usize::MAX,
            error: Some("an idea needs a name".into()),
        });
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("new idea"));
        assert!(out.contains("Export"));
        assert!(out.contains("usage to CSV"));
        assert!(out.contains("Enter save"));
        assert!(out.contains("an idea needs a name"));
    }

    #[test]
    fn draws_the_delete_popup_with_no_selected() {
        let mut app = sample_app();
        app.confirm = Some(crate::ui::app::Confirm {
            what: crate::ui::app::Doomed::Idea(0),
            name: "Cache".into(),
            yes: false,
        });
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("Delete \"Cache\"?"), "{out}");
        assert!(out.contains("[ Yes ]"));
        assert!(out.contains("[ No ]"));
    }

    #[test]
    fn draws_the_run_picker_with_running_marks_and_layout_hint() {
        let mut app = sample_app();
        app.model
            .pane_labels
            .insert("alpha:api".into(), "w1:p7".into());
        let alpha = app.model.projects[1].key.clone();
        let target = |name: &str, command: &str| crate::run::Target {
            project_key: alpha.clone(),
            name: name.into(),
            command: command.into(),
            cwd: String::new(),
            env: Default::default(),
        };
        app.targets = vec![target("api", "cargo run"), target("web", "pnpm dev")];
        app.screen = Screen::Project {
            key: alpha.clone(),
            tab: Tab::Agents,
        };
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("run · alpha"));
        assert!(out.contains("[ ] api"));
        assert!(out.contains("● running"));
        assert!(out.contains("pnpm dev"));
        assert!(out.contains("Space select"));
        assert!(out.contains("n new · e edit · d delete"));
        app.picker.as_mut().unwrap().choosing_layout = true;
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("s side by side"));
    }

    #[test]
    fn draws_an_empty_picker_with_a_hint() {
        let mut app = sample_app();
        app.screen = Screen::Project {
            key: app.model.projects[1].key.clone(),
            tab: Tab::Agents,
        };
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("run · alpha"));
        assert!(out.contains("no targets yet · n to add"));
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
        let claude = Source::new(
            crate::transcripts::Kind::Claude,
            &tmp.path().join("claude"),
            tmp.path().join("index.json"),
        );
        let rx = spawn_sources(
            vec![claude],
            events::Log::new(tmp.path().to_path_buf()),
            lock,
        );
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first.threads.is_empty());
        assert!(first.collector_running);
        let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(second.threads.len(), 1);
    }

    #[test]
    fn typed_text_goes_out_in_one_piece() {
        let t = |p: &str, s: &str| (p.to_string(), Input::Text(s.into()));
        let k = |p: &str, s: &str| (p.to_string(), Input::Key(s.into()));
        let got = coalesce(
            [
                t("a", "h"),
                t("a", "i"),
                k("a", "enter"),
                t("a", "x"),
                t("b", "y"),
                t("b", "z"),
            ]
            .into_iter(),
        );
        assert_eq!(
            got,
            [t("a", "hi"), k("a", "enter"), t("a", "x"), t("b", "yz")]
        );
    }

    #[test]
    fn only_typed_keys_leave_for_the_sender() {
        let (tx, rx) = mpsc::channel();
        let mut action = Some(Action::Prompt {
            pane_id: "w1:p1".into(),
            agent: "a".into(),
            text: "go".into(),
        });
        assert!(!send_typed(&mut action, &tx));
        assert!(
            matches!(action, Some(Action::Prompt { .. })),
            "the prompt stays for the loop"
        );
        let mut action = Some(Action::Input {
            pane_id: "w1:p1".into(),
            input: Input::Text("x".into()),
        });
        assert!(send_typed(&mut action, &tx));
        assert_eq!(action, None);
        assert_eq!(rx.try_recv().unwrap().1, Input::Text("x".into()));
    }
}
