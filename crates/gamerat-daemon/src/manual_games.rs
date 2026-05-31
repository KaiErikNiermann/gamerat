//! User-added game entries with TOML persistence.
//!
//! The launcher scanners (`gamerat_gamedb::scan_*`) cover Steam,
//! Lutris, and Heroic, but a game installed outside those — a raw Wine
//! prefix, a DRM-free build in an arbitrary folder — is invisible to
//! them. This store lets the user register such games by hand so they
//! show up in "Discovered games" and can be wired to a profile like
//! any scanned entry.
//!
//! Mirrors [`crate::rules::RuleStore`]: in-memory `Vec` for the read
//! path, atomic `rename`-over-tempfile writes, missing file = empty
//! store. Entries are kept as the wire [`GameEntry`] so the service
//! layer can merge them into `ListGames` without conversion.

use std::path::{Path, PathBuf};

use gamerat_proto::{GameEntry, game_launcher};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

#[derive(Debug, Error)]
pub enum ManualGameError {
    #[error("manual-games file I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("manual-games file at {path} is malformed: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("manual-games file at {path} could not be serialized: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
}

pub type ManualGameResult<T> = Result<T, ManualGameError>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ManualGamesFile {
    #[serde(default)]
    games: Vec<GameEntry>,
}

/// In-memory manual-game list behind a TOML file path.
#[derive(Debug)]
pub struct ManualGameStore {
    path: PathBuf,
    games: Vec<GameEntry>,
}

impl ManualGameStore {
    /// Load manual games from `path`. A missing file is an empty store
    /// (the parent dir is created lazily on first write).
    pub fn load_or_create(path: PathBuf) -> ManualGameResult<Self> {
        let games = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let file: ManualGamesFile =
                    toml::from_str(&text).map_err(|source| ManualGameError::Parse {
                        path: path.clone(),
                        source,
                    })?;
                info!(count = file.games.len(), path = %path.display(), "loaded manual games");
                file.games
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "no manual-games file yet; starting empty");
                Vec::new()
            }
            Err(source) => return Err(ManualGameError::Io { path, source }),
        };
        Ok(Self { path, games })
    }

    /// Add a manual game. Generates a unique `manual:<slug>` id from the
    /// name and returns the created entry. The caller is responsible for
    /// calling [`Self::save`] afterwards.
    pub fn add(&mut self, name: &str, install_dir: &str, app_id_hint: &str) -> GameEntry {
        let entry = GameEntry {
            id: self.unique_id(name),
            name: name.to_owned(),
            launcher: game_launcher::MANUAL.to_owned(),
            install_dir: install_dir.to_owned(),
            executable: String::new(),
            app_id_hint: app_id_hint.to_owned(),
        };
        self.games.push(entry.clone());
        entry
    }

    /// Remove a manual game by id. Returns true if one was removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.games.len();
        self.games.retain(|g| g.id != id);
        self.games.len() != before
    }

    /// All manual entries, in insertion order.
    #[must_use]
    pub fn list(&self) -> &[GameEntry] {
        &self.games
    }

    /// Atomically persist: serialize to a `<path>.tmp` sibling, then
    /// rename over the target.
    pub fn save(&self) -> ManualGameResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ManualGameError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let payload = toml::to_string_pretty(&ManualGamesFile {
            games: self.games.clone(),
        })
        .map_err(|source| ManualGameError::Serialize {
            path: self.path.clone(),
            source,
        })?;
        let tmp = tmp_sibling(&self.path);
        std::fs::write(&tmp, payload).map_err(|source| ManualGameError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|source| ManualGameError::Io {
            path: self.path.clone(),
            source,
        })?;
        debug!(path = %self.path.display(), count = self.games.len(), "wrote manual games");
        Ok(())
    }

    /// Build a `manual:<slug>` id unique within the store, appending a
    /// numeric suffix on collision so two games named the same don't
    /// clobber each other.
    fn unique_id(&self, name: &str) -> String {
        let base = format!("manual:{}", slugify(name));
        if !self.games.iter().any(|g| g.id == base) {
            return base;
        }
        // At most `games.len()` ids can collide, so `len + 1` candidates
        // are guaranteed to contain a free one (pigeonhole) — bounding
        // the range also keeps it provably finite.
        (2..)
            .take(self.games.len() + 1)
            .map(|n| format!("{base}-{n}"))
            .find(|candidate| !self.games.iter().any(|g| &g.id == candidate))
            .unwrap_or(base)
    }
}

/// Lowercase, collapse non-alphanumeric runs to single hyphens, trim
/// leading/trailing hyphens. Empty input (or all-punctuation) yields
/// `"game"` so the id is never just the `manual:` prefix.
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_hyphen = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "game".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store_in(dir: &TempDir) -> ManualGameStore {
        ManualGameStore::load_or_create(dir.path().join("manual-games.toml")).unwrap()
    }

    #[test]
    fn add_generates_manual_prefixed_slug_id() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        let entry = store.add("Elden Ring", "/games/eldenring", "eldenring.exe");
        assert_eq!(entry.id, "manual:elden-ring");
        assert_eq!(entry.launcher, game_launcher::MANUAL);
        assert_eq!(entry.app_id_hint, "eldenring.exe");
    }

    #[test]
    fn duplicate_names_get_distinct_ids() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        let a = store.add("My Game", "/a", "a");
        let b = store.add("My Game", "/b", "b");
        assert_eq!(a.id, "manual:my-game");
        assert_eq!(b.id, "manual:my-game-2");
    }

    #[test]
    fn slugify_handles_punctuation_and_empty() {
        assert_eq!(slugify("Counter-Strike 2!!!"), "counter-strike-2");
        assert_eq!(slugify("   "), "game");
        assert_eq!(slugify("***"), "game");
    }

    #[test]
    fn remove_reports_whether_it_hit() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        let entry = store.add("X", "/x", "x");
        assert!(store.remove(&entry.id));
        assert!(!store.remove(&entry.id));
        assert!(store.list().is_empty());
    }

    #[test]
    fn save_then_reload_roundtrips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("manual-games.toml");
        {
            let mut store = ManualGameStore::load_or_create(path.clone()).unwrap();
            store.add("Persisted Game", "/p", "pg");
            store.save().unwrap();
        }
        let reloaded = ManualGameStore::load_or_create(path).unwrap();
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].name, "Persisted Game");
    }

    #[test]
    fn missing_file_is_empty_store() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        assert!(store.list().is_empty());
    }
}
