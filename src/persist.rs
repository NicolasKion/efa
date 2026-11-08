//! Files around efa's database: reading the JSON files efa kept its data in before, which
//! [`crate::db`] imports once, setting aside files that cannot be read rather than overwriting
//! them, creating files only the user can read, and writing files so a reader never sees half
//! of one.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// The file at `path`, or its backup when the file itself cannot be read, or the default when
/// there is neither. A file that exists but does not parse is renamed so nothing overwrites it.
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
                    shown(aside.as_deref())
                ),
            );
        }
    }
    match read(&backup(path)) {
        Read::Ok(value) => value,
        Read::Missing | Read::Broken(_) => T::default(),
    }
}

/// Write `bytes` to `path` through a temporary file next to it, so the file is either the old
/// one or the whole new one, never a torn mix. Missing folders are created.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    // Per process and per write, so two writers of one file never share a temporary file.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let n = WRITES.fetch_add(1, Ordering::Relaxed);
    let tmp = with_suffix(path, &format!(".{}-{n}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
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

/// Create `path` empty if it is not there, readable by the user only, and make an existing one
/// owner-only too. SQLite gives the files it makes next to a database the database's mode.
pub fn create_private(path: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    open_private(&options, path).map(drop)
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

/// The copy the JSON saves kept of the file before.
pub fn backup(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

/// `path` with `suffix` added to its file name.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Rename an unreadable file out of the way, with the time in its name.
pub fn set_aside(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let aside = with_suffix(path, &format!(".unreadable-{secs}"));
    std::fs::rename(path, &aside).ok().map(|()| aside)
}

/// A path for a message, or a note that there is none.
pub fn shown(path: Option<&Path>) -> String {
    path.map_or("<not moved>".into(), |p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_file_is_kept_and_the_backup_loads() {
        let dir = std::env::temp_dir().join(format!("efa-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("fits.json");
        std::fs::write(backup(&path), b"[1, 2, 3]").expect("write the backup");
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

    #[test]
    fn atomic_writes_replace_the_file_and_leave_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("efa-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("deeper").join("file.bin");
        write_atomic(&path, b"first").expect("write into a new folder");
        write_atomic(&path, b"second").expect("overwrite");
        assert_eq!(std::fs::read(&path).expect("read"), b"second");
        let files = std::fs::read_dir(path.parent().expect("parent"))
            .expect("dir")
            .count();
        assert_eq!(files, 1, "no temporary file is left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("efa-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("secret");
        std::fs::write(&path, b"kept").expect("write");
        create_private(&path).expect("make private");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            std::fs::read(&path).expect("read"),
            b"kept",
            "not truncated"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
