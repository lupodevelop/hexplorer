//! Shared domain types used across all modules.
//! This module is the single source of truth for enums that cross module boundaries.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

// ── Language ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    Gleam,
    Elixir,
    Erlang,
    /// No language filter — returns top BEAM packages across all ecosystems.
    All,
}

impl Language {
    /// Query fragment to prepend to HEX.pm search.
    /// Uses `build_tool:X` — the only filter the v1 API actually honours.
    /// (`language:X` is silently ignored and returns the unfiltered top-100.)
    /// `None` = no filter (All BEAM mode).
    pub fn hex_filter(self) -> Option<&'static str> {
        match self {
            Language::Gleam => Some("build_tool:gleam"),
            Language::Elixir => Some("build_tool:mix"),
            Language::Erlang => Some("build_tool:rebar3"),
            Language::All => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::Gleam => "Gleam",
            Language::Elixir => "Elixir",
            Language::Erlang => "Erlang",
            Language::All => "All BEAM",
        }
    }

    /// Three-character badge used in `All BEAM` list mode.
    pub fn badge(self) -> &'static str {
        match self {
            Language::Gleam => "glm",
            Language::Elixir => "ex ",
            Language::Erlang => "erl",
            Language::All => " ? ",
        }
    }

    /// Per-ecosystem accent color matching official branding.
    pub fn accent(self) -> Color {
        match self {
            Language::Gleam => Color::Rgb(255, 121, 198), // Gleam pink
            Language::Elixir => Color::Rgb(100, 67, 217), // Elixir violet
            Language::Erlang => Color::Rgb(163, 62, 40),  // Erlang red-brown
            Language::All => Color::Rgb(97, 218, 251),    // Neutral BEAM cyan
        }
    }

    /// Cycle forward: Gleam → Elixir → Erlang → All → Gleam.
    pub fn cycle(self) -> Self {
        match self {
            Language::Gleam => Language::Elixir,
            Language::Elixir => Language::Erlang,
            Language::Erlang => Language::All,
            Language::All => Language::Gleam,
        }
    }

    /// Cycle backward: Gleam → All → Erlang → Elixir → Gleam.
    pub fn cycle_back(self) -> Self {
        match self {
            Language::Gleam => Language::All,
            Language::All => Language::Erlang,
            Language::Erlang => Language::Elixir,
            Language::Elixir => Language::Gleam,
        }
    }

    /// All variants in display order (used for tab bar rendering).
    pub fn all() -> &'static [Language] {
        &[
            Language::Gleam,
            Language::Elixir,
            Language::Erlang,
            Language::All,
        ]
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Language::Gleam => "gleam",
            Language::Elixir => "elixir",
            Language::Erlang => "erlang",
            Language::All => "all",
        })
    }
}

impl std::str::FromStr for Language {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "gleam" => Ok(Language::Gleam),
            "elixir" => Ok(Language::Elixir),
            "erlang" => Ok(Language::Erlang),
            "all" => Ok(Language::All),
            other => Err(format!(
                "unknown language: '{other}' (valid: gleam, elixir, erlang, all)"
            )),
        }
    }
}

/// Infer the language of a package from its `build_tools` array in HEX.pm metadata.
/// Returns `Language::All` (= unknown) when no known build tool is found.
///
/// NOTE: only use this in `Language::All` mode. In filtered modes (Gleam/Elixir/Erlang)
/// trust the API filter result — see `fetch_packages` in api.rs.
///
/// Priority: Gleam > Erlang > Elixir. Gleam is checked first because packages that
/// support both Gleam and Elixir list `["mix", "gleam"]`, and they are Gleam packages.
pub fn infer_language(build_tools: &[String]) -> Language {
    // First pass: look for gleam (highest priority).
    for tool in build_tools {
        if tool == "gleam" {
            return Language::Gleam;
        }
    }
    // Second pass: rebar3/erlang tooling.
    for tool in build_tools {
        match tool.as_str() {
            "rebar3" | "erlang.mk" | "erlang" => return Language::Erlang,
            _ => {}
        }
    }
    // Third pass: mix → Elixir.
    for tool in build_tools {
        if tool == "mix" {
            return Language::Elixir;
        }
    }
    Language::All
}

// ── Sort ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Sort {
    #[default]
    RecentDownloads,
    Downloads,
    Updated,
    Newest,
    Name,
}

impl Sort {
    pub fn api_param(self) -> &'static str {
        match self {
            Sort::RecentDownloads => "recent_downloads",
            Sort::Downloads => "downloads",
            Sort::Updated => "updated_at",
            Sort::Newest => "inserted_at",
            Sort::Name => "name",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::RecentDownloads => "Recent dls",
            Sort::Downloads => "All dls",
            Sort::Updated => "Last updated",
            Sort::Newest => "Newest first",
            Sort::Name => "Name A→Z",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Sort::RecentDownloads => Sort::Downloads,
            Sort::Downloads => Sort::Updated,
            Sort::Updated => Sort::Newest,
            Sort::Newest => Sort::Name,
            Sort::Name => Sort::RecentDownloads,
        }
    }
}

impl std::str::FromStr for Sort {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "recent_downloads" | "recent" => Ok(Sort::RecentDownloads),
            "downloads" | "all" => Ok(Sort::Downloads),
            "updated_at" | "updated" => Ok(Sort::Updated),
            "inserted_at" | "newest" => Ok(Sort::Newest),
            "name" => Ok(Sort::Name),
            other => Err(format!("unknown sort: '{other}'")),
        }
    }
}

// ── View ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    List,
    Detail,
    Settings,
}

// ── SettingRow ────────────────────────────────────────────────────────────────

/// The navigable rows in the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    GithubToken,
    KeepWeeks,
    Compress,
    ClearGhCache,
    ColorScheme,
}

impl SettingRow {
    pub fn all() -> &'static [Self] {
        &[
            Self::GithubToken,
            Self::ColorScheme,
            Self::KeepWeeks,
            Self::Compress,
            Self::ClearGhCache,
        ]
    }
}

// ── ColorScheme & Palette ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorScheme {
    #[default]
    Default,
    Dracula,
    Nord,
    Gruvbox,
}

impl ColorScheme {
    pub fn label(self) -> &'static str {
        match self {
            ColorScheme::Default => "Default",
            ColorScheme::Dracula => "Dracula",
            ColorScheme::Nord => "Nord",
            ColorScheme::Gruvbox => "Gruvbox",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            ColorScheme::Default => ColorScheme::Dracula,
            ColorScheme::Dracula => ColorScheme::Nord,
            ColorScheme::Nord => ColorScheme::Gruvbox,
            ColorScheme::Gruvbox => ColorScheme::Default,
        }
    }

    pub fn cycle_back(self) -> Self {
        match self {
            ColorScheme::Default => ColorScheme::Gruvbox,
            ColorScheme::Gruvbox => ColorScheme::Nord,
            ColorScheme::Nord => ColorScheme::Dracula,
            ColorScheme::Dracula => ColorScheme::Default,
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            ColorScheme::Default => Palette {
                yellow: Color::Rgb(255, 212, 59),
                green: Color::Rgb(80, 250, 123),
                dim: Color::Rgb(90, 88, 110),
                white: Color::White,
                bg_bar: Color::Rgb(16, 10, 26),
                bg_sel: Color::Rgb(38, 14, 52),
            },
            // Dracula: high-contrast bright colors on near-black background
            ColorScheme::Dracula => Palette {
                yellow: Color::Rgb(241, 250, 140), // #f1fa8c
                green: Color::Rgb(80, 250, 123),   // #50fa7b
                dim: Color::Rgb(98, 114, 164),     // #6272a4 comment
                white: Color::Rgb(248, 248, 242),  // #f8f8f2 foreground
                bg_bar: Color::Rgb(25, 26, 33),    // #191a21 darker bg
                bg_sel: Color::Rgb(68, 71, 90),    // #44475a current line
            },
            ColorScheme::Nord => Palette {
                yellow: Color::Rgb(235, 203, 139),
                green: Color::Rgb(163, 190, 140),
                dim: Color::Rgb(76, 86, 106),
                white: Color::Rgb(236, 239, 244),
                bg_bar: Color::Rgb(29, 33, 44),
                bg_sel: Color::Rgb(46, 52, 64),
            },
            ColorScheme::Gruvbox => Palette {
                yellow: Color::Rgb(250, 189, 47),
                green: Color::Rgb(184, 187, 38),
                dim: Color::Rgb(146, 131, 116),
                white: Color::Rgb(235, 219, 178),
                bg_bar: Color::Rgb(29, 32, 33),
                bg_sel: Color::Rgb(50, 48, 47),
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub yellow: Color,
    pub green: Color,
    pub dim: Color,
    pub white: Color,
    pub bg_bar: Color,
    pub bg_sel: Color,
}

// ── OutputFormat ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    /// Full JSON snapshot — suitable for `jq` and LLM context.
    Json,
    /// Compact Markdown table — suitable for piping to `llm`.
    Compact,
    /// Detailed Markdown block for a single named package.
    Detail,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(OutputFormat::Json),
            "compact" => Ok(OutputFormat::Compact),
            "detail" => Ok(OutputFormat::Detail),
            other => Err(format!(
                "unknown output format: '{other}' (valid: json, compact, detail)"
            )),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_gleam() {
        assert_eq!(infer_language(&["gleam".into()]), Language::Gleam);
    }
    #[test]
    fn infer_elixir() {
        assert_eq!(infer_language(&["mix".into()]), Language::Elixir);
    }
    #[test]
    fn infer_erlang_rebar3() {
        assert_eq!(infer_language(&["rebar3".into()]), Language::Erlang);
    }
    #[test]
    fn infer_erlang_mk() {
        assert_eq!(infer_language(&["erlang.mk".into()]), Language::Erlang);
    }
    #[test]
    fn infer_unknown() {
        assert_eq!(infer_language(&["cargo".into()]), Language::All);
    }
    #[test]
    fn infer_empty() {
        assert_eq!(infer_language(&[]), Language::All);
    }
    #[test]
    fn infer_gleam_beats_mix() {
        // Packages that support both Gleam and Elixir list ["mix", "gleam"].
        // Gleam must win regardless of order.
        assert_eq!(
            infer_language(&["mix".into(), "gleam".into()]),
            Language::Gleam
        );
        assert_eq!(
            infer_language(&["gleam".into(), "mix".into()]),
            Language::Gleam
        );
    }
    #[test]
    fn infer_gleam_beats_erlang() {
        assert_eq!(
            infer_language(&["rebar3".into(), "gleam".into()]),
            Language::Gleam
        );
    }

    #[test]
    fn cycle_forward() {
        assert_eq!(Language::Gleam.cycle(), Language::Elixir);
        assert_eq!(Language::Elixir.cycle(), Language::Erlang);
        assert_eq!(Language::Erlang.cycle(), Language::All);
        assert_eq!(Language::All.cycle(), Language::Gleam);
    }

    #[test]
    fn cycle_back_is_inverse() {
        for &lang in Language::all() {
            assert_eq!(lang.cycle().cycle_back(), lang);
        }
    }

    #[test]
    fn language_display_roundtrip() {
        for &lang in Language::all() {
            assert_eq!(lang.to_string().parse::<Language>().unwrap(), lang);
        }
    }

    #[test]
    fn language_from_str_case_insensitive() {
        assert_eq!("Gleam".parse::<Language>().unwrap(), Language::Gleam);
        assert_eq!("ELIXIR".parse::<Language>().unwrap(), Language::Elixir);
        assert!("unknown".parse::<Language>().is_err());
    }

    #[test]
    fn sort_cycle_wraps() {
        assert_eq!(Sort::Name.cycle(), Sort::RecentDownloads);
    }
}
