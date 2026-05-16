//! Lutris library scanner.
//!
//! Queries the Lutris `SQLite` database at
//! `~/.local/share/lutris/pga.db` for all installed games and builds a
//! [`GameEntry`] per row.
//!
//! # `app_id_hint`
//!
//! - **Native Linux games** (`runner = "linux"`): the Wayland `app_id`
//!   is typically the executable's filename stem, because the game sets
//!   it directly. We return `Some(basename)`.
//! - **Wine / Proton games** (`runner = "wine"`): the process runs
//!   under `XWayland`. The Wayland `app_id` exposed by `XWayland` for an
//!   X11 window is often the `.exe` basename, but it varies wildly
//!   (some games set a proper D-Bus name, others use the window class).
//!   We return `None` and let the user fix it in the rule editor.

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use rusqlite::{Connection, OpenFlags};
use tracing::debug;

use crate::{
    GameEntry, Launcher,
    error::{Error, Result},
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan the Lutris game database and return one [`GameEntry`] per
/// installed game.
///
/// Returns [`Error::NotInstalled`] if `~/.local/share/lutris/pga.db`
/// does not exist.
pub fn scan_lutris() -> Result<Vec<GameEntry>> {
    let base = BaseDirs::new().ok_or_else(|| Error::NotInstalled {
        launcher: Launcher::Lutris,
        path: PathBuf::from("~/.local/share/lutris/pga.db"),
    })?;

    let db_path = base.data_local_dir().join("lutris").join("pga.db");

    if !db_path.exists() {
        return Err(Error::NotInstalled {
            launcher: Launcher::Lutris,
            path: db_path,
        });
    }

    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| Error::Database {
        path: db_path.clone(),
        source,
    })?;

    scan_connection(&conn, &db_path)
}

/// Run the Lutris query against an open connection.
///
/// Factored out of [`scan_lutris`] so tests can pass an in-memory
/// connection without touching the filesystem.
pub(crate) fn scan_connection(conn: &Connection, db_path: &Path) -> Result<Vec<GameEntry>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, slug, runner, directory, executable
             FROM games
             WHERE installed = 1",
        )
        .map_err(|source| Error::Database {
            path: db_path.to_path_buf(),
            source,
        })?;

    let rows = stmt
        .query_map([], |row| {
            Ok(LutrisRow {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                runner: row.get(3)?,
                directory: row.get(4)?,
                executable: row.get(5)?,
            })
        })
        .map_err(|source| Error::Database {
            path: db_path.to_path_buf(),
            source,
        })?;

    let mut entries = Vec::new();
    for row_result in rows {
        let row = row_result.map_err(|source| Error::Database {
            path: db_path.to_path_buf(),
            source,
        })?;
        entries.push(build_entry(row));
    }
    debug!(count = entries.len(), "Lutris: found installed games");
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct LutrisRow {
    id: i64,
    name: String,
    slug: String,
    runner: Option<String>,
    directory: Option<String>,
    executable: Option<String>,
}

fn build_entry(row: LutrisRow) -> GameEntry {
    let slug = if row.slug.is_empty() {
        row.id.to_string()
    } else {
        row.slug
    };

    let install_dir = row
        .directory
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let executable = row
        .executable
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);

    // Derive a Wayland app_id hint only for native Linux runners where
    // we have a reasonable chance of guessing right.
    let app_id_hint = derive_app_id_hint(row.runner.as_deref(), executable.as_deref());

    GameEntry {
        id: format!("lutris:{slug}"),
        name: row.name,
        launcher: Launcher::Lutris,
        install_dir,
        executable,
        app_id_hint,
    }
}

/// Return a best-guess `app_id_hint` for a Lutris game.
///
/// Only native Linux runners (runner = `"linux"`) get a hint — we use
/// the executable's filename stem. Wine-wrapped games run under
/// `XWayland` with unpredictable `app_id`s, so we return `None` to avoid
/// seeding bad rules.
fn derive_app_id_hint(runner: Option<&str>, executable: Option<&Path>) -> Option<String> {
    match runner {
        Some("linux") => executable.and_then(exe_stem),
        _ => None,
    }
}

/// Return the filename stem of `path` as a `String`, or `None` if the
/// path has no filename or the name is not valid UTF-8.
fn exe_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(str::to_owned)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS games (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL,
                slug        TEXT NOT NULL DEFAULT '',
                runner      TEXT,
                directory   TEXT,
                executable  TEXT,
                installed   INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("create table");
    }

    fn insert_game(
        conn: &Connection,
        name: &str,
        slug: &str,
        runner: Option<&str>,
        directory: Option<&str>,
        executable: Option<&str>,
        installed: i32,
    ) {
        conn.execute(
            "INSERT INTO games (name, slug, runner, directory, executable, installed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, slug, runner, directory, executable, installed],
        )
        .expect("insert game");
    }

    #[test]
    fn filters_uninstalled_games() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        setup_db(&conn);
        insert_game(
            &conn,
            "Installed Game",
            "installed-game",
            Some("linux"),
            Some("/games/installed"),
            Some("/games/installed/game"),
            1,
        );
        insert_game(
            &conn,
            "Uninstalled Game",
            "uninstalled-game",
            Some("linux"),
            None,
            None,
            0,
        );

        let entries = scan_connection(&conn, Path::new("pga.db")).expect("scan ok");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Installed Game");
    }

    #[test]
    fn native_game_gets_app_id_hint() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        setup_db(&conn);
        insert_game(
            &conn,
            "OpenMW",
            "openmw",
            Some("linux"),
            Some("/usr/share/openmw"),
            Some("/usr/bin/openmw"),
            1,
        );

        let entries = scan_connection(&conn, Path::new("pga.db")).expect("scan ok");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.id, "lutris:openmw");
        assert_eq!(entry.launcher, Launcher::Lutris);
        // Native runner: executable stem becomes the hint.
        assert_eq!(entry.app_id_hint, Some("openmw".to_owned()));
    }

    #[test]
    fn wine_game_has_no_app_id_hint() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        setup_db(&conn);
        insert_game(
            &conn,
            "Guild Wars 2",
            "guild-wars-2",
            Some("wine"),
            Some("/games/gw2"),
            Some("/games/gw2/Gw2-64.exe"),
            1,
        );

        let entries = scan_connection(&conn, Path::new("pga.db")).expect("scan ok");
        assert_eq!(entries.len(), 1);
        // Wine runner: we can't reliably predict the XWayland app_id.
        assert!(entries[0].app_id_hint.is_none());
    }

    #[test]
    fn entry_id_uses_slug() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        setup_db(&conn);
        insert_game(
            &conn,
            "Counter-Strike 2",
            "counter-strike-2",
            None,
            None,
            None,
            1,
        );

        let entries = scan_connection(&conn, Path::new("pga.db")).expect("scan ok");
        assert_eq!(entries[0].id, "lutris:counter-strike-2");
    }

    #[test]
    fn empty_slug_falls_back_to_row_id() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        setup_db(&conn);
        // Empty slug — id should fall back to the numeric row id.
        conn.execute(
            "INSERT INTO games (name, slug, runner, directory, executable, installed)
             VALUES ('Nameless', '', 'linux', NULL, NULL, 1)",
            [],
        )
        .expect("insert");

        let entries = scan_connection(&conn, Path::new("pga.db")).expect("scan ok");
        assert_eq!(entries.len(), 1);
        // The id should be "lutris:<row_id>", not "lutris:".
        assert!(entries[0].id.starts_with("lutris:"));
        assert_ne!(entries[0].id, "lutris:");
    }
}
