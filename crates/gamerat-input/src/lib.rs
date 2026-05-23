//! Software-input pipeline backing the daemon's soft-macro feature.
//!
//! libratbag's macros are stateless and play once per press, so any
//! "press once → keys go down, press again → keys go up" semantic has
//! to live above the firmware layer. This crate is that layer:
//!
//! - [`EvdevBackend`] opens the mouse's `/dev/input/event*` nodes,
//!   reads input events through tokio async I/O, and emits
//!   [`ButtonEvent`]s for trampoline keycodes (`KEY_MACRO1..30`,
//!   exported as [`gamerat_proto::trampoline_keycode`]). The firmware
//!   binding for any button carrying a soft-macro is rewritten by the
//!   daemon to fire one of those trampoline keycodes; the events leak
//!   to other apps too, but they're niche enough that real-world
//!   collisions are vanishingly rare.
//! - [`UinputEmitter`] owns a virtual keyboard registered through
//!   `/dev/uinput`. The daemon hands it the press/release events the
//!   user's soft-macro should emit; the kernel routes those through to
//!   focused apps the same way a real keyboard would.
//! - [`discovery`] walks udev to find the `event*` nodes that belong
//!   to a given mouse (matched by HID `vendor:product` extracted from
//!   ratbagd's `Model` string).
//!
//! Test scaffolding follows the [`gamerat_focus`](https://docs.rs/gamerat-focus)
//! pattern: a [`SyntheticBackend`] driven by a channel so the daemon's
//! dispatch loop stays unit-testable without a real `/dev/input`.

pub mod discovery;
pub mod evdev_backend;
pub mod uinput;

pub use discovery::{DeviceMatch, DiscoveryError, find_evdev_nodes};
pub use evdev_backend::{EvdevBackend, EvdevError};
pub use uinput::{UinputEmitter, UinputError};

use std::pin::Pin;

use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// A single trampoline-keycode firing observed on a mouse's evdev
/// node. The daemon looks up the matching [`gamerat_proto::SoftMacro`]
/// and runs its state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ButtonEvent {
    /// Path of the evdev node that produced the event
    /// (e.g. `/dev/input/event7`). Useful in multi-device setups so
    /// the dispatcher can disambiguate identical trampoline keycodes
    /// across mice.
    pub device_path: String,
    /// Linux keycode the firmware emitted. Always within
    /// [`gamerat_proto::trampoline_keycode::FIRST`] …
    /// [`gamerat_proto::trampoline_keycode::LAST`] inclusive — the
    /// backend filters non-trampoline events out before pushing.
    pub trampoline_keycode: u32,
}

/// Boxed type alias mirroring [`gamerat_focus::FocusStream`]. Lets
/// downstream code stay generic over which backend feeds it.
pub type InputStream = Pin<Box<dyn Stream<Item = ButtonEvent> + Send>>;

/// A producer of [`ButtonEvent`]s. Concrete impls live alongside this
/// trait ([`EvdevBackend`] for real hardware, [`SyntheticBackend`] for
/// tests).
pub trait InputBackend: Send + 'static {
    /// Consume the backend and return its event stream. Called once
    /// during daemon startup.
    fn into_stream(self) -> InputStream;
}

/// Channel-fed test backend.
///
/// Mirrors `gamerat_focus::SyntheticBackend`. The daemon never wires
/// this in production; unit tests instantiate `(SyntheticInjector,
/// SyntheticBackend)` pairs and push events through the injector
/// half.
#[derive(Debug)]
pub struct SyntheticBackend {
    rx: mpsc::Receiver<ButtonEvent>,
}

/// Sender half of a [`SyntheticBackend`].
#[derive(Clone, Debug)]
pub struct SyntheticInjector {
    tx: mpsc::Sender<ButtonEvent>,
}

impl SyntheticBackend {
    /// Build a fresh injector / backend pair. Channel bound matches
    /// `gamerat_focus::SyntheticBackend` — 64 events is plenty for
    /// human-paced inputs.
    #[must_use]
    pub fn new() -> (SyntheticInjector, Self) {
        let (tx, rx) = mpsc::channel(64);
        (SyntheticInjector { tx }, Self { rx })
    }
}

impl InputBackend for SyntheticBackend {
    fn into_stream(self) -> InputStream {
        Box::pin(ReceiverStream::new(self.rx))
    }
}

impl SyntheticInjector {
    /// Push an event into the backend. Returns the channel error if
    /// the backend has been dropped — callers usually treat that as a
    /// non-issue since the test harness controls both ends.
    pub async fn push(
        &self,
        event: ButtonEvent,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<ButtonEvent>> {
        self.tx.send(event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;
    use gamerat_proto::trampoline_keycode;

    #[tokio::test]
    async fn synthetic_backend_delivers_events_in_order() {
        let (injector, backend) = SyntheticBackend::new();
        let mut stream = backend.into_stream();

        injector
            .push(ButtonEvent {
                device_path: "/dev/input/event7".to_owned(),
                trampoline_keycode: trampoline_keycode::FIRST,
            })
            .await
            .expect("push");
        injector
            .push(ButtonEvent {
                device_path: "/dev/input/event7".to_owned(),
                trampoline_keycode: trampoline_keycode::FIRST + 1,
            })
            .await
            .expect("push");

        let first = stream.next().await.expect("first event");
        assert_eq!(first.trampoline_keycode, trampoline_keycode::FIRST);
        let second = stream.next().await.expect("second event");
        assert_eq!(second.trampoline_keycode, trampoline_keycode::FIRST + 1);
    }

    #[tokio::test]
    async fn synthetic_stream_terminates_when_injector_dropped() {
        let (injector, backend) = SyntheticBackend::new();
        let mut stream = backend.into_stream();
        drop(injector);
        assert!(stream.next().await.is_none());
    }
}
