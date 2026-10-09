# Project Ideas and Run Targets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-project idea list (issue #2) and starting project run targets from `.jarvis/run.toml` in herdr panes (issue #8) to the Jarvis TUI.

**Architecture:** Two new pure modules: `src/ideas.rs` (JSON store) and `src/run.rs` (run.toml parser, request plan, request executor with an injected `send`). `App` stays free of I/O: it holds the ideas, the idea form and the run picker, and asks for side effects through new `Action` variants that `ui/mod.rs` performs (save ideas, load targets, send herdr requests). Rendering adds an Ideas tab in `views.rs` and two popups in a new `ui/overlays.rs`.

**Tech Stack:** Rust 2021 (MSRV 1.89), ratatui 0.30, serde/serde_json, toml 0.8, herdr socket API protocol 22.

**Spec:** `docs/superpowers/specs/2026-10-09-ideas-and-run-targets-design.md`

## Global Constraints

- No new dependencies; `Cargo.toml` stays as is.
- `rust-version = "1.89"`; must build on Linux, macOS and Windows.
- CI gates: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.
- Ideas file: `<state_dir>/ideas.json`, written through `ideas.json.tmp` + rename; unreadable file copied to `ideas.json.bad`.
- Run file: `<root>/.jarvis/run.toml`, `[[target]]` with `name`, `command`, `cwd` (default `"."`), optional `env`.
- Pane label of a started target: `<project>:<target>`; that label is how running targets are recognized.
- herdr requests: `tab.create`, `pane.split` (`direction = "right"`), `pane.rename`, `pane.send_input` with `keys = ["Enter"]`, `pane.focus`.
- Commits: Conventional Commits, author `13576711+NexorPL@users.noreply.github.com` (already set in this repo), no Claude attribution or co-author trailer. Never commit on `main`; work on `feat/ideas-and-run-targets`.
- Keys already taken in list views: `q ? r A T L U Esc Tab BackTab 1-9 ↑↓ j k Enter / s w f`. New: `I` (global ideas), `a e d` (Ideas tab only), `x` (run picker).

## Review Focus

- A command sent right after `tab.create`, before the shell (a slow PowerShell profile) shows its prompt: the command must still run. Not unit-testable; checked in Task 7's manual run on Windows.
- Non-ASCII text in the idea form (`Zażółć`) and Backspace on it: one character removed, no panic on a char boundary. Test in Task 2.
- Editing or deleting while the global Ideas view is filtered by search: the action hits the idea on screen, not the one at the same position in the file. Test in Task 2.
- A `run.toml` with extra keys or a TOML syntax error: extra keys ignored; a syntax error becomes a one-line status message, not a crash or a multi-line dump. Test in Task 4.
- `q`, `Esc`, `?` and digits while the idea form or the picker is open: typed into the form or handled by the popup, never quitting Jarvis or switching screens. Tests in Task 2 and Task 5.

---

### Task 1: Ideas store

**Files:**
- Create: `src/ideas.rs`
- Modify: `src/main.rs` (add `mod ideas;` after `mod herdr;`)

**Interfaces:**
- Produces: `pub struct Idea { pub project_key: String, pub project_name: String, pub name: String, pub description: String }` (derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`); `pub fn load(path: &Path) -> Result<Vec<Idea>, String>`; `pub fn save(path: &Path, ideas: &[Idea]) -> std::io::Result<()>`.

- [ ] **Step 1: Write the failing tests**

Create `src/ideas.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn idea(name: &str) -> Idea {
        Idea {
            project_key: "/home/u/alpha".into(),
            project_name: "alpha".into(),
            name: name.into(),
            description: format!("about {name}"),
        }
    }

    #[test]
    fn missing_file_is_no_ideas() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(&tmp.path().join("ideas.json")).unwrap(), vec![]);
    }

    #[test]
    fn save_then_load_round_trips_without_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        save(&path, &[idea("a"), idea("Zażółć")]).unwrap();
        assert_eq!(load(&path).unwrap(), vec![idea("a"), idea("Zażółć")]);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn unreadable_file_is_backed_up_and_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ideas.json");
        fs::write(&path, "{not json").unwrap();
        let err = load(&path).unwrap_err();
        assert!(err.contains("ideas.json.bad"), "{err}");
        assert_eq!(
            fs::read_to_string(path.with_extension("json.bad")).unwrap(),
            "{not json"
        );
    }
}
```

Add `mod ideas;` to `src/main.rs` after `mod herdr;`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test ideas::`
Expected: compile errors `cannot find type Idea` / `cannot find function load`.

- [ ] **Step 3: Implement**

Put this above the tests in `src/ideas.rs`:

```rust
//! Per-project ideas, stored as one JSON array in the plugin state directory.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// An idea belongs to a project, not to an agent: agents come and go, ideas stay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Idea {
    pub project_key: String,
    /// Kept so ideas of projects without live agents still show a readable name.
    pub project_name: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// A missing file is no ideas. An unreadable one is copied to `ideas.json.bad`, so the next save cannot lose it.
pub fn load(path: &Path) -> Result<Vec<Idea>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    serde_json::from_str(&text).map_err(|e| {
        let bad = path.with_extension("json.bad");
        let _ = std::fs::copy(path, &bad);
        format!("ideas file unreadable ({e}); kept a copy in {}", bad.display())
    })
}

/// Rewrites the whole file through a temporary one, so a crash never leaves half a file.
pub fn save(path: &Path, ideas: &[Idea]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(ideas)?)?;
    std::fs::rename(&tmp, path)
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test ideas::`
Expected: 3 passed. (Dead-code warnings for `Idea`/`load`/`save` are expected until Task 2.)

- [ ] **Step 5: Commit**

```bash
git add src/ideas.rs src/main.rs
git commit -m "feat: store per-project ideas in the plugin state directory"
```

---

### Task 2: Ideas tab, form and delete in `App`, saved by the UI loop

**Files:**
- Modify: `src/ui/app.rs` (Tab, Action, App fields, key handling, tests)
- Modify: `src/ui/mod.rs` (load ideas at start, handle `Action::SaveIdeas`)
- Modify: `src/ui/views.rs` (one match arm so it compiles; real view in Task 3)

**Interfaces:**
- Consumes: `crate::ideas::{Idea, load, save}` from Task 1.
- Produces:
  - `Tab::Ideas` (fifth in `Tab::ALL: [Tab; 5]`, title `"Ideas"`).
  - `Action::SaveIdeas`; `Action` now derives `Debug, Clone, PartialEq` (no `Eq`; Task 5 adds a variant holding `f32`).
  - `pub struct IdeaForm { pub editing: Option<usize>, pub project_key: String, pub project_name: String, pub name: String, pub description: String, pub on_description: bool }` with `pub fn field(&mut self) -> &mut String`.
  - `App` fields `pub ideas: Vec<Idea>`, `pub form: Option<IdeaForm>`, `pub confirm_delete: Option<usize>`.
  - `pub fn visible_ideas(&self) -> Vec<(usize, &Idea)>`: index into `ideas` plus the idea, scoped and searched.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `src/ui/app.rs` (add `use crate::ideas::Idea;` to the test module imports):

```rust
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
        assert_eq!(app.status.as_deref(), Some("delete \"Cache\"? y/n"));
        key(&mut app, KeyCode::Char('n'));
        assert_eq!(app.ideas.len(), 1);
        assert_eq!(app.action, None);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        assert!(app.ideas.is_empty());
        assert_eq!(app.action, Some(Action::SaveIdeas));
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
        assert_eq!(app.form.as_ref().unwrap().name, "Zażó");
    }

    #[test]
    fn global_ideas_need_a_filter_to_add_and_list_every_project() {
        let mut app = sample_app();
        app.ideas = vec![idea(&app, 0, "beta idea"), idea(&app, 1, "alpha idea")];
        key(&mut app, KeyCode::Char('I'));
        assert_eq!(app.visible_ideas().len(), 2);
        key(&mut app, KeyCode::Char('a'));
        assert!(app.form.is_none());
        assert!(app.status.as_deref().unwrap().contains("pick a project with f"));
        key(&mut app, KeyCode::Char('f'));
        assert_eq!(app.visible_ideas().len(), 1);
        key(&mut app, KeyCode::Char('a'));
        assert_eq!(app.form.as_ref().unwrap().project_name, "beta");
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
        assert_eq!(app.form.as_ref().unwrap().editing, Some(1));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(app.ideas, vec![idea(&app, 0, "first")]);
    }
```

Also update the existing test `digits_select_and_letters_open_global_views` only if it breaks (it should not).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test ui::app::`
Expected: compile errors (`Tab::Ideas`, `Action::SaveIdeas`, `app.ideas`, `app.form` unknown).

- [ ] **Step 3: Implement in `src/ui/app.rs`**

Imports: add `use crate::ideas::Idea;`.

Tab:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Agents,
    Threads,
    Timeline,
    Usage,
    Ideas,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Agents, Tab::Threads, Tab::Timeline, Tab::Usage, Tab::Ideas];

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
```

Action and form:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    FocusPane(String),
    Copy(String),
    /// `ideas` changed; write them to disk.
    SaveIdeas,
}

/// The add/edit idea popup; while open it takes every key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaForm {
    /// Index into `App::ideas` when editing; `None` adds a new idea.
    pub editing: Option<usize>,
    pub project_key: String,
    pub project_name: String,
    pub name: String,
    pub description: String,
    pub on_description: bool,
}

impl IdeaForm {
    pub fn field(&mut self) -> &mut String {
        if self.on_description {
            &mut self.description
        } else {
            &mut self.name
        }
    }
}
```

`App` fields (add after `last_left`), and initialize them in `App::new` as `ideas: Vec::new(), form: None, confirm_delete: None`:

```rust
    pub ideas: Vec<Idea>,
    pub form: Option<IdeaForm>,
    /// Index into `ideas` waiting for `y` to be deleted.
    pub confirm_delete: Option<usize>,
```

`visible_ideas` (next to `visible_events`):

```rust
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
```

`row_count`: add `Tab::Ideas => self.visible_ideas().len(),`.

`on_key`: after the `show_help` block and before `search_editing`:

```rust
        if self.form.is_some() {
            self.on_form_key(key);
            return;
        }
        if let Some(i) = self.confirm_delete.take() {
            self.status = None;
            if key.code == KeyCode::Char('y') && i < self.ideas.len() {
                self.ideas.remove(i);
                self.action = Some(Action::SaveIdeas);
                self.clamp();
            }
            return;
        }
```

and in its global key match add `KeyCode::Char('I') => self.goto(Screen::Global(Tab::Ideas)),` after the `U` line.

`on_list_key`: replace the tab-switch lines with length-based ones and add the Ideas keys before `_ => {}`:

```rust
            KeyCode::Tab => self.set_tab(Tab::ALL[(tab_index + 1) % Tab::ALL.len()]),
            KeyCode::BackTab => {
                self.set_tab(Tab::ALL[(tab_index + Tab::ALL.len() - 1) % Tab::ALL.len()])
            }
            KeyCode::Char(c @ '1'..='5') => self.set_tab(Tab::ALL[c as usize - '1' as usize]),
```

```rust
            KeyCode::Char('a') if self.tab() == Tab::Ideas => self.add_idea(),
            KeyCode::Char('e') if self.tab() == Tab::Ideas => self.edit_idea(),
            KeyCode::Char('d') if self.tab() == Tab::Ideas => {
                let target = (self.visible_ideas().get(self.selected_row))
                    .map(|(i, idea)| (*i, idea.name.clone()));
                if let Some((i, name)) = target {
                    self.status = Some(format!("delete \"{name}\"? y/n"));
                    self.confirm_delete = Some(i);
                }
            }
```

`activate_row`: change `Tab::Timeline | Tab::Usage => {}` to

```rust
            Tab::Ideas => self.edit_idea(),
            Tab::Timeline | Tab::Usage => {}
```

New methods in `impl App`:

```rust
    fn add_idea(&mut self) {
        let Some(key) = self.scope().map(str::to_string) else {
            self.status = Some("pick a project with f to add an idea here".into());
            return;
        };
        self.form = Some(IdeaForm {
            editing: None,
            project_name: self.project_name(&key),
            project_key: key,
            name: String::new(),
            description: String::new(),
            on_description: false,
        });
    }

    fn edit_idea(&mut self) {
        let target = (self.visible_ideas().get(self.selected_row))
            .map(|(i, idea)| (*i, (*idea).clone()));
        if let Some((i, idea)) = target {
            self.form = Some(IdeaForm {
                editing: Some(i),
                project_key: idea.project_key,
                project_name: idea.project_name,
                name: idea.name,
                description: idea.description,
                on_description: false,
            });
        }
    }

    fn on_form_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.form = None,
            KeyCode::Enter => self.submit_form(),
            code => {
                let Some(form) = self.form.as_mut() else {
                    return;
                };
                match code {
                    KeyCode::Tab | KeyCode::BackTab => form.on_description = !form.on_description,
                    KeyCode::Backspace => {
                        form.field().pop();
                    }
                    KeyCode::Char(c) => form.field().push(c),
                    _ => {}
                }
            }
        }
    }

    fn submit_form(&mut self) {
        let Some(form) = self.form.take() else {
            return;
        };
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.status = Some("an idea needs a name".into());
            self.form = Some(form);
            return;
        }
        let idea = Idea {
            name,
            description: form.description.trim().to_string(),
            project_key: form.project_key,
            project_name: form.project_name,
        };
        match form.editing {
            Some(i) if i < self.ideas.len() => self.ideas[i] = idea,
            _ => self.ideas.push(idea),
        }
        self.status = None;
        self.action = Some(Action::SaveIdeas);
    }
```

- [ ] **Step 4: Make `views.rs` compile**

In `src/ui/views.rs` `draw`, add to the tab match: `Tab::Ideas => empty(f, body, "no ideas yet", pal),` (Task 3 replaces it).

- [ ] **Step 5: Wire load and save in `src/ui/mod.rs`**

Imports: change `use crate::{collector, model, paths};` to `use crate::{collector, ideas, model, paths};`.

In `run()`, right after `let mut app = App::new(...)`:

```rust
    let ideas_path = state.join("ideas.json");
    match ideas::load(&ideas_path) {
        Ok(list) => app.ideas = list,
        Err(e) => app.status = Some(e),
    }
```

Pass it on: `event_loop(&mut terminal, &mut app, &watch, &sources, &me, &ideas_path)`, and add the parameter `ideas_path: &Path,` to `event_loop`. In the `match app.action.take()` add:

```rust
            Some(Action::SaveIdeas) => {
                if let Err(e) = ideas::save(ideas_path, &app.ideas) {
                    app.status = Some(format!("could not save ideas: {e}"));
                }
            }
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test`
Expected: all pass, including the 6 new `ui::app` tests and the existing `views` tests that loop over `Tab::ALL` (the Ideas placeholder contains `"no "`).

- [ ] **Step 7: Commit**

```bash
git add src/ui/app.rs src/ui/mod.rs src/ui/views.rs
git commit -m "feat: add, edit and delete project ideas in a new Ideas tab"
```

---

### Task 3: Draw the Ideas tab and the idea form

**Files:**
- Create: `src/ui/overlays.rs`
- Modify: `src/ui/mod.rs` (`pub mod overlays;`, draw the form, `centered` reuses `popup`, help text)
- Modify: `src/ui/views.rs` (ideas table, footer hints, tests)

**Interfaces:**
- Consumes: `App::visible_ideas`, `App::form`, `IdeaForm` (Task 2).
- Produces: `pub fn popup(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, pal: &Palette)` and `pub fn idea_form(f: &mut Frame, area: Rect, app: &App, form: &IdeaForm)` in `ui::overlays`. Task 6 adds `picker` next to them.

- [ ] **Step 1: Write the failing tests**

In `src/ui/views.rs` tests:

```rust
    #[test]
    fn ideas_show_name_description_and_project_globally() {
        let mut app = sample_app();
        app.ideas = vec![crate::ideas::Idea {
            project_key: app.model.projects[1].key.clone(),
            project_name: "alpha".into(),
            name: "Pricing cache".into(),
            description: "keep prices for 24h".into(),
        }];
        app.screen = Screen::Global(Tab::Ideas);
        let out = screen(&app, 120, 30);
        assert!(out.contains("Pricing cache"));
        assert!(out.contains("alpha"));
        assert!(out.contains("keep prices for 24h"));
        assert!(out.contains("a add"));
    }
```

In `src/ui/mod.rs` tests:

```rust
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
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test ui::`
Expected: the two new tests fail (no ideas table, no form drawn).

- [ ] **Step 3: Implement `src/ui/overlays.rs`**

```rust
//! Popups drawn over the current screen: the idea form and the run picker.

use super::app::{App, IdeaForm};
use super::theme::Palette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

/// A bordered box centered in `area`, sized to its lines.
pub fn popup(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, pal: &Palette) {
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
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .title(title.to_string())
                .border_style(Style::new().fg(pal.accent)),
        ),
        rect,
    );
}

pub fn idea_form(f: &mut Frame, area: Rect, app: &App, form: &IdeaForm) {
    let pal = &app.palette;
    let field = |label: &str, value: &str, focused: bool| {
        let style = if focused {
            Style::new().fg(pal.accent).bold()
        } else {
            Style::new().fg(pal.dim)
        };
        Line::from(vec![
            Span::styled(format!("{label:<12} "), style),
            Span::raw(value.to_string()),
            Span::styled(if focused { "▏" } else { "" }, Style::new().fg(pal.accent)),
        ])
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("{:<12} {}", "project", form.project_name),
            Style::new().fg(pal.dim),
        )),
        Line::raw(""),
        field("name", &form.name, !form.on_description),
        field("description", &form.description, form.on_description),
        Line::raw(""),
        Line::from(Span::styled(
            "Tab switch field · Enter save · Esc cancel",
            Style::new().fg(pal.dim),
        )),
    ];
    let title = if form.editing.is_some() {
        " edit idea "
    } else {
        " new idea "
    };
    popup(f, area, title, lines, pal);
}
```

- [ ] **Step 4: Use it from `src/ui/mod.rs`**

Add `pub mod overlays;` after `pub mod hud;`. Replace the body of `centered` with a call to `popup`, and drop the now unused `Clear`, `Wrap` and `Block` imports if clippy reports them:

```rust
fn centered(f: &mut Frame, area: Rect, title: &str, lines: &[&str], app: &App) {
    let lines = lines.iter().map(|l| Line::from(l.to_string())).collect();
    overlays::popup(f, area, title, lines, &app.palette);
}
```

In `draw`, after the `match &app.screen { ... }` and before `if app.show_help`:

```rust
    if let Some(form) = &app.form {
        overlays::idea_form(f, body, app, form);
    }
```

Update the help lines to:

```rust
            &[
                "Core:   arrows/hjkl move · 1-9 select · Enter open project",
                "        A agents · T threads · L timeline · U usage · I ideas (all projects)",
                "Lists:  ↑↓/jk move · Enter jump to pane / resume thread · Tab or 1-5 views",
                "        / search · s state filter · w time range · f project filter",
                "Ideas:  a add · e or Enter edit · d delete",
                "        Esc back · r refresh · q quit · ? this help",
            ],
```

- [ ] **Step 5: Ideas table and footer in `src/ui/views.rs`**

Replace the placeholder arm with `Tab::Ideas => ideas(f, body, app, pal),` and add:

```rust
fn ideas(f: &mut Frame, area: Rect, app: &App, pal: &Palette) {
    let rows = app.visible_ideas();
    if rows.is_empty() {
        let msg = if app.search.is_empty() {
            "no ideas yet · a to add"
        } else {
            "no ideas match the search"
        };
        return empty(f, area, msg, pal);
    }
    let [list, preview] = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).areas(area);
    let global = matches!(app.screen, Screen::Global(_));
    let body: Vec<Row> = rows
        .iter()
        .map(|(_, i)| {
            let mut cells = vec![Cell::from(Span::styled(i.name.clone(), Style::new().bold()))];
            if global {
                cells.push(Cell::from(Span::styled(
                    i.project_name.clone(),
                    Style::new().fg(pal.dim),
                )));
            }
            cells.push(Cell::from(i.description.clone()));
            Row::new(cells)
        })
        .collect();
    let mut widths = vec![Constraint::Length(28)];
    let mut header = vec!["idea"];
    if global {
        widths.push(Constraint::Length(18));
        header.push("project");
    }
    widths.push(Constraint::Min(10));
    header.push("description");
    let table = Table::new(body, widths)
        .header(Row::new(header).style(Style::new().fg(pal.dim)))
        .row_highlight_style(Style::new().bg(pal.highlight));
    f.render_stateful_widget(table, list, &mut table_state(app.selected_row));
    if let Some((_, i)) = rows.get(app.selected_row) {
        f.render_widget(
            Paragraph::new(i.description.clone())
                .wrap(Wrap { trim: true })
                .block(
                    Block::new()
                        .borders(Borders::TOP)
                        .border_style(Style::new().fg(pal.dim)),
                ),
            preview,
        );
    }
}
```

In `footer_line`, after the Timeline `w range` hint:

```rust
    if app.tab() == Tab::Ideas {
        hint.push_str(" · a add · e edit · d delete");
    }
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/ui/overlays.rs src/ui/mod.rs src/ui/views.rs
git commit -m "feat: draw the Ideas tab and the idea form"
```

---

### Task 4: Run targets: parser, request plan, executor

**Files:**
- Create: `src/run.rs`
- Modify: `src/main.rs` (add `mod run;` after `mod projects;`)

**Interfaces:**
- Produces (all `pub` in `crate::run`):
  - `struct Target { name: String, command: String, cwd: String, env: BTreeMap<String, String> }` (`Debug, Clone, PartialEq, Eq`)
  - `fn parse(text: &str) -> Result<Vec<Target>, String>`; `fn load(root: &Path) -> Result<Vec<Target>, String>`
  - `enum Layout { Tabs, Split }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `struct Launch { label: String, cwd: String, env: BTreeMap<String, String>, command: String }` (`Debug, Clone, PartialEq`)
  - `enum Step { Tab { label: String, launch: Launch }, Split { ratio: f32, launch: Launch } }` (`Debug, Clone, PartialEq`)
  - `fn pane_label(project: &str, target: &str) -> String` → `"<project>:<target>"`
  - `fn plan(project: &str, root: &Path, targets: &[Target], layout: Layout) -> Vec<Step>`
  - `fn execute(workspace_id: Option<&str>, steps: &[Step], send: impl FnMut(&str, Value) -> anyhow::Result<Value>) -> anyhow::Result<Option<String>>` → first new pane id.

- [ ] **Step 1: Write the failing tests**

Create `src/run.rs` with the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const RUN_TOML: &str = r#"
[[target]]
name = "api"
command = "cargo run"
cwd = "services/api"
env = { PORT = "3001" }
color = "blue"

[[target]]
name = "web"
command = "pnpm dev"
"#;

    fn targets() -> Vec<Target> {
        parse(RUN_TOML).unwrap()
    }

    #[test]
    fn parses_targets_with_defaults_and_ignores_extra_keys() {
        let t = targets();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].cwd, "services/api");
        assert_eq!(t[0].env.get("PORT").map(String::as_str), Some("3001"));
        assert_eq!(t[1].cwd, ".");
        assert!(t[1].env.is_empty());
    }

    #[test]
    fn parse_errors_name_the_target_and_stay_on_one_line() {
        let err = parse("[[target]]\nname = \"api\"\n").unwrap_err();
        assert_eq!(err, ".jarvis/run.toml: target \"api\" has no command");
        let err = parse("[[target]]\ncommand = \"x\"\n").unwrap_err();
        assert_eq!(err, ".jarvis/run.toml: target #1 has no name");
        let err = parse("[[target]\nname = ").unwrap_err();
        assert!(err.starts_with(".jarvis/run.toml: "), "{err}");
        assert!(!err.contains('\n'), "{err}");
        assert_eq!(parse("").unwrap_err(), ".jarvis/run.toml has no [[target]] entries");
    }

    #[test]
    fn load_reads_from_the_jarvis_folder() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load(tmp.path()).unwrap_err().contains("no run targets"));
        std::fs::create_dir_all(tmp.path().join(".jarvis")).unwrap();
        std::fs::write(tmp.path().join(".jarvis/run.toml"), RUN_TOML).unwrap();
        assert_eq!(load(tmp.path()).unwrap(), targets());
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
        assert_eq!(calls[1].1, json!({"pane_id": "w1:p9", "label": "alpha:api"}));
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
```

Add `mod run;` to `src/main.rs` after `mod projects;`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test run::`
Expected: compile errors (`parse`, `Target`, `plan`, `execute` unknown).

- [ ] **Step 3: Implement**

Above the tests in `src/run.rs`:

```rust
//! Run targets declared in `<root>/.jarvis/run.toml`, and the herdr requests that start them.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// One process of a project: a command and the directory (relative to the root) to run it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub command: String,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RunFile {
    #[serde(default)]
    target: Vec<RawTarget>,
}

#[derive(Deserialize)]
struct RawTarget {
    name: Option<String>,
    command: Option<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Errors are one line, for the status bar.
pub fn parse(text: &str) -> Result<Vec<Target>, String> {
    let file: RunFile =
        toml::from_str(text).map_err(|e| format!(".jarvis/run.toml: {}", e.message().trim()))?;
    let targets = (file.target.into_iter().enumerate())
        .map(|(i, t)| {
            let name = t
                .name
                .ok_or_else(|| format!(".jarvis/run.toml: target #{} has no name", i + 1))?;
            let command = t
                .command
                .ok_or_else(|| format!(".jarvis/run.toml: target \"{name}\" has no command"))?;
            Ok(Target {
                name,
                command,
                cwd: t.cwd.unwrap_or_else(|| ".".into()),
                env: t.env,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if targets.is_empty() {
        return Err(".jarvis/run.toml has no [[target]] entries".into());
    }
    Ok(targets)
}

pub fn load(root: &Path) -> Result<Vec<Target>, String> {
    let path = root.join(".jarvis").join("run.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("no run targets: {} ({e})", path.display()))?;
    parse(&text)
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
            let dir = if t.cwd == "." {
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
        send("pane.rename", json!({"pane_id": pane, "label": launch.label}))?;
        send(
            "pane.send_input",
            json!({"pane_id": pane, "text": launch.command, "keys": ["Enter"]}),
        )?;
        first.get_or_insert_with(|| pane.clone());
        prev = Some(pane);
    }
    Ok(first)
}
```

Note on `root.join("services/api")` on Windows: the test builds the expected string the same way, so it passes on every OS. `"/r"` with `cwd = "."` stays `"/r"`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test run::`
Expected: 7 passed. If `toml`'s `message()` text for the syntax error contains a newline, keep only its first line: `e.message().lines().next().unwrap_or("invalid TOML")`.

- [ ] **Step 5: Commit**

```bash
git add src/run.rs src/main.rs
git commit -m "feat: parse .jarvis/run.toml and plan the herdr requests that start targets"
```

---

### Task 5: Run picker in `App`, running detection, UI loop wiring

**Files:**
- Modify: `src/model.rs` (`pane_labels` field, build it, test)
- Modify: `src/ui/app.rs` (RunContext, Picker, actions, keys, tests)
- Modify: `src/ui/mod.rs` (perform `LoadTargets` and `Run`)

**Interfaces:**
- Consumes: `crate::run::{Target, Layout, Step, plan, pane_label, load, execute, parse}` (Task 4).
- Produces:
  - `Model.pane_labels: HashMap<String, String>` (label → pane id, from every snapshot pane with a label).
  - `pub struct RunContext { pub project: String, pub workspace_id: Option<String>, pub root: PathBuf }` (`Debug, Clone, PartialEq, Eq`).
  - `pub struct Picker { pub ctx: RunContext, pub targets: Vec<Target>, pub chosen: Vec<bool>, pub cursor: usize, pub choosing_layout: bool }` (`Debug, Clone, PartialEq, Eq`).
  - `Action::LoadTargets(RunContext)`, `Action::Run { workspace_id: Option<String>, steps: Vec<Step> }`.
  - `App.picker: Option<Picker>`; `pub fn running_pane(&self, project: &str, target: &str) -> Option<&String>`; `pub fn open_picker(&mut self, ctx: RunContext, targets: Result<Vec<Target>, String>)`.

- [ ] **Step 1: Write the failing model test**

In `src/model.rs` `mod tests`:

```rust
    #[test]
    fn pane_labels_point_at_their_panes() {
        let mut snap = snapshot();
        snap.panes[2].label = Some("gamma:web".into());
        let m = build(
            &snap,
            &[],
            &[],
            &Pricing::new(HashMap::new()),
            &mut Resolver::default(),
            "2026-10-08",
        );
        assert_eq!(m.pane_labels.len(), 1);
        assert_eq!(m.pane_labels["gamma:web"], "w3:p1");
    }
```

- [ ] **Step 2: Write the failing `App` tests**

In `src/ui/app.rs` `mod tests` (add `use crate::run::{self, Layout, Target};` and `use std::path::PathBuf;` to its imports):

```rust
    fn run_targets() -> Vec<Target> {
        run::parse(
            "[[target]]\nname = \"api\"\ncommand = \"cargo run\"\n\
             [[target]]\nname = \"web\"\ncommand = \"pnpm dev\"\n",
        )
        .unwrap()
    }

    /// alpha's project screen, Agents tab, its only agent selected.
    fn alpha_agents(app: &mut App) {
        app.screen = Screen::Project {
            key: app.model.projects[1].key.clone(),
            tab: Tab::Agents,
        };
    }

    fn open_picker(app: &mut App) -> RunContext {
        alpha_agents(app);
        key(app, KeyCode::Char('x'));
        let Some(Action::LoadTargets(ctx)) = app.action.take() else {
            panic!("x should ask for targets");
        };
        app.open_picker(ctx.clone(), Ok(run_targets()));
        ctx
    }

    #[test]
    fn x_asks_for_the_targets_of_the_selected_agents_worktree() {
        let mut app = sample_app();
        app.model.projects[1].agents[0].worktree = Some("/home/u/alpha-feat".into());
        alpha_agents(&mut app);
        key(&mut app, KeyCode::Char('x'));
        assert_eq!(
            app.action,
            Some(Action::LoadTargets(RunContext {
                project: "alpha".into(),
                workspace_id: Some("w1".into()),
                root: PathBuf::from("/home/u/alpha-feat"),
            }))
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
        let Some(Action::LoadTargets(ctx)) = app.action else {
            panic!("x should ask for targets");
        };
        assert_eq!(ctx.root, PathBuf::from(&app.model.projects[1].root));
        assert_eq!(ctx.workspace_id.as_deref(), Some("w1"));
    }

    #[test]
    fn x_needs_a_project_or_an_agent() {
        let mut app = sample_app();
        key(&mut app, KeyCode::Char('T'));
        key(&mut app, KeyCode::Char('x'));
        assert_eq!(app.action, None);
        assert!(app.status.as_deref().unwrap().contains("open a project"));
    }

    #[test]
    fn missing_targets_go_to_the_status_line() {
        let mut app = sample_app();
        alpha_agents(&mut app);
        key(&mut app, KeyCode::Char('x'));
        let Some(Action::LoadTargets(ctx)) = app.action.take() else {
            panic!("x should ask for targets");
        };
        app.open_picker(ctx, Err("no run targets: /home/u/alpha/.jarvis/run.toml".into()));
        assert!(app.picker.is_none());
        assert!(app.status.as_deref().unwrap().contains("no run targets"));
    }

    #[test]
    fn one_chosen_target_starts_in_its_own_tab() {
        let mut app = sample_app();
        let ctx = open_picker(&mut app);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Enter);
        assert!(app.picker.is_none());
        assert_eq!(
            app.action,
            Some(Action::Run {
                workspace_id: Some("w1".into()),
                steps: run::plan("alpha", &ctx.root, &run_targets()[1..], Layout::Tabs),
            })
        );
    }

    #[test]
    fn several_targets_ask_for_the_layout() {
        let mut app = sample_app();
        let ctx = open_picker(&mut app);
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
                steps: run::plan("alpha", &ctx.root, &run_targets(), Layout::Split),
            })
        );
    }

    #[test]
    fn running_targets_are_focused_or_skipped() {
        let mut app = sample_app();
        app.model
            .pane_labels
            .insert("alpha:api".into(), "w1:p7".into());
        open_picker(&mut app);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.action.take(), Some(Action::FocusPane("w1:p7".into())));

        let ctx = open_picker(&mut app);
        key(&mut app, KeyCode::Char('a'));
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('t'));
        assert_eq!(
            app.action,
            Some(Action::Run {
                workspace_id: Some("w1".into()),
                steps: run::plan("alpha", &ctx.root, &run_targets()[1..], Layout::Tabs),
            })
        );
        assert_eq!(app.status.as_deref(), Some("already running: api"));
    }

    #[test]
    fn picker_takes_every_key_and_esc_closes_it() {
        let mut app = sample_app();
        open_picker(&mut app);
        key(&mut app, KeyCode::Char('q'));
        key(&mut app, KeyCode::Char('T'));
        assert!(!app.quit);
        assert!(matches!(app.screen, Screen::Project { .. }));
        key(&mut app, KeyCode::Esc);
        assert!(app.picker.is_none());
        assert!(matches!(app.screen, Screen::Project { .. }));
    }
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test`
Expected: compile errors (`pane_labels`, `RunContext`, `open_picker`, `Action::LoadTargets` unknown).

- [ ] **Step 4: Implement `pane_labels` in `src/model.rs`**

Add to `Model`:

```rust
    /// herdr pane label → pane id, for every labelled pane (run targets are found by label).
    pub pane_labels: HashMap<String, String>,
```

and in `build`'s final `Model { ... }`:

```rust
        pane_labels: (snap.panes.iter())
            .filter_map(|p| Some((p.label.clone()?, p.pane_id.clone())))
            .collect(),
```

- [ ] **Step 5: Implement the picker in `src/ui/app.rs`**

Imports: add `use crate::run::{self, Layout, Step, Target};` and `use std::path::PathBuf;`.

Types (after `IdeaForm`):

```rust
/// Where `x` starts targets: the project's name, the agent's workspace and the folder holding `.jarvis/run.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    pub project: String,
    pub workspace_id: Option<String>,
    pub root: PathBuf,
}

/// The run-target popup; while open it takes every key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub ctx: RunContext,
    pub targets: Vec<Target>,
    pub chosen: Vec<bool>,
    pub cursor: usize,
    pub choosing_layout: bool,
}
```

`Action` gains:

```rust
    /// Read `.jarvis/run.toml` under the context's root and open the picker.
    LoadTargets(RunContext),
    /// Send these steps to herdr, then focus the first new pane.
    Run {
        workspace_id: Option<String>,
        steps: Vec<Step>,
    },
```

`App` field `pub picker: Option<Picker>,` (init `picker: None`).

`on_key`: right after the `form` block:

```rust
        if self.picker.is_some() {
            self.on_picker_key(key);
            return;
        }
```

`on_list_key`, before `_ => {}`:

```rust
            KeyCode::Char('x') => match self.run_context() {
                Some(ctx) => self.action = Some(Action::LoadTargets(ctx)),
                None => {
                    self.status = Some("x runs targets: open a project or select an agent".into())
                }
            },
```

Methods in `impl App`:

```rust
    /// Pane of a target that is already running, found by its herdr label.
    pub fn running_pane(&self, project: &str, target: &str) -> Option<&String> {
        self.model.pane_labels.get(&run::pane_label(project, target))
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
            project: project.name.clone(),
            workspace_id: agent
                .or(project.agents.first())
                .map(|a| a.pane.workspace_id.clone()),
            root: PathBuf::from(root),
        })
    }

    pub fn open_picker(&mut self, ctx: RunContext, targets: Result<Vec<Target>, String>) {
        match targets {
            Ok(targets) => {
                self.picker = Some(Picker {
                    chosen: vec![false; targets.len()],
                    ctx,
                    targets,
                    cursor: 0,
                    choosing_layout: false,
                })
            }
            Err(e) => self.status = Some(e),
        }
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let Some(p) = self.picker.as_mut() else {
            return;
        };
        let last = p.targets.len().saturating_sub(1);
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
        let Some(p) = self.picker.take() else {
            return;
        };
        let (running, to_start): (Vec<&Target>, Vec<&Target>) = (p.targets.iter().zip(&p.chosen))
            .filter(|(_, chosen)| **chosen)
            .map(|(t, _)| t)
            .partition(|t| self.running_pane(&p.ctx.project, &t.name).is_some());
        if to_start.is_empty() {
            self.action = (running.first())
                .and_then(|t| self.running_pane(&p.ctx.project, &t.name))
                .cloned()
                .map(Action::FocusPane);
            return;
        }
        if !running.is_empty() {
            let names: Vec<&str> = running.iter().map(|t| t.name.as_str()).collect();
            self.status = Some(format!("already running: {}", names.join(", ")));
        }
        let to_start: Vec<Target> = to_start.into_iter().cloned().collect();
        self.action = Some(Action::Run {
            steps: run::plan(&p.ctx.project, &p.ctx.root, &to_start, layout),
            workspace_id: p.ctx.workspace_id,
        });
    }
```

If the borrow checker rejects `on_picker_key` (calling `self.start_targets` while `p` is borrowed), copy what the match needs first: `let (choosing, chosen_count) = (p.choosing_layout, p.chosen.iter().filter(|c| **c).count());` and re-borrow `self.picker.as_mut()` inside the arms that mutate it.

- [ ] **Step 6: Perform the actions in `src/ui/mod.rs`**

Imports: `use crate::{collector, ideas, model, paths, run};`. In `match app.action.take()`:

```rust
            Some(Action::LoadTargets(ctx)) => {
                let targets = run::load(&ctx.root);
                app.open_picker(ctx, targets);
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
```

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo test`
Expected: all pass (1 new model test, 8 new app tests).

- [ ] **Step 8: Commit**

```bash
git add src/model.rs src/ui/app.rs src/ui/mod.rs
git commit -m "feat: pick and start project run targets from Jarvis"
```

---

### Task 6: Draw the run picker, keys in footer and help

**Files:**
- Modify: `src/ui/overlays.rs` (`picker`)
- Modify: `src/ui/mod.rs` (draw it, help line, test)
- Modify: `src/ui/views.rs` (footer hint)

**Interfaces:**
- Consumes: `App::picker`, `Picker`, `App::running_pane` (Task 5); `popup` (Task 3).
- Produces: `pub fn picker(f: &mut Frame, area: Rect, app: &App, p: &Picker)` in `ui::overlays`.

- [ ] **Step 1: Write the failing test**

In `src/ui/mod.rs` tests:

```rust
    #[test]
    fn draws_the_run_picker_with_running_marks_and_layout_hint() {
        let mut app = sample_app();
        app.model
            .pane_labels
            .insert("alpha:api".into(), "w1:p7".into());
        app.open_picker(
            crate::ui::app::RunContext {
                project: "alpha".into(),
                workspace_id: Some("w1".into()),
                root: "/home/u/alpha".into(),
            },
            crate::run::parse(
                "[[target]]\nname = \"api\"\ncommand = \"cargo run\"\n\
                 [[target]]\nname = \"web\"\ncommand = \"pnpm dev\"\n",
            ),
        );
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("run · alpha"));
        assert!(out.contains("[ ] api"));
        assert!(out.contains("● running"));
        assert!(out.contains("pnpm dev"));
        assert!(out.contains("Space select"));
        app.picker.as_mut().unwrap().choosing_layout = true;
        let out = render(100, 30, |f| draw(f, &app));
        assert!(out.contains("s side by side"));
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `cargo test ui::tests::draws_the_run_picker`
Expected: FAIL (picker not drawn).

- [ ] **Step 3: Implement `picker` in `src/ui/overlays.rs`**

Change the import to `use super::app::{App, IdeaForm, Picker};` and add:

```rust
pub fn picker(f: &mut Frame, area: Rect, app: &App, p: &Picker) {
    let pal = &app.palette;
    let mut lines: Vec<Line<'static>> = (p.targets.iter().enumerate())
        .map(|(i, t)| {
            let mark = if p.chosen[i] { "[x]" } else { "[ ]" };
            let mut spans = vec![
                Span::styled(
                    if i == p.cursor { "› " } else { "  " },
                    Style::new().fg(pal.accent),
                ),
                Span::raw(format!("{mark} {:<16}", t.name)),
                Span::styled(format!(" {}", t.command), Style::new().fg(pal.dim)),
            ];
            if app.running_pane(&p.ctx.project, &t.name).is_some() {
                spans.push(Span::styled("  ● running", Style::new().fg(pal.ok)));
            }
            Line::from(spans)
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        if p.choosing_layout {
            "t one tab per target · s side by side in one tab · Esc back"
        } else {
            "Space select · a all · Enter run · Esc close"
        },
        Style::new().fg(pal.accent),
    )));
    popup(f, area, &format!(" run · {} ", p.ctx.project), lines, pal);
}
```

- [ ] **Step 4: Draw it and document the key**

In `src/ui/mod.rs` `draw`, after the form block:

```rust
    if let Some(p) = &app.picker {
        overlays::picker(f, body, app, p);
    }
```

Help: insert after the `Ideas:` line

```rust
                "Run:    x start targets from .jarvis/run.toml (project screen, agent rows)",
```

In `src/ui/views.rs` `footer_line`, after the Ideas hint:

```rust
    if matches!(app.screen, Screen::Project { .. }) || app.tab() == Tab::Agents {
        hint.push_str(" · x run");
    }
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/ui/overlays.rs src/ui/mod.rs src/ui/views.rs
git commit -m "feat: draw the run-target picker"
```

---

### Task 7: README, gates, manual check in herdr

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document both features in `README.md`**

In "What you see", after the "All projects" bullet:

```markdown
- **Ideas**: a fifth tab with a list of ideas per project (name and description), stored in the plugin
  state directory (`ideas.json`). `I` shows the ideas of every project.
- **Run targets**: `x` starts a project's processes declared in `.jarvis/run.toml` (see below).
```

In the Keys table, change the Core row's views to `` `A` `T` `L` `U` `I` all-project views``, the Lists row's `` `1`–`4` `` to `` `1`–`5` ``, and add rows:

```markdown
| Ideas tab | `a` add · `e` / `Enter` edit · `d` delete (`y` confirms) · in the form `Tab` switches field, `Enter` saves, `Esc` cancels |
| Run | `x` on a project screen or an agent row opens the target picker · `Space` select · `a` all · `Enter` run · then `t` one tab per target or `s` side by side |
```

Add a section before "## Configuration":

````markdown
## Run targets

Declare a project's processes in `.jarvis/run.toml` at the project root (or the worktree root), versioned
with the project:

```toml
[[target]]
name = "backend"
cwd = "services/api"        # relative to the project root; default "."
command = "npm run dev"
env = { PORT = "3001" }     # optional

[[target]]
name = "web-admin"
cwd = "apps/admin"
command = "pnpm dev"
```

Each target starts in a new pane running your shell, labelled `<project>:<target>`, so it stays open
when the command exits. A target whose pane is still open counts as running: Jarvis marks it in the
picker and focuses it instead of starting it twice.
````

- [ ] **Step 2: Run the CI gates locally**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: no diff, no warnings, all tests pass. Fix anything they report (`cargo fmt` for formatting).

- [ ] **Step 3: Manual check in herdr (Windows)**

1. Build: `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-or-build.ps1` (it moves the locked `jarvis.exe` aside), then reopen Jarvis (`herdr plugin action invoke jarvis.open`).
2. Ideas: on a project screen press `5`, `a`, type a name with Polish letters, `Tab`, a description, `Enter`. Close and reopen Jarvis: the idea is still there (`ideas.json` in the plugin state dir). Press `I`: it shows with its project. Edit and delete it.
3. Run targets: in a scratch project add `.jarvis/run.toml` with two targets, e.g. `command = "ping -n 30 127.0.0.1"` and `command = "echo hello"`. Press `x`, `a`, `Enter`, `s`. Expected: one new tab with two side-by-side panes of equal width, labelled `<project>:<target>`, each running its command in the shell; the first pane is focused. If the widths are unequal the other way round, herdr's `ratio` is the new pane's share: change the ratio in `run::plan` to `1.0 - 1.0 / (n - i + 1) as f32` and its test to `[2.0 / 3.0, 1.0 / 2.0]`.
4. Go back to Jarvis, press `x`: both targets show `● running`; choose one, `Enter`: herdr focuses its pane, nothing new is created.
5. Press `x`, `t` path: close those panes, select both, `Enter`, `t`: two new tabs named after the targets.
6. Check the commands ran even though the shell was still starting (PowerShell profile). If a command was lost, note it in the PR; the fix is to wait for the prompt with `pane.wait_for_output` before `pane.send_input`.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: ideas and run targets in the README"
```

- [ ] **Step 5: Push and open the PR (after the user says go)**

```bash
git push -u origin feat/ideas-and-run-targets
gh pr create --title "feat: project ideas and run targets" --body "Closes #2. Closes #8. ..."
```

The PR body describes both features and the manual check results; no Claude attribution.
