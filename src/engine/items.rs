//! What the Drones and Boosters tabs show per item, read off a fit built into a solar system.

use std::collections::{HashMap, HashSet};

use rc::{
    ItemId, SolOwned,
    stats::{ItemStatsCommon, StatMiningResourceKind, StatTimeOptions},
};

use super::{pane::PEAK_SHIELD, read};
use crate::dogma::attr;

/// What the Boosters tab shows for a booster, after skills.
#[derive(Debug, Clone, Default)]
pub struct BoosterInfo {
    /// How long a dose lasts, in seconds.
    pub duration: Option<f64>,
    pub side_effects: Vec<SideEffectInfo>,
}

#[derive(Debug, Clone, Copy)]
pub struct SideEffectInfo {
    pub effect_id: i32,
    /// How much it changes its attribute, in percent, where it changes it by one amount.
    pub strength: Option<f64>,
    pub active: bool,
}

/// Per-drone numbers for the drones pane, read off the first drone of a stack (every drone in a
/// stack is the same).
#[derive(Debug, Clone, Default)]
pub struct DroneInfo {
    /// The attributes the drones pane shows, after bonuses; missing ones read as 0.
    pub attrs: HashMap<i32, f64>,
    /// Dogma effects the drone carries. Some attributes default to nonzero values, so pyfa asks
    /// for the effect before reading them.
    pub effects: HashSet<i32>,
    /// Effective HP of all layers together, against uniform damage.
    pub ehp: Option<f64>,
    /// Effective peak passive shield regeneration, per second.
    pub shield_regen: Option<f64>,
    /// Shield, armor and hull repaired per second as if launched.
    pub reps: [f64; 3],
    /// Mining yield and drain in m³ per second as if launched.
    pub mining: (f64, f64),
}

impl DroneInfo {
    /// An attribute after bonuses, if the drone has it with a nonzero value.
    pub fn attr(&self, id: i32) -> Option<f64> {
        self.attrs.get(&id).copied().filter(|&v| v != 0.0)
    }

    pub fn has_effect(&self, id: i32) -> bool {
        self.effects.contains(&id)
    }
}

/// Attributes the drones pane shows.
const DRONE_ATTRS: [i32; 25] = [
    attr::MAX_RANGE,
    attr::SHIELD_TRANSFER_RANGE,
    attr::POWER_TRANSFER_RANGE,
    attr::EMP_FIELD_RANGE,
    attr::ECM_BURST_RANGE,
    attr::ECM_RANGE_OPTIMAL,
    attr::FALLOFF,
    attr::FALLOFF_EFFECTIVENESS,
    attr::MAX_VELOCITY,
    attr::TRACKING_SPEED,
    attr::OPTIMAL_SIG_RADIUS,
    attr::NEUT_AMOUNT,
    attr::NEUT_DURATION,
    attr::FALLOFF_BONUS,
    attr::MAX_RANGE_BONUS,
    attr::TRACKING_SPEED_BONUS,
    attr::SCAN_GRAVIMETRIC_BONUS,
    attr::SCAN_LADAR_BONUS,
    attr::SCAN_MAGNETOMETRIC_BONUS,
    attr::SCAN_RADAR_BONUS,
    attr::ACCESS_DIFFICULTY_BONUS,
    attr::SPEED_FACTOR,
    attr::SIGNATURE_RADIUS_BONUS,
    attr::MAX_TARGET_RANGE_BONUS,
    attr::SCAN_RESOLUTION_BONUS,
];

pub(super) fn booster_info(sol: &mut SolOwned, id: ItemId) -> BoosterInfo {
    use rc::Lender;

    let duration = read::attr(sol, id, attr::BOOSTER_DURATION).map(|ms| ms / 1000.0);
    let mut side_effects = Vec::new();
    if let Ok(mut booster) = sol.get_booster(&id) {
        let mut iter = booster.iter_side_effects();
        while let Some(mut side_effect) = iter.next() {
            let rc::EffectId::Dogma(effect_id) = side_effect.get_effect_id() else {
                continue;
            };
            side_effects.push(SideEffectInfo {
                effect_id: effect_id.into_i32(),
                strength: side_effect
                    .get_strength()
                    .map(|s| s.get_strength().into_f64()),
                active: side_effect.get_state(),
            });
        }
    }
    BoosterInfo {
        duration,
        side_effects,
    }
}

pub(super) fn drone_info(sol: &mut SolOwned, id: ItemId) -> DroneInfo {
    let mut info = DroneInfo {
        attrs: read::attrs(sol, id, &DRONE_ATTRS),
        effects: read::effects(sol, id),
        ..DroneInfo::default()
    };
    let mut stats = sol.stats();
    let Ok(mut item) = stats.get_item(&id) else {
        return info;
    };
    if let Ok(ehp) = item.get_stat_ehp(None) {
        let layers = [ehp.shield, ehp.armor, ehp.hull];
        info.ehp = Some(
            layers
                .iter()
                .flatten()
                // refine's buffer already has the resists applied.
                .map(|l| l.buffer.into_f64())
                .sum(),
        );
    }
    info.shield_regen = item
        .get_stat_erps(
            None,
            StatTimeOptions::default(),
            rc::UnitInterval::from_f64_clamped(PEAK_SHIELD),
        )
        .ok()
        .and_then(|e| e.shield)
        .map(|s| s.regen.into_f64());
    // pyfa shows what a drone does even while it sits in the bay.
    let mut switched = item.switch_states();
    if let Ok(reps) = switched.get_stat_outgoing_rps(StatTimeOptions::default()) {
        info.reps = [reps.shield, reps.armor, reps.hull].map(rc::PValue::into_f64);
    }
    if let Ok(mining) =
        switched.get_stat_mps(StatTimeOptions::default(), StatMiningResourceKind::Regular)
    {
        let rate = read::mining_rate(&mining);
        info.mining = (rate.yield_, rate.drain);
    }
    info
}
