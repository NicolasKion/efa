//! The bridge to refine. Each recalculation builds a fresh solar system from the [`Fit`], which
//! keeps refine's state from ever drifting from what the UI shows.

use std::path::Path;

use std::collections::{HashMap, HashSet};

use rc::{
    AddMode, AttrId, EveAttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, SkillLevel,
    SolOwned, Src,
    stats::{
        ItemStatsCommon, StatCapBlcSrcKinds, StatCapSim, StatCritOptions, StatDmgEntry,
        StatDmgItemKinds, StatMiningResourceKind, StatResource, StatSlot, StatTimeOptions,
    },
};

use crate::{
    data::{Slot, TypeId},
    fit::{Fit, ModuleState},
};

/// Type ID of the character item. EVE uses this one for everyone.
const CHARACTER: i32 = 1373;

pub struct Engine {
    src: Src,
    skills: Vec<TypeId>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Slots {
    pub used: u32,
    pub total: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Resource {
    pub used: f64,
    pub output: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Damage {
    pub em: f64,
    pub thermal: f64,
    pub kinetic: f64,
    pub explosive: f64,
}

impl Damage {
    pub fn total(&self) -> f64 {
        self.em + self.thermal + self.kinetic + self.explosive
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Layer {
    pub hp: f64,
    pub ehp: Option<f64>,
    /// EM, thermal, kinetic, explosive, as fractions resisted.
    pub resists: [f64; 4],
}

/// Per-module numbers for the fitting view's columns, after all bonuses.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModuleInfo {
    pub cpu: Option<f64>,
    pub powergrid: Option<f64>,
    /// Capacitor used per second while cycling.
    pub cap_per_sec: Option<f64>,
    pub optimal: Option<f64>,
    pub falloff: Option<f64>,
    /// Cycles to full spool, for modules that spool up.
    pub spool_max: Option<u32>,
}

/// Per-drone numbers for the drones pane, read off the first drone of a stack (every drone in a
/// stack is the same).
#[derive(Debug, Clone, Default)]
pub struct DroneInfo {
    /// The attributes in [`drone_attr::ALL`] after bonuses; missing ones read as 0.
    pub attrs: HashMap<i32, f64>,
    /// Dogma effects the drone carries. Some attributes default to nonzero values, so pyfa asks
    /// for the effect before reading them.
    pub effects: HashSet<i32>,
    /// Effective HP of all layers together, against uniform damage.
    pub ehp: Option<f64>,
    /// Effective peak passive shield regeneration, per second.
    pub shield_regen: Option<f64>,
    /// Shield, armor and hull repaired per second as if launched.
    pub reps: [f64; 3],
    /// Mining yield and drain in m³ per second as if launched.
    pub mining: (f64, f64),
}

impl DroneInfo {
    /// An attribute after bonuses, if the drone has it with a nonzero value.
    pub fn attr(&self, id: i32) -> Option<f64> {
        self.attrs.get(&id).copied().filter(|&v| v != 0.0)
    }

    pub fn has_effect(&self, id: i32) -> bool {
        self.effects.contains(&id)
    }
}

/// Attributes the drones pane shows, by their dogma ID.
pub mod drone_attr {
    pub const MAX_RANGE: i32 = 54;
    pub const SHIELD_TRANSFER_RANGE: i32 = 87;
    pub const POWER_TRANSFER_RANGE: i32 = 91;
    pub const EMP_FIELD_RANGE: i32 = 99;
    pub const ECM_BURST_RANGE: i32 = 142;
    pub const ECM_RANGE_OPTIMAL: i32 = 936;
    pub const FALLOFF: i32 = 158;
    pub const FALLOFF_EFFECTIVENESS: i32 = 2044;
    pub const MAX_VELOCITY: i32 = 37;
    pub const TRACKING_SPEED: i32 = 160;
    pub const OPTIMAL_SIG_RADIUS: i32 = 620;
    pub const NEUT_AMOUNT: i32 = 97;
    pub const NEUT_DURATION: i32 = 942;
    pub const FALLOFF_BONUS: i32 = 349;
    pub const MAX_RANGE_BONUS: i32 = 351;
    pub const TRACKING_SPEED_BONUS: i32 = 767;
    pub const SCAN_GRAVIMETRIC_BONUS: i32 = 238;
    pub const SCAN_LADAR_BONUS: i32 = 239;
    pub const SCAN_MAGNETOMETRIC_BONUS: i32 = 240;
    pub const SCAN_RADAR_BONUS: i32 = 241;
    pub const ACCESS_DIFFICULTY_BONUS: i32 = 902;
    pub const SPEED_FACTOR: i32 = 20;
    pub const SIG_RADIUS_BONUS: i32 = 554;
    pub const MAX_TARGET_RANGE_BONUS: i32 = 309;
    pub const SCAN_RESOLUTION_BONUS: i32 = 566;

    pub const ALL: [i32; 25] = [
        MAX_RANGE,
        SHIELD_TRANSFER_RANGE,
        POWER_TRANSFER_RANGE,
        EMP_FIELD_RANGE,
        ECM_BURST_RANGE,
        ECM_RANGE_OPTIMAL,
        FALLOFF,
        FALLOFF_EFFECTIVENESS,
        MAX_VELOCITY,
        TRACKING_SPEED,
        OPTIMAL_SIG_RADIUS,
        NEUT_AMOUNT,
        NEUT_DURATION,
        FALLOFF_BONUS,
        MAX_RANGE_BONUS,
        TRACKING_SPEED_BONUS,
        SCAN_GRAVIMETRIC_BONUS,
        SCAN_LADAR_BONUS,
        SCAN_MAGNETOMETRIC_BONUS,
        SCAN_RADAR_BONUS,
        ACCESS_DIFFICULTY_BONUS,
        SPEED_FACTOR,
        SIG_RADIUS_BONUS,
        MAX_TARGET_RANGE_BONUS,
        SCAN_RESOLUTION_BONUS,
    ];
}

/// Effects of combat drones that also do electronic warfare, by their dogma ID.
pub mod drone_effect {
    pub const NEUTRALIZER: i32 = 6691;
    pub const WEBIFIER: i32 = 6690;
    pub const TARGET_PAINTER: i32 = 6692;
    pub const SENSOR_DAMPENER: i32 = 6693;
    pub const WEAPON_DISRUPTOR: i32 = 6694;
    pub const ECM: i32 = 6695;
}

/// Shield level at which passive regeneration peaks.
const PEAK_RECHARGE: f64 = 0.25;

mod attr {
    pub const CPU: i32 = 50;
    pub const POWER: i32 = 30;
    pub const CAP_NEED: i32 = 6;
    pub const DURATION: i32 = 73;
    pub const RATE_OF_FIRE: i32 = 51;
    pub const OPTIMAL: i32 = 54;
    pub const FALLOFF: i32 = 158;
    pub const CAPACITY: i32 = 38;
}

#[derive(Debug, Clone, Copy)]
pub enum Cap {
    Stable(f64),
    Lasts(f64),
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub high: Slots,
    pub mid: Slots,
    pub low: Slots,
    pub rig: Slots,
    pub subsystem: Slots,
    pub turret: Slots,
    pub launcher: Slots,
    pub drones_launched: Slots,
    pub cpu: Resource,
    pub powergrid: Resource,
    pub calibration: Resource,
    pub drone_bandwidth: Resource,
    pub drone_bay: Resource,
    pub dps: Damage,
    pub volley: Damage,
    pub weapon_dps: f64,
    pub drone_dps: f64,
    pub modules: HashMap<(Slot, usize), ModuleInfo>,
    /// Per-drone numbers, by drone stack.
    pub drones: Vec<DroneInfo>,
    /// Cargo hold size after bonuses, in m³.
    pub cargo_capacity: Option<f64>,
    pub shield: Layer,
    pub armor: Layer,
    pub hull: Layer,
    pub cap_amount: Option<f64>,
    pub cap_balance: Option<f64>,
    pub cap: Option<Cap>,
    pub speed: Option<f64>,
    pub align_time: Option<f64>,
    pub sig_radius: Option<f64>,
    pub warp_speed: Option<f64>,
    pub lock_range: Option<f64>,
    pub scan_res: Option<f64>,
    pub locks: Option<u32>,
    pub sensor_strength: Option<f64>,
    /// Items refine refused, e.g. a module the ship has no rack position for.
    pub problems: Vec<String>,
}

impl Engine {
    /// Load refine's data from a Phobos-layout dump. The adapted data is cached in `cache_dir`, so
    /// only the first start after a data update pays for converting it.
    pub fn load(data_dir: &Path, cache_dir: &Path, skills: Vec<TypeId>) -> Result<Self, String> {
        let edh = redh::PhbFsEdh::new(data_dir).into();
        let adc = radc::PostcardZfsAdc::new(cache_dir, "tq").into();
        let src = Src::new(&edh, Some(&adc)).map_err(|e| format!("loading engine data: {e}"))?;
        Ok(Self { src, skills })
    }

    pub fn compute(&self, fit: &Fit) -> Stats {
        let mut stats = Stats::default();
        let mut sol = SolOwned::new(&self.src);
        let mut f = sol.add_fit();
        f.set_character(ItemTypeId::from_i32(CHARACTER));
        for &skill in &self.skills {
            let _ = f.add_skill(ItemTypeId::from_i32(skill), SkillLevel::from_i32_clamped(5));
        }
        let Some(ship) = fit.ship else { return stats };
        let ship_id = f
            .set_ship(ItemTypeId::from_i32(ship), None, None)
            .get_item_id();

        let mut module_ids: Vec<(Slot, usize, ItemId)> = Vec::new();
        for (slot, rack, modules) in [
            (Slot::High, ModRack::High, &fit.high),
            (Slot::Mid, ModRack::Mid, &fit.mid),
            (Slot::Low, ModRack::Low, &fit.low),
        ] {
            for (i, m) in modules.iter().enumerate() {
                let Some(m) = m else { continue };
                // Each module goes to its own position; heat spreads by position.
                match f.add_module(
                    rack,
                    AddMode::Replace(rc::RackPos::from_usize_clamped(i)),
                    ItemTypeId::from_i32(m.type_id),
                    module_state(m.state),
                ) {
                    Ok(mut module) => {
                        module_ids.push((slot, i, module.get_item_id()));
                        if let Some(cycles) = m.spool {
                            module.set_spool_override(Some(rc::Spool::Cycles(
                                rc::Count::from_u32(cycles),
                            )));
                        }
                        if let Some(charge) = m.charge {
                            module.set_charge_type_id(ItemTypeId::from_i32(charge));
                        }
                    }
                    Err(e) => stats.problems.push(format!("module {}: {e}", m.type_id)),
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
        let mut drone_ids: Vec<Option<ItemId>> = Vec::with_capacity(fit.drones.len());
        for stack in &fit.drones {
            let mut first = None;
            for i in 0..stack.count {
                let minion_state = if i < stack.active {
                    MinionState::Engaging
                } else {
                    MinionState::InBay
                };
                let id = f
                    .add_drone(
                        ItemTypeId::from_i32(stack.type_id),
                        minion_state,
                        None,
                        None,
                    )
                    .get_item_id();
                first.get_or_insert(id);
            }
            drone_ids.push(first);
        }
        for &implant in &fit.implants {
            f.add_implant(ItemTypeId::from_i32(implant));
        }
        for &booster in &fit.boosters {
            f.add_booster(ItemTypeId::from_i32(booster));
        }

        fit_stats(&mut f, &mut stats);

        stats.cargo_capacity = sol.get_item(&ship_id).ok().and_then(|mut ship| {
            ship.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr::CAPACITY)))
                .ok()
                .map(|v| v.modified.into_f64())
        });

        stats.drones = drone_ids
            .iter()
            .map(|id| id.map_or_default(|id| drone_info(&mut sol, id)))
            .collect();

        for (slot, i, id) in module_ids {
            let spool_max = sol
                .get_module(&id)
                .ok()
                .and_then(|mut m| m.get_spool_cycle_count())
                .map(|info| info.max.into_u32());
            let Ok(mut item) = sol.get_item(&id) else {
                continue;
            };
            let mut get = |attr: i32| {
                item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
                    .ok()
                    .map(|v| v.modified.into_f64())
            };
            let cap_need = get(attr::CAP_NEED).filter(|&c| c > 0.0);
            // Missing attributes read as 0, so filter before falling back: turrets and launchers
            // cycle on their rate of fire, everything else on its duration.
            let cycle = get(attr::DURATION)
                .filter(|&d| d > 0.0)
                .or_else(|| get(attr::RATE_OF_FIRE).filter(|&d| d > 0.0));
            let info = ModuleInfo {
                cpu: get(attr::CPU).filter(|&v| v > 0.0),
                powergrid: get(attr::POWER).filter(|&v| v > 0.0),
                cap_per_sec: cap_need.zip(cycle).map(|(need, ms)| need / (ms / 1000.0)),
                optimal: get(attr::OPTIMAL).filter(|&v| v > 0.0),
                falloff: get(attr::FALLOFF).filter(|&v| v > 0.0),
                spool_max,
            };
            stats.modules.insert((slot, i), info);
        }
        stats
    }
}

fn fit_stats(f: &mut rc::Fit<'_>, stats: &mut Stats) {
    let mut s = f.stats();
    stats.high = slots(s.get_stat_high_slots());
    stats.mid = slots(s.get_stat_mid_slots());
    stats.low = slots(s.get_stat_low_slots());
    stats.rig = slots(s.get_stat_rig_slots());
    stats.subsystem = slots(s.get_stat_subsystem_slots());
    stats.turret = slots(s.get_stat_turret_slots());
    stats.launcher = slots(s.get_stat_launcher_slots());
    stats.drones_launched = slots(s.get_stat_launched_drones());
    stats.cpu = resource(s.get_stat_cpu());
    stats.powergrid = resource(s.get_stat_powergrid());
    stats.calibration = resource(s.get_stat_calibration());
    stats.drone_bandwidth = resource(s.get_stat_drone_bandwidth());
    stats.drone_bay = resource(s.get_stat_drone_bay_volume());

    let dmg = s.get_stat_dmg(
        StatDmgItemKinds::default(),
        StatTimeOptions::default(),
        StatCritOptions::default(),
    );
    stats.dps = damage(&dmg.dps);
    stats.volley = damage(&dmg.volley);
    let drones = s.get_stat_dmg(
        StatDmgItemKinds::new(false).with_minion_mobile(true),
        StatTimeOptions::default(),
        StatCritOptions::default(),
    );
    stats.drone_dps = damage(&drones.dps).total();
    stats.weapon_dps = stats.dps.total() - stats.drone_dps;

    if let Ok(hp) = s.get_stat_hp() {
        stats.shield.hp = hp.shield.buffer.into_f64();
        stats.armor.hp = hp.armor.buffer.into_f64();
        stats.hull.hp = hp.hull.buffer.into_f64();
    }
    if let Ok(ehp) = s.get_stat_ehp(None) {
        stats.shield.ehp = ehp.shield.map(|l| l.buffer.into_f64() * l.mult.into_f64());
        stats.armor.ehp = ehp.armor.map(|l| l.buffer.into_f64() * l.mult.into_f64());
        stats.hull.ehp = ehp.hull.map(|l| l.buffer.into_f64() * l.mult.into_f64());
    }
    if let Ok(r) = s.get_stat_resists() {
        for (layer, res) in [
            (&mut stats.shield, r.shield),
            (&mut stats.armor, r.armor),
            (&mut stats.hull, r.hull),
        ] {
            layer.resists = [
                res.em.into_f64(),
                res.thermal.into_f64(),
                res.kinetic.into_f64(),
                res.explosive.into_f64(),
            ];
        }
    }

    stats.cap_amount = s.get_stat_cap_amount().ok().map(rc::PValue::into_f64);
    stats.cap_balance = s
        .get_stat_cap_balance(StatCapBlcSrcKinds::default(), StatTimeOptions::default())
        .ok()
        .map(rc::Value::into_f64);
    stats.cap = s
        .get_stat_cap_sim(
            rc::UnitInterval::from_f64_clamped(1.0),
            None,
            &rc::stats::StatCapSimStagger::default(),
            None,
        )
        .ok()
        .map(|sim| match sim {
            StatCapSim::Stable(level) => Cap::Stable(level.into_f64()),
            StatCapSim::Time(secs) => Cap::Lasts(secs.into_f64()),
        });

    stats.speed = s.get_stat_speed().ok().map(rc::PValue::into_f64);
    stats.align_time = s.get_stat_align_time().ok().map(rc::PValue::into_f64);
    stats.sig_radius = s.get_stat_sig_radius().ok().map(rc::PValue::into_f64);
    stats.warp_speed = s.get_stat_warp_speed().ok().map(rc::PValue::into_f64);
    stats.lock_range = s.get_stat_lock_range().ok().map(rc::PValue::into_f64);
    stats.scan_res = s.get_stat_scan_res().ok().map(rc::PValue::into_f64);
    stats.locks = s.get_stat_locks().ok().map(rc::Count::into_u32);
    stats.sensor_strength = s.get_stat_sensors().ok().map(|v| v.strength.into_f64());
}

fn drone_info(sol: &mut SolOwned, id: ItemId) -> DroneInfo {
    let mut info = DroneInfo::default();
    if let Ok(mut item) = sol.get_item(&id) {
        for attr in drone_attr::ALL {
            if let Ok(v) = item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr))) {
                info.attrs.insert(attr, v.modified.into_f64());
            }
        }
        if let Ok(effects) = item.iter_effects() {
            info.effects = effects
                .filter_map(|(effect, _)| match effect {
                    rc::EffectId::Dogma(id) => Some(id.into_i32()),
                    _ => None,
                })
                .collect();
        }
    }
    let mut stats = sol.stats();
    let Ok(mut item) = stats.get_item(&id) else {
        return info;
    };
    if let Ok(ehp) = item.get_stat_ehp(None) {
        let layers = [ehp.shield, ehp.armor, ehp.hull];
        info.ehp = Some(
            layers
                .iter()
                .flatten()
                .map(|l| l.buffer.into_f64() * l.mult.into_f64())
                .sum(),
        );
    }
    info.shield_regen = item
        .get_stat_erps(
            None,
            StatTimeOptions::default(),
            rc::UnitInterval::from_f64_clamped(PEAK_RECHARGE),
        )
        .ok()
        .and_then(|e| e.shield)
        .map(|s| s.regen.into_f64());
    // pyfa shows what a drone does even while it sits in the bay.
    let mut switched = item.switch_states();
    if let Ok(reps) = switched.get_stat_outgoing_rps(StatTimeOptions::default()) {
        info.reps = [reps.shield, reps.armor, reps.hull].map(rc::PValue::into_f64);
    }
    if let Ok(mining) =
        switched.get_stat_mps(StatTimeOptions::default(), StatMiningResourceKind::Regular)
    {
        let entries = [mining.ore, mining.ice, mining.gas];
        info.mining = (
            entries.iter().map(|e| e.yield_.into_f64()).sum(),
            entries.iter().map(|e| e.drain.into_f64()).sum(),
        );
    }
    info
}

fn module_state(state: ModuleState) -> rc::ModuleState {
    match state {
        ModuleState::Offline => rc::ModuleState::Offline,
        ModuleState::Online => rc::ModuleState::Online,
        ModuleState::Active => rc::ModuleState::Active,
        ModuleState::Overload => rc::ModuleState::Overload,
    }
}

fn slots(s: StatSlot) -> Slots {
    Slots {
        used: s.used.into_u32(),
        total: s.total.map(rc::Count::into_u32),
    }
}

fn resource(r: StatResource) -> Resource {
    Resource {
        used: r.used.into_f64(),
        output: r.output.map(rc::Value::into_f64),
    }
}

fn damage(d: &StatDmgEntry) -> Damage {
    Damage {
        em: d.em.into_f64(),
        thermal: d.thermal.into_f64(),
        kinetic: d.kinetic.into_f64(),
        explosive: d.explosive.into_f64(),
    }
}
