//! Application state and event handling.

use log::{debug, error, info, warn};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use tokio::sync::mpsc::Sender;

use std::collections::HashMap;

use crate::{
    api::{
        fetch_by_names, fetch_docs_search_data, fetch_github_stats, fetch_package, fetch_packages,
        github_token, GhResult, Package, SearchItem,
    },
    cache::{self, CacheMap, CachedEntry},
    favorites,
    storage::{self, StorageConfig},
    types::{ColorScheme, Language, LinkStyle, SettingRow, Sort, View},
};

// ── Type aliases ─────────────────────────────────────────────────────────────

type PkgCacheKey = (String, String, Language, u32);
type PkgCacheValue = (Vec<Package>, bool);
type PkgCache = HashMap<PkgCacheKey, PkgCacheValue>;

// ── GitHub fetch state ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum GhState {
    /// Package has no repository URL.
    NoRepo,
    /// Data served from disk cache. The entry is kept by value so age_label()
    /// can be computed dynamically at render time (avoids stale age strings).
    Cached(CachedEntry),
    /// Network request in flight.
    Loading,
    /// Live data just fetched.
    Live(crate::api::GithubStats),
    /// Hit the unauthenticated 60 req/h rate limit.
    RateLimited,
    /// 401 Unauthorized — the stored/env token is invalid or expired.
    BadToken,
    /// Non-GitHub repo or any other error.
    Unavailable,
}

// ── Async messages ────────────────────────────────────────────────────────────

pub enum Msg {
    /// `u64` is the fetch generation — stale results (gen < app.fetch_gen) are discarded.
    /// `bool` = has_more (a next page exists).
    Loaded(u64, Vec<Package>, bool),
    GhFetched(String, GhResult), // (repo_url, result)
    /// Single-package detail fetch result: (package_name, versions_newest_first).
    DetailLoaded(String, Vec<String>),
    /// HexDocs search index fetch result: (query_term, filtered_results).
    DocsSearchLoaded(String, Vec<SearchItem>),
    Err(String),
}

// ── Application state ─────────────────────────────────────────────────────────

pub struct App {
    pub view: View,
    pub language: Language,
    pub packages: Vec<Package>,
    pub list_state: ListState,
    pub input: String,
    pub input_mode: bool,
    pub sort: Sort,
    pub loading: bool,
    pub gh: GhState,
    pub error: Option<String>,
    pub scroll: u16,
    /// Current browse page (1-indexed). Reset to 1 on search, language, or sort change.
    /// Inactive (always 1) when a search query is active.
    pub page: u32,
    /// True when the last fetch returned a full page — a next page likely exists.
    pub has_more: bool,
    /// In-memory mirror of `~/.cache/hexplorer/gh_stats.json`.
    pub cache: CacheMap,
    /// `GITHUB_TOKEN` from environment (`None` = unauthenticated).
    pub token: Option<String>,
    /// Starred packages: name → language. Persisted to `~/.cache/hexplorer/favorites.json`.
    pub favorites: HashMap<String, crate::types::Language>,
    /// When `true`, the list shows only starred packages (fetched individually by name).
    pub favorites_mode: bool,
    /// Active color scheme — loaded from config at startup, changed live via settings.
    pub color_scheme: ColorScheme,
    /// Active link highlight style — loaded from config at startup, changed live via settings.
    pub link_style: LinkStyle,
    // ── Settings screen state ─────────────────────────────────────────────────
    /// Index into `SettingRow::all()` for the settings cursor.
    pub settings_cursor: usize,
    /// `true` while the user is typing a new GitHub token value.
    pub settings_editing: bool,
    /// Buffer for the token being typed.
    pub settings_input: String,
    /// Local copy of `StorageConfig` while the settings screen is open.
    pub settings_config: StorageConfig,
    /// Current stored token (loaded when opening settings, for masked display).
    pub settings_token: Option<String>,
    /// Index of the currently highlighted link in the detail view (tab-navigated).
    /// Indexes into the ordered list [docs_url, hex_url, repo_url] filtered to Some values.
    pub link_cursor: Option<usize>,
    /// In-memory session cache for listing responses.
    /// Key: (query, sort_param, language, page). Cleared on manual refresh.
    pkg_cache: PkgCache,
    /// True when the last listing was served from `pkg_cache` instead of the network.
    pub from_cache: bool,
    /// True while a single-package detail fetch is in flight.
    pub detail_loading: bool,
    /// True while the user is typing a HexDocs search query.
    pub docs_search_mode: bool,
    /// Buffer for the HexDocs search query being typed.
    pub docs_search_input: String,
    /// True while the search_data.json fetch is in flight.
    pub docs_search_loading: bool,
    /// Results from the last HexDocs search, filtered locally.
    pub docs_search_results: Vec<SearchItem>,
    /// Cursor index into `docs_search_results`.
    pub docs_search_cursor: usize,
    /// Package name whose docs are being searched (for building open URLs).
    pub docs_search_pkg: String,
    /// View to return to when closing DocsSearch (List or Detail).
    prev_view: View,
    /// Active docs cache TTL in hours — mirrors settings_config, loaded at startup.
    pub docs_cache_ttl_hours: u32,
    tx: Sender<Msg>,
    /// Monotonically-increasing counter; each `load()` call increments it.
    /// `Msg::Loaded` carries the generation it was spawned with — results from
    /// earlier generations are silently discarded to prevent stale overwrites.
    fetch_gen: u64,
}

impl App {
    pub fn new(tx: Sender<Msg>, language: Language) -> Self {
        Self {
            view: View::List,
            language,
            packages: vec![],
            list_state: ListState::default(),
            input: String::new(),
            input_mode: false,
            sort: Sort::default(),
            loading: false,
            gh: GhState::NoRepo,
            error: None,
            scroll: 0,
            page: 1,
            has_more: false,
            cache: cache::load(),
            token: github_token(),
            favorites: favorites::load(),
            favorites_mode: false,
            color_scheme: storage::load_meta()
                .as_ref()
                .map(|m| m.config.color_scheme)
                .unwrap_or_default(),
            link_style: storage::load_meta()
                .map(|m| m.config.link_style)
                .unwrap_or_default(),
            settings_cursor: 0,
            settings_editing: false,
            settings_input: String::new(),
            settings_config: StorageConfig::default(),
            settings_token: None,
            link_cursor: None,
            pkg_cache: std::collections::HashMap::new(),
            from_cache: false,
            detail_loading: false,
            docs_search_mode: false,
            docs_search_input: String::new(),
            docs_search_loading: false,
            docs_search_results: vec![],
            docs_search_cursor: 0,
            docs_search_pkg: String::new(),
            prev_view: View::List,
            docs_cache_ttl_hours: storage::load_meta()
                .map(|m| m.config.docs_cache_ttl_hours)
                .unwrap_or(24),
            tx,
            fetch_gen: 0,
        }
    }

    // ── Data fetching ─────────────────────────────────────────────────────────

    pub fn load(&mut self) {
        let q = self.input.trim().to_string();
        let s = self.sort.api_param().to_string();
        let lng = self.language;
        let pg = self.page;
        let key = (q.clone(), s.clone(), lng, pg);

        // Serve from session cache when available.
        if let Some((pkgs, more)) = self.pkg_cache.get(&key) {
            self.packages = pkgs.clone();
            self.has_more = *more;
            self.from_cache = true;
            self.loading = false;
            self.error = None;
            if !self.packages.is_empty() {
                self.list_state.select(Some(0));
            }
            return;
        }

        self.fetch_gen += 1;
        let gen = self.fetch_gen;
        self.loading = true;
        self.from_cache = false;
        self.error = None;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match fetch_packages(&q, &s, lng, pg).await {
                Ok((pkgs, more)) => {
                    let _ = tx.send(Msg::Loaded(gen, pkgs, more)).await;
                }
                Err(e) => {
                    let _ = tx.send(Msg::Err(e.to_string())).await;
                }
            }
        });
    }

    /// Trigger a single-package detail fetch if `versions` is not yet populated.
    pub fn ensure_pkg_detail(&mut self) {
        let Some(pkg) = self.selected() else { return };
        if !pkg.versions.is_empty() {
            return; // already have version history
        }
        let name = pkg.name.clone();
        self.detail_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Ok(full) = fetch_package(&name).await {
                let _ = tx.send(Msg::DetailLoaded(name, full.versions)).await;
            } else {
                // Silently swallow errors — detail view degrades gracefully.
                let _ = tx.send(Msg::DetailLoaded(String::new(), vec![])).await;
            }
        });
    }

    /// Trigger GitHub stats fetch for the selected package (lazy, called on Enter).
    /// Serves from cache when fresh; shows stale data while re-fetching.
    pub fn ensure_gh_stats(&mut self) {
        let Some(pkg) = self.selected() else {
            self.gh = GhState::NoRepo;
            return;
        };
        let Some(repo_url) = pkg.repo_url.clone() else {
            self.gh = GhState::NoRepo;
            return;
        };

        if let Some(entry) = cache::get_fresh(&self.cache, &repo_url) {
            self.gh = GhState::Cached(entry.clone());
            return;
        }

        // Show stale data while re-fetching.
        if let Some(entry) = cache::get_any(&self.cache, &repo_url) {
            self.gh = GhState::Cached(entry.clone());
        } else {
            self.gh = GhState::Loading;
        }

        let tx = self.tx.clone();
        let token = self.token.clone();
        let url = repo_url.clone();
        tokio::spawn(async move {
            let result = fetch_github_stats(&url, token.as_deref())
                .await
                .unwrap_or(GhResult::Unavailable);
            let _ = tx.send(Msg::GhFetched(url, result)).await;
        });
    }

    // ── Message handler ───────────────────────────────────────────────────────

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Loaded(gen, pkgs, more) => {
                // Discard results from superseded fetches (e.g. rapid sort/language changes).
                if gen != self.fetch_gen {
                    debug!(
                        "[msg] Loaded gen={gen} discarded (current={})",
                        self.fetch_gen
                    );
                    return;
                }
                info!(
                    "[msg] Loaded gen={gen} packages={} has_more={more} lang={} query={:?} page={}",
                    pkgs.len(),
                    self.language,
                    self.input.trim(),
                    self.page,
                );
                self.loading = false;
                self.from_cache = false;
                self.has_more = more;
                self.packages = pkgs.clone();
                if !self.packages.is_empty() {
                    self.list_state.select(Some(0));
                }
                // Populate session cache so back-navigation is instant.
                let key = (
                    self.input.trim().to_string(),
                    self.sort.api_param().to_string(),
                    self.language,
                    self.page,
                );
                self.pkg_cache.insert(key, (pkgs, more));
            }
            Msg::DetailLoaded(name, versions) => {
                info!(
                    "[msg] DetailLoaded pkg={name:?} versions={}",
                    versions.len()
                );
                self.detail_loading = false;
                // Patch the matching package in-place so the detail view updates live.
                if let Some(pkg) = self.packages.iter_mut().find(|p| p.name == name) {
                    pkg.versions = versions;
                }
            }
            Msg::DocsSearchLoaded(term, results) => {
                info!(
                    "[msg] DocsSearchLoaded query={term:?} results={}",
                    results.len()
                );
                self.docs_search_loading = false;
                self.docs_search_results = results;
                self.docs_search_cursor = 0;
            }
            Msg::GhFetched(repo_url, result) => {
                let result_label = match &result {
                    GhResult::Ok(_) => "ok",
                    GhResult::RateLimited => "rate_limited",
                    GhResult::BadToken => "bad_token",
                    GhResult::Unavailable => "unavailable",
                };
                info!("[msg] GhFetched repo={repo_url:?} result={result_label}");
                if matches!(result, GhResult::BadToken) {
                    warn!("[msg] GhFetched bad_token — stored/env token is invalid or expired");
                }
                match result {
                    GhResult::Ok(stats) => {
                        cache::insert(&mut self.cache, repo_url, &stats);
                        self.gh = GhState::Live(stats);
                    }
                    GhResult::RateLimited => self.gh = GhState::RateLimited,
                    GhResult::BadToken => self.gh = GhState::BadToken,
                    GhResult::Unavailable => self.gh = GhState::Unavailable,
                }
            }
            Msg::Err(e) => {
                error!("[msg] Err: {e}");
                self.loading = false;
                self.error = Some(e);
            }
        }
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    pub fn selected(&self) -> Option<&Package> {
        self.list_state
            .selected()
            .and_then(|i| self.packages.get(i))
    }

    /// Cached stats for the highlighted package (list preview — no live fetch).
    pub fn preview_gh(&self) -> Option<&CachedEntry> {
        let repo = self.selected()?.repo_url.as_deref()?;
        cache::get_any(&self.cache, repo)
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    pub fn open_settings(&mut self) {
        self.view = View::Settings;
        self.settings_cursor = 0;
        self.settings_editing = false;
        self.settings_input.clear();
        self.settings_config = storage::load_meta().map(|m| m.config).unwrap_or_default();
        self.settings_token = storage::load_github_token();
    }

    fn key_settings(&mut self, key: KeyEvent) -> bool {
        let rows = SettingRow::all();

        if self.settings_editing {
            match key.code {
                KeyCode::Esc => {
                    self.settings_editing = false;
                    self.settings_input.clear();
                }
                KeyCode::Enter => {
                    let t = self.settings_input.trim().to_string();
                    let _ = storage::save_github_token(if t.is_empty() {
                        None
                    } else {
                        Some(t.as_str())
                    });
                    self.settings_token = storage::load_github_token();
                    self.token = github_token(); // refresh live token
                    self.settings_editing = false;
                    self.settings_input.clear();
                }
                KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    delete_word_back(&mut self.settings_input);
                }
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    delete_word_back(&mut self.settings_input);
                }
                KeyCode::Backspace => {
                    self.settings_input.pop();
                }
                KeyCode::Char(c) => {
                    if self.settings_input.len() < 256 {
                        self.settings_input.push(c);
                    }
                }
                _ => {}
            }
            return false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.view = View::List;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,

            KeyCode::Down | KeyCode::Char('j') => {
                self.settings_cursor = (self.settings_cursor + 1).min(rows.len() - 1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings_cursor = self.settings_cursor.saturating_sub(1);
            }

            KeyCode::Enter => match rows[self.settings_cursor] {
                SettingRow::GithubToken => {
                    // Pre-fill with current token so the user can edit it.
                    self.settings_input = self.settings_token.clone().unwrap_or_default();
                    self.settings_editing = true;
                }
                SettingRow::Compress => {
                    self.settings_config.compress = !self.settings_config.compress;
                    self.persist_settings_config();
                }
                SettingRow::ClearGhCache => {
                    self.cache.clear();
                    if let Ok(dir) = storage::cache_dir() {
                        let _ = std::fs::remove_file(dir.join("gh_stats.json"));
                    }
                    cache::clear_docs();
                }
                SettingRow::KeepWeeks => {}
                SettingRow::ColorScheme => {}
                SettingRow::LinkStyle => {}
                SettingRow::DefaultLanguage => {}
                SettingRow::DocsCacheTtl => {}
            },

            // `d` on the token row clears it.
            KeyCode::Char('d')
                if rows.get(self.settings_cursor) == Some(&SettingRow::GithubToken) =>
            {
                let _ = storage::save_github_token(None);
                self.settings_token = None;
                self.token = None;
            }

            // ← / → cycle keep_weeks through preset values.
            KeyCode::Left | KeyCode::Right
                if rows.get(self.settings_cursor) == Some(&SettingRow::KeepWeeks) =>
            {
                const PRESETS: &[u32] = &[1, 2, 4, 8, 12, 24, 52];
                let pos = PRESETS
                    .iter()
                    .position(|&w| w == self.settings_config.keep_weeks);
                let cur = pos.unwrap_or(4); // default to 12-week slot
                let next = if key.code == KeyCode::Left {
                    cur.saturating_sub(1)
                } else {
                    (cur + 1).min(PRESETS.len() - 1)
                };
                self.settings_config.keep_weeks = PRESETS[next];
                self.persist_settings_config();
            }

            // ← / → cycle link style.
            KeyCode::Left | KeyCode::Right
                if rows.get(self.settings_cursor) == Some(&SettingRow::LinkStyle) =>
            {
                self.settings_config.link_style = if key.code == KeyCode::Left {
                    self.settings_config.link_style.cycle_back()
                } else {
                    self.settings_config.link_style.cycle()
                };
                self.link_style = self.settings_config.link_style;
                self.persist_settings_config();
            }

            // ← / → cycle color scheme.
            KeyCode::Left | KeyCode::Right
                if rows.get(self.settings_cursor) == Some(&SettingRow::ColorScheme) =>
            {
                self.settings_config.color_scheme = if key.code == KeyCode::Left {
                    self.settings_config.color_scheme.cycle_back()
                } else {
                    self.settings_config.color_scheme.cycle()
                };
                self.color_scheme = self.settings_config.color_scheme;
                self.persist_settings_config();
            }

            // ← / → cycle default language.
            KeyCode::Left | KeyCode::Right
                if rows.get(self.settings_cursor) == Some(&SettingRow::DefaultLanguage) =>
            {
                self.settings_config.default_language = if key.code == KeyCode::Left {
                    self.settings_config.default_language.cycle_back()
                } else {
                    self.settings_config.default_language.cycle()
                };
                self.persist_settings_config();
            }

            // ← / → cycle docs cache TTL through presets.
            KeyCode::Left | KeyCode::Right
                if rows.get(self.settings_cursor) == Some(&SettingRow::DocsCacheTtl) =>
            {
                const PRESETS: &[u32] = &[0, 1, 6, 12, 24, 48, 168];
                let pos = PRESETS
                    .iter()
                    .position(|&h| h == self.settings_config.docs_cache_ttl_hours);
                let cur = pos.unwrap_or(4); // default to 24h slot
                let next = if key.code == KeyCode::Left {
                    cur.saturating_sub(1)
                } else {
                    (cur + 1).min(PRESETS.len() - 1)
                };
                self.settings_config.docs_cache_ttl_hours = PRESETS[next];
                self.docs_cache_ttl_hours = PRESETS[next];
                self.persist_settings_config();
            }

            _ => {}
        }
        false
    }

    fn persist_settings_config(&mut self) {
        if let Ok(mut meta) = storage::load_meta() {
            meta.config = self.settings_config.clone();
            let _ = storage::save_meta(&meta);
        }
    }

    // ── Language switching ────────────────────────────────────────────────────

    // ── Favorites ─────────────────────────────────────────────────────────────

    /// Toggle the star on the currently selected package and persist to disk.
    pub fn toggle_star(&mut self) {
        let Some(pkg) = self.selected() else { return };
        let name = pkg.name.clone();
        let lang = pkg.language;
        favorites::toggle(&mut self.favorites, &name, lang);
        favorites::save(&self.favorites);
        // In favorites mode: remove the package from view immediately if unstarred.
        if self.favorites_mode && !self.favorites.contains_key(&name) {
            self.packages.retain(|p| p.name != name);
            if self.packages.is_empty() {
                self.favorites_mode = false;
                self.load();
            } else {
                // Keep selection in bounds.
                let max = self.packages.len().saturating_sub(1);
                if let Some(i) = self.list_state.selected() {
                    self.list_state.select(Some(i.min(max)));
                }
            }
        }
    }

    /// Enter or exit the favorites view.
    pub fn toggle_favorites_mode(&mut self) {
        if self.favorites_mode {
            self.favorites_mode = false;
            self.reset_nav();
            self.load();
        } else {
            if self.favorites.is_empty() {
                return;
            } // nothing starred yet
            self.favorites_mode = true;
            self.list_state = ListState::default();
            self.scroll = 0;
            self.view = View::List;
            self.load_favorites();
        }
    }

    fn load_favorites(&mut self) {
        self.loading = true;
        self.error = None;
        self.fetch_gen += 1;
        let gen = self.fetch_gen;
        let tx = self.tx.clone();
        let favs = self.favorites.clone();
        tokio::spawn(async move {
            let names = favs.keys().cloned().collect();
            let mut pkgs = fetch_by_names(names).await;
            // Override language from saved favorites — avoids [?] when the
            // listing API omits build_tools for fetched-by-name packages.
            for pkg in &mut pkgs {
                if let Some(&lang) = favs.get(&pkg.name) {
                    pkg.language = lang;
                }
            }
            let _ = tx.send(Msg::Loaded(gen, pkgs, false)).await;
        });
    }

    pub fn switch_language(&mut self) {
        self.language = self.language.cycle();
        self.favorites_mode = false;
        self.reset_nav();
        self.load();
    }

    pub fn switch_language_back(&mut self) {
        self.language = self.language.cycle_back();
        self.favorites_mode = false;
        self.reset_nav();
        self.load();
    }

    fn reset_nav(&mut self) {
        self.view = View::List;
        self.list_state = ListState::default();
        self.scroll = 0;
        self.gh = GhState::NoRepo;
        self.page = 1;
        self.has_more = false;
    }

    pub fn next_page(&mut self) {
        if self.has_more && self.input.trim().is_empty() {
            self.page += 1;
            self.list_state = ListState::default();
            self.scroll = 0;
            self.load();
        }
    }

    pub fn prev_page(&mut self) {
        if self.page > 1 && self.input.trim().is_empty() {
            self.page -= 1;
            self.list_state = ListState::default();
            self.scroll = 0;
            self.load();
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    fn nav(&mut self, delta: i32) {
        if self.packages.is_empty() {
            return;
        }
        let n = self.packages.len() as i32;
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let nxt = (cur + delta).clamp(0, n - 1) as usize;
        if nxt != cur as usize {
            self.list_state.select(Some(nxt));
        }
    }

    // ── HexDocs search ────────────────────────────────────────────────────────

    fn open_docs_search(&mut self) {
        let term = self.docs_search_input.trim().to_string();
        if term.is_empty() {
            return;
        }
        let Some(pkg_name) = self.selected().map(|p| p.name.clone()) else {
            return;
        };
        self.docs_search_pkg = pkg_name.clone();
        self.prev_view = self.view;
        self.view = View::DocsSearch;
        self.docs_search_results.clear();
        self.docs_search_cursor = 0;

        // Serve from disk cache if available and TTL not expired.
        let ttl = self.docs_cache_ttl_hours;
        if let Some(cached_items) = cache::get_docs(&pkg_name, ttl) {
            let q = term.to_lowercase();
            self.docs_search_results = cached_items
                .into_iter()
                .filter(|item| {
                    item.title.to_lowercase().contains(&q)
                        || item.doc_text.to_lowercase().contains(&q)
                })
                .take(50)
                .collect();
            self.docs_search_loading = false;
            return;
        }

        self.docs_search_loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match fetch_docs_search_data(&pkg_name).await {
                Ok(items) => {
                    cache::insert_docs(&pkg_name, &items, ttl);
                    let q = term.to_lowercase();
                    let results = items
                        .into_iter()
                        .filter(|item| {
                            item.title.to_lowercase().contains(&q)
                                || item.doc_text.to_lowercase().contains(&q)
                        })
                        .take(50)
                        .collect();
                    let _ = tx.send(Msg::DocsSearchLoaded(term, results)).await;
                }
                Err(_) => {
                    let _ = tx.send(Msg::DocsSearchLoaded(term, vec![])).await;
                }
            }
        });
    }

    fn key_docs_search_view(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.view = self.prev_view;
                self.docs_search_results.clear();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.docs_search_results.is_empty() {
                    self.docs_search_cursor =
                        (self.docs_search_cursor + 1).min(self.docs_search_results.len() - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.docs_search_cursor = self.docs_search_cursor.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(item) = self.docs_search_results.get(self.docs_search_cursor) {
                    let url = format!(
                        "https://hexdocs.pm/{}/{}",
                        self.docs_search_pkg, item.ref_url
                    );
                    let _ = open::that(url);
                }
            }
            _ => {}
        }
        false
    }

    fn key_docs_search(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.docs_search_mode = false;
                self.docs_search_input.clear();
            }
            KeyCode::Enter => {
                self.open_docs_search();
                self.docs_search_mode = false;
                self.docs_search_input.clear();
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_word_back(&mut self.docs_search_input);
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_word_back(&mut self.docs_search_input);
            }
            KeyCode::Backspace => {
                self.docs_search_input.pop();
            }
            KeyCode::Char(c) => {
                if self.docs_search_input.len() < 200 {
                    self.docs_search_input.push(c);
                }
            }
            _ => {}
        }
        false
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Returns `true` when the app should quit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        debug!(
            "[key] code={:?} modifiers={:?} view={:?} input_mode={} docs_search_mode={}",
            key.code, key.modifiers, self.view, self.input_mode, self.docs_search_mode,
        );
        let prev_view = self.view;
        if self.docs_search_mode {
            return self.key_docs_search(key);
        }
        if self.input_mode {
            return self.key_input(key);
        }
        let quit = match self.view {
            View::List => self.key_list(key),
            View::Detail => self.key_detail(key),
            View::Settings => self.key_settings(key),
            View::DocsSearch => self.key_docs_search_view(key),
        };
        if self.view != prev_view {
            info!("[view] {:?} → {:?}", prev_view, self.view);
        }
        quit
    }

    fn key_input(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = false;
            }
            KeyCode::Enter => {
                self.input_mode = false;
                self.page = 1;
                self.load();
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_word_back(&mut self.input);
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_word_back(&mut self.input);
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                if self.input.len() < 200 {
                    self.input.push(c);
                }
            }
            _ => {}
        }
        false
    }

    fn key_list(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('D') if !self.packages.is_empty() => {
                self.docs_search_mode = true;
                self.docs_search_input.clear();
            }

            KeyCode::Char('/') => {
                self.input_mode = true;
            }
            KeyCode::Char('?') => {
                self.open_settings();
            }
            KeyCode::Tab => {
                self.sort = self.sort.cycle();
                self.page = 1;
                self.load();
            }
            KeyCode::Char(']') => self.next_page(),
            KeyCode::Char('[') => self.prev_page(),

            KeyCode::Char('l') => self.switch_language(),
            KeyCode::Char('L') => self.switch_language_back(),
            KeyCode::Char('s') => self.toggle_star(),
            KeyCode::Char('f') => self.toggle_favorites_mode(),

            KeyCode::Down | KeyCode::Char('j') => self.nav(1),
            KeyCode::Up | KeyCode::Char('k') => self.nav(-1),
            KeyCode::PageDown => self.nav(10),
            KeyCode::PageUp => self.nav(-10),

            KeyCode::Enter => {
                if !self.packages.is_empty() {
                    self.view = View::Detail;
                    self.scroll = 0;
                    self.link_cursor = None;
                    self.ensure_gh_stats();
                    self.ensure_pkg_detail();
                }
            }
            KeyCode::Char('r') => {
                if !self.loading {
                    self.pkg_cache.clear();
                    self.from_cache = false;
                    if self.favorites_mode {
                        self.load_favorites();
                    } else {
                        self.load();
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn key_detail(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Backspace => {
                self.view = View::List;
                self.link_cursor = None;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
            }
            KeyCode::Tab => {
                let count = self
                    .selected()
                    .map(|pkg| {
                        [&pkg.docs_url, &pkg.hex_url, &pkg.repo_url]
                            .iter()
                            .filter(|u| u.is_some())
                            .count()
                    })
                    .unwrap_or(0);
                if count > 0 {
                    self.link_cursor = Some(match self.link_cursor {
                        None => 0,
                        Some(i) => (i + 1) % count,
                    });
                }
            }
            KeyCode::BackTab => {
                let count = self
                    .selected()
                    .map(|pkg| {
                        [&pkg.docs_url, &pkg.hex_url, &pkg.repo_url]
                            .iter()
                            .filter(|u| u.is_some())
                            .count()
                    })
                    .unwrap_or(0);
                if count > 0 {
                    self.link_cursor = Some(match self.link_cursor {
                        None | Some(0) => count - 1,
                        Some(i) => i - 1,
                    });
                }
            }
            KeyCode::Enter => {
                if let Some(idx) = self.link_cursor {
                    let url = self.selected().and_then(|pkg| {
                        [&pkg.docs_url, &pkg.hex_url, &pkg.repo_url]
                            .iter()
                            .filter_map(|u| u.as_deref())
                            .nth(idx)
                            .map(str::to_string)
                    });
                    if let Some(url) = url {
                        let _ = open::that(url);
                    }
                }
            }
            KeyCode::Char('s') => {
                self.docs_search_mode = true;
                self.docs_search_input.clear();
            }
            // `r` — force-refresh GH stats + detail for the current package.
            KeyCode::Char('r') => {
                if let Some(repo_url) = self.selected().and_then(|p| p.repo_url.clone()) {
                    self.cache.remove(&repo_url);
                }
                if let Some(idx) = self.list_state.selected() {
                    if let Some(pkg) = self.packages.get_mut(idx) {
                        pkg.versions.clear();
                    }
                }
                self.gh = GhState::Loading;
                self.ensure_gh_stats();
                self.ensure_pkg_detail();
            }
            _ => {}
        }
        false
    }
}

// ── Text editing helpers ───────────────────────────────────────────────────────

/// Remove the last word from `s` — strips trailing whitespace then non-whitespace.
/// This is the standard Ctrl+W / Ctrl+Backspace behaviour.
fn delete_word_back(s: &mut String) {
    while s.ends_with(char::is_whitespace) {
        s.pop();
    }
    while s.ends_with(|c: char| !c.is_whitespace()) {
        s.pop();
    }
}
