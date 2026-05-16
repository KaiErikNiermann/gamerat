//! XDG-aware default paths for daemon-managed files.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use directories::ProjectDirs;

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

fn config_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("org", "appulsauce", "gamerat")
        .context("could not determine $HOME / XDG config dir")?;
    Ok(dirs.config_dir().to_path_buf())
}
