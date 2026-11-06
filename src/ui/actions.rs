//! Changes to the current fit, and which rows they apply to.
//!
//! Menus follow pyfa: an action applies to the selection when the clicked row is part of it, and
//! to just the clicked row otherwise. Some actions instead default to every similar module (same
//! type), with Ctrl or Alt swapping the two.

use std::collections::BTreeSet;

use gpui_kit::{ClickEvent, Context, Modifiers};

use super::Gyfa;
use crate::{
    data::{Slot, TypeId},
    fit::{Module, ModuleState, trim_rack},
};

/// A row of the fitting view that holds something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowRef {
    Module(Slot, usize),
    Rig(usize),
    Subsystem(usize),
}

/// Which modules a menu entry acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The selection, or the clicked row if it is not selected.
    Selection,
    /// Every module of the clicked row's type.
    Similar,
}

impl Scope {
    /// The scope a click gives: `default`, or the other one while Ctrl or Alt is held.
    pub fn from_click(default: Self, event: &ClickEvent) -> Self {
        Self::from_modifiers(default, event.modifiers())
    }

    fn from_modifiers(default: Self, modifiers: Modifiers) -> Self {
        if modifiers.control || modifiers.alt {
            match default {
                Self::Selection => Self::Similar,
                Self::Similar => Self::Selection,
            }
        } else {
            default
        }
    }
}

/// A change to the current fit, from a click or a menu.
#[derive(Debug, Clone)]
pub enum Action {
    CycleState(Slot, usize),
    SetState(Vec<RowRef>, ModuleState),
    SetCharge(Vec<RowRef>, Option<TypeId>),
    SetSpool(Vec<RowRef>, Option<u32>),
    Remove(Vec<RowRef>),
    /// Swap items for another type, keeping their state and charge.
    SwapType(Vec<RowRef>, TypeId),
    /// Put copies of a module into every free slot of its rack.
    FillRack(Slot, TypeId),
    /// Put an item at a rack position, replacing what is there.
    Place(Slot, usize, TypeId),
    /// Swap two positions of the same rack; either may be empty.
    Swap(RowRef, RowRef),
    SetDronesActive(usize, u32),
    AddCargo(TypeId, u32),
    AddDrones(TypeId, u32),
    /// Remove rows of an additions tab.
    RemoveAdditions(AddKind, Vec<usize>),
    /// Set the size of drone stacks or cargo stacks.
    SetAmount(AddKind, Vec<usize>, u32),
    /// Move this many drones out of a stack into a new one.
    SplitDrones(usize, u32),
    SwapAdditions(AddKind, Vec<usize>, TypeId),
    /// Add pasted items to a tab.
    AddAdditions(AddKind, Vec<(TypeId, u32)>),
    SetMode(TypeId),
    SetSecStatus(Option<f64>),
}

/// The additions pane's tabs that list items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddKind {
    Drones,
    Implants,
    Boosters,
    Cargo,
}

impl Gyfa {
    /// The rows an action on `clicked` should touch.
    pub(super) fn targets(&self, clicked: RowRef, scope: Scope) -> Vec<RowRef> {
        let Some(fit) = self.fit() else {
            return Vec::new();
        };
        match scope {
            Scope::Selection if self.selection.contains(&clicked) => self.selection.clone(),
            Scope::Selection => vec![clicked],
            Scope::Similar => {
                let Some(type_id) = self.row_type(clicked) else {
                    return vec![clicked];
                };
                match clicked {
                    RowRef::Module(..) => [Slot::High, Slot::Mid, Slot::Low]
                        .into_iter()
                        .flat_map(|slot| {
                            fit.rack(slot)
                                .iter()
                                .enumerate()
                                .filter(|(_, m)| m.as_ref().is_some_and(|m| m.type_id == type_id))
                                .map(move |(i, _)| RowRef::Module(slot, i))
                        })
                        .collect(),
                    RowRef::Rig(_) => (0..fit.rigs.len())
                        .filter(|&i| fit.rigs[i] == Some(type_id))
                        .map(RowRef::Rig)
                        .collect(),
                    RowRef::Subsystem(_) => vec![clicked],
                }
            }
        }
    }

    pub(super) fn row_type(&self, row: RowRef) -> Option<TypeId> {
        let fit = self.fit()?;
        match row {
            RowRef::Module(slot, i) => fit.rack(slot).get(i)?.as_ref().map(|m| m.type_id),
            RowRef::Rig(i) => fit.rigs.get(i).copied().flatten(),
            RowRef::Subsystem(i) => fit.subsystems.get(i).copied(),
        }
    }

    /// Click selection the way list views do it: plain click selects one row, Ctrl toggles,
    /// Shift extends from the last clicked row.
    pub(super) fn select(
        &mut self,
        row: RowRef,
        modifiers: Modifiers,
        order: &[RowRef],
        cx: &mut Context<Self>,
    ) {
        if modifiers.shift
            && let Some(anchor) = self.anchor
        {
            let position = |r: RowRef| order.iter().position(|&o| o == r);
            if let (Some(a), Some(b)) = (position(anchor), position(row)) {
                let (from, to) = (a.min(b), a.max(b));
                self.selection = order[from..=to].to_vec();
            }
        } else if modifiers.control {
            if let Some(at) = self.selection.iter().position(|&r| r == row) {
                self.selection.remove(at);
            } else {
                self.selection.push(row);
            }
            self.anchor = Some(row);
        } else {
            self.selection = vec![row];
            self.anchor = Some(row);
        }
        cx.notify();
    }

    pub(super) fn clear_selection(&mut self) {
        self.selection.clear();
        self.anchor = None;
        self.additions_selection.clear();
        self.additions_anchor = None;
    }

    pub fn apply(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let Some(current) = self.saved.current else {
            return;
        };
        let free = |slot: Slot, used: usize, stats: Option<&crate::engine::Stats>| {
            let total = stats.map_or(0, |s| match slot {
                Slot::High => s.high.total.unwrap_or(0),
                Slot::Mid => s.mid.total.unwrap_or(0),
                Slot::Low => s.low.total.unwrap_or(0),
                Slot::Rig => s.rig.total.unwrap_or(0),
                Slot::Subsystem => s.subsystem.total.unwrap_or(0),
            }) as usize;
            total.saturating_sub(used)
        };
        let stats = self.stats.as_ref();
        let fit = &mut self.saved.fits[current];
        let mut structural = false;
        match action {
            Action::CycleState(slot, i) => {
                let Some(Some(m)) = fit.rack_mut(slot).get_mut(i) else {
                    return;
                };
                let t = loaded.catalog.get(m.type_id);
                let activatable = t.is_some_and(|t| t.activatable);
                let overloadable = t.is_some_and(|t| t.overloadable);
                m.state = match m.state {
                    ModuleState::Offline => ModuleState::Online,
                    ModuleState::Online if activatable => ModuleState::Active,
                    ModuleState::Active if overloadable => ModuleState::Overload,
                    _ => ModuleState::Offline,
                };
            }
            Action::SetState(rows, new_state) => {
                for m in modules_mut(fit, &rows) {
                    m.state = new_state;
                }
            }
            Action::SetCharge(rows, charge) => {
                for m in modules_mut(fit, &rows) {
                    let fits = charge.is_none_or(|c| {
                        let (c, m) = (loaded.catalog.get(c), loaded.catalog.get(m.type_id));
                        c.zip(m).is_some_and(|(c, m)| c.fits_in(m))
                    });
                    if fits {
                        m.charge = charge;
                    }
                }
            }
            Action::SetSpool(rows, cycles) => {
                for m in modules_mut(fit, &rows) {
                    m.spool = cycles;
                }
            }
            Action::Remove(rows) => {
                // Modules and rigs leave their position empty; subsystems have no positions, so
                // remove from the end first to keep earlier indices valid.
                let rows: BTreeSet<RowRef> = rows.into_iter().collect();
                for row in rows.into_iter().rev() {
                    match row {
                        RowRef::Module(slot, i) => {
                            if let Some(position) = fit.rack_mut(slot).get_mut(i) {
                                *position = None;
                            }
                        }
                        RowRef::Rig(i) => {
                            if let Some(position) = fit.rigs.get_mut(i) {
                                *position = None;
                            }
                        }
                        RowRef::Subsystem(i) if i < fit.subsystems.len() => {
                            fit.subsystems.remove(i);
                        }
                        RowRef::Subsystem(_) => {}
                    }
                }
                for slot in [Slot::High, Slot::Mid, Slot::Low] {
                    trim_rack(fit.rack_mut(slot));
                }
                trim_rack(&mut fit.rigs);
                structural = true;
            }
            Action::Place(slot, i, type_id) => {
                let Some(t) = loaded.catalog.get(type_id) else {
                    return;
                };
                match slot {
                    Slot::High | Slot::Mid | Slot::Low => {
                        let rack = fit.rack_mut(slot);
                        if rack.len() <= i {
                            rack.resize_with(i + 1, || None);
                        }
                        let initial = if t.activatable {
                            ModuleState::Active
                        } else {
                            ModuleState::Online
                        };
                        rack[i] = Some(Module {
                            type_id,
                            state: initial,
                            charge: None,
                            spool: None,
                        });
                    }
                    Slot::Rig => {
                        if fit.rigs.len() <= i {
                            fit.rigs.resize(i + 1, None);
                        }
                        fit.rigs[i] = Some(type_id);
                    }
                    Slot::Subsystem => {
                        fit.add(&loaded.catalog, type_id);
                    }
                }
            }
            Action::Swap(a, b) => {
                match (a, b) {
                    (RowRef::Module(slot_a, i), RowRef::Module(slot_b, j)) if slot_a == slot_b => {
                        let rack = fit.rack_mut(slot_a);
                        if rack.len() <= i.max(j) {
                            rack.resize_with(i.max(j) + 1, || None);
                        }
                        rack.swap(i, j);
                        trim_rack(rack);
                    }
                    (RowRef::Rig(i), RowRef::Rig(j)) => {
                        if fit.rigs.len() <= i.max(j) {
                            fit.rigs.resize(i.max(j) + 1, None);
                        }
                        fit.rigs.swap(i, j);
                        trim_rack(&mut fit.rigs);
                    }
                    _ => return,
                }
                structural = true;
            }
            Action::SwapType(rows, type_id) => {
                for row in rows {
                    match row {
                        RowRef::Module(slot, i) => {
                            if let Some(Some(m)) = fit.rack_mut(slot).get_mut(i) {
                                m.type_id = type_id;
                                // The new variation may not take the old charge.
                                let keeps = m.charge.is_some_and(|c| {
                                    let (c, t) =
                                        (loaded.catalog.get(c), loaded.catalog.get(type_id));
                                    c.zip(t).is_some_and(|(c, t)| c.fits_in(t))
                                });
                                if !keeps {
                                    m.charge = None;
                                }
                            }
                        }
                        RowRef::Rig(i) => {
                            if let Some(r) = fit.rigs.get_mut(i).filter(|r| r.is_some()) {
                                *r = Some(type_id);
                            }
                        }
                        RowRef::Subsystem(i) => {
                            if let Some(s) = fit.subsystems.get_mut(i) {
                                *s = type_id;
                            }
                        }
                    }
                }
            }
            Action::FillRack(slot, type_id) => {
                let used = match slot {
                    Slot::Rig => fit.rigs.iter().flatten().count(),
                    Slot::Subsystem => fit.subsystems.len(),
                    Slot::High | Slot::Mid | Slot::Low => fit.rack(slot).iter().flatten().count(),
                };
                for _ in 0..free(slot, used, stats) {
                    fit.add(&loaded.catalog, type_id);
                }
            }
            Action::SetDronesActive(i, n) => fit.drones[i].active = n.min(fit.drones[i].count),
            Action::AddCargo(type_id, count) => fit.add_cargo(type_id, count),
            Action::AddDrones(type_id, count) => fit.add_drones(type_id, count),
            Action::RemoveAdditions(kind, rows) => {
                let rows: BTreeSet<usize> = rows.into_iter().collect();
                for i in rows.into_iter().rev() {
                    match kind {
                        AddKind::Drones if i < fit.drones.len() => {
                            fit.drones.remove(i);
                        }
                        AddKind::Implants if i < fit.implants.len() => {
                            fit.implants.remove(i);
                        }
                        AddKind::Boosters if i < fit.boosters.len() => {
                            fit.boosters.remove(i);
                        }
                        AddKind::Cargo if i < fit.cargo.len() => {
                            fit.cargo.remove(i);
                        }
                        _ => {}
                    }
                }
                structural = true;
            }
            Action::SetAmount(kind, rows, amount) => {
                for i in rows {
                    match kind {
                        AddKind::Drones => {
                            if let Some(stack) = fit.drones.get_mut(i) {
                                stack.count = amount.max(1);
                                stack.active = stack.active.min(stack.count);
                            }
                        }
                        AddKind::Cargo => {
                            if let Some(stack) = fit.cargo.get_mut(i) {
                                stack.1 = amount.max(1);
                            }
                        }
                        AddKind::Implants | AddKind::Boosters => {}
                    }
                }
            }
            Action::SplitDrones(i, amount) => {
                if let Some(stack) = fit.drones.get_mut(i)
                    && amount > 0
                    && amount < stack.count
                {
                    stack.count -= amount;
                    let moved_active = stack.active.saturating_sub(stack.count);
                    stack.active -= moved_active;
                    let split = crate::fit::DroneStack {
                        type_id: stack.type_id,
                        count: amount,
                        active: moved_active,
                    };
                    fit.drones.insert(i + 1, split);
                }
            }
            Action::SwapAdditions(kind, rows, type_id) => {
                for i in rows {
                    match kind {
                        AddKind::Drones => {
                            if let Some(stack) = fit.drones.get_mut(i) {
                                stack.type_id = type_id;
                            }
                        }
                        AddKind::Implants => {
                            if let Some(t) = fit.implants.get_mut(i) {
                                *t = type_id;
                            }
                        }
                        AddKind::Boosters => {
                            if let Some(t) = fit.boosters.get_mut(i) {
                                *t = type_id;
                            }
                        }
                        AddKind::Cargo => {
                            if let Some(stack) = fit.cargo.get_mut(i) {
                                stack.0 = type_id;
                            }
                        }
                    }
                }
            }
            Action::SetMode(mode) => fit.mode = Some(mode),
            Action::SetSecStatus(sec) => fit.sec_status = sec,
            Action::AddAdditions(kind, items) => {
                for (type_id, count) in items {
                    match kind {
                        AddKind::Drones => fit.add_drones(type_id, count),
                        AddKind::Implants => fit.implants.push(type_id),
                        AddKind::Boosters => fit.boosters.push(type_id),
                        AddKind::Cargo => fit.add_cargo(type_id, count),
                    }
                }
            }
        }
        if structural {
            self.clear_selection();
        }
        self.changed(cx);
    }
}

fn modules_mut<'f>(fit: &'f mut crate::fit::Fit, rows: &[RowRef]) -> Vec<&'f mut Module> {
    let wanted: BTreeSet<(Slot, usize)> = rows
        .iter()
        .filter_map(|r| match *r {
            RowRef::Module(slot, i) => Some((slot, i)),
            RowRef::Rig(_) | RowRef::Subsystem(_) => None,
        })
        .collect();
    let crate::fit::Fit { high, mid, low, .. } = fit;
    [(Slot::High, high), (Slot::Mid, mid), (Slot::Low, low)]
        .into_iter()
        .flat_map(|(slot, rack)| {
            rack.iter_mut()
                .enumerate()
                .filter(|(i, _)| wanted.contains(&(slot, *i)))
                .filter_map(|(_, m)| m.as_mut())
                .collect::<Vec<_>>()
        })
        .collect()
}
