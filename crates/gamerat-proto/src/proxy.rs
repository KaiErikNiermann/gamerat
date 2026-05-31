//! Generated-style proxy trait for the `org.appulsauce.GameRat1`
//! interface. Hand-mirrors `data/dbus/org.appulsauce.GameRat1.xml` —
//! keep them in sync.
//!
//! Clients (CLI, GUI, tests) use [`GameRatProxy`] to call the daemon;
//! the daemon implements the same surface on the server side via
//! `zbus::interface` (not declared here).

use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

use crate::types::{
    ButtonAction, DeviceInfo, GameEntry, GameratProfile, MacroStep, ProfileLed, RatbagButton,
    RatbagLed, Rule, SlotInfo, StatusInfo,
};

#[proxy(
    interface = "org.appulsauce.GameRat1",
    default_service = "org.appulsauce.GameRat1",
    default_path = "/org/appulsauce/GameRat1",
    gen_blocking = false
)]
pub trait GameRat {
    /// Inject a synthetic focus event. The daemon processes it
    /// identically to one coming from a real focus backend. Source
    /// label on `FocusChanged` is `synthetic`.
    fn simulate_focus(&self, app_id: &str, title: &str) -> zbus::Result<()>;

    /// Bridge entrypoint for the `KWin` Script. Called by
    /// `data/kwin-script/gamerat-focus/contents/code/main.js` whenever
    /// `workspace.windowActivated` fires. Source label on
    /// `FocusChanged` is `kwin`.
    fn ingest_kwin_focus(&self, app_id: &str, title: &str) -> zbus::Result<()>;

    /// Upsert a rule. Replaces any existing rule with the same glob.
    /// `profile_id` references a [`GameratProfile`] stored by the
    /// daemon; missing references are accepted but the rule will be
    /// inert until the profile is created.
    fn set_rule(&self, app_id_glob: &str, profile_id: &str) -> zbus::Result<()>;

    /// Remove the rule matching `app_id_glob`. No-op if absent.
    fn delete_rule(&self, app_id_glob: &str) -> zbus::Result<()>;

    /// Enumerate all loaded rules.
    fn list_rules(&self) -> zbus::Result<Vec<Rule>>;

    /// Enumerate ratbagd-managed devices the daemon currently sees.
    fn list_devices(&self) -> zbus::Result<Vec<DeviceInfo>>;

    /// Snapshot button bindings for a profile on the given device.
    /// Pass `profile_index = u32::MAX` to mean "currently active
    /// profile" — saves a roundtrip when the caller doesn't already
    /// know which slot is active.
    fn list_buttons(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
    ) -> zbus::Result<Vec<RatbagButton>>;

    /// Write a binding to one button. Same `profile_index = u32::MAX`
    /// shortcut. Implicitly commits to hardware.
    fn set_button(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
        button_index: u32,
        action: ButtonAction,
    ) -> zbus::Result<()>;

    /// Pure analysis: which keycodes does this macro leave pressed
    /// at end-of-sequence? Used by the GUI's binding editor to warn
    /// before saving an unbalanced macro that would stick a key down
    /// at the OS level. See [`crate::macro_balance`] for semantics.
    fn check_macro_balance(&self, steps: Vec<MacroStep>) -> zbus::Result<Vec<u32>>;

    /// Recover from a stuck-key situation: read `button`'s current
    /// binding, identify any unbalanced presses, rebind to a release-
    /// only macro (returns the affected keycodes + `awaiting_press =
    /// true`) and arm a 5-second auto-disable timer. If the binding
    /// wasn't an unbalanced macro the daemon goes straight to NONE
    /// and returns an empty list + `awaiting_press = false`.
    /// Listen for [`Self::panic_hatch_settled`] to know when the
    /// timer fires or [`Self::cancel_panic_hatch`] aborts it.
    fn panic_hatch(&self, device: OwnedObjectPath, button: u32) -> zbus::Result<(Vec<u32>, bool)>;

    /// Abort a pending panic-hatch auto-disable timer (the user
    /// dismissed the GUI countdown / pressed Ctrl-C in the CLI).
    /// Idempotent. Emits `PanicHatchSettled(_, _, "cancelled")` if a
    /// timer was armed.
    fn cancel_panic_hatch(&self, device: OwnedObjectPath, button: u32) -> zbus::Result<()>;

    /// Snapshot LED state for a profile on the given device. Same
    /// `profile_index = u32::MAX` shortcut as `list_buttons`. Returns
    /// an empty Vec for devices whose driver doesn't expose LEDs.
    fn list_leds(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
    ) -> zbus::Result<Vec<RatbagLed>>;

    /// Write one LED's mode + color + brightness. Same
    /// `profile_index = u32::MAX` shortcut. Implicitly commits.
    fn set_led(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
        led_index: u32,
        led: ProfileLed,
    ) -> zbus::Result<()>;

    /// Enumerate games: launcher-scanned entries (refreshed at startup
    /// and on [`Self::rescan_games`]) merged with user-added manual
    /// entries. Deduplicated by `id`.
    fn list_games(&self) -> zbus::Result<Vec<GameEntry>>;

    /// Re-run the launcher scanners live and replace the cached scanned
    /// set, then return the merged list (scanned ∪ manual). Lets the
    /// GUI recover from a stale startup scan — e.g. a Steam library on
    /// a drive that wasn't mounted when the daemon launched — without a
    /// daemon restart.
    fn rescan_games(&self) -> zbus::Result<Vec<GameEntry>>;

    /// Register a manual game entry for folders the scanners can't
    /// find. `app_id_hint` is the window match (Wayland `app_id` /
    /// `WM_CLASS`) used to seed a rule; `install_dir` is informational.
    /// Returns the created entry (with its generated `manual:<slug>`
    /// id). Persisted to `manual-games.toml`.
    fn add_manual_game(
        &self,
        name: &str,
        install_dir: &str,
        app_id_hint: &str,
    ) -> zbus::Result<GameEntry>;

    /// Remove a manual game entry by its `id`. No-op (Ok) if absent or
    /// if the id isn't a manual entry.
    fn remove_manual_game(&self, id: &str) -> zbus::Result<()>;

    /// List every user-defined software profile.
    fn list_profiles(&self) -> zbus::Result<Vec<GameratProfile>>;

    /// Fetch one profile by id. Returns a D-Bus error if absent.
    fn get_profile(&self, id: &str) -> zbus::Result<GameratProfile>;

    /// Upsert a profile (replaces any existing profile with the same id).
    fn set_profile(&self, profile: GameratProfile) -> zbus::Result<()>;

    /// Remove a profile by id. No-op if absent.
    fn delete_profile(&self, id: &str) -> zbus::Result<()>;

    /// Force the named profile onto the device, bypassing focus
    /// rules and autoswitch state. Used by manual-mode Apply in the
    /// GUI and by `gameratctl profile apply`.
    fn apply_profile(&self, profile_id: &str) -> zbus::Result<()>;

    /// Per-slot view for a device: which gamerat profile (if any)
    /// occupies each hardware slot, which slot is currently active,
    /// which is the reserved Desktop.
    fn get_slot_map(&self, device_path: OwnedObjectPath) -> zbus::Result<Vec<SlotInfo>>;

    /// Active DPI stage index of the device's currently-active hardware
    /// profile. Used by the GUI to keep the DPI-stage indicator in sync
    /// after a DPI-cycle / DPI-up / DPI-down press on the mouse.
    fn get_active_dpi_stage(&self, device_path: OwnedObjectPath) -> zbus::Result<u32>;

    /// Force the device back to the reserved Desktop slot (baseline
    /// bindings). Used by the GUI's manual-mode "Apply Base"
    /// affordance. Idempotent if Desktop is already active.
    fn apply_base(&self) -> zbus::Result<()>;

    /// DPI stages + active stage index of the device's currently-active
    /// hardware profile. Pairs with `apply_to_active_profile` so the
    /// GUI's Base-mode editor can read and write live hardware DPI
    /// without going through a gamerat profile record.
    fn get_active_profile_dpi(&self, device_path: OwnedObjectPath)
    -> zbus::Result<(Vec<u32>, u32)>;

    /// Like [`Self::get_active_profile_dpi`] but for an arbitrary slot.
    /// The GUI's Profiles panel uses this to render the Base row's
    /// DPI summary regardless of which slot is currently active.
    fn get_profile_dpi(
        &self,
        device_path: OwnedObjectPath,
        slot_index: u32,
    ) -> zbus::Result<(Vec<u32>, u32)>;

    /// Per-resolution-slot answer to "can this slot be hardware-disabled?".
    /// Returned vector matches the device's DPI slot count; entry `i` is
    /// `true` iff resolution slot `i` declares
    /// `RATBAG_RESOLUTION_CAP_DISABLE`. GUI uses this to decide whether
    /// the "− stage" / shorten-cycle affordance is honest (cap everywhere
    /// → firmware really skips removed stages) or merely cosmetic (cap
    /// missing → extra stages stay in the hardware cycle even after
    /// shortening the profile).
    fn get_dpi_stage_disable_caps(&self, device_path: OwnedObjectPath) -> zbus::Result<Vec<bool>>;

    /// Write a full set of DPI stages + button bindings + LED state
    /// to the currently-active hardware profile. Same batched commit
    /// as `apply_profile_complete` — one round-trip, one jitter.
    /// Either `buttons` or `leds` may be empty to leave that section
    /// untouched.
    fn apply_to_active_profile(
        &self,
        device_path: OwnedObjectPath,
        dpi: Vec<u32>,
        active_stage: u32,
        buttons: Vec<crate::types::ProfileButton>,
        leds: Vec<ProfileLed>,
    ) -> zbus::Result<()>;

    /// Write DPI + buttons + LEDs to an arbitrary slot on the device.
    /// Sibling of [`Self::apply_to_active_profile`] but targets a slot
    /// the caller picks. Bypasses the slot allocator's cache — pair
    /// with [`Self::wipe_gamerat_state`] when used as part of the
    /// "Purge & reset device" flow so the cache doesn't drift.
    ///
    /// Internally calls `Profile.SetActive` + `Device.Commit`, so the
    /// device ends up activated on `slot_index` after this returns.
    fn write_slot_content(
        &self,
        device_path: OwnedObjectPath,
        slot_index: u32,
        dpi: Vec<u32>,
        active_stage: u32,
        buttons: Vec<crate::types::ProfileButton>,
        leds: Vec<ProfileLed>,
    ) -> zbus::Result<()>;

    /// Wipe the gamerat-side profile store and slot-allocator cache,
    /// both on disk and in-memory. Does NOT touch the hardware — pair
    /// with per-slot [`Self::write_slot_content`] calls when the goal
    /// is "device + gamerat both back to factory state".
    ///
    /// Rules are not wiped (they're independent of devices). The next
    /// focus event / device connect triggers a fresh auto-import from
    /// whatever content is currently on the hardware.
    fn wipe_gamerat_state(&self) -> zbus::Result<()>;

    /// Force a re-read of one hardware slot and overwrite the matching
    /// `imported-slot-N` gamerat profile. Bypasses the
    /// allocator-already-owns-this check that the on-connect
    /// auto-import honours — used by `gameratctl device import-slot`
    /// to refresh an imported profile after an external tool
    /// rewrites the slot.
    fn reimport_slot(&self, device_path: OwnedObjectPath, slot_index: u32) -> zbus::Result<()>;

    /// One-shot status snapshot.
    fn status(&self) -> zbus::Result<StatusInfo>;

    /// Probe the KDE focus-bridge health without changing anything.
    /// Returns one of [`crate::focus_bridge`]: `active`, `not-loaded`,
    /// `not-applicable` (non-KDE session), or `unknown` (probe failed).
    /// Cheap — one `org.kde.KWin` round-trip on KDE, a single
    /// `NameHasOwner` elsewhere.
    fn check_focus_bridge(&self) -> zbus::Result<String>;

    /// Ensure the `gamerat-focus` `KWin` script is installed, enabled in
    /// `kwinrc`, and loaded into the running compositor (idempotent).
    /// Returns the resulting [`crate::focus_bridge`] state. No-op
    /// returning `not-applicable` off KDE. Backs the GUI's "Repair"
    /// button; the daemon also runs this once at startup.
    fn ensure_kwin_focus_bridge(&self) -> zbus::Result<String>;

    /// Emitted *before* the daemon writes the new profile to the
    /// device (i.e. before the `Commit` round-trip during which the
    /// firmware briefly reconfigures and the mouse jitters). The GUI
    /// uses this to surface a "switching…" indicator so the visible
    /// hardware jitter reads as expected, not broken.
    #[zbus(signal)]
    fn profile_switching(
        &self,
        device: OwnedObjectPath,
        to_profile: u32,
        reason: &str,
    ) -> zbus::Result<()>;

    /// Emitted by the daemon's DPI tracker whenever the device's
    /// live active DPI stage changes — either because the user
    /// pressed DPI-up / DPI-down / DPI-cycle on the mouse itself or
    /// because something explicitly wrote a new active stage.
    ///
    /// Requires the libratbag `RefreshActive` patch in
    /// `patches/libratbag/`; without it ratbagd can't observe
    /// firmware-internal cycles and the tracker stays silent.
    #[zbus(signal)]
    fn active_dpi_stage_changed(&self, device: OwnedObjectPath, stage: u32) -> zbus::Result<()>;

    /// Emitted after the daemon successfully writes `ActiveProfile` and
    /// `Commit`s on the device.
    #[zbus(signal)]
    fn profile_switched(
        &self,
        device: OwnedObjectPath,
        from_profile: u32,
        to_profile: u32,
        reason: &str,
    ) -> zbus::Result<()>;

    /// Emitted on every focus event, whether or not a rule matched.
    #[zbus(signal)]
    fn focus_changed(&self, app_id: &str, title: &str, source: &str) -> zbus::Result<()>;

    /// Emitted when a pending [`Self::panic_hatch`] resolves —
    /// `outcome` is one of `"timeout_disabled"` (5s expired and the
    /// daemon nulled the binding), `"cancelled"`
    /// ([`Self::cancel_panic_hatch`] aborted it), or `"superseded"`
    /// (the user rebound the button in the meantime, so the daemon
    /// left it alone).
    #[zbus(signal)]
    fn panic_hatch_settled(
        &self,
        device: OwnedObjectPath,
        button: u32,
        outcome: &str,
    ) -> zbus::Result<()>;

    /// Daemon version string (`CARGO_PKG_VERSION`).
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;

    /// When `false`, the dispatch loop emits `FocusChanged` but
    /// suppresses the rule-driven profile switch. Profile changes
    /// become purely manual (CLI / GUI). Persisted in the daemon's
    /// settings file.
    #[zbus(property)]
    fn auto_switch_enabled(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_auto_switch_enabled(&self, value: bool) -> zbus::Result<()>;

    /// When `false`, the dispatch loop skips the Desktop fallback on
    /// no-rule-match focus events — the current profile stays active.
    #[zbus(property)]
    fn desktop_return_enabled(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_desktop_return_enabled(&self, value: bool) -> zbus::Result<()>;

    /// Debounce window (milliseconds) before the Desktop fallback
    /// fires after a no-rule-match focus event. `0` means fire
    /// immediately (legacy behaviour).
    #[zbus(property)]
    fn desktop_return_delay_ms(&self) -> zbus::Result<u64>;

    #[zbus(property)]
    fn set_desktop_return_delay_ms(&self, value: u64) -> zbus::Result<()>;

    /// When `true`, the GUI raises a system notification each time a
    /// profile switch lands. Off by default.
    #[zbus(property)]
    fn notify_on_profile_switch(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_notify_on_profile_switch(&self, value: bool) -> zbus::Result<()>;

    /// Master opt-in for the software-input pipeline backing soft-
    /// macros (toggle, future auto-fire). Default `false`. Toggling
    /// takes effect on the next daemon start.
    #[zbus(property)]
    fn software_macros_enabled(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn set_software_macros_enabled(&self, value: bool) -> zbus::Result<()>;

    /// Snapshot of the software-input subsystem's runtime state. One
    /// of `disabled` / `active` / `unavailable` (matches the wire
    /// constants in `gamerat_daemon::soft_macros::soft_input_state`).
    fn soft_input_state(&self) -> zbus::Result<String>;
}
