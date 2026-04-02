//! Serialisable types for JSON/Markdown export and snapshot storage.
//! These are separate from the flat `api::Package` used by the TUI.
//!
//! Schema version history:
//!   v1 (current) — initial schema
//!   v2           — add `dependents_count`, `releases[]` history (planned)

use serde::{Deserialize, Serialize};

use crate::api::{GithubStats, Package};

pub const SCHEMA_VERSION: u8 = 1;

// ── Package export ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageExport {
    pub schema_version: u8,
    pub id: String,
    pub language: String, // "gleam" | "elixir" | "erlang" | "unknown"
    pub build_tool: String,
    pub release: PackageRelease,
    pub description: String,
    pub downloads: PackageDownloads,
    pub github: Option<PackageGithub>,
    pub links: PackageLinks,
    pub licenses: Vec<String>,
    pub meta: PackageMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageRelease {
    pub latest: String,
    pub latest_stable: Option<String>,
    pub published_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageDownloads {
    pub all_time: u64,
    pub recent_90d: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageGithub {
    pub url: String,
    pub stars: u32,
    pub forks: u32,
    pub open_issues: u32,
    pub fetched_at: String,
    /// `"live"` | `"cached"` | `"unavailable"`
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageLinks {
    pub hex: String,
    pub docs: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageMeta {
    pub fetched_at: String,
    pub data_source: String,
}

impl PackageExport {
    /// Build from a flat `Package`, optionally attaching live GitHub stats.
    pub fn from_package(pkg: &Package, github: Option<PackageGithubInput>) -> Self {
        let lang_str = match pkg.language {
            crate::types::Language::All => "unknown",
            other => Box::leak(other.to_string().into_boxed_str()),
        };

        let github_field = github.map(|g| PackageGithub {
            url: pkg.repo_url.clone().unwrap_or_default(),
            stars: g.stats.stars,
            forks: g.stats.forks,
            open_issues: g.stats.issues,
            fetched_at: g.fetched_at,
            source: g.source,
        });

        Self {
            schema_version: SCHEMA_VERSION,
            id: pkg.name.clone(),
            language: lang_str.to_string(),
            build_tool: pkg.build_tool.clone(),
            release: PackageRelease {
                latest: pkg.version.clone(),
                latest_stable: Some(pkg.version.clone()),
                published_at: pkg.inserted_at.clone(),
                updated_at: pkg.updated_at.clone(),
            },
            description: pkg.description.clone(),
            downloads: PackageDownloads {
                all_time: pkg.downloads_all,
                recent_90d: pkg.downloads_recent,
            },
            github: github_field,
            links: PackageLinks {
                hex: pkg.hex_url.clone().unwrap_or_default(),
                docs: pkg.docs_url.clone(),
                repository: pkg.repo_url.clone(),
            },
            licenses: pkg.licenses.clone(),
            meta: PackageMeta {
                fetched_at: chrono::Utc::now().to_rfc3339(),
                data_source: "hex.pm/api/v1".into(),
            },
        }
    }
}

/// Input helper for constructing `PackageGithub` inside `from_package`.
pub struct PackageGithubInput {
    pub stats: GithubStats,
    pub fetched_at: String,
    pub source: String,
}

// ── Snapshot ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u8,
    pub query: SnapshotQuery,
    pub meta: SnapshotMeta,
    pub packages: Vec<PackageExport>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotQuery {
    pub language: String,
    pub search: String,
    pub sort: String,
    pub page: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub total_results: usize,
    pub fetched_at: String,
    pub app_version: String,
}

impl Snapshot {
    pub fn build(packages: &[Package], language: &str, search: &str, sort: &str) -> Self {
        let exports: Vec<PackageExport> = packages
            .iter()
            .map(|p| PackageExport::from_package(p, None))
            .collect();

        Snapshot {
            schema_version: SCHEMA_VERSION,
            query: SnapshotQuery {
                language: language.to_string(),
                search: search.to_string(),
                sort: sort.to_string(),
                page: 1,
            },
            meta: SnapshotMeta {
                total_results: exports.len(),
                fetched_at: chrono::Utc::now().to_rfc3339(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            packages: exports,
        }
    }
}
