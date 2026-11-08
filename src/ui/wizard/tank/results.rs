//! The found tanks as a sortable list, each with its modules and numbers.

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Icon as UiIcon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, InteractiveElement, IntoElement, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
    px,
};

use super::text::{delta, goal_number, goal_unit, limit_text};
use super::{Sort, TankSection};
use crate::ui::wizard::notice;
use crate::{
    data::TypeId,
    engine::Resource,
    icons::Icon,
    ui::{
        columns, tooltips,
        widgets::{self, TipExt, empty_state},
    },
    wizard::tank::{Goal, Outcome},
};

/// Icons in a result row.
const ICON: f32 = 18.;
/// Result columns: the goal number with its change, the other two numbers, fitting and price.
const GOAL_W: f32 = 96.;
const OTHER_W: f32 = 52.;
const FITTING_W: f32 = 44.;
const PRICE_W: f32 = 56.;

impl TankSection {
    pub(super) fn results(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.shared.palette;
        let Some(searched) = &self.searched else {
            return self.empty_results(cx);
        };
        let busy = self.busy();
        let goal = searched.choices.goal;
        let rows: Vec<AnyElement> = self
            .order()
            .into_iter()
            .enumerate()
            .map(|(rank, i)| self.result_row(rank, i, &searched.found.outcomes[i], goal, cx))
            .collect();
        let found = &searched.found;
        let notes: Vec<SharedString> = [
            found.repairers_short.map(|got| {
                SharedString::from(match got {
                    0 => "No repairer fits with the rest of the fit; these tank without one."
                        .to_owned(),
                    1 => "Only 1 more repairer fits with the rest of the fit.".to_owned(),
                    n => format!("Only {n} more repairers fit with the rest of the fit."),
                })
            }),
            found.rejected.limit().map(limit_text),
        ]
        .into_iter()
        .flatten()
        .collect();
        v_flex()
            .gap_2()
            .when(self.stale() && !busy, |d| {
                d.child(div().px_2().child(notice(
                    AssetIcon::RotateCcw,
                    palette.yellow,
                    "The choices changed since these were found; search again for fresh ones.",
                    cx,
                )))
            })
            .child(
                v_flex()
                    // While a new search runs the old results stay, dimmed.
                    .when(busy, |d| d.opacity(0.45))
                    .child(self.results_head(goal, cx))
                    .children(rows),
            )
            .children(notes.into_iter().map(|n| {
                div()
                    .px_2()
                    .child(notice(AssetIcon::Info, palette.blue, n, cx))
            }))
    }

    /// The column titles in the fitting view's table head; those the list sorts by are buttons.
    fn results_head(&self, goal: Goal, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let right = |text: &'static str, width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .text_right()
                .child(text)
                .into_any_element()
        };
        let sort_head = |sort: Sort, text: &'static str, width: f32| {
            let on = self.sort == sort;
            h_flex()
                .id(SharedString::from(format!("wizard-sort-{sort:?}")))
                .w(px(width))
                .flex_none()
                .justify_end()
                .gap_0p5()
                .cursor_pointer()
                .when(on, |d| d.text_color(theme.foreground))
                .hover(|h| h.text_color(theme.foreground))
                .when(on, |d| d.child(UiIcon::new(AssetIcon::ArrowDown).xsmall()))
                .child(text)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &gpui_kit::MouseDownEvent, window, cx| {
                        this.sort = sort;
                        cx.notify();
                        this.refocus(window, cx);
                    }),
                )
                .into_any_element()
        };
        let others: Vec<AnyElement> = Goal::ALL
            .into_iter()
            .filter(|&g| g != goal)
            .map(|g| match g {
                Goal::Ehp => sort_head(Sort::Ehp, "EHP", OTHER_W),
                g => right(goal_unit(g), OTHER_W),
            })
            .collect();
        widgets::table_head(cx)
            .child(div().w(px(columns::STATE_W)).flex_none().child("#"))
            .child(div().flex_1().min_w_0().child("Modules"))
            .child(sort_head(Sort::Goal, goal_unit(goal), GOAL_W))
            .children(others)
            .child(right("Fitting", FITTING_W))
            .child(sort_head(Sort::Price, "Price", PRICE_W))
    }

    fn empty_results(&self, cx: &App) -> Div {
        if self.busy() {
            empty_state(
                AssetIcon::LoaderCircle,
                "Searching",
                "The best tanks show up here.",
                cx,
            )
        } else {
            empty_state(
                AssetIcon::Sparkles,
                "No tanks yet",
                "Pick a goal and what the search may use, then Find best tanks.",
                cx,
            )
        }
    }

    /// One found tank as a list row: what it puts on, its goal number with the change against
    /// now, the other two numbers, how full the fitting gets and the price.
    fn result_row(
        &self,
        rank: usize,
        i: usize,
        o: &Outcome,
        goal: Goal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let palette = self.shared.palette;
        let n = &o.numbers;
        let value = n.value(goal);
        let (delta_text, delta_color) = delta(goal, self.now.value(goal), value, palette, cx);
        let others: Vec<String> = Goal::ALL
            .into_iter()
            .filter(|&g| g != goal)
            .map(|g| goal_number(g, n.value(g)))
            .collect();
        // The fuller of CPU and powergrid, as a share of the ship's output.
        let share = |r: &Resource| r.output.filter(|&o| o > 0.0).map_or(0.0, |o| r.used / o);
        let full = share(&n.cpu).max(share(&n.powergrid));
        let fitting_tip = SharedString::from(format!(
            "CPU {:.0} / {:.0}, powergrid {:.0} / {:.0}",
            n.cpu.used,
            n.cpu.output.unwrap_or(0.0),
            n.powergrid.used,
            n.powergrid.output.unwrap_or(0.0)
        ));
        let price = if o.price > 0.0 {
            format!(
                "{}{}",
                if o.priced { "" } else { "~" },
                widgets::short(o.price)
            )
        } else {
            "?".to_owned()
        };
        widgets::list_row(
            SharedString::from(format!("wizard-row-{i}")),
            self.selected == Some(i),
            cx,
        )
        .cursor_pointer()
        .child(
            div()
                .w(px(columns::STATE_W))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(format!("{}", rank + 1)),
        )
        .child(self.module_strip(i, o, cx))
        .child(
            h_flex()
                .w(px(GOAL_W))
                .flex_none()
                .justify_end()
                .gap_1()
                .child(div().text_xs().text_color(delta_color).child(delta_text))
                .child(goal_number(goal, value)),
        )
        .children(others.into_iter().map(|v| {
            div()
                .w(px(OTHER_W))
                .flex_none()
                .text_right()
                .text_color(theme.muted_foreground)
                .child(v)
        }))
        .child(
            div()
                .id(SharedString::from(format!("wizard-row-{i}-fit")))
                .w(px(FITTING_W))
                .flex_none()
                .text_right()
                .when(full > 0.99, |d| {
                    d.text_color(widgets::color(palette.yellow))
                })
                .when(full <= 0.99, |d| d.text_color(theme.muted_foreground))
                .child(format!("{:.0}%", full * 100.0))
                .tip(fitting_tip),
        )
        .child(
            div()
                .w(px(PRICE_W))
                .flex_none()
                .text_right()
                .when(!o.priced, |d| d.text_color(theme.muted_foreground))
                .child(price),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.select(i, cx);
            this.refocus(window, cx);
        }))
        .into_any_element()
    }

    /// What a found tank puts on, as icons with their counts, rigs after the modules;
    /// hovering one shows the item.
    fn module_strip(&self, i: usize, o: &Outcome, cx: &App) -> Div {
        let theme = cx.theme();
        let loaded = &self.shared.loaded;
        let mut lines: Vec<(TypeId, usize)> = Vec::new();
        for &id in o.types.iter().chain(&o.rig_types) {
            match lines.iter_mut().find(|(t, _)| *t == id) {
                Some((_, n)) => *n += 1,
                None => lines.push((id, 1)),
            }
        }
        h_flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .gap_2()
            .children(lines.into_iter().map(|(id, n)| {
                let tip = Rc::clone(loaded);
                let price = self.prices.get(id);
                h_flex()
                    .id(SharedString::from(format!("wizard-row-{i}-{id}")))
                    .flex_none()
                    .gap_0p5()
                    .child(widgets::icon_box(
                        &loaded.catalog,
                        &loaded.icons,
                        Icon::Type(id),
                        ICON,
                    ))
                    .when(n > 1, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{n}")),
                        )
                    })
                    .tooltip(move |window, cx| tooltips::item_tooltip(&tip, id, price, window, cx))
            }))
    }
}
