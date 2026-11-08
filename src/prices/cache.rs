//! Prices on disk: what was fetched last, when it goes stale, and writing it back.

use std::{collections::HashSet, path::Path};

use super::{Origin, Prices};
use crate::{data::TypeId, net::now};

const CACHE_FILE: &str = "prices.json";

impl Prices {
    /// What was fetched last time, whatever its age; empty when nothing was.
    pub fn cached(cache_dir: &Path) -> Self {
        std::fs::read(cache_dir.join(CACHE_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Forget every price and the copy on disk, pyfa's "Delete All Prices". The next refresh
    /// fetches them again.
    pub fn delete_cached(cache_dir: &Path) -> Result<(), String> {
        match std::fs::remove_file(cache_dir.join(CACHE_FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    /// Whether these prices may be out of date.
    pub fn is_stale(&self) -> bool {
        now() >= self.expires
    }

    /// Whether these prices need fetching for `origin`: they expired, came from elsewhere, or
    /// one of `types` was never asked about.
    pub fn is_stale_for(&self, origin: Origin, types: &HashSet<TypeId>) -> bool {
        self.origin != origin || self.is_stale() || !types.is_subset(&self.asked)
    }

    /// Best effort: without the file the next start fetches again.
    pub(super) fn store(&self, cache_dir: &Path) {
        let Ok(json) = serde_json::to_vec(self) else {
            return;
        };
        let _ = crate::persist::write_atomic(&cache_dir.join(CACHE_FILE), &json);
    }
}
