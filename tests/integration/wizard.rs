//! The fitting wizard's picks against the real data. Skipped when no dump is present.

use std::{collections::HashSet, path::Path, sync::OnceLock};

use gyfa::{
    data::{Catalog, Hardpoint, Slot, TypeId},
    engine::Engine,
    fit::Fit,
    item_data::ItemData,
    wizard::{
        self, Ammo, Bling, Category, Choices, Cleared, DamageMods, Facts, Family, Goal, HullSize,
        Layer, Prop, Setup, Tab, Tabs, TankStyle, Tier, Utility,
        tank::{self, Progress},
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
            let key = gyfa::data::cache_key(Path::new(DATA));
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

fn names(catalog: &Catalog, picks: &[wizard::Pick], slot: Slot) -> Vec<String> {
    picks
        .iter()
        .filter(|p| p.slot == slot)
        .map(|p| catalog.name(p.type_id))
        .collect()
}

fn hull(catalog: &Catalog, engine: &Engine, data: &ItemData, fit: &Fit) -> (Facts, Vec<Family>) {
    let facts = Facts::gather(engine, data, catalog, fit, engine.all5()).expect("a ship");
    let families = wizard::families(catalog, data, &facts);
    (facts, families)
}

#[test]
fn a_hurricane_gets_a_sane_brawler() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Hurricane");
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    assert_eq!(wizard.facts.size, HullSize::Medium);
    let family = wizard.families.first().expect("weapons");
    assert_eq!(family.hardpoint, Hardpoint::Turret);
    let ac = wizard
        .families
        .iter()
        .position(|f| f.name == "425mm AutoCannon")
        .expect("425s are offered");
    assert!(
        wizard
            .families
            .iter()
            .any(|f| f.name == "720mm Howitzer Artillery"),
        "artillery is offered"
    );
    let choices = Choices {
        family: Some(ac),
        ammo: Ammo::Faction,
        prop: Prop::Microwarpdrive,
        utilities: vec![Utility::Scram, Utility::Web],
        tank: TankStyle::Buffer,
        layer: Layer::Shield,
        goal: Goal::Balanced,
        tank_fit: None,
        damage: DamageMods::Weapon,
        rigs: wizard::Rigs::Tank,
        clear: Tabs::NONE,
        touched: Tabs::ALL,
        bling: Bling::default(),
    };
    let picks = wizard.plan(&fit, &choices);
    let highs = names(catalog, &picks, Slot::High);
    assert_eq!(highs, vec!["425mm AutoCannon II"; 6]);
    assert!(picks[0].charge.is_some(), "the guns are loaded");
    let mids = names(catalog, &picks, Slot::Mid);
    assert_eq!(
        mids,
        [
            "50MN Microwarpdrive II",
            "Warp Scrambler II",
            "Stasis Webifier II",
            "Large Shield Extender II"
        ]
    );
    let lows = names(catalog, &picks, Slot::Low);
    assert_eq!(
        lows.iter().filter(|n| *n == "Gyrostabilizer II").count(),
        4,
        "{lows:?}"
    );
    assert!(lows.iter().any(|n| n == "Damage Control II"), "{lows:?}");
    let rigs = names(catalog, &picks, Slot::Rig);
    assert!(
        !rigs.is_empty()
            && rigs
                .iter()
                .all(|r| r == "Medium Core Defense Field Extender I")
    );

    let placements = wizard.placements(&fit, &picks, Cleared::NONE);
    let fitted = wizard.make_it_fit(engine, engine.all5(), &fit, &picks, Cleared::NONE);
    let stats = wizard::preview(
        engine,
        engine.all5(),
        &fit,
        &wizard.placements(&fit, &fitted, Cleared::NONE),
        Cleared::NONE,
    );
    assert!(wizard::fits(&stats), "made to fit");
    assert!(placements.len() >= fitted.len());
    assert!(
        fitted.iter().filter(|p| p.slot == Slot::High).count() == 6,
        "guns are never dropped"
    );
}

#[test]
fn a_vexor_picks_drone_damage_amplifiers() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Vexor");
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    let choices = wizard.defaults();
    assert_eq!(choices.damage, DamageMods::Drone);
    assert_eq!(choices.layer, Layer::Armor);
    let picks = wizard.plan(&fit, &choices);
    let lows = names(catalog, &picks, Slot::Low);
    assert!(
        lows.iter().any(|n| n == "Drone Damage Amplifier II"),
        "{lows:?}"
    );
    let family = wizard.family(&choices).expect("guns");
    assert!(family.name.contains("Blaster") || family.name.contains("Rail"));
}

#[test]
fn a_raven_picks_large_launchers() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Raven");
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    let choices = wizard.defaults();
    let family = wizard.family(&choices).expect("weapons");
    assert_eq!(family.hardpoint, Hardpoint::Launcher);
    assert_eq!(family.size, HullSize::Large);
    let picks = wizard.plan(&fit, &choices);
    let highs = names(catalog, &picks, Slot::High);
    assert_eq!(highs.len(), 6, "{highs:?}");
    assert_eq!(
        names(catalog, &picks, Slot::Low)[0],
        "Ballistic Control System II"
    );
}

#[test]
fn applying_fills_free_slots_and_keeps_fitted_modules() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let mut fit = ship(catalog, "Rifter");
    let kept = catalog.find("Damage Control II").expect("exists").id;
    fit.add(catalog, kept);
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    let choices = wizard.defaults();
    let picks = wizard.plan(&fit, &choices);
    assert_eq!(picks.iter().filter(|p| p.slot == Slot::Low).count(), 3);
    let placements = wizard.placements(&fit, &picks, Cleared::NONE);
    let mut next = fit.clone();
    wizard::apply(&mut next, &placements, Cleared::NONE);
    assert_eq!(next.low[0].as_ref().map(|m| m.type_id), Some(kept));
    assert_eq!(next.low.iter().flatten().count(), 4);
    let mut cleared = fit.clone();
    let all = wizard.plan(
        &fit,
        &Choices {
            clear: Tabs::ALL,
            ..choices
        },
    );
    wizard::apply(
        &mut cleared,
        &wizard.placements(&fit, &all, Cleared::ALL),
        Cleared::ALL,
    );
    assert_eq!(cleared.low.iter().flatten().count(), 4);
}

#[test]
fn masteries_decide_the_weapons() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Abaddon");
    let (_, families) = hull(catalog, engine, data, &fit);
    assert!(!families.is_empty(), "the Abaddon gets weapons");
    assert!(
        families
            .iter()
            .all(|f| f.kind == wizard::WeaponKind::Energy),
        "only lasers on an Abaddon: {:?}",
        families.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    let fit = ship(catalog, "Raven");
    let (facts, families) = hull(catalog, engine, data, &fit);
    assert!(
        families
            .iter()
            .all(|f| f.kind == wizard::WeaponKind::Missile)
    );
    let wizard = Setup::new(catalog, data, &facts, &families);
    assert_eq!(
        wizard.defaults().layer,
        Layer::Shield,
        "the Raven's mastery asks for shield tanking"
    );
}

#[test]
fn battleships_can_take_a_grappler() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Abaddon");
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    assert!(wizard::Utility::Grappler.fits(facts.size));
    let name = wizard.utility_name(wizard::Utility::Grappler, None);
    assert!(catalog.find(&name).is_some(), "{name} is in the catalog");
    let mut choices = wizard.defaults();
    choices.utilities = vec![wizard::Utility::Grappler];
    let picks = wizard.plan(&fit, &choices);
    assert!(
        names(catalog, &picks, Slot::Mid)
            .iter()
            .any(|n| n.contains("Stasis Grappler")),
        "the grappler goes in a mid slot"
    );
    let frigate = ship(catalog, "Rifter");
    let (facts, _) = hull(catalog, engine, data, &frigate);
    assert!(!wizard::Utility::Grappler.fits(facts.size));
}

/// The optimiser's best tanks for the choices.
fn best(
    catalog: &Catalog,
    engine: &Engine,
    data: &ItemData,
    fit: &Fit,
    edit: impl FnOnce(&mut Choices),
) -> (tank::Problem, Vec<tank::Outcome>) {
    let (facts, families) = hull(catalog, engine, data, fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    let mut choices = wizard.defaults();
    edit(&mut choices);
    let problem = wizard
        .tank_problem(engine, engine.all5(), fit, &choices)
        .expect("a tank to search for");
    let found = tank::optimise(engine, &problem, &Progress::default(), 5);
    (problem, found.outcomes)
}

#[test]
fn an_abaddon_active_tank_beats_the_naive_one() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Abaddon");
    let (problem, outcomes) = best(catalog, engine, data, &fit, |c| {
        c.tank = TankStyle::Active;
        c.layer = Layer::Armor;
        c.goal = Goal::Balanced;
    });
    let found = outcomes.first().expect("a tank");
    let names: Vec<String> = found.types.iter().map(|&t| catalog.name(t)).collect();
    assert!(
        names.iter().any(|n| n.contains("Armor Repairer")),
        "{names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("Reactive Armor Hardener") || n.contains("Multispectrum")),
        "{names:?}"
    );
    // What the wizard used to fit: two repairers, a membrane and a damage control.
    let by = |name: &str| {
        problem
            .candidates
            .iter()
            .find(|c| catalog.name(c.base) == name)
            .expect(name)
    };
    let repairer = by("Large Armor Repairer I");
    let membrane = by("Multispectrum Energized Membrane I");
    let control = by("Damage Control I");
    let naive = [repairer, membrane, repairer, control];
    let pump: Vec<_> = problem
        .rigs
        .iter()
        .filter(|r| catalog.name(r.base).contains("Auxiliary Nano Pump"))
        .copied()
        .collect();
    let rigs = vec![pump[0]; problem.rig_positions.len()];
    // As much of it as the CPU and powergrid take.
    let naive = (1..=naive.len())
        .rev()
        .find_map(|n| tank::score(engine, &problem, &naive[..n], &rigs))
        .expect("some of the naive tank fits");
    assert!(
        found.score > naive.score,
        "found {names:?} at {}, naive at {}",
        found.score,
        naive.score
    );
}

#[test]
fn a_drake_buffer_takes_extenders_over_amplifiers() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Drake");
    let (_, outcomes) = best(catalog, engine, data, &fit, |c| {
        c.tank = TankStyle::Buffer;
        c.layer = Layer::Shield;
        c.goal = wizard::Goal::Ehp;
    });
    let found = outcomes.first().expect("a tank");
    let names: Vec<String> = found.types.iter().map(|&t| catalog.name(t)).collect();
    assert!(
        names.iter().any(|n| n.contains("Shield Extender")),
        "{names:?}"
    );
    assert!(!names.iter().any(|n| n.contains("Amplifier")), "{names:?}");
    assert!(
        outcomes.windows(2).all(|w| w[0].ehp >= w[1].ehp),
        "best EHP first"
    );
}

#[test]
fn a_brick_abaddon_gets_plates() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Abaddon");
    let (problem, outcomes) = best(catalog, engine, data, &fit, |c| {
        c.tank = TankStyle::Brick;
        c.layer = Layer::Armor;
    });
    let found = outcomes.first().expect("a tank");
    assert_eq!(found.modules.len(), problem.positions.len());
    assert!(
        found
            .types
            .iter()
            .all(|&t| catalog.name(t).contains("Plates")),
        "{:?}",
        found
            .types
            .iter()
            .map(|&t| catalog.name(t))
            .collect::<Vec<_>>()
    );
}

#[test]
fn bling_picks_the_variation() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Hurricane");
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    let mut choices = wizard.defaults();
    let mwd = catalog.find("50MN Microwarpdrive I").expect("exists").id;
    let restrained = catalog
        .find("50MN Quad LiF Restrained Microwarpdrive")
        .expect("exists")
        .id;
    assert!(wizard.variants(mwd).contains(&restrained));
    choices
        .bling
        .choose(Category::Propulsion, mwd, restrained, Tier::Meta);
    choices
        .bling
        .categories
        .insert(Category::Tackle, Tier::Faction);
    let picks = wizard.plan(&fit, &choices);
    let mids = names(catalog, &picks, Slot::Mid);
    assert!(
        mids.iter()
            .any(|n| n == "50MN Quad LiF Restrained Microwarpdrive"),
        "{mids:?}"
    );
    assert!(
        mids.iter()
            .any(|n| n.contains("Warp Scrambler") && !n.ends_with(" II")),
        "a faction scram: {mids:?}"
    );
    // A found tank goes on as it was found.
    let (_, outcomes) = best(catalog, engine, data, &fit, |c| {
        c.tank = TankStyle::Buffer;
        c.layer = Layer::Shield;
    });
    choices.tank = TankStyle::Buffer;
    choices.layer = Layer::Shield;
    choices.tank_fit = Some(outcomes[0].tank_fit());
    let picks = wizard.plan(&fit, &choices);
    let fitted: Vec<TypeId> = picks
        .iter()
        .filter(|p| p.slot == Slot::Mid)
        .map(|p| p.type_id)
        .collect();
    assert!(
        outcomes[0].types.iter().all(|t| fitted.contains(t)),
        "the found tank is planned"
    );
}

#[test]
fn a_buffer_tank_mixes_in_resists() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let fit = ship(catalog, "Abaddon");
    let (_, outcomes) = best(catalog, engine, data, &fit, |c| {
        c.tank = TankStyle::Buffer;
        c.layer = Layer::Armor;
        c.goal = wizard::Goal::Balanced;
    });
    let found = outcomes.first().expect("a tank");
    let names: Vec<String> = found.types.iter().map(|&t| catalog.name(t)).collect();
    assert!(
        names.iter().any(|n| n.contains("Plates")),
        "some buffer: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("Membrane")
            || n.contains("Coating")
            || n.contains("Hardener")
            || n.contains("Damage Control")),
        "resist modules too: {names:?}"
    );
}

#[test]
fn the_tank_tab_alone_fills_free_lows_and_rigs_around_the_fit() {
    let Some((catalog, engine, data)) = setup() else {
        return;
    };
    let mut fit = ship(catalog, "Abaddon");
    let id = |name: &str| catalog.find(name).expect(name).id;
    let (gun, rep) = (id("Mega Pulse Laser II"), id("Large Armor Repairer II"));
    for _ in 0..4 {
        fit.add(catalog, gun);
    }
    fit.add(catalog, rep);
    fit.add(catalog, rep);
    let kept: Vec<_> = fit.low.iter().flatten().map(|m| m.type_id).collect();
    assert_eq!(kept, [rep, rep]);
    let (facts, families) = hull(catalog, engine, data, &fit);
    let wizard = Setup::new(catalog, data, &facts, &families);
    let mut choices = Choices {
        touched: Tabs::NONE.with(Tab::Tank),
        tank: TankStyle::Active,
        layer: Layer::Armor,
        goal: Goal::Resists,
        ..wizard.defaults()
    };
    let problem = wizard
        .tank_problem(engine, engine.all5(), &fit, &choices)
        .expect("a tank to search for");
    assert_eq!(problem.positions.len(), facts.total(Slot::Low) - 2);
    assert_eq!(problem.rig_positions.len(), facts.total(Slot::Rig));
    assert!(!problem.need_repair, "the fitted repairers count");
    let found = tank::optimise(engine, &problem, &Progress::default(), 5);
    let best = found.outcomes.first().expect("a tank");
    choices.tank_fit = Some(best.tank_fit());
    let picks = wizard.plan(&fit, &choices);
    assert!(
        picks
            .iter()
            .all(|p| matches!(p.slot, Slot::Low | Slot::Rig)),
        "{picks:?}"
    );
    assert!(picks.iter().any(|p| p.slot == Slot::Low), "{picks:?}");
    let mut next = fit.clone();
    wizard::apply(
        &mut next,
        &wizard.placements(&fit, &picks, choices.cleared()),
        choices.cleared(),
    );
    assert_eq!(
        next.high
            .iter()
            .flatten()
            .filter(|m| m.type_id == gun)
            .count(),
        4,
        "the guns stay and none are added"
    );
    assert_eq!(
        next.mid.iter().flatten().count(),
        0,
        "no tackle or propulsion"
    );
    let lows: Vec<_> = next.low.iter().flatten().map(|m| m.type_id).collect();
    assert_eq!(lows[..2], [rep, rep], "the repairers stay where they were");
    assert_eq!(
        lows.len(),
        facts.total(Slot::Low),
        "the free lows are filled"
    );
}
