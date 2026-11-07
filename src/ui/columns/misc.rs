//! pyfa's Miscellanea column for modules and subsystems (gui/builtinViewColumns/misc.py): one
//! short figure per kind of module, with a tooltip saying what it is.

use crate::{
    data::{Catalog, TypeId, category},
    engine::{ModuleMisc, misc_attr as a},
    ui::stats::format::{amount, capitalize, format_list, signed},
};

/// Module groups the column knows, by their ID; the comments give pyfa's names.
mod group {
    pub const SUPER_WEAPON: i32 = 588;
    pub const STRUCTURE_DOOMSDAY: i32 = 1333;
    pub const ENERGY_WEAPON: i32 = 53;
    pub const HYBRID_WEAPON: i32 = 74;
    pub const PROJECTILE_WEAPON: i32 = 55;
    pub const PRECURSOR_WEAPON: i32 = 1986;
    pub const VORTON_PROJECTOR: i32 = 4060;
    pub const ENERGY_NEUTRALIZER: i32 = 71;
    pub const STRUCTURE_NEUTRALIZER: i32 = 1329;
    pub const STRUCTURE_BURST_PROJECTOR: i32 = 1331;
    pub const BURST_PROJECTORS: i32 = 842;
    pub const ENERGY_NOSFERATU: i32 = 68;
    pub const SALVAGER: i32 = 1122;
    pub const DATA_MINERS: i32 = 538;
    pub const DAMAGE_CONTROL: i32 = 60;
    pub const WARP_SCRAMBLER: i32 = 52;
    pub const WARP_CORE_STABILIZER: i32 = 315;
    pub const STRUCTURE_WARP_SCRAMBLER: i32 = 1442;
    pub const STASIS_WEB: i32 = 65;
    pub const STASIS_GRAPPLER: i32 = 1672;
    pub const STRUCTURE_STASIS_WEBIFIER: i32 = 1441;
    pub const TARGET_PAINTER: i32 = 379;
    pub const STRUCTURE_DISRUPTION_BATTERY: i32 = 1719;
    pub const SENSOR_DAMPENER: i32 = 208;
    pub const WEAPON_DISRUPTOR: i32 = 291;
    pub const GYROSTABILIZER: i32 = 59;
    pub const MAGNETIC_FIELD_STABILIZER: i32 = 302;
    pub const HEAT_SINK: i32 = 205;
    pub const BALLISTIC_CONTROL_SYSTEM: i32 = 367;
    pub const STRUCTURE_WEAPON_UPGRADE: i32 = 1429;
    pub const ENTROPIC_RADIATION_SINK: i32 = 1988;
    pub const VORTON_PROJECTOR_UPGRADE: i32 = 4067;
    pub const DRONE_DAMAGE_MODULES: i32 = 645;
    pub const ECM: i32 = 201;
    pub const BURST_JAMMER: i32 = 80;
    pub const STRUCTURE_ECM_BATTERY: i32 = 1332;
    pub const REMOTE_SENSOR_BOOSTER: i32 = 290;
    pub const SENSOR_BOOSTER: i32 = 212;
    pub const SIGNAL_AMPLIFIER: i32 = 210;
    pub const STRUCTURE_SIGNAL_AMPLIFIER: i32 = 1535;
    pub const PROJECTED_ECCM: i32 = 289;
    pub const ECCM: i32 = 202;
    pub const SENSOR_BACKUP_ARRAY: i32 = 203;
    pub const CLOAKING_DEVICE: i32 = 330;
    pub const REMOTE_ARMOR_REPAIRER: i32 = 325;
    pub const MUTADAPTIVE_REMOTE_ARMOR_REPAIRER: i32 = 2018;
    pub const REMOTE_SHIELD_BOOSTER: i32 = 41;
    pub const REMOTE_CAPACITOR_TRANSMITTER: i32 = 67;
    pub const REMOTE_HULL_REPAIRER: i32 = 585;
    pub const GANG_COORDINATOR: i32 = 316;
    pub const FREQUENCY_MINING_LASER: i32 = 483;
    pub const STRIP_MINER: i32 = 464;
    pub const MINING_LASER: i32 = 54;
    pub const GAS_CLOUD_SCOOPS: i32 = 737;
    pub const GAS_CLOUD_HARVESTERS: i32 = 4138;
    pub const MICRO_JUMP_DRIVE: i32 = 1189;
    pub const MICRO_JUMP_FIELD_GENERATORS: i32 = 1533;
    pub const CAPITAL_MOBILITY_MODULES: i32 = 4769;
    pub const SIEGE_MODULE: i32 = 515;
    pub const CYNOSURAL_FIELD_GENERATOR: i32 = 658;
    pub const ANCILLARY_ARMOR_REPAIRER: i32 = 1199;
    pub const ANCILLARY_SHIELD_BOOSTER: i32 = 1156;
    pub const CAPACITOR_BOOSTER: i32 = 76;
    pub const ANCILLARY_REMOTE_ARMOR_REPAIRER: i32 = 1698;
    pub const ANCILLARY_REMOTE_SHIELD_BOOSTER: i32 = 1697;
    pub const ARMOR_RESISTANCE_SHIFT_HARDENER: i32 = 1150;
    pub const CARGO_SCANNER: i32 = 47;
    pub const SHIP_SCANNER: i32 = 48;
    pub const COMMAND_BURST: i32 = 1770;
    pub const BOMB: i32 = 90;
    pub const GUIDED_BOMB: i32 = 1548;
    pub const SCANNER_PROBE: i32 = 479;
    pub const SCARAB_BREACHER_PODS: i32 = 4808;
}

/// Effects that decide what a burst projector or disruption battery does.
mod effect {
    pub const DOOMSDAY_AOE_WEB: i32 = 6476;
    pub const DOOMSDAY_AOE_NEUT: i32 = 6477;
    pub const DOOMSDAY_AOE_PAINT: i32 = 6478;
    pub const DOOMSDAY_AOE_TRACK: i32 = 6479;
    pub const DOOMSDAY_AOE_DAMP: i32 = 6481;
    pub const DOOMSDAY_AOE_ECM: i32 = 6513;
    pub const STRUCTURE_TARGET_PAINTER: i32 = 6683;
    pub const STRUCTURE_SENSOR_DAMPENER: i32 = 6684;
}

/// Effective HP per raw HP of the fit's shield and armor, when the resistances section shows
/// effective HP: ancillary repairers then count what they restore in EHP, as pyfa does.
#[derive(Debug, Clone, Copy)]
pub struct EhpRatios {
    pub shield: f64,
    pub armor: f64,
}

/// pyfa's formatAmount(val, 3, 0, 3): three digits, k as the biggest suffix.
fn short(val: f64) -> String {
    amount(val, 3, 0, 3)
}

fn short_signed(val: f64) -> String {
    signed(val, 3, 0, 3)
}

/// The biggest bonus by size and the names of every bonus that size, pyfa's way of picking
/// what an electronic warfare module shows.
fn strongest<'n>(bonuses: &[(Option<f64>, &'n str)]) -> Option<(f64, Vec<&'n str>)> {
    let display = bonuses
        .iter()
        .filter_map(|&(v, _)| v)
        .fold(
            0.0_f64,
            |best, v| if v.abs() > best.abs() { v } else { best },
        );
    if display == 0.0 {
        return None;
    }
    let names = bonuses
        .iter()
        .filter(|&&(v, _)| v.is_some_and(|v| (v.abs() - display.abs()).abs() < f64::EPSILON))
        .map(|&(_, name)| name)
        .collect();
    Some((display, names))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "shaped like the column's answer, so every match arm reads alike"
)]
fn text(text: String, tooltip: impl Into<String>) -> Option<(String, String)> {
    Some((text, tooltip.into()))
}

/// What the column shows for a fitted module or subsystem, and its tooltip.
pub fn module_misc(
    catalog: &Catalog,
    type_id: TypeId,
    charge: Option<TypeId>,
    m: &ModuleMisc,
    ehp: Option<EhpRatios>,
) -> Option<(String, String)> {
    use group as g;

    let t = catalog.get(type_id)?;
    let attr = |id| m.attr(id);
    let nonzero = |id| m.attr(id).filter(|&v| v != 0.0);
    let cycle_ms = nonzero(a::DURATION).or_else(|| nonzero(a::SPEED));
    let effect_in = |effect| m.has_effect(effect);
    let burst = matches!(
        t.group_id,
        g::STRUCTURE_BURST_PROJECTOR | g::BURST_PROJECTORS
    );
    let percent = |v: f64| format!("{}%", short(v));
    let percent_signed = |v: f64| format!("{}%", short_signed(v));

    if t.category_id == category::SUBSYSTEM {
        let parts: Vec<String> = [
            (a::HI_SLOT_MODIFIER, 'H'),
            (a::MED_SLOT_MODIFIER, 'M'),
            (a::LOW_SLOT_MODIFIER, 'L'),
        ]
        .into_iter()
        .filter_map(|(id, letter)| {
            let n = attr(id).unwrap_or(0.0) as i64;
            (n > 0).then(|| format!("{n}{letter}"))
        })
        .collect();
        return text(format!("+ {}", parts.join(", ")), "Slot Modifiers");
    }

    match t.group_id {
        g::SUPER_WEAPON | g::STRUCTURE_DOOMSDAY => (m.volley > 0.0).then(|| {
            (
                format!("{} dmg", amount(m.volley, 3, 0, 6)),
                "Raw damage done".to_owned(),
            )
        }),
        g::ENERGY_WEAPON | g::HYBRID_WEAPON | g::PROJECTILE_WEAPON => {
            let tracking = nonzero(a::TRACKING_SPEED)?;
            let sig = nonzero(a::OPTIMAL_SIG_RADIUS)?;
            text(short(tracking * 40_000.0 / sig), "Tracking speed")
        }
        g::PRECURSOR_WEAPON => {
            let mut parts = Vec::new();
            if let Some(tracking) = nonzero(a::TRACKING_SPEED) {
                parts.push((short(tracking), "tracking speed"));
            }
            if let Some((time, _)) = m.spool_time.filter(|&(time, _)| time > 0.0) {
                parts.push((format!("{}s", short(time)), "spool up time"));
            }
            if parts.is_empty() {
                return None;
            }
            let (texts, tips): (Vec<String>, Vec<&str>) = parts.into_iter().unzip();
            text(texts.join(" | "), capitalize(&tips.join(" and ")))
        }
        g::VORTON_PROJECTOR => {
            let cloud = nonzero(a::AOE_CLOUD_SIZE)?;
            let velocity = nonzero(a::AOE_VELOCITY)?;
            text(
                format!("{}m | {}m/s", short(cloud), short(velocity)),
                "Explosion radius and explosion velocity",
            )
        }
        g::ENERGY_NEUTRALIZER | g::STRUCTURE_NEUTRALIZER => neut(m, a::NEUT_AMOUNT, cycle_ms),
        _ if burst && effect_in(effect::DOOMSDAY_AOE_NEUT) => neut(m, a::NEUT_AMOUNT, cycle_ms),
        g::ENERGY_NOSFERATU => neut(m, a::NOS_AMOUNT, cycle_ms),
        g::SALVAGER => text(
            percent(nonzero(a::ACCESS_DIFFICULTY_BONUS)?),
            "Item retrieval chance",
        ),
        g::DATA_MINERS => {
            let strength = nonzero(a::VIRUS_STRENGTH)?;
            let coherence = nonzero(a::VIRUS_COHERENCE)?;
            text(
                format!("{} | {}", short(strength), short(coherence)),
                "Virus strength and coherence",
            )
        }
        g::DAMAGE_CONTROL => {
            let duration = nonzero(a::DURATION)?;
            text(
                format!("{}s", amount(duration / 1000.0, 3, 0, 0)),
                "Assault ability duration",
            )
        }
        g::WARP_SCRAMBLER | g::WARP_CORE_STABILIZER | g::STRUCTURE_WARP_SCRAMBLER => text(
            short_signed(-nonzero(a::WARP_SCRAMBLE_STRENGTH)?),
            "Warp core strength modification",
        ),
        g::STASIS_WEB | g::STASIS_GRAPPLER | g::STRUCTURE_STASIS_WEBIFIER => {
            text(percent(nonzero(a::SPEED_FACTOR)?), "Speed reduction")
        }
        _ if burst && effect_in(effect::DOOMSDAY_AOE_WEB) => {
            text(percent(nonzero(a::SPEED_FACTOR)?), "Speed reduction")
        }
        g::TARGET_PAINTER => painter(m),
        g::STRUCTURE_DISRUPTION_BATTERY if effect_in(effect::STRUCTURE_TARGET_PAINTER) => {
            painter(m)
        }
        _ if burst && effect_in(effect::DOOMSDAY_AOE_PAINT) => painter(m),
        g::SENSOR_DAMPENER => dampener(m),
        g::STRUCTURE_DISRUPTION_BATTERY if effect_in(effect::STRUCTURE_SENSOR_DAMPENER) => {
            dampener(m)
        }
        _ if burst && effect_in(effect::DOOMSDAY_AOE_DAMP) => dampener(m),
        g::WEAPON_DISRUPTOR | g::STRUCTURE_DISRUPTION_BATTERY => disruptor(m),
        _ if burst && effect_in(effect::DOOMSDAY_AOE_TRACK) => disruptor(m),
        g::GYROSTABILIZER
        | g::MAGNETIC_FIELD_STABILIZER
        | g::HEAT_SINK
        | g::BALLISTIC_CONTROL_SYSTEM
        | g::STRUCTURE_WEAPON_UPGRADE
        | g::ENTROPIC_RADIATION_SINK
        | g::VORTON_PROJECTOR_UPGRADE => {
            let (damage, weapon) = match t.group_id {
                g::GYROSTABILIZER => (a::DAMAGE_MULTIPLIER, "Projectile weapon"),
                g::MAGNETIC_FIELD_STABILIZER => (a::DAMAGE_MULTIPLIER, "Hybrid weapon"),
                g::HEAT_SINK => (a::DAMAGE_MULTIPLIER, "Energy weapon"),
                g::ENTROPIC_RADIATION_SINK => (a::DAMAGE_MULTIPLIER, "Precursor weapon"),
                g::VORTON_PROJECTOR_UPGRADE => (a::DAMAGE_MULTIPLIER, "Vorton projector"),
                _ => (a::MISSILE_DAMAGE_MULTIPLIER_BONUS, "Missile"),
            };
            let damage = nonzero(damage)?;
            let rate = nonzero(a::SPEED_MULTIPLIER)?;
            let mut texts = vec![percent_signed((damage / rate - 1.0) * 100.0)];
            let mut tips = vec![format!("{weapon} DPS boost")];
            if let Some(drone) = nonzero(a::DRONE_DAMAGE_BONUS) {
                texts.push(percent_signed(drone));
                tips.push("drone DPS boost".to_owned());
            }
            text(texts.join(" | "), tips.join(" and "))
        }
        g::DRONE_DAMAGE_MODULES => text(
            percent_signed(nonzero(a::DRONE_DAMAGE_BONUS)?),
            "Drone DPS boost",
        ),
        g::ECM | g::BURST_JAMMER | g::STRUCTURE_ECM_BATTERY => jammer(m),
        _ if burst && effect_in(effect::DOOMSDAY_AOE_ECM) => jammer(m),
        g::REMOTE_SENSOR_BOOSTER
        | g::SENSOR_BOOSTER
        | g::SIGNAL_AMPLIFIER
        | g::STRUCTURE_SIGNAL_AMPLIFIER => {
            let mut texts = Vec::new();
            let mut tips = Vec::new();
            for (id, what) in [
                (a::SCAN_RESOLUTION_BONUS, "scan resolution"),
                (a::MAX_TARGET_RANGE_BONUS, "lock range"),
                (a::SCAN_GRAVIMETRIC_PERCENT, "sensor strength"),
            ] {
                if let Some(bonus) = nonzero(id) {
                    texts.push(percent(bonus));
                    tips.push(format!("{} {what}", percent(bonus)));
                }
            }
            if texts.is_empty() {
                return None;
            }
            text(
                texts.join(" | "),
                format!("Applied bonuses:\n{}", tips.join(" | ")),
            )
        }
        g::PROJECTED_ECCM | g::ECCM | g::SENSOR_BACKUP_ARRAY => {
            let (display, names) = strongest(&[
                (attr(a::SCAN_GRAVIMETRIC_PERCENT), "gravimetric"),
                (attr(a::SCAN_LADAR_PERCENT), "ladar"),
                (attr(a::SCAN_MAGNETOMETRIC_PERCENT), "magnetometric"),
                (attr(a::SCAN_RADAR_PERCENT), "radar"),
            ])?;
            let plural = if names.len() == 1 { "" } else { "s" };
            text(
                percent_signed(display),
                capitalize(&format!("{} strength{plural} bonus", format_list(&names))),
            )
        }
        g::CLOAKING_DEVICE => text(
            format!("{}s", short(attr(a::CLOAKING_TARGETING_DELAY)? / 1000.0)),
            "Sensor recalibration time",
        ),
        g::REMOTE_ARMOR_REPAIRER => reps(m.reps[2], "Armor repaired per second"),
        g::MUTADAPTIVE_REMOTE_ARMOR_REPAIRER => {
            let rps = m.reps[2];
            if rps == 0.0 {
                return None;
            }
            let mut texts = vec![format!("{}/s", short_signed(rps))];
            let mut tips = vec!["Armor repaired per second"];
            let (spool, full_spool) = m.spool_time.unwrap_or_default();
            if spool > 0.0 {
                texts.push(format!("{}s", short(spool)));
                tips.push("spool up time");
            }
            let mut tooltip = tips.join(" and ");
            if let Some((pre, full)) = m.reps_spool
                && full_spool > 0.0
            {
                tooltip = format!(
                    "{tooltip}\nSpool up: {}-{} over {}s",
                    short(pre),
                    short(full),
                    short(full_spool)
                );
            }
            text(texts.join(" | "), tooltip)
        }
        g::REMOTE_SHIELD_BOOSTER => reps(m.reps[1], "Shield transferred per second"),
        g::REMOTE_CAPACITOR_TRANSMITTER => reps(m.reps[0], "Energy transferred per second"),
        g::REMOTE_HULL_REPAIRER => reps(m.reps[3], "Structure repaired per second"),
        g::GANG_COORDINATOR => {
            let bonus = nonzero(a::COMMAND_BONUS).or_else(|| nonzero(a::COMMAND_BONUS_HIDDEN))?;
            text(percent_signed(bonus), "Gang bonus strength")
        }
        g::FREQUENCY_MINING_LASER
        | g::STRIP_MINER
        | g::MINING_LASER
        | g::GAS_CLOUD_SCOOPS
        | g::GAS_CLOUD_HARVESTERS => mining(m),
        g::MICRO_JUMP_DRIVE | g::MICRO_JUMP_FIELD_GENERATORS | g::CAPITAL_MOBILITY_MODULES => text(
            format!("{}s", short(attr(a::DURATION)? / 1000.0)),
            "Spoolup time",
        ),
        g::SIEGE_MODULE | g::CYNOSURAL_FIELD_GENERATOR => {
            let units = nonzero(a::CONSUMPTION_QUANTITY)?;
            let fuel =
                attr(a::CONSUMPTION_TYPE).map_or_else(String::new, |id| catalog.name(id as TypeId));
            text(format!("{} units", short(units)), fuel)
        }
        g::ANCILLARY_ARMOR_REPAIRER
        | g::ANCILLARY_SHIELD_BOOSTER
        | g::CAPACITOR_BOOSTER
        | g::ANCILLARY_REMOTE_ARMOR_REPAIRER
        | g::ANCILLARY_REMOTE_SHIELD_BOOSTER => ancillary(t.group_id, m, ehp),
        g::ARMOR_RESISTANCE_SHIFT_HARDENER => {
            let resist = |id| (1.0 - attr(id).unwrap_or(1.0)) * 100.0;
            let [em, thermal, kinetic, explosive] = [
                a::ARMOR_EM_RESONANCE,
                a::ARMOR_THERMAL_RESONANCE,
                a::ARMOR_KINETIC_RESONANCE,
                a::ARMOR_EXPLOSIVE_RESONANCE,
            ]
            .map(|id| short(resist(id)));
            text(
                format!("{em}% | {thermal}% | {kinetic}% | {explosive}%"),
                format!(
                    "Resistances shifted to damage profile:\n{em}% EM | {thermal}% Therm | \
                     {kinetic}% Kin | {explosive}% Exp"
                ),
            )
        }
        g::CARGO_SCANNER | g::SHIP_SCANNER => text(
            format!("{}s", amount(nonzero(a::DURATION)? / 1000.0, 3, 0, 0)),
            "Scan duration",
        ),
        g::COMMAND_BURST => command_burst(m),
        _ => charge.and_then(|charge| charge_misc(catalog, charge, m)),
    }
}

fn neut(m: &ModuleMisc, amount_attr: i32, cycle_ms: Option<f64>) -> Option<(String, String)> {
    let drained = m.attr(amount_attr).filter(|&v| v != 0.0)?;
    let cycle = cycle_ms?;
    text(
        format!("{}/s", short(-drained * 1000.0 / cycle)),
        "Energy neutralization per second",
    )
}

fn reps(rps: f64, what: &str) -> Option<(String, String)> {
    (rps != 0.0).then(|| (format!("{}/s", short_signed(rps)), what.to_owned()))
}

fn painter(m: &ModuleMisc) -> Option<(String, String)> {
    let bonus = m.attr(a::SIG_RADIUS_BONUS).filter(|&v| v != 0.0)?;
    text(
        format!("{}%", short_signed(bonus)),
        "Signature radius increase",
    )
}

fn dampener(m: &ModuleMisc) -> Option<(String, String)> {
    let lock = m.attr(a::MAX_TARGET_RANGE_BONUS)?;
    let scan = m.attr(a::SCAN_RESOLUTION_BONUS)?;
    let (display, names) =
        strongest(&[(Some(lock), "lock range"), (Some(scan), "scan resolution")])?;
    text(
        format!("{}%", short_signed(display)),
        capitalize(&format!("{} dampening", format_list(&names))),
    )
}

/// Weapon disruptors cover turrets and missiles alike; each kind they disrupt gets its strongest
/// bonus.
fn disruptor(m: &ModuleMisc) -> Option<(String, String)> {
    let tracking = [
        (m.attr(a::MAX_RANGE_BONUS), "optimal range"),
        (m.attr(a::FALLOFF_BONUS), "falloff range"),
        (m.attr(a::TRACKING_SPEED_BONUS), "tracking speed"),
    ];
    let guidance = [
        (m.attr(a::AOE_VELOCITY_BONUS), "explosion velocity"),
        (m.attr(a::AOE_CLOUD_SIZE_BONUS), "explosion radius"),
        (m.attr(a::EXPLOSION_DELAY_BONUS), "flight time"),
        (m.attr(a::MISSILE_VELOCITY_BONUS), "missile velocity"),
    ];
    let mut texts = Vec::new();
    let mut tips = Vec::new();
    for bonuses in [&tracking[..], &guidance[..]] {
        if let Some((display, names)) = strongest(bonuses) {
            texts.push(format!("{}%", short_signed(display)));
            tips.push(capitalize(&format!("{} disruption", format_list(&names))));
        }
    }
    if texts.is_empty() {
        return None;
    }
    text(texts.join(" | "), tips.join("\n"))
}

fn jammer(m: &ModuleMisc) -> Option<(String, String)> {
    let grav = m.attr(a::SCAN_GRAVIMETRIC_BONUS)?;
    let ladar = m.attr(a::SCAN_LADAR_BONUS)?;
    let radar = m.attr(a::SCAN_RADAR_BONUS)?;
    let magnet = m.attr(a::SCAN_MAGNETOMETRIC_BONUS)?;
    let all = [grav, ladar, radar, magnet];
    let max = all.into_iter().fold(f64::MIN, f64::max);
    let min = all.into_iter().fold(f64::MAX, f64::min);
    let shown = if (max - min).abs() < f64::EPSILON {
        short(max)
    } else {
        format!("{} | {}", short(max), short(min))
    };
    text(
        shown,
        format!(
            "ECM Jammer Strength:\n{} Gravimetric | {} Ladar | {} Magnetometric | {} Radar",
            short(grav),
            short(ladar),
            short(magnet),
            short(radar)
        ),
    )
}

fn mining(m: &ModuleMisc) -> Option<(String, String)> {
    let (yield_, drain) = m.mining;
    if yield_ <= 0.0 {
        return None;
    }
    let mut texts = vec![format!("{} m³/s", short(yield_))];
    let mut lines = vec![
        format!(
            "{} m³ mining yield per second ({} m³ per hour)",
            short(yield_),
            short(yield_ * 3600.0)
        ),
        format!(
            "{} m³ mining drain per second ({} m³ per hour)",
            short(drain),
            short(drain * 3600.0)
        ),
    ];
    let efficiency = if drain > 0.0 { yield_ / drain } else { 0.0 };
    if (efficiency - 1.0).abs() > 1e-9 {
        let shown = format!("{}%", amount(efficiency * 100.0, 4, 0, 0));
        lines.push(format!("{shown} mining efficiency"));
        texts.push(shown);
    }
    text(texts.join(" | "), lines.join("\n"))
}

/// Ancillary repairers and capacitor boosters: what one load of charges restores, how long it
/// takes and the reload after.
fn ancillary(group_id: i32, m: &ModuleMisc, ehp: Option<EhpRatios>) -> Option<(String, String)> {
    use group as g;

    let remote = matches!(
        group_id,
        g::ANCILLARY_REMOTE_ARMOR_REPAIRER | g::ANCILLARY_REMOTE_SHIELD_BOOSTER
    );
    let capacitor = group_id == g::CAPACITOR_BOOSTER;
    let cycles = m.charged_cycles.unwrap_or(0);
    if cycles == 0 && remote {
        // Remote ones run without charges too, at the plain rate.
        return if group_id == g::ANCILLARY_REMOTE_ARMOR_REPAIRER {
            reps(m.reps[2], "Armor repaired per second")
        } else {
            reps(m.reps[1], "Shield transferred per second")
        };
    }
    let cycle_ms = m.attr(a::DURATION).filter(|&d| d > 0.0)?;
    if cycles == 0 {
        return None;
    }
    let cycles = f64::from(cycles);
    let per_cycle = [
        m.attr(a::ARMOR_DAMAGE_AMOUNT),
        m.charge_attr(a::CAPACITOR_BONUS),
        m.attr(a::SHIELD_BONUS),
    ]
    .into_iter()
    .flatten()
    .fold(0.0_f64, f64::max);
    let restored = per_cycle * cycles;
    if restored <= 0.0 {
        return None;
    }
    let reload = if capacitor {
        10.0
    } else {
        m.attr(a::RELOAD_TIME).unwrap_or(0.0) / 1000.0
    };
    let what = if capacitor { "Cap" } else { "HP" };
    let mut tooltip = format!("{what} restored over duration using charges (plus reload)");
    // The paste bonus is already in the repair amount after refine's bonuses.
    let ratio = match ehp {
        Some(ratios) if !capacitor && !remote => {
            tooltip = format!("E{tooltip}");
            if group_id == g::ANCILLARY_ARMOR_REPAIRER {
                ratios.armor
            } else {
                ratios.shield
            }
        }
        _ => 1.0,
    };
    let duration = cycles * cycle_ms / 1000.0;
    for loads in [5.0, 10.0, 25.0] {
        tooltip = format!(
            "{tooltip}\n{} charges lasts {} seconds ({} cycles)",
            short(loads * cycles),
            short((duration + reload) * loads),
            short(loads)
        );
    }
    text(
        format!(
            "{} / {}s (+{}s)",
            amount(restored * ratio, 3, 0, 9),
            short(duration),
            short(reload)
        ),
        tooltip,
    )
}

/// Command bursts, by the buffs their charge gives, pyfa's wording per buff.
fn command_burst(m: &ModuleMisc) -> Option<(String, String)> {
    let mut texts = Vec::new();
    let mut tips = Vec::new();
    for (id_attr, value_attr) in a::WARFARE_BUFF_IDS.into_iter().zip(a::WARFARE_BUFF_VALUES) {
        let Some(buff) = m.charge_attr(id_attr).filter(|&b| b != 0.0) else {
            continue;
        };
        let value = m.attr(value_attr).unwrap_or(0.0);
        // Resistance and agility buffs read as the game shows them, the other way round.
        let (flip, what) = match buff as i32 {
            10 => (true, "shield resistance"),
            11 => (false, "shield RR duration & capacictor use"),
            12 => (false, "shield HP"),
            13 => (true, "armor resistance"),
            14 => (false, "armor RR duration & capacitor use"),
            15 => (false, "armor HP"),
            16 => (false, "scan resolution"),
            26 => (false, "targeting range"),
            17 => (false, "electronic warfare modules range & strength"),
            18 => (false, "sensor strength"),
            19 => (true, "sensor dampener & weapon disruption resistance"),
            20 => (false, "signature radius"),
            60 => (true, "agility"),
            21 => (false, "warp disruption & stasis web range"),
            22 => (false, "AB/MWD speed increase"),
            23 => (false, "mining module range"),
            24 => (false, "mining module duration & capacitor use"),
            25 => (false, "mining crystal volatility"),
            2464 => (false, "scan probe strength"),
            2465 => (false, "dscan, hacking & salvaging range"),
            2466 => (false, "scan probe deviation"),
            2468 => {
                texts.push(short_signed(value));
                tips.push("virus coherence");
                continue;
            }
            2481 => (false, "salvager cycle time"),
            2516 => (false, "crit chance"),
            2517 => (false, "waste chance"),
            _ => continue,
        };
        let shown = if flip { -value } else { value };
        texts.push(format!("{}%", short_signed(shown)));
        tips.push(what);
    }
    if texts.is_empty() {
        return None;
    }
    let tooltip = format!("{} bonus", tips.join(" | "));
    text(texts.join(" | "), capitalize(&tooltip))
}

/// What the loaded charge says when the module itself has nothing to show.
fn charge_misc(catalog: &Catalog, charge: TypeId, m: &ModuleMisc) -> Option<(String, String)> {
    use group as g;

    let charge_group = catalog.get(charge)?.group_id;
    let name = catalog
        .group_names
        .get(&charge_group)
        .map_or("", String::as_str);
    let get = |id| m.charge_attr(id).filter(|&v| v != 0.0);
    if name.ends_with("Rocket") || name.ends_with("Missile") || name.ends_with("Torpedo") {
        let cloud = get(a::AOE_CLOUD_SIZE)?;
        let velocity = get(a::AOE_VELOCITY)?;
        return text(
            format!("{}m | {}m/s", short(cloud), short(velocity)),
            "Explosion radius and explosion velocity",
        );
    }
    match charge_group {
        g::BOMB | g::GUIDED_BOMB => text(
            format!("{}m", short(get(a::AOE_CLOUD_SIZE)?)),
            "Explosion radius",
        ),
        g::SCANNER_PROBE => {
            let strength = get(a::BASE_SENSOR_STRENGTH)?;
            let range = get(a::BASE_SCAN_RANGE)?;
            text(
                amount(strength, 4, 0, 3),
                format!("Scan strength at {} AU scan range", amount(range, 3, 0, 0)),
            )
        }
        g::SCARAB_BREACHER_PODS => {
            let duration = m.charge_attr(a::DOT_DURATION)? / 1000.0;
            let absolute = m.charge_attr(a::DOT_MAX_DAMAGE_PER_TICK)?;
            let relative = m.charge_attr(a::DOT_MAX_HP_PERCENTAGE_PER_TICK)?;
            let full_from = if relative > 0.0 {
                absolute / (relative / 100.0)
            } else {
                f64::INFINITY
            };
            text(
                format!(
                    "{}/{}% over {}s",
                    amount(absolute * duration, 3, 0, 6),
                    amount(relative * duration, 3, 0, 6),
                    amount(duration, 0, 0, 0)
                ),
                format!(
                    "Pure damage inflicted over time, minimum of absolute / relative\nFull DPS \
                     from {} target HP",
                    amount(full_from, 3, 0, 6)
                ),
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{
        data::Slot,
        engine::{Engine, FitItem},
        fit::Fit,
    };

    /// pyfa's texts for a few modules of a real fit.
    #[test]
    fn texts_match_pyfa() {
        const DATA: &str = "data/eve";
        if !Path::new(DATA).join("phobos/metadata.json").exists() {
            eprintln!("no EVE data in {DATA}, skipping");
            return;
        }
        let catalog = Catalog::load(Path::new(DATA)).expect("the dump loads");
        let engine = Engine::load(
            Path::new(DATA),
            Path::new("data/cache"),
            catalog.skills.clone(),
        )
        .expect("the engine loads");
        let eft = "[Vexor, Misc]\nDamage Control II\n\nStasis Webifier II\nWarp Scrambler II\n";
        let (fit, _) = Fit::from_eft(&catalog, eft).expect("parses");
        let stats = engine.compute(&fit);
        let text = |slot: Slot, i: usize| {
            let m = fit.rack(slot)[i].as_ref().expect("fitted");
            let misc = &stats.module_misc[&FitItem::Module(slot, i)];
            module_misc(&catalog, m.type_id, m.charge, misc, None)
        };
        assert_eq!(
            text(Slot::Mid, 0),
            Some(("-60%".to_owned(), "Speed reduction".to_owned()))
        );
        assert_eq!(
            text(Slot::Mid, 1),
            Some((
                "-2".to_owned(),
                "Warp core strength modification".to_owned()
            ))
        );
        // Only assault damage controls have a duration; pyfa leaves the cell empty for the rest.
        assert_eq!(text(Slot::Low, 0), None);
    }

    #[test]
    fn strongest_bonus_takes_every_name_of_its_size() {
        let (display, names) =
            strongest(&[(Some(-30.0), "lock range"), (Some(30.0), "scan resolution")]).unwrap();
        assert_eq!(display, -30.0);
        assert_eq!(names, ["lock range", "scan resolution"]);
        assert!(strongest(&[(Some(0.0), "a"), (None, "b")]).is_none());
    }
}
