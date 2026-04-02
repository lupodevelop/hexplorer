//! Handlers for the `hexplorer storage <subcommand>` CLI surface.

use std::io::{self, Write};

use anyhow::Result;

use crate::{args::StorageSubcmd, storage};

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub fn run(cmd: StorageSubcmd) -> Result<()> {
    match cmd {
        StorageSubcmd::Status => run_status(),
        StorageSubcmd::Prune { yes } => run_prune(yes),
        StorageSubcmd::Clear => run_clear(),
        StorageSubcmd::Config(arg) => run_config(arg.as_deref()),
    }
}

// ── status ────────────────────────────────────────────────────────────────────

fn run_status() -> Result<()> {
    let s = storage::status()?;
    let meta = storage::load_meta()?;

    let cache_dir = storage::cache_dir()?;
    println!("\n  {}/", cache_dir.display());
    println!(
        "  ├── gh_stats.json    {:>4} entries  · {:>6}  · last write: –",
        s.gh_cache_entries,
        human_bytes(s.gh_cache_bytes),
    );
    println!(
        "  └── snapshots/       {:>4} files   · {:>6}",
        s.languages.iter().map(|l| l.count).sum::<usize>(),
        human_bytes(s.languages.iter().map(|l| l.total_bytes).sum()),
    );

    for ls in &s.languages {
        let oldest = ls.oldest.as_deref().unwrap_or("–");
        println!(
            "      ├── {:<8}      {:>3} files  · {:>6}  · oldest: {}",
            ls.lang.label(),
            ls.count,
            human_bytes(ls.total_bytes),
            oldest,
        );
    }

    println!();
    println!("  retention policy : {} weeks", s.config.keep_weeks);
    if let Some(p) = &meta.last_prune {
        println!("  last prune       : {}", &p[..10]);
    } else {
        println!("  last prune       : never");
    }
    println!("  total            : {}", human_bytes(s.total_bytes));
    println!();
    Ok(())
}

// ── prune ─────────────────────────────────────────────────────────────────────

fn run_prune(yes: bool) -> Result<()> {
    let meta = storage::load_meta()?;
    let weeks = meta.config.keep_weeks;

    if weeks == 0 {
        println!("Snapshots are disabled (keep_weeks=0). Nothing to prune.");
        return Ok(());
    }

    // Dry-run: collect what would be removed.
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::weeks(weeks as i64);

    // We use the public prune function directly but first report what it'll do.
    // Since prune() actually deletes, we list files manually for the dry-run.
    use std::fs;
    let snaps_dir = storage::cache_dir()?.join("snapshots");
    let mut to_remove: Vec<(std::path::PathBuf, u64)> = vec![];

    if snaps_dir.exists() {
        for entry in fs::read_dir(&snaps_dir)? {
            let path = entry?.path();
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let date_part = name.split_once('_').map(|x| x.1).unwrap_or("");
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_part, "%Y%m%d") {
                if date < cutoff {
                    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    to_remove.push((path, bytes));
                }
            }
        }
    }
    to_remove.sort_by_key(|(p, _)| p.clone());

    if to_remove.is_empty() {
        println!("Nothing to prune (all snapshots within {weeks}-week retention window).");
        return Ok(());
    }

    println!("\n  Files to remove (older than {weeks} weeks):");
    let total: u64 = to_remove.iter().map(|(_, b)| b).sum();
    for (path, bytes) in &to_remove {
        println!(
            "    {}   {}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            human_bytes(*bytes),
        );
    }
    println!(
        "\n  {} files · {} freed\n",
        to_remove.len(),
        human_bytes(total)
    );

    let confirmed = yes || confirm("  Proceed? [y/N]");
    if !confirmed {
        println!("Aborted.");
        return Ok(());
    }

    for (path, _) in &to_remove {
        fs::remove_file(path)?;
    }

    let mut meta = storage::load_meta()?;
    meta.last_prune = Some(chrono::Utc::now().to_rfc3339());
    storage::save_meta(&meta)?;

    println!("Done. Removed {} files.", to_remove.len());
    Ok(())
}

// ── clear ─────────────────────────────────────────────────────────────────────

fn run_clear() -> Result<()> {
    let s = storage::status()?;

    let snap_count: usize = s.languages.iter().map(|l| l.count).sum();
    println!("\n  This will remove:");
    println!(
        "    {} snapshot files    {}",
        snap_count,
        human_bytes(s.total_bytes - s.gh_cache_bytes)
    );
    println!(
        "    gh_stats.json cache  {}\n",
        human_bytes(s.gh_cache_bytes)
    );

    // Require explicit "yes" — intentional friction for a destructive operation.
    print!("  Type \"yes\" to confirm: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim() != "yes" {
        println!("Aborted.");
        return Ok(());
    }

    let cache_dir = storage::cache_dir()?;
    let snaps_dir = cache_dir.join("snapshots");
    if snaps_dir.exists() {
        std::fs::remove_dir_all(&snaps_dir)?;
    }
    let gh = cache_dir.join("gh_stats.json");
    if gh.exists() {
        std::fs::remove_file(&gh)?;
    }

    println!("Cleared.");
    Ok(())
}

// ── config ────────────────────────────────────────────────────────────────────

fn run_config(arg: Option<&str>) -> Result<()> {
    let mut meta = storage::load_meta()?;

    match arg {
        None => {
            // Show current config.
            let token_display = storage::load_github_token()
                .map(|t| mask_token(&t))
                .unwrap_or_else(|| "(not set — falls back to GITHUB_TOKEN env var)".into());
            println!("\n  retention  keep_weeks   = {}", meta.config.keep_weeks);
            println!("  storage    compress     = {}", meta.config.compress);
            println!("  github     token        = {}", token_display);
            println!("             stored in    ~/.config/hexplorer/credentials.json (mode 0600)");
            println!();
        }
        Some(kv) => {
            let (key, val) = kv
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("expected key=value, got '{kv}'"))?;

            match key.trim() {
                "keep_weeks" => {
                    meta.config.keep_weeks = val
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("keep_weeks must be an integer"))?;
                    storage::save_meta(&meta)?;
                    println!("keep_weeks set to {}", meta.config.keep_weeks);
                }
                "compress" => {
                    meta.config.compress = matches!(val.trim(), "true" | "1" | "yes");
                    storage::save_meta(&meta)?;
                    println!("compress set to {}", meta.config.compress);
                }
                "github_token" => {
                    let t = val.trim();
                    storage::save_github_token(if t.is_empty() { None } else { Some(t) })?;
                    if t.is_empty() {
                        println!("github_token cleared.");
                    } else {
                        println!(
                            "github_token set ({}) → ~/.config/hexplorer/credentials.json (0600)",
                            mask_token(t)
                        );
                    }
                    return Ok(()); // meta unchanged, skip save_meta below
                }
                other => anyhow::bail!(
                    "unknown config key: '{other}' (valid: keep_weeks, compress, github_token)"
                ),
            }
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mask_token(t: &str) -> String {
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= 8 {
        "***".to_string()
    } else {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}…{}", head, tail)
    }
}

fn human_bytes(b: u64) -> String {
    if b >= 1_000_000 {
        format!("{:.1} MB", b as f64 / 1_000_000.0)
    } else if b >= 1_000 {
        format!("{:.1} KB", b as f64 / 1_000.0)
    } else {
        format!("{b} B")
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} ");
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    matches!(buf.trim().to_lowercase().as_str(), "y" | "yes")
}
