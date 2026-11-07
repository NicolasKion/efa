//! The tank optimiser: tries tanks for the free slots of the tanking layer and the rigs, scores
//! each with the engine against the fit's damage pattern, and keeps the best.
//!
//! Trying every combination is out of reach on a battleship, so it searches in stages: each
//! stage adds one module to the best few tanks of the stage before, and the rigs are tried on the
//! best tanks last. Tanks that do not fit the CPU, powergrid or calibration left are dropped.

use std::{
    collections::{HashMap, HashSet},
    ops::AddAssign,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::{Category, Choices, Layer, Setup, TankFit, TankStyle, Utility, apply, attr, group};
use crate::{
    data::{Catalog, Kind, Slot, TypeId},
    engine::{Engine, Pilot, Resource, TankProbe},
    fit::{Fit, ModuleState},
};

/// Tanks kept from one stage to grow in the next.
const BEAM: usize = 20;
/// Tanks the rigs are tried on.
const RIG_SEEDS: usize = 8;
/// How many of the strongest variations of each faction, deadspace or officer group the wider
/// search tiers try per module.
const STRONGEST_PER_GROUP: usize = 3;
/// Repairers or boosters of one kind a tank takes at most.
const MAX_REPAIRERS: usize = 3;
/// Damage-type specific modules of one kind a tank takes at most.
const MAX_SPECIFIC: usize = 2;

const DAMAGE_TYPES: [&str; 4] = ["EM", "Thermal", "Kinetic", "Explosive"];

/// Plates, extenders, repairers and the rest, by group.
mod groups {
    pub const ARMOR_REPAIRER: i32 = 62;
    pub const SHIELD_BOOSTER: i32 = 40;
    pub const SHIELD_EXTENDER: i32 = 38;
    pub const ARMOR_PLATE: i32 = 329;
    pub const SHIELD_BOOST_AMPLIFIER: i32 = 338;
}

/// The kind of module a tank item is, for its bling.
pub fn category_of(catalog: &Catalog, id: TypeId) -> Category {
    let Some(t) = catalog.get(id) else {
        return Category::Hardener;
    };
    match t.group_id {
        _ if t.kind == Kind::Module(Slot::Rig) => Category::Rig,
        groups::ARMOR_PLATE | groups::SHIELD_EXTENDER => Category::Buffer,
        groups::ARMOR_REPAIRER
        | groups::SHIELD_BOOSTER
        | groups::SHIELD_BOOST_AMPLIFIER
        | group::ANCILLARY_ARMOR_REPAIRER
        | group::ANCILLARY_SHIELD_BOOSTER => Category::Repairer,
        group::CAP_BOOSTER => Category::Electronics,
        _ => Category::Hardener,
    }
}

/// What a tank is scored by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    /// Effective HP of all layers.
    Ehp,
    /// The tanked layer's resists against the fit's damage pattern.
    Resists,
    /// The tanked layer's sustained repair, times its resists.
    Repair,
    /// Repair and buffer of the tanked layer together.
    Balanced,
    /// Effective HP, weighted by how well the tanked layer resists: of two tanks with about
    /// the same EHP, the one that resists more takes logistics better and carries less plate.
    BufferBalanced,
}

impl Aim {
    pub fn of(choices: &Choices) -> Self {
        match choices.tank {
            TankStyle::Active => match choices.goal {
                super::Goal::Resists => Self::Resists,
                super::Goal::Repair => Self::Repair,
                super::Goal::Balanced | super::Goal::Ehp => Self::Balanced,
            },
            _ => match choices.goal {
                super::Goal::Ehp => Self::Ehp,
                super::Goal::Resists => Self::Resists,
                _ => Self::BufferBalanced,
            },
        }
    }
}

/// What a module does for the tank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Repair,
    Buffer,
    Resist,
    /// Capacitor for the repairers.
    Support,
}

/// A module the optimiser can fit.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub base: TypeId,
    pub module: crate::fit::Module,
    pub role: Role,
    /// How many one tank takes at most.
    pub max: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RigCandidate {
    pub base: TypeId,
    pub type_id: TypeId,
}

/// One search, with everything it needs, so it can run off the UI thread.
#[derive(Debug, Clone)]
pub struct Problem {
    /// Everything but the tank, weapons idle so the capacitor goes to the tank.
    pub fit: Fit,
    pub pilot: Pilot,
    pub layer: Layer,
    pub aim: Aim,
    /// Free positions in the layer's rack.
    pub positions: Vec<usize>,
    /// Free rig positions, when the tank takes the rigs.
    pub rig_positions: Vec<usize>,
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

/// A tank the optimiser found, with its numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The T1 items of the modules, repairers first.
    pub modules: Vec<TypeId>,
    /// The variations fitted, in the same order.
    pub types: Vec<TypeId>,
    pub rigs: Vec<TypeId>,
    pub rig_types: Vec<TypeId>,
    pub score: f64,
    /// Effective HP of all layers.
    pub ehp: f64,
    /// The tanked layer's resists: EM, thermal, kinetic, explosive.
    pub resists: [f64; 4],
    /// The tanked layer's resist against the damage pattern.
    pub resist: f64,
    /// The tanked layer's sustained repair, HP/s.
    pub repair: f64,
    pub cap_stable: bool,
    pub cpu: Resource,
    pub powergrid: Resource,
    /// What the tank's modules and rigs cost, as far as their prices are known.
    pub price: f64,
    /// Whether every module and rig had a price.
    pub priced: bool,
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub outcomes: Vec<Outcome>,
    pub rejected: Rejected,
    /// With a number of repairers chosen, how many the results have when fewer fit.
    pub repairers_short: Option<usize>,
}

impl Outcome {
    pub fn tank_fit(&self) -> TankFit {
        TankFit {
            modules: self.modules.clone(),
            rigs: self.rigs.clone(),
            types: self.types.clone(),
            rig_types: self.rig_types.clone(),
        }
    }
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
    let mut all: Vec<Outcome> = Vec::new();
    let mut states: Vec<(State, Outcome)> = Vec::new();
    let mut rejected = Rejected::default();

    let start = State::default();
    seen.insert(start.clone());
    let (mut beam, dropped) = evaluate_all(engine, problem, &[start], progress);
    rejected += dropped;
    states.extend(beam.iter().cloned());
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
                    rigs: Vec::new(),
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
    all.extend(
        states
            .into_iter()
            .filter(|(s, _)| exact.is_none_or(|n| repair_count(problem, &s.modules) == n))
            .filter(|(s, _)| !problem.need_repair || !any_repair || repairs(s))
            .filter(|(s, _)| !s.modules.is_empty() || !s.rigs.is_empty())
            .map(|(_, o)| o),
    );
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
        let rack = fit.rack_mut(slot);
        if rack.len() <= at {
            rack.resize_with(at + 1, || None);
        }
        rack[at] = Some(candidate.module.clone());
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
    let (layer, repair) = match problem.layer {
        Layer::Shield => (probe.shield, probe.sustained.shield),
        Layer::Armor => (probe.armor, probe.sustained.armor),
    };
    let mult = layer.mult.unwrap_or(1.0).max(1.0);
    let resist = 1.0 - 1.0 / mult;
    let effective = repair * mult;
    let layer_ehp = layer.ehp.unwrap_or(layer.hp);
    let ehp: f64 = [probe.shield, probe.armor, probe.hull]
        .iter()
        .map(|l| l.ehp.unwrap_or(l.hp))
        .sum();
    let mut score = match problem.aim {
        Aim::Ehp => ehp,
        // The resist multiplier, nudged towards more repair, then more EHP, among tanks that
        // resist the same.
        Aim::Resists => {
            resist + 0.01 * effective / (effective + 100.0) + 0.001 * ehp / (ehp + 100_000.0)
        }
        Aim::Repair => effective + layer_ehp * 1e-6,
        Aim::Balanced => effective * layer_ehp,
        Aim::BufferBalanced => ehp * (1.0 + resist),
    };
    if problem.need_repair && !modules.iter().any(|c| c.role == Role::Repair) {
        score *= 0.5;
    }
    let (price, priced) = price_of(problem, modules, rigs);
    Outcome {
        modules: modules.iter().map(|c| c.base).collect(),
        types: modules.iter().map(|c| c.module.type_id).collect(),
        rigs: rigs.iter().map(|r| r.base).collect(),
        rig_types: rigs.iter().map(|r| r.type_id).collect(),
        score,
        ehp,
        resists: layer.resists,
        resist,
        repair,
        cap_stable: probe.cap_stable,
        cpu: probe.cpu,
        powergrid: probe.powergrid,
        price,
        priced,
    }
}

impl Setup<'_> {
    /// The search for the choices' tank on `fit`, with everything else planned first. None when
    /// the choices fit no tank.
    pub fn tank_problem(
        &self,
        engine: &Engine,
        pilot: &Pilot,
        fit: &Fit,
        choices: &Choices,
        prices: &crate::prices::Prices,
    ) -> Option<Problem> {
        if choices.tank == TankStyle::None {
            return None;
        }
        let (base, room) = self.plan_base(fit, choices);
        let mut next = fit.clone();
        let cleared = choices.cleared();
        apply(&mut next, &self.placements(fit, &base, cleared), cleared);
        // Repair is judged with the guns idle, so the capacitor goes to the tank.
        for module in next.high.iter_mut().flatten() {
            if matches!(module.state, ModuleState::Active | ModuleState::Overload) {
                module.state = ModuleState::Online;
            }
        }
        let slot = choices.layer.slot();
        let positions: Vec<usize> = (0..self.facts.total(slot))
            .filter(|&i| next.rack(slot).get(i).is_none_or(Option::is_none))
            .take(room.slots)
            .collect();
        let rig_positions: Vec<usize> = (0..self.facts.total(Slot::Rig))
            .filter(|&i| next.rigs.get(i).is_none_or(Option::is_none))
            .take(room.rigs)
            .collect();
        // Damage-specific modules for every damage type the pattern brings, the most exposed
        // first; the score against the pattern decides whether a kinetic or thermal hardener
        // beats an omni one.
        let empty = engine.probe_tank(&next, pilot);
        let resists = match choices.layer {
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
        let weakest = exposed.as_slice();
        // Repairers already fitted count: the search need not add one, though it may, and a
        // chosen number of them counts the fitted ones too.
        let fitted_repairers = next
            .rack(slot)
            .iter()
            .flatten()
            .filter(|m| category_of(self.catalog, m.type_id) == Category::Repairer)
            .count();
        let repairing = fitted_repairers > 0;
        let repairers = choices
            .repairers
            .filter(|_| choices.tank == TankStyle::Active)
            .map(|n| usize::from(n).saturating_sub(fitted_repairers));
        // Anything over the per-module budget is left out. With a budget set, so is anything
        // without a price: the market has none of it, and it could cost anything.
        let budgeted = choices.budget.each.is_some() || choices.budget.total.is_some();
        let affordable = |id: TypeId| match prices.get(id) {
            Some(price) => choices.budget.each.is_none_or(|max| price <= max as f64),
            None => !budgeted,
        };
        let mut candidates = self.tank_candidates(choices, weakest);
        candidates.retain(|c| affordable(c.module.type_id));
        let mut rigs = if rig_positions.is_empty() {
            Vec::new()
        } else {
            self.rig_candidates(choices, weakest)
        };
        rigs.retain(|r| affordable(r.type_id));
        let known = candidates
            .iter()
            .map(|c| c.module.type_id)
            .chain(rigs.iter().map(|r| r.type_id))
            .filter_map(|id| Some((id, prices.get(id)?)))
            .collect();
        Some(Problem {
            candidates,
            rigs,
            prices: known,
            max_total: choices.budget.total.map(|max| max as f64),
            fit: next,
            pilot: pilot.clone(),
            layer: choices.layer,
            aim: Aim::of(choices),
            positions,
            rig_positions,
            need_repair: choices.tank == TankStyle::Active
                && !repairing
                && repairers.is_none_or(|n| n > 0),
            repairers,
        })
    }

    /// The modules worth trying for the style and layer, repairers first.
    fn tank_candidates(&self, choices: &Choices, weakest: &[usize]) -> Vec<Candidate> {
        let style = choices.tank;
        let active = style == TankStyle::Active;
        let max_repairers = MAX_REPAIRERS.max(choices.repairers.map_or(0, usize::from));
        let all = usize::MAX;
        let mut wanted: Vec<(String, Role, usize)> = Vec::new();
        let mut want = |name: String, role: Role, max: usize| wanted.push((name, role, max));
        match choices.layer {
            Layer::Armor => {
                if active {
                    want(self.repairer_name(), Role::Repair, max_repairers);
                    want(self.ancillary_name(Layer::Armor), Role::Repair, 1);
                }
                want(self.plate_name().into(), Role::Buffer, all);
                {
                    want(
                        "Multispectrum Energized Membrane I".into(),
                        Role::Resist,
                        all,
                    );
                    want("Multispectrum Coating I".into(), Role::Resist, all);
                    for &i in weakest {
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
            }
            Layer::Shield => {
                if active {
                    want(self.booster_name().into(), Role::Repair, max_repairers);
                    want(self.ancillary_name(Layer::Shield), Role::Repair, 1);
                    want(
                        "Shield Boost Amplifier I".into(),
                        Role::Repair,
                        MAX_SPECIFIC,
                    );
                }
                want(self.extender_name().into(), Role::Buffer, all);
                {
                    want("Multispectrum Shield Hardener I".into(), Role::Resist, all);
                    for &i in weakest {
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
                }
                if active && !choices.has(Utility::CapBooster) {
                    want(
                        self.utility_name(Utility::CapBooster, None),
                        Role::Support,
                        1,
                    );
                }
            }
        }
        wanted
            .into_iter()
            .filter_map(|(name, role, max)| Some((self.base(&name)?, role, max)))
            .flat_map(|(base, role, max)| {
                let category = category_of(self.catalog, base);
                self.search_variations(base, category, choices)
                    .into_iter()
                    .map(move |type_id| (base, type_id, role, max))
            })
            .map(|(base, type_id, role, max)| {
                let mut module = crate::fit::Module::new(self.catalog, type_id);
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

    /// The variations of `base` the search tries: the exact pick if the user made one,
    /// otherwise one per tier the search tiers allow, each only once.
    fn search_variations(
        &self,
        base: TypeId,
        category: Category,
        choices: &Choices,
    ) -> Vec<TypeId> {
        use super::SearchGroup;
        let tiers = choices.search_tiers;
        let mut found: Vec<TypeId> = Vec::new();
        let mut add = |id: TypeId| {
            if !found.contains(&id) {
                found.push(id);
            }
        };
        // A variation the user picked by hand is always in.
        if let Some(exact) = choices.bling.exact(category, base) {
            add(exact);
        }
        let variations: Vec<&crate::data::ItemType> = self
            .catalog
            .variations_of(base)
            .iter()
            .filter_map(|&v| self.catalog.get(v))
            .filter(|t| t.published && t.market_group.is_some())
            .collect();
        // The strongest few of a group: its weakest member is rarely the one worth fitting.
        let strongest = |keep: &dyn Fn(&crate::data::ItemType) -> bool| {
            let mut members: Vec<&crate::data::ItemType> =
                variations.iter().copied().filter(|t| keep(t)).collect();
            members.sort_by(|a, b| b.meta_level.cmp(&a.meta_level).then(a.id.cmp(&b.id)));
            members
                .into_iter()
                .take(STRONGEST_PER_GROUP)
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        if category == Category::Rig {
            add(self.resolve(base, choices.bling.tier_for(Category::Rig)));
            add(base);
            if tiers.has(SearchGroup::T2Rigs) {
                add(self.resolve(base, super::Tier::T2));
            }
            return found;
        }
        for group in SearchGroup::ALL {
            if !tiers.has(group) {
                continue;
            }
            match group {
                SearchGroup::T1 => add(base),
                SearchGroup::Meta => {
                    for id in
                        strongest(&|t| t.meta_group == Some(super::meta::TECH1) && t.id != base)
                    {
                        add(id);
                    }
                }
                SearchGroup::T2Rigs => {}
                _ => {
                    let wanted = group.meta_group();
                    for id in strongest(&|t| t.meta_group.is_some() && t.meta_group == wanted) {
                        add(id);
                    }
                }
            }
        }
        // A module none of the switched-on groups has still comes at the bling tier, so
        // switching on only Officer does not leave the plates out.
        let fallback = self.resolve(base, choices.bling.tier_for(category));
        add(fallback);
        if found.len() > 1 {
            // It was only wanted when nothing else came up.
            found.retain(|&id| id != fallback || tiers_cover(choices, self, fallback));
        }
        found
    }

    /// The rigs worth trying for the style and layer.
    fn rig_candidates(&self, choices: &Choices, weakest: &[usize]) -> Vec<RigCandidate> {
        let style = choices.tank;
        let mut names: Vec<String> = Vec::new();
        match choices.layer {
            Layer::Armor => {
                names.push("Trimark Armor Pump I".into());
                if style == TankStyle::Active {
                    names.push("Auxiliary Nano Pump I".into());
                    names.push("Nanobot Accelerator I".into());
                }
                {
                    for &i in weakest {
                        names.push(format!("{} Armor Reinforcer I", DAMAGE_TYPES[i]));
                    }
                }
            }
            Layer::Shield => {
                names.push("Core Defense Field Extender I".into());
                if style == TankStyle::Active {
                    names.push("Core Defense Operational Solidifier I".into());
                }
                {
                    for &i in weakest {
                        names.push(format!("{} Shield Reinforcer I", DAMAGE_TYPES[i]));
                    }
                }
            }
        }
        let size = self.facts.size.label();
        names
            .into_iter()
            .filter_map(|name| self.base(&format!("{size} {name}")))
            .flat_map(|base| {
                self.search_variations(base, Category::Rig, choices)
                    .into_iter()
                    .map(move |type_id| (base, type_id))
            })
            .filter(|&(_, type_id)| {
                self.attr(type_id, attr::UPGRADE_COST) <= self.facts.calibration
            })
            .map(|(base, type_id)| RigCandidate { base, type_id })
            .collect()
    }
}

/// Whether a switched-on search group covers `id`.
fn tiers_cover(choices: &Choices, setup: &Setup<'_>, id: TypeId) -> bool {
    use super::SearchGroup;
    let Some(t) = setup.catalog.get(id) else {
        return false;
    };
    let tiers = choices.search_tiers;
    SearchGroup::ALL
        .into_iter()
        .filter(|&g| tiers.has(g))
        .any(|g| match g {
            SearchGroup::T1 => t.variation_parent.is_none(),
            SearchGroup::Meta => {
                t.meta_group == Some(super::meta::TECH1) && t.variation_parent.is_some()
            }
            SearchGroup::T2Rigs => false,
            _ => t.meta_group.is_some() && t.meta_group == g.meta_group(),
        })
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
}
