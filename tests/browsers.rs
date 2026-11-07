//! The market and fittings browsers against the real data: jargon search, meta buttons, charges
//! for a fit, ship groups, races and the Omega marker. Skipped when no dump is present.

use std::path::Path;

use gyfa::{
    data::Catalog,
    fit::{Fit, Module, ModuleState},
    market::{MetaCategory, charges_for_fit},
    ship_browser::Race,
};

const DATA: &str = "data/eve";

fn catalog() -> Option<Catalog> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    Some(Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads"))
}

fn names(catalog: &Catalog, query: &str) -> Vec<String> {
    catalog
        .search(query, 300)
        .into_iter()
        .map(|t| catalog.name(t))
        .collect()
}

#[test]
fn jargon_finds_what_it_abbreviates() {
    let Some(catalog) = catalog() else { return };
    let hml = names(&catalog, "hml 2");
    assert!(
        hml.contains(&"Heavy Missile Launcher II".to_owned()),
        "{hml:?}"
    );
    assert!(!hml.iter().any(|n| n.starts_with("Rapid")), "{hml:?}");
    let lse = names(&catalog, "lse");
    assert!(
        lse.contains(&"Large Shield Extender II".to_owned()),
        "{lse:?}"
    );
}

#[test]
fn search_lists_variations_together() {
    let Some(catalog) = catalog() else { return };
    let found = names(&catalog, "small armor repairer");
    let t1 = found.iter().position(|n| n == "Small Armor Repairer I");
    let t2 = found.iter().position(|n| n == "Small Armor Repairer II");
    assert!(t1.zip(t2).is_some_and(|(a, b)| a < b), "{found:?}");
}

#[test]
fn meta_buttons_sort_items() {
    let Some(catalog) = catalog() else { return };
    let meta = |name: &str| MetaCategory::of_item(catalog.find(name).expect("in the catalog"));
    assert_eq!(meta("Small Armor Repairer II"), MetaCategory::Normal);
    assert_eq!(
        meta("Imperial Navy Small Armor Repairer"),
        MetaCategory::Faction
    );
    assert_eq!(
        meta("Corpii C-Type Small Armor Repairer"),
        MetaCategory::Complex
    );
}

#[test]
fn charges_for_a_fit() {
    let Some(catalog) = catalog() else { return };
    let launcher = catalog
        .find("Heavy Missile Launcher II")
        .expect("in the catalog");
    let fit = Fit {
        high: vec![Some(Module {
            type_id: launcher.id,
            state: ModuleState::Active,
            charge: None,
            spool: None,
        })],
        ..Fit::new("test")
    };
    let charges: Vec<String> = charges_for_fit(&catalog, &fit)
        .into_iter()
        .map(|t| catalog.name(t))
        .collect();
    assert!(
        charges.contains(&"Scourge Heavy Missile".to_owned()),
        "{charges:?}"
    );
    assert!(
        !charges.iter().any(|c| c.contains("Light Missile")),
        "{charges:?}"
    );
    assert_eq!(
        charges_for_fit(&catalog, &Fit::new("empty")),
        Vec::<i32>::new()
    );
}

#[test]
fn ships_by_group_with_races_and_omega() {
    let Some(catalog) = catalog() else { return };
    let ship = |name: &str| catalog.find(name).expect("in the catalog");
    let rifter = ship("Rifter");
    assert!(catalog.ships_in(rifter.group_id).contains(&rifter.id));
    assert_eq!(rifter.race, Some(Race::Minmatar));
    assert_eq!(ship("Gila").race, Some(Race::Guristas));
    assert_eq!(ship("Cynabal").race, Some(Race::Angel));
    assert!(!rifter.omega);
    assert!(ship("Paladin").omega);
    assert!(
        catalog
            .ship_groups
            .iter()
            .all(|(_, ships)| !ships.is_empty())
    );
}
