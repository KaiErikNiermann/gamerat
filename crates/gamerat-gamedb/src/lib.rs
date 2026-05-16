//! Game library discovery across Linux launchers.
//!
//! Maps installed games on the host to stable identifiers and
//! launch-time signatures (executable paths, Wine prefixes, Steam
//! `AppID`s) so the daemon can match focused processes back to the game
//! the user means.
//!
//! # Scanners
//!
//! - [`scan_steam`] — `~/.steam/steam/steamapps/libraryfolders.vdf` walk +
//!   `appmanifest_*.acf` parsing via the `keyvalues-serde` VDF parser.
//! - [`scan_lutris`] — `~/.local/share/lutris/pga.db` `SQLite` query for
//!   installed games.
//! - [`scan_heroic`] — `~/.config/heroic/store_cache/*.json` for Epic
//!   (Legendary), GOG, and Amazon store libraries.
//!
//! # High-level entry point
//!
//! [`scan_all`] runs all scanners, silently swallows
//! [`Error::NotInstalled`] variants, and logs other errors at `WARN`
//! level via [`tracing`].

pub mod error;
pub mod heroic;
pub mod lutris;
pub mod steam;

pub use error::Error;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

/// Which game launcher manages the entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Launcher {
    Steam,
    Lutris,
    Heroic,
    Other,
}

/// A discovered game installation with enough metadata to seed a
/// gamerat rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameEntry {
    /// Launcher-prefixed stable identifier, e.g. `"steam:730"`,
    /// `"lutris:counter-strike-2"`, `"heroic:epic:FortniteClient"`.
    pub id: String,
    /// Human-readable name, e.g. `"Counter-Strike 2"`.
    pub name: String,
    /// Which launcher manages this game.
    pub launcher: Launcher,
    /// Root installation directory, if known.
    pub install_dir: Option<PathBuf>,
    /// Main executable, if known.
    pub executable: Option<PathBuf>,
    /// Best-guess Wayland `app_id` for the focused window when this
    /// game is running. Used to pre-fill the rule editor in the GUI.
    /// `None` when we cannot determine it confidently.
    pub app_id_hint: Option<String>,
}

/// Run all scanners and collect every discovered game into one list.
///
/// `NotInstalled` errors are swallowed silently (the launcher simply
/// isn't present on this machine). All other errors are logged at
/// `WARN` level via [`tracing`] and then discarded — the caller
/// receives every entry that *did* succeed.
pub fn scan_all() -> Vec<GameEntry> {
    let mut entries: Vec<GameEntry> = Vec::new();

    match steam::scan_steam() {
        Ok(mut v) => entries.append(&mut v),
        Err(Error::NotInstalled { .. }) => {}
        Err(e) => warn!(error = %e, "Steam scanner failed"),
    }

    match lutris::scan_lutris() {
        Ok(mut v) => entries.append(&mut v),
        Err(Error::NotInstalled { .. }) => {}
        Err(e) => warn!(error = %e, "Lutris scanner failed"),
    }

    entries.append(&mut heroic::scan_heroic());

    entries
}
