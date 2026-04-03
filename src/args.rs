//! CLI argument parsing — intentionally minimal, no external crate.

use crate::types::{Language, OutputFormat, Sort};

// ── Storage subcommand ────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum StorageSubcmd {
    Status,
    Prune {
        yes: bool,
    },
    Clear,
    /// `None` = show current config; `Some("key=value")` = update field.
    Config(Option<String>),
}

// ── Top-level args ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct Args {
    pub output: Option<OutputFormat>,
    pub language: Language,
    /// `true` when `--lang` was explicitly passed; `false` means fall back to config default.
    pub lang_explicit: bool,
    pub search: Option<String>,
    pub sort: Sort,
    /// Package name for `--output detail <name>`.
    pub package: Option<String>,
    /// Present when the first positional arg is `storage`.
    pub storage_cmd: Option<StorageSubcmd>,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse `std::env::args()` into `Args`.
/// Unknown flags are silently ignored for forward-compatibility.
pub fn parse_args() -> anyhow::Result<Args> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    parse_from(&raw)
}

/// Inner function that accepts a slice — allows unit testing without spawning a process.
pub fn parse_from(raw: &[String]) -> anyhow::Result<Args> {
    let mut args = Args::default();

    // ── `hexplorer storage <subcommand>` ─────────────────────────────────────
    if raw.first().map(String::as_str) == Some("storage") {
        args.storage_cmd = Some(parse_storage(raw)?);
        return Ok(args);
    }

    let mut i = 0usize;
    while i < raw.len() {
        match raw[i].as_str() {
            "--output" | "-o" => {
                i += 1;
                let fmt = raw.get(i).ok_or_else(|| {
                    anyhow::anyhow!("--output requires a value (json|compact|detail)")
                })?;
                let fmt: OutputFormat = fmt.parse().map_err(|e: String| anyhow::anyhow!(e))?;

                // `detail` may be followed by a positional package name.
                if fmt == OutputFormat::Detail {
                    if let Some(next) = raw.get(i + 1) {
                        if !next.starts_with('-') {
                            i += 1;
                            args.package = Some(next.clone());
                        }
                    }
                }
                args.output = Some(fmt);
            }
            "--lang" | "-l" => {
                i += 1;
                let lang = raw.get(i).ok_or_else(|| {
                    anyhow::anyhow!("--lang requires a value (gleam|elixir|erlang|all)")
                })?;
                args.language = lang.parse().map_err(|e: String| anyhow::anyhow!(e))?;
                args.lang_explicit = true;
            }
            "--search" | "-s" => {
                i += 1;
                if let Some(q) = raw.get(i) {
                    args.search = Some(q.clone());
                }
            }
            "--sort" => {
                i += 1;
                let s = raw
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--sort requires a value"))?;
                args.sort = s.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            }
            _ => {} // Forward-compatible: ignore unknown flags.
        }
        i += 1;
    }

    Ok(args)
}

fn parse_storage(raw: &[String]) -> anyhow::Result<StorageSubcmd> {
    match raw.get(1).map(String::as_str) {
        Some("status") => Ok(StorageSubcmd::Status),
        Some("prune")  => Ok(StorageSubcmd::Prune { yes: raw.contains(&"--yes".to_string()) }),
        Some("clear")  => Ok(StorageSubcmd::Clear),
        Some("config") => Ok(StorageSubcmd::Config(raw.get(2).cloned())),
        other => anyhow::bail!(
            "unknown storage subcommand: {:?}\nUsage: hexplorer storage <status|prune|clear|config [key=value]>",
            other
        ),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults() {
        let args = parse_from(&[]).unwrap();
        assert_eq!(args.language, Language::Gleam);
        assert_eq!(args.sort, Sort::RecentDownloads);
        assert!(args.output.is_none());
    }

    #[test]
    fn output_json() {
        let args = parse_from(&s(&["--output", "json"])).unwrap();
        assert_eq!(args.output, Some(OutputFormat::Json));
    }

    #[test]
    fn output_compact_with_lang() {
        let args = parse_from(&s(&["--output", "compact", "--lang", "elixir"])).unwrap();
        assert_eq!(args.output, Some(OutputFormat::Compact));
        assert_eq!(args.language, Language::Elixir);
    }

    #[test]
    fn output_detail_with_package() {
        let args = parse_from(&s(&["--output", "detail", "lustre"])).unwrap();
        assert_eq!(args.output, Some(OutputFormat::Detail));
        assert_eq!(args.package, Some("lustre".to_string()));
    }

    #[test]
    fn output_detail_no_package() {
        // --output detail without a package name is valid (will use first result).
        let args = parse_from(&s(&["--output", "detail"])).unwrap();
        assert_eq!(args.output, Some(OutputFormat::Detail));
        assert_eq!(args.package, None);
    }

    #[test]
    fn full_flags() {
        let args = parse_from(&s(&[
            "--output", "json", "--lang", "elixir", "--search", "http", "--sort", "name",
        ]))
        .unwrap();
        assert_eq!(args.output, Some(OutputFormat::Json));
        assert_eq!(args.language, Language::Elixir);
        assert_eq!(args.search, Some("http".to_string()));
        assert_eq!(args.sort, Sort::Name);
    }

    #[test]
    fn storage_status() {
        let args = parse_from(&s(&["storage", "status"])).unwrap();
        assert!(matches!(args.storage_cmd, Some(StorageSubcmd::Status)));
    }

    #[test]
    fn storage_prune_yes() {
        let args = parse_from(&s(&["storage", "prune", "--yes"])).unwrap();
        assert!(matches!(
            args.storage_cmd,
            Some(StorageSubcmd::Prune { yes: true })
        ));
    }

    #[test]
    fn storage_config_with_value() {
        let args = parse_from(&s(&["storage", "config", "keep_weeks=4"])).unwrap();
        assert!(matches!(
            args.storage_cmd,
            Some(StorageSubcmd::Config(Some(ref v))) if v == "keep_weeks=4"
        ));
    }

    #[test]
    fn unknown_flag_ignored() {
        // Forward-compatibility: unknown flags must not cause an error.
        let args = parse_from(&s(&["--future-flag", "value", "--lang", "gleam"])).unwrap();
        assert_eq!(args.language, Language::Gleam);
    }

    #[test]
    fn invalid_lang_errors() {
        assert!(parse_from(&s(&["--lang", "cobol"])).is_err());
    }

    #[test]
    fn invalid_output_errors() {
        assert!(parse_from(&s(&["--output", "xml"])).is_err());
    }
}
