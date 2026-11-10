//! The market browser's logic, after pyfa's service/market.py and gui/builtinMarketBrowser:
//! the meta group buttons, recently used items, charges for the active fit and the order items
//! are listed in.

pub(crate) mod charges;
pub mod jargon;

use std::{
    collections::{HashMap, HashSet},
    hash::BuildHasher,
};

pub use crate::data::order::MetaCategory;
use crate::{
    data::{Catalog, Hardpoint, ItemType, Kind, Slot, TypeId, category},
    fit::Fit,
};
use jargon::{Jargon, Query};

/// Items pyfa remembers as recently used.
pub const RECENT_LIMIT: usize = 20;

/// Which meta buttons are pressed. None pressed shows everything, which is where it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetaFilter {
    on: [bool; MetaCategory::ALL.len()],
}

impl MetaFilter {
    pub fn is_on(self, category: MetaCategory) -> bool {
        self.on[category.index()]
    }

    /// Whether any button is pressed, so the list is filtered at all.
    pub fn active(self) -> bool {
        self.on.contains(&true)
    }

    pub fn allows(self, t: &ItemType) -> bool {
        !self.active() || self.is_on(MetaCategory::of_item(t))
    }

    /// With `only` the click shows only that category, or everything again when it already was
    /// the only one; without, it switches the category on or off next to the others.
    pub fn click(&mut self, category: MetaCategory, only: bool) {
        let i = category.index();
        if only {
            let only = self.on[i] && self.on.iter().filter(|&&on| on).count() == 1;
            self.on = [false; MetaCategory::ALL.len()];
            self.on[i] = !only;
        } else {
            self.on[i] = !self.on[i];
        }
    }

    /// Make sure items of `category` show, e.g. after jumping to one.
    pub fn enable(&mut self, category: MetaCategory) {
        if self.active() {
            self.on[category.index()] = true;
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Put an item at the front of the recently used list, keeping it short and without repeats.
pub fn remember(recent: &mut Vec<TypeId>, id: TypeId) {
    recent.retain(|&r| r != id);
    recent.insert(0, id);
    recent.truncate(RECENT_LIMIT);
}

/// Every charge the fit's modules can load, once each, in market order: pyfa's "Charges For
/// Active Fit".
pub fn charges_for_fit(catalog: &Catalog, fit: &Fit) -> Vec<TypeId> {
    let modules: HashSet<TypeId> = [Slot::High, Slot::Mid, Slot::Low]
        .into_iter()
        .flat_map(|slot| fit.rack(slot).iter().flatten().map(|m| m.type_id))
        .collect();
    let holders: Vec<&ItemType> = modules
        .iter()
        .filter_map(|&m| catalog.get(m))
        .filter(|m| !m.charge_groups.is_empty())
        .collect();
    if holders.is_empty() {
        return Vec::new();
    }
    let mut charges: Vec<TypeId> = holders
        .iter()
        .flat_map(|m| &m.charge_groups)
        .flat_map(|&group| catalog.charges_in_group(group))
        .copied()
        .filter(|&c| {
            catalog
                .get(c)
                .is_some_and(|c| holders.iter().any(|m| c.fits_in(m)))
        })
        .collect();
    sort_items(&catalog.types, &mut charges);
    charges.dedup();
    charges
}

/// What the results pane lists while the search box is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Listing {
    /// A market group's items, or its subgroups' when it has none of its own.
    Group(i32),
    /// pyfa's "Recently Used Items".
    Recent,
    /// pyfa's "Charges For Active Fit".
    FitCharges,
    /// Every charge one module can load.
    ModuleCharges(TypeId),
}

/// A run of results under an optional heading, such as a charge family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: Option<String>,
    pub items: Vec<TypeId>,
}

impl Section {
    pub fn untitled(items: Vec<TypeId>) -> Self {
        Self { title: None, items }
    }
}

/// A market group's items in market order: its own, or all of its subgroups' when it lists
/// none itself, so a click on "Hybrid Charges" shows every hybrid charge.
/// Whether `group` only splits rigs by size, like Armor Rigs into Small to Capital: groups with
/// nothing below them, holding rigs that all have a size.
pub fn rigs_by_size(catalog: &Catalog, group: i32) -> bool {
    let Some(g) = catalog.market_groups.get(&group) else {
        return false;
    };
    !g.children.is_empty()
        && g.children.iter().all(|child| {
            catalog.market_groups.get(child).is_some_and(|c| {
                c.children.is_empty()
                    && !c.types.is_empty()
                    && c.types.iter().all(|&t| {
                        catalog.get(t).is_some_and(|t| {
                            t.kind == Kind::Module(Slot::Rig) && t.rig_size.is_some()
                        })
                    })
            })
        })
}

pub fn group_items(catalog: &Catalog, group: i32) -> Vec<TypeId> {
    let Some(g) = catalog.market_groups.get(&group) else {
        return Vec::new();
    };
    let mut items = if g.types.is_empty() {
        let mut items = Vec::new();
        let mut pending = g.children.clone();
        while let Some(child) = pending.pop() {
            if let Some(c) = catalog.market_groups.get(&child) {
                items.extend_from_slice(&c.types);
                pending.extend_from_slice(&c.children);
            }
        }
        items
    } else {
        g.types.clone()
    };
    items.retain(|id| catalog.get(*id).is_some_and(|t| t.published));
    sort_items(&catalog.types, &mut items);
    items.dedup();
    items
}

/// The results pane's own filter text: the name contains it, or it matches as a market search
/// with jargon and wildcards.
#[derive(Debug)]
pub struct TextFilter {
    lower: String,
    query: Option<Query>,
}

impl TextFilter {
    /// None for blank text, which filters nothing.
    pub fn new(text: &str, jargon: &Jargon) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self {
            lower: text.to_lowercase(),
            query: Query::parse(text, jargon),
        })
    }

    pub fn matches(&self, t: &ItemType) -> bool {
        t.lower_name.contains(&self.lower)
            || self.query.as_ref().is_some_and(|q| q.matches(&t.name))
    }
}

/// What the current fit has room for, for the "Fits current ship" filter. Catalog checks only,
/// the cheap part of the fitting rules: slots, hardpoints, rig size, hull and ship limits.
#[derive(Debug, Clone)]
pub struct ShipRoom<'a> {
    pub ship: &'a ItemType,
    /// Slots per rack, after subsystems.
    pub slots: [u32; 6],
    pub turrets: u32,
    pub launchers: u32,
    pub drones: bool,
    pub fighters: bool,
    /// Fitted modules that take charges.
    pub holders: Vec<&'a ItemType>,
}

fn slot_index(slot: Slot) -> usize {
    match slot {
        Slot::High => 0,
        Slot::Mid => 1,
        Slot::Low => 2,
        Slot::Rig => 3,
        Slot::Subsystem => 4,
        Slot::Service => 5,
    }
}

impl ShipRoom<'_> {
    pub fn slots(&self, slot: Slot) -> u32 {
        self.slots[slot_index(slot)]
    }

    /// Whether the fit could use the item: a module that has a slot, hardpoint and the right
    /// rig size, a charge a fitted module loads, a drone or fighter with a bay to go in.
    pub fn can_use(&self, t: &ItemType) -> bool {
        let structure = self.ship.is_structure();
        match t.kind {
            Kind::Module(slot) => {
                let limited = !t.fits_ships.is_empty() || !t.fits_groups.is_empty();
                self.slots(slot) > 0
                    && match t.hardpoint {
                        Hardpoint::Turret => self.turrets > 0,
                        Hardpoint::Launcher => self.launchers > 0,
                        Hardpoint::None => true,
                    }
                    && (slot != Slot::Rig
                        || match (t.rig_size, self.ship.rig_size) {
                            (Some(rig), Some(ship)) => (rig - ship).abs() < f64::EPSILON,
                            _ => true,
                        })
                    && (!limited
                        || t.fits_ships.contains(&self.ship.id)
                        || t.fits_groups.contains(&self.ship.group_id))
                    && (t.category_id == category::STRUCTURE_MODULE) == structure
            }
            Kind::Charge => self.holders.iter().any(|m| t.fits_in(m)),
            Kind::Drone => self.drones,
            Kind::Fighter => self.fighters,
            Kind::Implant | Kind::Booster => !structure,
            Kind::Mode => t.fits_ships.is_empty() || t.fits_ships.contains(&self.ship.id),
            Kind::Ship | Kind::Skill | Kind::Cargo | Kind::Other => true,
        }
    }
}

/// A line of the results pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultRow {
    Heading(String),
    Item(TypeId),
}

/// The results pane's rows after its filters.
#[derive(Debug, Default)]
pub struct Filtered {
    pub rows: Vec<ResultRow>,
    /// Items shown.
    pub count: usize,
    /// Items before any filter.
    pub total: usize,
    /// Meta buttons with something to show once the text and ship filters ran.
    pub present: HashSet<MetaCategory>,
}

/// Run the results pane's filters over the listed sections: the text, the ship, then the meta
/// buttons, which are pressed so the list is not empty because of them, as pyfa does.
/// Whether `t` is anything but a rig of another size than `size`. The market keeps to the
/// current hull's rig size even with the ship filter off, as Axis does.
fn rig_fits(t: &ItemType, size: f64) -> bool {
    t.kind != Kind::Module(Slot::Rig)
        || t.rig_size
            .is_none_or(|rig| (rig - size).abs() < f64::EPSILON)
}

pub fn filter_results(
    catalog: &Catalog,
    sections: &[Section],
    text: Option<&TextFilter>,
    room: Option<&ShipRoom>,
    rig_size: Option<f64>,
    meta: MetaFilter,
) -> Filtered {
    let kept: Vec<(Option<&String>, Vec<&ItemType>)> = sections
        .iter()
        .map(|s| {
            let items = s
                .items
                .iter()
                .filter_map(|&id| catalog.get(id))
                .filter(|t| text.is_none_or(|f| f.matches(t)))
                .filter(|t| room.is_none_or(|r| r.can_use(t)))
                .filter(|t| rig_size.is_none_or(|size| rig_fits(t, size)))
                .collect();
            (s.title.as_ref(), items)
        })
        .collect();
    let mut filtered = Filtered {
        total: sections.iter().map(|s| s.items.len()).sum(),
        present: kept
            .iter()
            .flat_map(|(_, items)| items.iter().map(|t| MetaCategory::of_item(t)))
            .collect(),
        ..Filtered::default()
    };
    for (title, items) in kept {
        let shown: Vec<TypeId> = items
            .into_iter()
            .filter(|t| meta.allows(t))
            .map(|t| t.id)
            .collect();
        if shown.is_empty() {
            continue;
        }
        if let Some(title) = title {
            filtered.rows.push(ResultRow::Heading(title.clone()));
        }
        filtered.count += shown.len();
        filtered.rows.extend(shown.into_iter().map(ResultRow::Item));
    }
    filtered
}

/// Sort items the way pyfa lists them (`Market.itemSort`), by the place the catalog gave each
/// (see [`crate::data::order::rank`]). Unknown types come first.
pub fn sort_items<S: BuildHasher>(types: &HashMap<TypeId, ItemType, S>, items: &mut [TypeId]) {
    items.sort_by_key(|id| types.get(id).map(|t| t.market_rank));
}

/// Published, fittable types matching a market search the way pyfa runs it: every word found
/// in the name, with pyfa's jargon and wildcards, in pyfa's order, at most `limit` of them.
/// Nothing for a query too short to search for.
pub fn search(catalog: &Catalog, query: &str, limit: usize) -> Vec<TypeId> {
    let Some(query) = Query::parse(query, Jargon::global()) else {
        return Vec::new();
    };
    catalog
        .searchable()
        .iter()
        .copied()
        .filter(|&id| catalog.get(id).is_some_and(|t| query.matches(&t.name)))
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_groups_map_to_buttons() {
        assert_eq!(MetaCategory::of(None), MetaCategory::Tech1);
        assert_eq!(MetaCategory::of(Some(1)), MetaCategory::Tech1);
        assert_eq!(MetaCategory::of(Some(2)), MetaCategory::Tech2);
        assert_eq!(MetaCategory::of(Some(14)), MetaCategory::Tech3);
        assert_eq!(MetaCategory::of(Some(4)), MetaCategory::Faction);
        assert_eq!(MetaCategory::of(Some(52)), MetaCategory::Faction);
        assert_eq!(MetaCategory::of(Some(6)), MetaCategory::Deadspace);
        assert_eq!(MetaCategory::of(Some(5)), MetaCategory::Officer);
    }

    #[test]
    fn buttons_toggle_and_none_pressed_shows_all() {
        let mut filter = MetaFilter::default();
        assert!(!filter.active());
        filter.click(MetaCategory::Tech2, false);
        filter.click(MetaCategory::Faction, false);
        assert!(filter.is_on(MetaCategory::Tech2) && filter.is_on(MetaCategory::Faction));
        // Every button can be switched off again.
        filter.click(MetaCategory::Tech2, false);
        filter.click(MetaCategory::Faction, false);
        assert!(!filter.active());
        // Ctrl shows only one, and a second Ctrl click on it shows everything.
        filter.click(MetaCategory::Tech1, false);
        filter.click(MetaCategory::Officer, true);
        assert!(filter.is_on(MetaCategory::Officer) && !filter.is_on(MetaCategory::Tech1));
        filter.click(MetaCategory::Officer, true);
        assert!(!filter.active());
    }

    #[test]
    fn recent_items_are_unique_newest_first_and_capped() {
        let mut recent = Vec::new();
        for id in 0..30 {
            remember(&mut recent, id);
        }
        remember(&mut recent, 25);
        assert_eq!(recent.len(), RECENT_LIMIT);
        assert_eq!(recent[..3], [25, 29, 28]);
        assert_eq!(recent.iter().filter(|&&r| r == 25).count(), 1);
    }

    fn item(id: TypeId, name: &str, kind: Kind) -> ItemType {
        ItemType {
            id,
            name: name.to_owned(),
            lower_name: name.to_lowercase(),
            market_rank: 0,
            group_id: 0,
            category_id: match kind {
                Kind::Module(_) => category::MODULE,
                Kind::Charge => category::CHARGE,
                Kind::Ship => category::SHIP,
                _ => 0,
            },
            market_group: Some(1),
            published: true,
            kind,
            activatable: false,
            overloadable: false,
            charge_groups: Vec::new(),
            charge_size: None,
            icon: None,
            variation_parent: None,
            meta_group: None,
            meta_level: 0,
            volume: 0.0,
            hardpoint: Hardpoint::None,
            damage: [0.0; 4],
            range_multiplier: None,
            falloff_multiplier: None,
            optimal: None,
            falloff: None,
            mining: false,
            spools: false,
            drone_bandwidth: 0.0,
            signature_radius: None,
            booster_slot: None,
            booster_chance: None,
            side_effects: Vec::new(),
            implant_slot: None,
            capacity: 0.0,
            race: None,
            omega: false,
            rig_size: None,
            fits_ships: Vec::new(),
            fits_groups: Vec::new(),
        }
    }

    fn catalog(items: Vec<ItemType>) -> Catalog {
        let mut catalog = Catalog::default();
        for t in items {
            catalog.types.insert(t.id, t);
        }
        crate::data::order::rank(&mut catalog.types);
        catalog
    }

    fn group(id: i32, children: Vec<i32>, types: Vec<TypeId>) -> crate::data::MarketGroup {
        crate::data::MarketGroup {
            id,
            name: format!("Group {id}"),
            parent: None,
            children,
            types,
            icon_id: None,
        }
    }

    #[test]
    fn a_group_without_items_lists_its_subgroups_items() {
        let mut catalog = catalog(vec![
            item(10, "Alpha", Kind::Other),
            item(11, "Beta", Kind::Other),
            item(12, "Gamma", Kind::Other),
        ]);
        catalog
            .market_groups
            .insert(1, group(1, vec![2, 3], vec![]));
        catalog.market_groups.insert(2, group(2, vec![], vec![11]));
        catalog.market_groups.insert(3, group(3, vec![4], vec![10]));
        catalog.market_groups.insert(4, group(4, vec![], vec![12]));
        assert_eq!(group_items(&catalog, 1), [10, 11, 12]);
        // A group with items of its own lists just those.
        assert_eq!(group_items(&catalog, 3), [10]);
        assert_eq!(group_items(&catalog, 99), [] as [TypeId; 0]);
    }

    #[test]
    fn text_filter_takes_substrings_and_jargon() {
        let jargon = Jargon::from_yaml("hml: [Heavy Missile Launcher]").unwrap();
        assert!(TextFilter::new("   ", &jargon).is_none());
        let named = |name: &str| item(1, name, Kind::Other);
        let short = TextFilter::new("ii", &jargon).unwrap();
        assert!(short.matches(&named("Heavy Missile Launcher II")));
        assert!(!short.matches(&named("Heavy Missile Launcher I")));
        let abbreviated = TextFilter::new("hml", &jargon).unwrap();
        assert!(abbreviated.matches(&named("Heavy Missile Launcher II")));
        assert!(!abbreviated.matches(&named("Rapid Light Missile Launcher II")));
        let words = TextFilter::new("launcher heavy", &jargon).unwrap();
        assert!(words.matches(&named("Heavy Missile Launcher II")));
    }

    fn room(ship: &ItemType) -> ShipRoom<'_> {
        ShipRoom {
            ship,
            slots: [4, 3, 3, 3, 0, 0],
            turrets: 0,
            launchers: 2,
            drones: false,
            fighters: false,
            holders: Vec::new(),
        }
    }

    #[test]
    fn ship_room_checks_slots_hardpoints_rigs_and_limits() {
        let mut ship = item(1, "Caracal", Kind::Ship);
        ship.group_id = 26;
        ship.rig_size = Some(2.0);
        let mut launcher = item(2, "Heavy Missile Launcher II", Kind::Module(Slot::High));
        launcher.hardpoint = Hardpoint::Launcher;
        launcher.charge_groups = vec![385];
        let mut turret = item(3, "Heavy Neutron Blaster II", Kind::Module(Slot::High));
        turret.hardpoint = Hardpoint::Turret;
        let mut small_rig = item(
            4,
            "Small Core Defense Field Extender I",
            Kind::Module(Slot::Rig),
        );
        small_rig.rig_size = Some(1.0);
        let mut medium_rig = item(
            5,
            "Medium Core Defense Field Extender I",
            Kind::Module(Slot::Rig),
        );
        medium_rig.rig_size = Some(2.0);
        let subsystem = item(
            6,
            "Legion Core - Dissolution Sequencer",
            Kind::Module(Slot::Subsystem),
        );
        let mut limited = item(
            7,
            "Interdiction Sphere Launcher I",
            Kind::Module(Slot::High),
        );
        limited.fits_groups = vec![541];
        let mut missile = item(8, "Scourge Heavy Missile", Kind::Charge);
        missile.group_id = 385;
        let mut lens = item(9, "Multifrequency S", Kind::Charge);
        lens.group_id = 86;
        let drone = item(10, "Hobgoblin I", Kind::Drone);
        let mut structure_module = item(
            11,
            "Standup Point Defense Battery I",
            Kind::Module(Slot::High),
        );
        structure_module.category_id = category::STRUCTURE_MODULE;

        let mut r = room(&ship);
        r.holders.push(&launcher);
        assert!(r.can_use(&launcher));
        assert!(!r.can_use(&turret));
        assert!(!r.can_use(&small_rig));
        assert!(r.can_use(&medium_rig));
        assert!(!r.can_use(&subsystem));
        assert!(!r.can_use(&limited));
        assert!(r.can_use(&missile));
        assert!(!r.can_use(&lens));
        assert!(!r.can_use(&drone));
        assert!(!r.can_use(&structure_module));
        r.drones = true;
        assert!(r.can_use(&drone));
        limited.fits_ships = vec![1];
        assert!(r.can_use(&limited));
    }

    #[test]
    fn rigs_keep_to_the_hull_size() {
        let mut small = item(1, "Small Anti-EM Pump I", Kind::Module(Slot::Rig));
        small.rig_size = Some(1.0);
        let mut medium = item(2, "Medium Anti-EM Pump I", Kind::Module(Slot::Rig));
        medium.rig_size = Some(2.0);
        let catalog = catalog(vec![
            small,
            medium,
            item(3, "Damage Control II", Kind::Module(Slot::Low)),
        ]);
        let sections = vec![Section {
            title: None,
            items: vec![1, 2, 3],
        }];
        let kept = filter_results(
            &catalog,
            &sections,
            None,
            None,
            Some(2.0),
            MetaFilter::default(),
        );
        assert_eq!(kept.rows, [ResultRow::Item(2), ResultRow::Item(3)]);
    }

    #[test]
    fn rig_categories_split_by_size() {
        let mut small = item(1, "Small Anti-EM Pump I", Kind::Module(Slot::Rig));
        small.rig_size = Some(1.0);
        small.market_group = Some(11);
        let mut medium = item(2, "Medium Anti-EM Pump I", Kind::Module(Slot::Rig));
        medium.rig_size = Some(2.0);
        let mut catalog = catalog(vec![
            small,
            medium,
            item(3, "Damage Control II", Kind::Module(Slot::Low)),
        ]);
        let group = |id, children: Vec<i32>, types: Vec<TypeId>| crate::data::MarketGroup {
            id,
            name: String::new(),
            parent: None,
            children,
            types,
            icon_id: None,
        };
        catalog.market_groups.extend([
            (10, group(10, vec![11, 12], Vec::new())),
            (11, group(11, Vec::new(), vec![1])),
            (12, group(12, Vec::new(), vec![2])),
            (20, group(20, vec![21], Vec::new())),
            (21, group(21, Vec::new(), vec![3])),
        ]);
        assert!(rigs_by_size(&catalog, 10));
        assert!(!rigs_by_size(&catalog, 11), "no groups below");
        assert!(!rigs_by_size(&catalog, 20), "not rigs");
    }

    #[test]
    fn filters_drop_empty_sections_and_count_what_is_left() {
        let mut faction = item(3, "Caldari Navy Scourge Heavy Missile", Kind::Charge);
        faction.meta_group = Some(4);
        let catalog = catalog(vec![
            item(1, "Inferno Heavy Missile", Kind::Charge),
            item(2, "Scourge Heavy Missile", Kind::Charge),
            faction,
        ]);
        let sections = vec![
            Section {
                title: Some("Thermal".into()),
                items: vec![1],
            },
            Section {
                title: Some("Kinetic".into()),
                items: vec![2, 3],
            },
        ];
        let jargon = Jargon::default();
        let mut meta = MetaFilter::default();
        let all = filter_results(&catalog, &sections, None, None, None, meta);
        assert_eq!((all.count, all.total, all.rows.len()), (3, 3, 5));

        let text = TextFilter::new("scourge", &jargon);
        let some = filter_results(&catalog, &sections, text.as_ref(), None, None, meta);
        assert_eq!(
            some.rows,
            [
                ResultRow::Heading("Kinetic".into()),
                ResultRow::Item(2),
                ResultRow::Item(3)
            ]
        );

        meta.click(MetaCategory::Faction, false);
        let faction_only = filter_results(&catalog, &sections, text.as_ref(), None, None, meta);
        assert_eq!(faction_only.count, 1);
        assert!(faction_only.present.contains(&MetaCategory::Tech1));

        // A button that hides everything stays pressed; the pane offers to clear it.
        let inferno = TextFilter::new("inferno", &jargon);
        let hidden = filter_results(&catalog, &sections, inferno.as_ref(), None, None, meta);
        assert_eq!(hidden.count, 0);
        assert!(meta.is_on(MetaCategory::Faction));

        let none = TextFilter::new("nothing like this", &jargon);
        let empty = filter_results(&catalog, &sections, none.as_ref(), None, None, meta);
        assert_eq!(empty.rows, [] as [ResultRow; 0]);
        assert_eq!(empty.total, 3);
    }
}
