//! Everything around the fit that refine has to see in the same solar system: its fighters, the
//! fits boosting it from its fleet, fits and loose items projecting onto it, and environmental
//! effects.
//!
//! Loose projected modules, drones and fighters have no fit of their own in pyfa. refine needs one
//! to hold them, so each gets a hidden fit flying a capsule, which carries no bonuses that could
//! change them. A fit projected more than once is added once per copy, since refine has no
//! multiplier for projections.

use rc::{
    AbilityId, AttrId, CountNz, EveAttrId, ItemCommon, ItemId, ItemTypeId, Lender, MinionState,
    ModRack, SolOwned, stats::StatSlot,
};

use super::{DroneInfo, Engine, Slots, drone_info, module_state};
use crate::fit::{
    Fit, ModuleState,
    fighters::Fighter,
    projected::{EnvEffect, ProjectedModule},
};

/// The capsule: a ship item with nothing on it that bonuses modules or drones.
const CAPSULE: i32 = 670;
const RADIUS: i32 = 162;

/// Fighter abilities pyfa leaves off when a squadron has no attack to start with.
const PROPULSION_ABILITIES: [i32; 2] = [4, 13];

/// What the Fighters tab and the projected fighter rows show for a squadron.
#[derive(Debug, Clone, Default)]
pub struct FighterInfo {
    pub abilities: Vec<AbilityInfo>,
    /// Fighters in the squadron now, and at most.
    pub count: Option<(u32, u32)>,
    /// Speed, EHP and regeneration, read the same way as a drone's.
    pub stats: DroneInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbilityInfo {
    pub id: i32,
    pub active: bool,
}

impl FighterInfo {
    pub fn max_size(&self) -> Option<u32> {
        self.count.map(|(_, max)| max)
    }
}

/// Squadrons launched and tubes, per class: light, heavy, support.
#[derive(Debug, Clone, Copy, Default)]
pub struct FighterClasses {
    pub light: Slots,
    pub heavy: Slots,
    pub support: Slots,
}

/// Items of a fit added to the solar system that the stats read back.
pub(super) struct Built {
    pub ship: Option<ItemId>,
    pub modules: Vec<ItemId>,
    pub drones: Vec<ItemId>,
    pub fighters: Vec<ItemId>,
}

/// Projected items the stats read back, in the fit's order.
pub(super) struct Projections {
    pub fighters: Vec<ItemId>,
    pub modules: Vec<Option<ItemId>>,
}

/// Launch the fit's own fighters, in its order.
pub(super) fn add_fighters(f: &mut rc::Fit<'_>, fighters: &[Fighter]) -> Vec<ItemId> {
    fighters
        .iter()
        .map(|fighter| add_fighter(f, fighter))
        .collect()
}

fn add_fighter(f: &mut rc::Fit<'_>, fighter: &Fighter) -> ItemId {
    let state = if fighter.active {
        MinionState::Engaging
    } else {
        MinionState::InBay
    };
    let mut item = f.add_fighter(ItemTypeId::from_i32(fighter.type_id), state, None, None);
    if let Some(amount) = fighter.amount {
        item.set_count_override(Some(CountNz::from_u32_clamped(amount)));
    }
    set_abilities(&mut item, fighter);
    item.get_item_id()
}

/// pyfa's ability defaults: the standard attack when the squadron has one, otherwise everything
/// but its propulsion; then whatever the user switched.
fn set_abilities(item: &mut rc::Fighter<'_>, fighter: &Fighter) {
    let mut found: Vec<(i32, bool)> = Vec::new();
    {
        let mut abilities = item.iter_abilities();
        while let Some(ability) = abilities.next() {
            found.push((ability.get_id().into_i32(), ability.get_state()));
        }
    }
    // refine starts with only the default effect running, which is the attack.
    let has_attack = found.iter().any(|&(_, on)| on);
    for (id, refine_default) in found {
        let default =
            has_attack && refine_default || !has_attack && !PROPULSION_ABILITIES.contains(&id);
        let wanted = fighter.ability_override(id).unwrap_or(default);
        if wanted != refine_default
            && let Ok(mut ability) = item.get_ability(&AbilityId::from_i32(id))
        {
            ability.set_state(wanted);
        }
    }
}

pub(super) fn fighter_info(sol: &mut SolOwned, id: ItemId) -> FighterInfo {
    let mut info = FighterInfo {
        stats: drone_info(sol, id),
        ..FighterInfo::default()
    };
    if let Ok(mut fighter) = sol.get_fighter(&id) {
        info.count = fighter
            .get_count()
            .map(|c| (c.current.into_u32(), c.max.into_u32()));
        let mut abilities = fighter.iter_abilities();
        while let Some(ability) = abilities.next() {
            info.abilities.push(AbilityInfo {
                id: ability.get_id().into_i32(),
                active: ability.get_state(),
            });
        }
    }
    info
}

pub(super) fn fighter_classes(sol: &mut SolOwned, fit_id: rc::FitId) -> FighterClasses {
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return FighterClasses::default();
    };
    let mut s = f.stats();
    // Structures have their own standup tubes; a ship has none of those.
    let pick = |ship: StatSlot, standup: StatSlot| {
        let standup = super::slots(standup);
        if standup.total.unwrap_or(0) > 0 {
            standup
        } else {
            super::slots(ship)
        }
    };
    FighterClasses {
        light: pick(
            s.get_stat_launched_light_fighters(),
            s.get_stat_launched_st_light_fighters(),
        ),
        heavy: pick(
            s.get_stat_launched_heavy_fighters(),
            s.get_stat_launched_st_heavy_fighters(),
        ),
        support: pick(
            s.get_stat_launched_support_fighters(),
            s.get_stat_launched_st_support_fighters(),
        ),
    }
}

impl Engine {
    /// Put a whole fit into the solar system as a fit of its own, the way the main fit is built.
    pub(super) fn add_other_fit(&self, sol: &mut SolOwned, fit: &Fit) -> (rc::FitId, Built) {
        let mut f = sol.add_fit();
        self.add_skills(&mut f);
        let ship = fit.ship.map(|ship| {
            f.set_ship(ItemTypeId::from_i32(ship), None, None)
                .get_item_id()
        });
        let mut modules = Vec::new();
        for (rack, list) in [
            (ModRack::High, &fit.high),
            (ModRack::Mid, &fit.mid),
            (ModRack::Low, &fit.low),
        ] {
            for (i, m) in list.iter().enumerate() {
                let Some(m) = m else { continue };
                if let Ok(mut module) = f.add_module(
                    rack,
                    rc::AddMode::Replace(rc::RackPos::from_usize_clamped(i)),
                    ItemTypeId::from_i32(m.type_id),
                    module_state(m.state),
                ) {
                    if let Some(cycles) = m.spool {
                        module.set_spool_override(Some(rc::Spool::Cycles(rc::Count::from_u32(
                            cycles,
                        ))));
                    }
                    if let Some(charge) = m.charge {
                        module.set_charge_type_id(ItemTypeId::from_i32(charge));
                    }
                    modules.push(module.get_item_id());
                }
            }
        }
        if let Some(mode) = fit.mode {
            f.set_stance(ItemTypeId::from_i32(mode));
        }
        if let Some(sec) = fit.sec_status {
            f.set_sec_status(rc::FitSecStatus::from_f64_clamped(sec));
        }
        for &rig in fit.rigs.iter().flatten() {
            f.add_rig(ItemTypeId::from_i32(rig));
        }
        for &subsystem in &fit.subsystems {
            f.add_subsystem(ItemTypeId::from_i32(subsystem));
        }
        let mut drones = Vec::new();
        for stack in &fit.drones {
            for i in 0..stack.count {
                let state = if i < stack.active {
                    MinionState::Engaging
                } else {
                    MinionState::InBay
                };
                let drone = f.add_drone(ItemTypeId::from_i32(stack.type_id), state, None, None);
                if i < stack.active {
                    drones.push(drone.get_item_id());
                }
            }
        }
        let fighters = fit
            .fighters
            .iter()
            .map(|fighter| (fighter.active, add_fighter(&mut f, fighter)))
            .filter_map(|(active, id)| active.then_some(id))
            .collect();
        for (implant, active) in fit.applied_implants(&self.all5.implants) {
            f.add_implant(ItemTypeId::from_i32(implant))
                .set_state(active);
        }
        for booster in &fit.boosters {
            let mut item = f.add_booster(ItemTypeId::from_i32(booster.type_id));
            item.set_state(booster.active);
            for &effect in &booster.side_effects {
                let id = rc::EffectId::Dogma(rc::DogmaEffectId::from_i32(effect));
                if let Ok(mut side_effect) = item.get_side_effect(&id) {
                    side_effect.set_state(true);
                }
            }
        }
        let fit_id = f.get_fit_id();
        (
            fit_id,
            Built {
                ship,
                modules,
                drones,
                fighters,
            },
        )
    }

    /// Other fits fly All 5 for now; pyfa gives each its own character.
    fn add_skills(&self, f: &mut rc::Fit<'_>) {
        self.all5.board(f);
    }

    /// A hidden fit to hold loose projected items: a capsule with the character's skills.
    fn add_holder(&self, sol: &mut SolOwned) -> (rc::FitId, ItemId) {
        let mut f = sol.add_fit();
        self.add_skills(&mut f);
        let ship = f
            .set_ship(ItemTypeId::from_i32(CAPSULE), None, None)
            .get_item_id();
        (f.get_fit_id(), ship)
    }

    /// Add what surrounds `fit` to the solar system its own items are in. `others` are the saved
    /// fits that projected and command fits refer to.
    pub(super) fn add_surroundings(
        &self,
        sol: &mut SolOwned,
        fit_id: rc::FitId,
        ship_id: ItemId,
        fit: &Fit,
        others: &[Fit],
    ) -> Projections {
        let find = |uid: u64| others.iter().find(|f| uid != 0 && f.uid == uid);
        let target_radius = radius(sol, ship_id);

        let boosters: Vec<&Fit> = fit
            .command
            .iter()
            .filter(|c| c.active)
            .filter_map(|c| find(c.fit))
            .collect();
        if !boosters.is_empty() {
            let fleet = sol.add_fleet().get_fleet_id();
            if let Ok(mut f) = sol.get_fit(&fit_id) {
                let _ = f.set_fleet(Some(&fleet));
            }
            for booster in boosters {
                let (id, _) = self.add_other_fit(sol, booster);
                if let Ok(mut f) = sol.get_fit(&id) {
                    let _ = f.set_fleet(Some(&fleet));
                }
            }
        }

        for projected in fit.projected.fits.iter().filter(|p| p.active) {
            let Some(source) = find(projected.fit) else {
                continue;
            };
            for _ in 0..projected.amount {
                let (_, built) = self.add_other_fit(sol, source);
                if let Some(ship) = built.ship {
                    place_ship(sol, ship, projected.range, target_radius);
                }
                for id in &built.modules {
                    if let Ok(mut module) = sol.get_module(id) {
                        let _ = module.add_proj(&ship_id);
                    }
                }
                for id in &built.drones {
                    place_minion(sol, *id, projected.range, target_radius);
                    if let Ok(mut drone) = sol.get_drone(id) {
                        let _ = drone.add_proj(&ship_id);
                    }
                }
                for id in &built.fighters {
                    place_minion(sol, *id, projected.range, target_radius);
                    if let Ok(mut fighter) = sol.get_fighter(id) {
                        let _ = fighter.add_proj(&ship_id);
                    }
                }
            }
        }

        let modules = fit
            .projected
            .modules
            .iter()
            .map(|module| self.project_module(sol, module, ship_id, target_radius))
            .collect();

        for stack in &fit.projected.drones {
            let (holder, holder_ship) = self.add_holder(sol);
            place_ship(sol, holder_ship, stack.range, target_radius);
            let state = if stack.active {
                MinionState::Engaging
            } else {
                MinionState::InBay
            };
            for _ in 0..stack.count {
                let Ok(mut f) = sol.get_fit(&holder) else {
                    break;
                };
                let id = f
                    .add_drone(ItemTypeId::from_i32(stack.type_id), state, None, None)
                    .get_item_id();
                if stack.active {
                    place_minion(sol, id, stack.range, target_radius);
                    if let Ok(mut drone) = sol.get_drone(&id) {
                        let _ = drone.add_proj(&ship_id);
                    }
                }
            }
        }

        let mut fighter_ids = Vec::with_capacity(fit.projected.fighters.len());
        for projected in &fit.projected.fighters {
            let (holder, holder_ship) = self.add_holder(sol);
            place_ship(sol, holder_ship, projected.range, target_radius);
            let Ok(mut f) = sol.get_fit(&holder) else {
                continue;
            };
            let id = add_fighter(&mut f, &projected.fighter);
            if projected.fighter.active {
                place_minion(sol, id, projected.range, target_radius);
                if let Ok(mut fighter) = sol.get_fighter(&id) {
                    let _ = fighter.add_proj(&ship_id);
                }
            }
            fighter_ids.push(id);
        }

        for env in &fit.projected.env {
            add_env(sol, fit_id, ship_id, env);
        }
        Projections {
            fighters: fighter_ids,
            modules,
        }
    }

    fn project_module(
        &self,
        sol: &mut SolOwned,
        module: &ProjectedModule,
        target: ItemId,
        target_radius: f64,
    ) -> Option<ItemId> {
        let (holder, holder_ship) = self.add_holder(sol);
        place_ship(sol, holder_ship, module.range, target_radius);
        let mut f = sol.get_fit(&holder).ok()?;
        // A capsule has no racks to speak of; the module just needs somewhere to sit.
        let Ok(mut item) = f.add_module(
            ModRack::High,
            rc::AddMode::Append,
            ItemTypeId::from_i32(module.type_id),
            module_state(module.state),
        ) else {
            return None;
        };
        if let Some(cycles) = module.spool {
            item.set_spool_override(Some(rc::Spool::Cycles(rc::Count::from_u32(cycles))));
        }
        if let Some(charge) = module.charge {
            item.set_charge_type_id(ItemTypeId::from_i32(charge));
        }
        let id = item.get_item_id();
        if module.state != ModuleState::Offline {
            let _ = item.add_proj(&target);
        }
        Some(id)
    }
}

/// Environmental effects act on the whole fit, except abyssal weather and clouds, which refine
/// treats as something in space projecting onto ships.
fn add_env(sol: &mut SolOwned, fit_id: rc::FitId, ship_id: ItemId, env: &EnvEffect) {
    let type_id = ItemTypeId::from_i32(env.type_id);
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return;
    };
    let mut effect = f.add_fw_effect(type_id);
    let projects = effect.iter_effects().is_ok_and(|mut effects| {
        effects.any(|(id, _)| match id {
            rc::EffectId::Dogma(id) => {
                crate::data::projection::PROJECTED_ENV_EFFECTS.contains(&id.into_i32())
            }
            _ => false,
        })
    });
    if !projects {
        effect.set_state(env.active);
        return;
    }
    effect.remove();
    let mut effect = sol.add_proj_effect(type_id);
    effect.set_state(env.active);
    let _ = effect.add_proj(&ship_id);
}

fn radius(sol: &mut SolOwned, id: ItemId) -> f64 {
    sol.get_item(&id)
        .ok()
        .and_then(|mut item| {
            item.get_attr(&AttrId::Eve(EveAttrId::from_i32(RADIUS)))
                .ok()
                .map(|v| v.modified.into_f64())
        })
        .unwrap_or(0.0)
}

/// Where a projector `range` metres from the target's surface has its centre: refine measures
/// range surface to surface, and the target sits at the origin.
fn centre_distance(range: f64, own_radius: f64, target_radius: f64) -> rc::Coordinates {
    let x = range + own_radius + target_radius;
    rc::Coordinates::new(
        rc::Value::from_f64_clamped(x),
        rc::Value::from_f64_clamped(0.0),
        rc::Value::from_f64_clamped(0.0),
    )
}

/// Without a range everything overlaps at the origin, which refine treats as point blank.
fn place_ship(sol: &mut SolOwned, ship: ItemId, range: Option<f64>, target_radius: f64) {
    let Some(range) = range else { return };
    let own = radius(sol, ship);
    if let Ok(mut ship) = sol.get_ship(&ship) {
        ship.set_coordinates(centre_distance(range, own, target_radius));
    }
}

fn place_minion(sol: &mut SolOwned, id: ItemId, range: Option<f64>, target_radius: f64) {
    let Some(range) = range else { return };
    let own = radius(sol, id);
    let at = centre_distance(range, own, target_radius);
    if let Ok(mut drone) = sol.get_drone(&id) {
        drone.set_coordinates(at);
    } else if let Ok(mut fighter) = sol.get_fighter(&id) {
        fighter.set_coordinates(at);
    }
}
