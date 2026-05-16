//! Fixture replay: feed previously-recorded focus events through the
//! [`FocusBackend`] interface.
//!
//! Fixtures are TOML files of timestamp-delta records produced by
//! `gameratctl focus record`. Replay rebuilds the original cadence by
//! sleeping `delay_ms` between events. Source labels are preserved
//! from the recording, so a fixture captured against `KWin` replays
//! as if it came from `KWin` (the daemon's `FocusChanged` signal carries
//! the recorded source string verbatim).
//!
//! Schema (`data/fixtures/focus/example.toml`):
//!
//! ```toml
//! [meta]
//! description = "Switching between Firefox, terminal, and Counter-Strike 2"
//! source      = "kwin"            # which backend the recording came from
//!
//! [[event]]
//! delay_ms = 0                    # delay since the previous event
//! app_id   = "firefox"
//! title    = "Mozilla Firefox"
//! source   = "kwin"               # per-event; defaults to meta.source
//!
//! [[event]]
//! delay_ms = 2300
//! app_id   = "Alacritty"
//! title    = "user@host: ~"
//! source   = "kwin"
//! ```
//!
//! Delays are relative to the previous event (or to fixture start for
//! event 0). This makes hand-editing and splicing trivial; absolute
//! timestamps would need rebasing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, info, warn};

use crate::{BackendKind, FocusBackend, FocusEvent, FocusStream};

/// Top-level fixture document.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FixtureFile {
    #[serde(default)]
    pub meta: FixtureMeta,
    #[serde(default, rename = "event")]
    pub events: Vec<RecordedEvent>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FixtureMeta {
    #[serde(default)]
    pub description: String,
    /// Wire-stable source name (one of `gamerat_proto::focus_source::*`).
    /// Used as the default per-event source if a [`RecordedEvent`]
    /// omits the field.
    #[serde(default)]
    pub source: String,
}

/// One recorded focus transition.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecordedEvent {
    /// Milliseconds since the previous event (or since fixture start
    /// for the first event).
    pub delay_ms: u64,
    pub app_id: String,
    pub title: String,
    /// Wire-stable source label preserved from the recording. Empty
    /// means "use the fixture's `[meta] source` value, or fall back
    /// to `synthetic`".
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Error)]
pub enum FixtureError {
    #[error("could not read fixture {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("fixture {path} is malformed: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Backend that walks a [`FixtureFile`]'s event list, sleeping each
/// event's `delay_ms` before emitting.
///
/// When the list is exhausted the stream closes — the daemon's
/// dispatch loop notices and shuts down the dispatch task while the
/// main process keeps awaiting shutdown signals.
#[derive(Debug)]
pub struct FixtureReplayBackend {
    file: FixtureFile,
}

impl FixtureReplayBackend {
    /// Load a fixture from disk.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FixtureError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| FixtureError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let file: FixtureFile = toml::from_str(&text).map_err(|source| FixtureError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self::from_file(file))
    }

    /// Build directly from an in-memory fixture (handy for tests).
    #[must_use]
    pub const fn from_file(file: FixtureFile) -> Self {
        Self { file }
    }

    #[must_use]
    pub fn event_count(&self) -> usize {
        self.file.events.len()
    }
}

impl FocusBackend for FixtureReplayBackend {
    fn kind(&self) -> BackendKind {
        // Per-event source is preserved on the FocusEvent itself; this
        // is just the "what kind of backend am I as a whole" tag, which
        // for replay is closest to synthetic.
        BackendKind::Synthetic
    }

    fn into_stream(self) -> FocusStream {
        let (tx, rx) = mpsc::channel(64);
        let events = self.file.events;
        let default_source = self.file.meta.source;
        let total = events.len();
        info!(
            count = total,
            default_source = %default_source,
            "replaying focus fixture"
        );

        tokio::spawn(async move {
            for (idx, recorded) in events.into_iter().enumerate() {
                if recorded.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(recorded.delay_ms)).await;
                }

                let source_str = if recorded.source.is_empty() {
                    default_source.as_str()
                } else {
                    recorded.source.as_str()
                };
                let source = BackendKind::from_wire(source_str).unwrap_or_else(|| {
                    warn!(source = %source_str, "unknown source label, defaulting to Synthetic");
                    BackendKind::Synthetic
                });

                let event = FocusEvent {
                    app_id: recorded.app_id,
                    title: recorded.title,
                    source,
                };
                debug!(idx, total, ?event, "replay emit");
                if tx.send(event).await.is_err() {
                    debug!("replay receiver dropped; stopping early");
                    return;
                }
            }
            info!(total, "replay finished");
        });

        Box::pin(ReceiverStream::new(rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;

    fn fixture(events: Vec<RecordedEvent>) -> FixtureFile {
        FixtureFile {
            meta: FixtureMeta {
                description: "test".to_owned(),
                source: "kwin".to_owned(),
            },
            events,
        }
    }

    #[tokio::test]
    async fn empty_fixture_closes_stream_immediately() {
        let backend = FixtureReplayBackend::from_file(fixture(vec![]));
        let mut stream = backend.into_stream();
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn events_emit_in_order_with_preserved_source() {
        let backend = FixtureReplayBackend::from_file(fixture(vec![
            RecordedEvent {
                delay_ms: 0,
                app_id: "firefox".to_owned(),
                title: "Mozilla Firefox".to_owned(),
                source: "kwin".to_owned(),
            },
            RecordedEvent {
                delay_ms: 0, // no delay so test is fast
                app_id: "steam_app_730".to_owned(),
                title: "Counter-Strike 2".to_owned(),
                source: String::new(), // fall back to meta.source
            },
        ]));
        let mut stream = backend.into_stream();

        let a = stream.next().await.expect("first");
        assert_eq!(a.app_id, "firefox");
        assert_eq!(a.source, BackendKind::Kwin);

        let b = stream.next().await.expect("second");
        assert_eq!(b.app_id, "steam_app_730");
        // Empty source on event falls back to meta.source = "kwin".
        assert_eq!(b.source, BackendKind::Kwin);

        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn unknown_source_falls_back_to_synthetic() {
        let backend = FixtureReplayBackend::from_file(fixture(vec![RecordedEvent {
            delay_ms: 0,
            app_id: "a".to_owned(),
            title: "t".to_owned(),
            source: "nonexistent-backend".to_owned(),
        }]));
        let mut stream = backend.into_stream();
        let evt = stream.next().await.expect("event");
        assert_eq!(evt.source, BackendKind::Synthetic);
    }

    #[tokio::test]
    async fn delays_are_honored() {
        let backend = FixtureReplayBackend::from_file(fixture(vec![
            RecordedEvent {
                delay_ms: 0,
                app_id: "a".to_owned(),
                title: "t".to_owned(),
                source: "synthetic".to_owned(),
            },
            RecordedEvent {
                delay_ms: 50,
                app_id: "b".to_owned(),
                title: "t".to_owned(),
                source: "synthetic".to_owned(),
            },
        ]));
        let mut stream = backend.into_stream();
        let start = tokio::time::Instant::now();
        stream.next().await.unwrap();
        stream.next().await.unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(45),
            "second event arrived too early: {elapsed:?}"
        );
    }

    #[test]
    fn from_path_round_trips() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let toml_text = r#"
[meta]
description = "round-trip"
source = "kwin"

[[event]]
delay_ms = 0
app_id = "firefox"
title = "Mozilla Firefox"
source = "kwin"
"#;
        std::fs::write(tmp.path(), toml_text).unwrap();
        let backend = FixtureReplayBackend::from_path(tmp.path()).expect("load");
        assert_eq!(backend.event_count(), 1);
    }

    #[test]
    fn missing_file_returns_read_error() {
        let err = FixtureReplayBackend::from_path("/nonexistent/path/fixture.toml")
            .expect_err("should fail");
        assert!(matches!(err, FixtureError::Read { .. }));
    }
}
