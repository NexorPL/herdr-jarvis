# Jarvis

**Mission control for [herdr](https://herdr.dev).** One keypress opens Jarvis in its own tab, and it stays
open while you work. Its animated core: every project with herdr activity branches out of it, showing which agents are working,
which are done and which are blocked on you. Drill into a project for its agents, Claude Code threads,
event timeline and token cost.

Linux, macOS and Windows. herdr 0.9.0+.

## Install

```bash
herdr plugin install NexorPL/herdr-jarvis
```

The install step downloads a prebuilt, SHA-256-verified binary for your platform and falls back to
`cargo build --release` (Rust from https://rustup.rs) when none matches.

Open it with the `Open Jarvis` action, or bind a key in herdr's `config.toml`. If Jarvis is already open,
the same key jumps to its tab instead of starting a second one:

```toml
[[keys.command]]
key = "prefix+j"
type = "shell"
command = "herdr plugin pane open --plugin jarvis --entrypoint core --focus"
```

## What you see

- **Core**: the status bar with global counts and today's estimated cost; the reactor in the middle
  (spins faster with more working agents, turns amber for unseen results and pulses red when an agent
  is blocked); one branch per project, the most urgent first, overflow folded into `+N more`; a feed of
  the latest events at the bottom.
- **Project drill-down** (`Enter` on a node): tabs Agents, Threads, Timeline and Usage for that project.
- **All projects**: `A` agents, `T` threads, `L` timeline, `U` usage.

Projects are grouped by git repository; linked worktrees sit under their main repository. Threads and
usage come from Claude Code transcripts (`~/.claude/projects`); other agents appear in the tree and the
timeline, without threads or cost.

## Keys

| Where | Keys |
|---|---|
| Core | arrows / `hjkl` move · `1`–`9` select · `Enter` open project · `A` `T` `L` `U` all-project views |
| Lists | `↑↓` / `jk` move · `Enter` jump to the agent's pane (Jarvis stays open in its tab), or copy `claude --resume <id>` for a finished thread · `Tab` / `1`–`4` switch views |
| Filters | `/` search threads · `s` state · `w` time range (timeline) · `f` project (all-project views) |
| Anywhere | `Esc` back · `r` refresh · `?` help · `q` close Jarvis |

## Configuration

Optional `config.toml` in the plugin config directory (`herdr plugin config-dir jarvis`):

```toml
animation = "full"        # full | reduced | off
retention_days = 30       # timeline history
max_branches = 8          # projects on the core before "+N more"
claude_dir = "~/.claude"  # defaults to CLAUDE_CONFIG_DIR or ~/.claude

# Override or add a model price (USD per million tokens; all five fields required)
[pricing."claude-opus-5-5"]
input = 4.0
output = 20.0
cache_read = 0.20
cache_write_5m = 5.0
cache_write_1h = 8.0
```

Costs are estimates from public per-token prices, shown with `≈`.

## How it works

`jarvis collect` starts with herdr (detached, single instance) and records agent state changes to
daily JSONL files in the plugin state directory, so the timeline covers time when the overlay was
closed. herdr restores a session's layout but not plugin processes, so on startup Jarvis also replaces
the restored, empty "Jarvis" tab with a live one (without taking the focus). Jarvis reads herdr's socket API, an incremental index of Claude Code transcripts and those
files. Nothing leaves your machine.

## Development

```bash
cargo test
cargo build --release
herdr plugin link "$(pwd)"
herdr plugin action invoke jarvis.open
```

On Windows the running collector keeps `target/release/jarvis.exe` locked. `scripts/fetch-or-build.ps1`
moves it aside before building; with plain `cargo build`, rename or stop it first
(`taskkill /IM jarvis.exe /F` also closes an open overlay).

## Contributing

`main` changes only through pull requests, merged with a merge commit (no squash, no rebase) after CI
passes. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, the checks to run and how to test a change in herdr.

## License

MIT
