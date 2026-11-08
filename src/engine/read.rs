//! Reading numbers back from a fit built into a solar system: attributes after bonuses, the
//! effects an item carries, capacitor use and mining.

use std::collections::{HashMap, HashSet};

use rc::{AttrId, EveAttrId, ItemCommon, ItemId, Sol, stats::StatMining};

use super::MiningRate;
use crate::dogma::attr;

/// An attribute after bonuses, None when the item lacks it.
pub(super) fn attr(sol: &mut Sol, id: ItemId, attr: i32) -> Option<f64> {
    let mut item = sol.get_item(&id).ok()?;
    item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
        .ok()
        .map(|v| v.modified.into_f64())
}

/// An attribute before anything modifies it, None when the item lacks it.
pub(super) fn base_attr(sol: &mut Sol, id: ItemId, attr: i32) -> Option<f64> {
    let mut item = sol.get_item(&id).ok()?;
    item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
        .ok()
        .map(|v| v.base.into_f64())
}

/// An attribute after bonuses, 0 when the item lacks it.
pub(super) fn attr_or_zero(sol: &mut Sol, id: ItemId, attr: i32) -> f64 {
    self::attr(sol, id, attr).unwrap_or(0.0)
}

/// An attribute after bonuses when the item has it above zero.
pub(super) fn positive_attr(sol: &mut Sol, id: ItemId, attr: i32) -> Option<f64> {
    self::attr(sol, id, attr).filter(|&v| v > 0.0)
}

/// The first of `attrs` the item has a nonzero value for, 0 when none.
pub(super) fn first_attr(sol: &mut Sol, id: ItemId, attrs: &[i32]) -> f64 {
    attrs
        .iter()
        .map(|&a| attr_or_zero(sol, id, a))
        .find(|&v| v != 0.0)
        .unwrap_or(0.0)
}

/// The ones of `wanted` the item has, after bonuses.
pub(super) fn attrs(sol: &mut Sol, id: ItemId, wanted: &[i32]) -> HashMap<i32, f64> {
    let Ok(mut item) = sol.get_item(&id) else {
        return HashMap::new();
    };
    wanted
        .iter()
        .filter_map(|&attr| {
            let value = item
                .get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
                .ok()?;
            Some((attr, value.modified.into_f64()))
        })
        .collect()
}

/// The dogma effects an item carries, running or not.
pub(super) fn effects(sol: &mut Sol, id: ItemId) -> HashSet<i32> {
    let Ok(item) = sol.get_item(&id) else {
        return HashSet::new();
    };
    item.iter_effects().map_or_default(|effects| {
        effects
            .filter_map(|(effect, _)| match effect {
                rc::EffectId::Dogma(id) => Some(id.into_i32()),
                _ => None,
            })
            .collect()
    })
}

/// Capacitor a module uses per second while it cycles, None when it uses none. Missing
/// attributes read as 0, so turrets and launchers, which cycle on their rate of fire, are told
/// apart from everything else, which cycles on its duration, by that.
pub(super) fn cap_per_sec(sol: &mut Sol, id: ItemId) -> Option<f64> {
    let need = positive_attr(sol, id, attr::CAPACITOR_NEED)?;
    let cycle = positive_attr(sol, id, attr::DURATION)
        .or_else(|| positive_attr(sol, id, attr::RATE_OF_FIRE))?;
    Some(need / (cycle / 1000.0))
}

/// Ore, ice and gas mined together.
pub(super) fn mining_rate(m: &StatMining) -> MiningRate {
    let entries = [&m.ore, &m.ice, &m.gas];
    MiningRate {
        yield_: entries.iter().map(|e| e.yield_.into_f64()).sum(),
        drain: entries.iter().map(|e| e.drain.into_f64()).sum(),
    }
}

/// Fighters give their damage reduction as a factor and a sensitivity; pyfa folds them into one
/// exponent.
pub(super) fn reduction_exponent(factor: f64, sensitivity: f64) -> f64 {
    if factor > 0.0 && sensitivity > 0.0 && (sensitivity - 1.0).abs() > f64::EPSILON {
        factor.ln() / sensitivity.ln()
    } else {
        0.0
    }
}
