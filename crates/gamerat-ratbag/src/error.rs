//! Error and Result types for the gamerat-ratbag client.

use thiserror::Error;
use zbus::zvariant::OwnedObjectPath;

#[derive(Debug, Error)]
pub enum Error {
    /// Anything raised by zbus or its underlying I/O.
    #[error("D-Bus error: {0}")]
    Dbus(#[from] zbus::Error),

    /// The expected ratbagd service didn't claim its name on the bus.
    /// Most commonly: production ratbagd isn't running, or we're
    /// targeting the devel variant but `ratbagd.devel` isn't up.
    #[error("ratbagd service `{0}` not present on the system bus")]
    NotConnected(String),

    /// `Device::set_active_profile(idx)` was called with an index that
    /// no profile slot on the device matches.
    #[error("device {device} has no profile with index {index}")]
    NoSuchProfile { device: OwnedObjectPath, index: u32 },

    /// ratbagd returned a non-zero status from a `Commit` or
    /// `SetActive` call. The carried integer is ratbagd's own status —
    /// it's not always a POSIX errno but it's always non-zero for a
    /// failure.
    #[error("ratbagd reported status {status} from {op}")]
    Ratbagd { op: &'static str, status: u32 },

    /// A ratbagd property had a wire shape we don't understand —
    /// usually a sign that ratbagd's API has drifted (in which case
    /// [`scripts/check_ratbagd_drift.py`] should also be unhappy).
    #[error("malformed ratbagd payload: {0}")]
    MalformedPayload(&'static str),
}

impl Error {
    /// Shorthand for the common `MalformedPayload` case so call sites
    /// don't need to import the variant.
    #[must_use]
    pub const fn ratbagd_op(msg: &'static str) -> Self {
        Self::MalformedPayload(msg)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
