//! The fitting wizard's picks: which weapons, propulsion, tackle, tank, damage modules and rigs
//! suit a hull, and where they go. No UI here; the wizard dialog shows and applies what this
//! works out.
//!
//! Items are found by their T1 name and resolved to a tier through their variations, so a new
//! meta item or a renamed faction one is picked up without a list of type IDs.

use std::collections::HashMap;

use crate::{
    data::{Catalog, Hardpoint, ItemType, Kind, Slot, TypeId},
    engine::{CalcOptions, Engine, Pilot, Resource, Stats},
    fit::{Fit, Module},
    item_data::{ItemData, Traits},
};

/// Dogma attributes the wizard reads from the data.
mod attr {
    pub const POWER: i32 = 30;
    pub const CPU: i32 = 50;
    pub const UPGRADE_COST: i32 = 1153;
    pub const RIG_SIZE: i32 = 1547;
}

/// Meta groups, as the market's buttons name them.
mod meta {
    pub const TECH1: i32 = 1;
    pub const TECH2: i32 = 2;
    pub const FACTION: i32 = 4;
}

/// Battlecruiser groups: medium hulls that carry large shield extenders and boosters.
const BATTLECRUISER_GROUPS: [i32; 3] = [419, 540, 1201];

/// How big a hull is, read from its rig size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HullSize {
    Small,
    Medium,
    Large,
    Capital,
}

impl HullSize {
    fn from_rig_size(size: f64) -> Self {
        match size.round() as i32 {
            ..=1 => Self::Small,
            2 => Self::Medium,
            3 => Self::Large,
            _ => Self::Capital,
        }
    }

    /// The turret charge size this hull's guns take.
    fn from_charge_size(size: f64) -> Self {
        Self::from_rig_size(size)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Capital => "Capital",
        }
    }

    /// The word neutralizers, nosferatus and capacitor boosters are sized by.
    fn energy_word(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Heavy",
            Self::Capital => "Capital",
        }
    }
}

/// What a weapon family shoots, which decides its damage modules, rigs and computers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeaponKind {
    Projectile,
    Hybrid,
    Energy,
    Missile,
    Precursor,
    Vorton,
}

impl WeaponKind {
    fn from_group(group: i32) -> Option<Self> {
        Some(match group {
            53 => Self::Energy,
            55 => Self::Projectile,
            74 => Self::Hybrid,
            1986 => Self::Precursor,
            4060 => Self::Vorton,
            506..=511 | 524 | 771 | 1245 | 1673 | 1674 => Self::Missile,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Projectile => "Projectile",
            Self::Hybrid => "Hybrid",
            Self::Energy => "Energy",
            Self::Missile => "Missile",
            Self::Precursor => "Precursor",
            Self::Vorton => "Vorton",
        }
    }

    fn damage_mod(self) -> &'static str {
        match self {
            Self::Projectile => "Gyrostabilizer I",
            Self::Hybrid => "Magnetic Field Stabilizer I",
            Self::Energy => "Heat Sink I",
            Self::Missile => "Ballistic Control System I",
            Self::Precursor => "Entropic Radiation Sink I",
            Self::Vorton => "Vorton Tuning System I",
        }
    }

    fn computer(self) -> &'static str {
        match self {
            Self::Missile => "Missile Guidance Computer I",
            _ => "Tracking Computer I",
        }
    }

    fn enhancer(self) -> &'static str {
        match self {
            Self::Missile => "Missile Guidance Enhancer I",
            _ => "Tracking Enhancer I",
        }
    }

    /// Damage rigs, without the size word: the strongest first.
    fn rigs(self) -> &'static [&'static str] {
        match self {
            Self::Projectile => &[
                "Projectile Collision Accelerator I",
                "Projectile Burst Aerator I",
            ],
            Self::Hybrid => &["Hybrid Collision Accelerator I", "Hybrid Burst Aerator I"],
            Self::Energy => &["Energy Collision Accelerator I", "Energy Burst Aerator I"],
            Self::Missile => &[
                "Warhead Calefaction Catalyst I",
                "Bay Loading Accelerator I",
            ],
            Self::Precursor | Self::Vorton => &[],
        }
    }
}

/// Launcher groups by the size of hull that carries them, with the words a bonus names them by.
fn launcher(group: i32) -> Option<(HullSize, &'static str)> {
    Some(match group {
        507 => (HullSize::Small, "rocket"),
        509 => (HullSize::Small, "light missile"),
        511 => (HullSize::Medium, "rapid light missile"),
        510 => (HullSize::Medium, "heavy missile"),
        771 => (HullSize::Medium, "heavy assault missile"),
        1245 => (HullSize::Large, "rapid heavy missile"),
        506 => (HullSize::Large, "cruise missile"),
        508 => (HullSize::Large, "torpedo"),
        524 => (HullSize::Capital, "xl torpedo"),
        1674 => (HullSize::Capital, "xl cruise missile"),
        1673 => (HullSize::Capital, "rapid torpedo"),
        _ => return None,
    })
}

/// A weapon line the hull can mount, by its T1 item.
#[derive(Debug, Clone)]
pub struct Family {
    pub base: TypeId,
    /// The T1 name without its " I", e.g. `425mm AutoCannon`.
    pub name: String,
    pub kind: WeaponKind,
    pub hardpoint: Hardpoint,
    pub size: HullSize,
    /// 2 when the hull's bonuses name this weapon, 1 when they name its kind, else 0.
    pub bonus: u8,
    /// How many the hull mounts.
    pub count: usize,
}

/// Which variation of each item to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    T2,
    Faction,
    /// The best meta variation, which is usually the compact one.
    Meta,
    T1,
}

impl Tier {
    pub const ALL: [Self; 4] = [Self::T2, Self::Faction, Self::Meta, Self::T1];

    pub fn label(self) -> &'static str {
        match self {
            Self::T2 => "T2",
            Self::Faction => "Faction",
            Self::Meta => "Best meta",
            Self::T1 => "T1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ammo {
    Faction,
    Standard,
    None,
}

impl Ammo {
    pub const ALL: [Self; 3] = [Self::Faction, Self::Standard, Self::None];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    Microwarpdrive,
    Afterburner,
    None,
}

impl Prop {
    pub const ALL: [Self; 3] = [Self::Microwarpdrive, Self::Afterburner, Self::None];
}

/// Extra modules, each one of a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Utility {
    Scram,
    Point,
    Web,
    /// The heavy stasis grappler, a short-range web for battleships and up.
    Grappler,
    Neut,
    Nos,
    SensorBooster,
    Computer,
    CapBooster,
    Enhancer,
}

impl Utility {
    pub const ALL: [Self; 10] = [
        Self::Scram,
        Self::Point,
        Self::Web,
        Self::Grappler,
        Self::Neut,
        Self::Nos,
        Self::SensorBooster,
        Self::Computer,
        Self::CapBooster,
        Self::Enhancer,
    ];

    /// Whether a hull of this size can use it: grapplers only fit battleships and bigger.
    pub fn fits(self, size: HullSize) -> bool {
        self != Self::Grappler || matches!(size, HullSize::Large | HullSize::Capital)
    }

    pub fn slot(self) -> Slot {
        match self {
            Self::Neut | Self::Nos => Slot::High,
            Self::Enhancer => Slot::Low,
            _ => Slot::Mid,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Scram => "Scram",
            Self::Point => "Point",
            Self::Web => "Web",
            Self::Grappler => "Grappler",
            Self::Neut => "Neut",
            Self::Nos => "Nos",
            Self::SensorBooster => "Sensor booster",
            Self::Computer => "Computer",
            Self::CapBooster => "Cap booster",
            Self::Enhancer => "Enhancer",
        }
    }

    /// What is kept longest when the fit has to shrink.
    fn priority(self) -> u8 {
        match self {
            Self::Scram | Self::Point | Self::Web | Self::Grappler => 8,
            Self::CapBooster => 4,
            Self::Neut => 3,
            Self::Nos => 2,
            Self::SensorBooster | Self::Computer | Self::Enhancer => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tank {
    ShieldBuffer,
    ShieldActive,
    ArmorBuffer,
    ArmorActive,
    None,
}

impl Tank {
    pub const ALL: [Self; 5] = [
        Self::ShieldBuffer,
        Self::ShieldActive,
        Self::ArmorBuffer,
        Self::ArmorActive,
        Self::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ShieldBuffer => "Shield buffer",
            Self::ShieldActive => "Shield active",
            Self::ArmorBuffer => "Armor buffer",
            Self::ArmorActive => "Armor active",
            Self::None => "None",
        }
    }

    fn armor(self) -> bool {
        matches!(self, Self::ArmorBuffer | Self::ArmorActive)
    }
}

/// Which damage modules go into the low slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageMods {
    Weapon,
    Drone,
    None,
}

impl DamageMods {
    pub const ALL: [Self; 3] = [Self::Weapon, Self::Drone, Self::None];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rigs {
    Damage,
    Tank,
    None,
}

impl Rigs {
    pub const ALL: [Self; 3] = [Self::Damage, Self::Tank, Self::None];

    pub fn label(self) -> &'static str {
        match self {
            Self::Damage => "Damage",
            Self::Tank => "Tank",
            Self::None => "None",
        }
    }
}

/// Everything the user picks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    /// Index into [`Setup::families`]; None fits no weapons.
    pub family: Option<usize>,
    pub tier: Tier,
    pub ammo: Ammo,
    pub prop: Prop,
    /// In [`Utility::ALL`] order.
    pub utilities: Vec<Utility>,
    pub tank: Tank,
    pub damage: DamageMods,
    pub rigs: Rigs,
    /// Empty the module racks and rigs first, instead of filling the free slots.
    pub clear: bool,
}

impl Choices {
    pub fn toggle(&mut self, utility: Utility) {
        if let Some(i) = self.utilities.iter().position(|&u| u == utility) {
            self.utilities.remove(i);
        } else {
            self.utilities.push(utility);
            self.utilities.sort_unstable();
        }
    }

    pub fn has(&self, utility: Utility) -> bool {
        self.utilities.contains(&utility)
    }
}

/// What the hull offers, read from the engine with the fit's pilot.
#[derive(Debug, Clone)]
pub struct Facts {
    pub ship: TypeId,
    pub size: HullSize,
    battlecruiser: bool,
    /// Slots in each rack, high, mid, low and rig.
    totals: HashMap<Slot, usize>,
    turrets: usize,
    launchers: usize,
    pub cpu: f64,
    pub powergrid: f64,
    pub calibration: f64,
    drone_bandwidth: f64,
    /// The hull's bonuses in lower case, one per line, without markup.
    bonuses: Vec<String>,
    /// The certificates the ship's mastery asks for, lower case, like "large energy turret".
    mastery: Vec<String>,
}

impl Facts {
    /// None without a ship.
    pub fn gather(
        engine: &Engine,
        data: &ItemData,
        catalog: &Catalog,
        fit: &Fit,
        pilot: &Pilot,
    ) -> Option<Self> {
        let ship = fit.ship?;
        // The bare hull: outputs and slots without what is fitted now.
        let hull = Fit {
            ship: Some(ship),
            mode: fit.mode,
            subsystems: fit.subsystems.clone(),
            character: fit.character,
            ..Fit::default()
        };
        let stats = engine.compute_for(&hull, pilot, CalcOptions::default());
        let info = data.type_info(ship);
        let size = info
            .and_then(|i| i.attr(attr::RIG_SIZE))
            .map_or(HullSize::Medium, HullSize::from_rig_size);
        let total = |slot: Slot| stats.slots(slot).total.unwrap_or(0) as usize;
        let totals = [Slot::High, Slot::Mid, Slot::Low, Slot::Rig]
            .into_iter()
            .map(|s| (s, total(s)))
            .collect();
        Some(Self {
            ship,
            size,
            battlecruiser: catalog
                .get(ship)
                .is_some_and(|s| BATTLECRUISER_GROUPS.contains(&s.group_id)),
            totals,
            turrets: stats.turret.total.unwrap_or(0) as usize,
            launchers: stats.launcher.total.unwrap_or(0) as usize,
            cpu: stats.cpu.output.unwrap_or(0.0),
            powergrid: stats.powergrid.output.unwrap_or(0.0),
            calibration: stats.calibration.output.unwrap_or(0.0),
            drone_bandwidth: stats.drone_bandwidth.output.unwrap_or(0.0),
            bonuses: info
                .and_then(|i| i.traits.as_ref())
                .map_or_else(Vec::new, bonus_lines),
            mastery: data
                .masteries
                .get(&ship)
                .map_or_else(Vec::new, |m| m.iter().map(|c| c.to_lowercase()).collect()),
        })
    }

    pub fn total(&self, slot: Slot) -> usize {
        self.totals.get(&slot).copied().unwrap_or(0)
    }

    /// Whether the ship's mastery names weapons at all.
    fn mastery_names_weapons(&self) -> bool {
        self.mastery
            .iter()
            .any(|c| c.contains("turret") || c.contains("missiles") || c.contains("vorton"))
    }

    /// Whether the ship's mastery asks for this kind and size of weapon.
    fn mastery_weapon(&self, kind: WeaponKind, size: HullSize) -> bool {
        let size = size.label().to_lowercase();
        let wanted = match kind {
            WeaponKind::Projectile => format!("{size} projectile turret"),
            WeaponKind::Hybrid => format!("{size} hybrid turret"),
            WeaponKind::Energy => format!("{size} energy turret"),
            WeaponKind::Precursor => format!("{size} disintegrator turret"),
            WeaponKind::Vorton => format!("{size} vorton projector"),
            WeaponKind::Missile => format!("{size} missiles"),
        };
        self.mastery.iter().any(|c| c.starts_with(&wanted))
    }

    /// The tank the ship's mastery asks for: Some(true) for shield, Some(false) for armor.
    fn mastery_shield(&self) -> Option<bool> {
        let has = |word: &str| self.mastery.iter().any(|c| c.ends_with(word));
        match (has("shield tanking"), has("armor tanking")) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        }
    }

    fn bonused(&self, phrase: &str) -> bool {
        self.bonuses.iter().any(|b| b.contains(phrase))
    }
}

/// A ship's bonus texts, lower case and without the game's markup.
fn bonus_lines(traits: &Traits) -> Vec<String> {
    traits
        .skills
        .iter()
        .flat_map(|(_, b)| b)
        .chain(&traits.role)
        .chain(&traits.misc)
        .map(|b| strip_markup(&b.text).to_lowercase())
        .collect()
}

fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// One module the wizard puts on.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub slot: Slot,
    /// The T1 item it is a variation of.
    pub base: TypeId,
    pub type_id: TypeId,
    pub charge: Option<TypeId>,
    /// Higher is kept longer when the fit has to shrink; weapons are never dropped.
    pub priority: u8,
}

const WEAPON_PRIORITY: u8 = 10;

/// Groups a fit takes one module of: damage controls and propulsion modules.
const ONE_PER_FIT: [i32; 2] = [60, 46];

/// Where each pick goes, against the fit it was planned for.
pub type Placement = (Slot, usize, Module);

/// Weapon lines the hull can mount, the ones its bonuses favour first.
pub fn families(catalog: &Catalog, data: &ItemData, facts: &Facts) -> Vec<Family> {
    Setup::new(catalog, data, facts, &[]).find_families()
}

/// What the wizard works with: the item data, the hull's facts and the weapons it can mount,
/// worked out once when the wizard opens.
pub struct Setup<'a> {
    catalog: &'a Catalog,
    data: &'a ItemData,
    pub facts: &'a Facts,
    pub families: &'a [Family],
}

impl<'a> Setup<'a> {
    pub fn new(
        catalog: &'a Catalog,
        data: &'a ItemData,
        facts: &'a Facts,
        families: &'a [Family],
    ) -> Self {
        Self {
            catalog,
            data,
            facts,
            families,
        }
    }

    fn power(&self, id: TypeId) -> f64 {
        self.attr(id, attr::POWER)
    }

    /// Weapon lines that fit the hull, the ones its bonuses favour first.
    fn find_families(&self) -> Vec<Family> {
        let facts = self.facts;
        let mut families: Vec<(Family, bool, bool, bool, f64)> = self
            .catalog
            .types
            .values()
            .filter(|t| {
                t.published
                    && t.market_group.is_some()
                    && t.kind == Kind::Module(Slot::High)
                    && t.variation_parent.is_none()
                    && t.meta_group.is_none_or(|m| m == meta::TECH1)
                    && !t.name.starts_with("Civilian")
            })
            .filter_map(|t| {
                let kind = WeaponKind::from_group(t.group_id)?;
                let (size, bonus, count) = match t.hardpoint {
                    Hardpoint::Turret => {
                        let size = HullSize::from_charge_size(t.charge_size?);
                        let stem = t.name.trim_end_matches(" I").to_lowercase();
                        let phrase = match kind {
                            WeaponKind::Projectile => "projectile turret",
                            WeaponKind::Hybrid => "hybrid turret",
                            WeaponKind::Energy => "energy turret",
                            WeaponKind::Precursor => "entropic disintegrator",
                            _ => "vorton projector",
                        };
                        let sized = format!("{} {phrase}", size.label().to_lowercase());
                        let bonus = if facts.bonused(&sized) || facts.bonused(&stem) {
                            2
                        } else {
                            u8::from(facts.bonused(phrase))
                        };
                        (size, bonus, facts.turrets)
                    }
                    Hardpoint::Launcher => {
                        let (size, phrase) = launcher(t.group_id)?;
                        let bonus = if facts.bonused(phrase) {
                            2
                        } else {
                            u8::from(facts.bonused("missile"))
                        };
                        (size, bonus, facts.launchers)
                    }
                    Hardpoint::None => return None,
                };
                let count = count.min(facts.total(Slot::High));
                // The mastery says what the ship is meant to shoot with; that outranks the
                // bonus text, and with it a hull gets nothing its race does not use, like
                // autocannons on an Abaddon.
                let bonus = if facts.mastery_weapon(kind, size) {
                    3
                } else if facts.mastery_names_weapons() {
                    return None;
                } else {
                    bonus
                };
                if count == 0 || (size != facts.size && bonus < 2) {
                    return None;
                }
                // Hulls bonused for a bigger size than their own, like attack battlecruisers,
                // get a role bonus to its powergrid need, so the base need says nothing there.
                let power = self.power(t.id) * count as f64;
                let off_size = size != facts.size;
                if power > facts.powergrid * 1.2 && !off_size {
                    return None;
                }
                let has_t2 = self
                    .catalog
                    .variations_of(t.id)
                    .iter()
                    .any(|&v| self.meta_group(v) == Some(meta::TECH2));
                let short = [
                    "autocannon",
                    "repeating",
                    "blaster",
                    "pulse",
                    "torpedo",
                    "rocket",
                ]
                .iter()
                .any(|w| t.name.to_lowercase().contains(w))
                    || t.group_id == 771;
                let within = off_size || power <= facts.powergrid * 0.8;
                Some((
                    Family {
                        base: t.id,
                        name: t.name.trim_end_matches(" I").to_owned(),
                        kind,
                        hardpoint: t.hardpoint,
                        size,
                        bonus,
                        count,
                    },
                    has_t2,
                    within,
                    short,
                    power,
                ))
            })
            .collect();
        families.sort_by(|a, b| {
            (b.0.bonus, b.1, b.2, b.3)
                .cmp(&(a.0.bonus, a.1, a.2, a.3))
                .then(b.4.total_cmp(&a.4))
                .then_with(|| a.0.name.cmp(&b.0.name))
        });
        families.into_iter().map(|f| f.0).collect()
    }

    fn meta_group(&self, id: TypeId) -> Option<i32> {
        self.catalog.get(id).and_then(|t| t.meta_group)
    }

    /// A published T1 item by name.
    fn base(&self, name: &str) -> Option<TypeId> {
        self.catalog
            .find(name)
            .filter(|t| t.published && t.market_group.is_some())
            .map(|t| t.id)
    }

    /// The variation of `base` for a tier, falling back to the nearest one there is.
    pub fn resolve(&self, base: TypeId, tier: Tier) -> TypeId {
        let variations: Vec<&ItemType> = self
            .catalog
            .variations_of(base)
            .iter()
            .filter_map(|&v| self.catalog.get(v))
            .filter(|t| t.published && t.market_group.is_some())
            .collect();
        let lowest = |group: i32| {
            variations
                .iter()
                .filter(|t| t.meta_group == Some(group))
                .min_by_key(|t| (t.meta_level, t.id))
                .map(|t| t.id)
        };
        let meta = || {
            variations
                .iter()
                .filter(|t| t.meta_group == Some(meta::TECH1) && t.id != base)
                .max_by_key(|t| (t.meta_level, -t.id))
                .map(|t| t.id)
        };
        match tier {
            Tier::T2 => lowest(meta::TECH2).or_else(meta),
            Tier::Faction => lowest(meta::FACTION).or_else(|| lowest(meta::TECH2)),
            Tier::Meta => meta(),
            Tier::T1 => None,
        }
        .unwrap_or(base)
    }

    /// The charge a weapon is loaded with: the hardest hitting faction or T1 one.
    pub fn ammo(&self, weapon: TypeId, ammo: Ammo) -> Option<TypeId> {
        let group = match ammo {
            Ammo::Faction => meta::FACTION,
            Ammo::Standard => meta::TECH1,
            Ammo::None => return None,
        };
        let module = self.catalog.get(weapon)?;
        let damage = |t: &ItemType| t.damage.iter().sum::<f64>();
        self.catalog
            .types
            .values()
            .filter(|t| {
                t.published
                    && t.market_group.is_some()
                    && t.meta_group.unwrap_or(meta::TECH1) == group
                    && t.fits_in(module)
            })
            // Navy ammo before pirate ammo, then the hardest hitting.
            .max_by(|a, b| {
                b.meta_level
                    .cmp(&a.meta_level)
                    .then(damage(a).total_cmp(&damage(b)))
                    .then_with(|| b.name.cmp(&a.name))
            })
            .map(|t| t.id)
    }

    /// The biggest navy cap booster charge that goes into a capacitor booster.
    fn cap_charge(&self, booster: TypeId) -> Option<TypeId> {
        let module = self.catalog.get(booster)?;
        self.catalog
            .types
            .values()
            .filter(|t| {
                t.published
                    && t.name.starts_with("Navy Cap Booster")
                    && t.fits_in(module)
                    && t.volume <= module.capacity
            })
            .max_by(|a, b| a.volume.total_cmp(&b.volume))
            .map(|t| t.id)
    }

    /// What the wizard starts with for this hull: its favourite weapon, a T2 MWD, scram and web,
    /// the tank its bonuses and slots suggest, and matching damage modules and rigs.
    pub fn defaults(&self) -> Choices {
        let facts = self.facts;
        let lines = |word: &str| {
            facts
                .bonuses
                .iter()
                .filter(|b| b.contains(word) && !b.contains("remote"))
                .count()
        };
        let (shield, armor) = (lines("shield"), lines("armor"));
        let shield_tank = facts
            .mastery_shield()
            .unwrap_or_else(|| match shield.cmp(&armor) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => facts.total(Slot::Mid) > facts.total(Slot::Low),
            });
        let active = facts.bonuses.iter().any(|b| {
            !b.contains("remote")
                && if shield_tank {
                    b.contains("shield boost")
                } else {
                    b.contains("armor repair")
                }
        });
        let tank = match (shield_tank, active) {
            (true, false) => Tank::ShieldBuffer,
            (true, true) => Tank::ShieldActive,
            (false, false) => Tank::ArmorBuffer,
            (false, true) => Tank::ArmorActive,
        };
        let drones = facts.drone_bandwidth >= 25.0
            && facts
                .bonuses
                .iter()
                .any(|b| b.contains("drone") && b.contains("damage"));
        let family = (!self.families.is_empty()).then_some(0);
        let mut utilities = Vec::new();
        if facts.size != HullSize::Capital {
            let mids = facts.total(Slot::Mid);
            if mids >= 2 {
                utilities.push(Utility::Scram);
            }
            if mids >= 3 {
                utilities.push(Utility::Web);
            }
        }
        Choices {
            family,
            tier: Tier::T2,
            ammo: Ammo::Faction,
            prop: if facts.size == HullSize::Capital {
                Prop::None
            } else {
                Prop::Microwarpdrive
            },
            utilities,
            tank,
            damage: if drones || family.is_none() && facts.drone_bandwidth > 0.0 {
                DamageMods::Drone
            } else if family.is_some() {
                DamageMods::Weapon
            } else {
                DamageMods::None
            },
            rigs: if tank == Tank::None {
                Rigs::Damage
            } else {
                Rigs::Tank
            },
            clear: false,
        }
    }

    pub fn family(&self, choices: &Choices) -> Option<&Family> {
        choices.family.and_then(|i| self.families.get(i))
    }

    /// The T1 name of a utility module for this hull and weapon.
    pub fn utility_name(&self, utility: Utility, kind: Option<WeaponKind>) -> String {
        let size = self.facts.size;
        let kind = kind.unwrap_or(WeaponKind::Projectile);
        match utility {
            Utility::Scram => "Warp Scrambler I".into(),
            Utility::Point => "Warp Disruptor I".into(),
            Utility::Web => "Stasis Webifier I".into(),
            Utility::Grappler => "Heavy Stasis Grappler I".into(),
            Utility::Neut => format!("{} Energy Neutralizer I", size.energy_word()),
            Utility::Nos => format!("{} Energy Nosferatu I", size.energy_word()),
            Utility::SensorBooster => "Sensor Booster I".into(),
            Utility::Computer => kind.computer().into(),
            Utility::CapBooster => format!("{} Capacitor Booster I", size.energy_word()),
            Utility::Enhancer => kind.enhancer().into(),
        }
    }

    pub fn prop_name(&self, prop: Prop) -> Option<String> {
        let (ab, mwd) = match self.facts.size {
            HullSize::Small => ("1MN", "5MN"),
            HullSize::Medium => ("10MN", "50MN"),
            HullSize::Large => ("100MN", "500MN"),
            HullSize::Capital => ("10000MN", "50000MN"),
        };
        match prop {
            Prop::Microwarpdrive => Some(format!("{mwd} Microwarpdrive I")),
            Prop::Afterburner => Some(format!("{ab} Afterburner I")),
            Prop::None => None,
        }
    }

    pub fn damage_mod_name(&self, damage: DamageMods, kind: Option<WeaponKind>) -> Option<&str> {
        match damage {
            DamageMods::Weapon => kind.map(WeaponKind::damage_mod),
            DamageMods::Drone => Some("Drone Damage Amplifier I"),
            DamageMods::None => None,
        }
    }

    /// Rigs for the choices, without the size word.
    fn rig_names(choices: &Choices, kind: Option<WeaponKind>) -> Vec<&'static str> {
        let tank: &[&str] = match choices.tank {
            Tank::ShieldBuffer => &["Core Defense Field Extender I"],
            Tank::ShieldActive => &["Core Defense Operational Solidifier I"],
            Tank::ArmorBuffer => &["Trimark Armor Pump I"],
            Tank::ArmorActive => &["Auxiliary Nano Pump I"],
            Tank::None => &[],
        };
        let damage = match choices.damage {
            DamageMods::Drone => &[][..],
            _ => kind.map_or(&[][..], WeaponKind::rigs),
        };
        match choices.rigs {
            Rigs::Tank if !tank.is_empty() => tank.to_vec(),
            Rigs::Tank | Rigs::Damage if !damage.is_empty() => damage.to_vec(),
            Rigs::Damage => tank.to_vec(),
            Rigs::Tank | Rigs::None => Vec::new(),
        }
    }

    /// Free positions of each rack in `fit`, all of them when the racks are cleared first.
    fn free(&self, fit: &Fit, slot: Slot, clear: bool) -> Vec<usize> {
        let total = self.facts.total(slot);
        (0..total)
            .filter(|&i| {
                clear
                    || match slot {
                        Slot::Rig => fit.rigs.get(i).is_none_or(Option::is_none),
                        _ => fit.rack(slot).get(i).is_none_or(Option::is_none),
                    }
            })
            .collect()
    }

    /// What the choices put on `fit`, best first within each rack.
    pub fn plan(&self, fit: &Fit, choices: &Choices) -> Vec<Pick> {
        let catalog = self.catalog;
        let facts = self.facts;
        let clear = choices.clear;
        let free_high = self.free(fit, Slot::High, clear).len();
        let free_mid = self.free(fit, Slot::Mid, clear).len();
        let free_low = self.free(fit, Slot::Low, clear).len();
        let family = self.family(choices);
        let kind = family.map(|f| f.kind);
        let mut picks = Vec::new();
        let group = |id: TypeId| catalog.get(id).map(|t| t.group_id);
        let push = |picks: &mut Vec<Pick>, slot: Slot, name: &str, tier: Tier, priority| -> bool {
            let Some(base) = self.base(name) else {
                return false;
            };
            // One damage control and one propulsion module per fit.
            if let Some(g) = group(base).filter(|g| ONE_PER_FIT.contains(g)) {
                let fitted = !clear && fit.modules().any(|m| group(m.type_id) == Some(g));
                if fitted || picks.iter().any(|p| group(p.base) == Some(g)) {
                    return false;
                }
            }
            let type_id = self.resolve(base, tier);
            let charge = if name.contains("Capacitor Booster") {
                self.cap_charge(type_id)
            } else {
                None
            };
            picks.push(Pick {
                slot,
                base,
                type_id,
                charge,
                priority,
            });
            true
        };

        // Highs: weapons on every free hardpoint, then a neut and a nos.
        let mut highs_left = free_high;
        if let Some(family) = family {
            let used = if clear {
                0
            } else {
                fit.high
                    .iter()
                    .flatten()
                    .filter(|m| {
                        catalog.get(m.type_id).map(|t| t.hardpoint) == Some(family.hardpoint)
                    })
                    .count()
            };
            let hardpoints = match family.hardpoint {
                Hardpoint::Turret => facts.turrets,
                _ => facts.launchers,
            };
            let count = hardpoints.saturating_sub(used).min(highs_left);
            let type_id = self.resolve(family.base, choices.tier);
            let charge = self.ammo(type_id, choices.ammo);
            for _ in 0..count {
                picks.push(Pick {
                    slot: Slot::High,
                    base: family.base,
                    type_id,
                    charge,
                    priority: WEAPON_PRIORITY,
                });
            }
            highs_left -= count;
        }
        for utility in [Utility::Neut, Utility::Nos] {
            if choices.has(utility) && highs_left > 0 {
                let name = self.utility_name(utility, kind);
                push(&mut picks, Slot::High, &name, Tier::T2, utility.priority());
                highs_left -= 1;
            }
        }

        // Mids: propulsion, tackle and utilities, then the shield tank in what is left.
        let mut mids_left = free_mid;
        if let Some(name) = self.prop_name(choices.prop)
            && mids_left > 0
        {
            push(&mut picks, Slot::Mid, &name, Tier::T2, 9);
            mids_left -= 1;
        }
        for &utility in &choices.utilities {
            if utility.slot() == Slot::Mid && mids_left > 0 {
                let name = self.utility_name(utility, kind);
                push(&mut picks, Slot::Mid, &name, Tier::T2, utility.priority());
                mids_left -= 1;
            }
        }
        let size = facts.size;
        let large = size >= HullSize::Large || facts.battlecruiser;
        let hardener = "Multispectrum Shield Hardener I";
        match choices.tank {
            Tank::ShieldBuffer => {
                let extender = match size {
                    HullSize::Small => "Medium Shield Extender I",
                    HullSize::Medium | HullSize::Large => "Large Shield Extender I",
                    HullSize::Capital => "Capital Shield Extender I",
                };
                for i in 0..mids_left {
                    let (name, priority) = match i {
                        1 | 4 => (hardener, 5),
                        0 => (extender, 7),
                        _ => (extender, 4),
                    };
                    push(&mut picks, Slot::Mid, name, Tier::T2, priority);
                }
            }
            Tank::ShieldActive => {
                let booster = match size {
                    HullSize::Small => "Small Shield Booster I",
                    HullSize::Medium if !large => "Medium Shield Booster I",
                    HullSize::Medium => "Large Shield Booster I",
                    HullSize::Large => "X-Large Shield Booster I",
                    HullSize::Capital => "Capital Shield Booster I",
                };
                let cap_booster = self.utility_name(Utility::CapBooster, kind);
                for i in 0..mids_left {
                    let (name, priority) = match i {
                        0 => (booster, 7),
                        2 if !choices.has(Utility::CapBooster) => (cap_booster.as_str(), 4),
                        _ => (hardener, 5),
                    };
                    push(&mut picks, Slot::Mid, name, Tier::T2, priority);
                }
            }
            Tank::ArmorActive if mids_left > 0 && !choices.has(Utility::CapBooster) => {
                let name = self.utility_name(Utility::CapBooster, kind);
                push(&mut picks, Slot::Mid, &name, Tier::T2, 4);
            }
            _ => {}
        }

        // Lows: damage modules, an enhancer, the armor tank or a damage control.
        let enhancer = usize::from(choices.has(Utility::Enhancer) && free_low > 0);
        let damage_mod = self.damage_mod_name(choices.damage, kind);
        let damage_count = match (damage_mod, choices.tank.armor()) {
            (None, _) => 0,
            (Some(_), true) => free_low.saturating_sub(3 + enhancer).min(3),
            (Some(_), false) => free_low.saturating_sub(enhancer).min(3),
        };
        if let Some(name) = damage_mod {
            for i in 0..damage_count {
                push(
                    &mut picks,
                    Slot::Low,
                    name,
                    Tier::T2,
                    if i == 0 { 6 } else { 5 },
                );
            }
        }
        if enhancer > 0 {
            let name = self.utility_name(Utility::Enhancer, kind);
            push(
                &mut picks,
                Slot::Low,
                &name,
                Tier::T2,
                Utility::Enhancer.priority(),
            );
        }
        let mut lows_left = free_low - damage_count - enhancer;
        let membrane = "Multispectrum Energized Membrane I";
        let control = "Damage Control I";
        match choices.tank {
            Tank::ArmorBuffer => {
                let plate = match size {
                    HullSize::Small => "400mm Steel Plates I",
                    HullSize::Medium if !large => "800mm Steel Plates I",
                    HullSize::Medium | HullSize::Large => "1600mm Steel Plates I",
                    HullSize::Capital => "25000mm Steel Plates I",
                };
                for i in 0..lows_left {
                    let (name, priority) = match i {
                        0 => (plate, 7),
                        2 => (control, 6),
                        _ if i % 2 == 1 => (membrane, 5),
                        _ => (plate, 4),
                    };
                    // A fit that has its damage control gets a membrane instead.
                    if !push(&mut picks, Slot::Low, name, Tier::T2, priority) {
                        push(&mut picks, Slot::Low, membrane, Tier::T2, 5);
                    }
                }
                lows_left = 0;
            }
            Tank::ArmorActive => {
                let repairer = format!("{} Armor Repairer I", size.label());
                for i in 0..lows_left {
                    let (name, priority) = match i {
                        0 => (repairer.as_str(), 7),
                        2 => (repairer.as_str(), 4),
                        3 => (control, 6),
                        _ => (membrane, 5),
                    };
                    if !push(&mut picks, Slot::Low, name, Tier::T2, priority) {
                        push(&mut picks, Slot::Low, membrane, Tier::T2, 5);
                    }
                }
                lows_left = 0;
            }
            Tank::ShieldBuffer | Tank::ShieldActive if lows_left > 0 => {
                if push(&mut picks, Slot::Low, control, Tier::T2, 6) {
                    lows_left -= 1;
                }
            }
            _ => {}
        }
        // A fourth damage module where nothing else wants the slot.
        if let Some(name) = damage_mod
            && lows_left > 0
            && damage_count == 3
        {
            push(&mut picks, Slot::Low, name, Tier::T2, 2);
        }

        // Rigs, as many as the calibration takes.
        let rigs = Self::rig_names(choices, kind);
        let free_rigs = self.free(fit, Slot::Rig, clear).len();
        let mut calibration = facts.calibration
            - if clear {
                0.0
            } else {
                fit.rigs.iter().flatten().map(|&r| self.cost(r)).sum()
            };
        for i in 0..free_rigs {
            let Some(name) = rigs.get(i % rigs.len().max(1)) else {
                break;
            };
            let name = format!("{} {name}", size.label());
            let Some(base) = self.base(&name) else { break };
            let cost = self.cost(base);
            if cost > calibration + 1e-6 {
                break;
            }
            calibration -= cost;
            picks.push(Pick {
                slot: Slot::Rig,
                base,
                type_id: base,
                charge: None,
                priority: 3,
            });
        }
        picks
    }

    fn cost(&self, rig: TypeId) -> f64 {
        self.attr(rig, attr::UPGRADE_COST)
    }

    /// A type's base attribute, zero when it has none.
    fn attr(&self, id: TypeId, attr: i32) -> f64 {
        self.data
            .type_info(id)
            .and_then(|i| i.attr(attr))
            .unwrap_or(0.0)
    }

    /// The picks at the free positions of `fit`.
    pub fn placements(&self, fit: &Fit, picks: &[Pick], clear: bool) -> Vec<Placement> {
        let mut out = Vec::new();
        for slot in [Slot::High, Slot::Mid, Slot::Low, Slot::Rig] {
            let positions = self.free(fit, slot, clear);
            for (pick, i) in picks.iter().filter(|p| p.slot == slot).zip(positions) {
                let mut module = Module::new(self.catalog, pick.type_id);
                module.charge = pick.charge;
                out.push((slot, i, module));
            }
        }
        out
    }

    /// Step modules down to their meta variations, then drop the least wanted ones, until the
    /// fit's CPU and powergrid are within the ship's output. Weapons are stepped down last and
    /// never dropped.
    pub fn make_it_fit(
        &self,
        engine: &Engine,
        pilot: &Pilot,
        fit: &Fit,
        picks: &[Pick],
        clear: bool,
    ) -> Vec<Pick> {
        let mut picks = picks.to_vec();
        for _ in 0..64 {
            let stats = preview(
                engine,
                pilot,
                fit,
                &self.placements(fit, &picks, clear),
                clear,
            );
            if fits(&stats) {
                break;
            }
            // How much of each is missing, and what a change gives back of it, by the items'
            // base needs: the engine only checks the result.
            let over = |r: &Resource| (r.used - r.output.unwrap_or(f64::INFINITY)).max(0.0);
            let (cpu_over, pg_over) = (over(&stats.cpu), over(&stats.powergrid));
            // The share of the overuse a change makes up for, of whichever is short most.
            let relief = |from: TypeId, to: Option<TypeId>| {
                let need = |id: Option<TypeId>, a| id.map_or(0.0, |id| self.attr(id, a));
                let saves = |a| need(Some(from), a) - need(to, a);
                let share = |over: f64, a| if over > 0.0 { saves(a) / over } else { 0.0 };
                share(cpu_over, attr::CPU).max(share(pg_over, attr::POWER))
            };
            let helps = |from: TypeId, to: Option<TypeId>| relief(from, to) > 0.0;
            // The least wanted kind of module that has a lighter variation.
            let step = picks
                .iter()
                .filter(|p| p.slot != Slot::Rig)
                .filter(|p| {
                    let meta = self.resolve(p.base, Tier::Meta);
                    meta != p.type_id && helps(p.type_id, Some(meta))
                })
                .min_by_key(|p| p.priority)
                .map(|p| p.base);
            if let Some(base) = step {
                let meta = self.resolve(base, Tier::Meta);
                for p in picks.iter_mut().filter(|p| p.base == base) {
                    p.type_id = meta;
                    // A lighter launcher or gun takes the same charges.
                    if p.charge.is_some_and(|c| {
                        let (c, m) = (self.catalog.get(c), self.catalog.get(meta));
                        !c.zip(m).is_some_and(|(c, m)| c.fits_in(m))
                    }) {
                        p.charge = None;
                    }
                }
                continue;
            }
            // Then the least wanted module whose removal makes up for a good part of it, so a
            // rack is not emptied of small fry first.
            let droppable: Vec<(usize, &Pick, f64)> = picks
                .iter()
                .enumerate()
                .filter(|(_, p)| p.priority < WEAPON_PRIORITY && p.slot != Slot::Rig)
                .map(|(i, p)| (i, p, relief(p.type_id, None)))
                .filter(|&(_, _, r)| r > 0.0)
                .collect();
            let most = droppable.iter().map(|d| d.2).fold(0.0, f64::max);
            let drop = droppable
                .iter()
                .filter(|d| d.2 >= 0.2_f64.min(most))
                .min_by(|a, b| a.1.priority.cmp(&b.1.priority).then(b.2.total_cmp(&a.2)))
                .map(|d| d.0);
            match drop {
                Some(i) => {
                    picks.remove(i);
                }
                None => break,
            }
        }
        picks
    }
}

/// `fit` with the placements made, the racks emptied first if `clear`.
pub fn apply(fit: &mut Fit, placements: &[Placement], clear: bool) {
    if clear {
        fit.high.clear();
        fit.mid.clear();
        fit.low.clear();
        fit.rigs.clear();
    }
    for (slot, i, module) in placements {
        if *slot == Slot::Rig {
            if fit.rigs.len() <= *i {
                fit.rigs.resize(i + 1, None);
            }
            fit.rigs[*i] = Some(module.type_id);
        } else {
            let rack = fit.rack_mut(*slot);
            if rack.len() <= *i {
                rack.resize_with(i + 1, || None);
            }
            rack[*i] = Some(module.clone());
        }
    }
}

/// The stats `fit` would have with the placements made.
pub fn preview(
    engine: &Engine,
    pilot: &Pilot,
    fit: &Fit,
    placements: &[Placement],
    clear: bool,
) -> Stats {
    let mut next = fit.clone();
    apply(&mut next, placements, clear);
    engine.compute_for(&next, pilot, CalcOptions::default())
}

/// Whether CPU, powergrid and calibration are within the ship's output.
pub fn fits(stats: &Stats) -> bool {
    [&stats.cpu, &stats.powergrid, &stats.calibration]
        .into_iter()
        .all(|r| r.output.is_none_or(|out| r.used <= out + 1e-6))
}
