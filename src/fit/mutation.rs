//! Mutated modules and drones: a base item rolled by a mutaplasmid into an abyssal one, the way
//! pyfa keeps them (eos/saveddata/mutatedMixin.py, mutator.py).
//!
//! The fitted item's type is the abyssal type, so names, icons and prices follow it; the base
//! type and the rolled values live here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::data::{Catalog, TypeId, muta::short_name};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mutation {
    /// The item that was mutated, e.g. Stasis Webifier II.
    pub base: TypeId,
    pub mutaplasmid: TypeId,
    /// Rolled attribute values by attribute ID, as absolute values like pyfa stores them.
    pub attrs: BTreeMap<i32, f64>,
}

impl Mutation {
    /// `base` freshly mutated by `mutaplasmid`, every attribute at its unmutated value as pyfa
    /// starts them, with the abyssal type it becomes. None when the mutaplasmid does not take it.
    pub fn new(catalog: &Catalog, base: TypeId, mutaplasmid: TypeId) -> Option<(TypeId, Self)> {
        let mutas = &catalog.mutaplasmids;
        let result = mutas.result(mutaplasmid, base)?;
        let attrs = mutas
            .get(mutaplasmid)?
            .ranges
            .iter()
            .filter_map(|&(attr, _)| Some((attr, mutas.base_value(base, attr)?)))
            .collect();
        Some((
            result,
            Self {
                base,
                mutaplasmid,
                attrs,
            },
        ))
    }

    /// `base` mutated with these rolled values, as an import gives them; values out of range
    /// are pulled back in and attributes it does not name start unmutated.
    pub fn with_values(
        catalog: &Catalog,
        base: TypeId,
        mutaplasmid: TypeId,
        values: impl IntoIterator<Item = (i32, f64)>,
    ) -> Option<(TypeId, Self)> {
        let (result, mut mutation) = Self::new(catalog, base, mutaplasmid)?;
        for (attr, value) in values {
            mutation.set(catalog, attr, value);
        }
        Some((result, mutation))
    }

    /// Roll `attr` to `value`, kept within the mutaplasmid's range. Returns the value set, or
    /// None when the mutaplasmid does not change that attribute.
    pub fn set(&mut self, catalog: &Catalog, attr: i32, value: f64) -> Option<f64> {
        let limits = catalog
            .mutaplasmids
            .limits(self.mutaplasmid, self.base, attr)?;
        let value = limits.clamp(value);
        self.attrs.insert(attr, value);
        Some(value)
    }

    /// pyfa's `fullName`: the mutaplasmid's short name before the base item's, such as
    /// "Unstable Stasis Webifier II", where the short name says more than the full one.
    pub fn full_name(&self, catalog: &Catalog, type_id: TypeId) -> String {
        let muta = catalog.name(self.mutaplasmid);
        let short = short_name(&muta);
        if short == muta {
            catalog.name(type_id)
        } else {
            format!("{short} {}", catalog.name(self.base))
        }
    }

    /// The attributes as the muta text format writes them: by name, alphabetically.
    pub fn render_attrs(&self, catalog: &Catalog) -> String {
        let mut named: Vec<(String, f64)> = self
            .attrs
            .iter()
            .filter_map(|(attr, &value)| {
                let name = catalog.mutaplasmids.attrs.get(attr)?.name.clone();
                Some((name, value))
            })
            .collect();
        named.sort_by(|a, b| a.0.cmp(&b.0));
        named
            .into_iter()
            .map(|(name, value)| format!("{name} {}", float_unerr(value)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The name a fitted item shows: the abyssal type's, or with pyfa's "Include more information in
/// names of mutated items" the mutaplasmid's grade and the base item's name.
pub fn display_name(
    catalog: &Catalog,
    type_id: TypeId,
    mutation: Option<&Mutation>,
    expanded: bool,
) -> String {
    match mutation {
        Some(m) if expanded => m.full_name(catalog, type_id),
        _ => catalog.name(type_id),
    }
}

/// pyfa's `floatUnerr`: the value rounded to seven significant digits to drop float noise,
/// printed the way Python prints a float.
pub fn float_unerr(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return python_float(value);
    }
    let digits = 7 - value.abs().log10().ceil() as i32;
    let scale = 10f64.powi(digits.abs());
    let rounded = if digits >= 0 {
        (value * scale).round() / scale
    } else {
        (value / scale).round() * scale
    };
    python_float(rounded)
}

fn python_float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_like_pyfa() {
        assert_eq!(float_unerr(1.0), "1.0");
        assert_eq!(float_unerr(-52.5), "-52.5");
        assert_eq!(float_unerr(0.1 + 0.2), "0.3");
        assert_eq!(float_unerr(12_345.678_9), "12345.68");
        assert_eq!(float_unerr(0.931_234_567_8), "0.9312346");
    }
}
