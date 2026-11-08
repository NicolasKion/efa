//! What the UI needs to know about items: names, the market tree, where things fit and which
//! charges go where. refine works on type IDs only, so all of this comes from the dump directly.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use serde::{Deserialize, Serialize};

mod build;
pub(crate) mod cache;
pub mod muta;
pub mod order;
pub mod projection;
mod publish;
pub mod renames;
pub(crate) mod sde;
mod types;

pub use types::{AlphaClone, Hardpoint, ItemType, Kind, MarketGroup, Slot};

pub type TypeId = i32;

pub mod category {
    pub const SKILL: i32 = 16;
    pub const SHIP: i32 = 6;
    pub const MODULE: i32 = 7;
    pub const CHARGE: i32 = 8;
    pub const DRONE: i32 = 18;
    pub const IMPLANT: i32 = 20;
    pub const SUBSYSTEM: i32 = 32;
    pub const FIGHTER: i32 = 87;
    pub const CELESTIAL: i32 = 2;
    pub const MATERIAL: i32 = 4;
    pub const COMMODITY: i32 = 17;
    pub const DEPLOYABLE: i32 = 22;
    /// Upwell structures, and the modules, rigs and services they fit.
    pub const STRUCTURE: i32 = 65;
    pub const STRUCTURE_MODULE: i32 = 66;
}

/// Structures pyfa fits: citadels, engineering complexes and refineries (service/market.py
/// `FIT_GROUPS`). Sovereignty and FLEX structures are left out, as pyfa does.
pub const FIT_STRUCTURE_GROUPS: [i32; 3] = [1657, 1404, 1406];

/// The market's Implants & Boosters > Booster group.
const MARKET_BOOSTERS: i32 = 977;
/// A market group of efa's own under it, for usable boosters the market does not sell: AIR,
/// Agency, event and "Limited Synth" doses. Negative so it never meets a real group's ID.
pub const MARKET_SPECIAL_BOOSTERS: i32 = -MARKET_BOOSTERS;
/// Cargo, secure, audit log and freight containers: the only celestials a ship can carry.
pub const CONTAINER_GROUPS: [i32; 4] = [12, 340, 448, 649];

/// Effect categories in dogmaeffects.
mod effect_category {
    pub const ACTIVE: i32 = 1;
    pub const TARGET: i32 = 2;
    pub const OVERLOAD: i32 = 5;
}

#[derive(Default, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(serialize_with = "crate::sorted::map")]
    pub types: HashMap<TypeId, ItemType>,
    /// Lowercase names to types; see [`Catalog::find`].
    #[serde(serialize_with = "crate::sorted::map")]
    by_name: HashMap<String, TypeId>,
    pub market_groups: BTreeMap<i32, MarketGroup>,
    /// Top-level market groups that contain something fittable, in name order.
    pub market_roots: Vec<i32>,
    pub skills: Vec<TypeId>,
    /// Icon ID to `res:/` path, for attribute and market group icons.
    #[serde(serialize_with = "crate::sorted::map")]
    pub icon_files: HashMap<i32, String>,
    /// Every published type sharing a variation parent, keyed by that parent.
    #[serde(serialize_with = "crate::sorted::map")]
    pub variations: HashMap<TypeId, Vec<TypeId>>,
    #[serde(serialize_with = "crate::sorted::map")]
    pub meta_group_names: HashMap<i32, String>,
    #[serde(serialize_with = "crate::sorted::map")]
    pub group_names: HashMap<i32, String>,
    /// What the market search looks through, in market order: published types on the market
    /// that can be fitted or used, and the cargo pyfa's search finds.
    searchable: Vec<TypeId>,
    /// Published charges on the market by group, in market order.
    #[serde(serialize_with = "crate::sorted::map")]
    charges_by_group: HashMap<i32, Vec<TypeId>>,
    /// A tactical destroyer's modes, by ship.
    #[serde(serialize_with = "crate::sorted::map")]
    modes: HashMap<TypeId, Vec<TypeId>>,
    /// Skills each type needs and at what level, in the order the type lists them. Skills have
    /// their prerequisites here too.
    #[serde(serialize_with = "crate::sorted::map")]
    pub required_skills: HashMap<TypeId, Vec<(TypeId, u8)>>,
    /// The other way round: the types needing each skill, with the level they need, by type.
    #[serde(serialize_with = "crate::sorted::map")]
    required_for: HashMap<TypeId, Vec<(TypeId, u8)>>,
    /// Environmental effects, projectable items and fighter data.
    pub projection: projection::Projection,
    /// Published ships by group, groups by name and ships by race and name, the fittings
    /// browser's first two stages.
    pub ship_groups: Vec<(i32, Vec<TypeId>)>,
    /// What each mutaplasmid takes, gives and rolls.
    pub mutaplasmids: muta::Mutaplasmids,
    /// Published types that are the same item under another name, pyfa's item replacements
    /// (`db_update.py` `processReplacements`): same group, skills, effects and attributes.
    #[serde(serialize_with = "crate::sorted::map")]
    pub(crate) replacements: HashMap<TypeId, Vec<TypeId>>,
    /// The Alpha clone grades, by name.
    pub alpha_clones: Vec<AlphaClone>,
}

/// Bump whenever what the catalog holds or how it is built changes, so old caches are rebuilt
/// instead of loaded.
const CACHE_FORMAT: u32 = 13;
const CACHE_FILE: &str = "catalog.bin";

impl Catalog {
    /// The catalog from the cache when it was built from this exact data, otherwise built from
    /// the JSON and cached. Building means parsing types.json, which takes seconds; the cache
    /// loads in a fraction of that.
    pub fn load_cached(dir: &Path, cache_dir: &Path) -> Result<Self, String> {
        let path = cache_dir.join(CACHE_FILE);
        let key = cache_key(dir);
        if let Some(key) = &key
            && let Some(catalog) = cache::read(&path, key)
        {
            return Ok(catalog);
        }
        let catalog = Self::load(dir)?;
        if let Some(key) = key {
            cache::write(&path, &key, &catalog);
        }
        Ok(catalog)
    }

    /// The catalog as the data pack stores it, with the format it was written in.
    pub fn to_bytes(&self) -> Result<(u32, Vec<u8>), String> {
        postcard::to_stdvec(self)
            .map(|bytes| (CACHE_FORMAT, bytes))
            .map_err(|e| format!("writing the catalog: {e}"))
    }

    /// A catalog [`Catalog::to_bytes`] wrote.
    pub fn from_bytes(format: u32, bytes: &[u8]) -> Result<Self, String> {
        if format != CACHE_FORMAT {
            return Err(format!(
                "the catalog is in format {format}, this build reads {CACHE_FORMAT}"
            ));
        }
        postcard::from_bytes(bytes).map_err(|e| format!("reading the catalog: {e}"))
    }

    pub fn get(&self, id: TypeId) -> Option<&ItemType> {
        self.types.get(&id)
    }

    /// The type's variations, itself included, by meta level. Empty when it has none.
    pub fn variations_of(&self, id: TypeId) -> &[TypeId] {
        let Some(t) = self.get(id) else { return &[] };
        let parent = t.variation_parent.unwrap_or(id);
        self.variations.get(&parent).map_or(&[], Vec::as_slice)
    }

    /// Published types that are the same item as `id`, pyfa's replacements. Empty when none.
    pub fn replacements_of(&self, id: TypeId) -> &[TypeId] {
        self.replacements.get(&id).map_or(&[], Vec::as_slice)
    }

    /// A tactical destroyer's modes, by type ID. Empty for every other ship.
    pub fn modes_for(&self, ship: TypeId) -> &[TypeId] {
        self.modes.get(&ship).map_or(&[], Vec::as_slice)
    }

    /// Published charges of a group that are on the market, in market order.
    pub fn charges_in_group(&self, group: i32) -> &[TypeId] {
        self.charges_by_group.get(&group).map_or(&[], Vec::as_slice)
    }

    /// What the market search looks through, in market order.
    pub fn searchable(&self) -> &[TypeId] {
        &self.searchable
    }

    /// The market group chain from the root down to the type's own group.
    pub fn market_path(&self, id: TypeId) -> Vec<i32> {
        let mut path = Vec::new();
        let mut group = self.get(id).and_then(|t| t.market_group);
        while let Some(g) = group.and_then(|g| self.market_groups.get(&g)) {
            path.push(g.id);
            group = g.parent;
        }
        path.reverse();
        path
    }

    pub fn name(&self, id: TypeId) -> String {
        self.get(id)
            .map_or_else(|| format!("#{id}"), |t| t.name.clone())
    }

    /// The type by name. Names pyfa knows as renamed are looked up under their current name
    /// first, as pyfa's `getItem` does, so old fits still import.
    pub fn find(&self, name: &str) -> Option<&ItemType> {
        let key = name.trim().to_lowercase();
        renames::current_name(&key)
            .and_then(|current| self.by_name.get(current))
            .or_else(|| self.by_name.get(&key))
            .and_then(|id| self.types.get(id))
    }

    /// What needs `skill`, with the level each needs, by type ID.
    pub fn required_for(&self, skill: TypeId) -> &[(TypeId, u8)] {
        self.required_for.get(&skill).map_or(&[], Vec::as_slice)
    }

    /// An Alpha clone grade by ID.
    pub fn alpha_clone(&self, id: i32) -> Option<&AlphaClone> {
        self.alpha_clones.iter().find(|c| c.id == id)
    }

    /// A ship group's published ships, by race and name.
    pub fn ships_in(&self, group: i32) -> &[TypeId] {
        self.ship_groups
            .iter()
            .find(|(g, _)| *g == group)
            .map_or(&[], |(_, ships)| ships.as_slice())
    }
}

/// What a cache must have been built from: the cache format, efa's version, the dump's release
/// and types.json's size and time, so hand-replaced data is noticed too. None if there is no
/// data to describe.
pub fn cache_key(dir: &Path) -> Option<String> {
    let release = std::fs::read_to_string(dir.join("VERSION"))
        .or_else(|_| std::fs::read_to_string(dir.join("phobos/metadata.json")))
        .ok()?;
    let types = std::fs::metadata(dir.join("sde/types.json")).ok()?;
    let modified = types
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!(
        "{CACHE_FORMAT}:{}:{}:{}:{modified}",
        env!("CARGO_PKG_VERSION"),
        release.trim(),
        types.len()
    ))
}
