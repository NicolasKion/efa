//! Implant slots, states, sources and sets against the real data. Skipped when no dump is
//! present.

use std::{path::Path, sync::OnceLock};

use gyfa::{
    data::{Catalog, TypeId},
    fit::{Fit, ImplantSource},
    implant_set::{self, ImplantSet},
};

const DATA: &str = "data/eve";

fn catalog() -> Option<&'static Catalog> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            if !Path::new(DATA).join("phobos/metadata.json").exists() {
                eprintln!("no EVE data in {DATA}, skipping");
                return None;
            }
            Some(Catalog::load(Path::new(DATA)).expect("the dump loads"))
        })
        .as_ref()
}

fn id(catalog: &Catalog, name: &str) -> TypeId {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

#[test]
fn one_implant_per_slot_in_slot_order() {
    let Some(catalog) = catalog() else { return };
    let high = id(catalog, "High-grade Amulet Omega");
    let mid = id(catalog, "Mid-grade Amulet Omega");
    let ocular = id(catalog, "Ocular Filter - Basic");
    let mut fit = Fit::new("test");

    assert!(fit.add_implant(catalog, high));
    assert!(
        !fit.add_implant(catalog, high),
        "the same implant twice changes nothing"
    );
    assert!(fit.add_implant(catalog, mid));
    assert_eq!(
        fit.implants,
        vec![mid],
        "the mid-grade replaced the high-grade"
    );

    assert!(fit.add(catalog, ocular));
    assert_eq!(
        fit.implants,
        vec![ocular, mid],
        "slot 1 lists before slot 6"
    );
}

#[test]
fn states_survive_variation_swaps_but_not_removal() {
    let Some(catalog) = catalog() else { return };
    let high = id(catalog, "High-grade Amulet Omega");
    let mid = id(catalog, "Mid-grade Amulet Omega");
    let ocular = id(catalog, "Ocular Filter - Basic");
    let mut fit = Fit::new("test");
    fit.add_implant(catalog, ocular);
    fit.add_implant(catalog, high);

    // The clicked implant is on, so every target goes off.
    fit.toggle_implants(1, &[0, 1]);
    assert!(!fit.implant_active(ocular) && !fit.implant_active(high));
    fit.toggle_implants(0, &[0]);
    assert!(fit.implant_active(ocular) && !fit.implant_active(high));

    fit.swap_implant(catalog, 1, mid);
    assert_eq!(fit.implants, vec![ocular, mid]);
    assert!(
        !fit.implant_active(mid),
        "the swap keeps the implant switched off"
    );

    fit.remove_implant(1);
    fit.add_implant(catalog, mid);
    assert!(
        fit.implant_active(mid),
        "a re-added implant starts switched on"
    );
}

#[test]
fn character_source_applies_the_character_implants() {
    let Some(catalog) = catalog() else { return };
    let ocular = id(catalog, "Ocular Filter - Basic");
    let high = id(catalog, "High-grade Amulet Omega");
    let mut fit = Fit::new("test");
    fit.add_implant(catalog, ocular);
    fit.implant_source = ImplantSource::Character;

    let applied: Vec<(TypeId, bool)> = fit.applied_implants(&[high]).collect();
    assert_eq!(applied, vec![(high, true)]);
    assert_eq!(fit.applied_implants(&[]).count(), 0);

    // Adding an implant goes back to the fit's own, as in pyfa.
    fit.add_implant(catalog, high);
    assert_eq!(fit.implant_source, ImplantSource::Fit);
    assert_eq!(fit.applied_implants(&[]).count(), 2);
}

#[test]
fn pirate_sets_are_precalculated() {
    let Some(catalog) = catalog() else { return };
    let sets = implant_set::precalculated(catalog);
    let amulet = sets
        .iter()
        .find(|s| s.name == "Amulet")
        .expect("the Amulet set");
    let grades: Vec<Option<&str>> = amulet.grades.iter().map(|g| g.0.as_deref()).collect();
    assert_eq!(
        grades,
        vec![Some("Low-grade"), Some("Mid-grade"), Some("High-grade")]
    );
    let high = &amulet.grades[2].1;
    assert_eq!(high.len(), 6, "Alpha to Omega");
    assert!(high.contains(&id(catalog, "High-grade Amulet Omega")));
    assert!(
        sets.windows(2).all(|w| w[0].name < w[1].name),
        "sorted by name"
    );
    assert!(sets.iter().all(|s| s.grades.iter().all(|g| g.1.len() >= 2)));
}

#[test]
fn sets_round_trip_through_the_clipboard_format() {
    let Some(catalog) = catalog() else { return };
    let mut set = ImplantSet::new("Brawl");
    set.add(catalog, id(catalog, "High-grade Amulet Omega"));
    set.add(catalog, id(catalog, "Ocular Filter - Basic"));
    let text = implant_set::export(catalog, std::slice::from_ref(&set));
    let imported = implant_set::import(catalog, &format!("{text}\nNot An Implant\n"));
    assert_eq!(imported.sets, vec![set.clone()]);
    assert_eq!(imported.errors, 1);

    // Importing into an existing set only fills its free slots.
    let mut saved = vec![ImplantSet::new("Brawl")];
    saved[0].add(catalog, id(catalog, "Mid-grade Amulet Omega"));
    implant_set::merge(&mut saved, imported.sets, catalog);
    assert_eq!(
        saved[0].implants,
        vec![
            id(catalog, "Ocular Filter - Basic"),
            id(catalog, "Mid-grade Amulet Omega")
        ]
    );
}
