//! Dispatch loop: focus event in, rule match, profile lookup, slot
//! allocation, ratbag write, signal out.
//!
//! Pipeline per focus event (post-Phase D):
//!
//!   1. Emit `FocusChanged` (always).
//!   2. Update `DaemonStatus.focused_app_id`.
//!   3. Pick the first ratbagd device. Skip if none.
//!   4. Lazy-build the per-device `SlotAllocator` on first sight
//!      (needs the device's `profile_count`).
//!   5. Match focused `app_id` against rules.
//!      - **Matched**: look up the referenced `GameratProfile`. If
//!        missing, warn-and-skip. Otherwise call the allocator,
//!        apply DPI if it's a fresh slot, set active either way,
//!        emit `ProfileSwitched`.
//!      - **No match**: switch to the Desktop slot (the
//!        no-rule fallback per `profile_architecture.md`). The
//!        Desktop slot is reserved by the allocator and is never
//!        written, so the user's canonical baseline is preserved.
//!
//! Only the first device is currently switched. Multi-device
//! dispatch is a later slice.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use futures::StreamExt as _;
use gamerat_focus::FocusStream;
use gamerat_proto::GameratProfile;
use gamerat_ratbag::Device;
use tokio::sync::RwLock;
use tracing::{debug, info, instrument, warn};

use crate::allocator::{AllocationReason, Decision, SlotAllocator};
use crate::paths;
use crate::service::{AppHandle, GameRatService};

/// Fixed Desktop slot index. Configurable in a later slice; hardcoded
/// to 0 for MVP since that matches what most ratbagd-using mice ship
/// as the "default" profile.
const DESKTOP_SLOT: u32 = 0;

#[instrument(skip_all)]
pub async fn run_dispatch(
    handle: AppHandle,
    mut stream: FocusStream,
    conn: zbus::Connection,
) -> Result<()> {
    let iface_ref = conn
        .object_server()
        .interface::<_, GameRatService>(gamerat_proto::OBJECT_PATH)
        .await
        .context("looking up registered GameRatService interface")?;
    let emitter = iface_ref.signal_emitter();

    info!("dispatch loop running");

    while let Some(event) = stream.next().await {
        debug!(?event, "focus event");

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

        {
            let mut status = handle.status.write().await;
            status.focused_app_id.clone_from(&event.app_id);
        }

        // Autoswitch off ⇒ stop here. We still emitted FocusChanged
        // above so the GUI can update its "Focused app" line; we just
        // don't drive the rule → profile pipeline.
        if !handle.settings.read().await.auto_switch_enabled {
            debug!(app_id = %event.app_id, "autoswitch off; skipping rule match");
            continue;
        }

        let Some(device) = first_device(&handle).await else {
            continue;
        };

        if let Err(e) = ensure_allocator(&handle, &device).await {
            warn!(error = ?e, "couldn't build slot allocator; skipping");
            continue;
        }

        let matched = {
            let rules = handle.rules.read().await;
            rules.match_app_id(&event.app_id).cloned()
        };

        if let Some(rule) = matched {
            let profile = handle.profiles.read().await.get(&rule.profile_id).cloned();
            let Some(profile) = profile else {
                warn!(
                    app_id = %event.app_id,
                    profile_id = %rule.profile_id,
                    "rule matched but referenced profile is missing — skipping"
                );
                continue;
            };

            if let Err(e) = apply_rule(
                &handle,
                &device,
                emitter,
                &event.app_id,
                &rule.app_id_glob,
                &profile,
            )
            .await
            {
                warn!(error = ?e, "apply_rule failed");
            }
        } else {
            // No rule matched → fall back to Desktop slot.
            if let Err(e) = fallback_to_desktop(&handle, &device, emitter).await {
                warn!(error = ?e, "fallback_to_desktop failed");
            }
        }
    }

    info!("dispatch loop exiting (focus stream ended)");
    Ok(())
}

async fn first_device(handle: &AppHandle) -> Option<Device> {
    let devices = match handle.ratbag.devices().await {
        Ok(d) => d,
        Err(e) => {
            warn!(error = ?e, "ratbag devices() failed");
            return None;
        }
    };
    let first = devices.into_iter().next();
    if first.is_none() {
        debug!("no ratbagd devices; skipping focus event");
    }
    first
}

/// Public wrapper used by the service layer for manual-apply
/// (`ApplyProfile`) — the regular dispatch loop calls
/// [`ensure_allocator`] privately. Same semantics; idempotent.
pub async fn ensure_allocator_public(handle: &AppHandle, device: &Device) -> Result<()> {
    ensure_allocator(handle, device).await
}

/// Apply a profile to the device immediately, like `apply_rule`
/// but without a rule glob.
///
/// Used by the manual-mode `ApplyProfile` IPC. Goes through the
/// same allocator decision / write-or-set-active /
/// emit-ProfileSwitched path so the slot cache stays consistent.
pub async fn apply_profile_manual(
    handle: &AppHandle,
    device: &Device,
    emitter: &zbus::object_server::SignalEmitter<'_>,
    profile: &GameratProfile,
) -> Result<()> {
    apply_rule(handle, device, emitter, "(manual)", "(manual)", profile).await
}

/// Build a [`SlotAllocator`] for the device on first sight. Idempotent:
/// returns immediately if one already exists.
async fn ensure_allocator(handle: &AppHandle, device: &Device) -> Result<()> {
    {
        let alloc = handle.allocator.read().await;
        if alloc.is_some() {
            return Ok(());
        }
    }
    let profile_count = device
        .profile_count()
        .await
        .context("reading device profile_count for allocator")?;
    let cache_path = paths::default_slot_cache_path()?;
    let allocator = SlotAllocator::load_or_create(cache_path, DESKTOP_SLOT, profile_count)
        .context("building SlotAllocator")?;
    info!(
        profile_count,
        desktop = DESKTOP_SLOT,
        "slot allocator initialised"
    );
    *handle.allocator.write().await = Some(allocator);
    Ok(())
}

async fn apply_rule(
    handle: &AppHandle,
    device: &Device,
    emitter: &zbus::object_server::SignalEmitter<'_>,
    app_id: &str,
    matched_glob: &str,
    profile: &GameratProfile,
) -> Result<()> {
    let from = device
        .active_profile_index()
        .await
        .context("reading active profile before apply")?;

    let decision: Decision = {
        let mut alloc_guard = handle.allocator.write().await;
        let Some(alloc) = alloc_guard.as_mut() else {
            anyhow::bail!("allocator not initialised — caller must run ensure_allocator first");
        };
        let d = alloc.allocate(profile);
        drop(alloc_guard);
        d
    };
    debug!(?decision, profile_id = %profile.id, "allocator decision");

    if decision.needs_write {
        device
            .apply_profile_complete(
                decision.slot,
                &profile.dpi,
                profile.active_dpi_stage,
                &profile.buttons,
            )
            .await
            .context("apply_profile_complete")?;
    } else {
        // Cached — the slot already has this profile materialized.
        if from == decision.slot {
            debug!("already on target slot; no SetActive needed");
        } else {
            device
                .set_active_profile(decision.slot)
                .await
                .context("set_active_profile")?;
        }
    }

    // Persist the LRU bookkeeping so cache locality survives restarts.
    {
        let alloc_guard = handle.allocator.read().await;
        if let Some(alloc) = alloc_guard.as_ref() {
            if let Err(e) = alloc.save() {
                warn!(error = ?e, "couldn't persist slot cache");
            }
        }
    }

    let reason = match decision.reason {
        AllocationReason::Cached => format!("rule:{matched_glob}:{} (cached)", profile.id),
        AllocationReason::EmptySlot => format!("rule:{matched_glob}:{} (empty slot)", profile.id),
        AllocationReason::Evicted {
            previous_profile_id,
        } => format!(
            "rule:{matched_glob}:{} (evicted {previous_profile_id})",
            profile.id
        ),
    };

    if from != decision.slot {
        emit_profile_switched(emitter, device, from, decision.slot, &reason).await;
    }

    {
        let mut status = handle.status.write().await;
        status.last_switch_reason.clone_from(&reason);
    }
    info!(
        from,
        to = decision.slot,
        app_id,
        profile_id = %profile.id,
        "rule applied"
    );
    Ok(())
}

async fn fallback_to_desktop(
    handle: &AppHandle,
    device: &Device,
    emitter: &zbus::object_server::SignalEmitter<'_>,
) -> Result<()> {
    let from = device
        .active_profile_index()
        .await
        .context("reading active profile for desktop fallback")?;
    if from == DESKTOP_SLOT {
        return Ok(());
    }
    device
        .set_active_profile(DESKTOP_SLOT)
        .await
        .context("set_active_profile to desktop")?;
    emit_profile_switched(
        emitter,
        device,
        from,
        DESKTOP_SLOT,
        "desktop:no-rule-matched",
    )
    .await;
    {
        let mut status = handle.status.write().await;
        "desktop:no-rule-matched".clone_into(&mut status.last_switch_reason);
    }
    info!(
        from,
        to = DESKTOP_SLOT,
        "no rule matched — switched to Desktop"
    );
    Ok(())
}

async fn emit_profile_switched(
    emitter: &zbus::object_server::SignalEmitter<'_>,
    device: &Device,
    from: u32,
    to: u32,
    reason: &str,
) {
    if let Err(e) =
        GameRatService::profile_switched(emitter, device.owned_object_path(), from, to, reason)
            .await
    {
        warn!(error = ?e, "failed to emit ProfileSwitched");
    }
}

// Re-export for type-completeness; the existing daemon code already
// passes around `Arc<RwLock<...>>` for similar shared state.
#[allow(dead_code)]
pub type SharedAllocator = Arc<RwLock<Option<SlotAllocator>>>;
