//! Fighter squadrons, kept apart from drones the way pyfa keeps them: a squadron is one item with
//! a size, an on/off state and abilities that switch on and off one by one.

use serde::{Deserialize, Serialize};

use super::{DroneStack, Fit};
use crate::data::{Catalog, Kind, TypeId};

/// pyfa's `FIGHTER_ORDER`, by the group a fighter is in.
const FIGHTER_ORDER: [&str; 6] = [
    "Light Fighter",
    "Structure Light Fighter",
    "Heavy Fighter",
    "Structure Heavy Fighter",
    "Support Fighter",
    "Structure Support Fighter",
];

/// Where a fighter's group comes in pyfa's `FIGHTER_ORDER`; other groups go last.
pub fn group_rank(catalog: &Catalog, type_id: TypeId) -> usize {
    let group = catalog
        .get(type_id)
        .and_then(|t| catalog.group_names.get(&t.group_id));
    group
        .and_then(|g| FIGHTER_ORDER.iter().position(|o| o == g))
        .unwrap_or(FIGHTER_ORDER.len())
}

/// A squadron in the fighter bay or in a tube.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fighter {
    pub type_id: TypeId,
    /// Fighters in the squadron. None keeps it at the full size, which skills and the ship can
    /// change, the way pyfa stores -1.
    #[serde(default)]
    pub amount: Option<u32>,
    /// Launched into a tube, or sitting in the bay.
    #[serde(default = "launched_default")]
    pub active: bool,
    /// Abilities switched away from the defaults, as (ability ID, on).
    #[serde(default)]
    pub abilities: Vec<(i32, bool)>,
}

fn launched_default() -> bool {
    true
}

impl Fighter {
    pub fn new(type_id: TypeId) -> Self {
        Self {
            type_id,
            amount: None,
            active: true,
            abilities: Vec::new(),
        }
    }

    /// A squadron of `size` fighters, kept at the full size when `size` reaches the type's
    /// squadron size, the way pyfa's importers set a fighter's amount.
    pub fn with_size(catalog: &Catalog, type_id: TypeId, size: u32) -> Self {
        let mut fighter = Self::new(type_id);
        fighter.set_amount(size, catalog.projection.squadron_size(type_id));
        fighter
    }

    /// Fighters in the squadron as the formats write it: the set amount, or the type's full
    /// squadron size.
    pub fn size(&self, catalog: &Catalog) -> u32 {
        self.amount
            .or_else(|| catalog.projection.squadron_size(self.type_id))
            .unwrap_or(1)
    }

    /// The user's choice for an ability, if they made one.
    pub fn ability_override(&self, ability: i32) -> Option<bool> {
        self.abilities
            .iter()
            .find(|&&(id, _)| id == ability)
            .map(|&(_, on)| on)
    }

    pub fn set_ability(&mut self, ability: i32, on: bool) {
        match self.abilities.iter_mut().find(|(id, _)| *id == ability) {
            Some(entry) => entry.1 = on,
            None => self.abilities.push((ability, on)),
        }
    }

    /// Change the squadron size, where `max` is its full size: the full size or more goes back to
    /// following the ship, like pyfa's amount setter.
    pub fn set_amount(&mut self, amount: u32, max: Option<u32>) {
        self.amount = match max {
            Some(max) if amount >= max => None,
            _ => Some(amount),
        };
    }
}

impl Fit {
    /// pyfa's fighter state click: when the clicked squadron is out, every target is recalled,
    /// otherwise every target launches.
    pub fn toggle_fighters(&mut self, clicked: usize, rows: &[usize]) {
        let Some(launch) = self.fighters.get(clicked).map(|f| !f.active) else {
            return;
        };
        for &i in rows.iter().chain([&clicked]) {
            if let Some(fighter) = self.fighters.get_mut(i) {
                fighter.active = launch;
            }
        }
    }

    /// Move fighters that older versions kept as drone stacks into the fighter list. Returns
    /// whether anything moved.
    pub fn move_fighters_out_of_drones(&mut self, catalog: &Catalog) -> bool {
        let is_fighter = |t: TypeId| catalog.get(t).is_some_and(|t| t.kind == Kind::Fighter);
        if !self.drones.iter().any(|d| is_fighter(d.type_id)) {
            return false;
        }
        let (fighters, drones): (Vec<DroneStack>, Vec<DroneStack>) =
            std::mem::take(&mut self.drones)
                .into_iter()
                .partition(|d| is_fighter(d.type_id));
        self.drones = drones;
        for stack in fighters {
            for _ in 0..stack.count.max(1) {
                self.fighters.push(Fighter::new(stack.type_id));
            }
        }
        true
    }

    /// Squadrons with every fighter of the same type, for Alt clicks: pyfa's similar fighters.
    pub fn similar_fighters(fighters: &[Fighter], clicked: usize) -> Vec<usize> {
        let Some(type_id) = fighters.get(clicked).map(|f| f.type_id) else {
            return Vec::new();
        };
        (0..fighters.len())
            .filter(|&i| fighters[i].type_id == type_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saves_without_fighter_fields_load() {
        let fighter: Fighter = serde_json::from_str(r#"{"type_id": 40556}"#).expect("parses");
        assert_eq!(fighter, Fighter::new(40556));
    }

    #[test]
    fn full_size_follows_the_ship() {
        let mut fighter = Fighter::new(1);
        fighter.set_amount(6, Some(9));
        assert_eq!(fighter.amount, Some(6));
        fighter.set_amount(12, Some(9));
        assert_eq!(fighter.amount, None);
    }

    #[test]
    fn toggling_follows_the_clicked_squadron() {
        let mut fit = Fit {
            fighters: vec![Fighter::new(1), Fighter::new(2)],
            ..Fit::default()
        };
        fit.fighters[1].active = false;
        fit.toggle_fighters(0, &[0, 1]);
        assert!(fit.fighters.iter().all(|f| !f.active));
        fit.toggle_fighters(1, &[0]);
        assert!(fit.fighters.iter().all(|f| f.active));
    }

    #[test]
    fn ability_overrides_replace_earlier_ones() {
        let mut fighter = Fighter::new(1);
        assert_eq!(fighter.ability_override(4), None);
        fighter.set_ability(4, true);
        fighter.set_ability(4, false);
        assert_eq!(fighter.abilities, [(4, false)]);
    }
}
