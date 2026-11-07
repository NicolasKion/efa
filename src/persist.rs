//! Reading and writing gyfa's JSON files without ever losing them. A file that cannot be read
//! is set aside rather than overwritten, and every save keeps the previous file as a backup to
//! fall back on.

use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

/// The file at `path`, or its backup when the file itself cannot be read, or the default when
/// there is neither. A file that exists but does not parse is renamed so the next save cannot
/// overwrite it.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    match read(path) {
        Read::Ok(value) => return value,
        Read::Missing => {}
        Read::Broken(e) => {
            let aside = set_aside(path);
            crate::errors::report(
                format!("{} could not be read", path.display()),
                format!(
                    "{e}\n\nThe file was kept as {} and the backup loaded instead.",
                    aside
                        .as_deref()
                        .map_or("<not moved>".into(), |p| p.display().to_string()),
                ),
            );
        }
    }
    match read(&backup(path)) {
        Read::Ok(value) => value,
        Read::Missing | Read::Broken(_) => T::default(),
    }
}

/// Write `value` to `path` through a temporary file, keeping what was there as the backup.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let dir = path.parent().ok_or("no data directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let json = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("{}: {e}", tmp.display()))?;
    // Only a file that still reads is worth keeping as the backup.
    if matches!(read::<serde_json::Value>(path), Read::Ok(_)) {
        let _ = std::fs::copy(path, backup(path));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

enum Read<T> {
    Ok(T),
    Missing,
    Broken(String),
}

fn read<T: DeserializeOwned>(path: &Path) -> Read<T> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Read::Ok(value),
            Err(e) => Read::Broken(e.to_string()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Read::Missing,
        Err(e) => Read::Broken(e.to_string()),
    }
}

fn backup(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

/// Rename an unreadable file out of the way, with the time in its name.
fn set_aside(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let aside = path.with_extension(format!("json.unreadable-{secs}"));
    std::fs::rename(path, &aside).ok().map(|()| aside)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_file_is_kept_and_the_backup_loads() {
        let dir = std::env::temp_dir().join(format!("gyfa-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("fits.json");
        save(&path, &vec![1, 2, 3]).expect("first save");
        save(&path, &vec![4, 5]).expect("second save keeps the first as backup");
        std::fs::write(&path, b"{ not json").expect("break the file");
        let loaded: Vec<i32> = load(&path);
        assert_eq!(loaded, vec![1, 2, 3], "the backup loads");
        let kept = std::fs::read_dir(&dir)
            .expect("dir")
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains("unreadable"));
        assert!(kept, "the broken file is set aside, not overwritten");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
