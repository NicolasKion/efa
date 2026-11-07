//! What the UI needs to know about items: names, the market tree, where things fit and which
//! charges go where. refine works on type IDs only, so all of this comes from the dump directly.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    market::jargon::{Jargon, Query},
    ship_browser::Race,
};

pub mod muta;
pub mod projection;

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

const GROUP_BOOSTER: i32 = 303;
/// The market's Implants & Boosters > Booster group.
const MARKET_BOOSTERS: i32 = 977;
/// A market group of gyfa's own under it, for usable boosters the market does not sell: AIR,
/// Agency, event and "Limited Synth" doses. Negative so it never meets a real group's ID.
pub const MARKET_SPECIAL_BOOSTERS: i32 = -MARKET_BOOSTERS;
/// Tactical destroyer modes.
const GROUP_SHIP_MODIFIERS: i32 = 1306;
const GROUP_TACTICAL_DESTROYER: i32 = 1305;
/// Cargo, secure, audit log and freight containers: the only celestials a ship can carry.
pub const CONTAINER_GROUPS: [i32; 4] = [12, 340, 448, 649];
/// Strontium, heavy water and the like, which pyfa's market search finds outside its tree.
const GROUP_ICE_PRODUCT: i32 = 423;
/// Market groups pyfa shows as roots for things that only go in cargo: Deployable Structures and
/// Filaments. In the SDE they hang under Structures and Trade Goods, which pyfa leaves out.
const CARGO_MARKET_ROOTS: [i32; 2] = [404, 2456];

mod effect {
    pub const LAUNCHER_FITTED: i32 = 40;
    pub const TURRET_FITTED: i32 = 42;
    pub const LOW: i32 = 11;
    pub const HIGH: i32 = 12;
    pub const MID: i32 = 13;
    pub const RIG: i32 = 2663;
    pub const SUBSYSTEM: i32 = 3772;
    pub const SERVICE: i32 = 6306;
    /// Every fittable module has this, and the data files it under active effects.
    pub const ONLINE: i32 = 16;
}

mod attr {
    pub const CHARGE_SIZE: i32 = 128;
    pub const CHARGE_GROUPS: [i32; 5] = [604, 605, 606, 609, 610];
    /// EM, thermal, kinetic, explosive damage on a charge.
    pub const DAMAGE: [i32; 4] = [114, 118, 117, 116];
    pub const RANGE_MULTIPLIER: i32 = 120;
    pub const FALLOFF_MULTIPLIER: i32 = 517;
    pub const OPTIMAL: i32 = 54;
    pub const FALLOFF: i32 = 158;
    pub const MINING_AMOUNT: i32 = 77;
    pub const DRONE_BANDWIDTH_USED: i32 = 1272;
    pub const SIGNATURE_RADIUS: i32 = 552;
    /// Maximum spool bonus, on Precursor weapons and mutadaptive repairers.
    pub const SPOOL_MAX: [i32; 2] = [2734, 2797];
    /// The booster slot, which pyfa calls boosterness.
    pub const BOOSTERNESS: i32 = 1087;
    /// Chance of a booster's first side effect, which pyfa ranks booster variations by.
    pub const BOOSTER_EFFECT_CHANCE1: i32 = 1089;
    pub const IMPLANTNESS: i32 = 331;
    /// Rig size on a rig and on the ships that take it.
    pub const RIG_SIZE: i32 = 1547;
    /// canFitShipGroup01 to 20: the ship groups a module is limited to.
    pub const CAN_FIT_SHIP_GROUP: [i32; 20] = [
        1298, 1299, 1300, 1301, 1872, 1879, 1880, 1881, 2065, 2396, 2476, 2477, 2478, 2479, 2480,
        2481, 2482, 2483, 2484, 2485,
    ];
    /// canFitShipType1 to 12 and a subsystem's fitsToShipType: the ships an item is limited to.
    pub const CAN_FIT_SHIP_TYPE: [i32; 13] = [
        1302, 1303, 1304, 1305, 1944, 2103, 2463, 2486, 2487, 2488, 2758, 5948, 1380,
    ];
    /// requiredSkill1..6 and the level attribute that goes with each.
    pub const REQUIRED_SKILLS: [(i32, i32); 6] = [
        (182, 277),
        (183, 278),
        (184, 279),
        (1285, 1286),
        (1289, 1287),
        (1290, 1288),
    ];
}

/// Which kind of hardpoint a module takes, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hardpoint {
    None,
    Turret,
    Launcher,
}

/// Effect categories in dogmaeffects.
mod effect_category {
    pub const ACTIVE: i32 = 1;
    pub const TARGET: i32 = 2;
    pub const OVERLOAD: i32 = 5;
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Slot {
    High,
    Mid,
    Low,
    Rig,
    Subsystem,
    /// An Upwell structure's service modules.
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Ship,
    Module(Slot),
    Charge,
    Drone,
    Fighter,
    Implant,
    Booster,
    Skill,
    /// A tactical destroyer's mode.
    Mode,
    /// Something a ship only carries: commodities, materials, deployables, containers.
    Cargo,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts read from the data, not a state machine"
)]
pub struct ItemType {
    pub id: TypeId,
    pub name: String,
    pub group_id: i32,
    pub category_id: i32,
    pub market_group: Option<i32>,
    pub published: bool,
    pub kind: Kind,
    pub activatable: bool,
    pub overloadable: bool,
    pub charge_groups: Vec<i32>,
    pub charge_size: Option<f64>,
    /// Client path of the icon art, a `res:/` path on CCP's CDN.
    pub icon: Option<String>,
    /// The type this one is a variation of (the T1 base), if any.
    pub variation_parent: Option<TypeId>,
    pub meta_group: Option<i32>,
    pub meta_level: i32,
    /// Packaged volume where there is one, in m³.
    pub volume: f64,
    pub hardpoint: Hardpoint,
    /// EM, thermal, kinetic, explosive damage, for charges.
    pub damage: [f64; 4],
    pub range_multiplier: Option<f64>,
    pub falloff_multiplier: Option<f64>,
    /// Unmodified optimal and falloff, for sorting a turret's charges by range.
    pub optimal: Option<f64>,
    pub falloff: Option<f64>,
    pub mining: bool,
    /// Whether this module spools up over cycles.
    pub spools: bool,
    /// Bandwidth a drone takes while in space, in Mbit/s.
    pub drone_bandwidth: f64,
    /// Unmodified signature radius, which a bomb's explosion is sized by.
    pub signature_radius: Option<f64>,
    /// Slot a booster goes into; a fit holds one booster per slot.
    pub booster_slot: Option<u32>,
    /// Chance of a booster's first side effect, missing on boosters without one.
    pub booster_chance: Option<f64>,
    /// Effects that may come with a booster, by effect ID: the ones with a usage chance.
    pub side_effects: Vec<i32>,
    /// Which implant slot an implant plugs into; a character takes one implant per slot.
    pub implant_slot: Option<u8>,
    /// What the item holds, in m³: charges for a module, contents for a container.
    pub capacity: f64,
    /// Who builds a ship, for the fittings browser's race filter.
    pub race: Option<Race>,
    /// Needs a skill level beyond what an Alpha clone can train.
    pub omega: bool,
    /// Rig size of a rig, or of the rigs a ship takes.
    pub rig_size: Option<f64>,
    /// Ships and ship groups the item is limited to; empty lists mean no limit.
    pub fits_ships: Vec<TypeId>,
    pub fits_groups: Vec<i32>,
}

impl ItemType {
    /// Whether this is an Upwell structure rather than a ship.
    pub fn is_structure(&self) -> bool {
        self.category_id == category::STRUCTURE
    }

    /// The word pyfa puts into menu labels for this kind of item.
    pub fn category_label(&self) -> &'static str {
        match self.kind {
            Kind::Ship if self.is_structure() => "Citadel",
            Kind::Ship => "Ship",
            Kind::Module(Slot::Subsystem) => "Subsystem",
            Kind::Module(Slot::Service) => "Service",
            Kind::Module(_) => "Module",
            Kind::Charge => "Charge",
            Kind::Drone => "Drone",
            Kind::Fighter => "Fighter",
            Kind::Implant => "Implant",
            Kind::Booster => "Booster",
            Kind::Skill => "Skill",
            Kind::Mode => "Tactical Mode",
            Kind::Cargo => match self.category_id {
                category::COMMODITY => "Commodity",
                category::MATERIAL => "Material",
                category::DEPLOYABLE => "Deployable",
                _ => "Celestial",
            },
            Kind::Other => "Item",
        }
    }

    /// Whether this charge fits in a module.
    pub fn fits_in(&self, module: &ItemType) -> bool {
        self.kind == Kind::Charge
            && module.charge_groups.contains(&self.group_id)
            && match module.charge_size {
                Some(size) => self.charge_size == Some(size),
                None => true,
            }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketGroup {
    pub id: i32,
    pub name: String,
    pub parent: Option<i32>,
    pub children: Vec<i32>,
    pub types: Vec<TypeId>,
    pub icon_id: Option<i32>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Catalog {
    pub types: HashMap<TypeId, ItemType>,
    pub by_name: HashMap<String, TypeId>,
    pub market_groups: BTreeMap<i32, MarketGroup>,
    /// Top-level market groups that contain something fittable, in name order.
    pub market_roots: Vec<i32>,
    pub skills: Vec<TypeId>,
    /// Icon ID to `res:/` path, for attribute and market group icons.
    pub icon_files: HashMap<i32, String>,
    /// Every published type sharing a variation parent, keyed by that parent.
    pub variations: HashMap<TypeId, Vec<TypeId>>,
    pub meta_group_names: HashMap<i32, String>,
    pub group_names: HashMap<i32, String>,
    /// Cargo-only types the market search finds, the way pyfa limits its search.
    pub cargo_searchable: HashSet<TypeId>,
    /// Skills each type needs and at what level, in the order the type lists them. Skills have
    /// their prerequisites here too.
    pub required_skills: HashMap<TypeId, Vec<(TypeId, u8)>>,
    /// Environmental effects, projectable items and fighter data.
    pub projection: projection::Projection,
    /// Published ships by group, groups by name and ships by race and name, the fittings
    /// browser's first two stages.
    pub ship_groups: Vec<(i32, Vec<TypeId>)>,
    /// What each mutaplasmid takes, gives and rolls.
    pub mutaplasmids: muta::Mutaplasmids,
    /// Published types that are the same item under another name, pyfa's item replacements
    /// (`db_update.py` `processReplacements`): same group, skills, effects and attributes.
    pub replacements: HashMap<TypeId, Vec<TypeId>>,
    /// The Alpha clone grades, by name.
    pub alpha_clones: Vec<AlphaClone>,
}

/// An Alpha clone grade: the skills it can use and the highest level of each.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlphaClone {
    pub id: i32,
    pub name: String,
    pub caps: HashMap<TypeId, u8>,
}

/// Bump whenever what the catalog holds or how it is built changes, so old caches are rebuilt
/// instead of loaded.
const CACHE_FORMAT: u32 = 9;
const CACHE_FILE: &str = "catalog.bin";

/// The cache on disk: the key it was built for, then the catalog.
#[derive(Deserialize)]
struct CacheFile {
    key: String,
    catalog: Catalog,
}

/// [`CacheFile`] for writing, without copying the catalog. Same fields in the same order, which
/// is all postcard's layout depends on.
#[derive(Serialize)]
struct CacheFileRef<'a> {
    key: &'a str,
    catalog: &'a Catalog,
}

impl Catalog {
    /// The catalog from the cache when it was built from this exact data, otherwise built from
    /// the JSON and cached. Building means parsing types.json, which takes seconds; the cache
    /// loads in a fraction of that.
    pub fn load_cached(dir: &Path, cache_dir: &Path) -> Result<Self, String> {
        let path = cache_dir.join(CACHE_FILE);
        let key = cache_key(dir);
        if let Some(key) = &key
            && let Some(catalog) = read_cache(&path, key)
        {
            return Ok(catalog);
        }
        let catalog = Self::load(dir)?;
        if let Some(key) = key {
            write_cache(&path, &key, &catalog);
        }
        Ok(catalog)
    }

    pub fn load(dir: &Path) -> Result<Self, String> {
        let groups: HashMap<String, SdeGroup> = read(dir, "sde/groups.json")?;
        let market: HashMap<String, SdeMarketGroup> = read(dir, "sde/marketgroups.json")?;
        let sde_types: HashMap<String, SdeType> = read(dir, "sde/types.json")?;
        let effects: HashMap<String, PhbEffect> = read(dir, "fsd_built/dogmaeffects.json")?;
        let dogma: HashMap<String, PhbTypeDogma> = read(dir, "fsd_built/typedogma.json")?;
        let icons: HashMap<String, SdeIcon> = read(dir, "sde/icons.json")?;
        let meta_groups: HashMap<String, SdeMetaGroup> = read(dir, "sde/metagroups.json")?;
        let graphics: HashMap<String, SdeGraphic> = read(dir, "sde/graphics.json")?;
        let clone_grades: HashMap<String, SdeCloneGrade> = read(dir, "sde/clonegrades.json")?;
        let muta = muta::Source::read(dir)?;
        let stand_ins = muta.stand_ins();
        let icon_files: HashMap<i32, String> = icons
            .into_iter()
            .filter_map(|(id, icon)| Some((id.parse().ok()?, icon.icon_file?)))
            .collect();

        let chance_effects: HashSet<i32> = effects
            .iter()
            .filter(|(_, e)| e.fitting_usage_chance_attribute_id.is_some())
            .filter_map(|(id, _)| id.parse().ok())
            .collect();
        let effect_category: HashMap<i32, i32> = effects
            .into_iter()
            .filter_map(|(id, e)| Some((id.parse().ok()?, e.effect_category)))
            .collect();
        let group_category: HashMap<i32, i32> = groups
            .iter()
            .filter_map(|(id, g)| Some((id.parse().ok()?, g.category_id)))
            .collect();

        // Skills carry the race a ship's race is read from.
        let type_races: HashMap<TypeId, i32> = sde_types
            .iter()
            .filter_map(|(id, t)| Some((id.parse().ok()?, t.race_id.filter(|&r| r != 0)?)))
            .collect();

        let mut projection = projection::Builder::new(&effect_category);
        let mut same_items = SameItems::default();
        let mut types = HashMap::with_capacity(sde_types.len());
        let mut skills = Vec::new();
        let mut required_skills = HashMap::new();
        for (id, t) in sde_types {
            let Ok(id) = id.parse::<TypeId>() else {
                continue;
            };
            let category_id = group_category.get(&t.group_id).copied().unwrap_or(0);
            projection.consider(id, &t, category_id, dogma.get(&id.to_string()));
            // Cargo can hold anything, but these are what pyfa's market offers or EFT cargo lines
            // usually name (fuel blocks, ice products); loading every category would mostly add
            // NPC and map junk.
            let cargo_only = matches!(
                category_id,
                category::COMMODITY | category::MATERIAL | category::DEPLOYABLE
            ) || CONTAINER_GROUPS.contains(&t.group_id);
            let fit_structure =
                category_id == category::STRUCTURE && FIT_STRUCTURE_GROUPS.contains(&t.group_id);
            if !cargo_only
                && !fit_structure
                && !matches!(
                    category_id,
                    category::SHIP
                        | category::MODULE
                        | category::STRUCTURE_MODULE
                        | category::CHARGE
                        | category::DRONE
                        | category::IMPLANT
                        | category::SUBSYSTEM
                        | category::FIGHTER
                        | category::SKILL
                )
            {
                continue;
            }

            // Abyssal types read the attributes they lack from a type they are made from.
            let stand_in = stand_ins.get(&id).and_then(|b| dogma.get(&b.to_string()));
            let dogma = dogma.get(&id.to_string());
            let has_effect = |wanted: i32| {
                dogma.is_some_and(|d| d.effects.iter().any(|e| e.effect_id == wanted))
            };
            let has_effect_category = |wanted: &[i32]| {
                dogma.is_some_and(|d| {
                    d.effects
                        .iter()
                        .filter(|e| e.effect_id != effect::ONLINE)
                        .any(|e| {
                            effect_category
                                .get(&e.effect_id)
                                .is_some_and(|c| wanted.contains(c))
                        })
                })
            };
            let attr = |wanted: i32| {
                [dogma, stand_in].into_iter().flatten().find_map(|d| {
                    d.attributes
                        .iter()
                        .find(|a| a.attribute_id == wanted)
                        .map(|a| a.value)
                })
            };

            let kind = match category_id {
                category::SHIP | category::STRUCTURE => Kind::Ship,
                category::CHARGE => Kind::Charge,
                category::DRONE => Kind::Drone,
                category::FIGHTER => Kind::Fighter,
                category::SKILL => Kind::Skill,
                category::IMPLANT if t.group_id == GROUP_BOOSTER => Kind::Booster,
                category::MODULE if t.group_id == GROUP_SHIP_MODIFIERS => Kind::Mode,
                category::IMPLANT => Kind::Implant,
                category::SUBSYSTEM => Kind::Module(Slot::Subsystem),
                category::MODULE | category::STRUCTURE_MODULE if has_effect(effect::HIGH) => {
                    Kind::Module(Slot::High)
                }
                category::MODULE | category::STRUCTURE_MODULE if has_effect(effect::MID) => {
                    Kind::Module(Slot::Mid)
                }
                category::MODULE | category::STRUCTURE_MODULE if has_effect(effect::LOW) => {
                    Kind::Module(Slot::Low)
                }
                category::MODULE | category::STRUCTURE_MODULE if has_effect(effect::RIG) => {
                    Kind::Module(Slot::Rig)
                }
                category::MODULE if has_effect(effect::SUBSYSTEM) => Kind::Module(Slot::Subsystem),
                category::STRUCTURE_MODULE if has_effect(effect::SERVICE) => {
                    Kind::Module(Slot::Service)
                }
                _ if cargo_only => Kind::Cargo,
                _ => Kind::Other,
            };
            if kind == Kind::Skill && t.published {
                skills.push(id);
            }
            let required: Vec<(TypeId, u8)> = attr::REQUIRED_SKILLS
                .iter()
                .filter_map(|&(skill, level)| {
                    let skill = attr(skill).filter(|&s| s > 0.0)? as TypeId;
                    Some((skill, attr(level).unwrap_or(0.0) as u8))
                })
                .collect();
            let race = (kind == Kind::Ship).then(|| {
                let skill_races: Vec<i32> = required
                    .iter()
                    .filter_map(|(skill, _)| type_races.get(skill).copied())
                    .collect();
                Race::resolve(t.faction_id, &skill_races, t.race_id)
            });
            if t.published && stand_in.is_none() {
                same_items.consider(id, &t, category_id, dogma, &required);
            }
            if !required.is_empty() {
                required_skills.insert(id, required);
            }

            let limits = |attrs: &[i32]| -> Vec<i32> {
                attrs
                    .iter()
                    .filter_map(|&a| attr(a))
                    .filter(|&v| v > 0.0)
                    .map(|v| v as i32)
                    .collect()
            };
            let charge_groups = attr::CHARGE_GROUPS
                .iter()
                .filter_map(|&a| attr(a))
                .filter(|&g| g > 0.0)
                .map(|g| g as i32)
                .collect();

            // Ships, drones and fighters are drawn from their model's own icon folder, the way the
            // client does it; everything else names an icon directly.
            let render = || {
                let graphic = t.graphic_id?;
                let folder = graphics.get(&graphic.to_string())?.icon_folder.as_ref()?;
                Some(format!("{folder}/{graphic}_64.png"))
            };
            let icon = match kind {
                Kind::Ship | Kind::Drone | Kind::Fighter => render(),
                _ => None,
            }
            .or_else(|| t.icon_id.and_then(|i| icon_files.get(&i).cloned()));

            types.insert(
                id,
                ItemType {
                    id,
                    name: t.name.en,
                    group_id: t.group_id,
                    category_id,
                    market_group: t.market_group_id,
                    published: t.published,
                    kind,
                    activatable: has_effect_category(&[
                        effect_category::ACTIVE,
                        effect_category::TARGET,
                    ]),
                    overloadable: has_effect_category(&[effect_category::OVERLOAD]),
                    charge_groups,
                    charge_size: attr(attr::CHARGE_SIZE),
                    icon,
                    variation_parent: t.variation_parent_type_id,
                    meta_group: t.meta_group_id,
                    meta_level: t.meta_level.unwrap_or(0),
                    volume: t.packaged_volume.or(t.volume).unwrap_or(0.0),
                    hardpoint: if has_effect(effect::TURRET_FITTED) {
                        Hardpoint::Turret
                    } else if has_effect(effect::LAUNCHER_FITTED) {
                        Hardpoint::Launcher
                    } else {
                        Hardpoint::None
                    },
                    damage: attr::DAMAGE.map(|a| attr(a).unwrap_or(0.0)),
                    range_multiplier: attr(attr::RANGE_MULTIPLIER),
                    falloff_multiplier: attr(attr::FALLOFF_MULTIPLIER),
                    optimal: attr(attr::OPTIMAL),
                    falloff: attr(attr::FALLOFF),
                    mining: attr(attr::MINING_AMOUNT).is_some_and(|a| a > 0.0),
                    spools: attr::SPOOL_MAX
                        .iter()
                        .any(|&a| attr(a).is_some_and(|v| v > 0.0)),
                    drone_bandwidth: attr(attr::DRONE_BANDWIDTH_USED).unwrap_or(0.0),
                    signature_radius: attr(attr::SIGNATURE_RADIUS),
                    booster_slot: attr(attr::BOOSTERNESS)
                        .filter(|_| kind == Kind::Booster)
                        .map(|s| s as u32),
                    booster_chance: attr(attr::BOOSTER_EFFECT_CHANCE1),
                    side_effects: dogma.map_or_default(|d| {
                        d.effects
                            .iter()
                            .map(|e| e.effect_id)
                            .filter(|id| chance_effects.contains(id))
                            .collect()
                    }),
                    implant_slot: attr(attr::IMPLANTNESS).map(|v| v as u8),
                    capacity: t.capacity.unwrap_or(0.0),
                    race: race.flatten(),
                    omega: false,
                    rig_size: attr(attr::RIG_SIZE),
                    fits_ships: limits(&attr::CAN_FIT_SHIP_TYPE),
                    fits_groups: limits(&attr::CAN_FIT_SHIP_GROUP),
                },
            );
        }

        let mut alpha_clones: Vec<AlphaClone> = clone_grades
            .into_iter()
            .filter_map(|(id, grade)| {
                Some(AlphaClone {
                    id: id.parse().ok()?,
                    name: grade.name,
                    caps: grade.skills.iter().map(|s| (s.type_id, s.level)).collect(),
                })
            })
            .collect();
        alpha_clones.sort_by(|a, b| a.name.cmp(&b.name));
        let alpha = alpha_skill_caps(&alpha_clones);
        let mut omega = HashMap::new();
        for t in types.values_mut() {
            t.omega = needs_omega(t.id, &required_skills, &alpha, &mut omega);
        }

        let projection = projection.finish(dir);
        let mutaplasmids = muta.finish(
            dir,
            &types
                .iter()
                .filter(|(_, t)| t.kind == Kind::Cargo)
                .map(|(&id, t)| (id, t.name.clone()))
                .collect(),
            &dogma,
        );

        // Published names win when two types share one, which happens with old unpublished copies.
        let mut by_name = HashMap::with_capacity(types.len());
        for t in types.values() {
            let key = t.name.to_lowercase();
            match by_name.get(&key).and_then(|id| types.get(id)) {
                Some(existing) if existing.published && !t.published => {}
                _ => {
                    by_name.insert(key, t.id);
                }
            }
        }

        let mut market_groups: BTreeMap<i32, MarketGroup> = market
            .into_iter()
            .filter_map(|(id, g)| {
                let id = id.parse().ok()?;
                Some((
                    id,
                    MarketGroup {
                        id,
                        name: g.name.en,
                        parent: g.parent_group_id,
                        children: Vec::new(),
                        types: Vec::new(),
                        icon_id: g.icon_id,
                    },
                ))
            })
            .collect();
        for root in CARGO_MARKET_ROOTS {
            if let Some(group) = market_groups.get_mut(&root) {
                group.parent = None;
            }
        }
        // Boosters that can be plugged in but are not on the market get a group of their own,
        // so the tree, search and the boosters tab can offer them.
        let mut special_boosters = false;
        for t in types.values_mut() {
            let usable = t.kind == Kind::Booster
                && t.published
                && t.market_group.is_none()
                && t.booster_slot.is_some()
                && !t.name.contains("Accelerator")
                && !t.name.starts_with("Expired");
            if usable {
                t.market_group = Some(MARKET_SPECIAL_BOOSTERS);
                special_boosters = true;
            }
        }
        if special_boosters && market_groups.contains_key(&MARKET_BOOSTERS) {
            let icon_id = market_groups[&MARKET_BOOSTERS].icon_id;
            market_groups.insert(
                MARKET_SPECIAL_BOOSTERS,
                MarketGroup {
                    id: MARKET_SPECIAL_BOOSTERS,
                    name: "Special Boosters".to_owned(),
                    parent: Some(MARKET_BOOSTERS),
                    children: Vec::new(),
                    types: Vec::new(),
                    icon_id,
                },
            );
        }
        let under_cargo_roots: HashSet<TypeId> = types
            .values()
            .filter(|t| t.kind == Kind::Cargo && t.published)
            .filter(|t| {
                let mut group = t.market_group;
                while let Some(g) = group {
                    if CARGO_MARKET_ROOTS.contains(&g) {
                        return true;
                    }
                    group = market_groups.get(&g).and_then(|g| g.parent);
                }
                false
            })
            .map(|t| t.id)
            .collect();
        let cargo_searchable: HashSet<TypeId> = types
            .values()
            .filter(|t| t.published && t.group_id == GROUP_ICE_PRODUCT)
            .map(|t| t.id)
            .chain(under_cargo_roots.iter().copied())
            .collect();
        for t in types.values() {
            if !t.published || matches!(t.kind, Kind::Skill | Kind::Mode | Kind::Other) {
                continue;
            }
            if t.kind == Kind::Cargo && !under_cargo_roots.contains(&t.id) {
                continue;
            }
            if let Some(group) = t.market_group.and_then(|g| market_groups.get_mut(&g)) {
                group.types.push(t.id);
            }
        }
        for group in market_groups.values_mut() {
            crate::market::sort_items(&types, &mut group.types);
        }
        prune_and_link(&mut market_groups);
        let mut market_roots: Vec<i32> = market_groups
            .values()
            .filter(|g| g.parent.is_none())
            .map(|g| g.id)
            .collect();
        market_roots.sort_by(|a, b| market_groups[a].name.cmp(&market_groups[b].name));
        skills.sort_unstable();

        let mut variations: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
        for t in types.values().filter(|t| t.published) {
            if let Some(parent) = t.variation_parent {
                variations.entry(parent).or_default().push(t.id);
            }
        }
        for (parent, members) in &mut variations {
            if types.get(parent).is_some_and(|p| p.published) && !members.contains(parent) {
                members.push(*parent);
            }
            members.sort_by(|a, b| {
                let (a, b) = (&types[a], &types[b]);
                a.meta_level
                    .cmp(&b.meta_level)
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
        let meta_group_names = meta_groups
            .into_iter()
            .filter_map(|(id, g)| Some((id.parse().ok()?, g.name.en)))
            .collect();
        let group_names: HashMap<i32, String> = groups
            .into_iter()
            .filter_map(|(id, g)| Some((id.parse().ok()?, g.name.en)))
            .collect();
        let ship_groups = ship_groups(&types, &group_names);

        Ok(Self {
            types,
            by_name,
            market_groups,
            market_roots,
            skills,
            icon_files,
            variations,
            meta_group_names,
            group_names,
            cargo_searchable,
            required_skills,
            projection,
            ship_groups,
            mutaplasmids,
            alpha_clones,
            replacements: same_items.finish(),
        })
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

    /// A tactical destroyer's modes, found the way pyfa finds them: mode items named after the
    /// ship. Empty for every other ship.
    pub fn modes_for(&self, ship: TypeId) -> Vec<TypeId> {
        let Some(ship) = self.get(ship) else {
            return Vec::new();
        };
        if ship.group_id != GROUP_TACTICAL_DESTROYER && ship.name != "Anhinga" {
            return Vec::new();
        }
        let prefix = format!("{} ", ship.name.to_lowercase());
        let mut modes: Vec<TypeId> = self
            .types
            .values()
            .filter(|t| t.kind == Kind::Mode && t.name.to_lowercase().starts_with(&prefix))
            .map(|t| t.id)
            .collect();
        modes.sort_unstable();
        modes
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
        crate::port::renames::current_name(&key)
            .and_then(|current| self.by_name.get(current))
            .or_else(|| self.by_name.get(&key))
            .and_then(|id| self.types.get(id))
    }

    /// An Alpha clone grade by ID.
    pub fn alpha_clone(&self, id: i32) -> Option<&AlphaClone> {
        self.alpha_clones.iter().find(|c| c.id == id)
    }

    /// Published, fittable types matching a market search the way pyfa runs it: every word
    /// found in the name, with pyfa's jargon and wildcards, in pyfa's order. Nothing for a query
    /// too short to search for.
    pub fn search(&self, query: &str, limit: usize) -> Vec<TypeId> {
        let Some(query) = Query::parse(query, Jargon::global()) else {
            return Vec::new();
        };
        let mut hits: Vec<TypeId> = self
            .types
            .values()
            .filter(|t| {
                t.published
                    && t.market_group.is_some()
                    && !matches!(t.kind, Kind::Skill | Kind::Mode | Kind::Other)
                    && (t.kind != Kind::Cargo || self.cargo_searchable.contains(&t.id))
            })
            .filter(|t| query.matches(&t.name))
            .map(|t| t.id)
            .collect();
        crate::market::sort_items(&self.types, &mut hits);
        hits.truncate(limit);
        hits
    }

    /// A ship group's published ships, by race and name.
    pub fn ships_in(&self, group: i32) -> &[TypeId] {
        self.ship_groups
            .iter()
            .find(|(g, _)| *g == group)
            .map_or(&[], |(_, ships)| ships.as_slice())
    }
}

/// Published ships by group, as the fittings browser lists them.
fn ship_groups(
    types: &HashMap<TypeId, ItemType>,
    group_names: &HashMap<i32, String>,
) -> Vec<(i32, Vec<TypeId>)> {
    let mut groups: HashMap<i32, Vec<TypeId>> = HashMap::new();
    for t in types
        .values()
        .filter(|t| t.kind == Kind::Ship && t.published)
    {
        groups.entry(t.group_id).or_default().push(t.id);
    }
    let mut groups: Vec<(i32, Vec<TypeId>)> = groups.into_iter().collect();
    for (_, ships) in &mut groups {
        // pyfa puts ships of no known race last.
        ships.sort_by(|a, b| {
            let (a, b) = (&types[a], &types[b]);
            (a.race.is_none(), a.race, &a.name).cmp(&(b.race.is_none(), b.race, &b.name))
        });
    }
    let name = |g: i32| group_names.get(&g).map_or("", String::as_str);
    groups.sort_by(|a, b| name(a.0).cmp(name(b.0)));
    groups
}

/// The highest level any Alpha clone can train each skill to.
fn alpha_skill_caps(grades: &[AlphaClone]) -> HashMap<TypeId, u8> {
    let mut caps: HashMap<TypeId, u8> = HashMap::new();
    for (&skill, &level) in grades.iter().flat_map(|g| &g.caps) {
        let cap = caps.entry(skill).or_default();
        *cap = (*cap).max(level);
    }
    caps
}

/// Whether a type needs a skill, or a prerequisite of one, beyond the Alpha caps.
fn needs_omega(
    id: TypeId,
    required: &HashMap<TypeId, Vec<(TypeId, u8)>>,
    alpha: &HashMap<TypeId, u8>,
    memo: &mut HashMap<TypeId, bool>,
) -> bool {
    if let Some(&known) = memo.get(&id) {
        return known;
    }
    // Settles cycles, which the data should not have anyway.
    memo.insert(id, false);
    let omega = required.get(&id).is_some_and(|skills| {
        skills.iter().any(|&(skill, level)| {
            level > alpha.get(&skill).copied().unwrap_or(0)
                || needs_omega(skill, required, alpha, memo)
        })
    });
    memo.insert(id, omega);
    omega
}

/// Drop market groups with nothing fittable anywhere below them, and fill in child lists.
fn prune_and_link(groups: &mut BTreeMap<i32, MarketGroup>) {
    let mut keep: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for group in groups.values() {
        if group.types.is_empty() {
            continue;
        }
        let mut id = Some(group.id);
        while let Some(current) = id {
            if !keep.insert(current) {
                break;
            }
            id = groups.get(&current).and_then(|g| g.parent);
        }
    }
    groups.retain(|id, _| keep.contains(id));
    let links: Vec<(i32, i32)> = groups
        .values()
        .filter_map(|g| Some((g.parent?, g.id)))
        .collect();
    for (parent, child) in links {
        if let Some(parent) = groups.get_mut(&parent) {
            parent.children.push(child);
        }
    }
    let names: HashMap<i32, String> = groups.values().map(|g| (g.id, g.name.clone())).collect();
    for group in groups.values_mut() {
        group.children.sort_by(|a, b| names[a].cmp(&names[b]));
    }
}

/// What a cache must have been built from: the cache format, gyfa's version, the dump's release
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

fn read_cache(path: &Path, key: &str) -> Option<Catalog> {
    let bytes = std::fs::read(path).ok()?;
    let cache: CacheFile = postcard::from_bytes(&bytes).ok()?;
    (cache.key == key).then_some(cache.catalog)
}

/// Best effort: without a cache the next start just builds the catalog again.
fn write_cache(path: &Path, key: &str, catalog: &Catalog) {
    let Ok(bytes) = postcard::to_stdvec(&CacheFileRef { key, catalog }) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Per process, so two starts writing at once never interleave in one file.
    let tmp = path.with_extension(format!("bin.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn read<T: DeserializeOwned>(dir: &Path, file: &str) -> Result<T, String> {
    let path: PathBuf = dir.join(file);
    let reader = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_reader(BufReader::with_capacity(1 << 20, reader))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Deserialize)]
struct Localized {
    #[serde(default)]
    en: String,
}

#[derive(Deserialize)]
struct SdeGroup {
    #[serde(rename = "categoryID")]
    category_id: i32,
    name: Localized,
}

#[derive(Deserialize)]
struct SdeMarketGroup {
    name: Localized,
    #[serde(rename = "parentGroupID")]
    parent_group_id: Option<i32>,
    #[serde(rename = "iconID")]
    icon_id: Option<i32>,
}

#[derive(Deserialize)]
struct SdeIcon {
    #[serde(rename = "iconFile")]
    icon_file: Option<String>,
}

#[derive(Deserialize)]
struct SdeGraphic {
    #[serde(rename = "iconFolder")]
    icon_folder: Option<String>,
}

#[derive(Deserialize)]
struct SdeType {
    name: Localized,
    #[serde(rename = "groupID")]
    group_id: i32,
    #[serde(rename = "marketGroupID")]
    market_group_id: Option<i32>,
    #[serde(default)]
    published: bool,
    #[serde(rename = "iconID")]
    icon_id: Option<i32>,
    #[serde(rename = "graphicID")]
    graphic_id: Option<i32>,
    #[serde(rename = "variationParentTypeID")]
    variation_parent_type_id: Option<i32>,
    #[serde(rename = "metaGroupID")]
    meta_group_id: Option<i32>,
    #[serde(rename = "metaLevel")]
    meta_level: Option<i32>,
    volume: Option<f64>,
    #[serde(rename = "packagedVolume")]
    packaged_volume: Option<f64>,
    capacity: Option<f64>,
    #[serde(rename = "factionID")]
    faction_id: Option<i32>,
    #[serde(rename = "raceID")]
    race_id: Option<i32>,
}

#[derive(Deserialize)]
struct SdeCloneGrade {
    #[serde(default)]
    name: String,
    #[serde(default)]
    skills: Vec<SdeCloneSkill>,
}

#[derive(Deserialize)]
struct SdeCloneSkill {
    #[serde(rename = "typeID")]
    type_id: TypeId,
    level: u8,
}

#[derive(Deserialize)]
struct SdeMetaGroup {
    name: Localized,
}

#[derive(Deserialize)]
struct PhbEffect {
    #[serde(rename = "effectCategory", default)]
    effect_category: i32,
    #[serde(rename = "fittingUsageChanceAttributeID", default)]
    fitting_usage_chance_attribute_id: Option<i32>,
}

/// Groups types into pyfa's replacement sets while the catalog is built.
#[derive(Default)]
struct SameItems {
    /// Types by everything that makes them the same item.
    classes: HashMap<SameItemKey, Vec<TypeId>>,
}

/// Group, required skills (types, not levels), effects, and the attributes that matter with
/// their exact values.
type SameItemKey = (i32, Vec<TypeId>, Vec<i32>, Vec<(i32, u64)>);

impl SameItems {
    /// Categories pyfa looks for replacements in, less the ones nothing is swapped in.
    const CATEGORIES: [i32; 8] = [
        category::MODULE,
        category::CHARGE,
        category::DRONE,
        category::IMPLANT,
        category::DEPLOYABLE,
        category::SUBSYSTEM,
        category::STRUCTURE_MODULE,
        category::FIGHTER,
    ];
    /// Attributes that differ between otherwise identical items: mass, main color, radius, tech
    /// level, meta level, meta group and color scheme.
    const IGNORED: [i32; 7] = [4, 124, 162, 422, 633, 1692, 1768];

    fn consider(
        &mut self,
        id: TypeId,
        t: &SdeType,
        category_id: i32,
        dogma: Option<&PhbTypeDogma>,
        required: &[(TypeId, u8)],
    ) {
        if !Self::CATEGORIES.contains(&category_id) {
            return;
        }
        let skill_attrs: HashSet<i32> = attr::REQUIRED_SKILLS
            .iter()
            .flat_map(|&(skill, level)| [skill, level])
            .collect();
        // -0.0 and 0.0 are the same value, as in pyfa's comparison.
        let bits = |v: f64| (v + 0.0).to_bits();
        let mut attrs: Vec<(i32, u64)> = dogma
            .map_or_default(|d| d.attributes.as_slice())
            .iter()
            .filter(|a| !skill_attrs.contains(&a.attribute_id))
            .filter(|a| !Self::IGNORED.contains(&a.attribute_id))
            .map(|a| (a.attribute_id, bits(a.value)))
            .collect();
        // pyfa adds these type columns as attributes, unless dogma already has them.
        for (attr_id, value) in [(38, t.capacity), (161, t.volume)] {
            if let Some(v) = value
                && !attrs.iter().any(|&(a, _)| a == attr_id)
            {
                attrs.push((attr_id, bits(v)));
            }
        }
        // Items without attributes are never the same as anything.
        if attrs.is_empty() {
            return;
        }
        attrs.sort_unstable();
        attrs.dedup_by_key(|(a, _)| *a);
        let mut skills: Vec<TypeId> = required.iter().map(|&(s, _)| s).collect();
        skills.sort_unstable();
        let mut effects: Vec<i32> =
            dogma.map_or_default(|d| d.effects.iter().map(|e| e.effect_id).collect());
        effects.sort_unstable();
        effects.dedup();
        self.classes
            .entry((t.group_id, skills, effects, attrs))
            .or_default()
            .push(id);
    }

    fn finish(self) -> HashMap<TypeId, Vec<TypeId>> {
        let mut out = HashMap::new();
        for mut members in self.classes.into_values().filter(|m| m.len() > 1) {
            members.sort_unstable();
            for &id in &members {
                out.insert(id, members.iter().copied().filter(|&o| o != id).collect());
            }
        }
        out
    }
}

#[derive(Deserialize)]
struct PhbTypeDogma {
    #[serde(rename = "dogmaAttributes", default)]
    attributes: Vec<PhbAttr>,
    #[serde(rename = "dogmaEffects", default)]
    effects: Vec<PhbEffectRef>,
}

#[derive(Deserialize)]
struct PhbAttr {
    #[serde(rename = "attributeID")]
    attribute_id: i32,
    value: f64,
}

#[derive(Deserialize)]
struct PhbEffectRef {
    #[serde(rename = "effectID")]
    effect_id: i32,
}
