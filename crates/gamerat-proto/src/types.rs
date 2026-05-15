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
/// switch the device to `profile_index`.
///
/// D-Bus signature: `(sut)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct Rule {
    /// Glob pattern matched against the focused window's `app_id`.
    /// Syntax follows the [`globset`] crate (`*`, `?`, `[...]`).
    pub app_id_glob: String,
    /// Zero-based index into the device's profile slots.
    pub profile_index: u32,
    /// Creation timestamp (seconds since the UNIX epoch). Used for
    /// stable ordering when multiple rules match.
    pub created_unix: u64,
}

/// Snapshot of a ratbagd-managed device. The `object_path` is ratbagd's
/// (the daemon doesn't rewrite it) — clients pass it back unchanged on
/// any future per-device call.
///
/// D-Bus signature: `(osssuu)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub object_path: OwnedObjectPath,
    pub name: String,
    pub vendor: String,
    pub model: String,
    pub active_profile: u32,
    pub profile_count: u32,
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
    fn rule_signature_is_sut() {
        assert_eq!(Rule::SIGNATURE.to_string(), "(sut)");
    }

    #[test]
    fn device_info_signature_is_osssuu() {
        assert_eq!(DeviceInfo::SIGNATURE.to_string(), "(osssuu)");
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
            profile_index: 2,
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
