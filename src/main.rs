//! hexplorer — BEAM ecosystem explorer for HEX.pm
//!
//! Usage:
//!   hexplorer                                    # interactive TUI (Gleam by default)
//!   hexplorer --lang elixir                      # TUI starting on Elixir tab
//!   hexplorer --output json                      # JSON snapshot to stdout
//!   hexplorer --output compact --lang gleam      # Markdown table to stdout
//!   hexplorer --output detail lustre             # Markdown detail block for one package
//!   hexplorer storage status                     # show cache/snapshot usage
//!   hexplorer storage prune [--yes]              # remove snapshots beyond retention
//!   hexplorer storage clear                      # wipe everything (interactive confirm)
//!   hexplorer storage config [keep_weeks=N]      # read/write retention config

mod api;
mod app;
mod args;
mod cache;
mod export_types;
mod favorites;
mod fmt;
mod output;
mod storage;
mod storage_cmd;
mod types;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use args::Args;

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args = args::parse_args()?;

    // 1. Storage subcommand — sync, no TUI.
    if let Some(cmd) = args.storage_cmd {
        return storage_cmd::run(cmd);
    }

    // 2. Output mode — async fetch, no TUI.
    if args.output.is_some() {
        return output::run(&args).await;
    }

    // 3. Interactive TUI.
    run_tui(args).await
}

// ── TUI runtime ───────────────────────────────────────────────────────────────

async fn run_tui(args: Args) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<app::Msg>(64);
    let mut application = app::App::new(tx, args.language);
    application.load();

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = event_loop(&mut terminal, &mut application, &mut rx).await;

    // Always restore terminal, even on error.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    // Persist GitHub stats cache.
    cache::save(&application.cache);

    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut app::App,
    rx: &mut mpsc::Receiver<app::Msg>,
) -> Result<()> {
    loop {
        // Drain async messages before drawing.
        while let Ok(msg) = rx.try_recv() {
            app.on_msg(msg);
        }

        terminal.draw(|f| ui::render(f, app))?;

        // Wait up to 50ms for an event, then re-draw (handles async updates and resize).
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press && app.on_key(key) {
                        break;
                    }
                }
                Event::Resize(_, _) => {} // next loop iteration redraws
                _ => {}
            }
        }

        // Drain again — a key event may have triggered a new fetch.
        while let Ok(msg) = rx.try_recv() {
            app.on_msg(msg);
        }
    }
    Ok(())
}
