//! Where efa keeps things. User data (the database of fits, characters and logins, the crash
//! note) goes in the platform's data folder and everything that can be fetched or built again
//! (icons, prices, the CDN index, caches of a development dump) in its cache folder:
//!
//! - Linux: `$XDG_DATA_HOME/efa` (`~/.local/share/efa`) and `$XDG_CACHE_HOME/efa` (`~/.cache/efa`)
//! - macOS: `~/Library/Application Support/efa` and `~/Library/Caches/efa`
//! - Windows: `%APPDATA%\efa` and `%LOCALAPPDATA%\efa`

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

const APP: &str = "efa";
/// efa's name before, whose folder [`crate::persist::move_old_data`] brings along.
const OLD_APP: &str = "gyfa";
/// Where a development build looks for a deimos dump when `EFA_DATA` is not set.
const DEFAULT_DUMP: &str = "data/eve";

/// efa's data folder. None when the platform has no home to put it in.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP))
}

/// A file in efa's data folder.
pub fn data_file(name: &str) -> Option<PathBuf> {
    data_dir().map(|d| d.join(name))
}

/// The folder efa kept its data in when it was called gyfa.
pub fn old_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(OLD_APP))
}

/// efa's cache folder. Without a platform cache folder it falls back to the temporary one, since
/// everything in it can be had again.
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().map_or_else(|| std::env::temp_dir().join(APP), |d| d.join(APP))
}

/// Where the catalog, item data and engine data built from a development dump are cached.
pub fn dump_cache_dir() -> PathBuf {
    cache_dir().join("dump")
}

/// The deimos dump a build without an embedded data pack reads: `EFA_DATA`, or `data/eve`.
pub fn dump_dir() -> PathBuf {
    dump_dir_from(std::env::var_os("EFA_DATA"))
}

/// Whether `EFA_DATA` asks for a dump instead of the embedded data.
pub fn dump_requested() -> bool {
    std::env::var_os("EFA_DATA").is_some_and(|v| !v.is_empty())
}

fn dump_dir_from(var: Option<OsString>) -> PathBuf {
    var.filter(|v| !v.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_DUMP), PathBuf::from)
}

/// The user's home folder, where file dialogs open. The working folder if there is none.
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// A file in the user's home folder, as a path relative to it, such as Omarchy's theme.
pub fn in_home(relative: &Path) -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_and_cache_are_apart_and_named_efa() {
        let cache = cache_dir();
        assert!(cache.ends_with(APP));
        assert!(dump_cache_dir().starts_with(&cache));
        if let Some(data) = data_dir() {
            assert!(data.ends_with(APP));
            assert_ne!(data, cache, "user data never sits among the caches");
            assert_eq!(data_file("fits.json"), Some(data.join("fits.json")));
            assert_eq!(
                old_data_dir().and_then(|old| old.parent().map(Path::to_owned)),
                data.parent().map(Path::to_owned),
                "the old folder sits next to the new one"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_follows_xdg() {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")));
        assert_eq!(data_dir(), data.map(|d| d.join(APP)));
    }

    #[test]
    fn efa_data_picks_the_dump() {
        assert_eq!(dump_dir_from(None), PathBuf::from("data/eve"));
        assert_eq!(dump_dir_from(Some("".into())), PathBuf::from("data/eve"));
        assert_eq!(
            dump_dir_from(Some("/srv/eve".into())),
            PathBuf::from("/srv/eve")
        );
    }

    #[test]
    fn home_is_never_empty() {
        assert!(!home_dir().as_os_str().is_empty());
    }
}
