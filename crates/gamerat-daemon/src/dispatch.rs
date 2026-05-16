//! Dispatch loop: focus event in, rule match, ratbag write, signal out.

use anyhow::{Context as _, Result};
use futures::StreamExt as _;
use gamerat_focus::FocusStream;
use tracing::{debug, info, instrument, warn};

use crate::service::{AppHandle, GameRatService};

/// Poll the focus stream until it ends. For each event:
///
///   1. Emit `FocusChanged` (always, whether or not a rule matched).
///   2. Look up a matching rule.
///   3. If matched and the device is *not* already on that profile,
///      call ratbag to switch + commit, then emit `ProfileSwitched`.
///   4. Update [`DaemonStatus`] for the next `Status` call.
///
/// Only the *first* device is currently switched. The vast majority of
/// users have one mouse; multi-device dispatch is a follow-up.
#[instrument(skip_all)]
pub async fn run_dispatch(
    handle: AppHandle,
    mut stream: FocusStream,
    conn: zbus::Connection,
) -> Result<()> {
    // Resolve the SignalEmitter once — it's tied to the registered
    // interface object, not to any one method call.
    let iface_ref = conn
        .object_server()
        .interface::<_, GameRatService>(gamerat_proto::OBJECT_PATH)
        .await
        .context("looking up registered GameRatService interface")?;
    let emitter = iface_ref.signal_emitter();

    info!("dispatch loop running");

    while let Some(event) = stream.next().await {
        debug!(?event, "focus event");

        // (1) Always emit FocusChanged.
        if let Err(e) = GameRatService::focus_changed(
            emitter,
            &event.app_id,
            &event.title,
            event.source.as_wire(),
        )
        .await
        {
            warn!(error = ?e, "failed to emit FocusChanged");
        }

        // Update status snapshot.
        {
            let mut status = handle.status.write().await;
            status.focused_app_id.clone_from(&event.app_id);
        }

        // (2) Rule match.
        let matched = {
            let rules = handle.rules.read().await;
            rules.match_app_id(&event.app_id).cloned()
        };

        let Some(rule) = matched else {
            continue;
        };

        // (3) Push to the first device.
        match handle.ratbag.devices().await {
            Ok(devices) => {
                let Some(device) = devices.into_iter().next() else {
                    warn!(app_id = %event.app_id, "rule matched but no ratbagd devices");
                    continue;
                };
                let from = match device.active_profile_index().await {
                    Ok(idx) => idx,
                    Err(e) => {
                        warn!(error = ?e, "could not read active_profile_index; skipping");
                        continue;
                    }
                };
                if from == rule.profile_index {
                    debug!(
                        profile = rule.profile_index,
                        "device already on target profile"
                    );
                    continue;
                }
                if let Err(e) = device.set_active_profile(rule.profile_index).await {
                    warn!(error = ?e, "set_active_profile failed");
                    continue;
                }

                let reason = format!("rule:{}", rule.app_id_glob);
                if let Err(e) = GameRatService::profile_switched(
                    emitter,
                    device.owned_object_path(),
                    from,
                    rule.profile_index,
                    &reason,
                )
                .await
                {
                    warn!(error = ?e, "failed to emit ProfileSwitched");
                }

                {
                    let mut status = handle.status.write().await;
                    status.last_switch_reason = reason;
                }
                info!(from, to = rule.profile_index, glob = %rule.app_id_glob, "profile switched");
            }
            Err(e) => {
                warn!(error = ?e, "ratbag devices() failed");
            }
        }
    }

    info!("dispatch loop exiting (focus stream ended)");
    Ok(())
}
