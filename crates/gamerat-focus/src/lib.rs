//! Active-window / focus tracking across Linux desktop stacks.
//!
//! The daemon needs to answer "which app is the user looking at right
//! now?" so it can match against per-application profile rules. This
//! crate hides the absolute zoo of focus APIs behind one trait
//! ([`FocusBackend`]) yielding a stream of [`FocusEvent`]s.
//!
//! ## What ships now
//!
//! | Backend                       | Source                                          |
//! | ----------------------------- | ----------------------------------------------- |
//! | [`SyntheticBackend`]          | `gameratctl focus simulate` / `SimulateFocus`   |
//! | [`KwinBackend`]               | `data/kwin-script/` + `IngestKwinFocus`         |
//! | [`WlrForeignToplevelBackend`] | `zwlr_foreign_toplevel_manager_v1`              |
//! | [`FixtureReplayBackend`]      | TOML fixture, recorded earlier; for tests/demos |
//!
//! The synthetic backend is always available — the daemon attaches it
//! in every mode (except pure-replay) so `gameratctl focus simulate`
//! keeps working alongside real focus tracking. `KwinBackend` and
//! `WlrForeignToplevelBackend` are picked by `--backend auto` based on
//! what the running compositor advertises; `FixtureReplayBackend` is
//! engaged with `--replay-fixture <path>`.
//!
//! ## What's coming
//!
//! An X11 backend reading `_NET_ACTIVE_WINDOW`, behind the same trait,
//! for users on legacy desktops. Same `FocusEvent` shape; drop-in.

pub mod fixture;
pub mod wlr;

pub use fixture::{FixtureError, FixtureFile, FixtureMeta, FixtureReplayBackend, RecordedEvent};
pub use wlr::{WlrError, WlrForeignToplevelBackend};

use std::pin::Pin;

use futures::Stream;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::trace;

/// A single focus event: "this window is now active." Backends collapse
/// their wire-level protocol semantics (wlr's `done`-atom batching,
/// X11's `PropertyNotify`, etc.) into these.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusEvent {
    /// Application identifier. For wlr-foreign-toplevel this is the
    /// `app_id` field; for X11 the `WM_CLASS` instance; Steam Proton
    /// apps come through as `steam_app_<appid>`.
    pub app_id: String,
    /// Window title. May be empty if the backend can't read it (some
    /// fullscreen exclusive modes).
    pub title: String,
    /// Which backend produced this event.
    pub source: BackendKind,
}

/// Identifies which backend produced a [`FocusEvent`]. Mirrors the
/// wire-stable strings in [`gamerat_proto::focus_source`] — call
/// [`BackendKind::as_wire`] to round-trip through D-Bus.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    /// Driven by an in-process channel (CLI's `SimulateFocus`).
    Synthetic,
    /// wlr-foreign-toplevel-management-unstable-v1.
    WlrForeignToplevel,
    /// KDE Plasma / `KWin`.
    Kwin,
    /// X11 `_NET_ACTIVE_WINDOW`.
    X11,
}

impl BackendKind {
    /// Wire-stable string for D-Bus serialization. The complement is
    /// [`Self::from_wire`].
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Synthetic => gamerat_proto::focus_source::SYNTHETIC,
            Self::WlrForeignToplevel => gamerat_proto::focus_source::WLR_FOREIGN_TOPLEVEL,
            Self::Kwin => gamerat_proto::focus_source::KWIN,
            Self::X11 => gamerat_proto::focus_source::X11,
        }
    }

    /// Parse the wire-stable string produced by [`Self::as_wire`].
    /// Returns `None` for unknown values so callers can decide whether
    /// to log-and-skip or hard-error.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            gamerat_proto::focus_source::SYNTHETIC => Some(Self::Synthetic),
            gamerat_proto::focus_source::WLR_FOREIGN_TOPLEVEL => Some(Self::WlrForeignToplevel),
            gamerat_proto::focus_source::KWIN => Some(Self::Kwin),
            gamerat_proto::focus_source::X11 => Some(Self::X11),
            _ => None,
        }
    }
}

/// Boxed type alias for the stream of focus events emitted by a
/// backend. Avoids leaking concrete `Stream` types through downstream
/// generics (the daemon doesn't care which backend it has).
pub type FocusStream = Pin<Box<dyn Stream<Item = FocusEvent> + Send>>;

/// A producer of focus events. Each concrete backend implements this;
/// the daemon spawns one and polls its stream until shutdown.
pub trait FocusBackend: Send + 'static {
    /// Identifier for the backend — used to populate
    /// [`FocusEvent::source`] and the `FocusChanged` D-Bus signal.
    fn kind(&self) -> BackendKind;

    /// Consume the backend and return its event stream. Called once
    /// during daemon startup.
    fn into_stream(self) -> FocusStream;
}

/// Failure modes when injecting a synthetic focus event.
#[derive(Debug, Error)]
pub enum InjectError {
    /// The corresponding [`SyntheticBackend`] has been dropped — no
    /// receiver to deliver to.
    #[error("synthetic backend receiver was dropped")]
    Closed,
}

/// In-process focus backend driven by [`SyntheticInjector::push`]. Use
/// [`SyntheticBackend::new`] to build the injector / backend pair.
#[derive(Debug)]
pub struct SyntheticBackend {
    rx: mpsc::Receiver<FocusEvent>,
}

/// Sender half of a [`SyntheticBackend`]. Held by the daemon's
/// `SimulateFocus` D-Bus handler (and by tests).
#[derive(Clone, Debug)]
pub struct SyntheticInjector {
    tx: mpsc::Sender<FocusEvent>,
}

impl SyntheticBackend {
    /// Build a fresh injector / backend pair. The channel is bounded;
    /// 64 events is generous given that focus changes happen at human
    /// pace.
    #[must_use]
    pub fn new() -> (SyntheticInjector, Self) {
        let (tx, rx) = mpsc::channel(64);
        (SyntheticInjector { tx }, Self { rx })
    }
}

impl FocusBackend for SyntheticBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Synthetic
    }

    fn into_stream(self) -> FocusStream {
        Box::pin(ReceiverStream::new(self.rx))
    }
}

impl SyntheticInjector {
    /// Push a synthetic focus event into the backend.
    pub async fn push(
        &self,
        app_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<(), InjectError> {
        let event = FocusEvent {
            app_id: app_id.into(),
            title: title.into(),
            source: BackendKind::Synthetic,
        };
        trace!(?event, "injecting synthetic focus event");
        self.tx.send(event).await.map_err(|_| InjectError::Closed)
    }
}

/// Focus backend driven by a `KWin` Script bridge.
///
/// The script (shipped in `data/kwin-script/gamerat-focus/`) runs
/// inside the compositor — the only place on Plasma where toplevel
/// observation isn't gated — and forwards activations to the daemon's
/// `IngestKwinFocus` D-Bus method, which pushes into
/// [`KwinInjector::push`].
///
/// Structurally identical to [`SyntheticBackend`]; just emits with
/// `BackendKind::Kwin`.
#[derive(Debug)]
pub struct KwinBackend {
    rx: mpsc::Receiver<FocusEvent>,
}

/// Sender half of a [`KwinBackend`]. Held by the daemon's
/// `IngestKwinFocus` D-Bus handler.
#[derive(Clone, Debug)]
pub struct KwinInjector {
    tx: mpsc::Sender<FocusEvent>,
}

impl KwinBackend {
    /// Build a fresh injector / backend pair.
    #[must_use]
    pub fn new() -> (KwinInjector, Self) {
        let (tx, rx) = mpsc::channel(64);
        (KwinInjector { tx }, Self { rx })
    }
}

impl FocusBackend for KwinBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Kwin
    }

    fn into_stream(self) -> FocusStream {
        Box::pin(ReceiverStream::new(self.rx))
    }
}

impl KwinInjector {
    /// Forward a KWin-observed activation into the backend.
    pub async fn push(
        &self,
        app_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<(), InjectError> {
        let event = FocusEvent {
            app_id: app_id.into(),
            title: title.into(),
            source: BackendKind::Kwin,
        };
        trace!(?event, "ingesting kwin focus event");
        self.tx.send(event).await.map_err(|_| InjectError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;

    #[test]
    fn backend_kind_wire_round_trip() {
        for kind in [
            BackendKind::Synthetic,
            BackendKind::WlrForeignToplevel,
            BackendKind::Kwin,
            BackendKind::X11,
        ] {
            assert_eq!(BackendKind::from_wire(kind.as_wire()), Some(kind));
        }
        assert_eq!(BackendKind::from_wire("not-a-backend"), None);
    }

    #[test]
    fn wire_strings_match_proto_constants() {
        // Belt-and-braces: changes to gamerat-proto's focus_source
        // constants must propagate here or this test catches it.
        assert_eq!(BackendKind::Synthetic.as_wire(), "synthetic");
        assert_eq!(
            BackendKind::WlrForeignToplevel.as_wire(),
            "wlr-foreign-toplevel"
        );
    }

    #[tokio::test]
    async fn synthetic_injection_delivers_event_to_stream() {
        let (injector, backend) = SyntheticBackend::new();
        let mut stream = backend.into_stream();

        injector
            .push("steam_app_730", "Counter-Strike 2")
            .await
            .unwrap();

        let evt = stream.next().await.expect("event should arrive");
        assert_eq!(evt.app_id, "steam_app_730");
        assert_eq!(evt.title, "Counter-Strike 2");
        assert_eq!(evt.source, BackendKind::Synthetic);
    }

    #[tokio::test]
    async fn stream_terminates_when_injector_dropped() {
        let (injector, backend) = SyntheticBackend::new();
        let mut stream = backend.into_stream();
        drop(injector);

        // Channel is closed and empty — stream resolves to None.
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn injection_after_backend_dropped_returns_closed() {
        let (injector, backend) = SyntheticBackend::new();
        drop(backend);

        let err = injector.push("x", "y").await.expect_err("should fail");
        assert!(matches!(err, InjectError::Closed));
    }

    #[tokio::test]
    async fn kwin_injection_tags_source_as_kwin() {
        let (injector, backend) = KwinBackend::new();
        let mut stream = backend.into_stream();

        injector.push("org.kde.konsole", "Konsole").await.unwrap();

        let evt = stream.next().await.expect("event should arrive");
        assert_eq!(evt.app_id, "org.kde.konsole");
        assert_eq!(evt.title, "Konsole");
        assert_eq!(evt.source, BackendKind::Kwin);
    }
}
