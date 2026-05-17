//! User-defined profile store with TOML persistence.
//!
//! Mirrors the [`crate::rules::RuleStore`] pattern: in-memory map for
//! the hot path, single `profiles.toml` on disk for persistence,
//! atomic rename for writes. Phase A only — the dispatch loop doesn't
//! consume profiles yet (rules still address hardware slots
//! directly). The store exists so the wire surface, the persistence
//! format, and the CLI workflow can settle before Phases B/C/D wire
//! it into actual hardware writes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gamerat_proto::{GameratProfile, game_category};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("profile id `{0}` is empty")]
    EmptyId(String),

    #[error(
        "profile id `{0}` must be kebab-case (lowercase letters, digits, \
         hyphens, underscores)"
    )]
    InvalidId(String),

    #[error("profile `{0}` not found")]
    NotFound(String),

    #[error(
        "category `{got}` is not valid (expected `{}` or `{}`)",
        game_category::AGNOSTIC,
        game_category::SPECIFIC
    )]
    InvalidCategory { got: String },

    #[error("dpi stages must be non-empty")]
    NoDpi,

    #[error("active_dpi_stage {stage} is out of bounds (have {len} stage(s))")]
    StageOutOfBounds { stage: u32, len: usize },

    #[error("profile file I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("profile file at {path} is malformed: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("profile file at {path} could not be serialized: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
}

pub type ProfileResult<T> = Result<T, ProfileError>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProfilesFile {
    #[serde(default)]
    profiles: Vec<GameratProfile>,
}

/// In-memory profile set keyed by id, with TOML-on-disk persistence.
#[derive(Debug)]
pub struct ProfileStore {
    path: PathBuf,
    by_id: BTreeMap<String, GameratProfile>,
}

impl ProfileStore {
    /// Load profiles from `path`. A missing file yields an empty store;
    /// malformed contents surface as [`ProfileError::Parse`].
    pub fn load_or_create(path: PathBuf) -> ProfileResult<Self> {
        let by_id = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let file: ProfilesFile =
                    toml::from_str(&text).map_err(|source| ProfileError::Parse {
                        path: path.clone(),
                        source,
                    })?;
                let map: BTreeMap<_, _> = file
                    .profiles
                    .into_iter()
                    .map(|p| (p.id.clone(), p))
                    .collect();
                info!(count = map.len(), path = %path.display(), "loaded profiles");
                map
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "no profile file yet; starting with empty store");
                BTreeMap::new()
            }
            Err(source) => {
                return Err(ProfileError::Io { path, source });
            }
        };
        Ok(Self { path, by_id })
    }

    /// Insert-or-replace a profile. Validates id / category / dpi
    /// shape before accepting. Stamps `created_unix` if it's zero
    /// (i.e. the caller didn't supply one) but leaves it alone
    /// otherwise — that way `set_profile(get_profile(id))` round-trips
    /// without rewriting timestamps.
    pub fn upsert(&mut self, mut profile: GameratProfile) -> ProfileResult<()> {
        validate(&profile)?;
        if profile.created_unix == 0 {
            profile.created_unix = unix_now();
        }
        self.by_id.insert(profile.id.clone(), profile);
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> bool {
        self.by_id.remove(id).is_some()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&GameratProfile> {
        self.by_id.get(id)
    }

    /// All profiles, sorted by id (`BTreeMap` iteration order).
    pub fn list(&self) -> Vec<GameratProfile> {
        self.by_id.values().cloned().collect()
    }

    /// Atomically persist the store to disk. Same `<path>.tmp` →
    /// `rename` dance the rule store uses.
    pub fn save(&self) -> ProfileResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ProfileError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let file = ProfilesFile {
            profiles: self.by_id.values().cloned().collect(),
        };
        let payload = toml::to_string_pretty(&file).map_err(|source| ProfileError::Serialize {
            path: self.path.clone(),
            source,
        })?;
        let tmp = tmp_sibling(&self.path);
        std::fs::write(&tmp, payload).map_err(|source| ProfileError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|source| ProfileError::Io {
            path: self.path.clone(),
            source,
        })?;
        debug!(path = %self.path.display(), count = self.by_id.len(), "wrote profiles");
        Ok(())
    }
}

fn validate(profile: &GameratProfile) -> ProfileResult<()> {
    if profile.id.is_empty() {
        return Err(ProfileError::EmptyId(profile.id.clone()));
    }
    if !is_kebab_ish(&profile.id) {
        return Err(ProfileError::InvalidId(profile.id.clone()));
    }
    if profile.category != game_category::AGNOSTIC && profile.category != game_category::SPECIFIC {
        return Err(ProfileError::InvalidCategory {
            got: profile.category.clone(),
        });
    }
    if profile.dpi.is_empty() {
        return Err(ProfileError::NoDpi);
    }
    if (profile.active_dpi_stage as usize) >= profile.dpi.len() {
        return Err(ProfileError::StageOutOfBounds {
            stage: profile.active_dpi_stage,
            len: profile.dpi.len(),
        });
    }
    Ok(())
}

/// Lowercase letters / digits / `-` / `_`. Non-empty. Allow `inherits_from`
/// and similar identifiers to share the constraint.
fn is_kebab_ish(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn tmp_sibling(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fps_profile() -> GameratProfile {
        GameratProfile {
            id: "fps-low-dpi".to_owned(),
            name: "FPS — low DPI".to_owned(),
            description: String::new(),
            category: game_category::AGNOSTIC.to_owned(),
            inherits_from: String::new(),
            dpi: vec![400, 800, 1600],
            active_dpi_stage: 1,
            created_unix: 0,
            buttons: Vec::new(),
        }
    }

    #[test]
    fn empty_store_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn upsert_stamps_created_unix() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        store.upsert(fps_profile()).unwrap();
        let p = store.get("fps-low-dpi").unwrap();
        assert!(p.created_unix > 0);
    }

    #[test]
    fn upsert_preserves_explicit_created_unix() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        let mut p = fps_profile();
        p.created_unix = 1_700_000_000;
        store.upsert(p).unwrap();
        assert_eq!(
            store.get("fps-low-dpi").unwrap().created_unix,
            1_700_000_000
        );
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("profiles.toml");
        {
            let mut store = ProfileStore::load_or_create(path.clone()).unwrap();
            store.upsert(fps_profile()).unwrap();
            store
                .upsert(GameratProfile {
                    id: "cs2".to_owned(),
                    name: "Counter-Strike 2".to_owned(),
                    description: String::new(),
                    category: game_category::SPECIFIC.to_owned(),
                    inherits_from: "fps-low-dpi".to_owned(),
                    dpi: vec![800],
                    active_dpi_stage: 0,
                    created_unix: 0,
                    buttons: Vec::new(),
                })
                .unwrap();
            store.save().unwrap();
        }
        let reloaded = ProfileStore::load_or_create(path).unwrap();
        assert_eq!(reloaded.list().len(), 2);
        assert_eq!(reloaded.get("cs2").unwrap().inherits_from, "fps-low-dpi");
    }

    #[test]
    fn delete_returns_true_when_present() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        store.upsert(fps_profile()).unwrap();
        assert!(store.delete("fps-low-dpi"));
        assert!(!store.delete("fps-low-dpi"));
    }

    #[test]
    fn invalid_id_is_rejected() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        let mut p = fps_profile();
        p.id = "FPS Low DPI".to_owned(); // capitals + spaces
        let err = store.upsert(p).unwrap_err();
        assert!(matches!(err, ProfileError::InvalidId(_)));
    }

    #[test]
    fn invalid_category_is_rejected() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        let mut p = fps_profile();
        p.category = "mixed".to_owned();
        assert!(matches!(
            store.upsert(p).unwrap_err(),
            ProfileError::InvalidCategory { .. }
        ));
    }

    #[test]
    fn no_dpi_is_rejected() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        let mut p = fps_profile();
        p.dpi.clear();
        assert!(matches!(store.upsert(p).unwrap_err(), ProfileError::NoDpi));
    }

    #[test]
    fn active_stage_out_of_bounds_rejected() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        let mut p = fps_profile();
        p.active_dpi_stage = 99;
        assert!(matches!(
            store.upsert(p).unwrap_err(),
            ProfileError::StageOutOfBounds { .. }
        ));
    }

    #[test]
    fn list_is_sorted_by_id() {
        let dir = TempDir::new().unwrap();
        let mut store = ProfileStore::load_or_create(dir.path().join("profiles.toml")).unwrap();
        for id in ["zebra", "alpha", "mid"] {
            let mut p = fps_profile();
            p.id = id.to_owned();
            store.upsert(p).unwrap();
        }
        let ids: Vec<_> = store.list().into_iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            vec!["alpha".to_owned(), "mid".to_owned(), "zebra".to_owned()]
        );
    }
}
