# Jarvis — mission control for herdr

Date: 2026-10-08
Status: design approved in conversation, awaiting spec review

## 1. Summary

Jarvis is a herdr plugin that opens a full-screen terminal "mission control" overlay. On launch it shows an
animated Iron Man–style core (arc reactor) with branches radiating out to every project that has herdr
activity. Each branch shows what is happening in that project; selecting one zooms into the project and shows its
agents, Claude Code threads, event timeline and token/cost usage. Global views cover all projects at once.

The repository will be public (MIT) and published to the herdr marketplace by tagging it with the GitHub topic
`herdr-plugin`. Linux, macOS and Windows are first-class targets.

## 2. Goals and non-goals

### Goals (v1)

- Big-picture "core view": all projects with herdr activity, their agent states, a live event feed.
- Drill-down into a single project (grouped by git root, worktrees nested under their main repository).
- Views: Agents (per project), Threads, Timeline, Usage — each available per project and globally.
- Claude Code transcript integration: thread titles, turns, last activity, token usage and estimated cost.
- Timeline that survives the overlay being closed (background collector).
- Jump from any agent row to its pane.
- Works on Windows, Linux and macOS.

### Non-goals (v1)

- Sending prompts or controlling agents from Jarvis.
- Transcript adapters for agents other than Claude Code (Codex, OpenCode, Pi, …). Other agents still appear in
  the project tree and timeline, because herdr reports them, but without threads or usage.
- Web dashboard, notifications, alerts.
- Orchestration (that is herdr-projects' / herdr-board's job).

## 3. Prior art (discovery, 2026-10-08)

The herdr ecosystem has ~90 public repositories with the `herdr-plugin` topic. Closest overlaps:

| Plugin | What it does | Gap Jarvis fills |
|---|---|---|
| hhdebb/herdr-radar | Sidebar agent list grouped by project, worktrees nested, state marks | Sidebar-only, no dashboards, no history |
| eliasstravik/herdr-projects | Coordinator + worker threads, "what needs you" overview | Orchestration, not observation; macOS/Linux only |
| aigorahub/herdr-lantern | Chat agent answering "who needs you" | Conversational, no dashboard |
| lmilojevicc/seshagy | Session dashboard with agent states (tmux/herdr) | Separate tool, not a herdr pane |
| Davidcreador/herdr-token-dashboard, levi-qiao/herdr-agent-usage | Token spend / usage | Usage only |
| thanhdat77/herdr-navigator | Fuzzy jump to workspace/agent/project | Navigation only |

Differentiator: one full-screen overlay that combines project grouping, live state, history, threads and cost,
with a distinctive animated big-picture view, and cross-platform support including Windows.

## 4. Architecture

```
herdr server ──socket──┐
                       │ events.subscribe
              ┌────────▼─────────┐   append   ┌──────────────────────────────┐
[[startup]] ─►│ jarvis collect   │──────────►│ STATE_DIR/events-YYYY-MM-DD  │
              │ (background)     │            │ .jsonl (30-day retention)    │
              └──────────────────┘            └──────────────▲───────────────┘
                                                             │ read
[[panes]] overlay ─► jarvis tui ──snapshot + events (socket)─┤
                         │                                   │
                         └── incremental read ──► ~/.claude/projects/**/*.jsonl
```

A single Rust binary, `jarvis`, with subcommands:

- `jarvis collect` — long-running collector. Subscribes to herdr events and appends normalized records to the
  daily events file. Single instance enforced with a lockfile in `STATE_DIR`. Reconnects on socket loss.
- `jarvis tui` — the overlay UI (ratatui + crossterm).

Stack: Rust (edition 2021), ratatui, crossterm, serde/serde_json, toml. No async runtime unless the socket client
requires one; prefer a reader thread plus channels.

### Modules

Each module has one job and is testable without the others.

- `herdr` — socket client. Request/response for `session.snapshot`, `pane.focus` / `agent.focus`; streaming
  `events.subscribe`. Typed structs for the snapshot (agents, workspaces, tabs, panes) and events. Validates
  protocol version 22.
- `projects` — maps a `cwd` to a project: walk up to the nearest `.git`; if `.git` is a file containing `gitdir:`,
  resolve the worktree to its main repository and mark the entry as a worktree. Paths are normalized (case and
  separators on Windows). Results cached per path. A `cwd` outside any repository is its own project.
- `transcripts` — `TranscriptSource` trait with one implementation, `claude`. The trait exists on purpose: other
  agents are planned. The Claude adapter reads `~/.claude/projects/**/*.jsonl` incrementally (see §6).
- `events` — collector writer and reader; pairs state changes into durations (e.g. working → done took 4m12s).
- `pricing` — model price table compiled into the binary, overridable in `config.toml`. Unknown model shows
  `? $`, never zero.
- `ui` — core view, drill-down, global views, compact fallback, boot animation.
- `config` — reads `HERDR_PLUGIN_CONFIG_DIR/config.toml`.

## 5. User interface

### 5.1 Boot

On open: a short (~0.8 s) boot sequence (scan lines, "J.A.R.V.I.S. ONLINE", branches bloom out). Any key skips.
Disabled when `animation = "off"`.

### 5.2 Core view (home)

```
 JARVIS  ● 2 working  ▲ 1 blocked  ✓ 3 done  ○ 4 idle        today ≈$12.40 · 3.1M tok

        ┌ factory-game ──────┐                          ┌ herdr-jarvis ───┐
        │ ● 1  ✓ 1       │╲                       ╱│ ● 1   "Fix bug…"│
        └────────────────┘  ╲      ⡠⠔⠒⠒⠒⠢⢄      ╱  └─────────────────┘
                              ╲  ⡔⠁ ⣀⣤⣤⣀ ⠈⢢  ╱
   ┌ team-planner… ─┐  ━━━━━━━━ ⡇ ⣼⣿ ◉ ⣿⣧ ⢸ ━━━━━━━━  ┌ token-dash ┐
   │ ○ 2            │          ⢣ ⠙⠿⠿⠿⠋ ⡜          │ ✓ 1   12m        │
   └────────────────┘        ╱  ⠑⢄⣀⣀⣀⡠⠊  ╲         └──────────────────┘
                           ╱                ╲
        ┌ web-shop ──────┐                    ╲  ┌ +3 more ─────────┐
        │ ▲ BLOCKED  !   │                       │ notes, …         │
        └────────────────┘                       └──────────────────┘

 ▸ 14:31 factory-game working→done (4m12s) · 14:29 web-shop ▲ blocked · 14:20 …
```

- **Status bar**: global counts per state, today's tokens and estimated cost, clock.
- **Core**: braille arc reactor drawn on a ratatui `Canvas`. The ring rotates; rotation speed scales with the
  number of working agents. Colour: cyan when calm, amber when there are unseen `done` agents, red pulse when
  any agent is `blocked`. The centre shows the number of active agents.
- **Branches**: one per project, at most 8, ordered by urgency (blocked > unseen done > working > idle) and then
  by last activity. Overflow goes into a "+N more" node, which opens a list. A pulse travels along a branch
  while the project has a working agent. Blocked nodes pulse.
- **Feed**: the latest events scroll along the bottom.
- **Navigation**: arrows / `hjkl` move to the nearest node in that direction; `1`–`8` select directly; `Enter`
  zooms into the project; `T` / `L` / `U` open the global Threads / Timeline / Usage views; `?` help;
  `q` / `Esc` close.

### 5.3 Project drill-down

`Enter` on a node plays a short zoom animation; the node becomes the header:

```
 JARVIS › factory-game  (/home/me/factory-game · main)          Esc = back to core
─[Agents]─[Threads]─[Timeline]─[Usage]────────────────────────────────────────────
 ├ w2:t1 p3  claude ● working "Fix belt simulation"  3m
 └ ⎇ feat-x  w9:p1  ✓ done   "Fix belt sim"              12m
```

Tabs switch with `1`–`4` / `Tab`. They are the same four views as the global ones, filtered to the project.

### 5.4 Views

- **Agents**: workspaces / tabs / panes / agents under the project, worktrees nested. Row: pane id, agent kind,
  state, terminal title, time in state. Sorted blocked → unseen done → working → idle. Side panel for the
  selected agent: branch, linked thread, today's tokens and cost, last 5 events.
- **Threads**: every Claude Code session for the scope, live and finished. Row: title (`ai-title`), project,
  branch, last activity, turns, tokens, cost, `LIVE <pane>` badge if a pane is running it (linked via
  `agent_session.value` == transcript `sessionId`). Preview: last user prompt and last assistant reply,
  truncated. `/` searches titles.
- **Timeline**: events newest first, with durations for completed state pairs. Filters: state, range
  (`1h` / `today` / `7d`).
- **Usage**: per day (14 days), per project, per model. Columns: input, output, cache read, cache write,
  estimated cost (prefixed `≈`). Bar charts / sparklines.

### 5.5 Actions on rows

- `Enter` on a live agent: focus its pane via the herdr API and close the overlay.
- `Enter` on a finished thread: show `claude --resume <sessionId>` and copy it to the clipboard when a clipboard
  is available; otherwise only show it.
- `/` search, `f` project filter (global views), `r` refresh.

### 5.6 Performance and accessibility

- 30 fps only while something animates; ~4 fps idle ring refresh. No redraw when nothing changes in
  `animation = "off"` mode.
- `config.toml`: `animation = "full" | "reduced" | "off"`.
- Terminals smaller than ~90×28 get a compact project list instead of the core.
- 16-colour palette fallback when truecolor is unavailable.
- State is always shown as both a glyph and a colour, never by colour alone.

## 6. Data

### 6.1 herdr

- On TUI start: `session.snapshot` over `HERDR_SOCKET_PATH`. Then `events.subscribe` keeps the state current
  incrementally (no polling). On socket loss: reconnect every 5 s, status bar shows "herdr offline".
- Protocol version must be 22; otherwise show a clear message with the detected version and exit.

### 6.2 Collector records

One JSON object per line in `STATE_DIR/events-YYYY-MM-DD.jsonl`:

```json
{"ts":"2026-10-08T14:31:02Z","event":"pane.agent_status_changed","pane_id":"w6:pG","workspace_id":"w6",
 "cwd":"/home/me/factory-game","agent":"claude","from":"working","to":"done","session_id":"3c0e…"}
```

Subscribed events: `pane.agent_status_changed`, `pane.agent_detected`, `pane.created`, `pane.closed`,
`pane.exited`, `workspace.created`, `workspace.closed`, `worktree.created`, `worktree.removed`. Fields the event
does not carry (e.g. `cwd`) are filled from the collector's last known snapshot. Retention: 30 days by default
(`retention_days` in config); old files are deleted on collector start.

### 6.3 Claude Code transcripts

- Location: `~/.claude/projects/<encoded-path>/<sessionId>.jsonl` (respect `CLAUDE_CONFIG_DIR` if set).
- Index: `STATE_DIR/claude-index.json`, per file `{mtime, size, offset, session_id, title, cwd, branch, turns,
  first_ts, last_ts, usage[day][model]}`. On TUI start, only bytes after `offset` are read; if a file shrank, it
  is re-read from the start. While the overlay is open, files of live sessions are re-read when they change.
- Fields used: `type == "ai-title"` → `aiTitle`; `type == "user"` / `"assistant"` → turns, `timestamp`, `cwd`,
  `gitBranch`; `assistant.message.usage` → `input_tokens`, `output_tokens`, `cache_read_input_tokens`,
  `cache_creation_input_tokens`; `assistant.message.model`.
- Usage is de-duplicated by `message.id`: Claude Code writes several lines per response, each with the same
  usage.
- Unknown record types and unknown fields are ignored.

### 6.4 Pricing

Default table (per million tokens, by model id prefix) compiled in; `config.toml` `[pricing."<model>"]` overrides
`input`, `output`, `cache_read`, `cache_write`. Costs are labelled as estimates.

### 6.5 Config file

`HERDR_PLUGIN_CONFIG_DIR/config.toml`, all keys optional:

```toml
animation = "full"        # full | reduced | off
retention_days = 30
max_branches = 8
claude_dir = "~/.claude"  # overrides default / CLAUDE_CONFIG_DIR

[pricing."claude-opus-5-5"]
input = 0.0
output = 0.0
cache_read = 0.0
cache_write = 0.0
```

(Pricing values above are placeholders for the format; real defaults ship in the binary.)

## 7. Error handling

| Situation | Behaviour |
|---|---|
| Corrupt JSONL line | Skip, count in log, continue |
| No Claude directory | Threads/Usage show "no Claude Code data"; other views work |
| Collector not running | Timeline shows data from TUI start only, with a warning |
| herdr socket lost | Reconnect every 5 s, "herdr offline" in status bar, last state stays visible (dimmed) |
| Protocol ≠ 22 | Clear message with versions, exit |
| Panic in TUI | Panic hook restores the terminal (leave raw mode / alternate screen) before exit |
| Second collector | Lockfile check, second instance exits quietly |

Logs: `STATE_DIR/jarvis.log`, size-capped.

## 8. Plugin manifest (target shape)

```toml
id = "jarvis"
name = "Jarvis"
version = "0.1.0"
min_herdr_version = "0.9.0"
description = "Mission control for herdr: animated big-picture core, projects by path, threads, timeline and token usage."
platforms = ["linux", "macos", "windows"]

[[build]]            # fetch prebuilt binary for this platform; fall back to cargo build --release
command = [...]      # exact form decided by the spike (§10)

[[startup]]
command = [...]      # jarvis collect

[[actions]]
id = "open"
title = "Open Jarvis"
contexts = ["workspace"]
command = [...]      # opens the overlay pane

[[panes]]
id = "core"
title = "Jarvis"
placement = "overlay"
command = [...]      # jarvis tui
```

README suggests binding `jarvis.open` to `prefix+j`.

## 9. Testing

- Unit: Claude transcript parser using anonymized real JSONL fixtures (including usage de-duplication and
  shrunk-file re-read); git root / worktree detection in temp directories (including Windows path
  normalization); usage and cost aggregation; event pairing into durations.
- herdr client: parse recorded `session.snapshot` and event fixtures captured from herdr 0.9.3.
- UI: ratatui `TestBackend` snapshot tests for the core view (deterministic animation frame via an injected
  clock), drill-down, and compact fallback.
- CI (GitHub Actions): `cargo test` and `cargo clippy -- -D warnings` on ubuntu, macos and windows.
- Manual: `herdr plugin link .` on Windows and one Unix platform before each release.

## 10. Spike before implementation (~1 h)

Three platform questions decide manifest commands and collector lifetime:

1. Can a `[[startup]]` command run a long-lived process, or is it expected to exit? If it must exit, the
   collector is started by the TUI (detached) and/or re-spawned from `[[events]]` hooks.
2. Are relative paths in manifest `command` resolved against the plugin root on Windows, or only via `PATH`? If
   only `PATH`, the build step installs the binary to a user bin directory, or the command goes through a launcher
   that is on `PATH`.
3. How `events.subscribe` streams over the socket on Windows (named pipe vs. Unix socket path), and whether the
   CLI's `HERDR_SOCKET_PATH` is directly usable from Rust.

Results are recorded in this spec before the implementation plan is finalized.

## 11. Distribution

- License: MIT.
- README: GIF of the core view (recorded with vhs), install, keys, config, screenshots of drill-down and usage.
- GitHub Actions release on tag `v*`: binaries for x86_64/aarch64 Linux, x86_64/aarch64 macOS, x86_64 Windows.
- Repository topic `herdr-plugin`, so herdr.dev's marketplace indexes it automatically.
- Install: `herdr plugin install NexorPL/herdr-jarvis`.

## 12. Future (explicitly out of v1)

- Transcript adapters: Codex, OpenCode, Pi.
- Sending prompts / answering blocked agents from Jarvis.
- Notifications when an agent blocks.
- Web dashboard reusing the same data modules.
