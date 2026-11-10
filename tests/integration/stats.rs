//! Stats pane numbers against the real data. Skipped when no dump is present.

use crate::common::setup;
use efa::{
    data::Catalog,
    engine::{Layer, Stats},
    fit::{DamageProfile, Fit},
};

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
    let mut fit = vexor(catalog);
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
    let stats = engine.compute(&vexor(catalog));
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
    let stats = engine.compute(&vexor(catalog));
    assert!(stats.tank.armor > 0.0, "armor repair: {}", stats.tank.armor);
    assert!(
        stats.sustained.armor < stats.tank.armor,
        "the MWD drains the capacitor, so the repairer cannot run all the time: {} vs {}",
        stats.sustained.armor,
        stats.tank.armor
    );
    assert!(stats.sustained.armor >= 0.0);
}

#[test]
fn launchers_show_how_far_their_missiles_fly() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let (fit, unknown) = efa::fit::Fit::from_eft(
        catalog,
        "[Caracal, Missile Range]


Rapid Light Missile Launcher II, Scourge Light Missile
Rapid Light Missile Launcher II
Core Probe Launcher I, Core Scanner Probe I
",
    )
    .expect("the fit parses");
    assert!(unknown.is_empty(), "{unknown:?}");
    let stats = engine.compute(&fit);
    let range = |i| stats.modules[&(efa::data::Slot::High, i)].optimal;
    // 3750 m/s for 5 s before skills and hull.
    let loaded = range(0).expect("a loaded launcher has a range");
    assert!((18_000.0..120_000.0).contains(&loaded), "{loaded}");
    assert_eq!(range(1), None, "nothing loaded, nothing to fly");
    assert_eq!(range(2), None, "probes are not missiles");
}

#[test]
fn cap_boosters_count_in_the_cap_delta() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let recharge = |charge: &str| {
        let text = format!("[Caracal, Cap Booster]\n\n\nMedium Capacitor Booster II{charge}\n");
        let (fit, unknown) = efa::fit::Fit::from_eft(catalog, &text).expect("the fit parses");
        assert!(unknown.is_empty(), "{unknown:?}");
        engine.compute(&fit).cap_detail.recharge
    };
    // 400 GJ every cycle of a few seconds, averaged.
    let gained = recharge(", Navy Cap Booster 400") - recharge("");
    assert!((10.0..200.0).contains(&gained), "{gained}");
}

#[test]
fn ranges_come_from_the_attribute_each_module_uses() {
    use efa::data::Slot::{High, Mid};
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let (fit, unknown) = efa::fit::Fit::from_eft(
        catalog,
        "[Vexor, Ranges]

Damage Control II

Warp Scrambler II
Cargo Scanner II

Medium Remote Shield Booster II
Medium Energy Neutralizer II
Heavy Energy Nosferatu II
",
    )
    .expect("the fit parses");
    assert!(unknown.is_empty(), "{unknown:?}");
    let stats = engine.compute(&fit);
    let range = |slot, i| {
        let info = &stats.modules[&(slot, i)];
        (info.optimal, info.falloff)
    };
    // Neutralizers, nosferatus and remote reps keep their falloff in falloffEffectiveness.
    for (slot, i) in [(High, 0), (High, 1), (High, 2)] {
        let (optimal, falloff) = range(slot, i);
        assert!(
            optimal.is_some() && falloff.is_some(),
            "{slot:?} {i}: {optimal:?} {falloff:?}"
        );
    }
    // A scrambler's falloff is 0, which pyfa leaves out.
    assert!(matches!(range(Mid, 0), (Some(_), None)));
    // Cargo scanners have their own range attribute.
    assert!(range(Mid, 1).0.is_some());
}
