//! The bridge to refine. Each recalculation builds a fresh solar system from the [`Fit`], which
//! keeps refine's state from ever drifting from what the UI shows.

use std::path::Path;

use std::collections::HashMap;

use rc::{
    AddMode, AttrId, EveAttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, SkillLevel,
    SolOwned, Src,
    stats::{
        StatCapBlcSrcKinds, StatCapSim, StatCritOptions, StatDmgEntry, StatDmgItemKinds,
        StatResource, StatSlot, StatTimeOptions,
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

/// What the Boosters tab shows for a booster, after skills.
#[derive(Debug, Clone, Default)]
pub struct BoosterInfo {
    /// How long a dose lasts, in seconds.
    pub duration: Option<f64>,
    pub side_effects: Vec<SideEffectInfo>,
}

#[derive(Debug, Clone, Copy)]
pub struct SideEffectInfo {
    pub effect_id: i32,
    /// How much it changes its attribute, in percent, where it changes it by one amount.
    pub strength: Option<f64>,
    pub active: bool,
}

mod attr {
    pub const CPU: i32 = 50;
    pub const POWER: i32 = 30;
    pub const CAP_NEED: i32 = 6;
    pub const DURATION: i32 = 73;
    pub const RATE_OF_FIRE: i32 = 51;
    pub const OPTIMAL: i32 = 54;
    pub const FALLOFF: i32 = 158;
    pub const CAPACITY: i32 = 38;
    pub const BOOSTER_DURATION: i32 = 330;
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
    /// Per booster, in the fit's order.
    pub boosters: Vec<BoosterInfo>,
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
        for stack in &fit.drones {
            for i in 0..stack.count {
                let minion_state = if i < stack.active {
                    MinionState::Engaging
                } else {
                    MinionState::InBay
                };
                f.add_drone(
                    ItemTypeId::from_i32(stack.type_id),
                    minion_state,
                    None,
                    None,
                );
            }
        }
        for &implant in &fit.implants {
            f.add_implant(ItemTypeId::from_i32(implant));
        }
        let mut booster_ids = Vec::with_capacity(fit.boosters.len());
        for booster in &fit.boosters {
            let mut item = f.add_booster(ItemTypeId::from_i32(booster.type_id));
            item.set_state(booster.active);
            for &effect in &booster.side_effects {
                let id = rc::EffectId::Dogma(rc::DogmaEffectId::from_i32(effect));
                if let Ok(mut side_effect) = item.get_side_effect(&id) {
                    side_effect.set_state(true);
                }
            }
            booster_ids.push(item.get_item_id());
        }

        fit_stats(&mut f, &mut stats);

        stats.cargo_capacity = sol.get_item(&ship_id).ok().and_then(|mut ship| {
            ship.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr::CAPACITY)))
                .ok()
                .map(|v| v.modified.into_f64())
        });

        stats.boosters = booster_ids
            .iter()
            .map(|&id| booster_info(&mut sol, id))
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

fn booster_info(sol: &mut SolOwned, id: ItemId) -> BoosterInfo {
    use rc::Lender;

    let duration = sol.get_item(&id).ok().and_then(|mut item| {
        item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr::BOOSTER_DURATION)))
            .ok()
            .map(|v| v.modified.into_f64() / 1000.0)
    });
    let mut side_effects = Vec::new();
    if let Ok(mut booster) = sol.get_booster(&id) {
        let mut iter = booster.iter_side_effects();
        while let Some(mut side_effect) = iter.next() {
            let rc::EffectId::Dogma(effect_id) = side_effect.get_effect_id() else {
                continue;
            };
            side_effects.push(SideEffectInfo {
                effect_id: effect_id.into_i32(),
                strength: side_effect
                    .get_strength()
                    .map(|s| s.get_strength().into_f64()),
                active: side_effect.get_state(),
            });
        }
    }
    BoosterInfo {
        duration,
        side_effects,
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
