//! XDG-aware default paths for daemon-managed files.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use directories::{BaseDirs, ProjectDirs};

/// Default location of the persistent rule file. Resolves to
/// `$XDG_CONFIG_HOME/gamerat/rules.toml` (or `~/.config/gamerat/rules.toml`
/// when `XDG_CONFIG_HOME` isn't set).
pub fn default_rules_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("rules.toml"))
}

/// Default location of the persistent profile file. Resolves to
/// `$XDG_CONFIG_HOME/gamerat/profiles.toml`.
pub fn default_profiles_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("profiles.toml"))
}

/// Default location of the daemon-wide settings file (e.g.
/// `auto_switch_enabled`). Resolves to
/// `$XDG_CONFIG_HOME/gamerat/settings.toml`.
pub fn default_settings_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("settings.toml"))
}

/// Default location of the manual-games file — user-added game entries
/// for folders the launcher scanners can't find. Resolves to
/// `$XDG_CONFIG_HOME/gamerat/manual-games.toml`.
pub fn default_manual_games_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("manual-games.toml"))
}

fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("org", "appulsauce", "gamerat")
        .context("could not determine $HOME / XDG config dir")?;
    Ok(dirs.config_dir().to_path_buf())
}

/// Default location of the slot-allocator LRU cache.
///
/// Resolves to `$XDG_STATE_HOME/gamerat/slot-cache.toml`, falling
/// back to `~/.local/state/gamerat/slot-cache.toml`. State (not
/// config) because the cache is daemon-managed runtime data — it
/// can be deleted and the daemon will rebuild it on demand.
pub fn default_slot_cache_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("gamerat").join("slot-cache.toml"))
}

fn state_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path));
    }
    let dirs = BaseDirs::new().context("could not determine $HOME for state-dir fallback")?;
    Ok(dirs.home_dir().join(".local/state"))
}
