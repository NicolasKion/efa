//! The projected pane's menus: pyfa's projected entries for a row, with their charge and spool
//! submenus, and for the pane.

use std::rc::Rc;

use gpui_kit::component::{
    IconName,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{App, ClickEvent, Context, WeakEntity, Window};

use super::model::{RangeInput, Reach, Row};
use crate::market::charges::{ChargeMenu, compatible_charges, structure_charges};
use crate::ui::additions::{
    MENU_MAX_H,
    drones::format_amount,
    linked::{Link, add_fit_entries},
};
use crate::{
    data::{TypeId, projection::EnvMenu},
    engine::{FighterInfo, FitItem, Stats},
    fit::projected::{MAX_FIT_COPIES, Projected},
    ui::{
        Efa, Loaded,
        item_stats::{StatsOf, stats_entry},
        widgets::{entry, icon_label, item_entry, on_efa},
    },
};

/// Run `edit` on the targets of display row `n`, the reach picked by the modifiers held when the
/// entry is clicked.
fn on_targets<F>(
    this: &WeakEntity<Efa>,
    n: usize,
    default: Reach,
    edit: F,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static + use<F>
where
    F: Fn(&mut Efa, Vec<Row>, &mut Context<Efa>) + 'static,
{
    on_efa(this, move |g, _, window, cx| {
        let reach = match Reach::from_modifiers(window.modifiers()) {
            Reach::Similar => Reach::Similar,
            Reach::Selection => default,
        };
        let Some(loaded) = g.loaded() else { return };
        let targets = g.projected_targets(&loaded, n, reach);
        edit(g, targets, cx);
    })
}

/// pyfa's projectedModule, projectedDrone, projectedFighter and projectedFit entries.
#[expect(clippy::too_many_lines, reason = "four menus in one")]
pub(super) fn projected_item_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    n: usize,
    row: Row,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let Some((projected, current, spool_max, fighter_info)) = this.upgrade().and_then(|g| {
        let g = g.read(cx);
        let fit = g.fit()?;
        let stats: Option<&Stats> = g.stats.as_ref();
        let spool = match row {
            Row::Module(i) => stats.and_then(|s| s.projected_spool.get(i).copied().flatten()),
            _ => None,
        };
        let info = match row {
            Row::Fighter(i) => stats.and_then(|s| s.projected_fighters.get(i).cloned()),
            _ => None,
        };
        Some((fit.projected.clone(), g.saved.settings.current, spool, info))
    }) else {
        return menu;
    };
    let mut menu = menu;
    let type_id = match row {
        Row::Fit { .. } => None,
        Row::Module(i) => Some(projected.modules[i].type_id),
        Row::Env(i) => Some(projected.env[i].type_id),
        Row::Drone(i) => Some(projected.drones[i].type_id),
        Row::Fighter(i) => Some(projected.fighters[i].fighter.type_id),
    };

    if let Row::Fit { fit: index, .. } = row
        && current != Some(index)
    {
        menu = menu.item(
            entry(IconName::ExternalLink, "Open Fit in New Tab")
                .on_click(on_efa(this, move |g, _, _, cx| g.open_fit(index, cx))),
        );
    }
    if let Row::Module(i) = row {
        let module = &projected.modules[i];
        let charges = compatible_charges(catalog, module.type_id);
        if let Some(t) = catalog.get(module.type_id)
            && !charges.is_empty()
        {
            let structure = structure_charges(catalog, t, &charges);
            menu = charge_submenu(menu, this, loaded, n, structure, module.charge, window, cx);
        }
        if let Some(max) = spool_max {
            menu = spool_submenu(menu, this, n, module.spool, max, window, cx);
        }
    }
    if let Row::Fighter(i) = row
        && let Some(info) = fighter_info.filter(|info| !info.abilities.is_empty())
    {
        let names: Vec<String> = info
            .abilities
            .iter()
            .map(|a| catalog.projection.ability_name(a.id))
            .collect();
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Abilities",
            icon_label(IconName::Settings2, "Abilities"),
            window,
            cx,
            move |menu, _, _| {
                info.abilities
                    .iter()
                    .zip(&names)
                    .fold(menu, |menu, (ability, name)| {
                        let id = ability.id;
                        let main_on = ability.active;
                        menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(ability.active)
                                .on_click(on_targets(
                                    &this,
                                    n,
                                    Reach::Selection,
                                    move |g, targets, cx| {
                                        let infos =
                                            g.stats.as_ref().map(|s| s.projected_fighters.clone());
                                        g.edit_projected(cx, |projected, _| {
                                            for target in
                                                targets.into_iter().chain([Row::Fighter(i)])
                                            {
                                                let Row::Fighter(j) = target else { continue };
                                                let has = infos
                                                    .as_ref()
                                                    .and_then(|s| s.get(j))
                                                    .is_some_and(|f| {
                                                        f.abilities.iter().any(|a| a.id == id)
                                                    });
                                                if has
                                                    && let Some(p) = projected.fighters.get_mut(j)
                                                {
                                                    p.fighter.set_ability(id, !main_on);
                                                }
                                            }
                                            false
                                        });
                                    },
                                )),
                        )
                    })
            },
        );
    }
    let stats_of = match row {
        Row::Fit { fit, .. } => Some(StatsOf::OtherFit(fit, FitItem::Ship)),
        _ => type_id.map(StatsOf::Type),
    };
    if let Some(of) = stats_of {
        menu = menu.item(stats_entry(this, "Projected Item", of));
    }
    if let Some(type_id) = type_id
        && catalog
            .get(type_id)
            .and_then(|t| t.market_group)
            .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        menu = menu.item(
            entry(IconName::Search, "Projected Item Market Group").on_click(on_efa(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(type_id, window, cx);
                },
            )),
        );
    }
    let remove_label = match row {
        Row::Drone(_) | Row::Fit { .. } => "Remove Projected Item Stack",
        _ => "Remove Projected Item",
    };
    menu = menu.item(entry(IconName::Delete, remove_label).on_click(on_targets(
        this,
        n,
        Reach::Selection,
        |g, targets, cx| g.remove_projected(&targets, true, cx),
    )));
    let amount = match row {
        Row::Fit { entry, .. } => Some(projected.fits[entry].amount),
        Row::Drone(i) => Some(projected.drones[i].count),
        Row::Fighter(i) => {
            let f = &projected.fighters[i].fighter;
            f.amount
                .or_else(|| catalog.projection.squadron_size(f.type_id))
        }
        Row::Module(_) | Row::Env(_) => None,
    };
    if let Some(amount) = amount {
        let is_fit = matches!(row, Row::Fit { .. });
        let prompt = if is_fit {
            "New Amount (0-20):"
        } else {
            "New Amount:"
        };
        menu = menu.item(
            entry(IconName::Settings2, "Change Projected Item Quantity").on_click(on_efa(
                this,
                move |_, _, window, cx| {
                    Efa::open_input_dialog(
                        "Change Amount",
                        prompt,
                        amount.to_string(),
                        window,
                        cx,
                        move |g, amount: u32, cx| set_amount(g, row, amount, cx),
                    );
                },
            )),
        );
    }
    if !matches!(row, Row::Env(_)) {
        let current_km = projected
            .range(row)
            .map_or_default(|m| format_amount(m / 1000.0, 0, false));
        menu = menu.item(
            entry(IconName::Map, "Change Projected Item Range").on_click(on_efa(
                this,
                move |_, _, window, cx| {
                    let similar = window.modifiers().alt || window.modifiers().control;
                    Efa::open_input_dialog(
                        "Change Projection Range",
                        "New Range, km:",
                        current_km.clone(),
                        window,
                        cx,
                        move |g, range: RangeInput, cx| {
                            let Some(loaded) = g.loaded() else { return };
                            let reach = if similar {
                                Reach::Similar
                            } else {
                                Reach::Selection
                            };
                            let targets = g.projected_targets(&loaded, n, reach);
                            g.edit_projected(cx, |projected, _| {
                                for target in targets {
                                    projected.set_range(target, range.0);
                                }
                                false
                            });
                        },
                    );
                },
            )),
        );
    }
    if let Some(type_id) = type_id.filter(|_| !matches!(row, Row::Env(_))) {
        let variations = catalog.variations_of(type_id);
        if variations.len() > 1 {
            let entries: Vec<(TypeId, String)> =
                variations.iter().map(|&v| (v, catalog.name(v))).collect();
            let (this, loaded) = (this.clone(), Rc::clone(loaded));
            menu = menu.submenu_with_element(
                "Variations",
                icon_label(IconName::Replace, "Variations"),
                window,
                cx,
                move |menu, window, _| {
                    entries.iter().fold(
                        menu.scrollable(true)
                            .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H)),
                        |menu, (v, name)| {
                            let v = *v;
                            menu.item(
                                item_entry(&loaded, v, name.clone())
                                    .checked(v == type_id)
                                    .on_click(on_targets(
                                        &this,
                                        n,
                                        Reach::Selection,
                                        move |g, targets, cx| {
                                            g.edit_projected(cx, |projected, _| {
                                                swap_type(projected, &targets, type_id, v);
                                                false
                                            });
                                        },
                                    )),
                            )
                        },
                    )
                },
            );
        }
    }
    // pyfa's projectedCharge entries follow the module's own.
    if let Row::Module(i) = row
        && let Some(charge) = projected.modules[i].charge
    {
        menu = menu.item(stats_entry(this, "Charge", StatsOf::Type(charge)));
    }
    // pyfa's projectedCharge entries follow the module's own.
    if let Row::Module(i) = row
        && let Some(charge) = projected.modules[i].charge
        && catalog
            .get(charge)
            .and_then(|t| t.market_group)
            .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        menu = menu.item(
            entry(IconName::Search, "Charge Market Group").on_click(on_efa(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(charge, window, cx);
                },
            )),
        );
    }
    if let Row::Module(i) = row {
        let module = projected.modules[i].type_id;
        if !compatible_charges(catalog, module).is_empty() {
            menu = menu.item(
                entry(IconName::BatteryCharging, "Show Compatible Charges").on_click(on_efa(
                    this,
                    move |g, _, window, cx| {
                        g.show_charges_for(module, window, cx);
                    },
                )),
            );
        }
    }
    menu
}

/// Swap targets of the same type as the clicked one for a variation.
fn swap_type(projected: &mut Projected, targets: &[Row], from: TypeId, to: TypeId) {
    for &target in targets {
        let t = match target {
            Row::Module(i) => projected.modules.get_mut(i).map(|m| &mut m.type_id),
            Row::Drone(i) => projected.drones.get_mut(i).map(|d| &mut d.type_id),
            Row::Fighter(i) => projected
                .fighters
                .get_mut(i)
                .map(|f| &mut f.fighter.type_id),
            Row::Fit { .. } | Row::Env(_) => None,
        };
        if let Some(t) = t.filter(|t| **t == from) {
            *t = to;
        }
    }
    // A swapped module may not take its charge any more; the engine just ignores it then.
}

/// pyfa's amount changes: fits between 1 and 20 (0 removes), drones and fighters at least one
/// (0 removes).
fn set_amount(g: &mut Efa, row: Row, amount: u32, cx: &mut Context<Efa>) {
    let max = match row {
        Row::Fighter(i) => g
            .stats
            .as_ref()
            .and_then(|s| s.projected_fighters.get(i))
            .and_then(FighterInfo::max_size),
        _ => None,
    };
    g.edit_projected(cx, |projected, catalog| {
        if amount == 0 {
            projected.remove(&[row], true);
            return true;
        }
        match row {
            Row::Fit { entry, .. } => {
                if let Some(p) = projected.fits.get_mut(entry) {
                    p.amount = amount.min(MAX_FIT_COPIES);
                }
            }
            Row::Drone(i) => {
                if let Some(d) = projected.drones.get_mut(i) {
                    d.count = amount;
                }
            }
            Row::Fighter(i) => {
                if let Some(p) = projected.fighters.get_mut(i) {
                    let max = max.or_else(|| catalog.projection.squadron_size(p.fighter.type_id));
                    p.fighter.set_amount(amount, max);
                }
            }
            Row::Module(_) | Row::Env(_) => {}
        }
        false
    });
}

/// The projected module's Charge menu, laid out like the fitting view's.
#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn charge_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    n: usize,
    structure: ChargeMenu,
    current: Option<TypeId>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    let set = move |this: &WeakEntity<Efa>, charge: Option<TypeId>| {
        on_targets(this, n, Reach::Similar, move |g, targets, cx| {
            g.edit_projected(cx, |projected, catalog| {
                for target in &targets {
                    let Row::Module(index) = *target else {
                        continue;
                    };
                    let Some(module) = projected.modules.get_mut(index) else {
                        continue;
                    };
                    let fits = charge.is_none_or(|charge| {
                        let item = catalog.get(charge);
                        let holder = catalog.get(module.type_id);
                        item.zip(holder).is_some_and(|(c, m)| c.fits_in(m))
                    });
                    if fits {
                        module.charge = charge;
                    }
                }
                false
            });
        })
    };
    let charge_entry = move |this: &WeakEntity<Efa>, loaded: &Rc<Loaded>, id: TypeId| {
        item_entry(loaded, id, loaded.catalog.name(id))
            .checked(current == Some(id))
            .on_click(set(this, Some(id)))
    };
    menu.submenu_with_element(
        "Charge",
        icon_label(IconName::BatteryCharging, "Charge"),
        window,
        cx,
        move |menu, window, cx| {
            let catalog = &loaded.catalog;
            let mut menu = menu
                .scrollable(true)
                .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H));
            let ladder = |menu: PopupMenu,
                          title: String,
                          charges: Vec<TypeId>,
                          window: &mut Window,
                          cx: &mut Context<PopupMenu>| {
                let (this, loaded) = (this.clone(), Rc::clone(&loaded));
                menu.submenu(title, window, cx, move |menu, _, _| {
                    charges
                        .iter()
                        .fold(menu.label("─ Less Damage ─"), |menu, &c| {
                            menu.item(charge_entry(&this, &loaded, c))
                        })
                        .label("─ More Damage ─")
                })
            };
            match &structure {
                ChargeMenu::Turret(families) => {
                    menu = menu.label("─ Long Range ─");
                    for family in families {
                        menu = match family.as_slice() {
                            [only] => menu.item(charge_entry(&this, &loaded, *only)),
                            _ => ladder(menu, catalog.name(family[0]), family.clone(), window, cx),
                        };
                    }
                    menu = menu.label("─ Short Range ─");
                }
                ChargeMenu::Missile(by_type) => {
                    for (label, charges) in by_type {
                        menu = ladder(menu, (*label).to_owned(), charges.clone(), window, cx);
                    }
                }
                ChargeMenu::Flat(list) => {
                    for &c in list {
                        menu = menu.item(charge_entry(&this, &loaded, c));
                    }
                }
            }
            menu.item(entry(IconName::CircleX, "Empty").on_click(set(&this, None)))
        },
    )
}

fn spool_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    n: usize,
    current: Option<u32>,
    max: u32,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = this.clone();
    let set = move |this: &WeakEntity<Efa>, cycles: Option<u32>| {
        on_targets(this, n, Reach::Selection, move |g, targets, cx| {
            g.edit_projected(cx, |projected, _| {
                for target in &targets {
                    if let Row::Module(i) = *target
                        && let Some(m) = projected.modules.get_mut(i)
                    {
                        m.spool = cycles;
                    }
                }
                false
            });
        })
    };
    menu.submenu_with_element(
        "Spoolup Cycles",
        icon_label(IconName::RefreshCw, "Spoolup Cycles"),
        window,
        cx,
        move |menu, window, _| {
            let menu = (0..=max).fold(
                menu.scrollable(true)
                    .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H)),
                |menu, cycles| {
                    let label = if cycles == max {
                        format!("{cycles} (default)")
                    } else {
                        cycles.to_string()
                    };
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(current.unwrap_or(max) == cycles)
                            .on_click(set(&this, Some(cycles))),
                    )
                },
            );
            menu.separator()
                .item(entry(IconName::Undo2, "Reset").on_click(set(&this, None)))
        },
    )
}

/// pyfa's projected entries for the whole pane: add fits, and environmental effects.
pub(super) fn projected_view_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = add_fit_entries(menu, this, loaded, Link::Projected, window, cx);
    let env = Rc::new(loaded.catalog.projection.env_menu.clone());
    let this = this.clone();
    menu.submenu_with_element(
        "Add Environmental Effect",
        icon_label(IconName::Globe, "Add Environmental Effect"),
        window,
        cx,
        move |menu, window, cx| env_level(menu, &this, &env, window, cx),
    )
}

/// One level of the environmental effects tree: its submenus, then its effects.
fn env_level(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    level: &Rc<EnvMenu>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu
        .scrollable(true)
        .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H));
    for (name, group) in &level.groups {
        let (this, group) = (this.clone(), Rc::new(group.clone()));
        menu = menu.submenu(name.clone(), window, cx, move |menu, window, cx| {
            env_level(menu, &this, &group, window, cx)
        });
    }
    for item in &level.items {
        let type_id = item.type_id;
        menu = menu.item(PopupMenuItem::new(item.label.clone()).on_click(on_efa(
            this,
            move |g, _, _, cx| {
                g.edit_projected(cx, |projected, _| {
                    projected.add_env(type_id);
                    true
                });
            },
        )));
    }
    menu
}
