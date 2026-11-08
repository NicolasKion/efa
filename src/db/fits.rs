//! A fit as a `fits` row and its parts: racks, drones, fighters, implants, boosters, cargo, what
//! is projected onto it and its fleet boosters.

use std::collections::HashMap;

use rusqlite::{Connection, Transaction, params, types::Value};

use super::sync::{Code, Item, Loaded, big_or_null, code, text, unknown};
use crate::{
    character::CharacterId,
    data::Slot,
    fit::{
        Booster, DamageProfile, DroneStack, Fit, ImplantSource, Module, ModuleState, Mutation,
        RahPattern, SystemSecurity,
        fighters::Fighter,
        projected::{
            CommandFit, EnvEffect, ProjectedDrone, ProjectedFighter, ProjectedFit, ProjectedModule,
        },
    },
    target_profile::TargetProfile,
};

impl Code for ModuleState {
    const CODES: &'static [(Self, &'static str)] = &[
        (Self::Offline, "offline"),
        (Self::Online, "online"),
        (Self::Active, "active"),
        (Self::Overload, "overload"),
    ];
}

impl Code for Slot {
    const CODES: &'static [(Self, &'static str)] = &[
        (Self::High, "high"),
        (Self::Mid, "mid"),
        (Self::Low, "low"),
        (Self::Rig, "rig"),
        (Self::Subsystem, "subsystem"),
        (Self::Service, "service"),
    ];
}

impl Code for SystemSecurity {
    const CODES: &'static [(Self, &'static str)] = &[
        (Self::HighSec, "high"),
        (Self::LowSec, "low"),
        (Self::NullSec, "null"),
        (Self::WSpace, "wspace"),
    ];
}

impl Code for ImplantSource {
    const CODES: &'static [(Self, &'static str)] =
        &[(Self::Fit, "fit"), (Self::Character, "character")];
}

/// A damage profile as a kind, a name and four amounts.
fn damage_values(profile: &DamageProfile) -> [Value; 6] {
    match profile {
        DamageProfile::Uniform => kind_only("uniform"),
        DamageProfile::SelectedAmmo => kind_only("selected_ammo"),
        DamageProfile::Pattern { name, amounts } => {
            let [em, thermal, kinetic, explosive] = amounts.map(Value::Real);
            [text("pattern"), text(name), em, thermal, kinetic, explosive]
        }
    }
}

/// A kind without a name or amounts.
fn kind_only(kind: &str) -> [Value; 6] {
    let mut values = std::array::from_fn(|_| Value::Null);
    values[0] = text(kind);
    values
}

/// The damage profile in the six columns from `i`, whose kind is `kind`.
fn damage_profile(
    row: &rusqlite::Row<'_>,
    i: usize,
    kind: &str,
) -> rusqlite::Result<DamageProfile> {
    Ok(match kind {
        "uniform" => DamageProfile::Uniform,
        "selected_ammo" => DamageProfile::SelectedAmmo,
        "pattern" => DamageProfile::Pattern {
            name: row.get(i + 1)?,
            amounts: amounts(row, i + 2)?,
        },
        other => return Err(unknown(i, other)),
    })
}

/// EM, thermal, kinetic and explosive in the four columns from `i`.
pub fn amounts(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<[f64; 4]> {
    Ok([
        row.get(i)?,
        row.get(i + 1)?,
        row.get(i + 2)?,
        row.get(i + 3)?,
    ])
}

fn rah_values(rah: Option<&RahPattern>) -> [Value; 6] {
    match rah {
        None => std::array::from_fn(|_| Value::Null),
        Some(RahPattern::DoNotAdapt) => kind_only("do_not_adapt"),
        Some(RahPattern::Pattern(profile)) => damage_values(profile),
    }
}

fn rah(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<RahPattern>> {
    let kind: Option<String> = row.get(i)?;
    Ok(match kind.as_deref() {
        None => None,
        Some("do_not_adapt") => Some(RahPattern::DoNotAdapt),
        Some(kind) => Some(RahPattern::Pattern(damage_profile(row, i, kind)?)),
    })
}

fn character_values(id: CharacterId) -> [Value; 2] {
    match id {
        CharacterId::All0 => [text("all0"), Value::Null],
        CharacterId::All5 => [text("all5"), Value::Null],
        CharacterId::User(id) => [text("user"), id.into()],
    }
}

fn character(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<CharacterId> {
    let kind: String = row.get(i)?;
    Ok(match kind.as_str() {
        "all0" => CharacterId::All0,
        "all5" => CharacterId::All5,
        "user" => CharacterId::User(row.get(i + 1)?),
        other => return Err(unknown(i, other)),
    })
}

/// The racks that hold modules.
const RACKS: [Slot; 4] = [Slot::High, Slot::Mid, Slot::Low, Slot::Service];

impl Item for Fit {
    const TABLE: &'static str = "fits";
    const COLUMNS: &'static [&'static str] = &[
        "name",
        "ship_type_id",
        "uid",
        "character",
        "character_id",
        "implant_source",
        "mode_type_id",
        "sec_status",
        "system_security",
        "damage_profile",
        "damage_name",
        "damage_em",
        "damage_thermal",
        "damage_kinetic",
        "damage_explosive",
        "ignore_restrictions",
        "notes",
        "modified",
    ];
    const PARTS: &'static [(&'static str, &'static str)] = &[
        ("fit_modules", "fit_id"),
        ("fit_rigs", "fit_id"),
        ("fit_subsystems", "fit_id"),
        ("fit_drones", "fit_id"),
        ("fit_fighters", "fit_id"),
        ("fit_implants", "fit_id"),
        ("fit_boosters", "fit_id"),
        ("fit_cargo", "fit_id"),
        ("fit_target_profiles", "fit_id"),
        ("fit_projected_fits", "fit_id"),
        ("fit_projected_modules", "fit_id"),
        ("fit_projected_drones", "fit_id"),
        ("fit_projected_env", "fit_id"),
        ("fit_command", "fit_id"),
        ("fit_grouped_racks", "fit_id"),
    ];

    fn columns(&self) -> rusqlite::Result<Vec<Value>> {
        let [character, character_id] = character_values(self.character);
        let mut values = vec![
            text(&self.name),
            self.ship.into(),
            big_or_null(self.uid)?,
            character,
            character_id,
            text(self.implant_source.code()),
            self.mode.into(),
            self.sec_status.into(),
            text(self.system_security.code()),
        ];
        values.extend(damage_values(&self.damage_profile));
        values.extend([
            self.ignore_restrictions.into(),
            text(&self.notes),
            big_or_null(self.modified)?,
        ]);
        Ok(values)
    }

    fn write_parts(&self, tx: &Transaction<'_>, id: i64) -> rusqlite::Result<()> {
        write_modules(tx, id, self)?;
        let mut rig = tx.prepare_cached(
            "INSERT INTO fit_rigs (fit_id, position, type_id) VALUES (?1, ?2, ?3)",
        )?;
        for (i, t) in self.rigs.iter().enumerate() {
            rig.execute(params![id, i, t])?;
        }
        let mut sub = tx.prepare_cached(
            "INSERT INTO fit_subsystems (fit_id, position, type_id) VALUES (?1, ?2, ?3)",
        )?;
        for (i, t) in self.subsystems.iter().enumerate() {
            sub.execute(params![id, i, t])?;
        }
        write_drones(tx, id, &self.drones)?;
        write_fighters(tx, id, false, self.fighters.iter().map(|f| (f, None)))?;
        write_fighters(
            tx,
            id,
            true,
            self.projected
                .fighters
                .iter()
                .map(|p| (&p.fighter, p.range)),
        )?;
        let mut implant = tx.prepare_cached(
            "INSERT INTO fit_implants (fit_id, position, type_id, active) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (i, &t) in self.implants.iter().enumerate() {
            implant.execute(params![id, i, t, self.implant_active(t)])?;
        }
        write_boosters(tx, id, &self.boosters)?;
        let mut cargo = tx.prepare_cached(
            "INSERT INTO fit_cargo (fit_id, position, type_id, quantity) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (i, (t, n)) in self.cargo.iter().enumerate() {
            cargo.execute(params![id, i, t, n])?;
        }
        if let Some(p) = &self.target_profile {
            let [em, thermal, kinetic, explosive] = p.resists;
            tx.prepare_cached(
                "INSERT INTO fit_target_profiles (fit_id, name, em, thermal, kinetic, explosive,
                     max_velocity, signature_radius, radius, hp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![
                id,
                p.name,
                em,
                thermal,
                kinetic,
                explosive,
                p.max_velocity,
                p.signature_radius,
                p.radius,
                p.hp
            ])?;
        }
        write_projected(tx, id, self)?;
        let mut command = tx.prepare_cached(
            "INSERT INTO fit_command (fit_id, position, booster_uid, active)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (i, c) in self.command.iter().enumerate() {
            command.execute(params![id, i, c.fit, c.active])?;
        }
        let mut grouped =
            tx.prepare_cached("INSERT INTO fit_grouped_racks (fit_id, rack) VALUES (?1, ?2)")?;
        for slot in &self.grouped {
            grouped.execute(params![id, slot.code()])?;
        }
        Ok(())
    }

    fn read(conn: &Connection) -> rusqlite::Result<Loaded<Self>> {
        let mut fits = Loaded::rows(
            conn,
            &format!(
                "SELECT id, position, {} FROM fits ORDER BY position, id",
                Self::COLUMNS.join(", ")
            ),
            |r| {
                let kind: String = r.get(11)?;
                Ok(Fit {
                    name: r.get(2)?,
                    ship: r.get(3)?,
                    uid: r.get::<_, Option<u64>>(4)?.unwrap_or(0),
                    character: character(r, 5)?,
                    implant_source: code(r, 7)?,
                    mode: r.get(8)?,
                    sec_status: r.get(9)?,
                    system_security: code(r, 10)?,
                    damage_profile: damage_profile(r, 11, &kind)?,
                    ignore_restrictions: r.get(17)?,
                    notes: r.get(18)?,
                    modified: r.get::<_, Option<u64>>(19)?.unwrap_or(0),
                    ..Fit::default()
                })
            },
        )?;
        read_modules(conn, &mut fits)?;
        fits.parts(
            conn,
            "SELECT fit_id, type_id FROM fit_rigs ORDER BY fit_id, position",
            |r| r.get(1),
            |fit, t| fit.rigs.push(t),
        )?;
        fits.parts(
            conn,
            "SELECT fit_id, type_id FROM fit_subsystems ORDER BY fit_id, position",
            |r| r.get(1),
            |fit, t| fit.subsystems.push(t),
        )?;
        read_drones(conn, &mut fits)?;
        read_fighters(conn, &mut fits)?;
        fits.parts(
            conn,
            "SELECT fit_id, type_id, active FROM fit_implants ORDER BY fit_id, position",
            |r| Ok((r.get(1)?, r.get::<_, bool>(2)?)),
            // Switched off implants come back in slot order; only which ones are matters.
            |fit, (t, active)| {
                fit.implants.push(t);
                if !active && !fit.inactive_implants.contains(&t) {
                    fit.inactive_implants.push(t);
                }
            },
        )?;
        read_boosters(conn, &mut fits)?;
        fits.parts(
            conn,
            "SELECT fit_id, type_id, quantity FROM fit_cargo ORDER BY fit_id, position",
            |r| Ok((r.get(1)?, r.get(2)?)),
            |fit, item| fit.cargo.push(item),
        )?;
        fits.parts(
            conn,
            "SELECT fit_id, name, em, thermal, kinetic, explosive, max_velocity,
                 signature_radius, radius, hp
             FROM fit_target_profiles",
            |r| target_profile(r, 1),
            |fit, p| fit.target_profile = Some(p),
        )?;
        read_projected(conn, &mut fits)?;
        fits.parts(
            conn,
            "SELECT fit_id, booster_uid, active FROM fit_command ORDER BY fit_id, position",
            |r| {
                Ok(CommandFit {
                    fit: r.get(1)?,
                    active: r.get(2)?,
                })
            },
            |fit, c| fit.command.push(c),
        )?;
        fits.parts(
            conn,
            "SELECT fit_id, rack FROM fit_grouped_racks",
            |r| code(r, 1),
            |fit, slot| {
                fit.grouped.insert(slot);
            },
        )?;
        Ok(fits)
    }
}

/// A target profile in the columns from `i`: name, four resists, speed, signature, radius, HP.
pub fn target_profile(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<TargetProfile> {
    Ok(TargetProfile {
        name: r.get(i)?,
        resists: amounts(r, i + 1)?,
        max_velocity: r.get(i + 5)?,
        signature_radius: r.get(i + 6)?,
        radius: r.get(i + 7)?,
        hp: r.get(i + 8)?,
    })
}

fn write_modules(tx: &Transaction<'_>, id: i64, fit: &Fit) -> rusqlite::Result<()> {
    let mut insert = tx.prepare_cached(
        "INSERT INTO fit_modules (fit_id, rack, position, type_id, state, charge_type_id, spool,
             rah, rah_name, rah_em, rah_thermal, rah_kinetic, rah_explosive)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let mut empty =
        tx.prepare_cached("INSERT INTO fit_modules (fit_id, rack, position) VALUES (?1, ?2, ?3)")?;
    for (rack, modules) in fit.module_racks() {
        for (i, module) in modules.iter().enumerate() {
            let Some(m) = module else {
                empty.execute(params![id, rack.code(), i])?;
                continue;
            };
            let [rah, name, em, thermal, kinetic, explosive] = rah_values(m.rah.as_ref());
            insert.execute(params![
                id,
                rack.code(),
                i,
                m.type_id,
                m.state.code(),
                m.charge,
                m.spool,
                rah,
                name,
                em,
                thermal,
                kinetic,
                explosive
            ])?;
            if let Some(mutation) = &m.mutation {
                write_mutation(tx, "module", tx.last_insert_rowid(), mutation)?;
            }
        }
    }
    Ok(())
}

/// A module's or drone stack's mutation, `owner` being "module" or "drone".
fn write_mutation(
    tx: &Transaction<'_>,
    owner: &str,
    id: i64,
    mutation: &Mutation,
) -> rusqlite::Result<()> {
    tx.prepare_cached(&format!(
        "INSERT INTO {owner}_mutations ({owner}_id, base_type_id, mutaplasmid_type_id)
         VALUES (?1, ?2, ?3)"
    ))?
    .execute(params![id, mutation.base, mutation.mutaplasmid])?;
    let mut attr = tx.prepare_cached(&format!(
        "INSERT INTO {owner}_mutation_attrs ({owner}_id, attribute_id, value) VALUES (?1, ?2, ?3)"
    ))?;
    for (a, v) in &mutation.attrs {
        attr.execute(params![id, a, v])?;
    }
    Ok(())
}

/// Read `owner`'s mutations ("module" or "drone", whose rows are in `parent`) and hand each,
/// with its owner's row id, to `add`.
fn read_mutations(
    conn: &Connection,
    fits: &mut Loaded<Fit>,
    owner: &str,
    parent: &str,
    mut add: impl FnMut(&mut Fit, i64) -> Option<&mut Option<Mutation>>,
) -> rusqlite::Result<()> {
    fits.parts(
        conn,
        &format!(
            "SELECT p.fit_id, m.{owner}_id, m.base_type_id, m.mutaplasmid_type_id
             FROM {owner}_mutations m JOIN {parent} p ON p.id = m.{owner}_id"
        ),
        |r| Ok((r.get::<_, i64>(1)?, r.get(2)?, r.get(3)?)),
        |fit, (id, base, mutaplasmid)| {
            if let Some(slot) = add(fit, id) {
                *slot = Some(Mutation {
                    base,
                    mutaplasmid,
                    attrs: std::collections::BTreeMap::new(),
                });
            }
        },
    )?;
    fits.parts(
        conn,
        &format!(
            "SELECT p.fit_id, a.{owner}_id, a.attribute_id, a.value
             FROM {owner}_mutation_attrs a JOIN {parent} p ON p.id = a.{owner}_id"
        ),
        |r| Ok((r.get::<_, i64>(1)?, r.get(2)?, r.get(3)?)),
        |fit, (id, attr, value)| {
            if let Some(Some(m)) = add(fit, id) {
                m.attrs.insert(attr, value);
            }
        },
    )
}

fn read_modules(conn: &Connection, fits: &mut Loaded<Fit>) -> rusqlite::Result<()> {
    // Where each module row went, to hang its mutation on.
    let mut places: HashMap<i64, (Slot, usize)> = HashMap::new();
    fits.parts(
        conn,
        "SELECT fit_id, id, rack, type_id, state, charge_type_id, spool,
             rah, rah_name, rah_em, rah_thermal, rah_kinetic, rah_explosive
         FROM fit_modules ORDER BY fit_id, rack, position",
        |r| {
            let rack: Slot = code(r, 2)?;
            let module = match r.get::<_, Option<i32>>(3)? {
                None => None,
                Some(type_id) => Some(Module {
                    type_id,
                    state: code(r, 4)?,
                    charge: r.get(5)?,
                    spool: r.get(6)?,
                    mutation: None,
                    rah: rah(r, 7)?,
                }),
            };
            if !RACKS.contains(&rack) {
                return Err(unknown(2, rack.code()));
            }
            Ok((r.get::<_, i64>(1)?, rack, module))
        },
        |fit, (id, rack, module)| {
            let modules = fit.rack_mut(rack);
            places.insert(id, (rack, modules.len()));
            modules.push(module);
        },
    )?;
    read_mutations(conn, fits, "module", "fit_modules", |fit, id| {
        let &(rack, i) = places.get(&id)?;
        Some(&mut fit.rack_mut(rack).get_mut(i)?.as_mut()?.mutation)
    })
}

fn write_drones(tx: &Transaction<'_>, id: i64, drones: &[DroneStack]) -> rusqlite::Result<()> {
    let mut insert = tx.prepare_cached(
        "INSERT INTO fit_drones (fit_id, position, type_id, count, active)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (i, d) in drones.iter().enumerate() {
        insert.execute(params![id, i, d.type_id, d.count, d.active])?;
        if let Some(mutation) = &d.mutation {
            write_mutation(tx, "drone", tx.last_insert_rowid(), mutation)?;
        }
    }
    Ok(())
}

fn read_drones(conn: &Connection, fits: &mut Loaded<Fit>) -> rusqlite::Result<()> {
    let mut places: HashMap<i64, usize> = HashMap::new();
    fits.parts(
        conn,
        "SELECT fit_id, id, type_id, count, active FROM fit_drones ORDER BY fit_id, position",
        |r| {
            Ok((
                r.get::<_, i64>(1)?,
                DroneStack::new(r.get(2)?, r.get(3)?, r.get(4)?),
            ))
        },
        |fit, (id, drone)| {
            places.insert(id, fit.drones.len());
            fit.drones.push(drone);
        },
    )?;
    read_mutations(conn, fits, "drone", "fit_drones", |fit, id| {
        Some(&mut fit.drones.get_mut(*places.get(&id)?)?.mutation)
    })
}

fn write_fighters<'a>(
    tx: &Transaction<'_>,
    id: i64,
    projected: bool,
    fighters: impl Iterator<Item = (&'a Fighter, Option<f64>)>,
) -> rusqlite::Result<()> {
    let mut insert = tx.prepare_cached(
        "INSERT INTO fit_fighters (fit_id, projected, position, type_id, amount, active, range)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut ability = tx.prepare_cached(
        "INSERT INTO fighter_abilities (fighter_id, position, ability_id, enabled)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (i, (f, range)) in fighters.enumerate() {
        insert.execute(params![
            id, projected, i, f.type_id, f.amount, f.active, range
        ])?;
        let fighter = tx.last_insert_rowid();
        for (j, (a, on)) in f.abilities.iter().enumerate() {
            ability.execute(params![fighter, j, a, on])?;
        }
    }
    Ok(())
}

fn read_fighters(conn: &Connection, fits: &mut Loaded<Fit>) -> rusqlite::Result<()> {
    let mut places: HashMap<i64, (bool, usize)> = HashMap::new();
    fits.parts(
        conn,
        "SELECT fit_id, id, projected, type_id, amount, active, range
         FROM fit_fighters ORDER BY fit_id, projected, position",
        |r| {
            let fighter = Fighter {
                type_id: r.get(3)?,
                amount: r.get(4)?,
                active: r.get(5)?,
                abilities: Vec::new(),
            };
            Ok((
                r.get::<_, i64>(1)?,
                r.get::<_, bool>(2)?,
                fighter,
                r.get(6)?,
            ))
        },
        |fit, (id, projected, fighter, range)| {
            if projected {
                places.insert(id, (true, fit.projected.fighters.len()));
                fit.projected
                    .fighters
                    .push(ProjectedFighter { fighter, range });
            } else {
                places.insert(id, (false, fit.fighters.len()));
                fit.fighters.push(fighter);
            }
        },
    )?;
    fits.parts(
        conn,
        "SELECT f.fit_id, a.fighter_id, a.ability_id, a.enabled
         FROM fighter_abilities a JOIN fit_fighters f ON f.id = a.fighter_id
         ORDER BY a.fighter_id, a.position",
        |r| Ok((r.get::<_, i64>(1)?, r.get(2)?, r.get(3)?)),
        |fit, (id, ability, on)| {
            let fighter = match places.get(&id) {
                Some(&(false, i)) => fit.fighters.get_mut(i),
                Some(&(true, i)) => fit.projected.fighters.get_mut(i).map(|p| &mut p.fighter),
                None => None,
            };
            if let Some(f) = fighter {
                f.abilities.push((ability, on));
            }
        },
    )
}

fn write_boosters(tx: &Transaction<'_>, id: i64, boosters: &[Booster]) -> rusqlite::Result<()> {
    let mut insert = tx.prepare_cached(
        "INSERT INTO fit_boosters (fit_id, position, type_id, active) VALUES (?1, ?2, ?3, ?4)",
    )?;
    let mut effect = tx.prepare_cached(
        "INSERT INTO booster_side_effects (booster_id, position, effect_id) VALUES (?1, ?2, ?3)",
    )?;
    for (i, b) in boosters.iter().enumerate() {
        insert.execute(params![id, i, b.type_id, b.active])?;
        let booster = tx.last_insert_rowid();
        for (j, e) in b.side_effects.iter().enumerate() {
            effect.execute(params![booster, j, e])?;
        }
    }
    Ok(())
}

fn read_boosters(conn: &Connection, fits: &mut Loaded<Fit>) -> rusqlite::Result<()> {
    let mut places: HashMap<i64, usize> = HashMap::new();
    fits.parts(
        conn,
        "SELECT fit_id, id, type_id, active FROM fit_boosters ORDER BY fit_id, position",
        |r| {
            let mut booster = Booster::new(r.get(2)?);
            booster.active = r.get(3)?;
            Ok((r.get::<_, i64>(1)?, booster))
        },
        |fit, (id, booster)| {
            places.insert(id, fit.boosters.len());
            fit.boosters.push(booster);
        },
    )?;
    fits.parts(
        conn,
        "SELECT b.fit_id, e.booster_id, e.effect_id
         FROM booster_side_effects e JOIN fit_boosters b ON b.id = e.booster_id
         ORDER BY e.booster_id, e.position",
        |r| Ok((r.get::<_, i64>(1)?, r.get(2)?)),
        |fit, (id, effect)| {
            if let Some(b) = places.get(&id).and_then(|&i| fit.boosters.get_mut(i)) {
                b.side_effects.push(effect);
            }
        },
    )
}

fn write_projected(tx: &Transaction<'_>, id: i64, fit: &Fit) -> rusqlite::Result<()> {
    let p = &fit.projected;
    let mut fits = tx.prepare_cached(
        "INSERT INTO fit_projected_fits (fit_id, position, source_uid, amount, active, range)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (i, f) in p.fits.iter().enumerate() {
        fits.execute(params![id, i, f.fit, f.amount, f.active, f.range])?;
    }
    let mut modules = tx.prepare_cached(
        "INSERT INTO fit_projected_modules
             (fit_id, position, type_id, state, charge_type_id, spool, range)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for (i, m) in p.modules.iter().enumerate() {
        modules.execute(params![
            id,
            i,
            m.type_id,
            m.state.code(),
            m.charge,
            m.spool,
            m.range
        ])?;
    }
    let mut drones = tx.prepare_cached(
        "INSERT INTO fit_projected_drones (fit_id, position, type_id, count, active, range)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (i, d) in p.drones.iter().enumerate() {
        drones.execute(params![id, i, d.type_id, d.count, d.active, d.range])?;
    }
    let mut env = tx.prepare_cached(
        "INSERT INTO fit_projected_env (fit_id, position, type_id, active) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (i, e) in p.env.iter().enumerate() {
        env.execute(params![id, i, e.type_id, e.active])?;
    }
    Ok(())
}

fn read_projected(conn: &Connection, fits: &mut Loaded<Fit>) -> rusqlite::Result<()> {
    fits.parts(
        conn,
        "SELECT fit_id, source_uid, amount, active, range
         FROM fit_projected_fits ORDER BY fit_id, position",
        |r| {
            Ok(ProjectedFit {
                fit: r.get(1)?,
                amount: r.get(2)?,
                active: r.get(3)?,
                range: r.get(4)?,
            })
        },
        |fit, p| fit.projected.fits.push(p),
    )?;
    fits.parts(
        conn,
        "SELECT fit_id, type_id, state, charge_type_id, spool, range
         FROM fit_projected_modules ORDER BY fit_id, position",
        |r| {
            Ok(ProjectedModule {
                type_id: r.get(1)?,
                state: code(r, 2)?,
                charge: r.get(3)?,
                spool: r.get(4)?,
                range: r.get(5)?,
            })
        },
        |fit, m| fit.projected.modules.push(m),
    )?;
    fits.parts(
        conn,
        "SELECT fit_id, type_id, count, active, range
         FROM fit_projected_drones ORDER BY fit_id, position",
        |r| {
            Ok(ProjectedDrone {
                type_id: r.get(1)?,
                count: r.get(2)?,
                active: r.get(3)?,
                range: r.get(4)?,
            })
        },
        |fit, d| fit.projected.drones.push(d),
    )?;
    fits.parts(
        conn,
        "SELECT fit_id, type_id, active FROM fit_projected_env ORDER BY fit_id, position",
        |r| {
            Ok(EnvEffect {
                type_id: r.get(1)?,
                active: r.get(2)?,
            })
        },
        |fit, e| fit.projected.env.push(e),
    )
}
