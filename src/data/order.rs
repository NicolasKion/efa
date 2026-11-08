//! The order pyfa's market lists items in (`Market.itemSort`), worked out once when the catalog is
//! built: every type gets its place as a number, so lists sort by that alone.

use std::collections::HashMap;

use super::{ItemType, TypeId, category};

/// The meta group buttons over the market results. Each covers one or more of the game's meta
/// groups; pyfa lumps Tech I, II and III into one "Normal" button, efa splits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetaCategory {
    /// Tech I, including meta and compact variants, and anything without a button of its own.
    Tech1,
    /// Tech II, including structure Tech II.
    Tech2,
    Tech3,
    /// Faction, storyline and structure faction: meta groups 4, 3 and 52.
    Faction,
    /// Deadspace and abyssal.
    Deadspace,
    Officer,
}

impl MetaCategory {
    pub const ALL: [Self; 6] = [
        Self::Tech1,
        Self::Tech2,
        Self::Tech3,
        Self::Faction,
        Self::Deadspace,
        Self::Officer,
    ];

    /// Which button a meta group falls under.
    pub fn of(meta_group: Option<i32>) -> Self {
        match meta_group {
            Some(2 | 53) => Self::Tech2,
            Some(14) => Self::Tech3,
            Some(3 | 4 | 52) => Self::Faction,
            Some(6 | 15) => Self::Deadspace,
            Some(5) => Self::Officer,
            _ => Self::Tech1,
        }
    }

    pub fn of_item(t: &ItemType) -> Self {
        Self::of(t.meta_group)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Tech1 => "T1",
            Self::Tech2 => "T2",
            Self::Tech3 => "T3",
            Self::Faction => "Faction",
            Self::Deadspace => "Deadspace",
            Self::Officer => "Officer",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
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

/// Give every type its place in market order: by category, market group, the item it is a
/// variation of, meta button, meta level and name, so variations end up next to their T1 item.
/// Ties go by type ID, so every build agrees.
pub(crate) fn rank(types: &mut HashMap<TypeId, ItemType>) {
    let mut order: Vec<TypeId> = types.keys().copied().collect();
    order.sort_by_cached_key(|id| {
        let t = &types[id];
        let parent = t
            .variation_parent
            .and_then(|p| types.get(&p))
            .map_or_else(|| t.name.clone(), |p| p.name.clone());
        (
            category_name(t),
            t.market_group.unwrap_or(-1),
            parent,
            MetaCategory::of_item(t).index(),
            t.meta_level,
            t.name.clone(),
            *id,
        )
    });
    for (rank, id) in order.into_iter().enumerate() {
        if let Some(t) = types.get_mut(&id) {
            t.market_rank = rank as u32;
        }
    }
}
