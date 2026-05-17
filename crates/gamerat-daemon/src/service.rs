//! Server-side implementation of the `org.appulsauce.GameRat1` interface.
//!
//! Methods mutate shared state behind a [`tokio::sync::RwLock`]; signals
//! are emitted by the dispatch loop, not directly from method handlers
//! (focus simulation just pushes into the synthetic backend's channel
//! and the dispatch loop emits when it observes the resulting event).

use std::sync::Arc;

use gamerat_focus::{KwinInjector, SyntheticInjector};
use gamerat_proto::{
    ButtonAction, DeviceInfo, GameEntry, GameratProfile, RatbagButton, Rule, SlotInfo, StatusInfo,
};
use gamerat_ratbag::Client as RatbagClient;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument};
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
    pub ratbag: RatbagClient,
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
}

impl AppHandle {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        rules: Arc<RwLock<RuleStore>>,
        profiles: Arc<RwLock<ProfileStore>>,
        ratbag: RatbagClient,
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
        }
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
        {
            let mut store = self.handle.profiles.write().await;
            store
                .upsert(profile)
                .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
            store
                .save()
                .map_err(|e| zbus::fdo::Error::IOError(e.to_string()))?;
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

    async fn list_devices(&self) -> zbus::fdo::Result<Vec<DeviceInfo>> {
        let devices = self.handle.ratbag.devices().await.map_err(|e| {
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
            out.push(DeviceInfo {
                object_path: d.owned_object_path(),
                name: d.name().to_owned(),
                model: d.model().to_owned(),
                active_profile,
                profile_count,
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
            .ratbag
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
        .ratbag
        .devices()
        .await
        .map_err(|e| zbus::fdo::Error::Failed(format!("ratbag devices(): {e}")))?;
    devices
        .into_iter()
        .next()
        .ok_or_else(|| zbus::fdo::Error::Failed("no ratbagd devices connected".to_owned()))
}
