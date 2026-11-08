//! Moving items between the cargo hold and the fit, following pyfa's drag and drop commands
//! (`GuiLocalModuleToCargoCommand`, `GuiCargoToLocalModuleCommand`): whatever a move takes off
//! the fit lands in cargo, charges included, so nothing is lost along the way.

use super::{Fit, Module, ModuleState, trim_rack};
use crate::data::{Catalog, ItemType, Kind, Slot, TypeId};

/// Whether a drag leaves the source where it was. pyfa duplicates while Ctrl is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Move,
    Duplicate,
}

/// Charges of a type a module holds when full, counted from base capacity as pyfa does.
pub fn charges_per_module(catalog: &Catalog, module: TypeId, charge: TypeId) -> u32 {
    let (Some(module), Some(charge)) = (catalog.get(module), catalog.get(charge)) else {
        return 0;
    };
    if module.capacity <= 0.0 || charge.volume <= 0.0 {
        return 0;
    }
    // A hair of slack so 0.5 / 0.0125 counts as 40 and not 39.999.
    (module.capacity / charge.volume + 1e-6).floor() as u32
}

/// The best state a module can keep after turning into another type.
fn state_for(state: ModuleState, t: Option<&ItemType>) -> ModuleState {
    let activatable = t.is_some_and(|t| t.activatable);
    let overloadable = t.is_some_and(|t| t.overloadable);
    match state {
        ModuleState::Overload if overloadable => ModuleState::Overload,
        ModuleState::Overload | ModuleState::Active if activatable => ModuleState::Active,
        ModuleState::Offline => ModuleState::Offline,
        _ => ModuleState::Online,
    }
}

impl Fit {
    /// Take up to `count` of an item out of the hold, dropping the stack once it is empty.
    pub fn remove_cargo(&mut self, type_id: TypeId, count: u32) {
        if let Some(at) = self.cargo.iter().position(|&(t, _)| t == type_id) {
            let left = self.cargo[at].1.saturating_sub(count);
            if left == 0 {
                self.cargo.remove(at);
            } else {
                self.cargo[at].1 = left;
            }
        }
    }

    /// Fold stacks of the same item into the first one, as pyfa keeps one stack per item.
    pub fn merge_cargo(&mut self) {
        self.cargo = std::mem::take(&mut self.cargo)
            .into_iter()
            .collect::<crate::port::Tally<TypeId>>()
            .into_vec();
    }

    /// The item at a position and its loaded charge, if anything is there. A mutated module is
    /// its base item: cargo holds no mutations, as in pyfa.
    fn fitted(&self, (slot, i): (Slot, usize)) -> Option<(TypeId, Option<TypeId>)> {
        match slot {
            Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                let m = self.rack(slot).get(i)?.as_ref()?;
                Some((m.base_type(), m.charge))
            }
            Slot::Rig => self.rigs.get(i).copied().flatten().map(|r| (r, None)),
            Slot::Subsystem => self.subsystems.get(i).map(|&s| (s, None)),
        }
    }

    fn clear_position(&mut self, (slot, i): (Slot, usize)) {
        match slot {
            Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                let rack = self.rack_mut(slot);
                if let Some(position) = rack.get_mut(i) {
                    *position = None;
                }
                trim_rack(rack);
            }
            Slot::Rig => {
                if let Some(position) = self.rigs.get_mut(i) {
                    *position = None;
                }
                trim_rack(&mut self.rigs);
            }
            Slot::Subsystem if i < self.subsystems.len() => {
                self.subsystems.remove(i);
            }
            Slot::Subsystem => {}
        }
    }

    /// Put another type at a module or rig position. The module keeps what it can of its state
    /// and charge; charges it can no longer hold go to cargo, and a bigger magazine is topped up
    /// from cargo.
    fn refit(&mut self, catalog: &Catalog, (slot, i): (Slot, usize), type_id: TypeId) {
        let t = catalog.get(type_id);
        if slot == Slot::Rig {
            if self.rigs.len() <= i {
                self.rigs.resize(i + 1, None);
            }
            self.rigs[i] = Some(type_id);
            return;
        }
        let rack = self.rack_mut(slot);
        if rack.len() <= i {
            rack.resize_with(i + 1, || None);
        }
        let old = rack[i].take();
        let mut module = Module {
            type_id,
            state: state_for(old.as_ref().map_or(ModuleState::Active, |m| m.state), t),
            charge: None,
            spool: old
                .as_ref()
                .and_then(|m| m.spool)
                .filter(|_| t.is_some_and(|t| t.spools)),
            mutation: None,
            rah: old.as_ref().and_then(|m| m.rah.clone()),
        };
        let mut to_cargo = None;
        let mut from_cargo = None;
        if let Some((old_type, charge)) =
            old.as_ref().and_then(|m| Some((m.base_type(), m.charge?)))
        {
            let before = charges_per_module(catalog, old_type, charge);
            let fits = catalog
                .get(charge)
                .zip(t)
                .is_some_and(|(c, t)| c.fits_in(t));
            if fits {
                module.charge = Some(charge);
                let after = charges_per_module(catalog, type_id, charge);
                if after > before {
                    from_cargo = Some((charge, after - before));
                } else if before > after {
                    to_cargo = Some((charge, before - after));
                }
            } else {
                to_cargo = Some((charge, before));
            }
        }
        rack[i] = Some(module);
        if let Some((charge, n)) = to_cargo {
            self.add_cargo(charge, n);
        }
        if let Some((charge, n)) = from_cargo {
            self.remove_cargo(charge, n);
        }
    }

    /// A fitted item dragged into the hold. Dropped on a cargo module that fits the same rack,
    /// the two trade places; otherwise the item and its charges are put in cargo and, unless
    /// duplicating, taken off the fit. Returns whether anything changed.
    pub fn module_to_cargo(
        &mut self,
        catalog: &Catalog,
        from: (Slot, usize),
        onto: Option<TypeId>,
        transfer: Transfer,
    ) -> bool {
        let Some((type_id, charge)) = self.fitted(from) else {
            return false;
        };
        let onto = onto
            .filter(|_| transfer == Transfer::Move)
            .filter(|&t| self.cargo.iter().any(|&(c, _)| c == t))
            .and_then(|t| catalog.get(t))
            .filter(|t| matches!(t.kind, Kind::Module(_)));
        if let Some(onto) = onto {
            if onto.id == type_id {
                return false;
            }
            if onto.kind == Kind::Module(from.0) && from.0 != Slot::Subsystem {
                self.remove_cargo(onto.id, 1);
                self.add_cargo(type_id, 1);
                self.refit(catalog, from, onto.id);
                return true;
            }
        }
        self.add_cargo(type_id, 1);
        if let Some(charge) = charge {
            self.add_cargo(charge, charges_per_module(catalog, type_id, charge));
        }
        if transfer == Transfer::Move {
            self.clear_position(from);
        }
        true
    }

    /// A cargo stack dragged onto a slot. A charge loads into the module there, swapping out the
    /// old charge; a module of the slot's rack takes the position, and what was there goes to
    /// cargo. Moving takes what was used out of the hold. Returns whether anything changed.
    pub fn cargo_to_module(
        &mut self,
        catalog: &Catalog,
        type_id: TypeId,
        onto: (Slot, usize),
        transfer: Transfer,
    ) -> bool {
        if !self.cargo.iter().any(|&(t, _)| t == type_id) {
            return false;
        }
        let Some(t) = catalog.get(type_id) else {
            return false;
        };
        let (slot, i) = onto;
        match t.kind {
            Kind::Charge => {
                if !matches!(slot, Slot::High | Slot::Mid | Slot::Low) {
                    return false;
                }
                let Some(Some(module)) = self.rack(slot).get(i) else {
                    return false;
                };
                let (module_type, old_charge) = (module.base_type(), module.charge);
                let fits = catalog.get(module_type).is_some_and(|m| t.fits_in(m));
                if old_charge == Some(type_id) || !fits {
                    return false;
                }
                if transfer == Transfer::Move {
                    self.remove_cargo(type_id, charges_per_module(catalog, module_type, type_id));
                }
                if let Some(old) = old_charge {
                    self.add_cargo(old, charges_per_module(catalog, module_type, old));
                }
                if let Some(Some(module)) = self.rack_mut(slot).get_mut(i) {
                    module.charge = Some(type_id);
                }
                true
            }
            Kind::Module(rack) if rack == slot && slot != Slot::Subsystem => {
                let current = self.fitted(onto);
                if current.is_some_and(|(c, _)| c == type_id) {
                    return false;
                }
                if transfer == Transfer::Move {
                    self.remove_cargo(type_id, 1);
                }
                if let Some((c, _)) = current {
                    self.add_cargo(c, 1);
                }
                self.refit(catalog, onto, type_id);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_cargo_drops_empty_stacks() {
        let mut fit = Fit::default();
        fit.add_cargo(1, 5);
        fit.add_cargo(2, 1);
        fit.remove_cargo(1, 3);
        assert_eq!(fit.cargo, vec![(1, 2), (2, 1)]);
        fit.remove_cargo(1, 10);
        assert_eq!(fit.cargo, vec![(2, 1)]);
        fit.remove_cargo(3, 1);
        assert_eq!(fit.cargo, vec![(2, 1)]);
    }

    #[test]
    fn merging_keeps_first_position() {
        let mut fit = Fit {
            cargo: vec![(1, 2), (2, 1), (1, 3)],
            ..Fit::default()
        };
        fit.merge_cargo();
        assert_eq!(fit.cargo, vec![(1, 5), (2, 1)]);
    }

    #[test]
    fn states_fall_back_to_what_the_type_allows() {
        assert_eq!(state_for(ModuleState::Overload, None), ModuleState::Online);
        assert_eq!(state_for(ModuleState::Offline, None), ModuleState::Offline);
    }
}
