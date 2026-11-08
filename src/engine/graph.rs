//! Reading a fit out of refine for the graphs: per-weapon damage, cycle timing, the attributes
//! application math needs, ewar, remote repairs and capacitor users.
//!
//! Ship-wide numbers are the stats pane's, read from the same build as everything per item, so
//! they match it exactly.

mod ewar;
mod reps;
#[cfg(test)]
mod tests;
mod weapons;

use std::collections::HashSet;

use rc::{
    Count, FitId, ItemCommon, ItemId, ItemTypeId, SolOwned, Spool,
    stats::{StatTimeOptions, StatTimeOptionsBurst},
};

use super::{BuildOptions, CalcOptions, Engine, Pilot, build::Placed, read};
use crate::{
    damage_pattern,
    data::{Catalog, Hardpoint, Slot},
    dogma::{attr, effect, group},
    fit::{Fit, ModuleState},
    graphs::{
        ammo::{self, AmmoOption, AmmoQuality, WeaponGroup},
        snapshot::{CapUser, FitSnapshot, Ship, Weapon},
    },
};

use ewar::{drone_ewar, fighter_ewar, module_ewar};
use reps::{drone_rep, module_rep};
use weapons::{drone_weapon, fighter_weapons, module_weapon};

/// Fighter abilities by what they do, as refine maps abilities to effects.
mod ability {
    pub const ATTACK: [i32; 8] = [22, 23, 24, 25, 26, 27, 44, 45];
    pub const MISSILES: [i32; 12] = [18, 19, 20, 21, 29, 30, 31, 32, 33, 34, 35, 36];
    pub const BOMB: i32 = 7;
    pub const KAMIKAZE: i32 = 38;
    pub const WEB: i32 = 2;
    pub const NEUT: i32 = 11;
    pub const ECM: i32 = 12;
}

/// The civilian launcher has no launcher hardpoint but fires missiles.
const CIVILIAN_LAUNCHER: i32 = 32461;

/// A module of the fit as placed in the graph solar system.
struct PlacedModule {
    id: ItemId,
    type_id: i32,
    state: ModuleState,
}

/// What [`Engine::place`] put into the solar system.
struct Placement {
    ship: ItemId,
    modules: Vec<PlacedModule>,
    drones: Vec<PlacedDrones>,
    /// Launched squadrons.
    fighters: Vec<ItemId>,
}

/// A drone stack: its first drone stands for all, as every drone in a stack is the same.
struct PlacedDrones {
    id: ItemId,
    active: u32,
}

impl Engine {
    /// Everything the graphs need about `fit`, read from refine with what surrounds it, as the
    /// main window shows it: `fits` are the saved fits its command and projected fits are looked
    /// up in.
    pub fn graph_snapshot(
        &self,
        fit: &Fit,
        fits: &[Fit],
        pilot: &Pilot,
        catalog: &Catalog,
    ) -> FitSnapshot {
        let mut snap = FitSnapshot::default();
        if fit.ship.is_none() {
            return snap;
        }
        let Some(mut built) = self.build(fit, pilot, BuildOptions::among(fits)) else {
            return snap;
        };
        let stats = super::stats_of(&mut built, fit, CalcOptions::default());
        let Placement {
            ship: ship_id,
            modules,
            drones,
            fighters,
        } = placement(fit, &built.fit);
        let fit_id = built.fit.fit_id;
        let sol = &mut built.sol;

        let mut ship = Ship {
            max_velocity: stats.speed.unwrap_or(0.0),
            sig_radius: stats.sig_radius.unwrap_or(0.0),
            align_time: stats.align_time.unwrap_or(0.0),
            warp_speed: stats.warp_speed.unwrap_or(0.0),
            max_warp_distance: stats.misc.max_warp_range.unwrap_or(0.0),
            scan_res: stats.scan_res.unwrap_or(0.0),
            lock_range: stats.lock_range.unwrap_or(0.0),
            drone_control_range: stats.misc.drone_control_range.unwrap_or(0.0),
            mass: stats.misc.mass.unwrap_or(0.0),
            agility: stats.misc.agility.unwrap_or(0.0),
            resists: [
                stats.shield.resists,
                stats.armor.resists,
                stats.hull.resists,
            ],
            hp: [stats.shield.hp, stats.armor.hp, stats.hull.hp],
            shield_ehp_mult: stats.shield.mult.unwrap_or(1.0),
            tank: [stats.tank.shield, stats.tank.armor, stats.tank.hull],
            shield_regen: stats.tank.passive_shield,
            dps: stats.dps.total(),
            volley: stats.volley.total(),
            rr: stats.outgoing.reps,
            ehp: [stats.shield, stats.armor, stats.hull]
                .iter()
                .map(|l| l.ehp.unwrap_or(l.hp))
                .sum(),
            drone_dps: stats.drone_dps,
            ..Ship::default()
        };
        ship.ehp_uniform = std::array::from_fn(|i| {
            damage_pattern::effective_hp(ship.hp[i], ship.resists[i], damage_pattern::UNIFORM)
        });
        match stats.cap {
            Some(super::Cap::Stable(level)) => ship.cap_stable = Some(level),
            Some(super::Cap::Lasts(secs)) => ship.cap_lasts = Some(secs),
            None => {}
        }
        {
            let mut get = |a| read::attr_or_zero(sol, ship_id, a);
            ship.radius = get(attr::RADIUS);
            ship.cap_capacity = get(attr::CAPACITOR_CAPACITY);
            ship.cap_recharge = get(attr::RECHARGE_RATE) / 1000.0;
            ship.shield_capacity = get(attr::SHIELD_CAPACITY);
            ship.shield_recharge = get(attr::SHIELD_RECHARGE_RATE) / 1000.0;
        }
        // Ships without the attribute take breacher damage in full.
        ship.breacher_resist = read::attr(sol, ship_id, attr::BREACHER_RESIST).unwrap_or(1.0);
        // Warps start from the speed without the modules that cannot run in warp.
        ship.subwarp_speed = grounded(sol, fit_id, &modules, |p| {
            catalog
                .get(p.type_id)
                .is_some_and(|t| group::NOT_IN_WARP.contains(&t.group_id))
        })
        .map_or(ship.max_velocity, |(speed, _)| speed);

        let mut scrammable = Vec::new();
        for placed in &modules {
            let Some(item_type) = catalog.get(placed.type_id) else {
                continue;
            };
            if !placed.state.is_running() {
                continue;
            }
            let group = item_type.group_id;
            let effects = read::effects(sol, placed.id);
            let charge = charge_of(sol, placed.id);
            if effect::SCRAMMABLE.iter().any(|e| effects.contains(e)) {
                scrammable.push(placed.id);
            }
            let scrams = effect::SCRAM.iter().any(|e| effects.contains(e))
                && read::attr_or_zero(sol, placed.id, attr::ACTIVATION_BLOCKED_STRENGTH) != 0.0
                || effects.contains(&effect::DISRUPT_SPHERE)
                    && charge
                        .as_ref()
                        .is_some_and(|c| c.effects.contains(&effect::FOCUSED_SCRIPT));
            if scrams {
                let range = max_range(sol, placed.id);
                snap.scram_range = Some(snap.scram_range.unwrap_or(0.0).max(range));
            }
            if let Some(weapon) = module_weapon(
                sol,
                placed,
                item_type.hardpoint,
                group,
                &effects,
                charge.as_ref(),
                ship.radius,
            ) {
                snap.weapons.push(weapon);
            }
            snap.ewar.extend(module_ewar(sol, placed.id, &effects));
            if let Some(rep) = module_rep(sol, placed.id, group) {
                snap.reps.push(rep);
            }
            if let Some(user) = cap_user(
                sol,
                placed.id,
                group,
                item_type.hardpoint == Hardpoint::Turret,
                charge.as_ref(),
            ) {
                snap.cap_users.push(user);
            }
        }
        for stack in &drones {
            if stack.active == 0 {
                continue;
            }
            let effects = read::effects(sol, stack.id);
            if let Some(weapon) = drone_weapon(sol, stack) {
                snap.weapons.push(weapon);
            }
            snap.ewar.extend(drone_ewar(sol, stack, &effects));
            if let Some(rep) = drone_rep(sol, stack) {
                snap.reps.push(rep);
            }
        }
        for &id in &fighters {
            snap.weapons.extend(fighter_weapons(sol, id));
            snap.ewar.extend(fighter_ewar(sol, id));
        }
        // What a scram leaves of the ship as a target.
        ship.scrammed = grounded(sol, fit_id, &modules, |p| scrammable.contains(&p.id));
        snap.ship = ship;
        snap
    }

    /// The fit's turret or launcher groups, whichever it has more of, each with every charge of
    /// `quality` it takes loaded in turn, for the Application Profile graph. `fits` are the saved
    /// fits, as for [`Engine::graph_snapshot`].
    pub fn graph_ammo(
        &self,
        fit: &Fit,
        fits: &[Fit],
        pilot: &Pilot,
        catalog: &Catalog,
        quality: AmmoQuality,
    ) -> Vec<WeaponGroup> {
        let Some(mut built) = self.build(fit, pilot, BuildOptions::among(fits)) else {
            return Vec::new();
        };
        let placement = placement(fit, &built.fit);
        let sol = &mut built.sol;
        let ship_radius = read::attr_or_zero(sol, placement.ship, attr::RADIUS);
        let running = |p: &&PlacedModule| p.state.is_running();
        let weapon_of = |p: &PlacedModule| {
            catalog
                .get(p.type_id)
                .filter(|t| !t.mining)
                .map(|t| t.hardpoint)
                .filter(|h| *h != Hardpoint::None)
        };
        let count = |hardpoint: Hardpoint| {
            placement
                .modules
                .iter()
                .filter(running)
                .filter(|p| weapon_of(p) == Some(hardpoint))
                .count()
        };
        // pyfa plots the fit's main weapon system only.
        let hardpoint = if count(Hardpoint::Turret) >= count(Hardpoint::Launcher) {
            Hardpoint::Turret
        } else {
            Hardpoint::Launcher
        };
        let mut types: Vec<i32> = Vec::new();
        for p in placement.modules.iter().filter(running) {
            if weapon_of(p) == Some(hardpoint) && !types.contains(&p.type_id) {
                types.push(p.type_id);
            }
        }
        let mut groups = Vec::new();
        for module in types {
            let members: Vec<&PlacedModule> = placement
                .modules
                .iter()
                .filter(running)
                .filter(|p| p.type_id == module)
                .collect();
            let group = catalog.get(module).map_or(0, |t| t.group_id);
            let mut options = Vec::new();
            for charge in ammo::charges_for(catalog, module, quality) {
                for p in &members {
                    if let Ok(mut m) = sol.get_module(&p.id) {
                        m.set_charge_type_id(ItemTypeId::from_i32(charge));
                    }
                }
                let weapons: Vec<Weapon> = members
                    .iter()
                    .filter_map(|p| {
                        let effects = read::effects(sol, p.id);
                        let loaded = charge_of(sol, p.id);
                        module_weapon(
                            sol,
                            p,
                            hardpoint,
                            group,
                            &effects,
                            loaded.as_ref(),
                            ship_radius,
                        )
                    })
                    .collect();
                if !weapons.is_empty() {
                    options.push(AmmoOption {
                        charge,
                        name: catalog.name(charge),
                        weapons,
                    });
                }
            }
            groups.push(WeaponGroup { module, options });
        }
        groups
    }
}

/// Speed and signature with the running modules `off` picks switched to online, None when it
/// picks none. The build is left as it was.
fn grounded(
    sol: &mut SolOwned,
    fit_id: FitId,
    modules: &[PlacedModule],
    mut off: impl FnMut(&PlacedModule) -> bool,
) -> Option<(f64, f64)> {
    let ids: Vec<ItemId> = modules
        .iter()
        .filter(|p| p.state.is_running())
        .filter(|p| off(p))
        .map(|p| p.id)
        .collect();
    if ids.is_empty() {
        return None;
    }
    let mut tx = sol.begin_tx();
    for id in &ids {
        if let Ok(mut module) = tx.get_module(id) {
            module.set_state(rc::ModuleState::Online);
        }
    }
    let mut f = tx.get_fit(&fit_id).ok()?;
    let mut s = f.stats();
    Some((
        s.get_stat_speed().map_or(0.0, rc::PValue::into_f64),
        s.get_stat_sig_radius().map_or(0.0, rc::PValue::into_f64),
    ))
}

/// The modules, drone stacks and launched squadrons of a fit built into a solar system.
fn placement(fit: &Fit, placed: &Placed) -> Placement {
    let modules = placed
        .modules
        .iter()
        .filter(|&&(slot, ..)| slot != Slot::Service)
        .filter_map(|&(slot, i, id)| {
            let m = fit.rack(slot).get(i)?.as_ref()?;
            Some(PlacedModule {
                id,
                type_id: m.type_id,
                state: m.state,
            })
        })
        .collect();
    let drones = placed
        .first_drones()
        .zip(&fit.drones)
        .filter_map(|(id, stack)| {
            Some(PlacedDrones {
                id: id?,
                active: stack.active.min(stack.count),
            })
        })
        .collect();
    Placement {
        ship: placed.ship,
        modules,
        drones,
        fighters: placed.launched_fighters(fit).collect(),
    }
}

/// The loaded charge and its effects.
struct Charge {
    id: ItemId,
    effects: HashSet<i32>,
}

fn charge_of(sol: &mut SolOwned, module: ItemId) -> Option<Charge> {
    let id = sol.get_module(&module).ok()?.get_charge()?.get_item_id();
    let effects = read::effects(sol, id);
    Some(Charge { id, effects })
}

/// pyfa's `maxRange`: the first range attribute a module has.
fn max_range(sol: &mut SolOwned, id: ItemId) -> f64 {
    read::first_attr(
        sol,
        id,
        &[
            attr::MAX_RANGE,
            attr::SHIELD_TRANSFER_RANGE,
            attr::POWER_TRANSFER_RANGE,
            attr::EMP_FIELD_RANGE,
            attr::ECM_BURST_RANGE,
            attr::WARP_SCRAMBLE_RANGE,
            attr::CARGO_SCAN_RANGE,
            attr::SHIP_SCAN_RANGE,
            attr::SURVEY_SCAN_RANGE,
        ],
    )
}

fn falloff(sol: &mut SolOwned, id: ItemId) -> f64 {
    read::first_attr(
        sol,
        id,
        &[
            attr::FALLOFF_EFFECTIVENESS,
            attr::FALLOFF,
            attr::SHIP_SCAN_FALLOFF,
        ],
    )
}

fn spool_at(cycles: u32) -> StatTimeOptions {
    StatTimeOptions::Burst(StatTimeOptionsBurst {
        spool: Some(Spool::Cycles(Count::from_u32(cycles))),
    })
}

/// Cycles to full spool, 0 for modules that do not spool.
fn spool_cycles(sol: &mut SolOwned, id: ItemId) -> u32 {
    sol.get_module(&id)
        .ok()
        .and_then(|mut m| m.get_spool_cycle_count())
        .map_or(0, |info| info.max.into_u32())
}

/// Cycles before a reload and the reload's length, for modules that use charges.
fn reload_of(sol: &mut SolOwned, id: ItemId) -> (Option<u32>, f64) {
    let shots = sol
        .get_module(&id)
        .ok()
        .and_then(|mut m| m.get_charged_cycle_count())
        .map(Count::into_u32)
        .filter(|&n| n > 0);
    (
        shots,
        read::attr_or_zero(sol, id, attr::RELOAD_TIME) / 1000.0,
    )
}

/// A squadron's abilities and whether each runs.
fn abilities(sol: &mut SolOwned, id: ItemId) -> Vec<(i32, bool)> {
    use rc::Lender;

    let mut out = Vec::new();
    if let Ok(mut fighter) = sol.get_fighter(&id) {
        let mut iter = fighter.iter_abilities();
        while let Some(ability) = iter.next() {
            out.push((ability.get_id().into_i32(), ability.get_state()));
        }
    }
    out
}

fn cap_user(
    sol: &mut SolOwned,
    id: ItemId,
    group: i32,
    turret: bool,
    charge: Option<&Charge>,
) -> Option<CapUser> {
    let injector = group == group::CAPACITOR_BOOSTER;
    let need = if injector {
        -read::attr_or_zero(sol, charge?.id, attr::CAPACITOR_BONUS)
    } else {
        read::attr_or_zero(sol, id, attr::CAPACITOR_NEED)
    };
    if need == 0.0 {
        return None;
    }
    let cycle = read::first_attr(sol, id, &[attr::DURATION, attr::RATE_OF_FIRE]);
    let cycle_ms = cycle + read::attr_or_zero(sol, id, attr::REACTIVATION_DELAY);
    if cycle_ms <= 0.0 {
        return None;
    }
    let (shots, _) = reload_of(sol, id);
    Some(CapUser {
        cycle_ms,
        need,
        clip: shots.unwrap_or(0),
        reload_ms: read::attr_or_zero(sol, id, attr::RELOAD_TIME),
        no_stagger: turret,
        injector,
    })
}
