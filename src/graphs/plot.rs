//! Turning a graph request into points: pyfa's getters for every graph, over a
//! [`FitSnapshot`] and an optional target.

mod damage;
mod ewar;
mod motion;
mod regen;
mod reps;

use std::collections::HashMap;

use super::{
    defs::{Checkbox, Graph, Param, XDef, YDef},
    snapshot::{Cycle, Dmg, FitSnapshot, Layers, Ship},
};

use damage::damage;
pub(super) use damage::{applied_damage, engagement};
use ewar::{ecm_burst, ewar};
use motion::{lock, mobility, warp};
use regen::{capacitor, shield};
use reps::remote_reps;

/// pyfa caps damage and repair timelines at this many seconds.
const MAX_TIME: f64 = 2500.0;
/// And capacitor timelines at an hour.
const MAX_CAP_TIME: f64 = 3600.0;

/// pyfa's "mobile drone mode": how drones and fighters chase the target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Multiplier on breacher pod damage taken.
    pub breacher_resist: f64,
    /// Speed and signature once scrammed, for targets running a microwarpdrive.
    pub scrammed: Option<(f64, f64)>,
}

impl TargetData {
    /// An abstract target: no breacher resistance, nothing to scram.
    pub fn profile(
        max_velocity: f64,
        sig_radius: f64,
        radius: f64,
        resists: Dmg,
        hp: Option<f64>,
    ) -> Self {
        Self {
            max_velocity,
            sig_radius,
            radius,
            resists,
            hp,
            breacher_resist: 1.0,
            scrammed: None,
        }
    }
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
        Graph::EcmBurstDamps => ecm_burst(req, src)?,
        // Plotted in segments by [`super::ammo`].
        Graph::ApplicationProfile => return None,
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

/// A cycling item's cycles up to some time, laid out so the value at any moment is a binary
/// search away.
struct Timeline<T> {
    duration: f64,
    /// Cycle starts and ends, in order.
    starts: Vec<f64>,
    ends: Vec<f64>,
    amounts: Vec<T>,
    /// When each cycle's amount lands, in order.
    landings: Vec<f64>,
    /// What has landed by the end of each cycle, summed in cycle order.
    landed: Vec<T>,
}

impl<T: Copy + Default + Accumulate> Timeline<T> {
    /// Every cycle of `cycle` until `until` seconds.
    fn new(cycle: &Cycle<T>, until: f64) -> Self {
        let cycles = cycle.cycles(until);
        let mut total = T::default();
        let landed = cycles
            .iter()
            .map(|c| {
                total = total.plus(c.2);
                total
            })
            .collect();
        Self {
            duration: cycle.duration,
            starts: cycles.iter().map(|c| c.0).collect(),
            ends: cycles.iter().map(|c| c.1).collect(),
            amounts: cycles.iter().map(|c| c.2).collect(),
            landings: cycles.iter().map(|c| c.0 + cycle.delay).collect(),
            landed,
        }
    }

    /// DPS, volley, or what has been done so far at `time`.
    fn value(&self, y: Param, time: f64) -> T {
        if matches!(y, Param::Damage | Param::Total) {
            let done = self.landings.partition_point(|&at| at <= time);
            return done
                .checked_sub(1)
                .map_or_else(T::default, |last| self.landed[last]);
        }
        let started = self.starts.partition_point(|&at| at <= time);
        match started.checked_sub(1) {
            // Past the end of the last cycle that started: reloading, nothing is dealt.
            Some(c) if time < self.ends[c] => {
                if matches!(y, Param::Dps | Param::Rps) {
                    self.amounts[c].scaled(1.0 / self.duration)
                } else {
                    self.amounts[c]
                }
            }
            _ => T::default(),
        }
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

#[cfg(test)]
mod tests {
    use super::{
        damage::{Engagement, tackle},
        *,
    };
    use crate::graphs::{
        defs::{Unit, YDef},
        math::{self, Gun},
        snapshot::{Ewar, EwarKind, Weapon, WeaponKind},
    };

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
        TargetData::profile(0.0, f64::INFINITY, 0.0, [resist; 4], None)
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
        let tgt = TargetData::profile(1000.0, 40.0, 0.0, [0.0; 4], None);
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
