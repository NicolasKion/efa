//! The real data every test reads: the catalog and engine are loaded once, from the cache, and
//! shared. Tests skip when no dump is present.

use std::{path::Path, sync::OnceLock};

use efa::{data::Catalog, engine::Engine, item_data::ItemData};

pub const DATA: &str = "data/eve";
pub const CACHE: &str = "data/cache";

/// Whether a dump is there to test against; says so when it is not.
pub fn has_data() -> bool {
    let present = Path::new(DATA).join("phobos/metadata.json").exists();
    if !present {
        eprintln!("no EVE data in {DATA}, skipping");
    }
    present
}

pub fn catalog() -> Option<&'static Catalog> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            has_data().then(|| {
                Catalog::load_cached(Path::new(DATA), Path::new(CACHE)).expect("the dump loads")
            })
        })
        .as_ref()
}

/// The shared catalog and an engine on it. Tests that change the engine, such as its attribute
/// overrides, take an [`own_engine`] instead.
pub fn setup() -> Option<(&'static Catalog, &'static Engine)> {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    let catalog = catalog()?;
    Some((catalog, ENGINE.get_or_init(|| own_engine(catalog))))
}

/// An engine of the test's own.
pub fn own_engine(catalog: &Catalog) -> Engine {
    Engine::load(Path::new(DATA), Path::new(CACHE), catalog.skills.clone())
        .expect("the engine loads")
}

/// The item stats window's data for every type of the catalog.
pub fn item_data(catalog: &Catalog) -> &'static ItemData {
    static DATA_: OnceLock<ItemData> = OnceLock::new();
    DATA_.get_or_init(|| {
        let wanted = catalog.types.keys().copied().collect();
        let key = efa::data::cache_key(Path::new(DATA));
        ItemData::load_cached(Path::new(DATA), Path::new(CACHE), key.as_deref(), &wanted)
            .expect("the item data loads")
    })
}
