# Changelog

All notable changes to hexplorer are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.6] — 2026-04-07

### Added

- **Live filter in HexDocs search view** — after results load, type any characters to narrow the list in real time. The header search bar shows the active filter. `Backspace` / `Ctrl+W` work as usual. Exiting the view (`Esc`/`q`) restores the previous list search keyword.

### Fixed

- **HexDocs search regression for Gleam packages** — `parse_search_data` used `find('=')` on the entire body, hitting `=` inside string values (URLs, `key=value` patterns) and slicing the JSON incorrectly. Pure JSON is now parsed directly; `=` is only scanned after the `searchData` keyword.
- **Docs search cascade extended** — direct candidates now include `search_data.js`, `search-data.js`, `dist/search_data.js`, `dist/search-data.js` for packages that serve the index as a JS file. HTML asset discovery updated with the same variants.

## [0.1.5] — 2026-04-05

### Fixed

- **HexDocs search broken for Elixir/Erlang packages** — modern ExDoc (≥ v0.34) no longer generates `search_data.json` or `search-data.json`. Instead it generates `dist/search_data-{DIGEST}.js` containing a JS variable assignment (`searchData={...}`). hexplorer now falls back to fetching `search.html`, scraping the `<script src="dist/search_data-*.js">` tag, fetching the JS file, and parsing its `searchData=` payload. Packages built with older ExDoc (flat JSON) continue to work as before.
- **Cross-language package leaking into filtered views** — when searching by a specific language (e.g. Gleam) and the full-text search returned no results, the exact-name fallback lookup would accept packages with unknown build tools (`Language::All`) and re-label them as the selected language. For example, `credo` (Elixir) appeared as a Gleam package. The fallback now only includes packages whose `build_tools` positively confirm the selected language; packages from a different ecosystem are silently skipped and the search returns empty.
- **Docs search error surfaced in UI** — when all fetch strategies fail, a red error message is shown in the docs-search view instead of silently displaying an empty list.

## [0.1.4] — 2026-04-04

### Added

- **HexDocs in-TUI search** — press `D` from the list view or `s` from the detail view to open a search modal. After typing a term and pressing `Enter`, hexplorer fetches the package's ExDoc search index (`search_data.json` / `search-data.json`) and shows filtered results in a dedicated `DocsSearch` view with type badges, doc snippets, and the target URL. `Enter` on a result opens the exact page in the system browser; `Esc`/`q` returns to the previous view.
- **Docs search cache** — the ExDoc search index is cached to `~/.cache/hexplorer/docs/{package}.json`. Default TTL is 24 hours. Configurable via `?` → settings → `docs cache` with `←`/`→` (presets: off · 1h · 6h · 12h · 24h · 48h · 1 week). "Clear cache" in settings now also wipes the docs cache.
- **Session cache for package listings** — navigation between pages, sort changes, and language switches no longer trigger redundant HTTP requests within the same session. The cache is keyed on `(query, sort, language, page)` and cleared on manual refresh (`r`).
- **Package version history in detail view** — opening a package detail now fetches the full release list from `GET /api/packages/{name}` in the background. Up to 10 most-recent versions are shown in a new `versions` section; the list updates live when the fetch completes.
- **`(cached)` indicator in header** — when a listing is served from the session cache, the package count badge shows `(cached)`.
- **Diagnostics logging** — hexplorer writes a daily log file to `~/.cache/hexplorer/logs/hexplorer-YYYYMMDD.log` (via `log` + `simplelog`). Covers startup args, system info (OS, CPU, RAM via `sysinfo`), key events, view transitions, async message flow (fetch gen, result counts), all API calls with status codes, and errors. Pass `--log-file <path>` to write to a custom location.
- **Log retention** — log files older than the configured window are pruned automatically at startup. Default is 7 days. Configure via `hexplorer storage config log_retention_days=N` (`0` disables pruning).

### Changed

- Detail view `r` key now force-refreshes both GitHub stats and version history for the current package.

### Fixed

- **Stale language list after cycling with `l`** — when scrolling through languages and the target language was served from the session cache, a still-in-flight async fetch for the previous language could arrive later and overwrite the visible package list with wrong results (typically All BEAM). Fixed by incrementing `fetch_gen` on cache hits as well, so any in-flight task is invalidated immediately.

## [0.1.3] — 2026-04-03

### Added
- `hexplorer storage config default_language=<gleam|elixir|erlang|all>` to persist startup language.
- `?` settings screen now includes a `default_language` row and `color_scheme` row with `←`/`→` to cycle values.
- `Ctrl+W` / `Ctrl+Backspace` now delete the previous word in search mode and in the GitHub token input field.
- Detail view: `Tab` / `Shift+Tab` cycle a cursor through the available links (docs, hex.pm, repo). `Enter` opens the selected link in the system browser.
- Settings screen: new `link_style` row (under Appearance) cycles between `Cursor ▶` (vim-like marker) and `Block ■` (solid accent-color background on the selected link row). Setting persists to `meta.json`.

### Changed
- `hexplorer` now loads `default_language` from `~/.cache/hexplorer/meta.json` when `--lang` is not explicitly passed.
- UI now persistently loads selected color scheme from storage meta.

### Fixed

- `Ctrl+W` / `Ctrl+Backspace` had no effect in search mode (issue #3).

## [0.1.2] — 2026-04-02

### Changed
- README URLs and installation examples updated to use `lupodevelop` and current commit-based raw asset paths.

## [0.1.1] — 2026-04-02

### Fixed
- `ptr_arg`: `sort_packages` now accepts `&mut [Package]` instead of `&mut Vec<Package>`
- `manual_flatten`: replaced `if let Ok(pkgs)` in the loop with `.into_iter().flatten()`
- `derivable_impls`: manual `impl Default for Args` replaced with `#[derive(Default)]`
- `manual_split_once`: three occurrences of `splitn(2, '_').nth(1)` replaced with `split_once`
- `print_literal`: en-dash `–` inlined in `println!` format string
- `match_result_ok`: `if let Some(x) = y.ok()` replaced with `if let Ok(x) = y`


## [0.1.0] — 2026-03-22

### Added
- Interactive TUI for browsing HEX.pm packages (Gleam, Elixir, Erlang, All BEAM tabs)
- Full-text search across name + description, filtered by ecosystem
- Detail view with GitHub stars, forks, open issues (live fetch + 6h disk cache)
- Non-TUI output modes: `--output json`, `--output compact`, `--output detail <name>`
- Snapshot storage under `~/.cache/hexplorer/` with configurable retention (`storage` subcommand)
- `GITHUB_TOKEN` support to raise API rate limit from 60 to 5 000 req/h

### Technical
- Language-specific search fetches up to 5 pages in parallel (~500 packages) for full-ecosystem coverage
- All BEAM mode fetches Gleam + Elixir + Erlang concurrently, merges and assigns correct language badges client-side
- Fetch generation counter prevents stale results from overwriting newer fetches
- 10s HTTP timeout on all requests
