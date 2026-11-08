//! The graphs' own application math checked against refine's applied damage, on real data.
//! Skipped when no dump is present.

use std::{collections::HashMap, path::Path};

use rc::{
    Coordinates, Value,
    stats::{StatCritOptions, StatDmgItemKinds},
};

use super::*;
use crate::{
    engine::{Damage, build::add_fit},
    graphs::{
        defs::{Graph, Param},
        plot::{self, Options, Request, TargetData},
        snapshot::{EwarKind, WeaponKind},
    },
};

const DATA: &str = "data/eve";

const RUPTURE: &str = "[Rupture, Guns]
Gyrostabilizer II

Stasis Webifier II

425mm AutoCannon II, Barrage M
425mm AutoCannon II, Barrage M
425mm AutoCannon II, Barrage M
425mm AutoCannon II, Barrage M
";

const CARACAL: &str = "[Caracal, Missiles]
Ballistic Control System II

Rapid Light Missile Launcher II, Scourge Light Missile
Rapid Light Missile Launcher II, Scourge Light Missile
Rapid Light Missile Launcher II, Scourge Light Missile
";

const TARGET: &str = "[Vexor, Target]
Damage Control II
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

fn fit(catalog: &Catalog, eft: &str) -> Fit {
    let (mut fit, unknown) = Fit::from_eft(catalog, eft).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    for m in fit.modules_mut() {
        m.state = ModuleState::Active;
    }
    fit
}

/// Weapon DPS refine applies to a standing target `distance` meters away, surface to
/// surface, before resists.
fn refine_applied(engine: &Engine, attacker: &Fit, target: &Fit, distance: f64) -> f64 {
    let mut sol = engine.sol();
    let placed = add_fit(&mut sol, attacker, engine.all5()).expect("attacker places");
    let Placement {
        ship: atk_ship,
        modules,
        ..
    } = placement(attacker, &placed);
    let tgt_ship = add_fit(&mut sol, target, engine.all5())
        .expect("target places")
        .ship;
    let c2c = distance
        + read::attr_or_zero(&mut sol, atk_ship, attr::RADIUS)
        + read::attr_or_zero(&mut sol, tgt_ship, attr::RADIUS);
    sol.get_ship(&tgt_ship)
        .expect("target ship")
        .set_coordinates(Coordinates::new(
            Value::from_f64_clamped(c2c),
            Value::from_f64_clamped(0.0),
            Value::from_f64_clamped(0.0),
        ));
    for m in &modules {
        if let Ok(mut module) = sol.get_module(&m.id) {
            let _ = module.add_proj(&tgt_ship);
        }
    }
    let fit_id = sol.get_ship(&atk_ship).expect("attacker ship").get_fit_id();
    let mut f = sol.get_fit(&fit_id).expect("attacker fit");
    let applied = f
        .stats()
        .get_stat_dmg_applied(
            StatDmgItemKinds::default(),
            StatTimeOptions::default(),
            StatCritOptions::default(),
            &tgt_ship,
        )
        .expect("target takes projections");
    [
        applied.dps.em,
        applied.dps.thermal,
        applied.dps.kinetic,
        applied.dps.explosive,
    ]
    .map(rc::PValue::into_f64)
    .iter()
    .sum()
}

fn graph_dps(engine: &Engine, catalog: &Catalog, attacker: &Fit, target: &Fit) -> plot::Series {
    let src = engine.graph_snapshot(attacker, &[], engine.all5(), catalog);
    let tgt = engine.graph_snapshot(target, &[], engine.all5(), catalog);
    let target = TargetData::profile(
        tgt.ship.max_velocity,
        tgt.ship.sig_radius,
        tgt.ship.radius,
        [0.0; 4],
        None,
    );
    let graph = Graph::DamageStats;
    let req = Request {
        graph,
        x: graph.x_defs(true)[0],
        y: graph.y_defs(true, true)[0],
        range: (0.0, 50.0),
        // A standing target: refine only sees transversal when ships move.
        misc: HashMap::from([(Param::TgtSpeed, Some(0.0))]),
        checkboxes: HashMap::new(),
        options: Options {
            ignore_resists: true,
            apply_projected: false,
            ignore_lock_range: true,
            ..Options::default()
        },
    };
    plot::plot(&req, &src, Some(&target)).expect("a line")
}

fn compare(eft: &str, distances_km: &[f64]) {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let attacker = fit(&catalog, eft);
    let target = fit(&catalog, TARGET);
    let series = graph_dps(&engine, &catalog, &attacker, &target);
    for &km in distances_km {
        let ours = series.at(km).expect("in range");
        let theirs = refine_applied(&engine, &attacker, &target, km * 1000.0);
        assert!(
            (ours - theirs).abs() <= theirs.abs() * 0.02 + 0.5,
            "{km} km: graph {ours}, refine {theirs}"
        );
    }
}

#[test]
fn turret_application_matches_refine() {
    compare(RUPTURE, &[1.0, 5.0, 10.0, 15.0, 20.0, 30.0]);
}

/// Only where both agree a missile surely arrives or surely does not: pyfa adds the ship's
/// radius over missile velocity to the flight time (pyfa issue 2083) and refine does not,
/// so the partial band past the last whole second differs on purpose.
#[test]
fn missile_application_matches_refine() {
    compare(CARACAL, &[5.0, 20.0, 38.0, 49.0]);
}

/// Damage dealt by a time, from the graph's cycle model and from refine's own simulation.
#[test]
fn damage_over_time_matches_refine_simulation() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let attacker = fit(&catalog, RUPTURE);
    let snap = engine.graph_snapshot(&attacker, &[], engine.all5(), &catalog);
    let mut built = engine
        .build(&attacker, engine.all5(), BuildOptions::default())
        .expect("places");
    let (sol, fit_id) = (&mut built.sol, built.fit.fit_id);
    for time in [30.0, 61.0, 125.0] {
        let options = StatTimeOptions::Sim(rc::stats::StatTimeOptionsSim {
            time: Some(rc::stats::StatTime::from_f64_checked(time).expect("valid time")),
            ..Default::default()
        });
        let sim = sol.get_fit(&fit_id).expect("fit").stats().get_stat_dmg(
            StatDmgItemKinds::default(),
            options,
            StatCritOptions::default(),
        );
        let theirs = Damage::from(&sim.dps).as_array().iter().sum::<f64>() * time;
        let ours: f64 = snap
            .weapons
            .iter()
            .flat_map(|w| w.cycle.cycles(time))
            .filter(|c| c.0 <= time)
            .map(|c| c.2.iter().sum::<f64>())
            .sum();
        assert!(
            (ours - theirs).abs() <= theirs * 0.01,
            "{time} s: graph {ours}, refine {theirs}"
        );
    }
}

#[test]
fn fighters_become_weapons() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let carrier = fit(&catalog, "[Thanatos, Fighters]\n\nTemplar II x9\n");
    assert_eq!(carrier.fighters.len(), 1, "the squadron parsed");
    let snap = engine.graph_snapshot(&carrier, &[], engine.all5(), &catalog);
    let fighter = snap
        .weapons
        .iter()
        .find(|w| matches!(w.kind, WeaponKind::Fighter { plain: false, .. }))
        .expect("the attack ability is a weapon");
    assert!(fighter.dps.iter().sum::<f64>() > 0.0);
    assert!(matches!(fighter.kind, WeaponKind::Fighter { er, .. } if er > 0.0));
}

#[test]
fn breacher_pods_cap_by_target_hp() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let kestrel = fit(
        &catalog,
        "[Kestrel, Breacher]\n\nSmall Breacher Pod Launcher, SCARAB Breacher Pod S\n",
    );
    let snap = engine.graph_snapshot(&kestrel, &[], engine.all5(), &catalog);
    let pod = snap
        .weapons
        .iter()
        .find(|w| matches!(w.kind, WeaponKind::Breacher { .. }))
        .expect("the launcher is a breacher weapon");
    let (cap, share) = (pod.dps[0], pod.dps[1]);
    assert!(cap > 0.0 && share > 0.0, "{:?}", pod.dps);
    // Against a small target the share of its HP is what gets through.
    let small_hp = cap / share / 10.0;
    let graph = Graph::DamageStats;
    let req = Request {
        graph,
        x: graph.x_defs(true)[0],
        y: graph.y_defs(true, true)[0],
        range: (0.0, 1.0),
        misc: HashMap::new(),
        checkboxes: HashMap::new(),
        options: Options::default(),
    };
    let tgt = TargetData::profile(0.0, 40.0, 0.0, [0.5; 4], Some(small_hp));
    let series = plot::plot(&req, &snap, Some(&tgt)).expect("a line");
    let want = share * small_hp;
    assert!(
        (series.ys[0] - want).abs() < want * 1e-6,
        "{} vs {want}",
        series.ys[0]
    );
}

#[test]
fn scrams_turn_off_microwarpdrives() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let tackle = fit(&catalog, "[Rifter, Tackle]\n\nWarp Scrambler II\n");
    let runner = fit(&catalog, "[Rifter, Runner]\n\n5MN Microwarpdrive II\n");
    let tackle = engine.graph_snapshot(&tackle, &[], engine.all5(), &catalog);
    let runner = engine.graph_snapshot(&runner, &[], engine.all5(), &catalog);
    assert!(tackle.scram_range.is_some_and(|r| r > 5_000.0));
    let (speed, sig) = runner.ship.scrammed.expect("the MWD can be scrammed");
    assert!(speed < runner.ship.max_velocity / 2.0, "{speed}");
    assert!(sig < runner.ship.sig_radius);
}

#[test]
fn ammo_groups_load_every_charge() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let groups = engine.graph_ammo(
        &fit(&catalog, RUPTURE),
        &[],
        engine.all5(),
        &catalog,
        AmmoQuality::Navy,
    );
    assert_eq!(groups.len(), 1, "one autocannon group");
    let names: Vec<&str> = groups[0].options.iter().map(|o| o.name.as_str()).collect();
    assert!(
        names.contains(&"Barrage M") && names.contains(&"EMP M"),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("Arch Angel")),
        "{names:?}"
    );
    // Every option carries all four guns, and a long range charge reaches further.
    assert!(groups[0].options.iter().all(|o| o.weapons.len() == 4));
    let falloff = |name: &str| {
        groups[0]
            .options
            .iter()
            .find(|o| o.name == name)
            .and_then(|o| match o.weapons[0].kind {
                WeaponKind::Turret(g) => Some(g.falloff),
                _ => None,
            })
            .expect("a turret")
    };
    assert!(falloff("Barrage M") > falloff("EMP M"));
}

#[test]
fn snapshot_reads_weapons_ewar_and_cycles() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let snap = engine.graph_snapshot(&fit(&catalog, RUPTURE), &[], engine.all5(), &catalog);
    assert_eq!(snap.weapons.len(), 4);
    let gun = &snap.weapons[0];
    assert!(matches!(gun.kind, WeaponKind::Turret(g) if g.optimal > 0.0 && g.falloff > 0.0));
    assert!(gun.cycle.shots.is_some(), "autocannons reload");
    assert!(gun.cycle.duration > 1.0 && gun.cycle.duration < 10.0);
    assert!(
        snap.ewar
            .iter()
            .any(|e| e.kind == EwarKind::Web && e.strength < -50.0)
    );
    assert!(snap.ship.lock_range > 0.0 && snap.ship.radius > 0.0);
    assert!(snap.ship.subwarp_speed > 0.0);
    let total: f64 = snap.weapons.iter().map(|w| w.dps.iter().sum::<f64>()).sum();
    assert!(
        (total - snap.ship.dps).abs() < 1e-6,
        "{total} vs {}",
        snap.ship.dps
    );
}
