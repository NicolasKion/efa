//! Changes to the current fit, and which rows they apply to.
//!
//! Menus follow pyfa: an action applies to the selection when the clicked row is part of it, and
//! to just the clicked row otherwise. Some actions instead default to every similar module (same
//! type), with Ctrl or Alt swapping the two.

mod additions;
mod modules;
mod settings;

use std::rc::Rc;

use gpui_kit::{ClickEvent, Context, Modifiers};

use super::{Efa, Loaded};
use crate::{
    data::{Slot, TypeId},
    engine::{Pilot, Stats},
    fit::{
        DamageProfile, Fit, ImplantSource, Mutation, RahPattern, SystemSecurity, cargo::Transfer,
    },
    prices::Prices,
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
    /// A right click on a module's state: it overheats, or stops overheating when it was, and
    /// `others` that can overheat follow it.
    ToggleOverheat {
        main: (Slot, usize),
        others: Vec<RowRef>,
    },
    /// The state click on a rig: the clicked rig switches on or off and the others follow.
    ToggleRigs(usize, Vec<usize>),
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

impl Efa {
    /// The rows an action on `clicked` should touch.
    pub(super) fn targets(&self, clicked: RowRef, scope: Scope) -> Vec<RowRef> {
        let Some(fit) = self.fit() else {
            return Vec::new();
        };
        match scope {
            Scope::Selection => self.selection.targets(clicked),
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

    pub(super) fn clear_selection(&mut self) {
        self.selection.clear();
        self.additions_selection.clear();
    }

    pub fn apply(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(current) = self.saved.settings.current else {
            return;
        };
        // Actions that put modules on are checked against the fitting rules afterwards; filling
        // a rack checks each copy as it goes.
        let checked = matches!(
            action,
            Action::Place(..)
                | Action::SwapType(..)
                | Action::CargoToModule(..)
                | Action::Clone(..)
        );
        if let Action::SetDamageProfile(profile) = &action {
            self.last_damage_profile.clone_from(profile);
        }
        self.edit_fit(
            current,
            checked,
            |fit, env| apply_action(fit, action, env),
            cx,
        );
    }

    /// Change fit `index` as one undo step. `edit` makes the change and says how it went; with
    /// `checked`, the fitting rules then look at the result, and a refused change is put back.
    /// Problems show in the status line in the error color, reports as plain messages. Returns
    /// whether the change was kept.
    pub(super) fn edit_fit(
        &mut self,
        index: usize,
        checked: bool,
        edit: impl FnOnce(&mut Fit, &EditEnv) -> Outcome,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(loaded) = self.loaded() else {
            return false;
        };
        if index >= self.saved.fits.len() {
            return false;
        }
        let pilot = self.pilot(&loaded);
        let prices = Rc::clone(&self.prices);
        let before = Rc::clone(&self.saved.fits[index]);
        let env = EditEnv {
            loaded: &loaded,
            pilot: &pilot,
            stats: self.stats.as_ref(),
            prices: &prices,
            implants_in_cargo: self.saved.settings.preferences.general.implants_in_cargo,
        };
        let mut outcome = edit(self.saved.fit_mut(index), &env);
        if outcome.skipped || *self.saved.fits[index] == *before {
            // Unchanged, it stays the very fit the other windows share.
            self.saved.fits[index] = Rc::clone(&before);
        }
        if outcome.skipped {
            return false;
        }
        if checked {
            let engine = &loaded.engine;
            let after = &self.saved.fits[index];
            let (old, new) = (
                engine.validate(&before, &pilot),
                engine.validate(after, &pilot),
            );
            let refusal = old
                .refusal(&new, after.ignore_restrictions)
                .map(|problem| problem.describe(|t| loaded.catalog.name(t)));
            if let Some(reason) = refusal {
                self.saved.fits[index] = before;
                self.set_error(match outcome.about {
                    Some(about) => format!("{about}: {reason}"),
                    None => reason,
                });
                cx.notify();
                return false;
            }
        }
        if let Some(error) = outcome.error.take() {
            self.set_error(error);
        } else if let Some(report) = outcome.report.take() {
            self.set_status(report);
        }
        if outcome.structural {
            self.clear_selection();
        }
        self.record_edit(index, before);
        self.changed(cx);
        true
    }
}

/// What a fit edit gets to work with besides the fit.
pub(super) struct EditEnv<'a> {
    pub loaded: &'a Loaded,
    /// The current fit's pilot, which the fitting rules check against.
    pub pilot: &'a Pilot,
    pub stats: Option<&'a Stats>,
    pub prices: &'a Prices,
    /// pyfa's "implants and boosters count against the cargo hold".
    pub implants_in_cargo: bool,
}

/// How a fit edit went, for [`Efa::edit_fit`].
#[derive(Debug, Default)]
pub(super) struct Outcome {
    /// Nothing was done: no undo step, no recalculation.
    pub skipped: bool,
    /// What went wrong or was left out, for the status line.
    pub error: Option<String>,
    /// What was done, for the status line.
    pub report: Option<String>,
    /// Rows moved or went, so the selection is cleared.
    pub structural: bool,
    /// What the fitting rules' refusal is about, put before the reason.
    pub about: Option<String>,
}

impl Outcome {
    pub fn skipped() -> Self {
        Self {
            skipped: true,
            ..Self::default()
        }
    }
}

/// Make `action` on `fit`, pure of any window state.
fn apply_action(fit: &mut Fit, action: Action, env: &EditEnv) -> Outcome {
    // In a grouped rack, whatever happens to a module happens to its whole group.
    let action = modules::with_groups(fit, action);
    match action {
        Action::CycleState { .. }
        | Action::ToggleRigs(..)
        | Action::ToggleOverheat { .. }
        | Action::ToggleGrouped(..)
        | Action::SetCharge(..)
        | Action::SetSpool(..)
        | Action::SetRah(..)
        | Action::Remove(..)
        | Action::Place(..)
        | Action::Swap(..)
        | Action::Clone(..)
        | Action::SwapType(..)
        | Action::AddMutated(..)
        | Action::Mutate(..)
        | Action::Revert(..)
        | Action::FillRack(..)
        | Action::OptimizePrice
        | Action::FillClone(..)
        | Action::ModuleToCargo(..)
        | Action::CargoToModule(..) => modules::apply(fit, action, env),
        Action::AddDroneStacks(..)
        | Action::SetDronesActive(..)
        | Action::AddCargo(..)
        | Action::ApplyCargoPreset(..)
        | Action::AddDrones(..)
        | Action::RemoveAdditions(..)
        | Action::SetAmount(..)
        | Action::SplitDrones(..)
        | Action::ToggleDrones(..)
        | Action::RemoveDrone(..)
        | Action::MergeDrones { .. }
        | Action::CloneDrones(..)
        | Action::SwapAdditions(..)
        | Action::ToggleBoosters(..)
        | Action::ToggleBoosterSideEffect(..)
        | Action::ToggleImplants(..)
        | Action::SetImplantSource(..)
        | Action::AddImplants(..)
        | Action::AddAdditions(..) => additions::apply(fit, action, env),
        Action::SetMode(..)
        | Action::SetSecStatus(..)
        | Action::SetSystemSecurity(..)
        | Action::SetDamageProfile(..)
        | Action::SetTargetProfile(..) => settings::apply(fit, action),
    }
}
