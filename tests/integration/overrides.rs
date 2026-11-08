//! Attribute overrides against the real data: an overridden base value changes the stats while
//! overrides are on and not while they are off. Skipped when no dump is present.

use efa::{
    data::{Catalog, Slot},
    engine::{Engine, FitItem},
    fit::Fit,
    overrides::Overrides,
};

const SHIELD_CAPACITY: i32 = 263;
const DAMAGE_MULTIPLIER: i32 = 64;

const VEXOR: &str = "[Vexor, Override Vexor]
Damage Control II

Medium Shield Extender II

Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M
";

/// The shared catalog with an engine of the test's own, since overrides change the engine.
fn setup() -> Option<(&'static Catalog, Engine)> {
    let catalog = crate::common::catalog()?;
    Some((catalog, crate::common::own_engine(catalog)))
}

fn id(catalog: &Catalog, name: &str) -> i32 {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

#[test]
fn overrides_change_stats_only_while_on() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let (fit, unknown) = Fit::from_eft(catalog, VEXOR).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    let vexor = id(catalog, "Vexor");
    let blaster = id(catalog, "Heavy Neutron Blaster II");

    let plain = engine.compute(&fit);
    let base_shield = engine
        .inspect(&fit, engine.all5(), FitItem::Ship)
        .and_then(|i| i.attr(SHIELD_CAPACITY).map(|a| a.base))
        .expect("the Vexor has shield");
    let base_mult = engine
        .inspect(&fit, engine.all5(), FitItem::Module(Slot::High, 0))
        .and_then(|i| i.attr(DAMAGE_MULTIPLIER).map(|a| a.base))
        .expect("the blaster has a damage multiplier");

    let mut overrides = Overrides::default();
    overrides.set(vexor, SHIELD_CAPACITY, base_shield * 10.0);
    overrides.set(blaster, DAMAGE_MULTIPLIER, base_mult * 2.0);

    // Off: nothing changes.
    engine
        .apply_overrides(&overrides.active())
        .expect("the data loads");
    let off = engine.compute(&fit);
    assert!((off.shield.hp - plain.shield.hp).abs() < 1e-6);
    assert!((off.weapon_dps - plain.weapon_dps).abs() < 1e-6);

    // On: the base values are the overrides, and bonuses apply on top of them.
    overrides.enabled = true;
    engine
        .apply_overrides(&overrides.active())
        .expect("the overrides apply");
    let on = engine.compute(&fit);
    assert!(
        on.shield.hp > plain.shield.hp * 3.0,
        "shield {} vs {}",
        on.shield.hp,
        plain.shield.hp
    );
    assert!(
        (on.weapon_dps / plain.weapon_dps - 2.0).abs() < 1e-6,
        "dps {} vs {}",
        on.weapon_dps,
        plain.weapon_dps
    );
    let ship = engine
        .inspect(&fit, engine.all5(), FitItem::Ship)
        .expect("the ship is there");
    let shield = ship.attr(SHIELD_CAPACITY).expect("the Vexor has shield");
    assert!((shield.base - base_shield * 10.0).abs() < 1e-6);
    assert!(shield.modified > shield.base, "skills still apply");

    // Off again: back to the plain numbers.
    overrides.enabled = false;
    engine
        .apply_overrides(&overrides.active())
        .expect("the data loads");
    let back = engine.compute(&fit);
    assert!((back.shield.hp - plain.shield.hp).abs() < 1e-6);
    assert!((back.weapon_dps - plain.weapon_dps).abs() < 1e-6);
}
