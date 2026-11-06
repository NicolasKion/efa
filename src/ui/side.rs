//! The left side: market browser and saved fittings.

use gpui_kit::component::{
    ActiveTheme, h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::{BrowserTab, Gyfa, Loaded, widgets::icon_box};
use crate::{data::TypeId, icons::Icon};

/// Search results shown at most, to keep the list quick to draw.
const SEARCH_LIMIT: usize = 300;
const INDENT: f32 = 14.;

impl Gyfa {
    pub(super) fn browser(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(match self.browser_tab {
                BrowserTab::Market => self.market(loaded, cx).into_any_element(),
                BrowserTab::Fittings => self.fittings(loaded, cx).into_any_element(),
            })
    }

    fn market(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        if self.search_text.trim().len() >= 2 {
            for id in loaded.catalog.search(&self.search_text, SEARCH_LIMIT) {
                rows.push(Self::item_row(loaded, id, 0, cx));
            }
        } else {
            for &root in &loaded.catalog.market_roots {
                self.market_group(loaded, root, 0, &mut rows, cx);
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
                    .children(rows),
            )
    }

    fn market_group(
        &self,
        loaded: &Loaded,
        id: i32,
        depth: usize,
        rows: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        let Some(group) = loaded.catalog.market_groups.get(&id) else {
            return;
        };
        let expanded = self.expanded_groups.contains(&id);
        rows.push(
            h_flex()
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
        );
        if !expanded {
            return;
        }
        for &child in &group.children {
            self.market_group(loaded, child, depth + 1, rows, cx);
        }
        for &t in &group.types {
            rows.push(Self::item_row(loaded, t, depth + 1, cx));
        }
    }

    fn item_row(loaded: &Loaded, id: TypeId, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        let name = loaded.catalog.name(id);
        h_flex()
            .id(SharedString::from(format!("item-{id}")))
            .gap_2()
            .h(px(26.))
            .pl(px(18. + INDENT * depth as f32))
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
                    this.add_item(id, cx);
                }
            }))
            .context_menu({
                let this = Self::weak(cx);
                move |menu, _, _| {
                    let this = this.clone();
                    menu.item(PopupMenuItem::new("Add to fit").on_click(move |_, _, cx| {
                        Self::app_update(&this, cx, |this, cx| this.add_item(id, cx));
                    }))
                }
            })
            .into_any_element()
    }

    fn fittings(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let mut by_ship: Vec<(String, usize)> = self
            .saved
            .fits
            .iter()
            .enumerate()
            .map(|(i, f)| {
                (
                    f.ship.map_or_else(String::new, |s| loaded.catalog.name(s)),
                    i,
                )
            })
            .collect();
        by_ship.sort();
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
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_fit(i, cx)))
                    .context_menu({
                        let this = Self::weak(cx);
                        move |menu, _, _| {
                            let (open, delete) = (this.clone(), this.clone());
                            menu.item(PopupMenuItem::new("Open").on_click(move |_, _, cx| {
                                Self::app_update(&open, cx, |this, cx| this.open_fit(i, cx));
                            }))
                            .separator()
                            .item(
                                PopupMenuItem::new("Delete").on_click(move |_, _, cx| {
                                    Self::app_update(&delete, cx, |this, cx| {
                                        this.delete_fit(i, cx);
                                    });
                                }),
                            )
                        }
                    })
            }))
    }
}
