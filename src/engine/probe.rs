//! A quick look at a fit's tank and fitting room, for searches that try many fits: the fitting
//! wizard's tank optimiser. The fit is built once and each tank tried goes on and comes off again
//! in a transaction, and only what the score needs is read, so a probe costs a fraction of
//! [`Engine::compute_for`].

use std::collections::HashSet;

use rc::{AttrId, EveAttrId, ItemCommon, ItemId, ItemTypeId, ModRack, Sol};

use super::{
    BuildOptions, Built, CalcOptions, Cap, CapDetail, Engine, Layer, Pilot, Resource, Tank, attr,
    build::{add_fit_module, at},
    pane, resource, stats,
};
use crate::{
    data::{Slot, TypeId},
    fit::{Fit, Module},
};

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
    /// Repair rates the capacitor can keep up, reloads counted; armor spool-up is left out.
    pub sustained: Tank,
    pub cap_stable: bool,
}

impl TankProbe {
    /// Whether CPU, powergrid and calibration are within the ship's output.
    pub fn fits(&self) -> bool {
        [&self.cpu, &self.powergrid, &self.calibration]
            .into_iter()
            .all(|r| !r.is_over())
    }
}

/// A fit built once to try tanks on: modules go into its free rack positions and rigs into its
/// free rig slots, one tank at a time. Each search thread takes a copy of its own.
#[derive(Clone)]
pub struct TankBench {
    built: Built,
    fit: Fit,
    /// Module types that repair nothing, learnt along the way so they are not switched off
    /// again to find out.
    idle: HashSet<i32>,
}

/// Reloads count, so an ancillary repairer is judged by what it repairs on average.
const PROBE_OPTIONS: CalcOptions = CalcOptions {
    factor_reload: true,
    default_spool: None,
};

impl Engine {
    /// The fit's resources, layers and sustained repair, flown by `pilot` alone in space.
    pub fn probe_tank(&self, fit: &Fit, pilot: &Pilot) -> TankProbe {
        self.tank_bench(fit, pilot)
            .map_or_default(|mut bench| bench.probe(&[], &[]))
    }

    /// `fit` flown by `pilot` alone in space, built to try tanks on. None without a ship.
    pub fn tank_bench(&self, fit: &Fit, pilot: &Pilot) -> Option<TankBench> {
        Some(TankBench {
            built: self.build(fit, pilot, BuildOptions::default())?,
            fit: fit.clone(),
            idle: HashSet::new(),
        })
    }
}

impl TankBench {
    /// The probe of the fit with `modules` put on at their rack positions and `rigs` fitted.
    /// The bench is left as it was.
    pub fn probe(&mut self, modules: &[(Slot, usize, &Module)], rigs: &[TypeId]) -> TankProbe {
        let fit_id = self.built.fit.fit_id;
        let incoming = self.built.incoming;
        let mut tx = self.built.sol.begin_tx();
        let Ok(mut f) = tx.get_fit(&fit_id) else {
            return TankProbe::default();
        };
        // Running modules in rack order, the fit's and the tank's, for their capacitor use.
        let mut running: Vec<(Slot, usize, ItemId, TypeId)> = self
            .built
            .fit
            .modules
            .iter()
            .filter_map(|&(slot, i, id)| {
                let m = self.fit.rack(slot).get(i)?.as_ref()?;
                m.state.is_running().then_some((slot, i, id, m.type_id))
            })
            .collect();
        for &(slot, i, module) in modules {
            let Some(rack) = rack(slot) else { continue };
            if let Ok(added) = add_fit_module(&mut f, rack, at(i), module)
                && module.state.is_running()
            {
                running.push((slot, i, added.get_item_id(), module.type_id));
            }
        }
        for &rig in rigs {
            f.add_rig(ItemTypeId::from_i32(rig));
        }
        running.sort_by_key(|&(slot, i, ..)| (slot, i));

        let mut s = f.stats();
        let [shield, armor, hull] = stats::layers(&mut s, pane::dps_profile(incoming));
        let mut probe = TankProbe {
            cpu: resource(s.get_stat_cpu()),
            powergrid: resource(s.get_stat_powergrid()),
            calibration: resource(s.get_stat_calibration()),
            shield,
            armor,
            hull,
            incoming,
            cap_stable: matches!(stats::cap(&mut s, PROBE_OPTIONS), Some(Cap::Stable(_))),
            ..TankProbe::default()
        };
        let time = PROBE_OPTIONS.time();
        let base = pane::repairs(&mut f, time);
        probe.sustained = if probe.cap_stable {
            base
        } else {
            let (recharge, used) = pane::cap_flow(&mut f.stats());
            let cap = CapDetail {
                recharge,
                used,
                ..CapDetail::default()
            };
            // Only repairers change the repairs when switched off; the rest are known by type
            // once tried.
            let tried: Vec<(ItemId, f64, TypeId)> = running
                .iter()
                .filter(|&&(.., type_id)| !self.idle.contains(&type_id))
                .filter_map(|&(_, _, id, type_id)| Some((id, cap_per_sec(&mut tx, id)?, type_id)))
                .collect();
            let pairs: Vec<(ItemId, f64)> = tried.iter().map(|&(id, cap, _)| (id, cap)).collect();
            let shares = pane::repair_shares(&mut tx, fit_id, &pairs, time);
            let mut repairers = Vec::new();
            for (share, &(_, cap, type_id)) in shares.into_iter().zip(&tried) {
                match share {
                    Some(share) => repairers.push((share, cap)),
                    None => {
                        self.idle.insert(type_id);
                    }
                }
            }
            pane::sustain(base, repairers, cap)
        };
        probe
    }
}

/// The rack a module of this slot goes in; None for slots tanks are not built in.
fn rack(slot: Slot) -> Option<ModRack> {
    match slot {
        Slot::High => Some(ModRack::High),
        Slot::Mid => Some(ModRack::Mid),
        Slot::Low => Some(ModRack::Low),
        Slot::Rig | Slot::Subsystem | Slot::Service => None,
    }
}

/// Capacitor a module uses per second while it cycles, None when it uses none.
fn cap_per_sec(sol: &mut Sol, id: ItemId) -> Option<f64> {
    let mut item = sol.get_item(&id).ok()?;
    let mut get = |a: i32| {
        item.get_attr(&AttrId::Eve(EveAttrId::from_i32(a)))
            .ok()
            .map(|v| v.modified.into_f64())
            .filter(|&v| v > 0.0)
    };
    let need = get(attr::CAP_NEED)?;
    let cycle = get(attr::DURATION).or_else(|| get(attr::RATE_OF_FIRE))?;
    Some(need / (cycle / 1000.0))
}
