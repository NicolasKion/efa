//! What the fitting view's Miscellanea and Heat columns read off modules (pyfa's
//! gui/builtinViewColumns/misc.py and heat.py), gathered after the main recalculation.

use std::collections::{HashMap, HashSet};

use rc::{
    ItemCommon, ItemId, SolOwned,
    stats::{
        ItemStatsCommon, StatCritOptions, StatItemChargeOptions, StatMiningResourceKind,
        StatTimeOptions,
    },
};

use super::{Damage, FitItem, Stats, pane, read};
use crate::{
    data::Slot,
    dogma::attr,
    fit::{Fit, ModuleState},
};

/// Attributes the Miscellanea column reads off a module or subsystem.
pub(super) const MODULE_ATTRS: [i32; 54] = [
    attr::TRACKING_SPEED,
    attr::OPTIMAL_SIG_RADIUS,
    attr::NEUT_AMOUNT,
    attr::POWER_TRANSFER_AMOUNT,
    attr::ACCESS_DIFFICULTY_BONUS,
    attr::VIRUS_STRENGTH,
    attr::VIRUS_COHERENCE,
    attr::DURATION,
    attr::RATE_OF_FIRE,
    attr::WARP_SCRAMBLE_STRENGTH,
    attr::SPEED_FACTOR,
    attr::SIGNATURE_RADIUS_BONUS,
    attr::MAX_TARGET_RANGE_BONUS,
    attr::SCAN_RESOLUTION_BONUS,
    attr::MAX_RANGE_BONUS,
    attr::FALLOFF_BONUS,
    attr::TRACKING_SPEED_BONUS,
    attr::AOE_VELOCITY_BONUS,
    attr::AOE_CLOUD_SIZE_BONUS,
    attr::EXPLOSION_DELAY_BONUS,
    attr::MISSILE_VELOCITY_BONUS,
    attr::DAMAGE_MULTIPLIER,
    attr::SPEED_MULTIPLIER,
    attr::MISSILE_DAMAGE_MULTIPLIER_BONUS,
    attr::DRONE_DAMAGE_BONUS,
    attr::SCAN_GRAVIMETRIC_BONUS,
    attr::SCAN_LADAR_BONUS,
    attr::SCAN_MAGNETOMETRIC_BONUS,
    attr::SCAN_RADAR_BONUS,
    attr::SCAN_GRAVIMETRIC_PERCENT,
    attr::SCAN_LADAR_PERCENT,
    attr::SCAN_MAGNETOMETRIC_PERCENT,
    attr::SCAN_RADAR_PERCENT,
    attr::CLOAKING_TARGETING_DELAY,
    attr::COMMAND_BONUS,
    attr::COMMAND_BONUS_HIDDEN,
    attr::CONSUMPTION_QUANTITY,
    attr::CONSUMPTION_TYPE,
    attr::RELOAD_TIME,
    attr::ARMOR_DAMAGE_AMOUNT,
    attr::SHIELD_BONUS,
    attr::ARMOR_EM_RESONANCE,
    attr::ARMOR_EXPLOSIVE_RESONANCE,
    attr::ARMOR_KINETIC_RESONANCE,
    attr::ARMOR_THERMAL_RESONANCE,
    attr::WARFARE_BUFF_VALUES[0],
    attr::WARFARE_BUFF_VALUES[1],
    attr::WARFARE_BUFF_VALUES[2],
    attr::WARFARE_BUFF_VALUES[3],
    attr::HI_SLOT_MODIFIER,
    attr::MED_SLOT_MODIFIER,
    attr::LOW_SLOT_MODIFIER,
    // Vorton projectors carry their explosion on the module.
    attr::AOE_CLOUD_SIZE,
    attr::AOE_VELOCITY,
];

/// Attributes the Miscellanea column reads off a module's charge.
pub(super) const CHARGE_ATTRS: [i32; 12] = [
    attr::AOE_CLOUD_SIZE,
    attr::AOE_VELOCITY,
    attr::CAPACITOR_BONUS,
    attr::BASE_SENSOR_STRENGTH,
    attr::BASE_SCAN_RANGE,
    attr::DOT_DURATION,
    attr::DOT_MAX_DAMAGE_PER_TICK,
    attr::DOT_MAX_HP_PERCENTAGE_PER_TICK,
    attr::WARFARE_BUFF_IDS[0],
    attr::WARFARE_BUFF_IDS[1],
    attr::WARFARE_BUFF_IDS[2],
    attr::WARFARE_BUFF_IDS[3],
];

/// What pyfa's Miscellanea column reads off a module or subsystem, after all bonuses. Most of it
/// is plain attributes; the rest is what refine works out as if the module were running.
#[derive(Debug, Clone, Default)]
pub struct ModuleMisc {
    /// The attributes in [`MODULE_ATTRS`] the item has.
    pub attrs: HashMap<i32, f64>,
    /// The attributes in [`CHARGE_ATTRS`] its charge has.
    pub charge_attrs: HashMap<i32, f64>,
    /// Dogma effects the item carries.
    pub effects: HashSet<i32>,
    /// Remote repair per second: capacitor, shield, armor, hull.
    pub reps: [f64; 4],
    /// Armor repaired per second at no spool and at full spool, for repairers that spool up.
    pub reps_spool: Option<(f64, f64)>,
    /// Mining yield and drain, m³ per second.
    pub mining: (f64, f64),
    /// Seconds to spool up to the current setting, and to full spool.
    pub spool_time: Option<(f64, f64)>,
    /// Cycles before the charges run out, for modules that use them up.
    pub charged_cycles: Option<u32>,
    /// Raw damage of one volley.
    pub volley: f64,
}

impl ModuleMisc {
    /// An attribute after bonuses, if the item has it.
    pub fn attr(&self, id: i32) -> Option<f64> {
        self.attrs.get(&id).copied()
    }

    pub fn charge_attr(&self, id: i32) -> Option<f64> {
        self.charge_attrs.get(&id).copied()
    }

    pub fn has_effect(&self, id: i32) -> bool {
        self.effects.contains(&id)
    }
}

/// Everything [`ModuleMisc`] holds for one module or subsystem. pyfa shows what a module does
/// whatever its state, so the numbers come from the item switched on.
pub(super) fn module_misc(sol: &mut SolOwned, id: ItemId) -> ModuleMisc {
    let mut misc = ModuleMisc {
        attrs: read::attrs(sol, id, &MODULE_ATTRS),
        ..ModuleMisc::default()
    };
    misc.effects = read::effects(sol, id);
    let cycle = misc
        .attr(attr::DURATION)
        .filter(|&d| d > 0.0)
        .or_else(|| misc.attr(attr::RATE_OF_FIRE).filter(|&d| d > 0.0))
        .map(|ms| ms / 1000.0);
    if let Ok(mut module) = sol.get_module(&id) {
        if let Some(charge) = module.get_charge() {
            let charge = charge.get_item_id();
            misc.charge_attrs = read::attrs(sol, charge, &CHARGE_ATTRS);
        }
        if let Ok(mut module) = sol.get_module(&id) {
            misc.charged_cycles = module.get_charged_cycle_count().map(rc::Count::into_u32);
            misc.spool_time = module
                .get_spool_cycle_count()
                .zip(cycle)
                .map(|(info, cycle)| {
                    (
                        f64::from(info.current.into_u32()) * cycle,
                        f64::from(info.max.into_u32()) * cycle,
                    )
                });
        }
    }
    let spools = misc.spool_time.is_some();
    let mut stats = sol.stats();
    let Ok(mut item) = stats.get_item(&id) else {
        return misc;
    };
    let mut on = item.switch_states();
    if let Ok(reps) = on.get_stat_outgoing_rps(StatTimeOptions::default()) {
        misc.reps[1] = reps.shield.into_f64();
        misc.reps[2] = reps.armor.into_f64();
        misc.reps[3] = reps.hull.into_f64();
    }
    if let Ok(cap) = on.get_stat_outgoing_cps(StatTimeOptions::default()) {
        misc.reps[0] = cap.into_f64();
    }
    if spools {
        let armor = |on: &mut rc::stats::SwitchedItemStats<'_>, spool: f64| {
            on.get_stat_outgoing_rps(pane::burst_at(spool))
                .map_or(0.0, |r| r.armor.into_f64())
        };
        misc.reps_spool = Some((armor(&mut on, 0.0), armor(&mut on, 1.0)));
    }
    if let Ok(mining) = on.get_stat_mps(StatTimeOptions::default(), StatMiningResourceKind::Regular)
    {
        let rate = read::mining_rate(&mining);
        misc.mining = (rate.yield_, rate.drain);
    }
    if let Ok(dmg) = on.get_stat_dmg(
        StatTimeOptions::default(),
        StatCritOptions::default(),
        StatItemChargeOptions::Include,
    ) {
        misc.volley = Damage::from(&dmg.volley).as_array().into_iter().sum();
    }
    misc
}

/// One rack's overheating, the way pyfa's heat column models it: modules heat the rack, and
/// heat spreads to neighbours less the further away they are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RackHeat {
    /// How much less heat each step of distance passes on.
    pub attenuation: f64,
    /// Heat absorption of the overheated modules together.
    pub absorption: f64,
    /// Positions of the overheated modules.
    pub overheated: Vec<usize>,
}

/// The ship's side of overheating.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ShipHeat {
    /// heatGenerationMultiplier.
    pub generation: f64,
    /// Share of the ship's slots filled with modules that are not offline.
    pub slot_factor: f64,
}

/// Heat attenuation pyfa falls back to where a ship has none of its own.
const DEFAULT_ATTENUATION: f64 = 0.25;

/// How long pyfa's simulation of overheating runs, in seconds.
const HEAT_SIM_TIME: f64 = 600.0;

impl ShipHeat {
    /// Chance the module at `pos` takes heat damage at second `t` (pyfa's
    /// calcDamageProbability).
    fn damage_chance(&self, rack: &RackHeat, pos: usize, t: f64) -> f64 {
        let rack_heat = 1.0 - (-t * self.generation * rack.absorption).exp();
        let others: f64 = rack
            .overheated
            .iter()
            .filter(|&&other| other != pos)
            .map(|&other| {
                let distance = i32::try_from(other.abs_diff(pos)).unwrap_or(i32::MAX);
                1.0 - rack.attenuation.powi(distance) * self.slot_factor * rack_heat
            })
            .product();
        // A module heats itself only when it is overheated; the others only catch their
        // neighbours' heat.
        let own = if rack.overheated.contains(&pos) {
            self.slot_factor * rack_heat
        } else {
            0.0
        };
        1.0 - others * (1.0 - own)
    }

    /// Expected cycles until the module at `pos` burns out (pyfa's calcBurnCycles). `step` is
    /// how long the simulation advances per cycle, `hp` and `heat_damage` the module's.
    pub fn burn_cycles(
        &self,
        rack: &RackHeat,
        pos: usize,
        step: f64,
        hp: f64,
        heat_damage: f64,
    ) -> Option<f64> {
        if step <= 0.0 || heat_damage <= 0.0 {
            return None;
        }
        // pyfa stops once the chance settles at five decimals.
        let rounded = |p: f64| (p * 1e5).round() as i64;
        let mut chances = Vec::new();
        let (mut t, mut last) = (step, 0.0);
        while t < HEAT_SIM_TIME {
            let p = self.damage_chance(rack, pos, t);
            chances.push(p);
            if rounded(p) == rounded(last) {
                break;
            }
            t += step;
            last = p;
        }
        let &final_chance = chances.last()?;
        if final_chance <= 0.0 {
            return None;
        }
        // Chance of having taken k hits so far; burning out takes `n` of them.
        let n = (hp / heat_damage).ceil() as usize;
        if n == 0 {
            return None;
        }
        let mut hits = vec![0.0; n + 1];
        hits[0] = 1.0;
        let mut expected = 0.0;
        for (tick, &p) in chances.iter().enumerate() {
            expected += (tick + 1) as f64 * p * hits[n - 1];
            for k in (1..n).rev() {
                hits[k] = (1.0 - p) * hits[k] + p * hits[k - 1];
            }
            hits[0] *= 1.0 - p;
        }
        let last_tick = chances.len() as f64;
        for (k, &share) in hits.iter().enumerate().take(n) {
            expected += (last_tick + (n - k) as f64 / final_chance) * share;
        }
        Some(expected.floor())
    }
}

/// The ship's heat attributes: generation, and attenuation of the high, mid and low racks.
pub(super) fn ship_heat(sol: &mut SolOwned, ship: ItemId) -> (f64, [f64; 3]) {
    let attrs = read::attrs(
        sol,
        ship,
        &[
            attr::HEAT_GENERATION_MULTIPLIER,
            attr::HEAT_ATTENUATION_HI,
            attr::HEAT_ATTENUATION_MED,
            attr::HEAT_ATTENUATION_LOW,
        ],
    );
    let attenuation = |attr| attrs.get(&attr).copied().unwrap_or(DEFAULT_ATTENUATION);
    (
        attrs
            .get(&attr::HEAT_GENERATION_MULTIPLIER)
            .copied()
            .unwrap_or(0.0),
        [
            attenuation(attr::HEAT_ATTENUATION_HI),
            attenuation(attr::HEAT_ATTENUATION_MED),
            attenuation(attr::HEAT_ATTENUATION_LOW),
        ],
    )
}

/// A module's heat attributes: absorption rate, hit points and damage taken per heat hit.
pub(super) fn module_heat(sol: &mut SolOwned, id: ItemId) -> (f64, f64, f64) {
    let attrs = read::attrs(
        sol,
        id,
        &[
            attr::HEAT_ABSORBTION_RATE_MODIFIER,
            attr::HP,
            attr::HEAT_DAMAGE,
        ],
    );
    let get = |attr| attrs.get(&attr).copied().unwrap_or(0.0);
    (
        get(attr::HEAT_ABSORBTION_RATE_MODIFIER),
        get(attr::HP),
        get(attr::HEAT_DAMAGE),
    )
}

/// pyfa's heat column: how long each overheated module lasts before it burns out.
pub(super) fn burnout(
    sol: &mut SolOwned,
    fit: &Fit,
    ship_id: ItemId,
    module_ids: &[(Slot, usize, ItemId)],
    stats: &mut Stats,
) {
    const RACKS: [Slot; 3] = [Slot::High, Slot::Mid, Slot::Low];
    let overheated = |slot: Slot, i: usize| {
        fit.rack(slot)
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|m| m.state == ModuleState::Overload)
    };
    if !module_ids.iter().any(|&(slot, i, _)| overheated(slot, i)) {
        return;
    }
    let (generation, attenuation) = ship_heat(sol, ship_id);
    // Share of the ship's slots doing something: pyfa counts the empty and offline ones out, and
    // rig slots in.
    let total: u32 = RACKS
        .iter()
        .map(|&slot| stats.slots(slot).total.unwrap_or(0))
        .sum();
    let busy = RACKS
        .iter()
        .flat_map(|&slot| fit.rack(slot).iter().flatten())
        .filter(|m| m.state != ModuleState::Offline)
        .count();
    let all = total + stats.rig.total.unwrap_or(0);
    let ship = ShipHeat {
        generation,
        slot_factor: if all > 0 {
            busy as f64 / f64::from(all)
        } else {
            0.0
        },
    };
    let mut heat = HashMap::new();
    for &(slot, i, id) in module_ids {
        if RACKS.contains(&slot) {
            heat.insert((slot, i), module_heat(sol, id));
        }
    }
    for (n, &slot) in RACKS.iter().enumerate() {
        // The rack's modules by position, so sums come out the same every time.
        let in_rack: Vec<usize> = module_ids
            .iter()
            .filter(|&&(s, ..)| s == slot)
            .map(|&(_, i, _)| i)
            .collect();
        let hot: Vec<usize> = in_rack
            .iter()
            .copied()
            .filter(|&i| overheated(slot, i))
            .collect();
        let rack = RackHeat {
            attenuation: attenuation[n],
            absorption: hot.iter().map(|&i| heat[&(slot, i)].0).sum(),
            overheated: hot,
        };
        // Each overheated module's step and cycle, in seconds.
        let mut timings = Vec::new();
        for &i in &rack.overheated {
            let Some(misc) = stats.module_misc.get(&FitItem::Module(slot, i)) else {
                continue;
            };
            // pyfa steps the simulation by rate of fire first, but times the result by
            // duration first.
            let seconds = |a: i32| misc.attr(a).filter(|&v| v > 0.0).map(|ms| ms / 1000.0);
            let (speed, duration) = (seconds(attr::RATE_OF_FIRE), seconds(attr::DURATION));
            if let (Some(step), Some(cycle)) = (speed.or(duration), duration.or(speed)) {
                timings.push((i, step, cycle));
            }
        }
        for &(i, step, cycle) in &timings {
            let (_, hp, heat_damage) = heat[&(slot, i)];
            if let Some(info) = stats.modules.get_mut(&(slot, i)) {
                info.burnout = ship
                    .burn_cycles(&rack, i, step, hp, heat_damage)
                    .map(|cycles| cycles * cycle);
            }
        }
        // The rest of the rack takes damage from its overheated neighbours: at the pace of the
        // quickest of them, and as much per hit as they take on average.
        let Some(&(_, step, cycle)) = timings.iter().min_by(|a, b| a.1.total_cmp(&b.1)) else {
            continue;
        };
        let damages: Vec<f64> = timings
            .iter()
            .map(|&(i, _, _)| heat[&(slot, i)].2)
            .collect();
        let neighbour_damage = damages.iter().sum::<f64>() / damages.len() as f64;
        let cold = in_rack.iter().copied().filter(|&i| {
            !overheated(slot, i)
                && fit
                    .rack(slot)
                    .get(i)
                    .and_then(Option::as_ref)
                    .is_some_and(|m| m.state != ModuleState::Offline)
        });
        for i in cold {
            let (_, hp, _) = heat[&(slot, i)];
            if hp <= 0.0 {
                continue;
            }
            if let Some(info) = stats.modules.get_mut(&(slot, i)) {
                info.burnout = ship
                    .burn_cycles(&rack, i, step, hp, neighbour_damage)
                    .map(|cycles| cycles * cycle);
                info.burnout_from_neighbours = info.burnout.is_some();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cycle counts pyfa's heat.py gives for the same inputs.
    #[test]
    fn burnout_matches_pyfa() {
        let full = ShipHeat {
            generation: 0.01,
            slot_factor: 1.0,
        };
        let pair = RackHeat {
            attenuation: 0.25,
            absorption: 0.02,
            overheated: vec![0, 1],
        };
        assert_eq!(full.burn_cycles(&pair, 0, 5.0, 40.0, 4.0), Some(130.0));
        let alone = RackHeat {
            absorption: 0.01,
            overheated: vec![0],
            ..pair
        };
        assert_eq!(full.burn_cycles(&alone, 0, 5.0, 40.0, 4.0), Some(231.0));
        let partly = ShipHeat {
            slot_factor: 0.8,
            ..full
        };
        let three = RackHeat {
            attenuation: 0.25,
            absorption: 0.03,
            overheated: vec![0, 1, 3],
        };
        assert_eq!(partly.burn_cycles(&three, 1, 3.5, 40.0, 0.88), Some(357.0));
    }

    /// A module next to an overheated one burns too, more slowly than it, and slower still
    /// further away.
    #[test]
    fn neighbours_burn_from_the_overheated_ones() {
        let ship = ShipHeat {
            generation: 0.01,
            slot_factor: 1.0,
        };
        let rack = RackHeat {
            attenuation: 0.25,
            absorption: 0.01,
            overheated: vec![1],
        };
        let hot = ship.burn_cycles(&rack, 1, 5.0, 40.0, 4.0).expect("burns");
        let next = ship.burn_cycles(&rack, 0, 5.0, 40.0, 4.0).expect("burns");
        let far = ship.burn_cycles(&rack, 3, 5.0, 40.0, 4.0).expect("burns");
        assert!(hot < next && next < far, "{hot} {next} {far}");
    }

    #[test]
    fn no_heat_damage_means_no_burnout() {
        let ship = ShipHeat {
            generation: 0.01,
            slot_factor: 1.0,
        };
        let rack = RackHeat {
            attenuation: 0.25,
            absorption: 0.01,
            overheated: vec![0],
        };
        assert_eq!(ship.burn_cycles(&rack, 0, 5.0, 40.0, 0.0), None);
    }
}
