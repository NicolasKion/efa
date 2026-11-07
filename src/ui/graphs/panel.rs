//! The settings strip under the plot: small labelled groups of tight rows, like pyfa's panel,
//! with each setting's explanation on hover rather than taking room.

use gpui_kit::component::{ActiveTheme, Sizable, h_flex, switch::Switch, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::GraphsWindow;

/// A small uppercase label, over a card or a picker.
pub fn caption(text: &str, cx: &App) -> impl IntoElement {
    div()
        .px_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}

/// Height of a setting's row.
pub const ROW_H: f32 = 28.;

/// A setting: its name, then its control; the description shows on hover.
pub fn row(
    title: impl Into<SharedString>,
    description: &str,
    control: impl IntoElement,
    _cx: &App,
) -> AnyElement {
    let title: SharedString = title.into();
    let tip: SharedString = description.to_owned().into();
    h_flex()
        .id(SharedString::from(format!("row-{title}")))
        .h(px(ROW_H))
        .gap_2()
        .child(div().flex_1().min_w_0().truncate().child(title))
        .child(div().flex_none().child(control))
        .when(!tip.is_empty(), |d| {
            d.tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
        })
        .into_any_element()
}

/// A setting whose control takes the room its short name leaves, up to `max_w`.
pub fn fill_row(
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
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(description).build(window, cx)
        })
        .into_any_element()
}

/// A labelled group of rows.
pub fn section(title: &str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    v_flex()
        .gap_1p5()
        .child(caption(title, cx))
        .children(rows)
        .into_any_element()
}

/// A switch row that calls `apply` with the new value.
pub fn switch(
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
    row(title, description, control, cx)
}

/// One of a few values as a segmented control, under the setting's text.
pub fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    options: &[(T, &'static str)],
    current: T,
    apply: fn(&mut GraphsWindow, T, &mut Context<GraphsWindow>),
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let theme = cx.theme();
    let segments: Vec<AnyElement> = options
        .iter()
        .enumerate()
        .map(|(n, &(value, label))| {
            let selected = current == value;
            div()
                .id(SharedString::from(format!("{id}-{n}")))
                .flex()
                .justify_center()
                .px_2()
                .rounded(px(5.))
                .text_xs()
                .whitespace_nowrap()
                .when(selected, |d| {
                    d.bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .font_weight(FontWeight::MEDIUM)
                })
                .when(!selected, |d| {
                    d.text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.secondary).text_color(theme.foreground))
                })
                .child(label)
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(move |g, _: &gpui_kit::MouseDownEvent, _, cx| apply(g, value, cx)),
                )
                .into_any_element()
        })
        .collect();
    let control = h_flex()
        .flex_wrap()
        .max_w_full()
        .gap_0p5()
        .p_0p5()
        .rounded(px(5.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .children(segments);
    // The control drops under its name when the group is too narrow for both on a line.
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
        .child(control)
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(description).build(window, cx)
        })
        .into_any_element()
}

/// What a segmented setting's name keeps beside its control before the control wraps under it.
const CHOICE_TITLE_MIN_W: f32 = 80.;
