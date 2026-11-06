//! Turning a graph request into points: pyfa's getters for every graph, over a
//! [`FitSnapshot`] and an optional target.

use std::collections::HashMap;

use super::{
    capsim,
    defs::{Checkbox, Graph, Param, Unit, XDef, YDef},
    math::{self, Motion},
    snapshot::{Cycle, Dmg, Ewar, EwarKind, FitSnapshot, Layers, Ship, Weapon, WeaponKind},
};

/// pyfa caps damage and repair timelines at this many seconds.
const MAX_TIME: f64 = 2500.0;
/// And capacitor timelines at an hour.
const MAX_CAP_TIME: f64 = 3600.0;

/// pyfa's "mobile drone mode": how drones and fighters chase the target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DroneMode {
    /// Drones faster than the target stick to it; slower ones shoot from the attacker.
    #[default]
    Auto,
    FollowTarget,
    FollowAttacker,
}

impl DroneMode {
    pub const ALL: [Self; 3] = [Self::Auto, Self::FollowTarget, Self::FollowAttacker];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::FollowTarget => "Stick to Target",
            Self::FollowAttacker => "Stick to Attacker",
        }
    }
}

/// The graph options pyfa keeps in its graph settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent toggles, each a menu checkbox in pyfa"
)]
pub struct Options {
    pub ignore_resists: bool,
    pub apply_projected: bool,
    pub ignore_lock_range: bool,
    pub ignore_dcr: bool,
    pub drone_mode: DroneMode,
    /// Shield graphs count effective HP, following the stats pane in pyfa.
    pub effective_hp: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            ignore_resists: false,
            apply_projected: true,
            ignore_lock_range: false,
            ignore_dcr: false,
            drone_mode: DroneMode::Auto,
            effective_hp: true,
        }
    }
}

/// Which layer's resists a target fit takes damage with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResistMode {
    #[default]
    Auto,
    Shield,
    Armor,
    Hull,
    WeightedAverage,
}

impl ResistMode {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Shield,
        Self::Armor,
        Self::Hull,
        Self::WeightedAverage,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Shield => "Shield",
            Self::Armor => "Armor",
            Self::Hull => "Hull",
            Self::WeightedAverage => "Weighted Average",
        }
    }

    /// The resists a ship takes damage with under this mode, pyfa's graph wrapper logic.
    pub fn resists(self, ship: &Ship) -> Dmg {
        match self {
            Self::Shield => ship.resists[0],
            Self::Armor => ship.resists[1],
            Self::Hull => ship.resists[2],
            Self::WeightedAverage => weighted_resists(ship),
            Self::Auto => ship.resists[auto_layer(ship)],
        }
    }
}

fn weighted_resists(ship: &Ship) -> Dmg {
    let total: f64 = ship.hp.iter().sum();
    std::array::from_fn(|kind| {
        let ehp: f64 = (0..3)
            .map(|layer| ship.hp[layer] / (1.0 - ship.resists[layer][kind]))
            .sum();
        if ehp > 0.0 { 1.0 - total / ehp } else { 0.0 }
    })
}

/// The layer pyfa's "Auto" picks: the one the ship tanks with, scored by EHP, resists, active
/// repairs and shield regeneration.
fn auto_layer(ship: &Ship) -> usize {
    let total_ehp: f64 = ship.ehp_uniform.iter().sum();
    if total_ehp <= 0.0 {
        return 0;
    }
    let factor: Layers = std::array::from_fn(|i| {
        if ship.hp[i] > 0.0 {
            ship.ehp_uniform[i] / ship.hp[i]
        } else {
            1.0
        }
    });
    let best = factor.iter().copied().fold(f64::MIN, f64::max);
    let scores: Layers = std::array::from_fn(|i| {
        let mut score = 100.0 * (ship.ehp_uniform[i] / total_ehp).powf(1.5);
        score += 25.0 * (factor[i] / best).powf(1.5);
        score += 10_000.0 * ship.tank[i] * factor[i] / total_ehp;
        if i == 0 {
            score += 5000.0 * ship.shield_regen * factor[i] / total_ehp;
        }
        score
    });
    // On a tie the first layer wins, shield before armor before hull, as in pyfa.
    (1..3).fold(0, |best, i| if scores[i] > scores[best] { i } else { best })
}

/// What the damage graph needs to know about its target, fit or profile alike.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetData {
    pub max_velocity: f64,
    /// Infinite for profiles without a signature.
    pub sig_radius: f64,
    pub radius: f64,
    pub resists: Dmg,
    pub hp: Option<f64>,
}

/// Everything a plot needs besides the fits: axes, input values and options.
#[derive(Debug, Clone)]
pub struct Request {
    pub graph: Graph,
    pub x: XDef,
    pub y: YDef,
    /// The main input's range, in its own unit.
    pub range: (f64, f64),
    /// Every other input and the vectors, in their own units; None for an empty field.
    pub misc: HashMap<Param, Option<f64>>,
    pub checkboxes: HashMap<Checkbox, bool>,
    pub options: Options,
}

impl Request {
    fn value(&self, param: Param) -> Option<f64> {
        self.misc.get(&param).copied().flatten()
    }

    fn checkbox(&self, key: Checkbox) -> bool {
        self.checkboxes.get(&key).copied().unwrap_or(true)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

impl Series {
    fn new((xs, ys): (Vec<f64>, Vec<f64>)) -> Self {
        Self { xs, ys }
    }

    fn map_x(mut self, f: impl Fn(f64) -> f64) -> Self {
        self.xs.iter_mut().for_each(|x| *x = f(*x));
        self
    }

    fn map_y(mut self, f: impl Fn(f64) -> f64) -> Self {
        self.ys.iter_mut().for_each(|y| *y = f(*y));
        self
    }

    /// The value at `x`, read off the line between the points around it.
    pub fn at(&self, x: f64) -> Option<f64> {
        let (first, last) = (*self.xs.first()?, *self.xs.last()?);
        if x < first.min(last) || x > first.max(last) {
            return None;
        }
        // The last point at or before x, so steps read their new value.
        let i = self.xs.iter().rposition(|&px| px <= x)?;
        let Some(&next) = self.xs.get(i + 1) else {
            return Some(self.ys[i]);
        };
        // The points are sorted, so this is a vertical step.
        if next <= self.xs[i] {
            return Some(self.ys[i + 1]);
        }
        let t = (x - self.xs[i]) / (next - self.xs[i]);
        Some(self.ys[i] + (self.ys[i + 1] - self.ys[i]) * t)
    }
}

/// Plot one fit (against one target, for graphs with targets). None when the combination has
/// nothing to show, as pyfa leaves such lines out.
pub fn plot(req: &Request, src: &FitSnapshot, tgt: Option<&TargetData>) -> Option<Series> {
    let series = match req.graph {
        Graph::DamageStats => damage(req, src, tgt?)?,
        Graph::EwarStats => ewar(req, src),
        Graph::RemoteReps => remote_reps(req, src)?,
        Graph::ShieldRegen => shield(req, &src.ship),
        Graph::Capacitor => capacitor(req, src),
        Graph::Mobility => mobility(req, &src.ship)?,
        Graph::WarpTime => warp(req, &src.ship),
        Graph::LockTime => lock(req, &src.ship)?,
    };
    series
        .xs
        .iter()
        .chain(&series.ys)
        .all(|v| v.is_finite())
        .then_some(series)
}

/// The main input range in base units.
fn main_range(req: &Request, scale: f64) -> (f64, f64) {
    (req.range.0 * scale, req.range.1 * scale)
}

fn in_range(limit: f64, distance: Option<f64>, ignore: bool) -> bool {
    ignore || distance.is_none_or(|d| d <= limit)
}

// Damage stats.

/// Per weapon: what it deals at the time asked, before application.
fn damage_values(weapons: &[Weapon], y: Param, time: Option<f64>) -> Option<Vec<Dmg>> {
    weapons
        .iter()
        .map(|w| match (y, time) {
            (Param::Dps, None) => Some(w.dps),
            (Param::Volley, None) => Some(w.volley),
            (_, Some(t)) => Some(timeline_value(&w.cycle, &w.cycle.cycles(t), y, t)),
            // Damage inflicted means nothing without a time.
            _ => None,
        })
        .collect()
}

/// DPS, volley, or damage done so far at `time`, from a weapon's cycles.
fn timeline_value<T>(cycle: &Cycle<T>, cycles: &[(f64, f64, T)], y: Param, time: f64) -> T
where
    T: Copy + Default + Accumulate,
{
    if matches!(y, Param::Damage | Param::Total) {
        return cycles
            .iter()
            .filter(|c| c.0 + cycle.delay <= time)
            .fold(T::default(), |acc, c| acc.plus(c.2));
    }
    match cycles.iter().rev().find(|c| c.0 <= time) {
        // Past the end of the last cycle that started: reloading, nothing is dealt.
        Some(c) if time < c.1 => {
            if matches!(y, Param::Dps | Param::Rps) {
                c.2.scaled(1.0 / cycle.duration)
            } else {
                c.2
            }
        }
        _ => T::default(),
    }
}

/// Adding up what cycles deliver: damage by type, or repairs by layer.
trait Accumulate {
    fn plus(self, other: Self) -> Self;
    fn scaled(self, by: f64) -> Self;
}

impl<const N: usize> Accumulate for [f64; N] {
    fn plus(self, other: Self) -> Self {
        std::array::from_fn(|i| self[i] + other[i])
    }

    fn scaled(self, by: f64) -> Self {
        self.map(|v| v * by)
    }
}

fn add(a: Dmg, b: Dmg) -> Dmg {
    std::array::from_fn(|i| a[i] + b[i])
}

/// Times at which any weapon's output changes, up to `until`.
fn change_times<T: Clone + Default>(cycles: &[&Cycle<T>], until: f64, delayed: bool) -> Vec<f64> {
    let mut times: Vec<f64> = cycles
        .iter()
        .flat_map(|c| {
            c.cycles(until)
                .into_iter()
                .flat_map(move |(start, end, _)| {
                    if delayed {
                        vec![start + c.delay]
                    } else {
                        vec![start, end]
                    }
                })
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    times
}

/// The geometry of one damage sample.
#[derive(Debug, Clone, Copy)]
struct Engagement {
    distance: Option<f64>,
    atk_speed: f64,
    atk_angle: f64,
    tgt_speed: f64,
    tgt_angle: f64,
    tgt_sig: f64,
}

fn damage(req: &Request, src: &FitSnapshot, tgt: &TargetData) -> Option<Series> {
    let ship = &src.ship;
    let pct = |v: Option<f64>, of: f64| v.unwrap_or(100.0) / 100.0 * of;
    let base = Engagement {
        distance: req.value(Param::Distance).map(|km| km * 1000.0),
        atk_speed: pct(req.value(Param::AtkSpeed), ship.max_velocity),
        atk_angle: req.value(Param::AtkAngle).unwrap_or(0.0),
        tgt_speed: pct(req.value(Param::TgtSpeed), tgt.max_velocity),
        tgt_angle: req.value(Param::TgtAngle).unwrap_or(0.0),
        tgt_sig: tgt.sig_radius,
    };
    let time = req.value(Param::Time).map(|t| t.clamp(0.0, MAX_TIME));
    let y = req.y.param;
    let value = |values: &[Dmg], e: Engagement| -> f64 {
        let e = if req.options.apply_projected {
            tackle(req, src, tgt, e)
        } else {
            e
        };
        let applied = values
            .iter()
            .zip(&src.weapons)
            .fold([0.0; 4], |acc, (v, w)| {
                let mult = application(req, ship, tgt, w, e);
                add(acc, v.map(|d| d * mult))
            });
        let resists = if req.options.ignore_resists {
            [0.0; 4]
        } else {
            tgt.resists
        };
        (0..4).map(|i| applied[i] * (1.0 - resists[i])).sum()
    };

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
            let timelines: Vec<_> = cycles.iter().map(|c| c.cycles(high)).collect();
            Series::new(math::steps((low, high), &times, |t| {
                let values: Vec<Dmg> = cycles
                    .iter()
                    .zip(&timelines)
                    .map(|(c, timeline)| timeline_value(c, timeline, y, t))
                    .collect();
                value(&values, base)
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
fn tackle(req: &Request, src: &FitSnapshot, tgt: &TargetData, e: Engagement) -> Engagement {
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
    if tgt.max_velocity > 0.0 {
        let ratio = e.tgt_speed / tgt.max_velocity;
        let mut mults = Vec::new();
        for web in src
            .ewar
            .iter()
            .filter(usable)
            .filter(|w| w.kind == EwarKind::Web)
        {
            mults.push(1.0 + applied(web, out.tgt_speed) / 100.0);
            out.tgt_speed = tgt.max_velocity * math::stacked_multiplier(&mults) * ratio;
        }
    }
    let tp_mults: Vec<f64> = src
        .ewar
        .iter()
        .filter(usable)
        .filter(|w| w.kind == EwarKind::TargetPainter)
        .map(|tp| 1.0 + applied(tp, out.tgt_speed) / 100.0)
        .collect();
    if !tp_mults.is_empty() && e.tgt_sig.is_finite() {
        out.tgt_sig = e.tgt_sig * math::stacked_multiplier(&tp_mults);
    }
    out
}

// Electronic warfare.

fn ewar(req: &Request, src: &FitSnapshot) -> Series {
    let ship = &src.ship;
    let resonance = 1.0 - req.value(Param::Resist).unwrap_or(0.0).clamp(0.0, 100.0) / 100.0;
    let opts = &req.options;
    let kind = match req.y.param {
        Param::NeutStr => EwarKind::Neut,
        Param::WebStr => EwarKind::Web,
        Param::EcmStrMax => EwarKind::Ecm,
        Param::DampStrLockRange => EwarKind::Damp,
        Param::TdStrOptimal => EwarKind::TrackingDisruptor,
        Param::GdStrRange => EwarKind::GuidanceDisruptor,
        _ => EwarKind::TargetPainter,
    };
    let effects: Vec<&Ewar> = src.ewar.iter().filter(|e| e.kind == kind).collect();
    let point = |distance: f64| -> f64 {
        let d = Some(distance);
        let in_lock = in_range(ship.lock_range, d, opts.ignore_lock_range);
        let in_dcr = in_range(ship.drone_control_range, d, opts.ignore_dcr);
        let active = effects
            .iter()
            .filter(|e| (!e.needs_lock || in_lock) && (!e.needs_dcr || in_dcr));
        // Drones fly to the target, so pyfa gives them unlimited range here.
        let factor = |e: &Ewar| {
            if e.mobile.is_some() {
                1.0
            } else {
                math::range_factor(e.optimal, e.falloff, d, true)
            }
        };
        match kind {
            EwarKind::Neut | EwarKind::Ecm => {
                active.map(|e| e.strength * resonance * factor(e)).sum()
            }
            EwarKind::GuidanceDisruptor => {
                let (mut velocity, mut flight) = (Vec::new(), Vec::new());
                for e in active {
                    velocity.push(1.0 + e.strength * resonance * factor(e) / 100.0);
                    flight.push(1.0 + e.strength2 * resonance * factor(e) / 100.0);
                }
                (1.0 - math::stacked_multiplier(&velocity) * math::stacked_multiplier(&flight))
                    * 100.0
            }
            EwarKind::TargetPainter => {
                let mults: Vec<f64> = active
                    .map(|e| 1.0 + e.strength * resonance * factor(e) / 100.0)
                    .collect();
                (math::stacked_multiplier(&mults) - 1.0) * 100.0
            }
            EwarKind::Web | EwarKind::Damp | EwarKind::TrackingDisruptor => {
                let mults: Vec<f64> = active
                    .map(|e| 1.0 + e.strength * resonance * factor(e) / 100.0)
                    .collect();
                (1.0 - math::stacked_multiplier(&mults)) * 100.0
            }
        }
    };
    Series::new(math::smooth(main_range(req, 1000.0), 50, 2, point)).map_x(|m| m / 1000.0)
}

// Remote repairs.

fn remote_reps(req: &Request, src: &FitSnapshot) -> Option<Series> {
    let ship = &src.ship;
    let opts = &req.options;
    let anc_reload = req.checkbox(Checkbox::AncReload);
    let cycles: Vec<&Cycle<Layers>> = src
        .reps
        .iter()
        .map(|r| match (&r.no_reload, anc_reload) {
            (Some(no_reload), false) => no_reload,
            _ => &r.cycle,
        })
        .collect();
    let y = req.y.param;
    let total = y == Param::Total;
    // Per repairer: what it gives at `time`; None time is the stats pane's rate.
    let values = |time: Option<f64>| -> Option<Vec<f64>> {
        cycles
            .iter()
            .map(|c| match time {
                None if total => None,
                None => Some(average_rate(c)),
                Some(t) => Some(timeline_value(c, &c.cycles(t), y, t).iter().sum()),
            })
            .collect()
    };
    let applied = |values: &[f64], distance: Option<f64>| -> f64 {
        let in_lock = in_range(ship.lock_range, distance, opts.ignore_lock_range);
        let in_dcr = in_range(ship.drone_control_range, distance, opts.ignore_dcr);
        values
            .iter()
            .zip(&src.reps)
            .map(|(v, r)| {
                let mult = if !in_lock {
                    0.0
                } else if r.drone {
                    if in_dcr { 1.0 } else { 0.0 }
                } else {
                    math::range_factor(r.optimal, r.falloff, distance, true)
                };
                v * mult
            })
            .sum()
    };
    let series = if req.x.param == Param::Distance {
        let time = req.value(Param::Time).map(|t| t.clamp(0.0, MAX_TIME));
        let values = values(time)?;
        Series::new(math::smooth(main_range(req, 1000.0), 50, 2, |d| {
            applied(&values, Some(d))
        }))
        .map_x(|m| m / 1000.0)
    } else {
        let distance = req.value(Param::Distance).map(|km| km * 1000.0);
        let (low, high) = (
            req.range.0.clamp(0.0, MAX_TIME),
            req.range.1.clamp(0.0, MAX_TIME),
        );
        let times = change_times(&cycles, high, total);
        let timelines: Vec<_> = cycles.iter().map(|c| c.cycles(high)).collect();
        Series::new(math::steps((low, high), &times, |t| {
            let values: Vec<f64> = cycles
                .iter()
                .zip(&timelines)
                .map(|(c, timeline)| timeline_value(c, timeline, y, t).iter().sum())
                .collect();
            applied(&values, distance)
        }))
    };
    Some(series)
}

/// Shield, armor and hull repaired per second over a whole cycle and reload.
fn average_rate(cycle: &Cycle<Layers>) -> f64 {
    let amount: f64 = cycle.amount(usize::MAX).iter().sum();
    if cycle.duration <= 0.0 {
        return 0.0;
    }
    match cycle.shots {
        Some(shots) if shots > 0 => {
            let shots = f64::from(shots);
            amount * shots / (cycle.duration * shots + cycle.reload)
        }
        _ => amount / cycle.duration,
    }
}

// Shield regeneration.

fn shield(req: &Request, ship: &Ship) -> Series {
    let max = ship.shield_capacity;
    let regen = ship.shield_recharge;
    let ehp = if req.options.effective_hp {
        ship.shield_ehp_mult
    } else {
        1.0
    };
    let pct = |v: f64| v / 100.0 * max;
    let series = if req.x.param == Param::Time {
        let start = pct(req.value(Param::ShieldAmountT0).unwrap_or(0.0)).clamp(0.0, max);
        Series::new(math::smooth(req.range, 200, 0, |t| {
            let amount = math::regen_amount(max, regen, start, t);
            if req.y.param == Param::ShieldAmount {
                amount
            } else {
                math::regen_rate(max, regen, amount)
            }
        }))
    } else {
        let (low, high) = main_range(req, max / 100.0);
        let series = Series::new(math::smooth(
            (low.clamp(0.0, max), high.clamp(0.0, max)),
            200,
            0,
            |amount| {
                if req.y.param == Param::ShieldAmount {
                    amount
                } else {
                    math::regen_rate(max, regen, amount)
                }
            },
        ));
        match req.x.unit {
            Unit::Percent => series.map_x(|v| if max > 0.0 { v * 100.0 / max } else { 0.0 }),
            _ => series.map_x(|v| v * ehp),
        }
    };
    series.map_y(|v| v * ehp)
}

// Capacitor.

fn capacitor(req: &Request, src: &FitSnapshot) -> Series {
    let ship = &src.ship;
    let max = ship.cap_capacity;
    let regen = ship.cap_recharge;
    let pct = |v: f64| (v / 100.0 * max).clamp(0.0, max);
    let rate = |amount: f64| math::regen_rate(max, regen, amount);
    if req.x.param == Param::Time {
        let start = pct(req.value(Param::CapAmountT0).unwrap_or(100.0));
        let (low, high) = (
            req.range.0.clamp(0.0, MAX_CAP_TIME),
            req.range.1.clamp(0.0, MAX_CAP_TIME),
        );
        let sim = (req.y.param == Param::CapAmount && req.checkbox(Checkbox::UseCapsim))
            .then(|| capsim::simulate(&src.cap_users, max, regen, start, MAX_CAP_TIME))
            .filter(|sim| !sim.is_empty());
        match sim {
            Some(sim) => Series::new(cap_sim_series(&sim, max, regen, start, (low, high))),
            None => Series::new(math::smooth((low, high), 200, 0, |t| {
                let amount = math::regen_amount(max, regen, start, t);
                if req.y.param == Param::CapAmount {
                    amount
                } else {
                    rate(amount)
                }
            })),
        }
    } else {
        let (low, high) = main_range(req, max / 100.0);
        let series = Series::new(math::smooth(
            (low.clamp(0.0, max), high.clamp(0.0, max)),
            200,
            0,
            |amount| {
                if req.y.param == Param::CapAmount {
                    amount
                } else {
                    rate(amount)
                }
            },
        ));
        if req.x.unit == Unit::Percent {
            series.map_x(|v| if max > 0.0 { v * 100.0 / max } else { 0.0 })
        } else {
            series
        }
    }
}

/// Join the simulator's level after each activation with the regeneration curve between them,
/// as pyfa's capacitor getter does. The line ends where the simulation ends.
fn cap_sim_series(
    sim: &[(f64, f64)],
    max: f64,
    regen: f64,
    start: f64,
    (low, high): (f64, f64),
) -> (Vec<f64>, Vec<f64>) {
    let last_time = sim.last().map_or(0.0, |s| s.0);
    let high = high.min(last_time);
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    // The simulation ended before the range starts: nothing to draw.
    if last_time < low {
        return (xs, ys);
    }
    let step = ((high - low) / 200.0).max(f64::EPSILON);
    let regen_to = |xs: &mut Vec<f64>, ys: &mut Vec<f64>, from: f64, cap: f64, to: f64| {
        let parts = ((to - from) / step).ceil().max(1.0);
        for i in 1..=parts as u32 {
            let t = from + (to - from) * f64::from(i) / parts;
            xs.push(t);
            ys.push(math::regen_amount(max, regen, cap, t - from));
        }
    };
    let before = sim.iter().rev().find(|s| s.0 < low);
    let mut prev_cap = match before {
        Some(&(t, cap)) => math::regen_amount(max, regen, cap, low - t),
        None => math::regen_amount(max, regen, start, low),
    };
    let mut prev_time = low;
    xs.push(low);
    ys.push(prev_cap);
    for &(t, cap) in sim.iter().filter(|s| s.0 >= low && s.0 <= high) {
        if t > prev_time {
            regen_to(&mut xs, &mut ys, prev_time, prev_cap, t);
        }
        xs.push(t);
        ys.push(cap);
        prev_time = t;
        prev_cap = cap;
    }
    if high > prev_time {
        regen_to(&mut xs, &mut ys, prev_time, prev_cap, high);
    }
    (xs, ys)
}

// Mobility, warp and lock time.

fn mobility(req: &Request, ship: &Ship) -> Option<Series> {
    let (speed, mass, agility) = (ship.max_velocity, ship.mass, ship.agility);
    let tgt_mass = req.value(Param::TgtMass).unwrap_or(1300.0) * 1e6;
    let tgt_inertia = req.value(Param::TgtInertia).unwrap_or(0.015);
    let y = req.y.param;
    if matches!(y, Param::BumpSpeed | Param::BumpDistance) && req.value(Param::TgtMass).is_none() {
        return None;
    }
    Some(Series::new(math::smooth(req.range, 200, 0, |t| {
        let v = math::speed_at(speed, mass, agility, t);
        match y {
            Param::Speed => v,
            Param::Distance => math::distance_at(speed, mass, agility, t) / 1000.0,
            Param::Momentum => v * mass / 1e9,
            Param::BumpSpeed => math::bump_speed(v, mass, tgt_mass),
            _ => {
                math::bump_distance(math::bump_speed(v, mass, tgt_mass), tgt_mass, tgt_inertia)
                    / 1000.0
            }
        }
    })))
}

fn warp(req: &Request, ship: &Ship) -> Series {
    let scale = if req.x.unit == Unit::Au {
        math::AU_METERS
    } else {
        1000.0
    };
    let limit = ship.max_warp_distance * math::AU_METERS;
    let (low, high) = main_range(req, scale);
    Series::new(math::smooth(
        (low.clamp(0.0, limit), high.clamp(0.0, limit)),
        500,
        0,
        |d| math::warp_time(ship.warp_speed, ship.subwarp_speed, d),
    ))
    .map_x(|m| m / scale)
}

fn lock(req: &Request, ship: &Ship) -> Option<Series> {
    let (low, high) = (req.range.0.max(1.0), req.range.1.max(1.0));
    let series = Series::new(math::smooth((low, high), 200, 0, |sig| {
        math::lock_time(ship.scan_res, sig).unwrap_or(f64::NAN)
    }));
    series.ys.iter().all(|y| y.is_finite()).then_some(series)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::{defs::YDef, math::Gun};

    fn ship() -> Ship {
        Ship {
            radius: 50.0,
            max_velocity: 300.0,
            sig_radius: 40.0,
            mass: 1e6,
            agility: 3.0,
            lock_range: 50_000.0,
            drone_control_range: 60_000.0,
            cap_capacity: 1000.0,
            cap_recharge: 100.0,
            shield_capacity: 1000.0,
            shield_recharge: 200.0,
            shield_ehp_mult: 2.0,
            scan_res: 400.0,
            warp_speed: 3.0,
            max_warp_distance: 100.0,
            subwarp_speed: 300.0,
            ..Ship::default()
        }
    }

    fn turret(dps: f64) -> Weapon {
        Weapon {
            kind: WeaponKind::Turret(Gun {
                optimal: 10_000.0,
                falloff: 5_000.0,
                tracking: 0.5,
                optimal_sig: 40_000.0,
            }),
            dps: [dps, 0.0, 0.0, 0.0],
            volley: [dps * 2.0, 0.0, 0.0, 0.0],
            cycle: Cycle {
                duration: 2.0,
                shots: Some(2),
                reload: 10.0,
                amounts: vec![[dps * 2.0, 0.0, 0.0, 0.0]],
                delay: 0.0,
            },
        }
    }

    fn request(graph: Graph, x: usize, y: usize, range: (f64, f64)) -> Request {
        Request {
            graph,
            x: graph.x_defs(true)[x],
            y: graph.y_defs(false, true)[y],
            range,
            misc: HashMap::new(),
            checkboxes: HashMap::new(),
            options: Options::default(),
        }
    }

    fn ideal(resist: f64) -> TargetData {
        TargetData {
            max_velocity: 0.0,
            sig_radius: f64::INFINITY,
            radius: 0.0,
            resists: [resist; 4],
            hp: None,
        }
    }

    #[test]
    fn turret_dps_over_distance_follows_falloff_and_lock_range() {
        let src = FitSnapshot {
            ship: ship(),
            weapons: vec![turret(100.0)],
            ..FitSnapshot::default()
        };
        let req = request(Graph::DamageStats, 0, 0, (0.0, 60.0));
        let series = plot(&req, &src, Some(&ideal(0.5))).expect("a line");
        // In optimal, a sure hit: 100 dps * turret_mult(1) * 0.5 resists.
        let full = 100.0 * math::turret_mult(1.0) * 0.5;
        assert!((series.at(5.0).expect("in range") - full).abs() < 1e-9);
        // One falloff out: half the hits.
        let half = 100.0 * math::turret_mult(0.5) * 0.5;
        assert!((series.at(15.0).expect("in range") - half).abs() < 1e-9);
        // Past lock range, nothing.
        assert_eq!(series.at(55.0), Some(0.0));
    }

    #[test]
    fn inflicted_damage_steps_per_volley_and_pauses_for_reload() {
        let src = FitSnapshot {
            ship: ship(),
            weapons: vec![turret(100.0)],
            ..FitSnapshot::default()
        };
        let mut req = request(Graph::DamageStats, 1, 2, (0.0, 20.0));
        req.options.ignore_resists = true;
        let series = plot(&req, &src, Some(&ideal(0.0))).expect("a line");
        let per_volley = 200.0 * math::turret_mult(1.0);
        // Volleys at 0 and 2, reload until 14, then 14 and 16.
        assert_eq!(series.at(1.0), Some(per_volley));
        assert_eq!(series.at(13.0), Some(2.0 * per_volley));
        assert_eq!(series.at(16.5), Some(4.0 * per_volley));
        let dps = request(Graph::DamageStats, 1, 0, (0.0, 20.0));
        let dps = plot(&dps, &src, Some(&ideal(0.0))).expect("a line");
        assert_eq!(dps.at(8.0), Some(0.0), "reloading");
        assert!(dps.at(15.0).is_some_and(|v| v > 0.0));
    }

    #[test]
    fn webs_slow_the_target_for_application() {
        let mut src = FitSnapshot {
            ship: ship(),
            ..FitSnapshot::default()
        };
        src.ewar.push(Ewar {
            kind: EwarKind::Web,
            strength: -60.0,
            strength2: 0.0,
            optimal: 10_000.0,
            falloff: 0.0,
            needs_lock: true,
            needs_dcr: false,
            mobile: None,
        });
        let tgt = TargetData {
            max_velocity: 1000.0,
            sig_radius: 40.0,
            radius: 0.0,
            resists: [0.0; 4],
            hp: None,
        };
        let req = request(Graph::DamageStats, 0, 0, (0.0, 20.0));
        let e = Engagement {
            distance: Some(5_000.0),
            atk_speed: 0.0,
            atk_angle: 0.0,
            tgt_speed: 1000.0,
            tgt_angle: 90.0,
            tgt_sig: 40.0,
        };
        assert!((tackle(&req, &src, &tgt, e).tgt_speed - 400.0).abs() < 1e-9);
        let far = Engagement {
            distance: Some(15_000.0),
            ..e
        };
        assert_eq!(tackle(&req, &src, &tgt, far).tgt_speed, 1000.0);
    }

    #[test]
    fn capacitor_and_shield_regen_curves() {
        let src = FitSnapshot {
            ship: ship(),
            ..FitSnapshot::default()
        };
        // Regen against cap amount in percent: peaks at 25%.
        let req = request(Graph::Capacitor, 2, 1, (0.0, 100.0));
        let series = plot(&req, &src, None).expect("a line");
        let peak = series.at(25.0).expect("in range");
        assert!((peak - 25.0).abs() < 1e-6, "{peak}");
        // Shield amount in EHP from empty: the curve ends at the full shield's EHP.
        let req = request(Graph::ShieldRegen, 0, 0, (0.0, 2000.0));
        let series = plot(&req, &src, None).expect("a line");
        let end = *series.ys.last().expect("points");
        assert!((end - 2000.0).abs() < 1e-3, "{end}");
    }

    #[test]
    fn warp_and_lock_lines() {
        let src = FitSnapshot {
            ship: ship(),
            ..FitSnapshot::default()
        };
        let req = request(Graph::WarpTime, 0, 0, (0.0, 50.0));
        let series = plot(&req, &src, None).expect("a line");
        assert!(series.ys.windows(2).all(|w| w[0] <= w[1]));
        let mut req = request(Graph::LockTime, 0, 0, (25.0, 500.0));
        req.y = YDef {
            param: Param::LockTime,
            unit: Unit::S,
            label: "Lock time",
            selector: None,
        };
        let series = plot(&req, &src, None).expect("a line");
        let want = math::lock_time(400.0, 25.0).expect("valid");
        assert!((series.ys[0] - want).abs() < 1e-9);
    }

    #[test]
    fn auto_resist_mode_picks_the_tanked_layer() {
        let mut s = ship();
        s.hp = [100.0, 5000.0, 100.0];
        s.ehp_uniform = [150.0, 12_000.0, 130.0];
        s.resists = [[0.1; 4], [0.6; 4], [0.3; 4]];
        assert_eq!(ResistMode::Auto.resists(&s), [0.6; 4]);
        // Equal HP and resists everywhere: the weighted average is that resist.
        s.hp = [100.0; 3];
        s.resists = [[0.5; 4]; 3];
        let w = ResistMode::WeightedAverage.resists(&s);
        assert!((w[0] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn series_reads_steps_and_slopes() {
        let s = Series {
            xs: vec![0.0, 2.0, 2.0, 4.0],
            ys: vec![1.0, 1.0, 3.0, 5.0],
        };
        assert_eq!(s.at(1.0), Some(1.0));
        assert_eq!(s.at(2.0), Some(3.0));
        assert_eq!(s.at(3.0), Some(4.0));
        assert_eq!(s.at(5.0), None);
    }
}
