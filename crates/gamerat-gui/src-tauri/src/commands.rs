//! Tauri IPC commands — thin async wrappers around the daemon proxy.
//!
//! Every command stringifies D-Bus errors at the IPC boundary so the
//! frontend receives `Result<T, string>` via Tauri's invoke channel.

use gamerat_proto::{
    Compat, DeviceInfo, GameEntry, GameratProfile, RATBAGD_API_VERSION_EXPECTED, Rule, StatusInfo,
    compat_warning,
};
use serde::Serialize;
use tauri::State;

use crate::AppState;

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
