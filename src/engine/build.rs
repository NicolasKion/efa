//! Building a fit into a solar system: the pilot, the ship and everything fitted, the way every
//! calculation does it.

use rc::{AddMode, AttrId, EveAttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack};

use super::{ItemRef, Pilot, pane, surroundings, validate};
use crate::{
    data::{Slot, TypeId},
    fit::{DroneStack, Fit, Module, ModuleState, Mutation, RahPattern, SystemSecurity},
};

/// The solar system's security, which some structure bonuses depend on.
pub(super) fn sec_zone(security: SystemSecurity) -> rc::SecZone {
    match security {
        SystemSecurity::HighSec => rc::SecZone::HiSec(rc::SecZoneCorruption::None),
        SystemSecurity::LowSec => rc::SecZone::LowSec(rc::SecZoneCorruption::None),
        SystemSecurity::NullSec => rc::SecZone::NullSec,
        SystemSecurity::WSpace => rc::SecZone::WSpace,
    }
}

/// Put a structure's services into the fit, with each one's position.
pub(super) fn add_services(f: &mut rc::Fit<'_>, fit: &Fit) -> Vec<(usize, ItemId)> {
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

/// The fit's own items as built into a solar system, for reading back.
pub(super) struct Local {
    pub(super) ship_id: ItemId,
    pub(super) module_ids: Vec<(Slot, usize, ItemId)>,
    pub(super) subsystem_ids: Vec<ItemId>,
    /// The first drone of each stack.
    pub(super) drone_ids: Vec<Option<ItemId>>,
    pub(super) booster_ids: Vec<ItemId>,
    pub(super) fighter_ids: Vec<ItemId>,
    /// Which efa item each refine item stands for, for validation.
    pub(super) refs: validate::Refs,
}

/// Put the pilot, ship and everything fitted onto `f`. Items refine refuses go to `problems`.
/// None without a ship.
pub(super) fn build_local(
    f: &mut rc::Fit<'_>,
    fit: &Fit,
    pilot: &Pilot,
    problems: &mut Vec<String>,
) -> Option<Local> {
    pilot.board(f);
    let ship = fit.ship?;
    let ship_id = f
        .set_ship(ItemTypeId::from_i32(ship), None, None)
        .get_item_id();
    let mut refs = validate::Refs::new();
    refs.insert(ship_id, ItemRef::Ship);

    let mut module_ids: Vec<(Slot, usize, ItemId)> = Vec::new();
    for (slot, rack, modules) in [
        (Slot::High, ModRack::High, &fit.high),
        (Slot::Mid, ModRack::Mid, &fit.mid),
        (Slot::Low, ModRack::Low, &fit.low),
    ] {
        for (i, m) in modules.iter().enumerate() {
            let Some(m) = m else { continue };
            // Each module goes to its own position; heat spreads by position.
            match add_fit_module(
                f,
                rack,
                AddMode::Replace(rc::RackPos::from_usize_clamped(i)),
                m,
            ) {
                Ok(mut module) => {
                    let id = module.get_item_id();
                    module_ids.push((slot, i, id));
                    refs.insert(id, ItemRef::Module(slot, i));
                    if let Some(charge) = module.get_charge() {
                        refs.insert(charge.get_item_id(), ItemRef::Module(slot, i));
                    }
                }
                Err(e) => problems.push(format!("module {}: {e}", m.type_id)),
            }
        }
    }
    if let Some(mode) = fit.mode {
        let id = f.set_stance(ItemTypeId::from_i32(mode)).get_item_id();
        refs.insert(id, ItemRef::Mode);
    }
    f.set_sec_status(rc::FitSecStatus::from_f64_clamped(
        fit.sec_status.unwrap_or(pilot.sec_status),
    ));
    for (i, id) in add_services(f, fit) {
        module_ids.push((Slot::Service, i, id));
        refs.insert(id, ItemRef::Module(Slot::Service, i));
    }
    for (i, rig) in fit.rigs.iter().enumerate() {
        if let Some(rig) = rig {
            let id = f.add_rig(ItemTypeId::from_i32(*rig)).get_item_id();
            refs.insert(id, ItemRef::Rig(i));
        }
    }
    let mut subsystem_ids = Vec::with_capacity(fit.subsystems.len());
    for (i, &subsystem) in fit.subsystems.iter().enumerate() {
        let id = f
            .add_subsystem(ItemTypeId::from_i32(subsystem))
            .get_item_id();
        refs.insert(id, ItemRef::Subsystem(i));
        subsystem_ids.push(id);
    }
    let mut drone_ids: Vec<Option<ItemId>> = Vec::with_capacity(fit.drones.len());
    for (stack_index, stack) in fit.drones.iter().enumerate() {
        let mut first = None;
        for i in 0..stack.count {
            let minion_state = if i < stack.active {
                MinionState::Engaging
            } else {
                MinionState::InBay
            };
            let id = add_fit_drone(f, stack, minion_state).get_item_id();
            refs.insert(id, ItemRef::Drone(stack_index));
            first.get_or_insert(id);
        }
        drone_ids.push(first);
    }
    for (implant, active) in fit.applied_implants(&pilot.implants) {
        let mut item = f.add_implant(ItemTypeId::from_i32(implant));
        if !active {
            item.set_state(false);
        }
        refs.insert(item.get_item_id(), ItemRef::Implant(implant));
    }
    let mut booster_ids = Vec::with_capacity(fit.boosters.len());
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
        refs.insert(id, ItemRef::Booster(i));
        booster_ids.push(id);
    }
    let fighter_ids = surroundings::add_fighters(f, &fit.fighters);
    for (i, &id) in fighter_ids.iter().enumerate() {
        refs.insert(id, ItemRef::Fighter(i));
    }
    Some(Local {
        ship_id,
        module_ids,
        subsystem_ids,
        drone_ids,
        booster_ids,
        fighter_ids,
        refs,
    })
}

/// The type refine is given for a fit's module or drone: the base item, which the mutation then
/// turns into the abyssal one.
pub(super) fn added_type(type_id: TypeId, mutation: Option<&Mutation>) -> ItemTypeId {
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
pub(super) fn roll(applied: rc::Mutation<'_>, mutation: &Mutation) {
    let rc::Mutation::Effective(mut effective) = applied else {
        return;
    };
    for (&attr, &value) in &mutation.attrs {
        if let Ok(mut full) = effective.get_full_mattr(AttrId::Eve(EveAttrId::from_i32(attr))) {
            let _ = full.set_value(Some(rc::Value::from_f64_clamped(value)));
        }
    }
}

pub(super) fn module_state(state: ModuleState) -> rc::ModuleState {
    match state {
        ModuleState::Offline => rc::ModuleState::Offline,
        ModuleState::Online => rc::ModuleState::Online,
        ModuleState::Active => rc::ModuleState::Active,
        ModuleState::Overload => rc::ModuleState::Overload,
    }
}
