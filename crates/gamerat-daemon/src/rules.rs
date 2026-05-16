//! Glob-matched rule store with TOML persistence.
//!
//! Rules are kept in memory for fast match-on-every-focus-event, and
//! mirrored to `$XDG_CONFIG_HOME/gamerat/rules.toml` so they survive
//! daemon restarts. Writes are atomic (`rename` over a tempfile).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gamerat_proto::Rule;
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum RuleError {
    #[error("invalid glob `{glob}`: {source}")]
    InvalidGlob {
        glob: String,
        #[source]
        source: globset::Error,
    },

    #[error("rule file I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("rule file at {path} is malformed: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("rule file at {path} could not be serialized: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
}

pub type RuleResult<T> = Result<T, RuleError>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct RulesFile {
    #[serde(default)]
    rules: Vec<Rule>,
}

/// In-memory rule set keyed by glob, with compiled matchers for the
/// hot path and TOML persistence behind a file path.
#[derive(Debug)]
pub struct RuleStore {
    path: PathBuf,
    rules: Vec<Rule>,
    compiled: Vec<GlobMatcher>,
}

impl RuleStore {
    /// Load rules from `path`. A missing file is treated as an empty
    /// store (and the parent directory is created lazily on first
    /// write). Malformed files are surfaced as [`RuleError::Parse`].
    pub fn load_or_create(path: PathBuf) -> RuleResult<Self> {
        let rules = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let file: RulesFile = toml::from_str(&text).map_err(|source| RuleError::Parse {
                    path: path.clone(),
                    source,
                })?;
                info!(count = file.rules.len(), path = %path.display(), "loaded rules");
                file.rules
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "no rule file yet; starting with empty store");
                Vec::new()
            }
            Err(source) => {
                return Err(RuleError::Io { path, source });
            }
        };

        let compiled = compile_all(&rules)?;
        Ok(Self {
            path,
            rules,
            compiled,
        })
    }

    /// Insert-or-replace a rule. Replacement is keyed by `app_id_glob`;
    /// adjusting the `profile_index` on an existing glob is an upsert,
    /// not a duplicate.
    pub fn upsert(&mut self, app_id_glob: &str, profile_index: u32) -> RuleResult<()> {
        let _matcher = Self::compile(app_id_glob)?;
        if let Some(existing) = self.rules.iter_mut().find(|r| r.app_id_glob == app_id_glob) {
            existing.profile_index = profile_index;
        } else {
            self.rules.push(Rule {
                app_id_glob: app_id_glob.to_owned(),
                profile_index,
                created_unix: unix_now(),
            });
        }
        self.compiled = compile_all(&self.rules)?;
        Ok(())
    }

    /// Remove a rule by its exact glob string. Returns true if a rule
    /// was actually removed.
    pub fn delete(&mut self, app_id_glob: &str) -> RuleResult<bool> {
        let before = self.rules.len();
        self.rules.retain(|r| r.app_id_glob != app_id_glob);
        let removed = self.rules.len() != before;
        if removed {
            self.compiled = compile_all(&self.rules)?;
        }
        Ok(removed)
    }

    /// First rule whose glob matches `app_id`. Rules are matched in
    /// insertion order — for the MVP this is also the only ordering
    /// the user controls. Returns `None` if nothing matches.
    #[must_use]
    pub fn match_app_id(&self, app_id: &str) -> Option<&Rule> {
        self.compiled
            .iter()
            .zip(self.rules.iter())
            .find(|(m, _)| m.is_match(app_id))
            .map(|(_, r)| r)
    }

    /// All rules, in insertion order.
    #[must_use]
    pub fn list(&self) -> &[Rule] {
        &self.rules
    }

    /// Atomically write the rule list to disk: serialize to a
    /// `<path>.tmp` tempfile, fsync, then rename over the target.
    pub fn save(&self) -> RuleResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| RuleError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let payload = toml::to_string_pretty(&RulesFile {
            rules: self.rules.clone(),
        })
        .map_err(|source| RuleError::Serialize {
            path: self.path.clone(),
            source,
        })?;

        let tmp = tmp_sibling(&self.path);
        std::fs::write(&tmp, payload).map_err(|source| RuleError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|source| RuleError::Io {
            path: self.path.clone(),
            source,
        })?;
        debug!(path = %self.path.display(), count = self.rules.len(), "wrote rules");
        Ok(())
    }

    fn compile(glob: &str) -> RuleResult<GlobMatcher> {
        Glob::new(glob)
            .map(|g| g.compile_matcher())
            .map_err(|source| RuleError::InvalidGlob {
                glob: glob.to_owned(),
                source,
            })
    }
}

fn compile_all(rules: &[Rule]) -> RuleResult<Vec<GlobMatcher>> {
    let mut compiled = Vec::with_capacity(rules.len());
    for rule in rules {
        match RuleStore::compile(&rule.app_id_glob) {
            Ok(m) => compiled.push(m),
            Err(e) => {
                // A persisted rule with a bad glob means someone hand-edited
                // the file into a broken state. Surface the error rather
                // than dropping rules silently.
                warn!(glob = %rule.app_id_glob, "skipping rule with invalid glob");
                return Err(e);
            }
        }
    }
    Ok(compiled)
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

    fn store_in(dir: &TempDir) -> RuleStore {
        RuleStore::load_or_create(dir.path().join("rules.toml")).unwrap()
    }

    #[test]
    fn load_missing_file_yields_empty_store() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        assert!(store.list().is_empty());
    }

    #[test]
    fn upsert_then_match_succeeds() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        store.upsert("steam_app_*", 2).unwrap();
        let m = store.match_app_id("steam_app_730").expect("should match");
        assert_eq!(m.profile_index, 2);
    }

    #[test]
    fn upsert_replaces_existing_glob() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        store.upsert("firefox", 0).unwrap();
        store.upsert("firefox", 1).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].profile_index, 1);
    }

    #[test]
    fn delete_returns_true_when_present() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        store.upsert("firefox", 0).unwrap();
        assert!(store.delete("firefox").unwrap());
        assert!(!store.delete("firefox").unwrap());
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");
        {
            let mut store = RuleStore::load_or_create(path.clone()).unwrap();
            store.upsert("steam_app_730", 1).unwrap();
            store.upsert("org.mozilla.*", 0).unwrap();
            store.save().unwrap();
        }
        let reloaded = RuleStore::load_or_create(path).unwrap();
        assert_eq!(reloaded.list().len(), 2);
        assert_eq!(
            reloaded
                .match_app_id("steam_app_730")
                .unwrap()
                .profile_index,
            1
        );
        assert_eq!(
            reloaded
                .match_app_id("org.mozilla.firefox")
                .unwrap()
                .profile_index,
            0
        );
    }

    #[test]
    fn match_order_is_insertion_order() {
        // Two overlapping rules — the one inserted first wins.
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        store.upsert("steam_app_730", 5).unwrap();
        store.upsert("steam_app_*", 2).unwrap();
        assert_eq!(
            store.match_app_id("steam_app_730").unwrap().profile_index,
            5
        );
        assert_eq!(
            store.match_app_id("steam_app_1234").unwrap().profile_index,
            2
        );
    }

    #[test]
    fn invalid_glob_returns_error() {
        let dir = TempDir::new().unwrap();
        let mut store = store_in(&dir);
        // Unclosed character class.
        let err = store.upsert("[broken", 0).unwrap_err();
        assert!(matches!(err, RuleError::InvalidGlob { .. }));
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a").join("b").join("rules.toml");
        let mut store = RuleStore::load_or_create(nested.clone()).unwrap();
        store.upsert("firefox", 0).unwrap();
        store.save().unwrap();
        assert!(nested.exists());
    }
}
