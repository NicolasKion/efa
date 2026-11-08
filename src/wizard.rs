//! The fitting wizard's logic: sections that each work one concern of the current fit, leaving
//! the rest of it alone. No UI here; the wizard dialog shows and applies what this works out.
//!
//! A section turns its choices into a [`Change`]: modules it takes off and modules it puts on,
//! by rack position. The tank section is the only one so far; another concern goes in a module
//! of its own beside [`tank`].
//!
//! Items are found by their T1 name and tried through their variations, so a new meta item or a
//! renamed faction one is picked up without a list of type IDs.

pub mod tank;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    data::{Catalog, ItemType, Slot, TypeId},
    dogma::{attr, group},
    engine::{CalcOptions, Engine, Pilot},
    fit::{Fit, Module},
    item_data::{ItemData, Traits},
};

/// Meta groups, as the market's buttons name them.
mod meta {
    pub const TECH1: i32 = 1;
    pub const TECH2: i32 = 2;
    pub const STORYLINE: i32 = 3;
    pub const FACTION: i32 = 4;
    pub const OFFICER: i32 = 5;
    pub const DEADSPACE: i32 = 6;
}

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

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Capital => "Capital",
        }
    }
}

/// The wizard's sections, one per concern, in the order the dialog lists them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    /// Older saves name the guided wizard's steps; they open on the tank.
    #[default]
    #[serde(alias = "Loadout", alias = "Rigs")]
    Tank,
}

impl Section {
    pub const ALL: [Self; 1] = [Self::Tank];

    pub fn label(self) -> &'static str {
        match self {
            Self::Tank => "Tank",
        }
    }
}

/// The name the store saves the last section under.
pub type Tab = Section;

/// What the wizard remembers for the next fit, each section's choices on their own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remembered {
    pub tank: tank::Choices,
}

/// The name the store saves [`Remembered`] under. Older saves hold the guided wizard's bling
/// choices there, which read as the defaults.
pub type Bling = Remembered;

/// Which layer takes the damage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
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

/// What a search may spend, in whole ISK; None is no limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// The most any one module or rig may cost.
    pub each: Option<u64>,
    /// The most everything the search adds may cost.
    pub total: Option<u64>,
}

impl Budget {
    /// The choices offered for one module.
    pub const EACH: [Option<u64>; 5] = [
        None,
        Some(10_000_000),
        Some(50_000_000),
        Some(250_000_000),
        Some(1_000_000_000),
    ];
    /// The choices offered for the whole lot.
    pub const TOTAL: [Option<u64>; 5] = [
        None,
        Some(100_000_000),
        Some(500_000_000),
        Some(1_000_000_000),
        Some(5_000_000_000),
    ];

    /// An amount of ISK as typed: "75m", "1.5b", "250k" or a plain number, commas and "isk"
    /// allowed. None for anything else, an empty text included.
    pub fn parse(text: &str) -> Option<u64> {
        let text = text.trim().to_lowercase().replace([',', '_', ' '], "");
        let text = text.strip_suffix("isk").unwrap_or(&text);
        let (number, scale) = match text.chars().last()? {
            'k' => (&text[..text.len() - 1], 1e3),
            'm' => (&text[..text.len() - 1], 1e6),
            'b' => (&text[..text.len() - 1], 1e9),
            't' => (&text[..text.len() - 1], 1e12),
            _ => (text, 1.0),
        };
        let value: f64 = number.parse().ok()?;
        (value.is_finite() && value > 0.0).then(|| (value * scale).round() as u64)
    }

    /// A budget as a short label: "Any", "50M", "1B".
    pub fn label(limit: Option<u64>) -> String {
        match limit {
            None => "Any".to_owned(),
            Some(isk) if isk >= 1_000_000_000 => {
                trim_zeros(&format!("{:.2}", isk as f64 / 1e9)) + "B"
            }
            Some(isk) => trim_zeros(&format!("{:.2}", isk as f64 / 1e6)) + "M",
        }
    }

    fn is_set(self) -> bool {
        self.each.is_some() || self.total.is_some()
    }

    /// Whether an item at this price may go in: within the per-module budget, and priced at all
    /// when there is a budget, since what the market has no price for could cost anything.
    pub fn allows(self, price: Option<f64>) -> bool {
        match price {
            Some(price) => self.each.is_none_or(|max| price <= max as f64),
            None => !self.is_set(),
        }
    }
}

/// "1.50" as "1.5", "2.00" as "2".
fn trim_zeros(text: &str) -> String {
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A group of variations a search can try.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchGroup {
    T1,
    /// Meta and compact variations of the T1 item.
    Meta,
    T2,
    Faction,
    Storyline,
    Deadspace,
    Officer,
    /// T2 rigs; T1 rigs are always tried.
    T2Rigs,
}

impl SearchGroup {
    pub const ALL: [Self; 8] = [
        Self::T1,
        Self::Meta,
        Self::T2,
        Self::Faction,
        Self::Storyline,
        Self::Deadspace,
        Self::Officer,
        Self::T2Rigs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::Meta => "Meta",
            Self::T2 => "T2",
            Self::Faction => "Faction",
            Self::Storyline => "Storyline",
            Self::Deadspace => "Deadspace",
            Self::Officer => "Officer",
            Self::T2Rigs => "T2 rigs",
        }
    }

    /// The game's meta group for module groups; None for T1, Meta and rigs, which are found
    /// by other means.
    fn meta_group(self) -> Option<i32> {
        match self {
            Self::T2 => Some(meta::TECH2),
            Self::Faction => Some(meta::FACTION),
            Self::Storyline => Some(meta::STORYLINE),
            Self::Deadspace => Some(meta::DEADSPACE),
            Self::Officer => Some(meta::OFFICER),
            Self::T1 | Self::Meta | Self::T2Rigs => None,
        }
    }

    /// Whether the group holds this variation.
    fn covers(self, t: &ItemType) -> bool {
        match self {
            Self::T1 => t.variation_parent.is_none(),
            Self::Meta => t.meta_group == Some(meta::TECH1) && t.variation_parent.is_some(),
            Self::T2Rigs => false,
            _ => t.meta_group.is_some() && t.meta_group == self.meta_group(),
        }
    }
}

/// Which groups of variations a search tries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchTiers(u16);

impl Default for SearchTiers {
    /// T2 modules and T1 rigs, as most fits are built.
    fn default() -> Self {
        Self::of(&[SearchGroup::T2])
    }
}

impl SearchTiers {
    pub fn of(groups: &[SearchGroup]) -> Self {
        let mut tiers = Self(0);
        for &g in groups {
            tiers.toggle(g);
        }
        tiers
    }

    fn bit(group: SearchGroup) -> u16 {
        1 << SearchGroup::ALL
            .iter()
            .position(|&g| g == group)
            .unwrap_or(0)
    }

    pub fn has(self, group: SearchGroup) -> bool {
        self.0 & Self::bit(group) != 0
    }

    pub fn toggle(&mut self, group: SearchGroup) {
        self.0 ^= Self::bit(group);
    }

    /// Every group, for searches that should leave nothing out.
    pub fn everything() -> Self {
        Self::of(&SearchGroup::ALL)
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
    pub calibration: f64,
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
                .is_some_and(|s| group::BATTLECRUISERS.contains(&s.group_id)),
            totals,
            calibration: stats.calibration.output.unwrap_or(0.0),
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

    /// The layer the hull is built to tank: what its mastery asks for, else what its bonuses
    /// name more, else the rack with more slots.
    pub fn layer(&self) -> Layer {
        let has = |word: &str| self.mastery.iter().any(|c| c.ends_with(word));
        let lines = |word: &str| {
            self.bonuses
                .iter()
                .filter(|b| b.contains(word) && !b.contains("remote"))
                .count()
        };
        let shield = match (has("shield tanking"), has("armor tanking")) {
            (true, false) => true,
            (false, true) => false,
            _ => match lines("shield").cmp(&lines("armor")) {
                std::cmp::Ordering::Greater => true,
                std::cmp::Ordering::Less => false,
                std::cmp::Ordering::Equal => self.total(Slot::Mid) > self.total(Slot::Low),
            },
        };
        if shield { Layer::Shield } else { Layer::Armor }
    }

    /// Whether the hull counts as large for its tank modules: battlecruisers take large
    /// extenders, plates and boosters.
    fn large(&self) -> bool {
        self.size >= HullSize::Large || self.battlecruiser
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
        .map(|b| crate::text::strip_tags(&b.text).to_lowercase())
        .collect()
}

/// How many of the strongest variations of each faction, deadspace or officer group a search
/// tries per module.
const STRONGEST_PER_GROUP: usize = 3;

/// A module going on at a rack position.
pub type Placement = (Slot, usize, Module);

/// What a section does to the fit: the positions it empties, then the modules it puts on.
#[derive(Debug, Clone, Default)]
pub struct Change {
    pub removed: Vec<(Slot, usize)>,
    pub placements: Vec<Placement>,
}

/// What the sections work with: the item data and the hull's facts.
pub struct Setup<'a> {
    catalog: &'a Catalog,
    data: &'a ItemData,
    pub facts: &'a Facts,
}

impl<'a> Setup<'a> {
    pub fn new(catalog: &'a Catalog, data: &'a ItemData, facts: &'a Facts) -> Self {
        Self {
            catalog,
            data,
            facts,
        }
    }

    /// A published T1 item by name.
    fn base(&self, name: &str) -> Option<TypeId> {
        self.catalog
            .find(name)
            .filter(|t| t.published && t.market_group.is_some())
            .map(|t| t.id)
    }

    /// Published market variations of `base`, itself included.
    fn variations(&self, base: TypeId) -> Vec<&'a ItemType> {
        self.catalog
            .variations_of(base)
            .iter()
            .filter_map(|&v| self.catalog.get(v))
            .filter(|t| t.published && t.market_group.is_some())
            .collect()
    }

    /// The usual variation of `base`: its first T2, else its best meta one, else itself.
    fn usual(&self, base: TypeId) -> TypeId {
        let variations = self.variations(base);
        let t2 = variations
            .iter()
            .filter(|t| t.meta_group == Some(meta::TECH2))
            .min_by_key(|t| (t.meta_level, t.id));
        let meta = || {
            variations
                .iter()
                .filter(|t| t.meta_group == Some(meta::TECH1) && t.id != base)
                .max_by_key(|t| (t.meta_level, -t.id))
        };
        t2.or_else(meta).map_or(base, |t| t.id)
    }

    /// The variations of `base` a search tries: one or a few per group the tiers switch on,
    /// each only once. A module none of those groups has comes at its usual variation, so
    /// switching on only Officer does not leave the plates out.
    fn search_variations(&self, base: TypeId, rig: bool, tiers: SearchTiers) -> Vec<TypeId> {
        let mut found: Vec<TypeId> = Vec::new();
        let mut add = |id: TypeId| {
            if !found.contains(&id) {
                found.push(id);
            }
        };
        if rig {
            add(base);
            if tiers.has(SearchGroup::T2Rigs) {
                add(self.usual(base));
            }
            return found;
        }
        let variations = self.variations(base);
        // The strongest few of a group: its weakest member is rarely the one worth fitting.
        for group in SearchGroup::ALL.into_iter().filter(|&g| tiers.has(g)) {
            let mut members: Vec<&ItemType> = variations
                .iter()
                .copied()
                .filter(|t| group.covers(t) && (group != SearchGroup::T1 || t.id == base))
                .collect();
            members.sort_by(|a, b| b.meta_level.cmp(&a.meta_level).then(a.id.cmp(&b.id)));
            for t in members.into_iter().take(STRONGEST_PER_GROUP) {
                add(t.id);
            }
        }
        if found.is_empty() {
            found.push(self.usual(base));
        }
        found
    }

    /// What a module is loaded with: cap booster charges, and nanite paste for ancillary armor
    /// repairers.
    fn charge_for(&self, id: TypeId) -> Option<TypeId> {
        let module = self.catalog.get(id)?;
        match module.group_id {
            group::CAPACITOR_BOOSTER | group::ANCILLARY_SHIELD_BOOSTER => self.cap_charge(id),
            group::ANCILLARY_ARMOR_REPAIRER => self
                .catalog
                .types
                .values()
                .find(|t| t.published && t.name == "Nanite Repair Paste" && t.fits_in(module))
                .map(|t| t.id),
            _ => None,
        }
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

    /// A type's base attribute, zero when it has none.
    fn attr(&self, id: TypeId, attr: i32) -> f64 {
        self.data
            .type_info(id)
            .and_then(|i| i.attr(attr))
            .unwrap_or(0.0)
    }
}

/// Make the change on `fit`.
pub fn apply(fit: &mut Fit, change: &Change) {
    for &(slot, i) in &change.removed {
        if slot == Slot::Rig {
            if let Some(at) = fit.rigs.get_mut(i) {
                *at = None;
            }
        } else if let Some(at) = fit.rack_mut(slot).get_mut(i) {
            *at = None;
        }
    }
    for placement in &change.placements {
        place(fit, placement);
    }
}

/// Put one module on at its position.
pub fn place(fit: &mut Fit, (slot, i, module): &Placement) {
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
