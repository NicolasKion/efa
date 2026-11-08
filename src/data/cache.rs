//! What is built from the dump, kept on disk as postcard under a key naming what it was built
//! from, so the next start loads it in a fraction of the time.

use std::path::Path;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// The file on disk: the key it was built for, then the value.
#[derive(Deserialize)]
struct File<T> {
    key: String,
    value: T,
}

/// [`File`] for writing, without copying the value. Same fields in the same order, which is all
/// postcard's layout depends on.
#[derive(Serialize)]
struct FileRef<'a, T> {
    key: &'a str,
    value: &'a T,
}

/// The value cached at `path` when it was built for `key`.
pub fn read<T: DeserializeOwned>(path: &Path, key: &str) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    let file: File<T> = postcard::from_bytes(&bytes).ok()?;
    (file.key == key).then_some(file.value)
}

/// Best effort: without a cache the next start just builds the value again.
pub fn write<T: Serialize>(path: &Path, key: &str, value: &T) {
    let Ok(bytes) = postcard::to_stdvec(&FileRef { key, value }) else {
        return;
    };
    let _ = crate::persist::write_atomic(path, &bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_loads_only_under_its_key() {
        let dir = std::env::temp_dir().join(format!("efa-cache-{}", std::process::id()));
        let path = dir.join("value.bin");
        write(&path, "v1", &vec![1_u32, 2, 3]);
        assert_eq!(read::<Vec<u32>>(&path, "v1"), Some(vec![1, 2, 3]));
        assert_eq!(read::<Vec<u32>>(&path, "v2"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
