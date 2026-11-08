//! The tables, as schema versions in order. The database's `user_version` counts those applied.
//!
//! Enums are kept as the TEXT codes in the CHECK constraints, which [`super::codes`] maps to
//! and from. Lists keep their order in a `position` column; racks keep their empty slots as
//! rows without a type. A fit's parts hang off it by `fit_id` and go with it on delete.

pub const MIGRATIONS: &[&str] = &[r"
CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);

CREATE TABLE fits (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    ship_type_id INTEGER,
    -- What other fits project or boost by; NULL until something needs it.
    uid INTEGER,
    character TEXT NOT NULL CHECK (character IN ('all0', 'all5', 'user')),
    -- A user character's id. No foreign key: a deleted character's fits fall back to All 5.
    character_id INTEGER,
    implant_source TEXT NOT NULL CHECK (implant_source IN ('fit', 'character')),
    mode_type_id INTEGER,
    sec_status REAL,
    system_security TEXT NOT NULL
        CHECK (system_security IN ('high', 'low', 'null', 'wspace')),
    damage_profile TEXT NOT NULL
        CHECK (damage_profile IN ('uniform', 'selected_ammo', 'pattern')),
    damage_name TEXT,
    damage_em REAL,
    damage_thermal REAL,
    damage_kinetic REAL,
    damage_explosive REAL,
    ignore_restrictions INTEGER NOT NULL,
    notes TEXT NOT NULL,
    -- Seconds since the epoch; NULL for fits from before efa kept it.
    modified INTEGER,
    CHECK ((character = 'user') = (character_id IS NOT NULL)),
    CHECK ((damage_profile = 'pattern') = (damage_name IS NOT NULL))
);
CREATE INDEX fits_position ON fits (position);
CREATE INDEX fits_ship ON fits (ship_type_id);
CREATE INDEX fits_uid ON fits (uid);
CREATE INDEX fits_character ON fits (character_id);

-- High, mid, low and service racks, a row per slot; an empty slot has no type.
CREATE TABLE fit_modules (
    id INTEGER PRIMARY KEY,
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    rack TEXT NOT NULL CHECK (rack IN ('high', 'mid', 'low', 'service')),
    position INTEGER NOT NULL,
    type_id INTEGER,
    state TEXT CHECK (state IN ('offline', 'online', 'active', 'overload')),
    charge_type_id INTEGER,
    -- Spool cycles; NULL is fully spooled.
    spool INTEGER,
    -- A reactive armor hardener's own pattern; NULL takes the fit's.
    rah TEXT CHECK (rah IN ('do_not_adapt', 'uniform', 'selected_ammo', 'pattern')),
    rah_name TEXT,
    rah_em REAL,
    rah_thermal REAL,
    rah_kinetic REAL,
    rah_explosive REAL,
    UNIQUE (fit_id, rack, position),
    CHECK ((type_id IS NULL) = (state IS NULL)),
    CHECK (type_id IS NOT NULL OR (charge_type_id IS NULL AND spool IS NULL AND rah IS NULL)),
    CHECK ((rah IS 'pattern') = (rah_name IS NOT NULL))
);
CREATE INDEX fit_modules_type ON fit_modules (type_id);
CREATE INDEX fit_modules_charge ON fit_modules (charge_type_id);

-- What a mutaplasmid made of a module; the module's type is then the abyssal one.
CREATE TABLE module_mutations (
    module_id INTEGER PRIMARY KEY REFERENCES fit_modules (id) ON DELETE CASCADE,
    base_type_id INTEGER NOT NULL,
    mutaplasmid_type_id INTEGER NOT NULL
);
CREATE INDEX module_mutations_base ON module_mutations (base_type_id);
CREATE TABLE module_mutation_attrs (
    module_id INTEGER NOT NULL REFERENCES module_mutations (module_id) ON DELETE CASCADE,
    attribute_id INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (module_id, attribute_id)
);

CREATE TABLE fit_rigs (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER,
    PRIMARY KEY (fit_id, position)
);
CREATE INDEX fit_rigs_type ON fit_rigs (type_id);

CREATE TABLE fit_subsystems (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    PRIMARY KEY (fit_id, position)
);
CREATE INDEX fit_subsystems_type ON fit_subsystems (type_id);

CREATE TABLE fit_drones (
    id INTEGER PRIMARY KEY,
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    count INTEGER NOT NULL,
    active INTEGER NOT NULL,
    UNIQUE (fit_id, position)
);
CREATE INDEX fit_drones_type ON fit_drones (type_id);
CREATE TABLE drone_mutations (
    drone_id INTEGER PRIMARY KEY REFERENCES fit_drones (id) ON DELETE CASCADE,
    base_type_id INTEGER NOT NULL,
    mutaplasmid_type_id INTEGER NOT NULL
);
CREATE TABLE drone_mutation_attrs (
    drone_id INTEGER NOT NULL REFERENCES drone_mutations (drone_id) ON DELETE CASCADE,
    attribute_id INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (drone_id, attribute_id)
);

-- The fit's own squadrons, and squadrons projected onto it (with a range).
CREATE TABLE fit_fighters (
    id INTEGER PRIMARY KEY,
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    projected INTEGER NOT NULL,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    -- Fighters in the squadron; NULL is full size.
    amount INTEGER,
    active INTEGER NOT NULL,
    range REAL,
    UNIQUE (fit_id, projected, position),
    CHECK (projected OR range IS NULL)
);
CREATE INDEX fit_fighters_type ON fit_fighters (type_id);
-- Abilities switched away from their defaults.
CREATE TABLE fighter_abilities (
    fighter_id INTEGER NOT NULL REFERENCES fit_fighters (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    ability_id INTEGER NOT NULL,
    enabled INTEGER NOT NULL,
    PRIMARY KEY (fighter_id, position)
);

CREATE TABLE fit_implants (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    active INTEGER NOT NULL,
    PRIMARY KEY (fit_id, position)
);
CREATE INDEX fit_implants_type ON fit_implants (type_id);

CREATE TABLE fit_boosters (
    id INTEGER PRIMARY KEY,
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    active INTEGER NOT NULL,
    UNIQUE (fit_id, position)
);
CREATE INDEX fit_boosters_type ON fit_boosters (type_id);
-- Side effects switched on, by effect id.
CREATE TABLE booster_side_effects (
    booster_id INTEGER NOT NULL REFERENCES fit_boosters (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    effect_id INTEGER NOT NULL,
    PRIMARY KEY (booster_id, position)
);

CREATE TABLE fit_cargo (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    PRIMARY KEY (fit_id, position)
);
CREATE INDEX fit_cargo_type ON fit_cargo (type_id);

-- The target profile the fit's firepower is shown against: a copy, not a reference.
CREATE TABLE fit_target_profiles (
    fit_id INTEGER PRIMARY KEY REFERENCES fits (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    em REAL NOT NULL,
    thermal REAL NOT NULL,
    kinetic REAL NOT NULL,
    explosive REAL NOT NULL,
    max_velocity REAL,
    signature_radius REAL,
    radius REAL,
    hp REAL
);

-- Other fits projecting onto this one, by their uid.
CREATE TABLE fit_projected_fits (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    source_uid INTEGER NOT NULL,
    amount INTEGER NOT NULL,
    active INTEGER NOT NULL,
    range REAL,
    PRIMARY KEY (fit_id, position)
);
CREATE INDEX fit_projected_fits_source ON fit_projected_fits (source_uid);

CREATE TABLE fit_projected_modules (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('offline', 'online', 'active', 'overload')),
    charge_type_id INTEGER,
    spool INTEGER,
    range REAL,
    PRIMARY KEY (fit_id, position)
);

CREATE TABLE fit_projected_drones (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    count INTEGER NOT NULL,
    active INTEGER NOT NULL,
    range REAL,
    PRIMARY KEY (fit_id, position)
);

-- Wormhole, incursion and other system effects.
CREATE TABLE fit_projected_env (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    active INTEGER NOT NULL,
    PRIMARY KEY (fit_id, position)
);

-- Fleet boosters, by the boosting fit's uid.
CREATE TABLE fit_command (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    booster_uid INTEGER NOT NULL,
    active INTEGER NOT NULL,
    PRIMARY KEY (fit_id, position)
);
CREATE INDEX fit_command_booster ON fit_command (booster_uid);

-- Racks whose modules of one type are shown and edited as one.
CREATE TABLE fit_grouped_racks (
    fit_id INTEGER NOT NULL REFERENCES fits (id) ON DELETE CASCADE,
    rack TEXT NOT NULL
        CHECK (rack IN ('high', 'mid', 'low', 'rig', 'subsystem', 'service')),
    PRIMARY KEY (fit_id, rack)
);

CREATE TABLE implant_sets (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL
);
CREATE TABLE implant_set_implants (
    set_id INTEGER NOT NULL REFERENCES implant_sets (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    PRIMARY KEY (set_id, position)
);

CREATE TABLE damage_patterns (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    em REAL NOT NULL,
    thermal REAL NOT NULL,
    kinetic REAL NOT NULL,
    explosive REAL NOT NULL
);

CREATE TABLE target_profiles (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    em REAL NOT NULL,
    thermal REAL NOT NULL,
    kinetic REAL NOT NULL,
    explosive REAL NOT NULL,
    max_velocity REAL,
    signature_radius REAL,
    radius REAL,
    hp REAL
);

CREATE TABLE cargo_presets (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    policy TEXT NOT NULL CHECK (policy IN ('top_up', 'add_on_top', 'exact'))
);
CREATE TABLE cargo_preset_items (
    preset_id INTEGER NOT NULL REFERENCES cargo_presets (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    PRIMARY KEY (preset_id, position)
);

-- pyfa's attribute overrides; whether they are on is a setting.
CREATE TABLE attribute_overrides (
    type_id INTEGER NOT NULL,
    attribute_id INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (type_id, attribute_id)
);

-- The user's characters, by their own id (a fit's character_id).
CREATE TABLE characters (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    sec_status REAL NOT NULL,
    -- The EVE character skills are read from.
    esi_character_id INTEGER,
    -- The Alpha clone grade; NULL for Omega.
    alpha_clone INTEGER
);
CREATE TABLE character_skills (
    character_id INTEGER NOT NULL REFERENCES characters (id) ON DELETE CASCADE,
    skill_id INTEGER NOT NULL,
    level INTEGER NOT NULL,
    PRIMARY KEY (character_id, skill_id)
);
CREATE TABLE character_implants (
    character_id INTEGER NOT NULL REFERENCES characters (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    type_id INTEGER NOT NULL,
    PRIMARY KEY (character_id, position)
);

-- EVE SSO logins, by EVE character id. Access tokens are never written.
CREATE TABLE esi_accounts (
    id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    refresh_token TEXT NOT NULL
);
CREATE TABLE esi_account_scopes (
    account_id INTEGER NOT NULL REFERENCES esi_accounts (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    scope TEXT NOT NULL,
    PRIMARY KEY (account_id, position)
);
"];
