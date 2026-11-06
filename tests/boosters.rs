//! Boosters against the real data: one per slot, switches kept on swaps, side effects reaching the
//! engine. Skipped when no dump is present.

use std::path::Path;

use gyfa::{
    data::Catalog,
    engine::Engine,
    fit::{Booster, Fit},
};

const DATA: &str = "data/eve";

fn catalog() -> Option<Catalog> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    Some(Catalog::load(Path::new(DATA)).expect("the dump loads"))
}

fn id(catalog: &Catalog, name: &str) -> i32 {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

#[test]
fn one_booster_per_slot_in_slot_order() {
    let Some(catalog) = catalog() else { return };
    let mut fit = Fit::new("test");
    // Crash is slot 3, Blue Pill slot 1.
    assert!(fit.add(&catalog, id(&catalog, "Standard Crash Booster")));
    assert!(fit.add(&catalog, id(&catalog, "Standard Blue Pill Booster")));
    let names = |fit: &Fit| -> Vec<String> {
        fit.boosters
            .iter()
            .map(|b| catalog.name(b.type_id))
            .collect()
    };
    assert_eq!(
        names(&fit),
        ["Standard Blue Pill Booster", "Standard Crash Booster"]
    );

    // Another slot 1 booster replaces the Blue Pill; the same booster twice changes nothing.
    fit.add(&catalog, id(&catalog, "Standard Exile Booster"));
    assert!(!fit.add_booster(
        &catalog,
        Booster::new(id(&catalog, "Standard Exile Booster"))
    ));
    assert_eq!(
        names(&fit),
        ["Standard Exile Booster", "Standard Crash Booster"]
    );
}

#[test]
fn swapping_keeps_switches() {
    let Some(catalog) = catalog() else { return };
    let mut fit = Fit::new("test");
    let strong = id(&catalog, "Strong Blue Pill Booster");
    fit.add(&catalog, strong);
    let effect = catalog.get(strong).expect("booster").side_effects[0];
    fit.boosters[0].active = false;
    fit.boosters[0].side_effects.push(effect);

    let improved = id(&catalog, "Improved Blue Pill Booster");
    assert!(fit.swap_booster(&catalog, 0, improved));
    assert_eq!(
        fit.boosters,
        [Booster {
            type_id: improved,
            active: false,
            side_effects: vec![effect],
        }]
    );
}

#[test]
fn side_effects_change_stats() {
    let Some(catalog) = catalog() else { return };
    let cache = Path::new(DATA)
        .parent()
        .expect("data has a parent")
        .join("cache");
    let engine =
        Engine::load(Path::new(DATA), &cache, catalog.skills.clone()).expect("engine loads");
    let mut fit = Fit::new("test");
    fit.ship = Some(id(&catalog, "Rifter"));
    fit.add(&catalog, id(&catalog, "Strong Blue Pill Booster"));
    let base = engine.compute(&fit);
    let info = &base.boosters[0];
    assert!(info.side_effects.iter().all(|se| !se.active));
    assert!(info.duration.is_some_and(|d| d > 0.0));

    // Blue Pill's shield capacity penalty.
    let shield_penalty = 2737;
    assert!(
        info.side_effects
            .iter()
            .any(|se| se.effect_id == shield_penalty && se.strength.is_some_and(|s| s < 0.0)),
        "side effects: {:?}",
        info.side_effects
    );
    fit.boosters[0].side_effects.push(shield_penalty);
    let penalised = engine.compute(&fit);
    assert!(
        penalised.boosters[0]
            .side_effects
            .iter()
            .any(|se| se.active)
    );
    assert!(
        penalised.shield.hp < base.shield.hp,
        "shield {} -> {}",
        base.shield.hp,
        penalised.shield.hp
    );

    // An inactive booster does nothing at all, side effects included.
    fit.boosters[0].active = false;
    let off = engine.compute(&fit);
    assert!(
        (off.shield.hp - base.shield.hp).abs() < 1e-6,
        "shield {} with the booster off, {} without side effects",
        off.shield.hp,
        base.shield.hp
    );
}
