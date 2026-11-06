//! Saved fits and which of them are open, kept as JSON in the user's data directory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fit::Fit;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub fits: Vec<Fit>,
    /// Fits open as tabs, by index into `fits`.
    pub open: Vec<usize>,
    pub current: Option<usize>,
    /// Fitting view column widths the user set by dragging, by column.
    pub columns: std::collections::BTreeMap<String, f32>,
}

impl Saved {
    /// What was saved last time, or nothing if there is no save or it cannot be read.
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        let mut saved: Self = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        saved.open.retain(|&i| i < saved.fits.len());
        saved.current = saved.current.filter(|c| saved.open.contains(c));
        saved
    }

    /// Write the fits out. Goes through a temporary file so a crash mid-write cannot lose them.
    pub fn save(&self) -> Result<(), String> {
        let path = path().ok_or("no home directory")?;
        let dir = path.parent().ok_or("no data directory")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn path() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("gyfa/fits.json"))
}
