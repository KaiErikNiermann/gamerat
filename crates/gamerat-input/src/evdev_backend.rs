//! Async reader for `/dev/input/event*` that filters trampoline
//! keycodes out and forwards them as [`ButtonEvent`]s.
//!
//! No `EVIOCGRAB`: the trampoline keys also reach other apps, but the
//! `KEY_MACRO*` range is rare-enough-to-be-fine in practice. The
//! upside is that the user's mouse keeps working unmodified if this
//! task dies — there's no exclusive-ownership cliff to fall off.

use std::path::PathBuf;

use evdev::{Device, EventSummary};
use futures::StreamExt as _;
use gamerat_proto::trampoline_keycode;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, warn};

use crate::{ButtonEvent, InputBackend, InputStream};

/// Failure modes when opening or polling an evdev node. Non-fatal:
/// the daemon logs and keeps the rest of the device's nodes running.
#[derive(Debug, Error)]
pub enum EvdevError {
    /// `evdev::Device::open` failed.
    #[error("opening {path:?}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Switching the file descriptor into non-blocking mode failed.
    #[error("setting {path:?} non-blocking: {source}")]
    SetNonblocking {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Backend that listens on a fixed set of evdev nodes.
///
/// Typically the mouse + its keyboard interface; forwards trampoline-
/// keycode presses to the daemon. Hotplug is handled by the daemon
/// reconstructing the backend when the device set changes; this layer
/// is intentionally single-shot.
#[derive(Debug)]
pub struct EvdevBackend {
    devices: Vec<Device>,
    /// Internal channel: the per-device reader tasks push into the tx
    /// half; the public [`InputStream`] consumes the rx half. We
    /// build the channel during construction so the spawn-tasks step
    /// can take owned senders without lifetimes leaking through the
    /// trait object.
    rx: mpsc::Receiver<ButtonEvent>,
    tx: mpsc::Sender<ButtonEvent>,
}

impl EvdevBackend {
    /// Open every path in `nodes`, returning errors *per node* so
    /// partially-broken setups still produce a working backend. The
    /// returned tuple's second element lists nodes that failed —
    /// callers usually just `warn!` them.
    pub fn open(nodes: &[PathBuf]) -> (Self, Vec<EvdevError>) {
        let (tx, rx) = mpsc::channel(64);
        let mut devices = Vec::with_capacity(nodes.len());
        let mut errors = Vec::new();
        for path in nodes {
            match Device::open(path) {
                Ok(device) => {
                    debug!(?path, name = ?device.name(), "opened evdev node for soft-input");
                    devices.push(device);
                }
                Err(source) => errors.push(EvdevError::Open {
                    path: path.clone(),
                    source,
                }),
            }
        }
        (Self { devices, rx, tx }, errors)
    }

    /// Number of successfully-opened evdev nodes the backend owns.
    /// Mostly useful in tests + logging.
    #[must_use]
    pub fn open_count(&self) -> usize {
        self.devices.len()
    }
}

impl InputBackend for EvdevBackend {
    fn into_stream(self) -> InputStream {
        let Self { devices, rx, tx } = self;
        for device in devices {
            spawn_reader(device, tx.clone());
        }
        // Dropping our local copy of tx means the rx side terminates
        // cleanly once every reader task exits (device disconnect /
        // permission revoked / EOF).
        drop(tx);
        Box::pin(ReceiverStream::new(rx))
    }
}

/// Spawn the per-device async reader. `evdev::Device::into_event_stream`
/// hands us a `Stream<Item = io::Result<InputEvent>>` backed by tokio's
/// `AsyncFd`, so we just poll it and forward trampoline keydowns.
fn spawn_reader(device: Device, tx: mpsc::Sender<ButtonEvent>) {
    // Resolve the path *before* moving the device into the task so the
    // logs and ButtonEvent payloads can mention it. Some evdev builds
    // don't expose a path; fall back to the device name.
    let device_path = device
        .physical_path()
        .map(str::to_owned)
        .or_else(|| device.name().map(str::to_owned))
        .unwrap_or_else(|| "<unknown>".to_owned());

    tokio::spawn(async move {
        let mut stream = match device.into_event_stream() {
            Ok(s) => s,
            Err(e) => {
                error!(device_path, ?e, "couldn't build evdev event stream");
                return;
            }
        };
        info!(device_path, "soft-input reader online");

        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    warn!(device_path, ?e, "evdev read error; ending reader");
                    break;
                }
            };
            // Only forward key-down events for keycodes in the
            // trampoline range. Key-up doesn't matter — the toggle is
            // driven by physical button presses, and a held trampoline
            // keycode is exactly the same as a tap from our point of
            // view.
            let EventSummary::Key(_, code, value) = event.destructure() else {
                continue;
            };
            if value != 1 {
                continue;
            }
            let raw = u32::from(code.code());
            if (trampoline_keycode::FIRST..=trampoline_keycode::LAST).contains(&raw)
                && tx
                    .send(ButtonEvent {
                        device_path: device_path.clone(),
                        trampoline_keycode: raw,
                    })
                    .await
                    .is_err()
            {
                debug!(device_path, "soft-input dispatcher dropped; ending reader");
                break;
            }
        }
    });
}
