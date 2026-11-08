//! Self-updates from GitHub releases. Each release carries `latest.json`, which names a signed
//! bundle per platform: the `AppImage` on Linux, a `.app.tar.gz` on macOS and the NSIS installer on
//! Windows. The signature is checked against the minisign key baked into release builds.
//!
//! Builds without the key (local ones) and installs that cannot replace themselves (the .deb,
//! the AUR package, the tarball, the portable exe) still hear about new releases but only link to
//! the release page. Everything here blocks, so it runs off the UI thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cargo_packager_updater::semver::Version;
use cargo_packager_updater::{Config, Error, Update, UpdaterBuilder};
use serde::Deserialize;

use crate::preferences::Proxy;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const REPO: &str = "NicolasKion/efa";

/// The updater's public key, from the `UPDATER_PUBKEY` repository variable in release builds.
const PUBLIC_KEY: Option<&str> = option_env!("EFA_UPDATER_PUBKEY");

/// The manifest of the newest stable release; GitHub's "latest" never points at a pre-release.
const STABLE_MANIFEST: &str =
    "https://github.com/NicolasKion/efa/releases/latest/download/latest.json";

/// A release newer than this build.
pub struct Available {
    pub version: String,
    update: Update,
}

impl Available {
    pub fn release_url(&self) -> String {
        release_url(&self.version)
    }

    /// Whether this copy can install the release itself, rather than only link to it.
    pub fn installable(&self) -> bool {
        public_key().is_some()
            && !cfg!(debug_assertions)
            && !portable()
            && (!cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some())
    }
}

/// How far a download got, in bytes. The total is 0 until the server says.
#[derive(Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    /// Percent done, when the size is known.
    pub fn percent(&self) -> Option<u64> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed) * 100 / total).min(100))
    }
}

pub fn release_url(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{version}")
}

/// A pre-release version ("0.3.0-preview.1") marks a preview build, which also hears about newer
/// previews.
pub fn is_preview(version: &str) -> bool {
    version.contains('-')
}

fn public_key() -> Option<&'static str> {
    PUBLIC_KEY.map(str::trim).filter(|key| !key.is_empty())
}

/// The portable Windows exe has no installer to update; the installed one lives under the
/// per-user programs folder the NSIS installer uses.
fn portable() -> bool {
    cfg!(windows)
        && std::env::current_exe().is_ok_and(|exe| {
            let exe = exe.to_string_lossy().to_lowercase();
            !exe.contains(r"\appdata\local\") && !exe.contains(r"\program files")
        })
}

/// The newest release if it is newer than this build.
pub fn check(proxy: &Proxy) -> Result<Option<Available>, String> {
    let current = Version::parse(VERSION).map_err(|e| e.to_string())?;
    let manifest = if is_preview(VERSION) {
        match newest_release(proxy)? {
            Some(tag) => format!("https://github.com/{REPO}/releases/download/{tag}/latest.json"),
            None => return Ok(None),
        }
    } else {
        STABLE_MANIFEST.to_owned()
    };
    let config = Config {
        endpoints: vec![manifest.parse().map_err(|e| format!("{e}"))?],
        // Checking reads no signatures; without a key the release is only linked to.
        pubkey: public_key().unwrap_or_default().to_owned(),
        windows: None,
    };
    let updater = UpdaterBuilder::new(current, config)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    match updater.check() {
        Ok(update) => Ok(update.map(|update| Available {
            version: update.version.trim_start_matches('v').to_owned(),
            update,
        })),
        // No release with a manifest yet, or none for this platform.
        Err(Error::ReleaseNotFound | Error::TargetNotFound(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
}

/// The tag of the newest release, previews included.
fn newest_release(proxy: &Proxy) -> Result<Option<String>, String> {
    let client = proxy.client()?;
    let body = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases?per_page=20"
        ))
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::text)
        .map_err(|e| e.to_string())?;
    let releases: Vec<Release> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(newest(&releases).map(|r| r.tag_name.clone()))
}

fn newest(releases: &[Release]) -> Option<&Release> {
    releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| Some((Version::parse(r.tag_name.trim_start_matches('v')).ok()?, r)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, r)| r)
}

/// Downloads the release's bundle and checks its signature.
pub fn download(available: &Available, progress: &Arc<Progress>) -> Result<Vec<u8>, String> {
    available
        .update
        .download_extended(
            |chunk, total| {
                if let Some(total) = total {
                    progress.total.store(total, Ordering::Relaxed);
                }
                progress.done.fetch_add(chunk as u64, Ordering::Relaxed);
            },
            || {},
        )
        .map_err(|e| format!("Downloading the update failed: {e}"))
}

/// Puts the downloaded bundle in place. On Windows this starts the installer and exits; the
/// installer starts efa again when it is done.
pub fn install(available: &Available, bundle: Vec<u8>) -> Result<(), String> {
    available
        .update
        .install(bundle)
        .map_err(|e| format!("Installing the update failed: {e}"))
}

/// Whether installing already restarts efa (Windows), or the app has to after it (Linux, macOS).
pub const INSTALL_RESTARTS: bool = cfg!(windows);

/// Starts the installed version. The caller quits right after.
pub fn restart(available: &Available) -> Result<(), String> {
    // The AppImage on Linux, the .app bundle on macOS.
    let path: PathBuf = available.update.extract_path.clone();
    let started = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-n")
            .arg(&path)
            .spawn()
    } else {
        std::process::Command::new(&path).spawn()
    };
    started
        .map(drop)
        .map_err(|e| format!("Could not start the new version: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft,
        }
    }

    #[test]
    fn newest_release_wins_by_semver() {
        let releases = [
            release("v0.2.0", false),
            release("v0.3.0-preview.2", false),
            release("v0.3.0-preview.10", false),
            release("v0.4.0", true),
            release("nonsense", false),
        ];
        assert_eq!(
            newest(&releases).map(|r| r.tag_name.as_str()),
            Some("v0.3.0-preview.10")
        );
        assert!(newest(&[]).is_none());
    }

    #[test]
    fn previews() {
        assert!(is_preview("0.3.0-preview.1"));
        assert!(!is_preview("0.3.0"));
    }

    #[test]
    fn progress() {
        let progress = Progress::default();
        assert_eq!(progress.percent(), None);
        progress.total.store(200, Ordering::Relaxed);
        progress.done.store(50, Ordering::Relaxed);
        assert_eq!(progress.percent(), Some(25));
    }
}
