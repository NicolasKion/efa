//! The data pack against the dump it is built from: the catalog, the item data and the engine's
//! numbers come out the same, overrides included. Skipped when no dump is present.

use std::{path::Path, sync::Arc, time::Instant};

use efa::{
    engine::{Engine, EngineSource, FitItem, PackedEngine, Stats},
    fit::Fit,
    item_data::ItemData,
    overrides::Overrides,
    pack::Pack,
};

use crate::common::{DATA, own_engine};

const SHIELD_CAPACITY: i32 = 263;

const VEXOR: &str = "[Vexor, Pack Vexor]
Medium Armor Repairer II
Multispectrum Energized Membrane II
Damage Control II

50MN Microwarpdrive II
Medium Shield Extender II
Medium Shield Extender II

Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M

Medium Auxiliary Nano Pump I
Medium Capacitor Control Circuit I

Hammerhead II x5
";

/// The numbers the stats pane leads with.
fn numbers(stats: &Stats) -> Vec<(&'static str, Option<f64>)> {
    vec![
        ("weapon dps", Some(stats.weapon_dps)),
        ("drone dps", Some(stats.drone_dps)),
        ("volley", Some(stats.volley.total())),
        ("cpu", Some(stats.cpu.used)),
        ("powergrid", Some(stats.powergrid.used)),
        ("shield", Some(stats.shield.hp)),
        ("shield ehp", stats.shield.ehp),
        ("armor", Some(stats.armor.hp)),
        ("armor ehp", stats.armor.ehp),
        ("hull", Some(stats.hull.hp)),
        ("cap", stats.cap_amount),
        ("cap balance", stats.cap_balance),
    ]
}

fn assert_same(from_pack: &Stats, from_dump: &Stats) {
    for ((what, got), (_, want)) in numbers(from_pack).into_iter().zip(numbers(from_dump)) {
        assert_eq!(got, want, "{what}");
    }
}

#[test]
fn a_pack_gives_what_the_dump_gives() {
    let dir = Path::new(DATA);
    if !dir.join("phobos/metadata.json").exists() || !dir.join("VERSION").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return;
    }
    let bytes = Pack::build(dir).expect("the pack builds");

    let start = Instant::now();
    let pack = Pack::read(&bytes).expect("the pack reads");
    let catalog = pack.catalog().expect("the catalog loads from the pack");
    let engine = Engine::from_source(pack.engine(), catalog.skills.clone())
        .expect("the engine loads from the pack");
    eprintln!("loaded from the pack in {} ms", start.elapsed().as_millis());

    let start = Instant::now();
    let dump_catalog = crate::common::catalog().expect("the dump loads");
    // Its own, since the overrides below change it.
    let dump_engine = own_engine(dump_catalog);
    eprintln!("loaded from the dump in {} ms", start.elapsed().as_millis());

    assert_eq!(
        pack.version(),
        std::fs::read_to_string(dir.join("VERSION"))
            .expect("VERSION")
            .trim()
    );
    assert_eq!(catalog.types.len(), dump_catalog.types.len());
    assert_eq!(
        catalog.to_bytes().expect("serializes"),
        dump_catalog.to_bytes().expect("serializes"),
        "the catalog survives the pack unchanged"
    );

    let wanted = dump_catalog.types.keys().copied().collect();
    let items = pack.item_data().expect("the item data loads from the pack");
    let dump_items = ItemData::load(dir, &wanted).expect("the dump's item data loads");
    assert_eq!(
        items.to_bytes().expect("serializes"),
        dump_items.to_bytes().expect("serializes")
    );
    let frigate = catalog
        .find("Minmatar Frigate")
        .expect("the skill exists")
        .id;
    assert!(
        !catalog.required_for(frigate).is_empty(),
        "the pack's catalog links skills to what needs them"
    );

    let (mut fit, unknown) = Fit::from_eft(&catalog, VEXOR).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    for stack in &mut fit.drones {
        stack.active = stack.count;
    }
    let plain = engine.compute(&fit);
    assert!(plain.weapon_dps > 0.0 && plain.drone_dps > 0.0);
    assert_same(&plain, &dump_engine.compute(&fit));

    // Overrides rebuild the engine data from the pack's adapted data.
    let vexor = catalog.find("Vexor").expect("the Vexor").id;
    let base = engine
        .inspect(&fit, engine.all5(), FitItem::Ship)
        .and_then(|i| i.attr(SHIELD_CAPACITY).map(|a| a.base))
        .expect("the Vexor has shield");
    let mut overrides = Overrides {
        enabled: true,
        ..Overrides::default()
    };
    overrides.set(vexor, SHIELD_CAPACITY, base * 10.0);
    engine
        .apply_overrides(&overrides.active())
        .expect("overrides apply on the pack");
    dump_engine
        .apply_overrides(&overrides.active())
        .expect("overrides apply on the dump");
    let overridden = engine.compute(&fit);
    assert!(overridden.shield.hp > plain.shield.hp * 2.0);
    assert_same(&overridden, &dump_engine.compute(&fit));

    // A pack refine would not trust is refused outright, not regenerated from nothing.
    let EngineSource::Packed(packed) = pack.engine() else {
        panic!("the pack's engine data is packed");
    };
    let stale = PackedEngine {
        fingerprint: format!("{}-old", packed.fingerprint),
        ..(*packed).clone()
    };
    let error = Engine::from_source(EngineSource::Packed(Arc::new(stale)), Vec::new())
        .err()
        .expect("a stale pack is refused");
    assert!(error.contains("Rebuild the pack"), "{error}");
}

/// The pack build.rs embedded, when there is one, is one this build can run on.
#[test]
fn the_embedded_pack_loads() {
    let Some(bytes) = Pack::embedded() else {
        eprintln!("no data pack embedded, skipping");
        return;
    };
    let start = Instant::now();
    let pack = Pack::read(bytes).expect("the embedded pack reads");
    let catalog = pack.catalog().expect("its catalog loads");
    let engine = Engine::from_source(pack.engine(), catalog.skills.clone())
        .expect("its engine data fits this build");
    eprintln!(
        "loaded the embedded pack ({}) in {} ms",
        pack.version(),
        start.elapsed().as_millis()
    );
    let (fit, _) = Fit::from_eft(&catalog, VEXOR).expect("the fit parses");
    assert!(engine.compute(&fit).weapon_dps > 0.0);
}

#[test]
fn a_broken_pack_says_so() {
    let error = Pack::read(b"not a pack").expect_err("garbage is no pack");
    assert!(error.contains("data pack"), "{error}");
}
