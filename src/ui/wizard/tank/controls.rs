//! The tank section's controls column: goal, layer, repairers, module kinds, budgets and the search
//! button.

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
    px,
};

use super::TankSection;
use super::text::{goal_text, slot_word};
use crate::ui::wizard::{field, fieldset, join_and, muted, notice};
use crate::{
    fit::DamageProfile,
    ui::widgets::{self, Segment, TipExt, count, segmented},
    wizard::{Budget, Layer, SearchGroup, tank::Goal},
};

/// The most repairers the stepper goes to.
const MAX_REPAIRERS: u8 = 6;

impl TankSection {
    pub(super) fn controls(&self, across: bool, cx: &mut Context<Self>) -> Div {
        let _ = across;
        let c = self.choices;
        let mut search = vec![
            field("Goal", self.goals(cx), cx).into_any_element(),
            field("Layer", self.layers(cx), cx).into_any_element(),
        ];
        if c.goal == Goal::Repair {
            search.push(field("Repairers", self.repairers(cx), cx).into_any_element());
        }
        search.push(
            field(
                "Slots",
                segmented(
                    "wizard-slots",
                    vec![
                        Segment::new("Refit", c.refit).tip(
                            "Take off the tank modules and rigs fitted on the layer, and \
                             search their slots too",
                        ),
                        Segment::new("Free only", !c.refit)
                            .tip("Keep what is fitted and search only the free slots"),
                    ],
                    cx,
                    |this: &mut Self, i, window, cx| {
                        this.change(cx, |c| c.refit = i == 0);
                        this.refocus(window, cx);
                    },
                ),
                cx,
            )
            .into_any_element(),
        );
        v_flex()
            .gap_4()
            .child(fieldset("Search", search, cx))
            .child(fieldset("Module kinds", vec![self.kinds(cx)], cx))
            .child(fieldset(
                "Budget",
                vec![
                    self.budget(false, cx).into_any_element(),
                    self.budget(true, cx).into_any_element(),
                ],
                cx,
            ))
    }

    /// The goals side by side; what each aims for is in its tooltip.
    fn goals(&self, cx: &mut Context<Self>) -> AnyElement {
        let segments = Goal::ALL
            .into_iter()
            .map(|goal| {
                Segment::new(goal.short_label(), goal == self.choices.goal).tip(goal_text(goal))
            })
            .collect();
        segmented(
            "wizard-goal",
            segments,
            cx,
            |this: &mut Self, i, window, cx| {
                this.change(cx, |c| c.goal = Goal::ALL[i]);
                this.refocus(window, cx);
            },
        )
    }

    fn layers(&self, cx: &mut Context<Self>) -> AnyElement {
        let facts = &self.shared.facts;
        let segments = Layer::ALL
            .iter()
            .map(|&l| {
                let slots = facts.total(l.slot());
                Segment::new(l.label(), l == self.choices.layer).tip(format!(
                    "{} {} slots on this hull",
                    slots,
                    slot_word(l.slot())
                ))
            })
            .collect();
        segmented(
            "wizard-layer",
            segments,
            cx,
            |this: &mut Self, i, window, cx| {
                if this.shared.facts.total(Layer::ALL[i].slot()) > 0 {
                    this.change(cx, |c| c.layer = Layer::ALL[i]);
                }
                this.refocus(window, cx);
            },
        )
    }

    /// The number of repairers as a stepper: Auto, then 1 up.
    fn repairers(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let n = self.choices.repairers;
        let step = |id: &'static str, icon: IconName, back: bool, disabled: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .tab_stop(false)
                .icon(icon)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.change(cx, |c| {
                        c.repairers = match (c.repairers, back) {
                            (None, false) => Some(1),
                            (Some(1) | None, true) => None,
                            (Some(n), true) => Some(n - 1),
                            (Some(n), false) => Some((n + 1).min(MAX_REPAIRERS)),
                        };
                    });
                    this.refocus(window, cx);
                }))
        };
        let tip = if self.choices.refit {
            "How many the tank ends up with; Auto lets the search decide"
        } else {
            "Repairers already fitted count"
        };
        h_flex()
            .id("wizard-repairers")
            .flex_none()
            .w_full()
            .gap_px()
            .rounded(px(4.))
            .border_1()
            .border_color(theme.border)
            .child(step("wizard-rep-less", IconName::Minus, true, n.is_none()))
            .child(
                div()
                    .flex_1()
                    .text_center()
                    .text_xs()
                    .child(n.map_or_else(|| "Auto".to_owned(), |n| n.to_string())),
            )
            .child(step(
                "wizard-rep-more",
                IconName::Plus,
                false,
                n == Some(MAX_REPAIRERS),
            ))
            .tip(tip)
            .into_any_element()
    }

    /// What the search may use, as checkboxes in two columns.
    fn kinds(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        let boxes = SearchGroup::ALL.into_iter().zip(KIND_TIPS).map(|(g, tip)| {
            let this = this.clone();
            div()
                .id(SharedString::from(format!("wizard-kind-tip-{g:?}")))
                .child(
                    Checkbox::new(SharedString::from(format!("wizard-kind-{g:?}")))
                        .small()
                        .label(g.label())
                        .checked(self.choices.search_tiers.has(g))
                        .on_click(move |_, window, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.change(cx, |c| c.search_tiers.toggle(g));
                                this.refocus(window, cx);
                            });
                        }),
                )
                .tip(tip)
        });
        div()
            .grid()
            .grid_cols(2)
            .gap_x_2()
            .gap_y_1()
            .pt_0p5()
            .children(boxes)
            .into_any_element()
    }

    /// One budget on one line: a field in ISK, with the presets in a menu beside it.
    fn budget(&self, total: bool, cx: &mut Context<Self>) -> Div {
        let (field_state, presets, current, title) = if total {
            (
                self.budget_total.clone(),
                Budget::TOTAL,
                self.choices.budget.total,
                "Whole tank",
            )
        } else {
            (
                self.budget_each.clone(),
                Budget::EACH,
                self.choices.budget.each,
                "Per module",
            )
        };
        let this = cx.weak_entity();
        let menu = Button::new(SharedString::from(format!("wizard-budget-presets-{total}")))
            .ghost()
            .xsmall()
            .tab_stop(false)
            .icon(IconName::ChevronDown)
            .tooltip("Presets")
            .dropdown_menu(move |mut menu, _, _| {
                for preset in presets {
                    let (this, field_state) = (this.clone(), field_state.clone());
                    menu = menu.item(
                        PopupMenuItem::new(Budget::label(preset))
                            .checked(preset == current)
                            .on_click(move |_, window, cx| {
                                let text =
                                    preset.map_or_else(String::new, |p| Budget::label(Some(p)));
                                field_state.update(cx, |f, cx| f.set_value(text, window, cx));
                                let _ = this.update(cx, |this, cx| {
                                    this.change(cx, |c| {
                                        if total {
                                            c.budget.total = preset;
                                        } else {
                                            c.budget.each = preset;
                                        }
                                    });
                                });
                            }),
                    );
                }
                menu
            });
        let input = if total {
            &self.budget_total
        } else {
            &self.budget_each
        };
        field(
            title,
            h_flex()
                .gap_0p5()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(input).xsmall().suffix(muted("ISK", cx).pr_1())),
                )
                .child(menu),
            cx,
        )
    }

    /// The search button, or its progress while it runs, and what it covers.
    pub(super) fn search_block(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.shared.palette;
        let cancel = Self::cancel_button(cx);
        let theme = cx.theme();
        let mut block = v_flex().gap_1p5();
        if let Some(search) = &self.search {
            let fraction = search.progress.fraction();
            block = block.child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(
                                UiIcon::new(AssetIcon::LoaderCircle)
                                    .small()
                                    .text_color(theme.primary),
                            )
                            .child(div().flex_1().child("Searching tanks"))
                            .child(cancel),
                    )
                    .child(
                        widgets::gauge(
                            f64::from(fraction),
                            palette.accent,
                            format!("{:.0}%", fraction * 100.0),
                            0.,
                        )
                        .w_full(),
                    ),
            );
        } else {
            let fetching = self.fetching.is_some();
            block = block
                .child(
                    Button::new("wizard-find")
                        .small()
                        .w_full()
                        .tab_stop(false)
                        .loading(fetching)
                        .icon(AssetIcon::Search)
                        .label(if fetching {
                            "Fetching prices"
                        } else if self.searched.is_some() {
                            "Search again"
                        } else {
                            "Find best tanks"
                        })
                        .tooltip("F")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.find_best(cx);
                            this.refocus(window, cx);
                        })),
                )
                .when(fetching, |d| d.child(cancel));
        }
        let text = self
            .note
            .clone()
            .unwrap_or_else(|| self.search_text().into());
        let warn = self.note.is_some();
        block.child(if warn {
            notice(AssetIcon::TriangleAlert, palette.yellow, text, cx)
        } else {
            muted(text, cx)
        })
    }

    fn cancel_button(cx: &mut Context<Self>) -> Button {
        Button::new("wizard-cancel")
            .xsmall()
            .ghost()
            .tab_stop(false)
            .icon(AssetIcon::X)
            .label("Cancel")
            .tooltip("Escape")
            .on_click(cx.listener(|this, _, window, cx| {
                this.cancel_search(cx);
                this.refocus(window, cx);
            }))
    }

    /// What the search covers, in a sentence.
    fn search_text(&self) -> String {
        let r = self.room;
        let rack = slot_word(self.choices.layer.slot());
        let pattern = match &self.shared.fit.damage_profile {
            DamageProfile::Uniform => "uniform damage".to_owned(),
            DamageProfile::SelectedAmmo => "the selected ammo's damage".to_owned(),
            DamageProfile::Pattern { name, .. } => format!("the {name} pattern"),
        };
        let mut room = Vec::new();
        if r.slots > 0 {
            room.push(count(
                r.slots,
                &format!("{rack} slot"),
                &format!("{rack} slots"),
            ));
        }
        if r.rigs > 0 {
            room.push(count(r.rigs, "rig slot", "rig slots"));
        }
        if room.is_empty() {
            return format!("No free {rack} slots or rigs.");
        }
        let mut off = Vec::new();
        if r.modules_off > 0 {
            off.push(count(r.modules_off, "tank module", "tank modules"));
        }
        if r.rigs_off > 0 {
            off.push(count(r.rigs_off, "rig", "rigs"));
        }
        let off = if off.is_empty() {
            String::new()
        } else {
            format!(" after taking off {}", join_and(&off))
        };
        format!(
            "Searches {}{off}, against {pattern} with the guns idle.",
            join_and(&room)
        )
    }
}

/// The controls in a column that scrolls, the search pinned at its foot.
pub(super) fn controls_column(controls: Div, search: Div, width: f32, side: Hsla, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w(px(width))
        .flex_none()
        .h_full()
        .bg(side)
        .border_r_1()
        .border_color(theme.border)
        .child(
            div()
                .id("wizard-controls")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .child(controls),
        )
        .child(
            div()
                .flex_none()
                .p_2()
                .border_t_1()
                .border_color(theme.border)
                .child(search),
        )
}

/// What each kind of module is, in the order of [`SearchGroup::ALL`].
const KIND_TIPS: [&str; 8] = [
    "Tech 1 modules",
    "Meta and compact variations of the Tech 1 module",
    "Tech 2 modules",
    "Navy and pirate modules",
    "Storyline modules",
    "Deadspace modules, which get expensive",
    "Officer modules, which get very expensive",
    "Tech 2 rigs; Tech 1 rigs are always tried",
];
