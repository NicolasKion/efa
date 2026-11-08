//! The tank section: searches tanks for one layer of the current fit and its rigs, scores each
//! with the engine against the fit's damage pattern for one goal, and keeps the best. The rest
//! of the fit stays as it is.
//!
//! Trying every combination is out of reach on a battleship, so it searches in stages: each
//! stage adds one module to the best few tanks of the stage before, and the rigs are tried on the
//! best tanks last. Tanks that do not fit the CPU, powergrid or calibration are dropped.

mod candidates;
mod search;

use std::{
    collections::{HashMap, HashSet},
    ops::AddAssign,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use serde::{Deserialize, Serialize};

use super::{Budget, Change, Layer, SearchTiers};
use crate::{
    data::{Catalog, Slot, TypeId},
    engine::{CalcOptions, Cap, Engine, Layer as EngineLayer, Pilot, Resource, Stats, TankProbe},
    fit::{Fit, Module, ModuleState},
};

pub use search::{optimise, score};

/// What a tank is tuned for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goal {
    /// The most effective HP of all layers.
    #[default]
    Ehp,
    /// The highest resist multiplier on the layer: EHP per raw HP, so repairs land hardest.
    Resists,
    /// The most effective HP the layer's repairers bring back a second.
    Repair,
}

impl Goal {
    pub const ALL: [Self; 3] = [Self::Ehp, Self::Resists, Self::Repair];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ehp => "Max EHP",
            Self::Resists => "Max resists",
            Self::Repair => "Max repair",
        }
    }

    /// The goal in a word, for the goal switch.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Ehp => "EHP",
            Self::Resists => "Resists",
            Self::Repair => "Repair",
        }
    }
}

/// What the user picks on the tank section. All but the layer is remembered for the next fit;
/// the layer goes by the hull.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choices {
    #[serde(skip)]
    pub layer: Layer,
    pub goal: Goal,
    /// Which variations of each module and rig the search tries.
    pub search_tiers: SearchTiers,
    /// How many repairers or boosters a repair tank ends up with; None leaves it to the search.
    pub repairers: Option<u8>,
    pub budget: Budget,
    /// Take off the tank modules and rigs fitted on the layer and search their slots too,
    /// rather than only the free ones.
    pub refit: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            layer: Layer::Shield,
            goal: Goal::default(),
            search_tiers: SearchTiers::default(),
            repairers: None,
            budget: Budget::default(),
            refit: true,
        }
    }
}

impl Choices {
    /// The repairer count that applies: only a repair tank takes one.
    fn repairers(&self) -> Option<usize> {
        self.repairers
            .filter(|_| self.goal == Goal::Repair)
            .map(usize::from)
    }
}

/// What a module does for the tank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Repair,
    Buffer,
    Resist,
    /// Capacitor or boost amplification for the repairers.
    Support,
}

/// A module the optimiser can fit.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub base: TypeId,
    pub module: Module,
    pub role: Role,
    /// How many one tank takes at most.
    pub max: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RigCandidate {
    pub base: TypeId,
    pub type_id: TypeId,
    /// A powergrid or CPU rig: worth fitting first, for the room it makes for the modules.
    pub fitting: bool,
}

/// One search, with everything it needs, so it can run off the UI thread.
#[derive(Debug, Clone)]
pub struct Problem {
    /// The fit without the tank being redone, weapons idle so the capacitor goes to the tank.
    pub fit: Fit,
    pub pilot: Pilot,
    pub layer: Layer,
    pub goal: Goal,
    /// Free positions in the layer's rack, once the fitted tank is off.
    pub positions: Vec<usize>,
    /// Free rig positions, the same way.
    pub rig_positions: Vec<usize>,
    /// The fitted tank modules and rigs the search takes off.
    pub removed: Vec<(Slot, usize)>,
    pub candidates: Vec<Candidate>,
    pub rigs: Vec<RigCandidate>,
    /// Only tanks with a repairer or booster count.
    pub need_repair: bool,
    /// How many repairers or boosters the search adds, exactly; None leaves it open.
    pub repairers: Option<usize>,
    /// Known prices of the candidates and rigs, by type.
    pub prices: HashMap<TypeId, f64>,
    /// The most the whole tank may cost, in ISK.
    pub max_total: Option<f64>,
}

impl Problem {
    /// Every module and rig the search may try, for fetching their prices first.
    pub fn types(&self) -> HashSet<TypeId> {
        self.candidates
            .iter()
            .map(|c| c.module.type_id)
            .chain(self.rigs.iter().map(|r| r.type_id))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() && self.rig_positions.is_empty()
    }
}

/// What a tank has to work with on the fit.
#[derive(Debug, Clone)]
pub struct Room {
    /// The fit with the tank being redone taken off.
    pub fit: Fit,
    /// The positions of what was taken off.
    pub removed: Vec<(Slot, usize)>,
    /// Free positions in the layer's rack and the rigs.
    pub positions: Vec<usize>,
    pub rig_positions: Vec<usize>,
}

/// A tank's numbers on its layer, judged with the guns idle so the capacitor goes to the tank.
#[derive(Debug, Clone, Copy, Default)]
pub struct Numbers {
    /// Effective HP of all layers.
    pub ehp: f64,
    /// The tanked layer's resist against the damage pattern.
    pub resist: f64,
    /// The tanked layer's EHP per raw HP.
    pub multiplier: f64,
    /// The tanked layer's sustained repair, raw HP/s.
    pub repair: f64,
    /// That repair in effective HP/s: times the multiplier.
    pub repair_ehp: f64,
    /// Shield, armor and hull.
    pub layers: [EngineLayer; 3],
    pub cap_stable: bool,
    pub cpu: Resource,
    pub powergrid: Resource,
    pub calibration: Resource,
}

impl Numbers {
    pub fn of(probe: &TankProbe, layer: Layer) -> Self {
        let (stats, repair) = match layer {
            Layer::Shield => (probe.shield, probe.sustained.shield),
            Layer::Armor => (probe.armor, probe.sustained.armor),
        };
        let multiplier = stats.mult.unwrap_or(1.0).max(1.0);
        let layers = [probe.shield, probe.armor, probe.hull];
        Self {
            ehp: layers.iter().map(|l| l.ehp.unwrap_or(l.hp)).sum(),
            resist: 1.0 - 1.0 / multiplier,
            multiplier,
            repair,
            repair_ehp: repair * multiplier,
            layers,
            cap_stable: probe.cap_stable,
            cpu: probe.cpu,
            powergrid: probe.powergrid,
            calibration: probe.calibration,
        }
    }

    /// The numbers of `fit` with `change` made.
    pub fn after(engine: &Engine, pilot: &Pilot, fit: &Fit, change: &Change, layer: Layer) -> Self {
        Self::of(&engine.probe_tank(&judged(fit, change), pilot), layer)
    }

    /// The numbers in a fit's full stats, for when those are worked out anyway.
    pub fn of_stats(stats: &Stats, layer: Layer) -> Self {
        Self::of(
            &TankProbe {
                cpu: stats.cpu,
                powergrid: stats.powergrid,
                calibration: stats.calibration,
                shield: stats.shield,
                armor: stats.armor,
                hull: stats.hull,
                incoming: stats.incoming,
                sustained: stats.sustained,
                cap_stable: matches!(stats.cap, Some(Cap::Stable(_))),
            },
            layer,
        )
    }

    /// The tanked layer's stats.
    pub fn layer(&self, layer: Layer) -> EngineLayer {
        match layer {
            Layer::Shield => self.layers[0],
            Layer::Armor => self.layers[1],
        }
    }

    /// The number a goal ranks tanks by.
    pub fn value(&self, goal: Goal) -> f64 {
        match goal {
            Goal::Ehp => self.ehp,
            Goal::Resists => self.multiplier,
            Goal::Repair => self.repair_ehp,
        }
    }
}

/// `fit` with `change` made, as the tank numbers judge it: the guns idle.
pub fn judged(fit: &Fit, change: &Change) -> Fit {
    let mut next = fit.clone();
    super::apply(&mut next, change);
    idle_guns(&mut next);
    next
}

/// The full stats of `fit` with `change` made, judged the way [`Numbers::after`] judges it.
pub fn stats_after(engine: &Engine, pilot: &Pilot, fit: &Fit, change: &Change) -> (Fit, Stats) {
    let next = judged(fit, change);
    let options = CalcOptions {
        factor_reload: true,
        ..CalcOptions::default()
    };
    let stats = engine.compute_for(&next, pilot, options);
    (next, stats)
}

/// Turn active guns off, so the capacitor goes to the tank.
fn idle_guns(fit: &mut Fit) {
    for module in fit.high.iter_mut().flatten() {
        if module.state.is_running() {
            module.state = ModuleState::Online;
        }
    }
}

/// A tank the optimiser found, with its numbers.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The modules, repairers first, and what each is loaded with.
    pub types: Vec<TypeId>,
    pub charges: Vec<Option<TypeId>>,
    pub rig_types: Vec<TypeId>,
    pub score: f64,
    pub numbers: Numbers,
    /// What the tank's modules and rigs cost, as far as their prices are known.
    pub price: f64,
    /// Whether every module and rig had a price.
    pub priced: bool,
}

impl Outcome {
    /// What fitting this tank does to the fit the problem was made from.
    pub fn change(&self, problem: &Problem, catalog: &Catalog) -> Change {
        let slot = problem.layer.slot();
        let modules = self
            .types
            .iter()
            .zip(&self.charges)
            .zip(&problem.positions)
            .map(|((&id, &charge), &at)| {
                let mut module = Module::new(catalog, id);
                module.charge = charge;
                (slot, at, module)
            });
        let rigs = self
            .rig_types
            .iter()
            .zip(&problem.rig_positions)
            .map(|(&id, &at)| (Slot::Rig, at, Module::new(catalog, id)));
        Change {
            removed: problem.removed.clone(),
            placements: modules.chain(rigs).collect(),
        }
    }
}

/// A fitting resource a tank can run out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Cpu,
    Powergrid,
    Calibration,
    /// The total budget.
    Budget,
}

impl Limit {
    pub fn label(self) -> &'static str {
        match self {
            Limit::Cpu => "CPU",
            Limit::Powergrid => "Powergrid",
            Limit::Calibration => "Calibration",
            Limit::Budget => "The budget",
        }
    }
}

/// Tanks the search dropped for not fitting, by the resource they ran out of. A tank short of
/// more than one counts for each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rejected {
    pub cpu: usize,
    pub powergrid: usize,
    pub calibration: usize,
    /// Over the total budget.
    pub budget: usize,
}

impl Rejected {
    /// What the probed fit runs out of; all zero when it fits.
    pub fn of(probe: &TankProbe) -> Self {
        let over = |r: &Resource| usize::from(r.is_over());
        Self {
            cpu: over(&probe.cpu),
            powergrid: over(&probe.powergrid),
            calibration: over(&probe.calibration),
            budget: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The resource most dropped tanks ran out of, None when none were dropped.
    pub fn limit(&self) -> Option<Limit> {
        [
            (self.cpu, Limit::Cpu),
            (self.powergrid, Limit::Powergrid),
            (self.calibration, Limit::Calibration),
            (self.budget, Limit::Budget),
        ]
        .into_iter()
        .filter(|&(n, _)| n > 0)
        // The first of a tie wins.
        .rev()
        .max_by_key(|&(n, _)| n)
        .map(|(_, limit)| limit)
    }
}

impl AddAssign for Rejected {
    fn add_assign(&mut self, other: Self) {
        self.cpu += other.cpu;
        self.powergrid += other.powergrid;
        self.calibration += other.calibration;
        self.budget += other.budget;
    }
}

/// What a search found: the best tanks, best first, and what it dropped for not fitting.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub outcomes: Vec<Outcome>,
    pub rejected: Rejected,
    /// With a number of repairers chosen, how many the results have when fewer fit.
    pub repairers_short: Option<usize>,
}

/// How far a search is, shared with the UI, which can also call it off.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    cancel: AtomicBool,
}

impl Progress {
    /// Roughly how much of the search is done, 0 to 1.
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed).max(1);
        (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(used: f64, output: f64) -> Resource {
        Resource {
            used,
            output: Some(output),
        }
    }

    #[test]
    fn a_probe_short_of_cpu_counts_for_cpu() {
        let probe = TankProbe {
            cpu: resource(400.0, 350.0),
            powergrid: resource(900.0, 1000.0),
            calibration: resource(400.0, 400.0),
            ..TankProbe::default()
        };
        let rejected = Rejected::of(&probe);
        assert_eq!(
            rejected,
            Rejected {
                cpu: 1,
                ..Rejected::default()
            }
        );
        assert_eq!(rejected.limit(), Some(Limit::Cpu));
    }

    #[test]
    fn the_limit_is_what_most_tanks_ran_out_of() {
        assert_eq!(Rejected::default().limit(), None);
        let mut rejected = Rejected {
            cpu: 3,
            powergrid: 5,
            calibration: 0,
            budget: 0,
        };
        assert_eq!(rejected.limit(), Some(Limit::Powergrid));
        rejected += Rejected {
            cpu: 2,
            ..Rejected::default()
        };
        // A tie goes to the first.
        assert_eq!(rejected.limit(), Some(Limit::Cpu));
    }

    #[test]
    fn choices_survive_a_round_trip_without_the_layer() {
        let choices = Choices {
            layer: Layer::Armor,
            goal: Goal::Repair,
            repairers: Some(2),
            refit: false,
            ..Choices::default()
        };
        let json = serde_json::to_string(&choices).expect("serializes");
        let back: Choices = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(
            back,
            Choices {
                layer: Layer::Shield,
                ..choices
            }
        );
    }

    #[test]
    fn older_saves_read_as_the_defaults() {
        // The guided wizard saved its bling choices and its last step in the same places.
        let old = r#"{"tier":"Faction","categories":{"Rig":"T2"},"exact":{}}"#;
        let remembered: super::super::Remembered = serde_json::from_str(old).expect("reads");
        assert_eq!(remembered, super::super::Remembered::default());
        for step in ["\"Loadout\"", "\"Tank\"", "\"Rigs\""] {
            let section: super::super::Section = serde_json::from_str(step).expect("reads");
            assert_eq!(section, super::super::Section::Tank);
        }
    }
}
