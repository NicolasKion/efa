//! Reading a fit out of refine for the graphs: per-weapon damage, cycle timing, the attributes
//! application math needs, ewar, remote repairs and capacitor users.
//!
//! Ship-wide numbers come from [`Engine::compute`], so they match the stats pane exactly;
//! everything per item comes from one more solar system built here.

use std::collections::HashSet;

use rc::{
    AddMode, AttrId, Count, EveAttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack,
    SolOwned, Spool,
    stats::{
        ItemStatsCommon, StatCritOptions, StatDmg, StatItemChargeOptions, StatTimeOptions,
        StatTimeOptionsBurst,
    },
};

use super::{CalcOptions, Engine, Pilot, module_state};
use crate::{
    data::{Catalog, Hardpoint},
    fit::{Fit, ModuleState},
    graphs::{
        math::{self, Gun},
        snapshot::{
            CapUser, Cycle, Dmg, Ewar, EwarKind, FitSnapshot, Layers, RemoteRep, Ship, Weapon,
            WeaponKind,
        },
    },
};

/// Dogma attribute IDs read here.
mod attr {
    pub const CAP_NEED: i32 = 6;
    pub const SPEED_FACTOR: i32 = 20;
    pub const MASS: i32 = 4;
    pub const VELOCITY: i32 = 37;
    pub const RATE_OF_FIRE: i32 = 51;
    pub const OPTIMAL: i32 = 54;
    pub const RECHARGE_RATE: i32 = 55;
    pub const CAPACITOR_BONUS: i32 = 67;
    pub const AGILITY: i32 = 70;
    pub const DURATION: i32 = 73;
    pub const SHIELD_TRANSFER_RANGE: i32 = 87;
    pub const POWER_TRANSFER_RANGE: i32 = 91;
    pub const EMP_FIELD_RANGE: i32 = 99;
    pub const WARP_SCRAMBLE_RANGE: i32 = 103;
    pub const EXPLOSION_RANGE: i32 = 107;
    pub const SHIP_SCAN_RANGE: i32 = 125;
    pub const CARGO_SCAN_RANGE: i32 = 126;
    pub const ECM_BURST_RANGE: i32 = 142;
    pub const FALLOFF: i32 = 158;
    pub const TRACKING: i32 = 160;
    pub const RADIUS: i32 = 162;
    pub const SURVEY_SCAN_RANGE: i32 = 197;
    pub const ECM_STRENGTH: [i32; 4] = [238, 239, 240, 241];
    pub const SHIELD_CAPACITY: i32 = 263;
    pub const EXPLOSION_DELAY: i32 = 281;
    pub const MAX_TARGET_RANGE_BONUS: i32 = 309;
    pub const MAX_RANGE_BONUS: i32 = 351;
    pub const SHIELD_RECHARGE_RATE: i32 = 479;
    pub const CAP_CAPACITY: i32 = 482;
    pub const SHIP_SCAN_FALLOFF: i32 = 510;
    pub const MISSILE_VELOCITY_BONUS: i32 = 547;
    pub const SIGNATURE_RADIUS: i32 = 552;
    pub const SIGNATURE_RADIUS_BONUS: i32 = 554;
    pub const EXPLOSION_DELAY_BONUS: i32 = 596;
    pub const OPTIMAL_SIG_RADIUS: i32 = 620;
    pub const AOE_VELOCITY: i32 = 653;
    pub const AOE_CLOUD_SIZE: i32 = 654;
    pub const REACTIVATION_DELAY: i32 = 669;
    pub const ECM_RANGE_OPTIMAL: i32 = 936;
    pub const AOE_DRF: i32 = 1353;
    pub const RELOAD_TIME: i32 = 1795;
    pub const CHARGED_ARMOR_MULT: i32 = 1886;
    pub const NOS_OVERRIDE: i32 = 1945;
    pub const FALLOFF_EFFECTIVENESS: i32 = 2044;
    pub const DOOMSDAY_AOE_RANGE: i32 = 2279;
    pub const MAX_FOF_RANGE: i32 = 2700;
}

/// Dogma effect IDs that decide what a module or drone does to its target.
mod effect {
    pub const FOF_MISSILE: i32 = 104;
    pub const SUPER_WEAPONS: [i32; 5] = [4489, 4490, 4491, 4492, 6447];
    pub const NEUT: [i32; 2] = [6187, 6216];
    pub const NOS: i32 = 6197;
    pub const DD_NEUT: i32 = 6477;
    pub const WEB: [i32; 2] = [6426, 6682];
    pub const DD_WEB: i32 = 6476;
    pub const ECM: [i32; 2] = [6470, 6685];
    pub const DD_ECM: i32 = 6513;
    pub const DAMP: [i32; 2] = [6422, 6684];
    pub const DD_DAMP: i32 = 6481;
    pub const TD: [i32; 2] = [6424, 6686];
    pub const GD: [i32; 2] = [6423, 6686];
    pub const DD_TRACK: i32 = 6479;
    pub const TP: [i32; 2] = [6425, 6683];
    pub const DD_PAINT: i32 = 6478;
    pub const DRONE_NEUT: i32 = 6691;
    pub const DRONE_WEB: i32 = 6690;
    pub const DRONE_TP: i32 = 6692;
    pub const DRONE_DAMP: i32 = 6693;
    pub const DRONE_TD: i32 = 6694;
    pub const DRONE_ECM: i32 = 6695;
    pub const VORTON: i32 = 8037;
}

/// Group IDs the graphs treat specially, as pyfa does by group name.
mod group {
    pub const REMOTE_SHIELD: i32 = 41;
    pub const SMART_BOMB: i32 = 72;
    pub const CAP_BOOSTER: i32 = 76;
    pub const REMOTE_ARMOR: i32 = 325;
    pub const REMOTE_HULL: i32 = 585;
    pub const SUPER_WEAPON: i32 = 588;
    pub const BOMB_LAUNCHER: i32 = 862;
    pub const GUIDED_BOMB_LAUNCHER: i32 = 1328;
    pub const AREA_DENIAL: i32 = 1330;
    pub const STRUCTURE_DOOMSDAY: i32 = 1333;
    pub const ANCILLARY_SHIELD: i32 = 1697;
    pub const ANCILLARY_ARMOR: i32 = 1698;
    pub const MUTADAPTIVE_ARMOR: i32 = 2018;
    /// Modules that change speed but cannot run in warp: off for the speed a warp starts at.
    pub const NOT_IN_WARP: [i32; 8] = [46, 2008, 330, 515, 588, 658, 815, 590];
}

/// The civilian launcher has no launcher hardpoint but fires missiles.
const CIVILIAN_LAUNCHER: i32 = 32461;

/// A module of the fit as placed in the graph solar system.
struct Placed {
    id: ItemId,
    type_id: i32,
    state: ModuleState,
}

/// A drone stack: its first drone stands for all, as every drone in a stack is the same.
struct PlacedDrones {
    id: ItemId,
    active: u32,
}

impl Engine {
    /// Everything the graphs need about `fit`, read from refine.
    pub fn graph_snapshot(&self, fit: &Fit, pilot: &Pilot, catalog: &Catalog) -> FitSnapshot {
        let mut snap = FitSnapshot::default();
        if fit.ship.is_none() {
            return snap;
        }
        let stats = self.compute_for(fit, pilot, CalcOptions::default());
        let mut sol = SolOwned::new(&self.src);
        let Some((ship_id, modules, drones)) = Self::place(&mut sol, fit, pilot) else {
            return snap;
        };

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
            ..Ship::default()
        };
        // Against uniform damage a layer's EHP is its HP over the average resonance.
        ship.ehp_uniform = std::array::from_fn(|i| {
            let resonance = ship.resists[i].iter().map(|r| (1.0 - r) / 4.0).sum::<f64>();
            if resonance > 0.0 {
                ship.hp[i] / resonance
            } else {
                0.0
            }
        });
        match stats.cap {
            Some(super::Cap::Stable(level)) => ship.cap_stable = Some(level),
            Some(super::Cap::Lasts(secs)) => ship.cap_lasts = Some(secs),
            None => {}
        }
        {
            let mut get = |a| attr_of(&mut sol, ship_id, a);
            ship.radius = get(attr::RADIUS);
            ship.cap_capacity = get(attr::CAP_CAPACITY);
            ship.cap_recharge = get(attr::RECHARGE_RATE) / 1000.0;
            ship.shield_capacity = get(attr::SHIELD_CAPACITY);
            ship.shield_recharge = get(attr::SHIELD_RECHARGE_RATE) / 1000.0;
        }
        ship.subwarp_speed = self
            .subwarp_speed(fit, pilot, catalog)
            .unwrap_or(ship.max_velocity);

        for placed in &modules {
            let Some(item_type) = catalog.get(placed.type_id) else {
                continue;
            };
            let running = matches!(placed.state, ModuleState::Active | ModuleState::Overload);
            if !running {
                continue;
            }
            let group = item_type.group_id;
            let effects = effects_of(&mut sol, placed.id);
            let charge = charge_of(&mut sol, placed.id);
            if let Some(weapon) = module_weapon(
                &mut sol,
                placed,
                item_type.hardpoint,
                group,
                &effects,
                charge.as_ref(),
                ship.radius,
            ) {
                snap.weapons.push(weapon);
            }
            snap.ewar.extend(module_ewar(&mut sol, placed.id, &effects));
            if let Some(rep) = module_rep(&mut sol, placed.id, group) {
                snap.reps.push(rep);
            }
            if let Some(user) = cap_user(
                &mut sol,
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
            let effects = effects_of(&mut sol, stack.id);
            if let Some(weapon) = drone_weapon(&mut sol, stack) {
                snap.weapons.push(weapon);
            }
            snap.ewar.extend(drone_ewar(&mut sol, stack, &effects));
            if let Some(rep) = drone_rep(&mut sol, stack) {
                snap.reps.push(rep);
            }
        }
        snap.ship = ship;
        snap
    }

    /// Top speed with the modules that cannot run in warp switched off.
    fn subwarp_speed(&self, fit: &Fit, pilot: &Pilot, catalog: &Catalog) -> Option<f64> {
        let mut grounded = fit.clone();
        let mut changed = false;
        for module in [&mut grounded.high, &mut grounded.mid, &mut grounded.low]
            .into_iter()
            .flatten()
            .flatten()
        {
            let stops = catalog
                .get(module.type_id)
                .is_some_and(|t| group::NOT_IN_WARP.contains(&t.group_id));
            if stops && matches!(module.state, ModuleState::Active | ModuleState::Overload) {
                module.state = ModuleState::Online;
                changed = true;
            }
        }
        if !changed {
            return None;
        }
        self.compute_for(&grounded, pilot, CalcOptions::default())
            .speed
    }

    /// Put the fit into `sol` the way [`Engine::compute_with`] does, keeping item IDs.
    fn place(
        sol: &mut SolOwned,
        fit: &Fit,
        pilot: &Pilot,
    ) -> Option<(ItemId, Vec<Placed>, Vec<PlacedDrones>)> {
        let ship = fit.ship?;
        let mut f = sol.add_fit();
        pilot.board(&mut f);
        let ship_id = f
            .set_ship(ItemTypeId::from_i32(ship), None, None)
            .get_item_id();
        let mut placed = Vec::new();
        for (rack, modules) in [
            (ModRack::High, &fit.high),
            (ModRack::Mid, &fit.mid),
            (ModRack::Low, &fit.low),
        ] {
            for (i, m) in modules.iter().enumerate() {
                let Some(m) = m else { continue };
                let Ok(mut module) = f.add_module(
                    rack,
                    AddMode::Replace(rc::RackPos::from_usize_clamped(i)),
                    ItemTypeId::from_i32(m.type_id),
                    module_state(m.state),
                ) else {
                    continue;
                };
                if let Some(cycles) = m.spool {
                    module.set_spool_override(Some(Spool::Cycles(Count::from_u32(cycles))));
                }
                if let Some(charge) = m.charge {
                    module.set_charge_type_id(ItemTypeId::from_i32(charge));
                }
                placed.push(Placed {
                    id: module.get_item_id(),
                    type_id: m.type_id,
                    state: m.state,
                });
            }
        }
        if let Some(mode) = fit.mode {
            f.set_stance(ItemTypeId::from_i32(mode));
        }
        f.set_sec_status(rc::FitSecStatus::from_f64_clamped(
            fit.sec_status.unwrap_or(pilot.sec_status),
        ));
        for &rig in fit.rigs.iter().flatten() {
            f.add_rig(ItemTypeId::from_i32(rig));
        }
        for &subsystem in &fit.subsystems {
            f.add_subsystem(ItemTypeId::from_i32(subsystem));
        }
        super::add_services(&mut f, fit);
        let mut drones = Vec::new();
        for stack in &fit.drones {
            let mut first = None;
            for i in 0..stack.count {
                let state = if i < stack.active {
                    MinionState::Engaging
                } else {
                    MinionState::InBay
                };
                let id = f
                    .add_drone(ItemTypeId::from_i32(stack.type_id), state, None, None)
                    .get_item_id();
                first.get_or_insert(id);
            }
            if let Some(id) = first {
                drones.push(PlacedDrones {
                    id,
                    active: stack.active.min(stack.count),
                });
            }
        }
        for (implant, active) in fit.applied_implants(&pilot.implants) {
            let mut implant = f.add_implant(ItemTypeId::from_i32(implant));
            if !active {
                implant.set_state(false);
            }
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
        Some((ship_id, placed, drones))
    }
}

/// An attribute after bonuses, 0 when the item lacks it.
fn attr_of(sol: &mut SolOwned, id: ItemId, attr: i32) -> f64 {
    sol.get_item(&id)
        .ok()
        .and_then(|mut item| item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr))).ok())
        .map_or(0.0, |v| v.modified.into_f64())
}

/// The first of `attrs` the item has a nonzero value for.
fn first_attr(sol: &mut SolOwned, id: ItemId, attrs: &[i32]) -> f64 {
    attrs
        .iter()
        .map(|&a| attr_of(sol, id, a))
        .find(|&v| v != 0.0)
        .unwrap_or(0.0)
}

fn effects_of(sol: &mut SolOwned, id: ItemId) -> HashSet<i32> {
    sol.get_item(&id)
        .ok()
        .and_then(|item| {
            item.iter_effects().ok().map(|effects| {
                effects
                    .filter_map(|(effect, _)| match effect {
                        rc::EffectId::Dogma(id) => Some(id.into_i32()),
                        _ => None,
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// The loaded charge and its effects.
struct Charge {
    id: ItemId,
    effects: HashSet<i32>,
}

fn charge_of(sol: &mut SolOwned, module: ItemId) -> Option<Charge> {
    let id = sol.get_module(&module).ok()?.get_charge()?.get_item_id();
    let effects = effects_of(sol, id);
    Some(Charge { id, effects })
}

/// pyfa's `maxRange`: the first range attribute a module has.
fn max_range(sol: &mut SolOwned, id: ItemId) -> f64 {
    first_attr(
        sol,
        id,
        &[
            attr::OPTIMAL,
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
    first_attr(
        sol,
        id,
        &[
            attr::FALLOFF_EFFECTIVENESS,
            attr::FALLOFF,
            attr::SHIP_SCAN_FALLOFF,
        ],
    )
}

fn dmg(entry: &rc::stats::StatDmgEntry) -> Dmg {
    [entry.em, entry.thermal, entry.kinetic, entry.explosive].map(rc::PValue::into_f64)
}

fn item_dmg(sol: &mut SolOwned, id: ItemId, time: StatTimeOptions) -> Option<StatDmg> {
    let mut stats = sol.stats();
    let mut item = stats.get_item(&id).ok()?;
    item.get_stat_dmg(
        time,
        StatCritOptions::default(),
        StatItemChargeOptions::Include,
    )
    .ok()
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
    (shots, attr_of(sol, id, attr::RELOAD_TIME) / 1000.0)
}

fn module_weapon(
    sol: &mut SolOwned,
    placed: &Placed,
    hardpoint: Hardpoint,
    group: i32,
    effects: &HashSet<i32>,
    charge: Option<&Charge>,
    ship_radius: f64,
) -> Option<Weapon> {
    let id = placed.id;
    let burst = item_dmg(sol, id, StatTimeOptions::default())?;
    let (dps, volley) = (dmg(&burst.dps), dmg(&burst.volley));
    if dps.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let kind = if effects.contains(&effect::VORTON) {
        WeaponKind::Vorton {
            range: attr_of(sol, id, attr::OPTIMAL),
            er: attr_of(sol, id, attr::AOE_CLOUD_SIZE),
            ev: attr_of(sol, id, attr::AOE_VELOCITY),
            drf: attr_of(sol, id, attr::AOE_DRF),
        }
    } else if hardpoint == Hardpoint::Turret {
        WeaponKind::Turret(Gun {
            optimal: max_range(sol, id),
            falloff: falloff(sol, id),
            tracking: attr_of(sol, id, attr::TRACKING),
            optimal_sig: attr_of(sol, id, attr::OPTIMAL_SIG_RADIUS),
        })
    } else if hardpoint == Hardpoint::Launcher || placed.type_id == CIVILIAN_LAUNCHER {
        let charge = charge?;
        let c = charge.id;
        let fof = charge.effects.contains(&effect::FOF_MISSILE);
        let range = math::missile_range(
            attr_of(sol, c, attr::VELOCITY),
            attr_of(sol, c, attr::EXPLOSION_DELAY),
            attr_of(sol, c, attr::MASS),
            attr_of(sol, c, attr::AGILITY),
            ship_radius,
            fof.then(|| attr_of(sol, c, attr::MAX_FOF_RANGE)),
        );
        WeaponKind::Launcher {
            er: attr_of(sol, c, attr::AOE_CLOUD_SIZE),
            ev: attr_of(sol, c, attr::AOE_VELOCITY),
            drf: attr_of(sol, c, attr::AOE_DRF),
            range,
            fof,
        }
    } else if matches!(group, group::SMART_BOMB | group::AREA_DENIAL) {
        WeaponKind::Smartbomb {
            range: max_range(sol, id),
        }
    } else if group == group::BOMB_LAUNCHER {
        let c = charge?.id;
        // A bomb flies its full flight time before it goes off.
        let range =
            attr_of(sol, c, attr::VELOCITY) * attr_of(sol, c, attr::EXPLOSION_DELAY) / 1000.0;
        WeaponKind::Bomb {
            range,
            blast_radius: attr_of(sol, c, attr::EXPLOSION_RANGE),
            er: attr_of(sol, c, attr::AOE_CLOUD_SIZE),
        }
    } else if group == group::GUIDED_BOMB_LAUNCHER {
        let c = charge?.id;
        let range =
            attr_of(sol, c, attr::VELOCITY) * attr_of(sol, c, attr::EXPLOSION_DELAY) / 1000.0;
        WeaponKind::GuidedBomb {
            range,
            er: attr_of(sol, c, attr::AOE_CLOUD_SIZE),
        }
    } else if matches!(group, group::SUPER_WEAPON | group::STRUCTURE_DOOMSDAY) {
        WeaponKind::Doomsday {
            range: max_range(sol, id),
            damage_sig: attr_of(sol, id, attr::SIGNATURE_RADIUS),
            needs_lock: effect::SUPER_WEAPONS.iter().any(|e| effects.contains(e)),
        }
    } else {
        // Breacher pods and anything else pyfa has no application model for.
        return None;
    };

    let duration = cycle_of(&volley, &dps)?;
    let (shots, reload) = reload_of(sol, id);
    let spool = spool_cycles(sol, id);
    let amounts = if spool > 0 {
        (0..=spool)
            .map(|n| item_dmg(sol, id, spool_at(n)).map_or(volley, |d| dmg(&d.volley)))
            .collect()
    } else {
        vec![volley]
    };
    Some(Weapon {
        kind,
        dps,
        volley,
        cycle: Cycle {
            duration,
            shots,
            reload,
            amounts,
            delay: 0.0,
        },
    })
}

/// Seconds per cycle, from what refine says one volley and one second deal.
fn cycle_of(volley: &Dmg, dps: &Dmg) -> Option<f64> {
    let (v, d) = (volley.iter().sum::<f64>(), dps.iter().sum::<f64>());
    (v > 0.0 && d > 0.0).then_some(v / d)
}

fn drone_weapon(sol: &mut SolOwned, stack: &PlacedDrones) -> Option<Weapon> {
    let id = stack.id;
    let burst = item_dmg(sol, id, StatTimeOptions::default())?;
    let count = f64::from(stack.active);
    let (dps, volley) = (
        dmg(&burst.dps).map(|v| v * count),
        dmg(&burst.volley).map(|v| v * count),
    );
    let duration = cycle_of(&volley, &dps)?;
    Some(Weapon {
        kind: WeaponKind::Drone {
            gun: Gun {
                optimal: first_attr(sol, id, &[attr::OPTIMAL]),
                falloff: first_attr(sol, id, &[attr::FALLOFF, attr::FALLOFF_EFFECTIVENESS]),
                tracking: attr_of(sol, id, attr::TRACKING),
                optimal_sig: attr_of(sol, id, attr::OPTIMAL_SIG_RADIUS),
            },
            speed: attr_of(sol, id, attr::VELOCITY),
            radius: attr_of(sol, id, attr::RADIUS),
        },
        dps,
        volley,
        cycle: Cycle {
            duration,
            shots: None,
            reload: 0.0,
            amounts: vec![volley],
            delay: 0.0,
        },
    })
}

/// Neutralizing per second as refine counts it for one item.
fn item_nps(sol: &mut SolOwned, id: ItemId) -> f64 {
    let mut stats = sol.stats();
    stats
        .get_item(&id)
        .ok()
        .and_then(|mut item| {
            item.get_stat_outgoing_nps(StatTimeOptions::default(), StatItemChargeOptions::Include)
                .ok()
        })
        .map_or(0.0, rc::PValue::into_f64)
}

fn module_ewar(sol: &mut SolOwned, id: ItemId, effects: &HashSet<i32>) -> Vec<Ewar> {
    let has = |list: &[i32]| list.iter().any(|e| effects.contains(e));
    let optimal = max_range(sol, id);
    let falloff = falloff(sol, id);
    let dd_range = (optimal + attr_of(sol, id, attr::DOOMSDAY_AOE_RANGE)).max(0.0);
    let mut out = Vec::new();
    let mut push = |kind, strength, strength2, doomsday: bool| {
        out.push(Ewar {
            kind,
            strength,
            strength2,
            optimal: if doomsday { dd_range } else { optimal },
            falloff,
            needs_lock: !doomsday,
            needs_dcr: false,
            mobile: None,
        });
    };
    let nos_as_neut = effects.contains(&effect::NOS) && attr_of(sol, id, attr::NOS_OVERRIDE) != 0.0;
    if has(&effect::NEUT) || nos_as_neut {
        push(EwarKind::Neut, item_nps(sol, id), 0.0, false);
    }
    if effects.contains(&effect::DD_NEUT) {
        push(EwarKind::Neut, item_nps(sol, id), 0.0, true);
    }
    let web = attr_of(sol, id, attr::SPEED_FACTOR);
    if has(&effect::WEB) {
        push(EwarKind::Web, web, 0.0, false);
    }
    if effects.contains(&effect::DD_WEB) {
        push(EwarKind::Web, web, 0.0, true);
    }
    let ecm = attr::ECM_STRENGTH
        .iter()
        .map(|&a| attr_of(sol, id, a))
        .fold(0.0, f64::max);
    if has(&effect::ECM) {
        push(EwarKind::Ecm, ecm, 0.0, false);
    }
    if effects.contains(&effect::DD_ECM) {
        push(EwarKind::Ecm, ecm, 0.0, true);
    }
    let damp = attr_of(sol, id, attr::MAX_TARGET_RANGE_BONUS);
    if has(&effect::DAMP) {
        push(EwarKind::Damp, damp, 0.0, false);
    }
    if effects.contains(&effect::DD_DAMP) {
        push(EwarKind::Damp, damp, 0.0, true);
    }
    let td = attr_of(sol, id, attr::MAX_RANGE_BONUS);
    if has(&effect::TD) {
        push(EwarKind::TrackingDisruptor, td, 0.0, false);
    }
    let (gd_velocity, gd_flight) = (
        attr_of(sol, id, attr::MISSILE_VELOCITY_BONUS),
        attr_of(sol, id, attr::EXPLOSION_DELAY_BONUS),
    );
    if has(&effect::GD) {
        push(EwarKind::GuidanceDisruptor, gd_velocity, gd_flight, false);
    }
    if effects.contains(&effect::DD_TRACK) {
        push(EwarKind::TrackingDisruptor, td, 0.0, true);
        push(EwarKind::GuidanceDisruptor, gd_velocity, gd_flight, true);
    }
    let tp = attr_of(sol, id, attr::SIGNATURE_RADIUS_BONUS);
    if has(&effect::TP) {
        push(EwarKind::TargetPainter, tp, 0.0, false);
    }
    if effects.contains(&effect::DD_PAINT) {
        push(EwarKind::TargetPainter, tp, 0.0, true);
    }
    out
}

fn drone_ewar(sol: &mut SolOwned, stack: &PlacedDrones, effects: &HashSet<i32>) -> Vec<Ewar> {
    let id = stack.id;
    let speed = attr_of(sol, id, attr::VELOCITY);
    let radius = attr_of(sol, id, attr::RADIUS);
    let optimal = first_attr(sol, id, &[attr::OPTIMAL, attr::ECM_RANGE_OPTIMAL]);
    let falloff = first_attr(sol, id, &[attr::FALLOFF, attr::FALLOFF_EFFECTIVENESS]);
    let mut out = Vec::new();
    let kinds = [
        (effect::DRONE_NEUT, EwarKind::Neut),
        (effect::DRONE_WEB, EwarKind::Web),
        (effect::DRONE_ECM, EwarKind::Ecm),
        (effect::DRONE_DAMP, EwarKind::Damp),
        (effect::DRONE_TD, EwarKind::TrackingDisruptor),
        (effect::DRONE_TP, EwarKind::TargetPainter),
    ];
    for (effect_id, kind) in kinds {
        if !effects.contains(&effect_id) {
            continue;
        }
        let strength = match kind {
            EwarKind::Neut => item_nps(sol, id),
            EwarKind::Web => attr_of(sol, id, attr::SPEED_FACTOR),
            EwarKind::Ecm => attr::ECM_STRENGTH
                .iter()
                .map(|&a| attr_of(sol, id, a))
                .fold(0.0, f64::max),
            EwarKind::Damp => attr_of(sol, id, attr::MAX_TARGET_RANGE_BONUS),
            EwarKind::TrackingDisruptor => attr_of(sol, id, attr::MAX_RANGE_BONUS),
            EwarKind::GuidanceDisruptor | EwarKind::TargetPainter => {
                attr_of(sol, id, attr::SIGNATURE_RADIUS_BONUS)
            }
        };
        // pyfa counts each drone of the stack on its own, so stacking penalties apply.
        for _ in 0..stack.active {
            out.push(Ewar {
                kind,
                strength,
                strength2: 0.0,
                optimal,
                falloff,
                needs_lock: true,
                needs_dcr: true,
                mobile: Some((speed, radius)),
            });
        }
    }
    out
}

/// Shield, armor and hull repaired per second by one item at the given spool.
fn item_rps(sol: &mut SolOwned, id: ItemId, time: StatTimeOptions) -> Layers {
    let mut stats = sol.stats();
    stats
        .get_item(&id)
        .ok()
        .and_then(|mut item| item.get_stat_outgoing_rps(time).ok())
        .map_or([0.0; 3], |r| {
            [r.shield, r.armor, r.hull].map(rc::PValue::into_f64)
        })
}

fn module_rep(sol: &mut SolOwned, id: ItemId, group: i32) -> Option<RemoteRep> {
    let shield = matches!(group, group::REMOTE_SHIELD | group::ANCILLARY_SHIELD);
    let armor_or_hull = matches!(
        group,
        group::REMOTE_ARMOR
            | group::REMOTE_HULL
            | group::ANCILLARY_ARMOR
            | group::MUTADAPTIVE_ARMOR
    );
    if !shield && !armor_or_hull {
        return None;
    }
    let duration = attr_of(sol, id, attr::DURATION) / 1000.0;
    if duration <= 0.0 {
        return None;
    }
    let spool = spool_cycles(sol, id);
    let per_cycle = |rps: Layers| rps.map(|v| v * duration);
    let amounts: Vec<Layers> = if spool > 0 {
        (0..=spool)
            .map(|n| per_cycle(item_rps(sol, id, spool_at(n))))
            .collect()
    } else {
        vec![per_cycle(item_rps(sol, id, StatTimeOptions::default()))]
    };
    if amounts.iter().all(|a| a.iter().sum::<f64>() <= 0.0) {
        return None;
    }
    let (shots, reload) = reload_of(sol, id);
    // Armor and hull repairs land at the end of the cycle, shield boosts at its start.
    let delay = if shield { 0.0 } else { duration };
    let cycle = Cycle {
        duration,
        shots,
        reload,
        amounts: amounts.clone(),
        delay,
    };
    let ancillary = matches!(group, group::ANCILLARY_SHIELD | group::ANCILLARY_ARMOR);
    let no_reload = (ancillary && shots.is_some()).then(|| {
        let charged = amounts[0];
        // Out of paste, an ancillary armor repairer keeps going at its plain amount.
        let plain = if group == group::ANCILLARY_ARMOR {
            let mult = attr_of(sol, id, attr::CHARGED_ARMOR_MULT);
            if mult > 0.0 {
                charged.map(|v| v / mult)
            } else {
                charged
            }
        } else {
            charged
        };
        let mut amounts = vec![charged; shots.map_or(0, |s| s as usize)];
        amounts.push(plain);
        Cycle {
            duration,
            shots: None,
            reload: 0.0,
            amounts,
            delay,
        }
    });
    Some(RemoteRep {
        cycle,
        no_reload,
        optimal: max_range(sol, id),
        falloff: falloff(sol, id),
        drone: false,
    })
}

fn drone_rep(sol: &mut SolOwned, stack: &PlacedDrones) -> Option<RemoteRep> {
    let id = stack.id;
    let rps = item_rps(sol, id, StatTimeOptions::default());
    if rps.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let duration = first_attr(sol, id, &[attr::DURATION, attr::RATE_OF_FIRE]) / 1000.0;
    let duration = if duration > 0.0 { duration } else { 1.0 };
    let count = f64::from(stack.active);
    Some(RemoteRep {
        cycle: Cycle {
            duration,
            shots: None,
            reload: 0.0,
            amounts: vec![rps.map(|v| v * duration * count)],
            delay: duration,
        },
        no_reload: None,
        optimal: 0.0,
        falloff: 0.0,
        drone: true,
    })
}

fn cap_user(
    sol: &mut SolOwned,
    id: ItemId,
    group: i32,
    turret: bool,
    charge: Option<&Charge>,
) -> Option<CapUser> {
    let injector = group == group::CAP_BOOSTER;
    let need = if injector {
        -attr_of(sol, charge?.id, attr::CAPACITOR_BONUS)
    } else {
        attr_of(sol, id, attr::CAP_NEED)
    };
    if need == 0.0 {
        return None;
    }
    let cycle = first_attr(sol, id, &[attr::DURATION, attr::RATE_OF_FIRE]);
    let cycle_ms = cycle + attr_of(sol, id, attr::REACTIVATION_DELAY);
    if cycle_ms <= 0.0 {
        return None;
    }
    let (shots, _) = reload_of(sol, id);
    Some(CapUser {
        cycle_ms,
        need,
        clip: shots.unwrap_or(0),
        reload_ms: attr_of(sol, id, attr::RELOAD_TIME),
        no_stagger: turret,
        injector,
    })
}

#[cfg(test)]
mod tests {
    //! The graphs' own application math checked against refine's applied damage, on real data.
    //! Skipped when no dump is present.

    use std::{collections::HashMap, path::Path};

    use rc::{Coordinates, Value, stats::StatDmgItemKinds};

    use super::*;
    use crate::graphs::{
        defs::{Graph, Param},
        plot::{self, Options, Request, TargetData},
    };

    const DATA: &str = "data/eve";

    const RUPTURE: &str = "[Rupture, Guns]
Gyrostabilizer II

Stasis Webifier II

425mm AutoCannon II, Barrage M
425mm AutoCannon II, Barrage M
425mm AutoCannon II, Barrage M
425mm AutoCannon II, Barrage M
";

    const CARACAL: &str = "[Caracal, Missiles]
Ballistic Control System II

Rapid Light Missile Launcher II, Scourge Light Missile
Rapid Light Missile Launcher II, Scourge Light Missile
Rapid Light Missile Launcher II, Scourge Light Missile
";

    const TARGET: &str = "[Vexor, Target]
Damage Control II
";

    fn setup() -> Option<(Catalog, Engine)> {
        if !Path::new(DATA).join("phobos/metadata.json").exists() {
            eprintln!("no EVE data in {DATA}, skipping");
            return None;
        }
        let catalog = Catalog::load(Path::new(DATA)).expect("the dump loads");
        let engine = Engine::load(
            Path::new(DATA),
            Path::new("data/cache"),
            catalog.skills.clone(),
        )
        .expect("the engine loads");
        Some((catalog, engine))
    }

    fn fit(catalog: &Catalog, eft: &str) -> Fit {
        let (mut fit, unknown) = Fit::from_eft(catalog, eft).expect("the fit parses");
        assert!(unknown.is_empty(), "unknown items: {unknown:?}");
        for m in [&mut fit.high, &mut fit.mid, &mut fit.low]
            .into_iter()
            .flatten()
            .flatten()
        {
            m.state = ModuleState::Active;
        }
        fit
    }

    /// Weapon DPS refine applies to a standing target `distance` meters away, surface to
    /// surface, before resists.
    fn refine_applied(engine: &Engine, attacker: &Fit, target: &Fit, distance: f64) -> f64 {
        let mut sol = SolOwned::new(&engine.src);
        let (atk_ship, modules, _) =
            Engine::place(&mut sol, attacker, engine.all5()).expect("attacker places");
        let (tgt_ship, _, _) =
            Engine::place(&mut sol, target, engine.all5()).expect("target places");
        let c2c = distance
            + attr_of(&mut sol, atk_ship, attr::RADIUS)
            + attr_of(&mut sol, tgt_ship, attr::RADIUS);
        sol.get_ship(&tgt_ship)
            .expect("target ship")
            .set_coordinates(Coordinates::new(
                Value::from_f64_clamped(c2c),
                Value::from_f64_clamped(0.0),
                Value::from_f64_clamped(0.0),
            ));
        for m in &modules {
            if let Ok(mut module) = sol.get_module(&m.id) {
                let _ = module.add_proj(&tgt_ship);
            }
        }
        let fit_id = sol.get_ship(&atk_ship).expect("attacker ship").get_fit_id();
        let mut f = sol.get_fit(&fit_id).expect("attacker fit");
        let applied = f
            .stats()
            .get_stat_dmg_applied(
                StatDmgItemKinds::default(),
                StatTimeOptions::default(),
                StatCritOptions::default(),
                &tgt_ship,
            )
            .expect("target takes projections");
        [
            applied.dps.em,
            applied.dps.thermal,
            applied.dps.kinetic,
            applied.dps.explosive,
        ]
        .map(rc::PValue::into_f64)
        .iter()
        .sum()
    }

    fn graph_dps(engine: &Engine, catalog: &Catalog, attacker: &Fit, target: &Fit) -> plot::Series {
        let src = engine.graph_snapshot(attacker, engine.all5(), catalog);
        let tgt = engine.graph_snapshot(target, engine.all5(), catalog);
        let target = TargetData {
            max_velocity: tgt.ship.max_velocity,
            sig_radius: tgt.ship.sig_radius,
            radius: tgt.ship.radius,
            resists: [0.0; 4],
            hp: None,
        };
        let graph = Graph::DamageStats;
        let req = Request {
            graph,
            x: graph.x_defs(true)[0],
            y: graph.y_defs(true, true)[0],
            range: (0.0, 50.0),
            // A standing target: refine only sees transversal when ships move.
            misc: HashMap::from([(Param::TgtSpeed, Some(0.0))]),
            checkboxes: HashMap::new(),
            options: Options {
                ignore_resists: true,
                apply_projected: false,
                ignore_lock_range: true,
                ..Options::default()
            },
        };
        plot::plot(&req, &src, Some(&target)).expect("a line")
    }

    fn compare(eft: &str, distances_km: &[f64]) {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let attacker = fit(&catalog, eft);
        let target = fit(&catalog, TARGET);
        let series = graph_dps(&engine, &catalog, &attacker, &target);
        for &km in distances_km {
            let ours = series.at(km).expect("in range");
            let theirs = refine_applied(&engine, &attacker, &target, km * 1000.0);
            assert!(
                (ours - theirs).abs() <= theirs.abs() * 0.02 + 0.5,
                "{km} km: graph {ours}, refine {theirs}"
            );
        }
    }

    #[test]
    fn turret_application_matches_refine() {
        compare(RUPTURE, &[1.0, 5.0, 10.0, 15.0, 20.0, 30.0]);
    }

    /// Only where both agree a missile surely arrives or surely does not: pyfa adds the ship's
    /// radius over missile velocity to the flight time (pyfa issue 2083) and refine does not,
    /// so the partial band past the last whole second differs on purpose.
    #[test]
    fn missile_application_matches_refine() {
        compare(CARACAL, &[5.0, 20.0, 38.0, 49.0]);
    }

    /// Damage dealt by a time, from the graph's cycle model and from refine's own simulation.
    #[test]
    fn damage_over_time_matches_refine_simulation() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let attacker = fit(&catalog, RUPTURE);
        let snap = engine.graph_snapshot(&attacker, engine.all5(), &catalog);
        let mut sol = SolOwned::new(&engine.src);
        let (ship, _, _) = Engine::place(&mut sol, &attacker, engine.all5()).expect("places");
        let fit_id = sol.get_ship(&ship).expect("ship").get_fit_id();
        for time in [30.0, 61.0, 125.0] {
            let options = StatTimeOptions::Sim(rc::stats::StatTimeOptionsSim {
                time: Some(rc::stats::StatTime::from_f64_checked(time).expect("valid time")),
                ..Default::default()
            });
            let sim = sol.get_fit(&fit_id).expect("fit").stats().get_stat_dmg(
                StatDmgItemKinds::default(),
                options,
                StatCritOptions::default(),
            );
            let theirs = dmg(&sim.dps).iter().sum::<f64>() * time;
            let ours: f64 = snap
                .weapons
                .iter()
                .flat_map(|w| w.cycle.cycles(time))
                .filter(|c| c.0 <= time)
                .map(|c| c.2.iter().sum::<f64>())
                .sum();
            assert!(
                (ours - theirs).abs() <= theirs * 0.01,
                "{time} s: graph {ours}, refine {theirs}"
            );
        }
    }

    #[test]
    fn snapshot_reads_weapons_ewar_and_cycles() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let snap = engine.graph_snapshot(&fit(&catalog, RUPTURE), engine.all5(), &catalog);
        assert_eq!(snap.weapons.len(), 4);
        let gun = &snap.weapons[0];
        assert!(matches!(gun.kind, WeaponKind::Turret(g) if g.optimal > 0.0 && g.falloff > 0.0));
        assert!(gun.cycle.shots.is_some(), "autocannons reload");
        assert!(gun.cycle.duration > 1.0 && gun.cycle.duration < 10.0);
        assert!(
            snap.ewar
                .iter()
                .any(|e| e.kind == EwarKind::Web && e.strength < -50.0)
        );
        assert!(snap.ship.lock_range > 0.0 && snap.ship.radius > 0.0);
        assert!(snap.ship.subwarp_speed > 0.0);
        let total: f64 = snap.weapons.iter().map(|w| w.dps.iter().sum::<f64>()).sum();
        assert!(
            (total - snap.ship.dps).abs() < 1e-6,
            "{total} vs {}",
            snap.ship.dps
        );
    }
}
