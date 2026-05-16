//! Error types for game library scanners.

use std::path::PathBuf;

use crate::Launcher;

/// Errors returned by the per-launcher scan functions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The launcher is not installed: the expected path does not exist.
    #[error("{launcher:?} not installed (looked under {path})")]
    NotInstalled { launcher: Launcher, path: PathBuf },

    /// A filesystem I/O error was encountered while reading scanner
    /// input files.
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A file was readable but could not be parsed.
    #[error("could not parse {path}: {reason}")]
    Parse { path: PathBuf, reason: String },

    /// The `SQLite` database could not be opened or queried (Lutris).
    #[error("database error at {path}: {source}")]
    Database {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
