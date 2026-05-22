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
/// D-Bus signature: `(ossuuu)`.
///
/// `name` is the human-readable device name (e.g. `"Logitech G500s"`);
/// `model` is ratbagd's encoded `bustype:vid:pid:version` identifier
/// (e.g. `"usb:046d:c52b:0"`). ratbagd doesn't expose a separate vendor
/// string — vendor lookup from VID is a job for the GUI later.
///
/// `max_dpi_stages` is the number of resolution slots each profile
/// on this device exposes (queried at discovery via the active
/// profile's `Resolutions` list length). Same for every profile on
/// a given mouse, so we cache it on the device record rather than
/// per-profile.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub object_path: OwnedObjectPath,
    pub name: String,
    pub model: String,
    pub active_profile: u32,
    pub profile_count: u32,
    pub max_dpi_stages: u32,
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
/// Phase A scope: DPI only. Report rate lands in a later slice.
/// Button mappings + LED states (`buttons`, `leds`) are now part of
/// the profile.
///
/// D-Bus signature: `(sssssauuta(u(uua(uu)))a(uu(uuu)u))`.
///
/// See [`game_category`] for the wire-stable values of `category`.
/// `inherits_from` is a forward-compat slot for the future
/// equivalence-dedup feature: a game-specific profile that's
/// effectively the same as an agnostic profile can declare it, so
/// the daemon's slot allocator can reuse the agnostic profile's
/// hardware slot rather than writing duplicate bytes. Empty means
/// "no inheritance".
///
/// `buttons` is the full list of per-button bindings the profile
/// declares. Self-contained: when the dispatch loop materialises a
/// profile into a hardware slot, every button listed here gets
/// written. Buttons not listed are left alone — but the GUI's
/// profile editor lists every hardware button so in practice the
/// vec is fully populated. `#[serde(default)]` lets older
/// `profiles.toml` files (from before the bindings work) load
/// cleanly with an empty bindings vec.
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
    #[serde(default)]
    pub buttons: Vec<ProfileButton>,
    /// Per-LED state (color / mode / brightness) materialised when this
    /// profile is applied. Same self-contained convention as `buttons`:
    /// the GUI populates every hardware LED the user has chosen to set,
    /// LEDs not listed are left alone. `#[serde(default)]` keeps older
    /// `profiles.toml` files loadable without migration.
    #[serde(default)]
    pub leds: Vec<ProfileLed>,
}

/// One button-binding inside a [`GameratProfile`]. The profile's
/// `buttons` vec carries one of these per hardware button the user
/// has chosen to set.
///
/// D-Bus signature: `(u(uua(uu)))`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct ProfileButton {
    /// Hardware button index (matches ratbagd's `Profile.Buttons`
    /// ordering).
    pub index: u32,
    /// The action to bind when this profile is applied.
    pub action: ButtonAction,
}

/// One LED's per-profile state inside a [`GameratProfile`].
///
/// Maps 1:1 to a `Resolution.Led` proxy under
/// `/org/freedesktop/ratbag1/led/<dev>/p<profile>/l<index>`.
/// `color` is RGB 0–255 per channel; ratbagd clamps + downsamples
/// to the LED's actual `ColorDepth` (1-bit / 8-bit / monochrome).
/// `mode` is one of [`led_mode::*`]; `brightness` is 0–255 with
/// per-device clamp. We persist the color even when `mode == OFF`
/// so that flipping back to a color-driven mode restores the user's
/// last choice rather than resetting to black.
///
/// D-Bus signature: `(uu(uuu)u)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct ProfileLed {
    /// Hardware LED index (matches ratbagd's `Profile.Leds` ordering).
    pub index: u32,
    /// One of [`led_mode::*`].
    pub mode: u32,
    /// `(red, green, blue)` — each channel `0..=255`.
    pub color: (u32, u32, u32),
    /// `0..=255` — clamped by ratbagd to the device's max brightness.
    pub brightness: u32,
}

/// Wire-stable LED mode values. Mirrors libratbag's
/// `enum ratbag_led_mode` (also Piper's `RatbagdLed.Mode`):
///
/// | mode      | value | semantics                           |
/// |-----------|-------|-------------------------------------|
/// | `OFF`     | 0     | LED dark; color/brightness ignored. |
/// | `ON`      | 1     | Solid fixed color.                  |
/// | `CYCLE`   | 2     | Rainbow auto-cycle; color ignored.  |
/// | `BREATHING`| 3    | Fade in/out at color, fixed rate.   |
pub mod led_mode {
    pub const OFF: u32 = 0;
    pub const ON: u32 = 1;
    pub const CYCLE: u32 = 2;
    pub const BREATHING: u32 = 3;
}

/// Wire-stable LED color-depth values.
///
/// Mirrors libratbag's `enum ratbag_led_colordepth`. `MONOCHROME` LEDs
/// ignore the color channels (always render at whatever fixed colour
/// the firmware uses); `RGB_111` rounds each channel to 1-bit (8
/// effective colours); `RGB_888` is the full 24-bit gamut.
pub mod led_color_depth {
    pub const MONOCHROME: u32 = 0;
    pub const RGB_888: u32 = 1;
    pub const RGB_111: u32 = 2;
}

/// One row of the hardware slot map for a device. Returned by the
/// daemon's `GetSlotMap` method and rendered in the GUI to show
/// "which gamerat profile is materialised in which hardware slot".
///
/// `profile_id` / `profile_name` are `""` for empty slots and for
/// the reserved Desktop slot (which the allocator never writes).
///
/// D-Bus signature: `(ussbb)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct SlotInfo {
    pub index: u32,
    pub profile_id: String,
    pub profile_name: String,
    pub is_active: bool,
    pub is_desktop: bool,
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

// ─────────────────────────────────────────────────────────────────────
// Button bindings
// ─────────────────────────────────────────────────────────────────────

/// One step in a recorded macro. Mirrors libratbag's `(uu)` macro
/// event tuple — the daemon converts between this and ratbagd's
/// wire-level `a(uu)` macro value when reading/writing
/// `Button.Mapping`.
///
/// D-Bus signature: `(uu)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct MacroStep {
    /// One of [`macro_event_kind::*`]. Treat any other value as
    /// [`macro_event_kind::NONE`] (libratbag's "ignore this event").
    pub kind: u32,
    /// Linux keycode for `KEY_PRESS` / `KEY_RELEASE`, milliseconds for
    /// `WAIT`, ignored for `NONE`.
    pub value: u32,
}

/// Wire-stable macro event kinds. Mirrors libratbag's
/// `RATBAG_MACRO_EVENT_*` enum (also Piper's `RatbagdMacro.Macro`).
pub mod macro_event_kind {
    pub const NONE: u32 = 0;
    pub const KEY_PRESS: u32 = 1;
    pub const KEY_RELEASE: u32 = 2;
    pub const WAIT: u32 = 3;
}

/// A button action, flattened for D-Bus.
///
/// `kind` is one of [`button_action_kind::*`]; `value` and
/// `macro_steps` are interpreted per `kind`:
///
/// | `kind`              | `value`               | `macro_steps` |
/// |---------------------|-----------------------|-------------|
/// | `NONE`              | ignored               | empty       |
/// | `MOUSE`             | target mouse button   | empty       |
/// | `SPECIAL`           | one of `button_special::*` | empty |
/// | `KEY`               | Linux keycode         | empty       |
/// | `MACRO`             | ignored               | event list  |
///
/// We use a flat struct rather than a tagged enum because D-Bus
/// doesn't have a first-class sum type — emitting tagged enums via
/// `v` (variant) loses Rust-side type safety on the receiver. The
/// constructor helpers ([`Self::none`], [`Self::mouse`], …) make the
/// invariants ergonomic at call sites.
///
/// D-Bus signature: `(uua(uu))`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct ButtonAction {
    pub kind: u32,
    pub value: u32,
    pub macro_steps: Vec<MacroStep>,
}

impl ButtonAction {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            kind: button_action_kind::NONE,
            value: 0,
            macro_steps: Vec::new(),
        }
    }

    #[must_use]
    pub const fn mouse(target: u32) -> Self {
        Self {
            kind: button_action_kind::MOUSE,
            value: target,
            macro_steps: Vec::new(),
        }
    }

    #[must_use]
    pub const fn special(action: u32) -> Self {
        Self {
            kind: button_action_kind::SPECIAL,
            value: action,
            macro_steps: Vec::new(),
        }
    }

    #[must_use]
    pub const fn key(keycode: u32) -> Self {
        Self {
            kind: button_action_kind::KEY,
            value: keycode,
            macro_steps: Vec::new(),
        }
    }

    #[must_use]
    pub const fn macro_action(steps: Vec<MacroStep>) -> Self {
        Self {
            kind: button_action_kind::MACRO,
            value: 0,
            macro_steps: steps,
        }
    }

    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.kind == button_action_kind::NONE
    }
}

/// Wire-stable action kinds. Match libratbag's `RATBAG_BUTTON_ACTION_TYPE_*`
/// enum so a Mapping value pulled from ratbagd round-trips cleanly.
pub mod button_action_kind {
    pub const NONE: u32 = 0;
    /// "Map to mouse button N" — libratbag's `BUTTON`. Renamed here
    /// to avoid the noun clash with our own [`super::RatbagButton`].
    pub const MOUSE: u32 = 1;
    pub const SPECIAL: u32 = 2;
    pub const KEY: u32 = 3;
    pub const MACRO: u32 = 4;
}

/// Wire-stable special-action identifiers. Mirrors Piper's
/// `RatbagdButton.ActionSpecial` and libratbag's
/// `RATBAG_BUTTON_ACTION_SPECIAL_*`. All values are `(1 << 30) + N`.
///
/// Treat as public ABI — append only.
pub mod button_special {
    /// Base bit set on every special action. All other constants are
    /// `BASE + N`. ratbagd uses this prefix so the special-id range
    /// can't collide with raw button indices.
    pub const BASE: u32 = 1 << 30;

    pub const UNKNOWN: u32 = BASE;
    pub const DOUBLECLICK: u32 = BASE + 1;
    pub const WHEEL_LEFT: u32 = BASE + 2;
    pub const WHEEL_RIGHT: u32 = BASE + 3;
    pub const WHEEL_UP: u32 = BASE + 4;
    pub const WHEEL_DOWN: u32 = BASE + 5;
    pub const RATCHET_MODE_SWITCH: u32 = BASE + 6;
    pub const RESOLUTION_CYCLE_UP: u32 = BASE + 7;
    pub const RESOLUTION_CYCLE_DOWN: u32 = BASE + 8;
    pub const RESOLUTION_UP: u32 = BASE + 9;
    pub const RESOLUTION_DOWN: u32 = BASE + 10;
    pub const RESOLUTION_ALTERNATE: u32 = BASE + 11;
    pub const RESOLUTION_DEFAULT: u32 = BASE + 12;
    pub const PROFILE_CYCLE_UP: u32 = BASE + 13;
    pub const PROFILE_CYCLE_DOWN: u32 = BASE + 14;
    pub const PROFILE_UP: u32 = BASE + 15;
    pub const PROFILE_DOWN: u32 = BASE + 16;
    pub const SECOND_MODE: u32 = BASE + 17;
    pub const BATTERY_LEVEL: u32 = BASE + 18;
}

/// One hardware button on a connected device, paired with its current
/// mapping and the set of action kinds the firmware accepts.
///
/// The frontend uses [`Self::supported_action_types`] to gate which
/// editor controls are offered for a given button — some buttons
/// can only be `NONE` + `MOUSE`, others support full macros, etc.
///
/// D-Bus signature: `(u(uua(uu))au)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct RatbagButton {
    pub index: u32,
    pub action: ButtonAction,
    pub supported_action_types: Vec<u32>,
}

/// One hardware LED on a connected device, paired with its current
/// state and the set of modes the firmware accepts.
///
/// Returned by `ListLeds` so the GUI can render the editor with the
/// right capability gates (a monochrome LED hides the color picker;
/// a breathing-only LED greys out the Cycle option, etc.).
///
/// D-Bus signature: `(uu(uuu)uauu)`.
#[derive(Clone, Debug, Eq, PartialEq, Type, Serialize, Deserialize)]
pub struct RatbagLed {
    pub index: u32,
    pub mode: u32,
    pub color: (u32, u32, u32),
    pub brightness: u32,
    /// Subset of [`led_mode::*`] values the firmware accepts on this LED.
    pub supported_modes: Vec<u32>,
    /// One of [`led_color_depth::*`].
    pub color_depth: u32,
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

/// Wire-stable health states for the KDE focus bridge.
///
/// Returned by the daemon's `CheckFocusBridge` / `EnsureKwinFocusBridge`
/// methods. On KDE Plasma the daemon can only observe window focus
/// through the `gamerat-focus` `KWin` script (see `data/kwin-script/`);
/// these states let the GUI tell the user whether that bridge is live.
///
/// Treat these as public ABI — never rename, only add.
pub mod focus_bridge {
    /// KDE session detected and the `gamerat-focus` script is loaded
    /// into the running compositor — focus events flow.
    pub const ACTIVE: &str = "active";
    /// KDE session detected but the script isn't loaded — auto-switch
    /// is inert until it's repaired. This is the state the GUI surfaces
    /// as an actionable error.
    pub const NOT_LOADED: &str = "not-loaded";
    /// Not a KDE/`KWin` session (Sway/Hyprland via wlr, X11, or
    /// synthetic-only) — the bridge concept doesn't apply and the GUI
    /// hides the row entirely.
    pub const NOT_APPLICABLE: &str = "not-applicable";
    /// Couldn't probe `KWin` (the `org.kde.KWin` Scripting call failed).
    /// Shown muted rather than as a hard error.
    pub const UNKNOWN: &str = "unknown";
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
    fn device_info_signature_is_ossuuu() {
        assert_eq!(DeviceInfo::SIGNATURE.to_string(), "(ossuuu)");
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
    fn gamerat_profile_signature_includes_buttons_and_leds() {
        // The trailing arrays are the per-profile button bindings
        // (`a(u(uua(uu)))`) and the per-profile LED state
        // (`a(uu(uuu)u)`). Bumping either is wire-breaking; the daemon
        // / CLI / GUI all ship from this repo together so the breakage
        // is fine.
        assert_eq!(
            GameratProfile::SIGNATURE.to_string(),
            "(sssssauuta(u(uua(uu)))a(uu(uuu)u))",
        );
    }

    #[test]
    fn profile_led_signature_is_uu_uuu_u() {
        assert_eq!(ProfileLed::SIGNATURE.to_string(), "(uu(uuu)u)");
    }

    #[test]
    fn ratbag_led_signature_is_uu_uuu_uauu() {
        assert_eq!(RatbagLed::SIGNATURE.to_string(), "(uu(uuu)uauu)");
    }

    #[test]
    fn led_mode_constants_are_stable() {
        assert_eq!(led_mode::OFF, 0);
        assert_eq!(led_mode::ON, 1);
        assert_eq!(led_mode::CYCLE, 2);
        assert_eq!(led_mode::BREATHING, 3);
    }

    #[test]
    fn led_color_depth_constants_are_stable() {
        assert_eq!(led_color_depth::MONOCHROME, 0);
        assert_eq!(led_color_depth::RGB_888, 1);
        assert_eq!(led_color_depth::RGB_111, 2);
    }

    #[test]
    fn profile_button_signature_round_trips() {
        assert_eq!(ProfileButton::SIGNATURE.to_string(), "(u(uua(uu)))");
    }

    #[test]
    fn slot_info_signature_is_ussbb() {
        assert_eq!(SlotInfo::SIGNATURE.to_string(), "(ussbb)");
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
            buttons: vec![
                ProfileButton {
                    index: 0,
                    action: ButtonAction::mouse(0),
                },
                ProfileButton {
                    index: 3,
                    action: ButtonAction::key(30),
                },
            ],
            leds: vec![ProfileLed {
                index: 0,
                mode: led_mode::ON,
                color: (255, 51, 68),
                brightness: 220,
            }],
        };
        let json = serde_json::to_string(&profile).expect("serialize");
        let back: GameratProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(profile, back);
    }

    #[test]
    fn gamerat_profile_loads_legacy_toml_without_buttons_field() {
        // Profiles persisted before this commit don't have a
        // `buttons` field. `#[serde(default)]` should fill it in
        // as an empty vec so loading existing profiles.toml stays
        // forward-compatible.
        let legacy = r#"{
            "id": "old",
            "name": "Old",
            "description": "",
            "category": "agnostic",
            "inherits_from": "",
            "dpi": [800],
            "active_dpi_stage": 0,
            "created_unix": 0
        }"#;
        let parsed: GameratProfile = serde_json::from_str(legacy).expect("legacy load");
        assert!(parsed.buttons.is_empty());
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
    fn macro_step_signature_is_uu() {
        assert_eq!(MacroStep::SIGNATURE.to_string(), "(uu)");
    }

    #[test]
    fn button_action_signature_is_uua_uu() {
        // Flat shape: kind, value, list of (kind, value) macro steps.
        assert_eq!(ButtonAction::SIGNATURE.to_string(), "(uua(uu))");
    }

    #[test]
    fn ratbag_button_signature_round_trips() {
        // Spelt out so the test fails loudly if we change a field in
        // a wire-incompatible way.
        assert_eq!(RatbagButton::SIGNATURE.to_string(), "(u(uua(uu))au)");
    }

    #[test]
    fn button_action_constructors_set_kind_and_value() {
        assert_eq!(ButtonAction::none().kind, button_action_kind::NONE);
        assert!(ButtonAction::none().macro_steps.is_empty());

        let m = ButtonAction::mouse(3);
        assert_eq!(m.kind, button_action_kind::MOUSE);
        assert_eq!(m.value, 3);

        let s = ButtonAction::special(button_special::WHEEL_LEFT);
        assert_eq!(s.kind, button_action_kind::SPECIAL);
        assert_eq!(s.value, button_special::WHEEL_LEFT);

        let k = ButtonAction::key(30);
        assert_eq!(k.kind, button_action_kind::KEY);
        assert_eq!(k.value, 30);

        let m = ButtonAction::macro_action(vec![
            MacroStep {
                kind: macro_event_kind::KEY_PRESS,
                value: 30,
            },
            MacroStep {
                kind: macro_event_kind::WAIT,
                value: 10,
            },
            MacroStep {
                kind: macro_event_kind::KEY_RELEASE,
                value: 30,
            },
        ]);
        assert_eq!(m.kind, button_action_kind::MACRO);
        assert_eq!(m.macro_steps.len(), 3);
    }

    #[test]
    fn button_action_kind_constants_match_libratbag() {
        // These line up with libratbag's RATBAG_BUTTON_ACTION_TYPE_*.
        // Reordering would break compatibility with ratbagd.
        assert_eq!(button_action_kind::NONE, 0);
        assert_eq!(button_action_kind::MOUSE, 1);
        assert_eq!(button_action_kind::SPECIAL, 2);
        assert_eq!(button_action_kind::KEY, 3);
        assert_eq!(button_action_kind::MACRO, 4);
    }

    #[test]
    fn button_special_constants_match_piper() {
        // Spot-check a few — full list is Piper's RatbagdButton.ActionSpecial.
        assert_eq!(button_special::BASE, 1 << 30);
        assert_eq!(button_special::DOUBLECLICK, (1 << 30) + 1);
        assert_eq!(button_special::WHEEL_DOWN, (1 << 30) + 5);
        assert_eq!(button_special::RESOLUTION_CYCLE_UP, (1 << 30) + 7);
        assert_eq!(button_special::BATTERY_LEVEL, (1 << 30) + 18);
    }

    #[test]
    fn macro_event_kind_constants_match_libratbag() {
        assert_eq!(macro_event_kind::NONE, 0);
        assert_eq!(macro_event_kind::KEY_PRESS, 1);
        assert_eq!(macro_event_kind::KEY_RELEASE, 2);
        assert_eq!(macro_event_kind::WAIT, 3);
    }

    #[test]
    fn ratbag_button_json_round_trip() {
        let button = RatbagButton {
            index: 3,
            action: ButtonAction::macro_action(vec![
                MacroStep {
                    kind: macro_event_kind::KEY_PRESS,
                    value: 30,
                },
                MacroStep {
                    kind: macro_event_kind::KEY_RELEASE,
                    value: 30,
                },
            ]),
            supported_action_types: vec![0, 1, 2, 3, 4],
        };
        let json = serde_json::to_string(&button).expect("serialize");
        let back: RatbagButton = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(button, back);
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
