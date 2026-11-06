//! The Compare tab: the item's variations side by side, on the attributes where they differ,
//! with their prices (gui/builtinItemStatsViews/itemCompare.py).

use std::collections::HashSet;

use gpui_kit::component::{
    ActiveTheme, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::{ItemStats, ROW_H, format};
use crate::{
    data::{Catalog, TypeId},
    item_data::ItemData,
    prices::Prices,
};

const NAME_W: f32 = 200.;
const ATTR_W: f32 = 120.;
const PRICE_W: f32 = 80.;

/// Sorting, the raw toggle and highlighted rows.
#[derive(Debug, Default)]
pub struct State {
    /// The column sorted by, and whether it is reversed.
    sort: Option<(usize, bool)>,
    raw: bool,
    highlighted: HashSet<TypeId>,
}

/// The attributes worth a column: those with a display name on which the items differ.
pub fn columns(data: &ItemData, items: &[TypeId]) -> Vec<i32> {
    let mut attrs: Vec<i32> = Vec::new();
    for &item in items {
        let Some(info) = data.type_info(item) else {
            continue;
        };
        for &(attr, _) in &info.attrs {
            let named = data.attr(attr).is_some_and(|a| a.display_name.is_some());
            if named && !attrs.contains(&attr) {
                attrs.push(attr);
            }
        }
    }
    attrs.retain(|&attr| {
        let values: Vec<Option<f64>> = items
            .iter()
            .map(|&i| data.type_info(i).and_then(|t| t.attr(attr)))
            .collect();
        let first = values.first().copied().flatten();
        values.iter().any(|v| v.is_none() || *v != first)
    });
    attrs
}

/// The items in pyfa's order: meta level and name by default, else by the clicked column.
fn sorted(
    data: &ItemData,
    prices: &Prices,
    items: &[TypeId],
    attrs: &[i32],
    sort: Option<(usize, bool)>,
    catalog: &Catalog,
) -> Vec<TypeId> {
    let mut items = items.to_vec();
    let Some((column, reverse)) = sort else {
        return items;
    };
    let value = |item: TypeId, attr: i32| {
        data.type_info(item)
            .and_then(|t| t.attr(attr))
            .unwrap_or(0.0)
    };
    match column {
        0 => items.sort_by_key(|&i| catalog.name(i)),
        c if c <= attrs.len() => {
            let attr = attrs[c - 1];
            items.sort_by(|&a, &b| value(a, attr).total_cmp(&value(b, attr)));
        }
        _ => {
            let price = |i: TypeId| prices.get(i).filter(|&p| p != 0.0).unwrap_or(f64::INFINITY);
            items.sort_by(|&a, &b| price(a).total_cmp(&price(b)));
        }
    }
    if reverse {
        items.reverse();
    }
    items
}

impl ItemStats {
    pub(super) fn compare_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let catalog = &self.loaded.catalog;
        let prices = std::rc::Rc::clone(&self.hub.read(cx).prices);
        let items = catalog.variations_of(self.type_id).to_vec();
        let attrs = columns(data, &items);
        let order = sorted(data, &prices, &items, &attrs, self.compare.sort, catalog);
        let raw = self.compare.raw;
        let theme = cx.theme().clone();

        let header_cell = |n: usize, label: String, width: f32| {
            div()
                .id(SharedString::from(format!("compare-head-{n}")))
                .flex_none()
                .w(px(width))
                .px_1()
                .truncate()
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.compare.sort = match this.compare.sort {
                        Some((c, reverse)) if c == n => Some((c, !reverse)),
                        _ => Some((n, false)),
                    };
                    cx.notify();
                }))
        };
        let mut head = h_flex()
            .h(px(ROW_H))
            .bg(theme.table_head)
            .text_color(theme.table_head_foreground)
            .child(header_cell(0, "Item".into(), NAME_W));
        for (n, &attr) in attrs.iter().enumerate() {
            let label = data
                .attr(attr)
                .and_then(|a| a.display_name.clone())
                .unwrap_or_else(|| attr.to_string());
            head = head.child(header_cell(n + 1, label, ATTR_W));
        }
        head = head.child(header_cell(attrs.len() + 1, "Price".into(), PRICE_W));

        let rows = order.iter().enumerate().map(|(n, &item)| {
            let highlighted = self.compare.highlighted.contains(&item);
            let info = data.type_info(item);
            let mut row = h_flex()
                .id(SharedString::from(format!("compare-{item}")))
                .h(px(ROW_H))
                .when(n % 2 == 1, |r| r.bg(theme.table_even))
                .when(highlighted, |r| {
                    r.bg(theme.table_active).font_weight(FontWeight::BOLD)
                })
                .hover(|s| s.bg(theme.table_hover))
                .child(
                    div()
                        .flex_none()
                        .w(px(NAME_W))
                        .px_1()
                        .truncate()
                        .child(catalog.name(item)),
                );
            for &attr in &attrs {
                let text = info.and_then(|i| i.attr(attr)).map(|v| {
                    if raw {
                        format::raw(v)
                    } else {
                        format::value(data, catalog, data.attr(attr), v)
                    }
                });
                row = row.child(
                    div()
                        .flex_none()
                        .w(px(ATTR_W))
                        .px_1()
                        .truncate()
                        .children(text),
                );
            }
            row.child(
                div()
                    .flex_none()
                    .w(px(PRICE_W))
                    .px_1()
                    .truncate()
                    .children(prices.get(item).filter(|&p| p != 0.0).map(format::isk)),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    if !this.compare.highlighted.remove(&item) {
                        this.compare.highlighted.insert(item);
                    }
                    cx.notify();
                }
            }))
        });
        let min_w = NAME_W + ATTR_W * attrs.len() as f32 + PRICE_W;
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("compare-scroll-x")
                    .flex_1()
                    .min_h_0()
                    .overflow_x_scroll()
                    .child(
                        v_flex().h_full().min_w(px(min_w)).child(head).child(
                            v_flex()
                                .id("compare-rows")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .children(rows),
                        ),
                    ),
            )
            .child(
                Self::button_row(cx).child(
                    Button::new("compare-raw")
                        .small()
                        .ghost()
                        .label("Toggle view mode")
                        .selected(raw)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.compare.raw = !this.compare.raw;
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }
}
