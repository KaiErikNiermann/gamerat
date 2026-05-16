//! Steam library scanner.
//!
//! Reads `~/.steam/steam/steamapps/libraryfolders.vdf` to discover all
//! Steam library roots, then for each root walks `appmanifest_*.acf`
//! files and extracts game metadata. Both files use Valve's VDF
//! (`KeyValues`) format, parsed by [`keyvalues_serde`].
//!
//! # `app_id_hint`
//!
//! Steam sets the Wayland `app_id` to `steam_app_<appid>` for both
//! native Linux games and Proton-wrapped titles. That pattern is what
//! users will most commonly want as a glob in their gamerat rules.

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use directories::BaseDirs;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::{
    GameEntry, Launcher,
    error::{Error, Result},
};

// ---------------------------------------------------------------------------
// VDF wire types
// ---------------------------------------------------------------------------

/// Minimal subset of an `appmanifest_*.acf` `AppState` block.
///
/// The top-level VDF key for `libraryfolders.vdf` is an object with
/// integer-keyed sub-objects (`"0"`, `"1"`, …). We deserialise directly
/// into a `serde_json::Value` so we can iterate keys without needing
/// a concrete struct (the key names are runtime integers, not fixed
/// field names).
///
/// For ACF files, the top-level key is always `"AppState"` and the
/// inner fields are stable enough to deserialise into this struct.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
struct AppState {
    appid: String,
    name: String,
    installdir: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan all Steam library roots and return one [`GameEntry`] per
/// installed game.
///
/// Returns [`Error::NotInstalled`] if `~/.steam/steam` does not exist.
pub fn scan_steam() -> Result<Vec<GameEntry>> {
    let base = BaseDirs::new().ok_or_else(|| Error::NotInstalled {
        launcher: Launcher::Steam,
        path: PathBuf::from("~/.steam/steam"),
    })?;

    // Canonical Steam data root: ~/.steam/steam is a symlink to the
    // actual data directory (usually ~/.local/share/Steam).
    let steam_root = base.home_dir().join(".steam").join("steam");
    if !steam_root.exists() {
        return Err(Error::NotInstalled {
            launcher: Launcher::Steam,
            path: steam_root,
        });
    }

    let lf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let roots = parse_library_roots(&lf_path)?;
    debug!(count = roots.len(), "discovered Steam library roots");

    let mut entries = Vec::new();
    for root in roots {
        match scan_library_root(&root) {
            Ok(mut v) => entries.append(&mut v),
            Err(e) => warn!(root = %root.display(), error = %e, "skipping Steam library root"),
        }
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse `libraryfolders.vdf` and return all `path` values.
fn parse_library_roots(vdf_path: &Path) -> Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(vdf_path).map_err(|source| Error::Io {
        path: vdf_path.to_path_buf(),
        source,
    })?;

    parse_library_roots_from_str(&text, vdf_path)
}

/// Parse library roots from a VDF string slice.
///
/// Factored out of [`parse_library_roots`] so tests can call it without
/// touching the filesystem.
///
/// `libraryfolders.vdf` uses numeric keys (`"0"`, `"1"`, …) for each
/// library slot. `keyvalues-serde` cannot round-trip these into typed
/// structs cleanly (repeated keys, no fixed field names), so we use
/// the lower-level `keyvalues_parser` API and walk the AST directly.
pub(crate) fn parse_library_roots_from_str(text: &str, vdf_path: &Path) -> Result<Vec<PathBuf>> {
    use keyvalues_serde::parser::Value;

    let vdf = keyvalues_serde::parser::Vdf::parse(text).map_err(|e| Error::Parse {
        path: vdf_path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // The top-level value of libraryfolders.vdf is an Obj whose keys
    // are integer slot numbers ("0", "1", …) plus metadata keys like
    // "contentstatsid". Each slot value is itself an Obj with a "path"
    // string entry.
    let top_obj = vdf.value.get_obj().ok_or_else(|| Error::Parse {
        path: vdf_path.to_path_buf(),
        reason: "libraryfolders.vdf top-level value is not an object".to_owned(),
    })?;

    let mut roots = Vec::new();
    for (key, values) in &top_obj.0 {
        // Only process integer-keyed slots.
        if key.parse::<u32>().is_err() {
            continue;
        }
        for slot_value in values {
            let Value::Obj(slot_obj) = slot_value else {
                continue;
            };
            if let Some(path_values) = slot_obj.0.get("path") {
                for pv in path_values {
                    if let Value::Str(s) = pv {
                        roots.push(PathBuf::from(s.as_ref()));
                    }
                }
            }
        }
    }
    Ok(roots)
}

/// Walk `<root>/steamapps/appmanifest_*.acf` and build [`GameEntry`]s.
fn scan_library_root(root: &Path) -> Result<Vec<GameEntry>> {
    let steamapps = root.join("steamapps");

    let read_dir = std::fs::read_dir(&steamapps).map_err(|source| Error::Io {
        path: steamapps.clone(),
        source,
    })?;

    let mut entries = Vec::new();
    for dir_entry in read_dir.flatten() {
        let path = dir_entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        let ext_ok = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("acf"));
        if !name.starts_with("appmanifest_") || !ext_ok {
            continue;
        }

        match parse_acf(&path, &steamapps) {
            Ok(entry) => entries.push(entry),
            Err(e) => warn!(acf = %path.display(), error = %e, "skipping ACF"),
        }
    }
    Ok(entries)
}

/// Parse a single `.acf` file and build a [`GameEntry`].
fn parse_acf(acf_path: &Path, steamapps: &Path) -> Result<GameEntry> {
    let text = std::fs::read_to_string(acf_path).map_err(|source| Error::Io {
        path: acf_path.to_path_buf(),
        source,
    })?;

    parse_acf_from_str(&text, steamapps, acf_path)
}

/// Parse an ACF from a string slice — factored out for testability.
pub(crate) fn parse_acf_from_str(
    text: &str,
    steamapps: &Path,
    acf_path: &Path,
) -> Result<GameEntry> {
    let app: AppState = keyvalues_serde::from_str(text).map_err(|e| Error::Parse {
        path: acf_path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let install_dir = if app.installdir.is_empty() {
        None
    } else {
        Some(steamapps.join("common").join(&app.installdir))
    };

    // Validate the appid is a valid integer string (it always is for
    // real Steam games, but be defensive about hand-edited files).
    if u64::from_str(&app.appid).is_err() {
        return Err(Error::Parse {
            path: acf_path.to_path_buf(),
            reason: format!("invalid appid {:?}", app.appid),
        });
    }

    Ok(GameEntry {
        id: format!("steam:{}", app.appid),
        name: app.name,
        launcher: Launcher::Steam,
        install_dir,
        executable: None, // Steam doesn't store the executable path in ACF.
        // Proton and native Steam on Wayland both set app_id to
        // "steam_app_<appid>" on the toplevel surface.
        app_id_hint: Some(format!("steam_app_{}", app.appid)),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const LIBRARY_FOLDERS_VDF: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"label"		""
		"contentid"		"123456789"
		"totalsize"		"0"
		"update_clean_bytes_tally"		"0"
		"time_last_update_verified"		"0"
		"apps"
		{
			"228980"		"840413039"
			"730"		"65936017837"
		}
	}
	"1"
	{
		"path"		"/mnt/hdd/SteamLibrary"
		"label"		"HDD"
		"contentid"		"987654321"
		"totalsize"		"1000000000"
		"update_clean_bytes_tally"		"0"
		"time_last_update_verified"		"0"
		"apps"
		{
			"570"		"12345678"
		}
	}
}"#;

    const CS2_ACF: &str = r#""AppState"
{
	"appid"		"730"
	"universe"		"1"
	"name"		"Counter-Strike 2"
	"StateFlags"		"4"
	"installdir"		"Counter-Strike Global Offensive"
	"lastupdated"		"1700000000"
	"LastPlayed"		"1700000001"
	"SizeOnDisk"		"35000000000"
	"buildid"		"12345678"
	"LastOwner"		"76561198000000000"
	"UpdateResult"		"0"
}"#;

    #[test]
    fn parse_library_roots_returns_two_paths() {
        let roots =
            parse_library_roots_from_str(LIBRARY_FOLDERS_VDF, Path::new("libraryfolders.vdf"))
                .expect("parse should succeed");
        assert_eq!(roots.len(), 2);
        assert!(
            roots
                .iter()
                .any(|p| p == Path::new("/home/user/.local/share/Steam"))
        );
        assert!(
            roots
                .iter()
                .any(|p| p == Path::new("/mnt/hdd/SteamLibrary"))
        );
    }

    #[test]
    fn parse_cs2_acf_produces_correct_entry() {
        let steamapps = Path::new("/mnt/hdd/SteamLibrary/steamapps");
        let entry = parse_acf_from_str(CS2_ACF, steamapps, Path::new("appmanifest_730.acf"))
            .expect("parse should succeed");

        assert_eq!(entry.id, "steam:730");
        assert_eq!(entry.name, "Counter-Strike 2");
        assert_eq!(entry.launcher, Launcher::Steam);
        assert_eq!(
            entry.install_dir,
            Some(
                steamapps
                    .join("common")
                    .join("Counter-Strike Global Offensive")
            )
        );
        assert!(entry.executable.is_none());
        assert_eq!(entry.app_id_hint, Some("steam_app_730".to_owned()));
    }

    #[test]
    fn parse_acf_rejects_non_numeric_appid() {
        let bad_acf = r#""AppState"
{
	"appid"		"not-a-number"
	"name"		"Broken Game"
	"installdir"	"broken"
}"#;
        let result = parse_acf_from_str(bad_acf, Path::new("/tmp"), Path::new("bad.acf"));
        assert!(matches!(result, Err(Error::Parse { .. })));
    }
}
