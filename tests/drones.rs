//! Per-drone numbers from the engine against the real data. Skipped when no dump is present.

use std::path::Path;

use gyfa::{
    data::Catalog,
    engine::{Engine, drone_attr, drone_effect},
    fit::Fit,
};

const DATA: &str = "data/eve";

const VEXOR: &str = "[Vexor, Drone Vexor]

Hobgoblin II x5
Light Armor Maintenance Bot I x2
Mining Drone I x3
Warrior SW-300 x2
";

#[test]
fn stacks_get_their_own_numbers() {
    let data = Path::new(DATA);
    if !data.join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return;
    }
    let catalog = Catalog::load(data).expect("the dump loads");
    let engine = Engine::load(data, &data.join("../cache"), catalog.skills.clone())
        .expect("the engine loads");
    let (mut fit, unknown) = Fit::from_eft(&catalog, VEXOR).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    // Everything in the bay: what a drone does is shown whether it is out or not.
    for stack in &mut fit.drones {
        stack.active = 0;
    }
    let stats = engine.compute(&fit);
    assert_eq!(stats.drones.len(), 4);

    let [hobgoblin, logi, miner] = [&stats.drones[0], &stats.drones[1], &stats.drones[2]];
    for (name, info) in [("hobgoblin", hobgoblin), ("logi", logi), ("miner", miner)] {
        assert!(
            info.ehp.is_some_and(|v| v > 0.0),
            "{name} ehp {:?}",
            info.ehp
        );
        assert!(
            info.shield_regen.is_some_and(|v| v > 0.0),
            "{name} regen {:?}",
            info.shield_regen
        );
        assert!(
            info.attr(drone_attr::MAX_VELOCITY).is_some(),
            "{name} has a speed"
        );
    }
    assert!(hobgoblin.attr(drone_attr::TRACKING_SPEED).is_some());
    assert!(logi.reps[1] > 0.0, "armor reps {:?}", logi.reps);
    assert!(miner.mining.0 > 0.0, "mining {:?}", miner.mining);
    // Speed factor reads 1 on every drone; only the web effect makes it mean something.
    let web = &stats.drones[3];
    assert!(!hobgoblin.has_effect(drone_effect::WEBIFIER));
    assert!(web.has_effect(drone_effect::WEBIFIER));
    assert!(web.attr(drone_attr::SPEED_FACTOR).is_some_and(|v| v < 0.0));
}
