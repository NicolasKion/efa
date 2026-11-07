//! The tank optimiser: tries tanks for the free slots of the tanking layer and the rigs, scores
//! each with the engine against the fit's damage pattern, and keeps the best.
//!
//! Trying every combination is out of reach on a battleship, so it searches in stages: each
//! stage adds one module to the best few tanks of the stage before, and the rigs are tried on the
//! best tanks last. Tanks that do not fit the CPU, powergrid or calibration left are dropped.

use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::{Category, Choices, Layer, Setup, TankFit, TankStyle, Utility, apply, attr, group};
use crate::{
    data::{Catalog, Kind, Slot, TypeId},
    engine::{Engine, Pilot, TankProbe},
    fit::{Fit, ModuleState},
};

/// Tanks kept from one stage to grow in the next.
const BEAM: usize = 20;
/// Tanks the rigs are tried on.
const RIG_SEEDS: usize = 8;
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
}

impl Outcome {
    pub fn tank_fit(&self) -> TankFit {
        TankFit {
            modules: self.modules.clone(),
            rigs: self.rigs.clone(),
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
pub fn optimise(
    engine: &Engine,
    problem: &Problem,
    progress: &Progress,
    keep: usize,
) -> Vec<Outcome> {
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

    let start = State::default();
    seen.insert(start.clone());
    let mut beam = evaluate_all(engine, problem, &[start], progress);
    states.extend(beam.iter().cloned());
    for _ in 0..depth {
        let mut next = Vec::new();
        for (state, _) in &beam {
            for (c, candidate) in problem.candidates.iter().enumerate() {
                let count = state.modules.iter().filter(|&&m| m == c).count();
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
        let mut scored = evaluate_all(engine, problem, &next, progress);
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
            let mut scored = evaluate_all(engine, problem, &next, progress);
            if scored.is_empty() {
                break;
            }
            sort(&mut scored);
            beam = scored.iter().take(BEAM).cloned().collect();
            states.extend(scored);
        }
    }
    if progress.cancelled() {
        return Vec::new();
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
    all.extend(
        states
            .into_iter()
            .filter(|(s, _)| !problem.need_repair || !any_repair || repairs(s))
            .filter(|(s, _)| !s.modules.is_empty() || !s.rigs.is_empty())
            .map(|(_, o)| o),
    );
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    all.truncate(keep);
    all
}

fn sort(scored: &mut [(State, Outcome)]) {
    scored.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
}

/// Score the states, spread over a few threads; those that do not fit are left out.
fn evaluate_all(
    engine: &Engine,
    problem: &Problem,
    states: &[State],
    progress: &Progress,
) -> Vec<(State, Outcome)> {
    let threads = std::thread::available_parallelism()
        .map_or(2, std::num::NonZero::get)
        .clamp(1, 6);
    let chunk = states.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = states
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|state| {
                            if progress.cancelled() {
                                return None;
                            }
                            let outcome = evaluate(engine, problem, state);
                            progress.done.fetch_add(1, Ordering::Relaxed);
                            outcome.map(|o| (state.clone(), o))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

/// Fit the state's modules and rigs and score the result; None when it does not fit.
fn evaluate(engine: &Engine, problem: &Problem, state: &State) -> Option<Outcome> {
    let modules: Vec<&Candidate> = state
        .modules
        .iter()
        .map(|&m| &problem.candidates[m])
        .collect();
    let rigs: Vec<RigCandidate> = state.rigs.iter().map(|&r| problem.rigs[r]).collect();
    score(engine, problem, &modules, &rigs)
}

/// Score a tank of the problem's kind made of these modules and rigs; None when it does not fit.
/// Modules past the free positions are left off.
pub fn score(
    engine: &Engine,
    problem: &Problem,
    modules: &[&Candidate],
    rigs: &[RigCandidate],
) -> Option<Outcome> {
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
    probe
        .fits()
        .then(|| outcome(problem, modules, rigs, &probe))
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
        // A nudge towards more repair among tanks that resist the same.
        Aim::Resists => resist + 0.01 * effective / (effective + 100.0),
        Aim::Repair => effective + layer_ehp * 1e-6,
        Aim::Balanced => effective * layer_ehp,
        Aim::BufferBalanced => ehp * (1.0 + resist),
    };
    if problem.need_repair && !modules.iter().any(|c| c.role == Role::Repair) {
        score *= 0.5;
    }
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
    ) -> Option<Problem> {
        if choices.tank == TankStyle::None {
            return None;
        }
        let (base, room) = self.plan_base(fit, choices);
        let mut next = fit.clone();
        apply(
            &mut next,
            &self.placements(fit, &base, choices.clear),
            choices.clear,
        );
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
        Some(Problem {
            candidates: self.tank_candidates(choices, weakest),
            rigs: if rig_positions.is_empty() {
                Vec::new()
            } else {
                self.rig_candidates(choices, weakest)
            },
            fit: next,
            pilot: pilot.clone(),
            layer: choices.layer,
            aim: Aim::of(choices),
            positions,
            rig_positions,
            need_repair: choices.tank == TankStyle::Active,
        })
    }

    /// The modules worth trying for the style and layer, repairers first.
    fn tank_candidates(&self, choices: &Choices, weakest: &[usize]) -> Vec<Candidate> {
        let style = choices.tank;
        let active = style == TankStyle::Active;
        let brick = style == TankStyle::Brick;
        let all = usize::MAX;
        let mut wanted: Vec<(String, Role, usize)> = Vec::new();
        let mut want = |name: String, role: Role, max: usize| wanted.push((name, role, max));
        match choices.layer {
            Layer::Armor => {
                if active {
                    want(self.repairer_name(), Role::Repair, MAX_REPAIRERS);
                    want(self.ancillary_name(Layer::Armor), Role::Repair, 1);
                }
                want(self.plate_name().into(), Role::Buffer, all);
                if !brick {
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
                    want(self.booster_name().into(), Role::Repair, MAX_REPAIRERS);
                    want(self.ancillary_name(Layer::Shield), Role::Repair, 1);
                    want(
                        "Shield Boost Amplifier I".into(),
                        Role::Repair,
                        MAX_SPECIFIC,
                    );
                }
                want(self.extender_name().into(), Role::Buffer, all);
                if !brick {
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
            .filter_map(|(name, role, max)| {
                let base = self.base(&name)?;
                let type_id = self.pick(base, category_of(self.catalog, base), &choices.bling);
                let mut module = crate::fit::Module::new(self.catalog, type_id);
                module.charge = self.charge_for(type_id);
                Some(Candidate {
                    base,
                    module,
                    role,
                    max,
                })
            })
            .collect()
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
                if style != TankStyle::Brick {
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
                if style != TankStyle::Brick {
                    for &i in weakest {
                        names.push(format!("{} Shield Reinforcer I", DAMAGE_TYPES[i]));
                    }
                }
            }
        }
        let size = self.facts.size.label();
        names
            .into_iter()
            .filter_map(|name| {
                let base = self.base(&format!("{size} {name}"))?;
                let type_id = self.pick(base, Category::Rig, &choices.bling);
                (self.attr(type_id, attr::UPGRADE_COST) <= self.facts.calibration)
                    .then_some(RigCandidate { base, type_id })
            })
            .collect()
    }
}
