//! How much bling the wizard fits: one tier for everything, a tier per kind of module, and the
//! exact variation picked for a module. Saved, so the next fit starts from the last choices.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::meta;
use crate::data::{Catalog, TypeId};

/// Which variation of an item to fit, cheapest first.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Tier {
    T1,
    /// The best meta variation, which is usually the compact one.
    Meta,
    #[default]
    T2,
    /// Navy, pirate and storyline items.
    Faction,
    Deadspace,
    Officer,
}

impl Tier {
    pub const ALL: [Self; 6] = [
        Self::T1,
        Self::Meta,
        Self::T2,
        Self::Faction,
        Self::Deadspace,
        Self::Officer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::Meta => "Compact",
            Self::T2 => "T2",
            Self::Faction => "Faction",
            Self::Deadspace => "Deadspace",
            Self::Officer => "Officer",
        }
    }

    /// The tier a variation of `base` belongs to.
    pub fn of(catalog: &Catalog, id: TypeId, base: TypeId) -> Self {
        match catalog.get(id).and_then(|t| t.meta_group) {
            Some(meta::TECH2) => Self::T2,
            Some(meta::FACTION | meta::STORYLINE) => Self::Faction,
            Some(meta::DEADSPACE) => Self::Deadspace,
            Some(meta::OFFICER) => Self::Officer,
            _ if id == base => Self::T1,
            _ => Self::Meta,
        }
    }
}

/// The kinds of module a tier can be set for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    Weapon,
    Propulsion,
    /// Scramblers, disruptors, webs and grapplers.
    Tackle,
    /// Neutralizers and nosferatus.
    Energy,
    /// Sensor boosters, computers, enhancers and capacitor boosters.
    Electronics,
    /// Weapon and drone damage modules.
    Damage,
    /// Hardeners, membranes, damage controls and the reactive armor hardener.
    Hardener,
    /// Repairers, shield boosters and boost amplifiers.
    Repairer,
    /// Plates and extenders.
    Buffer,
    Rig,
}

/// The wizard's bling choices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bling {
    /// What everything gets unless told otherwise.
    pub tier: Tier,
    /// A tier of its own for some kinds of module.
    pub categories: BTreeMap<Category, Tier>,
    /// The exact variation picked for a module, by kind and then by its T1 item.
    pub exact: BTreeMap<Category, BTreeMap<TypeId, TypeId>>,
}

impl Bling {
    /// The tier a kind of module gets. Rigs stay T1 unless asked: T2 rigs cost a fortune and
    /// usually more calibration.
    pub fn tier_for(&self, category: Category) -> Tier {
        self.categories
            .get(&category)
            .copied()
            .unwrap_or(match category {
                Category::Rig => Tier::T1,
                _ => self.tier,
            })
    }

    /// One tier for everything, forgetting the per-kind choices.
    pub fn set_tier(&mut self, tier: Tier) {
        self.tier = tier;
        self.categories.clear();
        self.exact.clear();
    }

    /// The exact variation picked for `base`, if any.
    pub fn exact(&self, category: Category, base: TypeId) -> Option<TypeId> {
        self.exact.get(&category)?.get(&base).copied()
    }

    /// Fit `variation` for `base`, and its tier for the rest of its kind.
    pub fn choose(&mut self, category: Category, base: TypeId, variation: TypeId, tier: Tier) {
        let exact = self.exact.entry(category).or_default();
        // Others of the kind follow the new tier rather than an older exact pick.
        exact.clear();
        exact.insert(base, variation);
        self.categories.insert(category, tier);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tier_for_everything_forgets_the_rest() {
        let mut bling = Bling::default();
        assert_eq!(bling.tier_for(Category::Weapon), Tier::T2);
        assert_eq!(bling.tier_for(Category::Rig), Tier::T1);
        bling.choose(Category::Propulsion, 1, 2, Tier::Meta);
        assert_eq!(bling.tier_for(Category::Propulsion), Tier::Meta);
        assert_eq!(bling.exact(Category::Propulsion, 1), Some(2));
        bling.set_tier(Tier::Faction);
        assert_eq!(bling.tier_for(Category::Propulsion), Tier::Faction);
        assert_eq!(bling.exact(Category::Propulsion, 1), None);
    }

    #[test]
    fn bling_survives_a_round_trip() {
        let mut bling = Bling::default();
        bling.choose(Category::Tackle, 447, 448, Tier::Faction);
        let json = serde_json::to_string(&bling).expect("serializes");
        let back: Bling = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, bling);
        let empty: Bling = serde_json::from_str("{}").expect("defaults");
        assert_eq!(empty, Bling::default());
    }
}
