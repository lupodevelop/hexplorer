//! Persistent local favorites (starred packages).
//!
//! Stored as `~/.cache/hexplorer/favorites.json` — a JSON object mapping
//! package name → language string (e.g. `{"gleam_stdlib": "Gleam"}`).
//! Saving the language avoids re-inferring it from build_tools on load,
//! which would fail when the listing API omits that field.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::types::Language;

/// `package_name → Language` — the canonical in-memory representation.
pub type Favorites = HashMap<String, Language>;

fn path() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("hexplorer").join("favorites.json"))
}

pub fn load() -> Favorites {
    let Some(p) = path() else {
        return Favorites::new();
    };
    let Ok(bytes) = std::fs::read(&p) else {
        return Favorites::new();
    };
    serde_json::from_slice::<Favorites>(&bytes).unwrap_or_default()
}

pub fn save(favs: &Favorites) {
    let Some(p) = path() else { return };
    if let Ok(json) = serde_json::to_string_pretty(favs) {
        let _ = std::fs::write(&p, json);
    }
}

/// Toggle `name` in the set (storing its `language`).
/// Returns `true` if the package is now starred, `false` if it was removed.
pub fn toggle(favs: &mut Favorites, name: &str, language: Language) -> bool {
    if favs.contains_key(name) {
        favs.remove(name);
        false
    } else {
        favs.insert(name.to_string(), language);
        true
    }
}
