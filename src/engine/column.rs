//! What the fitting view's Miscellanea and Heat columns read off modules (pyfa's
//! gui/builtinViewColumns/misc.py and heat.py), gathered after the main recalculation.

use std::collections::{HashMap, HashSet};

use rc::{
    AttrId, EveAttrId, ItemCommon, ItemId, SolOwned, Spool, UnitInterval,
    stats::{
        ItemStatsCommon, StatCritOptions, StatItemChargeOptions, StatMiningResourceKind,
        StatTimeOptions, StatTimeOptionsBurst,
    },
};

/// Attributes the Miscellanea column reads, by their dogma ID.
pub mod misc_attr {
    pub const TRACKING_SPEED: i32 = 160;
    pub const OPTIMAL_SIG_RADIUS: i32 = 620;
    pub const NEUT_AMOUNT: i32 = 97;
    pub const NOS_AMOUNT: i32 = 90;
    pub const ACCESS_DIFFICULTY_BONUS: i32 = 902;
    pub const VIRUS_STRENGTH: i32 = 1910;
    pub const VIRUS_COHERENCE: i32 = 1909;
    pub const DURATION: i32 = 73;
    pub const SPEED: i32 = 51;
    pub const WARP_SCRAMBLE_STRENGTH: i32 = 105;
    pub const SPEED_FACTOR: i32 = 20;
    pub const SIG_RADIUS_BONUS: i32 = 554;
    pub const MAX_TARGET_RANGE_BONUS: i32 = 309;
    pub const SCAN_RESOLUTION_BONUS: i32 = 566;
    pub const MAX_RANGE_BONUS: i32 = 351;
    pub const FALLOFF_BONUS: i32 = 349;
    pub const TRACKING_SPEED_BONUS: i32 = 767;
    pub const AOE_VELOCITY_BONUS: i32 = 847;
    pub const AOE_CLOUD_SIZE_BONUS: i32 = 848;
    pub const EXPLOSION_DELAY_BONUS: i32 = 596;
    pub const MISSILE_VELOCITY_BONUS: i32 = 547;
    pub const DAMAGE_MULTIPLIER: i32 = 64;
    pub const SPEED_MULTIPLIER: i32 = 204;
    pub const MISSILE_DAMAGE_MULTIPLIER_BONUS: i32 = 213;
    pub const DRONE_DAMAGE_BONUS: i32 = 1255;
    pub const SCAN_GRAVIMETRIC_BONUS: i32 = 238;
    pub const SCAN_LADAR_BONUS: i32 = 239;
    pub const SCAN_MAGNETOMETRIC_BONUS: i32 = 240;
    pub const SCAN_RADAR_BONUS: i32 = 241;
    pub const SCAN_GRAVIMETRIC_PERCENT: i32 = 1027;
    pub const SCAN_LADAR_PERCENT: i32 = 1028;
    pub const SCAN_MAGNETOMETRIC_PERCENT: i32 = 1029;
    pub const SCAN_RADAR_PERCENT: i32 = 1030;
    pub const CLOAKING_TARGETING_DELAY: i32 = 560;
    pub const COMMAND_BONUS: i32 = 833;
    pub const COMMAND_BONUS_HIDDEN: i32 = 1310;
    pub const CONSUMPTION_QUANTITY: i32 = 714;
    pub const CONSUMPTION_TYPE: i32 = 713;
    pub const RELOAD_TIME: i32 = 1795;
    pub const ARMOR_DAMAGE_AMOUNT: i32 = 84;
    pub const SHIELD_BONUS: i32 = 68;
    pub const ARMOR_EM_RESONANCE: i32 = 267;
    pub const ARMOR_EXPLOSIVE_RESONANCE: i32 = 268;
    pub const ARMOR_KINETIC_RESONANCE: i32 = 269;
    pub const ARMOR_THERMAL_RESONANCE: i32 = 270;
    /// warfareBuff1Value to warfareBuff4Value.
    pub const WARFARE_BUFF_VALUES: [i32; 4] = [2469, 2471, 2473, 2537];
    pub const HI_SLOT_MODIFIER: i32 = 1374;
    pub const MED_SLOT_MODIFIER: i32 = 1375;
    pub const LOW_SLOT_MODIFIER: i32 = 1376;

    pub const MODULE: [i32; 54] = [
        TRACKING_SPEED,
        OPTIMAL_SIG_RADIUS,
        NEUT_AMOUNT,
        NOS_AMOUNT,
        ACCESS_DIFFICULTY_BONUS,
        VIRUS_STRENGTH,
        VIRUS_COHERENCE,
        DURATION,
        SPEED,
        WARP_SCRAMBLE_STRENGTH,
        SPEED_FACTOR,
        SIG_RADIUS_BONUS,
        MAX_TARGET_RANGE_BONUS,
        SCAN_RESOLUTION_BONUS,
        MAX_RANGE_BONUS,
        FALLOFF_BONUS,
        TRACKING_SPEED_BONUS,
        AOE_VELOCITY_BONUS,
        AOE_CLOUD_SIZE_BONUS,
        EXPLOSION_DELAY_BONUS,
        MISSILE_VELOCITY_BONUS,
        DAMAGE_MULTIPLIER,
        SPEED_MULTIPLIER,
        MISSILE_DAMAGE_MULTIPLIER_BONUS,
        DRONE_DAMAGE_BONUS,
        SCAN_GRAVIMETRIC_BONUS,
        SCAN_LADAR_BONUS,
        SCAN_MAGNETOMETRIC_BONUS,
        SCAN_RADAR_BONUS,
        SCAN_GRAVIMETRIC_PERCENT,
        SCAN_LADAR_PERCENT,
        SCAN_MAGNETOMETRIC_PERCENT,
        SCAN_RADAR_PERCENT,
        CLOAKING_TARGETING_DELAY,
        COMMAND_BONUS,
        COMMAND_BONUS_HIDDEN,
        CONSUMPTION_QUANTITY,
        CONSUMPTION_TYPE,
        RELOAD_TIME,
        ARMOR_DAMAGE_AMOUNT,
        SHIELD_BONUS,
        ARMOR_EM_RESONANCE,
        ARMOR_EXPLOSIVE_RESONANCE,
        ARMOR_KINETIC_RESONANCE,
        ARMOR_THERMAL_RESONANCE,
        WARFARE_BUFF_VALUES[0],
        WARFARE_BUFF_VALUES[1],
        WARFARE_BUFF_VALUES[2],
        WARFARE_BUFF_VALUES[3],
        HI_SLOT_MODIFIER,
        MED_SLOT_MODIFIER,
        LOW_SLOT_MODIFIER,
        // Vorton projectors carry their explosion on the module.
        AOE_CLOUD_SIZE,
        AOE_VELOCITY,
    ];

    /// Attributes mostly read off a module's charge.
    pub const AOE_CLOUD_SIZE: i32 = 654;
    pub const AOE_VELOCITY: i32 = 653;
    pub const CAPACITOR_BONUS: i32 = 67;
    pub const BASE_SENSOR_STRENGTH: i32 = 1371;
    pub const BASE_SCAN_RANGE: i32 = 1370;
    pub const DOT_DURATION: i32 = 5735;
    pub const DOT_MAX_DAMAGE_PER_TICK: i32 = 5736;
    pub const DOT_MAX_HP_PERCENTAGE_PER_TICK: i32 = 5737;
    /// warfareBuff1ID to warfareBuff4ID.
    pub const WARFARE_BUFF_IDS: [i32; 4] = [2468, 2470, 2472, 2536];

    pub const CHARGE: [i32; 12] = [
        AOE_CLOUD_SIZE,
        AOE_VELOCITY,
        CAPACITOR_BONUS,
        BASE_SENSOR_STRENGTH,
        BASE_SCAN_RANGE,
        DOT_DURATION,
        DOT_MAX_DAMAGE_PER_TICK,
        DOT_MAX_HP_PERCENTAGE_PER_TICK,
        WARFARE_BUFF_IDS[0],
        WARFARE_BUFF_IDS[1],
        WARFARE_BUFF_IDS[2],
        WARFARE_BUFF_IDS[3],
    ];
}

/// Attributes behind overheating, by their dogma ID.
mod heat_attr {
    pub const HEAT_GENERATION_MULTIPLIER: i32 = 1224;
    pub const HEAT_ABSORBTION_RATE_MODIFIER: i32 = 1180;
    pub const HEAT_ATTENUATION_HI: i32 = 1259;
    pub const HEAT_ATTENUATION_MED: i32 = 1261;
    pub const HEAT_ATTENUATION_LOW: i32 = 1262;
    pub const HP: i32 = 9;
    pub const HEAT_DAMAGE: i32 = 1211;
}

/// What pyfa's Miscellanea column reads off a module or subsystem, after all bonuses. Most of it
/// is plain attributes; the rest is what refine works out as if the module were running.
#[derive(Debug, Clone, Default)]
pub struct ModuleMisc {
    /// The attributes in [`misc_attr::MODULE`] the item has.
    pub attrs: HashMap<i32, f64>,
    /// The attributes in [`misc_attr::CHARGE`] its charge has.
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

fn attrs_of(sol: &mut SolOwned, id: ItemId, wanted: &[i32]) -> HashMap<i32, f64> {
    let Ok(mut item) = sol.get_item(&id) else {
        return HashMap::new();
    };
    wanted
        .iter()
        .filter_map(|&attr| {
            let value = item
                .get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
                .ok()?;
            Some((attr, value.modified.into_f64()))
        })
        .collect()
}

fn burst_at(spool: f64) -> StatTimeOptions {
    StatTimeOptions::Burst(StatTimeOptionsBurst {
        spool: Some(Spool::SpoolScale(UnitInterval::from_f64_clamped(spool))),
    })
}

/// Everything [`ModuleMisc`] holds for one module or subsystem. pyfa shows what a module does
/// whatever its state, so the numbers come from the item switched on.
pub(super) fn module_misc(sol: &mut SolOwned, id: ItemId) -> ModuleMisc {
    let mut misc = ModuleMisc {
        attrs: attrs_of(sol, id, &misc_attr::MODULE),
        ..ModuleMisc::default()
    };
    if let Ok(item) = sol.get_item(&id)
        && let Ok(effects) = item.iter_effects()
    {
        misc.effects = effects
            .filter_map(|(effect, _)| match effect {
                rc::EffectId::Dogma(id) => Some(id.into_i32()),
                _ => None,
            })
            .collect();
    }
    let cycle = misc
        .attr(misc_attr::DURATION)
        .filter(|&d| d > 0.0)
        .or_else(|| misc.attr(misc_attr::SPEED).filter(|&d| d > 0.0))
        .map(|ms| ms / 1000.0);
    if let Ok(mut module) = sol.get_module(&id) {
        if let Some(charge) = module.get_charge() {
            let charge = charge.get_item_id();
            misc.charge_attrs = attrs_of(sol, charge, &misc_attr::CHARGE);
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
            on.get_stat_outgoing_rps(burst_at(spool))
                .map_or(0.0, |r| r.armor.into_f64())
        };
        misc.reps_spool = Some((armor(&mut on, 0.0), armor(&mut on, 1.0)));
    }
    if let Ok(mining) = on.get_stat_mps(StatTimeOptions::default(), StatMiningResourceKind::Regular)
    {
        let entries = [mining.ore, mining.ice, mining.gas];
        misc.mining = (
            entries.iter().map(|e| e.yield_.into_f64()).sum(),
            entries.iter().map(|e| e.drain.into_f64()).sum(),
        );
    }
    if let Ok(dmg) = on.get_stat_dmg(
        StatTimeOptions::default(),
        StatCritOptions::default(),
        StatItemChargeOptions::Include,
    ) {
        let v = dmg.volley;
        misc.volley = [v.em, v.thermal, v.kinetic, v.explosive]
            .into_iter()
            .map(rc::PValue::into_f64)
            .sum();
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
        let own = self.slot_factor * rack_heat;
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
    let attrs = attrs_of(
        sol,
        ship,
        &[
            heat_attr::HEAT_GENERATION_MULTIPLIER,
            heat_attr::HEAT_ATTENUATION_HI,
            heat_attr::HEAT_ATTENUATION_MED,
            heat_attr::HEAT_ATTENUATION_LOW,
        ],
    );
    // pyfa falls back to 0.25 where a ship has no attenuation of its own.
    let attenuation = |attr| attrs.get(&attr).copied().unwrap_or(0.25);
    (
        attrs
            .get(&heat_attr::HEAT_GENERATION_MULTIPLIER)
            .copied()
            .unwrap_or(0.0),
        [
            attenuation(heat_attr::HEAT_ATTENUATION_HI),
            attenuation(heat_attr::HEAT_ATTENUATION_MED),
            attenuation(heat_attr::HEAT_ATTENUATION_LOW),
        ],
    )
}

/// A module's heat attributes: absorption rate, hit points and damage taken per heat hit.
pub(super) fn module_heat(sol: &mut SolOwned, id: ItemId) -> (f64, f64, f64) {
    let attrs = attrs_of(
        sol,
        id,
        &[
            heat_attr::HEAT_ABSORBTION_RATE_MODIFIER,
            heat_attr::HP,
            heat_attr::HEAT_DAMAGE,
        ],
    );
    let get = |attr| attrs.get(&attr).copied().unwrap_or(0.0);
    (
        get(heat_attr::HEAT_ABSORBTION_RATE_MODIFIER),
        get(heat_attr::HP),
        get(heat_attr::HEAT_DAMAGE),
    )
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
