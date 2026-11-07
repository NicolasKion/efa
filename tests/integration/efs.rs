//! Eve Fleet Simulator export against the real data. Skipped when no dump is present.

use efa::{
    character::{CharacterId, Characters},
    engine::EfsContext,
    port::efs,
};
use serde_json::Value;

use super::characters::{id, rifter, setup};

#[test]
fn efs_export_has_pyfas_shape() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = rifter(catalog);
    let pilot = Characters::default().pilot(CharacterId::All5, catalog);
    let cx = EfsContext {
        catalog,
        fits: &[],
        pilot: &pilot,
        character: None,
        exporter: "efa test",
    };
    let text = efs::export(engine, &fit, &cx).expect("a fit with a ship exports");
    let data: Value = serde_json::from_str(&text).expect("valid JSON");

    assert_eq!(data["name"], "Rifter: Test");
    assert_eq!(data["efsExportVersion"], 0.06);
    assert_eq!(data["shipSize"], "Frigate");
    assert_eq!(data["typeID"], id(catalog, "Rifter"));
    for layer in ["hull", "armor", "shield"] {
        assert!(
            data["resonance"][layer]["em"].is_number(),
            "{layer} resonance"
        );
        assert!(data["ehp"][layer].as_f64().is_some_and(|v| v > 0.0));
    }
    let weapons = data["weapons"].as_array().expect("weapons");
    let guns = weapons
        .iter()
        .find(|w| w["type"] == "Turret")
        .expect("the autocannons");
    assert_eq!(guns["name"], "200mm AutoCannon II, EMP S");
    assert!(guns["dps"].as_f64().is_some_and(|v| v > 0.0));
    assert!(
        weapons.iter().any(|w| w["type"] == "Drone"),
        "the Hobgoblin"
    );
    let ids: Vec<i64> = data["modTypeIDs"]
        .as_array()
        .expect("type IDs")
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    assert!(ids.contains(&i64::from(id(catalog, "200mm AutoCannon II"))));
    assert!(!ids.contains(&0), "no padding");
    let names = data["moduleNames"].as_array().expect("names");
    assert_eq!(names[0], "High Slots:");
    // The Rifter's projectile bonuses count as extra turrets.
    let turrets = data["turretSlots"].as_f64().expect("turret slots");
    let effective = data["effectiveTurrets"]
        .as_f64()
        .expect("effective turrets");
    assert!(
        effective > turrets * 1.2,
        "{effective} effective turrets from {turrets}"
    );
    assert!(data["effectiveLaunchers"].as_f64().is_some());
}
