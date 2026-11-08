//! Small building blocks shared by the views: icons, bars, the theme mapping, number formatting.

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Sizable, Theme, ThemeMode,
    button::{Button, ButtonCustomVariant, ButtonGroup, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, Div, ElementId, Entity, FontWeight, Hsla, InteractiveElement,
    IntoElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Subscription, WeakEntity, Window, div, img, prelude::FluentBuilder, px, relative, rgb,
};

use std::rc::Rc;

use gpui_kit::component::{IconName, menu::PopupMenuItem};

use super::{
    Efa, Loaded,
    columns::{ICON_W, STATE_W},
};

/// What a dialog's OK button and Enter both run. It closes the dialog itself when it is done,
/// and leaves it open to show a problem otherwise.
pub type Submit = Rc<dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App)>;
use crate::{
    data::{Catalog, Slot, TypeId},
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
pub fn gauge(fraction: f64, fill: Rgb, text: impl Into<SharedString>, width: f32) -> Div {
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

/// Height of a row in the fitting view and the lists that look like it.
pub const ROW_H: f32 = 24.;

/// A row of a list the way the fitting view draws one: its height, and the theme's colors for
/// the picked row and the one under the mouse.
pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .h(px(ROW_H))
        .flex_none()
        .px_1()
        .gap_1()
        .when(selected, |r| r.bg(theme.list_active))
        .hover(|s| s.bg(theme.list_hover))
}

/// The look of the fitting view's rack labels, a strip set off by borders over a run of rows.
pub fn band<E: Styled>(row: E, cx: &App) -> E {
    let theme = cx.theme();
    row.bg(theme.secondary)
        .border_t_1()
        .border_b_1()
        .border_color(theme.border)
}

/// A rack label's text: the rack's name and, muted after it, how many slots it has.
pub fn band_label(title: impl Into<SharedString>, count: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .gap_1()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(count.into()),
        )
}

/// A rack label's content, lined up with the fitting view's rows under it: the rack's icon over
/// their item icons, then its name and slot count.
pub fn rack_band_label(loaded: &Loaded, slot: Slot, total: u32, cx: &App) -> Div {
    let (label, icon_id) = rack_label(slot);
    h_flex()
        .gap_1()
        .child(div().w(px(STATE_W)).flex_none())
        .child(div().w(px(ICON_W - 4.)).flex_none().children(
            icon_id.map(|id| icon_box(&loaded.catalog, &loaded.icons, Icon::Id(id), 18.)),
        ))
        .child(band_label(label, total.to_string(), cx))
}

/// The fitting view's table head: a row in the theme's head colors that columns go in.
pub fn table_head(cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .h(px(ROW_H))
        .px_1()
        .gap_1()
        .bg(theme.table_head)
        .text_color(theme.table_head_foreground)
}

/// A rack's label in the fitting view and its icon, pyfa's "High Slots".
fn rack_label(slot: Slot) -> (&'static str, Option<i32>) {
    match slot {
        Slot::High => ("High Slots", Some(icon::HIGH)),
        Slot::Mid => ("Med Slots", Some(icon::MID)),
        Slot::Low => ("Low Slots", Some(icon::LOW)),
        Slot::Rig => ("Rig Slots", Some(icon::RIG)),
        Slot::Subsystem => ("Subsystem Slots", Some(icon::SUBSYSTEM)),
        Slot::Service => ("Service Slots", None),
    }
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

/// The tallest a menu may be in this window: `cap`, or less when the window is too short for
/// it, so a long menu scrolls instead of running off the window.
pub fn menu_max_h(window: &gpui_kit::Window, cap: f32) -> gpui_kit::Pixels {
    let room = window.viewport_size().height - px(MENU_WINDOW_MARGIN);
    px(cap).min(room).max(px(MENU_MIN_H))
}

/// The height for a dialog's list or text area: `preferred`, or less in a short window, where
/// `around` is what the rest of the dialog takes. Kept from shrinking past a few rows, so the
/// dialog body scrolls once even that does not fit.
pub fn dialog_fill_h(window: &gpui_kit::Window, preferred: f32, around: f32) -> gpui_kit::Pixels {
    let room = window.viewport_size().height - px(around);
    px(preferred).min(room).max(px(DIALOG_FILL_MIN))
}

/// The least a dialog's list or text area shrinks to.
const DIALOG_FILL_MIN: f32 = 96.;

/// What a menu leaves clear of the window's height, above and below it.
const MENU_WINDOW_MARGIN: f32 = 48.;
/// No menu is cut shorter than this, however short the window.
const MENU_MIN_H: f32 = 120.;

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

/// A menu entry's click handler that runs `f` on the fit window behind `this`, while it is
/// still open.
pub fn on_efa<F>(
    this: &WeakEntity<Efa>,
    f: F,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<F>
where
    F: Fn(&mut Efa, &ClickEvent, &mut Window, &mut Context<Efa>) + 'static,
{
    let this = this.clone();
    move |event, window, cx| {
        let _ = this.update(cx, |g, cx| f(g, event, window, cx));
    }
}

/// [`on_efa`] for an entry acting on several things at once, such as the picked market items:
/// `f` runs for each of `items`, in order, in one update of the window.
pub fn on_each<T: Copy + 'static, F>(
    this: &WeakEntity<Efa>,
    items: &Rc<[T]>,
    f: F,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<T, F>
where
    F: Fn(&mut Efa, T, &mut Window, &mut Context<Efa>) + 'static,
{
    let items = Rc::clone(items);
    on_efa(this, move |g, _, window, cx| {
        for &item in items.iter() {
            f(g, item, window, cx);
        }
    })
}

/// Hover text on an element, the one-line way.
pub trait TipExt: StatefulInteractiveElement + Sized {
    /// Show `text` on hover.
    #[must_use]
    fn tip(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        self.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
    }
}

impl<E: StatefulInteractiveElement> TipExt for E {}

/// A row of dialog or form buttons, flush right.
pub fn button_row() -> Div {
    h_flex().justify_end().gap_2()
}

/// A number typed by a person: trimmed, and with a decimal comma read as a point, so "0,5" is
/// a half.
pub fn parse_decimal<T: std::str::FromStr>(text: &str) -> Option<T> {
    text.trim().replace(',', ".").parse().ok()
}

/// "1 fit", "3 fits": `n` with the word that goes with it.
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The drop target look: an element lights up while something it takes is dragged over it.
pub trait DropHighlight: InteractiveElement + Sized {
    /// Light up while something of type `T` is dragged over.
    #[must_use]
    fn drop_highlight<T: 'static>(self) -> Self {
        self.drag_over::<T>(|style, _, _, cx| style.bg(cx.theme().drop_target))
    }

    /// Light up while something of type `T` that `accepts` takes is dragged over.
    #[must_use]
    fn drop_highlight_if<T: 'static>(self, accepts: impl Fn(&T) -> bool + 'static) -> Self {
        self.drag_over::<T>(move |style, dragged, _, cx| {
            if accepts(dragged) {
                style.bg(cx.theme().drop_target)
            } else {
                style
            }
        })
    }
}

impl<E: InteractiveElement> DropHighlight for E {}

/// Redraw the view whenever the text in `input` changes, for views that read it while drawing,
/// such as a filter box.
pub fn redraw_on_change<V: 'static>(
    cx: &mut Context<V>,
    input: &Entity<InputState>,
) -> Subscription {
    cx.subscribe(input, |_, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            cx.notify();
        }
    })
}

/// A small uppercase caption over a column or section.
pub fn caption(text: &str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}

/// What an empty list shows: an icon, a line on what is missing and one on what to do.
pub fn empty_state(icon: AssetIcon, title: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_8()
        .child(
            gpui_kit::component::Icon::new(icon)
                .size(px(28.))
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_center()
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_xs()
                .text_center()
                .text_color(theme.muted_foreground)
                .child(hint.to_owned()),
        )
}

/// What an empty list in a dense pane shows: one muted line where the rows would be.
pub fn empty_line(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .p_2()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// How far each level of a tree is indented.
const INDENT: f32 = 14.;
/// Height of a line of a tree.
const TREE_ROW_H: f32 = 22.;

/// A line of a tree, such as the market's groups: indented `depth` levels, with an open or
/// closed chevron when it has children (`expanded` is None for a leaf), then the icon and the
/// label. Clicks and the like are the caller's.
pub fn tree_row(
    id: impl Into<ElementId>,
    depth: usize,
    expanded: Option<bool>,
    icon: Option<gpui_kit::AnyElement>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .gap_1()
        .h(px(TREE_ROW_H))
        .flex_none()
        .pl(px(4. + INDENT * depth as f32))
        .pr_1()
        .when(selected, |row| row.bg(theme.list_active))
        .hover(|s| s.bg(theme.list_hover))
        .child(
            div()
                .w(px(14.))
                .flex_none()
                .flex()
                .items_center()
                .children(expanded.map(|open| {
                    gpui_kit::component::Icon::new(if open {
                        AssetIcon::ChevronDown
                    } else {
                        AssetIcon::ChevronRight
                    })
                    .size(px(14.))
                    .text_color(theme.muted_foreground)
                })),
        )
        .children(icon)
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
}

/// One option of a [`segmented`] control.
pub struct Segment {
    label: SharedString,
    selected: bool,
    tip: Option<SharedString>,
}

impl Segment {
    pub fn new(label: impl Into<SharedString>, selected: bool) -> Self {
        Self {
            label: label.into(),
            selected,
            tip: None,
        }
    }

    #[must_use]
    pub fn tip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tip = Some(tip.into());
        self
    }
}

/// Options side by side as the kit's button group, sharing the width evenly. `pick` gets the
/// clicked option's index.
pub fn segmented<T: 'static>(
    id: &str,
    segments: Vec<Segment>,
    cx: &Context<T>,
    pick: impl Fn(&mut T, usize, &mut Window, &mut Context<T>) + 'static,
) -> gpui_kit::AnyElement {
    let this = cx.weak_entity();
    // The kit marks a selected button with its pressed shade, which barely shows; the picked
    // option takes the accent tint the lists use for what is picked. The outline goes on the
    // others only, since an outlined button drops its fill.
    let theme = cx.theme();
    let picked = ButtonCustomVariant::new(cx)
        .color(theme.primary.opacity(0.18))
        .foreground(theme.foreground)
        .hover(theme.primary.opacity(0.24))
        .active(theme.primary.opacity(0.3));
    let buttons = segments.into_iter().enumerate().map(|(i, s)| {
        Button::new(SharedString::from(format!("{id}-{i}")))
            .label(s.label)
            .when(s.selected, |b| b.custom(picked))
            .when(!s.selected, |b| {
                b.outline().text_color(theme.muted_foreground)
            })
            .tab_stop(false)
            .flex_1()
            .when_some(s.tip, Button::tooltip)
    });
    ButtonGroup::new(SharedString::from(id.to_owned()))
        .xsmall()
        .w_full()
        .children(buttons)
        .on_click(move |clicked, window, cx| {
            if let Some(&i) = clicked.first() {
                let _ = this.update(cx, |this, cx| pick(this, i, window, cx));
            }
        })
        .into_any_element()
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
