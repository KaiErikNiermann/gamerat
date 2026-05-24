//! Auto-import of externally-written hardware slots.
//!
//! On daemon startup (and on first focus event after device connect),
//! the daemon walks every non-Desktop slot and asks the allocator's
//! cache: "do I already own this slot?". If yes, nothing to do. If
//! no, the slot was either factory-set or written by another tool
//! (Piper, the libratbag CLI), and gamerat reads its content,
//! synthesises a [`GameratProfile`] from it, persists it to the
//! profile store, and registers the binding in the allocator as
//! `stale=false` (the on-disk profile matches the hardware by
//! construction, so the next [`SlotAllocator::allocate`] returns
//! [`crate::allocator::AllocationReason::Cached`] without rewriting).
//!
//! The point: making the gamerat profile store the source of truth
//! for what's actually on the device. Without this, the DEVICES
//! panel's "(empty) vs occupied" label — derived from the allocator
//! cache alone — would silently misrepresent slots written by Piper
//! as "(empty)" despite holding content.

use anyhow::{Context as _, Result};
use gamerat_proto::{GameratProfile, game_category};
use gamerat_ratbag::Device;
use tracing::{debug, info, warn};

use crate::allocator::SlotAllocator;
use crate::profiles::ProfileStore;
use crate::service::AppHandle;

/// Read `slot`'s content from the device and create / overwrite the
/// matching `imported-slot-N` profile in the store, then register the
/// binding in the allocator as `stale=false`. Returns `true` if the
/// import actually happened, `false` if it was skipped (slot is the
/// desktop slot, or `force=false` and the allocator already owns the
/// slot).
///
/// Callers are responsible for persisting both the store and the
/// allocator afterwards — `auto_import_unknown_slots` batches one
/// `save()` per pass instead of paying the cost per slot.
async fn import_slot(
    device: &Device,
    store: &mut ProfileStore,
    allocator: &mut SlotAllocator,
    slot: u32,
    force: bool,
) -> Result<bool> {
    if slot == allocator.desktop_slot() {
        debug!(slot, "desktop slot; skipping import");
        return Ok(false);
    }
    if !force && allocator.is_managed(slot) {
        debug!(slot, "slot already managed; skipping import");
        return Ok(false);
    }
    let snap = device
        .read_profile_slot(slot)
        .await
        .with_context(|| format!("reading content of slot {slot}"))?;
    let id = format!("imported-slot-{slot}");
    let category = game_category::AGNOSTIC.to_owned();
    // Defensive default: a slot reporting zero DPI stages would fail
    // `ProfileStore::upsert`'s NoDpi validation. Any wedged
    // non-desktop slot still gets a usable starting point.
    let dpi = if snap.dpi.is_empty() {
        vec![800]
    } else {
        snap.dpi
    };
    let profile = GameratProfile {
        id: id.clone(),
        name: format!("Imported (slot {slot})"),
        description: "Auto-imported from device on connect. Rename or \
                      edit like any other profile."
            .to_owned(),
        category: category.clone(),
        inherits_from: String::new(),
        dpi,
        active_dpi_stage: snap.active_dpi_stage,
        created_unix: 0,
        buttons: snap.buttons,
        leds: snap.leds,
        soft_macros: Vec::new(),
    };
    store
        .upsert(profile)
        .with_context(|| format!("upserting imported profile for slot {slot}"))?;
    allocator.import_entry(slot, id, category);
    info!(slot, force, "imported slot content into profile store");
    Ok(true)
}

/// Walk every non-Desktop slot on `device` and import the ones the
/// allocator doesn't already own. Returns the number of newly-imported
/// profiles (0 means everything was already known).
///
/// Per-slot read failures are logged and skipped — one flaky slot
/// doesn't kill the others. The synthesised profile id is
/// deterministic (`imported-slot-N`) so re-running this function is
/// idempotent for already-imported slots and self-healing for any
/// slot whose `GameratProfile` was deleted by Purge (which clears the
/// allocator cache too — see [`crate::service`]).
pub async fn auto_import_unknown_slots(
    device: &Device,
    store: &mut ProfileStore,
    allocator: &mut SlotAllocator,
) -> Result<usize> {
    let profile_count = allocator.profile_count();
    let desktop_slot = allocator.desktop_slot();
    let mut imported = 0usize;

    for slot in (0..profile_count).filter(|&i| i != desktop_slot) {
        match import_slot(device, store, allocator, slot, false).await {
            Ok(true) => imported = imported.saturating_add(1),
            Ok(false) => {} // skipped (managed or desktop)
            Err(e) => warn!(slot, error = ?e, "couldn't import slot; continuing"),
        }
    }

    if imported > 0 {
        store
            .save()
            .context("persisting profile store after auto-import")?;
        allocator
            .save()
            .context("persisting slot allocator after auto-import")?;
    }
    Ok(imported)
}

/// Force a re-read of `slot`'s content on `device` and overwrite the
/// matching `imported-slot-N` profile.
///
/// Bypasses the allocator-already-owns-this check that
/// [`auto_import_unknown_slots`] honours. Used by `gameratctl device
/// import-slot` to refresh an imported profile when something
/// external (Piper, libratbag CLI) has rewritten the hardware slot
/// while gamerat already had a stale entry for it.
pub async fn reimport_slot(
    device: &Device,
    store: &mut ProfileStore,
    allocator: &mut SlotAllocator,
    slot: u32,
) -> Result<()> {
    let imported = import_slot(device, store, allocator, slot, true).await?;
    if imported {
        store
            .save()
            .context("persisting profile store after reimport")?;
        allocator
            .save()
            .context("persisting slot allocator after reimport")?;
    }
    Ok(())
}

/// Daemon-startup convenience: pull the first device, ensure the
/// allocator is initialised for it, then run
/// [`auto_import_unknown_slots`].
///
/// Idempotent — safe to invoke multiple times; safe to race with the
/// dispatch loop's own `ensure_allocator` call (both reach for
/// `handle.allocator` under a write lock). All failures are logged +
/// swallowed: the daemon stays functional even if import fails (the
/// DEVICES pill just won't reflect external content until manual
/// re-import via CLI).
//
// Both write locks are deliberately held across the import pass:
// releasing them between the is_managed check and the upsert opens a
// TOCTOU window where the dispatch loop could race to re-import a
// slot we're about to import.
#[allow(clippy::significant_drop_tightening)]
pub async fn run_initial_import(handle: AppHandle) {
    let Some(ratbag) = handle.ratbag.as_ref() else {
        debug!("--no-ratbagd; skipping initial import");
        return;
    };
    let devices = match ratbag.devices().await {
        Ok(d) => d,
        Err(e) => {
            warn!(error = ?e, "ratbag.devices() failed; skipping initial import");
            return;
        }
    };
    let Some(device) = devices.into_iter().next() else {
        debug!("no ratbag devices; skipping initial import");
        return;
    };
    if let Err(e) = crate::dispatch::ensure_allocator_public(&handle, &device).await {
        warn!(error = ?e, "couldn't ensure allocator for initial import; skipping");
        return;
    }
    // Acquire both write locks. `auto_import_unknown_slots` makes
    // exactly one round of D-Bus reads per unknown slot — fine to
    // hold the locks across that, since this runs once at startup
    // before the dispatch loop starts pumping focus events.
    let mut alloc_guard = handle.allocator.write().await;
    let Some(alloc) = alloc_guard.as_mut() else {
        warn!("allocator missing post-ensure; skipping initial import");
        return;
    };
    let mut store_guard = handle.profiles.write().await;
    match auto_import_unknown_slots(&device, &mut store_guard, alloc).await {
        Ok(0) => debug!("initial auto-import: nothing to do"),
        Ok(n) => info!(imported = n, "initial auto-import complete"),
        Err(e) => warn!(error = ?e, "initial auto-import failed"),
    }
}
