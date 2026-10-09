# Jarvis: project ideas and run targets

Date: 2026-10-09
Status: design approved in conversation, awaiting spec review
Issues: #2 (per-project idea list), #8 (run project targets)

## 1. Summary

Two additions to the Jarvis TUI, shipped on one branch:

- **Ideas (#2):** a list of ideas per project, each a name and a description, shown in a new Ideas tab per
  project and globally. Stored locally in the plugin state directory.
- **Run targets (#8):** you define a project's processes from Jarvis (stored privately in the plugin state
  directory); you pick some or all of them and start them in herdr panes, one tab each or side by side in
  one tab.

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
  atomically (write `ideas.json.tmp`, then rename). A missing file is an empty list. An unreadable file is
  copied to `ideas.json.bad` (so the next edit cannot lose it), reported in the status line and treated as
  empty.
- Several Jarvis panes writing at once: last write wins. Jarvis normally runs as one pane.

### UI

- `Tab::Ideas` becomes the fifth tab: `5`, `Tab`/`BackTab` reach it like the others. `I` opens the global
  Ideas view (alongside `A`, `T`, `L`, `U`).
- Project screen: the ideas of that project, in file order. Global view: every idea, with a project column;
  `f` (project filter) and `/` (search over name and description) apply.
- Keys on the Ideas tab:
  - `a` add. On the global view this needs a project filter (`f`), otherwise the status line says so.
  - `e` or `Enter` edit the selected idea.
  - `d` delete, through the delete popup (below).
  - `Space` moves the selected idea to its next status: todo → doing → done → todo. Each idea has a
    `status` (`todo`, `doing`, `done`; a file written before statuses existed loads as `todo`), shown as
    `○ todo`, `◐ doing`, `✓ done` in a first column; done ideas are dimmed. Editing keeps the status.
  - `s` filters by status: all → todo → doing → done → all. It is separate from the agent state filter.
- Form: an overlay with two single-line fields, Name and Description. `Tab`/`BackTab` move between fields,
  typing edits the focused field, `Backspace` deletes, `Enter` saves (an empty name is refused with a status
  message), `Esc` cancels. While the form is open it takes every key. Ideas and run targets share this form.
- Delete popup: a centered `Delete "<name>"?` with `[ Yes ]` and `[ No ]`, **No** selected.
  `Left`/`Right`/`Tab`/`h`/`l` switch the button, `Enter` confirms the selected one, `y` deletes, `n` or
  `Esc` cancels. While open it takes every key. Ideas and run targets share it.

## 3. Run targets (#8)

### Definition

Targets are private to the user, not versioned with the project. They live in `<state_dir>/targets.json`,
a JSON array:

```json
[{ "project_key": "c:/projects/home/herdr-jarvis", "name": "backend", "command": "npm run dev",
   "cwd": "services/api", "env": { "PORT": "3001" } }]
```

- `project_key` is the same normalized project root as for ideas; the picker shows the targets of its project.
- `cwd` is relative to `<root>`; empty means `<root>` itself.
- `env` is optional (missing means none) and is edited by hand in the file only.
- Loaded at TUI start and saved on every change exactly like `ideas.json`: atomic write through
  `targets.json.tmp`, an unreadable file copied to `targets.json.bad` and reported in the status line.

`<root>` is the worktree of the selected agent when it works in a linked worktree, otherwise the project
root.

### Picker

- `x` opens the picker for:
  - the project screen's project, using the selected agent's worktree when the Agents tab has a selected row;
  - the selected agent's project on the global Agents view.
- `x` always opens the picker, also when the project has no targets yet; an empty picker shows
  `no targets yet · n to add`.
- The picker lists the targets with `[ ]`/`[x]` and `● running` for targets already running. `Space` toggles
  the selected target, `a` selects all or none, `j`/`k`/arrows move, `Esc` closes. The footer lists the keys.
- `n` adds a target, `e` edits the one under the cursor, `d` deletes it through the delete popup. The form
  has the fields name, command and cwd; name and command are required (a status message says so), cwd is
  optional. `Enter` saves and returns to the picker, `Esc` cancels.
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

An unreadable `targets.json` or a failed herdr request is shown in the status line; already created panes
stay. Nothing crashes the TUI.

## 4. Testing

- JSON store (shared by ideas and targets): load/save round trip, missing file, unreadable file is backed up
  to `<file>.bad`, a target without `env`.
- `App` key handling: Ideas tab add/edit/delete flow, global add needs a filter, the delete popup (default
  No, `y`, `Enter` on Yes, `Esc`), an empty picker on `x`, targets filtered by project, adding, editing and
  deleting targets from the picker, picker toggling, select all, layout choice, running targets skipped.
- Drawing: the form, the delete popup, the picker and the empty picker.
- Request plan: the pure function's output for both layouts and for partially running targets, checked
  against expected JSON without a running herdr.
- Manual: add targets from the picker and start them in herdr on Windows, both layouts, and re-press `x` to see
  them marked running.
