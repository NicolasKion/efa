//! A module's charges as pyfa's Charge menu orders them, which the market lists the same way.

use crate::data::{Catalog, Hardpoint, ItemType, TypeId};

/// A module's charges, structured the way pyfa's Charge menu shows them.
pub(crate) enum ChargeMenu {
    /// Families of the same charge in different grades, from long range to short.
    Turret(Vec<Vec<TypeId>>),
    /// By damage type, each from less damage to more.
    Missile(Vec<(&'static str, Vec<TypeId>)>),
    Flat(Vec<TypeId>),
}

pub(crate) fn structure_charges(
    catalog: &Catalog,
    module: &ItemType,
    charges: &[TypeId],
) -> ChargeMenu {
    let charges: Vec<&ItemType> = charges.iter().filter_map(|&c| catalog.get(c)).collect();
    let damage = |c: &ItemType| c.damage.iter().sum::<f64>();
    let name_base = |c: &ItemType| {
        let words: Vec<&str> = c.name.split_whitespace().collect();
        words[words.len().saturating_sub(2)..].join(" ")
    };
    match module.hardpoint {
        Hardpoint::Turret if !module.mining => {
            let range = |c: &ItemType| {
                module.optimal.unwrap_or(0.0) * c.range_multiplier.unwrap_or(1.0)
                    + module.falloff.unwrap_or(0.0) * c.falloff_multiplier.unwrap_or(1.0)
            };
            let mut sorted: Vec<&ItemType> = charges
                .into_iter()
                .filter(|c| !c.name.to_lowercase().contains("civilian"))
                .collect();
            // Longest range first, then by name family and damage; the keys are worked out once
            // per charge rather than on every comparison.
            sorted.sort_by_cached_key(|c| {
                (
                    std::cmp::Reverse(Total(range(c))),
                    name_base(c),
                    Total(damage(c)),
                    c.name.clone(),
                )
            });
            let mut families: Vec<Vec<TypeId>> = Vec::new();
            let mut previous: Option<(String, Option<f64>)> = None;
            for c in sorted {
                let key = (name_base(c), c.range_multiplier);
                match families.last_mut() {
                    Some(last) if previous.as_ref() == Some(&key) => last.push(c.id),
                    _ => families.push(vec![c.id]),
                }
                previous = Some(key);
            }
            ChargeMenu::Turret(families)
        }
        Hardpoint::Launcher => {
            const TYPES: [&str; 4] = ["EM", "Thermal", "Kinetic", "Explosive"];
            // A charge dealing one damage type only files under it; anything else is mixed.
            let damage_type = |c: &ItemType| {
                let total = damage(c);
                c.damage
                    .iter()
                    .position(|&d| d > 0.0 && (d - total).abs() < 1e-9)
                    .unwrap_or(TYPES.len())
            };
            let mut sorted = charges;
            sorted.sort_by(|a, b| {
                damage_type(a)
                    .cmp(&damage_type(b))
                    .then_with(|| damage(a).total_cmp(&damage(b)))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let mut by_type: Vec<(&'static str, Vec<TypeId>)> = Vec::new();
            for c in sorted {
                let label = TYPES.get(damage_type(c)).copied().unwrap_or("Mixed");
                match by_type.last_mut() {
                    Some((l, members)) if *l == label => members.push(c.id),
                    _ => by_type.push((label, vec![c.id])),
                }
            }
            ChargeMenu::Missile(by_type)
        }
        Hardpoint::Turret | Hardpoint::None => {
            ChargeMenu::Flat(charges.iter().map(|c| c.id).collect())
        }
    }
}

/// An `f64` ordered by `total_cmp`, for sort keys.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Total(f64);

impl Eq for Total {}

impl PartialOrd for Total {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Total {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// Published charges that fit a module type, by name. Only the module's charge groups are
/// looked through.
pub(crate) fn compatible_charges(catalog: &Catalog, module: TypeId) -> Vec<TypeId> {
    let Some(m) = catalog.get(module).filter(|m| !m.charge_groups.is_empty()) else {
        return Vec::new();
    };
    let mut charges: Vec<&ItemType> = m
        .charge_groups
        .iter()
        .flat_map(|&group| catalog.charges_in_group(group))
        .filter_map(|&c| catalog.get(c))
        .filter(|c| c.fits_in(m))
        .collect();
    charges.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    charges.dedup_by_key(|c| c.id);
    charges.into_iter().map(|c| c.id).collect()
}
