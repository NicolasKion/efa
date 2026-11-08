//! The database the fits, characters and logins are kept in: importing the JSON files, saving
//! only what changed, and surviving a broken file.

use std::path::{Path, PathBuf};

use efa::{
    character::{Character, Characters},
    db::{Contents, Store},
    esi::accounts::{Access, Account, Accounts},
    fit::Fit,
    store::Saved,
};

const FIXTURES: &str = "tests/fixtures/legacy";

/// An empty folder of its own for each test, never the real data folder.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("efa-db-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open(dir: &Path) -> (Store, Contents) {
    let (store, contents) = Store::open(dir);
    (store.expect("the database opens"), contents)
}

fn json(saved: &Saved) -> serde_json::Value {
    serde_json::to_value(saved).expect("serializes")
}

fn fits(names: &[&str]) -> Saved {
    let mut saved = Saved {
        fits: names.iter().map(|n| Fit::new(*n)).collect(),
        ..Saved::default()
    };
    for (ship, fit) in (587..).zip(&mut saved.fits) {
        fit.ship = Some(ship);
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
        expected.open,
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
    assert!(contents.saved.fits.is_empty());

    let mut saved = fits(&["a", "b", "c"]);
    saved.fits[1].notes = "notes".into();
    saved.open = vec![2, 0];
    saved.current = Some(0);
    saved.factor_reload = true;
    saved.columns.insert("name".into(), 123.5);
    saved.market_results = Some(200.0);
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

    saved.fits[2].notes = "changed".into();
    store.save_saved(&saved).expect("save one change");
    assert_eq!(store.total_changes(), before + 1);

    saved.factor_reload = true;
    store.save_saved(&saved).expect("save a setting");
    assert_eq!(store.total_changes(), before + 2);

    saved.fits.push(Fit::new("f"));
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
    saved.fits.insert(0, Fit::new("first"));
    saved.fits[2].notes = "moved and changed".into();
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
