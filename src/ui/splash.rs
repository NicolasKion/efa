//! What the main window shows before the EVE data is in: the efa mark over a running bar, or
//! what went wrong when loading failed.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, v_flex};
use gpui_kit::{
    Animation, AnimationExt, App, FontWeight, Image, ImageFormat, IntoElement, ParentElement,
    SharedString, Styled, div, ease_in_out, img, prelude::FluentBuilder, px, relative,
};

static MARK: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../../assets/icons/efa-mark-e.svg").to_vec(),
    ))
});

const BAR_W: f32 = 180.;

/// The splash while loading, or the error when `failed` says why loading did not work.
pub fn render(failed: Option<SharedString>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    // The mark's amber, so the bar belongs to the logo above it.
    let accent = gpui_kit::rgb(0xf2a33a);
    let (muted, track) = (theme.muted_foreground, theme.border);
    v_flex()
        .flex_1()
        .min_h_0()
        .p_6()
        .items_center()
        .justify_center()
        .gap_3()
        .child(img(MARK.clone()).size(px(88.)).flex_none())
        .child(
            v_flex()
                .items_center()
                .gap_0p5()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("efa"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("EVE Online fitting"),
                ),
        )
        .map(|el| match failed {
            None => el.child(
                v_flex()
                    .mt_3()
                    .items_center()
                    .gap_2()
                    .child(
                        // A sliver running along a track, since how long loading takes is
                        // not known ahead.
                        div()
                            .relative()
                            .w(px(BAR_W))
                            .h(px(3.))
                            .rounded_full()
                            .bg(track)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .h_full()
                                    .w(relative(0.35))
                                    .rounded_full()
                                    .bg(accent)
                                    .with_animation(
                                        "splash-bar",
                                        Animation::new(Duration::from_millis(1100))
                                            .repeat()
                                            .with_easing(ease_in_out),
                                        |bar, delta| bar.left(relative(delta * 1.35 - 0.35)),
                                    ),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child("Loading EVE data")),
            ),
            Some(e) => el.child(
                v_flex()
                    .mt_3()
                    .max_w(px(520.))
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_color(theme.danger)
                            .font_weight(FontWeight::MEDIUM)
                            .child(Icon::new(AssetIcon::TriangleAlert).small())
                            .child("The EVE data could not be loaded"),
                    )
                    .child(
                        div()
                            .w_full()
                            .px_3()
                            .py_2()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(track)
                            .bg(theme.secondary)
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(e),
                    ),
            ),
        })
}
