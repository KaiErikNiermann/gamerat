//! Slot allocator: which hardware profile slot do we use for a given
//! gamerat profile?
//!
//! Policy (per `memory/profile_architecture.md`):
//!
//!   - **One fixed "Desktop" slot** is reserved and *never written* by
//!     the daemon. The user's canonical no-game baseline lives there
//!     and the daemon's allocator pretends it doesn't exist.
//!   - **Remaining slots are an LRU cache** keyed by `profile_id`. On
//!     allocation, we look for the profile already materialized in a
//!     managed slot; if absent, we use any empty slot; if all are
//!     occupied, we evict the LRU candidate.
//!   - **Eviction tie-breaker**: prefer evicting game-*specific*
//!     profiles before game-*agnostic* ones. Agnostic profiles get
//!     reused across multiple games (so they cache-hit more often) and
//!     keeping them materialized minimizes write traffic.
//!
//! Phase B scope: the allocator is a self-contained library module.
//! The dispatch loop doesn't consume it yet — wiring lands in Phase D
//! together with the Rule rewire (`profile_index` → `profile_id`).
//!
//! ## Recency representation
//!
//! Internally we track recency with a monotonic per-allocator sequence
//! counter, not wall-clock time. This makes tests deterministic (no
//! `sleep` between operations) and dodges clock-skew / DST / leap
//! questions. The counter is rebased on load (`next_seq = 1 + max(seq)`)
//! so the values stay bounded for any realistic session length.
//!
//! ## Hardware-write cost model
//!
//! [`Decision::needs_write`] tells the dispatch loop whether to write
//! profile content to the slot before activating it. A cache hit
//! (profile already in the slot) returns `false` — just `SetActive`
//! is enough. All evictions and empty-slot fills set `true`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gamerat_proto::{GameratProfile, game_category};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum AllocatorError {
    #[error("device reports no profile slots — allocator can't function")]
    NoSlots,

    #[error(
        "desktop slot {desktop} is out of range (device has {count} slot(s); \
         valid range is 0..{count})"
    )]
    DesktopSlotOutOfRange { desktop: u32, count: u32 },

    #[error(
        "no managed slots — every slot would be reserved as the desktop \
         (profile_count={count}, desktop_slot={desktop})"
    )]
    NoManagedSlots { count: u32, desktop: u32 },

    #[error("slot cache file I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("slot cache file at {path} is malformed: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("slot cache file at {path} could not be serialized: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
}

pub type AllocatorResult<T> = Result<T, AllocatorError>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SlotCacheFile {
    desktop_slot: u32,
    profile_count: u32,
    #[serde(default)]
    slots: Vec<SlotEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SlotEntry {
    index: u32,
    /// `""` when the slot has never been written by the daemon (we
    /// store empty entries to distinguish "never touched" from
    /// "currently free after eviction" — both behave the same for
    /// allocation but the cache file is nicer to read).
    profile_id: String,
    /// One of [`game_category`]'s wire constants, or `""` for empty.
    category: String,
    last_used_seq: u64,
}

/// One slot in [`SlotAllocator::snapshot`]'s output. The service
/// layer joins this with the device's active-slot reading and the
/// profile store to produce the on-the-wire `SlotInfo`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotSnapshot {
    pub index: u32,
    /// Empty when the slot has never been written or is reserved.
    pub profile_id: String,
    pub is_desktop: bool,
}

/// Outcome of an [`SlotAllocator::allocate`] call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    /// Slot index (0-based) on the device.
    pub slot: u32,
    /// True if the dispatch loop must write the profile's content to
    /// the slot before activating it. False means the profile is
    /// already materialized — `SetActive` alone is enough.
    pub needs_write: bool,
    pub reason: AllocationReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllocationReason {
    /// Profile was already materialized in this slot — no write needed.
    Cached,
    /// Took an empty slot.
    EmptySlot,
    /// Evicted the LRU candidate (with tie-break preferring
    /// game-specific profiles before game-agnostic ones).
    Evicted { previous_profile_id: String },
}

#[derive(Debug)]
pub struct SlotAllocator {
    path: PathBuf,
    desktop_slot: u32,
    profile_count: u32,
    /// All managed slot entries (i.e. excluding the desktop slot).
    /// Indices outside `0..profile_count` and the desktop index are
    /// filtered out on load.
    managed: BTreeMap<u32, SlotEntry>,
    next_seq: u64,
}

impl SlotAllocator {
    /// Build an allocator for a device with `profile_count` total
    /// slots and `desktop_slot` reserved for the user's canonical
    /// baseline. Loads any existing cache from `path`; a missing or
    /// device-mismatched cache yields an empty allocator (warn at
    /// startup).
    pub fn load_or_create(
        path: PathBuf,
        desktop_slot: u32,
        profile_count: u32,
    ) -> AllocatorResult<Self> {
        if profile_count == 0 {
            return Err(AllocatorError::NoSlots);
        }
        if desktop_slot >= profile_count {
            return Err(AllocatorError::DesktopSlotOutOfRange {
                desktop: desktop_slot,
                count: profile_count,
            });
        }
        if profile_count == 1 {
            return Err(AllocatorError::NoManagedSlots {
                count: profile_count,
                desktop: desktop_slot,
            });
        }

        let (managed, next_seq) = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let file: SlotCacheFile =
                    toml::from_str(&text).map_err(|source| AllocatorError::Parse {
                        path: path.clone(),
                        source,
                    })?;
                if file.profile_count != profile_count || file.desktop_slot != desktop_slot {
                    warn!(
                        cached_count = file.profile_count,
                        actual_count = profile_count,
                        cached_desktop = file.desktop_slot,
                        actual_desktop = desktop_slot,
                        "slot cache shape mismatch (different device?); discarding"
                    );
                    (BTreeMap::new(), 1)
                } else {
                    let mut managed = BTreeMap::new();
                    let mut max_seq = 0;
                    for entry in file.slots {
                        if entry.index < profile_count && entry.index != desktop_slot {
                            max_seq = max_seq.max(entry.last_used_seq);
                            managed.insert(entry.index, entry);
                        }
                    }
                    info!(count = managed.len(), path = %path.display(), "loaded slot cache");
                    (managed, max_seq.saturating_add(1))
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "no slot cache yet; starting empty");
                (BTreeMap::new(), 1)
            }
            Err(source) => return Err(AllocatorError::Io { path, source }),
        };

        Ok(Self {
            path,
            desktop_slot,
            profile_count,
            managed,
            next_seq,
        })
    }

    #[must_use]
    pub const fn desktop_slot(&self) -> u32 {
        self.desktop_slot
    }

    #[must_use]
    pub const fn profile_count(&self) -> u32 {
        self.profile_count
    }

    /// Decide which slot to apply `profile` to. Mutates internal state
    /// (recency, slot↔profile mapping). The caller is responsible for
    /// calling [`Self::save`] to persist; in practice the daemon
    /// saves after every allocation but tests can batch.
    pub fn allocate(&mut self, profile: &GameratProfile) -> Decision {
        // (1) Already materialized?
        if let Some(slot) = self
            .managed
            .iter()
            .find(|(_, e)| e.profile_id == profile.id)
            .map(|(idx, _)| *idx)
        {
            self.touch_slot(slot);
            return Decision {
                slot,
                needs_write: false,
                reason: AllocationReason::Cached,
            };
        }

        // (2) Any empty managed slot? Includes never-touched indices.
        if let Some(slot) = self.first_empty_managed_slot() {
            self.place(slot, profile);
            return Decision {
                slot,
                needs_write: true,
                reason: AllocationReason::EmptySlot,
            };
        }

        // (3) LRU evict (specific-preferred tie-break).
        let evict_slot = self.pick_eviction_target();
        let previous_id = self
            .managed
            .get(&evict_slot)
            .map(|e| e.profile_id.clone())
            .unwrap_or_default();
        self.place(evict_slot, profile);
        Decision {
            slot: evict_slot,
            needs_write: true,
            reason: AllocationReason::Evicted {
                previous_profile_id: previous_id,
            },
        }
    }

    /// Mark a slot as recently used without changing its content.
    /// Useful when the user activates a slot through ratbagd directly
    /// (e.g. via Piper) and the daemon wants to update its LRU view.
    pub fn touch(&mut self, slot: u32) {
        if self.managed.contains_key(&slot) {
            self.touch_slot(slot);
        }
    }

    /// Read-only view of every slot the allocator knows about, for
    /// the daemon's `GetSlotMap` IPC. Returns one [`SlotSnapshot`]
    /// per slot index in `0..profile_count`, including the reserved
    /// Desktop slot (with `is_desktop = true`). Empty slots get
    /// `profile_id = ""`.
    #[must_use]
    pub fn snapshot(&self) -> Vec<SlotSnapshot> {
        (0..self.profile_count)
            .map(|index| {
                let is_desktop = index == self.desktop_slot;
                let entry = self.managed.get(&index);
                SlotSnapshot {
                    index,
                    profile_id: entry.map(|e| e.profile_id.clone()).unwrap_or_default(),
                    is_desktop,
                }
            })
            .collect()
    }

    /// Atomically persist the cache to disk. Mirrors the rule / profile
    /// stores' `<path>.tmp` → `rename` pattern.
    pub fn save(&self) -> AllocatorResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| AllocatorError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let file = SlotCacheFile {
            desktop_slot: self.desktop_slot,
            profile_count: self.profile_count,
            slots: self.managed.values().cloned().collect(),
        };
        let payload =
            toml::to_string_pretty(&file).map_err(|source| AllocatorError::Serialize {
                path: self.path.clone(),
                source,
            })?;
        let tmp = tmp_sibling(&self.path);
        std::fs::write(&tmp, payload).map_err(|source| AllocatorError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &self.path).map_err(|source| AllocatorError::Io {
            path: self.path.clone(),
            source,
        })?;
        debug!(
            path = %self.path.display(),
            count = self.managed.len(),
            "wrote slot cache"
        );
        Ok(())
    }

    // ─── internals ───────────────────────────────────────────────────

    fn touch_slot(&mut self, slot: u32) {
        let seq = self.bump_seq();
        if let Some(entry) = self.managed.get_mut(&slot) {
            entry.last_used_seq = seq;
        }
    }

    fn place(&mut self, slot: u32, profile: &GameratProfile) {
        let seq = self.bump_seq();
        self.managed.insert(
            slot,
            SlotEntry {
                index: slot,
                profile_id: profile.id.clone(),
                category: profile.category.clone(),
                last_used_seq: seq,
            },
        );
    }

    const fn bump_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        seq
    }

    fn first_empty_managed_slot(&self) -> Option<u32> {
        // Empty = either no entry at this index, or an entry with an
        // empty profile_id (defensive — we don't create those, but a
        // hand-edited cache might).
        for idx in self.managed_indices() {
            let is_empty = self
                .managed
                .get(&idx)
                .is_none_or(|e| e.profile_id.is_empty());
            if is_empty {
                return Some(idx);
            }
        }
        None
    }

    fn pick_eviction_target(&self) -> u32 {
        // Prefer evicting the oldest game-specific profile.
        if let Some((slot, _)) = self
            .managed
            .iter()
            .filter(|(_, e)| e.category == game_category::SPECIFIC)
            .min_by_key(|(_, e)| e.last_used_seq)
        {
            return *slot;
        }
        // Fall back to the oldest of anything still present.
        self.managed
            .iter()
            .min_by_key(|(_, e)| e.last_used_seq)
            .map_or(0, |(slot, _)| *slot)
    }

    fn managed_indices(&self) -> impl Iterator<Item = u32> {
        let desktop = self.desktop_slot;
        (0..self.profile_count).filter(move |i| *i != desktop)
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

    fn agnostic(id: &str) -> GameratProfile {
        GameratProfile {
            id: id.to_owned(),
            name: id.to_owned(),
            description: String::new(),
            category: game_category::AGNOSTIC.to_owned(),
            inherits_from: String::new(),
            dpi: vec![800],
            active_dpi_stage: 0,
            created_unix: 0,
            buttons: Vec::new(),
            leds: Vec::new(),
        }
    }

    fn specific(id: &str) -> GameratProfile {
        GameratProfile {
            category: game_category::SPECIFIC.to_owned(),
            ..agnostic(id)
        }
    }

    fn fresh(dir: &TempDir, count: u32) -> SlotAllocator {
        // Desktop = 0, managed = 1..count.
        SlotAllocator::load_or_create(dir.path().join("slot-cache.toml"), 0, count).unwrap()
    }

    #[test]
    fn rejects_zero_slot_devices() {
        let dir = TempDir::new().unwrap();
        let err = SlotAllocator::load_or_create(dir.path().join("a.toml"), 0, 0).unwrap_err();
        assert!(matches!(err, AllocatorError::NoSlots));
    }

    #[test]
    fn rejects_out_of_range_desktop_slot() {
        let dir = TempDir::new().unwrap();
        let err = SlotAllocator::load_or_create(dir.path().join("a.toml"), 5, 3).unwrap_err();
        assert!(matches!(err, AllocatorError::DesktopSlotOutOfRange { .. }));
    }

    #[test]
    fn rejects_one_slot_device() {
        // Single-slot devices can't have both a desktop reservation
        // and any managed slots.
        let dir = TempDir::new().unwrap();
        let err = SlotAllocator::load_or_create(dir.path().join("a.toml"), 0, 1).unwrap_err();
        assert!(matches!(err, AllocatorError::NoManagedSlots { .. }));
    }

    #[test]
    fn allocate_fills_empty_slots_first() {
        let dir = TempDir::new().unwrap();
        let mut allo = fresh(&dir, 5); // desktop=0, managed=[1,2,3,4]

        let d = allo.allocate(&agnostic("fps"));
        assert_eq!(d.slot, 1);
        assert!(d.needs_write);
        assert_eq!(d.reason, AllocationReason::EmptySlot);

        let d = allo.allocate(&agnostic("mmo"));
        assert_eq!(d.slot, 2);
        assert!(d.needs_write);
    }

    #[test]
    fn cache_hit_returns_same_slot_without_write() {
        let dir = TempDir::new().unwrap();
        let mut allo = fresh(&dir, 5);
        let first = allo.allocate(&agnostic("fps"));
        let second = allo.allocate(&agnostic("fps"));
        assert_eq!(first.slot, second.slot);
        assert!(!second.needs_write);
        assert_eq!(second.reason, AllocationReason::Cached);
    }

    #[test]
    fn lru_evicts_oldest_when_full() {
        let dir = TempDir::new().unwrap();
        // 2 managed slots so we hit the eviction path fast.
        let mut allo = fresh(&dir, 3); // desktop=0, managed=[1,2]
        allo.allocate(&agnostic("a")); // → slot 1
        allo.allocate(&agnostic("b")); // → slot 2
        // Touch "a" so "b" is the oldest.
        allo.allocate(&agnostic("a"));
        // Now allocate "c" — must evict the LRU (b).
        let d = allo.allocate(&agnostic("c"));
        assert_eq!(d.slot, 2);
        assert_eq!(
            d.reason,
            AllocationReason::Evicted {
                previous_profile_id: "b".to_owned()
            }
        );
    }

    #[test]
    fn eviction_prefers_specific_over_agnostic() {
        let dir = TempDir::new().unwrap();
        let mut allo = fresh(&dir, 3); // desktop=0, managed=[1,2]
        // Insert agnostic first (it's the OLDEST), then specific.
        allo.allocate(&agnostic("fps")); // slot 1, seq=1
        allo.allocate(&specific("cs2")); // slot 2, seq=2
        // Allocate something new. Pure LRU would evict the agnostic
        // (slot 1, older). The tie-break-preferring-specific policy
        // evicts the specific (slot 2) instead.
        let d = allo.allocate(&agnostic("mmo"));
        assert_eq!(d.slot, 2);
        assert!(matches!(
            d.reason,
            AllocationReason::Evicted { previous_profile_id } if previous_profile_id == "cs2"
        ));
    }

    #[test]
    fn touch_updates_recency() {
        let dir = TempDir::new().unwrap();
        let mut allo = fresh(&dir, 3);
        allo.allocate(&agnostic("a")); // slot 1
        allo.allocate(&agnostic("b")); // slot 2
        // Without touching, b is younger. Touch a to flip the order.
        allo.touch(1);
        // Now allocate c — evicts b (now the LRU).
        let d = allo.allocate(&agnostic("c"));
        assert_eq!(d.slot, 2);
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("slot-cache.toml");
        {
            let mut allo = SlotAllocator::load_or_create(path.clone(), 0, 5).unwrap();
            allo.allocate(&agnostic("fps"));
            allo.allocate(&specific("cs2"));
            allo.save().unwrap();
        }
        let reloaded = SlotAllocator::load_or_create(path, 0, 5).unwrap();
        assert_eq!(reloaded.managed.len(), 2);
        // After reload, cached fps stays in slot 1 — re-allocating
        // returns Cached.
        let mut reloaded = reloaded;
        let d = reloaded.allocate(&agnostic("fps"));
        assert_eq!(d.slot, 1);
        assert_eq!(d.reason, AllocationReason::Cached);
    }

    #[test]
    fn load_discards_cache_on_device_shape_mismatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("slot-cache.toml");
        // First session: 5-slot device.
        {
            let mut allo = SlotAllocator::load_or_create(path.clone(), 0, 5).unwrap();
            allo.allocate(&agnostic("fps"));
            allo.save().unwrap();
        }
        // Second session: 3-slot device (e.g. user plugged in a different mouse).
        // Cache should be discarded — no Cached hit for "fps".
        let mut allo = SlotAllocator::load_or_create(path, 0, 3).unwrap();
        let d = allo.allocate(&agnostic("fps"));
        assert_eq!(d.reason, AllocationReason::EmptySlot);
    }

    #[test]
    fn load_drops_cache_entries_at_desktop_slot_index() {
        // If the user reconfigures desktop_slot, any cached entry at
        // the new desktop index must be dropped (it's no longer
        // managed).
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("slot-cache.toml");
        {
            let mut allo = SlotAllocator::load_or_create(path.clone(), 0, 5).unwrap();
            allo.allocate(&agnostic("fps")); // → slot 1
            allo.save().unwrap();
        }
        // Reload with desktop_slot moved to 1 — but profile_count and
        // structure otherwise match? Actually the mismatch logic
        // discards on ANY shape change. So we expect the cache to be
        // fully wiped.
        let mut allo = SlotAllocator::load_or_create(path, 1, 5).unwrap();
        let d = allo.allocate(&agnostic("fps"));
        assert_eq!(d.reason, AllocationReason::EmptySlot);
        // First non-desktop slot is 0 now.
        assert_eq!(d.slot, 0);
    }

    #[test]
    fn empty_eviction_for_all_agnostic_falls_through_to_overall_lru() {
        // All managed slots hold agnostic profiles — no specific to
        // evict — so we evict the overall LRU.
        let dir = TempDir::new().unwrap();
        let mut allo = fresh(&dir, 3); // managed = [1, 2]
        allo.allocate(&agnostic("a")); // slot 1, oldest
        allo.allocate(&agnostic("b")); // slot 2
        let d = allo.allocate(&agnostic("c"));
        assert_eq!(d.slot, 1); // evicted "a"
    }
}
