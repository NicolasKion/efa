//! Stats pane numbers against the real data. Skipped when no dump is present.

use std::path::Path;

use gyfa::{
    data::Catalog,
    engine::{Engine, Layer, Stats},
    fit::{DamageProfile, Fit},
};

const DATA: &str = "data/eve";

/// Shield and armor tanked, with an armor repairer that cannot run forever.
const VEXOR: &str = "[Vexor, Tank Vexor]
Medium Armor Repairer II
Multispectrum Energized Membrane II
Damage Control II
Reactive Armor Hardener

50MN Microwarpdrive II
Medium Shield Extender II
Medium Shield Extender II

Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M

Medium Auxiliary Nano Pump I
Medium Capacitor Control Circuit I
Medium Capacitor Control Circuit I

Garde II x2
";

fn setup() -> Option<(Catalog, Engine)> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    let catalog = Catalog::load(Path::new(DATA)).expect("the dump loads");
    let engine = Engine::load(
        Path::new(DATA),
        Path::new("data/cache"),
        catalog.skills.clone(),
    )
    .expect("the engine loads");
    Some((catalog, engine))
}

fn vexor(catalog: &Catalog) -> Fit {
    let (mut fit, unknown) = Fit::from_eft(catalog, VEXOR).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    for stack in &mut fit.drones {
        stack.active = stack.count;
    }
    fit
}

fn layers(stats: &Stats) -> [(&'static str, Layer); 3] {
    [
        ("shield", stats.shield),
        ("armor", stats.armor),
        ("hull", stats.hull),
    ]
}

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= want.abs() * 1e-9 + 1e-9,
        "{what}: got {got}, want {want}"
    );
}

/// EHP is HP divided by the share of damage that gets through, once. Applying the resist
/// multiplier a second time once overstated it by about 2.5 times.
#[test]
fn ehp_applies_resists_once() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut fit = vexor(&catalog);
    let stats = engine.compute(&fit);
    let mut total = (0.0, 0.0);
    for (name, layer) in layers(&stats) {
        let through = 1.0 - layer.resists.iter().sum::<f64>() / 4.0;
        let ehp = layer.ehp.expect("every layer has EHP");
        assert_close(ehp, layer.hp / through, name);
        total = (total.0 + ehp, total.1 + layer.hp / through);
    }
    assert_close(total.0, total.1, "ship EHP");

    fit.damage_profile = DamageProfile::Pattern {
        name: "EM".into(),
        amounts: [1.0, 0.0, 0.0, 0.0],
    };
    let stats = engine.compute(&fit);
    for (name, layer) in layers(&stats) {
        let ehp = layer.ehp.expect("every layer has EHP");
        assert_close(ehp, layer.hp / (1.0 - layer.resists[0]), name);
    }
}

#[test]
fn sentries_count_as_drone_damage() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let stats = engine.compute(&vexor(&catalog));
    assert!(stats.drone_dps > 0.0, "sentry DPS: {}", stats.drone_dps);
    assert_close(
        stats.weapon_dps + stats.drone_dps,
        stats.dps.total(),
        "weapon plus drone DPS",
    );
}

#[test]
fn sustained_repair_never_beats_reinforced() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let stats = engine.compute(&vexor(&catalog));
    assert!(stats.tank.armor > 0.0, "armor repair: {}", stats.tank.armor);
    assert!(
        stats.sustained.armor < stats.tank.armor,
        "the MWD drains the capacitor, so the repairer cannot run all the time: {} vs {}",
        stats.sustained.armor,
        stats.tank.armor
    );
    assert!(stats.sustained.armor >= 0.0);
}
