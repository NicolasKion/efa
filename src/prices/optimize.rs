//! pyfa's Optimize Fit Price (mainFrame.py `optimizeFitPrice`, service/price.py
//! `findCheaperReplacements`, fitCommands/gui/itemsRebase.py): every item of a fit is swapped
//! for the cheapest of its replacements, the items that are the same in everything that matters
//! (see [`crate::data::Catalog::replacements`]), when that one is cheaper.

use std::collections::{BTreeMap, HashSet};

use crate::{
    data::{Catalog, TypeId},
    fit::Fit,
};

/// Every item of a fit pyfa looks at, the ship left out: modules, charges, rigs, subsystems,
/// drones, fighters, implants, boosters and cargo. Mutated items stay as they are.
pub fn fit_items(fit: &Fit) -> HashSet<TypeId> {
    fit.modules()
        .filter(|m| m.mutation.is_none())
        .map(|m| m.type_id)
        .chain(fit.modules().filter_map(|m| m.charge))
        .chain(fit.rigs.iter().flatten().copied())
        .chain(fit.subsystems.iter().copied())
        .chain(
            fit.drones
                .iter()
                .filter(|d| d.mutation.is_none())
                .map(|d| d.type_id),
        )
        .chain(fit.fighters.iter().map(|f| f.type_id))
        .chain(fit.implants.iter().copied())
        .chain(fit.boosters.iter().map(|b| b.type_id))
        .chain(fit.cargo.iter().map(|&(t, _)| t))
        .collect()
}

/// `items` and all their replacements: what needs a price before choosing.
pub fn types_to_price(
    catalog: &Catalog,
    items: impl IntoIterator<Item = TypeId>,
) -> HashSet<TypeId> {
    items
        .into_iter()
        .flat_map(|t| std::iter::once(t).chain(catalog.replacements_of(t).iter().copied()))
        .collect()
}

/// For each of `items` with a cheaper replacement, the cheapest one. No price counts as
/// endlessly expensive, as in pyfa, so an unpriced item is swapped for any priced one.
pub fn cheaper(
    catalog: &Catalog,
    items: impl IntoIterator<Item = TypeId>,
    price: impl Fn(TypeId) -> Option<f64>,
) -> BTreeMap<TypeId, TypeId> {
    let cost = |t: TypeId| price(t).filter(|&p| p > 0.0).unwrap_or(f64::INFINITY);
    items
        .into_iter()
        .filter_map(|item| {
            // Ties go to the lowest type ID, so the choice does not depend on hash order.
            let best = catalog
                .replacements_of(item)
                .iter()
                .copied()
                .min_by(|&a, &b| cost(a).total_cmp(&cost(b)).then(a.cmp(&b)))?;
            (cost(best) < cost(item)).then_some((item, best))
        })
        .collect()
}

/// Swap items for their replacements in `swap`, keeping their state, charge and amounts.
/// Cargo stacks that end up the same item are merged, as pyfa does. Returns how many items
/// changed.
pub fn rebase(fit: &mut Fit, swap: &BTreeMap<TypeId, TypeId>) -> usize {
    let mut changed = 0;
    let mut swap_id = |t: &mut TypeId| {
        if let Some(&to) = swap.get(t) {
            *t = to;
            changed += 1;
        }
    };
    for m in fit
        .high
        .iter_mut()
        .chain(&mut fit.mid)
        .chain(&mut fit.low)
        .chain(&mut fit.services)
        .flatten()
    {
        if m.mutation.is_none() {
            swap_id(&mut m.type_id);
        }
        if let Some(charge) = &mut m.charge {
            swap_id(charge);
        }
    }
    fit.rigs.iter_mut().flatten().for_each(&mut swap_id);
    fit.subsystems.iter_mut().for_each(&mut swap_id);
    for d in fit.drones.iter_mut().filter(|d| d.mutation.is_none()) {
        swap_id(&mut d.type_id);
    }
    for f in &mut fit.fighters {
        swap_id(&mut f.type_id);
    }
    for implant in &mut fit.implants {
        let was = *implant;
        swap_id(implant);
        // A switched off implant stays switched off under its new name.
        if *implant != was
            && let Some(off) = fit.inactive_implants.iter_mut().find(|i| **i == was)
        {
            *off = *implant;
        }
    }
    for b in &mut fit.boosters {
        swap_id(&mut b.type_id);
    }
    let cargo_before = fit.cargo.len();
    fit.cargo.iter_mut().for_each(|(t, _)| swap_id(t));
    if changed > 0 && cargo_before > 0 {
        fit.merge_cargo();
    }
    changed
}

/// Swap in cheaper replacements one item type at a time, skipping any swap `refused` turns
/// down (it gets the fit before and after). Returns the swaps made.
pub fn optimize(
    fit: &mut Fit,
    catalog: &Catalog,
    price: impl Fn(TypeId) -> Option<f64>,
    mut refused: impl FnMut(&Fit, &Fit) -> bool,
) -> BTreeMap<TypeId, TypeId> {
    let mut made = BTreeMap::new();
    for (from, to) in cheaper(catalog, fit_items(fit), price) {
        let mut next = fit.clone();
        if rebase(&mut next, &BTreeMap::from([(from, to)])) == 0 || refused(fit, &next) {
            continue;
        }
        *fit = next;
        made.insert(from, to);
    }
    made
}

/// What everything on a fit costs, the ship left out: each item times how many there are,
/// loaded charges included. Unpriced items count as free.
pub fn value(fit: &Fit, catalog: &Catalog, price: impl Fn(TypeId) -> Option<f64>) -> f64 {
    let each = |t: TypeId, n: u32| price(t).unwrap_or(0.0) * f64::from(n);
    let modules: f64 = fit
        .modules()
        .map(|m| {
            each(m.type_id, 1)
                + m.charge.map_or(0.0, |c| {
                    each(c, crate::port::charge_count(catalog, m.type_id, c))
                })
        })
        .sum();
    let single = fit
        .rigs
        .iter()
        .flatten()
        .chain(&fit.subsystems)
        .chain(&fit.implants)
        .copied()
        .chain(fit.boosters.iter().map(|b| b.type_id))
        .map(|t| each(t, 1))
        .sum::<f64>();
    let stacks = fit
        .drones
        .iter()
        .map(|d| (d.type_id, d.count))
        .chain(
            fit.fighters
                .iter()
                .map(|f| (f.type_id, f.amount.unwrap_or(1))),
        )
        .chain(fit.cargo.iter().copied())
        .map(|(t, n)| each(t, n))
        .sum::<f64>();
    modules + single + stacks
}

/// Cheaper replacements for a list of `(type, amount)`, merged like pyfa's multibuy export
/// does, in the order the items first appear.
pub fn swap_amounts(
    amounts: &[(TypeId, u32)],
    swap: &BTreeMap<TypeId, TypeId>,
) -> Vec<(TypeId, u32)> {
    let mut out: Vec<(TypeId, u32)> = Vec::with_capacity(amounts.len());
    for &(t, n) in amounts {
        let t = swap.get(&t).copied().unwrap_or(t);
        match out.iter_mut().find(|(o, _)| *o == t) {
            Some((_, m)) => *m += n,
            None => out.push((t, n)),
        }
    }
    out
}

/// Every type of `amounts` mapped to its cheaper replacement, where it has one.
pub fn cheaper_amounts(
    catalog: &Catalog,
    amounts: &[(TypeId, u32)],
    price: impl Fn(TypeId) -> Option<f64>,
) -> Vec<(TypeId, u32)> {
    let swap = cheaper(catalog, amounts.iter().map(|&(t, _)| t), price);
    swap_amounts(amounts, &swap)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::fit::{DroneStack, Module, ModuleState};

    fn catalog() -> Catalog {
        // 1, 2 and 3 are the same item; so are 10 and 11.
        Catalog {
            replacements: HashMap::from([
                (1, vec![2, 3]),
                (2, vec![1, 3]),
                (3, vec![1, 2]),
                (10, vec![11]),
                (11, vec![10]),
            ]),
            ..Catalog::default()
        }
    }

    /// 11 has no price.
    fn prices(t: TypeId) -> Option<f64> {
        match t {
            1 => Some(100.0),
            2 => Some(50.0),
            3 => Some(70.0),
            10 => Some(5.0),
            _ => None,
        }
    }

    #[test]
    fn picks_the_cheapest_and_keeps_the_cheap() {
        let swap = cheaper(&catalog(), [1, 3, 10], prices);
        assert_eq!(swap, BTreeMap::from([(1, 2), (3, 2)]));
    }

    #[test]
    fn rebases_everything_and_merges_cargo() {
        let module = Module {
            type_id: 1,
            state: ModuleState::Active,
            charge: Some(1),
            spool: None,
            mutation: None,
            rah: None,
        };
        let mut fit = Fit {
            high: vec![Some(module), None],
            drones: vec![DroneStack::new(3, 2, 1)],
            cargo: vec![(1, 5), (2, 3)],
            implants: vec![3],
            inactive_implants: vec![3],
            ..Fit::default()
        };
        let swap = BTreeMap::from([(1, 2), (3, 2)]);
        assert_eq!(rebase(&mut fit, &swap), 5);
        let m = fit.high[0].as_ref().expect("still fitted");
        assert_eq!(
            (m.type_id, m.charge, m.state),
            (2, Some(2), ModuleState::Active)
        );
        assert_eq!(fit.drones[0].type_id, 2);
        assert_eq!(fit.drones[0].active, 1);
        assert_eq!(fit.cargo, vec![(2, 8)]);
        assert_eq!(fit.inactive_implants, vec![2]);
    }

    #[test]
    fn refused_swaps_are_left_out() {
        let mut fit = Fit {
            cargo: vec![(1, 1), (3, 1)],
            ..Fit::default()
        };
        // Refuse anything that gets rid of type 3.
        let made = optimize(&mut fit, &catalog(), prices, |_, after| {
            !after.cargo.iter().any(|&(t, _)| t == 3)
        });
        assert_eq!(made, BTreeMap::from([(1, 2)]));
        assert_eq!(fit.cargo, vec![(2, 1), (3, 1)]);
    }

    #[test]
    fn multibuy_amounts_merge() {
        let amounts = [(1, 2), (2, 1), (10, 4)];
        assert_eq!(
            cheaper_amounts(&catalog(), &amounts, prices),
            vec![(2, 3), (10, 4)]
        );
    }
}
