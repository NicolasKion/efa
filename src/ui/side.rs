//! The left side: market browser and saved fittings.

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Action, BrowserTab, Gyfa, Loaded,
    additions::goes_to_cargo,
    fitting::{DragPreview, DragSource, Dragged},
    widgets::{entry, icon_box},
};
use crate::{
    data::{Kind, Slot, TypeId},
    icons::Icon,
};

/// Search results shown at most, to keep the list quick to draw.
const SEARCH_LIMIT: usize = 300;
const INDENT: f32 = 14.;
/// Drones that can be in space at once without skills, the floor for a stack's size.
const BASE_ACTIVE_DRONES: u32 = 5;
/// Charges "Add to Cargo" puts in at once, and the smaller batch for scan probes.
const AMMO_BATCH: u32 = 1000;
const PROBE_BATCH: u32 = 8;

/// A row of the market list, with the item it shows so a jump can find it.
struct MarketRow {
    id: Option<TypeId>,
    element: AnyElement,
}

/// Whether the market list shows search results or browses groups, as pyfa tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketContext {
    Group,
    Search,
}

impl Gyfa {
    pub(super) fn browser(&mut self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.browser_tab {
            BrowserTab::Market => self.market(loaded, cx).into_any_element(),
            BrowserTab::Fittings => self.fittings(loaded, cx).into_any_element(),
        };
        let theme = cx.theme();
        let tab = |id: &'static str, label: &'static str, which: BrowserTab| {
            let selected = self.browser_tab == which;
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded(px(4.))
                .when(selected, |t| {
                    t.bg(theme.tab_active)
                        .text_color(theme.tab_active_foreground)
                })
                .when(!selected, |t| t.text_color(theme.tab_foreground))
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.browser_tab = which;
                    cx.notify();
                }))
        };
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .gap_1()
                    .p_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(tab("tab-market", "Market", BrowserTab::Market))
                    .child(tab("tab-fittings", "Fittings", BrowserTab::Fittings)),
            )
            .child(content)
    }

    fn market(&mut self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<MarketRow> = Vec::new();
        if self.search_text.trim().len() >= 2 {
            for id in loaded.catalog.search(&self.search_text, SEARCH_LIMIT) {
                rows.push(self.item_row(loaded, id, 0, MarketContext::Search, cx));
            }
        } else {
            for &root in &loaded.catalog.market_roots {
                self.market_group(loaded, root, 0, &mut rows, cx);
            }
        }
        if self.market_scroll_pending {
            self.market_scroll_pending = false;
            let target = self.market_highlight;
            if let Some(index) = rows
                .iter()
                .position(|row| row.id.is_some() && row.id == target)
            {
                self.market_scroll.scroll_to_item(index);
            }
        }
        v_flex()
            .flex_1()
            .min_h_0()
            .child(div().p_1().child(self.search_input()))
            .child(
                v_flex()
                    .id("market-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.market_scroll)
                    .children(rows.into_iter().map(|row| row.element)),
            )
    }

    fn market_group(
        &self,
        loaded: &Loaded,
        id: i32,
        depth: usize,
        rows: &mut Vec<MarketRow>,
        cx: &mut Context<Self>,
    ) {
        let Some(group) = loaded.catalog.market_groups.get(&id) else {
            return;
        };
        let expanded = self.expanded_groups.contains(&id);
        rows.push(MarketRow {
            id: None,
            element: h_flex()
                .id(SharedString::from(format!("group-{id}")))
                .gap_1()
                .h(px(22.))
                .pl(px(4. + INDENT * depth as f32))
                .hover(|s| s.bg(cx.theme().list_hover))
                .child(
                    div()
                        .w(px(10.))
                        .text_xs()
                        .child(if expanded { "▾" } else { "▸" }),
                )
                .when_some(group.icon_id, |row, icon_id| {
                    row.child(icon_box(
                        &loaded.catalog,
                        &loaded.icons,
                        Icon::Id(icon_id),
                        18.,
                    ))
                })
                .child(group.name.clone())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.expanded_groups.remove(&id) {
                        this.expanded_groups.insert(id);
                    }
                    cx.notify();
                }))
                .into_any_element(),
        });
        if !expanded {
            return;
        }
        for &child in &group.children {
            self.market_group(loaded, child, depth + 1, rows, cx);
        }
        for &t in &group.types {
            rows.push(self.item_row(loaded, t, depth + 1, MarketContext::Group, cx));
        }
    }

    fn item_row(
        &self,
        loaded: &Loaded,
        id: TypeId,
        depth: usize,
        context: MarketContext,
        cx: &mut Context<Self>,
    ) -> MarketRow {
        let name = loaded.catalog.name(id);
        let highlighted = self.market_highlight == Some(id);
        let element = h_flex()
            .id(SharedString::from(format!("item-{id}")))
            .gap_2()
            .h(px(26.))
            .pl(px(18. + INDENT * depth as f32))
            .when(highlighted, |row| row.bg(cx.theme().list_active))
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(id),
                24.,
            ))
            .child(div().truncate().child(name))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.market_double_click(id, event.modifiers(), cx);
                }
            }))
            .when_some(
                Dragged::of(loaded, DragSource::Market, Some(id)),
                |row, dragged| {
                    row.on_drag(dragged, |dragged, _, _, cx| {
                        cx.new(|_| DragPreview(dragged.clone()))
                    })
                },
            )
            .context_menu({
                let this = Self::weak(cx);
                move |menu, _, cx| market_menu(menu, &this, id, context, cx)
            })
            .into_any_element();
        MarketRow {
            id: Some(id),
            element,
        }
    }

    fn fittings(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let mut by_ship: Vec<(String, usize)> = self
            .saved
            .fits
            .iter()
            .enumerate()
            .filter(|(_, f)| self.fittings_ship.is_none() || f.ship == self.fittings_ship)
            .map(|(i, f)| {
                (
                    f.ship.map_or_else(String::new, |s| loaded.catalog.name(s)),
                    i,
                )
            })
            .collect();
        by_ship.sort();
        let filter = self.fittings_ship.map(|ship| {
            h_flex()
                .id("fittings-filter")
                .gap_2()
                .p_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Type(ship),
                    20.,
                ))
                .child(div().flex_1().child(loaded.catalog.name(ship)))
                .child(
                    div()
                        .id("fittings-show-all")
                        .px_1()
                        .text_color(cx.theme().link)
                        .child("Show all")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.fittings_ship = None;
                            cx.notify();
                        })),
                )
        });
        v_flex().flex_1().min_h_0().children(filter).child(
            v_flex()
                .id("fittings-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .when(by_ship.is_empty(), |list| {
                    list.child(
                        div()
                            .p_2()
                            .text_color(cx.theme().muted_foreground)
                            .child("No fits yet."),
                    )
                })
                .children(by_ship.into_iter().map(|(ship_name, i)| {
                    let fit = &self.saved.fits[i];
                    h_flex()
                        .id(SharedString::from(format!("fitting-{i}")))
                        .gap_2()
                        .p_1()
                        .hover(|s| s.bg(cx.theme().list_hover))
                        .when_some(Self::fitting_tooltip(fit, &ship_name), |row, tip| {
                            row.tooltip(move |window, cx| {
                                Tooltip::new(tip.clone()).build(window, cx)
                            })
                        })
                        .when_some(fit.ship, |row, ship| {
                            row.child(icon_box(
                                &loaded.catalog,
                                &loaded.icons,
                                Icon::Type(ship),
                                32.,
                            ))
                        })
                        .child(
                            v_flex().child(fit.name.clone()).child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(ship_name),
                            ),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.open_fit(i, cx);
                        }))
                        .context_menu({
                            let this = Self::weak(cx);
                            move |menu, _, _| {
                                let (open, delete) = (this.clone(), this.clone());
                                menu.item(entry(IconName::FolderOpen, "Open in new tab").on_click(
                                    move |_, _, cx| {
                                        Self::app_update(&open, cx, |this, cx| {
                                            this.open_fit(i, cx);
                                        });
                                    },
                                ))
                                .separator()
                                .item(
                                    entry(IconName::Delete, "Delete").on_click(move |_, _, cx| {
                                        Self::app_update(&delete, cx, |this, cx| {
                                            this.delete_fit(i, cx);
                                        });
                                    }),
                                )
                            }
                        })
                })),
        )
    }

    /// How many of a drone one "Add to Drone Bay" adds, as pyfa sizes it: no more than can be in
    /// space at once, and no more than the bandwidth can run.
    pub(super) fn drone_stack_size(&self, loaded: &Loaded, drone: TypeId) -> u32 {
        let Some(stats) = &self.stats else { return 0 };
        let hard_limit = BASE_ACTIVE_DRONES.max(stats.drones_launched.total.unwrap_or(0));
        let bandwidth = stats.drone_bandwidth.output.unwrap_or(0.0);
        let per_drone = loaded.catalog.get(drone).map_or(0.0, |t| t.drone_bandwidth);
        if per_drone > 0.0 {
            hard_limit.min((bandwidth / per_drone).floor() as u32)
        } else {
            hard_limit
        }
    }

    /// Units of an item that still fit in the cargo hold.
    pub(super) fn cargo_room_for(&self, loaded: &Loaded, item: TypeId) -> u32 {
        let (Some(fit), Some(stats)) = (self.fit(), &self.stats) else {
            return 0;
        };
        let volume = loaded.catalog.get(item).map_or(0.0, |t| t.volume);
        let free = stats.cargo_capacity.unwrap_or(0.0) - fit.cargo_volume(&loaded.catalog);
        if volume <= 0.0 || free <= 0.0 {
            return 0;
        }
        (free / volume).floor() as u32
    }
}

/// pyfa's marketItemGroup / marketItemMisc menu.
fn market_menu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    id: TypeId,
    context: MarketContext,
    cx: &App,
) -> PopupMenu {
    let Some(g) = this.upgrade() else { return menu };
    let g = g.read(cx);
    let Some(loaded) = g.loaded() else {
        return menu;
    };
    let Some(t) = loaded.catalog.get(id) else {
        return menu;
    };
    let category = t.category_label();
    let has_fit = g.fit().is_some_and(|f| f.ship.is_some());
    let on = |f: fn(&mut Gyfa, TypeId, &mut Window, &mut Context<Gyfa>)| {
        let this = this.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| f(g, id, window, cx));
        }
    };
    let apply = |action: Action| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| g.apply(action.clone(), cx));
        }
    };

    let mut menu = menu;
    // Ice products are found by search but sit outside the market tree, so there is no group.
    let listed = t
        .market_group
        .is_some_and(|g| loaded.catalog.market_groups.contains_key(&g));
    if context == MarketContext::Search && listed {
        menu = menu.item(
            entry(IconName::Search, format!("{category} Market Group"))
                .on_click(on(super::Gyfa::jump_to_market)),
        );
    }
    if !has_fit {
        return menu.item(
            entry(IconName::Plus, format!("Add {category} to Fit"))
                .on_click(on(|g, id, _, cx| g.add_item(id, cx))),
        );
    }
    if let Kind::Module(slot) = t.kind
        && slot != Slot::Subsystem
    {
        menu = menu.item(
            entry(IconName::Plus, "Fill With Module").on_click(apply(Action::FillRack(slot, id))),
        );
    }
    if t.kind == Kind::Drone {
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
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| g.add_to_cargo(id, count, cx));
        }
    };
    menu =
        menu.item(entry(IconName::Inbox, format!("Add {category} to Cargo")).on_click(to_cargo(1)));
    if goes_to_cargo(t.kind) {
        menu = menu.item(
            entry(IconName::Inbox, format!("Fill Cargo With {category}"))
                .on_click(on(|g, id, _, cx| g.fill_cargo_with(id, cx))),
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
    if t.damage.iter().any(|&d| d > 0.0) {
        let profile = crate::fit::DamageProfile::Pattern {
            name: crate::damage_pattern::SELECTED_AMMO.to_owned(),
            amounts: t.damage,
        };
        menu = menu.item(
            entry(
                IconName::ChartPie,
                format!("Set {category} as Damage Pattern"),
            )
            .on_click(apply(Action::SetDamageProfile(profile))),
        );
    }
    menu
}
