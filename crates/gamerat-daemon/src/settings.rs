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

    /// When `false`, focusing a window with no matching rule keeps
    /// the current profile active instead of falling back to Desktop.
    /// Useful for users who don't curate the Desktop slot but still
    /// want autoswitching between game profiles. Defaults to `true`.
    #[serde(default = "default_true")]
    pub desktop_return_enabled: bool,

    /// Debounce window before falling back to Desktop after a focus
    /// event with no matching rule. Briefly tabbing out of a game
    /// (Discord ping, quick Google) shouldn't kick the profile back
    /// to baseline — anything under this delay rides out the
    /// non-game focus. Defaults to `120_000` ms (2 minutes).
    #[serde(default = "default_desktop_return_delay_ms")]
    pub desktop_return_delay_ms: u64,

    /// When `true`, the GUI raises a system notification each time a
    /// profile switch lands. Off by default — the Linux gamer crowd
    /// is often in fullscreen and would experience notifications as
    /// noise rather than feedback.
    #[serde(default)]
    pub notify_on_profile_switch: bool,

    /// Path the settings load/save under. Skipped in serde so it
    /// doesn't end up on disk.
    #[serde(skip)]
    pub path: PathBuf,
}

const fn default_true() -> bool {
    true
}

const fn default_desktop_return_delay_ms() -> u64 {
    120_000
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
                desktop_return_enabled: true,
                desktop_return_delay_ms: default_desktop_return_delay_ms(),
                notify_on_profile_switch: false,
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
        assert!(s.desktop_return_enabled);
        assert_eq!(s.desktop_return_delay_ms, 120_000);
        assert!(!s.notify_on_profile_switch);
    }

    #[test]
    fn legacy_file_with_only_auto_switch_loads() {
        // Existing users' settings.toml files only have one field;
        // serde defaults need to fill in the new ones.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        fs::write(&path, "auto_switch_enabled = false\n").expect("write");
        let s = Settings::load_or_create(path).expect("load");
        assert!(!s.auto_switch_enabled);
        assert!(s.desktop_return_enabled);
        assert_eq!(s.desktop_return_delay_ms, 120_000);
        assert!(!s.notify_on_profile_switch);
    }
}
