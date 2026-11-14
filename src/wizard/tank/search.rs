//! The search itself: tanks grown a module or rig at a time from the best few of the stage
//! before, each scored on a bench the fit is built on once.

use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};

use super::{
    Candidate, Found, Goal, Numbers, Outcome, Problem, Progress, Rejected, RigCandidate, Role,
};
use crate::{
    data::{Slot, TypeId},
    engine::{Engine, TankBench, TankProbe},
    fit::Module,
};

/// Tanks kept from one stage to grow in the next.
const BEAM: usize = 20;

/// Threads a search spreads over at most, leaving the rest of the machine some room.
const MAX_THREADS: usize = 6;

/// A tank being tried: candidate indices, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct State {
    modules: Vec<usize>,
    rigs: Vec<usize>,
}

/// The best tanks for the problem, best first, at most `keep` of them. Empty when nothing fits
/// or the search was called off.
pub fn optimise(engine: &Engine, problem: &Problem, progress: &Progress, keep: usize) -> Found {
    let Some(bench) = engine.tank_bench(&problem.fit, &problem.pilot) else {
        return Found::default();
    };
    let bench = &bench;
    let n = problem.candidates.len();
    let depth = problem.positions.len();
    let rig_depth = problem.rig_positions.len();
    progress.total.store(
        ((depth + rig_depth) * BEAM * (n + problem.rigs.len()) + 1).max(1),
        Ordering::Relaxed,
    );
    let mut seen: HashSet<State> = HashSet::new();
    let mut states: Vec<(State, Outcome)> = Vec::new();
    let mut rejected = Rejected::default();

    // Besides the empty start, starts with a powergrid or CPU rig on, so the modules can use
    // the room it makes.
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
    let (beam, dropped) = evaluate_all(bench, problem, &starts, progress);
    rejected += dropped;
    states.extend(beam.iter().cloned());
    // What filling the slots still free costs, so a tank on a budget keeps that much back to
    // grow on rather than spending it all on its first modules: the cheapest module for each
    // module slot, and for each rig slot enough for the cheapest variation of any tank rig.
    let price = |id: TypeId| problem.prices.get(&id).copied();
    let cheapest_module = problem
        .candidates
        .iter()
        .filter_map(|c| price(c.module.type_id))
        .fold(f64::INFINITY, f64::min);
    let mut cheapest_by_rig: HashMap<TypeId, f64> = HashMap::new();
    for rig in problem.rigs.iter().filter(|r| !r.fitting) {
        if let Some(p) = price(rig.type_id) {
            let at = cheapest_by_rig.entry(rig.base).or_insert(p);
            *at = at.min(p);
        }
    }
    let cheapest_rig = cheapest_by_rig.into_values().fold(0.0, f64::max);
    let reserve = |state: &State| {
        let missing = |left: usize, price: f64| if left == 0 { 0.0 } else { left as f64 * price };
        missing(depth.saturating_sub(state.modules.len()), cheapest_module)
            + missing(rig_depth.saturating_sub(state.rigs.len()), cheapest_rig)
    };
    let reserve = &reserve;
    let mut beam = beam;
    // Each stage adds a module or a rig, so cheap rigs compete with costly modules for the
    // budget rather than coming last onto tanks that spent it all.
    for _ in 0..depth + rig_depth {
        let mut next = Vec::new();
        for (state, _) in &beam {
            let mut grow = |grown: State| {
                if seen.insert(grown.clone()) {
                    next.push(grown);
                }
            };
            if state.modules.len() < depth {
                for (c, candidate) in problem.candidates.iter().enumerate() {
                    // Variations of one module share its limit, so a T2 and a faction damage
                    // control do not both go on.
                    let count = state
                        .modules
                        .iter()
                        .filter(|&&m| problem.candidates[m].base == candidate.base)
                        .count();
                    // A chosen number of repairers is a ceiling while growing, and exact at
                    // the end.
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
                    grow(State {
                        modules,
                        rigs: state.rigs.clone(),
                    });
                }
            }
            if state.rigs.len() < rig_depth {
                for r in 0..problem.rigs.len() {
                    let mut rigs = state.rigs.clone();
                    rigs.push(r);
                    rigs.sort_unstable();
                    grow(State {
                        modules: state.modules.clone(),
                        rigs,
                    });
                }
            }
        }
        if next.is_empty() || progress.cancelled() {
            break;
        }
        let (mut scored, dropped) = evaluate_all(bench, problem, &next, progress);
        rejected += dropped;
        if scored.is_empty() {
            break;
        }
        sort(&mut scored);
        beam = pick_beam(problem, &scored, reserve);
        states.extend(scored);
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
    // Different paths reach the same tank (a variation picked through another candidate, the
    // same rigs in another order): show each tank once.
    let mut shown = HashSet::new();
    all.retain(|o| {
        let mut types = o.types.clone();
        types.sort_unstable();
        let mut rigs = o.rig_types.clone();
        rigs.sort_unstable();
        shown.insert((types, rigs))
    });
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

/// The tanks to grow next: the best that leave room in the budget to fill their free slots,
/// topped up with the best of the rest.
fn pick_beam(
    problem: &Problem,
    scored: &[(State, Outcome)],
    reserve: &dyn Fn(&State) -> f64,
) -> Vec<(State, Outcome)> {
    let Some(max) = problem.max_total else {
        return scored.iter().take(BEAM).cloned().collect();
    };
    let (room, rest): (Vec<_>, Vec<_>) = scored
        .iter()
        .partition(|(s, o)| o.price + reserve(s) <= max);
    room.into_iter().chain(rest).take(BEAM).cloned().collect()
}

fn sort(scored: &mut [(State, Outcome)]) {
    scored.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
}

/// Score the states, spread over a few threads; those that do not fit are left out and counted.
fn evaluate_all(
    bench: &TankBench,
    problem: &Problem,
    states: &[State],
    progress: &Progress,
) -> (Vec<(State, Outcome)>, Rejected) {
    let threads = std::thread::available_parallelism()
        .map_or(2, std::num::NonZero::get)
        .clamp(1, MAX_THREADS);
    let chunk = states.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = states
            .chunks(chunk)
            .map(|chunk| {
                let mut bench = bench.clone();
                scope.spawn(move || {
                    let mut scored = Vec::new();
                    let mut rejected = Rejected::default();
                    for state in chunk {
                        if progress.cancelled() {
                            break;
                        }
                        match evaluate(&mut bench, problem, state) {
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
fn evaluate(bench: &mut TankBench, problem: &Problem, state: &State) -> Result<Outcome, Rejected> {
    let modules: Vec<&Candidate> = state
        .modules
        .iter()
        .map(|&m| &problem.candidates[m])
        .collect();
    let rigs: Vec<RigCandidate> = state.rigs.iter().map(|&r| problem.rigs[r]).collect();
    try_score(bench, problem, &modules, &rigs)
}

/// Score a tank of the problem's kind made of these modules and rigs; None when it does not fit.
/// Modules past the free positions are left off.
pub fn score(
    engine: &Engine,
    problem: &Problem,
    modules: &[&Candidate],
    rigs: &[RigCandidate],
) -> Option<Outcome> {
    let mut bench = engine.tank_bench(&problem.fit, &problem.pilot)?;
    try_score(&mut bench, problem, modules, rigs).ok()
}

/// [`score`], with what the tank ran out of when it does not fit.
fn try_score(
    bench: &mut TankBench,
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
    let slot = problem.layer.slot();
    let placed: Vec<(Slot, usize, &Module)> = modules
        .iter()
        .zip(&problem.positions)
        .map(|(candidate, &at)| (slot, at, &candidate.module))
        .collect();
    let rig_types: Vec<TypeId> = rigs
        .iter()
        .zip(&problem.rig_positions)
        .map(|(rig, _)| rig.type_id)
        .collect();
    let probe = bench.probe(&placed, &rig_types);
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
    let mut score = match problem.goal {
        Goal::Ehp => ehp,
        // The multiplier, nudged towards more repair, then more EHP, among tanks that resist
        // the same.
        Goal::Resists => {
            multiplier + 0.01 * repair_ehp / (repair_ehp + 100.0) + 0.001 * ehp / (ehp + 100_000.0)
        }
        // Repair alone: among tanks that repair the same, the one with the fewest modules and
        // rigs, so a slot nothing improves the repair from stays free rather than taking
        // buffer the goal did not ask for.
        Goal::Repair => repair_ehp - 1e-6 * (modules.len() + rigs.len()) as f64,
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
