//! Icons for items and attributes, as files on disk.
//!
//! The deimos dump carries the attribute and market group icons. Item icons are fetched from CCP's
//! CDN the first time they are asked for and kept on disk, so the cost is paid once per icon.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
};

use crate::{cdn::Cdn, data::Catalog};

/// Downloads running at once. Opening a market group asks for dozens of icons together.
const WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// An icon ID, as attributes and market groups carry.
    Id(i32),
    /// An item's own icon.
    Type(i32),
    /// An overview bracket, by texture file name without the extension; see [`crate::brackets`].
    Bracket(&'static str),
    /// A corporation's logo from CCP's image server, such as a faction's.
    CorpLogo(i32),
}

#[derive(Debug)]
enum State {
    Ready(PathBuf),
    Fetching,
    Missing,
}

type States = Arc<Mutex<HashMap<Icon, State>>>;

pub struct Icons {
    dump_dir: PathBuf,
    cache_dir: PathBuf,
    states: States,
    fetch: Sender<(Icon, String, PathBuf)>,
    /// Set when a download lands, so the UI knows to redraw.
    updated: Arc<AtomicBool>,
}

impl Icons {
    pub fn new(data_dir: &Path, cache_dir: &Path) -> Self {
        let states: States = Arc::default();
        let updated = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<(Icon, String, PathBuf)>();
        let rx = Arc::new(Mutex::new(rx));
        // Connecting costs a few seconds the first time, so it only happens once something is
        // actually missing, and the workers share the one connection.
        let cdn: Arc<OnceLock<Result<Cdn, String>>> = Arc::default();
        for _ in 0..WORKERS {
            let (rx, states, cdn, updated) = (
                Arc::clone(&rx),
                Arc::clone(&states),
                Arc::clone(&cdn),
                Arc::clone(&updated),
            );
            let cdn_cache = cache_dir.to_owned();
            std::thread::spawn(move || {
                loop {
                    let job = rx.lock().expect("icon queue lock poisoned").recv();
                    let Ok((icon, res_path, dest)) = job else {
                        return;
                    };
                    let state = match fetch(&cdn, &cdn_cache, &res_path, &dest) {
                        Ok(()) => State::Ready(dest),
                        Err(_) => State::Missing,
                    };
                    states
                        .lock()
                        .expect("icon state lock poisoned")
                        .insert(icon, state);
                    updated.store(true, Ordering::Release);
                }
            });
        }
        Self {
            dump_dir: data_dir.join("icons"),
            cache_dir: cache_dir.join("icons"),
            states,
            fetch: tx,
            updated,
        }
    }

    /// Path of the icon if it is on disk; otherwise starts fetching it and returns None for now.
    pub fn path(&self, catalog: &Catalog, icon: Icon) -> Option<PathBuf> {
        let mut states = self.states.lock().expect("icon state lock poisoned");
        match states.get(&icon) {
            Some(State::Ready(path)) => return Some(path.clone()),
            Some(State::Fetching | State::Missing) => return None,
            None => {}
        }
        let (dumped, cached, res_path) = match icon {
            Icon::Id(id) => (
                Some(self.dump_dir.join(format!("{id}.png"))),
                self.cache_dir.join(format!("{id}.png")),
                catalog.icon_files.get(&id).cloned(),
            ),
            Icon::Type(id) => (
                None,
                self.cache_dir.join("types").join(format!("{id}.png")),
                catalog.get(id).and_then(|t| t.icon.clone()),
            ),
            Icon::Bracket(name) => (
                None,
                self.cache_dir.join("brackets").join(format!("{name}.png")),
                Some(format!("res:/ui/texture/shared/brackets/{name}.png")),
            ),
            Icon::CorpLogo(id) => (
                None,
                self.cache_dir.join("corps").join(format!("{id}.png")),
                Some(format!(
                    "https://images.evetech.net/corporations/{id}/logo?size=64"
                )),
            ),
        };
        let on_disk = dumped
            .filter(|p| p.exists())
            .or_else(|| cached.exists().then(|| cached.clone()));
        if let Some(path) = on_disk {
            let path = std::path::absolute(&path).unwrap_or(path);
            states.insert(icon, State::Ready(path.clone()));
            return Some(path);
        }
        match res_path {
            Some(res_path) => {
                states.insert(icon, State::Fetching);
                let dest = std::path::absolute(&cached).unwrap_or(cached);
                let _ = self.fetch.send((icon, res_path, dest));
            }
            None => {
                states.insert(icon, State::Missing);
            }
        }
        None
    }

    /// Whether any icon arrived since the last call.
    pub fn take_updated(&self) -> bool {
        self.updated.swap(false, Ordering::AcqRel)
    }
}

fn fetch(
    cdn: &OnceLock<Result<Cdn, String>>,
    cache: &Path,
    res_path: &str,
    dest: &Path,
) -> Result<(), String> {
    let bytes = if res_path.starts_with("https://") {
        // The image server, not the game's resource CDN.
        let response = reqwest::blocking::Client::builder()
            .user_agent(crate::prices::user_agent())
            .build()
            .and_then(|client| client.get(res_path).send())
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| e.to_string())?;
        response.bytes().map_err(|e| e.to_string())?.to_vec()
    } else {
        let cdn = cdn
            .get_or_init(|| Cdn::connect(cache))
            .as_ref()
            .map_err(Clone::clone)?;
        cdn.fetch(res_path)?
    };
    std::fs::create_dir_all(dest.parent().unwrap_or(dest)).map_err(|e| e.to_string())?;
    std::fs::write(dest, bytes).map_err(|e| e.to_string())
}
