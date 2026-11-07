//! The fitting wizard's picks against the real data. Skipped when no dump is present.

use std::{collections::HashSet, path::Path, sync::OnceLock};

use gyfa::{
    data::{Catalog, Hardpoint, Slot, TypeId},
    engine::Engine,
    fit::Fit,
    item_data::ItemData,
    wizard::{
        self, Ammo, Choices, DamageMods, Facts, Family, HullSize, Prop, Setup, Tank, Tier, Utility,
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
        tier: Tier::T2,
        ammo: Ammo::Faction,
        prop: Prop::Microwarpdrive,
        utilities: vec![Utility::Scram, Utility::Web],
        tank: Tank::ShieldBuffer,
        damage: DamageMods::Weapon,
        rigs: wizard::Rigs::Tank,
        clear: false,
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

    let placements = wizard.placements(&fit, &picks, false);
    let fitted = wizard.make_it_fit(engine, engine.all5(), &fit, &picks, false);
    let stats = wizard::preview(
        engine,
        engine.all5(),
        &fit,
        &wizard.placements(&fit, &fitted, false),
        false,
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
    assert!(matches!(
        choices.tank,
        Tank::ArmorBuffer | Tank::ArmorActive
    ));
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
    let placements = wizard.placements(&fit, &picks, false);
    let mut next = fit.clone();
    wizard::apply(&mut next, &placements, false);
    assert_eq!(next.low[0].as_ref().map(|m| m.type_id), Some(kept));
    assert_eq!(next.low.iter().flatten().count(), 4);
    let mut cleared = fit.clone();
    let all = wizard.plan(
        &fit,
        &Choices {
            clear: true,
            ..choices
        },
    );
    wizard::apply(&mut cleared, &wizard.placements(&fit, &all, true), true);
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
    assert!(
        matches!(
            wizard.defaults().tank,
            wizard::Tank::ShieldBuffer | wizard::Tank::ShieldActive
        ),
        "the Raven's mastery asks for shield tanking"
    );
}
