//! Numbers only the stats pane shows: repair rates, outgoing support, mining, capacitor detail and
//! the targeting and misc column, gathered after the core recalculation.

use rc::{
    AttrId, CtlAffectors, DpsProfile, EveAttrId, FitId, ItemCommon, ItemId, OptionExt,
    OptionalReload, PValue, SolOwned, Spool, UnitInterval,
    stats::{
        StatCapBlcSrcKinds, StatCritOptions, StatDmgItemKinds, StatMining, StatMiningItemKinds,
        StatMiningResourceKind, StatNeutItemKinds, StatOutRepItemKinds, StatSensorsKind,
        StatTimeOptions, StatTimeOptionsBurst, StatTimeOptionsSim,
    },
};

use super::{Cap, Damage, Stats};
use crate::fit::DamageProfile;

/// Settings that change how numbers are calculated rather than what is fitted.
#[derive(Debug, Clone, Copy, Default)]
pub struct CalcOptions {
    /// Average reload time into DPS, repair rates and the capacitor simulation.
    pub factor_reload: bool,
    /// Spool-up for modules without their own setting, as a fraction of full spool. None keeps
    /// refine's default, full spool.
    pub default_spool: Option<f64>,
}

impl CalcOptions {
    pub(super) fn time(self) -> StatTimeOptions {
        if self.factor_reload {
            StatTimeOptions::Sim(StatTimeOptionsSim {
                time: None,
                optional_reloads: Some(OptionalReload::OnEmpty),
                rearm_minions: None,
            })
        } else {
            StatTimeOptions::default()
        }
    }

    pub(super) fn optional_reloads(self) -> Option<OptionalReload> {
        self.factor_reload.then_some(OptionalReload::OnEmpty)
    }
}

/// A rate at zero spool and at full spool, for modules that spool up over cycles.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpoolRange {
    pub pre: f64,
    pub full: f64,
}

/// What the firepower tooltips break down: spool ranges, which only weapons have, and the
/// damage types drones deal.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirepowerDetail {
    pub drone_dps: Damage,
    pub weapon_dps: SpoolRange,
    pub volley: SpoolRange,
    pub dps: SpoolRange,
}

/// Raw repair rates in HP/s, including incoming remote repairs.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tank {
    /// Shield regeneration at its peak, at 25% shield.
    pub passive_shield: f64,
    pub shield: f64,
    pub armor: f64,
    pub hull: f64,
    pub armor_spool: SpoolRange,
}

/// What the fit gives others per second.
#[derive(Debug, Clone, Copy, Default)]
pub struct Outgoing {
    /// Shield, armor and hull HP/s.
    pub reps: [f64; 3],
    pub reps_spool: [SpoolRange; 3],
    /// Capacitor transferred, GJ/s.
    pub cap: f64,
    /// Capacitor neutralized, GJ/s.
    pub neut: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MiningRate {
    /// m³ per second that ends up in the hold.
    pub yield_: f64,
    /// m³ per second taken from the rock, including waste.
    pub drain: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Mining {
    pub miner: MiningRate,
    pub drone: MiningRate,
}

/// What pyfa's capacitor view shows besides capacity and stability.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapDetail {
    /// GJ/s gained at peak recharge, plus transfers and nosferatus.
    pub recharge: f64,
    /// GJ/s spent by active modules and incoming neutralizers.
    pub used: f64,
    /// Fraction of neutralization resisted.
    pub neut_resist: f64,
    /// Seconds to recharge from empty to full, as the game labels it.
    pub recharge_time: Option<f64>,
}

/// The rest of pyfa's targeting and misc view.
#[derive(Debug, Clone, Default)]
pub struct Misc {
    pub drone_control_range: Option<f64>,
    /// Chance in 0 to 1 that projected ECM jams the fit.
    pub jam_chance: f64,
    pub sensor_kind: Option<&'static str>,
    /// In AU.
    pub max_warp_range: Option<f64>,
    pub warp_core_strength: f64,
    pub mass: Option<f64>,
    pub agility: Option<f64>,
    pub probe_size: Option<f64>,
    /// Special holds the ship has, with their capacity in m³.
    pub holds: Vec<(&'static str, f64)>,
}

mod attr {
    pub const RECHARGE_RATE: i32 = 55;
    pub const WARP_SCRAMBLE_STATUS: i32 = 104;
}

/// Special holds in the order pyfa lists them in the cargo tooltip.
const HOLDS: [(i32, &str); 21] = [
    (912, "Fleet hangar"),
    (908, "Maintenance bay"),
    (5646, "Infrastructure hold"),
    (1573, "Ammo hold"),
    (1549, "Fuel bay"),
    (1560, "Ship hold"),
    (1561, "Small ship hold"),
    (1562, "Medium ship hold"),
    (1563, "Large ship hold"),
    (1564, "Industrial ship hold"),
    (1556, "Mining hold"),
    (3136, "Ice hold"),
    (1557, "Gas hold"),
    (1558, "Mineral hold"),
    (1770, "Material bay"),
    (1559, "Salvage hold"),
    (1646, "Command center hold"),
    (1653, "Planetary goods hold"),
    (1804, "Quafe hold"),
    (5325, "Mobile depot hold"),
    (5944, "Expedition hold"),
];

/// Shield regeneration peaks at this fill level.
const PEAK_SHIELD: f64 = 0.25;

/// Uniform damage, pyfa's default pattern.
const UNIFORM: [f64; 4] = [25.0; 4];

/// The EM, thermal, kinetic, explosive mix a damage profile stands for on this fit.
pub(super) fn incoming_mix(f: &mut rc::Fit<'_>, profile: &DamageProfile) -> [f64; 4] {
    match profile {
        DamageProfile::Uniform => UNIFORM,
        DamageProfile::Pattern { amounts, .. } => {
            if amounts.iter().sum::<f64>() > 0.0 {
                *amounts
            } else {
                UNIFORM
            }
        }
        DamageProfile::SelectedAmmo => {
            let volley = f
                .stats()
                .get_stat_dmg(
                    weapons(),
                    StatTimeOptions::default(),
                    StatCritOptions::default(),
                )
                .volley;
            let mix = [volley.em, volley.thermal, volley.kinetic, volley.explosive]
                .map(rc::PValue::into_f64);
            if mix.iter().sum::<f64>() > 0.0 {
                mix
            } else {
                UNIFORM
            }
        }
    }
}

pub(super) fn dps_profile(mix: [f64; 4]) -> DpsProfile {
    let [em, thermal, kinetic, explosive] = mix.map(PValue::from_f64_clamped);
    DpsProfile {
        em,
        thermal,
        kinetic,
        explosive,
        breacher: None,
    }
}

/// Everything that deals damage except drones and fighters.
pub(super) fn weapons() -> StatDmgItemKinds {
    StatDmgItemKinds::default()
        .with_minion_mobile(false)
        .with_minion_static(false)
}

pub(super) fn minions() -> StatDmgItemKinds {
    StatDmgItemKinds::new(false)
        .with_minion_mobile(true)
        .with_minion_static(true)
}

fn burst_at(spool: f64) -> StatTimeOptions {
    StatTimeOptions::Burst(StatTimeOptionsBurst {
        spool: Some(Spool::SpoolScale(UnitInterval::from_f64_clamped(spool))),
    })
}

pub(super) fn firepower_detail(f: &mut rc::Fit<'_>, time: StatTimeOptions) -> FirepowerDetail {
    let mut s = f.stats();
    let drone_dps = super::damage(
        &s.get_stat_dmg(minions(), time, StatCritOptions::default())
            .dps,
    );
    let mut at = |spool: f64| {
        let weapon = s.get_stat_dmg(weapons(), burst_at(spool), StatCritOptions::default());
        let all = s.get_stat_dmg(
            StatDmgItemKinds::default(),
            burst_at(spool),
            StatCritOptions::default(),
        );
        (total(&weapon.dps), total(&all.volley), total(&all.dps))
    };
    let (pre, full) = (at(0.0), at(1.0));
    FirepowerDetail {
        drone_dps,
        weapon_dps: SpoolRange {
            pre: pre.0,
            full: full.0,
        },
        volley: SpoolRange {
            pre: pre.1,
            full: full.1,
        },
        dps: SpoolRange {
            pre: pre.2,
            full: full.2,
        },
    }
}

fn total(d: &rc::stats::StatDmgEntry) -> f64 {
    (d.em + d.thermal + d.kinetic + d.explosive).into_f64()
}

/// Shield, armor and hull repair the fit does on itself, HP/s.
fn local_reps(f: &mut rc::Fit<'_>, time: StatTimeOptions) -> [f64; 3] {
    f.stats()
        .get_stat_rps(time, UnitInterval::from_f64_clamped(PEAK_SHIELD))
        .map_or([0.0; 3], |r| {
            [r.shield.local, r.armor.local, r.hull.local].map(PValue::into_f64)
        })
}

fn tank(f: &mut rc::Fit<'_>, time: StatTimeOptions) -> Tank {
    let Ok(r) = f
        .stats()
        .get_stat_rps(time, UnitInterval::from_f64_clamped(PEAK_SHIELD))
    else {
        return Tank::default();
    };
    let armor = (r.armor.local + r.armor.remote).into_f64();
    let armor_at = |f: &mut rc::Fit<'_>, spool| {
        f.stats()
            .get_stat_rps(burst_at(spool), UnitInterval::from_f64_clamped(PEAK_SHIELD))
            .map_or(armor, |r| (r.armor.local + r.armor.remote).into_f64())
    };
    Tank {
        passive_shield: r.shield.regen.into_f64(),
        shield: (r.shield.local + r.shield.remote).into_f64(),
        armor,
        hull: (r.hull.local + r.hull.remote).into_f64(),
        armor_spool: SpoolRange {
            pre: armor_at(f, 0.0),
            full: armor_at(f, 1.0),
        },
    }
}

/// Gathers the pane-only numbers. `cap_users` are the active modules that use capacitor, with
/// their GJ/s, which the sustained tank needs.
pub(super) fn pane_stats(
    sol: &mut SolOwned,
    fit_id: FitId,
    ship_id: ItemId,
    cap_users: &[(ItemId, f64)],
    options: CalcOptions,
    stats: &mut Stats,
) {
    let time = options.time();
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return;
    };
    {
        let mut s = f.stats();
        stats.fighter_tubes = super::slots(s.get_stat_launched_fighters());
        stats.fighter_bay = super::resource(s.get_stat_fighter_bay_volume());

        let reps =
            s.get_stat_outgoing_rps(StatOutRepItemKinds::default(), StatTimeOptions::default());
        let reps_at = |s: &mut rc::stats::FitStats<'_>, spool| {
            let r = s.get_stat_outgoing_rps(StatOutRepItemKinds::default(), burst_at(spool));
            [r.shield, r.armor, r.hull].map(PValue::into_f64)
        };
        let (pre, full) = (reps_at(&mut s, 0.0), reps_at(&mut s, 1.0));
        stats.outgoing = Outgoing {
            reps: [reps.shield, reps.armor, reps.hull].map(PValue::into_f64),
            reps_spool: [0, 1, 2].map(|i| SpoolRange {
                pre: pre[i],
                full: full[i],
            }),
            cap: s
                .get_stat_outgoing_cps(StatTimeOptions::default())
                .into_f64(),
            neut: s
                .get_stat_outgoing_nps(StatNeutItemKinds::default(), StatTimeOptions::default())
                .into_f64(),
        };

        let mining = |s: &mut rc::stats::FitStats<'_>, kinds| {
            let m: StatMining = s.get_stat_mps(kinds, time, StatMiningResourceKind::Regular);
            MiningRate {
                yield_: (m.ore.yield_ + m.ice.yield_ + m.gas.yield_).into_f64(),
                drain: (m.ore.drain + m.ice.drain + m.gas.drain).into_f64(),
            }
        };
        stats.mining = Mining {
            miner: mining(&mut s, StatMiningItemKinds::new(false).with_module(true)),
            drone: mining(&mut s, StatMiningItemKinds::new(false).with_minion(true)),
        };

        let balance = |s: &mut rc::stats::FitStats<'_>, kinds| {
            s.get_stat_cap_balance(kinds, StatTimeOptions::default())
                .map_or(0.0, rc::Value::into_f64)
        };
        // Cap injectors stay out, like pyfa's delta, which only counts what flows steadily.
        let recharge = balance(
            &mut s,
            StatCapBlcSrcKinds::new(false)
                .with_regen(OptionExt::Enabled)
                .with_nosfs(OptionExt::Enabled)
                .with_incoming_transfers(true),
        );
        let used = -balance(
            &mut s,
            StatCapBlcSrcKinds::new(false)
                .with_consumers(true)
                .with_incoming_neuts(true),
        );
        stats.cap_detail = CapDetail {
            recharge,
            used,
            neut_resist: s.get_stat_neut_resist().map_or(0.0, UnitInterval::into_f64),
            recharge_time: None,
        };

        stats.misc = Misc {
            drone_control_range: s.get_stat_drone_control_range().ok().map(PValue::into_f64),
            jam_chance: s
                .get_stat_incoming_jam(time)
                .map_or(0.0, |j| j.chance.into_f64()),
            sensor_kind: s.get_stat_sensors().ok().map(|v| match v.kind {
                StatSensorsKind::Radar => "Radar",
                StatSensorsKind::Ladar => "Ladar",
                StatSensorsKind::Magnetometric => "Magnetometric",
                StatSensorsKind::Gravimetric => "Gravimetric",
            }),
            max_warp_range: s.get_stat_max_warp_range().ok().map(PValue::into_f64),
            warp_core_strength: 0.0,
            mass: s
                .get_stat_mass(CtlAffectors::Unmodified)
                .ok()
                .map(PValue::into_f64),
            agility: s.get_stat_agility().ok().map(PValue::into_f64),
            probe_size: s.get_stat_probing_size().ok().map(PValue::into_f64),
            holds: Vec::new(),
        };
    }

    stats.tank = tank(&mut f, StatTimeOptions::default());
    let cap_stable = matches!(stats.cap, Some(Cap::Stable(_)));
    stats.sustained = if cap_stable && !options.factor_reload {
        stats.tank
    } else {
        let base = tank(&mut f, time);
        sustained(sol, fit_id, base, cap_users, time, stats.cap_detail)
    };

    let Ok(mut ship) = sol.get_item(&ship_id) else {
        return;
    };
    let mut get = |attr: i32| {
        ship.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
            .ok()
            .map(|v| v.modified.into_f64())
    };
    stats.cap_detail.recharge_time = get(attr::RECHARGE_RATE).map(|ms| ms / 1000.0);
    // Stabilizers lower the status; strength is its negation, and 0 should not print as -0.
    stats.misc.warp_core_strength = 0.0 - get(attr::WARP_SCRAMBLE_STATUS).unwrap_or(0.0);
    stats.misc.holds = HOLDS
        .iter()
        .filter_map(|&(attr, name)| get(attr).filter(|&v| v > 0.0).map(|v| (name, v)))
        .collect();
}

/// pyfa's sustainable tank: capacitor-using repairers only count for the share of their cycles
/// the capacitor can pay for at peak recharge, most efficient repairer first.
fn sustained(
    sol: &mut SolOwned,
    fit_id: FitId,
    base: Tank,
    cap_users: &[(ItemId, f64)],
    time: StatTimeOptions,
    cap: CapDetail,
) -> Tank {
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return base;
    };
    let before = local_reps(&mut f, time);
    let mut repairers: Vec<([f64; 3], f64)> = Vec::new();
    for &(id, cap_per_sec) in cap_users {
        let Ok(mut module) = sol.get_module(&id) else {
            continue;
        };
        let state = module.get_state();
        module.set_state(rc::ModuleState::Online);
        let after = sol
            .get_fit(&fit_id)
            .map_or(before, |mut f| local_reps(&mut f, time));
        if let Ok(mut module) = sol.get_module(&id) {
            module.set_state(state);
        }
        let share = [0, 1, 2].map(|i| before[i] - after[i]);
        if share.iter().any(|&v| v > 1e-9) {
            repairers.push((share, cap_per_sec));
        }
    }
    let mut reps = [base.shield, base.armor, base.hull];
    let mut used = cap.used;
    for (share, cap_per_sec) in &repairers {
        for (rep, s) in reps.iter_mut().zip(share) {
            *rep -= s;
        }
        used -= cap_per_sec;
    }
    repairers.sort_by(|a, b| {
        let efficiency = |(share, cap): &([f64; 3], f64)| share.iter().sum::<f64>() / cap;
        efficiency(b).total_cmp(&efficiency(a))
    });
    for (share, cap_per_sec) in repairers {
        if used > cap.recharge {
            break;
        }
        let sustainability = ((cap.recharge - used) / cap_per_sec).min(1.0);
        for (rep, s) in reps.iter_mut().zip(share) {
            *rep += sustainability * s;
        }
        used += cap_per_sec;
    }
    let armor_change = reps[1] - base.armor;
    Tank {
        shield: reps[0],
        armor: reps[1],
        hull: reps[2],
        armor_spool: SpoolRange {
            pre: base.armor_spool.pre + armor_change,
            full: base.armor_spool.full + armor_change,
        },
        ..base
    }
}
