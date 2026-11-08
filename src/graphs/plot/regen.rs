//! Shield and capacitor regeneration, and the capacitor simulation.

use super::{MAX_CAP_TIME, Request, Series, main_range};
use crate::graphs::{
    capsim,
    defs::{Checkbox, Param, Unit},
    math,
    snapshot::{FitSnapshot, Ship},
};

pub(super) fn shield(req: &Request, ship: &Ship) -> Series {
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

pub(super) fn capacitor(req: &Request, src: &FitSnapshot) -> Series {
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
pub(super) fn cap_sim_series(
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
