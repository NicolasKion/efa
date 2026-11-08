//! Acceleration and bumping, warp time and lock time.

use super::{Request, Series, main_range};
use crate::graphs::{
    defs::{Param, Unit},
    math,
    snapshot::Ship,
};

pub(super) fn mobility(req: &Request, ship: &Ship) -> Option<Series> {
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

pub(super) fn warp(req: &Request, ship: &Ship) -> Series {
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

pub(super) fn lock(req: &Request, ship: &Ship) -> Option<Series> {
    let (low, high) = (req.range.0.max(1.0), req.range.1.max(1.0));
    let series = Series::new(math::smooth((low, high), 200, 0, |sig| {
        math::lock_time(ship.scan_res, sig).unwrap_or(f64::NAN)
    }));
    series.ys.iter().all(|y| y.is_finite()).then_some(series)
}
