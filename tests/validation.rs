//! Fit validation against the real data: refine's checks come back tied to the right rows.
//! Skipped when no dump is present.

use std::{path::Path, sync::OnceLock};

use gyfa::{
    data::{Catalog, Slot, TypeId},
    engine::{CalcOptions, Engine, ItemRef, Pilot, Problem},
    fit::Fit,
};

const DATA: &str = "data/eve";

fn setup() -> Option<&'static (Catalog, Engine)> {
    static SETUP: OnceLock<Option<(Catalog, Engine)>> = OnceLock::new();
    SETUP
        .get_or_init(|| {
            if !Path::new(DATA).join("phobos/metadata.json").exists() {
                eprintln!("no EVE data in {DATA}, skipping");
                return None;
            }
            let catalog = Catalog::load_cached(Path::new(DATA), Path::new("data/cache"))
                .expect("the dump loads");
            let engine = Engine::load(
                Path::new(DATA),
                Path::new("data/cache"),
                catalog.skills.clone(),
            )
            .expect("the engine loads");
            Some((catalog, engine))
        })
        .as_ref()
}

fn id(catalog: &Catalog, name: &str) -> TypeId {
    catalog
        .find(name)
        .unwrap_or_else(|| panic!("{name} exists"))
        .id
}

fn eft(catalog: &Catalog, text: &str) -> Fit {
    Fit::from_eft(catalog, text).expect("the fit parses").0
}

fn problems(engine: &Engine, fit: &Fit, item: ItemRef) -> Vec<Problem> {
    engine
        .validate(fit, engine.all5())
        .of(item)
        .cloned()
        .collect()
}

#[test]
fn a_clean_fit_has_no_problems() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = eft(
        catalog,
        "[Rifter, Clean]
Damage Control II
Gyrostabilizer II

1MN Afterburner II

200mm AutoCannon II, EMP S
200mm AutoCannon II, EMP S

Small Projectile Burst Aerator I
",
    );
    let validation = engine.validate(&fit, engine.all5());
    assert!(
        validation.items.is_empty(),
        "unexpected: {:?}",
        validation.items
    );
}

#[test]
fn too_many_turrets_mark_every_turret() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    // The Rifter has three turret hardpoints.
    let fit = eft(
        catalog,
        "[Rifter, Turrets]
200mm AutoCannon II
200mm AutoCannon II
200mm AutoCannon II
200mm AutoCannon II
",
    );
    for i in 0..4 {
        assert!(
            problems(engine, &fit, ItemRef::Module(Slot::High, i)).contains(&Problem::Turrets),
            "turret {i}"
        );
    }
}

#[test]
fn rig_size_ship_limit_and_max_group() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = eft(
        catalog,
        "[Rifter, Broken]
Damage Control II
Damage Control II

Bastion Module I

Large Projectile Burst Aerator I
",
    );
    let bastion = id(catalog, "Bastion Module I");
    let validation = engine.validate(&fit, engine.all5());
    let at = |item| validation.of(item).cloned().collect::<Vec<_>>();
    assert!(at(ItemRef::Rig(0)).contains(&Problem::RigSize));
    assert!(at(ItemRef::Module(Slot::Low, 0)).contains(&Problem::MaxGroupFitted(1)));
    assert!(at(ItemRef::Module(Slot::Low, 1)).contains(&Problem::MaxGroupFitted(1)));
    let position = fit
        .high
        .iter()
        .position(|m| m.as_ref().is_some_and(|m| m.type_id == bastion))
        .expect("bastion fitted");
    assert!(at(ItemRef::Module(Slot::High, position)).contains(&Problem::ShipLimit));
    assert!(validation.restrictions().count() >= 4);
}

#[test]
fn modules_past_the_rack_are_too_many() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut fit = Fit::new("Rifter");
    fit.ship = Some(id(catalog, "Rifter"));
    let gyro = id(catalog, "Gyrostabilizer II");
    // The Rifter has four low slots; the fifth module sits past the end.
    for _ in 0..5 {
        fit.low.push(Some(gyfa::fit::Module {
            type_id: gyro,
            state: gyfa::fit::ModuleState::Online,
            charge: None,
            spool: None,
            rah: None,
        }));
    }
    let validation = engine.validate(&fit, engine.all5());
    assert!(
        validation
            .of(ItemRef::Module(Slot::Low, 4))
            .any(|p| *p == Problem::SlotCount(Slot::Low))
    );
    assert!(
        !validation
            .of(ItemRef::Module(Slot::Low, 0))
            .any(|p| matches!(p, Problem::SlotCount(_)))
    );
}

#[test]
fn unmet_skills_follow_the_pilot() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = eft(
        catalog,
        "[Rifter, Skills]
Gyrostabilizer II

200mm AutoCannon II
",
    );
    let novice = Pilot::all(&catalog.skills, 0);
    let validation = engine.validate(&fit, &novice);
    let skills = |item| {
        validation
            .of(item)
            .find_map(|p| match p {
                Problem::Skills(missing) => Some(missing.clone()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let gun = skills(ItemRef::Module(Slot::High, 0));
    let small_projectile = id(catalog, "Small Projectile Turret");
    assert!(
        gun.iter()
            .any(|m| m.skill == small_projectile && m.need >= 1),
        "{gun:?}"
    );
    assert!(!skills(ItemRef::Ship).is_empty(), "the hull needs a skill");
    // Skill problems never refuse a module.
    assert_eq!(validation.restrictions().count(), 0);
    // All 5 has every skill.
    assert!(
        engine
            .validate(&fit, engine.all5())
            .items
            .iter()
            .all(|(_, p)| !matches!(p, Problem::Skills(_)))
    );
}

#[test]
fn stats_carry_the_same_validation() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = eft(
        catalog,
        "[Rifter, Rigs]
Large Projectile Burst Aerator I
",
    );
    let stats = engine.compute_with(&fit, CalcOptions::default());
    assert!(
        stats
            .validation
            .of(ItemRef::Rig(0))
            .any(|p| *p == Problem::RigSize)
    );
}

#[test]
fn only_new_restrictions_refuse_a_change() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut fit = eft(
        catalog,
        "[Rifter, Restrict]
Damage Control II
",
    );
    let before = engine.validate(&fit, engine.all5());
    fit.add(catalog, id(catalog, "Damage Control II"));
    let after = engine.validate(&fit, engine.all5());
    assert_eq!(
        before.refusal(&after, false),
        Some(&Problem::MaxGroupFitted(1))
    );
    // Without restrictions only a full rack refuses.
    assert_eq!(before.refusal(&after, true), None);

    // Adding something allowed to a fit that is already broken goes through.
    let mut more = fit.clone();
    more.add(catalog, id(catalog, "1MN Afterburner II"));
    assert_eq!(
        after.refusal(&engine.validate(&more, engine.all5()), false),
        None
    );

    // The Rifter has four low slots; a fifth module has none, restrictions or not.
    let mut full = fit.clone();
    for _ in 0..2 {
        full.add(catalog, id(catalog, "Gyrostabilizer II"));
    }
    let base = engine.validate(&full, engine.all5());
    full.add(catalog, id(catalog, "Gyrostabilizer II"));
    assert_eq!(
        base.refusal(&engine.validate(&full, engine.all5()), true),
        Some(&Problem::SlotCount(Slot::Low))
    );
}
