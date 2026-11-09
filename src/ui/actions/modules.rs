//! Actions on the fitted modules, rigs and subsystems.

use std::collections::BTreeSet;

use super::{Action, EditEnv, MutaTarget, Outcome, RowRef};
use crate::{
    data::{Slot, TypeId},
    fit::{Fit, Module, ModuleState, trim_rack},
    prices::optimize,
};

/// Make a module action on `fit`: states, charges, racks, mutations and the moves between
/// slots and cargo.
pub(super) fn apply(fit: &mut Fit, action: Action, env: &EditEnv) -> Outcome {
    let EditEnv {
        loaded,
        pilot,
        stats,
        prices,
        ..
    } = *env;
    let free = |slot: Slot, used: usize| {
        let total = stats.map_or(0, |s| s.slots(slot).total.unwrap_or(0)) as usize;
        total.saturating_sub(used)
    };
    let mut out = Outcome::default();
    match action {
        Action::CycleState {
            main: (slot, i),
            others,
            offline,
        } => {
            let group = fit.group(slot, i);
            let Some(Some(m)) = fit.rack_mut(slot).get_mut(i) else {
                return Outcome::skipped();
            };
            let highest = highest_state(&loaded.catalog, m.type_id);
            let next = if offline {
                ModuleState::Offline
            } else {
                match m.state {
                    ModuleState::Offline => ModuleState::Online,
                    ModuleState::Online if highest >= ModuleState::Active => ModuleState::Active,
                    ModuleState::Active if highest >= ModuleState::Overload => {
                        ModuleState::Overload
                    }
                    _ => ModuleState::Offline,
                }
            };
            let rows: Vec<RowRef> = group
                .into_iter()
                .map(|j| RowRef::Module(slot, j))
                .chain(others)
                .collect();
            for m in modules_mut(fit, &rows) {
                m.state = next.min(highest_state(&loaded.catalog, m.type_id));
            }
        }
        Action::ToggleRigs(clicked, rows) => fit.toggle_rigs(clicked, &rows),
        Action::ToggleGrouped(slot) => {
            if !fit.grouped.remove(&slot) {
                fit.grouped.insert(slot);
            }
        }
        Action::SetCharge(rows, charge) => {
            for m in modules_mut(fit, &rows) {
                let fits = charge.is_none_or(|c| {
                    let (c, m) = (loaded.catalog.get(c), loaded.catalog.get(m.type_id));
                    c.zip(m).is_some_and(|(c, m)| c.fits_in(m))
                });
                if fits {
                    m.charge = charge;
                }
            }
        }
        Action::SetSpool(rows, cycles) => {
            for m in modules_mut(fit, &rows) {
                m.spool = cycles;
            }
        }
        Action::SetRah(rows, pattern) => {
            for m in modules_mut(fit, &rows) {
                m.rah.clone_from(&pattern);
            }
        }
        Action::Remove(rows) => {
            // Modules and rigs leave their position empty; subsystems have no positions, so
            // remove from the end first to keep earlier indices valid.
            let rows: BTreeSet<RowRef> = rows.into_iter().collect();
            for row in rows.into_iter().rev() {
                match row {
                    RowRef::Module(slot, i) => {
                        if let Some(position) = fit.rack_mut(slot).get_mut(i) {
                            *position = None;
                        }
                    }
                    RowRef::Rig(i) => {
                        if let Some(position) = fit.rigs.get_mut(i) {
                            *position = None;
                        }
                    }
                    RowRef::Subsystem(i) if i < fit.subsystems.len() => {
                        fit.subsystems.remove(i);
                    }
                    RowRef::Subsystem(_) => {}
                }
            }
            for slot in [Slot::High, Slot::Mid, Slot::Low] {
                trim_rack(fit.rack_mut(slot));
            }
            trim_rack(&mut fit.rigs);
            out.structural = true;
        }
        Action::Place(slot, i, type_id) => {
            if loaded.catalog.get(type_id).is_none() {
                return Outcome::skipped();
            }
            match slot {
                Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                    let rack = fit.rack_mut(slot);
                    if rack.len() <= i {
                        rack.resize_with(i + 1, || None);
                    }
                    rack[i] = Some(Module::new(&loaded.catalog, type_id));
                }
                Slot::Rig => {
                    if fit.rigs.len() <= i {
                        fit.rigs.resize(i + 1, None);
                    }
                    fit.rigs[i] = Some(type_id);
                }
                Slot::Subsystem => {
                    fit.add(&loaded.catalog, type_id);
                }
            }
        }
        Action::Swap(a, b) => {
            match (a, b) {
                (RowRef::Module(slot_a, i), RowRef::Module(slot_b, j)) if slot_a == slot_b => {
                    let rack = fit.rack_mut(slot_a);
                    if rack.len() <= i.max(j) {
                        rack.resize_with(i.max(j) + 1, || None);
                    }
                    rack.swap(i, j);
                    trim_rack(rack);
                }
                (RowRef::Rig(i), RowRef::Rig(j)) => {
                    if fit.rigs.len() <= i.max(j) {
                        fit.rigs.resize(i.max(j) + 1, None);
                    }
                    let on = (fit.rig_active(i), fit.rig_active(j));
                    fit.rigs.swap(i, j);
                    fit.set_rig_active(i, on.1);
                    fit.set_rig_active(j, on.0);
                    trim_rack(&mut fit.rigs);
                }
                _ => return Outcome::skipped(),
            }
            out.structural = true;
        }
        Action::Clone(from, to) => match (from, to) {
            (RowRef::Module(slot_a, i), RowRef::Module(slot_b, j))
                if slot_a == slot_b && i != j =>
            {
                let rack = fit.rack_mut(slot_a);
                let Some(Some(module)) = rack.get(i).cloned() else {
                    return Outcome::skipped();
                };
                if rack.get(j).is_some_and(Option::is_some) {
                    return Outcome::skipped();
                }
                if rack.len() <= j {
                    rack.resize_with(j + 1, || None);
                }
                rack[j] = Some(module);
            }
            (RowRef::Rig(i), RowRef::Rig(j)) if i != j => {
                let Some(Some(rig)) = fit.rigs.get(i).copied() else {
                    return Outcome::skipped();
                };
                if fit.rigs.get(j).is_some_and(Option::is_some) {
                    return Outcome::skipped();
                }
                if fit.rigs.len() <= j {
                    fit.rigs.resize(j + 1, None);
                }
                fit.rigs[j] = Some(rig);
                let on = fit.rig_active(i);
                fit.set_rig_active(j, on);
            }
            _ => return Outcome::skipped(),
        },
        Action::SwapType(rows, type_id) => {
            for row in rows {
                match row {
                    RowRef::Module(slot, i) => {
                        if let Some(Some(m)) = fit.rack_mut(slot).get_mut(i) {
                            m.type_id = type_id;
                            // The new variation may not take the old charge.
                            let keeps = m.charge.is_some_and(|c| {
                                let (c, t) = (loaded.catalog.get(c), loaded.catalog.get(type_id));
                                c.zip(t).is_some_and(|(c, t)| c.fits_in(t))
                            });
                            if !keeps {
                                m.charge = None;
                            }
                        }
                    }
                    RowRef::Rig(i) => {
                        let on = fit.rig_active(i);
                        if let Some(r) = fit.rigs.get_mut(i).filter(|r| r.is_some()) {
                            *r = Some(type_id);
                        }
                        fit.set_rig_active(i, on);
                    }
                    RowRef::Subsystem(i) => {
                        if let Some(s) = fit.subsystems.get_mut(i) {
                            *s = type_id;
                        }
                    }
                }
            }
        }
        Action::AddMutated(type_id, mutation) => {
            if !fit.add_mutated(&loaded.catalog, type_id, mutation) {
                return Outcome::skipped();
            }
        }
        Action::Mutate(target, mutaplasmid) => match target {
            MutaTarget::Modules(rows) => {
                for m in modules_mut(fit, &rows) {
                    m.mutate(&loaded.catalog, mutaplasmid);
                }
            }
            MutaTarget::Drones(i) => {
                if let Some(stack) = fit.drones.get_mut(i) {
                    stack.mutate(&loaded.catalog, mutaplasmid);
                }
            }
        },
        Action::Revert(target) => match target {
            MutaTarget::Modules(rows) => {
                for m in modules_mut(fit, &rows) {
                    m.revert();
                }
            }
            MutaTarget::Drones(i) => {
                if let Some(stack) = fit.drones.get_mut(i) {
                    stack.revert();
                }
            }
        },
        Action::FillRack(slot, type_id) => {
            let used = match slot {
                Slot::Rig => fit.rigs.iter().flatten().count(),
                Slot::Subsystem => fit.subsystems.len(),
                Slot::High | Slot::Mid | Slot::Low | Slot::Service => {
                    fit.rack(slot).iter().flatten().count()
                }
            };
            // Like pyfa, copies go on until the rack is full or the next one is refused.
            let mut base = loaded.engine.validate(fit, pilot);
            for _ in 0..free(slot, used) {
                let mut next = fit.clone();
                next.add(&loaded.catalog, type_id);
                let after = loaded.engine.validate(&next, pilot);
                if let Some(problem) = base.refusal(&after, next.ignore_restrictions) {
                    out.error = Some(problem.describe(|t| loaded.catalog.name(t)));
                    break;
                }
                *fit = next;
                base = after;
            }
        }
        Action::OptimizePrice => {
            let catalog = &loaded.catalog;
            let price = |t: TypeId| prices.get(t);
            let was = optimize::value(fit, catalog, price);
            let mut base = loaded.engine.validate(fit, pilot);
            let made = optimize::optimize(fit, catalog, price, |_, next| {
                let after = loaded.engine.validate(next, pilot);
                let refused = base.refusal(&after, next.ignore_restrictions).is_some();
                if !refused {
                    base = after;
                }
                refused
            });
            let saving = was - optimize::value(fit, catalog, price);
            out.report = Some(if made.is_empty() {
                "No cheaper replacements found".to_owned()
            } else {
                format!(
                    "Swapped {} for cheaper ones, saving {} ISK",
                    crate::ui::widgets::count(made.len(), "item type", "item types"),
                    crate::ui::stats::format::currency(saving)
                )
            });
            out.structural = true;
        }
        Action::FillClone(slot, i) => {
            let Some(Some(template)) = fit.rack(slot).get(i).cloned() else {
                return Outcome::skipped();
            };
            let used = fit.rack(slot).iter().flatten().count();
            for _ in 0..free(slot, used) {
                fit.place_module(slot, template.clone());
            }
        }
        Action::ModuleToCargo(from, onto, transfer) => {
            out.structural = fit.module_to_cargo(&loaded.catalog, from, onto, transfer);
        }
        Action::CargoToModule(type_id, onto, transfer) => {
            out.structural = fit.cargo_to_module(&loaded.catalog, type_id, onto, transfer);
        }
        _ => unreachable!("not a module action"),
    }
    out
}

/// The furthest a module's state goes: passive modules only come online, and only some active
/// ones overheat.
fn highest_state(catalog: &crate::data::Catalog, type_id: TypeId) -> ModuleState {
    match catalog.get(type_id) {
        Some(t) if t.overloadable => ModuleState::Overload,
        Some(t) if t.activatable => ModuleState::Active,
        _ => ModuleState::Online,
    }
}

/// `action` with each module row it names widened to the module's group.
pub(super) fn with_groups(fit: &crate::fit::Fit, action: Action) -> Action {
    let widen = |rows: Vec<RowRef>| -> Vec<RowRef> {
        let mut out: Vec<RowRef> = Vec::with_capacity(rows.len());
        for row in rows {
            let members = match row {
                RowRef::Module(slot, i) => fit
                    .group(slot, i)
                    .into_iter()
                    .map(|j| RowRef::Module(slot, j))
                    .collect(),
                RowRef::Rig(_) | RowRef::Subsystem(_) => vec![row],
            };
            for member in members {
                if !out.contains(&member) {
                    out.push(member);
                }
            }
        }
        out
    };
    match action {
        Action::CycleState {
            main,
            others,
            offline,
        } => Action::CycleState {
            main,
            others: widen(others),
            offline,
        },
        Action::SetRah(rows, pattern) => Action::SetRah(widen(rows), pattern),
        Action::SetCharge(rows, charge) => Action::SetCharge(widen(rows), charge),
        Action::SetSpool(rows, cycles) => Action::SetSpool(widen(rows), cycles),
        Action::Remove(rows) => Action::Remove(widen(rows)),
        Action::SwapType(rows, type_id) => Action::SwapType(widen(rows), type_id),
        other => other,
    }
}

fn modules_mut<'f>(fit: &'f mut crate::fit::Fit, rows: &[RowRef]) -> Vec<&'f mut Module> {
    let wanted: BTreeSet<(Slot, usize)> = rows
        .iter()
        .filter_map(|r| match *r {
            RowRef::Module(slot, i) => Some((slot, i)),
            RowRef::Rig(_) | RowRef::Subsystem(_) => None,
        })
        .collect();
    let crate::fit::Fit {
        high,
        mid,
        low,
        services,
        ..
    } = fit;
    [
        (Slot::High, high),
        (Slot::Mid, mid),
        (Slot::Low, low),
        (Slot::Service, services),
    ]
    .into_iter()
    .flat_map(|(slot, rack)| {
        rack.iter_mut()
            .enumerate()
            .filter(|(i, _)| wanted.contains(&(slot, *i)))
            .filter_map(|(_, m)| m.as_mut())
            .collect::<Vec<_>>()
    })
    .collect()
}
