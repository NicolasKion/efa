//! Fits in and out of the formats pyfa speaks (service/port/): EFT, DNA, EVE's XML, ESI's
//! fitting JSON and multibuy lists, plus the HTML page of every fit.
//!
//! Import picks the format from the text the way pyfa's `importAuto` does, so the clipboard and
//! files go through the same door.

pub mod dna;
pub mod efs;
pub mod eft;
pub mod esi;
pub mod html;
pub mod multibuy;
pub mod muta;
pub mod renames;
pub mod xml;

use serde::{Deserialize, Serialize};

use crate::{
    data::{Catalog, Kind, Slot, TypeId},
    fit::{Booster, Fit, MAX_STACK, Module, fighters::Fighter, split_count},
};

/// Fits read from text, and names in it that matched no item.
#[derive(Debug, Clone, Default)]
pub struct Imported {
    pub fits: Vec<Fit>,
    pub unknown: Vec<String>,
}

/// The formats a fit can be copied or saved as, in the order of pyfa's copy dialog.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Eft,
    MultiBuy,
    Esi,
    Dna,
    /// Eve Fleet Simulator's JSON.
    Efs,
    /// The stats pane's numbers as text, pyfa's "Stats".
    Stats,
    Xml,
}

impl Format {
    pub const ALL: [Self; 7] = [
        Self::Eft,
        Self::MultiBuy,
        Self::Esi,
        Self::Dna,
        Self::Efs,
        Self::Stats,
        Self::Xml,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Eft => "EFT",
            Self::MultiBuy => "MultiBuy",
            Self::Esi => "ESI",
            Self::Dna => "DNA",
            Self::Efs => "EFS",
            Self::Stats => "Stats",
            Self::Xml => "XML",
        }
    }

    /// File name extension when saving in this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Eft | Self::MultiBuy | Self::Dna | Self::Stats => "txt",
            Self::Esi | Self::Efs => "json",
            Self::Xml => "xml",
        }
    }
}

/// The copy dialog's last choices, kept like pyfa's "pyfaExport" setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: Format,
    pub eft: eft::Options,
    pub multibuy: multibuy::Options,
    pub dna: dna::Options,
}

/// Read fits from text in any format pyfa reads. `file_name` is the file the text came from,
/// which EFT config files need: they are named after their ship.
pub fn import(catalog: &Catalog, text: &str, file_name: Option<&str>) -> Result<Imported, String> {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or("nothing to import")?;
    if first.starts_with("<?xml") || first.starts_with("<fittings") {
        return xml::import(catalog, text);
    }
    if first.starts_with('{') {
        return esi::import(catalog, text);
    }
    if first.starts_with('[') && first.ends_with(']') {
        let header_ship = first[1..first.len() - 1]
            .split_once(',')
            .and_then(|(ship, _)| catalog.find(ship))
            .is_some_and(|t| t.kind == Kind::Ship);
        if !header_ship && let Some(ship) = file_name.and_then(config_ship) {
            return eft::import_config(catalog, ship, text);
        }
        return eft::import(catalog, text);
    }
    if let Some(imported) = dna::import(catalog, text) {
        return imported;
    }
    Err("not a fit in a format efa reads (EFT, DNA, XML, ESI)".to_owned())
}

/// The ship an EFT config file is for, from its name: "Rifter.cfg" holds Rifter fits.
fn config_ship(file_name: &str) -> Option<&str> {
    let stem = std::path::Path::new(file_name).file_stem()?.to_str()?;
    Some(stem.split('.').next().unwrap_or(stem))
}

/// Items from lines of names, optionally with amounts ("Hobgoblin II x5"), the way pyfa reads a
/// paste that is not a fit into the additions panes. None when any line is not an item.
pub fn parse_items(catalog: &Catalog, text: &str) -> Option<Vec<(TypeId, u32)>> {
    let items: Option<Vec<(TypeId, u32)>> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|line| {
            let (name, count) = split_count(line);
            catalog.find(name).map(|t| (t.id, count.unwrap_or(1)))
        })
        .collect();
    items.filter(|items| !items.is_empty())
}

/// Cargo in pyfa's export order: by category, then group, then name.
pub(crate) fn sort_cargo(catalog: &Catalog, cargo: &[(TypeId, u32)]) -> Vec<(TypeId, u32)> {
    let mut sorted = cargo.to_vec();
    sorted.sort_by_cached_key(|&(t, _)| {
        let item = catalog.get(t);
        let group = item
            .and_then(|i| catalog.group_names.get(&i.group_id))
            .cloned()
            .unwrap_or_default();
        (
            item.map_or("", |i| i.category_label()),
            group,
            catalog.name(t),
        )
    });
    sorted
}

/// Charges a module holds, the way pyfa counts them for DNA, ESI and multibuy: as many as fit in
/// its capacity, and one for scripts and other charges that take no room.
pub(crate) fn charge_count(catalog: &Catalog, module: TypeId, charge: TypeId) -> u32 {
    let capacity = catalog.get(module).map_or(0.0, |t| t.capacity);
    let volume = catalog.get(charge).map_or(0.0, |t| t.volume);
    if capacity <= 0.0 || volume <= 0.0 {
        return 1;
    }
    // A hair of tolerance so 400 / 0.0125 does not come out one short.
    ((capacity / volume + 1e-9).floor() as u32).max(1)
}

/// The inventory slot a subsystem goes in, 125 to 128, by its group as the game orders them.
pub(crate) fn subsystem_flag(catalog: &Catalog, subsystem: TypeId) -> u32 {
    match catalog.get(subsystem).map(|t| t.group_id) {
        Some(954) => 126,
        Some(956) => 127,
        Some(957) => 128,
        _ => 125,
    }
}

/// Subsystems in the order the game wants them.
pub(crate) fn sorted_subsystems(catalog: &Catalog, fit: &Fit) -> Vec<TypeId> {
    let mut subsystems = fit.subsystems.clone();
    subsystems.sort_by_key(|&s| subsystem_flag(catalog, s));
    subsystems
}

/// Where an item from a flat list (DNA, ESI, XML) goes when the format does not say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bay {
    Fitted,
    Cargo,
}

/// The most positions a rack has on any hull, so an import never makes a longer one.
pub(crate) fn max_positions(slot: Slot) -> usize {
    match slot {
        Slot::High | Slot::Mid | Slot::Low | Slot::Service => 8,
        Slot::Subsystem => 4,
        Slot::Rig => 3,
    }
}

/// Put `count` of an item where it belongs on a fit built from a flat list, the way pyfa's
/// importers do: modules into racks in order, drones and fighters into their bays, charges and
/// anything unfittable into cargo. Modules beyond what a rack can hold are dropped, as pyfa
/// drops modules that do not fit, and amounts are capped, so a hostile count cannot run away.
pub(crate) fn place(fit: &mut Fit, catalog: &Catalog, type_id: TypeId, count: u32, bay: Bay) {
    let Some(t) = catalog.get(type_id) else {
        return;
    };
    let count = count.min(MAX_STACK);
    if bay == Bay::Cargo {
        fit.add_cargo(type_id, count);
        return;
    }
    let free =
        |used: usize, slot: Slot| max_positions(slot).saturating_sub(used).min(count as usize);
    match t.kind {
        Kind::Module(slot @ (Slot::High | Slot::Mid | Slot::Low | Slot::Service)) => {
            let rack = fit.rack_mut(slot);
            for _ in 0..free(rack.len(), slot) {
                rack.push(Some(Module::new(catalog, type_id)));
            }
        }
        Kind::Module(Slot::Rig) => {
            let n = free(fit.rigs.len(), Slot::Rig);
            fit.rigs.extend(std::iter::repeat_n(Some(type_id), n));
        }
        Kind::Module(Slot::Subsystem) => {
            let n = free(fit.subsystems.len(), Slot::Subsystem);
            fit.subsystems.extend(std::iter::repeat_n(type_id, n));
        }
        Kind::Drone => fit.add_drones(type_id, count),
        // Formats without a fighter section write one squadron per entry, its size as the count.
        Kind::Fighter => fit
            .fighters
            .push(Fighter::with_size(catalog, type_id, count)),
        Kind::Implant => {
            fit.add_implant(catalog, type_id);
        }
        Kind::Booster => {
            fit.add_booster(catalog, Booster::new(type_id));
        }
        Kind::Mode => fit.mode = Some(type_id),
        Kind::Ship | Kind::Skill => {}
        Kind::Charge | Kind::Cargo | Kind::Other => fit.add_cargo(type_id, count),
    }
}

/// Amounts by key in the order keys first came, the way the exports list items. Adding never
/// overflows: amounts stop at `u32::MAX`.
#[derive(Debug, Clone)]
pub struct Tally<K>(Vec<(K, u32)>);

impl<K> Default for Tally<K> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<K: PartialEq> Tally<K> {
    pub fn add(&mut self, key: K, count: u32) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n = n.saturating_add(count),
            None => self.0.push((key, count)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_vec(self) -> Vec<(K, u32)> {
        self.0
    }
}

impl<K: PartialEq> FromIterator<(K, u32)> for Tally<K> {
    fn from_iter<I: IntoIterator<Item = (K, u32)>>(items: I) -> Self {
        let mut tally = Self::default();
        tally.extend(items);
        tally
    }
}

impl<K: PartialEq> Extend<(K, u32)> for Tally<K> {
    fn extend<I: IntoIterator<Item = (K, u32)>>(&mut self, items: I) {
        for (key, count) in items {
            self.add(key, count);
        }
    }
}

impl<K> IntoIterator for Tally<K> {
    type Item = (K, u32);
    type IntoIter = std::vec::IntoIter<(K, u32)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// A new fit on a ship, named, with the ship's first mode like every new fit.
pub(crate) fn new_fit(catalog: &Catalog, ship: TypeId, name: String) -> Fit {
    let mut fit = Fit::new(name);
    fit.ship = Some(ship);
    fit.mode = catalog.modes_for(ship).first().copied();
    fit
}

/// Text with XML's five entities escaped, for attribute values.
pub(crate) fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Every type a fit uses, ship included, for pricing.
pub fn fit_types(fit: &Fit) -> impl Iterator<Item = TypeId> + '_ {
    fit.ship
        .into_iter()
        .chain(
            fit.modules()
                .flat_map(|m| [Some(m.type_id), m.charge])
                .flatten(),
        )
        .chain(fit.rigs.iter().flatten().copied())
        .chain(fit.subsystems.iter().copied())
        .chain(fit.drones.iter().map(|d| d.type_id))
        .chain(fit.fighters.iter().map(|f| f.type_id))
        .chain(fit.implants.iter().copied())
        .chain(fit.boosters.iter().map(|b| b.type_id))
        .chain(fit.cargo.iter().map(|&(t, _)| t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tallies_keep_first_seen_order_and_never_overflow() {
        let mut tally: Tally<i32> = [(2, 1), (1, 5), (2, 3)].into_iter().collect();
        tally.add(1, u32::MAX);
        assert_eq!(tally.into_vec(), vec![(2, 4), (1, u32::MAX)]);
    }
}
