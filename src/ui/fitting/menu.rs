//! The fitting rows' context menus: pyfa's fittingModule, fittingCharge and fittingShip.

use std::rc::Rc;

use gpui_kit::component::{
    IconName,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{App, ClickEvent, Context, WeakEntity, Window};

use crate::market::charges::{ChargeMenu, compatible_charges, structure_charges};
use crate::ui::{
    Action, Efa, Loaded, RowRef, Scope,
    actions::MutaTarget,
    characters::skills_submenu,
    item_stats::{StatsOf, stats_entry},
    mutations::{copy_entry, mutation_entries},
    profile_menus::{PickPattern, damage_pattern_menu},
    widgets::{entry, icon_label, item_entry, item_label, on_efa},
};
use crate::{
    damage_pattern::DamagePattern,
    data::{ItemType, Slot, TypeId},
    engine::{FitItem, ModuleInfo, SkillTarget},
    fit::{DamageProfile, Module, RahPattern, SystemSecurity},
};

/// Tallest a long menu grows before it scrolls.
const MENU_MAX_H: f32 = 480.;

/// What a row's menu needs to know, read when the menu opens.
pub(super) struct MenuSnapshot {
    item: RowRef,
    type_id: TypeId,
    module: Option<Module>,
    info: ModuleInfo,
    /// The user's damage patterns, for the reactive armor hardener's menu.
    patterns: Vec<DamagePattern>,
}

impl MenuSnapshot {
    pub(super) fn take(this: &WeakEntity<Efa>, item: RowRef, cx: &App) -> Option<Self> {
        let g = this.upgrade()?;
        let g = g.read(cx);
        let fit = g.fit()?;
        let (type_id, module) = match item {
            RowRef::Module(slot, i) => {
                let m = fit.rack(slot).get(i)?.clone()?;
                (m.type_id, Some(m))
            }
            RowRef::Rig(i) => ((*fit.rigs.get(i)?)?, None),
            RowRef::Subsystem(i) => (*fit.subsystems.get(i)?, None),
        };
        let info = match item {
            RowRef::Module(slot, i) => g
                .stats
                .as_ref()
                .and_then(|s| s.modules.get(&(slot, i)).copied())
                .unwrap_or_default(),
            RowRef::Rig(_) | RowRef::Subsystem(_) => ModuleInfo::default(),
        };
        Some(Self {
            item,
            type_id,
            module,
            info,
            patterns: g.saved.damage_patterns.clone(),
        })
    }
}

/// Act on `item` and whatever the click's scope adds to it.
fn scoped(
    this: &WeakEntity<Efa>,
    item: RowRef,
    default: Scope,
    action: impl Fn(Vec<RowRef>) -> Action + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    on_efa(this, move |g, event, _, cx| {
        let targets = g.targets(item, Scope::from_click(default, event));
        g.apply(action(targets), cx);
    })
}

/// pyfa's fittingModule, fittingCharge and fittingShip menus, in its order.
pub(super) fn item_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    snapshot: &MenuSnapshot,
    ship: TypeId,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let (item, type_id) = (snapshot.item, snapshot.type_id);
    let Some(t) = catalog.get(type_id) else {
        return menu;
    };
    let mut menu = menu;
    let shown = crate::ui::hub::shown_menus(window, cx);

    let mut takes_charges = false;
    if let Some(m) = &snapshot.module {
        let charges = compatible_charges(catalog, type_id);
        takes_charges = !charges.is_empty();
        if takes_charges {
            let structure = structure_charges(catalog, t, &charges);
            menu = charge_submenu(menu, this, loaded, item, structure, m.charge, window, cx);
        }
        if let Some(max) = snapshot
            .info
            .spool_max
            .filter(|_| t.spools && shown.spoolup)
        {
            menu = spool_submenu(menu, this, item, m.spool, max, window, cx);
        }
        if t.group_id == GROUP_RAH {
            menu = rah_submenu(
                menu,
                this,
                item,
                m.rah.clone(),
                &snapshot.patterns,
                window,
                cx,
            );
        }
    }
    let fitted = match item {
        RowRef::Module(slot, i) => FitItem::Module(slot, i),
        RowRef::Rig(i) => FitItem::Rig(i),
        RowRef::Subsystem(i) => FitItem::Subsystem(i),
    };
    menu = menu.item(stats_entry(
        this,
        t.category_label(),
        StatsOf::Fitted(fitted),
    ));
    // A mutated module has no market group of its own; pyfa jumps to its base item's.
    let mutation = snapshot.module.as_ref().and_then(|m| m.mutation.as_ref());
    let market_type = mutation.map_or(type_id, |m| m.base);
    if catalog
        .get(market_type)
        .is_some_and(|t| t.market_group.is_some())
    {
        menu = menu.item(
            entry(IconName::Search, "Module Market Group").on_click(on_efa(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(market_type, window, cx);
                },
            )),
        );
    }
    menu = menu.item(entry(IconName::Delete, "Remove Module").on_click(scoped(
        this,
        item,
        Scope::Selection,
        Action::Remove,
    )));
    let variations = catalog.variations_of(type_id);
    if variations.len() > 1 && shown.variations {
        menu = variations_submenu(menu, this, loaded, item, type_id, variations, window, cx);
    }
    if let RowRef::Module(..) = item {
        let target = MutaTarget::Modules(vec![item]);
        menu = mutation_entries(menu, this, loaded, type_id, mutation, &target, window, cx);
    }
    let rack = match item {
        RowRef::Module(slot, _) => slot,
        RowRef::Rig(_) => Slot::Rig,
        RowRef::Subsystem(_) => Slot::Subsystem,
    };
    let slot = (rack != Slot::Subsystem && shown.module_fill).then_some(rack);
    if let Some(slot) = slot {
        menu = menu.item(
            entry(IconName::Plus, format!("Fill With {}", t.category_label())).on_click(on_efa(
                this,
                move |g, _, _, cx| {
                    let action = match item {
                        RowRef::Module(slot, i) => Action::FillClone(slot, i),
                        RowRef::Rig(_) | RowRef::Subsystem(_) => Action::FillRack(slot, type_id),
                    };
                    g.apply(action, cx);
                },
            )),
        );
    }
    if let Some(mutation) = mutation {
        menu = menu.item(copy_entry(loaded, mutation));
    }

    menu = skills_submenu(
        menu,
        this,
        loaded,
        // Skills see the base item; the mutation only moves numbers.
        SkillTarget::Module(rack, market_type),
        window,
        cx,
    );

    let charge = snapshot.module.as_ref().and_then(|m| m.charge);
    if charge.is_some()
        && let RowRef::Module(slot, i) = item
    {
        menu = menu.separator().item(stats_entry(
            this,
            "Charge",
            StatsOf::Fitted(FitItem::Charge(slot, i)),
        ));
    }
    if let Some(charge) = charge
        && catalog
            .get(charge)
            .is_some_and(|c| c.market_group.is_some())
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
    if takes_charges {
        menu = menu.item(
            entry(IconName::BatteryCharging, "Show Compatible Charges").on_click(on_efa(
                this,
                move |g, _, window, cx| {
                    g.show_charges_for(type_id, window, cx);
                },
            )),
        );
    }
    if let Some(charge) = charge {
        let target = SkillTarget::Charge {
            slot: rack,
            module: market_type,
            charge,
        };
        menu = skills_submenu(menu, this, loaded, target, window, cx);
    }

    ship_entries(menu.separator(), this, ship, window, cx)
}

pub(super) fn ship_menu_builder(
    this: WeakEntity<Efa>,
    ship: TypeId,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| ship_entries(menu, &this, ship, window, cx)
}

/// Ships whose bonuses scale with the pilot's security status.
const SEC_STATUS_SHIPS: [&str; 6] = [
    "Pacifier",
    "Enforcer",
    "Marshal",
    "Sidewinder",
    "Cobra",
    "Python",
];
const SEC_STATUS_PRESETS: [f64; 11] = [-10., -8., -6., -4., -2., 0., 1., 2., 3., 4., 5.];

/// pyfa's fittingShip menu.
pub(in crate::ui) fn ship_entries(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    ship: TypeId,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(g) = this.upgrade() else { return menu };
    let (loaded, mode, sec_status, character_sec, security, browsing_ship) = {
        let g = g.read(cx);
        let fit = g.fit();
        (
            g.loaded(),
            fit.and_then(|f| f.mode),
            fit.and_then(|f| f.sec_status),
            g.characters.sec_status(g.fit_character()),
            fit.map_or_default(|f| f.system_security),
            g.browsing_fits_of(ship),
        )
    };
    let Some(loaded) = loaded else { return menu };
    let ship_name = loaded.catalog.name(ship);
    let structure = loaded.catalog.get(ship).is_some_and(ItemType::is_structure);
    let mut menu = menu;

    let modes = loaded.catalog.modes_for(ship).to_vec();
    if !modes.is_empty() {
        let (this, loaded, prefix) = (this.clone(), Rc::clone(&loaded), format!("{ship_name} "));
        menu = menu.submenu_with_element(
            "Tactical Mode",
            icon_label(IconName::Settings2, "Tactical Mode"),
            window,
            cx,
            move |menu, _, _| {
                modes.iter().fold(menu, |menu, &id| {
                    let name = loaded.catalog.name(id);
                    let label = name
                        .strip_prefix(&prefix)
                        .unwrap_or(&name)
                        .trim_end_matches(" Mode")
                        .to_owned();
                    menu.item(
                        item_entry(&loaded, id, label)
                            .checked(mode == Some(id))
                            .on_click(on_efa(&this, move |g, _, _, cx| {
                                g.apply(Action::SetMode(id), cx);
                            })),
                    )
                })
            },
        );
    }
    let hull = if structure { "Citadel" } else { "Ship" };
    menu = menu.item(stats_entry(this, hull, StatsOf::Fitted(FitItem::Ship)));
    if structure {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Citadel System Security",
            icon_label(IconName::Globe, "Citadel System Security"),
            window,
            cx,
            move |menu, _, _| {
                SystemSecurity::ALL.into_iter().fold(menu, |menu, option| {
                    menu.item(
                        PopupMenuItem::new(option.label())
                            .checked(security == option)
                            .on_click(on_efa(&this, move |g, _, _, cx| {
                                g.apply(Action::SetSystemSecurity(option), cx);
                            })),
                    )
                })
            },
        );
    }
    if SEC_STATUS_SHIPS.contains(&ship_name.as_str()) {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Pilot Security Status",
            icon_label(IconName::CircleUser, "Pilot Security Status"),
            window,
            cx,
            move |menu, _, _| {
                let set = |value: Option<f64>| {
                    on_efa(&this, move |g, _, _, cx| {
                        g.apply(Action::SetSecStatus(value), cx);
                    })
                };
                let custom = sec_status.filter(|v| !SEC_STATUS_PRESETS.contains(v));
                let custom_label = match custom {
                    Some(v) => format!("Custom ({v})"),
                    None => "Custom…".to_owned(),
                };
                let mut menu = menu
                    .item(
                        PopupMenuItem::new(format!("Character ({})", py_round(character_sec)))
                            .checked(sec_status.is_none())
                            .on_click(set(None)),
                    )
                    .item(
                        PopupMenuItem::new(custom_label)
                            .checked(custom.is_some())
                            .on_click(on_efa(&this, move |_, _, window, cx| {
                                Efa::open_value_dialog(
                                    "Security Status (-10 to 5)",
                                    custom.unwrap_or(0.0).to_string(),
                                    window,
                                    cx,
                                    |v: f64| Action::SetSecStatus(Some(v.clamp(-10.0, 5.0))),
                                );
                            })),
                    )
                    .separator();
                for preset in SEC_STATUS_PRESETS {
                    menu = menu.item(
                        PopupMenuItem::new(format!("{preset}"))
                            .checked(sec_status == Some(preset))
                            .on_click(set(Some(preset))),
                    );
                }
                menu
            },
        );
    }
    // pyfa leaves this out while the browser already lists the ship's fits.
    if !browsing_ship {
        menu = menu.item(
            entry(IconName::FolderOpen, "Open in Fitting Browser").on_click(on_efa(
                this,
                move |g, _, _, cx| {
                    g.jump_to_fittings(ship, cx);
                },
            )),
        );
    }
    skills_submenu(menu, this, &loaded, SkillTarget::Ship, window, cx)
}

/// A number as Python prints `round(x, 2)`, which pyfa shows the character's status as.
fn py_round(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.1}")
    } else {
        format!("{rounded}")
    }
}

/// A charge entry: loads it into the selection, or every similar module when the preference
/// says so; Ctrl or Alt swaps the two.
fn charge_entry(
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    id: TypeId,
    loaded_charge: Option<TypeId>,
) -> PopupMenuItem {
    item_entry(loaded, id, loaded.catalog.name(id))
        .checked(loaded_charge == Some(id))
        .on_click(ammo_scoped(this, item, move |rows| {
            Action::SetCharge(rows, Some(id))
        }))
}

/// A charge change: to every module of the same type or just the selection, as the "Change
/// charge in all modules of the same type" preference says, with Ctrl or Alt doing the other.
fn ammo_scoped(
    this: &WeakEntity<Efa>,
    item: RowRef,
    action: impl Fn(Vec<RowRef>) -> Action + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    on_efa(this, move |g, event, _, cx| {
        let default = if g.saved.settings.preferences.general.ammo_change_all {
            Scope::Similar
        } else {
            Scope::Selection
        };
        let targets = g.targets(item, Scope::from_click(default, event));
        g.apply(action(targets), cx);
    })
}

/// A submenu of charges between pyfa's "Less Damage" and "More Damage" markers.
#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn damage_ladder(
    menu: PopupMenu,
    title: String,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    charges: Vec<TypeId>,
    loaded_charge: Option<TypeId>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    menu.submenu_with_element(
        title.clone(),
        item_label(&loaded, charges.first().copied(), title),
        window,
        cx,
        move |menu, _, _| {
            charges
                .iter()
                .fold(menu.label("─ Less Damage ─"), |menu, &c| {
                    menu.item(charge_entry(&this, &loaded, item, c, loaded_charge))
                })
                .label("─ More Damage ─")
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn charge_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    structure: ChargeMenu,
    loaded_charge: Option<TypeId>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
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
            match &structure {
                ChargeMenu::Turret(families) => {
                    menu = menu.label("─ Long Range ─");
                    for family in families {
                        menu = match family.as_slice() {
                            [only] => {
                                menu.item(charge_entry(&this, &loaded, item, *only, loaded_charge))
                            }
                            _ => damage_ladder(
                                menu,
                                catalog.name(family[0]),
                                &this,
                                &loaded,
                                item,
                                family.clone(),
                                loaded_charge,
                                window,
                                cx,
                            ),
                        };
                    }
                    menu = menu.label("─ Short Range ─");
                }
                ChargeMenu::Missile(by_type) => {
                    for (label, charges) in by_type {
                        menu = damage_ladder(
                            menu,
                            (*label).to_owned(),
                            &this,
                            &loaded,
                            item,
                            charges.clone(),
                            loaded_charge,
                            window,
                            cx,
                        );
                    }
                }
                ChargeMenu::Flat(list) => {
                    for &c in list {
                        menu = menu.item(charge_entry(&this, &loaded, item, c, loaded_charge));
                    }
                }
            }
            menu.item(entry(IconName::CircleX, "Empty").on_click(ammo_scoped(
                &this,
                item,
                |rows| Action::SetCharge(rows, None),
            )))
        },
    )
}

fn spool_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    item: RowRef,
    current: Option<u32>,
    max: u32,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = this.clone();
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
                            .on_click(scoped(&this, item, Scope::Selection, move |rows| {
                                Action::SetSpool(rows, Some(cycles))
                            })),
                    )
                },
            );
            menu.separator()
                .item(entry(IconName::Undo2, "Reset").on_click(scoped(
                    &this,
                    item,
                    Scope::Selection,
                    |rows| Action::SetSpool(rows, None),
                )))
        },
    )
}

/// pyfa's Armor Resistance Shift Hardener group, the reactive armor hardeners.
const GROUP_RAH: i32 = 1150;

/// pyfa's "RAH Damage Pattern": the fit's pattern, no adapting, or any damage pattern.
fn rah_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    item: RowRef,
    current: Option<RahPattern>,
    user: &[DamagePattern],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, user) = (this.clone(), user.to_vec());
    menu.submenu_with_element(
        "RAH Damage Pattern",
        icon_label(IconName::ChartPie, "RAH Damage Pattern"),
        window,
        cx,
        move |menu, window, cx| {
            let set = |pattern: Option<RahPattern>| {
                scoped(&this, item, Scope::Selection, move |rows| {
                    Action::SetRah(rows, pattern.clone())
                })
            };
            let menu = menu
                .item(
                    PopupMenuItem::new("Fit Pattern")
                        .checked(current.is_none())
                        .on_click(set(None)),
                )
                .item(
                    PopupMenuItem::new("Do Not Adapt")
                        .checked(current == Some(RahPattern::DoNotAdapt))
                        .on_click(set(Some(RahPattern::DoNotAdapt))),
                )
                .separator();
            // Nothing in the pattern list is checked while the fit's pattern or no adapting is.
            let checked = match &current {
                Some(RahPattern::Pattern(profile)) => profile.clone(),
                _ => DamageProfile::Pattern {
                    name: String::new(),
                    amounts: [0.0; 4],
                },
            };
            let this = this.clone();
            let pick: PickPattern = Rc::new(move |profile, _, cx| {
                Efa::app_update(&this, cx, |g, cx| {
                    let rows = g.targets(item, Scope::Selection);
                    g.apply(Action::SetRah(rows, Some(RahPattern::Pattern(profile))), cx);
                });
            });
            damage_pattern_menu(menu, &user, &checked, &pick, window, cx)
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn variations_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    current: TypeId,
    variations: &[TypeId],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    // Grouped under their meta group, groups in the order they first appear by meta level.
    let mut groups: Vec<(String, Vec<(TypeId, String)>)> = Vec::new();
    for &v in variations {
        let Some(t) = catalog.get(v) else { continue };
        let group = t
            .meta_group
            .and_then(|g| catalog.meta_group_names.get(&g))
            .cloned()
            .unwrap_or_else(|| "Other".to_owned());
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, members)) => members.push((v, t.name.clone())),
            None => groups.push((group, vec![(v, t.name.clone())])),
        }
    }
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    menu.submenu_with_element(
        "Variations",
        icon_label(IconName::Replace, "Variations"),
        window,
        cx,
        move |menu, window, _| {
            let mut menu = menu
                .scrollable(true)
                .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H));
            for (group, members) in &groups {
                menu = menu.label(group.clone());
                for (id, name) in members {
                    let id = *id;
                    menu = menu.item(
                        item_entry(&loaded, id, name.clone())
                            .checked(id == current)
                            .on_click(scoped(&this, item, Scope::Selection, move |rows| {
                                Action::SwapType(rows, id)
                            })),
                    );
                }
            }
            menu
        },
    )
}
