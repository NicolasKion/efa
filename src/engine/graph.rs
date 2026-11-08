//! Reading a fit out of refine for the graphs: per-weapon damage, cycle timing, the attributes
//! application math needs, ewar, remote repairs and capacitor users.
//!
//! Ship-wide numbers come from [`Engine::compute`], so they match the stats pane exactly;
//! everything per item comes from one more solar system built here.

use std::collections::HashSet;

use rc::{
    Count, FitId, ItemCommon, ItemId, ItemTypeId, SolOwned, Spool,
    stats::{
        ItemStatsCommon, StatCritOptions, StatDmg, StatItemChargeOptions, StatTimeOptions,
        StatTimeOptionsBurst,
    },
};

use super::{BuildOptions, CalcOptions, Damage, Engine, Pilot, build::Placed, read};
use crate::{
    damage_pattern,
    data::{Catalog, Hardpoint, Slot},
    dogma::{attr, effect, group},
    fit::{Fit, ModuleState},
    graphs::{
        ammo::{self, AmmoOption, AmmoQuality, WeaponGroup},
        math::{self, Gun},
        snapshot::{
            CapUser, Cycle, Dmg, Ewar, EwarKind, FitSnapshot, Layers, RemoteRep, Ship, Weapon,
            WeaponKind,
        },
    },
};

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
    (
        shots,
        read::attr_or_zero(sol, id, attr::RELOAD_TIME) / 1000.0,
    )
}

fn module_weapon(
    sol: &mut SolOwned,
    placed: &PlacedModule,
    hardpoint: Hardpoint,
    group: i32,
    effects: &HashSet<i32>,
    charge: Option<&Charge>,
    ship_radius: f64,
) -> Option<Weapon> {
    let id = placed.id;
    let burst = item_dmg(sol, id, StatTimeOptions::default())?;
    let (dps, volley) = (
        Damage::from(&burst.dps).as_array(),
        Damage::from(&burst.volley).as_array(),
    );
    if dps.iter().sum::<f64>() <= 0.0 {
        return breacher_weapon(sol, id, &burst, charge, ship_radius);
    }
    let kind = if effects.contains(&effect::VORTON) {
        WeaponKind::Vorton {
            range: read::attr_or_zero(sol, id, attr::MAX_RANGE),
            er: read::attr_or_zero(sol, id, attr::AOE_CLOUD_SIZE),
            ev: read::attr_or_zero(sol, id, attr::AOE_VELOCITY),
            drf: read::attr_or_zero(sol, id, attr::AOE_DAMAGE_REDUCTION_FACTOR),
        }
    } else if hardpoint == Hardpoint::Turret {
        WeaponKind::Turret(Gun {
            optimal: max_range(sol, id),
            falloff: falloff(sol, id),
            tracking: read::attr_or_zero(sol, id, attr::TRACKING_SPEED),
            optimal_sig: read::attr_or_zero(sol, id, attr::OPTIMAL_SIG_RADIUS),
        })
    } else if hardpoint == Hardpoint::Launcher || placed.type_id == CIVILIAN_LAUNCHER {
        let charge = charge?;
        let c = charge.id;
        let fof = charge.effects.contains(&effect::FOF_MISSILE);
        let range = math::missile_range(
            read::attr_or_zero(sol, c, attr::MAX_VELOCITY),
            read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY),
            read::attr_or_zero(sol, c, attr::MASS),
            read::attr_or_zero(sol, c, attr::AGILITY),
            ship_radius,
            fof.then(|| read::attr_or_zero(sol, c, attr::MAX_FOF_RANGE)),
        );
        WeaponKind::Launcher {
            er: read::attr_or_zero(sol, c, attr::AOE_CLOUD_SIZE),
            ev: read::attr_or_zero(sol, c, attr::AOE_VELOCITY),
            drf: read::attr_or_zero(sol, c, attr::AOE_DAMAGE_REDUCTION_FACTOR),
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
        let range = read::attr_or_zero(sol, c, attr::MAX_VELOCITY)
            * read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY)
            / 1000.0;
        WeaponKind::Bomb {
            range,
            blast_radius: read::attr_or_zero(sol, c, attr::EXPLOSION_RANGE),
            er: read::attr_or_zero(sol, c, attr::AOE_CLOUD_SIZE),
        }
    } else if group == group::GUIDED_BOMB_LAUNCHER {
        let c = charge?.id;
        let range = read::attr_or_zero(sol, c, attr::MAX_VELOCITY)
            * read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY)
            / 1000.0;
        WeaponKind::GuidedBomb {
            range,
            er: read::attr_or_zero(sol, c, attr::AOE_CLOUD_SIZE),
        }
    } else if matches!(group, group::SUPER_WEAPON | group::STRUCTURE_DOOMSDAY) {
        WeaponKind::Doomsday {
            range: max_range(sol, id),
            damage_sig: read::attr_or_zero(sol, id, attr::SIGNATURE_RADIUS),
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
            .map(|n| {
                item_dmg(sol, id, spool_at(n))
                    .map_or(volley, |d| Damage::from(&d.volley).as_array())
            })
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

/// A breacher pod launcher: its damage is a share of the target's HP, so the weapon carries
/// the cap and the share, and application picks the smaller for the target at hand.
fn breacher_weapon(
    sol: &mut SolOwned,
    id: ItemId,
    burst: &StatDmg,
    charge: Option<&Charge>,
    ship_radius: f64,
) -> Option<Weapon> {
    let pair = |e: &rc::stats::StatDmgEntry| {
        [
            e.breacher.absolute_max.into_f64(),
            e.breacher.relative_max.into_f64(),
            0.0,
            0.0,
        ]
    };
    let (dps, volley) = (pair(&burst.dps), pair(&burst.volley));
    if dps[0] <= 0.0 && dps[1] <= 0.0 {
        return None;
    }
    let c = charge?.id;
    let range = math::missile_range(
        read::attr_or_zero(sol, c, attr::MAX_VELOCITY),
        read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY),
        read::attr_or_zero(sol, c, attr::MASS),
        read::attr_or_zero(sol, c, attr::AGILITY),
        ship_radius,
        None,
    );
    let duration = read::first_attr(sol, id, &[attr::RATE_OF_FIRE, attr::DURATION]) / 1000.0;
    let (shots, reload) = reload_of(sol, id);
    Some(Weapon {
        kind: WeaponKind::Breacher { range },
        dps,
        volley,
        cycle: Cycle {
            duration: if duration > 0.0 { duration } else { 1.0 },
            shots,
            reload,
            // pyfa has the pod start eating a second after launch.
            amounts: vec![dps],
            delay: 1.0,
        },
    })
}

/// Every damaging ability of a launched squadron as a weapon of its own, read from refine by
/// switching the squadron's other abilities off in turn.
fn fighter_weapons(sol: &mut SolOwned, id: ItemId) -> Vec<Weapon> {
    let states = abilities(sol, id);
    let speed = read::attr_or_zero(sol, id, attr::MAX_VELOCITY);
    let radius = read::attr_or_zero(sol, id, attr::RADIUS);
    let mut out = Vec::new();
    for &(ability_id, on) in &states {
        let attack = ability::ATTACK.contains(&ability_id);
        let missiles = ability::MISSILES.contains(&ability_id);
        let plain = ability_id == ability::BOMB || ability_id == ability::KAMIKAZE;
        if !on || !(attack || missiles || plain) {
            continue;
        }
        for &(other, other_on) in &states {
            set_ability(sol, id, other, other == ability_id && other_on);
        }
        let burst = item_dmg(sol, id, StatTimeOptions::default());
        for &(other, other_on) in &states {
            set_ability(sol, id, other, other_on);
        }
        let Some(burst) = burst else { continue };
        let (dps, volley) = (
            Damage::from(&burst.dps).as_array(),
            Damage::from(&burst.volley).as_array(),
        );
        let Some(duration) = cycle_of(&volley, &dps) else {
            continue;
        };
        let kind = if attack {
            WeaponKind::Fighter {
                optimal: read::attr_or_zero(sol, id, attr::FTR_ATTACK_OPTIMAL),
                falloff: read::attr_or_zero(sol, id, attr::FTR_ATTACK_FALLOFF),
                er: read::attr_or_zero(sol, id, attr::FTR_ATTACK_ER),
                ev: read::attr_or_zero(sol, id, attr::FTR_ATTACK_EV),
                drf: read::reduction_exponent(
                    read::attr_or_zero(sol, id, attr::FTR_ATTACK_DRF),
                    read::attr_or_zero(sol, id, attr::FTR_ATTACK_DRS),
                ),
                speed,
                radius,
                plain: false,
            }
        } else if missiles {
            WeaponKind::Fighter {
                optimal: read::attr_or_zero(sol, id, attr::FTR_MISSILES_RANGE),
                falloff: 0.0,
                er: read::attr_or_zero(sol, id, attr::FTR_MISSILES_ER),
                ev: read::attr_or_zero(sol, id, attr::FTR_MISSILES_EV),
                drf: read::reduction_exponent(
                    read::attr_or_zero(sol, id, attr::FTR_MISSILES_DRF),
                    read::attr_or_zero(sol, id, attr::FTR_MISSILES_DRS),
                ),
                speed,
                radius,
                plain: false,
            }
        } else {
            WeaponKind::Fighter {
                optimal: read::attr_or_zero(sol, id, attr::FTR_KAMIKAZE_RANGE),
                falloff: 0.0,
                er: 0.0,
                ev: 0.0,
                drf: 0.0,
                speed,
                radius,
                plain: true,
            }
        };
        out.push(Weapon {
            kind,
            dps,
            volley,
            cycle: Cycle {
                duration,
                // pyfa's rearm model for fighter charges is left out of the timelines.
                shots: None,
                reload: 0.0,
                amounts: vec![volley],
                delay: 0.0,
            },
        });
    }
    out
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

fn set_ability(sol: &mut SolOwned, id: ItemId, ability: i32, on: bool) {
    if let Ok(mut fighter) = sol.get_fighter(&id)
        && let Ok(mut a) = fighter.get_ability(&rc::AbilityId::from_i32(ability))
        && a.get_state() != on
    {
        a.set_state(on);
    }
}

/// Webs, neutralizers and ECM a launched squadron carries, pyfa's fighter ewar.
fn fighter_ewar(sol: &mut SolOwned, id: ItemId) -> Vec<Ewar> {
    let count = sol
        .get_fighter(&id)
        .ok()
        .and_then(|f| f.get_count())
        .map_or(1.0, |c| f64::from(c.current.into_u32()));
    let mut out = Vec::new();
    for (ability_id, on) in abilities(sol, id) {
        if !on {
            continue;
        }
        let (kind, strength, optimal, falloff) = match ability_id {
            ability::WEB => (
                EwarKind::Web,
                read::attr_or_zero(sol, id, attr::FTR_WEB_PENALTY) * count,
                read::attr_or_zero(sol, id, attr::FTR_WEB_OPTIMAL),
                read::attr_or_zero(sol, id, attr::FTR_WEB_FALLOFF),
            ),
            ability::NEUT => (EwarKind::Neut, item_nps(sol, id), f64::INFINITY, 0.0),
            ability::ECM => (
                EwarKind::Ecm,
                attr::FTR_ECM_STRENGTH
                    .iter()
                    .map(|&a| read::attr_or_zero(sol, id, a))
                    .fold(0.0, f64::max)
                    * count,
                f64::INFINITY,
                0.0,
            ),
            _ => continue,
        };
        out.push(Ewar {
            kind,
            strength,
            strength2: 0.0,
            optimal,
            falloff,
            needs_lock: true,
            needs_dcr: false,
            mobile: (kind == EwarKind::Web).then(|| {
                (
                    read::attr_or_zero(sol, id, attr::MAX_VELOCITY),
                    read::attr_or_zero(sol, id, attr::RADIUS),
                )
            }),
        });
    }
    out
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
        Damage::from(&burst.dps).as_array().map(|v| v * count),
        Damage::from(&burst.volley).as_array().map(|v| v * count),
    );
    let duration = cycle_of(&volley, &dps)?;
    Some(Weapon {
        kind: WeaponKind::Drone {
            gun: Gun {
                optimal: read::first_attr(sol, id, &[attr::MAX_RANGE]),
                falloff: read::first_attr(sol, id, &[attr::FALLOFF, attr::FALLOFF_EFFECTIVENESS]),
                tracking: read::attr_or_zero(sol, id, attr::TRACKING_SPEED),
                optimal_sig: read::attr_or_zero(sol, id, attr::OPTIMAL_SIG_RADIUS),
            },
            speed: read::attr_or_zero(sol, id, attr::MAX_VELOCITY),
            radius: read::attr_or_zero(sol, id, attr::RADIUS),
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
    let dd_range = (optimal + read::attr_or_zero(sol, id, attr::DOOMSDAY_AOE_RANGE)).max(0.0);
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
    let nos_as_neut = effects.contains(&effect::NOSFERATU)
        && read::attr_or_zero(sol, id, attr::NOS_OVERRIDE) != 0.0;
    if has(&effect::NEUTRALIZER) || nos_as_neut {
        push(EwarKind::Neut, item_nps(sol, id), 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_NEUT) {
        push(EwarKind::Neut, item_nps(sol, id), 0.0, true);
    }
    let web = read::attr_or_zero(sol, id, attr::SPEED_FACTOR);
    if has(&effect::WEBIFIER) {
        push(EwarKind::Web, web, 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_WEB) {
        push(EwarKind::Web, web, 0.0, true);
    }
    let ecm = attr::ECM_STRENGTH
        .iter()
        .map(|&a| read::attr_or_zero(sol, id, a))
        .fold(0.0, f64::max);
    if has(&effect::ECM) {
        push(EwarKind::Ecm, ecm, 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_ECM) {
        push(EwarKind::Ecm, ecm, 0.0, true);
    }
    let damp = read::attr_or_zero(sol, id, attr::MAX_TARGET_RANGE_BONUS);
    let damp_res = read::attr_or_zero(sol, id, attr::SCAN_RESOLUTION_BONUS);
    if has(&effect::SENSOR_DAMPENER) {
        push(EwarKind::Damp, damp, damp_res, false);
    }
    if effects.contains(&effect::DOOMSDAY_DAMP) {
        push(EwarKind::Damp, damp, damp_res, true);
    }
    let td = read::attr_or_zero(sol, id, attr::MAX_RANGE_BONUS);
    if has(&effect::TRACKING_DISRUPTOR) {
        push(EwarKind::TrackingDisruptor, td, 0.0, false);
    }
    let (gd_velocity, gd_flight) = (
        read::attr_or_zero(sol, id, attr::MISSILE_VELOCITY_BONUS),
        read::attr_or_zero(sol, id, attr::EXPLOSION_DELAY_BONUS),
    );
    if has(&effect::GUIDANCE_DISRUPTOR) {
        push(EwarKind::GuidanceDisruptor, gd_velocity, gd_flight, false);
    }
    if effects.contains(&effect::DOOMSDAY_TRACK) {
        push(EwarKind::TrackingDisruptor, td, 0.0, true);
        push(EwarKind::GuidanceDisruptor, gd_velocity, gd_flight, true);
    }
    let tp = read::attr_or_zero(sol, id, attr::SIGNATURE_RADIUS_BONUS);
    if has(&effect::TARGET_PAINTER) {
        push(EwarKind::TargetPainter, tp, 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_PAINT) {
        push(EwarKind::TargetPainter, tp, 0.0, true);
    }
    out
}

fn drone_ewar(sol: &mut SolOwned, stack: &PlacedDrones, effects: &HashSet<i32>) -> Vec<Ewar> {
    let id = stack.id;
    let speed = read::attr_or_zero(sol, id, attr::MAX_VELOCITY);
    let radius = read::attr_or_zero(sol, id, attr::RADIUS);
    let optimal = read::first_attr(sol, id, &[attr::MAX_RANGE, attr::ECM_RANGE_OPTIMAL]);
    let falloff = read::first_attr(sol, id, &[attr::FALLOFF, attr::FALLOFF_EFFECTIVENESS]);
    let mut out = Vec::new();
    let kinds = [
        (effect::ENTITY_NEUTRALIZER, EwarKind::Neut),
        (effect::ENTITY_WEBIFIER, EwarKind::Web),
        (effect::ENTITY_ECM, EwarKind::Ecm),
        (effect::ENTITY_SENSOR_DAMPENER, EwarKind::Damp),
        (effect::ENTITY_WEAPON_DISRUPTOR, EwarKind::TrackingDisruptor),
        (effect::ENTITY_TARGET_PAINTER, EwarKind::TargetPainter),
    ];
    for (effect_id, kind) in kinds {
        if !effects.contains(&effect_id) {
            continue;
        }
        let strength = match kind {
            EwarKind::Neut => item_nps(sol, id),
            EwarKind::Web => read::attr_or_zero(sol, id, attr::SPEED_FACTOR),
            EwarKind::Ecm => attr::ECM_STRENGTH
                .iter()
                .map(|&a| read::attr_or_zero(sol, id, a))
                .fold(0.0, f64::max),
            EwarKind::Damp => read::attr_or_zero(sol, id, attr::MAX_TARGET_RANGE_BONUS),
            EwarKind::TrackingDisruptor => read::attr_or_zero(sol, id, attr::MAX_RANGE_BONUS),
            EwarKind::GuidanceDisruptor | EwarKind::TargetPainter => {
                read::attr_or_zero(sol, id, attr::SIGNATURE_RADIUS_BONUS)
            }
        };
        let strength2 = if kind == EwarKind::Damp {
            read::attr_or_zero(sol, id, attr::SCAN_RESOLUTION_BONUS)
        } else {
            0.0
        };
        // pyfa counts each drone of the stack on its own, so stacking penalties apply.
        for _ in 0..stack.active {
            out.push(Ewar {
                kind,
                strength,
                strength2,
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
    let shield = matches!(
        group,
        group::REMOTE_SHIELD_BOOSTER | group::ANCILLARY_REMOTE_SHIELD_BOOSTER
    );
    let armor_or_hull = matches!(
        group,
        group::REMOTE_ARMOR_REPAIRER
            | group::REMOTE_HULL_REPAIRER
            | group::ANCILLARY_REMOTE_ARMOR_REPAIRER
            | group::MUTADAPTIVE_REMOTE_ARMOR_REPAIRER
    );
    if !shield && !armor_or_hull {
        return None;
    }
    let duration = read::attr_or_zero(sol, id, attr::DURATION) / 1000.0;
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
    let ancillary = matches!(
        group,
        group::ANCILLARY_REMOTE_SHIELD_BOOSTER | group::ANCILLARY_REMOTE_ARMOR_REPAIRER
    );
    let no_reload = (ancillary && shots.is_some()).then(|| {
        let charged = amounts[0];
        // Out of paste, an ancillary armor repairer keeps going at its plain amount.
        let plain = if group == group::ANCILLARY_REMOTE_ARMOR_REPAIRER {
            let mult = read::attr_or_zero(sol, id, attr::CHARGED_ARMOR_DAMAGE_MULTIPLIER);
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
    let duration = read::first_attr(sol, id, &[attr::DURATION, attr::RATE_OF_FIRE]) / 1000.0;
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

#[cfg(test)]
mod tests {
    //! The graphs' own application math checked against refine's applied damage, on real data.
    //! Skipped when no dump is present.

    use std::{collections::HashMap, path::Path};

    use rc::{Coordinates, Value, stats::StatDmgItemKinds};

    use super::*;
    use crate::engine::build::add_fit;
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
        for m in fit.modules_mut() {
            m.state = ModuleState::Active;
        }
        fit
    }

    /// Weapon DPS refine applies to a standing target `distance` meters away, surface to
    /// surface, before resists.
    fn refine_applied(engine: &Engine, attacker: &Fit, target: &Fit, distance: f64) -> f64 {
        let mut sol = engine.sol();
        let placed = add_fit(&mut sol, attacker, engine.all5()).expect("attacker places");
        let Placement {
            ship: atk_ship,
            modules,
            ..
        } = placement(attacker, &placed);
        let tgt_ship = add_fit(&mut sol, target, engine.all5())
            .expect("target places")
            .ship;
        let c2c = distance
            + read::attr_or_zero(&mut sol, atk_ship, attr::RADIUS)
            + read::attr_or_zero(&mut sol, tgt_ship, attr::RADIUS);
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
        let src = engine.graph_snapshot(attacker, &[], engine.all5(), catalog);
        let tgt = engine.graph_snapshot(target, &[], engine.all5(), catalog);
        let target = TargetData::profile(
            tgt.ship.max_velocity,
            tgt.ship.sig_radius,
            tgt.ship.radius,
            [0.0; 4],
            None,
        );
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
        let snap = engine.graph_snapshot(&attacker, &[], engine.all5(), &catalog);
        let mut built = engine
            .build(&attacker, engine.all5(), BuildOptions::default())
            .expect("places");
        let (sol, fit_id) = (&mut built.sol, built.fit.fit_id);
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
            let theirs = Damage::from(&sim.dps).as_array().iter().sum::<f64>() * time;
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
    fn fighters_become_weapons() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let carrier = fit(&catalog, "[Thanatos, Fighters]\n\nTemplar II x9\n");
        assert_eq!(carrier.fighters.len(), 1, "the squadron parsed");
        let snap = engine.graph_snapshot(&carrier, &[], engine.all5(), &catalog);
        let fighter = snap
            .weapons
            .iter()
            .find(|w| matches!(w.kind, WeaponKind::Fighter { plain: false, .. }))
            .expect("the attack ability is a weapon");
        assert!(fighter.dps.iter().sum::<f64>() > 0.0);
        assert!(matches!(fighter.kind, WeaponKind::Fighter { er, .. } if er > 0.0));
    }

    #[test]
    fn breacher_pods_cap_by_target_hp() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let kestrel = fit(
            &catalog,
            "[Kestrel, Breacher]\n\nSmall Breacher Pod Launcher, SCARAB Breacher Pod S\n",
        );
        let snap = engine.graph_snapshot(&kestrel, &[], engine.all5(), &catalog);
        let pod = snap
            .weapons
            .iter()
            .find(|w| matches!(w.kind, WeaponKind::Breacher { .. }))
            .expect("the launcher is a breacher weapon");
        let (cap, share) = (pod.dps[0], pod.dps[1]);
        assert!(cap > 0.0 && share > 0.0, "{:?}", pod.dps);
        // Against a small target the share of its HP is what gets through.
        let small_hp = cap / share / 10.0;
        let graph = Graph::DamageStats;
        let req = Request {
            graph,
            x: graph.x_defs(true)[0],
            y: graph.y_defs(true, true)[0],
            range: (0.0, 1.0),
            misc: HashMap::new(),
            checkboxes: HashMap::new(),
            options: Options::default(),
        };
        let tgt = TargetData::profile(0.0, 40.0, 0.0, [0.5; 4], Some(small_hp));
        let series = plot::plot(&req, &snap, Some(&tgt)).expect("a line");
        let want = share * small_hp;
        assert!(
            (series.ys[0] - want).abs() < want * 1e-6,
            "{} vs {want}",
            series.ys[0]
        );
    }

    #[test]
    fn scrams_turn_off_microwarpdrives() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let tackle = fit(&catalog, "[Rifter, Tackle]\n\nWarp Scrambler II\n");
        let runner = fit(&catalog, "[Rifter, Runner]\n\n5MN Microwarpdrive II\n");
        let tackle = engine.graph_snapshot(&tackle, &[], engine.all5(), &catalog);
        let runner = engine.graph_snapshot(&runner, &[], engine.all5(), &catalog);
        assert!(tackle.scram_range.is_some_and(|r| r > 5_000.0));
        let (speed, sig) = runner.ship.scrammed.expect("the MWD can be scrammed");
        assert!(speed < runner.ship.max_velocity / 2.0, "{speed}");
        assert!(sig < runner.ship.sig_radius);
    }

    #[test]
    fn ammo_groups_load_every_charge() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let groups = engine.graph_ammo(
            &fit(&catalog, RUPTURE),
            &[],
            engine.all5(),
            &catalog,
            AmmoQuality::Navy,
        );
        assert_eq!(groups.len(), 1, "one autocannon group");
        let names: Vec<&str> = groups[0].options.iter().map(|o| o.name.as_str()).collect();
        assert!(
            names.contains(&"Barrage M") && names.contains(&"EMP M"),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("Arch Angel")),
            "{names:?}"
        );
        // Every option carries all four guns, and a long range charge reaches further.
        assert!(groups[0].options.iter().all(|o| o.weapons.len() == 4));
        let falloff = |name: &str| {
            groups[0]
                .options
                .iter()
                .find(|o| o.name == name)
                .and_then(|o| match o.weapons[0].kind {
                    WeaponKind::Turret(g) => Some(g.falloff),
                    _ => None,
                })
                .expect("a turret")
        };
        assert!(falloff("Barrage M") > falloff("EMP M"));
    }

    #[test]
    fn snapshot_reads_weapons_ewar_and_cycles() {
        let Some((catalog, engine)) = setup() else {
            return;
        };
        let snap = engine.graph_snapshot(&fit(&catalog, RUPTURE), &[], engine.all5(), &catalog);
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
