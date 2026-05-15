//! Ergonomic client wrapper around the `ratbagd` D-Bus service.
//!
//! `ratbagd`'s wire protocol is functional but low-level: profiles are
//! object paths, resolutions are nested children, and mutating state
//! involves a `Commit` dance per device. This crate exposes a typed,
//! async, snapshot-oriented API that the rest of gamerat builds on.
//!
//! Planned surface area:
//!
//! - `RatbagClient::connect()` → discovers devices, returns handles.
//! - `Device::snapshot()` / `Device::apply(snapshot)` — read/write whole
//!   profile state without manual `Commit` choreography.
//! - Signal streams for hotplug and external-write events.
//!
//! Scaffolding only.
