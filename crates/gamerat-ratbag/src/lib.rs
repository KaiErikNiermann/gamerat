//! Ergonomic async client wrapper around the `ratbagd` D-Bus service.
//!
//! `ratbagd` exposes three interfaces on the system bus
//! (`org.freedesktop.ratbag1.Manager`, `…Device`, `…Profile`); raw
//! usage involves walking object paths, calling `SetActive` on the
//! desired profile object, then `Commit`-ing on the device.
//!
//! This crate flattens that into a small, typed API:
//!
//! ```no_run
//! # async fn demo() -> gamerat_ratbag::Result<()> {
//! let client = gamerat_ratbag::Client::connect().await?;
//! for device in client.devices().await? {
//!     println!("{} ({})", device.name(), device.model());
//!     device.set_active_profile(0).await?;
//! }
//! # Ok(()) }
//! ```
//!
//! ## Variant selection
//!
//! [`Client::connect`] talks to production ratbagd. For integration
//! tests against the locally-built `ratbagd.devel`, use
//! [`Client::connect_to`] with [`Service::Devel`]; that variant also
//! exposes [`Client::load_test_device`] for spawning virtual mice.
//!
//! ## What's *not* here yet
//!
//! Resolutions, buttons, LEDs, and report-rate configuration are
//! deliberately out of scope until the daemon needs them. The current
//! MVP only swaps the active profile.

pub mod button;
pub mod caps;
mod client;
mod error;
mod proxy;

pub use client::{Client, Device, Service};
pub use error::{Error, Result};

/// One-shot ratbagd compatibility probe.
///
/// Connects to production ratbagd on the system bus, reads
/// `Manager.APIVersion`, classifies it via [`gamerat_proto::compat`],
/// and disposes of the connection. Returns `Ok(None)` when ratbagd
/// isn't reachable — useful for CLI banners that should gracefully
/// say "ratbagd not running" rather than aborting.
pub async fn probe_compat() -> Result<Option<gamerat_proto::Compat>> {
    match Client::connect().await {
        Ok(client) => Ok(Some(gamerat_proto::classify_compat(
            client.api_version().await?,
        ))),
        Err(Error::NotConnected(_)) => Ok(None),
        Err(other) => Err(other),
    }
}
