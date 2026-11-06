//! Icons for items and attributes, as `file://` URIs egui's image loaders can open.
//!
//! The deimos dump carries the attribute and market group icons. Item icons are fetched from CCP's
//! CDN the first time they are shown and kept on disk, so the cost is paid once per icon.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        mpsc::{self, Sender},
    },
};

use crate::{cdn::Cdn, data::Catalog};

const WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// An icon ID, as attributes and market groups carry.
    Id(i32),
    /// An item's own icon.
    Type(i32),
}

enum State {
    Ready(String),
    Fetching,
    Missing,
}

pub struct Icons {
    dump_dir: PathBuf,
    cache_dir: PathBuf,
    states: Arc<Mutex<HashMap<Icon, State>>>,
    fetch: Sender<(Icon, String, PathBuf)>,
}

impl Icons {
    pub fn new(data_dir: &Path, cache_dir: &Path, ctx: &eframe::egui::Context) -> Self {
        let states: Arc<Mutex<HashMap<Icon, State>>> = Arc::default();
        let (tx, rx) = mpsc::channel::<(Icon, String, PathBuf)>();
        let rx = Arc::new(Mutex::new(rx));
        // Connecting costs a few seconds the first time, so it only happens once something is
        // actually missing, and the workers share the one connection.
        let cdn: Arc<OnceLock<Result<Cdn, String>>> = Arc::default();
        for _ in 0..WORKERS {
            let (rx, states, cdn, ctx) = (
                Arc::clone(&rx),
                Arc::clone(&states),
                Arc::clone(&cdn),
                ctx.clone(),
            );
            let cdn_cache = cache_dir.to_owned();
            std::thread::spawn(move || {
                loop {
                    let Ok((icon, res_path, dest)) =
                        rx.lock().expect("icon queue lock poisoned").recv()
                    else {
                        return;
                    };
                    let result = match cdn.get_or_init(|| Cdn::connect(&cdn_cache)) {
                        Ok(cdn) => cdn.fetch(&res_path).and_then(|bytes| {
                            std::fs::create_dir_all(dest.parent().unwrap_or(&dest))
                                .map_err(|e| e.to_string())?;
                            std::fs::write(&dest, bytes).map_err(|e| e.to_string())
                        }),
                        Err(e) => Err(e.clone()),
                    };
                    let state = match result {
                        Ok(()) => State::Ready(uri(&dest)),
                        Err(_) => State::Missing,
                    };
                    states
                        .lock()
                        .expect("icon state lock poisoned")
                        .insert(icon, state);
                    ctx.request_repaint();
                }
            });
        }
        Self {
            dump_dir: data_dir.join("icons"),
            cache_dir: cache_dir.join("icons"),
            states,
            fetch: tx,
        }
    }

    /// URI of the icon if it is on disk; otherwise starts fetching it and returns None for now.
    pub fn get(&self, catalog: &Catalog, icon: Icon) -> Option<String> {
        let mut states = self.states.lock().expect("icon state lock poisoned");
        match states.get(&icon) {
            Some(State::Ready(uri)) => return Some(uri.clone()),
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
        };
        if let Some(path) = dumped
            .filter(|p| p.exists())
            .or_else(|| cached.exists().then(|| cached.clone()))
        {
            let uri = uri(&path);
            states.insert(icon, State::Ready(uri.clone()));
            return Some(uri);
        }
        match res_path {
            Some(res_path) => {
                states.insert(icon, State::Fetching);
                let _ = self.fetch.send((icon, res_path, cached));
            }
            None => {
                states.insert(icon, State::Missing);
            }
        }
        None
    }
}

fn uri(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    format!("file://{}", absolute.display())
}
