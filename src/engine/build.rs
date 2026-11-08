//! Building a fit into a solar system: the pilot, the ship and everything fitted, the way every
//! calculation does it.

use std::collections::HashMap;

use rc::{
    AddMode, AttrId, EveAttrId, FitId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, Sol,
    SolOwned,
};

use super::{Engine, FitItem, Pilot, pane, surroundings, surroundings::Projections};
use crate::{
    data::{Slot, TypeId},
    fit::{DroneStack, Fit, Module, ModuleState, Mutation, RahPattern, SystemSecurity},
};

/// Settings for [`Engine::build`].
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct BuildOptions<'a> {
    /// What surrounds the fit: the saved fits its command and projected fits are looked up in.
    /// Projected items and the environment come in even when that list is empty. None builds
    /// the fit alone in space.
    pub surroundings: Option<&'a [Fit]>,
    /// Spool-up for modules without their own setting, as a fraction of full spool.
    pub default_spool: Option<f64>,
}

impl<'a> BuildOptions<'a> {
    /// The fit with what surrounds it, `fits` being the saved fits.
    pub fn among(fits: &'a [Fit]) -> Self {
        Self {
            surroundings: Some(fits),
            default_spool: None,
        }
    }
}

/// A fit's own items as added to a solar system, for reading back.
#[derive(Debug, Clone)]
pub(super) struct Placed {
    pub fit_id: FitId,
    pub ship: ItemId,
    /// Which item of the fit each refine item stands for, the pilot's skills left out.
    pub items: HashMap<ItemId, FitItem>,
    /// Modules by rack and position, services after them.
    pub modules: Vec<(Slot, usize, ItemId)>,
    pub subsystems: Vec<ItemId>,
    /// Each stack's drones, the engaged ones first; all of a stack are the same.
    pub drones: Vec<Vec<ItemId>>,
    pub boosters: Vec<ItemId>,
    /// Every squadron in the fit's order, launched or not.
    pub fighters: Vec<ItemId>,
    /// Items refine refused, e.g. a module the ship has no rack position for.
    pub problems: Vec<String>,
}

impl Placed {
    /// The first drone of each stack, which stands for the stack.
    pub fn first_drones(&self) -> impl Iterator<Item = Option<ItemId>> + '_ {
        self.drones.iter().map(|stack| stack.first().copied())
    }

    /// The squadrons that are launched.
    pub fn launched_fighters<'a>(&'a self, fit: &'a Fit) -> impl Iterator<Item = ItemId> + 'a {
        self.fighters
            .iter()
            .zip(&fit.fighters)
            .filter(|(_, fighter)| fighter.active)
            .map(|(&id, _)| id)
    }
}

/// A solar system with a fit built into it, ready to read numbers from. Changes made to look at
/// the fit another way (a module switched off, a charge taken out) belong in a transaction, so
/// the build reads as it was afterwards.
#[derive(Clone)]
pub(super) struct Built {
    pub sol: SolOwned,
    pub fit: Placed,
    /// Projected items the stats read back, in the fit's order.
    pub projections: Projections,
    /// The EM, thermal, kinetic, explosive mix the fit's damage profile stands for.
    pub incoming: [f64; 4],
}

impl Engine {
    /// Build `fit` flown by `pilot` into a fresh solar system: the security zone, the fit, what
    /// surrounds it, and the damage its reactive armor hardener adapts to. None without a ship.
    pub(super) fn build(
        &self,
        fit: &Fit,
        pilot: &Pilot,
        options: BuildOptions<'_>,
    ) -> Option<Built> {
        let mut sol = self.sol();
        sol.set_sec_zone(sec_zone(fit.system_security));
        if let Some(spool) = options.default_spool {
            sol.set_default_spool(rc::Spool::SpoolScale(rc::UnitInterval::from_f64_clamped(
                spool,
            )));
        }
        let placed = add_fit(&mut sol, fit, pilot)?;
        let projections = options.surroundings.map_or_default(|fits| {
            self.add_surroundings(&mut sol, placed.fit_id, placed.ship, fit, fits)
        });
        let mut f = sol.get_fit(&placed.fit_id).ok()?;
        let incoming = pane::incoming_mix(&mut f, &fit.damage_profile);
        let rah = rah_incoming(&mut f, fit, pane::dps_profile(incoming));
        f.set_rah_incoming_dps(Some(rah));
        Some(Built {
            sol,
            fit: placed,
            projections,
            incoming,
        })
    }
}

/// The solar system's security, which some structure bonuses depend on.
fn sec_zone(security: SystemSecurity) -> rc::SecZone {
    match security {
        SystemSecurity::HighSec => rc::SecZone::HiSec(rc::SecZoneCorruption::None),
        SystemSecurity::LowSec => rc::SecZone::LowSec(rc::SecZoneCorruption::None),
        SystemSecurity::NullSec => rc::SecZone::NullSec,
        SystemSecurity::WSpace => rc::SecZone::WSpace,
    }
}

/// What the reactive armor hardener adapts to: the fit's incoming damage, unless the hardener has
/// a pattern of its own. refine takes one pattern per fit, and a fit holds one hardener at most,
/// so the first override found decides. Not adapting is no damage at all: with nothing taken, the
/// hardener has no reason to shift.
pub(super) fn rah_incoming(
    f: &mut rc::Fit<'_>,
    fit: &Fit,
    incoming: rc::DpsProfile,
) -> rc::DpsProfile {
    match fit.modules().find_map(|m| m.rah.as_ref()) {
        None => incoming,
        Some(RahPattern::DoNotAdapt) => pane::dps_profile([0.0; 4]),
        Some(RahPattern::Pattern(profile)) => pane::dps_profile(pane::incoming_mix(f, profile)),
    }
}

/// Add `fit` to the solar system as a fit of its own: the pilot and its skills, the ship and
/// everything on it. None without a ship.
pub(super) fn add_fit(sol: &mut Sol, fit: &Fit, pilot: &Pilot) -> Option<Placed> {
    let mut f = sol.add_fit();
    pilot.board(&mut f);
    let ship = fit.ship?;
    let ship_id = f
        .set_ship(ItemTypeId::from_i32(ship), None, None)
        .get_item_id();
    let mut placed = Placed {
        fit_id: f.get_fit_id(),
        ship: ship_id,
        items: HashMap::from([(ship_id, FitItem::Ship)]),
        modules: Vec::new(),
        subsystems: Vec::with_capacity(fit.subsystems.len()),
        drones: Vec::with_capacity(fit.drones.len()),
        boosters: Vec::with_capacity(fit.boosters.len()),
        fighters: Vec::with_capacity(fit.fighters.len()),
        problems: Vec::new(),
    };
    for (slot, rack, modules) in [
        (Slot::High, ModRack::High, &fit.high),
        (Slot::Mid, ModRack::Mid, &fit.mid),
        (Slot::Low, ModRack::Low, &fit.low),
    ] {
        for (i, m) in modules.iter().enumerate() {
            let Some(m) = m else { continue };
            // Each module goes to its own position; heat spreads by position.
            match add_fit_module(&mut f, rack, at(i), m) {
                Ok(mut module) => {
                    let id = module.get_item_id();
                    placed.modules.push((slot, i, id));
                    placed.items.insert(id, FitItem::Module(slot, i));
                    if let Some(charge) = module.get_charge() {
                        placed
                            .items
                            .insert(charge.get_item_id(), FitItem::Charge(slot, i));
                    }
                }
                Err(e) => placed.problems.push(format!("module {}: {e}", m.type_id)),
            }
        }
    }
    if let Some(mode) = fit.mode {
        let id = f.set_stance(ItemTypeId::from_i32(mode)).get_item_id();
        placed.items.insert(id, FitItem::Mode);
    }
    f.set_sec_status(rc::FitSecStatus::from_f64_clamped(
        fit.sec_status.unwrap_or(pilot.sec_status),
    ));
    for (i, id) in add_services(&mut f, fit) {
        placed.modules.push((Slot::Service, i, id));
        placed.items.insert(id, FitItem::Module(Slot::Service, i));
    }
    for (i, rig) in fit.rigs.iter().enumerate() {
        if let Some(rig) = rig {
            let id = f.add_rig(ItemTypeId::from_i32(*rig)).get_item_id();
            placed.items.insert(id, FitItem::Rig(i));
        }
    }
    for (i, &subsystem) in fit.subsystems.iter().enumerate() {
        let id = f
            .add_subsystem(ItemTypeId::from_i32(subsystem))
            .get_item_id();
        placed.items.insert(id, FitItem::Subsystem(i));
        placed.subsystems.push(id);
    }
    for (n, stack) in fit.drones.iter().enumerate() {
        let mut ids = Vec::with_capacity(stack.count as usize);
        for i in 0..stack.count {
            let state = if i < stack.active {
                MinionState::Engaging
            } else {
                MinionState::InBay
            };
            let id = add_fit_drone(&mut f, stack, state).get_item_id();
            placed.items.insert(id, FitItem::Drone(n));
            ids.push(id);
        }
        placed.drones.push(ids);
    }
    for (implant, active) in fit.applied_implants(&pilot.implants) {
        let mut item = f.add_implant(ItemTypeId::from_i32(implant));
        if !active {
            item.set_state(false);
        }
        placed
            .items
            .insert(item.get_item_id(), FitItem::Implant(implant));
    }
    for (i, booster) in fit.boosters.iter().enumerate() {
        let mut item = f.add_booster(ItemTypeId::from_i32(booster.type_id));
        item.set_state(booster.active);
        for &effect in &booster.side_effects {
            let id = rc::EffectId::Dogma(rc::DogmaEffectId::from_i32(effect));
            if let Ok(mut side_effect) = item.get_side_effect(&id) {
                side_effect.set_state(true);
            }
        }
        let id = item.get_item_id();
        placed.items.insert(id, FitItem::Booster(i));
        placed.boosters.push(id);
    }
    for (i, id) in surroundings::add_fighters(&mut f, &fit.fighters)
        .into_iter()
        .enumerate()
    {
        placed.items.insert(id, FitItem::Fighter(i));
        placed.fighters.push(id);
    }
    Some(placed)
}

/// The rack position a module goes to.
pub(super) fn at(position: usize) -> AddMode {
    AddMode::Replace(rc::RackPos::from_usize_clamped(position))
}

/// Put a structure's services into the fit, with each one's position.
fn add_services(f: &mut rc::Fit<'_>, fit: &Fit) -> Vec<(usize, ItemId)> {
    fit.services
        .iter()
        .enumerate()
        .filter_map(|(i, service)| {
            let service = service.as_ref()?;
            let state = match service.state {
                ModuleState::Offline => rc::ServiceState::Offline,
                ModuleState::Online | ModuleState::Active | ModuleState::Overload => {
                    rc::ServiceState::Online
                }
            };
            let id = f
                .add_service(ItemTypeId::from_i32(service.type_id), state)
                .get_item_id();
            Some((i, id))
        })
        .collect()
}

/// The type refine is given for a fit's module or drone: the base item, which the mutation then
/// turns into the abyssal one.
fn added_type(type_id: TypeId, mutation: Option<&Mutation>) -> ItemTypeId {
    ItemTypeId::from_i32(mutation.map_or(type_id, |m| m.base))
}

/// Add a fit's module at a rack position the way every build does: the base item, mutated, with
/// its spool and charge.
pub(super) fn add_fit_module<'a>(
    f: &'a mut rc::Fit<'_>,
    rack: ModRack,
    mode: AddMode,
    m: &Module,
) -> Result<rc::Module<'a>, rc::err::ModuleAddError> {
    let mut module = f.add_module(
        rack,
        mode,
        added_type(m.type_id, m.mutation.as_ref()),
        module_state(m.state),
    )?;
    if let Some(mutation) = &m.mutation
        && let Ok(applied) = module.mutate(ItemTypeId::from_i32(mutation.mutaplasmid))
    {
        roll(applied, mutation);
    }
    if let Some(cycles) = m.spool {
        module.set_spool_override(Some(rc::Spool::Cycles(rc::Count::from_u32(cycles))));
    }
    if let Some(charge) = m.charge {
        module.set_charge_type_id(ItemTypeId::from_i32(charge));
    }
    Ok(module)
}

/// Add one drone of a stack, mutated like the stack is.
pub(super) fn add_fit_drone<'a>(
    f: &'a mut rc::Fit<'_>,
    stack: &DroneStack,
    state: MinionState,
) -> rc::Drone<'a> {
    let mut drone = f.add_drone(
        added_type(stack.type_id, stack.mutation.as_ref()),
        state,
        None,
        None,
    );
    if let Some(mutation) = &stack.mutation
        && let Ok(applied) = drone.mutate(ItemTypeId::from_i32(mutation.mutaplasmid))
    {
        roll(applied, mutation);
    }
    drone
}

/// Set a fresh mutation's attributes to the rolled values.
fn roll(applied: rc::Mutation<'_>, mutation: &Mutation) {
    let rc::Mutation::Effective(mut effective) = applied else {
        return;
    };
    for (&attr, &value) in &mutation.attrs {
        if let Ok(mut full) = effective.get_full_mattr(AttrId::Eve(EveAttrId::from_i32(attr))) {
            let _ = full.set_value(Some(rc::Value::from_f64_clamped(value)));
        }
    }
}

fn module_state(state: ModuleState) -> rc::ModuleState {
    match state {
        ModuleState::Offline => rc::ModuleState::Offline,
        ModuleState::Online => rc::ModuleState::Online,
        ModuleState::Active => rc::ModuleState::Active,
        ModuleState::Overload => rc::ModuleState::Overload,
    }
}
