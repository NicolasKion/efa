//! The picked tank: what it changes, the fit with it in the stats sections, its modules, and the
//! footer that fits it.

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::text::{delta, goal_number, goal_text_short};
use super::{Picked, TankSection};
use crate::ui::wizard::Target;
use crate::{
    data::{Slot, TypeId},
    icons::Icon,
    ui::{
        columns,
        stats::{self, SectionKind},
        tooltips,
        widgets::{self, TipExt, empty_state},
    },
    wizard::{Change, tank::Goal},
};

/// What a module line in a preview is to the fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Added,
    Kept,
    Removed,
}

impl TankSection {
    /// The picked tank: what it changes against the fit as it is, then the fit with it in the
    /// main window's resources, resistances and recharge sections, then its modules: under them
    /// in a `column`, else beside them.
    pub(super) fn detail(&self, column: bool, cx: &App) -> Div {
        let (Some(picked), Some(i)) = (&self.after, self.selected) else {
            return empty_state(
                AssetIcon::MousePointerClick,
                "No tank picked",
                "Pick a result to see the fit with it.",
                cx,
            );
        };
        let shared = &self.shared;
        let pane = stats::Pane::plain(
            &shared.loaded,
            &picked.stats,
            &picked.fit,
            shared.palette,
            &self.prices,
        );
        let sections = stats::render_sections(
            &pane,
            &[
                SectionKind::Resources,
                SectionKind::Resistances,
                SectionKind::Recharge,
            ],
            None,
            cx,
        );
        // Under the results the stats keep the stats pane's width, with the modules beside them
        // when there is room.
        let stats = v_flex()
            .gap_2()
            .text_size(px(stats::TEXT_SIZE))
            .child(self.changes(i, picked, cx))
            .children(sections);
        let modules = self.module_list(&picked.change, cx);
        if column {
            v_flex().gap_2().child(stats).child(modules)
        } else {
            h_flex()
                .items_start()
                .flex_wrap()
                .gap_x_4()
                .gap_y_2()
                .child(stats.w(px(stats::PANE_WIDTH)).flex_none())
                .child(modules.flex_1().min_w(px(stats::PANE_WIDTH)))
        }
    }

    /// The picked tank's numbers against the fit's now, in a line under its name.
    fn changes(&self, i: usize, picked: &Picked, cx: &App) -> Div {
        let theme = cx.theme();
        let palette = self.shared.palette;
        let layer = self
            .searched
            .as_ref()
            .map_or(self.choices.layer, |s| s.choices.layer)
            .label();
        let rank = self.order().iter().position(|&o| o == i).unwrap_or(i) + 1;
        let (now, after) = (&self.now, &picked.numbers);
        let item = |goal: Goal, label: String| {
            let (before, value) = (now.value(goal), after.value(goal));
            let (text, color) = delta(goal, before, value, palette, cx);
            let tip = SharedString::from(format!(
                "{label}: {} now, {} with this tank",
                goal_number(goal, before),
                goal_number(goal, value)
            ));
            h_flex()
                .id(SharedString::from(format!("wizard-change-{goal:?}")))
                .gap_1()
                .child(div().text_color(theme.muted_foreground).child(label))
                .child(div().text_color(color).child(text))
                .tip(tip)
        };
        let price = self
            .outcomes()
            .get(i)
            .map_or_default(|o| (o.price, o.priced));
        let cap_lost = now.cap_stable && !after.cap_stable;
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .child(widgets::section_title(format!("Tank {rank}")))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(goal_text_short(self.choices.goal)),
                    )
                    .child(div().flex_1())
                    .when(price.0 > 0.0, |d| {
                        d.child(format!(
                            "{}{} ISK",
                            if price.1 { "" } else { "~" },
                            widgets::short(price.0)
                        ))
                    }),
            )
            .child(
                h_flex()
                    .pl_1()
                    .flex_wrap()
                    .gap_x_3()
                    .child(item(Goal::Ehp, "EHP".to_owned()))
                    .child(item(Goal::Resists, format!("{layer} resists")))
                    .child(item(Goal::Repair, format!("{layer} repair")))
                    .when(cap_lost, |d| {
                        d.child(
                            div()
                                .text_color(widgets::color(palette.yellow))
                                .child("Capacitor runs out"),
                        )
                    }),
            )
    }

    /// The racks the change touches under the fitting view's rack labels: what goes on, what
    /// stays and what comes off, with the price of what goes on.
    fn module_list(&self, change: &Change, cx: &App) -> Div {
        let shared = &self.shared;
        let fit = &shared.fit;
        let slot = self.choices.layer.slot();
        let removed = |s: Slot, at: usize| change.removed.contains(&(s, at));
        let placed = |s: Slot| change.placements.iter().filter(move |p| p.0 == s);
        let mut out = v_flex();
        for rack in [slot, Slot::Rig] {
            let fitted: Vec<(TypeId, bool)> = if rack == Slot::Rig {
                fit.rigs
                    .iter()
                    .enumerate()
                    .filter_map(|(at, r)| Some(((*r)?, removed(rack, at))))
                    .collect()
            } else {
                fit.rack(rack)
                    .iter()
                    .enumerate()
                    .filter_map(|(at, m)| Some((m.as_ref()?.type_id, removed(rack, at))))
                    .collect()
            };
            let mut lines: Vec<(TypeId, Mark, usize)> = Vec::new();
            let mut add = |id: TypeId, mark: Mark| match lines
                .iter_mut()
                .find(|l| l.0 == id && l.1 == mark)
            {
                Some(l) => l.2 += 1,
                None => lines.push((id, mark, 1)),
            };
            for p in placed(rack) {
                add(p.2.type_id, Mark::Added);
            }
            for &(id, off) in &fitted {
                add(id, if off { Mark::Removed } else { Mark::Kept });
            }
            // A module that comes off and goes back on stays.
            let mut net: Vec<(TypeId, Mark, usize)> = Vec::new();
            for &(id, mark, n) in &lines {
                let opposite = match mark {
                    Mark::Added => Mark::Removed,
                    Mark::Removed => Mark::Added,
                    Mark::Kept => Mark::Kept,
                };
                let other = if mark == Mark::Kept {
                    0
                } else {
                    lines
                        .iter()
                        .find(|l| l.0 == id && l.1 == opposite)
                        .map_or(0, |l| l.2)
                };
                let same = n.min(other);
                if n > same {
                    net.push((id, mark, n - same));
                }
                if mark == Mark::Added && same > 0 {
                    net.push((id, Mark::Kept, same));
                }
            }
            if net.is_empty() {
                continue;
            }
            let total = u32::try_from(shared.facts.total(rack)).unwrap_or(u32::MAX);
            out = out
                .child(widgets::band(
                    h_flex()
                        .h(px(widgets::ROW_H))
                        .px_1()
                        .gap_1()
                        .mt_2()
                        .child(widgets::rack_band_label(&shared.loaded, rack, total, cx)),
                    cx,
                ))
                .children(
                    net.into_iter()
                        .enumerate()
                        .map(|(k, (id, mark, n))| self.module_line(rack, k, id, mark, n, cx)),
                );
        }
        out
    }

    /// A module the change puts on, keeps or takes off, as a row of the fitting view.
    fn module_line(
        &self,
        rack: Slot,
        k: usize,
        id: TypeId,
        mark: Mark,
        n: usize,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let palette = self.shared.palette;
        let loaded = &self.shared.loaded;
        let price = self.prices.get(id);
        let (icon, color, tag) = match mark {
            Mark::Added => (IconName::Plus, widgets::color(palette.green), "on"),
            Mark::Kept => (IconName::Check, theme.muted_foreground, "kept"),
            Mark::Removed => (IconName::Minus, widgets::color(palette.red), "off"),
        };
        let item_tip = Rc::clone(loaded);
        widgets::list_row(
            SharedString::from(format!("wizard-line-{rack:?}-{k}")),
            false,
            cx,
        )
        .child(
            div()
                .w(px(columns::STATE_W))
                .flex_none()
                .flex()
                .items_center()
                .child(UiIcon::new(icon).size(px(15.)).text_color(color)),
        )
        .child(widgets::icon_box(
            &loaded.catalog,
            &loaded.icons,
            Icon::Type(id),
            22.,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(mark == Mark::Removed, |d| {
                    d.line_through().text_color(theme.muted_foreground)
                })
                .child(if n > 1 {
                    format!("{n}x {}", loaded.catalog.name(id))
                } else {
                    loaded.catalog.name(id)
                }),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(match (mark, price) {
                    (Mark::Added, Some(p)) => widgets::short(p * n as f64),
                    _ => tag.to_owned(),
                }),
        )
        .tooltip(move |window, cx| tooltips::item_tooltip(&item_tip, id, price, window, cx))
        .into_any_element()
    }

    pub(super) fn footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let rank = self
            .selected
            .and_then(|s| self.order().iter().position(|&o| o == s));
        let label = match rank {
            Some(r) if self.after.is_some() => format!("Fit tank {}", r + 1),
            _ => "Fit tank".to_owned(),
        };
        h_flex()
            .flex_none()
            .justify_end()
            .gap_2()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .child(
                Button::new("wizard-apply")
                    .small()
                    .primary()
                    .tab_stop(false)
                    .icon(IconName::Check)
                    .label(label)
                    .tooltip(if self.target == Target::Gone {
                        "The fit was deleted"
                    } else if self.after.is_some() {
                        "Enter: fit this tank on the fit, as one undo step"
                    } else {
                        "Find tanks and pick one first"
                    })
                    .disabled(!self.can_apply())
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
            )
    }
}
