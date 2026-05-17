//! Tauri IPC commands — thin async wrappers around the daemon proxy.
//!
//! Every command stringifies D-Bus errors at the IPC boundary so the
//! frontend receives `Result<T, string>` via Tauri's invoke channel.

use gamerat_proto::{DeviceInfo, GameEntry, GameratProfile, Rule, StatusInfo};
use tauri::State;

use crate::AppState;

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
