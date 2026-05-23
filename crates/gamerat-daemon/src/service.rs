//! Server-side implementation of the `org.appulsauce.GameRat1` interface.
//!
//! Methods mutate shared state behind a [`tokio::sync::RwLock`]; signals
//! are emitted by the dispatch loop, not directly from method handlers
//! (focus simulation just pushes into the synthetic backend's channel
//! and the dispatch loop emits when it observes the resulting event).

use std::collections::HashMap;
use std::sync::Arc;

use gamerat_focus::{KwinInjector, SyntheticInjector};
use gamerat_proto::{
    ButtonAction, DeviceInfo, GameEntry, GameratProfile, MacroStep, ProfileLed, RatbagButton,
    RatbagLed, Rule, SlotInfo, StatusInfo, button_action_kind, macro_event_kind,
};
use gamerat_ratbag::Client as RatbagClient;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, instrument, warn};
use zbus::zvariant::OwnedObjectPath;

use crate::allocator::SlotAllocator;
use crate::profiles::ProfileStore;
use crate::rules::RuleStore;
use crate::settings::Settings;

/// Mutable daemon-wide state shared between the D-Bus interface, the
/// dispatch loop, and the bus connection. All inner fields are cheap
/// to clone (`Arc`-shared or `Client`'s internal `Arc`).
#[derive(Clone, Debug)]
pub struct AppHandle {
    pub rules: Arc<RwLock<RuleStore>>,
    pub profiles: Arc<RwLock<ProfileStore>>,
    /// `None` when the daemon was started with `--no-ratbagd` or when
    /// ratbagd is unreachable. IPC methods that need ratbag access go
    /// through [`AppHandle::ratbag_or_err`]; the dispatch loop and
    /// DPI tracker check this directly and degrade to no-op + warn.
    /// This is also what makes the daemon survive in a packaging-smoke
    /// container without ratbagd installed.
    pub ratbag: Option<RatbagClient>,
    pub injector: SyntheticInjector,
    pub kwin: KwinInjector,
    pub status: Arc<RwLock<DaemonStatus>>,
    /// Snapshot of every launcher-scanned game on the host, taken once
    /// at startup. Immutable for now (no rescan); wrap in `RwLock` when
    /// runtime refresh lands.
    pub games: Arc<Vec<GameEntry>>,
    /// Per-process slot allocator. `None` until the dispatch loop sees
    /// its first device; built lazily then because allocator
    /// construction needs the device's `profile_count`. Wrapped in
    /// `Option` rather than failing daemon startup so the daemon can
    /// run useful (status, rules CRUD, profile CRUD) even with no
    /// mouse plugged in.
    pub allocator: Arc<RwLock<Option<SlotAllocator>>>,
    /// Daemon-wide settings (auto-switch flag, etc.). Persisted via
    /// [`Settings::save`] whenever a setter mutates it.
    pub settings: Arc<RwLock<Settings>>,
    /// Auto-disable timers spawned by [`PanicHatch`] keyed by
    /// `(device_path, button_index)`. The IPC method inserts a handle;
    /// the spawned task removes its own entry on completion; an
    /// explicit `CancelPanicHatch` aborts the handle and removes the
    /// entry. Kept in a tokio-async lock so the timer task itself can
    /// acquire it on exit without blocking the runtime.
    pub panic_hatch_timers: Arc<RwLock<HashMap<(OwnedObjectPath, u32), JoinHandle<()>>>>,
}

impl AppHandle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rules: Arc<RwLock<RuleStore>>,
        profiles: Arc<RwLock<ProfileStore>>,
        ratbag: Option<RatbagClient>,
        injector: SyntheticInjector,
        kwin: KwinInjector,
        status: Arc<RwLock<DaemonStatus>>,
        games: Arc<Vec<GameEntry>>,
        allocator: Arc<RwLock<Option<SlotAllocator>>>,
        settings: Arc<RwLock<Settings>>,
    ) -> Self {
        Self {
            rules,
            profiles,
            ratbag,
            injector,
            kwin,
            status,
            games,
            allocator,
            settings,
            panic_hatch_timers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Borrow the ratbag client or return a `NotSupported` D-Bus
    /// error. IPC methods funnel through this so clients get a clear,
    /// machine-distinguishable error when the daemon is running in
    /// `--no-ratbagd` mode (vs. transient ratbagd-side failures, which
    /// stay as `Failed`).
    pub fn ratbag_or_err(&self) -> zbus::fdo::Result<&RatbagClient> {
        self.ratbag.as_ref().ok_or_else(|| {
            zbus::fdo::Error::NotSupported(
                "ratbagd integration disabled (daemon started with --no-ratbagd, \
                 or ratbagd is unreachable)"
                    .to_owned(),
            )
        })
    }
}

/// Snapshot of what the daemon is currently doing. Reads (via `Status`)
/// take a read lock; the dispatch loop updates it on each focus event.
#[derive(Clone, Debug, Default)]
pub struct DaemonStatus {
    pub focused_app_id: String,
    pub last_switch_reason: String,
}

/// The `#[interface]` impl mounted at `/org/appulsauce/GameRat1`.
#[derive(Debug)]
pub struct GameRatService {
    handle: AppHandle,
}

impl GameRatService {
    #[must_use]
    pub const fn new(handle: AppHandle) -> Self {
        Self { handle }
    }
}

#[zbus::interface(name = "org.appulsauce.GameRat1")]
impl GameRatService {
    /// Inject a synthetic focus event. The daemon's dispatch loop
    /// will emit `FocusChanged` and (if a rule matches) `ProfileSwitched`.
    #[instrument(skip(self), name = "SimulateFocus")]
    async fn simulate_focus(&self, app_id: &str, title: &str) -> zbus::fdo::Result<()> {
        self.handle
            .injector
            .push(app_id.to_owned(), title.to_owned())
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("synthetic backend closed: {e}")))?;
        Ok(())
    }

    /// Bridge entrypoint for the `KWin` Script. The script (which runs
    /// inside the compositor) calls this on every `windowActivated`
    /// signal. The event is tagged `source = "kwin"` downstream.
    #[instrument(skip(self), name = "IngestKwinFocus")]
    async fn ingest_kwin_focus(&self, app_id: &str, title: &str) -> zbus::fdo::Result<()> {
        self.handle
            .kwin
            .push(app_id.to_owned(), title.to_owned())
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("kwin backend closed: {e}")))?;
        Ok(())
    }

    #[instrument(skip(self), name = "SetRule")]
    async fn set_rule(&self, app_id_glob: &str, profile_id: &str) -> zbus::fdo::Result<()> {
        // Warn (don't reject) if the referenced profile is missing —
        // rules can legitimately be authored before profiles.
        if !profile_id.is_empty() && self.handle.profiles.read().await.get(profile_id).is_none() {
            tracing::warn!(
                profile_id,
                "rule references a profile that doesn't exist yet"
            );
        }
        {
            let mut rules = self.handle.rules.write().await;
            rules
                .upsert(app_id_glob, profile_id)
                .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
            rules
                .save()
                .map_err(|e| zbus::fdo::Error::IOError(e.to_string()))?;
        }
        debug!(app_id_glob, profile_id, "rule upserted");
        Ok(())
    }

    #[instrument(skip(self), name = "DeleteRule")]
    async fn delete_rule(&self, app_id_glob: &str) -> zbus::fdo::Result<()> {
        let mut rules = self.handle.rules.write().await;
        let removed = rules
            .delete(app_id_glob)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        if removed {
            rules
                .save()
                .map_err(|e| zbus::fdo::Error::IOError(e.to_string()))?;
        }
        drop(rules);
        Ok(())
    }

    async fn list_rules(&self) -> Vec<Rule> {
        self.handle.rules.read().await.list().to_vec()
    }

    /// Return the cached game library scanned at daemon startup.
    fn list_games(&self) -> Vec<GameEntry> {
        (*self.handle.games).clone()
    }

    // ===== Profile CRUD =====

    async fn list_profiles(&self) -> Vec<GameratProfile> {
        self.handle.profiles.read().await.list()
    }

    async fn get_profile(&self, id: &str) -> zbus::fdo::Result<GameratProfile> {
        self.handle
            .profiles
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("profile `{id}` not found")))
    }

    #[instrument(skip(self, profile), fields(id = %profile.id), name = "SetProfile")]
    async fn set_profile(&self, profile: GameratProfile) -> zbus::fdo::Result<()> {
        let profile_id = profile.id.clone();
        {
            let mut store = self.handle.profiles.write().await;
            store
                .upsert(profile)
                .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
            store
                .save()
                .map_err(|e| zbus::fdo::Error::IOError(e.to_string()))?;
        }
        // Invalidate any allocator slot currently holding this
        // profile so the next focus / manual-Apply event re-materialises
        // with the fresh content (LEDs / DPI / buttons just edited).
        // Without this, the allocator's "Cached" decision suppresses
        // the write and the hardware keeps the old materialisation.
        if let Some(alloc) = self.handle.allocator.write().await.as_mut() {
            let dirty = alloc.invalidate_content(&profile_id);
            debug!(profile_id = %profile_id, invalidated = dirty, "slot-cache invalidate after set_profile");
        }
        debug!("profile upserted");
        Ok(())
    }

    #[instrument(skip(self), name = "DeleteProfile")]
    async fn delete_profile(&self, id: &str) -> zbus::fdo::Result<()> {
        let mut store = self.handle.profiles.write().await;
        let removed = store.delete(id);
        if removed {
            store
                .save()
                .map_err(|e| zbus::fdo::Error::IOError(e.to_string()))?;
        }
        drop(store);
        Ok(())
    }

    /// Snapshot every button on a device's chosen profile.
    ///
    /// `profile_index = u32::MAX` is the "active profile" sentinel —
    /// useful for clients that don't know which slot is currently
    /// active yet (the GUI's initial load on first paint).
    #[instrument(skip(self), name = "ListButtons")]
    async fn list_buttons(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
    ) -> zbus::fdo::Result<Vec<RatbagButton>> {
        let device = self.find_device(&device_path).await?;
        if profile_index == u32::MAX {
            device
                .buttons()
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag buttons(): {e}")))
        } else {
            device
                .buttons_on_profile(profile_index)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag buttons_on_profile: {e}")))
        }
    }

    /// Snapshot the LEDs on `device_path`'s profile `profile_index`.
    /// `profile_index = u32::MAX` reads the currently-active profile.
    /// Returns an empty Vec for devices whose driver doesn't expose
    /// any LED objects — that's the same "graceful no-affordance" path
    /// the GUI uses for non-RGB mice.
    #[instrument(skip(self), name = "ListLeds")]
    async fn list_leds(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
    ) -> zbus::fdo::Result<Vec<RatbagLed>> {
        let device = self.find_device(&device_path).await?;
        if profile_index == u32::MAX {
            device
                .leds()
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag leds(): {e}")))
        } else {
            device
                .leds_on_profile(profile_index)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag leds_on_profile: {e}")))
        }
    }

    /// Force the named gamerat profile onto the device, bypassing
    /// the focus-rule pipeline. Used by the GUI's manual-mode Apply
    /// button and the CLI's `gameratctl profile apply`.
    ///
    /// Same path as a rule-matched switch (allocator decision → write
    /// or activate → emit `ProfileSwitched`) but driven directly by
    /// `profile_id`, ignoring autoswitch state and rules.
    #[instrument(skip(self), name = "ApplyProfile")]
    async fn apply_profile(
        &self,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
        profile_id: &str,
    ) -> zbus::fdo::Result<()> {
        let profile = {
            let store = self.handle.profiles.read().await;
            store.get(profile_id).cloned().ok_or_else(|| {
                zbus::fdo::Error::Failed(format!("profile `{profile_id}` not found"))
            })?
        };

        let device = first_device_or_err(&self.handle).await?;
        crate::dispatch::ensure_allocator_public(&self.handle, &device)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("ensure_allocator: {e}")))?;

        crate::dispatch::apply_profile_manual(&self.handle, &device, &emitter, &profile)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("apply_profile_manual: {e}")))?;

        Ok(())
    }

    /// Return the per-slot view for `device_path`: the gamerat
    /// profile (if any) materialised in each hardware slot, which
    /// slot is currently active, and which slot is reserved as the
    /// Desktop baseline.
    ///
    /// The active flag is recomputed on every call rather than
    /// cached — the user might've changed slots via Piper or some
    /// other tool and we want the GUI to reflect that.
    #[instrument(skip(self), name = "GetSlotMap")]
    async fn get_slot_map(&self, device_path: OwnedObjectPath) -> zbus::fdo::Result<Vec<SlotInfo>> {
        let device = self.find_device(&device_path).await?;
        let active = device
            .active_profile_index()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("active_profile_index: {e}")))?;

        // The allocator is lazily built on first focus event. If no
        // focus event has fired yet (fresh daemon start, autoswitch
        // off, no rules) we'd return an empty Vec and the GUI would
        // sit on "Loading slot map…" forever. Build it on demand
        // here so the user gets a useful view immediately.
        crate::dispatch::ensure_allocator_public(&self.handle, &device)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("ensure_allocator: {e}")))?;

        let snapshots = self
            .handle
            .allocator
            .read()
            .await
            .as_ref()
            .map_or_else(Vec::new, crate::allocator::SlotAllocator::snapshot);

        // Cross-reference profile_id → profile_name from the
        // profile store so the GUI can render a human-readable row
        // without a second lookup.
        let profiles = self.handle.profiles.read().await;
        let out = snapshots
            .into_iter()
            .map(|snap| {
                let profile_name = if snap.profile_id.is_empty() {
                    String::new()
                } else {
                    profiles
                        .get(&snap.profile_id)
                        .map(|p| p.name.clone())
                        .unwrap_or_default()
                };
                SlotInfo {
                    index: snap.index,
                    profile_id: snap.profile_id,
                    profile_name,
                    is_active: snap.index == active,
                    is_desktop: snap.is_desktop,
                }
            })
            .collect();
        Ok(out)
    }

    /// Active DPI stage index on the device's currently-active hardware
    /// profile. Walks ratbagd's Resolution proxies and returns the one
    /// whose `IsActive` flag is set.
    ///
    /// Used by the GUI to keep the stage indicator in sync when the
    /// user cycles DPI on the mouse itself — without it the radio
    /// would stay pinned to the profile record's `active_dpi_stage`,
    /// which can drift after any DPI-up / DPI-down / DPI-cycle press.
    #[instrument(skip(self), name = "GetActiveDpiStage")]
    async fn get_active_dpi_stage(&self, device_path: OwnedObjectPath) -> zbus::fdo::Result<u32> {
        let device = self.find_device(&device_path).await?;
        device
            .active_dpi_stage_index()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("active_dpi_stage_index: {e}")))
    }

    /// Read the DPI stages + active stage index of the device's
    /// currently-active hardware profile. Drives the GUI's Base-mode
    /// DPI editor (no gamerat profile record to read from in that
    /// mode).
    #[instrument(skip(self), name = "GetActiveProfileDpi")]
    async fn get_active_profile_dpi(
        &self,
        device_path: OwnedObjectPath,
    ) -> zbus::fdo::Result<(Vec<u32>, u32)> {
        let device = self.find_device(&device_path).await?;
        device
            .active_profile_dpi()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("active_profile_dpi: {e}")))
    }

    /// Per-resolution-slot answer to "can this slot be hardware-disabled?".
    /// Returns one `bool` per DPI slot on the device's currently-active
    /// profile; entry `i` is `true` iff slot `i` declares
    /// `RATBAG_RESOLUTION_CAP_DISABLE`. The GUI consults this before
    /// offering a "shorten the DPI cycle" affordance — without the cap,
    /// shortening the profile array doesn't remove the slot from the
    /// firmware-internal cycle.
    #[instrument(skip(self), name = "GetDpiStageDisableCaps")]
    async fn get_dpi_stage_disable_caps(
        &self,
        device_path: OwnedObjectPath,
    ) -> zbus::fdo::Result<Vec<bool>> {
        let device = self.find_device(&device_path).await?;
        device
            .resolution_disable_caps()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("resolution_disable_caps: {e}")))
    }

    /// Write DPI + button bindings to the device's currently-active
    /// hardware profile in one batched commit. Used by the GUI's
    /// Base-mode editor (DPI stage edits, Reset to defaults) — one
    /// round-trip, one firmware jitter, rather than N per-button +
    /// per-stage round-trips.
    ///
    /// `buttons` / `leds` may be empty to skip those writes — useful
    /// when the user is only tweaking DPI or only changing a single
    /// affordance.
    #[instrument(skip(self, dpi, buttons, leds), name = "ApplyToActiveProfile")]
    async fn apply_to_active_profile(
        &self,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
        device_path: OwnedObjectPath,
        dpi: Vec<u32>,
        active_stage: u32,
        buttons: Vec<gamerat_proto::ProfileButton>,
        leds: Vec<ProfileLed>,
    ) -> zbus::fdo::Result<()> {
        let device = self.find_device(&device_path).await?;
        let active_idx = device
            .active_profile_index()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("active_profile_index: {e}")))?;

        crate::dispatch::emit_profile_switching_for_path(
            &emitter,
            device.owned_object_path(),
            active_idx,
            "manual:base-edit",
        )
        .await;

        device
            .apply_profile_complete(active_idx, &dpi, active_stage, &buttons, &leds)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("apply_profile_complete: {e}")))?;

        // Same slot before+after, so no ProfileSwitched fires for a
        // pure base-mode edit. The GUI's switching badge clears via a
        // short-lived timeout when the spinner has been up for at
        // least the min-hold; we still emit a Switched signal here so
        // the indicator clears properly.
        crate::dispatch::emit_profile_switched_for_path(
            &emitter,
            device.owned_object_path(),
            active_idx,
            active_idx,
            "manual:base-edit",
        )
        .await;
        Ok(())
    }

    /// Force the device back to its reserved Desktop slot (the
    /// canonical no-game baseline). Used by the GUI's manual-mode
    /// "Apply Base" affordance — without it the only way to leave a
    /// game profile is to flip autoswitch on and focus a non-rule
    /// window.
    ///
    /// Idempotent if Desktop is already active. Emits `ProfileSwitched`
    /// with reason `manual:base` so the slot map and dev log update.
    #[instrument(skip(self), name = "ApplyBase")]
    async fn apply_base(
        &self,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let device = first_device_or_err(&self.handle).await?;
        crate::dispatch::ensure_allocator_public(&self.handle, &device)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("ensure_allocator: {e}")))?;

        let desktop = {
            let alloc = self.handle.allocator.read().await;
            alloc
                .as_ref()
                .map(SlotAllocator::desktop_slot)
                .ok_or_else(|| zbus::fdo::Error::Failed("allocator not initialised".to_owned()))?
        };

        let from = device.active_profile_index().await.unwrap_or(u32::MAX);
        if from == desktop {
            // Already on Desktop — emit anyway so the GUI's slot-map
            // revision bumps and any "currently active" highlights
            // refresh in case they drifted.
            crate::dispatch::emit_profile_switched_for_path(
                &emitter,
                device.owned_object_path(),
                from,
                desktop,
                "manual:base",
            )
            .await;
            return Ok(());
        }

        crate::dispatch::emit_profile_switching_for_path(
            &emitter,
            device.owned_object_path(),
            desktop,
            "manual:base",
        )
        .await;

        device
            .set_active_profile(desktop)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("set_active_profile: {e}")))?;

        crate::dispatch::emit_profile_switched_for_path(
            &emitter,
            device.owned_object_path(),
            from,
            desktop,
            "manual:base",
        )
        .await;
        crate::dispatch::notify_profile_switch_with(
            &self.handle,
            emitter.connection(),
            "Base",
            "manual:base",
        )
        .await;
        Ok(())
    }

    /// Write a new binding to one button. `profile_index = u32::MAX`
    /// targets the currently active profile.
    #[instrument(skip(self, action), name = "SetButton")]
    async fn set_button(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
        button_index: u32,
        action: ButtonAction,
    ) -> zbus::fdo::Result<()> {
        let device = self.find_device(&device_path).await?;
        if profile_index == u32::MAX {
            device
                .set_button(button_index, &action)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("set_button: {e}")))?;
        } else {
            device
                .set_button_on_profile(profile_index, button_index, &action)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("set_button_on_profile: {e}")))?;
        }
        Ok(())
    }

    /// Pure analysis: which keycodes does this macro leave pressed?
    /// Wraps [`gamerat_proto::macro_balance`] so the GUI can ask
    /// without porting the walker logic to TypeScript.
    #[instrument(skip(self, steps), name = "CheckMacroBalance")]
    // zbus interface methods must take &self even when they don't read
    // it. The owned-Vec argument matches the wire format zbus decodes
    // into; borrowing wouldn't shave anything meaningful.
    #[allow(clippy::unused_self, clippy::needless_pass_by_value)]
    fn check_macro_balance(&self, steps: Vec<MacroStep>) -> Vec<u32> {
        gamerat_proto::macro_balance(&steps).stuck_keys
    }

    /// Recover from a stuck-key situation on `button`. See the
    /// `PanicHatch` doc in `data/dbus/org.appulsauce.GameRat1.xml` for
    /// the two-phase behaviour and refusal semantics.
    #[instrument(skip(self), name = "PanicHatch")]
    async fn panic_hatch(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        device: OwnedObjectPath,
        button: u32,
    ) -> zbus::fdo::Result<(Vec<u32>, bool)> {
        let ratbag_device = self.find_device(&device).await?;
        let buttons = ratbag_device
            .buttons()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag buttons(): {e}")))?;
        let target = buttons.iter().find(|b| b.index == button).ok_or_else(|| {
            zbus::fdo::Error::Failed(format!("button {button} not present on device"))
        })?;

        if is_essential_button(target) {
            return Err(zbus::fdo::Error::Failed(format!(
                "refusing to panic-hatch essential button {button}"
            )));
        }

        let balance = gamerat_proto::macro_balance(&target.action.macro_steps);
        let stuck_keys = balance.stuck_keys;

        // Nothing to release → straight to NONE. Common path when the
        // user clicked Panic on a non-macro binding "just to clear it".
        if target.action.kind != button_action_kind::MACRO || stuck_keys.is_empty() {
            ratbag_device
                .set_button(button, &ButtonAction::none())
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("set_button(none): {e}")))?;
            return Ok((Vec::new(), false));
        }

        // Stuck keys identified — bind a release-only macro and arm
        // the auto-disable timer. The user gets a 5s window to press
        // the button (firing the release events) before the binding is
        // wiped regardless.
        let release_steps: Vec<MacroStep> = stuck_keys
            .iter()
            .map(|k| MacroStep {
                kind: macro_event_kind::KEY_RELEASE,
                value: *k,
            })
            .collect();
        ratbag_device
            .set_button(button, &ButtonAction::macro_action(release_steps))
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("set_button(release): {e}")))?;

        spawn_panic_hatch_timer(self.handle.clone(), conn.clone(), device.clone(), button).await;
        Ok((stuck_keys, true))
    }

    /// Abort a pending panic-hatch auto-disable timer. No-op if no
    /// timer is armed. Emits `PanicHatchSettled` with
    /// `"cancelled"` so any listener (GUI countdown modal) can close
    /// cleanly.
    #[instrument(skip(self), name = "CancelPanicHatch")]
    async fn cancel_panic_hatch(
        &self,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
        device: OwnedObjectPath,
        button: u32,
    ) -> zbus::fdo::Result<()> {
        let removed = self
            .handle
            .panic_hatch_timers
            .write()
            .await
            .remove(&(device.clone(), button));
        if let Some(handle) = removed {
            handle.abort();
            emit_panic_hatch_settled(&emitter, device, button, "cancelled").await;
        }
        Ok(())
    }

    /// Write one LED's state (mode + color + brightness) into a
    /// profile + Commit. `profile_index = u32::MAX` targets the
    /// currently active profile. Used by the GUI's per-LED Apply
    /// button in base mode and by `gameratctl led set`.
    #[instrument(skip(self, led), name = "SetLed")]
    async fn set_led(
        &self,
        device_path: OwnedObjectPath,
        profile_index: u32,
        led_index: u32,
        led: ProfileLed,
    ) -> zbus::fdo::Result<()> {
        let device = self.find_device(&device_path).await?;
        if profile_index == u32::MAX {
            device
                .set_led(led_index, &led)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("set_led: {e}")))?;
        } else {
            device
                .set_led_on_profile(profile_index, led_index, &led)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("set_led_on_profile: {e}")))?;
        }
        Ok(())
    }

    async fn list_devices(&self) -> zbus::fdo::Result<Vec<DeviceInfo>> {
        let devices = self.handle.ratbag_or_err()?.devices().await.map_err(|e| {
            error!(error = ?e, "ratbag devices() failed");
            zbus::fdo::Error::Failed(format!("ratbag query failed: {e}"))
        })?;

        let mut out = Vec::with_capacity(devices.len());
        for d in devices {
            let active_profile = d
                .active_profile_index()
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("active_profile_index: {e}")))?;
            let profile_count = d
                .profile_count()
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("profile_count: {e}")))?;
            let max_dpi_stages = d
                .max_dpi_stages()
                .await
                .map_err(|e| zbus::fdo::Error::Failed(format!("max_dpi_stages: {e}")))?;
            out.push(DeviceInfo {
                object_path: d.owned_object_path(),
                name: d.name().to_owned(),
                model: d.model().to_owned(),
                active_profile,
                profile_count,
                max_dpi_stages,
            });
        }
        Ok(out)
    }

    async fn status(&self) -> StatusInfo {
        let status = self.handle.status.read().await.clone();
        let rules_loaded =
            u32::try_from(self.handle.rules.read().await.list().len()).unwrap_or(u32::MAX);
        StatusInfo {
            focused_app_id: status.focused_app_id,
            last_switch_reason: status.last_switch_reason,
            rules_loaded,
        }
    }

    /// Probe the KDE focus-bridge health without changing anything.
    /// Returns one of [`gamerat_proto::focus_bridge`]. The connection
    /// is the live session bus, used to query `org.kde.KWin`.
    #[instrument(skip(self, conn), name = "CheckFocusBridge")]
    async fn check_focus_bridge(&self, #[zbus(connection)] conn: &zbus::Connection) -> String {
        crate::kwin_bridge::check(conn).await.as_wire().to_owned()
    }

    /// Install + enable + load the `gamerat-focus` `KWin` script
    /// (idempotent), returning the resulting [`gamerat_proto::focus_bridge`]
    /// state. Backs the GUI's "Repair" button.
    #[instrument(skip(self, conn), name = "EnsureKwinFocusBridge")]
    async fn ensure_kwin_focus_bridge(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> String {
        crate::kwin_bridge::ensure(conn).await.as_wire().to_owned()
    }

    #[zbus(signal)]
    pub async fn profile_switching(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device: OwnedObjectPath,
        to_profile: u32,
        reason: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn active_dpi_stage_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device: OwnedObjectPath,
        stage: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn profile_switched(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device: OwnedObjectPath,
        from_profile: u32,
        to_profile: u32,
        reason: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn focus_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        app_id: &str,
        title: &str,
        source: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn panic_hatch_settled(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device: OwnedObjectPath,
        button: u32,
        outcome: &str,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    #[allow(clippy::unused_self)] // zbus interface methods require &self.
    fn version(&self) -> String {
        gamerat_proto::VERSION.to_owned()
    }

    /// When `false`, the dispatch loop still emits `FocusChanged` but
    /// suppresses the rule-driven profile switch — profile changes
    /// become purely manual.
    #[zbus(property)]
    async fn auto_switch_enabled(&self) -> bool {
        self.handle.settings.read().await.auto_switch_enabled
    }

    #[zbus(property)]
    async fn set_auto_switch_enabled(&self, value: bool) -> zbus::Result<()> {
        // zbus property setters demand `zbus::Error`, not `fdo::Error`
        // — wrap any save() failure via the Failure variant rather
        // than panicking; the client gets a clear D-Bus error back.
        let result = {
            let mut s = self.handle.settings.write().await;
            s.auto_switch_enabled = value;
            s.save()
        };
        result.map_err(|e| zbus::Error::Failure(format!("save settings: {e}")))?;
        debug!(value, "auto-switch toggled");
        Ok(())
    }

    /// When `false`, focusing a window with no matching rule keeps
    /// the current profile active. Useful for users who don't curate
    /// the Desktop slot but still want autoswitching between games.
    #[zbus(property)]
    async fn desktop_return_enabled(&self) -> bool {
        self.handle.settings.read().await.desktop_return_enabled
    }

    #[zbus(property)]
    async fn set_desktop_return_enabled(&self, value: bool) -> zbus::Result<()> {
        let result = {
            let mut s = self.handle.settings.write().await;
            s.desktop_return_enabled = value;
            s.save()
        };
        result.map_err(|e| zbus::Error::Failure(format!("save settings: {e}")))?;
        debug!(value, "desktop-return toggled");
        Ok(())
    }

    /// Debounce window (ms) before Desktop fallback fires after a
    /// no-rule-match focus event. Brief tab-outs (Discord, Google)
    /// shorter than this delay don't kick the profile back.
    #[zbus(property)]
    async fn desktop_return_delay_ms(&self) -> u64 {
        self.handle.settings.read().await.desktop_return_delay_ms
    }

    #[zbus(property)]
    async fn set_desktop_return_delay_ms(&self, value: u64) -> zbus::Result<()> {
        let result = {
            let mut s = self.handle.settings.write().await;
            s.desktop_return_delay_ms = value;
            s.save()
        };
        result.map_err(|e| zbus::Error::Failure(format!("save settings: {e}")))?;
        debug!(value, "desktop-return delay set");
        Ok(())
    }

    /// When `true`, the GUI raises a system notification on each
    /// profile switch. Off by default — gamers in fullscreen tend to
    /// find notifications more disruptive than useful.
    #[zbus(property)]
    async fn notify_on_profile_switch(&self) -> bool {
        self.handle.settings.read().await.notify_on_profile_switch
    }

    #[zbus(property)]
    async fn set_notify_on_profile_switch(&self, value: bool) -> zbus::Result<()> {
        let result = {
            let mut s = self.handle.settings.write().await;
            s.notify_on_profile_switch = value;
            s.save()
        };
        result.map_err(|e| zbus::Error::Failure(format!("save settings: {e}")))?;
        debug!(value, "notify-on-profile-switch toggled");
        Ok(())
    }
}

impl GameRatService {
    /// Resolve a `ratbagd`-issued object path to the matching
    /// [`gamerat_ratbag::Device`]. Errors when no device on the bus
    /// uses that path — usually because the device was unplugged
    /// between the client's `list_devices` and a follow-up call.
    async fn find_device(
        &self,
        device_path: &OwnedObjectPath,
    ) -> zbus::fdo::Result<gamerat_ratbag::Device> {
        let devices = self
            .handle
            .ratbag_or_err()?
            .devices()
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag devices(): {e}")))?;
        devices
            .into_iter()
            .find(|d| d.owned_object_path() == *device_path)
            .ok_or_else(|| {
                zbus::fdo::Error::UnknownObject(format!("no ratbagd device at {device_path:?}"))
            })
    }
}

/// Convenience for IPC methods that don't take a device path (e.g.
/// `ApplyProfile` — single-device targeting matches the dispatch
/// loop). Returns the first device or a clear error. Lives outside
/// `GameRatService` so the dispatch helpers can reuse it.
async fn first_device_or_err(
    handle: &crate::service::AppHandle,
) -> zbus::fdo::Result<gamerat_ratbag::Device> {
    let devices = handle
        .ratbag_or_err()?
        .devices()
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag devices(): {e}")))?;
    devices
        .into_iter()
        .next()
        .ok_or_else(|| zbus::fdo::Error::Failed("no ratbagd devices connected".to_owned()))
}

/// Window the user has to fire the release-only macro by pressing the
/// affected button before the panic-hatch auto-disables the binding.
const PANIC_HATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Refuse panic-hatch on buttons that look like primary mouse buttons.
/// Two signals stack into one decision:
///   1. Index 0–2 still bound to MOUSE → factory left/middle/right.
///      Tightly conventional but covers the common case fast.
///   2. The button's firmware-declared `supported_action_types` does
///      not list NONE → ratbagd / libratbag itself refuses to disable
///      the button. Trying anyway would fail loudly downstream; better
///      to refuse with a clear message here.
fn is_essential_button(button: &RatbagButton) -> bool {
    let primary_by_index = button.index <= 2
        && button.action.kind == button_action_kind::MOUSE
        && button.action.value <= 2;
    let firmware_locked = !button
        .supported_action_types
        .contains(&button_action_kind::NONE);
    primary_by_index || firmware_locked
}

/// Spawn the auto-disable timer for a freshly-armed panic-hatch. The
/// task self-removes its entry from `handle.panic_hatch_timers` after
/// running, so the map stays bounded. Idempotent against an already-
/// armed timer for the same `(device, button)`: any previous handle is
/// aborted and replaced (re-running panic-hatch resets the clock).
async fn spawn_panic_hatch_timer(
    handle: AppHandle,
    conn: zbus::Connection,
    device: OwnedObjectPath,
    button: u32,
) {
    let key = (device.clone(), button);
    let timer_handle = handle.clone();
    let timer_conn = conn.clone();
    let timer_device = device.clone();
    let join: JoinHandle<()> = tokio::spawn(async move {
        tokio::time::sleep(PANIC_HATCH_TIMEOUT).await;

        // Make the cleanup write best-effort: the user may have already
        // rebound the button by hand from the GUI in the meantime, and
        // a stale NONE write would clobber that. Read the current
        // binding first and skip if it's no longer our release-only
        // macro.
        let outcome = match clear_release_macro_if_ours(&timer_handle, &timer_device, button).await
        {
            Ok(true) => "timeout_disabled",
            Ok(false) => "superseded",
            Err(e) => {
                warn!(?e, ?timer_device, button, "panic-hatch auto-disable failed");
                "timeout_disabled"
            }
        };

        // Remove our entry before emitting so a listener that triggers
        // another panic-hatch on receipt doesn't race against our own
        // handle still being in the map.
        timer_handle
            .panic_hatch_timers
            .write()
            .await
            .remove(&(timer_device.clone(), button));

        match timer_conn
            .object_server()
            .interface::<_, GameRatService>(gamerat_proto::OBJECT_PATH)
            .await
        {
            Ok(iface_ref) => {
                emit_panic_hatch_settled(iface_ref.signal_emitter(), timer_device, button, outcome)
                    .await;
            }
            Err(e) => warn!(
                ?e,
                "couldn't look up GameRatService to emit PanicHatchSettled"
            ),
        }
    });

    let mut map = handle.panic_hatch_timers.write().await;
    if let Some(prev) = map.insert(key, join) {
        prev.abort();
    }
}

/// Rewrite the button binding to NONE only if the active binding is
/// still the release-only macro we installed when panic-hatch fired.
/// Returns `Ok(true)` if we cleared it, `Ok(false)` if the user (or
/// another path) replaced it in the meantime.
async fn clear_release_macro_if_ours(
    handle: &AppHandle,
    device: &OwnedObjectPath,
    button: u32,
) -> zbus::fdo::Result<bool> {
    let ratbag = handle.ratbag_or_err()?;
    let devices = ratbag
        .devices()
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag devices(): {e}")))?;
    let Some(ratbag_device) = devices
        .into_iter()
        .find(|d| d.owned_object_path() == *device)
    else {
        // Device disappeared between arming and timer fire — nothing
        // to clean up.
        return Ok(false);
    };
    let buttons = ratbag_device
        .buttons()
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag buttons(): {e}")))?;
    let Some(target) = buttons.into_iter().find(|b| b.index == button) else {
        return Ok(false);
    };

    let still_release_only = target.action.kind == button_action_kind::MACRO
        && !target.action.macro_steps.is_empty()
        && target
            .action
            .macro_steps
            .iter()
            .all(|s| s.kind == macro_event_kind::KEY_RELEASE);
    if !still_release_only {
        return Ok(false);
    }

    ratbag_device
        .set_button(button, &ButtonAction::none())
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("set_button(none): {e}")))?;
    Ok(true)
}

/// Public emitter wrapper so the spawned timer task and the
/// `CancelPanicHatch` handler can share one logging path.
pub async fn emit_panic_hatch_settled(
    emitter: &zbus::object_server::SignalEmitter<'_>,
    device: OwnedObjectPath,
    button: u32,
    outcome: &str,
) {
    if let Err(e) = GameRatService::panic_hatch_settled(emitter, device, button, outcome).await {
        warn!(?e, outcome, "failed to emit PanicHatchSettled");
    }
}
