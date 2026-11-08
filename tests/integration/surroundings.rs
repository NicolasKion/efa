//! Projected items, environmental effects, fleet boosts and fighters change the stats of the fit
//! they act on. Skipped when no dump is present.

use std::path::Path;

use efa::{
    data::Catalog,
    engine::{CalcOptions, Engine, FitItem, SourceKind, Stats},
    fit::{Fit, ModuleState},
};

const DATA: &str = "data/eve";

/// shieldCapacity.
const SHIELD_CAPACITY: i32 = 263;

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

fn id(catalog: &Catalog, name: &str) -> i32 {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

fn env(catalog: &Catalog, name: &str) -> i32 {
    catalog
        .projection
        .env_types
        .iter()
        .find_map(|(&id, t)| (t.name == name).then_some(id))
        .unwrap_or_else(|| panic!("{name} is an environmental effect"))
}

fn eft(catalog: &Catalog, text: &str, uid: u64) -> Fit {
    let (mut fit, unknown) = Fit::from_eft(catalog, text).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    fit.uid = uid;
    fit
}

fn rifter(catalog: &Catalog) -> Fit {
    eft(
        catalog,
        "[Rifter, Target]\nDamage Control II\n\n1MN Afterburner II\nMedium Shield Extender II\n",
        1,
    )
}

fn speed(stats: &Stats) -> f64 {
    stats.speed.expect("the ship has a speed")
}

#[test]
fn command_bursts_boost_fleet_members() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut target = rifter(&catalog);
    let booster = eft(
        &catalog,
        "[Vulture, Booster]\n\n\nShield Command Burst II, Shield Extension Charge\n",
        2,
    );
    let fits = vec![target.clone(), booster];
    let alone = engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default());
    assert!(target.add_command_fit(2));
    let with_boost = engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default());
    assert!(
        with_boost.shield.hp > alone.shield.hp * 1.05,
        "shield {} -> {}",
        alone.shield.hp,
        with_boost.shield.hp
    );

    // A switched off booster does nothing.
    target.command[0].active = false;
    let off = engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default());
    assert!((off.shield.hp - alone.shield.hp).abs() < 1e-6);
}

#[test]
fn projected_webs_slow_the_target() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut target = rifter(&catalog);
    let base = speed(&engine.compute(&target));

    target
        .projected
        .add_module(id(&catalog, "Stasis Webifier II"), true);
    let webbed = speed(&engine.compute(&target));
    assert!(webbed < base * 0.6, "speed {base} -> {webbed}");

    // Far out of range the web does nothing.
    target.projected.modules[0].range = Some(200_000.0);
    let far = speed(&engine.compute(&target));
    assert!((far - base).abs() < 1e-6, "speed {base} -> {far}");

    target.projected.modules[0].range = None;
    target.projected.modules[0].state = ModuleState::Offline;
    let offline = speed(&engine.compute(&target));
    assert!((offline - base).abs() < 1e-6);
}

#[test]
fn projected_fits_apply_once_per_copy() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut target = rifter(&catalog);
    let webber = eft(&catalog, "[Rifter, Webber]\n\nStasis Webifier I\n\n", 5);
    let fits = vec![target.clone(), webber];
    let base = speed(&engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default()));
    target.projected.add_fit(5);
    let once = speed(&engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default()));
    target.projected.add_fit(5);
    let twice = speed(&engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default()));
    assert!(once < base, "speed {base} -> {once}");
    assert!(twice < once, "speed {once} -> {twice}");
}

#[test]
fn projected_drones_apply_while_out() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut target = rifter(&catalog);
    let base = speed(&engine.compute(&target));
    target
        .projected
        .add_drones(id(&catalog, "Berserker SW-900"), 2);
    let webbed = speed(&engine.compute(&target));
    assert!(webbed < base, "speed {base} -> {webbed}");
    target.projected.drones[0].active = false;
    let recalled = speed(&engine.compute(&target));
    assert!((recalled - base).abs() < 1e-6);
}

#[test]
fn environment_effects_change_the_fit() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut target = rifter(&catalog);
    let base = engine.compute(&target);

    // A class 6 pulsar adds a lot of shield.
    let pulsar = env(&catalog, "Class 6 Pulsar Effects");
    target.projected.add_env(pulsar);
    let in_pulsar = engine.compute(&target);
    assert!(
        in_pulsar.shield.hp > base.shield.hp * 1.5,
        "shield {} -> {}",
        base.shield.hp,
        in_pulsar.shield.hp
    );
    target.projected.env[0].active = false;
    let off = engine.compute(&target);
    assert!((off.shield.hp - base.shield.hp).abs() < 1e-6);

    // Abyssal weather projects onto ships rather than acting on the system.
    let mut target = rifter(&catalog);
    let weather = env(&catalog, "Exotic Particle Storm 3");
    target.projected.add_env(weather);
    let stormed = engine.compute(&target);
    let fingerprint = |s: &Stats| {
        format!(
            "{:?} {:?} {:?} {:?} {:?} {:?}",
            s.shield, s.armor, s.hull, s.cap_amount, s.scan_res, s.sig_radius
        )
    };
    assert_ne!(
        fingerprint(&stormed),
        fingerprint(&base),
        "the storm changed nothing"
    );
}

#[test]
fn fighters_launch_and_their_abilities_switch() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut carrier = eft(&catalog, "[Thanatos, Carrier]\n\nFirbolg I x9\n", 3);
    assert!(carrier.drones.is_empty(), "fighters are not drones");
    assert_eq!(carrier.fighters.len(), 1);
    let stats = engine.compute(&carrier);
    let info = &stats.fighters[0];
    assert!(stats.drone_dps > 0.0, "the squadron deals damage");
    assert!(
        info.count.is_some_and(|(n, max)| n == max && max > 0),
        "count {:?}",
        info.count
    );
    let attack = info
        .abilities
        .iter()
        .find(|a| a.active)
        .expect("the attack starts switched on")
        .id;
    assert_eq!(stats.fighter_classes.light.used, 1);

    carrier.fighters[0].set_ability(attack, false);
    let quiet = engine.compute(&carrier);
    assert!(quiet.drone_dps < stats.drone_dps);

    carrier.fighters[0].active = false;
    let docked = engine.compute(&carrier);
    assert!(docked.drone_dps.abs() < 1e-9);
    assert_eq!(docked.fighter_classes.light.used, 0);
}

#[test]
fn projected_fighters_report_their_abilities() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut target = rifter(&catalog);
    let base = speed(&engine.compute(&target));
    // A support fighter with a web: no attack, so pyfa switches everything else on.
    target.projected.add_fighter(id(&catalog, "Dromi I"));
    let stats = engine.compute(&target);
    let info = &stats.projected_fighters[0];
    assert!(info.abilities.iter().any(|a| a.active));
    assert!(speed(&stats) < base, "the squadron webs the target");
}

/// A gunboat boosted by a fleet and sitting in a system effect, with the booster among the saved
/// fits, and those saved fits.
fn boosted_gunboat(catalog: &Catalog, effect: &str) -> (Fit, Vec<Fit>) {
    let mut target = eft(
        catalog,
        "[Rifter, Guns]\nGyrostabilizer II\n\n1MN Afterburner II\nMedium Shield Extender II\n\n\
         200mm AutoCannon II, EMP S\n200mm AutoCannon II, EMP S\n",
        1,
    );
    for m in [&mut target.high, &mut target.mid, &mut target.low]
        .into_iter()
        .flatten()
        .flatten()
    {
        m.state = ModuleState::Active;
    }
    let booster = eft(
        catalog,
        "[Vulture, Booster]\n\n\nShield Command Burst II, Shield Extension Charge\n",
        2,
    );
    assert!(target.add_command_fit(2));
    target.projected.add_env(env(catalog, effect));
    let fits = vec![target.clone(), booster];
    (target, fits)
}

#[test]
fn graphs_see_boosts_and_system_effects_like_the_stats_pane() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let (target, fits) = boosted_gunboat(&catalog, "Class 6 Magnetar Effects");
    let stats = engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default());
    let mut calm = target.clone();
    calm.projected.env.clear();
    calm.command.clear();
    let plain = engine.compute(&calm);
    assert!(stats.shield.hp > plain.shield.hp * 1.05, "the burst boosts");
    assert!(
        stats.dps.total() > plain.dps.total() * 1.05,
        "the magnetar boosts"
    );

    let snap = engine.graph_snapshot(&target, &fits, engine.all5(), &catalog);
    assert!((snap.ship.hp[0] - stats.shield.hp).abs() < 1e-6);
    assert!((snap.ship.max_velocity - speed(&stats)).abs() < 1e-6);
    assert!((snap.ship.dps - stats.dps.total()).abs() < 1e-6);
    let weapons: f64 = snap.weapons.iter().map(|w| w.dps.iter().sum::<f64>()).sum();
    assert!(
        (weapons - stats.dps.total()).abs() < 1e-6,
        "per weapon {weapons}, ship {}",
        stats.dps.total()
    );
}

#[test]
fn item_stats_see_boosts_and_system_effects_like_the_stats_pane() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let (target, fits) = boosted_gunboat(&catalog, "Class 6 Pulsar Effects");
    let stats = engine.compute_among(&target, &fits, engine.all5(), CalcOptions::default());
    let ship = engine
        .inspect_among(&target, &fits, engine.all5(), FitItem::Ship)
        .expect("the ship");
    let shield = ship.attr(SHIELD_CAPACITY).expect("a shield").modified;
    assert!(
        (shield - stats.shield.hp).abs() < 1e-6,
        "item stats {shield}, stats pane {}",
        stats.shield.hp
    );
    let from = |projected: bool| {
        ship.modifiers
            .iter()
            .filter(|(attr, _)| *attr == SHIELD_CAPACITY)
            .flat_map(|(_, mods)| mods)
            .any(|m| m.source.projected == projected && m.source.kind != SourceKind::Skill)
    };
    assert!(from(true), "the burst shows as a projected source");
}
