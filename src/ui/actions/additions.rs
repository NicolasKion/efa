//! Actions on the additions: drones, cargo, implants and boosters.

use std::collections::BTreeSet;

use super::{Action, AddKind, EditEnv, Outcome};
use crate::fit::Fit;

/// Make an action on the additions: drones, cargo, implants and boosters.
pub(super) fn apply(fit: &mut Fit, action: Action, env: &EditEnv) -> Outcome {
    let EditEnv {
        loaded,
        stats,
        implants_in_cargo,
        ..
    } = *env;
    let mut out = Outcome::default();
    match action {
        Action::AddDroneStacks(stacks) => {
            for stack in stacks {
                if stack.mutation.is_some() {
                    fit.add_drone_stack(stack);
                } else {
                    fit.add_drones(stack.type_id, stack.count);
                }
            }
        }
        Action::SetDronesActive(i, n) => fit.drones[i].active = n.min(fit.drones[i].count),
        Action::AddCargo(type_id, count) => fit.add_cargo(type_id, count),
        Action::ApplyCargoPreset(preset) => {
            let applied = crate::cargo_preset::apply(&preset, &mut fit.cargo);
            let noun = crate::cargo_preset::noun(&loaded.catalog, &preset);
            let mut text = format!("{}: {}", preset.name, applied.summary(&noun));
            let capacity = stats.and_then(|s| s.cargo_capacity);
            let used = fit.hold_volume(&loaded.catalog, implants_in_cargo);
            if let Some(capacity) = capacity
                && applied.changed()
                && used > capacity + 1e-6
            {
                use std::fmt::Write;
                let _ = write!(
                    text,
                    ". The hold is overfull: {} / {} m³",
                    crate::ui::widgets::short(used),
                    crate::ui::widgets::short(capacity)
                );
            }
            out.report = Some(text);
            out.structural = applied.removed > 0;
        }
        Action::AddDrones(type_id, count) => fit.add_drones(type_id, count),
        Action::RemoveAdditions(kind, rows) => {
            let rows: BTreeSet<usize> = rows.into_iter().collect();
            for i in rows.into_iter().rev() {
                match kind {
                    AddKind::Drones if i < fit.drones.len() => {
                        fit.drones.remove(i);
                    }
                    AddKind::Implants => fit.remove_implant(i),
                    AddKind::Boosters if i < fit.boosters.len() => {
                        fit.boosters.remove(i);
                    }
                    AddKind::Cargo if i < fit.cargo.len() => {
                        fit.cargo.remove(i);
                    }
                    _ => {}
                }
            }
            out.structural = true;
        }
        Action::SetAmount(kind, rows, amount) => {
            for i in rows {
                match kind {
                    AddKind::Drones => {
                        if let Some(stack) = fit.drones.get_mut(i) {
                            stack.count = amount.max(1);
                            stack.active = stack.active.min(stack.count);
                        }
                    }
                    AddKind::Cargo => {
                        if let Some(stack) = fit.cargo.get_mut(i) {
                            stack.1 = amount;
                        }
                    }
                    AddKind::Implants | AddKind::Boosters => {}
                }
            }
            // pyfa removes cargo stacks set to zero.
            if kind == AddKind::Cargo && amount == 0 {
                fit.cargo.retain(|&(_, n)| n > 0);
                out.structural = true;
            }
        }
        Action::SplitDrones(i, amount) => {
            if let Some(stack) = fit.drones.get_mut(i)
                && amount > 0
                && amount < stack.count
            {
                stack.count -= amount;
                let moved_active = stack.active.saturating_sub(stack.count);
                stack.active -= moved_active;
                let split = crate::fit::DroneStack {
                    count: amount,
                    active: moved_active,
                    ..stack.clone()
                };
                fit.drones.insert(i + 1, split);
            }
        }
        Action::ToggleDrones(clicked, stacks) => fit.toggle_drones(clicked, &stacks),
        Action::RemoveDrone(i) => {
            fit.remove_drone(i);
            out.structural = true;
        }
        Action::MergeDrones { from, to } => {
            if !fit.merge_drones(from, to) {
                return Outcome::skipped();
            }
            out.structural = true;
        }
        Action::CloneDrones(i) => fit.clone_drones(i),
        Action::SwapAdditions(kind, rows, type_id) => {
            for i in rows {
                match kind {
                    AddKind::Drones => {
                        if let Some(stack) = fit.drones.get_mut(i) {
                            stack.type_id = type_id;
                        }
                    }
                    AddKind::Implants => fit.swap_implant(&loaded.catalog, i, type_id),
                    AddKind::Boosters => {
                        fit.swap_booster(&loaded.catalog, i, type_id);
                    }
                    AddKind::Cargo => {
                        if let Some(stack) = fit.cargo.get_mut(i) {
                            stack.0 = type_id;
                        }
                    }
                }
            }
            if kind == AddKind::Cargo {
                fit.merge_cargo();
                out.structural = true;
            }
        }
        Action::ToggleBoosters(main, rows) => {
            let Some(on) = fit.boosters.get(main).map(|b| !b.active) else {
                return Outcome::skipped();
            };
            for i in rows.into_iter().chain([main]) {
                if let Some(b) = fit.boosters.get_mut(i) {
                    b.active = on;
                }
            }
        }
        Action::ToggleBoosterSideEffect(i, effect) => {
            let Some(b) = fit.boosters.get_mut(i) else {
                return Outcome::skipped();
            };
            match b.side_effects.iter().position(|&e| e == effect) {
                Some(at) => {
                    b.side_effects.remove(at);
                }
                None => b.side_effects.push(effect),
            }
        }
        Action::ToggleImplants(clicked, rows) => fit.toggle_implants(clicked, &rows),
        Action::SetImplantSource(source) => {
            fit.implant_source = source;
            out.structural = true;
        }
        Action::AddImplants(implants) => {
            for type_id in implants {
                fit.add_implant(&loaded.catalog, type_id);
            }
            out.structural = true;
        }
        Action::AddAdditions(kind, items) => {
            for (type_id, count) in items {
                match kind {
                    AddKind::Drones => fit.add_drones(type_id, count),
                    AddKind::Implants => {
                        fit.add_implant(&loaded.catalog, type_id);
                    }
                    AddKind::Boosters => {
                        fit.add_booster(&loaded.catalog, crate::fit::Booster::new(type_id));
                    }
                    AddKind::Cargo => fit.add_cargo(type_id, count),
                }
            }
        }
        _ => unreachable!("not an additions action"),
    }
    out
}
