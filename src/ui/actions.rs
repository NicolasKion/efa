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
    fit::{
        DamageProfile, ImplantSource, Module, ModuleState, Mutation, RahPattern, SystemSecurity,
        cargo::Transfer, trim_rack,
    },
    prices::optimize,
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
    /// pyfa's state column click: the clicked module steps to its next state, or goes offline
    /// with `offline`, and `others` follow it as far as each can.
    CycleState {
        main: (Slot, usize),
        others: Vec<RowRef>,
        offline: bool,
    },
    SetCharge(Vec<RowRef>, Option<TypeId>),
    SetSpool(Vec<RowRef>, Option<u32>),
    /// What reactive armor hardeners adapt to; None goes back to the fit's pattern.
    SetRah(Vec<RowRef>, Option<RahPattern>),
    Remove(Vec<RowRef>),
    /// Swap items for another type, keeping their state and charge.
    SwapType(Vec<RowRef>, TypeId),
    /// Add pasted drone stacks: plain ones join stacks of their drone, mutated ones stand alone.
    AddDroneStacks(Vec<crate::fit::DroneStack>),
    /// Add a mutated module or drone, as a pasted mutated item comes in.
    AddMutated(TypeId, Mutation),
    /// Apply a mutaplasmid to modules or to a drone stack.
    Mutate(MutaTarget, TypeId),
    /// Turn mutated modules or a drone stack back into their base item.
    Revert(MutaTarget),
    /// Put copies of a module into every free slot of its rack.
    FillRack(Slot, TypeId),
    /// Copy a fitted module, charge, state and mutation included, into every free slot of its
    /// rack, pyfa's "Fill With Module" on a fitted one.
    FillClone(Slot, usize),
    /// Put an item at a rack position, replacing what is there.
    Place(Slot, usize, TypeId),
    /// Swap two positions of the same rack; either may be empty.
    Swap(RowRef, RowRef),
    /// Copy a fitted module or rig, charge, state and mutation included, into an empty
    /// position of its rack, pyfa's Ctrl-drag.
    Clone(RowRef, RowRef),
    SetDronesActive(usize, u32),
    AddCargo(TypeId, u32),
    AddDrones(TypeId, u32),
    /// Remove rows of an additions tab.
    RemoveAdditions(AddKind, Vec<usize>),
    /// Set the size of drone stacks or cargo stacks.
    SetAmount(AddKind, Vec<usize>, u32),
    /// Move this many drones out of a stack into a new one.
    SplitDrones(usize, u32),
    /// pyfa's drone state click: the clicked stack and the stacks it applies to.
    ToggleDrones(usize, Vec<usize>),
    /// Take a single drone out of a stack.
    RemoveDrone(usize),
    /// Fold one drone stack into another of the same drone.
    MergeDrones {
        from: usize,
        to: usize,
    },
    /// Copy a drone stack into a new stack.
    CloneDrones(usize),
    SwapAdditions(AddKind, Vec<usize>, TypeId),
    /// Add pasted items to a tab.
    AddAdditions(AddKind, Vec<(TypeId, u32)>),
    SetMode(TypeId),
    SetSecStatus(Option<f64>),
    SetSystemSecurity(SystemSecurity),
    /// Switch boosters on or off, all the opposite of the clicked one, as pyfa does.
    ToggleBoosters(usize, Vec<usize>),
    /// Switch one side effect of a booster, by effect ID.
    ToggleBoosterSideEffect(usize, i32),
    /// Switch implants on or off the way pyfa's state column does: the clicked row decides.
    ToggleImplants(usize, Vec<usize>),
    SetImplantSource(ImplantSource),
    /// Plug in a set of implants, each replacing what is in its slot.
    AddImplants(Vec<TypeId>),
    /// A fitted item dragged into cargo, optionally onto a cargo stack of this type.
    ModuleToCargo((Slot, usize), Option<TypeId>, Transfer),
    /// A cargo stack dragged onto a slot position.
    CargoToModule(TypeId, (Slot, usize), Transfer),
    SetDamageProfile(DamageProfile),
    SetTargetProfile(Option<crate::target_profile::TargetProfile>),
    /// Group a rack's modules by type, or stop grouping them.
    ToggleGrouped(Slot),
    /// The fitting wizard's modules at their positions, the `clear` racks emptied first.
    /// Modules the fitting rules refuse are left out.
    ApplyWizard {
        placements: Vec<crate::wizard::Placement>,
        clear: crate::wizard::Cleared,
    },
    /// pyfa's Optimize Fit Price: swap items for their cheapest replacement at the current
    /// prices, leaving out swaps the fitting rules refuse.
    OptimizePrice,
    /// Put a cargo preset into the hold as its policy says.
    ApplyCargoPreset(crate::cargo_preset::CargoPreset),
}

/// What a mutaplasmid is applied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutaTarget {
    Modules(Vec<RowRef>),
    Drones(usize),
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
                    RowRef::Module(..) => [Slot::High, Slot::Mid, Slot::Low, Slot::Service]
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
        // Kept for undo; recorded only if the action changed the fit.
        let before = self.saved.fits[current].clone();
        // Actions that put modules on are checked against the fitting rules afterwards; filling
        // a rack checks each copy as it goes.
        let checked = matches!(
            action,
            Action::Place(..)
                | Action::SwapType(..)
                | Action::CargoToModule(..)
                | Action::Clone(..)
        );
        let pilot = self.pilot(&loaded);
        let mut refused: Option<String> = None;
        let free = |slot: Slot, used: usize, stats: Option<&crate::engine::Stats>| {
            let total = stats.map_or(0, |s| s.slots(slot).total.unwrap_or(0)) as usize;
            total.saturating_sub(used)
        };
        let stats = self.stats.as_ref();
        let prices = std::rc::Rc::clone(&self.prices);
        let mut report: Option<String> = None;
        let implants_in_cargo = self.saved.preferences.general.implants_in_cargo;
        let fit = &mut self.saved.fits[current];
        // In a grouped rack, whatever happens to a module happens to its whole group.
        let action = with_groups(fit, action);
        let mut structural = false;
        match action {
            Action::CycleState {
                main: (slot, i),
                others,
                offline,
            } => {
                let group = fit.group(slot, i);
                let Some(Some(m)) = fit.rack_mut(slot).get_mut(i) else {
                    return;
                };
                let highest = highest_state(&loaded.catalog, m.type_id);
                let next = if offline {
                    ModuleState::Offline
                } else {
                    match m.state {
                        ModuleState::Offline => ModuleState::Online,
                        ModuleState::Online if highest >= ModuleState::Active => {
                            ModuleState::Active
                        }
                        ModuleState::Active if highest >= ModuleState::Overload => {
                            ModuleState::Overload
                        }
                        _ => ModuleState::Offline,
                    }
                };
                let rows: Vec<RowRef> = group
                    .into_iter()
                    .map(|j| RowRef::Module(slot, j))
                    .chain(others)
                    .collect();
                for m in modules_mut(fit, &rows) {
                    m.state = next.min(highest_state(&loaded.catalog, m.type_id));
                }
            }
            Action::ToggleGrouped(slot) => {
                if !fit.grouped.remove(&slot) {
                    fit.grouped.insert(slot);
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
            Action::SetRah(rows, pattern) => {
                for m in modules_mut(fit, &rows) {
                    m.rah.clone_from(&pattern);
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
                if loaded.catalog.get(type_id).is_none() {
                    return;
                }
                match slot {
                    Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                        let rack = fit.rack_mut(slot);
                        if rack.len() <= i {
                            rack.resize_with(i + 1, || None);
                        }
                        rack[i] = Some(Module::new(&loaded.catalog, type_id));
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
            Action::Clone(from, to) => match (from, to) {
                (RowRef::Module(slot_a, i), RowRef::Module(slot_b, j))
                    if slot_a == slot_b && i != j =>
                {
                    let rack = fit.rack_mut(slot_a);
                    let Some(Some(module)) = rack.get(i).cloned() else {
                        return;
                    };
                    if rack.get(j).is_some_and(Option::is_some) {
                        return;
                    }
                    if rack.len() <= j {
                        rack.resize_with(j + 1, || None);
                    }
                    rack[j] = Some(module);
                }
                (RowRef::Rig(i), RowRef::Rig(j)) if i != j => {
                    let Some(Some(rig)) = fit.rigs.get(i).copied() else {
                        return;
                    };
                    if fit.rigs.get(j).is_some_and(Option::is_some) {
                        return;
                    }
                    if fit.rigs.len() <= j {
                        fit.rigs.resize(j + 1, None);
                    }
                    fit.rigs[j] = Some(rig);
                }
                _ => return,
            },
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
            Action::AddDroneStacks(stacks) => {
                for stack in stacks {
                    if stack.mutation.is_some() {
                        fit.add_drone_stack(stack);
                    } else {
                        fit.add_drones(stack.type_id, stack.count);
                    }
                }
            }
            Action::AddMutated(type_id, mutation) => {
                if !fit.add_mutated(&loaded.catalog, type_id, mutation) {
                    return;
                }
            }
            Action::Mutate(target, mutaplasmid) => match target {
                MutaTarget::Modules(rows) => {
                    for m in modules_mut(fit, &rows) {
                        m.mutate(&loaded.catalog, mutaplasmid);
                    }
                }
                MutaTarget::Drones(i) => {
                    if let Some(stack) = fit.drones.get_mut(i) {
                        stack.mutate(&loaded.catalog, mutaplasmid);
                    }
                }
            },
            Action::Revert(target) => match target {
                MutaTarget::Modules(rows) => {
                    for m in modules_mut(fit, &rows) {
                        m.revert();
                    }
                }
                MutaTarget::Drones(i) => {
                    if let Some(stack) = fit.drones.get_mut(i) {
                        stack.revert();
                    }
                }
            },
            Action::FillRack(slot, type_id) => {
                let used = match slot {
                    Slot::Rig => fit.rigs.iter().flatten().count(),
                    Slot::Subsystem => fit.subsystems.len(),
                    Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                        fit.rack(slot).iter().flatten().count()
                    }
                };
                // Like pyfa, copies go on until the rack is full or the next one is refused.
                let mut base = loaded.engine.validate(fit, &pilot);
                for _ in 0..free(slot, used, stats) {
                    let mut next = fit.clone();
                    next.add(&loaded.catalog, type_id);
                    let after = loaded.engine.validate(&next, &pilot);
                    if let Some(problem) = base.refusal(&after, next.ignore_restrictions) {
                        refused = Some(problem.describe(|t| loaded.catalog.name(t)));
                        break;
                    }
                    *fit = next;
                    base = after;
                }
            }
            Action::ApplyWizard { placements, clear } => {
                if clear.any() {
                    crate::wizard::apply(fit, &[], clear);
                }
                let mut base = loaded.engine.validate(fit, &pilot);
                for placement in placements {
                    let mut next = fit.clone();
                    crate::wizard::apply(
                        &mut next,
                        std::slice::from_ref(&placement),
                        crate::wizard::Cleared::NONE,
                    );
                    let after = loaded.engine.validate(&next, &pilot);
                    if let Some(problem) = base.refusal(&after, next.ignore_restrictions) {
                        refused = Some(problem.describe(|t| loaded.catalog.name(t)));
                        continue;
                    }
                    *fit = next;
                    base = after;
                }
                structural = true;
            }
            Action::OptimizePrice => {
                let catalog = &loaded.catalog;
                let price = |t: TypeId| prices.get(t);
                let was = optimize::value(fit, catalog, price);
                let mut base = loaded.engine.validate(fit, &pilot);
                let made = optimize::optimize(fit, catalog, price, |_, next| {
                    let after = loaded.engine.validate(next, &pilot);
                    let refused = base.refusal(&after, next.ignore_restrictions).is_some();
                    if !refused {
                        base = after;
                    }
                    refused
                });
                let saving = was - optimize::value(fit, catalog, price);
                report = Some(if made.is_empty() {
                    "No cheaper replacements found".to_owned()
                } else {
                    format!(
                        "Swapped {} item type{} for cheaper ones, saving {} ISK",
                        made.len(),
                        if made.len() == 1 { "" } else { "s" },
                        super::stats::format::currency(saving)
                    )
                });
                structural = true;
            }
            Action::FillClone(slot, i) => {
                let Some(Some(template)) = fit.rack(slot).get(i).cloned() else {
                    return;
                };
                let used = fit.rack(slot).iter().flatten().count();
                for _ in 0..free(slot, used, stats) {
                    fit.place_module(slot, template.clone());
                }
            }
            Action::SetDronesActive(i, n) => fit.drones[i].active = n.min(fit.drones[i].count),
            Action::AddCargo(type_id, count) => fit.add_cargo(type_id, count),
            Action::ApplyCargoPreset(preset) => {
                let applied = crate::cargo_preset::apply(&preset, &mut fit.cargo);
                let noun = crate::cargo_preset::noun(&loaded.catalog, &preset);
                let mut text = format!("{}: {}", preset.name, applied.summary(&noun));
                let capacity = stats.and_then(|s| s.cargo_capacity);
                let used = fit.hold_volume(&loaded.catalog, implants_in_cargo);
                if let Some(capacity) = capacity
                    && applied.changed()
                    && used > capacity + 1e-6
                {
                    use std::fmt::Write;
                    let _ = write!(
                        text,
                        ". The hold is overfull: {} / {} m³",
                        super::widgets::short(used),
                        super::widgets::short(capacity)
                    );
                }
                report = Some(text);
                structural = applied.removed > 0;
            }
            Action::AddDrones(type_id, count) => fit.add_drones(type_id, count),
            Action::RemoveAdditions(kind, rows) => {
                let rows: BTreeSet<usize> = rows.into_iter().collect();
                for i in rows.into_iter().rev() {
                    match kind {
                        AddKind::Drones if i < fit.drones.len() => {
                            fit.drones.remove(i);
                        }
                        AddKind::Implants => fit.remove_implant(i),
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
                                stack.1 = amount;
                            }
                        }
                        AddKind::Implants | AddKind::Boosters => {}
                    }
                }
                // pyfa removes cargo stacks set to zero.
                if kind == AddKind::Cargo && amount == 0 {
                    fit.cargo.retain(|&(_, n)| n > 0);
                    structural = true;
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
                        count: amount,
                        active: moved_active,
                        ..stack.clone()
                    };
                    fit.drones.insert(i + 1, split);
                }
            }
            Action::ToggleDrones(clicked, stacks) => fit.toggle_drones(clicked, &stacks),
            Action::RemoveDrone(i) => {
                fit.remove_drone(i);
                structural = true;
            }
            Action::MergeDrones { from, to } => {
                if !fit.merge_drones(from, to) {
                    return;
                }
                structural = true;
            }
            Action::CloneDrones(i) => fit.clone_drones(i),
            Action::SwapAdditions(kind, rows, type_id) => {
                for i in rows {
                    match kind {
                        AddKind::Drones => {
                            if let Some(stack) = fit.drones.get_mut(i) {
                                stack.type_id = type_id;
                            }
                        }
                        AddKind::Implants => fit.swap_implant(&loaded.catalog, i, type_id),
                        AddKind::Boosters => {
                            fit.swap_booster(&loaded.catalog, i, type_id);
                        }
                        AddKind::Cargo => {
                            if let Some(stack) = fit.cargo.get_mut(i) {
                                stack.0 = type_id;
                            }
                        }
                    }
                }
                if kind == AddKind::Cargo {
                    fit.merge_cargo();
                    structural = true;
                }
            }
            Action::SetMode(mode) => fit.mode = Some(mode),
            Action::SetSecStatus(sec) => fit.sec_status = sec,
            Action::SetSystemSecurity(security) => fit.system_security = security,
            Action::SetDamageProfile(profile) => {
                self.last_damage_profile.clone_from(&profile);
                fit.damage_profile = profile;
            }
            Action::SetTargetProfile(profile) => fit.target_profile = profile,
            Action::ToggleBoosters(main, rows) => {
                let Some(on) = fit.boosters.get(main).map(|b| !b.active) else {
                    return;
                };
                for i in rows.into_iter().chain([main]) {
                    if let Some(b) = fit.boosters.get_mut(i) {
                        b.active = on;
                    }
                }
            }
            Action::ToggleBoosterSideEffect(i, effect) => {
                let Some(b) = fit.boosters.get_mut(i) else {
                    return;
                };
                match b.side_effects.iter().position(|&e| e == effect) {
                    Some(at) => {
                        b.side_effects.remove(at);
                    }
                    None => b.side_effects.push(effect),
                }
            }
            Action::ToggleImplants(clicked, rows) => fit.toggle_implants(clicked, &rows),
            Action::SetImplantSource(source) => {
                fit.implant_source = source;
                structural = true;
            }
            Action::AddImplants(implants) => {
                for type_id in implants {
                    fit.add_implant(&loaded.catalog, type_id);
                }
                structural = true;
            }
            Action::ModuleToCargo(from, onto, transfer) => {
                structural = fit.module_to_cargo(&loaded.catalog, from, onto, transfer);
            }
            Action::CargoToModule(type_id, onto, transfer) => {
                structural = fit.cargo_to_module(&loaded.catalog, type_id, onto, transfer);
            }
            Action::AddAdditions(kind, items) => {
                for (type_id, count) in items {
                    match kind {
                        AddKind::Drones => fit.add_drones(type_id, count),
                        AddKind::Implants => {
                            fit.add_implant(&loaded.catalog, type_id);
                        }
                        AddKind::Boosters => {
                            fit.add_booster(&loaded.catalog, crate::fit::Booster::new(type_id));
                        }
                        AddKind::Cargo => fit.add_cargo(type_id, count),
                    }
                }
            }
        }
        if checked {
            let after = self.saved.fits[current].clone();
            refused = self.refusal(&loaded, &before, &after);
            if refused.is_some() {
                self.saved.fits[current] = before.clone();
            }
        }
        if let Some(reason) = refused.or(report) {
            self.status = Some(reason.into());
        }
        if structural {
            self.clear_selection();
        }
        self.record_edit(current, before);
        self.changed(cx);
    }
}

/// The furthest a module's state goes: passive modules only come online, and only some active
/// ones overheat.
fn highest_state(catalog: &crate::data::Catalog, type_id: TypeId) -> ModuleState {
    match catalog.get(type_id) {
        Some(t) if t.overloadable => ModuleState::Overload,
        Some(t) if t.activatable => ModuleState::Active,
        _ => ModuleState::Online,
    }
}

/// `action` with each module row it names widened to the module's group.
fn with_groups(fit: &crate::fit::Fit, action: Action) -> Action {
    let widen = |rows: Vec<RowRef>| -> Vec<RowRef> {
        let mut out: Vec<RowRef> = Vec::with_capacity(rows.len());
        for row in rows {
            let members = match row {
                RowRef::Module(slot, i) => fit
                    .group(slot, i)
                    .into_iter()
                    .map(|j| RowRef::Module(slot, j))
                    .collect(),
                RowRef::Rig(_) | RowRef::Subsystem(_) => vec![row],
            };
            for member in members {
                if !out.contains(&member) {
                    out.push(member);
                }
            }
        }
        out
    };
    match action {
        Action::CycleState {
            main,
            others,
            offline,
        } => Action::CycleState {
            main,
            others: widen(others),
            offline,
        },
        Action::SetRah(rows, pattern) => Action::SetRah(widen(rows), pattern),
        Action::SetCharge(rows, charge) => Action::SetCharge(widen(rows), charge),
        Action::SetSpool(rows, cycles) => Action::SetSpool(widen(rows), cycles),
        Action::Remove(rows) => Action::Remove(widen(rows)),
        Action::SwapType(rows, type_id) => Action::SwapType(widen(rows), type_id),
        other => other,
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
    let crate::fit::Fit {
        high,
        mid,
        low,
        services,
        ..
    } = fit;
    [
        (Slot::High, high),
        (Slot::Mid, mid),
        (Slot::Low, low),
        (Slot::Service, services),
    ]
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
