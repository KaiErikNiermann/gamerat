//! Server-side implementation of the `org.appulsauce.GameRat1` interface.
//!
//! Methods mutate shared state behind a [`tokio::sync::RwLock`]; signals
//! are emitted by the dispatch loop, not directly from method handlers
//! (focus simulation just pushes into the synthetic backend's channel
//! and the dispatch loop emits when it observes the resulting event).

use std::sync::Arc;

use gamerat_focus::{KwinInjector, SyntheticInjector};
use gamerat_proto::{DeviceInfo, GameEntry, GameratProfile, Rule, StatusInfo};
use gamerat_ratbag::Client as RatbagClient;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument};
use zbus::zvariant::OwnedObjectPath;

use crate::profiles::ProfileStore;
use crate::rules::RuleStore;

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
}

impl AppHandle {
    pub const fn new(
        rules: Arc<RwLock<RuleStore>>,
        profiles: Arc<RwLock<ProfileStore>>,
        ratbag: RatbagClient,
        injector: SyntheticInjector,
        kwin: KwinInjector,
        status: Arc<RwLock<DaemonStatus>>,
        games: Arc<Vec<GameEntry>>,
    ) -> Self {
        Self {
            rules,
            profiles,
            ratbag,
            injector,
            kwin,
            status,
            games,
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
    async fn set_rule(&self, app_id_glob: &str, profile_index: u32) -> zbus::fdo::Result<()> {
        {
            let mut rules = self.handle.rules.write().await;
            rules
                .upsert(app_id_glob, profile_index)
                .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
            rules
                .save()
                .map_err(|e| zbus::fdo::Error::IOError(e.to_string()))?;
        }
        debug!(app_id_glob, profile_index, "rule upserted");
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
}
