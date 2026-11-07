//! The game's multibuy list (service/port/multibuy.py): the ship, then every item with its
//! total amount, for buying the whole fit at once.

use serde::{Deserialize, Serialize};

use super::{charge_count, sort_cargo};
use crate::{
    data::{Catalog, TypeId},
    fit::Fit,
    prices::optimize,
};

/// pyfa's multibuy options, with its defaults.
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
    /// Swap items for their cheapest replacement, as Optimize Fit Price does.
    pub optimize_prices: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            loaded_charges: true,
            implants: false,
            boosters: false,
            cargo: true,
            optimize_prices: false,
        }
    }
}

/// The items the list holds and how many of each, the ship left out, in fit order.
pub fn amounts(fit: &Fit, catalog: &Catalog, options: &Options) -> Vec<(TypeId, u32)> {
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
    amounts
}

/// The list as it is, whatever `options` says about optimizing prices.
pub fn export(fit: &Fit, catalog: &Catalog, options: &Options) -> String {
    format(fit, catalog, &amounts(fit, catalog, options))
}

/// The list with `options.optimize_prices` honoured: `price` picks the replacements.
pub fn export_priced(
    fit: &Fit,
    catalog: &Catalog,
    options: &Options,
    price: impl Fn(TypeId) -> Option<f64>,
) -> String {
    let amounts = amounts(fit, catalog, options);
    if options.optimize_prices {
        let cheaper = optimize::cheaper_amounts(catalog, &amounts, price);
        format(fit, catalog, &cheaper)
    } else {
        format(fit, catalog, &amounts)
    }
}

fn format(fit: &Fit, catalog: &Catalog, amounts: &[(TypeId, u32)]) -> String {
    let ship = fit.ship.map(|s| catalog.name(s));
    let lines = sort_cargo(catalog, amounts)
        .into_iter()
        .map(|(t, n)| match n {
            1 => catalog.name(t),
            n => format!("{} x{n}", catalog.name(t)),
        });
    ship.into_iter().chain(lines).collect::<Vec<_>>().join("\n")
}
