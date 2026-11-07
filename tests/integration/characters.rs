//! Characters, skill levels and requirements against the real data. Skipped when no dump is
//! present.

use std::{path::Path, sync::OnceLock};

use gyfa::{
    character::{
        CharacterId, Characters, Keep,
        requirements::{self, Numerals},
    },
    data::{Catalog, Slot, TypeId},
    engine::{CalcOptions, Engine, SkillTarget},
    fit::Fit,
};

const DATA: &str = "data/eve";

const RIFTER: &str = "[Rifter, Test]
200mm AutoCannon II, EMP S
200mm AutoCannon II, EMP S

Hobgoblin II x1
";

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

fn rifter(catalog: &Catalog) -> Fit {
    Fit::from_eft(catalog, RIFTER).expect("the fit parses").0
}

#[test]
fn stats_follow_the_characters_skills() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let fit = rifter(catalog);
    let mut characters = Characters::default();
    let pilot = characters.copy(CharacterId::All5, "Pilot", catalog);
    let gunnery = id(catalog, "Gunnery");
    let dps = |characters: &Characters, who| {
        let pilot = characters.pilot(who, &catalog.skills);
        engine
            .compute_for(&fit, &pilot, CalcOptions::default())
            .weapon_dps
    };
    let all5 = dps(&characters, CharacterId::All5);
    assert!(
        (dps(&characters, pilot) - all5).abs() < 1e-6,
        "a copy of All 5"
    );
    assert!((engine.compute(&fit).weapon_dps - all5).abs() < 1e-6);

    characters.change_level(pilot, gunnery, Some(0), Keep::Pending, catalog);
    let lower = dps(&characters, pilot);
    assert!(lower < all5, "Gunnery 0 shoots slower: {lower} vs {all5}");
    assert!(characters.is_dirty(pilot));
    characters.revert_levels(pilot);
    assert!((dps(&characters, pilot) - all5).abs() < 1e-6);

    assert!(dps(&characters, CharacterId::All0) < lower);
}

#[test]
fn levels_train_prerequisites_and_unlearn_dependents() {
    let Some((catalog, _)) = setup() else { return };
    let mut characters = Characters::default();
    let pilot = characters.create("Pilot");
    let gunnery = id(catalog, "Gunnery");
    let rapid = id(catalog, "Rapid Firing");
    let small_proj = id(catalog, "Small Projectile Turret");

    characters.change_level(pilot, rapid, Some(3), Keep::Saved, catalog);
    assert_eq!(characters.level(pilot, rapid), Some(3));
    assert!(characters.level(pilot, gunnery).is_some_and(|l| l >= 2));
    characters.change_level(pilot, small_proj, Some(5), Keep::Saved, catalog);

    // Rapid Firing needs Gunnery II, so taking Gunnery down to I unlearns it.
    characters.change_level(pilot, gunnery, Some(1), Keep::Saved, catalog);
    assert_eq!(characters.level(pilot, rapid), None);
    assert_eq!(characters.level(pilot, small_proj), Some(5));
    characters.change_level(pilot, gunnery, None, Keep::Saved, catalog);
    assert_eq!(characters.level(pilot, small_proj), None);
    assert!(!characters.is_dirty(pilot));
}

#[test]
fn without_strict_levels_dependents_stay() {
    let Some((catalog, _)) = setup() else { return };
    let mut characters = Characters::default();
    let pilot = characters.create("Pilot");
    let gunnery = id(catalog, "Gunnery");
    let rapid = id(catalog, "Rapid Firing");

    characters.change_level(pilot, rapid, Some(3), Keep::Saved, catalog);
    characters.change_level_with(pilot, gunnery, Some(1), Keep::Saved, false, catalog);
    assert_eq!(characters.level(pilot, gunnery), Some(1));
    assert_eq!(characters.level(pilot, rapid), Some(3));
}

#[test]
fn missing_skills_and_granting_them() {
    let Some((catalog, _)) = setup() else { return };
    let fit = rifter(catalog);
    let mut characters = Characters::default();
    let pilot = characters.create("Pilot");
    let missing = |characters: &Characters| {
        requirements::missing(&fit, catalog, &[], |s| characters.level_or_zero(pilot, s))
    };
    let before = missing(&characters);
    assert_ne!(before, []);
    let condensed = requirements::condensed(&before, catalog);
    let text = requirements::lines(&condensed, catalog, Numerals::Arabic);
    assert!(text.contains("Minmatar Frigate 1\n"), "{text}");
    let condensed_more = requirements::super_condensed(&before, catalog);
    assert!(condensed_more.len() < condensed.len());

    for (skill, level) in condensed {
        characters.raise_level(pilot, skill, level, catalog);
    }
    assert_eq!(missing(&characters), []);
    assert!(characters.is_dirty(pilot), "granted skills wait for a save");
    assert!(
        requirements::missing(&fit, catalog, &[], |_| 5).is_empty(),
        "All 5 has everything"
    );
}

#[test]
fn skills_round_trip_through_the_clipboard_format() {
    let Some((catalog, _)) = setup() else { return };
    let mut characters = Characters::default();
    let from = characters.create("From");
    characters.change_level(from, id(catalog, "Gunnery"), Some(4), Keep::Saved, catalog);
    let text = characters.export_skills(from, catalog);
    assert!(text.contains("Gunnery 4\n"));

    let to = characters.create("To");
    let errors = characters
        .import_skills(to, &text, catalog)
        .expect("a user character");
    assert_eq!(errors, 0);
    characters.save_levels(to);
    for &skill in &catalog.skills {
        assert_eq!(
            characters.level_or_zero(to, skill),
            characters.level_or_zero(from, skill)
        );
    }
    let errors = characters
        .import_skills(to, "Gunnery II\nNo Such Skill 3\n", catalog)
        .expect("a user character");
    assert_eq!(errors, 1);
    assert_eq!(characters.level(to, id(catalog, "Gunnery")), Some(2));
}

#[test]
fn skills_affecting_items() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let rifter = id(catalog, "Rifter");
    let gun = id(catalog, "200mm AutoCannon II");
    let skills = engine.affecting_skills(Some(rifter), SkillTarget::Module(Slot::High, gun));
    for name in ["Gunnery", "Small Projectile Turret", "Rapid Firing"] {
        assert!(skills.contains(&id(catalog, name)), "{name} in {skills:?}");
    }
    let ship = engine.affecting_skills(Some(rifter), SkillTarget::Ship);
    assert!(ship.contains(&id(catalog, "Minmatar Frigate")), "{ship:?}");
    let drone = engine.affecting_skills(
        Some(rifter),
        SkillTarget::Drone(id(catalog, "Hobgoblin II")),
    );
    assert!(
        drone.contains(&id(catalog, "Drone Interfacing")),
        "{drone:?}"
    );
    let charge = engine.affecting_skills(
        Some(rifter),
        SkillTarget::Charge {
            slot: Slot::High,
            module: id(catalog, "Rocket Launcher II"),
            charge: id(catalog, "Mjolnir Rocket"),
        },
    );
    assert!(
        charge.contains(&id(catalog, "Warhead Upgrades")),
        "{charge:?}"
    );
}
