//! The tank section: searches tanks for one layer of the current fit and its rigs, scores each
//! with the engine against the fit's damage pattern for one goal, and keeps the best. The rest
//! of the fit stays as it is.
//!
//! Trying every combination is out of reach on a battleship, so it searches in stages: each
//! stage adds one module to the best few tanks of the stage before, and the rigs are tried on the
//! best tanks last. Tanks that do not fit the CPU, powergrid or calibration are dropped.

use std::{
    collections::{HashMap, HashSet},
    ops::AddAssign,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use serde::{Deserialize, Serialize};

use super::{Budget, Change, HullSize, Layer, SearchTiers, Setup, attr, group};
use crate::{
    data::{Catalog, Slot, TypeId},
    engine::{Engine, Layer as EngineLayer, Pilot, Resource, TankProbe},
    fit::{Fit, Module, ModuleState},
    prices::Prices,
};

/// Tanks kept from one stage to grow in the next.
const BEAM: usize = 20;
/// Tanks the rigs are tried on.
const RIG_SEEDS: usize = 8;
/// Repairers or boosters of one kind a tank takes at most, unless more are asked for.
const MAX_REPAIRERS: usize = 3;
/// Damage-type specific modules of one kind a tank takes at most.
const MAX_SPECIFIC: usize = 2;

const DAMAGE_TYPES: [&str; 4] = ["EM", "Thermal", "Kinetic", "Explosive"];

/// Repairers and boosters, by group.
mod groups {
    pub const ARMOR_REPAIRER: i32 = 62;
    pub const SHIELD_BOOSTER: i32 = 40;
}

/// Whether a module repairs its layer: a repairer or booster, ancillary or not.
fn is_repairer(catalog: &Catalog, id: TypeId) -> bool {
    catalog.get(id).is_some_and(|t| {
        matches!(
            t.group_id,
            groups::ARMOR_REPAIRER
                | groups::SHIELD_BOOSTER
                | group::ANCILLARY_ARMOR_REPAIRER
                | group::ANCILLARY_SHIELD_BOOSTER
        )
    })
}

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
        let mut next = fit.clone();
        super::apply(&mut next, change);
        idle_guns(&mut next);
        Self::of(&engine.probe_tank(&next, pilot), layer)
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

/// Turn active guns off, so the capacitor goes to the tank.
fn idle_guns(fit: &mut Fit) {
    for module in fit.high.iter_mut().flatten() {
        if matches!(module.state, ModuleState::Active | ModuleState::Overload) {
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
        let over = |r: &Resource| usize::from(r.output.is_some_and(|out| r.used > out + 1e-6));
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

/// A tank being tried: candidate indices, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct State {
    modules: Vec<usize>,
    rigs: Vec<usize>,
}

/// The best tanks for the problem, best first, at most `keep` of them. Empty when nothing fits
/// or the search was called off.
pub fn optimise(engine: &Engine, problem: &Problem, progress: &Progress, keep: usize) -> Found {
    let n = problem.candidates.len();
    let depth = problem.positions.len();
    let rig_depth = problem.rig_positions.len();
    progress.total.store(
        (depth * BEAM * n + rig_depth * BEAM * problem.rigs.len() + 1).max(1),
        Ordering::Relaxed,
    );
    let mut seen: HashSet<State> = HashSet::new();
    let mut states: Vec<(State, Outcome)> = Vec::new();
    let mut rejected = Rejected::default();

    // Besides the empty start, starts with a powergrid or CPU rig on, so the modules can use
    // the room it makes; the rig stage later fills any rig slots still free.
    let mut starts = vec![State::default()];
    if !problem.rig_positions.is_empty() {
        for (r, rig) in problem.rigs.iter().enumerate() {
            if rig.fitting {
                starts.push(State {
                    modules: Vec::new(),
                    rigs: vec![r],
                });
            }
        }
    }
    seen.extend(starts.iter().cloned());
    let (beam, dropped) = evaluate_all(engine, problem, &starts, progress);
    rejected += dropped;
    states.extend(beam.iter().cloned());
    let mut beam = beam;
    for _ in 0..depth {
        let mut next = Vec::new();
        for (state, _) in &beam {
            for (c, candidate) in problem.candidates.iter().enumerate() {
                // Variations of one module share its limit, so a T2 and a faction damage
                // control do not both go on.
                let count = state
                    .modules
                    .iter()
                    .filter(|&&m| problem.candidates[m].base == candidate.base)
                    .count();
                // A chosen number of repairers is a ceiling while growing, and exact at the end.
                if candidate.role == Role::Repair
                    && problem
                        .repairers
                        .is_some_and(|n| repair_count(problem, &state.modules) >= n)
                {
                    continue;
                }
                if count >= candidate.max {
                    continue;
                }
                let mut modules = state.modules.clone();
                modules.push(c);
                modules.sort_unstable();
                let grown = State {
                    modules,
                    rigs: state.rigs.clone(),
                };
                if seen.insert(grown.clone()) {
                    next.push(grown);
                }
            }
        }
        if next.is_empty() || progress.cancelled() {
            break;
        }
        let (mut scored, dropped) = evaluate_all(engine, problem, &next, progress);
        rejected += dropped;
        if scored.is_empty() {
            break;
        }
        sort(&mut scored);
        beam = scored.iter().take(BEAM).cloned().collect();
        states.extend(scored);
    }

    // The rigs, on the best tanks.
    if rig_depth > 0 && !problem.rigs.is_empty() && !progress.cancelled() {
        sort(&mut states);
        let mut beam: Vec<(State, Outcome)> = states.iter().take(RIG_SEEDS).cloned().collect();
        for _ in 0..rig_depth {
            let mut next = Vec::new();
            for (state, _) in &beam {
                for r in 0..problem.rigs.len() {
                    let mut rigs = state.rigs.clone();
                    rigs.push(r);
                    rigs.sort_unstable();
                    let grown = State {
                        modules: state.modules.clone(),
                        rigs,
                    };
                    if seen.insert(grown.clone()) {
                        next.push(grown);
                    }
                }
            }
            if next.is_empty() || progress.cancelled() {
                break;
            }
            let (mut scored, dropped) = evaluate_all(engine, problem, &next, progress);
            rejected += dropped;
            if scored.is_empty() {
                break;
            }
            sort(&mut scored);
            beam = scored.iter().take(BEAM).cloned().collect();
            states.extend(scored);
        }
    }
    if progress.cancelled() {
        return Found::default();
    }
    progress
        .done
        .store(progress.total.load(Ordering::Relaxed), Ordering::Relaxed);

    let repairs = |s: &State| {
        s.modules
            .iter()
            .any(|&m| problem.candidates[m].role == Role::Repair)
    };
    let any_repair = states.iter().any(|(s, _)| repairs(s));
    // With a number of repairers chosen, only tanks with that many, or the most there are when
    // none get there.
    let exact = problem.repairers.map(|n| {
        let most = states
            .iter()
            .map(|(s, _)| repair_count(problem, &s.modules))
            .max()
            .unwrap_or(0);
        n.min(most)
    });
    let mut all: Vec<Outcome> = states
        .into_iter()
        .filter(|(s, _)| exact.is_none_or(|n| repair_count(problem, &s.modules) == n))
        .filter(|(s, _)| !problem.need_repair || !any_repair || repairs(s))
        .filter(|(s, _)| !s.modules.is_empty() || !s.rigs.is_empty())
        .map(|(_, o)| o)
        .collect();
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    all.truncate(keep);
    Found {
        outcomes: all,
        rejected,
        repairers_short: problem
            .repairers
            .zip(exact)
            .and_then(|(wanted, got)| (got < wanted).then_some(got)),
    }
}

/// How many of `modules` are repairers or boosters.
fn repair_count(problem: &Problem, modules: &[usize]) -> usize {
    modules
        .iter()
        .filter(|&&m| problem.candidates[m].role == Role::Repair)
        .count()
}

fn sort(scored: &mut [(State, Outcome)]) {
    scored.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
}

/// Score the states, spread over a few threads; those that do not fit are left out and counted.
fn evaluate_all(
    engine: &Engine,
    problem: &Problem,
    states: &[State],
    progress: &Progress,
) -> (Vec<(State, Outcome)>, Rejected) {
    let threads = std::thread::available_parallelism()
        .map_or(2, std::num::NonZero::get)
        .clamp(1, 6);
    let chunk = states.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = states
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut scored = Vec::new();
                    let mut rejected = Rejected::default();
                    for state in chunk {
                        if progress.cancelled() {
                            break;
                        }
                        match evaluate(engine, problem, state) {
                            Ok(outcome) => scored.push((state.clone(), outcome)),
                            Err(dropped) => rejected += dropped,
                        }
                        progress.done.fetch_add(1, Ordering::Relaxed);
                    }
                    (scored, rejected)
                })
            })
            .collect();
        let mut scored = Vec::new();
        let mut rejected = Rejected::default();
        for (chunk, dropped) in handles.into_iter().filter_map(|h| h.join().ok()) {
            scored.extend(chunk);
            rejected += dropped;
        }
        (scored, rejected)
    })
}

/// Fit the state's modules and rigs and score the result; what it ran out of when it does not
/// fit.
fn evaluate(engine: &Engine, problem: &Problem, state: &State) -> Result<Outcome, Rejected> {
    let modules: Vec<&Candidate> = state
        .modules
        .iter()
        .map(|&m| &problem.candidates[m])
        .collect();
    let rigs: Vec<RigCandidate> = state.rigs.iter().map(|&r| problem.rigs[r]).collect();
    try_score(engine, problem, &modules, &rigs)
}

/// Score a tank of the problem's kind made of these modules and rigs; None when it does not fit.
/// Modules past the free positions are left off.
pub fn score(
    engine: &Engine,
    problem: &Problem,
    modules: &[&Candidate],
    rigs: &[RigCandidate],
) -> Option<Outcome> {
    try_score(engine, problem, modules, rigs).ok()
}

/// [`score`], with what the tank ran out of when it does not fit.
fn try_score(
    engine: &Engine,
    problem: &Problem,
    modules: &[&Candidate],
    rigs: &[RigCandidate],
) -> Result<Outcome, Rejected> {
    // The budget first: it needs no engine.
    let (price, _) = price_of(problem, modules, rigs);
    if problem.max_total.is_some_and(|max| price > max) {
        return Err(Rejected {
            budget: 1,
            ..Rejected::default()
        });
    }
    let mut fit = problem.fit.clone();
    let slot = problem.layer.slot();
    for (candidate, &at) in modules.iter().zip(&problem.positions) {
        super::place(&mut fit, &(slot, at, candidate.module.clone()));
    }
    for (rig, &at) in rigs.iter().zip(&problem.rig_positions) {
        if fit.rigs.len() <= at {
            fit.rigs.resize(at + 1, None);
        }
        fit.rigs[at] = Some(rig.type_id);
    }
    let probe = engine.probe_tank(&fit, &problem.pilot);
    let rejected = Rejected::of(&probe);
    if rejected.is_empty() {
        Ok(outcome(problem, modules, rigs, &probe))
    } else {
        Err(rejected)
    }
}

/// What these modules and rigs cost, as far as prices are known, and whether all were.
fn price_of(problem: &Problem, modules: &[&Candidate], rigs: &[RigCandidate]) -> (f64, bool) {
    let ids = modules
        .iter()
        .take(problem.positions.len())
        .map(|c| c.module.type_id)
        .chain(
            rigs.iter()
                .take(problem.rig_positions.len())
                .map(|r| r.type_id),
        );
    let mut priced = true;
    let total = ids
        .map(|id| {
            problem.prices.get(&id).copied().unwrap_or_else(|| {
                priced = false;
                0.0
            })
        })
        .sum();
    (total, priced)
}

fn outcome(
    problem: &Problem,
    modules: &[&Candidate],
    rigs: &[RigCandidate],
    probe: &TankProbe,
) -> Outcome {
    let numbers = Numbers::of(probe, problem.layer);
    let Numbers {
        ehp,
        multiplier,
        repair_ehp,
        ..
    } = numbers;
    let layer = numbers.layer(problem.layer);
    let layer_ehp = layer.ehp.unwrap_or(layer.hp);
    let mut score = match problem.goal {
        Goal::Ehp => ehp,
        // The multiplier, nudged towards more repair, then more EHP, among tanks that resist
        // the same.
        Goal::Resists => {
            multiplier + 0.01 * repair_ehp / (repair_ehp + 100.0) + 0.001 * ehp / (ehp + 100_000.0)
        }
        Goal::Repair => repair_ehp + layer_ehp * 1e-6,
    };
    if problem.need_repair && !modules.iter().any(|c| c.role == Role::Repair) {
        score *= 0.5;
    }
    let (price, priced) = price_of(problem, modules, rigs);
    let modules = &modules[..modules.len().min(problem.positions.len())];
    let rigs = &rigs[..rigs.len().min(problem.rig_positions.len())];
    Outcome {
        types: modules.iter().map(|c| c.module.type_id).collect(),
        charges: modules.iter().map(|c| c.module.charge).collect(),
        rig_types: rigs.iter().map(|r| r.type_id).collect(),
        score,
        numbers,
        price,
        priced,
    }
}

impl Setup<'_> {
    /// The room the choices' tank has on `fit`: the fit with the tank being redone taken off,
    /// and the positions it leaves free. No engine needed, so it is cheap to work out often.
    pub fn tank_room(&self, fit: &Fit, choices: &Choices) -> Room {
        let slot = choices.layer.slot();
        // What counts as the fitted tank: anything of a group the search fits, repairers
        // included whatever the goal, but not a cap booster, which may be there for anything.
        let module_groups: HashSet<i32> = self
            .tank_candidates(choices, true, &[0, 1, 2, 3])
            .iter()
            .filter_map(|c| self.catalog.get(c.module.type_id))
            .map(|t| t.group_id)
            .filter(|&g| g != group::CAP_BOOSTER)
            .collect();
        let rig_groups: HashSet<i32> = self
            .rig_candidates(choices, true, &[0, 1, 2, 3])
            .iter()
            .filter(|r| !r.fitting)
            .filter_map(|r| self.catalog.get(r.type_id))
            .map(|t| t.group_id)
            .collect();
        let tank = |id: TypeId, groups: &HashSet<i32>| {
            self.catalog
                .get(id)
                .is_some_and(|t| groups.contains(&t.group_id))
        };
        let mut next = fit.clone();
        let mut removed = Vec::new();
        if choices.refit {
            for (i, at) in next.rack_mut(slot).iter_mut().enumerate() {
                if at.as_ref().is_some_and(|m| tank(m.type_id, &module_groups)) {
                    *at = None;
                    removed.push((slot, i));
                }
            }
            for (i, at) in next.rigs.iter_mut().enumerate() {
                if at.is_some_and(|r| tank(r, &rig_groups)) {
                    *at = None;
                    removed.push((Slot::Rig, i));
                }
            }
        }
        let positions = (0..self.facts.total(slot))
            .filter(|&i| next.rack(slot).get(i).is_none_or(Option::is_none))
            .collect();
        let rig_positions = (0..self.facts.total(Slot::Rig))
            .filter(|&i| next.rigs.get(i).is_none_or(Option::is_none))
            .collect();
        Room {
            fit: next,
            removed,
            positions,
            rig_positions,
        }
    }

    /// The search for the choices' tank on `fit`. Its positions are empty when the layer and
    /// the rigs have no room.
    pub fn tank_problem(
        &self,
        engine: &Engine,
        pilot: &Pilot,
        fit: &Fit,
        choices: &Choices,
        prices: &Prices,
    ) -> Problem {
        let layer = choices.layer;
        let slot = layer.slot();
        let repair = choices.goal == Goal::Repair;
        let Room {
            fit: mut next,
            removed,
            positions,
            rig_positions,
        } = self.tank_room(fit, choices);
        idle_guns(&mut next);
        // Damage-specific modules for every damage type the pattern brings, the most exposed
        // first; the score against the pattern decides whether a kinetic or thermal hardener
        // beats an omni one.
        let empty = engine.probe_tank(&next, pilot);
        let resists = match layer {
            Layer::Shield => empty.shield.resists,
            Layer::Armor => empty.armor.resists,
        };
        let mut order = [0, 1, 2, 3];
        order.sort_by(|&a, &b| {
            let hole = |i: usize| empty.incoming[i] * (1.0 - resists[i]);
            hole(b).total_cmp(&hole(a))
        });
        let exposed: Vec<usize> = order
            .into_iter()
            .filter(|&i| empty.incoming[i] > 0.0)
            .collect();
        // Repairers left on count: the search need not add one, though it may, and a chosen
        // number of them counts these too.
        let fitted_repairers = next
            .rack(slot)
            .iter()
            .flatten()
            .filter(|m| is_repairer(self.catalog, m.type_id))
            .count();
        let repairers = choices
            .repairers()
            .map(|n| n.saturating_sub(fitted_repairers));
        let affordable = |id: TypeId| choices.budget.allows(prices.get(id));
        let mut candidates = self.tank_candidates(choices, repair, &exposed);
        candidates.retain(|c| affordable(c.module.type_id));
        let mut rigs = if rig_positions.is_empty() {
            Vec::new()
        } else {
            self.rig_candidates(choices, repair, &exposed)
        };
        rigs.retain(|r| affordable(r.type_id));
        let known = candidates
            .iter()
            .map(|c| c.module.type_id)
            .chain(rigs.iter().map(|r| r.type_id))
            .filter_map(|id| Some((id, prices.get(id)?)))
            .collect();
        Problem {
            candidates,
            rigs,
            prices: known,
            max_total: choices.budget.total.map(|max| max as f64),
            fit: next,
            pilot: pilot.clone(),
            layer,
            goal: choices.goal,
            positions,
            rig_positions,
            removed,
            need_repair: repair && fitted_repairers == 0 && repairers.is_none_or(|n| n > 0),
            repairers,
        }
    }

    /// The modules worth trying for the layer, repairers first when `repair`; `exposed` are
    /// the damage types to try specific resist modules for.
    fn tank_candidates(
        &self,
        choices: &Choices,
        repair: bool,
        exposed: &[usize],
    ) -> Vec<Candidate> {
        let max_repairers = MAX_REPAIRERS.max(choices.repairers().unwrap_or(0));
        let all = usize::MAX;
        let mut wanted: Vec<(String, Role, usize)> = Vec::new();
        let mut want = |name: String, role: Role, max: usize| wanted.push((name, role, max));
        match choices.layer {
            Layer::Armor => {
                if repair {
                    want(self.repairer_name(), Role::Repair, max_repairers);
                    want(self.ancillary_name(Layer::Armor), Role::Repair, 1);
                }
                want(self.plate_name().into(), Role::Buffer, all);
                want(
                    "Multispectrum Energized Membrane I".into(),
                    Role::Resist,
                    all,
                );
                want("Multispectrum Coating I".into(), Role::Resist, all);
                for &i in exposed {
                    let kind = DAMAGE_TYPES[i];
                    want(
                        format!("{kind} Energized Membrane I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                    want(format!("{kind} Coating I"), Role::Resist, MAX_SPECIFIC);
                    want(
                        format!("{kind} Armor Hardener I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                }
                want("Reactive Armor Hardener".into(), Role::Resist, 1);
                want("Damage Control I".into(), Role::Resist, 1);
            }
            Layer::Shield => {
                if repair {
                    want(self.booster_name().into(), Role::Repair, max_repairers);
                    want(self.ancillary_name(Layer::Shield), Role::Repair, 1);
                    want(
                        "Shield Boost Amplifier I".into(),
                        Role::Support,
                        MAX_SPECIFIC,
                    );
                }
                want(self.extender_name().into(), Role::Buffer, all);
                want("Multispectrum Shield Hardener I".into(), Role::Resist, all);
                for &i in exposed {
                    let kind = DAMAGE_TYPES[i];
                    want(
                        format!("{kind} Shield Hardener I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                    want(
                        format!("{kind} Shield Amplifier I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                }
                if repair {
                    want(self.cap_booster_name(), Role::Support, 1);
                }
            }
        }
        wanted
            .into_iter()
            .filter_map(|(name, role, max)| Some((self.base(&name)?, role, max)))
            .flat_map(|(base, role, max)| {
                self.search_variations(base, false, choices.search_tiers)
                    .into_iter()
                    .map(move |type_id| (base, type_id, role, max))
            })
            .map(|(base, type_id, role, max)| {
                let mut module = Module::new(self.catalog, type_id);
                module.charge = self.charge_for(type_id);
                Candidate {
                    base,
                    module,
                    role,
                    max,
                }
            })
            .collect()
    }

    /// The rigs worth trying for the layer, repair rigs when `repair`.
    fn rig_candidates(
        &self,
        choices: &Choices,
        repair: bool,
        exposed: &[usize],
    ) -> Vec<RigCandidate> {
        let mut names: Vec<String> = Vec::new();
        match choices.layer {
            Layer::Armor => {
                names.push("Trimark Armor Pump I".into());
                if repair {
                    names.push("Auxiliary Nano Pump I".into());
                    names.push("Nanobot Accelerator I".into());
                }
                for &i in exposed {
                    names.push(format!("{} Armor Reinforcer I", DAMAGE_TYPES[i]));
                }
            }
            Layer::Shield => {
                names.push("Core Defense Field Extender I".into());
                if repair {
                    names.push("Core Defense Operational Solidifier I".into());
                }
                for &i in exposed {
                    names.push(format!("{} Shield Reinforcer I", DAMAGE_TYPES[i]));
                }
            }
        }
        // Rigs that make room rather than tank: for hulls short of powergrid or CPU.
        let fitting = [
            "Ancillary Current Router I",
            "Processor Overclocking Unit I",
        ];
        names.extend(fitting.iter().map(|&n| n.to_owned()));
        let size = self.facts.size.label();
        names
            .into_iter()
            .filter_map(|name| {
                let base = self.base(&format!("{size} {name}"))?;
                Some((base, fitting.contains(&name.as_str())))
            })
            .flat_map(|(base, fitting)| {
                self.search_variations(base, true, choices.search_tiers)
                    .into_iter()
                    .map(move |type_id| (base, type_id, fitting))
            })
            .filter(|&(_, type_id, _)| {
                self.attr(type_id, attr::UPGRADE_COST) <= self.facts.calibration
            })
            .map(|(base, type_id, fitting)| RigCandidate {
                base,
                type_id,
                fitting,
            })
            .collect()
    }

    /// The plate a hull this size takes.
    fn plate_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "400mm Steel Plates I",
            HullSize::Medium if !self.facts.large() => "800mm Steel Plates I",
            HullSize::Medium | HullSize::Large => "1600mm Steel Plates I",
            HullSize::Capital => "25000mm Steel Plates I",
        }
    }

    fn extender_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "Medium Shield Extender I",
            HullSize::Medium | HullSize::Large => "Large Shield Extender I",
            HullSize::Capital => "Capital Shield Extender I",
        }
    }

    fn repairer_name(&self) -> String {
        format!("{} Armor Repairer I", self.facts.size.label())
    }

    fn booster_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "Small Shield Booster I",
            HullSize::Medium if !self.facts.large() => "Medium Shield Booster I",
            HullSize::Medium => "Large Shield Booster I",
            HullSize::Large => "X-Large Shield Booster I",
            HullSize::Capital => "Capital Shield Booster I",
        }
    }

    fn cap_booster_name(&self) -> String {
        let size = match self.facts.size {
            HullSize::Small => "Small",
            HullSize::Medium => "Medium",
            HullSize::Large => "Heavy",
            HullSize::Capital => "Capital",
        };
        format!("{size} Capacitor Booster I")
    }

    /// The ancillary repairer or booster for the layer.
    fn ancillary_name(&self, layer: Layer) -> String {
        match layer {
            Layer::Armor => format!("{} Ancillary Armor Repairer", self.facts.size.label()),
            Layer::Shield => {
                let size = match self.facts.size {
                    HullSize::Small => "Small",
                    HullSize::Medium if !self.facts.large() => "Medium",
                    HullSize::Medium => "Large",
                    HullSize::Large => "X-Large",
                    HullSize::Capital => "Capital",
                };
                format!("{size} Ancillary Shield Booster")
            }
        }
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
