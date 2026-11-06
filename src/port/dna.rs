//! Ship DNA (service/port/dna.py): `ship:item;count:item;count::`, as the game's fitting links
//! carry it. Also reads the chat link around it and the "DNA:ship:item*count" variant.

use serde::{Deserialize, Serialize};

use super::{Bay, Imported, charge_count, new_fit, place, sorted_subsystems};
use crate::{
    data::{Catalog, Kind, TypeId},
    fit::Fit,
};

/// pyfa's DNA export option, with its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Wrap the DNA in a chat link named after the fit, for pasting into the game.
    pub formatting: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { formatting: true }
    }
}

/// The fit as DNA: subsystems, modules by type with counts, drones, then loaded charges and
/// charges in cargo. Cargo that is not a charge stays out, since the game would try to fit it.
pub fn export(fit: &Fit, catalog: &Catalog, options: &Options) -> String {
    let Some(ship) = fit.ship else {
        return String::new();
    };
    let mut parts = vec![ship.to_string()];
    for subsystem in sorted_subsystems(catalog, fit) {
        parts.push(format!("{subsystem};1"));
    }
    let mut modules: Vec<(TypeId, u32)> = Vec::new();
    let mut charges: Vec<(TypeId, u32)> = Vec::new();
    let fitted = fit
        .modules()
        .map(|m| (m.type_id, m.charge))
        .chain(fit.rigs.iter().flatten().map(|&r| (r, None)));
    for (type_id, charge) in fitted {
        bump(&mut modules, type_id, 1);
        if let Some(charge) = charge {
            bump(&mut charges, charge, charge_count(catalog, type_id, charge));
        }
    }
    for &(type_id, count) in &fit.cargo {
        if catalog.get(type_id).is_some_and(|t| t.kind == Kind::Charge) {
            bump(&mut charges, type_id, count);
        }
    }
    let drones = fit.drones.iter().map(|d| (d.type_id, d.count));
    for (type_id, count) in modules.into_iter().chain(drones).chain(charges) {
        parts.push(format!("{type_id};{count}"));
    }
    let dna = format!("{}::", parts.join(":"));
    if options.formatting {
        format!("<url=fitting:{dna}>{}</url>", fit.name)
    } else {
        dna
    }
}

/// Add to an amount kept in first-seen order, which the export keeps.
fn bump(list: &mut Vec<(TypeId, u32)>, type_id: TypeId, count: u32) {
    match list.iter_mut().find(|(t, _)| *t == type_id) {
        Some((_, n)) => *n += count,
        None => list.push((type_id, count)),
    }
}

/// Read DNA in any of its forms, or None when the text is not DNA.
pub fn import(catalog: &Catalog, text: &str) -> Option<Result<Imported, String>> {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    if let Some(rest) = first.split_once("<url=fitting:").map(|(_, r)| r) {
        let (dna, rest) = rest.split_once('>')?;
        let name = rest.split_once("</url>").map(|(n, _)| n.to_owned());
        return Some(read(catalog, dna, ';', name));
    }
    if let Some(at) = first.find("DNA:") {
        return Some(read(catalog, &first[at + 4..], '*', None));
    }
    let looks_like_dna = first.contains("::")
        && first
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ':' | ';'));
    looks_like_dna.then(|| read(catalog, first, ';', None))
}

/// Build the fit from "ship:item<sep>count:...". The ship is the first ID that is one, as pyfa
/// looks for it.
fn read(
    catalog: &Catalog,
    dna: &str,
    separator: char,
    name: Option<String>,
) -> Result<Imported, String> {
    let dna = dna.split("::").next().unwrap_or(dna);
    let mut entries = dna.split(':').filter(|e| !e.is_empty()).map(|e| {
        let (id, count) = e.split_once(separator).unwrap_or((e, "1"));
        (
            id.trim().parse::<TypeId>(),
            count.trim().parse::<u32>().unwrap_or(1),
        )
    });
    let ship = entries
        .by_ref()
        .find_map(|(id, _)| {
            id.ok()
                .filter(|&id| catalog.get(id).is_some_and(|t| t.kind == Kind::Ship))
        })
        .ok_or("no ship in the DNA")?;
    let name = name.unwrap_or_else(|| format!("{} - DNA Imported", catalog.name(ship)));
    let mut fit = new_fit(catalog, ship, name);
    let mut unknown = Vec::new();
    for (id, count) in entries {
        match id {
            Ok(id) if catalog.get(id).is_some() => place(&mut fit, catalog, id, count, Bay::Fitted),
            Ok(id) => unknown.push(format!("#{id}")),
            Err(_) => {}
        }
    }
    Ok(Imported {
        fits: vec![fit],
        unknown,
    })
}
