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

mod client;
mod error;
mod proxy;

pub use client::{Client, Device, Service};
pub use error::{Error, Result};
