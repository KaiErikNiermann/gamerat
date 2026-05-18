//! Background task that polls each connected ratbagd device for its
//! live active DPI stage, emitting `ActiveDpiStageChanged` whenever
//! the index moves.
//!
//! Why this exists: the firmware-internal DPI cycle that DPI-up /
//! DPI-down / DPI-cycle macro buttons trigger is **invisible** to
//! libratbag in upstream form — `Resolution.IsActive` reports the
//! last-written stage, not the live one. The companion libratbag
//! patch (`patches/libratbag/0001-refresh-active-resolution.patch`)
//! adds a `Device.RefreshActive` method that issues a HID++ 2.0
//! `GetCurrentDpiIndex` round-trip and updates `IsActive` to match.
//! This tracker calls `RefreshActive` periodically + re-reads
//! `IsActive` on each Resolution, then publishes our own
//! `ActiveDpiStageChanged` so the GUI can update without polling.
//!
//! Without the libratbag patch installed, `RefreshActive` returns
//! `UnknownMethod` / `NotSupported`; the tracker logs one warning
//! and goes quiet — the rest of gamerat keeps working with stale
//! "default active" semantics.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result};
use gamerat_proto::OBJECT_PATH;
use tracing::{debug, info, instrument, warn};
use zbus::zvariant::OwnedObjectPath;

use crate::service::{AppHandle, GameRatService};

const POLL_INTERVAL: Duration = Duration::from_millis(1_500);

/// Spawn the DPI tracker. Returns immediately; the actual work runs
/// in a tokio task. The task exits cleanly when `cancelled` flips
/// to true (typically on daemon shutdown).
pub fn spawn(
    handle: AppHandle,
    conn: zbus::Connection,
    cancelled: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = run(handle, conn, cancelled).await {
            warn!(error = ?e, "DPI tracker exited with error");
        }
    })
}

#[instrument(skip_all)]
async fn run(handle: AppHandle, conn: zbus::Connection, cancelled: Arc<AtomicBool>) -> Result<()> {
    let iface_ref = conn
        .object_server()
        .interface::<_, GameRatService>(OBJECT_PATH)
        .await
        .context("looking up GameRatService interface for DPI tracker")?;
    let emitter = iface_ref.signal_emitter();

    // Cache of last-seen active stage per device. Lets us only emit
    // ActiveDpiStageChanged on actual transitions, not every poll.
    let mut last_seen: HashMap<OwnedObjectPath, u32> = HashMap::new();

    // One-time warning when RefreshActive isn't available. Stays
    // useful while debugging; we only log it once per process.
    let mut warned_unsupported = false;

    info!("DPI tracker running ({}ms poll)", POLL_INTERVAL.as_millis());

    while !cancelled.load(Ordering::Relaxed) {
        let devices = match handle.ratbag.devices().await {
            Ok(d) => d,
            Err(e) => {
                debug!(error = ?e, "DPI tracker: devices() failed; will retry");
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
        };

        for device in devices {
            // Trigger ratbagd to re-read the hardware's live active
            // resolution. Without the libratbag patch, this errors;
            // we warn once and continue — the rest of gamerat works
            // fine with stale data.
            if let Err(e) = device.refresh_active().await {
                if !warned_unsupported {
                    warn!(
                        error = ?e,
                        "DPI tracker: Device.RefreshActive failed — \
                         install the libratbag patch in patches/libratbag/ \
                         to enable live DPI tracking"
                    );
                    warned_unsupported = true;
                }
                continue;
            }

            let stage = match device.active_dpi_stage_index().await {
                Ok(s) => s,
                Err(e) => {
                    debug!(error = ?e, "DPI tracker: stage read failed");
                    continue;
                }
            };

            let path = device.owned_object_path();
            match last_seen.get(&path) {
                Some(prev) if *prev == stage => {} // no change, no signal
                _ => {
                    last_seen.insert(path.clone(), stage);
                    debug!(?path, stage, "DPI tracker: stage changed");
                    if let Err(e) =
                        GameRatService::active_dpi_stage_changed(emitter, path, stage).await
                    {
                        warn!(error = ?e, "failed to emit ActiveDpiStageChanged");
                    }
                }
            }
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }

    info!("DPI tracker exiting");
    Ok(())
}
