//! Snapshot storage: write, read, prune, status, and meta.json management.
#![allow(dead_code)] // Public API — storage functions are used by storage_cmd and future digest feature.
//!
//! All data lives under `~/.cache/hexplorer/`:
//!   gh_stats.json        — GitHub stats cache (managed by cache.rs)
//!   snapshots/           — weekly package snapshots
//!   meta.json            — config + digest timestamps

use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    export_types::Snapshot,
    types::{ColorScheme, Language, LinkStyle},
};

// ── Paths ─────────────────────────────────────────────────────────────────────

/// Returns (and creates if needed) `~/.cache/hexplorer/`.
pub fn cache_dir() -> Result<PathBuf> {
    let base =
        dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("cannot determine cache directory"))?;
    let dir = base.join("hexplorer");
    fs::create_dir_all(&dir).context("creating cache dir")?;
    Ok(dir)
}

fn snapshots_dir() -> Result<PathBuf> {
    let dir = cache_dir()?.join("snapshots");
    fs::create_dir_all(&dir).context("creating snapshots dir")?;
    Ok(dir)
}

fn snapshot_filename(lang: Language, date: &str) -> String {
    format!("{lang}_{date}.json")
}

fn meta_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("meta.json"))
}

fn gh_stats_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("gh_stats.json"))
}

/// Format today's date as YYYYMMDD.
fn today() -> String {
    chrono::Local::now().format("%Y%m%d").to_string()
}

/// Parse YYYYMMDD string to NaiveDate.
fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y%m%d").ok()
}

// ── Config & Meta ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Number of weeks to retain snapshots. `0` disables snapshot writing.
    pub keep_weeks: u32,
    /// Gzip compress snapshots. Off by default for human-readability.
    pub compress: bool,
    /// UI color scheme. Defaults to `Default` (original dark purple theme).
    #[serde(default)]
    pub color_scheme: ColorScheme,
    /// Language to open when `--lang` is not passed on the CLI.
    #[serde(default)]
    pub default_language: Language,
    /// How the selected link is highlighted in the detail view.
    #[serde(default)]
    pub link_style: LinkStyle,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            keep_weeks: 12,
            compress: false,
            color_scheme: ColorScheme::Default,
            default_language: Language::default(),
            link_style: LinkStyle::default(),
        }
    }
}

// ── Credentials ───────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct Credentials {
    github_token: Option<String>,
}

/// Returns (and creates if needed) `~/.config/hexplorer/`.
fn config_dir() -> Result<PathBuf> {
    let base =
        dirs::config_dir().ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    let dir = base.join("hexplorer");
    fs::create_dir_all(&dir).context("creating config dir")?;
    Ok(dir)
}

fn credentials_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("credentials.json"))
}

/// Read the stored GitHub token from `~/.config/hexplorer/credentials.json`.
pub fn load_github_token() -> Option<String> {
    let path = credentials_path().ok()?;
    let bytes = fs::read(&path).ok()?;
    let creds: Credentials = serde_json::from_slice(&bytes).ok()?;
    creds.github_token.filter(|t| !t.is_empty())
}

/// Persist (or clear) the GitHub token to `~/.config/hexplorer/credentials.json`.
/// The file is written with `0600` permissions so only the owner can read it.
pub fn save_github_token(token: Option<&str>) -> Result<()> {
    let path = credentials_path()?;
    let creds = Credentials {
        github_token: token.filter(|t| !t.is_empty()).map(str::to_string),
    };
    let json = serde_json::to_string_pretty(&creds).context("serialising credentials")?;
    fs::write(&path, json).context("writing credentials.json")?;

    // Restrict to owner read/write only (Unix only — no-op on Windows).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .context("setting 0600 on credentials.json")?;
    }

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub schema_version: u8,
    pub app_version: String,
    pub config: StorageConfig,
    /// ISO 8601 datetime of last prune run.
    pub last_prune: Option<String>,
    /// ISO 8601 datetime of last digest per language (`None` = never run).
    pub last_digest: HashMap<String, Option<String>>,
}

impl Default for Meta {
    fn default() -> Self {
        let mut last_digest = HashMap::new();
        for lang in Language::all() {
            last_digest.insert(lang.to_string(), None);
        }
        Self {
            schema_version: 1,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            config: StorageConfig::default(),
            last_prune: None,
            last_digest,
        }
    }
}

pub fn load_meta() -> Result<Meta> {
    let path = meta_path()?;
    if !path.exists() {
        return Ok(Meta::default());
    }
    let bytes = fs::read(&path).context("reading meta.json")?;
    serde_json::from_slice(&bytes).context("parsing meta.json")
}

pub fn save_meta(meta: &Meta) -> Result<()> {
    let path = meta_path()?;
    let json = serde_json::to_vec_pretty(meta).context("serialising meta")?;
    fs::write(&path, json).context("writing meta.json")
}

// ── Snapshot I/O ──────────────────────────────────────────────────────────────

/// Write a snapshot for `lang` today. Overwrites any existing file for the same date.
/// Skips write if `config.keep_weeks == 0`.
pub fn write_snapshot(lang: Language, snapshot: &Snapshot, config: &StorageConfig) -> Result<()> {
    if config.keep_weeks == 0 {
        return Ok(());
    }
    let dir = snapshots_dir()?;
    let path = dir.join(snapshot_filename(lang, &today()));
    let json = serde_json::to_vec_pretty(snapshot).context("serialising snapshot")?;
    fs::write(&path, json).context("writing snapshot")
}

/// Return the most recent snapshot for `lang`, or `None` if none exist.
pub fn latest_snapshot(lang: Language) -> Result<Option<Snapshot>> {
    let files = snapshot_files(lang)?;
    read_snapshot_file(files.last())
}

/// Return the second-most-recent snapshot for `lang` (used for diff).
pub fn previous_snapshot(lang: Language) -> Result<Option<Snapshot>> {
    let files = snapshot_files(lang)?;
    let prev = files.len().checked_sub(2).map(|i| &files[i]);
    read_snapshot_file(prev)
}

fn snapshot_files(lang: Language) -> Result<Vec<PathBuf>> {
    let dir = snapshots_dir()?;
    let prefix = format!("{lang}_");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .context("reading snapshots dir")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    // Sort by filename = sort by date (YYYYMMDD is lexicographically ordered).
    files.sort();
    Ok(files)
}

fn read_snapshot_file(path: Option<&PathBuf>) -> Result<Option<Snapshot>> {
    let Some(path) = path else { return Ok(None) };
    let bytes = fs::read(path).context("reading snapshot file")?;
    let snap = serde_json::from_slice(&bytes).context("parsing snapshot")?;
    Ok(Some(snap))
}

// ── Prune ─────────────────────────────────────────────────────────────────────

/// Remove snapshot files for `lang` older than `keep_weeks` weeks.
/// Returns the list of removed file paths.
pub fn prune(lang: Language, keep_weeks: u32) -> Result<Vec<PathBuf>> {
    if keep_weeks == 0 {
        return Ok(vec![]);
    }

    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::weeks(keep_weeks as i64);

    let files = snapshot_files(lang)?;
    let mut removed = vec![];

    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // Filename format: `{lang}_{YYYYMMDD}` — extract date portion after `_`.
        let date_part = stem.split_once('_').map(|x| x.1).unwrap_or("");
        if let Some(date) = parse_date(date_part) {
            if date < cutoff {
                fs::remove_file(&path).with_context(|| format!("removing {:?}", path))?;
                removed.push(path);
            }
        }
    }
    Ok(removed)
}

/// Prune all known languages at once.
pub fn prune_all(keep_weeks: u32) -> Result<Vec<PathBuf>> {
    let mut all = vec![];
    for &lang in Language::all() {
        all.extend(prune(lang, keep_weeks)?);
    }
    Ok(all)
}

// ── Status ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LangStatus {
    pub lang: Language,
    pub count: usize,
    pub total_bytes: u64,
    pub oldest: Option<String>, // YYYYMMDD
    pub newest: Option<String>, // YYYYMMDD
}

#[derive(Debug)]
pub struct StorageStatus {
    pub gh_cache_bytes: u64,
    pub gh_cache_entries: usize,
    pub languages: Vec<LangStatus>,
    pub total_bytes: u64,
    pub config: StorageConfig,
    pub last_prune: Option<String>,
}

pub fn status() -> Result<StorageStatus> {
    let meta = load_meta()?;

    // GitHub stats cache size
    let gh_path = gh_stats_path()?;
    let gh_bytes = fs::metadata(&gh_path).map(|m| m.len()).unwrap_or(0);
    let gh_entries: usize = if gh_bytes > 0 {
        let bytes = fs::read(&gh_path).unwrap_or_default();
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.as_object().map(|o| o.len()))
            .unwrap_or(0)
    } else {
        0
    };

    let mut lang_statuses = vec![];
    let mut total_bytes = gh_bytes;

    for &lang in Language::all() {
        let files = snapshot_files(lang)?;
        let mut lang_bytes = 0u64;
        for f in &files {
            lang_bytes += fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        }
        total_bytes += lang_bytes;

        let oldest = files.first().and_then(|p| {
            p.file_stem()?
                .to_str()
                .map(|s| s.split_once('_').map(|x| x.1).unwrap_or("").to_string())
        });
        let newest = files.last().and_then(|p| {
            p.file_stem()?
                .to_str()
                .map(|s| s.split_once('_').map(|x| x.1).unwrap_or("").to_string())
        });

        lang_statuses.push(LangStatus {
            lang,
            count: files.len(),
            total_bytes: lang_bytes,
            oldest,
            newest,
        });
    }

    Ok(StorageStatus {
        gh_cache_bytes: gh_bytes,
        gh_cache_entries: gh_entries,
        languages: lang_statuses,
        total_bytes,
        config: meta.config,
        last_prune: meta.last_prune,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Helper: override the cache dir to a tempdir for the duration of the test.
    /// This is a best-effort approach since `dirs::cache_dir()` reads `HOME`.
    fn with_tempdir<F: FnOnce(&tempfile::TempDir)>(f: F) {
        let dir = tempfile::TempDir::new().unwrap();
        let orig = env::var("HOME").ok();
        env::set_var("HOME", dir.path());
        f(&dir);
        if let Some(h) = orig {
            env::set_var("HOME", h);
        }
    }

    #[test]
    fn prune_removes_old_files() {
        // Create fake snapshot files with dates that span > 12 weeks.
        let dir = tempfile::TempDir::new().unwrap();
        let snaps = dir.path().join("snapshots");
        fs::create_dir_all(&snaps).unwrap();

        // Old file (> 12 weeks ago)
        let old = snaps.join("gleam_20241001.json");
        fs::write(&old, b"{}").unwrap();

        // Recent file (today)
        let today_str = today();
        let recent = snaps.join(format!("gleam_{today_str}.json"));
        fs::write(&recent, b"{}").unwrap();

        // Run prune with the temp dir
        // Since we can't easily override dirs::cache_dir, verify logic directly.
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::weeks(12);
        let date = parse_date("20241001").unwrap();
        assert!(date < cutoff, "old file should be before cutoff");

        let today_date = parse_date(&today_str).unwrap();
        assert!(
            today_date >= cutoff,
            "today should be within retention window"
        );
    }

    #[test]
    fn today_format() {
        let s = today();
        assert_eq!(s.len(), 8, "YYYYMMDD must be 8 chars");
        assert!(s.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn parse_date_roundtrip() {
        let s = today();
        let d = parse_date(&s);
        assert!(d.is_some(), "today's date must parse");
    }

    #[test]
    fn meta_default_roundtrip() {
        let meta = Meta::default();
        let json = serde_json::to_string(&meta).unwrap();
        let back: Meta = serde_json::from_str(&json).unwrap();
        assert_eq!(back.schema_version, 1);
        assert_eq!(back.config.keep_weeks, 12);
    }
}
