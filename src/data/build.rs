//! Building the catalog from the dump: reading the SDE files, classifying types, and the market
//! tree, modes, ship groups, Omega needs and replacements worked out from them.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use super::{
    AlphaClone, CONTAINER_GROUPS, Catalog, FIT_STRUCTURE_GROUPS, Hardpoint, ItemType, Kind,
    MARKET_BOOSTERS, MARKET_SPECIAL_BOOSTERS, MarketGroup, Slot, TypeId, category, effect_category,
    muta, order, projection, publish, sde,
};
use crate::ship_browser::Race;

const GROUP_BOOSTER: i32 = 303;

/// Tactical destroyer modes.
const GROUP_SHIP_MODIFIERS: i32 = 1306;
const GROUP_TACTICAL_DESTROYER: i32 = 1305;

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
}

impl Catalog {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let groups: HashMap<i32, sde::Group> = sde::read(dir, "sde/groups.json")?;
        let market: HashMap<i32, sde::MarketGroup> = sde::read(dir, "sde/marketgroups.json")?;
        let sde_types: HashMap<TypeId, sde::Type> = sde::read(dir, "sde/types.json")?;
        let effects: HashMap<i32, sde::PhbEffect> = sde::read(dir, "fsd_built/dogmaeffects.json")?;
        let dogma: HashMap<TypeId, sde::TypeDogma> = sde::read(dir, "fsd_built/typedogma.json")?;
        let icons: HashMap<i32, sde::Icon> = sde::read(dir, "sde/icons.json")?;
        let meta_groups: HashMap<i32, sde::MetaGroup> = sde::read(dir, "sde/metagroups.json")?;
        let graphics: HashMap<i32, sde::Graphic> = sde::read(dir, "sde/graphics.json")?;
        let clone_grades: HashMap<i32, sde::CloneGrade> = sde::read(dir, "sde/clonegrades.json")?;
        let muta = muta::Source::read(dir)?;
        let stand_ins = muta.stand_ins();
        let icon_files: HashMap<i32, String> = icons
            .into_iter()
            .filter_map(|(id, icon)| Some((id, icon.icon_file?)))
            .collect();

        let chance_effects: HashSet<i32> = effects
            .iter()
            .filter(|(_, e)| e.fitting_usage_chance_attribute_id.is_some())
            .map(|(&id, _)| id)
            .collect();
        let effect_category: HashMap<i32, i32> = effects
            .into_iter()
            .map(|(id, e)| (id, e.effect_category))
            .collect();
        let group_category: HashMap<i32, i32> =
            groups.iter().map(|(&id, g)| (id, g.category_id)).collect();

        // Skills carry the race a ship's race is read from.
        let type_races: HashMap<TypeId, i32> = sde_types
            .iter()
            .filter_map(|(&id, t)| Some((id, t.race_id.filter(|&r| r != 0)?)))
            .collect();

        let mut projection = projection::Builder::new(&effect_category);
        let mut same_items = SameItems::default();
        let mut types = HashMap::with_capacity(sde_types.len());
        let mut skills = Vec::new();
        let mut required_skills = HashMap::new();
        let today = publish::today();
        for (id, mut t) in sde_types {
            let category_id = group_category.get(&t.group_id).copied().unwrap_or(0);
            let last_injection = dogma.get(&id).and_then(|d| d.attr(publish::LAST_INJECTION));
            t.published = publish::published(
                id,
                &t.name.en,
                t.group_id,
                t.market_group_id.is_some(),
                t.published,
                last_injection,
                today,
            );
            projection.consider(id, &t, category_id, dogma.get(&id));
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
            let stand_in = stand_ins.get(&id).and_then(|b| dogma.get(b));
            let dogma = dogma.get(&id);
            let has_effect = |wanted: i32| dogma.is_some_and(|d| d.has_effect(wanted));
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
                [dogma, stand_in]
                    .into_iter()
                    .flatten()
                    .find_map(|d| d.attr(wanted))
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
            let required: Vec<(TypeId, u8)> = sde::REQUIRED_SKILL_ATTRS
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
                let folder = graphics.get(&graphic)?.icon_folder.as_ref()?;
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
                    lower_name: t.name.en.to_lowercase(),
                    market_rank: 0,
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
            .map(|(id, grade)| AlphaClone {
                id,
                name: grade.name,
                caps: grade.skills.iter().map(|s| (s.type_id, s.level)).collect(),
            })
            .collect();
        alpha_clones.sort_by(|a, b| a.name.cmp(&b.name));
        let alpha = alpha_skill_caps(&alpha_clones);
        let mut omega = HashMap::new();
        for t in types.values_mut() {
            t.omega = needs_omega(t.id, &required_skills, &alpha, &mut omega);
        }
        let mut required_for: HashMap<TypeId, Vec<(TypeId, u8)>> = HashMap::new();
        for (&id, skills) in &required_skills {
            for &(skill, level) in skills {
                required_for.entry(skill).or_default().push((id, level));
            }
        }
        for users in required_for.values_mut() {
            users.sort_unstable();
        }

        let projection = projection.finish(dir)?;
        let mutaplasmids = muta.finish(
            dir,
            &types
                .iter()
                .filter(|(_, t)| t.kind == Kind::Cargo)
                .map(|(&id, t)| (id, t.name.clone()))
                .collect(),
            &dogma,
        )?;

        // Published names win when two types share one, which happens with old unpublished copies;
        // otherwise the lowest type ID does, so every build picks the same one.
        let mut by_name = HashMap::with_capacity(types.len());
        let mut in_order: Vec<&ItemType> = types.values().collect();
        in_order.sort_unstable_by_key(|t| t.id);
        for t in in_order {
            let key = t.name.to_lowercase();
            match by_name.get(&key).and_then(|id| types.get(id)) {
                Some(existing) if existing.published || !t.published => {}
                _ => {
                    by_name.insert(key, t.id);
                }
            }
        }

        let mut market_groups: BTreeMap<i32, MarketGroup> = market
            .into_iter()
            .map(|(id, g)| {
                (
                    id,
                    MarketGroup {
                        id,
                        name: g.name.en,
                        parent: g.parent_group_id,
                        children: Vec::new(),
                        types: Vec::new(),
                        icon_id: g.icon_id,
                    },
                )
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
        order::rank(&mut types);
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
            group.types.sort_by_key(|id| types[id].market_rank);
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
            .map(|(id, g)| (id, g.name.en))
            .collect();
        let group_names: HashMap<i32, String> =
            groups.into_iter().map(|(id, g)| (id, g.name.en)).collect();
        let ship_groups = ship_groups(&types, &group_names);
        let mut searchable: Vec<TypeId> = types
            .values()
            .filter(|t| {
                t.published
                    && t.market_group.is_some()
                    && !matches!(t.kind, Kind::Skill | Kind::Mode | Kind::Other)
                    && (t.kind != Kind::Cargo || cargo_searchable.contains(&t.id))
            })
            .map(|t| t.id)
            .collect();
        searchable.sort_by_key(|id| types[id].market_rank);
        let mut charges_by_group: HashMap<i32, Vec<TypeId>> = HashMap::new();
        for t in types.values() {
            if t.kind == Kind::Charge && t.published && t.market_group.is_some() {
                charges_by_group.entry(t.group_id).or_default().push(t.id);
            }
        }
        for charges in charges_by_group.values_mut() {
            charges.sort_by_key(|id| types[id].market_rank);
        }
        let modes = modes(&types);

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
            searchable,
            charges_by_group,
            modes,
            required_skills,
            required_for,
            projection,
            ship_groups,
            mutaplasmids,
            alpha_clones,
            replacements: same_items.finish(),
        })
    }
}

/// Each tactical destroyer's modes, found the way pyfa finds them: mode items named after the
/// ship.
fn modes(types: &HashMap<TypeId, ItemType>) -> HashMap<TypeId, Vec<TypeId>> {
    let modes: Vec<&ItemType> = types.values().filter(|t| t.kind == Kind::Mode).collect();
    types
        .values()
        .filter(|s| s.group_id == GROUP_TACTICAL_DESTROYER || s.name == "Anhinga")
        .filter_map(|ship| {
            let prefix = format!("{} ", ship.lower_name);
            let mut of_ship: Vec<TypeId> = modes
                .iter()
                .filter(|m| m.lower_name.starts_with(&prefix))
                .map(|m| m.id)
                .collect();
            of_ship.sort_unstable();
            (!of_ship.is_empty()).then_some((ship.id, of_ship))
        })
        .collect()
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
        t: &sde::Type,
        category_id: i32,
        dogma: Option<&sde::TypeDogma>,
        required: &[(TypeId, u8)],
    ) {
        if !Self::CATEGORIES.contains(&category_id) {
            return;
        }
        let skill_attrs: HashSet<i32> = sde::REQUIRED_SKILL_ATTRS
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
