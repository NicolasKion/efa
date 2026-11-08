//! The side's context menus: pyfa's baseShip, fit row and market item menus.

use std::rc::Rc;

use gpui_kit::component::{IconName, menu::PopupMenu};
use gpui_kit::{App, Context, WeakEntity, Window};

use super::market::MarketContext;
use crate::data::{Kind, Slot, TypeId};
use crate::ui::{
    Action, AdditionsTab, Efa,
    additions::{Link, goes_to_cargo},
    item_stats,
    widgets::{entry, on_each, on_efa},
};

/// Charges "Add to Cargo" puts in at once, and the smaller batch for scan probes.
const AMMO_BATCH: u32 = 1000;
const PROBE_BATCH: u32 = 8;

/// pyfa's baseShip menu, with the browser's New Fit.
pub(super) fn ship_menu(menu: PopupMenu, this: &WeakEntity<Efa>, ship: TypeId) -> PopupMenu {
    menu.item(item_stats::stats_entry(
        this,
        "Ship Basic",
        item_stats::StatsOf::Type(ship),
    ))
    .item(
        entry(IconName::Plus, "New Fit").on_click(on_efa(this, move |g, _, _, cx| {
            g.new_fit_for_ship(ship, cx);
        })),
    )
}

/// pyfa's fit row menu, plus the copy, rename and delete buttons pyfa has on the row.
pub(super) fn fit_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    i: usize,
    has_fit: bool,
) -> PopupMenu {
    let on =
        |f: fn(&mut Efa, usize, &mut Context<Efa>)| on_efa(this, move |g, _, _, cx| f(g, i, cx));
    let link = |link: Link| on_efa(this, move |g, _, _, cx| g.link(link, &[i], cx));
    let mut menu =
        menu.item(entry(IconName::FolderOpen, "Open in new tab").on_click(on(Efa::open_fit)));
    if has_fit {
        menu = menu
            .item(entry(IconName::Globe, "Project onto Active Fit").on_click(link(Link::Projected)))
            .item(entry(IconName::Network, "Add Command Booster").on_click(link(Link::Command)));
    }
    menu.separator()
        .item(entry(IconName::Copy, "Copy").on_click(on(Efa::copy_fit)))
        .item(
            entry(IconName::Replace, "Rename").on_click(on_efa(this, move |g, _, window, cx| {
                g.open_from_browser(i, cx);
                g.start_rename(window, cx);
            })),
        )
        .item(entry(IconName::Delete, "Delete").on_click(on_efa(
            this,
            move |g, event, window, cx| {
                g.confirm_delete_fit(i, event.modifiers().shift, window, cx);
            },
        )))
}

/// pyfa's marketItemGroup / marketItemMisc menu.
pub(super) fn market_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    id: TypeId,
    context: MarketContext,
    window: &Window,
    cx: &App,
) -> PopupMenu {
    let shown = crate::ui::hub::shown_menus(window, cx);
    let Some(g) = this.upgrade() else { return menu };
    let g = g.read(cx);
    let Some(loaded) = g.loaded() else {
        return menu;
    };
    let Some(t) = loaded.catalog.get(id) else {
        return menu;
    };
    // A right-click inside a selection of several acts on all of them.
    let ids: Rc<[TypeId]> = g.market_selection.targets(id).into();
    let several = ids.len() > 1;
    let category = if several {
        format!("{} Items", ids.len())
    } else {
        t.category_label().to_string()
    };
    let has_fit = g.fit().is_some_and(|f| f.ship.is_some());
    let structure = g
        .fit()
        .and_then(|f| f.ship)
        .and_then(|s| loaded.catalog.get(s))
        .is_some_and(crate::data::ItemType::is_structure);
    // Adding an item from here counts as using it, as pyfa's commands remember it: for every
    // selected item, or the one clicked.
    let use_each = |f: fn(&mut Efa, TypeId, &mut Context<Efa>)| {
        on_each(this, &ids, move |g, id, _, cx| {
            g.used_from_market(id);
            f(g, id, cx);
        })
    };
    let apply = |action: Action| {
        on_efa(this, move |g, _, _, cx| {
            g.used_from_market(id);
            g.apply(action.clone(), cx);
        })
    };

    let mut menu = menu.item(item_stats::stats_entry(
        this,
        t.category_label(),
        item_stats::StatsOf::Type(id),
    ));
    // Ice products are found by search but sit outside the market tree, so there is no group.
    let listed = t
        .market_group
        .is_some_and(|g| loaded.catalog.market_groups.contains_key(&g));
    if context == MarketContext::Search && listed && !several {
        menu = menu.item(
            entry(IconName::Search, format!("{category} Market Group")).on_click(on_efa(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(id, window, cx);
                },
            )),
        );
    }
    if !has_fit {
        return menu.item(
            entry(IconName::Plus, format!("Add {category} to Fit"))
                .on_click(use_each(Efa::add_item)),
        );
    }
    if several {
        menu = menu.item(
            entry(IconName::Plus, format!("Add {category} to Fit"))
                .on_click(use_each(Efa::add_item)),
        );
    }
    if let Kind::Module(slot) = t.kind
        && slot != Slot::Subsystem
        && shown.module_fill
        && !several
    {
        menu = menu.item(
            entry(IconName::Plus, "Fill With Module").on_click(apply(Action::FillRack(slot, id))),
        );
    }
    if t.kind == Kind::Drone && !several {
        let amount = g.drone_stack_size(&loaded, id);
        if amount >= 1 {
            let label = if amount == 1 {
                format!("Add {} to Drone Bay", t.name)
            } else {
                format!("Add {} to Drone Bay (x{amount})", t.name)
            };
            menu = menu
                .item(entry(IconName::Plus, label).on_click(apply(Action::AddDrones(id, amount))));
        }
    }
    let to_cargo = |count: u32| {
        on_each(this, &ids, move |g, id, _, cx| {
            g.used_from_market(id);
            g.add_to_cargo(id, count, cx);
        })
    };
    // pyfa offers structures only charges for their cargo.
    if !structure || t.kind == Kind::Charge {
        menu = menu
            .item(entry(IconName::Inbox, format!("Add {category} to Cargo")).on_click(to_cargo(1)));
    }
    if goes_to_cargo(t.kind) && shown.cargo_fill && !several {
        menu = menu.item(
            entry(IconName::Inbox, format!("Fill Cargo With {category}"))
                .on_click(use_each(Efa::fill_cargo_with)),
        );
    }
    if t.kind == Kind::Charge {
        let is_probe = t
            .market_group
            .and_then(|m| loaded.catalog.market_groups.get(&m))
            .is_some_and(|m| m.name == "Scan Probes");
        let batch = if is_probe { PROBE_BATCH } else { AMMO_BATCH };
        menu = menu.item(
            entry(
                IconName::Inbox,
                format!("Add {category} to Cargo (x{batch})"),
            )
            .on_click(to_cargo(batch)),
        );
    }
    if shown.ammo_pattern && t.damage.iter().any(|&d| d > 0.0) && !several {
        let profile = crate::fit::DamageProfile::Pattern {
            name: crate::damage_pattern::SELECTED_AMMO.to_owned(),
            amounts: t.damage,
        };
        menu = menu.item(
            entry(
                IconName::ChartPie,
                format!("Set {category} as Damage Pattern"),
            )
            .on_click(on_efa(this, move |g, _, _, cx| {
                g.apply(Action::SetDamageProfile(profile.clone()), cx);
            })),
        );
    }
    if shown.project && !structure && loaded.catalog.projection.projectable.contains(&id) {
        menu = menu.item(
            entry(IconName::Globe, format!("Project {category} onto Fit")).on_click(use_each(
                |g, id, cx| {
                    g.project_item(id, cx);
                    g.show_additions_tab(AdditionsTab::Projected);
                    cx.notify();
                },
            )),
        );
    }
    menu
}
