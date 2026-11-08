//! The game's multibuy list (service/port/multibuy.py): the ship, then every item with its
//! total amount, for buying the whole fit at once.

use serde::{Deserialize, Serialize};

use super::{Tally, sort_cargo};
use crate::{
    data::{Catalog, TypeId},
    fit::{Fit, Place},
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
    fit.items()
        .filter(|i| match i.place {
            Place::Ship => false,
            Place::Charge { .. } => options.loaded_charges,
            Place::Cargo => options.cargo,
            Place::Implant => options.implants,
            Place::Booster => options.boosters,
            Place::Module | Place::Rig | Place::Subsystem | Place::Drone | Place::Fighter => true,
        })
        .map(|i| (i.type_id, i.count(catalog)))
        .collect::<Tally<TypeId>>()
        .into_vec()
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
