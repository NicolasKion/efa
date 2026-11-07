//! Reading and writing efa's JSON files without ever losing them. A file that cannot be read
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

/// The app was called gyfa before; its folder moves to efa's the first time efa starts, so fits,
/// characters and logins come along. Nothing happens when efa's folder already exists.
pub fn move_old_data() {
    let (Some(new), Some(old)) = (crate::paths::data_dir(), crate::paths::old_data_dir()) else {
        return;
    };
    if new.exists() || !old.is_dir() {
        return;
    }
    if let Err(e) = copy_dir(&old, &new) {
        eprintln!("copying {} to {}: {e}", old.display(), new.display());
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Write `value` to `path` through a temporary file, keeping what was there as the backup.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write(path, value, false)
}

/// [`save`] for secrets: the file and its backup are readable by the user only.
pub fn save_private<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write(path, value, true)
}

fn write<T: Serialize>(path: &Path, value: &T, private: bool) -> Result<(), String> {
    let dir = path.parent().ok_or("no data directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let json = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    write_file(&tmp, &json, private).map_err(|e| format!("{}: {e}", tmp.display()))?;
    // Only a file that still reads is worth keeping as the backup.
    if matches!(read::<serde_json::Value>(path), Read::Ok(_)) {
        let _ = std::fs::copy(path, backup(path));
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Create or truncate a file and write it, owner-only from the start when `private`.
fn write_file(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    let file = if private {
        open_private(&options, path)?
    } else {
        options.open(path)?
    };
    (&file).write_all(bytes)
}

#[cfg(unix)]
fn open_private(options: &std::fs::OpenOptions, path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    let file = options.clone().mode(0o600).open(path)?;
    // The mode only applies to a new file; a leftover one keeps its own.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Windows has no mode bits. Files under `%APPDATA%` inherit the profile's ACL, which already
/// keeps other users out, so the plain open is as private as it gets without Win32 calls.
#[cfg(not(unix))]
fn open_private(options: &std::fs::OpenOptions, path: &Path) -> std::io::Result<std::fs::File> {
    options.open(path)
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
        let dir = std::env::temp_dir().join(format!("efa-persist-{}", std::process::id()));
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
