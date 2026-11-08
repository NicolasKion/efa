//! The fitting wizard's tank section against the real data. Skipped when no dump is present.

use std::{collections::HashSet, path::Path, sync::OnceLock};

use efa::{
    data::{Catalog, Slot, TypeId},
    engine::Engine,
    fit::{DamageProfile, Fit},
    item_data::ItemData,
    prices::Prices,
    wizard::{
        self, Budget, Facts, Layer, SearchGroup, SearchTiers, Setup,
        tank::{self, Choices, Goal, Outcome, Problem, Progress},
    },
};

const DATA: &str = "data/eve";

fn setup() -> Option<&'static (Catalog, Engine, ItemData)> {
    static SETUP: OnceLock<Option<(Catalog, Engine, ItemData)>> = OnceLock::new();
    SETUP
        .get_or_init(|| {
            if !Path::new(DATA).join("phobos/metadata.json").exists() {
                eprintln!("no EVE data in {DATA}, skipping");
                return None;
            }
            let cache = Path::new("data/cache");
            let catalog = Catalog::load_cached(Path::new(DATA), cache).expect("the dump loads");
            let engine = Engine::load(Path::new(DATA), cache, catalog.skills.clone())
                .expect("the engine loads");
            let wanted: HashSet<TypeId> = catalog.types.keys().copied().collect();
            let key = efa::data::cache_key(Path::new(DATA));
            let data = ItemData::load_cached(Path::new(DATA), cache, key.as_deref(), &wanted)
                .expect("the item data loads");
            Some((catalog, engine, data))
        })
        .as_ref()
}

fn ship(catalog: &Catalog, name: &str) -> Fit {
    let mut fit = Fit::new(name);
    fit.add(catalog, catalog.find(name).expect("the ship exists").id);
    fit
}

fn id(catalog: &Catalog, name: &str) -> TypeId {
    catalog.find(name).expect(name).id
}

fn facts(catalog: &Catalog, engine: &Engine, data: &ItemData, fit: &Fit) -> Facts {
    Facts::gather(engine, data, catalog, fit, engine.all5()).expect("a ship")
}

/// The search for the choices on `fit`, the layer the hull is built for unless `edit` says
/// otherwise.
fn problem(
    (catalog, engine, data): &(Catalog, Engine, ItemData),
    fit: &Fit,
    prices: &Prices,
    edit: impl FnOnce(&mut Choices),
) -> Problem {
    let facts = facts(catalog, engine, data, fit);
    let mut choices = Choices {
        layer: facts.layer(),
        ..Choices::default()
    };
    edit(&mut choices);
    Setup::new(catalog, data, &facts).tank_problem(engine, engine.all5(), fit, &choices, prices)
}

/// The best tanks for the choices on `fit`.
fn best(
    all: &(Catalog, Engine, ItemData),
    fit: &Fit,
    edit: impl FnOnce(&mut Choices),
) -> (Problem, Vec<Outcome>) {
    let problem = problem(all, fit, &Prices::default(), edit);
    let found = tank::optimise(&all.1, &problem, &Progress::default(), 5);
    (problem, found.outcomes)
}

fn names(catalog: &Catalog, ids: &[TypeId]) -> Vec<String> {
    ids.iter().map(|&t| catalog.name(t)).collect()
}

#[test]
fn the_layer_starts_at_what_the_hull_is_built_for() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let layer = |name: &str| facts(catalog, engine, data, &ship(catalog, name)).layer();
    assert_eq!(layer("Raven"), Layer::Shield, "the Raven's mastery");
    assert_eq!(layer("Drake"), Layer::Shield);
    assert_eq!(layer("Abaddon"), Layer::Armor);
}

#[test]
fn max_ehp_takes_extenders_and_ranks_by_ehp() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    let fit = ship(catalog, "Drake");
    let (_, outcomes) = best(all, &fit, |c| {
        c.layer = Layer::Shield;
        c.goal = Goal::Ehp;
    });
    let found = outcomes.first().expect("a tank");
    let names = names(catalog, &found.types);
    assert!(
        names.iter().any(|n| n.contains("Shield Extender")),
        "{names:?}"
    );
    assert!(!names.iter().any(|n| n.contains("Amplifier")), "{names:?}");
    assert!(
        outcomes
            .windows(2)
            .all(|w| w[0].numbers.ehp >= w[1].numbers.ehp),
        "best EHP first"
    );
}

#[test]
fn max_resists_beats_max_ehp_on_the_multiplier() {
    let Some(all) = setup() else {
        return;
    };
    let fit = ship(&all.0, "Abaddon");
    let run = |goal| {
        best(all, &fit, |c| {
            c.layer = Layer::Armor;
            c.goal = goal;
        })
        .1
    };
    let resists = run(Goal::Resists);
    let ehp = run(Goal::Ehp);
    let (r, e) = (&resists[0], &ehp[0]);
    assert!(
        r.numbers.multiplier > e.numbers.multiplier,
        "{} vs {}",
        r.numbers.multiplier,
        e.numbers.multiplier
    );
    assert!(
        e.numbers.ehp > r.numbers.ehp,
        "{} vs {}",
        e.numbers.ehp,
        r.numbers.ehp
    );
    assert!(
        resists
            .windows(2)
            .all(|w| w[0].numbers.multiplier >= w[1].numbers.multiplier - 0.02),
        "the highest multiplier first"
    );
    assert!(
        (r.numbers.multiplier - 1.0 / (1.0 - r.numbers.resist)).abs() < 1e-9,
        "the multiplier is EHP per raw HP"
    );
}

#[test]
fn max_repair_ranks_by_repaired_ehp_a_second() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    let fit = ship(catalog, "Abaddon");
    let (problem, outcomes) = best(all, &fit, |c| {
        c.layer = Layer::Armor;
        c.goal = Goal::Repair;
    });
    assert!(problem.need_repair);
    let found = outcomes.first().expect("a tank");
    let names = names(catalog, &found.types);
    assert!(
        names.iter().any(|n| n.contains("Armor Repairer")),
        "{names:?}"
    );
    assert!(
        (found.numbers.repair_ehp - found.numbers.repair * found.numbers.multiplier).abs() < 1e-6,
        "the raw repair times the multiplier"
    );
    assert!(
        outcomes
            .windows(2)
            .all(|w| w[0].numbers.repair_ehp >= w[1].numbers.repair_ehp),
        "the most repair first"
    );
    let (_, ehp) = best(all, &fit, |c| {
        c.layer = Layer::Armor;
        c.goal = Goal::Ehp;
    });
    assert!(found.numbers.repair_ehp > ehp[0].numbers.repair_ehp);
    // It beats what a hand fit would do: two repairers, a membrane and a damage control.
    let by = |name: &str| {
        problem
            .candidates
            .iter()
            .find(|c| catalog.name(c.base) == name)
            .expect(name)
    };
    let repairer = by("Large Armor Repairer I");
    let naive = [
        repairer,
        by("Multispectrum Energized Membrane I"),
        repairer,
        by("Damage Control I"),
    ];
    let pump = problem
        .rigs
        .iter()
        .find(|r| catalog.name(r.base).contains("Auxiliary Nano Pump"))
        .copied()
        .expect("a nano pump");
    let rigs = vec![pump; problem.rig_positions.len()];
    let naive = (1..=naive.len())
        .rev()
        .find_map(|n| tank::score(&all.1, &problem, &naive[..n], &rigs))
        .expect("some of the hand fit fits");
    assert!(found.numbers.repair_ehp > naive.numbers.repair_ehp);
}

#[test]
fn a_repair_tank_takes_the_chosen_number_of_repairers() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    let fit = ship(catalog, "Abaddon");
    for wanted in [1_u8, 2] {
        let (_, outcomes) = best(all, &fit, |c| {
            c.layer = Layer::Armor;
            c.goal = Goal::Repair;
            c.repairers = Some(wanted);
        });
        let found = outcomes.first().expect("a tank");
        let repairers = found
            .types
            .iter()
            .filter(|&&t| catalog.name(t).contains("Repairer"))
            .count();
        assert_eq!(repairers, usize::from(wanted), "{wanted} repairers wanted");
    }
}

#[test]
fn two_repairers_fit_with_an_ancillary_current_router() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    // Two medium repairers are more powergrid than an Arbitrator with beams has left;
    // a powergrid rig makes the room.
    let mut fit = ship(catalog, "Arbitrator");
    let gun = id(catalog, "Heavy Beam Laser II");
    for _ in 0..3 {
        fit.add(catalog, gun);
    }
    let choose = |c: &mut Choices| {
        c.layer = Layer::Armor;
        c.goal = Goal::Repair;
        c.repairers = Some(2);
    };
    let (mut problem, outcomes) = best(all, &fit, choose);
    let found = outcomes.first().expect("a tank");
    let repairers = names(catalog, &found.types)
        .iter()
        .filter(|n| n.contains("Armor Repairer"))
        .count();
    assert_eq!(repairers, 2, "{:?}", names(catalog, &found.types));
    let rigs = names(catalog, &found.rig_types);
    assert!(
        rigs.iter().any(|n| n.contains("Ancillary Current Router")),
        "{rigs:?}"
    );
    // Without the fitting rigs, two do not fit.
    problem.rigs.retain(|r| !r.fitting);
    let without = tank::optimise(&all.1, &problem, &Progress::default(), 5);
    assert_eq!(without.repairers_short, Some(1), "{without:?}");
}

#[test]
fn max_ehp_matches_a_brute_force_search() {
    let Some(all) = setup() else {
        return;
    };
    let (catalog, engine, _) = all;
    // One damage type keeps the candidates few enough to try every tank.
    let mut fit = ship(catalog, "Punisher");
    fit.damage_profile = DamageProfile::Pattern {
        name: "EM".into(),
        amounts: [1.0, 0.0, 0.0, 0.0],
    };
    let mut problem = problem(all, &fit, &Prices::default(), |c| {
        c.layer = Layer::Armor;
        c.goal = Goal::Ehp;
    });
    problem.rigs.clear();
    problem.rig_positions.clear();
    let found = tank::optimise(engine, &problem, &Progress::default(), 1);
    let best = found.outcomes.first().expect("a tank");

    // Every multiset of candidates up to the free slots, within each module's limit.
    let n = problem.candidates.len();
    let within = |t: &[usize]| {
        t.iter().all(|&c| {
            let base = problem.candidates[c].base;
            t.iter()
                .filter(|&&o| problem.candidates[o].base == base)
                .count()
                <= problem.candidates[c].max
        })
    };
    let mut tanks: Vec<Vec<usize>> = Vec::new();
    let mut frontier: Vec<Vec<usize>> = vec![Vec::new()];
    for _ in 0..problem.positions.len() {
        frontier = frontier
            .iter()
            .flat_map(|t| {
                (t.last().copied().unwrap_or(0)..n).map(move |c| {
                    let mut t = t.clone();
                    t.push(c);
                    t
                })
            })
            .filter(|t| within(t))
            .collect();
        tanks.extend(frontier.iter().cloned());
    }
    let brute = std::thread::scope(|scope| {
        let chunk = tanks.len().div_ceil(6).max(1);
        let handles: Vec<_> = tanks
            .chunks(chunk)
            .map(|chunk| {
                let problem = &problem;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|t| {
                            let modules: Vec<_> =
                                t.iter().map(|&c| &problem.candidates[c]).collect();
                            tank::score(engine, problem, &modules, &[]).map(|o| o.numbers.ehp)
                        })
                        .fold(0.0_f64, f64::max)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("joins"))
            .fold(0.0_f64, f64::max)
    });
    assert!(
        best.numbers.ehp >= brute * (1.0 - 1e-9),
        "the search found {}, every tank tried gives {brute}",
        best.numbers.ehp
    );
}

#[test]
fn everything_and_max_resists_reach_for_officer_membranes() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    let fit = ship(catalog, "Abaddon");
    let (problem, outcomes) = best(all, &fit, |c| {
        c.layer = Layer::Armor;
        c.goal = Goal::Resists;
        c.search_tiers = SearchTiers::everything();
    });
    let officer = |t: &TypeId| catalog.get(*t).is_some_and(|t| t.meta_group == Some(5));
    assert!(
        problem
            .candidates
            .iter()
            .any(|c| officer(&c.module.type_id)),
        "officer variations are tried"
    );
    let found = outcomes.first().expect("a tank");
    let names = names(catalog, &found.types);
    assert!(
        found
            .types
            .iter()
            .any(|t| officer(t) && catalog.name(*t).contains("Membrane")),
        "{names:?}"
    );
}

#[test]
fn wider_search_tiers_try_faction_hardeners() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    let fit = ship(catalog, "Abaddon");
    let (problem, outcomes) = best(all, &fit, |c| {
        c.layer = Layer::Armor;
        c.goal = Goal::Resists;
        c.search_tiers = SearchTiers::of(&[SearchGroup::T2, SearchGroup::Faction]);
    });
    assert!(
        problem.candidates.iter().any(|c| catalog
            .get(c.module.type_id)
            .is_some_and(|t| t.meta_group == Some(4))),
        "faction variations are tried"
    );
    assert!(!outcomes.is_empty(), "a tank is found");
}

#[test]
fn a_tank_budget_caps_the_price() {
    let Some(all) = setup() else {
        return;
    };
    let catalog = &all.0;
    let fit = ship(catalog, "Abaddon");
    // Faction items cost 100M each, everything else 1M.
    let prices = Prices::fixed(catalog.types.values().map(|t| {
        let price = if t.meta_group == Some(4) { 100e6 } else { 1e6 };
        (t.id, price)
    }));
    let run = |budget: Budget| {
        let problem = problem(all, &fit, &prices, |c| {
            c.layer = Layer::Armor;
            c.search_tiers = SearchTiers::of(&[SearchGroup::T2, SearchGroup::Faction]);
            c.budget = budget;
        });
        tank::optimise(&all.1, &problem, &Progress::default(), 5)
    };
    let free = run(Budget::default());
    let best = free.outcomes.first().expect("a tank");
    assert!(best.priced && best.price > 0.0);
    let capped = run(Budget {
        each: None,
        total: Some(20_000_000),
    });
    let cheap = capped.outcomes.first().expect("a cheap tank");
    assert!(cheap.price <= 20e6, "{}", cheap.price);
    let per_module = run(Budget {
        each: Some(10_000_000),
        total: None,
    });
    assert!(
        per_module.outcomes.iter().all(|o| o
            .types
            .iter()
            .all(|&t| catalog.get(t).is_some_and(|t| t.meta_group != Some(4)))),
        "no faction module over the per-module budget"
    );
}

#[test]
fn a_budget_leaves_out_what_has_no_price() {
    let Some(all) = setup() else {
        return;
    };
    let fit = ship(&all.0, "Abaddon");
    let problem = problem(all, &fit, &Prices::default(), |c| {
        c.layer = Layer::Armor;
        c.budget.total = Some(1_000_000_000);
    });
    assert!(problem.candidates.is_empty() && problem.rigs.is_empty());
}

#[test]
fn refit_redoes_the_fitted_tank_and_keeps_the_rest() {
    let Some(all) = setup() else {
        return;
    };
    let (catalog, engine, _) = all;
    let mut fit = ship(catalog, "Abaddon");
    let (gun, plate, heat) = (
        id(catalog, "Mega Pulse Laser II"),
        id(catalog, "1600mm Steel Plates I"),
        id(catalog, "Heat Sink II"),
    );
    for _ in 0..4 {
        fit.add(catalog, gun);
    }
    fit.add(catalog, plate);
    fit.add(catalog, heat);
    fit.add(catalog, plate);
    let lows = fit.low.iter().flatten().count();
    assert_eq!(lows, 3);
    let (problem, outcomes) = best(all, &fit, |c| {
        c.layer = Layer::Armor;
        c.refit = true;
    });
    assert_eq!(problem.removed, [(Slot::Low, 0), (Slot::Low, 2)]);
    let facts = facts(catalog, engine, &all.2, &fit);
    assert_eq!(problem.positions.len(), facts.total(Slot::Low) - 1);
    let found = outcomes.first().expect("a tank");
    let mut next = fit.clone();
    wizard::apply(&mut next, &found.change(&problem, catalog));
    let low: Vec<TypeId> = next.low.iter().flatten().map(|m| m.type_id).collect();
    assert_eq!(low.len(), facts.total(Slot::Low), "every low is filled");
    assert_eq!(
        low.iter().filter(|&&t| t == heat).count(),
        1,
        "the heat sink stays"
    );
    assert!(!low.contains(&plate), "the T1 plates make way: {low:?}");
    let mut found_types = found.types.clone();
    let mut fitted: Vec<TypeId> = low.into_iter().filter(|&t| t != heat).collect();
    found_types.sort_unstable();
    fitted.sort_unstable();
    assert_eq!(fitted, found_types, "what the search found goes on");
    assert_eq!(
        next.high
            .iter()
            .flatten()
            .filter(|m| m.type_id == gun)
            .count(),
        4,
        "the guns stay"
    );

    // Free only keeps the plates and searches around them.
    let kept = problem_free(all, &fit);
    assert_eq!(kept.removed, [] as [(efa::data::Slot, usize); 0]);
    assert_eq!(kept.positions.len(), facts.total(Slot::Low) - 3);
}

fn problem_free(all: &(Catalog, Engine, ItemData), fit: &Fit) -> Problem {
    problem(all, fit, &Prices::default(), |c| {
        c.layer = Layer::Armor;
        c.refit = false;
    })
}

#[test]
fn budgets_read_like_eve_amounts() {
    assert_eq!(Budget::parse("75m"), Some(75_000_000));
    assert_eq!(Budget::parse("1.5b"), Some(1_500_000_000));
    assert_eq!(Budget::parse("250k"), Some(250_000));
    assert_eq!(Budget::parse("2,000,000 ISK"), Some(2_000_000));
    assert_eq!(Budget::parse(""), None);
    assert_eq!(Budget::parse("lots"), None);
    assert_eq!(Budget::label(Some(1_500_000_000)), "1.5B");
    assert_eq!(Budget::label(Some(75_000_000)), "75M");
}
