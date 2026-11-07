//! The settings column next to the plot, drawn like the preferences window: titled cards of
//! rows with the text on the left and the control on the right.

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

/// A setting's title with its description under it.
pub fn text(title: impl Into<SharedString>, description: &str, cx: &App) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(div().font_weight(FontWeight::MEDIUM).child(title.into()))
        .when(!description.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(description.to_owned())),
            )
        })
}

/// A setting: its text on the left, its control on the right.
pub fn row(
    title: impl Into<SharedString>,
    description: &str,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    h_flex()
        .gap_4()
        .px_3()
        .py_2p5()
        .child(text(title, description, cx))
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// A titled card of rows, divided by hairlines.
pub fn section(title: &str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut card = v_flex()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.sidebar)
        .overflow_hidden();
    for (n, row) in rows.into_iter().enumerate() {
        if n > 0 {
            card = card.child(div().h_px().mx_3().bg(theme.border));
        }
        card = card.child(row);
    }
    v_flex()
        .gap_1p5()
        .child(caption(title, cx))
        .child(card)
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
                .flex_1()
                .flex()
                .justify_center()
                .px_2()
                .py_0p5()
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
        .w_full()
        .gap_0p5()
        .p_0p5()
        .rounded(px(7.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .children(segments);
    v_flex()
        .gap_2()
        .px_3()
        .py_2p5()
        .child(text(title, description, cx))
        .child(control)
        .into_any_element()
}
