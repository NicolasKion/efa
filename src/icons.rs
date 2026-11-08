//! Icons for items and attributes, as files on disk.
//!
//! A deimos dump carries the attribute and market group icons. Everything else, and those too
//! when running on the data pack, is fetched from CCP's CDN the first time it is asked for and
//! kept in the cache folder, so the cost is paid once per icon. What could not be fetched is
//! asked for again a while later, so a start without network does not lose icons for good.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

use crate::{cdn::Cdn, data::Catalog};

/// Downloads running at once. Opening a market group asks for dozens of icons together.
const WORKERS: usize = 4;
/// How long a failed connection to the CDN, or a failed icon, waits before it is tried again.
const RETRY: Duration = Duration::from_secs(120);

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
    /// The download failed at this time.
    Failed(Instant),
    /// Nothing to fetch it from: the data names no art for it, or there is no client.
    Missing,
}

type States = Arc<Mutex<HashMap<Icon, State>>>;

/// The CDN connection the workers share, made once something is actually missing.
enum Connection {
    Untried,
    Up(Arc<Cdn>),
    /// Connecting failed at this time.
    Down(Instant, String),
}

pub struct Icons {
    /// The dump's attribute and market group icons; the data pack has none.
    dump_dir: Option<PathBuf>,
    cache_dir: PathBuf,
    states: States,
    /// None without a client to download with.
    fetch: Option<Sender<(Icon, String, PathBuf)>>,
    /// Set when a download lands, so the UI knows to redraw.
    updated: Arc<AtomicBool>,
}

impl Icons {
    /// Icons from the dump and the cache folder, and what is not there yet from CCP through
    /// `http`. Without a client only what is on disk shows.
    pub fn new(
        dump_dir: Option<&Path>,
        cache_dir: &Path,
        http: Option<reqwest::blocking::Client>,
    ) -> Self {
        let states: States = Arc::default();
        let updated = Arc::new(AtomicBool::new(false));
        let fetch = http.map(|http| {
            let (tx, rx) = mpsc::channel::<(Icon, String, PathBuf)>();
            let rx = Arc::new(Mutex::new(rx));
            // Connecting costs a few seconds the first time, so it only happens once something
            // is actually missing, and the workers share the one connection.
            let cdn = Arc::new(Mutex::new(Connection::Untried));
            for _ in 0..WORKERS {
                let (rx, states, cdn, updated, http) = (
                    Arc::clone(&rx),
                    Arc::clone(&states),
                    Arc::clone(&cdn),
                    Arc::clone(&updated),
                    http.clone(),
                );
                let cdn_cache = cache_dir.to_owned();
                std::thread::spawn(move || {
                    loop {
                        let job = rx.lock().expect("icon queue lock poisoned").recv();
                        let Ok((icon, res_path, dest)) = job else {
                            return;
                        };
                        let state = match fetch(&http, &cdn, &cdn_cache, &res_path, &dest) {
                            Ok(()) => State::Ready(dest),
                            Err(_) => State::Failed(Instant::now()),
                        };
                        states
                            .lock()
                            .expect("icon state lock poisoned")
                            .insert(icon, state);
                        updated.store(true, Ordering::Release);
                    }
                });
            }
            tx
        });
        Self {
            dump_dir: dump_dir.map(|d| d.join("icons")),
            cache_dir: cache_dir.join("icons"),
            states,
            fetch,
            updated,
        }
    }

    /// Path of the icon if it is on disk; otherwise starts fetching it and returns None for now.
    pub fn path(&self, catalog: &Catalog, icon: Icon) -> Option<PathBuf> {
        let mut states = self.states.lock().expect("icon state lock poisoned");
        match states.get(&icon) {
            Some(State::Ready(path)) => return Some(path.clone()),
            Some(State::Fetching | State::Missing) => return None,
            Some(State::Failed(when)) if when.elapsed() < RETRY => return None,
            Some(State::Failed(_)) | None => {}
        }
        let (dumped, cached, res_path) = match icon {
            Icon::Id(id) => (
                self.dump_dir.as_ref().map(|d| d.join(format!("{id}.png"))),
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
        let sent = res_path
            .zip(self.fetch.as_ref())
            .is_some_and(|(res_path, fetch)| {
                let dest = std::path::absolute(&cached).unwrap_or(cached);
                fetch.send((icon, res_path, dest)).is_ok()
            });
        states.insert(
            icon,
            if sent {
                State::Fetching
            } else {
                State::Missing
            },
        );
        None
    }

    /// Whether any icon arrived since the last call.
    pub fn take_updated(&self) -> bool {
        self.updated.swap(false, Ordering::AcqRel)
    }
}

fn fetch(
    http: &reqwest::blocking::Client,
    cdn: &Mutex<Connection>,
    cache: &Path,
    res_path: &str,
    dest: &Path,
) -> Result<(), String> {
    let bytes = if res_path.starts_with("https://") {
        // The image server, not the game's resource CDN.
        crate::cdn::get_bytes(http, res_path)?
    } else {
        connect(http, cdn, cache)?.fetch(res_path)?
    };
    // Written whole or not at all: a file that is there counts as the icon from then on.
    crate::persist::write_atomic(dest, &bytes).map_err(|e| e.to_string())
}

/// The shared CDN connection, made now if there is none and the last try is long enough ago.
/// Other workers wait while one connects rather than all connecting at once.
fn connect(
    http: &reqwest::blocking::Client,
    cdn: &Mutex<Connection>,
    cache: &Path,
) -> Result<Arc<Cdn>, String> {
    let mut connection = cdn.lock().expect("CDN lock poisoned");
    match &*connection {
        Connection::Up(cdn) => return Ok(Arc::clone(cdn)),
        Connection::Down(when, e) if when.elapsed() < RETRY => return Err(e.clone()),
        Connection::Down(..) | Connection::Untried => {}
    }
    match Cdn::connect(http.clone(), cache) {
        Ok(up) => {
            let up = Arc::new(up);
            *connection = Connection::Up(Arc::clone(&up));
            Ok(up)
        }
        Err(e) => {
            *connection = Connection::Down(Instant::now(), e.clone());
            Err(e)
        }
    }
}
