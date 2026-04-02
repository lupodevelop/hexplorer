# Changelog

All notable changes to hexplorer are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
