//! The database the fits, characters and logins are kept in: importing the JSON files, saving
//! only what changed, and surviving a broken file.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use efa::{
    cargo_preset::{CargoPolicy, CargoPreset},
    character::{Character, CharacterId, Characters},
    damage_pattern::DamagePattern,
    data::Slot,
    db::{Contents, Store},
    esi::accounts::{Access, Account, Accounts},
    fit::{
        Booster, DamageProfile, DroneStack, Fit, ImplantSource, Module, ModuleState, Mutation,
        RahPattern, SystemSecurity,
        fighters::Fighter,
        links::{CommandLink, LinkCategory, LinkSet},
        projected::{
            CommandFit, EnvEffect, ProjectedDrone, ProjectedFighter, ProjectedFit, ProjectedModule,
        },
    },
    implant_set::ImplantSet,
    store::{Saved, Settings},
    target_profile::TargetProfile,
};
use rusqlite::Connection;

const FIXTURES: &str = "tests/fixtures/legacy";

/// An empty folder of its own for each test, never the real data folder.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("efa-db-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The database in `dir`, with this start's backup written.
fn open(dir: &Path) -> (Store, Contents) {
    let (store, contents) = Store::open(dir);
    let mut store = store.expect("the database opens");
    store.wait_for_backup();
    (store, contents)
}

fn json(saved: &Saved) -> serde_json::Value {
    serde_json::to_value(saved).expect("serializes")
}

fn fits(names: &[&str]) -> Saved {
    let mut saved = Saved {
        fits: names.iter().map(|n| Rc::new(Fit::new(*n))).collect(),
        ..Saved::default()
    };
    for (ship, fit) in (587..).zip(&mut saved.fits) {
        Rc::make_mut(fit).ship = Some(ship);
    }
    saved
}

fn names(saved: &Saved) -> Vec<&str> {
    saved.fits.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn json_files_are_imported_once_and_kept() {
    let dir = temp_dir("migrate");
    for file in ["fits.json", "characters.json", "esi.json"] {
        std::fs::copy(Path::new(FIXTURES).join(file), dir.join(file)).expect("copy fixture");
    }
    let read = |file: &str| std::fs::read_to_string(Path::new(FIXTURES).join(file)).expect(file);
    let mut expected: Saved = serde_json::from_str(&read("fits.json")).expect("fixture parses");
    expected.tidy();
    let characters: Characters = serde_json::from_str(&read("characters.json")).expect("parses");
    let accounts: Accounts = serde_json::from_str(&read("esi.json")).expect("parses");
    assert_eq!(
        expected.settings.open,
        vec![0, 2],
        "a tab of a missing fit is dropped"
    );
    assert_ne!(
        expected.fits[0].uid, expected.fits[1].uid,
        "shared uids are split"
    );

    let (store, contents) = open(&dir);
    assert_eq!(json(&contents.saved), json(&expected));
    assert_eq!(contents.characters, characters);
    assert_eq!(contents.accounts, accounts);
    for file in ["fits.json", "characters.json", "esi.json"] {
        assert!(!dir.join(file).exists(), "{file} is moved");
        assert!(
            dir.join(format!("{file}.migrated")).exists(),
            "{file} is kept"
        );
    }
    drop(store);

    let (_store, again) = open(&dir);
    assert_eq!(
        json(&again.saved),
        json(&expected),
        "read back from the database"
    );
    assert_eq!(again.characters, characters);
    assert_eq!(again.accounts, accounts);
    assert_eq!(again.characters.characters[0].skills.get(&3300), Some(&5));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn everything_saved_loads_back_the_same() {
    let dir = temp_dir("round-trip");
    let (mut store, contents) = open(&dir);
    assert_eq!(contents.saved.fits, [] as [std::rc::Rc<efa::fit::Fit>; 0]);

    let mut saved = fits(&["a", "b", "c"]);
    saved.fit_mut(1).notes = "notes".into();
    saved.settings.open = vec![2, 0];
    saved.settings.current = Some(0);
    saved.settings.factor_reload = true;
    saved.settings.columns.insert("name".into(), 123.5);
    saved.settings.market_results = Some(200.0);
    saved.implant_sets = vec![efa::implant_set::ImplantSet::new("set")];
    saved.cargo_presets = vec![efa::cargo_preset::CargoPreset::new("preset")];
    let characters = Characters {
        characters: vec![Character {
            id: 2,
            name: "Pilot".into(),
            sec_status: 1.5,
            ..Character::default()
        }],
        last_id: 2,
    };
    let account = Account {
        character_id: 91,
        name: "Pilot".into(),
        refresh_token: "secret".into(),
        scopes: vec!["a".into()],
        access: Some(Access {
            token: "access-token".into(),
            expires_at: 9,
        }),
    };
    let accounts = Accounts {
        accounts: vec![account.clone()],
    };
    store.save_saved(&saved).expect("save fits");
    store.save_characters(&characters).expect("save characters");
    store.save_accounts(&accounts).expect("save accounts");
    drop(store);

    let (_store, loaded) = open(&dir);
    assert_eq!(json(&loaded.saved), json(&saved));
    assert_eq!(loaded.characters, characters);
    assert_eq!(loaded.accounts.accounts[0].refresh_token, "secret");
    assert!(
        loaded.accounts.accounts[0].access.is_none(),
        "never written"
    );
    let raw = std::fs::read(dir.join("efa.db.bak")).expect("backup");
    assert!(
        !String::from_utf8_lossy(&raw).contains("access-token"),
        "access tokens stay in memory"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changing_one_fit_writes_one_row() {
    let dir = temp_dir("one-row");
    let (mut store, _) = open(&dir);
    let mut saved = fits(&["a", "b", "c", "d", "e"]);
    store.save_saved(&saved).expect("save");

    let before = store.total_changes();
    store.save_saved(&saved).expect("save unchanged");
    assert_eq!(
        store.total_changes(),
        before,
        "nothing changed, nothing written"
    );

    saved.fit_mut(2).notes = "changed".into();
    store.save_saved(&saved).expect("save one change");
    assert_eq!(store.total_changes(), before + 1);

    saved.settings.factor_reload = true;
    store.save_saved(&saved).expect("save a setting");
    assert_eq!(store.total_changes(), before + 2);

    saved.push_fit(Fit::new("f"));
    store.save_saved(&saved).expect("save a new fit");
    assert_eq!(store.total_changes(), before + 3);
    drop(store);

    let (_store, loaded) = open(&dir);
    assert_eq!(json(&loaded.saved), json(&saved));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fits_are_deleted_and_reordered() {
    let dir = temp_dir("reorder");
    let (mut store, _) = open(&dir);
    let mut saved = fits(&["a", "b", "c", "d", "e"]);
    store.save_saved(&saved).expect("save");

    let before = store.total_changes();
    saved.fits.remove(1);
    store.save_saved(&saved).expect("save a deletion");
    assert_eq!(
        store.total_changes(),
        before + 1,
        "the others keep their rows"
    );

    saved.fits.swap(0, 3);
    store.save_saved(&saved).expect("save a reorder");
    saved.fits.insert(0, Rc::new(Fit::new("first")));
    saved.fit_mut(2).notes = "moved and changed".into();
    store.save_saved(&saved).expect("save an insert");
    drop(store);

    let (mut store, loaded) = open(&dir);
    assert_eq!(names(&loaded.saved), ["first", "e", "c", "d", "a"]);
    assert_eq!(json(&loaded.saved), json(&saved));

    let mut saved = loaded.saved;
    saved.fits.reverse();
    saved.fits.truncate(2);
    store.save_saved(&saved).expect("save");
    drop(store);
    let (_store, loaded) = open(&dir);
    assert_eq!(names(&loaded.saved), ["a", "d"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_database_is_set_aside_and_the_backup_loads() {
    let dir = temp_dir("corrupt");
    let (mut store, _) = open(&dir);
    let saved = fits(&["kept", "too"]);
    store.save_saved(&saved).expect("save");
    drop(store);
    // The backup is made when efa starts, so this start's has the fits.
    drop(open(&dir));

    let db = dir.join("efa.db");
    std::fs::write(&db, b"this is not a database, it is a broken file").expect("break it");
    let (store, loaded) = Store::open(&dir);
    assert!(store.is_some(), "efa goes on with the backup");
    assert_eq!(json(&loaded.saved), json(&saved));
    let aside = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("efa.db.unreadable-"))
        .expect("the broken file is kept");
    assert_eq!(
        std::fs::read(dir.join(aside)).expect("read"),
        b"this is not a database, it is a broken file"
    );
    drop(store);

    let (_store, again) = open(&dir);
    assert_eq!(names(&again.saved), ["kept", "too"], "restored for good");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn the_database_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir("private");
    let (mut store, _) = open(&dir);
    store.save_saved(&fits(&["a"])).expect("save");
    for file in ["efa.db", "efa.db.bak", "efa.db-wal"] {
        let mode = std::fs::metadata(dir.join(file))
            .expect(file)
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{file}");
    }
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

fn module(type_id: i32, state: ModuleState) -> Module {
    Module {
        type_id,
        state,
        charge: None,
        spool: None,
        mutation: None,
        rah: None,
    }
}

fn mutation(base: i32, mutaplasmid: i32, attrs: &[(i32, f64)]) -> Mutation {
    Mutation {
        base,
        mutaplasmid,
        attrs: attrs.iter().copied().collect(),
    }
}

fn fighter(type_id: i32) -> Fighter {
    Fighter {
        type_id,
        amount: Some(6),
        active: false,
        abilities: vec![(2, false), (1, true)],
    }
}

/// A fit with every field set to something other than its default.
fn full_fit(name: &str) -> Fit {
    let mut point = module(3242, ModuleState::Overload);
    point.charge = Some(29_001);
    point.spool = Some(7);
    point.mutation = Some(mutation(447, 47_702, &[(20, 1.125), (54, 26_250.5)]));
    let mut rah = module(4403, ModuleState::Active);
    rah.rah = Some(RahPattern::Pattern(DamageProfile::Pattern {
        name: "[NPC]Sansha".into(),
        amounts: [0.5, 0.25, 0.0, 0.125],
    }));
    let mut flat = module(4403, ModuleState::Online);
    flat.rah = Some(RahPattern::DoNotAdapt);
    let mut ammo = module(4403, ModuleState::Offline);
    ammo.rah = Some(RahPattern::Pattern(DamageProfile::SelectedAmmo));
    let mut drone = DroneStack::new(2488, 5, 2);
    drone.mutation = Some(mutation(2488, 60_478, &[(37, 2_000.25)]));
    Fit {
        name: name.into(),
        ship: Some(17_740),
        high: vec![
            Some(point),
            None,
            Some(module(2873, ModuleState::Active)),
            None,
        ],
        mid: vec![None, Some(module(5973, ModuleState::Online))],
        low: vec![Some(rah), None, Some(flat), Some(ammo)],
        rigs: vec![Some(31_790), None, Some(31_790)],
        subsystems: vec![45_586, 45_590],
        services: vec![Some(module(35_892, ModuleState::Online)), None],
        drones: vec![drone, DroneStack::new(2454, 3, 0)],
        implants: vec![13_219, 13_220, 13_221],
        inactive_implants: vec![13_220],
        inactive_rigs: vec![(2, 31_790)],
        implant_source: ImplantSource::Character,
        boosters: vec![
            Booster {
                type_id: 5440,
                active: false,
                side_effects: vec![3, 1],
            },
            Booster::new(28_672),
        ],
        cargo: vec![(185, 1000), (27_441, 2)],
        notes: "kite at 9 km\nsecond line".into(),
        mode: Some(34_317),
        sec_status: Some(-2.25),
        system_security: SystemSecurity::WSpace,
        damage_profile: DamageProfile::Pattern {
            name: "Mostly kinetic".into(),
            amounts: [0.0, 1.0, 3.5, 0.25],
        },
        target_profile: Some(TargetProfile {
            name: "[Frigates]Rifter".into(),
            resists: [0.1, 0.2, 0.3, 0.4],
            max_velocity: Some(412.5),
            signature_radius: Some(35.0),
            radius: None,
            hp: Some(3_000.0),
        }),
        character: CharacterId::User(4),
        // Distinct per name, since fits sharing one are split on load.
        uid: name.bytes().fold(0, |h, b| h * 31 + u64::from(b)),
        fighters: vec![fighter(40_556), Fighter::new(40_557)],
        projected: efa::fit::projected::Projected {
            fits: vec![ProjectedFit {
                fit: 3,
                amount: 4,
                active: false,
                range: Some(24_000.0),
            }],
            modules: vec![ProjectedModule {
                type_id: 526,
                state: ModuleState::Overload,
                charge: Some(12_608),
                spool: Some(3),
                range: Some(9_000.0),
            }],
            drones: vec![ProjectedDrone {
                type_id: 2203,
                count: 3,
                active: true,
                range: None,
            }],
            fighters: vec![ProjectedFighter {
                fighter: fighter(40_558),
                range: Some(50_000.0),
            }],
            env: vec![EnvEffect {
                type_id: 30_844,
                active: false,
            }],
        },
        command: vec![
            CommandFit {
                fit: 7,
                active: true,
            },
            CommandFit {
                fit: 9,
                active: false,
            },
        ],
        command_links: vec![
            CommandLink::new(LinkSet::All, 3, true),
            CommandLink {
                active: false,
                ..CommandLink::new(LinkSet::Charge(42_838), 0, false)
            },
            CommandLink::new(LinkSet::Category(LinkCategory::Mining), 5, false),
        ],
        ignore_restrictions: true,
        grouped: [Slot::High, Slot::Low, Slot::Service].into_iter().collect(),
        modified: 1_700_000_123,
    }
}

fn fit_json(fit: &Fit) -> serde_json::Value {
    serde_json::to_value(fit).expect("serializes")
}

/// Every table holding part of a fit, and the fits.
const FIT_TABLES: [&str; 23] = [
    "fit_modules",
    "module_mutations",
    "module_mutation_attrs",
    "fit_rigs",
    "fit_subsystems",
    "fit_drones",
    "drone_mutations",
    "drone_mutation_attrs",
    "fit_fighters",
    "fighter_abilities",
    "fit_implants",
    "fit_boosters",
    "booster_side_effects",
    "fit_cargo",
    "fit_target_profiles",
    "fit_projected_fits",
    "fit_projected_modules",
    "fit_projected_drones",
    "fit_projected_env",
    "fit_command",
    "fit_command_links",
    "fit_grouped_racks",
    "fits",
];

fn sql(dir: &Path) -> Connection {
    Connection::open(dir.join("efa.db")).expect("open the database")
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .expect(table)
}

fn counts(conn: &Connection) -> Vec<i64> {
    FIT_TABLES.iter().map(|t| count(conn, t)).collect()
}

#[test]
fn a_full_fit_and_the_other_lists_load_back_the_same() {
    let dir = temp_dir("full-fit");
    let (mut store, _) = open(&dir);
    let mut set = ImplantSet::new("Snakes");
    set.implants = vec![13_219, 13_220];
    let mut preset = CargoPreset::new("Ammo");
    preset.items = vec![(185, 2000), (186, 50)];
    preset.policy = CargoPolicy::Exact;
    let mut overrides = efa::overrides::Overrides {
        enabled: true,
        ..Default::default()
    };
    overrides.set(587, 263, 1_250.5);
    overrides.set(587, 37, 400.0);
    overrides.set(2873, 54, 9_000.0);
    let mut saved = Saved {
        settings: Settings {
            overrides,
            ..Settings::default()
        },
        fits: [full_fit("full"), Fit::new("empty"), full_fit("again")]
            .map(Rc::new)
            .into(),
        implant_sets: vec![set, ImplantSet::new("none")],
        damage_patterns: vec![DamagePattern::new("[NPC]Guristas", [0.0, 0.0, 1.5, 0.25])],
        target_profiles: vec![
            TargetProfile::ideal(),
            full_fit("x").target_profile.expect("set"),
        ],
        cargo_presets: vec![preset, CargoPreset::new("empty")],
    };
    store.save_saved(&saved).expect("save");
    drop(store);

    let (mut store, loaded) = open(&dir);
    assert_eq!(fit_json(&loaded.saved.fits[0]), fit_json(&full_fit("full")));
    assert_eq!(json(&loaded.saved), json(&saved));
    let before = store.total_changes();
    store.save_saved(&loaded.saved).expect("save unchanged");
    assert_eq!(
        store.total_changes(),
        before,
        "nothing changed, nothing written"
    );
    saved.settings.overrides.set(587, 263, 1_000.0);
    saved.settings.overrides.remove(2873, 54);
    store.save_saved(&saved).expect("save overrides");
    assert_eq!(store.total_changes(), before + 2, "one changed, one gone");
    drop(store);
    let (store, loaded) = open(&dir);
    assert_eq!(json(&loaded.saved), json(&saved));
    drop(store);

    let conn = sql(&dir);
    assert_eq!(count(&conn, "attribute_overrides"), 2);
    let setting: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'overrides'",
            [],
            |r| r.get(0),
        )
        .expect("the switch is a setting");
    assert_eq!(setting, r#"{"enabled":true}"#);
    let payloads: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_schema t JOIN pragma_table_info(t.name) c
             WHERE t.type = 'table' AND c.name = 'payload'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(payloads, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_changed_fit_rewrites_only_its_own_rows() {
    let dir = temp_dir("one-fit");
    let (mut store, _) = open(&dir);
    let mut saved = Saved {
        fits: ["a", "b", "c", "d"]
            .into_iter()
            .map(|n| Rc::new(full_fit(n)))
            .collect(),
        ..Saved::default()
    };
    store.save_saved(&saved).expect("save");
    let others = |conn: &Connection| -> Vec<(i64, i64, Option<i32>)> {
        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.fit_id, m.type_id FROM fit_modules m JOIN fits f ON f.id = m.fit_id
                 WHERE f.name <> 'c' ORDER BY m.id",
            )
            .expect("prepare");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    };
    let conn = sql(&dir);
    let untouched = others(&conn);
    let before_counts = counts(&conn);
    let per_fit: i64 = before_counts.iter().sum::<i64>() / 4;

    let before = store.total_changes();
    saved.fit_mut(2).notes = "changed".into();
    store.save_saved(&saved).expect("save one change");
    let written = store.total_changes() - before;
    assert_eq!(others(&conn), untouched, "the other fits' rows stay");
    assert_eq!(
        counts(&conn),
        before_counts,
        "parts are replaced, not added"
    );
    // Its row updated and its parts deleted and inserted again, nothing of the other fits.
    let parts = per_fit - 1;
    assert_eq!(written, 1 + 2 * parts as u64);
    drop(store);

    let (_store, loaded) = open(&dir);
    assert_eq!(json(&loaded.saved), json(&saved));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_a_fit_takes_all_its_parts() {
    let dir = temp_dir("cascade");
    let (mut store, _) = open(&dir);
    let mut saved = Saved {
        fits: vec![Rc::new(full_fit("gone")), Rc::new(full_fit("kept"))],
        ..Saved::default()
    };
    store.save_saved(&saved).expect("save");
    let conn = sql(&dir);
    let both = counts(&conn);
    for (table, n) in FIT_TABLES.iter().zip(&both) {
        assert!(
            *n > 0 && n % 2 == 0,
            "{table} has {n} rows for two full fits"
        );
    }

    saved.fits.remove(0);
    store.save_saved(&saved).expect("save the deletion");
    let halves: Vec<i64> = both.iter().map(|n| n / 2).collect();
    assert_eq!(counts(&conn), halves);

    saved.fits.clear();
    store.save_saved(&saved).expect("save no fits");
    assert_eq!(counts(&conn), vec![0; FIT_TABLES.len()]);
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fits_using_a_type_are_a_query() {
    let dir = temp_dir("query");
    let (mut store, _) = open(&dir);
    let mut other = Fit::new("other");
    other.mid = vec![Some(module(3242, ModuleState::Active))];
    let saved = Saved {
        fits: [full_fit("full"), Fit::new("empty"), other]
            .map(Rc::new)
            .into(),
        ..Saved::default()
    };
    store.save_saved(&saved).expect("save");
    let conn = sql(&dir);
    let query = "SELECT DISTINCT f.name FROM fits f JOIN fit_modules m ON m.fit_id = f.id
                 WHERE m.type_id = ?1 ORDER BY f.position";
    let names = |type_id: i32| -> Vec<String> {
        let mut stmt = conn.prepare(query).expect("prepare");
        stmt.query_map([type_id], |r| r.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    };
    assert_eq!(names(3242), ["full", "other"]);
    assert_eq!(names(5973), ["full"]);
    assert_eq!(names(1), Vec::<String>::new());
    let plan: Vec<String> = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {query}"))
        .expect("prepare")
        .query_map([3242], |r| r.get(3))
        .expect("plan")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert!(
        plan.iter().any(|p| p.contains("fit_modules_type")),
        "{plan:?}"
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn characters_keep_their_skills_and_ids() {
    let dir = temp_dir("characters");
    let (mut store, _) = open(&dir);
    let character = |id: u32, name: &str| Character {
        id,
        name: name.into(),
        skills: [(3300, 5), (3301, 4), (3327, 1)].into_iter().collect(),
        implants: vec![13_219, 13_221],
        sec_status: -9.75,
        esi_character: Some(2_119_000_001),
        alpha_clone: Some(2),
        ..Character::default()
    };
    let mut characters = Characters {
        characters: vec![
            character(3, "Main"),
            character(5, "Alt"),
            Character {
                id: 6,
                ..Character::default()
            },
        ],
        last_id: 6,
    };
    store.save_characters(&characters).expect("save");
    drop(store);
    let (mut store, loaded) = open(&dir);
    assert_eq!(loaded.characters, characters);

    characters.characters.swap(0, 1);
    characters.characters[0].skills.insert(3302, 3);
    characters.characters.remove(2);
    store.save_characters(&characters).expect("save changes");
    drop(store);
    let (_store, loaded) = open(&dir);
    assert_eq!(loaded.characters, characters);
    let conn = sql(&dir);
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM characters ORDER BY position")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(ids, [5, 3], "kept under their own ids");
    assert_eq!(count(&conn, "character_skills"), 7);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fit_that_does_not_read_is_left_alone() {
    let dir = temp_dir("unreadable");
    let (mut store, _) = open(&dir);
    store.save_saved(&fits(&["broken", "fine"])).expect("save");
    drop(store);
    sql(&dir)
        .execute(
            "UPDATE fits SET character = 'user', character_id = -1 WHERE name = 'broken'",
            [],
        )
        .expect("break a fit");

    let (mut store, loaded) = open(&dir);
    assert_eq!(names(&loaded.saved), ["fine"]);
    let mut saved = loaded.saved;
    saved.push_fit(Fit::new("new"));
    store.save_saved(&saved).expect("save");
    drop(store);
    assert_eq!(count(&sql(&dir), "fits"), 3, "the broken fit is kept");
    let _ = std::fs::remove_dir_all(&dir);
}
