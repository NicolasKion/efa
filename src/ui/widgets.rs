//! Small building blocks shared by the views: icons, bars, the theme mapping, number formatting.

use gpui_kit::component::{ActiveTheme, Theme, ThemeMode, h_flex};
use gpui_kit::{
    App, Div, FontWeight, Hsla, IntoElement, ParentElement, SharedString, Styled, div, img, px,
    relative, rgb,
};

use std::rc::Rc;

use gpui_kit::component::{IconName, menu::PopupMenuItem};

use super::Loaded;
use crate::{
    data::{Catalog, TypeId},
    icons::{Icon, Icons},
    theme::{Palette, Rgb},
};

/// Icon IDs the game uses for these attributes, which label the matching stats.
pub mod icon {
    pub const CPU: i32 = 1405;
    pub const POWERGRID: i32 = 70;
    pub const CALIBRATION: i32 = 2887;
    pub const TURRET: i32 = 387;
    pub const LAUNCHER: i32 = 168;
    pub const DRONE_BANDWIDTH: i32 = 2987;
    pub const DRONE_BAY: i32 = 1084;
    pub const SHIELD: i32 = 1384;
    pub const ARMOR: i32 = 1383;
    pub const HULL: i32 = 67;
    pub const EM: i32 = 1396;
    pub const THERMAL: i32 = 1394;
    pub const KINETIC: i32 = 1393;
    pub const EXPLOSIVE: i32 = 1395;
    pub const CAPACITOR: i32 = 1668;
    pub const RECHARGE: i32 = 1392;
    pub const RANGE: i32 = 1391;
    pub const DAMAGE: i32 = 1432;
    pub const HIGH: i32 = 293;
    pub const MID: i32 = 294;
    pub const LOW: i32 = 295;
    pub const RIG: i32 = 3266;
    pub const SUBSYSTEM: i32 = 3756;
}

/// EM, thermal, kinetic, explosive, in the colors the game uses for them.
pub const DAMAGE_COLORS: [Rgb; 4] = [0x0034_78be, 0x00b4_322d, 0x0078_7878, 0x00be_821e];

pub fn color(value: Rgb) -> Hsla {
    rgb(value).into()
}

/// An icon of the given size, or an empty box of that size while it downloads.
pub fn icon_box(catalog: &Catalog, icons: &Icons, icon: Icon, size: f32) -> Div {
    let slot = div().flex_none().size(px(size));
    match icons.path(catalog, icon) {
        Some(path) => slot.child(img(path).size(px(size))),
        None => slot,
    }
}

/// Track behind every gauge, pyfa's constant dark grey, so the white label always reads.
const GAUGE_TRACK: Rgb = 0x0033_3333;
/// The label's drop shadow, as pyfa draws it.
const GAUGE_SHADOW: Rgb = 0x0050_5050;
/// How far the fill darkens towards its right end; pyfa's gradient effect of -35%.
const GAUGE_DARKEN: f32 = 0.35;

/// A filled bar with its text centered on top, drawn like pyfa's gauges: a dark track, a fill
/// that darkens along its length, and white text with a drop shadow, so the label stays legible
/// over both the fill and the track in any theme.
pub fn gauge(
    fraction: f64,
    fill: Rgb,
    text: impl Into<SharedString>,
    width: f32,
    _cx: &App,
) -> Div {
    let fraction = fraction.clamp(0.0, 1.0) as f32;
    let text: SharedString = text.into();
    let start = color(fill);
    let end = Hsla {
        l: start.l * (1.0 - GAUGE_DARKEN),
        ..start
    };
    let label = |shade: Rgb, offset: f32| {
        h_flex()
            .absolute()
            .top(px(offset))
            .left(px(offset))
            .size_full()
            .justify_center()
            .text_color(color(shade))
            .child(text.clone())
    };
    // A width of zero or less stretches the gauge to the room its row gives it.
    let track = if width > 0.0 {
        div().w(px(width))
    } else {
        // Grow without a zero basis: in a column a zero basis would take the height away.
        div().flex_grow(1.).min_w_0()
    };
    track
        .relative()
        .h(px(16.))
        .rounded(px(3.))
        .bg(color(GAUGE_TRACK))
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(relative(fraction))
                .bg(gpui_kit::linear_gradient(
                    90.,
                    gpui_kit::linear_color_stop(start, 0.),
                    gpui_kit::linear_color_stop(end, 1.),
                )),
        )
        .child(label(GAUGE_SHADOW, 1.))
        .child(label(0x00ff_ffff, 0.))
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().font_weight(FontWeight::SEMIBOLD).child(text.into())
}

/// Map the Omarchy palette onto gpui-component's theme.
pub fn apply_palette(palette: &Palette, cx: &mut App) {
    let mode = if palette.dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, None, cx);
    let p = palette;
    let c = color;
    let colors = &mut Theme::global_mut(cx).colors;

    colors.background = c(p.background);
    colors.foreground = c(p.foreground);
    colors.border = c(p.lighter_background);
    colors.window_border = c(p.lighter_background);
    colors.muted = c(p.darker_background);
    colors.muted_foreground = c(p.dark_foreground);
    colors.accent = c(p.selection);
    colors.accent_foreground = c(p.foreground);
    colors.selection = c(p.selection);
    colors.ring = c(p.accent);
    colors.caret = c(p.accent);
    colors.link = c(p.accent);
    colors.link_hover = c(p.accent);
    colors.link_active = c(p.accent);

    colors.primary = c(p.accent);
    colors.primary_hover = c(p.accent);
    colors.primary_active = c(p.accent);
    colors.primary_foreground = c(p.darker_background);
    colors.secondary = c(p.lighter_background);
    colors.secondary_hover = c(p.selection);
    colors.secondary_active = c(p.selection);
    colors.secondary_foreground = c(p.foreground);

    colors.input = c(p.lighter_background);
    colors.popover = c(p.dark_background);
    colors.popover_foreground = c(p.foreground);
    colors.overlay = c(p.darker_background).opacity(0.6);

    colors.list = c(p.background);
    colors.list_even = c(p.dark_background);
    colors.list_hover = c(p.selection).opacity(0.5);
    colors.list_active = c(p.selection);
    colors.list_active_border = c(p.accent);
    colors.list_head = c(p.dark_background);
    colors.table = c(p.background);
    colors.table_even = c(p.dark_background);
    colors.table_hover = c(p.selection).opacity(0.5);
    colors.table_active = c(p.selection);
    colors.table_active_border = c(p.accent);
    colors.table_head = c(p.dark_background);
    colors.table_head_foreground = c(p.dark_foreground);
    colors.table_row_border = c(p.lighter_background).opacity(0.4);

    colors.tab_bar = c(p.dark_background);
    colors.tab = c(p.dark_background);
    colors.tab_active = c(p.background);
    colors.tab_foreground = c(p.dark_foreground);
    colors.tab_active_foreground = c(p.foreground);
    colors.title_bar = c(p.dark_background);
    colors.title_bar_border = c(p.lighter_background);
    colors.sidebar = c(p.dark_background);
    colors.sidebar_foreground = c(p.foreground);
    colors.sidebar_border = c(p.lighter_background);
    colors.sidebar_accent = c(p.selection);
    colors.sidebar_accent_foreground = c(p.foreground);
    colors.scrollbar = c(p.background).opacity(0.0);
    colors.scrollbar_thumb = c(p.lighter_background);
    colors.scrollbar_thumb_hover = c(p.muted);
    colors.progress_bar = c(p.accent);
    colors.drag_border = c(p.accent);
    colors.drop_target = c(p.selection).opacity(0.4);

    colors.danger = c(p.red);
    colors.danger_hover = c(p.red);
    colors.danger_active = c(p.red);
    colors.warning = c(p.yellow);
    colors.warning_hover = c(p.yellow);
    colors.warning_active = c(p.yellow);
    colors.success = c(p.green);
    colors.success_hover = c(p.green);
    colors.success_active = c(p.green);
    colors.info = c(p.blue);
    colors.red = c(p.red);
    colors.green = c(p.green);
    colors.yellow = c(p.yellow);
    colors.blue = c(p.blue);
    cx.refresh_windows();
}

/// Gap between a menu entry's icon and its label.
const MENU_ICON_GAP: f32 = 10.;
/// Extra room above and below each menu entry.
const MENU_ROW_PADDING: f32 = 3.;
const MENU_ICON_SIZE: f32 = 20.;

/// The row every menu entry is drawn as, so plain, item and submenu entries line up.
fn menu_row(icon: impl IntoElement, label: SharedString) -> Div {
    h_flex()
        .gap(px(MENU_ICON_GAP))
        .py(px(MENU_ROW_PADDING))
        .child(
            div()
                .flex_none()
                .size(px(MENU_ICON_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .child(icon),
        )
        .child(label)
}

fn ui_icon(icon: IconName, cx: &App) -> impl IntoElement {
    gpui_kit::component::Icon::new(icon)
        .size(px(16.))
        .text_color(cx.theme().muted_foreground)
}

/// A menu entry showing an item's own icon next to its label.
pub fn item_entry(
    loaded: &Rc<Loaded>,
    id: TypeId,
    label: impl Into<SharedString>,
) -> PopupMenuItem {
    PopupMenuItem::element(item_label(loaded, Some(id), label))
}

/// A menu entry with one of the UI's own icons.
pub fn entry(icon: IconName, label: impl Into<SharedString>) -> PopupMenuItem {
    PopupMenuItem::element(icon_label(icon, label))
}

/// A submenu label showing an item's icon, for groups named after the item they start with.
pub fn item_label(
    loaded: &Rc<Loaded>,
    id: Option<TypeId>,
    label: impl Into<SharedString>,
) -> impl Fn(&mut gpui_kit::Window, &mut App) -> Div + 'static {
    let (loaded, label) = (Rc::clone(loaded), label.into());
    move |_, _| {
        let icon = id.map(|id| {
            icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(id),
                MENU_ICON_SIZE,
            )
        });
        menu_row(div().children(icon), label.clone())
    }
}

/// A submenu label with one of the UI's own icons.
pub fn icon_label(
    icon: IconName,
    label: impl Into<SharedString>,
) -> impl Fn(&mut gpui_kit::Window, &mut App) -> Div + 'static {
    let label = label.into();
    move |_, cx| menu_row(ui_icon(icon.clone(), cx), label.clone())
}

/// Short numbers the way pyfa writes them: 1.38k, 16.6k, 2.25M.
pub fn short(v: f64) -> String {
    let magnitude = v.abs();
    let (scaled, suffix) = if magnitude >= 1e9 {
        (v / 1e9, "B")
    } else if magnitude >= 1e6 {
        (v / 1e6, "M")
    } else if magnitude >= 1e4 {
        (v / 1e3, "k")
    } else if magnitude >= 100.0 || v.fract().abs() < 1e-9 {
        return format!("{v:.0}");
    } else {
        return format!("{v:.1}");
    };
    let digits = match scaled.abs() {
        a if a >= 100.0 => 0,
        a if a >= 10.0 => 1,
        _ => 2,
    };
    format!("{scaled:.digits$}{suffix}")
}

pub fn km(meters: f64) -> String {
    if meters >= 1000.0 {
        format!("{:.1}km", meters / 1000.0)
    } else {
        format!("{meters:.0}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_numbers_match_pyfa() {
        for (value, want) in [
            (0.0, "0"),
            (12.5, "12.5"),
            (630.0, "630"),
            (16_590.0, "16.6k"),
            (2_250_000.0, "2.25M"),
        ] {
            assert_eq!(short(value), want, "value {value}");
        }
    }
}
