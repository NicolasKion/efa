//! What the item stats window reads, against the real data: per-item attributes after bonuses,
//! where each modification comes from, and the extra type data. Skipped when no dump is present.

use std::{collections::HashSet, path::Path};

use efa::{
    data::{Catalog, Slot, TypeId},
    engine::{Engine, FitItem, ModOp, SourceKind},
    fit::Fit,
    item_data::{ItemData, type_attr},
};

const DATA: &str = "data/eve";

const RIFTER: &str = "[Rifter, Stats]
Gyrostabilizer II
Gyrostabilizer II

1MN Afterburner II

200mm AutoCannon I, EMP S
";

/// Damage multiplier, which gyrostabilizers and skills raise.
const DAMAGE_MULTIPLIER: i32 = 64;

fn setup() -> Option<(Catalog, Engine)> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    let catalog =
        Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads");
    let engine = Engine::load(
        Path::new(DATA),
        Path::new("data/cache"),
        catalog.skills.clone(),
    )
    .expect("the engine loads");
    Some((catalog, engine))
}

fn item_data(catalog: &Catalog) -> ItemData {
    let wanted: HashSet<TypeId> = catalog.types.keys().copied().collect();
    let key = efa::data::cache_key(Path::new(DATA));
    ItemData::load_cached(
        Path::new(DATA),
        Path::new("data/cache"),
        key.as_deref(),
        &wanted,
    )
    .expect("the item data loads")
}

fn rifter(catalog: &Catalog) -> Fit {
    let (fit, unknown) = Fit::from_eft(catalog, RIFTER).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    fit
}

#[test]
fn a_turret_shows_its_bonuses_and_where_they_come_from() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = rifter(&catalog);
    let turret = engine
        .inspect(&fit, engine.all5(), FitItem::Module(Slot::High, 0))
        .expect("the turret is there");
    assert_eq!(
        turret.type_id,
        catalog.find("200mm AutoCannon I").unwrap().id
    );
    let damage = turret
        .attr(DAMAGE_MULTIPLIER)
        .expect("turrets have a damage multiplier");
    assert!(damage.modified > damage.base, "{damage:?}");

    let (_, mods) = turret
        .modifiers
        .iter()
        .find(|(attr, _)| *attr == DAMAGE_MULTIPLIER)
        .expect("the damage multiplier is modified");
    let gyro = catalog.find("Gyrostabilizer II").unwrap().id;
    let from_gyros: Vec<_> = mods.iter().filter(|m| m.source.type_id == gyro).collect();
    assert_eq!(from_gyros.len(), 2, "one modification per gyrostabilizer");
    assert!(
        from_gyros
            .iter()
            .all(|m| m.penalized && m.op == ModOp::Multiply)
    );
    assert!(from_gyros[0].amount > 1.0);
    assert!(
        from_gyros
            .iter()
            .all(|m| matches!(m.source.item, Some(FitItem::Module(Slot::Low, _))))
    );
    assert!(
        mods.iter().any(|m| m.source.kind == SourceKind::Skill),
        "skills raise turret damage too"
    );
    assert!(turret.effects.iter().any(|&(_, running)| running));
}

#[test]
fn charges_ships_and_empty_places() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = rifter(&catalog);
    let charge = engine
        .inspect(&fit, engine.all5(), FitItem::Charge(Slot::High, 0))
        .expect("the turret is loaded");
    assert_eq!(charge.type_id, catalog.find("EMP S").unwrap().id);
    let ship = engine
        .inspect(&fit, engine.all5(), FitItem::Ship)
        .expect("the ship is there");
    assert!(
        ship.modifiers
            .iter()
            .flat_map(|(_, m)| m)
            .any(|m| m.source.kind == SourceKind::Module),
        "the afterburner changes the ship"
    );
    assert!(
        engine
            .inspect(&fit, engine.all5(), FitItem::Module(Slot::Mid, 3))
            .is_none()
    );
    assert!(
        engine
            .inspect(&fit, engine.all5(), FitItem::Drone(0))
            .is_none()
    );
}

#[test]
fn type_data_has_descriptions_traits_and_requirements() {
    let Some((catalog, _)) = setup() else { return };
    let data = item_data(&catalog);
    let rifter = catalog.find("Rifter").unwrap().id;
    let info = data.type_info(rifter).expect("the Rifter is described");
    assert!(
        info.description
            .as_deref()
            .is_some_and(|d| d.contains("Rifter"))
    );
    let traits = info.traits.as_ref().expect("the Rifter has bonuses");
    let frigate = catalog.find("Minmatar Frigate").unwrap().id;
    assert_eq!(traits.skills[0].0, frigate);
    assert_eq!(traits.skills[0].1[0].number.as_deref(), Some("7.5%"));
    assert!(info.attr(type_attr::MASS).is_some_and(|m| m > 0.0));
    assert_eq!(info.required_skills(), [(frigate, 1)]);
    assert!(
        catalog
            .required_for(frigate)
            .iter()
            .any(|&(id, level)| id == rifter && level == 1)
    );
    let cpu = data.attr(50).expect("CPU is an attribute");
    assert_eq!(cpu.name, "cpu");
    assert_eq!(data.unit(cpu).map(|u| u.name.as_str()), Some("Teraflops"));
}
