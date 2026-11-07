//! EFT text the way pyfa writes it (service/port/eft.py), several fits in one paste, and EFT's
//! own config files, which hold every fit of one ship.

use serde::{Deserialize, Serialize};

use super::{Imported, sort_cargo};
use crate::{
    data::{Catalog, Kind, Slot},
    fit::{DroneStack, Fit, Module, ModuleState},
};

/// pyfa's EFT export options, with its defaults. pyfa's "Mutated Attributes" has nothing to do
/// until gyfa has mutated items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent export switches, one per checkbox"
)]
pub struct Options {
    pub loaded_charges: bool,
    pub implants: bool,
    pub boosters: bool,
    pub cargo: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            loaded_charges: true,
            implants: true,
            boosters: true,
            cargo: true,
        }
    }
}

/// How many slots each rack has, so empty ones past the last module are written too, as pyfa
/// does. Without it only the fit's own positions are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RackSizes {
    pub high: usize,
    pub mid: usize,
    pub low: usize,
    pub rig: usize,
    pub service: usize,
}

/// The fit as pyfa's EFT export: racks low to high then rigs, subsystems and services one blank line
/// apart, and drones, implants and boosters, and cargo each two blank lines apart.
pub fn export(fit: &Fit, catalog: &Catalog, options: &Options, sizes: Option<RackSizes>) -> String {
    let sizes = sizes.unwrap_or_default();
    let module_line = |m: &Option<Module>, empty: &str| match m {
        None => format!("[Empty {empty} slot]"),
        Some(m) => {
            let mut line = catalog.name(m.type_id);
            if let Some(charge) = m.charge.filter(|_| options.loaded_charges) {
                line = format!("{line}, {}", catalog.name(charge));
            }
            if m.state == ModuleState::Offline {
                line.push_str(" /offline");
            }
            line
        }
    };
    let padded = |rack: &[Option<Module>], size: usize, empty: &str| -> Vec<String> {
        let mut lines: Vec<String> = rack.iter().map(|m| module_line(m, empty)).collect();
        lines.extend((rack.len()..size).map(|_| format!("[Empty {empty} slot]")));
        lines
    };
    let mut rigs: Vec<String> = fit
        .rigs
        .iter()
        .map(|r| r.map_or_else(|| "[Empty Rig slot]".to_owned(), |r| catalog.name(r)))
        .collect();
    rigs.extend((fit.rigs.len()..sizes.rig).map(|_| "[Empty Rig slot]".to_owned()));
    let racks: Vec<String> = [
        padded(&fit.low, sizes.low, "Low"),
        padded(&fit.mid, sizes.mid, "Med"),
        padded(&fit.high, sizes.high, "High"),
        rigs,
        fit.subsystems.iter().map(|&s| catalog.name(s)).collect(),
        padded(&fit.services, sizes.service, "Service"),
    ]
    .into_iter()
    .filter(|rack| !rack.is_empty())
    .map(|rack| rack.join("\n"))
    .collect();

    let mut sections = Vec::new();
    if !racks.is_empty() {
        sections.push(racks.join("\n\n"));
    }
    if !fit.drones.is_empty() {
        sections.push(stack_lines(
            catalog,
            fit.drones.iter().map(|d| (d.type_id, d.count)),
        ));
    }
    if !fit.fighters.is_empty() {
        sections.push(stack_lines(
            catalog,
            fit.fighters.iter().map(|f| {
                let size = f
                    .amount
                    .or_else(|| catalog.projection.squadron_size(f.type_id))
                    .unwrap_or(1);
                (f.type_id, size)
            }),
        ));
    }
    let mut character = Vec::new();
    if options.implants && !fit.implants.is_empty() {
        character.push(name_lines(catalog, &fit.implants));
    }
    if options.boosters && !fit.boosters.is_empty() {
        let boosters: Vec<_> = fit.boosters.iter().map(|b| b.type_id).collect();
        character.push(name_lines(catalog, &boosters));
    }
    if !character.is_empty() {
        sections.push(character.join("\n\n"));
    }
    if options.cargo && !fit.cargo.is_empty() {
        sections.push(stack_lines(catalog, sort_cargo(catalog, &fit.cargo)));
    }

    let ship = fit
        .ship
        .map_or_else(|| "No ship".to_owned(), |s| catalog.name(s));
    format!("[{ship}, {}]\n\n{}", fit.name, sections.join("\n\n\n"))
}

fn stack_lines(catalog: &Catalog, stacks: impl IntoIterator<Item = (i32, u32)>) -> String {
    stacks
        .into_iter()
        .map(|(t, n)| format!("{} x{n}", catalog.name(t)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn name_lines(catalog: &Catalog, types: &[i32]) -> String {
    types
        .iter()
        .map(|&t| catalog.name(t))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a line opens an EFT fit: "[Ship, Name]", which an empty slot stub never is.
fn is_header(line: &str) -> bool {
    line.starts_with('[')
        && line.ends_with(']')
        && line.contains(',')
        && !line.starts_with("[Empty ")
}

/// pyfa writes "/offline", the game "/OFFLINE"; the fit parser knows the latter.
fn normalize_offline(line: &str) -> String {
    let trimmed = line.trim_end();
    match trimmed.len().checked_sub(OFFLINE.len()) {
        Some(at) if trimmed.is_char_boundary(at) && trimmed[at..].eq_ignore_ascii_case(OFFLINE) => {
            format!("{}{OFFLINE}", &trimmed[..at])
        }
        _ => line.to_owned(),
    }
}

const OFFLINE: &str = "/OFFLINE";

/// Every fit in EFT text: one per "[Ship, Name]" header, so several pasted at once all come in.
pub fn import(catalog: &Catalog, text: &str) -> Result<Imported, String> {
    let mut blocks: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        if is_header(line.trim()) {
            blocks.push(Vec::new());
        }
        if let Some(block) = blocks.last_mut() {
            block.push(normalize_offline(line));
        }
    }
    if blocks.is_empty() {
        return Err("an EFT fit starts with [Ship, Name]".to_owned());
    }
    let mut imported = Imported::default();
    let mut errors = Vec::new();
    for block in blocks {
        match Fit::from_eft(catalog, &block.join("\n")) {
            Ok((fit, unknown)) => {
                imported.fits.push(fit);
                imported.unknown.extend(unknown);
            }
            Err(e) => errors.push(e),
        }
    }
    if imported.fits.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(imported)
}

/// EFT's own config file for one ship, named after it: "[Fit name]" headers followed by module
/// lines and `Drones_Active=`, `Implant_Active=`, `Booster_Active=` and `Cargohold=` entries.
pub fn import_config(catalog: &Catalog, ship_name: &str, text: &str) -> Result<Imported, String> {
    let ship = catalog
        .find(ship_name)
        .filter(|t| t.kind == Kind::Ship)
        .ok_or_else(|| format!("unknown ship \"{ship_name}\""))?;
    let mut imported = Imported::default();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let mut fit = Fit::new(name);
            fit.ship = Some(ship.id);
            fit.mode = catalog.modes_for(ship.id).first().copied();
            imported.fits.push(fit);
            continue;
        }
        let Some(fit) = imported.fits.last_mut() else {
            continue;
        };
        if line.is_empty() {
            continue;
        }
        // EFT keeps the notes on one line, with | for line breaks.
        if let Some(description) = line.strip_prefix("Description=") {
            fit.notes = description.replace('|', "\n");
            continue;
        }
        let entry = config_entry(line);
        let Some(t) = catalog.find(entry.name) else {
            imported.unknown.push(entry.name.to_owned());
            continue;
        };
        match (entry.kind, t.kind) {
            (ConfigKind::Drones { .. }, Kind::Fighter) => {
                fit.fighters.push(crate::fit::fighters::Fighter {
                    amount: Some(entry.count),
                    ..crate::fit::fighters::Fighter::new(t.id)
                });
            }
            (ConfigKind::Drones { active }, Kind::Drone) => {
                fit.drones.push(DroneStack {
                    type_id: t.id,
                    count: entry.count,
                    active: if active { entry.count } else { 0 },
                });
            }
            (ConfigKind::Cargo, _) => fit.add_cargo(t.id, entry.count),
            (ConfigKind::Implant { active }, Kind::Implant) => {
                fit.add_implant(catalog, t.id);
                if !active {
                    fit.inactive_implants.push(t.id);
                }
            }
            (ConfigKind::Booster { active }, Kind::Booster) => {
                let mut booster = crate::fit::Booster::new(t.id);
                booster.active = active;
                fit.add_booster(catalog, booster);
            }
            (ConfigKind::Module { charge }, Kind::Module(slot)) => {
                let charge = charge.and_then(|c| catalog.find(c)).map(|c| c.id);
                match slot {
                    Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                        fit.rack_mut(slot).push(Some(Module {
                            type_id: t.id,
                            state: if t.activatable {
                                ModuleState::Active
                            } else {
                                ModuleState::Online
                            },
                            charge,
                            spool: None,
                            rah: None,
                        }));
                    }
                    Slot::Rig => fit.rigs.push(Some(t.id)),
                    Slot::Subsystem => fit.subsystems.push(t.id),
                }
            }
            _ => {
                fit.add(catalog, t.id);
            }
        }
    }
    if imported.fits.is_empty() {
        return Err(format!("no fits for {ship_name} in the file"));
    }
    Ok(imported)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigKind<'a> {
    Module { charge: Option<&'a str> },
    Drones { active: bool },
    Implant { active: bool },
    Booster { active: bool },
    Cargo,
}

struct ConfigEntry<'a> {
    kind: ConfigKind<'a>,
    name: &'a str,
    count: u32,
}

/// One line of an EFT config fit.
fn config_entry(line: &str) -> ConfigEntry<'_> {
    if let Some((key, data)) = line.split_once('=') {
        let (name, count) = split_amount(data);
        let kind = match key {
            "Drones_Active" => Some(ConfigKind::Drones { active: true }),
            "Drones_Inactive" => Some(ConfigKind::Drones { active: false }),
            "Implant_Active" => Some(ConfigKind::Implant { active: true }),
            "Implant_Inactive" => Some(ConfigKind::Implant { active: false }),
            "Booster_Active" => Some(ConfigKind::Booster { active: true }),
            "Booster_Inactive" => Some(ConfigKind::Booster { active: false }),
            "Cargohold" => Some(ConfigKind::Cargo),
            _ => None,
        };
        if let Some(kind) = kind {
            return ConfigEntry { kind, name, count };
        }
    }
    let (name, charge) = match line.split_once(',') {
        Some((module, charge)) => (module.trim(), Some(charge.trim())),
        None => (line, None),
    };
    ConfigEntry {
        kind: ConfigKind::Module { charge },
        name,
        count: 1,
    }
}

/// "Hobgoblin II,5" -> ("Hobgoblin II", 5); a missing or odd amount counts as one.
fn split_amount(data: &str) -> (&str, u32) {
    match data.rsplit_once(',') {
        Some((name, n)) => match n.trim().parse() {
            Ok(n) => (name.trim(), n),
            Err(_) => (data.trim(), 1),
        },
        None => (data.trim(), 1),
    }
}
