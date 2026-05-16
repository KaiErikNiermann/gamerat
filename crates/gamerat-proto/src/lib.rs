//! Protocol definitions for the gamerat ecosystem.
//!
//! Holds:
//!
//! - The wire-level Rust types ([`types`]) that mirror the D-Bus
//!   interface defined in `data/dbus/org.appulsauce.GameRat1.xml`.
//! - The hand-written zbus [`proxy`] trait, used by every client crate
//!   to call into the daemon.
//! - Stable string constants for bus name, object path, interface
//!   name, and `focus_source` discriminators.
//!
//! This crate has **no runtime cost** — it pulls in zbus and serde for
//! their derives only. Anything that performs I/O lives in
//! `gamerat-ratbag`, `gamerat-daemon`, or `gamerat-cli`.

pub mod proxy;
pub mod types;

pub use proxy::{
    FocusChanged, FocusChangedArgs, FocusChangedStream, GameRatProxy, ProfileSwitched,
    ProfileSwitchedArgs, ProfileSwitchedStream,
};
pub use types::{
    DeviceInfo, FocusChangedEvent, ProfileSwitchedEvent, Rule, StatusInfo, focus_source,
};

/// Crate version, exposed for D-Bus introspection and `--version` output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Well-known D-Bus name the daemon claims on the session bus.
pub const BUS_NAME: &str = "org.appulsauce.GameRat1";

/// Path of the daemon's manager object.
pub const OBJECT_PATH: &str = "/org/appulsauce/GameRat1";

/// Interface name (matches [`BUS_NAME`] for the manager object).
pub const INTERFACE: &str = "org.appulsauce.GameRat1";
