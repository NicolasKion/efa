//! The lists besides fits: implant sets, damage patterns, target profiles, cargo presets,
//! characters and ESI logins.

use rusqlite::{Connection, Transaction, params, types::Value};

use super::{
    fits::{amounts, target_profile},
    sync::{Code, Item, Loaded, big, code, int, text},
};
use crate::{
    cargo_preset::{CargoPolicy, CargoPreset},
    character::Character,
    damage_pattern::DamagePattern,
    esi::accounts::Account,
    implant_set::ImplantSet,
    target_profile::TargetProfile,
};

impl Code for CargoPolicy {
    const CODES: &'static [(Self, &'static str)] = &[
        (Self::TopUp, "top_up"),
        (Self::AddOnTop, "add_on_top"),
        (Self::Exact, "exact"),
    ];
}

impl Item for ImplantSet {
    const TABLE: &'static str = "implant_sets";
    const COLUMNS: &'static [&'static str] = &["name"];
    const PARTS: &'static [(&'static str, &'static str)] = &[("implant_set_implants", "set_id")];

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        Ok(vec![text(&self.name)])
    }

    fn write_parts(&self, tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
        let mut insert = tx.prepare_cached(
            "INSERT INTO implant_set_implants (set_id, position, type_id) VALUES (?1, ?2, ?3)",
        )?;
        for (i, t) in self.implants.iter().enumerate() {
            insert.execute(params![id, i, t])?;
        }
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        let mut sets = Loaded::rows(
            conn,
            "SELECT id, position, name FROM implant_sets ORDER BY position, id",
            |r| Ok(Self::new(r.get::<_, String>(2)?)),
        )?;
        sets.parts(
            conn,
            "SELECT set_id, type_id FROM implant_set_implants ORDER BY set_id, position",
            |r| r.get(1),
            |set, t| set.implants.push(t),
        )?;
        Ok(sets)
    }
}

impl Item for DamagePattern {
    const TABLE: &'static str = "damage_patterns";
    const COLUMNS: &'static [&'static str] = &["name", "em", "thermal", "kinetic", "explosive"];
    const PARTS: &'static [(&'static str, &'static str)] = &[];

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        let mut values = vec![text(&self.name)];
        values.extend(self.amounts.map(Value::Real));
        Ok(values)
    }

    fn write_parts(&self, _: &Transaction<'_>, _: i64) -> rusqlite::Result<()> {
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        Loaded::rows(
            conn,
            "SELECT id, position, name, em, thermal, kinetic, explosive
             FROM damage_patterns ORDER BY position, id",
            |r| Ok(Self::new(r.get::<_, String>(2)?, amounts(r, 3)?)),
        )
    }
}

impl Item for TargetProfile {
    const TABLE: &'static str = "target_profiles";
    const COLUMNS: &'static [&'static str] = &[
        "name",
        "em",
        "thermal",
        "kinetic",
        "explosive",
        "max_velocity",
        "signature_radius",
        "radius",
        "hp",
    ];
    const PARTS: &'static [(&'static str, &'static str)] = &[];

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        let mut values = vec![text(&self.name)];
        values.extend(self.resists.map(Value::Real));
        values.extend(
            [
                self.max_velocity,
                self.signature_radius,
                self.radius,
                self.hp,
            ]
            .map(Value::from),
        );
        Ok(values)
    }

    fn write_parts(&self, _: &Transaction<'_>, _: i64) -> rusqlite::Result<()> {
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        Loaded::rows(
            conn,
            "SELECT id, position, name, em, thermal, kinetic, explosive, max_velocity,
                 signature_radius, radius, hp
             FROM target_profiles ORDER BY position, id",
            |r| target_profile(r, 2),
        )
    }
}

impl Item for CargoPreset {
    const TABLE: &'static str = "cargo_presets";
    const COLUMNS: &'static [&'static str] = &["name", "policy"];
    const PARTS: &'static [(&'static str, &'static str)] = &[("cargo_preset_items", "preset_id")];

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        Ok(vec![text(&self.name), text(self.policy.code())])
    }

    fn write_parts(&self, tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
        let mut insert = tx.prepare_cached(
            "INSERT INTO cargo_preset_items (preset_id, position, type_id, quantity)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (i, (t, n)) in self.items.iter().enumerate() {
            insert.execute(params![id, i, t, n])?;
        }
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        let mut presets = Loaded::rows(
            conn,
            "SELECT id, position, name, policy FROM cargo_presets ORDER BY position, id",
            |r| {
                Ok(Self {
                    name: r.get(2)?,
                    items: Vec::new(),
                    policy: code(r, 3)?,
                })
            },
        )?;
        presets.parts(
            conn,
            "SELECT preset_id, type_id, quantity FROM cargo_preset_items
             ORDER BY preset_id, position",
            |r| Ok((r.get(1)?, r.get(2)?)),
            |preset, item| preset.items.push(item),
        )?;
        Ok(presets)
    }
}

impl Item for Character {
    const TABLE: &'static str = "characters";
    const COLUMNS: &'static [&'static str] =
        &["name", "sec_status", "esi_character_id", "alpha_clone"];
    const PARTS: &'static [(&'static str, &'static str)] = &[
        ("character_skills", "character_id"),
        ("character_implants", "character_id"),
    ];

    fn own_id(&self) -> rusqlite::Result<Option<i64>> {
        Ok(Some(self.id.into()))
    }

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        Ok(vec![
            text(&self.name),
            Value::Real(self.sec_status),
            self.esi_character
                .map(big)
                .transpose()?
                .unwrap_or(Value::Null),
            self.alpha_clone.into(),
        ])
    }

    fn write_parts(&self, tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
        let mut skill = tx.prepare_cached(
            "INSERT INTO character_skills (character_id, skill_id, level) VALUES (?1, ?2, ?3)",
        )?;
        for (s, level) in &self.skills {
            skill.execute(params![id, s, level])?;
        }
        let mut implant = tx.prepare_cached(
            "INSERT INTO character_implants (character_id, position, type_id)
             VALUES (?1, ?2, ?3)",
        )?;
        for (i, t) in self.implants.iter().enumerate() {
            implant.execute(params![id, i, t])?;
        }
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        let mut characters = Loaded::rows(
            conn,
            "SELECT id, position, name, sec_status, esi_character_id, alpha_clone
             FROM characters ORDER BY position, id",
            |r| {
                Ok(Self {
                    id: r.get(0)?,
                    name: r.get(2)?,
                    sec_status: r.get(3)?,
                    esi_character: r.get(4)?,
                    alpha_clone: r.get(5)?,
                    ..Self::default()
                })
            },
        )?;
        characters.parts(
            conn,
            "SELECT character_id, skill_id, level FROM character_skills",
            |r| Ok((r.get(1)?, r.get(2)?)),
            |c, (skill, level)| {
                c.skills.insert(skill, level);
            },
        )?;
        characters.parts(
            conn,
            "SELECT character_id, type_id FROM character_implants
             ORDER BY character_id, position",
            |r| r.get(1),
            |c, t| c.implants.push(t),
        )?;
        Ok(characters)
    }
}

impl Item for Account {
    const TABLE: &'static str = "esi_accounts";
    const COLUMNS: &'static [&'static str] = &["name", "refresh_token"];
    const PARTS: &'static [(&'static str, &'static str)] = &[("esi_account_scopes", "account_id")];

    fn own_id(&self) -> rusqlite::Result<Option<i64>> {
        int(self.character_id).map(Some)
    }

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        Ok(vec![text(&self.name), text(&self.refresh_token)])
    }

    fn write_parts(&self, tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
        let mut insert = tx.prepare_cached(
            "INSERT INTO esi_account_scopes (account_id, position, scope) VALUES (?1, ?2, ?3)",
        )?;
        for (i, s) in self.scopes.iter().enumerate() {
            insert.execute(params![id, i, s])?;
        }
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        let mut accounts = Loaded::rows(
            conn,
            "SELECT id, position, name, refresh_token FROM esi_accounts ORDER BY position, id",
            |r| {
                Ok(Self {
                    character_id: r.get(0)?,
                    name: r.get(2)?,
                    refresh_token: r.get(3)?,
                    ..Self::default()
                })
            },
        )?;
        accounts.parts(
            conn,
            "SELECT account_id, scope FROM esi_account_scopes ORDER BY account_id, position",
            |r| r.get(1),
            |a, s| a.scopes.push(s),
        )?;
        Ok(accounts)
    }
}
