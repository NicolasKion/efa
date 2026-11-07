//! pyfa's Mutations tab (gui/builtinItemStatsViews/itemMutator.py, attributeSlider.py): one
//! slider per rolled attribute, the worst roll on the left and the best on the right, with the
//! value to type in exactly.
//!
//! Moving a slider updates this window's numbers right away; letting go hands the rolls to the
//! main window, which applies them to the fit as one undoable edit.

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, px,
};

use super::{ItemStats, Target, format};
use crate::{
    data::{TypeId, muta::Limits},
    icons::Icon,
    item_data::ItemData,
};

/// Slider positions per unit of quality, worst to best.
const STEPS: f32 = 1000.;

/// pyfa's `higOverrides`: attributes whose better direction differs from the data's for one
/// group of items, by group and attribute name.
const HIGH_IS_GOOD_OVERRIDES: [(&str, &str, bool); 3] = [
    ("Stasis Web", "speedFactor", false),
    ("Damage Control", "duration", true),
    ("Siege Module", "siegeLocalLogisticsDurationBonus", false),
];

/// The sliders and fields of the Mutations tab.
pub(super) struct Panel {
    /// The mutaplasmid and base item it was built for; anything else needs a new panel.
    key: (TypeId, TypeId),
    rows: Vec<Row>,
    /// The rolls when the window first showed them, for "Revert changes".
    initial: Vec<(i32, f64)>,
    _subscriptions: Vec<Subscription>,
}

struct Row {
    attr: i32,
    limits: Limits,
    /// The worst and best values, which the slider runs between.
    worse: f64,
    better: f64,
    /// The unit's internal name, which decides how the value is shown.
    unit: Option<String>,
    slider: Entity<SliderState>,
    input: Entity<InputState>,
}

/// Where on a slider from `worse` to `better` a value sits, in steps.
fn position(worse: f64, better: f64, value: f64) -> f32 {
    let span = better - worse;
    if span == 0.0 {
        return 0.0;
    }
    (((value - worse) / span).clamp(0.0, 1.0) as f32) * STEPS
}

/// A value as the field shows it: converted for its unit, with up to five decimals as pyfa's
/// spin control does.
fn shown(unit: Option<&str>, value: f64) -> String {
    let text = format!("{:.5}", simplify(unit, value));
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_owned()
}

impl Row {
    fn position(&self, value: f64) -> f32 {
        position(self.worse, self.better, value)
    }

    fn value_at(&self, position: f32) -> f64 {
        let quality = f64::from(position / STEPS).clamp(0.0, 1.0);
        self.limits
            .clamp(self.worse + (self.better - self.worse) * quality)
    }

    fn shown(&self, value: f64) -> String {
        shown(self.unit.as_deref(), value)
    }
}

/// A value as pyfa shows it for its unit (`Unit.SimplifyValue`).
fn simplify(unit: Option<&str>, value: f64) -> f64 {
    match unit {
        Some("Inverse Absolute Percent" | "Inversed Modifier Percent") => (1.0 - value) * 100.0,
        Some("Modifier Percent") => (value - 1.0) * 100.0,
        Some("Absolute Percent") => value * 100.0,
        Some("Milliseconds") => value / 1000.0,
        _ => value,
    }
}

/// A shown value back to the attribute's own (`Unit.ComplicateValue`).
fn complicate(unit: Option<&str>, shown: f64) -> f64 {
    match unit {
        Some("Inverse Absolute Percent" | "Inversed Modifier Percent") => 1.0 - shown / 100.0,
        Some("Modifier Percent") => shown / 100.0 + 1.0,
        Some("Absolute Percent") => shown / 100.0,
        Some("Milliseconds") => shown * 1000.0,
        _ => shown,
    }
}

/// A uniform number in [0, 1) for "Random stats"; good enough for picking rolls.
fn random_unit(state: &mut u64) -> f64 {
    // xorshift64*
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    let bits = state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
    bits as f64 / (1u64 << 53) as f64
}

impl ItemStats {
    /// The rolls of the item this window shows, when it is a mutated item of a fit.
    pub(super) fn mutation(&self) -> Option<&crate::fit::Mutation> {
        let Target::Fitted { item, .. } = self.target else {
            return None;
        };
        self.fit.as_ref()?.mutation(item)
    }

    /// Build the panel for the current mutation unless it is built already.
    fn ensure_panel(&mut self, data: &ItemData, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mutation) = self.mutation().cloned() else {
            self.mutator = None;
            return;
        };
        let key = (mutation.mutaplasmid, mutation.base);
        if self.mutator.as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        let catalog = &self.loaded.catalog;
        let group = catalog
            .get(self.type_id)
            .and_then(|t| catalog.group_names.get(&t.group_id))
            .cloned()
            .unwrap_or_default();
        let mut attrs: Vec<(String, i32, f64)> = mutation
            .attrs
            .iter()
            .map(|(&attr, &value)| {
                let info = data.attr(attr);
                let name = info
                    .and_then(|i| i.display_name.clone())
                    .or_else(|| info.map(|i| i.name.clone()))
                    .unwrap_or_else(|| attr.to_string());
                (name, attr, value)
            })
            .collect();
        attrs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut rows = Vec::new();
        let mut subscriptions = Vec::new();
        for (_, attr, value) in attrs {
            let Some(limits) =
                catalog
                    .mutaplasmids
                    .limits(mutation.mutaplasmid, mutation.base, attr)
            else {
                continue;
            };
            // pyfa leaves out what has nothing to roll.
            if limits.base == 0.0 {
                continue;
            }
            let muta_attr = catalog.mutaplasmids.attrs.get(&attr);
            let high_is_good = HIGH_IS_GOOD_OVERRIDES
                .iter()
                .find(|&&(g, a, _)| g == group && muta_attr.is_some_and(|m| m.name == a))
                .map_or_else(|| muta_attr.is_some_and(|m| m.high_is_good), |o| o.2);
            let (worse, better) = if high_is_good {
                (limits.low(), limits.high())
            } else {
                (limits.high(), limits.low())
            };
            let unit = data
                .attr(attr)
                .and_then(|a| data.unit(a))
                .map(|u| u.name.clone());
            let at = position(worse, better, value);
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(0.)
                    .max(STEPS)
                    .step(1.)
                    .default_value(at)
            });
            let text = shown(unit.as_deref(), value);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
            let row = Row {
                attr,
                limits,
                worse,
                better,
                unit,
                slider,
                input,
            };
            subscriptions.push(cx.subscribe_in(
                &row.slider,
                window,
                move |this, _, event: &SliderEvent, window, cx| match event {
                    SliderEvent::Change(position) => {
                        this.slide(attr, position.start(), false, window, cx);
                    }
                    SliderEvent::Release(position) => {
                        this.slide(attr, position.start(), true, window, cx);
                    }
                },
            ));
            subscriptions.push(cx.subscribe_in(
                &row.input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let text = input.read(cx).value().to_string();
                        this.typed(attr, &text, window, cx);
                    }
                },
            ));
            rows.push(row);
        }
        self.mutator = Some(Panel {
            key,
            initial: mutation.attrs.iter().map(|(&a, &v)| (a, v)).collect(),
            rows,
            _subscriptions: subscriptions,
        });
    }

    /// A slider moved to `position`; `commit` when it was let go.
    fn slide(
        &mut self,
        attr: i32,
        position: f32,
        commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.row(attr) else { return };
        let value = row.value_at(position);
        let shown = row.shown(value);
        let input = row.input.clone();
        input.update(cx, |input, cx| input.set_value(shown, window, cx));
        self.set_rolls(&[(attr, value)], commit, cx);
    }

    /// A value typed into a field.
    fn typed(&mut self, attr: i32, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.row(attr) else { return };
        let current = self
            .mutation()
            .and_then(|m| m.attrs.get(&attr).copied())
            .unwrap_or(row.limits.base);
        let value = text
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f64>()
            .map_or(current, |shown| {
                row.limits.clamp(complicate(row.unit.as_deref(), shown))
            });
        let changed = (value - current).abs() > f64::EPSILON * current.abs().max(1.0);
        self.sync_row(attr, value, window, cx);
        if changed {
            self.set_rolls(&[(attr, value)], true, cx);
        }
    }

    fn row(&self, attr: i32) -> Option<&Row> {
        self.mutator.as_ref()?.rows.iter().find(|r| r.attr == attr)
    }

    /// Put a row's slider and field at `value`.
    fn sync_row(&self, attr: i32, value: f64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.row(attr) else { return };
        let (position, shown) = (row.position(value), row.shown(value));
        row.slider
            .update(cx, |slider, cx| slider.set_value(position, window, cx));
        row.input
            .update(cx, |input, cx| input.set_value(shown, window, cx));
    }

    /// Take rolls into this window's copy of the fit and show what they do; with `commit`, also
    /// hand them to the main window to keep.
    fn set_rolls(&mut self, values: &[(i32, f64)], commit: bool, cx: &mut Context<Self>) {
        let Target::Fitted { fit, item } = self.target else {
            return;
        };
        let catalog = &self.loaded.catalog;
        let Some(mutation) = self.fit.as_mut().and_then(|f| f.mutation_mut(item)) else {
            return;
        };
        for &(attr, value) in values {
            mutation.set(catalog, attr, value);
        }
        let values: Vec<(i32, f64)> = mutation.attrs.iter().map(|(&a, &v)| (a, v)).collect();
        self.recompute();
        if commit {
            let edit = crate::ui::hub::MutationEdit { fit, item, values };
            self.hub.update(cx, |hub, cx| {
                hub.mutation_edits.push(edit);
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Set every roll at once, from the buttons under the sliders.
    fn set_all(
        &mut self,
        mut pick: impl FnMut(&Row) -> f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = &self.mutator else { return };
        let values: Vec<(i32, f64)> = panel
            .rows
            .iter()
            .map(|r| (r.attr, r.limits.clamp(pick(r))))
            .collect();
        for &(attr, value) in &values {
            self.sync_row(attr, value, window, cx);
        }
        self.set_rolls(&values, true, cx);
    }

    /// Catch the sliders up with rolls changed elsewhere, such as an undo in the main window.
    pub(super) fn sync_mutator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mutation) = self.mutation().cloned() else {
            return;
        };
        let Some(panel) = &self.mutator else { return };
        let stale: Vec<(i32, f64)> = panel
            .rows
            .iter()
            .filter_map(|row| {
                let value = *mutation.attrs.get(&row.attr)?;
                let shown = row.slider.read(cx).value().start();
                ((row.position(value) - shown).abs() >= 0.5).then_some((row.attr, value))
            })
            .collect();
        for (attr, value) in stale {
            self.sync_row(attr, value, window, cx);
        }
    }

    pub(super) fn mutations_tab(
        &mut self,
        data: &ItemData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_panel(data, window, cx);
        let (Some(panel), Some(mutation)) = (&self.mutator, self.mutation()) else {
            return Self::empty("Not mutated.", cx);
        };
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let (good, bad) = (theme.success, theme.danger);
        let header = h_flex()
            .justify_center()
            .gap_2()
            .p_2()
            .border_b_1()
            .border_color(theme.border)
            .child(self.icon(Some(Icon::Type(self.type_id)), 32.))
            .child(self.icon(Some(Icon::Type(mutation.mutaplasmid)), 32.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(mutation.full_name(catalog, self.type_id)),
            );
        let rows: Vec<AnyElement> = panel
            .rows
            .iter()
            .map(|row| {
                let info = data.attr(row.attr);
                let name = info
                    .and_then(|i| i.display_name.clone())
                    .or_else(|| info.map(|i| i.name.clone()))
                    .unwrap_or_else(|| row.attr.to_string());
                let shown = |v: f64| format::value(data, catalog, info, v);
                let value = mutation
                    .attrs
                    .get(&row.attr)
                    .copied()
                    .unwrap_or(row.limits.base);
                let change = if row.limits.base == 0.0 {
                    0.0
                } else {
                    (value / row.limits.base - 1.0) * 100.0
                };
                let better = (value - row.limits.base) * (row.better - row.worse) > 0.0;
                let worse = (value - row.limits.base) * (row.better - row.worse) < 0.0;
                v_flex()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(self.icon(info.and_then(|i| i.icon).map(Icon::Id), 16.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .child(div().text_color(bad).child(shown(row.worse)))
                            .child(" \u{2500} ")
                            .child(div().text_color(good).child(shown(row.better))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Slider::new(&row.slider)))
                            .child(div().w(px(110.)).child(Input::new(&row.input).small()))
                            .child(
                                div()
                                    .w(px(64.))
                                    .text_xs()
                                    .text_right()
                                    .text_color(if better {
                                        good
                                    } else if worse {
                                        bad
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(format!("{change:+.2}%")),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
            | 1;
        v_flex()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(
                v_flex()
                    .id(SharedString::from("mutations"))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(
                Self::button_row(cx)
                    .justify_start()
                    .child(
                        Button::new("muta-reset")
                            .small()
                            .ghost()
                            .label("Reset defaults")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.set_all(|r| r.limits.base, window, cx);
                            })),
                    )
                    .child(
                        Button::new("muta-random")
                            .small()
                            .ghost()
                            .label("Random stats")
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                let mut state = seed;
                                this.set_all(
                                    |r| {
                                        let t = random_unit(&mut state);
                                        r.limits.low() + (r.limits.high() - r.limits.low()) * t
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("muta-revert")
                            .small()
                            .ghost()
                            .label("Revert changes")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                let initial =
                                    this.mutator.as_ref().map_or_default(|p| p.initial.clone());
                                this.set_all(
                                    |r| {
                                        initial
                                            .iter()
                                            .find(|&&(a, _)| a == r.attr)
                                            .map_or(r.limits.base, |&(_, v)| v)
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_convert_both_ways() {
        for unit in [
            None,
            Some("Inverse Absolute Percent"),
            Some("Modifier Percent"),
            Some("Absolute Percent"),
            Some("Milliseconds"),
        ] {
            let value = 0.875;
            let back = complicate(unit, simplify(unit, value));
            assert!((back - value).abs() < 1e-12, "{unit:?}");
        }
        assert!((simplify(Some("Modifier Percent"), 1.05) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn random_stays_in_range() {
        let mut state = 42;
        for _ in 0..1000 {
            let t = random_unit(&mut state);
            assert!((0.0..1.0).contains(&t));
        }
    }
}
