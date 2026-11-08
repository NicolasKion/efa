//! Remote repairs by distance and time.

use super::{MAX_TIME, Request, Series, Timeline, change_times, in_range, main_range};
use crate::graphs::{
    defs::{Checkbox, Param},
    math,
    snapshot::{Cycle, FitSnapshot, Layers},
};

pub(super) fn remote_reps(req: &Request, src: &FitSnapshot) -> Option<Series> {
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
                Some(t) => Some(Timeline::new(c, t).value(y, t).iter().sum()),
            })
            .collect()
    };
    // How much of each repairer's output lands at `distance`.
    let mults = |distance: Option<f64>| -> Vec<f64> {
        let in_lock = in_range(ship.lock_range, distance, opts.ignore_lock_range);
        let in_dcr = in_range(ship.drone_control_range, distance, opts.ignore_dcr);
        src.reps
            .iter()
            .map(|r| {
                if !in_lock {
                    0.0
                } else if r.drone {
                    if in_dcr { 1.0 } else { 0.0 }
                } else {
                    math::range_factor(r.optimal, r.falloff, distance, true)
                }
            })
            .collect()
    };
    let applied = |values: &[f64], mults: &[f64]| -> f64 {
        values.iter().zip(mults).map(|(v, m)| v * m).sum()
    };
    let series = if req.x.param == Param::Distance {
        let time = req.value(Param::Time).map(|t| t.clamp(0.0, MAX_TIME));
        let values = values(time)?;
        Series::new(math::smooth(main_range(req, 1000.0), 50, 2, |d| {
            applied(&values, &mults(Some(d)))
        }))
        .map_x(|m| m / 1000.0)
    } else {
        let distance = req.value(Param::Distance).map(|km| km * 1000.0);
        let (low, high) = (
            req.range.0.clamp(0.0, MAX_TIME),
            req.range.1.clamp(0.0, MAX_TIME),
        );
        let times = change_times(&cycles, high, total);
        let timelines: Vec<_> = cycles.iter().map(|c| Timeline::new(c, high)).collect();
        let mults = mults(distance);
        Series::new(math::steps((low, high), &times, |t| {
            let values: Vec<f64> = timelines
                .iter()
                .map(|tl| tl.value(y, t).iter().sum())
                .collect();
            applied(&values, &mults)
        }))
    };
    Some(series)
}

/// Shield, armor and hull repaired per second over a whole cycle and reload.
pub(super) fn average_rate(cycle: &Cycle<Layers>) -> f64 {
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
