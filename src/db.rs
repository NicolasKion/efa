//! The user's fits, characters, ESI logins and settings, in one SQLite database: `efa.db` in the
//! data folder, readable by the user only since it holds the refresh tokens.
//!
//! The data is kept in tables of its own: a fit is a `fits` row with its racks, drones,
//! fighters, implants, boosters, cargo and projections in child tables that point at it and go
//! with it on delete; implant sets, cargo presets, characters and logins likewise. Lists keep
//! their order in a `position` column. The tables are in [`schema`].
//!
//! Preferences and window state (open tabs, column widths, graph and export settings and the
//! like) are configuration rather than data, and are kept as JSON values in `settings`, by
//! field name, so they can change shape without a schema change. Attribute overrides are data
//! and have a table; only whether they are on is a setting.
//!
//! The app keeps its lists as vectors, so the store remembers what it last wrote for each row
//! (the item serialized, in memory only) and a save only touches the items that changed: a
//! changed fit has its row updated and its parts replaced, in one transaction.
//!
//! A database that cannot be read is set aside and the backup, made once per start with
//! `VACUUM INTO`, loads instead. The JSON files of earlier versions are imported the first time.

mod fits;
mod lists;
mod overrides;
mod schema;
mod sync;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, ErrorCode, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use self::sync::{Item, Row};
use crate::{character::Characters, esi::accounts::Accounts, store::Saved};

pub const FILE: &str = "efa.db";

/// A struct whose lists are tables and whose other fields are settings, under a prefix so
/// structs do not share keys.
struct Doc {
    prefix: &'static str,
    /// The fields kept as tables.
    lists: &'static [&'static str],
    /// Fields of a setting kept as tables, by setting.
    nested: &'static [(&'static str, &'static str)],
}

impl Doc {
    /// The field a settings key belongs to, if it is this struct's.
    fn field<'a>(&self, key: &'a str) -> Option<&'a str> {
        key.strip_prefix(self.prefix).filter(|f| !f.contains('.'))
    }

    /// The struct's settings: its fields besides the lists.
    fn settings(&self, value: &impl Serialize) -> Result<Map<String, Value>, String> {
        let Value::Object(mut object) = serde_json::to_value(value).map_err(|e| e.to_string())?
        else {
            return Err("not a struct".into());
        };
        for list in self.lists {
            object.remove(*list);
        }
        for (field, inner) in self.nested {
            if let Some(Value::Object(setting)) = object.get_mut(*field) {
                setting.remove(*inner);
            }
        }
        Ok(object)
    }
}

const SAVED: Doc = Doc {
    prefix: "",
    lists: &[
        "fits",
        "implant_sets",
        "damage_patterns",
        "target_profiles",
        "cargo_presets",
    ],
    nested: &[("overrides", "values")],
};

const CHARACTERS: Doc = Doc {
    prefix: "characters.",
    lists: &["characters"],
    nested: &[],
};

const ESI: Doc = Doc {
    prefix: "esi.",
    lists: &["accounts"],
    nested: &[],
};

/// Everything the database holds.
#[derive(Debug, Clone, Default)]
pub struct Contents {
    pub saved: Saved,
    pub characters: Characters,
    pub accounts: Accounts,
}

/// The open database and what it holds as of the last load or save.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    rows: HashMap<&'static str, Vec<Row>>,
    settings: HashMap<String, String>,
    /// The attribute overrides as last written.
    overrides: crate::overrides::Values,
}

/// Why a database could not be opened.
enum OpenError {
    /// The file is damaged or not a database; it is set aside.
    Unreadable(String),
    /// Anything else, such as a missing permission; the file is left alone.
    Other(String),
}

impl From<rusqlite::Error> for OpenError {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
                Self::Unreadable(e.to_string())
            }
            _ => Self::Other(e.to_string()),
        }
    }
}

/// Where the database is kept.
pub fn path() -> Option<PathBuf> {
    crate::paths::data_file(FILE)
}

impl Store {
    /// Open the database in the data folder. See [`Store::open`].
    pub fn open_default() -> (Option<Self>, Contents) {
        if let Some(dir) = crate::paths::data_dir() {
            Self::open(&dir)
        } else {
            crate::errors::report(
                "No data folder",
                "There is no home folder to keep fits in, so changes are not saved.",
            );
            (None, Contents::default())
        }
    }

    /// Open `dir`'s database and load it, importing earlier versions' JSON files when there is
    /// no database yet. A database that cannot be read is set aside and its backup loads
    /// instead. Problems are reported; without a store, changes are not saved.
    pub fn open(dir: &Path) -> (Option<Self>, Contents) {
        if let Err(e) = std::fs::create_dir_all(dir) {
            crate::errors::report(
                format!("{} could not be created", dir.display()),
                format!("{e}\n\nChanges are not saved."),
            );
            return (None, Contents::default());
        }
        let path = dir.join(FILE);
        let opened = if path.exists() {
            Self::open_existing(&path)
        } else {
            Self::create(dir, &path)
        };
        if let (Some(store), _) = &opened
            && let Err(e) = store.backup()
        {
            crate::errors::report("Backing up efa.db failed", e);
        }
        opened
    }

    fn open_existing(path: &Path) -> (Option<Self>, Contents) {
        let e = match Self::open_file(path) {
            Ok((store, contents)) => return (Some(store), contents),
            Err(OpenError::Other(e)) => {
                crate::errors::report(
                    format!("{} could not be opened", path.display()),
                    format!("{e}\n\nChanges are not saved."),
                );
                return (None, Contents::default());
            }
            Err(OpenError::Unreadable(e)) => e,
        };
        let Some(aside) = set_aside(path) else {
            crate::errors::report(
                format!("{} could not be read", path.display()),
                format!(
                    "{e}\n\nIt could not be moved out of the way either. Changes are not saved."
                ),
            );
            return (None, Contents::default());
        };
        let aside = aside.display();
        let backup = backup_path(path);
        if backup.exists() && copy_private(&backup, path).is_ok() {
            if let Ok((store, contents)) = Self::open_file(path) {
                crate::errors::report(
                    format!("{} could not be read", path.display()),
                    format!(
                        "{e}\n\nIt was kept as {aside} and the backup from the last start \
                         loaded instead."
                    ),
                );
                return (Some(store), contents);
            }
            // The backup is no good either; keep it from being replaced by an empty one.
            remove_db(path);
            crate::persist::set_aside(&backup);
        }
        crate::errors::report(
            format!("{} could not be read", path.display()),
            format!(
                "{e}\n\nIt was kept as {aside}. There was no readable backup, so efa starts empty."
            ),
        );
        Self::open_new(path)
    }

    /// A new database, with the JSON files of earlier versions imported if there are any.
    fn create(dir: &Path, path: &Path) -> (Option<Self>, Contents) {
        let legacy = Legacy::find(dir);
        if legacy.files.is_empty() {
            return Self::open_new(path);
        }
        let contents = legacy.read();
        let (store, _) = Self::open_new(path);
        let Some(mut store) = store else {
            return (None, contents);
        };
        if let Err(e) = store.write(
            Some(&contents.saved),
            Some(&contents.characters),
            Some(&contents.accounts),
        ) {
            // Without the import, the next start tries again from the same files.
            drop(store);
            remove_db(path);
            crate::errors::report(
                "Moving fits to efa.db failed",
                format!(
                    "{e}\n\nThe JSON files are kept and read again next time. Changes made now \
                     are not saved."
                ),
            );
            return (None, contents);
        }
        legacy.retire();
        (Some(store), contents)
    }

    fn open_new(path: &Path) -> (Option<Self>, Contents) {
        match Self::open_file(path) {
            Ok((store, contents)) => (Some(store), contents),
            Err(OpenError::Other(e) | OpenError::Unreadable(e)) => {
                crate::errors::report(
                    format!("{} could not be created", path.display()),
                    format!("{e}\n\nChanges are not saved."),
                );
                (None, Contents::default())
            }
        }
    }

    /// Open, check, bring the schema up to date and load.
    fn open_file(path: &Path) -> Result<(Self, Contents), OpenError> {
        crate::persist::create_private(path)
            .map_err(|e| OpenError::Other(format!("{}: {e}", path.display())))?;
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(2))?;
        let check: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(OpenError::Unreadable(check));
        }
        let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        let mut store = Self {
            conn,
            path: path.to_owned(),
            rows: HashMap::new(),
            settings: HashMap::new(),
            overrides: crate::overrides::Values::new(),
        };
        let contents = store.load().map_err(OpenError::Other)?;
        Ok((store, contents))
    }

    fn load(&mut self) -> Result<Contents, String> {
        let mut problems = Vec::new();
        let settings = self
            .read_settings(&mut problems)
            .map_err(|e| e.to_string())?;
        let mut saved: Saved = self.load_doc(&SAVED, &settings, &mut problems)?;
        saved.fits = self.load_list(&mut problems)?;
        saved.implant_sets = self.load_list(&mut problems)?;
        saved.damage_patterns = self.load_list(&mut problems)?;
        saved.target_profiles = self.load_list(&mut problems)?;
        saved.cargo_presets = self.load_list(&mut problems)?;
        self.overrides = overrides::read(&self.conn).map_err(|e| e.to_string())?;
        saved.overrides.values.clone_from(&self.overrides);
        saved.tidy();
        let mut characters: Characters = self.load_doc(&CHARACTERS, &settings, &mut problems)?;
        characters.characters = self.load_list(&mut problems)?;
        let mut accounts: Accounts = self.load_doc(&ESI, &settings, &mut problems)?;
        accounts.accounts = self.load_list(&mut problems)?;
        if !problems.is_empty() {
            crate::errors::report(
                format!("Parts of {} could not be read", self.path.display()),
                format!(
                    "{}\n\nThey stay in the database untouched, but efa leaves them out.",
                    problems.join("\n")
                ),
            );
        }
        Ok(Contents {
            saved,
            characters,
            accounts,
        })
    }

    fn read_settings(&self, problems: &mut Vec<String>) -> rusqlite::Result<Vec<(String, Value)>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut settings = Vec::new();
        for row in rows {
            let (key, text) = row?;
            match serde_json::from_str(&text) {
                Ok(value) => settings.push((key, value)),
                Err(e) => problems.push(format!("setting {key}: {e}")),
            }
        }
        Ok(settings)
    }

    /// A struct from its settings, its lists left empty. Settings that do not deserialize are
    /// left out, and left alone by later saves.
    fn load_doc<T: DeserializeOwned>(
        &mut self,
        doc: &Doc,
        settings: &[(String, Value)],
        problems: &mut Vec<String>,
    ) -> Result<T, String> {
        let mut fields: Vec<(String, String, Value)> = settings
            .iter()
            .filter_map(|(key, value)| {
                let field = doc.field(key)?;
                Some((key.clone(), field.to_owned(), value.clone()))
            })
            .collect();
        let assemble = |fields: &[(String, String, Value)]| {
            let object: Map<String, Value> = fields
                .iter()
                .map(|(_, field, value)| (field.clone(), value.clone()))
                .collect();
            serde_json::from_value::<T>(Value::Object(object))
        };
        let value = if let Ok(value) = assemble(&fields) {
            value
        } else {
            fields.retain(|field| {
                let alone = assemble(std::slice::from_ref(field));
                if let Err(e) = &alone {
                    problems.push(format!("setting {}: {e}", field.0));
                }
                alone.is_ok()
            });
            assemble(&fields).map_err(|e| e.to_string())?
        };
        for (key, _, value) in fields {
            self.settings.insert(key, value.to_string());
        }
        Ok(value)
    }

    /// A table's items in order. Rows that do not read are left out, and left alone by later
    /// saves.
    fn load_list<T: Item>(&mut self, problems: &mut Vec<String>) -> Result<Vec<T>, String> {
        let loaded = T::read(&self.conn).map_err(|e| e.to_string())?;
        let (mut rows, mut items) = (Vec::new(), Vec::new());
        for read in loaded.rows {
            match read.item {
                Ok(item) => {
                    rows.push(Row {
                        id: read.id,
                        position: read.position,
                        fingerprint: sync::fingerprint(&item).map_err(|e| e.to_string())?,
                    });
                    items.push(item);
                }
                Err(e) => problems.push(format!("{} row {}: {e}", T::TABLE, read.id)),
            }
        }
        self.rows.insert(T::TABLE, rows);
        Ok(items)
    }

    /// Write the fits and settings, touching only what changed since the last save.
    pub fn save_saved(&mut self, saved: &Saved) -> Result<(), String> {
        self.write(Some(saved), None, None)
    }

    /// Write the characters, touching only what changed.
    pub fn save_characters(&mut self, characters: &Characters) -> Result<(), String> {
        self.write(None, Some(characters), None)
    }

    /// Write the ESI logins, touching only what changed.
    pub fn save_accounts(&mut self, accounts: &Accounts) -> Result<(), String> {
        self.write(None, None, Some(accounts))
    }

    /// How many rows were inserted, changed or deleted since the database was opened.
    pub fn total_changes(&self) -> u64 {
        self.conn.total_changes()
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write structs in one transaction. What was written is only remembered once it is
    /// committed, so a failed save is tried again in full next time.
    fn write(
        &mut self,
        saved: Option<&Saved>,
        characters: Option<&Characters>,
        accounts: Option<&Accounts>,
    ) -> Result<(), String> {
        let mut docs = Vec::new();
        if let Some(saved) = saved {
            docs.push((&SAVED, SAVED.settings(saved)?));
        }
        if let Some(characters) = characters {
            docs.push((&CHARACTERS, CHARACTERS.settings(characters)?));
        }
        if let Some(accounts) = accounts {
            docs.push((&ESI, ESI.settings(accounts)?));
        }
        let e = |e: rusqlite::Error| e.to_string();
        let tx = self.conn.transaction().map_err(e)?;
        let old = &self.rows;
        let mut rows = Vec::new();
        if let Some(saved) = saved {
            rows.push(sync_list(&tx, old, &saved.fits).map_err(e)?);
            rows.push(sync_list(&tx, old, &saved.implant_sets).map_err(e)?);
            rows.push(sync_list(&tx, old, &saved.damage_patterns).map_err(e)?);
            rows.push(sync_list(&tx, old, &saved.target_profiles).map_err(e)?);
            rows.push(sync_list(&tx, old, &saved.cargo_presets).map_err(e)?);
            overrides::sync(&tx, &self.overrides, &saved.overrides.values).map_err(e)?;
        }
        if let Some(characters) = characters {
            rows.push(sync_list(&tx, old, &characters.characters).map_err(e)?);
        }
        if let Some(accounts) = accounts {
            rows.push(sync_list(&tx, old, &accounts.accounts).map_err(e)?);
        }
        let mut settings = Vec::new();
        for (doc, object) in docs {
            sync_settings(&tx, doc, &self.settings, object, &mut settings).map_err(e)?;
        }
        tx.commit().map_err(e)?;
        self.rows.extend(rows);
        if let Some(saved) = saved {
            self.overrides.clone_from(&saved.overrides.values);
        }
        for (key, value) in settings {
            match value {
                Some(value) => self.settings.insert(key, value),
                None => self.settings.remove(&key),
            };
        }
        Ok(())
    }

    /// Copy the database to `efa.db.bak`, through a temporary file so a failed copy leaves the
    /// last backup as it was.
    fn backup(&self) -> Result<(), String> {
        let backup = backup_path(&self.path);
        let tmp = crate::persist::with_suffix(&backup, ".tmp");
        let _ = std::fs::remove_file(&tmp);
        // VACUUM INTO writes into an empty file, which keeps the mode it was made with.
        crate::persist::create_private(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let target = tmp.to_str().ok_or("the data folder's path is not UTF-8")?;
        let vacuumed = self
            .conn
            .execute("VACUUM INTO ?1", [target])
            .map_err(|e| e.to_string());
        if let Err(e) = vacuumed {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, &backup).map_err(|e| format!("{}: {e}", backup.display()))
    }
}

/// Bring an item's table in line with `items`, given the rows last written.
fn sync_list<T: Item>(
    tx: &Transaction<'_>,
    rows: &HashMap<&'static str, Vec<Row>>,
    items: &[T],
) -> rusqlite::Result<(&'static str, Vec<Row>)> {
    let old = rows.get(T::TABLE).map_or(&[][..], Vec::as_slice);
    Ok((T::TABLE, sync::sync(tx, old, items)?))
}

/// Write the settings that changed and drop those no longer there, noting each change.
fn sync_settings(
    tx: &Transaction<'_>,
    doc: &Doc,
    written: &HashMap<String, String>,
    object: Map<String, Value>,
    changes: &mut Vec<(String, Option<String>)>,
) -> rusqlite::Result<()> {
    let mut upsert = tx.prepare_cached(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )?;
    let mut delete = tx.prepare_cached("DELETE FROM settings WHERE key = ?1")?;
    let keys: std::collections::HashSet<String> = object
        .keys()
        .map(|f| format!("{}{f}", doc.prefix))
        .collect();
    for (field, value) in object {
        let key = format!("{}{field}", doc.prefix);
        let value = value.to_string();
        if written.get(&key) != Some(&value) {
            upsert.execute(params![key, value])?;
            changes.push((key, Some(value)));
        }
    }
    for key in written.keys() {
        if doc.field(key).is_some() && !keys.contains(key) {
            delete.execute([key])?;
            changes.push((key.clone(), None));
        }
    }
    Ok(())
}

/// Apply the schema changes the database has not had yet, each in its own transaction.
fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    // A newer efa's database: its additions are left alone and the rest read as far as known.
    for (i, sql) in (1_i64..).zip(schema::MIGRATIONS) {
        if i <= version {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i)?;
        tx.commit()?;
    }
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    crate::persist::with_suffix(path, ".bak")
}

/// The database's own files next to it.
fn companions(path: &Path) -> [PathBuf; 2] {
    [
        crate::persist::with_suffix(path, "-wal"),
        crate::persist::with_suffix(path, "-shm"),
    ]
}

/// Move an unreadable database out of the way with its write-ahead log, under a name SQLite
/// still pairs them by.
fn set_aside(path: &Path) -> Option<PathBuf> {
    let aside = crate::persist::set_aside(path)?;
    let [wal, shm] = companions(path);
    let _ = std::fs::rename(wal, crate::persist::with_suffix(&aside, "-wal"));
    let _ = std::fs::remove_file(shm);
    Some(aside)
}

fn remove_db(path: &Path) {
    let _ = std::fs::remove_file(path);
    for file in companions(path) {
        let _ = std::fs::remove_file(file);
    }
}

fn copy_private(from: &Path, to: &Path) -> std::io::Result<()> {
    for file in companions(to) {
        let _ = std::fs::remove_file(file);
    }
    crate::persist::create_private(to)?;
    std::fs::copy(from, to).map(drop)
}

/// The JSON files efa kept its data in before the database.
struct Legacy {
    dir: PathBuf,
    files: Vec<&'static str>,
}

const LEGACY_FILES: [&str; 3] = ["fits.json", "characters.json", "esi.json"];

impl Legacy {
    fn find(dir: &Path) -> Self {
        let files = LEGACY_FILES
            .into_iter()
            .filter(|f| {
                let path = dir.join(f);
                path.exists() || crate::persist::backup(&path).exists()
            })
            .collect();
        Self {
            dir: dir.to_owned(),
            files,
        }
    }

    fn read(&self) -> Contents {
        let [fits, characters, esi] = LEGACY_FILES.map(|f| self.dir.join(f));
        let mut saved: Saved = crate::persist::load(&fits);
        saved.tidy();
        Contents {
            saved,
            characters: crate::persist::load(&characters),
            accounts: crate::persist::load(&esi),
        }
    }

    /// Rename the imported files, and their backups, to `<name>.migrated`.
    fn retire(&self) {
        for file in &self.files {
            let path = self.dir.join(file);
            for path in [crate::persist::backup(&path), path] {
                if path.exists() {
                    let to = crate::persist::with_suffix(&path, ".migrated");
                    if let Err(e) = std::fs::rename(&path, &to) {
                        eprintln!("renaming {}: {e}", path.display());
                    }
                }
            }
        }
    }
}
