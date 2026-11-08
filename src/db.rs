//! The user's fits, characters, ESI logins and settings, in one SQLite database: `efa.db` in the
//! data folder, readable by the user only since it holds the refresh tokens.
//!
//! Each list (fits, implant sets, characters, ...) is a table with a row per item: its place in
//! the list, a few columns worth looking at and the item itself as JSON. Everything else is a
//! JSON value in `settings`, by field name. The app keeps its lists as vectors, so the store
//! remembers what it last wrote for each row and a save only touches rows that changed.
//!
//! A database that cannot be read is set aside and the backup, made once per start with
//! `VACUUM INTO`, loads instead. The JSON files of earlier versions are imported the first time.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, ErrorCode, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::{character::Characters, esi::accounts::Accounts, store::Saved};

pub const FILE: &str = "efa.db";

/// Schema changes in order; the database's `user_version` counts those applied.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
     CREATE TABLE fits (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         ship_type_id INTEGER, payload TEXT NOT NULL);
     CREATE INDEX fits_ship ON fits (ship_type_id);
     CREATE TABLE implant_sets (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         payload TEXT NOT NULL);
     CREATE TABLE damage_patterns (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         payload TEXT NOT NULL);
     CREATE TABLE target_profiles (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         payload TEXT NOT NULL);
     CREATE TABLE cargo_presets (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         payload TEXT NOT NULL);
     CREATE TABLE characters (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         character_id INTEGER, payload TEXT NOT NULL);
     CREATE TABLE esi_accounts (
         id INTEGER PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
         character_id INTEGER, payload TEXT NOT NULL);",
];

/// A list kept as a table: the field holding it, and an integer column taken from each item.
struct Table {
    name: &'static str,
    field: &'static str,
    /// The column and the item's field it copies.
    key: Option<(&'static str, &'static str)>,
}

/// A struct kept in the database: its lists as tables and its other fields as settings, under
/// a prefix so structs do not share keys.
struct Doc {
    prefix: &'static str,
    tables: &'static [Table],
}

impl Doc {
    /// The field a settings key belongs to, if it is this struct's.
    fn field<'a>(&self, key: &'a str) -> Option<&'a str> {
        key.strip_prefix(self.prefix).filter(|f| !f.contains('.'))
    }
}

const fn list(name: &'static str) -> Table {
    Table {
        name,
        field: name,
        key: None,
    }
}

const SAVED: Doc = Doc {
    prefix: "",
    tables: &[
        Table {
            name: "fits",
            field: "fits",
            key: Some(("ship_type_id", "ship")),
        },
        list("implant_sets"),
        list("damage_patterns"),
        list("target_profiles"),
        list("cargo_presets"),
    ],
};

const CHARACTERS: Doc = Doc {
    prefix: "characters.",
    tables: &[Table {
        name: "characters",
        field: "characters",
        key: Some(("character_id", "id")),
    }],
};

const ESI: Doc = Doc {
    prefix: "esi.",
    tables: &[Table {
        name: "esi_accounts",
        field: "accounts",
        key: Some(("character_id", "character_id")),
    }],
};

/// Everything the database holds.
#[derive(Debug, Clone, Default)]
pub struct Contents {
    pub saved: Saved,
    pub characters: Characters,
    pub accounts: Accounts,
}

/// What was last written for a row, in list order.
#[derive(Debug, Clone)]
struct Row {
    id: i64,
    position: i64,
    payload: String,
}

/// An item about to be written.
struct Entry {
    name: String,
    key: Option<i64>,
    payload: String,
}

/// The open database and what it holds as of the last load or save.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    rows: HashMap<&'static str, Vec<Row>>,
    settings: HashMap<String, String>,
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
        if let Err(e) = store.write(&[
            (&SAVED, &contents.saved as &dyn Erased),
            (&CHARACTERS, &contents.characters as &dyn Erased),
            (&ESI, &contents.accounts as &dyn Erased),
        ]) {
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
        saved.tidy();
        let characters = self.load_doc(&CHARACTERS, &settings, &mut problems)?;
        let accounts = self.load_doc(&ESI, &settings, &mut problems)?;
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

    /// Put a struct together from its settings and tables. Rows and settings that do not
    /// deserialize are left out, and left alone by later saves.
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
        let mut lists = Vec::new();
        for table in doc.tables {
            let (rows, items) = self
                .read_table(table, problems)
                .map_err(|e| e.to_string())?;
            lists.push((rows, items));
        }

        let assemble = |fields: &[(String, String, Value)], lists: &[(Vec<Row>, Vec<Value>)]| {
            let mut object: Map<String, Value> = fields
                .iter()
                .map(|(_, field, value)| (field.clone(), value.clone()))
                .collect();
            for (table, (_, items)) in doc.tables.iter().zip(lists) {
                object.insert(table.field.to_owned(), Value::Array(items.clone()));
            }
            serde_json::from_value::<T>(Value::Object(object))
        };
        let value = if let Ok(value) = assemble(&fields, &lists) {
            value
        } else {
            drop_unreadable::<T>(doc, &mut fields, &mut lists, problems);
            assemble(&fields, &lists).map_err(|e| e.to_string())?
        };
        for (key, _, value) in fields {
            self.settings.insert(key, value.to_string());
        }
        for (table, (rows, _)) in doc.tables.iter().zip(lists) {
            self.rows.insert(table.name, rows);
        }
        Ok(value)
    }

    /// A table's rows in order, with the items that parse as JSON.
    fn read_table(
        &self,
        table: &Table,
        problems: &mut Vec<String>,
    ) -> rusqlite::Result<(Vec<Row>, Vec<Value>)> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, position, payload FROM {} ORDER BY position, id",
            table.name
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                position: r.get(1)?,
                payload: r.get(2)?,
            })
        })?;
        let (mut kept, mut items) = (Vec::new(), Vec::new());
        for row in rows {
            let row = row?;
            match serde_json::from_str::<Value>(&row.payload) {
                Ok(item) => {
                    items.push(item);
                    kept.push(row);
                }
                Err(e) => problems.push(format!("{} row {}: {e}", table.name, row.id)),
            }
        }
        Ok((kept, items))
    }

    /// Write the fits and settings, touching only what changed since the last save.
    pub fn save_saved(&mut self, saved: &Saved) -> Result<(), String> {
        self.write(&[(&SAVED, saved as &dyn Erased)])
    }

    /// Write the characters, touching only what changed.
    pub fn save_characters(&mut self, characters: &Characters) -> Result<(), String> {
        self.write(&[(&CHARACTERS, characters as &dyn Erased)])
    }

    /// Write the ESI logins, touching only what changed.
    pub fn save_accounts(&mut self, accounts: &Accounts) -> Result<(), String> {
        self.write(&[(&ESI, accounts as &dyn Erased)])
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
    fn write(&mut self, docs: &[(&Doc, &dyn Erased)]) -> Result<(), String> {
        let mut values = Vec::with_capacity(docs.len());
        for (doc, value) in docs {
            match value.to_value().map_err(|e| e.to_string())? {
                Value::Object(object) => values.push((*doc, object)),
                _ => return Err("not a struct".into()),
            }
        }
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        let mut settings = Vec::new();
        for (doc, mut object) in values {
            for table in doc.tables {
                let items = match object.remove(table.field) {
                    Some(Value::Array(items)) => items,
                    _ => Vec::new(),
                };
                let entries = items.iter().map(|item| entry(table, item)).collect();
                let old = self.rows.get(table.name).map_or(&[][..], Vec::as_slice);
                let new = sync_table(&tx, table, old, entries).map_err(|e| e.to_string())?;
                rows.push((table.name, new));
            }
            sync_settings(&tx, doc, &self.settings, object, &mut settings)
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.rows.extend(rows);
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

/// A struct the store writes, without the store being generic over it.
trait Erased {
    fn to_value(&self) -> serde_json::Result<Value>;
}

impl<T: Serialize> Erased for T {
    fn to_value(&self) -> serde_json::Result<Value> {
        serde_json::to_value(self)
    }
}

/// Leave out the settings and items that do not deserialize on their own, noting each.
fn drop_unreadable<T: DeserializeOwned>(
    doc: &Doc,
    fields: &mut Vec<(String, String, Value)>,
    lists: &mut [(Vec<Row>, Vec<Value>)],
    problems: &mut Vec<String>,
) {
    let alone = |field: &str, value: &Value| {
        let object = Map::from_iter([(field.to_owned(), value.clone())]);
        serde_json::from_value::<T>(Value::Object(object)).err()
    };
    fields.retain(|(key, field, value)| {
        let error = alone(field, value);
        if let Some(e) = &error {
            problems.push(format!("setting {key}: {e}"));
        }
        error.is_none()
    });
    for (table, (rows, items)) in doc.tables.iter().zip(lists) {
        let errors: Vec<_> = items
            .iter()
            .map(|item| alone(table.field, &Value::Array(vec![item.clone()])))
            .collect();
        let (mut kept_rows, mut kept_items) = (Vec::new(), Vec::new());
        for ((row, item), error) in rows.drain(..).zip(items.drain(..)).zip(errors) {
            if let Some(e) = error {
                problems.push(format!("{} row {}: {e}", table.name, row.id));
            } else {
                kept_rows.push(row);
                kept_items.push(item);
            }
        }
        *rows = kept_rows;
        *items = kept_items;
    }
}

fn entry(table: &Table, item: &Value) -> Entry {
    Entry {
        name: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        key: table.key.and_then(|(_, field)| item.get(field)?.as_i64()),
        payload: item.to_string(),
    }
}

/// Bring a table in line with `new`, reusing rows: an unchanged item keeps its row, a changed
/// one takes the row at its old place or any row left over. Positions only have to increase,
/// so items keep theirs while they stay in order; deleting a fit moves no others. Returns the
/// rows as now written, in order.
fn sync_table(
    tx: &Transaction<'_>,
    table: &Table,
    old: &[Row],
    new: Vec<Entry>,
) -> rusqlite::Result<Vec<Row>> {
    let mut by_payload: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, row) in old.iter().enumerate().rev() {
        by_payload.entry(row.payload.as_str()).or_default().push(i);
    }
    let mut used = vec![false; old.len()];
    let mut assigned: Vec<Option<usize>> = new
        .iter()
        .map(|entry| {
            let i = by_payload.get_mut(entry.payload.as_str())?.pop()?;
            used[i] = true;
            Some(i)
        })
        .collect();
    for (i, slot) in assigned.iter_mut().enumerate() {
        if slot.is_none() && i < old.len() && !used[i] {
            used[i] = true;
            *slot = Some(i);
        }
    }
    let mut spare = (0..old.len())
        .filter(|&i| !used[i])
        .collect::<Vec<_>>()
        .into_iter();
    for slot in &mut assigned {
        if slot.is_none() {
            *slot = spare.next();
        }
    }
    let unused: Vec<usize> = spare.collect();

    let (key_column, key_value) = match table.key {
        Some((column, _)) => (format!(", {column}"), ", ?4"),
        None => (String::new(), ""),
    };
    let set_key = table
        .key
        .map_or(String::new(), |(column, _)| format!(", {column} = ?4"));
    let mut insert = tx.prepare_cached(&format!(
        "INSERT INTO {} (position, name, payload{key_column}) VALUES (?1, ?2, ?3{key_value})",
        table.name
    ))?;
    let mut update = tx.prepare_cached(&format!(
        "UPDATE {} SET position = ?1, name = ?2, payload = ?3{set_key} WHERE id = ?5",
        table.name
    ))?;
    let mut place = tx.prepare_cached(&format!(
        "UPDATE {} SET position = ?1 WHERE id = ?2",
        table.name
    ))?;
    let mut delete = tx.prepare_cached(&format!("DELETE FROM {} WHERE id = ?1", table.name))?;

    let mut rows = Vec::with_capacity(new.len());
    let mut last = -1_i64;
    for (entry, slot) in new.into_iter().zip(assigned) {
        let reused = slot.map(|i| &old[i]);
        let position = match reused {
            Some(row) if row.position > last => row.position,
            _ => last + 1,
        };
        last = position;
        let id = match reused {
            Some(row) if row.payload == entry.payload => {
                if row.position != position {
                    place.execute(params![position, row.id])?;
                }
                row.id
            }
            Some(row) => {
                update.execute(params![
                    position,
                    entry.name,
                    entry.payload,
                    entry.key,
                    row.id
                ])?;
                row.id
            }
            None => {
                if table.key.is_some() {
                    insert.execute(params![position, entry.name, entry.payload, entry.key])?;
                } else {
                    insert.execute(params![position, entry.name, entry.payload])?;
                }
                tx.last_insert_rowid()
            }
        };
        rows.push(Row {
            id,
            position,
            payload: entry.payload,
        });
    }
    for i in unused {
        delete.execute([old[i].id])?;
    }
    Ok(rows)
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
    // A newer efa's database: its additions are left alone and its JSON read as far as known.
    for (i, sql) in (1_i64..).zip(MIGRATIONS) {
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
