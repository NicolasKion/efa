//! A quick look at a fit's tank and fitting room, for searches that try many fits: the fitting
//! wizard's tank optimiser. It reads only what the score needs, so it is a good deal cheaper than
//! [`Engine::compute_for`].

use rc::{AttrId, EveAttrId, ItemCommon, ItemId, SolOwned};

use super::{
    CalcOptions, Cap, CapDetail, Engine, Layer, Pilot, Resource, Stats, Tank, attr, build_local,
    fit_stats, pane, rah_incoming, sec_zone,
};
use crate::fit::{Fit, ModuleState};

/// What the optimiser scores a fit by.
#[derive(Debug, Clone, Copy, Default)]
pub struct TankProbe {
    pub cpu: Resource,
    pub powergrid: Resource,
    pub calibration: Resource,
    pub shield: Layer,
    pub armor: Layer,
    pub hull: Layer,
    /// The damage mix the layers are judged against: EM, thermal, kinetic, explosive.
    pub incoming: [f64; 4],
    /// Repair rates the capacitor can keep up, reloads counted.
    pub sustained: Tank,
    pub cap_stable: bool,
}

impl TankProbe {
    /// Whether CPU, powergrid and calibration are within the ship's output.
    pub fn fits(&self) -> bool {
        [&self.cpu, &self.powergrid, &self.calibration]
            .into_iter()
            .all(|r| r.output.is_none_or(|out| r.used <= out + 1e-6))
    }
}

impl Engine {
    /// The fit's resources, layers and sustained repair, flown by `pilot` alone in space.
    pub fn probe_tank(&self, fit: &Fit, pilot: &Pilot) -> TankProbe {
        let mut sol = self.sol();
        sol.set_sec_zone(sec_zone(fit.system_security));
        let mut f = sol.add_fit();
        let Some(local) = build_local(&mut f, fit, pilot, &mut Vec::new()) else {
            return TankProbe::default();
        };
        let fit_id = f.get_fit_id();
        let mut stats = Stats {
            incoming: pane::incoming_mix(&mut f, &fit.damage_profile),
            ..Stats::default()
        };
        let incoming = pane::dps_profile(stats.incoming);
        let rah = rah_incoming(&mut f, fit, incoming);
        f.set_rah_incoming_dps(Some(rah));
        // Reloads count, so an ancillary repairer is judged by what it repairs on average.
        let options = CalcOptions {
            factor_reload: true,
            ..CalcOptions::default()
        };
        fit_stats(&mut f, &mut stats, incoming, options);
        let (recharge, used) = pane::cap_flow(&mut f.stats());
        let time = options.time();
        let base = pane::tank(&mut f, time);
        let cap_stable = matches!(stats.cap, Some(Cap::Stable(_)));
        let sustained = if cap_stable {
            base
        } else {
            let users = cap_users(&mut sol, fit, &local.module_ids);
            let cap = CapDetail {
                recharge,
                used,
                ..CapDetail::default()
            };
            pane::sustained(&mut sol, fit_id, base, &users, time, cap)
        };
        TankProbe {
            cpu: stats.cpu,
            powergrid: stats.powergrid,
            calibration: stats.calibration,
            shield: stats.shield,
            armor: stats.armor,
            hull: stats.hull,
            incoming: stats.incoming,
            sustained,
            cap_stable,
        }
    }
}

/// The active modules that use capacitor, with their GJ/s.
fn cap_users(
    sol: &mut SolOwned,
    fit: &Fit,
    module_ids: &[(crate::data::Slot, usize, ItemId)],
) -> Vec<(ItemId, f64)> {
    module_ids
        .iter()
        .filter(|&&(slot, i, _)| {
            fit.rack(slot)
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|m| matches!(m.state, ModuleState::Active | ModuleState::Overload))
        })
        .filter_map(|&(_, _, id)| {
            let mut item = sol.get_item(&id).ok()?;
            let mut get = |a: i32| {
                item.get_attr(&AttrId::Eve(EveAttrId::from_i32(a)))
                    .ok()
                    .map(|v| v.modified.into_f64())
                    .filter(|&v| v > 0.0)
            };
            let need = get(attr::CAP_NEED)?;
            let cycle = get(attr::DURATION).or_else(|| get(attr::RATE_OF_FIRE))?;
            Some((id, need / (cycle / 1000.0)))
        })
        .collect()
}
