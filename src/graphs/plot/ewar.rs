//! Electronic warfare by distance, and the hidden ECM burst and damp graph.

use super::{Request, Series, in_range, main_range};
use crate::graphs::{
    defs::{Checkbox, Param},
    math,
    snapshot::{Ewar, EwarKind, FitSnapshot},
};

pub(super) fn ewar(req: &Request, src: &FitSnapshot) -> Series {
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

/// pyfa's ECM burst cycle: a burst breaks every lock every 30 seconds.
pub(super) const ECM_BURST_CYCLE: f64 = 30.0;

/// Seconds drones take to lock again after a burst.
pub(super) const DRONE_LOCK_TIME: f64 = 2.0;

/// The hidden ECM burst and scan resolution damp graph: how long an enemy takes to lock this
/// fit, how long it stays locked between bursts, and what the fit deals meanwhile.
pub(super) fn ecm_burst(req: &Request, src: &FitSnapshot) -> Option<Series> {
    let ship = &src.ship;
    let damp_mult = if req.checkbox(Checkbox::ApplyDamps) {
        let mults: Vec<f64> = src
            .ewar
            .iter()
            .filter(|e| e.kind == EwarKind::Damp)
            .map(|e| 1.0 + e.strength2 / 100.0)
            .collect();
        math::stacked_multiplier(&mults)
    } else {
        1.0
    };
    let weapon_dps = ship.dps - ship.drone_dps;
    let drone_dps = if req.checkbox(Checkbox::ApplyDrones) {
        ship.drone_dps
    } else {
        0.0
    };
    let adjustment = req.value(Param::UptimeAdj).unwrap_or(1.0);
    let limit = req.value(Param::UptimeAmtLimit).unwrap_or(3.0).max(0.0) as u32;
    let lock_time = |scan_res: f64| math::lock_time(scan_res * damp_mult, ship.sig_radius);
    let inflicted = |scan_res: f64, tgt_dps: f64| -> f64 {
        let Some(lock) = lock_time(scan_res) else {
            return f64::NAN;
        };
        let uptime = (ECM_BURST_CYCLE - lock - adjustment).max(0.0);
        let downtime = ECM_BURST_CYCLE - uptime;
        let (mut dealt, mut ehp) = (0.0, ship.ehp);
        for _ in 0..limit {
            let under_fire = if tgt_dps > 0.0 {
                uptime.min(ehp / tgt_dps)
            } else {
                uptime
            };
            let alive = downtime + under_fire;
            ehp -= uptime * tgt_dps;
            dealt += alive * weapon_dps;
            dealt += (alive - DRONE_LOCK_TIME - 1.0).max(0.0) * drone_dps;
            if ehp <= 0.0 {
                break;
            }
        }
        dealt
    };
    let (low, high) = (req.range.0.max(1.0), req.range.1.max(1.0));
    let series = match (req.x.param, req.y.param) {
        (Param::TgtScanRes, Param::TgtLockTime) => {
            math::smooth((low, high), 200, 0, |r| lock_time(r).unwrap_or(f64::NAN))
        }
        (Param::TgtScanRes, Param::TgtLockUptime) => math::smooth((low, high), 200, 0, |r| {
            lock_time(r).map_or(f64::NAN, |t| (ECM_BURST_CYCLE - t).max(0.0))
        }),
        (Param::TgtScanRes, Param::SrcDmg) => {
            let tgt_dps = req.value(Param::TgtDps).unwrap_or(200.0);
            math::smooth((low, high), 200, 0, |r| inflicted(r, tgt_dps))
        }
        (Param::TgtDps, Param::SrcDmg) => {
            let scan_res = req.value(Param::TgtScanRes).unwrap_or(700.0);
            math::smooth(req.range, 200, 0, |d| inflicted(scan_res, d))
        }
        // pyfa has no getter for the other pairs and draws nothing.
        _ => return None,
    };
    Some(Series::new(series))
}
