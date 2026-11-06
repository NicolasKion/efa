//! The formulas pyfa's graphs use, ported one to one so the curves match pyfa's.
//!
//! Every function here is pure; the numbers it works on come from refine (see
//! [`crate::engine`]'s graph snapshot).

use std::f64::consts::PI;

/// Meters in an astronomical unit, as EVE counts it.
pub const AU_METERS: f64 = 149_597_870_700.0;

/// Lock time never goes over half an hour, however small the target.
const MAX_LOCK_TIME: f64 = 30.0 * 60.0;

/// Range factor for guns, ewar and remote repairs (pyfa's `calculateRangeFactor`).
///
/// `None` distance means "no distance set", which always applies fully. Most modules cannot be
/// activated past optimal plus three falloffs; guns can (`restricted` false).
pub fn range_factor(optimal: f64, falloff: f64, distance: Option<f64>, restricted: bool) -> f64 {
    let Some(distance) = distance else { return 1.0 };
    if falloff > 0.0 {
        if restricted && distance > optimal + 3.0 * falloff {
            return 0.0;
        }
        0.5_f64.powf(((distance - optimal).max(0.0) / falloff).powi(2))
    } else if distance <= optimal {
        1.0
    } else {
        0.0
    }
}

/// Stacking penalty over a set of multipliers, the way EVE applies it within one group:
/// bonuses and penalties apart, strongest first, each after the first weakened by
/// `exp(-(i^2) / 7.1289)`.
pub fn stacked_multiplier(multipliers: &[f64]) -> f64 {
    let mut bonuses: Vec<f64> = multipliers.iter().copied().filter(|&m| m > 1.0).collect();
    let mut penalties: Vec<f64> = multipliers.iter().copied().filter(|&m| m < 1.0).collect();
    let mut value = 1.0;
    for list in [&mut bonuses, &mut penalties] {
        list.sort_by(|a, b| (b - 1.0).abs().total_cmp(&(a - 1.0).abs()));
        for (i, m) in list.iter().enumerate() {
            value *= 1.0 + (m - 1.0) * (-((i * i) as f64) / 7.1289).exp();
        }
    }
    value
}

/// Damage multiplier from a turret's chance to hit, counting wrecking shots.
pub fn turret_mult(chance_to_hit: f64) -> f64 {
    let wrecking = chance_to_hit.min(0.01);
    let normal = chance_to_hit - wrecking;
    let normal_part = if normal > 0.0 {
        normal * (f64::midpoint(0.01, chance_to_hit) + 0.49)
    } else {
        0.0
    };
    normal_part + wrecking * 3.0
}

/// How two ships move relative to each other, for turret tracking.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Motion {
    pub atk_speed: f64,
    /// Degrees; 0 means straight at the target.
    pub atk_angle: f64,
    pub atk_radius: f64,
    pub tgt_speed: f64,
    pub tgt_angle: f64,
    pub tgt_radius: f64,
}

/// Angular speed of the target as the attacker sees it, in radians per second.
pub fn angular_speed(motion: &Motion, distance: Option<f64>) -> f64 {
    let Some(distance) = distance else { return 0.0 };
    let atk = motion.atk_angle * PI / 180.0;
    let tgt = motion.tgt_angle * PI / 180.0;
    let c2c = motion.atk_radius + distance + motion.tgt_radius;
    // The target sits to the attacker's right, so transversal is the projection on the Y axis.
    let transversal = (motion.atk_speed * atk.sin() - motion.tgt_speed * tgt.sin()).abs();
    if c2c == 0.0 {
        if transversal == 0.0 {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        transversal / c2c
    }
}

/// What a turret (or a drone, which shoots like one) needs to hit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Gun {
    pub optimal: f64,
    pub falloff: f64,
    pub tracking: f64,
    pub optimal_sig: f64,
}

/// Chance to hit for turret-like weapons.
pub fn turret_chance_to_hit(
    gun: &Gun,
    motion: &Motion,
    distance: Option<f64>,
    tgt_sig: f64,
) -> f64 {
    let angular = angular_speed(motion, distance);
    let range = range_factor(gun.optimal, gun.falloff, distance, false);
    let tracking = if angular == 0.0 {
        1.0
    } else {
        0.5_f64.powf(((angular * gun.optimal_sig) / (gun.tracking * tgt_sig)).powi(2))
    };
    range * tracking
}

/// Missile application against a target's signature and speed.
pub fn missile_factor(er: f64, ev: f64, drf: f64, tgt_speed: f64, tgt_sig: f64) -> f64 {
    let mut factor: f64 = 1.0;
    if er > 0.0 {
        factor = factor.min(tgt_sig / er);
    }
    if tgt_speed > 0.0 {
        factor = factor.min(((ev * tgt_sig) / (er * tgt_speed)).powf(drf));
    }
    factor
}

/// Bomb or guided bomb application: only signature matters.
pub fn bomb_factor(er: f64, tgt_sig: f64) -> f64 {
    if er == 0.0 {
        1.0
    } else {
        (tgt_sig / er).min(1.0)
    }
}

/// How far a missile flies: the whole seconds it surely reaches and the next one it reaches
/// with a chance, both from the ship's surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MissileRange {
    pub lower: f64,
    pub higher: f64,
    pub higher_chance: f64,
}

/// pyfa's `missileMaxRangeData`. `fof_limit` caps the range of friend-or-foe missiles.
pub fn missile_range(
    velocity: f64,
    flight_ms: f64,
    mass: f64,
    agility: f64,
    ship_radius: f64,
    fof_limit: Option<f64>,
) -> Option<MissileRange> {
    if velocity <= 0.0 {
        return None;
    }
    let range = |time: f64| {
        let accel = time.min(mass * agility / 1_000_000.0);
        velocity / 2.0 * accel + velocity * (time - accel)
    };
    // Flight time grows with the ship's radius, as missiles start in its center.
    let flight = flight_ms / 1000.0 + ship_radius / velocity;
    let (lower_time, higher_time) = (flight.floor(), flight.ceil());
    let (mut lower, mut higher) = (range(lower_time), range(higher_time));
    if let Some(limit) = fof_limit.filter(|&l| l > 0.0) {
        lower = lower.min(limit);
        higher = higher.min(limit);
    }
    Some(MissileRange {
        lower: (lower - ship_radius).max(0.0),
        higher: (higher - ship_radius).max(0.0),
        higher_chance: flight - lower_time,
    })
}

impl MissileRange {
    pub fn factor(&self, distance: Option<f64>) -> f64 {
        match distance {
            None => 1.0,
            Some(d) if d <= self.lower => 1.0,
            Some(d) if d <= self.higher => self.higher_chance,
            Some(_) => 0.0,
        }
    }
}

/// Capacitor or shield amount after `time` seconds of regeneration from `start`.
pub fn regen_amount(max: f64, regen_time: f64, start: f64, time: f64) -> f64 {
    if max <= 0.0 || regen_time <= 0.0 {
        return start;
    }
    max * (1.0 + (-5.0 * time / regen_time).exp() * ((start / max).sqrt() - 1.0)).powi(2)
}

/// Capacitor or shield regeneration per second at `current`.
pub fn regen_rate(max: f64, regen_time: f64, current: f64) -> f64 {
    if max <= 0.0 || regen_time <= 0.0 {
        return 0.0;
    }
    let ratio = current / max;
    10.0 * max / regen_time * (ratio.sqrt() - ratio)
}

/// Speed after accelerating from a standstill for `time` seconds.
pub fn speed_at(max_speed: f64, mass: f64, agility: f64, time: f64) -> f64 {
    let k = agility * mass;
    if k <= 0.0 {
        return max_speed;
    }
    max_speed * (1.0 - (-time * 1_000_000.0 / k).exp())
}

/// Distance covered after accelerating from a standstill for `time` seconds.
pub fn distance_at(max_speed: f64, mass: f64, agility: f64, time: f64) -> f64 {
    let k = agility * mass;
    if k <= 0.0 {
        return max_speed * time;
    }
    let at = |t: f64| max_speed * t + max_speed * k * (-t * 1_000_000.0 / k).exp() / 1_000_000.0;
    at(time) - at(0.0)
}

/// Speed a bumped target picks up (Santorine's collision formula, masses in millions of kg).
pub fn bump_speed(bumper_speed: f64, bumper_mass: f64, tgt_mass: f64) -> f64 {
    let (bumper, tgt) = (bumper_mass / 1e6, tgt_mass / 1e6);
    if bumper + tgt <= 0.0 {
        return 0.0;
    }
    2.0 * bumper_speed * bumper / (bumper + tgt)
}

/// How far a bumped target drifts before it stops.
pub fn bump_distance(tgt_speed: f64, tgt_mass: f64, tgt_inertia: f64) -> f64 {
    tgt_speed * tgt_mass / 1e6 * tgt_inertia
}

/// Seconds a warp of `distance` meters takes (EVE University's warp time formula).
/// `warp_speed` is in AU/s, `subwarp_speed` in m/s.
pub fn warp_time(warp_speed: f64, subwarp_speed: f64, distance: f64) -> f64 {
    if distance <= 0.0 || warp_speed <= 0.0 {
        return 0.0;
    }
    let k_accel = warp_speed;
    let k_decel = (warp_speed / 3.0).min(2.0);
    let dropout = (subwarp_speed / 2.0).min(100.0);
    let mut max_speed = warp_speed * AU_METERS;
    let accel_dist = AU_METERS;
    let decel_dist = max_speed / k_decel;
    let minimum = accel_dist + decel_dist;
    let mut cruise = 0.0;
    if minimum > distance {
        max_speed = distance * k_accel * k_decel / (k_accel + k_decel);
    } else {
        cruise = (distance - minimum) / max_speed;
    }
    let accel = (max_speed / k_accel).ln() / k_accel;
    let decel = (max_speed / dropout).ln() / k_decel;
    cruise + accel + decel
}

/// Seconds to lock a target of `sig` meters.
pub fn lock_time(scan_res: f64, sig: f64) -> Option<f64> {
    if scan_res <= 0.0 || sig <= 0.0 {
        return None;
    }
    Some((40_000.0 / scan_res / sig.asinh().powi(2)).min(MAX_LOCK_TIME))
}

/// Sample `f` across `range` the way pyfa's smooth getter does: `resolution` even steps, and
/// up to `depth` rounds of midpoints wherever neighbouring values differ, so steps in the curve
/// (a weapon dropping out of range) stay sharp.
pub fn smooth(
    range: (f64, f64),
    resolution: u32,
    depth: u32,
    mut f: impl FnMut(f64) -> f64,
) -> (Vec<f64>, Vec<f64>) {
    let (low, high) = (range.0.min(range.1), range.0.max(range.1));
    let step = (high - low) / f64::from(resolution);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    if step == 0.0 || step.is_nan() {
        xs.push(low);
        ys.push(f(low));
        return (xs, ys);
    }
    let mut prev: Option<(f64, f64)> = None;
    for i in 0..=resolution {
        let x = low + step * f64::from(i);
        let y = f(x);
        if let Some((px, py)) = prev {
            refine(&mut f, (px, py), (x, y), depth, &mut xs, &mut ys);
        }
        prev = Some((x, y));
        xs.push(x);
        ys.push(y);
    }
    (xs, ys)
}

#[expect(
    clippy::float_cmp,
    reason = "pyfa only refines where neighbouring values differ at all"
)]
fn refine(
    sample: &mut impl FnMut(f64) -> f64,
    left: (f64, f64),
    right: (f64, f64),
    depth: u32,
    xs: &mut Vec<f64>,
    ys: &mut Vec<f64>,
) {
    if depth == 0 || left.1 == right.1 {
        return;
    }
    let mid_x = f64::midpoint(left.0, right.0);
    let mid = (mid_x, sample(mid_x));
    refine(sample, left, mid, depth - 1, xs, ys);
    xs.push(mid.0);
    ys.push(mid.1);
    refine(sample, mid, right, depth - 1, xs, ys);
}

/// Turn values that change at `times` into a step line across `range`, pyfa's time graph
/// shape: flat until each change, then straight up or down.
#[expect(clippy::float_cmp, reason = "a step is any change at all, as in pyfa")]
pub fn steps(
    range: (f64, f64),
    times: &[f64],
    mut f: impl FnMut(f64) -> f64,
) -> (Vec<f64>, Vec<f64>) {
    let (low, high) = (range.0.min(range.1), range.0.max(range.1));
    let mut xs = vec![low];
    let mut ys = vec![f(low)];
    let mut last = ys[0];
    for &t in times.iter().filter(|&&t| t > low && t < high) {
        let y = f(t);
        if y != last {
            xs.extend([t, t]);
            ys.extend([last, y]);
            last = y;
        }
    }
    if high > low {
        xs.push(high);
        ys.push(f(high));
    }
    (xs, ys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-6 * b.abs().max(1.0)
    }

    #[test]
    fn range_factor_halves_at_one_falloff() {
        assert!(close(
            range_factor(10_000.0, 5_000.0, Some(15_000.0), true),
            0.5
        ));
        assert!(close(
            range_factor(10_000.0, 5_000.0, Some(20_000.0), true),
            0.0625
        ));
        assert_eq!(range_factor(10_000.0, 5_000.0, Some(26_000.0), true), 0.0);
        assert!(range_factor(10_000.0, 5_000.0, Some(26_000.0), false) > 0.0);
        assert_eq!(range_factor(10_000.0, 0.0, Some(10_001.0), true), 0.0);
        assert_eq!(range_factor(10_000.0, 0.0, None, true), 1.0);
    }

    #[test]
    fn stacking_matches_eve_numbers() {
        // Two 60% webs: 0.4 * (1 - 0.6 * 0.8691) = 0.19140...
        let mult = stacked_multiplier(&[0.4, 0.4]);
        assert!(
            close(mult, 0.4 * (1.0 - 0.6 * (-1.0 / 7.1289_f64).exp())),
            "{mult}"
        );
        // Bonuses and penalties stack apart.
        assert!(close(stacked_multiplier(&[1.3, 0.5]), 0.65));
    }

    #[test]
    fn turret_mult_counts_wrecking_shots() {
        // A sure hit averages 1.0 * ((0.01 + 1) / 2 + 0.49) for normal hits plus 3x wrecking.
        assert!(close(turret_mult(1.0), 0.99 * 0.995 + 0.03));
        assert!(close(turret_mult(0.005), 0.015));
        assert_eq!(turret_mult(0.0), 0.0);
    }

    #[test]
    fn tracking_halves_hits_at_matching_angular() {
        // Angular 0.1 rad/s, tracking 0.1 rad/s, sig equal to the gun's: chance 0.5.
        let gun = Gun {
            optimal: 1e9,
            falloff: 1.0,
            tracking: 0.1,
            optimal_sig: 40_000.0,
        };
        let motion = Motion {
            tgt_speed: 1000.0,
            tgt_angle: 90.0,
            ..Motion::default()
        };
        let cth = turret_chance_to_hit(&gun, &motion, Some(10_000.0), 40_000.0);
        assert!(close(cth, 0.5), "{cth}");
    }

    #[test]
    fn missile_application_hand_checked() {
        // Cruise missile: er 165, ev 69, drf 0.882 against a 125m frigate doing 400 m/s.
        let f = missile_factor(165.0, 69.0, 0.882, 400.0, 125.0);
        let want = ((69.0 * 125.0) / (165.0 * 400.0_f64)).powf(0.882);
        assert!(close(f, want), "{f} vs {want}");
        assert!(close(
            missile_factor(165.0, 69.0, 0.882, 0.0, 125.0),
            125.0 / 165.0
        ));
        assert_eq!(missile_factor(165.0, 69.0, 0.882, 0.0, 400.0), 1.0);
    }

    #[test]
    fn missile_range_splits_at_whole_seconds() {
        // 4000 m/s, 5.5 s flight, no acceleration time, 100m ship: 5.525 s of flight.
        let r = missile_range(4000.0, 5500.0, 0.0, 0.0, 100.0, None).expect("missile flies");
        assert!(close(r.lower, 20_000.0 - 100.0));
        assert!(close(r.higher, 24_000.0 - 100.0));
        assert!(close(r.higher_chance, 0.525));
        assert_eq!(r.factor(Some(25_000.0)), 0.0);
        assert!(close(r.factor(Some(21_000.0)), 0.525));
    }

    #[test]
    fn regen_peaks_at_a_quarter() {
        let peak = regen_rate(1000.0, 100.0, 250.0);
        assert!(close(peak, 2.5 * 1000.0 / 100.0), "{peak}");
        assert!(close(regen_amount(1000.0, 100.0, 1000.0, 50.0), 1000.0));
        // From empty, after one time constant (tau = 20 s): (1 - e^-1)^2.
        let want = 1000.0 * (1.0 - (-1.0_f64).exp()).powi(2);
        assert!(close(regen_amount(1000.0, 100.0, 0.0, 20.0), want));
    }

    #[test]
    fn acceleration_curve() {
        // Mass 1e6 kg, agility 1: time constant 1 s.
        assert!(close(
            speed_at(100.0, 1e6, 1.0, 1.0),
            100.0 * (1.0 - (-1.0_f64).exp())
        ));
        let d = distance_at(100.0, 1e6, 1.0, 1.0);
        assert!(close(d, 100.0 * (-1.0_f64).exp()), "{d}");
        assert!(close(bump_speed(100.0, 1e6, 1e6), 100.0));
        assert!(close(bump_distance(100.0, 2e6, 0.5), 100.0));
    }

    #[test]
    fn warp_time_matches_eve_uni_example() {
        // 3 AU/s, 10 AU, subwarp 200 m/s: accel 1 AU, decel 3 AU at k_decel 1, cruise 6 AU.
        let t = warp_time(3.0, 200.0, 10.0 * AU_METERS);
        let max = 3.0 * AU_METERS;
        let want = 2.0 + (max / 3.0).ln() / 3.0 + (max / 100.0).ln();
        assert!(close(t, want), "{t} vs {want}");
    }

    #[test]
    fn lock_time_formula() {
        let t = lock_time(400.0, 40.0).expect("positive inputs");
        assert!(close(t, 100.0 / 40.0_f64.asinh().powi(2)));
        assert_eq!(lock_time(1.0, 0.1), Some(MAX_LOCK_TIME));
        assert_eq!(lock_time(0.0, 40.0), None);
    }

    #[test]
    fn smooth_sampling_adds_points_at_steps() {
        let (xs, _) = smooth((0.0, 10.0), 10, 2, |x| if x < 4.2 { 1.0 } else { 0.0 });
        // 11 even points plus 4.5 and then 4.25, homing in on the step at 4.2.
        assert_eq!(xs.len(), 13);
        assert!(xs.contains(&4.25));
        assert!(xs.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn steps_are_flat_then_vertical() {
        // Nothing changes at 2, so only the change at 5 gets a vertical.
        let (xs, ys) = steps((0.0, 10.0), &[2.0, 5.0], |t| (t / 2.5).floor());
        assert_eq!(xs, vec![0.0, 5.0, 5.0, 10.0]);
        assert_eq!(ys, vec![0.0, 0.0, 2.0, 4.0]);
    }
}
