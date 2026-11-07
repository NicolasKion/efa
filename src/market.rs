//! The market browser's logic, after pyfa's service/market.py and gui/builtinMarketBrowser:
//! the meta group buttons, recently used items, charges for the active fit and the order items
//! are listed in.

pub mod jargon;

use std::{
    collections::{HashMap, HashSet},
    hash::BuildHasher,
};

use crate::{
    data::{Catalog, ItemType, Kind, Slot, TypeId, category},
    fit::Fit,
};

/// Items pyfa remembers as recently used.
pub const RECENT_LIMIT: usize = 20;

/// pyfa's meta group buttons, in its order. Each covers several of the game's meta groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetaCategory {
    /// Tech I, II and III, and anything else without a button of its own.
    Normal,
    /// Faction, storyline and structure faction: meta groups 4, 3 and 52.
    Faction,
    /// Deadspace.
    Complex,
    Officer,
}

impl MetaCategory {
    pub const ALL: [Self; 4] = [Self::Normal, Self::Faction, Self::Complex, Self::Officer];

    /// Which button a meta group falls under, as pyfa's `META_MAP` sorts them.
    pub fn of(meta_group: Option<i32>) -> Self {
        match meta_group {
            Some(3 | 4 | 52) => Self::Faction,
            Some(6) => Self::Complex,
            Some(5) => Self::Officer,
            _ => Self::Normal,
        }
    }

    pub fn of_item(t: &ItemType) -> Self {
        Self::of(t.meta_group)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Faction => "Faction",
            Self::Complex => "Complex",
            Self::Officer => "Officer",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Which meta buttons are pressed. All of them to start with, as in pyfa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaFilter {
    on: [bool; 4],
}

impl Default for MetaFilter {
    fn default() -> Self {
        Self { on: [true; 4] }
    }
}

impl MetaFilter {
    pub fn is_on(self, category: MetaCategory) -> bool {
        self.on[category.index()]
    }

    pub fn allows(self, t: &ItemType) -> bool {
        self.is_on(MetaCategory::of_item(t))
    }

    /// A click on a button: on its own it shows just that category; with Ctrl it toggles the
    /// one button, but never turns the last one off.
    pub fn click(&mut self, category: MetaCategory, ctrl: bool) {
        if !ctrl {
            self.on = [false; 4];
            self.on[category.index()] = true;
            return;
        }
        let i = category.index();
        let others = self.on.iter().enumerate().any(|(j, &on)| on && j != i);
        if self.on[i] && !others {
            return;
        }
        self.on[i] = !self.on[i];
    }

    pub fn enable(&mut self, category: MetaCategory) {
        self.on[category.index()] = true;
    }

    /// pyfa's default for a list the buttons filter down to nothing: press the leftmost button
    /// that would show something. Returns whether anything changed.
    pub fn show_something(&mut self, present: &HashSet<MetaCategory>) -> bool {
        if present.iter().any(|&c| self.is_on(c)) {
            return false;
        }
        match MetaCategory::ALL.into_iter().find(|c| present.contains(c)) {
            Some(c) => {
                self.enable(c);
                true
            }
            None => false,
        }
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
    let mut charges: Vec<TypeId> = catalog
        .types
        .values()
        .filter(|c| c.kind == Kind::Charge && c.published && c.market_group.is_some())
        .filter(|c| holders.iter().any(|m| c.fits_in(m)))
        .map(|c| c.id)
        .collect();
    sort_items(&catalog.types, &mut charges);
    charges
}

/// The category name pyfa sorts by, the game's own: boosters are implants, for one.
fn category_name(t: &ItemType) -> &'static str {
    match t.category_id {
        category::SHIP => "Ship",
        category::MODULE => "Module",
        category::CHARGE => "Charge",
        category::DRONE => "Drone",
        category::FIGHTER => "Fighter",
        category::IMPLANT => "Implant",
        category::SUBSYSTEM => "Subsystem",
        category::COMMODITY => "Commodity",
        category::MATERIAL => "Material",
        category::DEPLOYABLE => "Deployable",
        category::CELESTIAL => "Celestial",
        _ => "",
    }
}

/// Sort items the way pyfa lists them (`Market.itemSort`): by category, market group, the item
/// they are a variation of, meta button, meta level and name. Variations end up next to
/// their T1 item.
pub fn sort_items<S: BuildHasher>(types: &HashMap<TypeId, ItemType, S>, items: &mut [TypeId]) {
    items.sort_by_cached_key(|id| {
        let Some(t) = types.get(id) else {
            return (String::new(), -1, String::new(), 0, 0, String::new());
        };
        let parent = t
            .variation_parent
            .and_then(|p| types.get(&p))
            .map_or_else(|| t.name.clone(), |p| p.name.clone());
        (
            category_name(t).to_owned(),
            t.market_group.unwrap_or(-1),
            parent,
            MetaCategory::of_item(t).index(),
            t.meta_level,
            t.name.clone(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_groups_map_to_pyfa_buttons() {
        assert_eq!(MetaCategory::of(None), MetaCategory::Normal);
        assert_eq!(MetaCategory::of(Some(1)), MetaCategory::Normal);
        assert_eq!(MetaCategory::of(Some(2)), MetaCategory::Normal);
        assert_eq!(MetaCategory::of(Some(14)), MetaCategory::Normal);
        assert_eq!(MetaCategory::of(Some(4)), MetaCategory::Faction);
        assert_eq!(MetaCategory::of(Some(52)), MetaCategory::Faction);
        assert_eq!(MetaCategory::of(Some(6)), MetaCategory::Complex);
        assert_eq!(MetaCategory::of(Some(5)), MetaCategory::Officer);
    }

    #[test]
    fn plain_click_shows_one_ctrl_click_toggles() {
        let mut filter = MetaFilter::default();
        filter.click(MetaCategory::Faction, false);
        assert!(filter.is_on(MetaCategory::Faction));
        assert!(!filter.is_on(MetaCategory::Normal));
        filter.click(MetaCategory::Officer, true);
        assert!(filter.is_on(MetaCategory::Officer));
        filter.click(MetaCategory::Faction, true);
        assert!(!filter.is_on(MetaCategory::Faction));
        // The last button stays on.
        filter.click(MetaCategory::Officer, true);
        assert!(filter.is_on(MetaCategory::Officer));
    }

    #[test]
    fn an_empty_list_presses_the_leftmost_useful_button() {
        let mut filter = MetaFilter::default();
        filter.click(MetaCategory::Officer, false);
        let present: HashSet<_> = [MetaCategory::Complex, MetaCategory::Faction].into();
        assert!(filter.show_something(&present));
        assert!(filter.is_on(MetaCategory::Faction));
        assert!(!filter.is_on(MetaCategory::Complex));
        assert!(!filter.show_something(&present));
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
}
