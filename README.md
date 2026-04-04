<p align="center">
  <img src="https://raw.githubusercontent.com/lupodevelop/hexplorer/e705653c020fc332c5b8c5f45fc0a6b034e15330/assets/img/logo.png" alt="hexplorer logo" width="256" />
</p>


# hexplorer

[![crates.io](https://img.shields.io/crates/v/hexplorer.svg)](https://crates.io/crates/hexplorer) [![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![Workflow](https://img.shields.io/github/actions/workflow/status/lupodevelop/hexplorer/ci.yml?branch=main)](https://github.com/lupodevelop/hexplorer/actions)

Terminal UI for browsing [Hex.pm](https://hex.pm) the package registry for the BEAM ecosystem (Gleam, Elixir, Erlang).

<p align="center">
  <img src="https://raw.githubusercontent.com/lupodevelop/hexplorer/e705653c020fc332c5b8c5f45fc0a6b034e15330/assets/img/terminal.png" alt="hexplorer TUI screenshot" width="640" />
</p>

## Install

### Cargo (recommended)

If this crate is published to crates.io:

```sh
cargo install hexplorer
```

This installs the binaries under `$HOME/.cargo/bin` and is the easiest way to get a stable release.

#### From local source

If you are developing locally or want the latest commit:

```sh
git clone https://github.com/lupodevelop/hexplorer
cd hexplorer
cargo install --path .
```

#### From GitHub directly

```sh
cargo install --git https://github.com/lupodevelop/hexplorer --branch main
```

### Build from source

```sh
git clone https://github.com/lupodevelop/hexplorer
cd hexplorer
cargo build --release
# binary at ./target/release/hexplorer
```

## Usage

```
hexplorer                              # interactive TUI — Gleam tab by default
hexplorer --lang elixir                # start on Elixir tab
hexplorer --lang all                   # start on All BEAM tab

hexplorer --output json                              # top Gleam packages as JSON
hexplorer --output json --lang elixir               # top Elixir packages as JSON
hexplorer --output json --lang all --search http    # search "http" across all BEAM
hexplorer --output compact                          # Markdown table, top Gleam
hexplorer --output compact --lang erlang --search cowboy
hexplorer --output detail lustre                    # Markdown detail block for exact package name
hexplorer --output detail --lang elixir --search phoenix  # detail for first search result

hexplorer storage status               # show cache / snapshot usage
hexplorer storage prune                # remove snapshots beyond retention window
hexplorer storage prune --yes          # skip confirmation prompt
hexplorer storage clear                # wipe all cached data (requires typing "yes")
hexplorer storage config                          # show current config
hexplorer storage config keep_weeks=4             # set snapshot retention
hexplorer storage config default_language=elixir  # set startup language (gleam/elixir/erlang/all)
hexplorer storage config github_token=ghp_...    # store GitHub token persistently
hexplorer storage config github_token=           # remove stored token
hexplorer storage config log_retention_days=14   # keep logs for 14 days (0 = disable)

hexplorer --log-file /tmp/hexplorer.log          # write diagnostics to a custom path
```

## Keyboard shortcuts

### List view

| Key | Action |
| --- | --- |
| `/` | Enter search mode |
| `Enter` | Submit search / open detail |
| `Backspace` | Delete previous character (search mode) |
| `Ctrl+W` / `Ctrl+Backspace` | Delete previous word (search mode) |
| `Esc` / `q` | Back / quit |
| `↑↓` `j k` | Navigate list |
| `PgUp/Dn` | Fast scroll |
| `l` / `L` | Cycle language tab forward / backward |
| `Tab` | Cycle sort order |
| `]` / `[` | Next / previous page (browse mode only) |
| `s` | Star / unstar the selected package |
| `f` | Toggle favorites view (shows all starred packages) |
| `D` | Search HexDocs for the selected package |
| `r` | Refresh |
| `?` | Open settings |

### Detail view

| Key | Action |
| --- | --- |
| `Esc` / `q` | Back to list |
| `↑↓` `j k` | Scroll |
| `PgUp/Dn` | Fast scroll |
| `Tab` / `Shift+Tab` | Cycle through available links (docs, hex.pm, repo) |
| `Enter` | Open selected link in system browser |
| `s` | Search HexDocs for this package |
| `r` | Force-refresh GitHub stats and version history |

### HexDocs search view

| Key | Action |
| --- | --- |
| `Esc` / `q` | Back to previous view |
| `↑↓` `j k` | Navigate results |
| `Enter` | Open result in system browser |

## GitHub stats

hexplorer fetches stars, forks, and open issues for packages with a GitHub repo.
Without a token the GitHub API allows 60 requests/hour; with one, 5 000/hour.

**Option 1 — store persistently** (survives reboots, no shell config needed):

```sh
hexplorer storage config github_token=ghp_...
```

**Option 2 — env var** (per-session or via `.zshrc`/`.bashrc`):

```sh
export GITHUB_TOKEN=ghp_...
```

The env var takes priority over the stored token. Generate one at
[github.com/settings/tokens](https://github.com/settings/tokens) — only the
`public_repo` read scope is required.

The stored token is written to `~/.config/hexplorer/credentials.json` with `0600`
permissions (owner read/write only), separate from the cache directory so it
is not swept up by backup tools that sync `~/.cache/`.

Stats are cached locally for 6 hours under `~/.cache/hexplorer/gh_stats.json`.

## Output modes

Pipe to [`llm`](https://llm.datasette.io), `jq`, or any other tool:

```sh
# Compact Markdown table of top Gleam packages
hexplorer --output compact --lang gleam | llm "which of these should I use for HTTP?"

# Full JSON snapshot
hexplorer --output json --lang elixir | jq '.[].name'

# Detailed block for a single package
hexplorer --output detail lustre
```

## HexDocs search

Press `D` from the list view (or `s` from the detail view) to search the documentation of the selected package directly inside the TUI. hexplorer fetches the package's ExDoc search index and filters results locally — no browser required until you open one.

Results show the item type (`value`, `module`, `page`, `type`, `callback`), title, parent module, a doc snippet, and the target URL. Press `Enter` to open the exact page in your system browser.

The search index is cached to disk (default 24h TTL). Configure the TTL via `?` → settings → `docs cache`.

## Diagnostics logging

hexplorer writes a daily log file to:

```
~/.cache/hexplorer/logs/hexplorer-YYYYMMDD.log
```

Logs cover startup arguments and config, system information (OS, CPU, RAM), every key event with current view, view transitions, async message flow (fetch gen, result counts), all API requests with status codes, and errors. They are useful for diagnosing rendering or fetch issues.

Log files are pruned automatically at startup. Default retention is **7 days**. Change it:

```sh
hexplorer storage config log_retention_days=14   # keep 14 days
hexplorer storage config log_retention_days=0    # disable pruning
```

To write to a custom path for a single session:

```sh
hexplorer --log-file /tmp/hexplorer-debug.log
```

Logs never include stored API credentials or GitHub tokens.

## Cache & snapshots

All data lives under `~/.cache/hexplorer/`:

```
~/.cache/hexplorer/
├── docs/                ExDoc search indexes per package (24h TTL, configurable)
│   ├── gleam_stdlib.json
│   └── ...
├── favorites.json       Starred package names
├── gh_stats.json        GitHub stats (6h TTL, pruned at 42h)
├── logs/                Daily diagnostics logs (7-day retention by default)
│   └── hexplorer-20260404.log
├── meta.json            Config + digest timestamps
└── snapshots/           Weekly package snapshots
    ├── gleam_20260322.json
    └── ...
```

Snapshot retention: **12 weeks**. Configure with `hexplorer storage config keep_weeks=N`.
Log retention: **7 days**. Configure with `hexplorer storage config log_retention_days=N`.
Docs cache TTL: configurable via `?` → settings → `docs cache` (presets: off, 1h, 6h, 12h, 24h, 48h, 1 week).

## License

MIT
