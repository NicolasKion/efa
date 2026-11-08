//! Optimize Fit Price and the multibuy "Optimize Prices" option against the real data, with a
//! fixed price table. Skipped when no dump is present.

use std::collections::HashMap;

use crate::common::setup;
use efa::{
    data::{Catalog, TypeId},
    fit::Fit,
    port::multibuy,
    prices::optimize,
};

fn id(catalog: &Catalog, name: &str) -> TypeId {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

const FIT: &str = "[Incursus, Cheap]
Damage Control II
Magnetic Field Stabilizer II

1MN Afterburner II

Light Neutron Blaster II, Federation Navy Antimatter Charge S
Light Neutron Blaster II, Federation Navy Antimatter Charge S

Small Hybrid Burst Aerator I

Federation Navy Antimatter Charge S x200
Federation Navy Iron Charge S x100
";

/// Navy charges of one kind are the same item whichever navy makes them; the fixed prices
/// make the Caldari ones cheaper.
fn price_table(catalog: &Catalog) -> HashMap<TypeId, f64> {
    [
        ("Damage Control II", 300_000.0),
        ("1MN Afterburner II", 400_000.0),
        ("Light Neutron Blaster II", 500_000.0),
        ("Small Hybrid Burst Aerator I", 600_000.0),
        ("Federation Navy Antimatter Charge S", 100.0),
        ("Caldari Navy Antimatter Charge S", 40.0),
        ("Federation Navy Iron Charge S", 90.0),
        ("Magnetic Field Stabilizer II", 700_000.0),
        ("Caldari Navy Iron Charge S", 20.0),
    ]
    .into_iter()
    .filter_map(|(name, price)| Some((catalog.find(name)?.id, price)))
    .collect()
}

#[test]
fn replacements_are_the_same_item() {
    let Some((catalog, _)) = setup() else { return };
    let fed = id(catalog, "Federation Navy Antimatter Charge S");
    let cal = id(catalog, "Caldari Navy Antimatter Charge S");
    assert!(
        catalog.replacements_of(fed).contains(&cal),
        "navy antimatter of two factions are the same item: {:?}",
        catalog.replacements_of(fed)
    );
    assert!(catalog.replacements_of(cal).contains(&fed));
    // Tech II and meta variations differ in their attributes.
    let t2 = id(catalog, "Damage Control II");
    for &other in catalog.replacements_of(t2) {
        assert!(
            !catalog.variations_of(t2).contains(&other),
            "{} is a different damage control",
            catalog.name(other)
        );
    }
}

#[test]
fn optimize_fit_price_swaps_in_cheaper_equivalents() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let (mut fit, _) = Fit::from_eft(catalog, FIT).expect("the fit parses");
    let before = fit.clone();
    let table = price_table(catalog);
    let price = |t: TypeId| table.get(&t).copied();
    let valid = engine.validate(&fit, engine.all5());
    assert!(valid.is_empty(), "{:?}", valid.items);

    let was = optimize::value(&fit, catalog, price);
    let made = optimize::optimize(&mut fit, catalog, price, |_, next| {
        !engine.validate(next, engine.all5()).is_empty()
    });

    let fed = id(catalog, "Federation Navy Antimatter Charge S");
    let cal = id(catalog, "Caldari Navy Antimatter Charge S");
    assert_eq!(made.get(&fed), Some(&cal), "{made:?}");
    for m in fit.modules() {
        assert_ne!(m.charge, Some(fed), "loaded charges are swapped too");
    }
    // Each kind of charge finds its own equivalent; antimatter and iron stay apart.
    let cal_iron = id(catalog, "Caldari Navy Iron Charge S");
    assert_eq!(fit.cargo, vec![(cal, 200), (cal_iron, 100)]);
    // Items without a cheaper equivalent stay.
    let dc = id(catalog, "Damage Control II");
    assert!(fit.modules().any(|m| m.type_id == dc));
    assert!(optimize::value(&fit, catalog, price) < was);
    assert!(
        engine.validate(&fit, engine.all5()).is_empty(),
        "the optimized fit is still valid"
    );
    // Slots, states and the rest are as they were.
    assert_eq!(fit.high.len(), before.high.len());
    for (a, b) in fit.modules().zip(before.modules()) {
        assert_eq!(a.state, b.state);
    }
}

#[test]
fn multibuy_optimize_prices_lists_the_cheaper_items() {
    let Some((catalog, _)) = setup() else { return };
    let (fit, _) = Fit::from_eft(catalog, FIT).expect("the fit parses");
    let table = price_table(catalog);
    let price = |t: TypeId| table.get(&t).copied();
    let options = multibuy::Options {
        optimize_prices: true,
        ..multibuy::Options::default()
    };
    let plain = multibuy::export(&fit, catalog, &options);
    assert!(
        plain.contains("Federation Navy Antimatter Charge S"),
        "{plain}"
    );
    let cheap = multibuy::export_priced(&fit, catalog, &options, price);
    assert!(
        !cheap.contains("Federation Navy Antimatter Charge S"),
        "{cheap}"
    );
    assert!(
        cheap.contains("Caldari Navy Antimatter Charge S x"),
        "{cheap}"
    );
    assert_eq!(cheap.lines().next(), Some("Incursus"));
}
