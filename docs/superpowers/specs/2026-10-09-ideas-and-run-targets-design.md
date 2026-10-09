# Jarvis: project ideas and run targets

Date: 2026-10-09
Status: design approved in conversation, awaiting spec review
Issues: #2 (per-project idea list), #8 (run project targets)

## 1. Summary

Two additions to the Jarvis TUI, shipped on one branch:

- **Ideas (#2):** a list of ideas per project, each a name and a description, shown in a new Ideas tab per
  project and globally. Stored locally in the plugin state directory.
- **Run targets (#8):** a project declares its processes in `.jarvis/run.toml`; from Jarvis you pick some or
  all of them and start them in herdr panes, one tab each or side by side in one tab.

Out of scope: showing GitHub issues next to ideas, turning an idea into an issue, restarting running targets.

## 2. Ideas (#2)

### Storage

`<state_dir>/ideas.json`, a JSON array:

```json
[{ "project_key": "c:/projects/home/herdr-jarvis", "project_name": "herdr-jarvis",
   "name": "Pricing cache", "description": "keep prices for 24h" }]
```

- `project_key` is the normalized project root (`projects::normalize`), the same key the model groups by.
  Ideas belong to the project, not to an agent.
- `project_name` is stored so ideas of projects with no live agents still show a readable label.
- The TUI loads the file at start and keeps the list in memory. Every change rewrites the whole file
  atomically (write `ideas.json.tmp`, then rename). A missing file is an empty list; an unreadable file is
  shown in the status line and treated as empty, without overwriting it until the next successful edit.
- Several Jarvis panes writing at once: last write wins. Jarvis normally runs as one pane.

### UI

- `Tab::Ideas` becomes the fifth tab: `5`, `Tab`/`BackTab` reach it like the others. `I` opens the global
  Ideas view (alongside `A`, `T`, `L`, `U`).
- Project screen: the ideas of that project, in file order. Global view: every idea, with a project column;
  `f` (project filter) and `/` (search over name and description) apply.
- Keys on the Ideas tab:
  - `a` add. On the global view this needs a project filter (`f`), otherwise the status line says so.
  - `e` or `Enter` edit the selected idea.
  - `d` delete: the status line asks `delete "<name>"? y/n`; any key other than `y` cancels.
- Form: an overlay with two single-line fields, Name and Description. `Tab` switches field, typing edits the
  focused field, `Backspace` deletes, `Enter` saves (an empty name is refused with a status message), `Esc`
  cancels. While the form is open it takes every key.

## 3. Run targets (#8)

### Definition

`<root>/.jarvis/run.toml`, versioned with the project:

```toml
[[target]]
name = "backend"
cwd = "services/api"      # relative to <root>; default "."
command = "npm run dev"
env = { PORT = "3001" }   # optional
```

`<root>` is the worktree of the selected agent when it works in a linked worktree, otherwise the project
root. Unknown keys are ignored; a missing `name` or `command` is a parse error naming the target.

### Picker

- `x` opens the picker for:
  - the project screen's project, using the selected agent's worktree when the Agents tab has a selected row;
  - the selected agent's project on the global Agents view.
- The picker lists the targets with `[ ]`/`[x]` and `● running` for targets already running. `Space` toggles
  the selected target, `a` selects all or none, `j`/`k`/arrows move, `Esc` closes.
- `Enter` with one target selected starts it in its own tab. With more selected it asks for the layout:
  `t` one tab per target, `s` one tab with the targets side by side, `Esc` back to the picker.
- `Enter` with nothing selected does nothing.

### Running

A target counts as running when a pane in the herdr snapshot has the label `<project>:<target>`
(project name, target name). Running targets are not started again; if every selected target is running,
Jarvis focuses the first of them.

Starting the rest is a list of socket requests built by a pure function, then sent in order:

- One tab per target: for each target, `tab.create` with `workspace_id` (the agent's workspace), `cwd`
  (`<root>/<cwd>`), `env`, `label` = target name; then `pane.rename` of the returned `root_pane` to
  `<project>:<target>` and `pane.send_input` with `text` = command and `keys` = `["Enter"]`.
- One tab for all: `tab.create` for the first target (tab label = project name), then for each next target
  `pane.split` of the previous pane with `direction` = `right` and a ratio that gives equal widths, then
  rename and send input for each pane as above.
- Commands run in the pane's default shell, so shell syntax and Windows `.cmd` shims work, and the pane stays
  open after the command exits.
- The first new pane is focused.

### Errors

A missing `run.toml`, a parse error, or a failed herdr request is shown in the status line; already created
panes stay. Nothing crashes the TUI.

## 4. Testing

- Ideas: load/save round trip, missing file, unreadable file is not overwritten on load.
- `run.toml` parser: defaults, env, missing fields.
- `App` key handling: Ideas tab add/edit/delete flow, global add needs a filter, picker toggling, select all,
  layout choice, running targets skipped.
- Request plan: the pure function's output for both layouts and for partially running targets, checked
  against expected JSON without a running herdr.
- Manual: start targets from a real `run.toml` in herdr on Windows, both layouts, and re-press `x` to see
  them marked running.
