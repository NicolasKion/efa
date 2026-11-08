//! Electronic warfare a fit projects: modules, drones and fighters.

use std::collections::HashSet;

use rc::{
    ItemId, SolOwned,
    stats::{ItemStatsCommon, StatItemChargeOptions, StatTimeOptions},
};

use super::{PlacedDrones, abilities, ability, falloff, max_range};
use crate::engine::read;
use crate::{
    dogma::{attr, effect},
    graphs::snapshot::{Ewar, EwarKind},
};

/// Neutralizing per second as refine counts it for one item.
pub(super) fn item_nps(sol: &mut SolOwned, id: ItemId) -> f64 {
    let mut stats = sol.stats();
    stats
        .get_item(&id)
        .ok()
        .and_then(|mut item| {
            item.get_stat_outgoing_nps(StatTimeOptions::default(), StatItemChargeOptions::Include)
                .ok()
        })
        .map_or(0.0, rc::PValue::into_f64)
}

/// Webs, neutralizers and ECM a launched squadron carries, pyfa's fighter ewar.
pub(super) fn fighter_ewar(sol: &mut SolOwned, id: ItemId) -> Vec<Ewar> {
    let count = sol
        .get_fighter(&id)
        .ok()
        .and_then(|f| f.get_count())
        .map_or(1.0, |c| f64::from(c.current.into_u32()));
    let mut out = Vec::new();
    for (ability_id, on) in abilities(sol, id) {
        if !on {
            continue;
        }
        let (kind, strength, optimal, falloff) = match ability_id {
            ability::WEB => (
                EwarKind::Web,
                read::attr_or_zero(sol, id, attr::FTR_WEB_PENALTY) * count,
                read::attr_or_zero(sol, id, attr::FTR_WEB_OPTIMAL),
                read::attr_or_zero(sol, id, attr::FTR_WEB_FALLOFF),
            ),
            ability::NEUT => (EwarKind::Neut, item_nps(sol, id), f64::INFINITY, 0.0),
            ability::ECM => (
                EwarKind::Ecm,
                attr::FTR_ECM_STRENGTH
                    .iter()
                    .map(|&a| read::attr_or_zero(sol, id, a))
                    .fold(0.0, f64::max)
                    * count,
                f64::INFINITY,
                0.0,
            ),
            _ => continue,
        };
        out.push(Ewar {
            kind,
            strength,
            strength2: 0.0,
            optimal,
            falloff,
            needs_lock: true,
            needs_dcr: false,
            mobile: (kind == EwarKind::Web).then(|| {
                (
                    read::attr_or_zero(sol, id, attr::MAX_VELOCITY),
                    read::attr_or_zero(sol, id, attr::RADIUS),
                )
            }),
        });
    }
    out
}

pub(super) fn module_ewar(sol: &mut SolOwned, id: ItemId, effects: &HashSet<i32>) -> Vec<Ewar> {
    let has = |list: &[i32]| list.iter().any(|e| effects.contains(e));
    let optimal = max_range(sol, id);
    let falloff = falloff(sol, id);
    let dd_range = (optimal + read::attr_or_zero(sol, id, attr::DOOMSDAY_AOE_RANGE)).max(0.0);
    let mut out = Vec::new();
    let mut push = |kind, strength, strength2, doomsday: bool| {
        out.push(Ewar {
            kind,
            strength,
            strength2,
            optimal: if doomsday { dd_range } else { optimal },
            falloff,
            needs_lock: !doomsday,
            needs_dcr: false,
            mobile: None,
        });
    };
    let nos_as_neut = effects.contains(&effect::NOSFERATU)
        && read::attr_or_zero(sol, id, attr::NOS_OVERRIDE) != 0.0;
    if has(&effect::NEUTRALIZER) || nos_as_neut {
        push(EwarKind::Neut, item_nps(sol, id), 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_NEUT) {
        push(EwarKind::Neut, item_nps(sol, id), 0.0, true);
    }
    let web = read::attr_or_zero(sol, id, attr::SPEED_FACTOR);
    if has(&effect::WEBIFIER) {
        push(EwarKind::Web, web, 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_WEB) {
        push(EwarKind::Web, web, 0.0, true);
    }
    let ecm = attr::ECM_STRENGTH
        .iter()
        .map(|&a| read::attr_or_zero(sol, id, a))
        .fold(0.0, f64::max);
    if has(&effect::ECM) {
        push(EwarKind::Ecm, ecm, 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_ECM) {
        push(EwarKind::Ecm, ecm, 0.0, true);
    }
    let damp = read::attr_or_zero(sol, id, attr::MAX_TARGET_RANGE_BONUS);
    let damp_res = read::attr_or_zero(sol, id, attr::SCAN_RESOLUTION_BONUS);
    if has(&effect::SENSOR_DAMPENER) {
        push(EwarKind::Damp, damp, damp_res, false);
    }
    if effects.contains(&effect::DOOMSDAY_DAMP) {
        push(EwarKind::Damp, damp, damp_res, true);
    }
    let td = read::attr_or_zero(sol, id, attr::MAX_RANGE_BONUS);
    if has(&effect::TRACKING_DISRUPTOR) {
        push(EwarKind::TrackingDisruptor, td, 0.0, false);
    }
    let (gd_velocity, gd_flight) = (
        read::attr_or_zero(sol, id, attr::MISSILE_VELOCITY_BONUS),
        read::attr_or_zero(sol, id, attr::EXPLOSION_DELAY_BONUS),
    );
    if has(&effect::GUIDANCE_DISRUPTOR) {
        push(EwarKind::GuidanceDisruptor, gd_velocity, gd_flight, false);
    }
    if effects.contains(&effect::DOOMSDAY_TRACK) {
        push(EwarKind::TrackingDisruptor, td, 0.0, true);
        push(EwarKind::GuidanceDisruptor, gd_velocity, gd_flight, true);
    }
    let tp = read::attr_or_zero(sol, id, attr::SIGNATURE_RADIUS_BONUS);
    if has(&effect::TARGET_PAINTER) {
        push(EwarKind::TargetPainter, tp, 0.0, false);
    }
    if effects.contains(&effect::DOOMSDAY_PAINT) {
        push(EwarKind::TargetPainter, tp, 0.0, true);
    }
    out
}

pub(super) fn drone_ewar(
    sol: &mut SolOwned,
    stack: &PlacedDrones,
    effects: &HashSet<i32>,
) -> Vec<Ewar> {
    let id = stack.id;
    let speed = read::attr_or_zero(sol, id, attr::MAX_VELOCITY);
    let radius = read::attr_or_zero(sol, id, attr::RADIUS);
    let optimal = read::first_attr(sol, id, &[attr::MAX_RANGE, attr::ECM_RANGE_OPTIMAL]);
    let falloff = read::first_attr(sol, id, &[attr::FALLOFF, attr::FALLOFF_EFFECTIVENESS]);
    let mut out = Vec::new();
    let kinds = [
        (effect::ENTITY_NEUTRALIZER, EwarKind::Neut),
        (effect::ENTITY_WEBIFIER, EwarKind::Web),
        (effect::ENTITY_ECM, EwarKind::Ecm),
        (effect::ENTITY_SENSOR_DAMPENER, EwarKind::Damp),
        (effect::ENTITY_WEAPON_DISRUPTOR, EwarKind::TrackingDisruptor),
        (effect::ENTITY_TARGET_PAINTER, EwarKind::TargetPainter),
    ];
    for (effect_id, kind) in kinds {
        if !effects.contains(&effect_id) {
            continue;
        }
        let strength = match kind {
            EwarKind::Neut => item_nps(sol, id),
            EwarKind::Web => read::attr_or_zero(sol, id, attr::SPEED_FACTOR),
            EwarKind::Ecm => attr::ECM_STRENGTH
                .iter()
                .map(|&a| read::attr_or_zero(sol, id, a))
                .fold(0.0, f64::max),
            EwarKind::Damp => read::attr_or_zero(sol, id, attr::MAX_TARGET_RANGE_BONUS),
            EwarKind::TrackingDisruptor => read::attr_or_zero(sol, id, attr::MAX_RANGE_BONUS),
            EwarKind::GuidanceDisruptor | EwarKind::TargetPainter => {
                read::attr_or_zero(sol, id, attr::SIGNATURE_RADIUS_BONUS)
            }
        };
        let strength2 = if kind == EwarKind::Damp {
            read::attr_or_zero(sol, id, attr::SCAN_RESOLUTION_BONUS)
        } else {
            0.0
        };
        // pyfa counts each drone of the stack on its own, so stacking penalties apply.
        for _ in 0..stack.active {
            out.push(Ewar {
                kind,
                strength,
                strength2,
                optimal,
                falloff,
                needs_lock: true,
                needs_dcr: true,
                mobile: Some((speed, radius)),
            });
        }
    }
    out
}
