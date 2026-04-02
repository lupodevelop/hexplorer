//! Non-TUI output modes: JSON, compact Markdown, and single-package detail.
//! Activated by `--output json|compact|detail`.

use anyhow::Result;

use crate::{
    api::{self, GhResult},
    args::Args,
    export_types::{PackageExport, PackageGithubInput, Snapshot},
    fmt,
    types::OutputFormat,
};

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn run(args: &Args) -> Result<()> {
    match args.output.as_ref().expect("output mode must be set") {
        OutputFormat::Json => run_json(args).await,
        OutputFormat::Compact => run_compact(args).await,
        OutputFormat::Detail => run_detail(args).await,
    }
}

// ── JSON mode ─────────────────────────────────────────────────────────────────

async fn run_json(args: &Args) -> Result<()> {
    let (packages, _) = api::fetch_packages(
        args.search.as_deref().unwrap_or(""),
        args.sort.api_param(),
        args.language,
        1,
    )
    .await?;

    let snapshot = Snapshot::build(
        &packages,
        &args.language.to_string(),
        args.search.as_deref().unwrap_or(""),
        args.sort.api_param(),
    );

    print_json(&snapshot)
}

// ── Compact Markdown mode ─────────────────────────────────────────────────────

async fn run_compact(args: &Args) -> Result<()> {
    let (packages, _) = api::fetch_packages(
        args.search.as_deref().unwrap_or(""),
        args.sort.api_param(),
        args.language,
        1,
    )
    .await?;

    let snapshot = Snapshot::build(
        &packages,
        &args.language.to_string(),
        args.search.as_deref().unwrap_or(""),
        args.sort.api_param(),
    );

    print_compact(&snapshot);
    Ok(())
}

// ── Detail mode ───────────────────────────────────────────────────────────────

async fn run_detail(args: &Args) -> Result<()> {
    let pkg = match &args.package {
        Some(name) => api::fetch_package(name).await?,
        None => {
            // No name given: fetch the top result with current filters.
            let (pkgs, _) = api::fetch_packages(
                args.search.as_deref().unwrap_or(""),
                args.sort.api_param(),
                args.language,
                1,
            )
            .await?;
            pkgs.into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no packages found"))?
        }
    };

    // Optionally fetch GitHub stats for this one package.
    let github = match &pkg.repo_url {
        Some(url) if url.contains("github.com") => {
            let token = api::github_token();
            match api::fetch_github_stats(url, token.as_deref()).await? {
                GhResult::Ok(stats) => Some(PackageGithubInput {
                    stats,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                    source: "live".into(),
                }),
                _ => None,
            }
        }
        _ => None,
    };

    let export = PackageExport::from_package(&pkg, github);
    print_detail(&export);
    Ok(())
}

// ── Formatters ────────────────────────────────────────────────────────────────

pub fn print_json(snapshot: &Snapshot) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(snapshot)?);
    Ok(())
}

pub fn print_compact(snapshot: &Snapshot) {
    let lang = &snapshot.query.language;
    let search = if snapshot.query.search.is_empty() {
        "(all)".to_string()
    } else {
        snapshot.query.search.clone()
    };
    let date = &snapshot.meta.fetched_at[..10];
    let sort = &snapshot.query.sort;

    println!("## {lang} packages — {search} (sort: {sort}) — {date}\n");
    println!(
        "| {:<20} | {:>7} | {:>5} | {:>9} | {:>8} | {:>5} | {:<10} |",
        "package", "version", "lang", "dl_recent", "dl_total", "stars", "updated"
    );
    println!(
        "|{:-<22}|{:-<9}|{:-<7}|{:-<11}|{:-<10}|{:-<7}|{:-<12}|",
        "", "", "", "", "", "", ""
    );

    for p in &snapshot.packages {
        let stars = p
            .github
            .as_ref()
            .map(|g| fmt::dl_full(g.stars as u64))
            .unwrap_or_else(|| "-".into());
        println!(
            "| {:<20} | {:>7} | {:>5} | {:>9} | {:>8} | {:>5} | {:<10} |",
            p.id,
            p.release.latest,
            &p.language[..p.language.len().min(5)],
            fmt::dl_full(p.downloads.recent_90d),
            fmt::dl_short(p.downloads.all_time),
            stars,
            fmt::date(&p.release.updated_at),
        );
    }
}

pub fn print_detail(p: &PackageExport) {
    println!("### {} — v{}\n", p.id, p.release.latest);

    if !p.description.is_empty() {
        println!("**Description:** {}\n", p.description);
    }

    println!("- **Language:** {}", p.language);
    println!(
        "- **Downloads:** {} total · {} recent (90d)",
        fmt::dl_full(p.downloads.all_time),
        fmt::dl_full(p.downloads.recent_90d),
    );

    if let Some(gh) = &p.github {
        println!(
            "- **GitHub:** ★ {} · ⑂ {} · ⊙ {} open issues",
            fmt::dl_full(gh.stars as u64),
            fmt::dl_full(gh.forks as u64),
            gh.open_issues,
        );
    }

    println!(
        "- **Updated:** {} · Published: {}",
        fmt::date(&p.release.updated_at),
        fmt::date(&p.release.published_at),
    );

    if !p.licenses.is_empty() {
        println!("- **License:** {}", p.licenses.join(", "));
    }

    if let Some(docs) = &p.links.docs {
        println!("- **Docs:** {docs}");
    }
    if let Some(repo) = &p.links.repository {
        println!("- **Repo:** {repo}");
    }
}
