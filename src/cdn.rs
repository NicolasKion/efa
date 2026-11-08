//! Client art straight off CCP's content CDN, found the way the launcher finds it:
//! `eveclient_TQ.json` names the build, `eveonline_<build>.txt` points at the resource index, and
//! the resource index maps every `res:/` path to a content hash served by resources.eveonline.com.
//!
//! The resource index is about 20 MB, so it is kept on disk per build. Only the newest one is
//! kept, and it is used as it is when the build cannot be asked for.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const BINARIES: &str = "https://binaries.eveonline.com";
const RESOURCES: &str = "https://resources.eveonline.com";
const INDEX_PREFIX: &str = "resfileindex-";
const INDEX_SUFFIX: &str = ".txt";

pub struct Cdn {
    http: reqwest::blocking::Client,
    /// Lowercased `res:/` path to content hash. Only art is kept.
    index: HashMap<String, String>,
}

impl Cdn {
    /// Find the current build's resource index, downloading it when it is not cached yet. When
    /// the CDN cannot be reached the newest cached index stands in, since art rarely changes.
    pub fn connect(http: reqwest::blocking::Client, cache_dir: &Path) -> Result<Self, String> {
        let cached = match current_index(&http, cache_dir) {
            Ok(path) => {
                remove_older_indexes(cache_dir, &path);
                path
            }
            Err(e) => newest_cached_index(cache_dir).ok_or(e)?,
        };
        let text =
            std::fs::read_to_string(&cached).map_err(|e| format!("{}: {e}", cached.display()))?;
        let index = text
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(',');
                let path = fields.next()?.to_lowercase();
                let wanted = path.starts_with("res:/ui/") || path.contains("/icons/");
                wanted.then(|| Some((path, fields.next()?.to_owned())))?
            })
            .collect();
        Ok(Self { http, index })
    }

    pub fn fetch(&self, res_path: &str) -> Result<Vec<u8>, String> {
        let hash = self
            .index
            .get(&res_path.to_lowercase())
            .ok_or_else(|| format!("{res_path} is not in the client"))?;
        get_bytes(&self.http, &format!("{RESOURCES}/{hash}"))
    }
}

/// The cached resource index of the build the CDN serves now, fetched first if need be.
fn current_index(http: &reqwest::blocking::Client, cache_dir: &Path) -> Result<PathBuf, String> {
    #[derive(serde::Deserialize)]
    struct ClientInfo {
        build: String,
    }

    let info: ClientInfo =
        serde_json::from_reader(get(http, &format!("{BINARIES}/eveclient_TQ.json"))?)
            .map_err(|e| format!("client info: {e}"))?;
    // The build goes into a file name.
    if info.build.is_empty() || !info.build.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("odd client build \"{}\"", info.build));
    }
    let cached = cache_dir.join(format!("{INDEX_PREFIX}{}{INDEX_SUFFIX}", info.build));
    if cached.exists() {
        return Ok(cached);
    }
    let app_index = get(http, &format!("{BINARIES}/eveonline_{}.txt", info.build))?;
    let res_index = BufReader::new(app_index)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            let mut fields = line.split(',');
            (fields.next()? == "app:/resfileindex.txt").then(|| fields.next().map(str::to_owned))?
        })
        .ok_or("the client index has no resource index")?;
    let bytes = get_bytes(http, &format!("{BINARIES}/{res_index}"))?;
    crate::persist::write_atomic(&cached, &bytes)
        .map_err(|e| format!("{}: {e}", cached.display()))?;
    Ok(cached)
}

/// Every resource index in the cache folder, with when it was written.
fn cached_indexes(cache_dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with(INDEX_PREFIX) && name.ends_with(INDEX_SUFFIX)
        })
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect()
}

fn newest_cached_index(cache_dir: &Path) -> Option<PathBuf> {
    cached_indexes(cache_dir)
        .into_iter()
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

/// Old builds' indexes are 20 MB each and never read again once a newer one is there.
fn remove_older_indexes(cache_dir: &Path, current: &Path) {
    for (path, _) in cached_indexes(cache_dir) {
        if path != current {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn get(http: &reqwest::blocking::Client, url: &str) -> Result<reqwest::blocking::Response, String> {
    http.get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("{url}: {e}"))
}

/// The body of a successful GET.
pub fn get_bytes(http: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, String> {
    Ok(get(http, url)?
        .bytes()
        .map_err(|e| format!("{url}: {e}"))?
        .to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_cached_index_stands_in_and_older_ones_go() {
        let dir = std::env::temp_dir().join(format!("efa-cdn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let old = dir.join("resfileindex-1.txt");
        let new = dir.join("resfileindex-2.txt");
        std::fs::write(&old, b"old").expect("write");
        std::fs::write(dir.join("unrelated.txt"), b"x").expect("write");
        // Some filesystems keep whole seconds.
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .and_then(|f| f.set_modified(earlier))
            .expect("age the old index");
        std::fs::write(&new, b"new").expect("write");
        assert_eq!(newest_cached_index(&dir), Some(new.clone()));
        remove_older_indexes(&dir, &new);
        assert!(!old.exists());
        assert!(new.exists());
        assert!(dir.join("unrelated.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
