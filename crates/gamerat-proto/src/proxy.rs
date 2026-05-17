//! Generated-style proxy trait for the `org.appulsauce.GameRat1`
//! interface. Hand-mirrors `data/dbus/org.appulsauce.GameRat1.xml` —
//! keep them in sync.
//!
//! Clients (CLI, GUI, tests) use [`GameRatProxy`] to call the daemon;
//! the daemon implements the same surface on the server side via
//! `zbus::interface` (not declared here).

use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

use crate::types::{DeviceInfo, GameEntry, GameratProfile, Rule, StatusInfo};

#[proxy(
    interface = "org.appulsauce.GameRat1",
    default_service = "org.appulsauce.GameRat1",
    default_path = "/org/appulsauce/GameRat1",
    gen_blocking = false
)]
pub trait GameRat {
    /// Inject a synthetic focus event. The daemon processes it
    /// identically to one coming from a real focus backend. Source
    /// label on `FocusChanged` is `synthetic`.
    fn simulate_focus(&self, app_id: &str, title: &str) -> zbus::Result<()>;

    /// Bridge entrypoint for the `KWin` Script. Called by
    /// `data/kwin-script/gamerat-focus/contents/code/main.js` whenever
    /// `workspace.windowActivated` fires. Source label on
    /// `FocusChanged` is `kwin`.
    fn ingest_kwin_focus(&self, app_id: &str, title: &str) -> zbus::Result<()>;

    /// Upsert a rule. Replaces any existing rule with the same glob.
    /// `profile_id` references a [`GameratProfile`] stored by the
    /// daemon; missing references are accepted but the rule will be
    /// inert until the profile is created.
    fn set_rule(&self, app_id_glob: &str, profile_id: &str) -> zbus::Result<()>;

    /// Remove the rule matching `app_id_glob`. No-op if absent.
    fn delete_rule(&self, app_id_glob: &str) -> zbus::Result<()>;

    /// Enumerate all loaded rules.
    fn list_rules(&self) -> zbus::Result<Vec<Rule>>;

    /// Enumerate ratbagd-managed devices the daemon currently sees.
    fn list_devices(&self) -> zbus::Result<Vec<DeviceInfo>>;

    /// Enumerate games discovered by the launcher scanners. Scanned
    /// once at daemon startup and cached for the process lifetime.
    fn list_games(&self) -> zbus::Result<Vec<GameEntry>>;

    /// List every user-defined software profile.
    fn list_profiles(&self) -> zbus::Result<Vec<GameratProfile>>;

    /// Fetch one profile by id. Returns a D-Bus error if absent.
    fn get_profile(&self, id: &str) -> zbus::Result<GameratProfile>;

    /// Upsert a profile (replaces any existing profile with the same id).
    fn set_profile(&self, profile: GameratProfile) -> zbus::Result<()>;

    /// Remove a profile by id. No-op if absent.
    fn delete_profile(&self, id: &str) -> zbus::Result<()>;

    /// One-shot status snapshot.
    fn status(&self) -> zbus::Result<StatusInfo>;

    /// Emitted after the daemon successfully writes `ActiveProfile` and
    /// `Commit`s on the device.
    #[zbus(signal)]
    fn profile_switched(
        &self,
        device: OwnedObjectPath,
        from_profile: u32,
        to_profile: u32,
        reason: &str,
    ) -> zbus::Result<()>;

    /// Emitted on every focus event, whether or not a rule matched.
    #[zbus(signal)]
    fn focus_changed(&self, app_id: &str, title: &str, source: &str) -> zbus::Result<()>;

    /// Daemon version string (`CARGO_PKG_VERSION`).
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;
}
