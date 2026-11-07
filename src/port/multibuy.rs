//! The game's multibuy list (service/port/multibuy.py): the ship, then every item with its
//! total amount, for buying the whole fit at once.

use serde::{Deserialize, Serialize};

use super::{charge_count, sort_cargo};
use crate::{
    data::{Catalog, TypeId},
    fit::Fit,
};

/// pyfa's multibuy options, with its defaults. pyfa's "Optimize Prices" swaps in cheaper
/// variations through its price service, which gyfa's price sources cannot answer; it is left
/// out.
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
            implants: false,
            boosters: false,
            cargo: true,
        }
    }
}

pub fn export(fit: &Fit, catalog: &Catalog, options: &Options) -> String {
    let mut amounts: Vec<(TypeId, u32)> = Vec::new();
    let mut add =
        |type_id: TypeId, count: u32| match amounts.iter_mut().find(|(t, _)| *t == type_id) {
            Some((_, n)) => *n += count,
            None => amounts.push((type_id, count)),
        };
    for m in fit.modules() {
        add(m.type_id, 1);
        if let Some(charge) = m.charge.filter(|_| options.loaded_charges) {
            add(charge, charge_count(catalog, m.type_id, charge));
        }
    }
    for &rig in fit.rigs.iter().flatten() {
        add(rig, 1);
    }
    for &subsystem in &fit.subsystems {
        add(subsystem, 1);
    }
    for d in &fit.drones {
        add(d.type_id, d.count);
    }
    for f in &fit.fighters {
        add(f.type_id, f.size(catalog));
    }
    if options.cargo {
        for &(type_id, count) in &fit.cargo {
            add(type_id, count);
        }
    }
    if options.implants {
        for &implant in &fit.implants {
            add(implant, 1);
        }
    }
    if options.boosters {
        for b in &fit.boosters {
            add(b.type_id, 1);
        }
    }
    let ship = fit.ship.map(|s| catalog.name(s));
    let lines = sort_cargo(catalog, &amounts)
        .into_iter()
        .map(|(t, n)| match n {
            1 => catalog.name(t),
            n => format!("{} x{n}", catalog.name(t)),
        });
    ship.into_iter().chain(lines).collect::<Vec<_>>().join("\n")
}
