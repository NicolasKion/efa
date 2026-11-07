//! User damage patterns and target profiles against the real data. Skipped when no dump is
//! present.

use std::path::Path;

use efa::{
    damage_pattern::{self, DamagePattern},
    data::Catalog,
    engine::Engine,
    fit::Fit,
    target_profile::{self, TargetProfile},
};

const DATA: &str = "data/eve";

const VEXOR: &str = "[Vexor, Profiles Vexor]
Medium Armor Repairer II
Multispectrum Energized Membrane II
Damage Control II

Medium Shield Extender II

Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M

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

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= want.abs() * 1e-9 + 1e-9,
        "{what}: got {got}, want {want}"
    );
}

/// A user pattern reaches refine's EHP, which agrees with pyfa's `effectivify` on every layer.
#[test]
fn ehp_under_a_user_pattern_matches_pyfa() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut fit = vexor(&catalog);
    let pattern = DamagePattern::new("[Mine]Kinetic heavy", [0.0, 10.0, 30.0, 5.0]);
    fit.damage_profile = pattern.to_profile();
    let stats = engine.compute(&fit);
    for (name, layer) in [
        ("shield", stats.shield),
        ("armor", stats.armor),
        ("hull", stats.hull),
    ] {
        let want = damage_pattern::effective_hp(layer.hp, layer.resists, pattern.amounts);
        assert_close(layer.ehp.expect("the layer has EHP"), want, name);
    }
}

/// The firepower view's numbers against a target profile: each damage type cut by its resist,
/// weapons and drones alike.
#[test]
fn applied_dps_against_a_profile() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = vexor(&catalog);
    let stats = engine.compute(&fit);
    let profile = target_profile::BUILTINS
        .iter()
        .find(|p| p.name == "[T2 Resist]Gallente (Armor)")
        .expect("a pyfa builtin");
    let raw = stats.dps;
    let applied = profile.apply(raw);
    assert!(
        raw.thermal > 0.0 && raw.kinetic > 0.0,
        "Void and Garde deal damage"
    );
    let want =
        raw.em * 0.5 + raw.thermal * 0.325 + raw.kinetic * (1.0 - 0.8375) + raw.explosive * 0.9;
    assert_close(applied.total(), want, "applied DPS");
    assert!(applied.total() < raw.total());
    assert_close(
        target_profile::applied(raw, None).total(),
        raw.total(),
        "no profile",
    );
    assert_close(
        TargetProfile::ideal().apply(raw).total(),
        raw.total(),
        "ideal target",
    );
}
