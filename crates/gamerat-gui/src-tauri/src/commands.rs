//! Tauri IPC commands — thin async wrappers around the daemon proxy.
//!
//! Every command stringifies D-Bus errors at the IPC boundary so the
//! frontend receives `Result<T, string>` via Tauri's invoke channel.

use gamerat_proto::{
    BUS_NAME, ButtonAction, Compat, DeviceInfo, GameEntry, GameratProfile,
    RATBAGD_API_VERSION_EXPECTED, RatbagButton, Rule, SlotInfo, StatusInfo, compat_warning,
};
use serde::Serialize;
use tauri::State;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::zvariant::OwnedObjectPath;

use crate::AppState;

/// Probe whether the gamerat-daemon name is currently claimed on the
/// session bus.
///
/// The GUI's `pingDaemon` calls this every 1.5s while the daemon is
/// offline; we go through `org.freedesktop.DBus.NameHasOwner` rather
/// than poking a method on `AppState`'s proxy because that proxy was
/// built when the daemon may not have been running yet. Asking the
/// session bus directly is cheap (1 round-trip to dbus-broker, no
/// daemon involvement) and always works whatever state our long-lived
/// proxy is in.
///
/// The fresh `Connection::session().await` per call adds a few ms of
/// socket-setup but keeps the probe independent of `AppState` — at
/// the poll rate this is fine, and it sidesteps any future changes
/// to `AppState`'s connection lifetime.
#[tauri::command]
pub async fn daemon_alive() -> Result<bool, String> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| format!("session bus: {e}"))?;
    let dbus = DBusProxy::new(&conn)
        .await
        .map_err(|e| format!("DBusProxy::new: {e}"))?;
    let name =
        BusName::try_from(BUS_NAME).map_err(|e| format!("invalid bus name {BUS_NAME}: {e}"))?;
    dbus.name_has_owner(name)
        .await
        .map_err(|e| format!("NameHasOwner: {e}"))
}

/// Frontend-friendly summary of [`Compat`]. `kind` is a discriminated
/// union tag the UI can switch on without translating Rust enums.
#[derive(Clone, Debug, Serialize)]
pub struct RatbagdCompatInfo {
    pub kind: &'static str,
    pub api_version: Option<i32>,
    pub expected: i32,
    pub warning: Option<String>,
}

impl RatbagdCompatInfo {
    fn from_compat(c: Option<Compat>) -> Self {
        let Some(compat) = c else {
            return Self {
                kind: "unreachable",
                api_version: None,
                expected: RATBAGD_API_VERSION_EXPECTED,
                warning: Some(
                    "ratbagd isn't running — gamerat-daemon can't apply profiles \
                     until it's started (systemctl start ratbagd)."
                        .to_owned(),
                ),
            };
        };
        let (kind, actual) = match compat {
            Compat::Exact => ("exact", Some(RATBAGD_API_VERSION_EXPECTED)),
            Compat::KnownCompat { actual } => ("known_compat", Some(actual)),
            Compat::BelowMin { actual, .. } => ("below_min", Some(actual)),
            Compat::AboveKnown { actual, .. } => ("above_known", Some(actual)),
        };
        Self {
            kind,
            api_version: actual,
            expected: RATBAGD_API_VERSION_EXPECTED,
            warning: compat_warning(compat),
        }
    }
}

/// Fetch a one-shot status snapshot from the daemon.
#[tauri::command]
pub async fn status(state: State<'_, AppState>) -> Result<StatusInfo, String> {
    state.proxy.status().await.map_err(|e| e.to_string())
}

/// Fetch the daemon version string.
#[tauri::command]
pub async fn version(state: State<'_, AppState>) -> Result<String, String> {
    state.proxy.version().await.map_err(|e| e.to_string())
}

/// List all loaded rules.
#[tauri::command]
pub async fn list_rules(state: State<'_, AppState>) -> Result<Vec<Rule>, String> {
    state.proxy.list_rules().await.map_err(|e| e.to_string())
}

/// Upsert a rule (replaces any existing rule with the same glob).
/// `profile_id` references a `GameratProfile` — see `list_profiles`.
#[tauri::command]
pub async fn set_rule(
    state: State<'_, AppState>,
    app_id_glob: String,
    profile_id: String,
) -> Result<(), String> {
    state
        .proxy
        .set_rule(&app_id_glob, &profile_id)
        .await
        .map_err(|e| e.to_string())
}

// ─── Profile CRUD ───────────────────────────────────────────────────

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<GameratProfile>, String> {
    state.proxy.list_profiles().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>, id: String) -> Result<GameratProfile, String> {
    state
        .proxy
        .get_profile(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_profile(
    state: State<'_, AppState>,
    profile: GameratProfile,
) -> Result<(), String> {
    state
        .proxy
        .set_profile(profile)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .proxy
        .delete_profile(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Force a gamerat profile onto the device. Bypasses focus rules
/// and the autoswitch flag — drives the daemon's manual-apply
/// path. Used by the GUI's manual-mode Apply button.
#[tauri::command]
pub async fn apply_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .proxy
        .apply_profile(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Per-slot snapshot for a device — which gamerat profile (if any)
/// occupies each hardware slot, which is currently active, which
/// is reserved as the Desktop. Drives the `DevicesPanel` slot map.
#[tauri::command]
pub async fn get_slot_map(
    state: State<'_, AppState>,
    device_path: String,
) -> Result<Vec<SlotInfo>, String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .get_slot_map(path)
        .await
        .map_err(|e| e.to_string())
}

/// Active DPI stage index on the device's currently-active hardware
/// profile. Polled by `MouseView` so on-mouse DPI cycles update the
/// UI without requiring a profile re-select.
#[tauri::command]
pub async fn get_active_dpi_stage(
    state: State<'_, AppState>,
    device_path: String,
) -> Result<u32, String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .get_active_dpi_stage(path)
        .await
        .map_err(|e| e.to_string())
}

/// Force the device back to its reserved Desktop slot. Manual-mode
/// "Apply Base" — idempotent if Desktop is already active.
#[tauri::command]
pub async fn apply_base(state: State<'_, AppState>) -> Result<(), String> {
    state.proxy.apply_base().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_profile_dpi(
    state: State<'_, AppState>,
    device_path: String,
) -> Result<(Vec<u32>, u32), String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .get_active_profile_dpi(path)
        .await
        .map_err(|e| e.to_string())
}

/// Per-resolution-slot "can this slot be hardware-disabled?" — `true`
/// iff the slot declares `RATBAG_RESOLUTION_CAP_DISABLE`. GUI uses this
/// to decide whether shortening the DPI cycle is honest (cap supported
/// → firmware skips removed stages) or merely cosmetic (cap missing →
/// removed stages stay in the cycle).
#[tauri::command]
pub async fn get_dpi_stage_disable_caps(
    state: State<'_, AppState>,
    device_path: String,
) -> Result<Vec<bool>, String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .get_dpi_stage_disable_caps(path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_to_active_profile(
    state: State<'_, AppState>,
    device_path: String,
    dpi: Vec<u32>,
    active_stage: u32,
    buttons: Vec<gamerat_proto::ProfileButton>,
) -> Result<(), String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .apply_to_active_profile(path, dpi, active_stage, buttons)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a rule by its exact glob string.
#[tauri::command]
pub async fn delete_rule(state: State<'_, AppState>, app_id_glob: String) -> Result<(), String> {
    state
        .proxy
        .delete_rule(&app_id_glob)
        .await
        .map_err(|e| e.to_string())
}

/// List all ratbagd-managed devices.
#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    state.proxy.list_devices().await.map_err(|e| e.to_string())
}

/// List games the daemon's launcher scanners discovered at startup
/// (Steam / Lutris / Heroic).
#[tauri::command]
pub async fn list_games(state: State<'_, AppState>) -> Result<Vec<GameEntry>, String> {
    state.proxy.list_games().await.map_err(|e| e.to_string())
}

/// Inject a synthetic focus event into the daemon.
#[tauri::command]
pub async fn simulate_focus(
    state: State<'_, AppState>,
    app_id: String,
    title: String,
) -> Result<(), String> {
    state
        .proxy
        .simulate_focus(&app_id, &title)
        .await
        .map_err(|e| e.to_string())
}

/// Snapshot every button on a device profile. `profile_index`
/// is the hardware slot index; pass `u32::MAX` to mean "currently
/// active profile" (matches the daemon-side convention).
#[tauri::command]
pub async fn list_buttons(
    state: State<'_, AppState>,
    device_path: String,
    profile_index: u32,
) -> Result<Vec<RatbagButton>, String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .list_buttons(path, profile_index)
        .await
        .map_err(|e| e.to_string())
}

/// Write a binding to one button. Mirrors the CLI's
/// `gameratctl button set` flow. The daemon implicitly commits the
/// change to hardware.
#[tauri::command]
pub async fn set_button(
    state: State<'_, AppState>,
    device_path: String,
    profile_index: u32,
    button_index: u32,
    action: ButtonAction,
) -> Result<(), String> {
    let path =
        OwnedObjectPath::try_from(device_path).map_err(|e| format!("invalid device path: {e}"))?;
    state
        .proxy
        .set_button(path, profile_index, button_index, action)
        .await
        .map_err(|e| e.to_string())
}

/// Read the daemon's autoswitch flag.
#[tauri::command]
pub async fn get_autoswitch(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .proxy
        .auto_switch_enabled()
        .await
        .map_err(|e| e.to_string())
}

/// Flip the daemon's autoswitch flag. Returns the new value.
#[tauri::command]
pub async fn set_autoswitch(state: State<'_, AppState>, value: bool) -> Result<bool, String> {
    state
        .proxy
        .set_auto_switch_enabled(value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

#[tauri::command]
pub async fn get_desktop_return_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .proxy
        .desktop_return_enabled()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_desktop_return_enabled(
    state: State<'_, AppState>,
    value: bool,
) -> Result<bool, String> {
    state
        .proxy
        .set_desktop_return_enabled(value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

#[tauri::command]
pub async fn get_desktop_return_delay_ms(state: State<'_, AppState>) -> Result<u64, String> {
    state
        .proxy
        .desktop_return_delay_ms()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_desktop_return_delay_ms(
    state: State<'_, AppState>,
    value: u64,
) -> Result<u64, String> {
    state
        .proxy
        .set_desktop_return_delay_ms(value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

#[tauri::command]
pub async fn get_notify_on_profile_switch(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .proxy
        .notify_on_profile_switch()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_notify_on_profile_switch(
    state: State<'_, AppState>,
    value: bool,
) -> Result<bool, String> {
    state
        .proxy
        .set_notify_on_profile_switch(value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(value)
}

/// Probe ratbagd's `APIVersion` and classify against the gamerat
/// support range.
///
/// Used by the `StatusCard` to display a compatibility pill. `AppState`
/// is unused here — we hit ratbagd's system-bus surface directly, not
/// the gamerat session-bus proxy.
#[tauri::command]
pub async fn ratbagd_compat() -> Result<RatbagdCompatInfo, String> {
    let probed = gamerat_ratbag::probe_compat()
        .await
        .map_err(|e| e.to_string())?;
    Ok(RatbagdCompatInfo::from_compat(probed))
}
