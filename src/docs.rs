//! HexDocs search types, fetch logic, and HTML utilities.
//!
//! This module is the single home for everything docs-related:
//! types (`SearchItem`, `SearchSource`), remote fetch + parse cascade,
//! and the HTML stripping / entity-decoding helpers. The cache integration
//! lives in `cache.rs` but uses the types defined here.

use anyhow::{Context, Result};
use log::{debug, error, info};
use reqwest::Url;
use serde::Deserialize;

use crate::api::client;

// ── Public types ──────────────────────────────────────────────────────────────

/// Where a `SearchItem` was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SearchSource {
    /// Fetched from hexdocs.pm.
    Remote,
    /// Ingested from a local project's build artefacts.
    Local,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchItem {
    /// Origin of this entry — remote (hexdocs.pm) or local (ingested).
    pub source: SearchSource,
    /// Package this entry belongs to, e.g. "gleam_stdlib".
    pub package: String,
    /// "value", "module", "page", "callback", "type"
    pub item_type: String,
    pub title: String,
    pub parent_title: String,
    /// Plain text doc snippet (HTML stripped).
    pub doc_text: String,
    /// Relative URL within the package docs, e.g. "gleeunit.html#main".
    pub ref_url: String,
}

// ── Raw deserialization types (private) ───────────────────────────────────────

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

// ── HTML helpers ──────────────────────────────────────────────────────────────

/// Strip HTML tags and decode common HTML entities.
pub(crate) fn strip_html(s: &str) -> String {
    let tag_stripped = strip_html_tags(s);
    decode_html_entities(&tag_stripped)
}

fn strip_html_tags(s: &str) -> String {
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

/// Decode named and numeric HTML entities. Covers the common set found in ExDoc output.
fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        if let Some(semi) = rest.find(';') {
            let entity = &rest[..=semi]; // "&...;"
            let decoded = decode_entity(entity);
            out.push_str(decoded.as_deref().unwrap_or(entity));
            rest = &rest[semi + 1..];
        } else {
            // No closing `;` — output the `&` literally and advance.
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<String> {
    match entity {
        "&amp;" => Some("&".into()),
        "&lt;" => Some("<".into()),
        "&gt;" => Some(">".into()),
        "&quot;" => Some("\"".into()),
        "&apos;" | "&#39;" => Some("'".into()),
        "&nbsp;" => Some(" ".into()),
        "&mdash;" | "&#8212;" | "&#x2014;" => Some("—".into()),
        "&ndash;" | "&#8211;" | "&#x2013;" => Some("–".into()),
        "&hellip;" | "&#8230;" | "&#x2026;" => Some("…".into()),
        _ if entity.starts_with("&#x") || entity.starts_with("&#X") => {
            let hex = entity
                .trim_start_matches("&#x")
                .trim_start_matches("&#X")
                .trim_end_matches(';');
            u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
        }
        _ if entity.starts_with("&#") => {
            let dec = entity.trim_start_matches("&#").trim_end_matches(';');
            dec.parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
        }
        _ => None,
    }
}

// ── Remote fetch ──────────────────────────────────────────────────────────────

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
                info!(
                    "[docs] resolved via direct candidate {url} items={}",
                    data.items.len()
                );
                return Ok(map_search_items(data, package));
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
                            info!(
                                "[docs] resolved via HTML asset {asset_url} items={}",
                                data.items.len()
                            );
                            return Ok(map_search_items(data, package));
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
                    info!(
                        "[docs] resolved via inline searchData items={}",
                        data.items.len()
                    );
                    return Ok(map_search_items(data, package));
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

// ── Private helpers ───────────────────────────────────────────────────────────

fn map_search_items(data: SearchData, package: &str) -> Vec<SearchItem> {
    data.items
        .into_iter()
        .map(|r| SearchItem {
            source: SearchSource::Remote,
            package: package.to_string(),
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

    Err(anyhow::anyhow!(
        "no parseable search data found in response"
    ))
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

    fn item(type_: &str, title: &str, ref_: &str) -> String {
        format!(
            r#"{{"type":"{type_}","title":"{title}","parentTitle":"","doc":"","ref":"{ref_}"}}"#
        )
    }

    fn wrap(items: &str) -> String {
        format!(r#"{{"items":[{items}]}}"#)
    }

    // ── parse_search_data ─────────────────────────────────────────────────────

    #[test]
    fn parse_pure_json() {
        let json = wrap(&item("function", "add/2", "Math.html#add/2"));
        let data = parse_search_data(&json).unwrap();
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].title, "add/2");
    }

    #[test]
    fn parse_pure_json_with_equals_in_values() {
        let json = wrap(&item("function", "query_param=value", "Mod.html#f/1"));
        let data = parse_search_data(&json).unwrap();
        assert_eq!(data.items.len(), 1);
        assert_eq!(data.items[0].title, "query_param=value");
    }

    #[test]
    fn parse_js_assignment() {
        let body = format!(
            r#"var searchData={};"#,
            wrap(&item("module", "MyMod", "MyMod.html"))
        );
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

    // ── SearchItem source / package fields ────────────────────────────────────

    #[test]
    fn map_search_items_sets_source_and_package() {
        let json = wrap(&item("function", "add/2", "Math.html#add/2"));
        let data = parse_search_data(&json).unwrap();
        let items = map_search_items(data, "my_pkg");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source, SearchSource::Remote);
        assert_eq!(items[0].package, "my_pkg");
        assert_eq!(items[0].title, "add/2");
    }

    #[test]
    fn search_item_serialises_with_source_and_package() {
        let si = SearchItem {
            source: SearchSource::Remote,
            package: "gleam_stdlib".into(),
            item_type: "module".into(),
            title: "List".into(),
            parent_title: "".into(),
            doc_text: "List utilities.".into(),
            ref_url: "list.html".into(),
        };
        let json = serde_json::to_string(&si).unwrap();
        assert!(json.contains("\"source\":\"Remote\""));
        assert!(json.contains("\"package\":\"gleam_stdlib\""));
        let roundtrip: SearchItem = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.source, SearchSource::Remote);
        assert_eq!(roundtrip.package, "gleam_stdlib");
    }

    // ── strip_html / decode_html_entities ─────────────────────────────────────

    #[test]
    fn strip_html_removes_tags() {
        assert_eq!(strip_html("<p>hello</p>"), "hello");
        assert_eq!(strip_html("<em>foo</em> bar"), "foo bar");
        assert_eq!(strip_html("no tags here"), "no tags here");
    }

    #[test]
    fn strip_html_decodes_named_entities() {
        assert_eq!(strip_html("a &amp; b"), "a & b");
        assert_eq!(strip_html("a &lt; b &gt; c"), "a < b > c");
        assert_eq!(strip_html("say &quot;hi&quot;"), "say \"hi\"");
        assert_eq!(strip_html("it&apos;s"), "it's");
        assert_eq!(strip_html("foo&nbsp;bar"), "foo bar");
    }

    #[test]
    fn strip_html_decodes_dash_entities() {
        assert_eq!(strip_html("a&mdash;b"), "a—b");
        assert_eq!(strip_html("a&ndash;b"), "a–b");
        assert_eq!(strip_html("wait&hellip;"), "wait…");
    }

    #[test]
    fn strip_html_decodes_decimal_numeric_entities() {
        assert_eq!(strip_html("&#65;"), "A");
        assert_eq!(strip_html("&#8212;"), "—");
        assert_eq!(strip_html("&#39;"), "'");
    }

    #[test]
    fn strip_html_decodes_hex_numeric_entities() {
        assert_eq!(strip_html("&#x41;"), "A");
        assert_eq!(strip_html("&#x2014;"), "—");
        assert_eq!(strip_html("&#X2026;"), "…");
    }

    #[test]
    fn strip_html_handles_amp_without_semicolon() {
        assert_eq!(strip_html("foo & bar"), "foo & bar");
    }

    #[test]
    fn strip_html_combined_tags_and_entities() {
        let input = "<p>foo &amp; <em>bar</em> &lt;baz&gt;</p>";
        assert_eq!(strip_html(input), "foo & bar <baz>");
    }
}
