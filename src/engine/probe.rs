//! A quick look at a fit's tank and fitting room, for searches that try many fits: the fitting
//! wizard's tank optimiser. The fit is built once and each tank tried goes on and comes off again
//! in a transaction, and only what the score needs is read, so a probe costs a fraction of
//! [`Engine::compute_for`].

use rc::{ItemTypeId, ModRack};

use super::{
    BuildOptions, Built, CalcOptions, Cap, Engine, Layer, Pilot, Resource, Tank,
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
    /// Local repairs per second while the repairers run.
    pub repairs: Tank,
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
}

/// Reloads count, so an ancillary repairer is judged by what it repairs on average.
const PROBE_OPTIONS: CalcOptions = CalcOptions {
    factor_reload: true,
    default_spool: None,
};

impl Engine {
    /// The fit's resources, layers and repair, flown by `pilot` alone in space.
    pub fn probe_tank(&self, fit: &Fit, pilot: &Pilot) -> TankProbe {
        self.tank_bench(fit, pilot)
            .map_or_default(|mut bench| bench.probe(&[], &[]))
    }

    /// `fit` flown by `pilot` alone in space, built to try tanks on. None without a ship.
    pub fn tank_bench(&self, fit: &Fit, pilot: &Pilot) -> Option<TankBench> {
        Some(TankBench {
            built: self.build(fit, pilot, BuildOptions::default())?,
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
        for &(slot, i, module) in modules {
            let Some(rack) = rack(slot) else { continue };
            let _ = add_fit_module(&mut f, rack, at(i), module);
        }
        for &rig in rigs {
            f.add_rig(ItemTypeId::from_i32(rig));
        }

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
        // The repairs while the repairers cycle, cap permitting: an active tank fights on its
        // cap booster, so whether the capacitor lasts is the preview's warning, not the score.
        probe.repairs = pane::repairs(&mut f, time);
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
