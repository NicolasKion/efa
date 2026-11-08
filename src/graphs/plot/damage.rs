//! The damage graph: what a fit's weapons apply to a target by distance, time, speed and
//! signature.

use super::{
    DroneMode, MAX_TIME, Request, Series, TargetData, Timeline, change_times, in_range, main_range,
};
use crate::graphs::{
    defs::{Param, Unit},
    math::{self, Motion},
    snapshot::{Cycle, Dmg, Ewar, EwarKind, FitSnapshot, Ship, Weapon, WeaponKind},
};

/// Per weapon: what it deals at the time asked, before application.
fn damage_values(weapons: &[Weapon], y: Param, time: Option<f64>) -> Option<Vec<Dmg>> {
    weapons
        .iter()
        .map(|w| match (y, time) {
            (Param::Dps, None) => Some(w.dps),
            (Param::Volley, None) => Some(w.volley),
            (_, Some(t)) => Some(Timeline::new(&w.cycle, t).value(y, t)),
            // Damage inflicted means nothing without a time.
            _ => None,
        })
        .collect()
}

fn add(a: Dmg, b: Dmg) -> Dmg {
    std::array::from_fn(|i| a[i] + b[i])
}

/// The geometry of one damage sample.
#[derive(Debug, Clone, Copy)]
pub(in crate::graphs) struct Engagement {
    pub(in crate::graphs) distance: Option<f64>,
    pub(super) atk_speed: f64,
    pub(super) atk_angle: f64,
    pub(super) tgt_speed: f64,
    pub(super) tgt_angle: f64,
    pub(super) tgt_sig: f64,
}

/// Where attacker and target are and how they move, from the inputs and vectors.
pub(in crate::graphs) fn engagement(req: &Request, ship: &Ship, tgt: &TargetData) -> Engagement {
    let pct = |v: Option<f64>, of: f64| v.unwrap_or(100.0) / 100.0 * of;
    Engagement {
        distance: req.value(Param::Distance).map(|km| km * 1000.0),
        atk_speed: pct(req.value(Param::AtkSpeed), ship.max_velocity),
        atk_angle: req.value(Param::AtkAngle).unwrap_or(0.0),
        tgt_speed: pct(req.value(Param::TgtSpeed), tgt.max_velocity),
        tgt_angle: req.value(Param::TgtAngle).unwrap_or(0.0),
        tgt_sig: tgt.sig_radius,
    }
}

/// What `weapons` dealing `values` apply to the target under `e`, after resists.
pub(in crate::graphs) fn applied_damage(
    req: &Request,
    src: &FitSnapshot,
    tgt: &TargetData,
    weapons: &[Weapon],
    values: &[Dmg],
    e: Engagement,
) -> f64 {
    let mults = applications(req, src, tgt, weapons, e);
    applied_with(req, tgt, weapons, values, &mults)
}

/// How much of each weapon's damage lands on the target under `e`, the attacker's webs and
/// target painters counted when asked to.
fn applications(
    req: &Request,
    src: &FitSnapshot,
    tgt: &TargetData,
    weapons: &[Weapon],
    e: Engagement,
) -> Vec<f64> {
    let e = if req.options.apply_projected {
        tackle(req, src, tgt, e)
    } else {
        e
    };
    weapons
        .iter()
        .map(|w| application(req, &src.ship, tgt, w, e))
        .collect()
}

/// What `weapons` dealing `values` apply to the target, each landing its share in `mults`,
/// after resists.
fn applied_with(
    req: &Request,
    tgt: &TargetData,
    weapons: &[Weapon],
    values: &[Dmg],
    mults: &[f64],
) -> f64 {
    let mut breach = 0.0;
    let applied = values
        .iter()
        .zip(weapons)
        .zip(mults)
        .fold([0.0; 4], |acc, ((v, w), &mult)| {
            if matches!(w.kind, WeaponKind::Breacher { .. }) {
                // A share of the target's HP up to a cap, past resists.
                let hp = tgt.hp.unwrap_or(f64::INFINITY);
                breach += v[0].min(v[1] * hp) * mult;
                acc
            } else {
                add(acc, v.map(|d| d * mult))
            }
        });
    let resists = if req.options.ignore_resists {
        [0.0; 4]
    } else {
        tgt.resists
    };
    breach + (0..4).map(|i| applied[i] * (1.0 - resists[i])).sum::<f64>()
}

pub(super) fn damage(req: &Request, src: &FitSnapshot, tgt: &TargetData) -> Option<Series> {
    let base = engagement(req, &src.ship, tgt);
    let time = req.value(Param::Time).map(|t| t.clamp(0.0, MAX_TIME));
    let y = req.y.param;
    let value =
        |values: &[Dmg], e: Engagement| applied_damage(req, src, tgt, &src.weapons, values, e);

    let series = match req.x.param {
        Param::Distance => {
            let values = damage_values(&src.weapons, y, time)?;
            let range = main_range(req, 1000.0);
            Series::new(math::smooth(range, 50, 2, |d| {
                value(
                    &values,
                    Engagement {
                        distance: Some(d),
                        ..base
                    },
                )
            }))
            .map_x(|m| m / 1000.0)
        }
        Param::Time => {
            let (low, high) = req.range;
            let (low, high) = (low.clamp(0.0, MAX_TIME), high.clamp(0.0, MAX_TIME));
            let cycles: Vec<&Cycle<Dmg>> = src.weapons.iter().map(|w| &w.cycle).collect();
            let times = change_times(&cycles, high, y == Param::Damage);
            let timelines: Vec<_> = cycles.iter().map(|c| Timeline::new(c, high)).collect();
            // The engagement stays put over time, so each weapon lands the same share throughout.
            let mults = applications(req, src, tgt, &src.weapons, base);
            Series::new(math::steps((low, high), &times, |t| {
                let values: Vec<Dmg> = timelines.iter().map(|tl| tl.value(y, t)).collect();
                applied_with(req, tgt, &src.weapons, &values, &mults)
            }))
        }
        Param::TgtSpeed => {
            let values = damage_values(&src.weapons, y, time)?;
            let range = main_range(req, tgt.max_velocity / 100.0);
            let series = Series::new(math::smooth(range, 50, 2, |speed| {
                value(
                    &values,
                    Engagement {
                        tgt_speed: speed,
                        ..base
                    },
                )
            }));
            percent_axis(req, series, tgt.max_velocity)
        }
        Param::TgtSigRad => {
            let values = damage_values(&src.weapons, y, time)?;
            let range = main_range(req, tgt.sig_radius / 100.0);
            let series = Series::new(math::smooth(range, 50, 2, |sig| {
                value(
                    &values,
                    Engagement {
                        tgt_sig: sig,
                        ..base
                    },
                )
            }));
            percent_axis(req, series, tgt.sig_radius)
        }
        _ => return None,
    };
    Some(series)
}

/// Express an x axis as a percentage of `of`, pyfa's denormalizer. When that cannot be done (a
/// standing target, an infinite signature) and the line is flat anyway, pyfa draws it flat
/// across the input range.
fn percent_axis(req: &Request, series: Series, of: f64) -> Series {
    if req.x.unit != Unit::Percent {
        return series;
    }
    if of > 0.0 && of.is_finite() {
        return series.map_x(|v| v * 100.0 / of);
    }
    let y = series.ys.first().copied().unwrap_or(0.0);
    Series {
        xs: vec![req.range.0.min(req.range.1), req.range.0.max(req.range.1)],
        ys: vec![y, y],
    }
}

/// How much of a weapon's damage lands, pyfa's `getApplicationPerKey` for one weapon.
fn application(req: &Request, ship: &Ship, tgt: &TargetData, w: &Weapon, e: Engagement) -> f64 {
    let opts = &req.options;
    let in_lock = in_range(ship.lock_range, e.distance, opts.ignore_lock_range);
    let in_dcr = in_range(ship.drone_control_range, e.distance, opts.ignore_dcr);
    let lock = |f: f64| if in_lock { f } else { 0.0 };
    match &w.kind {
        WeaponKind::Turret(gun) => {
            let motion = Motion {
                atk_speed: e.atk_speed,
                atk_angle: e.atk_angle,
                atk_radius: ship.radius,
                tgt_speed: e.tgt_speed,
                tgt_angle: e.tgt_angle,
                tgt_radius: tgt.radius,
            };
            lock(math::turret_mult(math::turret_chance_to_hit(
                gun, &motion, e.distance, e.tgt_sig,
            )))
        }
        WeaponKind::Vorton { range, er, ev, drf } => lock(
            math::range_factor(*range, 0.0, e.distance, true)
                * math::missile_factor(*er, *ev, *drf, e.tgt_speed, e.tgt_sig),
        ),
        WeaponKind::Launcher {
            er,
            ev,
            drf,
            range,
            fof,
        } => {
            let Some(range) = range else { return 0.0 };
            if !in_lock && !fof {
                return 0.0;
            }
            range.factor(e.distance) * math::missile_factor(*er, *ev, *drf, e.tgt_speed, e.tgt_sig)
        }
        WeaponKind::Smartbomb { range } => {
            if e.distance.is_some_and(|d| d > *range) {
                0.0
            } else {
                1.0
            }
        }
        WeaponKind::Bomb {
            range,
            blast_radius,
            er,
        } => {
            // Bombs start in the ship's center and hit what their blast touches.
            if let Some(d) = e.distance {
                let near = (range - ship.radius - tgt.radius - blast_radius).max(0.0);
                let far = (range - ship.radius + tgt.radius + blast_radius).max(0.0);
                if d < near || d > far {
                    return 0.0;
                }
            }
            math::bomb_factor(*er, e.tgt_sig)
        }
        WeaponKind::GuidedBomb { range, er } => {
            if e.distance.is_some_and(|d| d > range - ship.radius) {
                return 0.0;
            }
            lock(math::bomb_factor(*er, e.tgt_sig))
        }
        WeaponKind::Doomsday {
            range,
            damage_sig,
            needs_lock,
        } => {
            if *needs_lock && !in_lock {
                return 0.0;
            }
            if e.distance.is_some_and(|d| *range > 0.0 && d > *range) {
                return 0.0;
            }
            if *damage_sig <= 0.0 {
                1.0
            } else {
                (e.tgt_sig / damage_sig).min(1.0)
            }
        }
        WeaponKind::Breacher { range } => {
            let Some(range) = range else { return 0.0 };
            lock(range.factor(e.distance) * tgt.breacher_resist)
        }
        WeaponKind::Fighter {
            optimal,
            falloff,
            er,
            ev,
            drf,
            speed,
            radius,
            plain,
        } => {
            if *plain {
                return lock(1.0);
            }
            let range = if follows(opts.drone_mode, *speed, e.tgt_speed) {
                1.0
            } else {
                // Slower squadrons fire from the attacker's center, as pyfa places them.
                let distance = e.distance.map(|d| d + ship.radius - radius);
                math::range_factor(*optimal, *falloff, distance, true)
            };
            lock(range * math::missile_factor(*er, *ev, *drf, e.tgt_speed, e.tgt_sig))
        }
        WeaponKind::Drone { gun, speed, radius } => {
            if !in_lock || !in_dcr {
                return 0.0;
            }
            if *speed > 1.0 && follows(opts.drone_mode, *speed, e.tgt_speed) {
                // Drones that keep up with the target are assumed to always hit.
                return math::turret_mult(1.0);
            }
            // Otherwise the drone sits in the attacker's center, flying at the lower of its
            // own and the ship's speed, and shoots from there.
            let motion = Motion {
                atk_speed: e.atk_speed.min(*speed),
                atk_angle: e.atk_angle,
                atk_radius: *radius,
                tgt_speed: e.tgt_speed,
                tgt_angle: e.tgt_angle,
                tgt_radius: tgt.radius,
            };
            let distance = e.distance.map(|d| d + ship.radius - radius);
            math::turret_mult(math::turret_chance_to_hit(
                gun, &motion, distance, e.tgt_sig,
            ))
        }
    }
}

/// Whether a drone or fighter of `speed` sticks to the target under `mode`.
fn follows(mode: DroneMode, speed: f64, tgt_speed: f64) -> bool {
    match mode {
        DroneMode::Auto => speed >= tgt_speed,
        DroneMode::FollowTarget => true,
        DroneMode::FollowAttacker => false,
    }
}

/// Apply the attacker's own webs and target painters to the target ("Apply Projected Items").
///
/// pyfa recomputes a target fit's speed with the new multipliers stacked against the
/// penalties already on it; here they stack among themselves only, which matches pyfa for
/// target profiles and for fits nobody else webs or paints.
pub(super) fn tackle(
    req: &Request,
    src: &FitSnapshot,
    tgt: &TargetData,
    e: Engagement,
) -> Engagement {
    let opts = &req.options;
    let ship = &src.ship;
    let in_lock = in_range(ship.lock_range, e.distance, opts.ignore_lock_range);
    let in_dcr = in_range(ship.drone_control_range, e.distance, opts.ignore_dcr);
    if !in_lock {
        return e;
    }
    let applied = |ewar: &Ewar, tgt_speed: f64| -> f64 {
        match ewar.mobile {
            None => {
                ewar.strength * math::range_factor(ewar.optimal, ewar.falloff, e.distance, true)
            }
            Some((speed, radius)) => {
                if follows(opts.drone_mode, speed, tgt_speed) {
                    ewar.strength
                } else {
                    let distance = e.distance.map(|d| d + ship.radius - radius);
                    ewar.strength * math::range_factor(ewar.optimal, ewar.falloff, distance, true)
                }
            }
        }
    };
    let usable = |ewar: &&Ewar| !ewar.needs_dcr || in_dcr;

    let mut out = e;
    // A scram in range shuts the target's microwarpdrive down: it flies and blooms as without.
    let scrammed = src
        .scram_range
        .filter(|&range| e.distance.is_none_or(|d| d <= range))
        .and(tgt.scrammed);
    let max_velocity = scrammed.map_or(tgt.max_velocity, |s| s.0);
    if let Some((_, sig)) = scrammed
        && tgt.sig_radius > 0.0
        && e.tgt_sig.is_finite()
    {
        out.tgt_sig = e.tgt_sig * sig / tgt.sig_radius;
    }
    if tgt.max_velocity > 0.0 {
        let ratio = e.tgt_speed / tgt.max_velocity;
        out.tgt_speed = max_velocity * ratio;
        let mut mults = Vec::new();
        for web in src
            .ewar
            .iter()
            .filter(usable)
            .filter(|w| w.kind == EwarKind::Web)
        {
            mults.push(1.0 + applied(web, out.tgt_speed) / 100.0);
            out.tgt_speed = max_velocity * math::stacked_multiplier(&mults) * ratio;
        }
    }
    let tp_mults: Vec<f64> = src
        .ewar
        .iter()
        .filter(usable)
        .filter(|w| w.kind == EwarKind::TargetPainter)
        .map(|tp| 1.0 + applied(tp, out.tgt_speed) / 100.0)
        .collect();
    if !tp_mults.is_empty() && out.tgt_sig.is_finite() {
        out.tgt_sig *= math::stacked_multiplier(&tp_mults);
    }
    out
}
