//! The pieces every preference page is built from: a setting's text and row, section bands,
//! switches and choices.

use gpui_kit::component::{ActiveTheme, Disableable, h_flex, switch::Switch, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder, px,
};

use super::PreferencesWindow;
use crate::store::Saved;
use crate::ui::widgets::{ROW_H, Segment, band, section_title, segmented};

/// A setting's text keeps this much beside its control before the control drops under it.
const TEXT_MIN_W: f32 = 200.;
/// The widest a choice's buttons grow, so a few short options do not span a wide window.
const CHOICE_MAX_W: f32 = 420.;

/// A switch: id, title, description, whether it is on, and how to set it.
pub(super) type Toggle = (
    &'static str,
    &'static str,
    &'static str,
    bool,
    fn(&mut Saved, bool),
);

impl PreferencesWindow {
    /// A switch bound to one setting.
    pub(super) fn toggle(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        checked: bool,
        apply: fn(&mut Saved, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Self::toggle_under(id, title, description, checked, None, apply, cx)
    }

    /// A switch that depends on another setting: indented under it, and off limits while
    /// `enabled` is false. None for a switch that stands on its own.
    pub(super) fn toggle_under(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        checked: bool,
        enabled: Option<bool>,
        apply: fn(&mut Saved, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disabled = enabled == Some(false);
        let control = Switch::new(id)
            .checked(checked)
            .disabled(disabled)
            .on_click(cx.listener(move |this, value: &bool, _, cx| {
                let value = *value;
                this.set(cx, |s| apply(s, value));
            }))
            .into_any_element();
        row(title, description, control, enabled.is_some(), disabled, cx)
    }

    /// One of a few values, as a segmented control under the setting's text.
    pub(super) fn choice<T: Copy + PartialEq + 'static>(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        options: &[(T, &'static str)],
        current: T,
        apply: fn(&mut Saved, T),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let values: Vec<T> = options.iter().map(|&(value, _)| value).collect();
        let segments = options
            .iter()
            .map(|&(value, label)| Segment::new(label, current == value))
            .collect();
        let control = segmented(id, segments, cx, move |this, n, _, cx| {
            if let Some(&value) = values.get(n) {
                this.set(cx, |s| apply(s, value));
            }
        });
        v_flex()
            .gap_2()
            .px_4()
            .py_2()
            .child(text(title, description, false, cx))
            .child(div().w_full().max_w(px(CHOICE_MAX_W)).child(control))
            .into_any_element()
    }
}

/// A setting's title with its description under it.
fn text(
    title: impl Into<SharedString>,
    description: &'static str,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .when(disabled, |d| d.opacity(0.5))
        .child(div().font_weight(FontWeight::MEDIUM).child(title.into()))
        .when(!description.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(description),
            )
        })
        .into_any_element()
}

/// A setting: its text on the left, its control on the right, or under the text when the row
/// is too narrow for both. `nested` indents it under the setting before it.
pub(super) fn row(
    title: impl Into<SharedString>,
    description: &'static str,
    control: AnyElement,
    nested: bool,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    h_flex()
        .flex_wrap()
        .gap_x_6()
        .gap_y_2()
        .px_4()
        .py_2()
        .when(nested, Styled::pl_10)
        .child(
            div()
                .flex_1()
                .min_w(px(TEXT_MIN_W))
                .child(text(title, description, disabled, cx)),
        )
        .child(div().flex_none().max_w_full().child(control))
        .into_any_element()
}

/// A band titling a run of settings, like the fitting view's rack labels, with hairlines
/// between the settings.
pub(in crate::ui) fn section(title: &'static str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let count = rows.len();
    v_flex()
        .child(band(
            h_flex().h(px(ROW_H)).px_4().child(section_title(title)),
            cx,
        ))
        .children(rows.into_iter().enumerate().map(|(n, row)| {
            div()
                .when(n + 1 < count, |d| d.border_b_1().border_color(theme.border))
                .child(row)
        }))
        .into_any_element()
}
