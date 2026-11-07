//! The fitting wizard's picks: which weapons, propulsion, tackle, tank, damage modules and rigs
//! suit a hull, and where they go. No UI here; the wizard dialog shows and applies what this
//! works out.
//!
//! Items are found by their T1 name and resolved to a tier through their variations, so a new
//! meta item or a renamed faction one is picked up without a list of type IDs.

mod bling;
pub mod tank;

use std::collections::HashMap;

pub use bling::{Bling, Category, Tier};

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
    pub const STORYLINE: i32 = 3;
    pub const FACTION: i32 = 4;
    pub const OFFICER: i32 = 5;
    pub const DEADSPACE: i32 = 6;
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

    pub fn category(self) -> Category {
        match self {
            Self::Scram | Self::Point | Self::Web | Self::Grappler => Category::Tackle,
            Self::Neut | Self::Nos => Category::Energy,
            Self::SensorBooster | Self::Computer | Self::CapBooster | Self::Enhancer => {
                Category::Electronics
            }
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

/// How the ship tanks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TankStyle {
    /// Plates or extenders with resists.
    Buffer,
    /// Repairers or boosters with resists.
    Active,
    /// Nothing but plates or extenders.
    Brick,
    None,
}

impl TankStyle {
    pub const ALL: [Self; 4] = [Self::Buffer, Self::Active, Self::Brick, Self::None];

    pub fn label(self) -> &'static str {
        match self {
            Self::Buffer => "Buffer",
            Self::Active => "Active",
            Self::Brick => "Brick",
            Self::None => "None",
        }
    }
}

/// Which layer takes the damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Shield,
    Armor,
}

impl Layer {
    pub const ALL: [Self; 2] = [Self::Shield, Self::Armor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Shield => "Shield",
            Self::Armor => "Armor",
        }
    }

    /// The rack the layer's modules go in.
    pub fn slot(self) -> Slot {
        match self {
            Self::Shield => Slot::Mid,
            Self::Armor => Slot::Low,
        }
    }
}

/// What a tank is tuned for. Active tanks pick from resists, repair or both; buffer and brick
/// tanks from balanced or the most raw effective HP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Resists,
    Repair,
    Balanced,
    Ehp,
}

impl Goal {
    const ACTIVE: [Self; 3] = [Self::Resists, Self::Repair, Self::Balanced];
    const BUFFER: [Self; 2] = [Self::Balanced, Self::Ehp];

    /// The goals that make sense for a tank style, in the order they are offered.
    pub fn for_style(style: TankStyle) -> &'static [Self] {
        match style {
            TankStyle::Active => &Self::ACTIVE,
            _ => &Self::BUFFER,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Resists => "Max resists",
            Self::Repair => "Max repair",
            Self::Balanced => "Balanced",
            Self::Ehp => "Max EHP",
        }
    }
}

/// A tank the optimiser found, by the T1 items of its modules and rigs; the bling choices decide
/// which variation of each goes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TankFit {
    pub modules: Vec<TypeId>,
    pub rigs: Vec<TypeId>,
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
    pub ammo: Ammo,
    pub prop: Prop,
    /// In [`Utility::ALL`] order.
    pub utilities: Vec<Utility>,
    pub tank: TankStyle,
    pub layer: Layer,
    pub goal: Goal,
    /// The optimiser's pick for the tank; None lays out the usual tank for the style.
    pub tank_fit: Option<TankFit>,
    pub damage: DamageMods,
    pub rigs: Rigs,
    /// Empty the module racks and rigs first, instead of filling the free slots.
    pub clear: bool,
    pub bling: Bling,
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
    /// Part of the tank, which the tank section lists and the optimiser replaces.
    pub tank: bool,
}

const WEAPON_PRIORITY: u8 = 10;

/// Item groups the wizard tells apart.
mod group {
    pub const CAP_BOOSTER: i32 = 76;
    pub const ANCILLARY_SHIELD_BOOSTER: i32 = 1156;
    pub const ANCILLARY_ARMOR_REPAIRER: i32 = 1199;
}

/// What the tank has to work with once everything else is planned.
#[derive(Debug, Clone, Copy)]
struct Room {
    /// Free slots in the tank's rack.
    slots: usize,
    /// Free rig slots, when the tank takes the rigs.
    rigs: usize,
    calibration: f64,
}

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

    /// Published market variations of `base`, itself included, cheapest tier first.
    pub fn variants(&self, base: TypeId) -> Vec<TypeId> {
        let mut variations: Vec<&ItemType> = self
            .catalog
            .variations_of(base)
            .iter()
            .filter_map(|&v| self.catalog.get(v))
            .filter(|t| t.published && t.market_group.is_some())
            .collect();
        if variations.is_empty() {
            return vec![base];
        }
        variations.sort_by_key(|t| (Tier::of(self.catalog, t.id, base), t.meta_level, t.id));
        variations.into_iter().map(|t| t.id).collect()
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
        let t2 = || lowest(meta::TECH2).or_else(meta);
        let faction = || {
            lowest(meta::FACTION)
                .or_else(|| lowest(meta::STORYLINE))
                .or_else(t2)
        };
        let deadspace = || lowest(meta::DEADSPACE).or_else(faction);
        match tier {
            Tier::T1 => None,
            Tier::Meta => meta(),
            Tier::T2 => t2(),
            Tier::Faction => faction(),
            Tier::Deadspace => deadspace(),
            Tier::Officer => lowest(meta::OFFICER).or_else(deadspace),
        }
        .unwrap_or(base)
    }

    /// The variation of `base` the bling choices ask for: the exact one picked for it, or the
    /// tier of its kind.
    pub fn pick(&self, base: TypeId, category: Category, bling: &Bling) -> TypeId {
        bling
            .exact(category, base)
            .filter(|&id| id == base || self.catalog.variations_of(base).contains(&id))
            .unwrap_or_else(|| self.resolve(base, bling.tier_for(category)))
    }

    /// What a module other than a weapon is loaded with: cap booster charges, and nanite paste
    /// for ancillary armor repairers.
    fn charge_for(&self, id: TypeId) -> Option<TypeId> {
        let module = self.catalog.get(id)?;
        match module.group_id {
            group::CAP_BOOSTER | group::ANCILLARY_SHIELD_BOOSTER => self.cap_charge(id),
            group::ANCILLARY_ARMOR_REPAIRER => self
                .catalog
                .types
                .values()
                .find(|t| t.published && t.name == "Nanite Repair Paste" && t.fits_in(module))
                .map(|t| t.id),
            _ => None,
        }
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
            ammo: Ammo::Faction,
            prop: if facts.size == HullSize::Capital {
                Prop::None
            } else {
                Prop::Microwarpdrive
            },
            utilities,
            tank: if active {
                TankStyle::Active
            } else {
                TankStyle::Buffer
            },
            layer: if shield_tank {
                Layer::Shield
            } else {
                Layer::Armor
            },
            goal: Goal::Balanced,
            tank_fit: None,
            damage: if drones || family.is_none() && facts.drone_bandwidth > 0.0 {
                DamageMods::Drone
            } else if family.is_some() {
                DamageMods::Weapon
            } else {
                DamageMods::None
            },
            rigs: Rigs::Tank,
            clear: false,
            bling: Bling::default(),
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

    /// Whether the hull counts as large for its tank modules: battlecruisers take large
    /// extenders, plates and boosters.
    fn large(&self) -> bool {
        self.facts.size >= HullSize::Large || self.facts.battlecruiser
    }

    /// The plate a hull this size takes.
    fn plate_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "400mm Steel Plates I",
            HullSize::Medium if !self.large() => "800mm Steel Plates I",
            HullSize::Medium | HullSize::Large => "1600mm Steel Plates I",
            HullSize::Capital => "25000mm Steel Plates I",
        }
    }

    fn extender_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "Medium Shield Extender I",
            HullSize::Medium | HullSize::Large => "Large Shield Extender I",
            HullSize::Capital => "Capital Shield Extender I",
        }
    }

    fn repairer_name(&self) -> String {
        format!("{} Armor Repairer I", self.facts.size.label())
    }

    fn booster_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "Small Shield Booster I",
            HullSize::Medium if !self.large() => "Medium Shield Booster I",
            HullSize::Medium => "Large Shield Booster I",
            HullSize::Large => "X-Large Shield Booster I",
            HullSize::Capital => "Capital Shield Booster I",
        }
    }

    /// The ancillary repairer or booster for the layer.
    fn ancillary_name(&self, layer: Layer) -> String {
        match layer {
            Layer::Armor => format!("{} Ancillary Armor Repairer", self.facts.size.label()),
            Layer::Shield => {
                let size = match self.facts.size {
                    HullSize::Small => "Small",
                    HullSize::Medium if !self.large() => "Medium",
                    HullSize::Medium => "Large",
                    HullSize::Large => "X-Large",
                    HullSize::Capital => "Capital",
                };
                format!("{size} Ancillary Shield Booster")
            }
        }
    }

    /// Whether the rig slots go to the tank: when tank rigs are asked for, or damage rigs are and
    /// the weapons have none.
    pub fn tank_takes_rigs(choices: &Choices, kind: Option<WeaponKind>) -> bool {
        let damage = match choices.damage {
            DamageMods::Drone => &[][..],
            _ => kind.map_or(&[][..], WeaponKind::rigs),
        };
        choices.tank != TankStyle::None
            && match choices.rigs {
                Rigs::Tank => true,
                Rigs::Damage => damage.is_empty(),
                Rigs::None => false,
            }
    }

    /// The usual tank rigs for the style, without the size word.
    fn tank_rig_names(choices: &Choices) -> &'static [&'static str] {
        match (choices.tank, choices.layer) {
            (TankStyle::None, _) => &[],
            (TankStyle::Active, Layer::Shield) => &["Core Defense Operational Solidifier I"],
            (TankStyle::Active, Layer::Armor) => &["Auxiliary Nano Pump I"],
            (_, Layer::Shield) => &["Core Defense Field Extender I"],
            (_, Layer::Armor) => &["Trimark Armor Pump I"],
        }
    }

    /// Damage rigs for the choices, without the size word; empty when the tank takes the rigs.
    fn damage_rig_names(choices: &Choices, kind: Option<WeaponKind>) -> &'static [&'static str] {
        if choices.rigs == Rigs::None || Self::tank_takes_rigs(choices, kind) {
            return &[];
        }
        match choices.damage {
            DamageMods::Drone => &[],
            _ => kind.map_or(&[], WeaponKind::rigs),
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

    /// Calibration left for new rigs.
    fn calibration_left(&self, fit: &Fit, clear: bool) -> f64 {
        self.facts.calibration
            - if clear {
                0.0
            } else {
                fit.rigs.iter().flatten().map(|&r| self.cost(r)).sum()
            }
    }

    /// What the choices put on `fit`, best first within each rack.
    pub fn plan(&self, fit: &Fit, choices: &Choices) -> Vec<Pick> {
        let (mut picks, room) = self.plan_base(fit, choices);
        picks.extend(self.plan_tank(fit, choices, &room, &picks));
        picks
    }

    /// One pick of the item named `name`, at the variation the bling choices ask for. None when
    /// the item is unknown, or is a damage control or propulsion module and the fit has one.
    fn pick_named(
        &self,
        fit: &Fit,
        choices: &Choices,
        picks: &[Pick],
        (slot, name, category, priority): (Slot, &str, Category, u8),
    ) -> Option<Pick> {
        let base = self.base(name)?;
        self.pick_base(fit, choices, picks, (slot, base, category, priority))
    }

    fn pick_base(
        &self,
        fit: &Fit,
        choices: &Choices,
        picks: &[Pick],
        (slot, base, category, priority): (Slot, TypeId, Category, u8),
    ) -> Option<Pick> {
        let group = |id: TypeId| self.catalog.get(id).map(|t| t.group_id);
        // One damage control and one propulsion module per fit.
        if let Some(g) = group(base).filter(|g| ONE_PER_FIT.contains(g)) {
            let fitted = !choices.clear && fit.modules().any(|m| group(m.type_id) == Some(g));
            if fitted || picks.iter().any(|p| group(p.base) == Some(g)) {
                return None;
            }
        }
        let type_id = self.pick(base, category, &choices.bling);
        Some(Pick {
            slot,
            base,
            type_id,
            charge: self.charge_for(type_id),
            priority,
            tank: false,
        })
    }

    /// Everything but the tank: weapons, propulsion, tackle, utilities, damage modules and damage
    /// rigs, with what is left for the tank.
    fn plan_base(&self, fit: &Fit, choices: &Choices) -> (Vec<Pick>, Room) {
        let catalog = self.catalog;
        let facts = self.facts;
        let clear = choices.clear;
        let free_high = self.free(fit, Slot::High, clear).len();
        let free_mid = self.free(fit, Slot::Mid, clear).len();
        let free_low = self.free(fit, Slot::Low, clear).len();
        let family = self.family(choices);
        let kind = family.map(|f| f.kind);
        let tank = choices.tank != TankStyle::None;
        let armor = tank && choices.layer == Layer::Armor;
        let mut picks = Vec::new();
        let push = |picks: &mut Vec<Pick>, slot: Slot, name: &str, category, priority| -> bool {
            match self.pick_named(fit, choices, picks, (slot, name, category, priority)) {
                Some(pick) => {
                    picks.push(pick);
                    true
                }
                None => false,
            }
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
            let type_id = self.pick(family.base, Category::Weapon, &choices.bling);
            let charge = self.ammo(type_id, choices.ammo);
            for _ in 0..count {
                picks.push(Pick {
                    slot: Slot::High,
                    base: family.base,
                    type_id,
                    charge,
                    priority: WEAPON_PRIORITY,
                    tank: false,
                });
            }
            highs_left -= count;
        }
        for utility in [Utility::Neut, Utility::Nos] {
            if choices.has(utility) && highs_left > 0 {
                let name = self.utility_name(utility, kind);
                push(
                    &mut picks,
                    Slot::High,
                    &name,
                    utility.category(),
                    utility.priority(),
                );
                highs_left -= 1;
            }
        }

        // Mids: propulsion, tackle and utilities; a cap booster for an armor repairer.
        let mut mids_left = free_mid;
        if let Some(name) = self.prop_name(choices.prop)
            && mids_left > 0
            && push(&mut picks, Slot::Mid, &name, Category::Propulsion, 9)
        {
            mids_left -= 1;
        }
        for &utility in &choices.utilities {
            if utility.slot() == Slot::Mid && mids_left > 0 {
                let name = self.utility_name(utility, kind);
                push(
                    &mut picks,
                    Slot::Mid,
                    &name,
                    utility.category(),
                    utility.priority(),
                );
                mids_left -= 1;
            }
        }
        if armor
            && choices.tank == TankStyle::Active
            && mids_left > 0
            && !choices.has(Utility::CapBooster)
        {
            let name = self.utility_name(Utility::CapBooster, kind);
            if push(&mut picks, Slot::Mid, &name, Category::Electronics, 4) {
                mids_left -= 1;
            }
        }

        // Lows: damage modules and an enhancer; a damage control under a shield tank.
        let enhancer = usize::from(choices.has(Utility::Enhancer) && free_low > 0);
        let damage_mod = self.damage_mod_name(choices.damage, kind);
        let damage_count = match (damage_mod, armor) {
            (None, _) => 0,
            (Some(_), true) => free_low.saturating_sub(3 + enhancer).min(3),
            (Some(_), false) => free_low.saturating_sub(enhancer).min(3),
        };
        if let Some(name) = damage_mod {
            for i in 0..damage_count {
                let priority = if i == 0 { 6 } else { 5 };
                push(&mut picks, Slot::Low, name, Category::Damage, priority);
            }
        }
        if enhancer > 0 {
            let name = self.utility_name(Utility::Enhancer, kind);
            push(
                &mut picks,
                Slot::Low,
                &name,
                Category::Electronics,
                Utility::Enhancer.priority(),
            );
        }
        let mut lows_left = free_low - damage_count - enhancer;
        if tank
            && !armor
            && lows_left > 0
            && push(
                &mut picks,
                Slot::Low,
                "Damage Control I",
                Category::Hardener,
                6,
            )
        {
            lows_left -= 1;
        }
        // A fourth damage module where nothing else wants the slot.
        if let Some(name) = damage_mod
            && !armor
            && lows_left > 0
            && damage_count == 3
            && push(&mut picks, Slot::Low, name, Category::Damage, 2)
        {
            lows_left -= 1;
        }

        // Damage rigs, as many as the calibration takes.
        let mut calibration = self.calibration_left(fit, clear);
        let mut rigs_left = self.free(fit, Slot::Rig, clear).len();
        let names = Self::damage_rig_names(choices, kind);
        let bases: Vec<TypeId> = (0..rigs_left)
            .filter_map(|i| {
                let name = names.get(i % names.len().max(1))?;
                self.base(&format!("{} {name}", facts.size.label()))
            })
            .collect();
        for pick in self.rig_picks(&bases, &choices.bling, &mut calibration, false) {
            picks.push(pick);
            rigs_left -= 1;
        }
        let room = Room {
            slots: match (tank, armor) {
                (false, _) => 0,
                (true, true) => lows_left,
                (true, false) => mids_left,
            },
            rigs: if Self::tank_takes_rigs(choices, kind) {
                rigs_left
            } else {
                0
            },
            calibration,
        };
        (picks, room)
    }

    /// Rigs in order, until one costs more calibration than is left.
    fn rig_picks(
        &self,
        bases: &[TypeId],
        bling: &Bling,
        calibration: &mut f64,
        tank: bool,
    ) -> Vec<Pick> {
        let mut picks = Vec::new();
        for &base in bases {
            let type_id = self.pick(base, Category::Rig, bling);
            let cost = self.cost(type_id);
            if cost > *calibration + 1e-6 {
                break;
            }
            *calibration -= cost;
            picks.push(Pick {
                slot: Slot::Rig,
                base,
                type_id,
                charge: None,
                priority: 3,
                tank,
            });
        }
        picks
    }

    /// The tank in the room the rest left: the optimiser's pick if there is one, else the usual
    /// layout for the style.
    fn plan_tank(&self, fit: &Fit, choices: &Choices, room: &Room, base: &[Pick]) -> Vec<Pick> {
        let catalog = self.catalog;
        let layer = choices.layer;
        let slot = layer.slot();
        let mut picks: Vec<Pick> = Vec::new();
        let push_base = |picks: &mut Vec<Pick>, base_id: TypeId, priority| {
            let category = tank::category_of(catalog, base_id);
            let all: Vec<Pick> = base.iter().chain(picks.iter()).cloned().collect();
            if let Some(pick) =
                self.pick_base(fit, choices, &all, (slot, base_id, category, priority))
            {
                picks.push(Pick { tank: true, ..pick });
                true
            } else {
                false
            }
        };
        let rig_bases: Vec<TypeId> = if let Some(found) = &choices.tank_fit {
            for &id in found.modules.iter().take(room.slots) {
                let priority = match tank::category_of(catalog, id) {
                    Category::Repairer => 7,
                    Category::Buffer => 6,
                    Category::Hardener => 5,
                    _ => 4,
                };
                push_base(&mut picks, id, priority);
            }
            found.rigs.clone()
        } else {
            let id = |name: &str| self.base(name);
            let plate = id(self.plate_name());
            let extender = id(self.extender_name());
            let membrane = id("Multispectrum Energized Membrane I");
            let control = id("Damage Control I");
            let hardener = id("Multispectrum Shield Hardener I");
            let repairer = id(&self.repairer_name());
            let booster = id(self.booster_name());
            let cap_booster = id(&self.utility_name(Utility::CapBooster, None));
            for i in 0..room.slots {
                let (wanted, priority) = match (choices.tank, layer) {
                    (TankStyle::None, _) => break,
                    (TankStyle::Brick, Layer::Shield) => (extender, 6),
                    (TankStyle::Brick, Layer::Armor) => (plate, 6),
                    (TankStyle::Buffer, Layer::Shield) => match i {
                        1 | 4 => (hardener, 5),
                        0 => (extender, 7),
                        _ => (extender, 4),
                    },
                    (TankStyle::Active, Layer::Shield) => match i {
                        0 => (booster, 7),
                        2 if !choices.has(Utility::CapBooster) => (cap_booster, 4),
                        _ => (hardener, 5),
                    },
                    (TankStyle::Buffer, Layer::Armor) => match i {
                        0 => (plate, 7),
                        2 => (control, 6),
                        _ if i % 2 == 1 => (membrane, 5),
                        _ => (plate, 4),
                    },
                    (TankStyle::Active, Layer::Armor) => match i {
                        0 => (repairer, 7),
                        2 => (repairer, 4),
                        3 => (control, 6),
                        _ => (membrane, 5),
                    },
                };
                let Some(wanted) = wanted else { continue };
                // A fit that has its damage control gets a membrane instead.
                if !push_base(&mut picks, wanted, priority)
                    && layer == Layer::Armor
                    && let Some(membrane) = membrane
                {
                    push_base(&mut picks, membrane, 5);
                }
            }
            let names = Self::tank_rig_names(choices);
            (0..room.rigs)
                .filter_map(|i| {
                    let name = names.get(i % names.len().max(1))?;
                    self.base(&format!("{} {name}", self.facts.size.label()))
                })
                .collect()
        };
        let mut calibration = room.calibration;
        let rigs: Vec<TypeId> = rig_bases.into_iter().take(room.rigs).collect();
        picks.extend(self.rig_picks(&rigs, &choices.bling, &mut calibration, true));
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
