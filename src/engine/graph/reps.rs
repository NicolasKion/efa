//! Remote repairs a fit gives: modules and drones.

use rc::{
    ItemId, SolOwned,
    stats::{ItemStatsCommon, StatTimeOptions},
};

use super::{PlacedDrones, falloff, max_range, reload_of, spool_at, spool_cycles};
use crate::engine::read;
use crate::{
    dogma::{attr, group},
    graphs::snapshot::{Cycle, Layers, RemoteRep},
};

/// Shield, armor and hull repaired per second by one item at the given spool.
pub(super) fn item_rps(sol: &mut SolOwned, id: ItemId, time: StatTimeOptions) -> Layers {
    let mut stats = sol.stats();
    stats
        .get_item(&id)
        .ok()
        .and_then(|mut item| item.get_stat_outgoing_rps(time).ok())
        .map_or([0.0; 3], |r| {
            [r.shield, r.armor, r.hull].map(rc::PValue::into_f64)
        })
}

pub(super) fn module_rep(sol: &mut SolOwned, id: ItemId, group: i32) -> Option<RemoteRep> {
    let shield = matches!(
        group,
        group::REMOTE_SHIELD_BOOSTER | group::ANCILLARY_REMOTE_SHIELD_BOOSTER
    );
    let armor_or_hull = matches!(
        group,
        group::REMOTE_ARMOR_REPAIRER
            | group::REMOTE_HULL_REPAIRER
            | group::ANCILLARY_REMOTE_ARMOR_REPAIRER
            | group::MUTADAPTIVE_REMOTE_ARMOR_REPAIRER
    );
    if !shield && !armor_or_hull {
        return None;
    }
    let duration = read::attr_or_zero(sol, id, attr::DURATION) / 1000.0;
    if duration <= 0.0 {
        return None;
    }
    let spool = spool_cycles(sol, id);
    let per_cycle = |rps: Layers| rps.map(|v| v * duration);
    let amounts: Vec<Layers> = if spool > 0 {
        (0..=spool)
            .map(|n| per_cycle(item_rps(sol, id, spool_at(n))))
            .collect()
    } else {
        vec![per_cycle(item_rps(sol, id, StatTimeOptions::default()))]
    };
    if amounts.iter().all(|a| a.iter().sum::<f64>() <= 0.0) {
        return None;
    }
    let (shots, reload) = reload_of(sol, id);
    // Armor and hull repairs land at the end of the cycle, shield boosts at its start.
    let delay = if shield { 0.0 } else { duration };
    let cycle = Cycle {
        duration,
        shots,
        reload,
        amounts: amounts.clone(),
        delay,
    };
    let ancillary = matches!(
        group,
        group::ANCILLARY_REMOTE_SHIELD_BOOSTER | group::ANCILLARY_REMOTE_ARMOR_REPAIRER
    );
    let no_reload = (ancillary && shots.is_some()).then(|| {
        let charged = amounts[0];
        // Out of paste, an ancillary armor repairer keeps going at its plain amount.
        let plain = if group == group::ANCILLARY_REMOTE_ARMOR_REPAIRER {
            let mult = read::attr_or_zero(sol, id, attr::CHARGED_ARMOR_DAMAGE_MULTIPLIER);
            if mult > 0.0 {
                charged.map(|v| v / mult)
            } else {
                charged
            }
        } else {
            charged
        };
        let mut amounts = vec![charged; shots.map_or(0, |s| s as usize)];
        amounts.push(plain);
        Cycle {
            duration,
            shots: None,
            reload: 0.0,
            amounts,
            delay,
        }
    });
    Some(RemoteRep {
        cycle,
        no_reload,
        optimal: max_range(sol, id),
        falloff: falloff(sol, id),
        drone: false,
    })
}

pub(super) fn drone_rep(sol: &mut SolOwned, stack: &PlacedDrones) -> Option<RemoteRep> {
    let id = stack.id;
    let rps = item_rps(sol, id, StatTimeOptions::default());
    if rps.iter().sum::<f64>() <= 0.0 {
        return None;
    }
    let duration = read::first_attr(sol, id, &[attr::DURATION, attr::RATE_OF_FIRE]) / 1000.0;
    let duration = if duration > 0.0 { duration } else { 1.0 };
    let count = f64::from(stack.active);
    Some(RemoteRep {
        cycle: Cycle {
            duration,
            shots: None,
            reload: 0.0,
            amounts: vec![rps.map(|v| v * duration * count)],
            delay: duration,
        },
        no_reload: None,
        optimal: 0.0,
        falloff: 0.0,
        drone: true,
    })
}
