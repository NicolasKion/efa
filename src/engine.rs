//! The bridge to refine. Each recalculation builds a fresh solar system from the [`Fit`], which
//! keeps refine's state from ever drifting from what the UI shows.

mod graph;
mod inspect;
mod pane;
mod pilot;
mod surroundings;

use std::path::Path;

use std::collections::{HashMap, HashSet};

use rc::{
    AddMode, AttrId, EveAttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, SolOwned,
    Src,
    stats::{
        ItemStatsCommon, StatCapBlcSrcKinds, StatCapSim, StatCritOptions, StatDmgEntry,
        StatDmgItemKinds, StatMiningResourceKind, StatResource, StatSlot, StatTimeOptions,
    },
};

use crate::{
    data::{Slot, TypeId},
    fit::{Fit, ModuleState},
};

pub use inspect::{AttrValue, FitItem, Inspection, ModOp, Modifier, Source, SourceKind};
pub use pane::{
    CalcOptions, CapDetail, FirepowerDetail, Mining, MiningRate, Misc, Outgoing, SpoolRange, Tank,
};
pub use pilot::{MAX_LEVEL, Pilot, SkillTarget};
pub use surroundings::{AbilityInfo, FighterClasses, FighterInfo};

/// Type ID of the character item. EVE uses this one for everyone.
const CHARACTER: i32 = 1373;

pub struct Engine {
    src: Src,
    /// The built-in All 5 character, which [`Engine::compute`] flies fits with.
    all5: Pilot,
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
    /// EHP per HP under the fit's damage profile.
    pub mult: Option<f64>,
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
    pub fighter_tubes: Slots,
    pub fighter_bay: Resource,
    /// Damage mix the tank numbers assume: EM, thermal, kinetic, explosive.
    pub incoming: [f64; 4],
    pub firepower: FirepowerDetail,
    /// Repair rates with every repairer cycling.
    pub tank: Tank,
    /// Repair rates the capacitor can keep up.
    pub sustained: Tank,
    pub outgoing: Outgoing,
    pub mining: Mining,
    pub cap_detail: CapDetail,
    pub misc: Misc,
    /// Per booster, in the fit's order.
    pub boosters: Vec<BoosterInfo>,
    /// Items refine refused, e.g. a module the ship has no rack position for.
    pub problems: Vec<String>,
    /// Per squadron, in the fit's order.
    pub fighters: Vec<FighterInfo>,
    /// Per projected squadron, in the fit's order.
    pub projected_fighters: Vec<FighterInfo>,
    pub fighter_classes: FighterClasses,
    /// Cycles to full spool per projected module, for those that spool up.
    pub projected_spool: Vec<Option<u32>>,
}

impl Engine {
    /// Load refine's data from a Phobos-layout dump. The adapted data is cached in `cache_dir`, so
    /// only the first start after a data update pays for converting it.
    pub fn load(data_dir: &Path, cache_dir: &Path, skills: Vec<TypeId>) -> Result<Self, String> {
        let edh = redh::PhbFsEdh::new(data_dir).into();
        let adc = radc::PostcardZfsAdc::new(cache_dir, "tq").into();
        let src = Src::new(&edh, Some(&adc)).map_err(|e| format!("loading engine data: {e}"))?;
        let all5 = Pilot {
            skills: skills.into_iter().map(|s| (s, MAX_LEVEL)).collect(),
            ..Pilot::default()
        };
        Ok(Self { src, all5 })
    }

    /// The built-in All 5 character.
    pub fn all5(&self) -> &Pilot {
        &self.all5
    }

    pub fn compute(&self, fit: &Fit) -> Stats {
        self.compute_with(fit, CalcOptions::default())
    }

    /// Stats with the All 5 character; [`Engine::compute_for`] takes the fit's own.
    pub fn compute_with(&self, fit: &Fit, options: CalcOptions) -> Stats {
        self.compute_for(fit, &self.all5, options)
    }

    /// The fit's stats flown by `pilot`, alone in space.
    pub fn compute_for(&self, fit: &Fit, pilot: &Pilot, options: CalcOptions) -> Stats {
        self.compute_among(fit, &[], pilot, options)
    }

    /// The fit's stats flown by `pilot`, with what surrounds it: `fits` are the saved fits its
    /// projected and command fits are looked up in.
    pub fn compute_among(
        &self,
        fit: &Fit,
        fits: &[Fit],
        pilot: &Pilot,
        options: CalcOptions,
    ) -> Stats {
        let mut stats = Stats::default();
        let mut sol = SolOwned::new(&self.src);
        if let Some(spool) = options.default_spool {
            sol.set_default_spool(rc::Spool::SpoolScale(rc::UnitInterval::from_f64_clamped(
                spool,
            )));
        }
        let mut f = sol.add_fit();
        pilot.board(&mut f);
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
        f.set_sec_status(rc::FitSecStatus::from_f64_clamped(
            fit.sec_status.unwrap_or(pilot.sec_status),
        ));
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
        for (implant, active) in fit.applied_implants(&pilot.implants) {
            let mut implant = f.add_implant(ItemTypeId::from_i32(implant));
            if !active {
                implant.set_state(false);
            }
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

        let fighter_ids = surroundings::add_fighters(&mut f, &fit.fighters);
        let fit_id = f.get_fit_id();
        let projections = self.add_surroundings(&mut sol, fit_id, ship_id, fit, fits);
        let Ok(mut f) = sol.get_fit(&fit_id) else {
            return stats;
        };

        stats.incoming = pane::incoming_mix(&mut f, &fit.damage_profile);
        let incoming = pane::dps_profile(stats.incoming);
        f.set_rah_incoming_dps(Some(incoming));

        fit_stats(&mut f, &mut stats, incoming, options);
        stats.firepower = pane::firepower_detail(&mut f, options.time());

        stats.cargo_capacity = sol.get_item(&ship_id).ok().and_then(|mut ship| {
            ship.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr::CAPACITY)))
                .ok()
                .map(|v| v.modified.into_f64())
        });

        stats.boosters = booster_ids
            .iter()
            .map(|&id| booster_info(&mut sol, id))
            .collect();
        stats.drones = drone_ids
            .iter()
            .map(|id| id.map_or_default(|id| drone_info(&mut sol, id)))
            .collect();
        stats.fighters = fighter_ids
            .into_iter()
            .map(|id| surroundings::fighter_info(&mut sol, id))
            .collect();
        stats.projected_fighters = projections
            .fighters
            .into_iter()
            .map(|id| surroundings::fighter_info(&mut sol, id))
            .collect();
        stats.projected_spool = projections
            .modules
            .iter()
            .map(|id| {
                let id = id.as_ref()?;
                let mut module = sol.get_module(id).ok()?;
                module
                    .get_spool_cycle_count()
                    .map(|info| info.max.into_u32())
            })
            .collect();
        stats.fighter_classes = surroundings::fighter_classes(&mut sol, fit_id);

        for &(slot, i, id) in &module_ids {
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

        let cap_users: Vec<(ItemId, f64)> = module_ids
            .iter()
            .filter(|&&(slot, i, _)| {
                let m = match slot {
                    Slot::High => &fit.high,
                    Slot::Mid => &fit.mid,
                    _ => &fit.low,
                };
                m.get(i)
                    .and_then(Option::as_ref)
                    .is_some_and(|m| matches!(m.state, ModuleState::Active | ModuleState::Overload))
            })
            .filter_map(|&(slot, i, id)| {
                let cap = stats.modules.get(&(slot, i))?.cap_per_sec?;
                Some((id, cap))
            })
            .collect();
        pane::pane_stats(&mut sol, fit_id, ship_id, &cap_users, options, &mut stats);
        stats
    }
}

fn fit_stats(
    f: &mut rc::Fit<'_>,
    stats: &mut Stats,
    incoming: rc::DpsProfile,
    options: CalcOptions,
) {
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
        options.time(),
        StatCritOptions::default(),
    );
    stats.dps = damage(&dmg.dps);
    stats.volley = damage(&dmg.volley);
    // Sentries are static minions; they count as drones too.
    let drones = s.get_stat_dmg(pane::minions(), options.time(), StatCritOptions::default());
    stats.drone_dps = damage(&drones.dps).total();
    stats.weapon_dps = stats.dps.total() - stats.drone_dps;

    if let Ok(hp) = s.get_stat_hp() {
        stats.shield.hp = hp.shield.buffer.into_f64();
        stats.armor.hp = hp.armor.buffer.into_f64();
        stats.hull.hp = hp.hull.buffer.into_f64();
    }
    // The buffer already has the resist multiplier applied. Ancillary repairer charges stay out,
    // like pyfa's EHP.
    if let Ok(ehp) = s.get_stat_ehp(Some(incoming)) {
        for (layer, ehp) in [
            (&mut stats.shield, ehp.shield),
            (&mut stats.armor, ehp.armor),
            (&mut stats.hull, ehp.hull),
        ] {
            layer.ehp = ehp.map(|l| l.buffer.into_f64());
            layer.mult = ehp.map(|l| l.mult.into_f64());
        }
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
            options.optional_reloads(),
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
                // refine's buffer already has the resists applied.
                .map(|l| l.buffer.into_f64())
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
