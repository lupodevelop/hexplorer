//! ratatui rendering — all draw functions live here.

use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, List, ListItem, Paragraph, Wrap},
};

use crate::{
    app::{App, GhState},
    fmt,
    types::{Language, SettingRow, View},
};

// ── Fixed palette (non-language-specific) ─────────────────────────────────────

const YELLOW: Color = Color::Rgb(255, 212, 59);
const GREEN: Color = Color::Rgb(80, 250, 123);
const DIM: Color = Color::Rgb(90, 88, 110);
const WHITE: Color = Color::White;
const BG_BAR: Color = Color::Rgb(16, 10, 26);
const BG_SEL: Color = Color::Rgb(38, 14, 52);

// ── Favorites accent ──────────────────────────────────────────────────────────

const FAVORITES_ACCENT: Color = Color::Rgb(255, 212, 59); // same gold as the ★ star

/// Returns the UI accent colour: gold when in favorites mode, language colour otherwise.
fn accent(app: &App) -> Color {
    if app.favorites_mode {
        FAVORITES_ACCENT
    } else {
        app.language.accent()
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &App) {
    let [header, content, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_header(f, app, header);
    match app.view {
        View::List => draw_list_view(f, app, content),
        View::Detail => draw_detail_view(f, app, content),
        View::Settings => draw_settings_view(f, app, content),
    }
    draw_footer(f, app, footer);
}

// ── Header ────────────────────────────────────────────────────────────────────

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let [left, center, right] = Layout::horizontal([
        Constraint::Length(26),
        Constraint::Fill(1),
        Constraint::Length(42),
    ])
    .areas(area);

    // ── Left: logo ────────────────────────────────────────────────────────────
    let accent = accent(app);
    let count = if app.loading {
        " fetching… ".to_string()
    } else if app.page > 1 {
        format!(" {} pkgs · p.{} ", app.packages.len(), app.page)
    } else {
        format!(" {} pkgs ", app.packages.len())
    };

    let logo_block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .title(Line::from(vec![
            Span::styled(" ✦ ", Style::new().fg(accent)),
            Span::styled("hexplorer", Style::new().fg(WHITE).bold()),
        ]))
        .title_bottom(Span::styled(count, Style::new().fg(accent)));
    f.render_widget(logo_block, left);

    // ── Center: language tab bar ──────────────────────────────────────────────
    draw_tab_bar(f, app, center);

    // ── Right: search + sort ──────────────────────────────────────────────────
    let (search_txt, search_sty) = if app.input_mode {
        (format!("  /{}_", app.input), Style::new().fg(YELLOW).bold())
    } else if app.input.is_empty() {
        (
            "  press / to search…".to_string(),
            Style::new().fg(DIM).italic(),
        )
    } else {
        (format!("  /{}", app.input), Style::new().fg(WHITE))
    };

    let search_block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if app.input_mode {
            Style::new().fg(YELLOW)
        } else {
            Style::new().fg(accent)
        });

    let lines = vec![
        Line::from(Span::styled(search_txt, search_sty)),
        Line::from(vec![
            Span::styled("  sort: ", Style::new().fg(DIM)),
            Span::styled(app.sort.label(), Style::new().fg(accent)),
            Span::styled("  [tab]", Style::new().fg(DIM).italic()),
        ]),
    ];
    f.render_widget(Paragraph::new(lines).block(search_block), right);
}

fn draw_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let bar_accent = accent(app);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(bar_accent));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut tab_spans: Vec<Span> = vec![Span::raw(" ")];

    if app.favorites_mode {
        // Favorites mode: show a ★ favorites tab as active, language tabs dimmed.
        tab_spans.push(Span::styled(
            "[★ favorites]",
            Style::new().fg(FAVORITES_ACCENT).bold().underlined(),
        ));
        tab_spans.push(Span::raw("  "));
        for &lang in Language::all() {
            tab_spans.push(Span::styled(lang.label(), Style::new().fg(DIM)));
            tab_spans.push(Span::raw("  "));
        }
    } else {
        // Normal mode: active language tab highlighted, others dimmed.
        for &lang in Language::all() {
            if lang == app.language {
                tab_spans.push(Span::styled(
                    format!("[■ {}]", lang.label()),
                    Style::new().fg(lang.accent()).bold().underlined(),
                ));
            } else {
                tab_spans.push(Span::styled(lang.label(), Style::new().fg(DIM)));
            }
            tab_spans.push(Span::raw("  "));
        }
    }

    let hint = Line::from(Span::styled(
        "  l / L  cycle language",
        Style::new().fg(DIM).italic(),
    ));

    f.render_widget(Paragraph::new(vec![Line::from(tab_spans), hint]), inner);
}

// ── List view ─────────────────────────────────────────────────────────────────

fn draw_list_view(f: &mut Frame, app: &App, area: Rect) {
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Fill(1)]).areas(area);

    draw_package_list(f, app, list_area);
    draw_preview(f, app, preview_area);
}

fn draw_package_list(f: &mut Frame, app: &App, area: Rect) {
    let accent = accent(app);
    let title = if app.favorites_mode {
        format!(" ★ favorites ({}) ", app.packages.len())
    } else {
        format!(" packages ({}) ", app.language.label())
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .title(Span::styled(title, Style::new().fg(accent).bold()));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Error / loading / empty states
    if let Some(err) = &app.error {
        f.render_widget(
            Paragraph::new(format!("\n  ✗  {err}"))
                .style(Style::new().fg(Color::Red))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    if app.loading {
        f.render_widget(
            Paragraph::new("\n  ⟳  fetching from hex.pm…").style(Style::new().fg(DIM).italic()),
            inner,
        );
        return;
    }
    if app.packages.is_empty() {
        f.render_widget(
            Paragraph::new("\n  no packages found").style(Style::new().fg(DIM)),
            inner,
        );
        return;
    }

    let show_badge = app.language == Language::All || app.favorites_mode;

    let items: Vec<ListItem> = app
        .packages
        .iter()
        .map(|p| {
            let mut spans = vec![];

            // Star indicator for favorited packages.
            if app.favorites.contains_key(&p.name) {
                spans.push(Span::styled("★ ", Style::new().fg(YELLOW)));
            } else {
                spans.push(Span::raw("  "));
            }

            // Language badge in All-BEAM mode or favorites mode.
            if show_badge {
                spans.push(Span::styled(
                    format!("[{}] ", p.language.badge()),
                    Style::new().fg(p.language.accent()),
                ));
            }

            spans.push(Span::styled(p.name.clone(), Style::new().fg(WHITE)));
            spans.push(Span::styled(
                format!("  v{}", p.version),
                Style::new().fg(DIM),
            ));
            spans.push(Span::styled(
                format!("  {}⬇", fmt::dl_short(p.downloads_recent)),
                Style::new().fg(accent),
            ));

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .highlight_symbol("▶ ")
        .highlight_style(Style::new().bg(BG_SEL).fg(accent).bold());

    let mut state = app.list_state.clone();
    f.render_stateful_widget(list, inner, &mut state);
}

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let accent = accent(app);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(pkg) = app.selected() else { return };
    let w = inner.width as usize;

    // In All-BEAM mode, use the package's own accent color for its name.
    let name_color = if app.language == Language::All {
        pkg.language.accent()
    } else {
        accent
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(pkg.name.clone(), Style::new().fg(name_color).bold()),
            Span::styled(format!("  v{}", pkg.version), Style::new().fg(accent)),
        ]),
        Line::from(Span::styled("─".repeat(w.min(44)), Style::new().fg(DIM))),
    ];

    if !pkg.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            pkg.description.clone(),
            Style::new().fg(WHITE),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("⬇ ", Style::new().fg(accent)),
        Span::styled(
            fmt::dl_full(pkg.downloads_all),
            Style::new().fg(WHITE).bold(),
        ),
        Span::styled("  total", Style::new().fg(DIM)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("⬇ ", Style::new().fg(accent)),
        Span::styled(fmt::dl_full(pkg.downloads_recent), Style::new().fg(WHITE)),
        Span::styled("  recent", Style::new().fg(DIM)),
    ]));

    // Cached GitHub stats (no live fetch in list view)
    if let Some(entry) = app.preview_gh() {
        lines.push(Line::from(vec![
            Span::styled("★ ", Style::new().fg(YELLOW)),
            Span::styled(entry.stars.to_string(), Style::new().fg(WHITE).bold()),
            Span::styled("  ⑂ ", Style::new().fg(accent)),
            Span::styled(entry.forks.to_string(), Style::new().fg(WHITE)),
            Span::styled(
                format!("  ({})", entry.age_label()),
                Style::new().fg(DIM).italic(),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("updated   ", Style::new().fg(DIM)),
        Span::styled(
            fmt::date(&pkg.updated_at).to_string(),
            Style::new().fg(WHITE),
        ),
    ]));
    if !pkg.licenses.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("license   ", Style::new().fg(DIM)),
            Span::styled(pkg.licenses.join(", "), Style::new().fg(WHITE)),
        ]));
    }
    if let Some(docs) = &pkg.docs_url {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("📖 ", Style::new().fg(accent)),
            Span::styled(
                fmt::truncate(docs, w.saturating_sub(4)),
                Style::new().fg(accent),
            ),
        ]));
    }
    if let Some(repo) = &pkg.repo_url {
        lines.push(Line::from(vec![
            Span::styled("⌥  ", Style::new().fg(accent)),
            Span::styled(
                fmt::truncate(repo, w.saturating_sub(4)),
                Style::new().fg(accent),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↵ Enter for full detail",
        Style::new().fg(DIM).italic(),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ── Detail view ───────────────────────────────────────────────────────────────

fn draw_detail_view(f: &mut Frame, app: &App, area: Rect) {
    let accent = if app.favorites_mode {
        FAVORITES_ACCENT
    } else if app.language == Language::All {
        app.selected()
            .map(|p| p.language.accent())
            .unwrap_or(app.language.accent())
    } else {
        app.language.accent()
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .title(Span::styled(" detail ", Style::new().fg(accent).bold()))
        .title_bottom(Span::styled(" esc / q → back ", Style::new().fg(DIM)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(pkg) = app.selected() else { return };
    let w = inner.width as usize;
    let mut lines: Vec<Line> = vec![];

    // Name + version + language badge
    lines.push(Line::from(vec![
        Span::styled(
            pkg.name.clone(),
            Style::new().fg(accent).bold().underlined(),
        ),
        Span::styled(format!("  v{}", pkg.version), Style::new().fg(accent)),
        Span::styled(
            format!("   [{}]", pkg.language.label()),
            Style::new().fg(pkg.language.accent()).bold(),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "═".repeat(w.min(54)),
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(""));

    // Description
    if !pkg.description.is_empty() {
        lines.push(section("description"));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            pkg.description.clone(),
            Style::new().fg(WHITE),
        )));
        lines.push(Line::from(""));
    }

    // Downloads
    lines.push(section("downloads"));
    lines.push(Line::from(""));
    lines.push(kv("  all-time   ", fmt::dl_full(pkg.downloads_all), accent));
    lines.push(kv(
        "  recent     ",
        fmt::dl_full(pkg.downloads_recent),
        accent,
    ));
    lines.push(Line::from(""));

    // GitHub
    lines.push(section("github"));
    lines.push(Line::from(""));
    match &app.gh {
        GhState::Loading => {
            lines.push(Line::from(Span::styled(
                "  loading…",
                Style::new().fg(DIM).italic(),
            )));
        }
        GhState::Live(stats) => {
            lines.push(kv("  ★ stars    ", stats.stars.to_string(), YELLOW));
            lines.push(kv("  ⑂ forks    ", stats.forks.to_string(), accent));
            lines.push(kv("  ⊙ issues   ", stats.issues.to_string(), WHITE));
            lines.push(Line::from(Span::styled(
                "  (live)",
                Style::new().fg(GREEN).italic(),
            )));
        }
        GhState::Cached(entry) => {
            lines.push(kv("  ★ stars    ", entry.stars.to_string(), YELLOW));
            lines.push(kv("  ⑂ forks    ", entry.forks.to_string(), accent));
            lines.push(kv("  ⊙ issues   ", entry.issues.to_string(), WHITE));
            lines.push(Line::from(Span::styled(
                format!("  (cached {})", entry.age_label()),
                Style::new().fg(DIM).italic(),
            )));
        }
        GhState::RateLimited => {
            lines.push(Line::from(Span::styled(
                "  rate limited (60 req/h)",
                Style::new().fg(YELLOW),
            )));
            lines.push(Line::from(Span::styled(
                "  set GITHUB_TOKEN to raise limit to 5000/h",
                Style::new().fg(DIM).italic(),
            )));
        }
        GhState::BadToken => {
            lines.push(Line::from(Span::styled(
                "  token invalid or expired (HTTP 401)",
                Style::new().fg(Color::Red),
            )));
            lines.push(Line::from(Span::styled(
                "  update via ? → settings or GITHUB_TOKEN env var",
                Style::new().fg(DIM).italic(),
            )));
        }
        GhState::Unavailable => {
            lines.push(Line::from(Span::styled(
                "  stats unavailable",
                Style::new().fg(DIM),
            )));
        }
        GhState::NoRepo => {
            lines.push(Line::from(Span::styled(
                "  no repository",
                Style::new().fg(DIM),
            )));
        }
    }
    lines.push(Line::from(""));

    // Links
    lines.push(section("links"));
    lines.push(Line::from(""));
    if let Some(u) = &pkg.docs_url {
        lines.push(url_line("  📖 docs     ", u.clone(), accent));
    }
    if let Some(u) = &pkg.hex_url {
        lines.push(url_line("  ◈  hex.pm   ", u.clone(), accent));
    }
    if let Some(u) = &pkg.repo_url {
        lines.push(url_line("  ⌥  repo     ", u.clone(), accent));
    }
    lines.push(Line::from(""));

    // Metadata
    lines.push(section("metadata"));
    lines.push(Line::from(""));
    if !pkg.build_tool.is_empty() {
        lines.push(kv("  build tool ", pkg.build_tool.clone(), WHITE));
    }
    lines.push(kv(
        "  updated    ",
        fmt::date(&pkg.updated_at).to_string(),
        WHITE,
    ));
    lines.push(kv(
        "  published  ",
        fmt::date(&pkg.inserted_at).to_string(),
        WHITE,
    ));
    if !pkg.licenses.is_empty() {
        lines.push(kv("  license    ", pkg.licenses.join(", "), WHITE));
    }

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.scroll, 0)),
        inner,
    );
}

// ── Settings view ─────────────────────────────────────────────────────────────

const SETTINGS_ACCENT: Color = Color::Rgb(97, 218, 251); // neutral BEAM cyan

fn draw_settings_view(f: &mut Frame, app: &App, area: Rect) {
    let ac = SETTINGS_ACCENT;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ac))
        .title(Span::styled(" ⚙ settings ", Style::new().fg(ac).bold()))
        .title_bottom(Span::styled(" esc / q → back ", Style::new().fg(DIM)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = SettingRow::all();
    let cursor = app.settings_cursor;
    let mut lines: Vec<Line> = vec![Line::from("")];

    // ── GitHub section ────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        "  github",
        Style::new().fg(DIM).bold(),
    )));
    lines.push(Line::from(""));

    // Token row
    let is_token = rows[cursor] == SettingRow::GithubToken;
    let prefix = if is_token { "▶  " } else { "   " };
    let row_color = if is_token { ac } else { WHITE };

    let token_val: Line = if app.settings_editing {
        Line::from(vec![
            Span::styled(prefix, Style::new().fg(ac).bold()),
            Span::styled("token    ", Style::new().fg(DIM)),
            Span::styled(
                format!("[{}█]", app.settings_input),
                Style::new().fg(YELLOW).bold(),
            ),
            Span::styled(
                "  enter to confirm · esc to cancel",
                Style::new().fg(DIM).italic(),
            ),
        ])
    } else {
        let val = app
            .settings_token
            .as_deref()
            .map(mask_token_ui)
            .unwrap_or_else(|| "(not set)".to_string());
        let hint = if app.settings_token.is_some() {
            "  enter to edit · d to clear"
        } else {
            "  enter to set"
        };
        Line::from(vec![
            Span::styled(prefix, Style::new().fg(ac).bold()),
            Span::styled("token    ", Style::new().fg(DIM)),
            Span::styled(val, Style::new().fg(row_color).bold()),
            Span::styled(hint, Style::new().fg(DIM).italic()),
        ])
    };
    lines.push(token_val);
    lines.push(Line::from(Span::styled(
        "             ~/.config/hexplorer/credentials.json (0600)",
        Style::new().fg(DIM).italic(),
    )));
    lines.push(Line::from(""));

    // ── Storage section ───────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        "  storage",
        Style::new().fg(DIM).bold(),
    )));
    lines.push(Line::from(""));

    // keep_weeks row
    let is_kw = rows[cursor] == SettingRow::KeepWeeks;
    let (pre, col) = if is_kw { ("▶  ", ac) } else { ("   ", WHITE) };
    lines.push(Line::from(vec![
        Span::styled(pre, Style::new().fg(ac).bold()),
        Span::styled("keep_weeks  ", Style::new().fg(DIM)),
        Span::styled(
            format!("{} weeks", app.settings_config.keep_weeks),
            Style::new().fg(col).bold(),
        ),
        Span::styled("  ← →", Style::new().fg(DIM).italic()),
    ]));

    // compress row
    let is_cmp = rows[cursor] == SettingRow::Compress;
    let (pre, col) = if is_cmp {
        ("▶  ", ac)
    } else {
        ("   ", WHITE)
    };
    let compress_val = if app.settings_config.compress {
        "on"
    } else {
        "off"
    };
    lines.push(Line::from(vec![
        Span::styled(pre, Style::new().fg(ac).bold()),
        Span::styled("compress    ", Style::new().fg(DIM)),
        Span::styled(compress_val, Style::new().fg(col).bold()),
        Span::styled("  enter to toggle", Style::new().fg(DIM).italic()),
    ]));

    // gh cache row
    let is_gc = rows[cursor] == SettingRow::ClearGhCache;
    let (pre, col) = if is_gc { ("▶  ", ac) } else { ("   ", WHITE) };
    let cache_count = app.cache.len();
    lines.push(Line::from(vec![
        Span::styled(pre, Style::new().fg(ac).bold()),
        Span::styled("gh cache    ", Style::new().fg(DIM)),
        Span::styled(
            format!("{cache_count} entries"),
            Style::new().fg(col).bold(),
        ),
        Span::styled("  enter to clear", Style::new().fg(DIM).italic()),
    ]));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn mask_token_ui(t: &str) -> String {
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= 8 {
        "***".to_string()
    } else {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}…{}", head, tail)
    }
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let accent = accent(app);

    let spans: Vec<Span> = match app.view {
        View::List => {
            let mut spans = vec![
                Span::styled(" /", Style::new().fg(accent).bold()),
                Span::styled(" search  ", Style::new().fg(DIM)),
                Span::styled("↑↓ j k", Style::new().fg(accent).bold()),
                Span::styled(" nav  ", Style::new().fg(DIM)),
                Span::styled("↵", Style::new().fg(accent).bold()),
                Span::styled(" detail  ", Style::new().fg(DIM)),
                Span::styled("l / L", Style::new().fg(accent).bold()),
                Span::styled(" lang  ", Style::new().fg(DIM)),
                Span::styled("tab", Style::new().fg(accent).bold()),
                Span::styled(" sort  ", Style::new().fg(DIM)),
            ];
            // Show pagination hint only when browsing (no active query).
            if app.input.trim().is_empty() && (app.page > 1 || app.has_more) {
                spans.push(Span::styled("[ ]", Style::new().fg(accent).bold()));
                spans.push(Span::styled(" page  ", Style::new().fg(DIM)));
            }
            spans.push(Span::styled("s", Style::new().fg(YELLOW).bold()));
            spans.push(Span::styled(" star  ", Style::new().fg(DIM)));
            if !app.favorites.is_empty() || app.favorites_mode {
                spans.push(Span::styled("f", Style::new().fg(YELLOW).bold()));
                spans.push(Span::styled(" favorites  ", Style::new().fg(DIM)));
            }
            spans.push(Span::styled("r", Style::new().fg(accent).bold()));
            spans.push(Span::styled(" refresh  ", Style::new().fg(DIM)));
            spans.push(Span::styled("?", Style::new().fg(accent).bold()));
            spans.push(Span::styled(" settings  ", Style::new().fg(DIM)));
            spans.push(Span::styled("q", Style::new().fg(accent).bold()));
            spans.push(Span::styled(" quit", Style::new().fg(DIM)));
            spans
        }
        View::Detail => vec![
            Span::styled(" esc / q", Style::new().fg(accent).bold()),
            Span::styled(" back  ", Style::new().fg(DIM)),
            Span::styled("↑↓ j k", Style::new().fg(accent).bold()),
            Span::styled(" scroll  ", Style::new().fg(DIM)),
            Span::styled("PgUp/Dn", Style::new().fg(accent).bold()),
            Span::styled(" fast", Style::new().fg(DIM)),
        ],
        View::Settings => vec![
            Span::styled(" esc / q", Style::new().fg(SETTINGS_ACCENT).bold()),
            Span::styled(" back  ", Style::new().fg(DIM)),
            Span::styled("↑↓ j k", Style::new().fg(SETTINGS_ACCENT).bold()),
            Span::styled(" navigate  ", Style::new().fg(DIM)),
            Span::styled("enter", Style::new().fg(SETTINGS_ACCENT).bold()),
            Span::styled(" edit  ", Style::new().fg(DIM)),
            Span::styled("← →", Style::new().fg(SETTINGS_ACCENT).bold()),
            Span::styled(" cycle", Style::new().fg(DIM)),
        ],
    };

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(BG_BAR)),
        area,
    );
}

// ── Line helpers ──────────────────────────────────────────────────────────────

fn section(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(title, Style::new().fg(DIM).bold()))
}

fn kv(key: &'static str, val: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(key, Style::new().fg(DIM)),
        Span::styled(val, Style::new().fg(color).bold()),
    ])
}

fn url_line(label: &'static str, url: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, Style::new().fg(DIM)),
        Span::styled(url, Style::new().fg(color).underlined()),
    ])
}
