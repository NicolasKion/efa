//! Cargo against the real data: which items the catalog offers for cargo, and moving items
//! between the hold and the fit. Skipped when no dump is present.

use std::path::Path;

use gyfa::{
    data::{Catalog, Kind, Slot, TypeId},
    fit::{
        Fit, Module, ModuleState,
        cargo::{Transfer, charges_per_module},
    },
};

const DATA: &str = "data/eve";

fn catalog() -> Option<Catalog> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    Some(Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads"))
}

fn id(catalog: &Catalog, name: &str) -> TypeId {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} is in the catalog"))
        .id
}

fn count(fit: &Fit, type_id: TypeId) -> u32 {
    fit.cargo
        .iter()
        .find(|&&(t, _)| t == type_id)
        .map_or(0, |&(_, n)| n)
}

/// A fit with one high slot holding `module` loaded with `charge`.
fn armed(module: TypeId, charge: TypeId) -> Fit {
    let mut fit = Fit::new("test");
    fit.high.push(Some(Module {
        type_id: module,
        state: ModuleState::Active,
        charge: Some(charge),
        spool: None,
        mutation: None,
        rah: None,
    }));
    fit
}

#[test]
fn cargo_only_items_are_listed_like_pyfa() {
    let Some(catalog) = catalog() else { return };
    for name in [
        "Mobile Depot",
        "Calm Exotic Filament",
        "Giant Secure Container",
        "Strontium Clathrates",
        "Helium Fuel Block",
    ] {
        let t = catalog.find(name).expect("loaded");
        assert_eq!(t.kind, Kind::Cargo, "{name}");
    }
    let roots: Vec<&str> = catalog
        .market_roots
        .iter()
        .map(|r| catalog.market_groups[r].name.as_str())
        .collect();
    assert!(roots.contains(&"Deployable Structures"), "{roots:?}");
    assert!(roots.contains(&"Filaments"), "{roots:?}");
    assert!(!roots.contains(&"Trade Goods"), "{roots:?}");

    let found = |query: &str| {
        catalog
            .search(query, 50)
            .into_iter()
            .map(|t| catalog.name(t))
            .collect::<Vec<_>>()
    };
    assert!(found("strontium").contains(&"Strontium Clathrates".to_owned()));
    assert!(found("mobile depot").contains(&"Mobile Depot".to_owned()));
    assert_eq!(found("tritanium"), Vec::<String>::new());

    let mut fit = Fit::new("test");
    assert!(fit.add(&catalog, id(&catalog, "Mobile Depot")));
    assert_eq!(fit.cargo.len(), 1);
}

#[test]
fn charges_fill_by_capacity() {
    let Some(catalog) = catalog() else { return };
    let void = id(&catalog, "Void M");
    assert_eq!(
        charges_per_module(&catalog, id(&catalog, "Heavy Neutron Blaster II"), void),
        80
    );
    assert_eq!(
        charges_per_module(&catalog, id(&catalog, "Damage Control II"), void),
        0
    );
}

#[test]
fn modules_go_to_cargo_with_their_charges() {
    let Some(catalog) = catalog() else { return };
    let (neutron, void) = (
        id(&catalog, "Heavy Neutron Blaster II"),
        id(&catalog, "Void M"),
    );

    let mut fit = armed(neutron, void);
    assert!(fit.module_to_cargo(&catalog, (Slot::High, 0), None, Transfer::Duplicate));
    assert_eq!(fit.high.len(), 1);
    assert_eq!((count(&fit, neutron), count(&fit, void)), (1, 80));

    let mut fit = armed(neutron, void);
    assert!(fit.module_to_cargo(&catalog, (Slot::High, 0), None, Transfer::Move));
    assert!(fit.high.is_empty());
    assert_eq!((count(&fit, neutron), count(&fit, void)), (1, 80));
}

#[test]
fn a_module_dropped_on_a_cargo_module_trades_places() {
    let Some(catalog) = catalog() else { return };
    let (neutron, ion, void) = (
        id(&catalog, "Heavy Neutron Blaster II"),
        id(&catalog, "Heavy Ion Blaster II"),
        id(&catalog, "Void M"),
    );
    let mut fit = armed(neutron, void);
    fit.add_cargo(ion, 1);
    fit.add_cargo(void, 100);
    assert!(fit.module_to_cargo(&catalog, (Slot::High, 0), Some(ion), Transfer::Move));
    let m = fit.high[0].as_ref().expect("still fitted");
    assert_eq!((m.type_id, m.charge), (ion, Some(void)));
    // The ion blaster holds 120 against the neutron's 80; the rest comes out of cargo.
    assert_eq!(
        (count(&fit, ion), count(&fit, neutron), count(&fit, void)),
        (0, 1, 60)
    );

    // A cargo module for another rack does not swap; the dragged one just goes into cargo.
    let dc = id(&catalog, "Damage Control II");
    let mut fit = armed(neutron, void);
    fit.add_cargo(dc, 1);
    assert!(fit.module_to_cargo(&catalog, (Slot::High, 0), Some(dc), Transfer::Move));
    assert!(fit.high.is_empty());
    assert_eq!((count(&fit, dc), count(&fit, neutron)), (1, 1));
}

#[test]
fn cargo_charges_load_into_modules() {
    let Some(catalog) = catalog() else { return };
    let (neutron, void, null) = (
        id(&catalog, "Heavy Neutron Blaster II"),
        id(&catalog, "Void M"),
        id(&catalog, "Null M"),
    );
    let mut fit = armed(neutron, void);
    fit.add_cargo(null, 100);
    assert!(fit.cargo_to_module(&catalog, null, (Slot::High, 0), Transfer::Move));
    assert_eq!(fit.high[0].as_ref().and_then(|m| m.charge), Some(null));
    assert_eq!((count(&fit, null), count(&fit, void)), (20, 80));
    // Loading the same charge again changes nothing.
    assert!(!fit.cargo_to_module(&catalog, null, (Slot::High, 0), Transfer::Move));
}

#[test]
fn cargo_modules_replace_fitted_ones_of_their_rack() {
    let Some(catalog) = catalog() else { return };
    let (neutron, electron, void, dc) = (
        id(&catalog, "Heavy Neutron Blaster II"),
        id(&catalog, "Heavy Electron Blaster II"),
        id(&catalog, "Void M"),
        id(&catalog, "Damage Control II"),
    );
    let mut fit = armed(neutron, void);
    fit.add_cargo(electron, 2);
    fit.add_cargo(dc, 1);
    assert!(!fit.cargo_to_module(&catalog, dc, (Slot::High, 0), Transfer::Move));
    assert!(fit.cargo_to_module(&catalog, electron, (Slot::High, 0), Transfer::Duplicate));
    let m = fit.high[0].as_ref().expect("fitted");
    assert_eq!((m.type_id, m.charge), (electron, Some(void)));
    assert_eq!((count(&fit, electron), count(&fit, neutron)), (2, 1));

    // Onto an empty position of the right rack, the module just moves in.
    assert!(fit.cargo_to_module(&catalog, dc, (Slot::Low, 0), Transfer::Move));
    assert_eq!(fit.low[0].as_ref().map(|m| m.type_id), Some(dc));
    assert_eq!(count(&fit, dc), 0);
}
