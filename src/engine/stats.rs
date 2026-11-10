//! The fit's core numbers: slots, resources, damage, layers and the capacitor, read from a fit
//! built into a solar system.

use std::collections::HashMap;

use rc::stats::{
    FitStats, StatCapBlcSrcKinds, StatCapSim, StatCritOptions, StatDmgEntry, StatDmgItemKinds,
    StatResource, StatSlot, StatTimeOptions,
};

use super::{
    BoosterInfo, CalcOptions, CapDetail, DroneInfo, FighterClasses, FighterInfo, FirepowerDetail,
    FitItem, Mining, Misc, ModuleMisc, Outgoing, Tank, Validation, pane,
};
use crate::data::Slot;

#[derive(Debug, Clone, Copy, Default)]
pub struct Slots {
    pub used: u32,
    pub total: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Resource {
    pub used: f64,
    pub output: Option<f64>,
}

/// Leeway for float noise when comparing what a fit uses with what it has.
const RESOURCE_TOLERANCE: f64 = 1e-6;

impl Resource {
    /// Whether more is used than there is.
    pub fn is_over(&self) -> bool {
        self.output
            .is_some_and(|out| self.used > out + RESOURCE_TOLERANCE)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Damage {
    pub em: f64,
    pub thermal: f64,
    pub kinetic: f64,
    pub explosive: f64,
    /// Breacher pods, which ignore resists; how much lands depends on the target's HP.
    pub breacher: Breacher,
}

impl Damage {
    /// The four damage types; breacher damage is not in it, see [`Breacher`].
    pub fn total(&self) -> f64 {
        self.em + self.thermal + self.kinetic + self.explosive
    }

    /// EM, thermal, kinetic, explosive.
    pub fn as_array(&self) -> [f64; 4] {
        [self.em, self.thermal, self.kinetic, self.explosive]
    }
}

impl From<&StatDmgEntry> for Damage {
    fn from(d: &StatDmgEntry) -> Self {
        Self {
            em: d.em.into_f64(),
            thermal: d.thermal.into_f64(),
            kinetic: d.kinetic.into_f64(),
            explosive: d.explosive.into_f64(),
            breacher: Breacher {
                absolute: d.breacher.absolute_max.into_f64(),
                relative: d.breacher.relative_max.into_f64(),
            },
        }
    }
}

/// Breacher pod damage before a target is known: at most `absolute`, and at most `relative`
/// times the target's HP, pyfa's `BreacherInfo`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Breacher {
    pub absolute: f64,
    pub relative: f64,
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
    /// How far the loaded missiles fly, for the range's tooltip.
    pub missile: Option<crate::graphs::math::MissileRange>,
    /// Cycles to full spool, for modules that spool up.
    pub spool_max: Option<u32>,
    /// Expected seconds until the module burns out: an overheated one from its own heat and
    /// its overheated neighbours', any other one in the rack from its neighbours' alone.
    pub burnout: Option<f64>,
    /// The burnout comes from overheated neighbours, not from overheating this module.
    pub burnout_from_neighbours: bool,
    /// How many of its loaded charge the module holds.
    pub charges: Option<u32>,
    /// Cycles before the loaded charges run out, for modules that use them up.
    pub charged_cycles: Option<u32>,
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
    pub service: Slots,
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
    /// What the Miscellanea column shows, per module and subsystem.
    pub module_misc: HashMap<FitItem, ModuleMisc>,
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
    /// What breaks the fitting rules, by item.
    pub validation: Validation,
}

impl Stats {
    /// A rack's slots, used and in total.
    pub fn slots(&self, slot: Slot) -> Slots {
        match slot {
            Slot::High => self.high,
            Slot::Mid => self.mid,
            Slot::Low => self.low,
            Slot::Rig => self.rig,
            Slot::Subsystem => self.subsystem,
            Slot::Service => self.service,
        }
    }
}

pub(super) fn fit_stats(
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
    stats.service = slots(s.get_stat_service_slots());
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
    stats.dps = Damage::from(&dmg.dps);
    stats.volley = Damage::from(&dmg.volley);
    // Sentries are static minions; they count as drones too.
    let drones = s.get_stat_dmg(pane::minions(), options.time(), StatCritOptions::default());
    stats.drone_dps = Damage::from(&drones.dps).total();
    stats.weapon_dps = stats.dps.total() - stats.drone_dps;

    [stats.shield, stats.armor, stats.hull] = layers(&mut s, incoming);

    stats.cap_amount = s.get_stat_cap_amount().ok().map(rc::PValue::into_f64);
    stats.cap_balance = s
        .get_stat_cap_balance(StatCapBlcSrcKinds::default(), StatTimeOptions::default())
        .ok()
        .map(rc::Value::into_f64);
    stats.cap = cap(&mut s, options);

    stats.speed = s.get_stat_speed().ok().map(rc::PValue::into_f64);
    stats.align_time = s.get_stat_align_time().ok().map(rc::PValue::into_f64);
    stats.sig_radius = s.get_stat_sig_radius().ok().map(rc::PValue::into_f64);
    stats.warp_speed = s.get_stat_warp_speed().ok().map(rc::PValue::into_f64);
    stats.lock_range = s.get_stat_lock_range().ok().map(rc::PValue::into_f64);
    stats.scan_res = s.get_stat_scan_res().ok().map(rc::PValue::into_f64);
    stats.locks = s.get_stat_locks().ok().map(rc::Count::into_u32);
    stats.sensor_strength = s.get_stat_sensors().ok().map(|v| v.strength.into_f64());
}

/// Shield, armor and hull: HP, EHP against `incoming` and resists.
pub(super) fn layers(s: &mut FitStats<'_>, incoming: rc::DpsProfile) -> [Layer; 3] {
    let mut layers = [Layer::default(); 3];
    if let Ok(hp) = s.get_stat_hp() {
        for (layer, hp) in layers.iter_mut().zip([hp.shield, hp.armor, hp.hull]) {
            layer.hp = hp.buffer.into_f64();
        }
    }
    // The buffer already has the resist multiplier applied. Ancillary repairer charges stay out,
    // like pyfa's EHP.
    if let Ok(ehp) = s.get_stat_ehp(Some(incoming)) {
        for (layer, ehp) in layers.iter_mut().zip([ehp.shield, ehp.armor, ehp.hull]) {
            layer.ehp = ehp.map(|l| l.buffer.into_f64());
            layer.mult = ehp.map(|l| l.mult.into_f64());
        }
    }
    if let Ok(r) = s.get_stat_resists() {
        for (layer, res) in layers.iter_mut().zip([r.shield, r.armor, r.hull]) {
            layer.resists =
                [res.em, res.thermal, res.kinetic, res.explosive].map(rc::UnitInterval::into_f64);
        }
    }
    layers
}

/// How long the capacitor lasts with everything running, or where it settles.
pub(super) fn cap(s: &mut FitStats<'_>, options: CalcOptions) -> Option<Cap> {
    s.get_stat_cap_sim(
        rc::UnitInterval::from_f64_clamped(1.0),
        options.optional_reloads(),
        &rc::stats::StatCapSimStagger::default(),
        None,
    )
    .ok()
    .map(|sim| match sim {
        StatCapSim::Stable(level) => Cap::Stable(level.into_f64()),
        StatCapSim::Time(secs) => Cap::Lasts(secs.into_f64()),
    })
}

pub(super) fn slots(s: StatSlot) -> Slots {
    Slots {
        used: s.used.into_u32(),
        total: s.total.map(rc::Count::into_u32),
    }
}

pub(super) fn resource(r: StatResource) -> Resource {
    Resource {
        used: r.used.into_f64(),
        output: r.output.map(rc::Value::into_f64),
    }
}
