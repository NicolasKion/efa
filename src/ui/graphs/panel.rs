//! The settings strip under the plot: small labelled groups of tight rows, like pyfa's panel,
//! with each setting's explanation on hover rather than taking room.

use gpui_kit::component::{Sizable, h_flex, switch::Switch, v_flex};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    div, prelude::FluentBuilder, px,
};

use super::GraphsWindow;
use crate::ui::widgets::{Segment, TipExt, caption, segmented};

/// Height of a setting's row.
pub(super) const ROW_H: f32 = 28.;

/// A setting: its name, then its control; the description shows on hover.
fn row(title: impl Into<SharedString>, description: &str, control: impl IntoElement) -> AnyElement {
    let title: SharedString = title.into();
    h_flex()
        .id(SharedString::from(format!("row-{title}")))
        .h(px(ROW_H))
        .gap_2()
        .child(div().flex_1().min_w_0().truncate().child(title))
        .child(div().flex_none().child(control))
        .when(!description.is_empty(), |d| d.tip(description.to_owned()))
        .into_any_element()
}

/// A setting whose control takes the room its short name leaves, up to `max_w`.
pub(super) fn fill_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
    max_w: f32,
) -> AnyElement {
    h_flex()
        .id(SharedString::from(format!("row-{title}")))
        .h(px(ROW_H))
        .gap_2()
        .justify_between()
        .child(div().flex_none().child(title))
        .child(div().flex_1().min_w_0().max_w(px(max_w)).child(control))
        .tip(description)
        .into_any_element()
}

/// A labelled group of rows.
pub(super) fn section(title: &str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    v_flex()
        .gap_1p5()
        .child(caption(title, cx).px_1())
        .children(rows)
        .into_any_element()
}

/// A switch row that calls `apply` with the new value.
pub(super) fn switch(
    id: impl Into<SharedString>,
    title: &'static str,
    description: &'static str,
    checked: bool,
    apply: impl Fn(&mut GraphsWindow, bool, &mut Context<GraphsWindow>) + 'static,
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let control = Switch::new(id.into())
        .small()
        .checked(checked)
        .on_click(cx.listener(move |g, value: &bool, _, cx| apply(g, *value, cx)));
    row(title, description, control)
}

/// One of a few values as a segmented control beside the setting's name, or under it when the
/// group is too narrow for both on a line.
pub(super) fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    options: &[(T, &'static str)],
    current: T,
    apply: fn(&mut GraphsWindow, T, &mut Context<GraphsWindow>),
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let segments = options
        .iter()
        .map(|&(value, label)| Segment::new(label, value == current))
        .collect();
    let values: Vec<T> = options.iter().map(|&(value, _)| value).collect();
    let control = segmented(id, segments, cx, move |g, i, _, cx| {
        if let Some(&value) = values.get(i) {
            apply(g, value, cx);
        }
    });
    // The segments share the width evenly, so the widest label sets how much each needs.
    let widest = options
        .iter()
        .map(|(_, label)| label.chars().count())
        .max()
        .unwrap_or(0);
    let min_w = options.len() as f32 * (widest as f32 * SEGMENT_CHAR_W + SEGMENT_PAD);
    h_flex()
        .id(SharedString::from(format!("row-{title}")))
        .flex_wrap()
        .min_h(px(ROW_H))
        .gap_x_2()
        .gap_y_1()
        .child(
            div()
                .flex_1()
                .min_w(px(CHOICE_TITLE_MIN_W))
                .truncate()
                .child(title),
        )
        .child(div().flex_1().min_w(px(min_w)).child(control))
        .tip(description)
        .into_any_element()
}

/// What a segmented setting's name keeps beside its control before the control wraps under it.
const CHOICE_TITLE_MIN_W: f32 = 80.;
/// About how wide a character of a segment's label is, and the room a segment takes around it.
const SEGMENT_CHAR_W: f32 = 6.5;
const SEGMENT_PAD: f32 = 18.;
