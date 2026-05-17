//! Daemon-wide settings store.
//!
//! Currently small (one boolean) — separated from `RuleStore` /
//! `ProfileStore` so adding future global toggles (default backend,
//! switch debounce, etc.) doesn't churn the rule schema. Persisted as
//! TOML at `$XDG_CONFIG_HOME/gamerat/settings.toml`.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// When `false`, the dispatch loop still emits `FocusChanged` but
    /// stops calling into the slot allocator — profile switching
    /// becomes purely manual (CLI / GUI). Defaults to `true`.
    #[serde(default = "default_true")]
    pub auto_switch_enabled: bool,

    /// Path the settings load/save under. Skipped in serde so it
    /// doesn't end up on disk.
    #[serde(skip)]
    pub path: PathBuf,
}

const fn default_true() -> bool {
    true
}

impl Settings {
    /// Load from `path`, creating a fresh default file if it doesn't
    /// exist. The returned struct remembers `path` for [`Self::save`].
    pub fn load_or_create(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        if path.exists() {
            let text =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let mut parsed: Self =
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            parsed.path = path;
            Ok(parsed)
        } else {
            let s = Self {
                auto_switch_enabled: true,
                path,
            };
            s.save()?;
            Ok(s)
        }
    }

    pub fn save(&self) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializing settings")?;
        fs::write(&self.path, text).with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_load_creates_file_and_enables_autoswitch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        let s = Settings::load_or_create(path.clone()).expect("load");
        assert!(s.auto_switch_enabled);
        assert!(path.exists());
    }

    #[test]
    fn round_trip_disabled_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        let mut s = Settings::load_or_create(path.clone()).expect("load");
        s.auto_switch_enabled = false;
        s.save().expect("save");
        let back = Settings::load_or_create(path).expect("reload");
        assert!(!back.auto_switch_enabled);
    }

    #[test]
    fn missing_field_falls_back_to_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        // empty file — every field defaults
        fs::write(&path, "").expect("write");
        let s = Settings::load_or_create(path).expect("load");
        assert!(s.auto_switch_enabled);
    }
}
