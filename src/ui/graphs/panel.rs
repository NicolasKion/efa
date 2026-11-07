//! The settings strip under the plot: small labelled groups of tight rows, like pyfa's panel,
//! with each setting's explanation on hover rather than taking room.

use gpui_kit::component::{ActiveTheme, Sizable, h_flex, switch::Switch, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
    px,
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
pub const ROW_H: f32 = 24.;

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

/// A labelled group of rows.
pub fn section(title: &str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    v_flex()
        .gap_0p5()
        .child(div().pb_1().child(caption(title, cx)))
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
                .on_click(cx.listener(move |g, _: &ClickEvent, _, cx| apply(g, value, cx)))
                .into_any_element()
        })
        .collect();
    let control = h_flex()
        .gap_0p5()
        .p_0p5()
        .rounded(px(5.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .children(segments);
    row(title, description, control, cx)
}
