//! Persistent in-memory cache for GitHub repository stats.
//! Serialised to `~/.cache/hexplorer/gh_stats.json`.

use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::api::{GithubStats, SearchItem};

// ── TTL ───────────────────────────────────────────────────────────────────────

/// Stats stay fresh for 6 h; a re-fetch is triggered after this window.
const TTL_SECS: u64 = 6 * 3600;

/// Entries older than 7 × TTL are pruned from the map on every write.
const PRUNE_SECS: u64 = TTL_SECS * 7;

// ── On-disk entry ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedEntry {
    pub stars: u32,
    pub forks: u32,
    pub issues: u32,
    pub cached_at: u64, // Unix timestamp (seconds)
}

impl CachedEntry {
    pub fn is_fresh(&self) -> bool {
        unix_now().saturating_sub(self.cached_at) < TTL_SECS
    }

    /// Human-readable age: "just now", "42m ago", "3h ago".
    pub fn age_label(&self) -> String {
        let secs = unix_now().saturating_sub(self.cached_at);
        if secs < 60 {
            "just now".into()
        } else if secs < 3600 {
            format!("{}m ago", secs / 60)
        } else {
            format!("{}h ago", secs / 3600)
        }
    }
}

impl From<&GithubStats> for CachedEntry {
    fn from(s: &GithubStats) -> Self {
        Self {
            stars: s.stars,
            forks: s.forks,
            issues: s.issues,
            cached_at: unix_now(),
        }
    }
}

// ── Cache map ─────────────────────────────────────────────────────────────────

pub type CacheMap = HashMap<String, CachedEntry>;

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("hexplorer").join("gh_stats.json"))
}

/// Load the cache from disk. Returns an empty map on any error.
pub fn load() -> CacheMap {
    let Some(path) = cache_path() else {
        return CacheMap::new();
    };
    let Ok(bytes) = fs::read(&path) else {
        return CacheMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Persist the cache map to disk. Best-effort — silently ignores failures.
pub fn save(map: &CacheMap) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(map) {
        let _ = fs::write(&path, json);
    }
}

/// Return the entry only if it is still within TTL.
pub fn get_fresh<'a>(map: &'a CacheMap, repo_url: &str) -> Option<&'a CachedEntry> {
    map.get(repo_url).filter(|e| e.is_fresh())
}

/// Return the entry regardless of freshness (show stale data while re-fetching).
pub fn get_any<'a>(map: &'a CacheMap, repo_url: &str) -> Option<&'a CachedEntry> {
    map.get(repo_url)
}

/// Store a fresh entry and flush to disk. Prunes entries older than PRUNE_SECS.
pub fn insert(map: &mut CacheMap, repo_url: String, stats: &GithubStats) {
    map.insert(repo_url, CachedEntry::from(stats));
    map.retain(|_, e| unix_now().saturating_sub(e.cached_at) < PRUNE_SECS);
    save(map);
}

// ── Docs search cache ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct DocsCacheEntry {
    items: Vec<SearchItem>,
    cached_at: u64,
}

fn docs_cache_path(package: &str) -> Option<PathBuf> {
    dirs::cache_dir().map(|d| {
        d.join("hexplorer")
            .join("docs")
            .join(format!("{package}.json"))
    })
}

/// Return cached docs items for `package` if they exist and are within `ttl_hours`.
/// Returns `None` when ttl_hours == 0 (cache disabled) or entry is stale/missing.
pub fn get_docs(package: &str, ttl_hours: u32) -> Option<Vec<SearchItem>> {
    if ttl_hours == 0 {
        return None;
    }
    let path = docs_cache_path(package)?;
    let bytes = fs::read(&path).ok()?;
    let entry: DocsCacheEntry = serde_json::from_slice(&bytes).ok()?;
    let ttl_secs = ttl_hours as u64 * 3600;
    if unix_now().saturating_sub(entry.cached_at) < ttl_secs {
        Some(entry.items)
    } else {
        None
    }
}

/// Write docs items for `package` to disk. No-op when ttl_hours == 0.
pub fn insert_docs(package: &str, items: &[SearchItem], ttl_hours: u32) {
    if ttl_hours == 0 {
        return;
    }
    let Some(path) = docs_cache_path(package) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let entry = DocsCacheEntry {
        items: items.to_vec(),
        cached_at: unix_now(),
    };
    if let Ok(json) = serde_json::to_vec_pretty(&entry) {
        let _ = fs::write(&path, json);
    }
}

/// Remove all cached docs files.
pub fn clear_docs() {
    let Some(dir) = dirs::cache_dir().map(|d| d.join("hexplorer").join("docs")) else {
        return;
    };
    let _ = fs::remove_dir_all(dir);
}
