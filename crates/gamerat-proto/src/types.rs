//! Wire types for the `org.appulsauce.GameRat1` interface.
//!
//! Every type here derives both [`zbus::zvariant::Type`] (for the D-Bus
//! wire format) and serde's `Serialize` / `Deserialize` (for the
//! TOML-on-disk rule store and any other secondary encodings). The
//! D-Bus signature of each struct is asserted in the test module to
//! keep the Rust definitions and the interface XML from drifting.

use serde::{Deserialize, Serialize};
use zbus::zvariant::{OwnedObjectPath, Type};

/// A focus-rule: when an active window's `app_id` matches `app_id_glob`,
/// the daemon resolves `profile_id` against the [`GameratProfile`]
/// store and applies that profile to the device.
///
/// D-Bus signature: `(sst)`.
///
/// **Wire-breaking change since Phase D**: this struct used to carry
/// `profile_index: u32` (a raw hardware slot index). It now carries
/// `profile_id: String` referencing a software profile by stable id.
/// Slot assignment moves into the daemon's `SlotAllocator`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct Rule {
    /// Glob pattern matched against the focused window's `app_id`.
    /// Syntax follows the [`globset`] crate (`*`, `?`, `[...]`).
    pub app_id_glob: String,
    /// Id of a [`GameratProfile`] the daemon should apply when this
    /// rule matches. If the daemon's profile store doesn't currently
    /// hold a profile with this id, the rule is logged-and-skipped.
    pub profile_id: String,
    /// Creation timestamp (seconds since the UNIX epoch). Used for
    /// stable ordering when multiple rules match.
    pub created_unix: u64,
}

/// Snapshot of a ratbagd-managed device. The `object_path` is ratbagd's
/// (the daemon doesn't rewrite it) — clients pass it back unchanged on
/// any future per-device call.
///
/// D-Bus signature: `(ossuu)`.
///
/// `name` is the human-readable device name (e.g. `"Logitech G500s"`);
/// `model` is ratbagd's encoded `bustype:vid:pid:version` identifier
/// (e.g. `"usb:046d:c52b:0"`). ratbagd doesn't expose a separate vendor
/// string — vendor lookup from VID is a job for the GUI later.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub object_path: OwnedObjectPath,
    pub name: String,
    pub model: String,
    pub active_profile: u32,
    pub profile_count: u32,
}

/// A game discovered by one of the launcher scanners
/// (`gamerat_gamedb::scan_*`), reduced to its wire-friendly fields.
///
/// `launcher` is a wire-stable lowercase string from
/// [`game_launcher`]; `install_dir`, `executable`, and `app_id_hint`
/// are empty strings when absent (D-Bus has no Option type so the
/// daemon flattens `Option<PathBuf>` / `Option<String>` to empty).
///
/// D-Bus signature: `(ssssss)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct GameEntry {
    /// Launcher-prefixed stable identifier (e.g. `"steam:730"`).
    pub id: String,
    /// Human-readable name (e.g. `"Counter-Strike 2"`).
    pub name: String,
    /// Wire-stable launcher tag — see [`game_launcher`].
    pub launcher: String,
    /// Root installation directory, or empty if unknown.
    pub install_dir: String,
    /// Main executable, or empty if unknown.
    pub executable: String,
    /// Best-guess Wayland `app_id` when this game is focused
    /// (e.g. `"steam_app_730"`), or empty if uncertain.
    pub app_id_hint: String,
}

/// Wire-stable identifiers for [`GameEntry::launcher`]. Treat these
/// as public ABI — never rename, only add.
pub mod game_launcher {
    pub const STEAM: &str = "steam";
    pub const LUTRIS: &str = "lutris";
    pub const HEROIC: &str = "heroic";
    pub const OTHER: &str = "other";
}

/// A user-defined software profile.
///
/// The "what the user wants this profile to mean" record. Lives in
/// user space (persisted by the daemon to
/// `$XDG_CONFIG_HOME/gamerat/profiles.toml`); the daemon never
/// auto-mutates it.
///
/// Phase A scope: DPI only. Button mappings, LED states, report rate
/// land in a later slice.
///
/// D-Bus signature: `(sssssauut)`.
///
/// See [`game_category`] for the wire-stable values of `category`.
/// `inherits_from` is a forward-compat slot for the future
/// equivalence-dedup feature: a game-specific profile that's
/// effectively the same as an agnostic profile can declare it, so
/// the daemon's slot allocator can reuse the agnostic profile's
/// hardware slot rather than writing duplicate bytes. Empty means
/// "no inheritance".
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct GameratProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub inherits_from: String,
    pub dpi: Vec<u32>,
    pub active_dpi_stage: u32,
    pub created_unix: u64,
}

/// Wire-stable identifiers for [`GameratProfile::category`]. Treat
/// these as public ABI — never rename, only add.
pub mod game_category {
    /// Reusable across games (e.g., `"fps-low-dpi"`, `"mmo-multi-button"`).
    pub const AGNOSTIC: &str = "agnostic";
    /// Tied to one specific game (e.g., `"cs2"`, `"mw3"`).
    pub const SPECIFIC: &str = "specific";
}

/// One-shot status snapshot of the daemon. Returned by the `Status`
/// method.
///
/// D-Bus signature: `(ssu)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct StatusInfo {
    pub focused_app_id: String,
    pub last_switch_reason: String,
    pub rules_loaded: u32,
}

/// Payload of the `ProfileSwitched` signal. Exists as a struct for the
/// daemon's internal plumbing; the signal itself is declared with
/// flattened args in the XML (D-Bus convention).
///
/// D-Bus signature: `(ouus)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct ProfileSwitchedEvent {
    pub device: OwnedObjectPath,
    pub from_profile: u32,
    pub to_profile: u32,
    pub reason: String,
}

/// Payload of the `FocusChanged` signal. Same flattening convention as
/// [`ProfileSwitchedEvent`].
///
/// D-Bus signature: `(sss)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct FocusChangedEvent {
    pub app_id: String,
    pub title: String,
    pub source: String,
}

/// Wire-stable identifiers for the `source` field of [`FocusChangedEvent`].
/// Treat these as part of the public ABI — never rename, only add.
pub mod focus_source {
    /// Synthetic backend (driven by `SimulateFocus` from the CLI / tests).
    pub const SYNTHETIC: &str = "synthetic";
    /// Real Wayland backend via wlr-foreign-toplevel-management-unstable-v1.
    pub const WLR_FOREIGN_TOPLEVEL: &str = "wlr-foreign-toplevel";
    /// KDE Plasma / `KWin` script bridge.
    pub const KWIN: &str = "kwin";
    /// X11 `_NET_ACTIVE_WINDOW`.
    pub const X11: &str = "x11";
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-Bus signatures must match the strings called out in the
    /// interface XML — these tests prevent silent drift when fields
    /// get added or reordered.
    #[test]
    fn rule_signature_is_sst() {
        assert_eq!(Rule::SIGNATURE.to_string(), "(sst)");
    }

    #[test]
    fn device_info_signature_is_ossuu() {
        assert_eq!(DeviceInfo::SIGNATURE.to_string(), "(ossuu)");
    }

    #[test]
    fn game_entry_signature_is_ssssss() {
        assert_eq!(GameEntry::SIGNATURE.to_string(), "(ssssss)");
    }

    #[test]
    fn game_launcher_constants_are_stable() {
        assert_eq!(game_launcher::STEAM, "steam");
        assert_eq!(game_launcher::LUTRIS, "lutris");
        assert_eq!(game_launcher::HEROIC, "heroic");
        assert_eq!(game_launcher::OTHER, "other");
    }

    #[test]
    fn gamerat_profile_signature_is_sssssauut() {
        assert_eq!(GameratProfile::SIGNATURE.to_string(), "(sssssauut)");
    }

    #[test]
    fn game_category_constants_are_stable() {
        assert_eq!(game_category::AGNOSTIC, "agnostic");
        assert_eq!(game_category::SPECIFIC, "specific");
    }

    #[test]
    fn gamerat_profile_json_round_trip() {
        let profile = GameratProfile {
            id: "fps-low-dpi".to_owned(),
            name: "FPS — low DPI".to_owned(),
            description: "shooter sensitivity baseline".to_owned(),
            category: game_category::AGNOSTIC.to_owned(),
            inherits_from: String::new(),
            dpi: vec![400, 800, 1600],
            active_dpi_stage: 1,
            created_unix: 1_715_000_000,
        };
        let json = serde_json::to_string(&profile).expect("serialize");
        let back: GameratProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(profile, back);
    }

    #[test]
    fn game_entry_json_round_trip() {
        let entry = GameEntry {
            id: "steam:881100".to_owned(),
            name: "Noita".to_owned(),
            launcher: game_launcher::STEAM.to_owned(),
            install_dir: String::new(),
            executable: String::new(),
            app_id_hint: "steam_app_881100".to_owned(),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: GameEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn status_info_signature_is_ssu() {
        assert_eq!(StatusInfo::SIGNATURE.to_string(), "(ssu)");
    }

    #[test]
    fn profile_switched_signature_is_ouus() {
        assert_eq!(ProfileSwitchedEvent::SIGNATURE.to_string(), "(ouus)");
    }

    #[test]
    fn focus_changed_signature_is_sss() {
        assert_eq!(FocusChangedEvent::SIGNATURE.to_string(), "(sss)");
    }

    #[test]
    fn rule_json_round_trip() {
        let rule = Rule {
            app_id_glob: "steam_app_*".to_owned(),
            profile_id: "fps-low-dpi".to_owned(),
            created_unix: 1_715_000_000,
        };
        let json = serde_json::to_string(&rule).expect("serialize");
        let back: Rule = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rule, back);
    }

    #[test]
    fn status_info_json_round_trip() {
        let status = StatusInfo {
            focused_app_id: "org.mozilla.firefox".to_owned(),
            last_switch_reason: "rule:org.mozilla.*".to_owned(),
            rules_loaded: 3,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        let back: StatusInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, back);
    }

    #[test]
    fn focus_source_constants_are_kebab_case() {
        // Sanity-check we don't accidentally rename a wire-stable
        // string — these are part of the public ABI.
        assert_eq!(focus_source::SYNTHETIC, "synthetic");
        assert_eq!(focus_source::WLR_FOREIGN_TOPLEVEL, "wlr-foreign-toplevel");
        assert_eq!(focus_source::KWIN, "kwin");
        assert_eq!(focus_source::X11, "x11");
    }
}
