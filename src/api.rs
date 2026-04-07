//! HEX.pm and GitHub API clients.

use anyhow::{Context, Result};
use log::{debug, error, info};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

use crate::types::{infer_language, Language};

// ── Raw HEX.pm API response types ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HexRaw {
    name: String,
    latest_version: Option<String>,
    latest_stable_version: Option<String>,
    updated_at: Option<String>,
    inserted_at: Option<String>,
    downloads: Option<RawDownloads>,
    meta: Option<RawMeta>,
    links: Option<HashMap<String, String>>,
    docs_html_url: Option<String>,
    html_url: Option<String>,
    /// Populated only by the single-package endpoint, not the listing.
    releases: Option<Vec<RawRelease>>,
}

#[derive(Debug, Deserialize)]
struct RawDownloads {
    all: Option<u64>,
    recent: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    description: Option<String>,
    licenses: Option<Vec<String>>,
    links: Option<HashMap<String, String>>,
    build_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawRelease {
    version: String,
}

// ── Public view model (TUI-flat) ──────────────────────────────────────────────

/// Flat package record optimised for TUI rendering.
/// For JSON/Markdown export see `export_types::PackageExport`.
#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub updated_at: String,
    pub inserted_at: String,
    pub downloads_all: u64,
    pub downloads_recent: u64,
    pub repo_url: Option<String>,
    pub docs_url: Option<String>,
    pub hex_url: Option<String>,
    pub licenses: Vec<String>,
    /// Language inferred from `build_tools`. `Language::All` means unknown.
    pub language: Language,
    /// Raw primary build tool string from HEX.pm metadata.
    pub build_tool: String,
    /// All published versions, newest first.
    /// Empty when the package came from a listing response (populated only via `fetch_package`).
    pub versions: Vec<String>,
}

// ── HexDocs search types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchItem {
    /// "value", "module", "page", "callback", "type"
    pub item_type: String,
    pub title: String,
    pub parent_title: String,
    /// Plain text doc snippet (HTML stripped).
    pub doc_text: String,
    /// Relative URL within the package docs, e.g. "gleeunit.html#main".
    pub ref_url: String,
}

#[derive(Deserialize)]
struct RawSearchItem {
    #[serde(rename = "type")]
    item_type: String,
    title: String,
    #[serde(rename = "parentTitle", default)]
    parent_title: String,
    #[serde(default)]
    doc: String,
    #[serde(rename = "ref")]
    ref_url: String,
}

#[derive(Deserialize)]
struct SearchData {
    items: Vec<RawSearchItem>,
}

/// Strip HTML tags via a simple char-scan — no regex dependency needed.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// ── GitHub types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GithubStats {
    pub stars: u32,
    pub forks: u32,
    pub issues: u32,
}

#[derive(Debug)]
pub enum GhResult {
    Ok(GithubStats),
    RateLimited,
    /// 401 Unauthorized — token is present but invalid or expired.
    BadToken,
    Unavailable,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn find_repo(
    top: Option<&HashMap<String, String>>,
    meta: Option<&HashMap<String, String>>,
) -> Option<String> {
    const KEYS: &[&str] = &[
        "Repository",
        "GitHub",
        "Github",
        "repository",
        "github",
        "Source",
        "source",
    ];
    for map in [top, meta].into_iter().flatten() {
        for key in KEYS {
            if let Some(u) = map.get(*key) {
                return Some(u.clone());
            }
        }
        // Fallback: any value that looks like a git forge URL.
        for u in map.values() {
            if u.contains("github.com") || u.contains("gitlab.com") {
                return Some(u.clone());
            }
        }
    }
    None
}

fn into_package(r: HexRaw) -> Package {
    let repo_url = find_repo(
        r.links.as_ref(),
        r.meta.as_ref().and_then(|m| m.links.as_ref()),
    );
    let docs_url = r
        .links
        .as_ref()
        .and_then(|l| l.get("Documentation").cloned())
        .or_else(|| r.docs_html_url.clone());

    let build_tools: &[String] = r
        .meta
        .as_ref()
        .and_then(|m| m.build_tools.as_deref())
        .unwrap_or(&[]);

    let language = infer_language(build_tools);
    let build_tool = build_tools.first().cloned().unwrap_or_default();

    Package {
        version: r
            .latest_stable_version
            .or(r.latest_version)
            .unwrap_or_else(|| "0.0.0".into()),
        description: r
            .meta
            .as_ref()
            .and_then(|m| m.description.clone())
            .unwrap_or_default(),
        updated_at: r.updated_at.unwrap_or_default(),
        inserted_at: r.inserted_at.unwrap_or_default(),
        downloads_all: r.downloads.as_ref().and_then(|d| d.all).unwrap_or(0),
        downloads_recent: r.downloads.as_ref().and_then(|d| d.recent).unwrap_or(0),
        licenses: r
            .meta
            .as_ref()
            .and_then(|m| m.licenses.clone())
            .unwrap_or_default(),
        hex_url: r.html_url,
        name: r.name,
        repo_url,
        docs_url,
        language,
        build_tool,
        versions: r
            .releases
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.version)
            .collect(),
    }
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!("hexplorer/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .build()?)
}

// ── Token ─────────────────────────────────────────────────────────────────────

/// Returns the GitHub Personal Access Token, checking in order:
/// 1. `GITHUB_TOKEN` env var (highest priority — CI, dotenv, shell export).
/// 2. Token stored in `~/.config/hexplorer/credentials.json` via `hexplorer storage config github_token=`.
pub fn github_token() -> Option<String> {
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    crate::storage::load_github_token()
}

// ── Public API calls ──────────────────────────────────────────────────────────

/// Fetch a single page of raw packages from HEX.pm.
async fn fetch_page(search: &str, sort: &str, page: u32) -> Result<Vec<Package>> {
    let page_str = page.to_string();
    let url = "https://hex.pm/api/packages";
    info!(
        "[fetch] fetch_page search={} sort={} page={} url={} ",
        search, sort, page, url
    );

    let resp = client()?
        .get(url)
        .query(&[
            ("search", search),
            ("sort", sort),
            ("page", page_str.as_str()),
        ])
        .send()
        .await?;

    let status = resp.status();
    let url = resp.url().clone();
    let body = resp.text().await?;

    if !status.is_success() {
        let snippet: String = body.lines().take(8).collect::<Vec<_>>().join("\n");
        error!("[fetch] fetch_page failed: {} {}\n{}", status, url, snippet);
    } else {
        debug!(
            "[fetch] fetch_page response: {} {} body_len={}",
            status,
            url,
            body.len()
        );
    }

    let raw: Vec<HexRaw> =
        serde_json::from_str(&body).context("parsing hex.pm package list response")?;
    Ok(raw.into_iter().map(into_package).collect())
}

/// Sort a package list client-side by the HEX.pm sort param string.
fn sort_packages(packages: &mut [Package], sort: &str) {
    match sort {
        "recent_downloads" => packages.sort_by(|a, b| b.downloads_recent.cmp(&a.downloads_recent)),
        "downloads" => packages.sort_by(|a, b| b.downloads_all.cmp(&a.downloads_all)),
        "updated_at" => packages.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)),
        "inserted_at" => packages.sort_by(|a, b| b.inserted_at.cmp(&a.inserted_at)),
        "name" => packages.sort_by(|a, b| a.name.cmp(&b.name)),
        _ => {}
    }
}

/// Remove duplicate packages by name, preserving the first occurrence.
fn dedup_packages(packages: &mut Vec<Package>) {
    let mut seen = std::collections::HashSet::new();
    packages.retain(|p| seen.insert(p.name.clone()));
}

/// Fetch packages from HEX.pm for the given `language`, optional `query`, and `page`.
///
/// Returns `(packages, has_more)` where `has_more` = a next page is available.
///
/// ## Strategy per mode
///
/// - **All BEAM**: fetch page N of Gleam + Elixir + Erlang in parallel, merge and sort
///   client-side. Gives correct language badges. `has_more` = any ecosystem returned 100.
/// - **Language, no query**: single page N; `has_more` = page was full (100 items).
/// - **Language, with query**: pages 1-5 in parallel (~500 pkgs), filter client-side;
///   `has_more` = false (pagination is irrelevant in search mode).
pub async fn fetch_packages(
    query: &str,
    sort: &str,
    language: Language,
    page: u32,
) -> Result<(Vec<Package>, bool)> {
    let q = query.trim();
    info!(
        "[fetch] fetch_packages language={} query={q} sort={sort} page={page}",
        language
    );

    if language == Language::All {
        info!("[fetch] All BEAM mode: fetching gleam/mix/rebar3 buckets");
        let (r_gleam, r_elixir, r_erlang) = tokio::join!(
            fetch_page("build_tool:gleam", sort, page),
            fetch_page("build_tool:mix", sort, page),
            fetch_page("build_tool:rebar3", sort, page),
        );
        let mut packages: Vec<Package> = vec![];
        let mut has_more = false;
        for (result, lang) in [
            (r_gleam, Language::Gleam),
            (r_elixir, Language::Elixir),
            (r_erlang, Language::Erlang),
        ] {
            if let Ok(mut pkgs) = result {
                if pkgs.len() >= 100 {
                    has_more = true;
                }
                for pkg in &mut pkgs {
                    pkg.language = lang;
                }
                packages.extend(pkgs);
            }
        }
        // Dedup (cross-language packages can appear in multiple buckets — Gleam wins).
        dedup_packages(&mut packages);
        if !q.is_empty() {
            let q_lower = q.to_lowercase();
            packages.retain(|p| {
                p.name.to_lowercase().contains(&q_lower)
                    || p.description.to_lowercase().contains(&q_lower)
            });
        }
        sort_packages(&mut packages, sort);
        return Ok((packages, has_more));
    }

    // ── Language-specific mode ────────────────────────────────────────────────
    let api_search = language.hex_filter().unwrap().to_string();

    let (mut packages, has_more) = if !q.is_empty() {
        // Multi-page parallel fetch for full text-search coverage (~500 packages).
        // User-facing pagination is irrelevant in search mode; always fetch from page 1.
        let (r1, r2, r3, r4, r5) = tokio::join!(
            fetch_page(&api_search, sort, 1),
            fetch_page(&api_search, sort, 2),
            fetch_page(&api_search, sort, 3),
            fetch_page(&api_search, sort, 4),
            fetch_page(&api_search, sort, 5),
        );
        let mut all = vec![];
        for pkgs in [r1, r2, r3, r4, r5].into_iter().flatten() {
            all.extend(pkgs);
        }
        (all, false)
    } else {
        let pkgs = fetch_page(&api_search, sort, page).await?;
        let full = pkgs.len() >= 100;
        (pkgs, full)
    };

    // API filter is authoritative for language; override any inferred value.
    for pkg in &mut packages {
        pkg.language = language;
    }
    dedup_packages(&mut packages);

    if !q.is_empty() {
        let q_lower = q.to_lowercase();
        packages.retain(|p| {
            p.name.to_lowercase().contains(&q_lower)
                || p.description.to_lowercase().contains(&q_lower)
        });

        // Fallback: if even the 500-package search returns nothing, try an exact name lookup.
        // Handles packages outside the top-500 (e.g. very new or niche packages).
        // NOTE: only include the package if we can positively confirm it belongs to the
        // selected language. Packages with unknown build_tools (Language::All) are excluded
        // to avoid showing e.g. Elixir packages labelled as Gleam.
        if packages.is_empty() {
            info!("[fetch] search fallback: no results from full-text search, trying exact package lookup for {q}");
            if let Ok(pkg) = fetch_package(q).await {
                if pkg.language == language {
                    info!("[fetch] fallback exact package found: {q} lang={language}");
                    packages.push(pkg);
                } else {
                    info!(
                        "[fetch] fallback exact package {q} has language={}, expected {language} — skipping",
                        pkg.language
                    );
                }
            }
        }
    }

    Ok((packages, has_more))
}

/// Fetch a single package by exact name from HEX.pm.
pub async fn fetch_package(name: &str) -> Result<Package> {
    let url = format!("https://hex.pm/api/packages/{name}");
    info!("[fetch] fetch_package url={url}");

    let resp = client()?.get(&url).send().await?;
    let status = resp.status();
    let url = resp.url().clone();
    let body = resp.text().await?;

    if !status.is_success() {
        let snippet: String = body.lines().take(8).collect::<Vec<_>>().join("\n");
        error!(
            "[fetch] fetch_package failed: {} {}\n{}",
            status, url, snippet
        );
    } else {
        debug!(
            "[fetch] fetch_package response: {} {} body_len={}",
            status,
            url,
            body.len()
        );
    }

    let raw: HexRaw = serde_json::from_str(&body).context("parsing hex.pm package response")?;
    Ok(into_package(raw))
}

/// Fetch multiple packages by exact name in parallel (used for the favorites view).
pub async fn fetch_by_names(names: Vec<String>) -> Vec<Package> {
    let mut set = tokio::task::JoinSet::new();
    for name in names {
        set.spawn(async move { fetch_package(&name).await });
    }
    let mut packages = vec![];
    while let Some(result) = set.join_next().await {
        if let Ok(Ok(pkg)) = result {
            packages.push(pkg);
        }
    }
    // Sort alphabetically for a consistent display order.
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Fetch GitHub repository stats.
/// Pass a bearer token to raise the rate limit from 60 to 5 000 req/h.
pub async fn fetch_github_stats(repo_url: &str, token: Option<&str>) -> Result<GhResult> {
    if !repo_url.contains("github.com") {
        return Ok(GhResult::Unavailable);
    }

    let path = repo_url
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("github.com/");

    let parts: Vec<&str> = path.splitn(3, '/').collect();
    if parts.len() < 2 {
        return Ok(GhResult::Unavailable);
    }

    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return Ok(GhResult::Unavailable);
    }

    #[derive(Deserialize)]
    struct Gh {
        stargazers_count: u32,
        forks_count: u32,
        open_issues_count: u32,
    }

    let url = format!("https://api.github.com/repos/{owner}/{repo}");
    info!(
        "[github] fetch_github_stats url={} token_present={}",
        url,
        token.is_some()
    );

    let mut req = client()?
        .get(url.clone())
        .header("Accept", "application/vnd.github+json");

    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }

    let resp = req.send().await?;
    let status = resp.status();
    let body = resp.text().await?;

    if !status.is_success() {
        let snippet: String = body.lines().take(8).collect::<Vec<_>>().join("\n");
        error!(
            "[github] fetch_github_stats failed: {} {}\n{}",
            status, url, snippet
        );
    } else {
        debug!(
            "[github] fetch_github_stats response: {} {} body_len={}",
            status,
            url,
            body.len()
        );
    }

    match status.as_u16() {
        401 => return Ok(GhResult::BadToken),
        403 | 429 => return Ok(GhResult::RateLimited),
        s if s >= 400 => return Ok(GhResult::Unavailable),
        _ => {}
    }

    let gh: Gh = serde_json::from_str(&body).context("parsing GitHub repo response")?;
    Ok(GhResult::Ok(GithubStats {
        stars: gh.stargazers_count,
        forks: gh.forks_count,
        issues: gh.open_issues_count,
    }))
}

/// Fetch the ExDoc search index for a HexDocs package.
///
/// Tries candidates in a cascade so both Gleam and Elixir packages are covered:
///
/// **Step 1 — direct URL candidates** (tried in order, first non-empty result wins):
///   `search_data.json` · `search-data.json` · `search_data.js` · `search-data.js`
///   `dist/search_data.js` · `dist/search-data.js`
///
/// **Step 2 — HTML discovery** (if all direct candidates fail):
///   Fetch `search.html`, scan `<script src="...">` for known asset paths,
///   fetch the discovered asset, or parse inline `searchData = {...}` from the page.
pub async fn fetch_docs_search_data(package: &str) -> Result<Vec<SearchItem>> {
    let c = client()?;
    let base = format!("https://hexdocs.pm/{package}/");
    info!("[docs] fetch_docs_search_data package={package}");

    // ── Step 1: direct URL candidates ────────────────────────────────────────
    const DIRECT: &[&str] = &[
        "search_data.json",
        "search-data.json",
        "search_data.js",
        "search-data.js",
        "dist/search_data.js",
        "dist/search-data.js",
    ];

    for candidate in DIRECT {
        let url = format!("{base}{candidate}");
        let resp = match c.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                debug!("[docs] {url} request error: {e}");
                continue;
            }
        };
        if !resp.status().is_success() {
            debug!("[docs] {url} → {}", resp.status());
            continue;
        }
        debug!("[docs] {url} → 200, parsing");
        let body = resp.text().await.unwrap_or_default();
        match parse_search_data(&body) {
            Ok(data) if !data.items.is_empty() => {
                info!("[docs] resolved via direct candidate {url} items={}", data.items.len());
                return Ok(map_search_items(data));
            }
            Ok(_) => debug!("[docs] {url} parsed but empty, continuing"),
            Err(e) => debug!("[docs] {url} parse failed ({e}), continuing"),
        }
    }

    // ── Step 2: HTML discovery ────────────────────────────────────────────────
    let search_html_url = format!("{base}search.html");
    info!("[docs] direct candidates exhausted, trying HTML discovery at {search_html_url}");

    let page_resp = c.get(&search_html_url).send().await;
    let page_body = match page_resp {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        Ok(r) => {
            debug!("[docs] search.html → {}", r.status());
            String::new()
        }
        Err(e) => {
            debug!("[docs] search.html request error: {e}");
            String::new()
        }
    };

    if !page_body.is_empty() {
        // 2a: linked asset discovered from <script src="...">
        if let Some(asset_url) = find_search_index_url(&search_html_url, &page_body) {
            info!("[docs] HTML discovery found asset {asset_url}");
            match c.get(&asset_url).send().await {
                Ok(r) if r.status().is_success() => {
                    let body = r.text().await.unwrap_or_default();
                    match parse_search_data(&body) {
                        Ok(data) if !data.items.is_empty() => {
                            info!("[docs] resolved via HTML asset {asset_url} items={}", data.items.len());
                            return Ok(map_search_items(data));
                        }
                        Ok(_) => debug!("[docs] HTML asset empty"),
                        Err(e) => debug!("[docs] HTML asset parse failed: {e}"),
                    }
                }
                Ok(r) => debug!("[docs] HTML asset → {}", r.status()),
                Err(e) => debug!("[docs] HTML asset request error: {e}"),
            }
        }

        // 2b: inline searchData embedded directly in the HTML page
        if page_body.contains("searchData") {
            match parse_search_data(&page_body) {
                Ok(data) if !data.items.is_empty() => {
                    info!("[docs] resolved via inline searchData items={}", data.items.len());
                    return Ok(map_search_items(data));
                }
                Ok(_) => debug!("[docs] inline searchData empty"),
                Err(e) => debug!("[docs] inline searchData parse failed: {e}"),
            }
        }
    }

    error!("[docs] all strategies failed for package={package}");
    Err(anyhow::anyhow!(
        "docs search index not found for '{package}' — tried direct candidates and HTML discovery"
    ))
}

fn map_search_items(data: SearchData) -> Vec<SearchItem> {
    data.items
        .into_iter()
        .map(|r| SearchItem {
            item_type: r.item_type,
            title: r.title,
            parent_title: r.parent_title,
            doc_text: strip_html(&r.doc),
            ref_url: r.ref_url,
        })
        .collect()
}

/// Parse an ExDoc search payload from either:
/// - **Pure JSON**: `{"items":[...]}`  (`.json` files)
/// - **JS assignment**: `searchData = {"items":[...]}` or `var searchData={...};`  (`.js` files / inline)
fn parse_search_data(body: &str) -> Result<SearchData> {
    let body = body.trim();

    // Case 1: pure JSON — starts with `{` directly.
    // Do NOT use find('=') here: `=` appears inside string values (URLs, Gleam
    // `key=value` patterns) and would slice the JSON at the wrong position.
    if body.starts_with('{') {
        return serde_json::from_str(body).context("parsing docs search data as JSON");
    }

    // Case 2: JS assignment — `searchData = { ... };`
    // Only search for `=` *after* the `searchData` keyword, not in the whole body.
    if let Some(kw_pos) = body.find("searchData") {
        let after_kw = body[kw_pos + "searchData".len()..].trim_start();
        if let Some(eq_pos) = after_kw.find('=') {
            let json_part = after_kw[eq_pos + 1..].trim_start();
            if json_part.starts_with('{') {
                let json_text = extract_json_object(json_part);
                return serde_json::from_str(json_text)
                    .context("parsing docs search data from JS assignment");
            }
        }
    }

    Err(anyhow::anyhow!("no parseable search data found in response"))
}

fn extract_json_object(s: &str) -> &str {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;
    for (idx, c) in s.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &s[..=idx];
                    }
                }
                '"' => in_string = true,
                _ => {}
            }
        }
    }
    s.trim_end_matches(';').trim()
}

/// Scan `html` for `<script src="...">` attributes that look like ExDoc search assets.
/// Returns the first matching absolute URL resolved against `base_url`.
fn find_search_index_url(base_url: &str, html: &str) -> Option<String> {
    let base = Url::parse(base_url).ok()?;
    // Candidates covering both JSON and JS variants across ExDoc versions.
    let candidates = [
        "dist/search_data-",
        "dist/search-data-",
        "search_data.json",
        "search-data.json",
        "search_data.js",
        "search-data.js",
        "dist/search_data.js",
        "dist/search-data.js",
        "search.json",
        "search-index.json",
    ];

    // Pass 1: walk every `src="..."` attribute and match against candidates.
    let mut start = 0;
    while let Some(src_pos) = html[start..].find("src=") {
        let src_pos = start + src_pos + "src=".len();
        let trimmed = html[src_pos..].trim_start();
        let quote = match trimmed.chars().next() {
            Some(q) if q == '"' || q == '\'' => q,
            _ => {
                start = src_pos;
                continue;
            }
        };
        let trimmed = &trimmed[1..];
        if let Some(end_quote) = trimmed.find(quote) {
            let path = &trimmed[..end_quote];
            if candidates.iter().any(|c| path.contains(c)) {
                if let Ok(url) = base.join(path) {
                    return Some(url.into());
                }
            }
            start = src_pos + 1 + end_quote;
        } else {
            break;
        }
    }

    // Pass 2: search for candidate strings anywhere in the HTML and extract
    // the surrounding quoted path.
    for candidate in candidates {
        let mut start = 0;
        while let Some(pos) = html[start..].find(candidate) {
            let pos = start + pos;
            if let Some(path) = extract_quoted_path(html, pos) {
                if let Ok(url) = base.join(&path) {
                    return Some(url.into());
                }
            }
            start = pos + candidate.len();
        }
    }
    None
}

fn extract_quoted_path(html: &str, pos: usize) -> Option<String> {
    let before = &html[..pos];
    let quote_pos = before.rfind(['"', '\''])?;
    let quote = html.chars().nth(quote_pos)?;
    let suffix = &html[pos..];
    let end = suffix.find(quote)?;
    Some(html[quote_pos + 1..pos + end].to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_search_data ─────────────────────────────────────────────────────

    fn item(type_: &str, title: &str, ref_: &str) -> String {
        format!(r#"{{"type":"{type_}","title":"{title}","parentTitle":"","doc":"","ref":"{ref_}"}}"#)
    }

    fn wrap(items: &str) -> String {
        format!(r#"{{"items":[{items}]}}"#)
    }

    #[test]
    fn parse_pure_json() {
        let json = wrap(&item("function", "add/2", "Math.html#add/2"));
        let data = parse_search_data(&json).unwrap();
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].title, "add/2");
    }

    #[test]
    fn parse_pure_json_with_equals_in_values() {
        // Regression: `=` inside string values must not confuse the parser.
        let json = wrap(&item("function", "query_param=value", "Mod.html#f/1"));
        let data = parse_search_data(&json).unwrap();
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].title, "query_param=value");
    }

    #[test]
    fn parse_js_assignment() {
        let body = format!(r#"var searchData={};"#, wrap(&item("module", "MyMod", "MyMod.html")));
        let data = parse_search_data(&body).unwrap();
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].title, "MyMod");
    }

    #[test]
    fn parse_js_assignment_with_spaces() {
        let body = format!(r#"searchData = {};"#, wrap(&item("type", "T", "T.html")));
        let data = parse_search_data(&body).unwrap();
        assert_eq!(data.items.len(), 1);
    }

    // ── find_search_index_url ─────────────────────────────────────────────────

    #[test]
    fn find_url_from_script_src_json() {
        // src path relative to search.html in the same directory
        let html = r#"<script src="search_data.json"></script>"#;
        let url = find_search_index_url("https://hexdocs.pm/gleam_stdlib/search.html", html);
        assert_eq!(
            url.as_deref(),
            Some("https://hexdocs.pm/gleam_stdlib/search_data.json")
        );
    }

    #[test]
    fn find_url_from_script_src_js() {
        let html = r#"<script defer="defer" src="dist/search_data-abc123.js"></script>"#;
        let url = find_search_index_url("https://hexdocs.pm/lustre/search.html", html);
        assert!(url.is_some());
        assert!(url.unwrap().contains("dist/search_data-abc123.js"));
    }

    #[test]
    fn find_url_prefers_src_attribute() {
        // src= pass runs before the substring pass, so the <script> wins.
        let html = r#"<link href="search-data.json"><script src="search-data.js"></script>"#;
        let url = find_search_index_url("https://hexdocs.pm/pkg/search.html", html);
        assert_eq!(
            url.as_deref(),
            Some("https://hexdocs.pm/pkg/search-data.js")
        );
    }

    #[test]
    fn find_url_returns_none_for_unrelated_html() {
        let html = r#"<html><body><p>No search here.</p></body></html>"#;
        let url = find_search_index_url("https://hexdocs.pm/pkg/search.html", html);
        assert!(url.is_none());
    }
}
