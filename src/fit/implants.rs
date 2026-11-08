//! Boosters and implants on a fit: adding and swapping them, slot order, switching them on and off,
//! and whose implants count.

use serde::{Deserialize, Serialize};

use super::{Fit, ImplantSource};
use crate::data::{Catalog, TypeId};

/// A booster and the switches pyfa keeps for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BoosterSave")]
pub struct Booster {
    pub type_id: TypeId,
    pub active: bool,
    /// Side effects switched on, by effect ID. pyfa starts every booster with none.
    pub side_effects: Vec<i32>,
}

impl Booster {
    pub fn new(type_id: TypeId) -> Self {
        Self {
            type_id,
            active: true,
            side_effects: Vec::new(),
        }
    }
}

/// Saves from before boosters had switches hold bare type IDs.
#[derive(Deserialize)]
#[serde(untagged)]
enum BoosterSave {
    Bare(TypeId),
    Full {
        type_id: TypeId,
        #[serde(default = "active_default")]
        active: bool,
        #[serde(default)]
        side_effects: Vec<i32>,
    },
}

fn active_default() -> bool {
    true
}

impl From<BoosterSave> for Booster {
    fn from(save: BoosterSave) -> Self {
        match save {
            BoosterSave::Bare(type_id) => Self::new(type_id),
            BoosterSave::Full {
                type_id,
                active,
                side_effects,
            } => Self {
                type_id,
                active,
                side_effects,
            },
        }
    }
}

impl Fit {
    /// Add a booster the way pyfa does: one already on the fit is left alone, and one sharing its
    /// slot is replaced. Returns whether the fit changed.
    pub fn add_booster(&mut self, catalog: &Catalog, booster: Booster) -> bool {
        if self.boosters.iter().any(|b| b.type_id == booster.type_id) {
            return false;
        }
        let slot = |id: TypeId| catalog.get(id).and_then(|t| t.booster_slot);
        let wanted = slot(booster.type_id);
        if wanted.is_some() {
            self.boosters.retain(|b| slot(b.type_id) != wanted);
        }
        self.boosters.push(booster);
        // pyfa lists boosters by slot; keeping them stored that way makes rows and indices agree.
        self.boosters.sort_by_key(|b| slot(b.type_id).unwrap_or(0));
        true
    }

    /// Swap a booster for a variation, keeping whether it is on and those of its side effects the
    /// new one has too.
    pub fn swap_booster(&mut self, catalog: &Catalog, index: usize, type_id: TypeId) -> bool {
        let Some(old) = self.boosters.get(index) else {
            return false;
        };
        if self.boosters.iter().any(|b| b.type_id == type_id) {
            return false;
        }
        let effects = catalog
            .get(type_id)
            .map_or(&[][..], |t| t.side_effects.as_slice());
        let new = Booster {
            type_id,
            active: old.active,
            side_effects: old
                .side_effects
                .iter()
                .copied()
                .filter(|e| effects.contains(e))
                .collect(),
        };
        self.boosters.remove(index);
        self.add_booster(catalog, new)
    }

    /// Plug in an implant the way pyfa does: the fit switches to its own implants, an implant
    /// already in that slot makes way, and the list stays in slot order. Returns false when
    /// nothing changed.
    pub fn add_implant(&mut self, catalog: &Catalog, type_id: TypeId) -> bool {
        let switched = self.implant_source != ImplantSource::Fit;
        self.implant_source = ImplantSource::Fit;
        let plugged = plug_implant(&mut self.implants, catalog, type_id);
        self.inactive_implants.retain(|t| self.implants.contains(t));
        switched || plugged
    }

    pub fn remove_implant(&mut self, i: usize) {
        if i < self.implants.len() {
            let gone = self.implants.remove(i);
            self.inactive_implants.retain(|&t| t != gone);
        }
    }

    /// Swap an implant for another type, keeping whether it is switched on.
    pub fn swap_implant(&mut self, catalog: &Catalog, i: usize, type_id: TypeId) {
        let Some(&old) = self.implants.get(i) else {
            return;
        };
        if old == type_id {
            return;
        }
        let active = self.implant_active(old);
        self.remove_implant(i);
        self.add_implant(catalog, type_id);
        if !active && self.implants.contains(&type_id) {
            self.inactive_implants.push(type_id);
        }
    }

    pub fn implant_active(&self, type_id: TypeId) -> bool {
        !self.inactive_implants.contains(&type_id)
    }

    /// pyfa's state toggle: if the clicked implant is on, switch every target off, otherwise
    /// switch them all on.
    pub fn toggle_implants(&mut self, clicked: usize, rows: &[usize]) {
        let Some(&main) = self.implants.get(clicked) else {
            return;
        };
        let turn_on = !self.implant_active(main);
        let targets: Vec<TypeId> = rows
            .iter()
            .chain([&clicked])
            .filter_map(|&r| self.implants.get(r).copied())
            .collect();
        self.inactive_implants.retain(|t| !targets.contains(t));
        if !turn_on {
            self.inactive_implants.extend(targets);
            self.inactive_implants.sort_unstable();
            self.inactive_implants.dedup();
        }
    }

    /// Put the implants in slot order, which older saves did not keep. Returns whether any moved.
    pub fn sort_implants(&mut self, catalog: &Catalog) -> bool {
        if self.implants_in_slot_order(catalog) {
            return false;
        }
        let slot = |&t: &TypeId| implant_slot(catalog, t);
        self.implants.sort_by_key(slot);
        true
    }

    /// Whether the implants are in slot order, as the Implants tab lists them.
    pub fn implants_in_slot_order(&self, catalog: &Catalog) -> bool {
        self.implants
            .is_sorted_by_key(|&t: &TypeId| implant_slot(catalog, t))
    }

    /// Implants the fit is calculated with, as (type, switched on): its own, or `character`'s
    /// when the fit uses the character's implants.
    pub fn applied_implants<'a>(
        &'a self,
        character: &'a [TypeId],
    ) -> impl Iterator<Item = (TypeId, bool)> + 'a {
        let (implants, own) = match self.implant_source {
            ImplantSource::Fit => (self.implants.as_slice(), true),
            ImplantSource::Character => (character, false),
        };
        implants
            .iter()
            .map(move |&t| (t, !own || self.implant_active(t)))
    }
}

/// The slot an implant takes; implants without one sort last.
pub fn implant_slot(catalog: &Catalog, type_id: TypeId) -> u8 {
    catalog
        .get(type_id)
        .and_then(|t| t.implant_slot)
        .unwrap_or(u8::MAX)
}

/// Add an implant to a slot-ordered list, replacing whatever is in its slot. Returns false when
/// it is already there or is not an implant.
pub fn plug_implant(implants: &mut Vec<TypeId>, catalog: &Catalog, type_id: TypeId) -> bool {
    let Some(slot) = catalog.get(type_id).and_then(|t| t.implant_slot) else {
        return false;
    };
    if implants.contains(&type_id) {
        return false;
    }
    implants.retain(|&t| implant_slot(catalog, t) != slot);
    let at = implants.partition_point(|&t| implant_slot(catalog, t) <= slot);
    implants.insert(at, type_id);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boosters_from_old_saves_load() {
        let old: Vec<Booster> = serde_json::from_str("[10155]").expect("old boosters parse");
        assert_eq!(old, [Booster::new(10155)]);

        let booster = Booster {
            type_id: 10155,
            active: false,
            side_effects: vec![2737],
        };
        let json = serde_json::to_string(&booster).expect("booster serializes");
        let again: Booster = serde_json::from_str(&json).expect("booster parses");
        assert_eq!(again, booster);
    }
}
