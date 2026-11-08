//! Lists kept as tables: reading an item back from its row and parts, and bringing a table in
//! line with a list while writing only the items that changed.

use std::{borrow::Borrow, collections::HashMap};

use rusqlite::{
    Connection, Transaction, params_from_iter,
    types::{Type, Value},
};
use serde::Serialize;

/// An item of a list kept as a table: a row with an `id` and a `position`, plus parts in
/// tables of their own that point at the row.
pub trait Item: Serialize + Sized {
    const TABLE: &'static str;
    /// The row's columns besides `id` and `position`, in the order [`Item::columns`] gives.
    const COLUMNS: &'static [&'static str];
    /// The tables holding the item's parts, with the column pointing at its row. Their own
    /// parts go with them by cascade.
    const PARTS: &'static [(&'static str, &'static str)];

    /// The id the row is kept under, for items that have one of their own; others get one
    /// from SQLite.
    fn own_id(&self) -> rusqlite::Result<Option<i64>> {
        Ok(None)
    }

    fn columns(&self) -> rusqlite::Result<Vec<Value>>;

    /// Insert the item's parts for the row `id`.
    fn write_parts(&self, tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()>;

    /// Every row, in list order, with its parts.
    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>>;
}

/// What was last written for a row, in list order. The fingerprint is a hash of the item
/// serialized, kept in memory only to tell whether it changed.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: i64,
    pub position: i64,
    pub fingerprint: u64,
}

/// A row read back, or why it could not be.
pub struct Read<T> {
    pub id: i64,
    pub position: i64,
    pub item: Result<T, String>,
}

/// A table's rows as they are read, with their parts added as those are.
pub struct Loaded<T> {
    pub rows: Vec<Read<T>>,
    by_id: HashMap<i64, usize>,
}

impl<T> Loaded<T> {
    /// Read the rows of `sql`, which selects `id` and `position` first and orders by them.
    pub fn rows(
        conn: &Connection,
        sql: &str,
        decode: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<Self> {
        let mut stmt = conn.prepare(sql)?;
        let mut query = stmt.query([])?;
        let mut rows = Vec::new();
        while let Some(row) = query.next()? {
            let item = decode(row);
            rows.push(Read {
                id: row.get(0)?,
                position: row.get(1)?,
                item: unreadable(item)?,
            });
        }
        let by_id = rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
        Ok(Self { rows, by_id })
    }

    /// Add the parts `sql` selects, the owning row's id first, in order. A part that does not
    /// read makes its item unreadable.
    pub fn parts<P>(
        &mut self,
        conn: &Connection,
        sql: &str,
        decode: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<P>,
        mut add: impl FnMut(&mut T, P),
    ) -> rusqlite::Result<()> {
        let mut stmt = conn.prepare(sql)?;
        let mut query = stmt.query([])?;
        while let Some(row) = query.next()? {
            let owner: i64 = row.get(0)?;
            let Some(read) = self.by_id.get(&owner).map(|&i| &mut self.rows[i]) else {
                continue;
            };
            let Ok(item) = &mut read.item else {
                continue;
            };
            match unreadable(decode(row))? {
                Ok(part) => add(item, part),
                Err(e) => read.item = Err(e),
            }
        }
        Ok(())
    }
}

/// A value that does not convert is a problem with that row, not with the database.
fn unreadable<T>(result: rusqlite::Result<T>) -> rusqlite::Result<Result<T, String>> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(
            e @ (rusqlite::Error::FromSqlConversionFailure(..)
            | rusqlite::Error::InvalidColumnType(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)),
        ) => Ok(Err(e.to_string())),
        Err(e) => Err(e),
    }
}

/// An enum kept as a TEXT code.
pub trait Code: Copy + PartialEq + 'static {
    const CODES: &'static [(Self, &'static str)];

    fn code(self) -> &'static str {
        Self::CODES
            .iter()
            .find(|(v, _)| *v == self)
            .map_or("", |(_, c)| c)
    }
}

/// The enum at column `i`.
pub fn code<T: Code>(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<T> {
    let text: String = row.get(i)?;
    from_code(i, &text)
}

fn from_code<T: Code>(i: usize, text: &str) -> rusqlite::Result<T> {
    T::CODES
        .iter()
        .find(|(_, c)| *c == text)
        .map(|(v, _)| *v)
        .ok_or_else(|| unknown(i, text))
}

pub fn unknown(i: usize, text: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(i, Type::Text, format!("unknown code {text}").into())
}

/// A text value.
pub fn text(s: &str) -> Value {
    Value::Text(s.to_owned())
}

/// An unsigned 64 bit value as SQLite keeps it, signed.
pub fn int(v: u64) -> rusqlite::Result<i64> {
    i64::try_from(v).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

pub fn big(v: u64) -> rusqlite::Result<Value> {
    int(v).map(Value::Integer)
}

/// Zero as NULL, for values where zero means none.
pub fn big_or_null(v: u64) -> rusqlite::Result<Value> {
    if v == 0 { Ok(Value::Null) } else { big(v) }
}

/// What tells a changed item from an unchanged one: a hash of its serialized form, made
/// without building the text.
pub fn fingerprint(item: &impl Serialize) -> rusqlite::Result<u64> {
    let mut hasher = HashWriter(std::hash::DefaultHasher::new());
    serde_json::to_writer(&mut hasher, item)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(std::hash::Hasher::finish(&hasher.0))
}

/// Feeds what is written into a hasher.
struct HashWriter(std::hash::DefaultHasher);

impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        std::hash::Hasher::write(&mut self.0, bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Bring a table in line with `new`, whose fingerprints are `prints`. An unchanged item keeps
/// its row; a changed one is written to the row at its old place or any row left over, its parts
/// replaced. Items with ids of their own keep the row of that id. Positions only have to
/// increase, so items keep theirs while they stay in order; deleting a fit moves no others.
/// Returns the rows as now written, in order.
pub fn sync<T: Item>(
    tx: &Transaction<'_>,
    old: &[Row],
    new: &[impl Borrow<T>],
    prints: Vec<u64>,
) -> rusqlite::Result<Vec<Row>> {
    let new: Vec<&T> = new.iter().map(Borrow::borrow).collect();
    let own = new
        .iter()
        .map(|item| item.own_id())
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut used = vec![false; old.len()];
    let mut assigned: Vec<Option<usize>> = vec![None; new.len()];
    let by_id: HashMap<i64, usize> = old.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut seen = std::collections::HashSet::new();
    for (slot, id) in assigned.iter_mut().zip(&own) {
        let Some(id) = id else { continue };
        if !seen.insert(*id) {
            let e = format!("two {} share the id {id}", T::TABLE);
            return Err(rusqlite::Error::ToSqlConversionFailure(e.into()));
        }
        if let Some(&i) = by_id.get(id) {
            used[i] = true;
            *slot = Some(i);
        }
    }
    let mut by_print: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, row) in old.iter().enumerate().rev() {
        if !used[i] {
            by_print.entry(row.fingerprint).or_default().push(i);
        }
    }
    let free = |own: &Option<i64>, slot: &Option<usize>| own.is_none() && slot.is_none();
    for ((slot, print), id) in assigned.iter_mut().zip(&prints).zip(&own) {
        if free(id, slot)
            && let Some(i) = by_print.get_mut(print).and_then(Vec::pop)
        {
            used[i] = true;
            *slot = Some(i);
        }
    }
    for (i, (slot, id)) in assigned.iter_mut().zip(&own).enumerate() {
        if free(id, slot) && i < old.len() && !used[i] {
            used[i] = true;
            *slot = Some(i);
        }
    }
    let mut spare = (0..old.len())
        .filter(|&i| !used[i])
        .collect::<Vec<_>>()
        .into_iter();
    for (slot, id) in assigned.iter_mut().zip(&own) {
        if free(id, slot) {
            *slot = spare.next();
        }
    }

    let table = T::TABLE;
    let columns = T::COLUMNS.join(", ");
    let marks = (0..T::COLUMNS.len())
        .map(|i| format!("?{}", i + 3))
        .collect::<Vec<_>>();
    let sets = T::COLUMNS
        .iter()
        .zip(&marks)
        .map(|(c, m)| format!(", {c} = {m}"))
        .collect::<Vec<_>>()
        .concat();
    let marks = marks.join(", ");
    let mut delete = tx.prepare_cached(&format!("DELETE FROM {table} WHERE id = ?1"))?;
    for i in spare {
        delete.execute([old[i].id])?;
    }
    let mut insert = tx.prepare_cached(&format!(
        "INSERT INTO {table} (id, position, {columns}) VALUES (?1, ?2, {marks})"
    ))?;
    let mut update = tx.prepare_cached(&format!(
        "UPDATE {table} SET position = ?2{sets} WHERE id = ?1"
    ))?;
    let mut place =
        tx.prepare_cached(&format!("UPDATE {table} SET position = ?2 WHERE id = ?1"))?;
    let mut clear = T::PARTS
        .iter()
        .map(|(part, column)| tx.prepare_cached(&format!("DELETE FROM {part} WHERE {column} = ?1")))
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut rows = Vec::with_capacity(new.len());
    let mut last = -1_i64;
    for (((item, print), id), slot) in new.iter().zip(prints).zip(own).zip(assigned) {
        let reused = slot.map(|i| &old[i]);
        let position = match reused {
            Some(row) if row.position > last => row.position,
            _ => last + 1,
        };
        last = position;
        let id = match reused {
            Some(row) if row.fingerprint == print => {
                if row.position != position {
                    place.execute((row.id, position))?;
                }
                row.id
            }
            Some(row) => {
                let values = [Value::Integer(row.id), Value::Integer(position)];
                update.execute(params_from_iter(values.into_iter().chain(item.columns()?)))?;
                for stmt in &mut clear {
                    stmt.execute([row.id])?;
                }
                item.write_parts(tx, row.id)?;
                row.id
            }
            None => {
                // A row of this id that did not read is in the way; the item takes its place.
                if let Some(id) = id {
                    delete.execute([id])?;
                }
                let key = id.map_or(Value::Null, Value::Integer);
                let values = [key, Value::Integer(position)];
                insert.execute(params_from_iter(values.into_iter().chain(item.columns()?)))?;
                let id = tx.last_insert_rowid();
                item.write_parts(tx, id)?;
                id
            }
        };
        rows.push(Row {
            id,
            position,
            fingerprint: print,
        });
    }
    Ok(rows)
}
