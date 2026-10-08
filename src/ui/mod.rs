pub mod app;
pub mod hud;
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
use crate::{collector, model, paths};
use app::{Action, App, Screen, BOOT_TICKS};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use ratatui::DefaultTerminal;
use std::io::Write;
use std::path::PathBuf;
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
    if app.show_help {
        centered(
            f,
            body,
            " help ",
            &[
                "Core:   arrows/hjkl move · 1-9 select · Enter open project",
                "        A agents · T threads · L timeline · U usage (all projects)",
                "Lists:  ↑↓/jk move · Enter jump to pane / resume thread · Tab or 1-4 views",
                "        / search · s state filter · w time range · f project filter",
                "        Esc back · r refresh · q quit · ? this help",
            ],
            app,
        );
    }
}

fn centered(f: &mut Frame, area: Rect, title: &str, lines: &[&str], app: &App) {
    let w = 84.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines.iter().map(|l| Line::from(*l)).collect::<Vec<_>>())
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .title(title.to_string())
                    .border_style(Style::new().fg(app.palette.accent)),
            ),
        rect,
    );
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
    std::thread::spawn(move || loop {
        let now = chrono::Utc::now();
        let update = SourceUpdate {
            threads: claude.as_mut().map(|c| c.refresh()).unwrap_or_default(),
            records: store.read_since(now - chrono::Duration::days(7), now),
            collector_running: collector::is_running(&lock),
        };
        if tx.send(update).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    });
    rx
}

pub fn run() -> anyhow::Result<()> {
    let config = Config::load(&paths::config_dir());
    if let Err(e) = collector::ensure_running() {
        log(format!("tui: could not start collector: {e}"));
    }
    let state = paths::state_dir();
    std::fs::create_dir_all(&state)?;
    let watch = herdr::watcher::spawn(herdr::socket_path());
    let claude = config
        .claude_dir()
        .map(|dir| ClaudeSource::new(&dir, state.join("claude-index.json")));
    let sources = spawn_sources(
        claude,
        events::Log::new(state.clone()),
        state.join("collector.lock"),
    );
    let mut app = App::new(&config, Pricing::new(config.pricing.clone()));
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &watch, &sources);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    watch: &Receiver<WatchMsg>,
    sources: &Receiver<SourceUpdate>,
) -> anyhow::Result<()> {
    let start = Instant::now();
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
            app.collector_running = update.collector_running;
            app.clamp();
            dirty = false;
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
            // The overlay restores the previous focus when it closes, so focus from a detached
            // process after the overlay is gone.
            Some(Action::FocusPane(id)) => {
                collector::spawn_detached(&["focus", &id])?;
                app.quit = true;
            }
            Some(Action::Copy(text)) => copy_to_clipboard(&text),
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
    fn base64_encodes() {
        assert_eq!(base64(b"claude"), "Y2xhdWRl");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"a"), "YQ==");
    }
}
