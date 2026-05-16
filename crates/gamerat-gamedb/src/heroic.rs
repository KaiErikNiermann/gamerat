//! Heroic Games Launcher library scanner.
//!
//! Reads per-store JSON cache files from `~/.config/heroic/store_cache/`:
//!
//! | File                        | Store          |
//! |-----------------------------|----------------|
//! | `legendary_library.json`    | Epic Games     |
//! | `gog_library.json`          | GOG            |
//! | `amazon_library.json`       | Amazon Games   |
//!
//! Each file's schema differs slightly between stores. All fields are
//! deserialized with `#[serde(default)]` so missing or renamed keys
//! don't cause hard failures.
//!
//! # `app_id_hint`
//!
//! Heroic games are typically run through a compatibility layer (Wine /
//! Proton / native), and Wayland `app_id`s are highly game-specific.
//! We return `None` for all stores — the user will need to set the
//! correct glob in the rule editor.
//!
//! # Error strategy
//!
//! [`scan_heroic`] returns a flat `Vec<GameEntry>` (not `Result`) so
//! that a missing Amazon store, for example, doesn't prevent Epic or
//! GOG entries from being returned. Per-store errors are logged at
//! `WARN` level via [`tracing`].

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::{GameEntry, Launcher, error::Error};

// ---------------------------------------------------------------------------
// Wire types — Epic (Legendary)
// ---------------------------------------------------------------------------

/// Root of `legendary_library.json`.
///
/// The file is a JSON object with a `"library"` array.
#[derive(Debug, Deserialize)]
struct LegendaryLibrary {
    #[serde(default)]
    library: Vec<LegendaryGame>,
}

/// One game entry from the Legendary (Epic) library cache.
#[derive(Debug, Deserialize)]
struct LegendaryGame {
    /// Epic app name / product slug, e.g. `"Fortnite"`.
    #[serde(default)]
    app_name: String,
    /// Human-readable title.
    #[serde(default)]
    title: String,
    /// Installation path (absent for uninstalled games).
    #[serde(default)]
    install_path: Option<String>,
    /// Main executable relative to `install_path`.
    #[serde(default)]
    executable: Option<String>,
    /// Whether the game is installed. Missing field → false.
    #[serde(default)]
    is_installed: bool,
}

// ---------------------------------------------------------------------------
// Wire types — GOG
// ---------------------------------------------------------------------------

/// Root of `gog_library.json`.
#[derive(Debug, Deserialize)]
struct GogLibrary {
    #[serde(default)]
    games: Vec<GogGame>,
}

/// One game entry from the GOG library cache.
#[derive(Debug, Deserialize)]
struct GogGame {
    /// GOG numeric app ID, stored as a string.
    #[serde(default)]
    app_id: String,
    /// Human-readable title.
    #[serde(default)]
    title: String,
    /// Installation folder.
    #[serde(default)]
    install_path: Option<String>,
    /// Main executable relative to `install_path`.
    #[serde(default)]
    executable: Option<String>,
    /// Whether the game is installed.
    #[serde(default)]
    is_installed: bool,
}

// ---------------------------------------------------------------------------
// Wire types — Amazon
// ---------------------------------------------------------------------------

/// Root of `amazon_library.json`.
#[derive(Debug, Deserialize)]
struct AmazonLibrary {
    #[serde(default)]
    library: Vec<AmazonGame>,
}

/// One game entry from the Amazon Games library cache.
#[derive(Debug, Deserialize)]
struct AmazonGame {
    /// Amazon product ASIN or internal ID.
    #[serde(default)]
    app_name: String,
    /// Human-readable title.
    #[serde(default)]
    title: String,
    /// Installation path.
    #[serde(default)]
    install_path: Option<String>,
    /// Main executable.
    #[serde(default)]
    executable: Option<String>,
    /// Whether the game is installed.
    #[serde(default)]
    is_installed: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan all Heroic store caches and return one [`GameEntry`] per
/// installed game.
///
/// Never returns an error at the top level — individual store failures
/// are logged and skipped. Returns an empty `Vec` if Heroic is not
/// installed.
pub fn scan_heroic() -> Vec<GameEntry> {
    let Some(base) = BaseDirs::new() else {
        warn!("could not determine home directory; skipping Heroic scan");
        return Vec::new();
    };

    let heroic_dir = base.config_dir().join("heroic");
    if !heroic_dir.exists() {
        debug!(path = %heroic_dir.display(), "Heroic config dir not found; skipping");
        return Vec::new();
    }

    let cache_dir = heroic_dir.join("store_cache");
    let mut entries = Vec::new();

    // Epic (Legendary)
    let epic_path = cache_dir.join("legendary_library.json");
    match scan_epic(&epic_path) {
        Ok(mut v) => entries.append(&mut v),
        Err(Error::NotInstalled { .. }) => {}
        Err(e) => warn!(path = %epic_path.display(), error = %e, "Epic library scan failed"),
    }

    // GOG
    let gog_path = cache_dir.join("gog_library.json");
    match scan_gog(&gog_path) {
        Ok(mut v) => entries.append(&mut v),
        Err(Error::NotInstalled { .. }) => {}
        Err(e) => warn!(path = %gog_path.display(), error = %e, "GOG library scan failed"),
    }

    // Amazon (optional — the file may not exist even on installed Heroic)
    let amazon_path = cache_dir.join("amazon_library.json");
    match scan_amazon(&amazon_path) {
        Ok(mut v) => entries.append(&mut v),
        Err(Error::NotInstalled { .. }) => {}
        Err(e) => warn!(path = %amazon_path.display(), error = %e, "Amazon library scan failed"),
    }

    debug!(
        count = entries.len(),
        "Heroic: found installed games across all stores"
    );
    entries
}

// ---------------------------------------------------------------------------
// Per-store scanners
// ---------------------------------------------------------------------------

/// Parse the Epic (Legendary) store cache.
pub(crate) fn scan_epic(path: &Path) -> Result<Vec<GameEntry>, Error> {
    let text = read_cache_file(path, Launcher::Heroic)?;
    parse_epic_library(&text, path)
}

/// Parse Epic library JSON from a string — exposed for testing.
pub(crate) fn parse_epic_library(text: &str, path: &Path) -> Result<Vec<GameEntry>, Error> {
    let lib: LegendaryLibrary = serde_json::from_str(text).map_err(|e| Error::Parse {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let entries = lib
        .library
        .into_iter()
        .filter(|g| g.is_installed && !g.app_name.is_empty())
        .map(|g| {
            let install_dir = g
                .install_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let executable = build_executable(install_dir.as_deref(), g.executable.as_deref());
            let app_id_hint = executable.as_deref().and_then(exe_stem);
            GameEntry {
                id: format!("heroic:epic:{}", g.app_name),
                name: g.title,
                launcher: Launcher::Heroic,
                install_dir,
                executable,
                app_id_hint,
            }
        })
        .collect();

    Ok(entries)
}

/// Parse the GOG store cache.
pub(crate) fn scan_gog(path: &Path) -> Result<Vec<GameEntry>, Error> {
    let text = read_cache_file(path, Launcher::Heroic)?;
    parse_gog_library(&text, path)
}

/// Parse GOG library JSON from a string — exposed for testing.
pub(crate) fn parse_gog_library(text: &str, path: &Path) -> Result<Vec<GameEntry>, Error> {
    let lib: GogLibrary = serde_json::from_str(text).map_err(|e| Error::Parse {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let entries = lib
        .games
        .into_iter()
        .filter(|g| g.is_installed && !g.app_id.is_empty())
        .map(|g| {
            let install_dir = g
                .install_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let executable = build_executable(install_dir.as_deref(), g.executable.as_deref());
            let app_id_hint = executable.as_deref().and_then(exe_stem);
            GameEntry {
                id: format!("heroic:gog:{}", g.app_id),
                name: g.title,
                launcher: Launcher::Heroic,
                install_dir,
                executable,
                app_id_hint,
            }
        })
        .collect();

    Ok(entries)
}

/// Parse the Amazon store cache.
pub(crate) fn scan_amazon(path: &Path) -> Result<Vec<GameEntry>, Error> {
    let text = read_cache_file(path, Launcher::Heroic)?;
    parse_amazon_library(&text, path)
}

/// Parse Amazon library JSON from a string — exposed for testing.
pub(crate) fn parse_amazon_library(text: &str, path: &Path) -> Result<Vec<GameEntry>, Error> {
    let lib: AmazonLibrary = serde_json::from_str(text).map_err(|e| Error::Parse {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let entries = lib
        .library
        .into_iter()
        .filter(|g| g.is_installed && !g.app_name.is_empty())
        .map(|g| {
            let install_dir = g
                .install_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let executable = build_executable(install_dir.as_deref(), g.executable.as_deref());
            let app_id_hint = executable.as_deref().and_then(exe_stem);
            GameEntry {
                id: format!("heroic:amazon:{}", g.app_name),
                name: g.title,
                launcher: Launcher::Heroic,
                install_dir,
                executable,
                app_id_hint,
            }
        })
        .collect();

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Return the filename stem of `path` as a `String`, or `None` if the
/// path has no filename or the name is not valid UTF-8.
fn exe_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(str::to_owned)
}

fn read_cache_file(path: &Path, launcher: Launcher) -> Result<String, Error> {
    if !path.exists() {
        return Err(Error::NotInstalled {
            launcher,
            path: path.to_path_buf(),
        });
    }
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Build a full executable path from an optional install dir and a
/// relative executable string.
///
/// If `executable` is already absolute, return it as-is. If it's
/// relative and `install_dir` is provided, join them. If `executable`
/// is `None`, return `None`.
fn build_executable(install_dir: Option<&Path>, executable: Option<&str>) -> Option<PathBuf> {
    let exe = executable.filter(|s| !s.is_empty())?;
    let exe_path = Path::new(exe);
    if exe_path.is_absolute() {
        Some(exe_path.to_path_buf())
    } else {
        install_dir.map(|d| d.join(exe_path))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ---- Epic ----

    const EPIC_JSON: &str = r#"{
        "library": [
            {
                "app_name": "FortniteClient",
                "title": "Fortnite",
                "install_path": "/games/Fortnite",
                "executable": "FortniteClient-Win64-Shipping.exe",
                "is_installed": true
            },
            {
                "app_name": "Rocket League",
                "title": "Rocket League",
                "install_path": "/games/RocketLeague",
                "executable": "RocketLeague.exe",
                "is_installed": false
            },
            {
                "app_name": "HelloNeighbor",
                "title": "Hello Neighbor",
                "install_path": "/games/HelloNeighbor",
                "executable": "HelloNeighbor.exe",
                "is_installed": true
            }
        ]
    }"#;

    #[test]
    fn epic_filters_uninstalled_and_produces_correct_ids() {
        let entries =
            parse_epic_library(EPIC_JSON, Path::new("legendary_library.json")).expect("parse ok");
        // Rocket League is not installed, so only 2 entries.
        assert_eq!(entries.len(), 2);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"heroic:epic:FortniteClient"));
        assert!(ids.contains(&"heroic:epic:HelloNeighbor"));
    }

    #[test]
    fn epic_entry_fields() {
        let entries =
            parse_epic_library(EPIC_JSON, Path::new("legendary_library.json")).expect("parse ok");
        let fortnite = entries
            .iter()
            .find(|e| e.id == "heroic:epic:FortniteClient")
            .expect("Fortnite entry");

        assert_eq!(fortnite.name, "Fortnite");
        assert_eq!(fortnite.launcher, Launcher::Heroic);
        assert_eq!(fortnite.install_dir, Some(PathBuf::from("/games/Fortnite")));
        assert_eq!(
            fortnite.executable,
            Some(PathBuf::from(
                "/games/Fortnite/FortniteClient-Win64-Shipping.exe"
            ))
        );
        // exe stem without extension
        assert_eq!(
            fortnite.app_id_hint,
            Some("FortniteClient-Win64-Shipping".to_owned())
        );
    }

    // ---- GOG ----

    const GOG_JSON: &str = r#"{
        "games": [
            {
                "app_id": "1207658924",
                "title": "The Witcher 3: Wild Hunt",
                "install_path": "/games/witcher3",
                "executable": "witcher3",
                "is_installed": true
            },
            {
                "app_id": "1458058728",
                "title": "Cyberpunk 2077",
                "install_path": null,
                "executable": null,
                "is_installed": false
            }
        ]
    }"#;

    #[test]
    fn gog_filters_uninstalled() {
        let entries = parse_gog_library(GOG_JSON, Path::new("gog_library.json")).expect("parse ok");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "heroic:gog:1207658924");
        assert_eq!(entries[0].name, "The Witcher 3: Wild Hunt");
        assert_eq!(entries[0].app_id_hint, Some("witcher3".to_owned()));
    }

    // ---- Amazon ----

    const AMAZON_JSON: &str = r#"{
        "library": [
            {
                "app_name": "Fallout76",
                "title": "Fallout 76",
                "install_path": "/games/Fallout76",
                "executable": "Fallout76.exe",
                "is_installed": true
            }
        ]
    }"#;

    #[test]
    fn amazon_produces_correct_entry() {
        let entries =
            parse_amazon_library(AMAZON_JSON, Path::new("amazon_library.json")).expect("parse ok");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "heroic:amazon:Fallout76");
        assert_eq!(entries[0].name, "Fallout 76");
    }

    // ---- Missing / empty fields ----

    #[test]
    fn epic_entry_with_missing_fields_does_not_panic() {
        // Minimal entry — all optional fields absent.
        let json = r#"{"library": [{"app_name": "MinimalGame", "is_installed": true}]}"#;
        let entries =
            parse_epic_library(json, Path::new("legendary_library.json")).expect("parse ok");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].install_dir.is_none());
        assert!(entries[0].executable.is_none());
        assert!(entries[0].app_id_hint.is_none());
    }

    #[test]
    fn build_executable_absolute_path_returned_as_is() {
        let result = build_executable(Some(Path::new("/games/foo")), Some("/usr/bin/my_game"));
        assert_eq!(result, Some(PathBuf::from("/usr/bin/my_game")));
    }

    #[test]
    fn build_executable_relative_joined_with_install_dir() {
        let result = build_executable(Some(Path::new("/games/foo")), Some("bin/game"));
        assert_eq!(result, Some(PathBuf::from("/games/foo/bin/game")));
    }
}
