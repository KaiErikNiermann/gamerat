//! Protocol definitions for the gamerat ecosystem.
//!
//! This crate is the shared vocabulary between [`gamerat-daemon`],
//! [`gamerat-cli`], and [`gamerat-gui`]. It will eventually hold:
//!
//! - The `org.appulsauce.GameRat` D-Bus interface XML and zbus proxies.
//! - Serde-serializable types for hardware-abstracted profiles, resolution
//!   steps, button mappings, LED states, and per-application rules.
//! - Version negotiation primitives for daemon ↔ client compatibility.
//!
//! Nothing lives here yet — scaffolding only.

/// Crate version, exposed for D-Bus introspection and `--version` output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
