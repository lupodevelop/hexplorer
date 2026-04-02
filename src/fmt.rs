//! Formatting utilities shared between the TUI renderer and the CLI output module.

/// Compact download count: `4.5M`, `890K`, `233`.
pub fn dl_short(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Full download count with underscore thousands separator: `4_521_003`.
pub fn dl_full(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push('_');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

/// Extract `YYYY-MM-DD` from an ISO 8601 datetime string.
pub fn date(s: &str) -> &str {
    s.get(..10).unwrap_or(s)
}

/// Truncate a string to `max` chars, appending `…` if cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dl_short_millions() {
        assert_eq!(dl_short(4_521_003), "4.5M");
    }
    #[test]
    fn dl_short_thousands() {
        assert_eq!(dl_short(890_000), "890K");
    }
    #[test]
    fn dl_short_small() {
        assert_eq!(dl_short(42), "42");
    }

    #[test]
    fn dl_full_separators() {
        assert_eq!(dl_full(4_521_003), "4_521_003");
    }
    #[test]
    fn dl_full_small() {
        assert_eq!(dl_full(42), "42");
    }

    #[test]
    fn date_strips_time() {
        assert_eq!(date("2025-03-21T09:00:00Z"), "2025-03-21");
    }
    #[test]
    fn date_passthrough() {
        assert_eq!(date("2025-03-21"), "2025-03-21");
    }
}
