//! Runtime state for the software-input pipeline.
//!
//! The daemon owns:
//!
//! - a [`UinputEmitter`] behind a `tokio::sync::Mutex` (shared across
//!   all soft-macro firings),
//! - a [`SoftInputState`] tag describing what the input subsystem is
//!   currently doing (surfaced as the GUI's "Soft input" status pill),
//! - a [`ToggleStateMap`] tracking per-button on/off state across
//!   firings — keyed by `(device_path, button_index)` so multiple
//!   mice don't share state,
//! - a [`TrampolineRegistry`] mapping every active soft-macro by its
//!   allocated trampoline keycode + device, so the input dispatcher
//!   can look up "what soft-macro just fired" without scanning the
//!   active profile on every event.
//!
//! Trampoline keycodes are allocated by [`TrampolineAllocator`]
//! at profile-apply time, drawn from the keycodes the device can
//! actually emit ([`available_trampolines`]). Allocations are persisted *into* the profile
//! (`SoftMacro::trampoline_keycode`), so the same button gets the same
//! keycode across daemon restarts — the allocator just plugs the
//! "needs a fresh slot" holes a newly-edited profile has.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::StreamExt as _;
use gamerat_input::{EvdevBackend, EvdevError, InputBackend, UinputEmitter, discovery};
use gamerat_proto::{SoftMacro, soft_macro_kind, trampoline_keycode};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};
use zbus::zvariant::OwnedObjectPath;

use crate::service::AppHandle;

/// Wire-stable strings for the "Soft input" status pill the GUI shows.
/// Mirrors the [`gamerat_proto::focus_bridge`] pattern.
pub mod soft_input_state {
    /// Master flag off; no `/dev/uinput`, no evdev readers.
    pub const DISABLED: &str = "disabled";
    /// Master flag on, evdev + uinput online.
    pub const ACTIVE: &str = "active";
    /// Master flag on but `/dev/uinput` rejected our open
    /// (permissions / module missing). The daemon stays up; soft-
    /// macros are effectively no-ops until the user fixes the perms.
    pub const UNAVAILABLE: &str = "unavailable";
}

/// Snapshot of the input subsystem's health. See [`soft_input_state`]
/// for the wire strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoftInputState {
    Disabled,
    Active,
    Unavailable,
}

impl From<SoftInputState> for String {
    fn from(s: SoftInputState) -> Self {
        match s {
            SoftInputState::Disabled => soft_input_state::DISABLED,
            SoftInputState::Active => soft_input_state::ACTIVE,
            SoftInputState::Unavailable => soft_input_state::UNAVAILABLE,
        }
        .to_owned()
    }
}

/// Read-only view of the per-button toggle state, keyed by
/// `(device_path, button_index)`. Cleared on profile switch — toggles
/// don't survive across profiles even on the same physical button.
pub type ToggleStateMap = Arc<RwLock<HashMap<(OwnedObjectPath, u32), bool>>>;

/// Per-device dispatch table: trampoline keycode → soft-macro spec.
/// Rebuilt every time a profile is applied. The dispatcher looks up
/// `(device, trampoline_keycode)` here on each evdev event.
pub type TrampolineRegistry = Arc<RwLock<HashMap<OwnedObjectPath, HashMap<u32, SoftMacroEntry>>>>;

/// Active soft-macro entry indexed under [`TrampolineRegistry`].
///
/// Snapshots the fields the dispatcher needs rather than cloning the
/// proto type, so the dispatcher doesn't reach back into the active
/// profile on every press.
#[derive(Clone, Debug)]
pub struct SoftMacroEntry {
    pub button_index: u32,
    pub kind: u32,
    pub keys: Vec<u32>,
}

/// Allocator for trampoline keycode slots within a single profile-apply
/// batch. Lives only for the duration of one apply call.
///
/// Allocates exclusively from `available` — the candidate pool already
/// intersected with *this device's* advertised keys, in preference
/// order — so it can never hand out a keycode the firmware can't emit.
#[derive(Debug)]
pub struct TrampolineAllocator {
    available: Vec<u32>,
    used: HashSet<u32>,
}

impl TrampolineAllocator {
    /// Build an allocator over the keycodes this device can actually
    /// emit (`available`). Seeds `used` with any of the profile's
    /// existing trampolines that are still in `available`, so re-applies
    /// keep stable assignments instead of churning the firmware.
    #[must_use]
    pub fn new(available: Vec<u32>, soft_macros: &[SoftMacro]) -> Self {
        let avail: HashSet<u32> = available.iter().copied().collect();
        let used: HashSet<u32> = soft_macros
            .iter()
            .filter(|m| m.kind != soft_macro_kind::DISABLED)
            .map(|m| m.trampoline_keycode)
            .filter(|k| avail.contains(k))
            .collect();
        Self { available, used }
    }

    /// Allocate the next free device-emittable trampoline keycode (in
    /// preference order). Returns `None` when every available slot is
    /// taken — only reachable on a device that advertises fewer
    /// candidates than the profile has soft-macros.
    pub fn allocate(&mut self) -> Option<u32> {
        let next = self
            .available
            .iter()
            .copied()
            .find(|k| !self.used.contains(k))?;
        self.used.insert(next);
        Some(next)
    }
}

/// Trampoline keycodes the device behind `model` can actually emit, in
/// preference order (F-keys before the macro range).
///
/// Intersects the candidate pool with the device's advertised evdev
/// keys (read from its `/dev/input/event*` nodes). Empty when the model
/// is unparseable, the nodes can't be read, or the device advertises
/// none of the candidates — in which case soft-macros simply can't work
/// on that mouse and the caller surfaces that instead of writing a
/// keycode the firmware would mangle.
#[must_use]
pub fn available_trampolines(model: &str) -> Vec<u32> {
    let Ok(target) = gamerat_input::parse_model(model) else {
        warn!(
            model,
            "couldn't parse device model; soft-macro trampolines unavailable"
        );
        return Vec::new();
    };
    let nodes = gamerat_input::find_evdev_nodes(target).unwrap_or_default();
    let supported = gamerat_input::supported_keycodes(&nodes);
    trampoline_keycode::candidates()
        .filter(|k| supported.contains(k))
        .collect()
}

/// Allocate trampoline keycodes for fresh soft-macros, drawing only
/// from `available` (this device's emittable candidates).
///
/// Reassigns any entry whose stored keycode isn't in `available` — that
/// covers both never-assigned macros (`0`) and profiles carried over
/// from a different mouse (e.g. a stored `KEY_MACRO1` on a device that
/// can't emit it). The caller persists the rewrites so allocations stay
/// stable across runs.
pub fn allocate_missing_trampolines(soft_macros: &mut [SoftMacro], available: &[u32]) {
    let avail: HashSet<u32> = available.iter().copied().collect();
    let mut allocator = TrampolineAllocator::new(available.to_vec(), soft_macros);
    for m in soft_macros
        .iter_mut()
        .filter(|m| m.kind != soft_macro_kind::DISABLED)
    {
        if !avail.contains(&m.trampoline_keycode) {
            if let Some(slot) = allocator.allocate() {
                debug!(
                    button_index = m.button_index,
                    trampoline = slot,
                    "assigned device-emittable trampoline keycode"
                );
                m.trampoline_keycode = slot;
            } else {
                warn!(
                    button_index = m.button_index,
                    "no free device-emittable trampoline keycode; soft-macro will be inert"
                );
            }
        }
    }
}

/// Prepare the buttons vec the daemon will hand to ratbagd.
///
/// Steps, in order:
///
/// 1. Take a working copy of `profile.soft_macros` and allocate
///    trampoline keycodes for any entries that don't have one yet.
/// 2. If anything changed, persist the rewritten profile back through
///    [`crate::profiles::ProfileStore`] so the trampoline assignments
///    survive the next daemon restart.
/// 3. Build the rewritten button list: every `ProfileButton` whose
///    index matches an active soft-macro gets its action replaced
///    with `ButtonAction::key(trampoline)`; the rest are passed
///    through unchanged.
/// 4. Install the per-device trampoline registry so the input task
///    can resolve incoming events.
///
/// Returns the rewritten buttons vec. Callers pass it to
/// `gamerat_ratbag::Device::apply_profile_complete` in place of
/// `&profile.buttons`. When `software_macros_enabled = false` this is
/// effectively `profile.buttons.clone()` — the per-button override
/// path is gated on the master flag.
pub async fn prepare_buttons_for_apply(
    handle: &AppHandle,
    device: &OwnedObjectPath,
    model: &str,
    profile: &gamerat_proto::GameratProfile,
) -> Vec<gamerat_proto::ProfileButton> {
    if !handle.settings.read().await.software_macros_enabled || profile.soft_macros.is_empty() {
        return profile.buttons.clone();
    }

    // Which trampoline keycodes can *this* mouse actually emit? If none,
    // the soft-macro relay can't work here — leave the firmware bindings
    // untouched rather than writing a keycode the device would mangle.
    let available = available_trampolines(model);
    if available.is_empty() {
        warn!(
            model,
            profile_id = %profile.id,
            "device advertises no emittable trampoline keycode; soft-macros inert on this device"
        );
        return profile.buttons.clone();
    }

    // Allocate trampolines on a working copy.
    let mut working = profile.soft_macros.clone();
    let before: Vec<u32> = working.iter().map(|m| m.trampoline_keycode).collect();
    allocate_missing_trampolines(&mut working, &available);
    let after: Vec<u32> = working.iter().map(|m| m.trampoline_keycode).collect();
    let mutated = before != after;

    if mutated {
        // Persist the freshly-allocated trampolines back to disk so
        // they're stable across restarts and visible to the GUI's
        // next ListProfiles call.
        let mut updated_profile = profile.clone();
        updated_profile.soft_macros = working.clone();
        let mut store = handle.profiles.write().await;
        if let Err(e) = store.upsert(updated_profile) {
            warn!(?e, profile_id = %profile.id, "couldn't upsert profile after trampoline allocation");
        } else if let Err(e) = store.save() {
            warn!(?e, "couldn't persist profile after trampoline allocation");
        }
    }

    // Rewrite the button actions for any active soft-macro.
    let mut buttons = profile.buttons.clone();
    for m in working
        .iter()
        .filter(|m| m.kind != soft_macro_kind::DISABLED)
    {
        if !trampoline_keycode::is_candidate(m.trampoline_keycode) {
            continue;
        }
        let override_action = gamerat_proto::ButtonAction::key(m.trampoline_keycode);
        if let Some(existing) = buttons.iter_mut().find(|b| b.index == m.button_index) {
            existing.action = override_action;
        } else {
            buttons.push(gamerat_proto::ProfileButton {
                index: m.button_index,
                action: override_action,
            });
        }
    }

    install_profile_registry(handle, device, &working).await;
    buttons
}

/// Build the per-device trampoline lookup table for a freshly-applied
/// profile and atomically install it under the registry's entry for
/// `device`.
pub async fn install_profile_registry(
    handle: &AppHandle,
    device: &OwnedObjectPath,
    soft_macros: &[SoftMacro],
) {
    let mut table: HashMap<u32, SoftMacroEntry> = HashMap::new();
    for m in soft_macros
        .iter()
        .filter(|m| m.kind != soft_macro_kind::DISABLED)
    {
        if !trampoline_keycode::is_candidate(m.trampoline_keycode) {
            continue;
        }
        table.insert(
            m.trampoline_keycode,
            SoftMacroEntry {
                button_index: m.button_index,
                kind: m.kind,
                keys: m.keys.clone(),
            },
        );
    }

    {
        let mut reg = handle.soft_macro_registry.write().await;
        reg.insert(device.clone(), table);
    }

    // A profile switch invalidates any held toggle: the new profile
    // owns its own per-button state. Release whatever was held via
    // uinput so apps don't see a stale press.
    let stale = {
        let mut toggles = handle.toggle_states.write().await;
        let to_release: Vec<(OwnedObjectPath, u32, Vec<u32>)> = toggles
            .iter()
            .filter(|((p, _), held)| *p == *device && **held)
            .filter_map(|((_, button_index), _)| {
                lookup_keys_for_button(soft_macros, *button_index)
                    .map(|keys| (device.clone(), *button_index, keys))
            })
            .collect();
        for (p, b, _) in &to_release {
            toggles.remove(&(p.clone(), *b));
        }
        to_release
    };

    if !stale.is_empty()
        && let Some(emitter) = handle.uinput_emitter.as_ref()
    {
        let all_keys: Vec<u32> = stale.into_iter().flat_map(|(_, _, k)| k).collect();
        emitter.lock().await.release_all(&all_keys);
    }
}

/// Apply a soft-macro by flipping its toggle state and emitting the
/// matching press/release batch via uinput. Called by the
/// input-dispatch task on each [`gamerat_input::ButtonEvent`].
pub async fn handle_button_event(handle: &AppHandle, device: &OwnedObjectPath, trampoline: u32) {
    let reg = handle.soft_macro_registry.read().await;
    let Some(device_table) = reg.get(device) else {
        return;
    };
    let Some(entry) = device_table.get(&trampoline).cloned() else {
        return;
    };
    drop(reg);

    match entry.kind {
        soft_macro_kind::STICKY_TOGGLE => emit_sticky_toggle(handle, device, &entry).await,
        other => warn!(
            kind = other,
            "unsupported soft-macro kind reached the dispatcher; dropping"
        ),
    }
}

async fn emit_sticky_toggle(handle: &AppHandle, device: &OwnedObjectPath, entry: &SoftMacroEntry) {
    let pressed = {
        let mut toggles = handle.toggle_states.write().await;
        let key = (device.clone(), entry.button_index);
        let next = !toggles.get(&key).copied().unwrap_or(false);
        toggles.insert(key, next);
        next
    };

    let Some(emitter) = handle.uinput_emitter.as_ref() else {
        warn!("soft-toggle fired but uinput emitter is unavailable");
        return;
    };
    let mut emitter = emitter.lock().await;
    if let Err(e) = emitter.emit_keys(&entry.keys, pressed) {
        warn!(?e, pressed, "uinput emit failed");
    } else {
        info!(
            button_index = entry.button_index,
            pressed,
            keys = ?entry.keys,
            "soft-toggle"
        );
    }
}

fn lookup_keys_for_button(soft_macros: &[SoftMacro], button_index: u32) -> Option<Vec<u32>> {
    soft_macros
        .iter()
        .find(|m| m.button_index == button_index)
        .map(|m| m.keys.clone())
}

/// Snapshot the current [`SoftInputState`] for the GUI status pill.
///
/// `Active` requires both halves of the pipeline: a working uinput
/// emitter *and* at least one evdev reader attached to a mouse node.
/// With only one half (typically: uinput up via the logind ACL but
/// `/dev/input/event*` denied because the user isn't in the `input`
/// group), the pipeline is entirely inert — firmware trampoline
/// presses never reach the toggle dispatcher — so we surface that
/// honestly as `Unavailable` rather than misleading the user with a
/// green pill.
pub async fn current_state(handle: &AppHandle) -> SoftInputState {
    if !handle.settings.read().await.software_macros_enabled {
        return SoftInputState::Disabled;
    }
    let emitter_ok = handle.uinput_emitter.is_some();
    let readers = handle
        .input_readers_online
        .load(std::sync::atomic::Ordering::Relaxed);
    if emitter_ok && readers > 0 {
        SoftInputState::Active
    } else {
        SoftInputState::Unavailable
    }
}

/// Build a fresh uinput emitter wrapped in `Arc<Mutex<_>>`.
///
/// Mirrors the shared-mutable-state pattern the daemon uses for
/// similar I/O. Returns `None` on uinput failure; the caller logs and
/// surfaces this via [`SoftInputState::Unavailable`].
pub fn build_uinput_emitter() -> Option<Arc<Mutex<UinputEmitter>>> {
    match UinputEmitter::new() {
        Ok(emitter) => Some(Arc::new(Mutex::new(emitter))),
        Err(e) => {
            warn!(
                ?e,
                "couldn't create uinput emitter; soft-macros will be inert"
            );
            None
        }
    }
}

/// Spawn the input-dispatch task.
///
/// Enumerates evdev nodes for every currently-connected ratbagd
/// device, builds an [`EvdevBackend`] covering all of them, and
/// forwards trampoline-keycode firings to [`handle_button_event`].
///
/// No-op when:
///
/// - software-macros are disabled in settings,
/// - `ratbag` is `None` (`--no-ratbagd`),
/// - the uinput emitter wasn't built (permissions / module missing).
///
/// Discovery is done once at spawn time. Hotplug after that point is
/// a known v1 limitation — documented in the plan.
pub async fn spawn_input_dispatch(handle: AppHandle) {
    if !handle.settings.read().await.software_macros_enabled {
        debug!("software_macros_enabled=false; skipping input-dispatch spawn");
        return;
    }
    if handle.uinput_emitter.is_none() {
        debug!("uinput emitter unavailable; skipping input-dispatch spawn");
        return;
    }

    // Resolve device list via the daemon's existing ratbag client.
    let Ok(client) = handle.ratbag_or_err() else {
        debug!("--no-ratbagd; skipping input-dispatch spawn");
        return;
    };
    let devices = match client.devices().await {
        Ok(d) => d,
        Err(e) => {
            warn!(?e, "couldn't enumerate ratbagd devices for soft-input");
            return;
        }
    };

    let mut all_nodes = Vec::new();
    for device in &devices {
        let model = device.model();
        let target = match discovery::parse_model(model) {
            Ok(t) => t,
            Err(e) => {
                warn!(model, ?e, "couldn't parse ratbagd model for soft-input");
                continue;
            }
        };
        match discovery::find_evdev_nodes(target) {
            Ok(nodes) => all_nodes.extend(nodes),
            Err(e) => warn!(model, ?e, "udev enumeration failed for soft-input"),
        }
    }

    if all_nodes.is_empty() {
        info!("no evdev nodes discovered; soft-input task not started");
        return;
    }

    let (backend, errors) = EvdevBackend::open(&all_nodes);
    let attempted = all_nodes.len();
    let permission_denied_count = errors
        .iter()
        .filter(|e| {
            matches!(
                e,
                EvdevError::Open { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied,
            )
        })
        .count();
    for e in errors {
        warn!(?e, "couldn't open evdev node for soft-input");
    }
    let opened = backend.open_count();
    if opened == 0 {
        if permission_denied_count > 0 {
            // Loud actionable warning — this is the recoverable user-
            // facing setup error, not an internal failure. The CLI's
            // `gameratctl soft-input setup` subcommand prints the same
            // remediation; the GUI surfaces it via the "Soft input"
            // pill.
            warn!(
                attempted_nodes = attempted,
                permission_denied = permission_denied_count,
                "soft-macro pipeline INERT: `/dev/input/event*` access denied. \
                 Add your user to the `input` group and log out + back in: \
                 `sudo usermod -aG input $USER`. After re-login, restart the \
                 daemon. Soft-toggles will do nothing until this is fixed."
            );
        } else {
            warn!("no evdev nodes could be opened; soft-input task not started");
        }
        handle
            .input_readers_online
            .store(0, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    handle
        .input_readers_online
        .store(opened, std::sync::atomic::Ordering::Relaxed);
    info!(nodes = opened, "soft-input dispatch online");

    let readers_counter = handle.input_readers_online.clone();
    let mut stream = backend.into_stream();
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            // Pick the first ratbagd device matching this evdev node's
            // path prefix. For v1 we route every trampoline firing to
            // *all* known ratbagd devices' registries; the lookup is
            // keyed by (device, trampoline) so non-matching devices'
            // entries return None and are skipped harmlessly.
            let reg_keys: Vec<OwnedObjectPath> = {
                let reg = handle.soft_macro_registry.read().await;
                reg.keys().cloned().collect()
            };
            for device in reg_keys {
                handle_button_event(&handle, &device, event.trampoline_keycode).await;
                // Path-matching disambiguation could land here later
                // — for now, multi-mouse setups with overlapping
                // trampoline keycodes need different profiles.
                let _ = &event.device_path;
            }
        }
        // Every per-device reader has hung up — flag the pipeline as
        // offline so the GUI pill flips back to Unavailable.
        readers_counter.store(0, std::sync::atomic::Ordering::Relaxed);
        info!("soft-input dispatch stream ended");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macro_at(button_index: u32, keys: Vec<u32>) -> SoftMacro {
        SoftMacro {
            button_index,
            kind: soft_macro_kind::STICKY_TOGGLE,
            trampoline_keycode: 0,
            keys,
        }
    }

    /// Full candidate pool, as a device that advertises everything
    /// would expose it (F13.. first, macros after).
    fn pool() -> Vec<u32> {
        trampoline_keycode::candidates().collect()
    }

    const F13: u32 = 183;

    #[test]
    fn allocator_assigns_in_preference_order() {
        let mut alloc = TrampolineAllocator::new(pool(), &[]);
        assert_eq!(alloc.allocate(), Some(F13)); // KEY_F13 first
        assert_eq!(alloc.allocate(), Some(F13 + 1)); // KEY_F14
    }

    #[test]
    fn allocator_only_hands_out_device_emittable_keycodes() {
        // A device that advertises just two F-keys never yields a third
        // (and never a KEY_MACRO it can't emit).
        let mut alloc = TrampolineAllocator::new(vec![F13, F13 + 1], &[]);
        assert_eq!(alloc.allocate(), Some(F13));
        assert_eq!(alloc.allocate(), Some(F13 + 1));
        assert_eq!(alloc.allocate(), None);
    }

    #[test]
    fn allocator_skips_already_used_slots() {
        let mut existing = macro_at(3, vec![30]);
        existing.trampoline_keycode = F13 + 2; // KEY_F15 already taken
        let mut alloc = TrampolineAllocator::new(pool(), std::slice::from_ref(&existing));
        // The used slot (+2) is skipped: F13, F14, then F16.
        assert_eq!(alloc.allocate(), Some(F13));
        assert_eq!(alloc.allocate(), Some(F13 + 1));
        assert_eq!(alloc.allocate(), Some(F13 + 3));
    }

    #[test]
    fn allocate_missing_trampolines_fills_zero_slots() {
        let mut macros = vec![
            macro_at(2, vec![30]),
            SoftMacro {
                button_index: 4,
                kind: soft_macro_kind::STICKY_TOGGLE,
                trampoline_keycode: F13 + 1,
                keys: vec![46],
            },
            macro_at(5, vec![56]),
        ];
        allocate_missing_trampolines(&mut macros, &pool());
        let assigned: Vec<u32> = macros.iter().map(|m| m.trampoline_keycode).collect();
        // Pre-assigned entry keeps its slot; new entries get unique
        // device-emittable ones that don't collide.
        assert_eq!(assigned[1], F13 + 1);
        assert!(
            assigned
                .iter()
                .all(|&k| trampoline_keycode::is_candidate(k))
        );
        assert_ne!(assigned[0], assigned[2]);
        assert_ne!(assigned[0], assigned[1]);
        assert_ne!(assigned[2], assigned[1]);
    }

    #[test]
    fn allocate_missing_reassigns_keycode_the_device_cant_emit() {
        // The G502 bug: a profile carries KEY_MACRO1 (0x290), but this
        // device only advertises F-keys. The stale keycode must be
        // reassigned to one the device can actually emit.
        let mut macros = vec![SoftMacro {
            button_index: 3,
            kind: soft_macro_kind::STICKY_TOGGLE,
            trampoline_keycode: 0x290, // KEY_MACRO1 — not emittable here
            keys: vec![2],
        }];
        let f_keys_only: Vec<u32> = (183..=194).collect();
        allocate_missing_trampolines(&mut macros, &f_keys_only);
        assert_eq!(macros[0].trampoline_keycode, F13);
    }

    #[test]
    fn allocate_skips_disabled_macros() {
        let mut macros = vec![SoftMacro {
            button_index: 2,
            kind: soft_macro_kind::DISABLED,
            trampoline_keycode: 0,
            keys: vec![],
        }];
        allocate_missing_trampolines(&mut macros, &pool());
        // Disabled entries don't get a trampoline — they're inert.
        assert_eq!(macros[0].trampoline_keycode, 0);
    }
}
