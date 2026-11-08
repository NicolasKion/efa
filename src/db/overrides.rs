//! pyfa's attribute overrides, a row per type and attribute. Whether they are on is a setting.

use rusqlite::{Connection, Transaction, params};

use crate::overrides::Values;

pub fn read(conn: &Connection) -> rusqlite::Result<Values> {
    let mut stmt = conn.prepare("SELECT type_id, attribute_id, value FROM attribute_overrides")?;
    let mut rows = stmt.query([])?;
    let mut values = Values::new();
    while let Some(row) = rows.next()? {
        values
            .entry(row.get(0)?)
            .or_default()
            .insert(row.get(1)?, row.get(2)?);
    }
    Ok(values)
}

/// Write the values that changed since `old` and drop those no longer there.
pub fn sync(tx: &Transaction<'_>, old: &Values, new: &Values) -> rusqlite::Result<()> {
    if old == new {
        return Ok(());
    }
    let mut upsert = tx.prepare_cached(
        "INSERT INTO attribute_overrides (type_id, attribute_id, value) VALUES (?1, ?2, ?3)
         ON CONFLICT (type_id, attribute_id) DO UPDATE SET value = excluded.value",
    )?;
    let mut delete = tx.prepare_cached(
        "DELETE FROM attribute_overrides WHERE type_id = ?1 AND attribute_id = ?2",
    )?;
    for (type_id, attrs) in new {
        for (attr, value) in attrs {
            let was = old.get(type_id).and_then(|a| a.get(attr));
            if was != Some(value) {
                upsert.execute(params![type_id, attr, value])?;
            }
        }
    }
    for (type_id, attrs) in old {
        for attr in attrs.keys() {
            if !new.get(type_id).is_some_and(|a| a.contains_key(attr)) {
                delete.execute(params![type_id, attr])?;
            }
        }
    }
    Ok(())
}
