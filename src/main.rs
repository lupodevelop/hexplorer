//! hexplorer — BEAM ecosystem explorer for HEX.pm
//!
//! Usage:
//!   hexplorer                                    # interactive TUI (uses configured default_language)
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
mod docs;
mod export_types;
mod favorites;
mod fmt;
mod output;
mod storage;
mod storage_cmd;
mod types;
mod ui;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use log::{debug, info};
use ratatui::prelude::*;
use simplelog::{Config, LevelFilter, WriteLogger};
use std::fs::OpenOptions;
use std::path::Path;
use sysinfo::{CpuExt, System, SystemExt};
use tokio::sync::mpsc;

use args::Args;

fn default_log_path() -> Result<String> {
    let dir = storage::logs_dir()?;
    let path = dir.join(format!(
        "hexplorer-{}.log",
        chrono::Local::now().format("%Y%m%d")
    ));
    Ok(path.to_string_lossy().to_string())
}

fn init_logger(log_file: Option<&str>) -> Result<()> {
    let path = if let Some(path) = log_file {
        path.to_string()
    } else {
        default_log_path()?
    };

    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent).context("creating log file directory")?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("opening log file")?;

    let config = Config::default();

    WriteLogger::init(LevelFilter::Debug, config, file).context("initialising log file")?;

    info!("[startup] hexplorer started");
    info!("[log] file={path}");
    debug!("[debug] logging initialized to {path}");
    Ok(())
}

fn log_startup_preferences(args: &Args, meta: &storage::Meta) {
    info!(
        "[preference] startup args output={:?} lang={} lang_explicit={} search={:?} sort={:?} package={:?} log_file={:?} storage_cmd={:?}",
        args.output,
        args.language,
        args.lang_explicit,
        args.search,
        args.sort,
        args.package,
        args.log_file,
        args.storage_cmd,
    );
    info!(
        "[preference] startup config keep_weeks={} compress={} color_scheme={} default_language={} link_style={} docs_cache_ttl_hours={} log_retention_days={}",
        meta.config.keep_weeks,
        meta.config.compress,
        meta.config.color_scheme.label(),
        meta.config.default_language,
        meta.config.link_style.label(),
        meta.config.docs_cache_ttl_hours,
        meta.config.log_retention_days,
    );
}

fn log_system_info() {
    let mut sys = System::new_all();
    sys.refresh_all();

    let os_name = sys
        .name()
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    let os_version = sys
        .long_os_version()
        .or_else(|| sys.os_version())
        .unwrap_or_default();
    let kernel_version = sys.kernel_version().unwrap_or_default();
    let host_name = sys.host_name().unwrap_or_default();
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|cpu| cpu.brand())
        .unwrap_or_default();
    let cpu_count = sys.cpus().len();
    let total_mem_mb = sys.total_memory() / (1024 * 1024);
    let free_mem_mb = sys.available_memory() / (1024 * 1024);
    let arch = std::env::consts::ARCH;
    let family = std::env::consts::FAMILY;

    info!(
        "[system] platform={} family={} arch={} os_name={} os_version={} kernel_version={} hostname={} cpu_count={} cpu_brand={} total_mem_mb={} free_mem_mb={}",
        std::env::consts::OS,
        family,
        arch,
        os_name,
        os_version,
        kernel_version,
        host_name,
        cpu_count,
        cpu_brand,
        total_mem_mb,
        free_mem_mb,
    );
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = args::parse_args()?;
    let meta = storage::load_meta().unwrap_or_default();
    // cleanup_logs runs before the logger is initialized — errors go to stderr.
    if let Err(e) = storage::cleanup_logs(meta.config.log_retention_days) {
        eprintln!("[warn] log cleanup failed: {e}");
    }
    init_logger(args.log_file.as_deref())?;
    log_system_info();
    log_startup_preferences(&args, &meta);

    // 1. Storage subcommand — sync, no TUI.
    if let Some(cmd) = args.storage_cmd {
        return storage_cmd::run(cmd);
    }

    // Apply config default_language when --lang was not passed explicitly.
    if !args.lang_explicit {
        if let Ok(meta) = storage::load_meta() {
            args.language = meta.config.default_language;
        }
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

    let result = event_loop(&mut terminal, &mut application, &mut rx);

    // Always restore terminal, even on error.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    // Persist GitHub stats cache.
    cache::save(&application.cache);

    result
}

fn event_loop(
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
                Event::Key(key) if key.kind == KeyEventKind::Press && app.on_key(key) => {
                    break;
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
