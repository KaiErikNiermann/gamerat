//! Tauri backend for the gamerat GUI.
//!
//! Holds a single shared [`GameRatProxy`] for the app lifetime (behind
//! [`tauri::State`]). All Tauri commands live in [`commands`]; this module
//! owns the app setup, signal-forwarding task, and the `AppState` type.
//!
//! Signal forwarding: the `setup()` hook spawns a Tokio task that selects on
//! the two signal streams and forwards each arrival as a Tauri event:
//!   - `"focus-changed"` → [`FocusChangedPayload`]
//!   - `"profile-switched"` → [`ProfileSwitchedPayload`]

// Tauri entry-point convention: bail loudly on launch failure.
#![allow(clippy::expect_used)]

pub mod commands;

use std::sync::Arc;

use anyhow::Context as _;
use futures::StreamExt as _;
use gamerat_proto::{
    ActiveDpiStageChangedArgs, FocusChangedArgs, GameRatProxy, PanicHatchSettledArgs,
    ProfileSwitchedArgs, ProfileSwitchingArgs,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, Manager as _};
use tracing::error;
use zbus::Connection;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Shared D-Bus proxy, held for the app lifetime.
///
/// `GameRatProxy::new(&conn)` clones the underlying `Connection` into
/// the proxy (zbus 5's `Proxy` owns its connection via an internal
/// `Arc`), so the proxy is `'static` without leaking and `Send +
/// Sync` is auto-derived — no `unsafe` needed.
#[derive(Clone, Debug)]
pub struct AppState {
    pub proxy: Arc<GameRatProxy<'static>>,
}

// ---------------------------------------------------------------------------
// IPC payloads (Tauri events sent from Rust → frontend)
// ---------------------------------------------------------------------------

/// Payload for the `"focus-changed"` Tauri event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusChangedPayload {
    pub app_id: String,
    pub title: String,
    pub source: String,
}

/// Payload for the `"profile-switched"` Tauri event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSwitchedPayload {
    pub device: String,
    pub from_profile: u32,
    pub to_profile: u32,
    pub reason: String,
}

/// Payload for the `"profile-switching"` Tauri event (emitted before
/// the daemon writes to the device, so the GUI can flash a
/// "switching…" indicator over the firmware-jitter window).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSwitchingPayload {
    pub device: String,
    pub to_profile: u32,
    pub reason: String,
}

/// Payload for the `"active-dpi-stage-changed"` Tauri event — the
/// daemon's DPI tracker observed a live cycle change on the device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveDpiStageChangedPayload {
    pub device: String,
    pub stage: u32,
}

/// Payload for the `"panic-hatch-settled"` Tauri event — the daemon's
/// auto-disable timer fired, was cancelled, or was superseded by an
/// unrelated rebind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanicHatchSettledPayload {
    pub device: String,
    pub button: u32,
    pub outcome: String,
}

// ---------------------------------------------------------------------------
// Signal forwarding
// ---------------------------------------------------------------------------

/// Spawns a Tokio task that drives the two signal streams and emits Tauri
/// events for each arrival. The task runs until both streams close or the app
/// exits (dropping the `AppHandle` unregisters all listeners).
#[allow(clippy::too_many_lines)] // Forwarder grows linearly with signal count; splitting hurts readability.
async fn spawn_signal_forwarder(app: AppHandle, proxy: Arc<GameRatProxy<'static>>) {
    let mut focus_stream = match proxy.receive_focus_changed().await {
        Ok(s) => s,
        Err(e) => {
            error!("failed to subscribe to FocusChanged: {e}");
            return;
        }
    };

    let mut switched_stream = match proxy.receive_profile_switched().await {
        Ok(s) => s,
        Err(e) => {
            error!("failed to subscribe to ProfileSwitched: {e}");
            return;
        }
    };

    let mut switching_stream = match proxy.receive_profile_switching().await {
        Ok(s) => s,
        Err(e) => {
            error!("failed to subscribe to ProfileSwitching: {e}");
            return;
        }
    };

    let mut dpi_stream = match proxy.receive_active_dpi_stage_changed().await {
        Ok(s) => s,
        Err(e) => {
            error!("failed to subscribe to ActiveDpiStageChanged: {e}");
            return;
        }
    };

    let mut panic_stream = match proxy.receive_panic_hatch_settled().await {
        Ok(s) => s,
        Err(e) => {
            error!("failed to subscribe to PanicHatchSettled: {e}");
            return;
        }
    };

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(signal) = focus_stream.next() => {
                    match signal.args() {
                        Ok(args) => {
                            let args: FocusChangedArgs<'_> = args;
                            let payload = FocusChangedPayload {
                                app_id: args.app_id.to_owned(),
                                title: args.title.to_owned(),
                                source: args.source.to_owned(),
                            };
                            if let Err(e) = app.emit("focus-changed", &payload) {
                                error!("emit focus-changed failed: {e}");
                            }
                        }
                        Err(e) => error!("decode FocusChanged args: {e}"),
                    }
                }
                Some(signal) = switching_stream.next() => {
                    match signal.args() {
                        Ok(args) => {
                            let args: ProfileSwitchingArgs<'_> = args;
                            let payload = ProfileSwitchingPayload {
                                device: args.device.as_str().to_owned(),
                                to_profile: args.to_profile,
                                reason: args.reason.to_owned(),
                            };
                            if let Err(e) = app.emit("profile-switching", &payload) {
                                error!("emit profile-switching failed: {e}");
                            }
                        }
                        Err(e) => error!("decode ProfileSwitching args: {e}"),
                    }
                }
                Some(signal) = switched_stream.next() => {
                    match signal.args() {
                        Ok(args) => {
                            let args: ProfileSwitchedArgs<'_> = args;
                            let payload = ProfileSwitchedPayload {
                                device: args.device.as_str().to_owned(),
                                from_profile: args.from_profile,
                                to_profile: args.to_profile,
                                reason: args.reason.to_owned(),
                            };
                            if let Err(e) = app.emit("profile-switched", &payload) {
                                error!("emit profile-switched failed: {e}");
                            }
                        }
                        Err(e) => error!("decode ProfileSwitched args: {e}"),
                    }
                }
                Some(signal) = dpi_stream.next() => {
                    match signal.args() {
                        Ok(args) => {
                            let args: ActiveDpiStageChangedArgs<'_> = args;
                            let payload = ActiveDpiStageChangedPayload {
                                device: args.device.as_str().to_owned(),
                                stage: args.stage,
                            };
                            if let Err(e) = app.emit("active-dpi-stage-changed", &payload) {
                                error!("emit active-dpi-stage-changed failed: {e}");
                            }
                        }
                        Err(e) => error!("decode ActiveDpiStageChanged args: {e}"),
                    }
                }
                Some(signal) = panic_stream.next() => {
                    match signal.args() {
                        Ok(args) => {
                            let args: PanicHatchSettledArgs<'_> = args;
                            let payload = PanicHatchSettledPayload {
                                device: args.device.as_str().to_owned(),
                                button: args.button,
                                outcome: args.outcome.to_owned(),
                            };
                            if let Err(e) = app.emit("panic-hatch-settled", &payload) {
                                error!("emit panic-hatch-settled failed: {e}");
                            }
                        }
                        Err(e) => error!("decode PanicHatchSettled args: {e}"),
                    }
                }
                else => {
                    error!("signal streams all closed — forwarder exiting");
                    break;
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialise tracing early (before any async work).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Build the D-Bus connection + proxy on Tauri's Tokio runtime.
            // block_on is safe here: setup() runs before the event loop, so
            // there's no risk of a cross-runtime deadlock. The local `conn`
            // is dropped at the end of the async block — `GameRatProxy::new`
            // clones it internally, so the proxy keeps the wire alive.
            let proxy: Arc<GameRatProxy<'static>> = tauri::async_runtime::block_on(async {
                let conn = Connection::session()
                    .await
                    .context("opening D-Bus session bus")?;
                // Disable property caching. The GUI is allowed to
                // outlive a daemon-down → daemon-up cycle (see the
                // DaemonGate modal); with the default `CacheProperties::Yes`,
                // the initial GetAll on a missing daemon leaves the
                // cache empty and *every subsequent* property read
                // returns `ServiceUnknown` — even after the daemon
                // is back. With caching off, each property read is
                // a fresh Properties.Get over the wire, so the GUI
                // recovers automatically once the daemon registers
                // its name.
                let proxy = GameRatProxy::builder(&conn)
                    .cache_properties(zbus::proxy::CacheProperties::No)
                    .build()
                    .await
                    .context("connecting to gamerat daemon (is it running?)")?;
                anyhow::Ok(Arc::new(proxy))
            })
            .expect("failed to connect to gamerat daemon");

            // Kick off the signal-forwarding background task.
            let proxy_clone = Arc::clone(&proxy);
            tauri::async_runtime::spawn(async move {
                spawn_signal_forwarder(app_handle, proxy_clone).await;
            });

            app.manage(AppState { proxy });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::version,
            commands::check_focus_bridge,
            commands::ensure_kwin_focus_bridge,
            commands::list_rules,
            commands::set_rule,
            commands::delete_rule,
            commands::list_devices,
            commands::list_games,
            commands::list_profiles,
            commands::get_profile,
            commands::set_profile,
            commands::delete_profile,
            commands::simulate_focus,
            commands::ratbagd_compat,
            commands::list_buttons,
            commands::set_button,
            commands::check_macro_balance,
            commands::panic_hatch,
            commands::cancel_panic_hatch,
            commands::list_leds,
            commands::set_led,
            commands::apply_profile,
            commands::apply_base,
            commands::get_slot_map,
            commands::get_active_dpi_stage,
            commands::get_active_profile_dpi,
            commands::get_dpi_stage_disable_caps,
            commands::apply_to_active_profile,
            commands::get_autoswitch,
            commands::set_autoswitch,
            commands::get_desktop_return_enabled,
            commands::set_desktop_return_enabled,
            commands::get_desktop_return_delay_ms,
            commands::set_desktop_return_delay_ms,
            commands::get_notify_on_profile_switch,
            commands::set_notify_on_profile_switch,
            commands::get_software_macros_enabled,
            commands::set_software_macros_enabled,
            commands::fetch_soft_input_state,
            commands::daemon_alive,
        ])
        .run(tauri::generate_context!())
        .expect("error while running gamerat-gui tauri app");
}
