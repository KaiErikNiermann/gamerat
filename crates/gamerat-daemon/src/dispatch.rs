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

    // Deadline for the next no-rule-match → Desktop-fallback fire.
    // `None` means no fallback pending. The deadline is *not* reset
    // on repeated no-match events — see the no-match branch below
    // for the rationale.
    let mut pending_desktop_at: Option<tokio::time::Instant> = None;

    loop {
        // Build the sleep arm of the select. When no fallback is
        // pending, this is a never-completing `pending` future so
        // only the stream arm can fire.
        let sleep_arm = async {
            match pending_desktop_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending::<()>().await,
            }
        };

        tokio::select! {
            // `StreamExt::next` is cancel-safe — fine to re-poll
            // across loop iterations if the sleep arm fired first.
            maybe_event = stream.next() => {
                let Some(event) = maybe_event else { break };
                handle_focus_event(
                    &handle,
                    emitter,
                    event,
                    &mut pending_desktop_at,
                )
                .await;
            }
            () = sleep_arm => {
                pending_desktop_at = None;
                if let Some(device) = first_device(&handle).await {
                    if let Err(e) = fallback_to_desktop(&handle, &device, emitter).await {
                        warn!(error = ?e, "deferred fallback_to_desktop failed");
                    }
                }
            }
        }
    }

    info!("dispatch loop exiting (focus stream ended)");
    Ok(())
}

/// Process a single focus event and update `pending_desktop_at` so
/// the outer `select!` loop schedules / cancels the Desktop-return
/// timer correctly.
async fn handle_focus_event(
    handle: &AppHandle,
    emitter: &zbus::object_server::SignalEmitter<'_>,
    event: gamerat_focus::FocusEvent,
    pending_desktop_at: &mut Option<tokio::time::Instant>,
) {
    debug!(?event, "focus event");

    if let Err(e) =
        GameRatService::focus_changed(emitter, &event.app_id, &event.title, event.source.as_wire())
            .await
    {
        warn!(error = ?e, "failed to emit FocusChanged");
    }

    {
        let mut status = handle.status.write().await;
        status.focused_app_id.clone_from(&event.app_id);
    }

    // Snapshot settings once per event so we don't take the lock
    // multiple times within one focus handling.
    let (auto_switch_enabled, desktop_return_enabled, desktop_return_delay_ms) = {
        let s = handle.settings.read().await;
        (
            s.auto_switch_enabled,
            s.desktop_return_enabled,
            s.desktop_return_delay_ms,
        )
    };

    if !auto_switch_enabled {
        debug!(app_id = %event.app_id, "autoswitch off; skipping rule match");
        return;
    }

    let Some(device) = first_device(handle).await else {
        return;
    };

    if let Err(e) = ensure_allocator(handle, &device).await {
        warn!(error = ?e, "couldn't build slot allocator; skipping");
        return;
    }

    let matched = {
        let rules = handle.rules.read().await;
        rules.match_app_id(&event.app_id).cloned()
    };

    if let Some(rule) = matched {
        // Any rule match cancels a pending Desktop fallback — the
        // user is back on a game window before the debounce
        // window elapsed.
        *pending_desktop_at = None;

        let profile = handle.profiles.read().await.get(&rule.profile_id).cloned();
        let Some(profile) = profile else {
            warn!(
                app_id = %event.app_id,
                profile_id = %rule.profile_id,
                "rule matched but referenced profile is missing — skipping"
            );
            return;
        };

        if let Err(e) = apply_rule(
            handle,
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
        match no_match_action(
            desktop_return_enabled,
            desktop_return_delay_ms,
            pending_desktop_at.is_some(),
        ) {
            NoMatchAction::Suppress | NoMatchAction::KeepPending => {}
            NoMatchAction::FireNow => {
                if let Err(e) = fallback_to_desktop(handle, &device, emitter).await {
                    warn!(error = ?e, "fallback_to_desktop failed");
                }
            }
            NoMatchAction::Schedule(delay) => {
                *pending_desktop_at = Some(tokio::time::Instant::now() + delay);
                debug!(
                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    "scheduled deferred Desktop fallback"
                );
            }
        }
    }
}

/// What to do on a focus event that didn't match any rule. Decision
/// logic lives in a pure function so it can be unit-tested without
/// standing up the full dispatch loop (which is bound to a live zbus
/// connection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoMatchAction {
    /// `desktop_return_enabled = false` — don't fall back at all.
    Suppress,
    /// No pending fallback was scheduled; fire one immediately
    /// (delay = 0, legacy behaviour).
    FireNow,
    /// No pending fallback was scheduled; arm one for `delay` from
    /// now.
    Schedule(std::time::Duration),
    /// A pending fallback already exists; leave its deadline alone.
    /// Repeated no-match events (e.g. cycling between non-game
    /// windows) shouldn't extend the debounce — the first no-match
    /// starts the clock and subsequent ones ride it.
    KeepPending,
}

const fn no_match_action(
    desktop_return_enabled: bool,
    desktop_return_delay_ms: u64,
    has_pending: bool,
) -> NoMatchAction {
    if !desktop_return_enabled {
        return NoMatchAction::Suppress;
    }
    if has_pending {
        return NoMatchAction::KeepPending;
    }
    if desktop_return_delay_ms == 0 {
        return NoMatchAction::FireNow;
    }
    NoMatchAction::Schedule(std::time::Duration::from_millis(desktop_return_delay_ms))
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

    let reason = match &decision.reason {
        AllocationReason::Cached => format!("rule:{matched_glob}:{} (cached)", profile.id),
        AllocationReason::EmptySlot => format!("rule:{matched_glob}:{} (empty slot)", profile.id),
        AllocationReason::Evicted {
            previous_profile_id,
        } => format!(
            "rule:{matched_glob}:{} (evicted {previous_profile_id})",
            profile.id
        ),
        AllocationReason::ContentChanged => {
            format!("rule:{matched_glob}:{} (content changed)", profile.id)
        }
    };

    // Pre-emit ProfileSwitching whenever we're about to either write
    // the slot or change the active profile index. Skip the
    // already-on-target / no-write case (truly nothing to surface).
    let will_switch = decision.needs_write || from != decision.slot;
    if will_switch {
        emit_profile_switching(emitter, device, decision.slot, &reason).await;
    }

    if decision.needs_write {
        device
            .apply_profile_complete(
                decision.slot,
                &profile.dpi,
                profile.active_dpi_stage,
                &profile.buttons,
                &profile.leds,
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

    if from != decision.slot {
        emit_profile_switched(emitter, device, from, decision.slot, &reason).await;
        notify_profile_switch(handle, emitter.connection(), &profile.name, &reason).await;
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
    emit_profile_switching(emitter, device, DESKTOP_SLOT, "desktop:no-rule-matched").await;
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
    notify_profile_switch(
        handle,
        emitter.connection(),
        "Base",
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
    emit_profile_switched_for_path(emitter, device.owned_object_path(), from, to, reason).await;
}

/// Pre-commit "swap is about to happen" signal. The GUI flashes a
/// switching indicator on receipt; covers the brief firmware-jitter
/// window during the upcoming `device.commit()`.
async fn emit_profile_switching(
    emitter: &zbus::object_server::SignalEmitter<'_>,
    device: &Device,
    to: u32,
    reason: &str,
) {
    if let Err(e) =
        GameRatService::profile_switching(emitter, device.owned_object_path(), to, reason).await
    {
        warn!(error = ?e, "failed to emit ProfileSwitching");
    }
}

/// Public form used by service-layer call-sites that already hold an
/// owned object path. Pairs with `emit_profile_switched_for_path`.
pub async fn emit_profile_switching_for_path(
    emitter: &zbus::object_server::SignalEmitter<'_>,
    device_path: zbus::zvariant::OwnedObjectPath,
    to: u32,
    reason: &str,
) {
    if let Err(e) = GameRatService::profile_switching(emitter, device_path, to, reason).await {
        warn!(error = ?e, "failed to emit ProfileSwitching");
    }
}

/// Raise an `org.freedesktop.Notifications.Notify` if the
/// `notify_on_profile_switch` setting is on. Talks to the session bus
/// directly via zbus — we ran the `tauri-plugin-notification`
/// equivalent on the GUI side and hit `notify-rust`'s
/// "Cannot start a runtime from within a runtime" panic inside Tauri's
/// Tokio runtime. Doing it here also means notifications fire even
/// when the GUI is closed, which is the whole point of OS-level
/// notifications.
///
/// `profile_name` is the human-readable name for the body; the reason
/// string is used only to detect Base / Desktop switches so they
/// render as "Switched to base" instead of a slot index.
async fn notify_profile_switch(
    handle: &AppHandle,
    conn: &zbus::Connection,
    profile_name: &str,
    reason: &str,
) {
    if !handle.settings.read().await.notify_on_profile_switch {
        return;
    }
    let body = if reason.starts_with("manual:base") || reason.starts_with("desktop:") {
        "Switched to base (desktop)".to_owned()
    } else {
        format!("Switched to {profile_name}")
    };
    if let Err(e) = send_notification(conn, &body).await {
        warn!(error = ?e, "couldn't dispatch system notification");
    }
}

pub async fn notify_profile_switch_with(
    handle: &AppHandle,
    conn: &zbus::Connection,
    profile_name: &str,
    reason: &str,
) {
    notify_profile_switch(handle, conn, profile_name, reason).await;
}

async fn send_notification(conn: &zbus::Connection, body: &str) -> zbus::Result<()> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let actions: Vec<&str> = Vec::new();
    let hints: HashMap<&str, Value<'_>> = HashMap::new();
    let proxy = zbus::Proxy::new(
        conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await?;
    // Notify signature: s u s s s as a{sv} i → u
    // We ignore the returned id.
    let _id: u32 = proxy
        .call(
            "Notify",
            &(
                "gamerat", // app_name
                0u32,      // replaces_id
                "gamerat", // app_icon (matches the .desktop Icon entry)
                "gamerat", // summary (title)
                body,      // body
                actions, hints, 5_000i32, // expire_timeout (ms)
            ),
        )
        .await?;
    Ok(())
}

/// Same as `emit_profile_switched` but takes an owned object path directly.
///
/// Convenient for service-layer call-sites that already hold the
/// device wrapper but want the emission helper available without
/// re-deriving it. Re-exported as
/// `crate::dispatch::emit_profile_switched_for_path` for the service.
pub async fn emit_profile_switched_for_path(
    emitter: &zbus::object_server::SignalEmitter<'_>,
    device_path: zbus::zvariant::OwnedObjectPath,
    from: u32,
    to: u32,
    reason: &str,
) {
    if let Err(e) = GameRatService::profile_switched(emitter, device_path, from, to, reason).await {
        warn!(error = ?e, "failed to emit ProfileSwitched");
    }
}

// Re-export for type-completeness; the existing daemon code already
// passes around `Arc<RwLock<...>>` for similar shared state.
#[allow(dead_code)]
pub type SharedAllocator = Arc<RwLock<Option<SlotAllocator>>>;

#[cfg(test)]
mod tests {
    use super::{NoMatchAction, no_match_action};
    use std::time::Duration;

    #[test]
    fn return_disabled_suppresses_fallback() {
        assert_eq!(no_match_action(false, 0, false), NoMatchAction::Suppress);
        assert_eq!(no_match_action(false, 5_000, true), NoMatchAction::Suppress);
    }

    #[test]
    fn delay_zero_fires_immediately_without_pending() {
        assert_eq!(no_match_action(true, 0, false), NoMatchAction::FireNow);
    }

    #[test]
    fn delay_nonzero_schedules_without_pending() {
        assert_eq!(
            no_match_action(true, 5_000, false),
            NoMatchAction::Schedule(Duration::from_millis(5_000))
        );
        assert_eq!(
            no_match_action(true, 120_000, false),
            NoMatchAction::Schedule(Duration::from_millis(120_000))
        );
    }

    #[test]
    fn pending_is_preserved_on_repeat_no_match() {
        // The whole point of the debounce: cycling between non-game
        // windows must not extend the deadline. First no-match arms
        // it, subsequent ones leave it alone.
        assert_eq!(
            no_match_action(true, 5_000, true),
            NoMatchAction::KeepPending
        );
        assert_eq!(no_match_action(true, 0, true), NoMatchAction::KeepPending);
    }
}
