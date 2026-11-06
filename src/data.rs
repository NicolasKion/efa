//! What the UI needs to know about items: names, the market tree, where things fit and which
//! charges go where. refine works on type IDs only, so all of this comes from the dump directly.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use serde::{Deserialize, de::DeserializeOwned};

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
}

const GROUP_BOOSTER: i32 = 303;
/// Tactical destroyer modes.
const GROUP_SHIP_MODIFIERS: i32 = 1306;
const GROUP_TACTICAL_DESTROYER: i32 = 1305;

mod effect {
    pub const LAUNCHER_FITTED: i32 = 40;
    pub const TURRET_FITTED: i32 = 42;
    pub const LOW: i32 = 11;
    pub const HIGH: i32 = 12;
    pub const MID: i32 = 13;
    pub const RIG: i32 = 2663;
    pub const SUBSYSTEM: i32 = 3772;
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
    /// Maximum spool bonus, on Precursor weapons and mutadaptive repairers.
    pub const SPOOL_MAX: [i32; 2] = [2734, 2797];
    /// The booster slot, which pyfa calls boosterness.
    pub const BOOSTERNESS: i32 = 1087;
    /// Chance of a booster's first side effect, which pyfa ranks booster variations by.
    pub const BOOSTER_EFFECT_CHANCE1: i32 = 1089;
}

/// Which kind of hardpoint a module takes, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Other,
}

#[derive(Debug, Clone)]
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
    /// Slot a booster goes into; a fit holds one booster per slot.
    pub booster_slot: Option<u32>,
    /// Chance of a booster's first side effect, missing on boosters without one.
    pub booster_chance: Option<f64>,
    /// Effects that may come with a booster, by effect ID: the ones with a usage chance.
    pub side_effects: Vec<i32>,
}

impl ItemType {
    /// The word pyfa puts into menu labels for this kind of item.
    pub fn category_label(&self) -> &'static str {
        match self.kind {
            Kind::Ship => "Ship",
            Kind::Module(Slot::Subsystem) => "Subsystem",
            Kind::Module(_) => "Module",
            Kind::Charge => "Charge",
            Kind::Drone => "Drone",
            Kind::Fighter => "Fighter",
            Kind::Implant => "Implant",
            Kind::Booster => "Booster",
            Kind::Skill => "Skill",
            Kind::Mode => "Tactical Mode",
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

#[derive(Debug, Clone)]
pub struct MarketGroup {
    pub id: i32,
    pub name: String,
    pub parent: Option<i32>,
    pub children: Vec<i32>,
    pub types: Vec<TypeId>,
    pub icon_id: Option<i32>,
}

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
}

impl Catalog {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let groups: HashMap<String, SdeGroup> = read(dir, "sde/groups.json")?;
        let market: HashMap<String, SdeMarketGroup> = read(dir, "sde/marketgroups.json")?;
        let sde_types: HashMap<String, SdeType> = read(dir, "sde/types.json")?;
        let effects: HashMap<String, PhbEffect> = read(dir, "fsd_built/dogmaeffects.json")?;
        let dogma: HashMap<String, PhbTypeDogma> = read(dir, "fsd_built/typedogma.json")?;
        let icons: HashMap<String, SdeIcon> = read(dir, "sde/icons.json")?;
        let meta_groups: HashMap<String, SdeMetaGroup> = read(dir, "sde/metagroups.json")?;
        let graphics: HashMap<String, SdeGraphic> = read(dir, "sde/graphics.json")?;
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

        let mut types = HashMap::with_capacity(sde_types.len());
        let mut skills = Vec::new();
        for (id, t) in sde_types {
            let Ok(id) = id.parse::<TypeId>() else {
                continue;
            };
            let category_id = group_category.get(&t.group_id).copied().unwrap_or(0);
            if !matches!(
                category_id,
                category::SHIP
                    | category::MODULE
                    | category::CHARGE
                    | category::DRONE
                    | category::IMPLANT
                    | category::SUBSYSTEM
                    | category::FIGHTER
                    | category::SKILL
            ) {
                continue;
            }

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
                dogma.and_then(|d| {
                    d.attributes
                        .iter()
                        .find(|a| a.attribute_id == wanted)
                        .map(|a| a.value)
                })
            };

            let kind = match category_id {
                category::SHIP => Kind::Ship,
                category::CHARGE => Kind::Charge,
                category::DRONE => Kind::Drone,
                category::FIGHTER => Kind::Fighter,
                category::SKILL => Kind::Skill,
                category::IMPLANT if t.group_id == GROUP_BOOSTER => Kind::Booster,
                category::MODULE if t.group_id == GROUP_SHIP_MODIFIERS => Kind::Mode,
                category::IMPLANT => Kind::Implant,
                category::SUBSYSTEM => Kind::Module(Slot::Subsystem),
                category::MODULE if has_effect(effect::HIGH) => Kind::Module(Slot::High),
                category::MODULE if has_effect(effect::MID) => Kind::Module(Slot::Mid),
                category::MODULE if has_effect(effect::LOW) => Kind::Module(Slot::Low),
                category::MODULE if has_effect(effect::RIG) => Kind::Module(Slot::Rig),
                category::MODULE if has_effect(effect::SUBSYSTEM) => Kind::Module(Slot::Subsystem),
                _ => Kind::Other,
            };
            if kind == Kind::Skill && t.published {
                skills.push(id);
            }

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
                },
            );
        }

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
        for t in types.values() {
            if !t.published || matches!(t.kind, Kind::Skill | Kind::Mode | Kind::Other) {
                continue;
            }
            if let Some(group) = t.market_group.and_then(|g| market_groups.get_mut(&g)) {
                group.types.push(t.id);
            }
        }
        for group in market_groups.values_mut() {
            group
                .types
                .sort_by(|a, b| types[a].name.cmp(&types[b].name));
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

        Ok(Self {
            types,
            by_name,
            market_groups,
            market_roots,
            skills,
            icon_files,
            variations,
            meta_group_names,
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

    pub fn find(&self, name: &str) -> Option<&ItemType> {
        self.by_name
            .get(&name.trim().to_lowercase())
            .and_then(|id| self.types.get(id))
    }

    /// Published, fittable types whose name contains every word of the query.
    pub fn search(&self, query: &str, limit: usize) -> Vec<TypeId> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<&ItemType> = self
            .types
            .values()
            .filter(|t| {
                t.published
                    && t.market_group.is_some()
                    && !matches!(t.kind, Kind::Skill | Kind::Mode | Kind::Other)
            })
            .filter(|t| {
                let name = t.name.to_lowercase();
                words.iter().all(|w| name.contains(w))
            })
            .collect();
        hits.sort_by(|a, b| {
            a.name
                .len()
                .cmp(&b.name.len())
                .then_with(|| a.name.cmp(&b.name))
        });
        hits.into_iter().take(limit).map(|t| t.id).collect()
    }
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
