//! On-disk parse cache — the only persistence noslop keeps (ARCHITECTURE.md §0).
//!
//! We cache the expensive step (parse + extract per file), keyed by content
//! hash, and recompute the cheap step (graph + passes) every run. This is why a
//! warm scan is sub-second while staying deterministic and trivially
//! invalidated: a changed file mismatches its hash and is reparsed.

use noslop_graph::FileFacts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const CACHE_REL: &str = ".noslopcode/cache/facts.json";

#[derive(Serialize, Deserialize)]
struct Entry {
    hash: u64,
    facts: FileFacts,
}

/// A content-hash-keyed store of per-file facts.
#[derive(Default)]
pub struct Cache {
    entries: HashMap<PathBuf, Entry>,
    /// Whether any usable cache was loaded (drives the "warm cache" label).
    loaded: bool,
}

impl Cache {
    /// Load the cache from `root`, or an empty cache if absent/unreadable.
    pub fn load(root: &Path) -> Self {
        let path = root.join(CACHE_REL);
        let Ok(bytes) = std::fs::read(&path) else {
            return Cache::default();
        };
        match serde_json::from_slice::<HashMap<PathBuf, Entry>>(&bytes) {
            Ok(entries) => Cache {
                loaded: !entries.is_empty(),
                entries,
            },
            Err(_) => Cache::default(),
        }
    }

    pub fn was_loaded(&self) -> bool {
        self.loaded
    }

    /// Return cached facts for `path` iff the content hash still matches.
    pub fn get(&self, path: &Path, hash: u64) -> Option<FileFacts> {
        self.entries
            .get(path)
            .filter(|e| e.hash == hash)
            .map(|e| e.facts.clone())
    }

    /// Persist the given facts as the new cache, best-effort (a cache write
    /// failure must never fail a scan).
    pub fn save(root: &Path, facts: &[FileFacts]) {
        let path = root.join(CACHE_REL);
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let map: HashMap<&Path, Entry> = facts
            .iter()
            .map(|f| {
                (
                    f.path.as_path(),
                    Entry {
                        hash: f.content_hash,
                        facts: f.clone(),
                    },
                )
            })
            .collect();
        if let Ok(bytes) = serde_json::to_vec(&map) {
            let _ = std::fs::write(&path, bytes);
        }
    }
}
