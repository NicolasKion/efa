//! The market and fittings browsers against the real data: jargon search, meta buttons, charges
//! for a fit, ship groups, races and the Omega marker. Skipped when no dump is present.

use std::path::Path;

use efa::{
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
    efa::market::search(catalog, query, 300)
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
    assert_eq!(meta("Small Armor Repairer II"), MetaCategory::Tech2);
    assert_eq!(
        meta("Imperial Navy Small Armor Repairer"),
        MetaCategory::Faction
    );
    assert_eq!(
        meta("Corpii C-Type Small Armor Repairer"),
        MetaCategory::Deadspace
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
            mutation: None,
            rah: None,
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

#[test]
fn off_market_boosters_get_a_group_of_their_own() {
    let Some(catalog) = catalog() else { return };
    // AIR boosters are unpublished in the SDE and sold nowhere; pyfa publishes them.
    let booster = catalog
        .find("AIR Repairer Booster III")
        .expect("in the catalog");
    assert_eq!(
        booster.market_group,
        Some(efa::data::MARKET_SPECIAL_BOOSTERS)
    );
    let group = &catalog.market_groups[&efa::data::MARKET_SPECIAL_BOOSTERS];
    assert!(group.types.contains(&booster.id));
    assert!(
        efa::market::search(&catalog, "air repairer booster", 50).contains(&booster.id),
        "found by search"
    );
    // Accelerators stay out of it.
    let accelerator = catalog
        .find("Standard Cerebral Accelerator")
        .expect("in the catalog");
    assert!(!group.types.contains(&accelerator.id));
    // Boosters pyfa hides, or that expired, stay out too.
    let hidden = catalog
        .find("Limited Synth Blue Pill Booster")
        .expect("in the catalog");
    assert!(!group.types.contains(&hidden.id));
}

#[test]
fn search_stops_at_the_limit_in_market_order() {
    let Some(catalog) = catalog() else { return };
    let all = efa::market::search(&catalog, "launcher", 1000);
    assert!(all.len() > 20, "{}", all.len());
    assert_eq!(efa::market::search(&catalog, "launcher", 20), all[..20]);
    let mut sorted = all.clone();
    efa::market::sort_items(&catalog.types, &mut sorted);
    assert_eq!(sorted, all, "results come in market order");
}

#[test]
fn the_charge_index_finds_what_a_scan_finds() {
    let Some(catalog) = catalog() else { return };
    for name in [
        "Heavy Missile Launcher II",
        "Heavy Neutron Blaster II",
        "Warp Disruptor II",
    ] {
        let module = catalog.find(name).expect("in the catalog");
        let fit = Fit {
            high: vec![Some(Module::new(&catalog, module.id))],
            ..Fit::new("test")
        };
        let mut scanned: Vec<i32> = catalog
            .types
            .values()
            .filter(|c| c.published && c.market_group.is_some() && c.fits_in(module))
            .map(|c| c.id)
            .collect();
        efa::market::sort_items(&catalog.types, &mut scanned);
        assert_eq!(charges_for_fit(&catalog, &fit), scanned, "{name}");
    }
}

#[test]
fn tactical_destroyers_have_their_modes() {
    let Some(catalog) = catalog() else { return };
    let svipul = catalog.find("Svipul").expect("in the catalog").id;
    let modes: Vec<String> = catalog
        .modes_for(svipul)
        .iter()
        .map(|&m| catalog.name(m))
        .collect();
    assert_eq!(modes.len(), 3, "{modes:?}");
    assert!(modes.iter().all(|m| m.starts_with("Svipul ")), "{modes:?}");
    let rifter = catalog.find("Rifter").expect("in the catalog").id;
    assert_eq!(catalog.modes_for(rifter), [] as [i32; 0]);
}
