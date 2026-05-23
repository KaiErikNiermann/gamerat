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
//! Trampoline keycodes are allocated by [`TrampolineAllocator::allocate_for_profile`]
//! at profile-apply time. Allocations are persisted *into* the profile
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

/// Allocator for `KEY_MACRO1..30` slots within a single profile-apply
/// batch. Lives only for the duration of one apply call.
#[derive(Debug)]
pub struct TrampolineAllocator {
    used: HashSet<u32>,
    cursor: u32,
}

impl TrampolineAllocator {
    /// Build an allocator seeded with any trampoline keycodes already
    /// assigned in this profile. New soft-macros get fresh slots that
    /// don't collide with the existing ones.
    #[must_use]
    pub fn from_profile(soft_macros: &[SoftMacro]) -> Self {
        let used: HashSet<u32> = soft_macros
            .iter()
            .filter(|m| m.kind != soft_macro_kind::DISABLED)
            .map(|m| m.trampoline_keycode)
            .filter(|k| (trampoline_keycode::FIRST..=trampoline_keycode::LAST).contains(k))
            .collect();
        Self {
            used,
            cursor: trampoline_keycode::FIRST,
        }
    }

    /// Allocate the next available trampoline keycode. Returns `None`
    /// when the entire `KEY_MACRO1..30` pool is exhausted — 30 soft-
    /// macros per profile is more than any realistic mouse will need.
    pub fn allocate(&mut self) -> Option<u32> {
        while self.cursor <= trampoline_keycode::LAST {
            let candidate = self.cursor;
            self.cursor += 1;
            if self.used.insert(candidate) {
                return Some(candidate);
            }
        }
        None
    }
}

/// Allocate trampoline keycodes for fresh soft-macros.
///
/// Walks `soft_macros`, assigning trampoline keycodes to any entries
/// that don't have one yet. Profiles flow daemon → ratbagd so this is
/// called just before [`prepare_buttons_for_apply`]; the caller
/// persists the rewritten soft-macros so the allocations are stable
/// across runs.
pub fn allocate_missing_trampolines(soft_macros: &mut [SoftMacro]) {
    let mut allocator = TrampolineAllocator::from_profile(soft_macros);
    for m in soft_macros
        .iter_mut()
        .filter(|m| m.kind != soft_macro_kind::DISABLED)
    {
        if !(trampoline_keycode::FIRST..=trampoline_keycode::LAST).contains(&m.trampoline_keycode) {
            if let Some(slot) = allocator.allocate() {
                debug!(
                    button_index = m.button_index,
                    trampoline = format_args!("0x{slot:x}"),
                    "assigned trampoline keycode"
                );
                m.trampoline_keycode = slot;
            } else {
                warn!(
                    button_index = m.button_index,
                    "trampoline pool exhausted; soft-macro will be ignored"
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
    profile: &gamerat_proto::GameratProfile,
) -> Vec<gamerat_proto::ProfileButton> {
    if !handle.settings.read().await.software_macros_enabled || profile.soft_macros.is_empty() {
        return profile.buttons.clone();
    }

    // Allocate trampolines on a working copy.
    let mut working = profile.soft_macros.clone();
    let before: Vec<u32> = working.iter().map(|m| m.trampoline_keycode).collect();
    allocate_missing_trampolines(&mut working);
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
        if !(trampoline_keycode::FIRST..=trampoline_keycode::LAST).contains(&m.trampoline_keycode) {
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
        if !(trampoline_keycode::FIRST..=trampoline_keycode::LAST).contains(&m.trampoline_keycode) {
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

    #[test]
    fn allocator_assigns_unique_slots_starting_at_first() {
        let mut alloc = TrampolineAllocator::from_profile(&[]);
        let a = alloc.allocate().expect("first slot");
        let b = alloc.allocate().expect("second slot");
        assert_eq!(a, trampoline_keycode::FIRST);
        assert_eq!(b, trampoline_keycode::FIRST + 1);
    }

    #[test]
    fn allocator_skips_already_used_slots() {
        let mut existing = macro_at(3, vec![30]);
        existing.trampoline_keycode = trampoline_keycode::FIRST + 2;
        let mut alloc = TrampolineAllocator::from_profile(&[existing]);

        let a = alloc.allocate().expect("first");
        let b = alloc.allocate().expect("second");
        let c = alloc.allocate().expect("third");
        // The used slot (+2) gets skipped, so allocations land on
        // FIRST, FIRST+1, FIRST+3.
        assert_eq!(a, trampoline_keycode::FIRST);
        assert_eq!(b, trampoline_keycode::FIRST + 1);
        assert_eq!(c, trampoline_keycode::FIRST + 3);
    }

    #[test]
    fn allocator_returns_none_when_pool_exhausted() {
        let mut alloc = TrampolineAllocator::from_profile(&[]);
        for _ in 0..trampoline_keycode::COUNT {
            assert!(alloc.allocate().is_some());
        }
        assert!(alloc.allocate().is_none());
    }

    #[test]
    fn allocate_missing_trampolines_fills_zero_slots() {
        let mut macros = vec![
            macro_at(2, vec![30]),
            SoftMacro {
                button_index: 4,
                kind: soft_macro_kind::STICKY_TOGGLE,
                trampoline_keycode: trampoline_keycode::FIRST + 1,
                keys: vec![46],
            },
            macro_at(5, vec![56]),
        ];
        allocate_missing_trampolines(&mut macros);
        let assigned: Vec<u32> = macros.iter().map(|m| m.trampoline_keycode).collect();
        // Pre-assigned entry keeps its slot; new entries get unique
        // ones that don't collide.
        assert_eq!(assigned[1], trampoline_keycode::FIRST + 1);
        assert_ne!(assigned[0], 0);
        assert_ne!(assigned[2], 0);
        assert_ne!(assigned[0], assigned[2]);
        assert_ne!(assigned[0], assigned[1]);
        assert_ne!(assigned[2], assigned[1]);
    }

    #[test]
    fn allocate_skips_disabled_macros() {
        let mut macros = vec![SoftMacro {
            button_index: 2,
            kind: soft_macro_kind::DISABLED,
            trampoline_keycode: 0,
            keys: vec![],
        }];
        allocate_missing_trampolines(&mut macros);
        // Disabled entries don't get a trampoline — they're inert.
        assert_eq!(macros[0].trampoline_keycode, 0);
    }
}
