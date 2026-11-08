//! The Drones tab, laid out like pyfa's drone pane: stacks sorted by drone class, a state column
//! that launches and recalls them, and per-drone numbers from the engine.

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, h_flex, menu::ContextMenuExt, v_flex};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, CursorStyle, Div, DragMoveEvent,
    InteractiveElement, IntoElement, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    ParentElement, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::{ROW_HEIGHT, addition_menu, misc_entries, price_text};
use crate::ui::widgets::{DropHighlight, TipExt, empty_line};
use crate::{
    data::{Catalog, Kind},
    dogma::{attr as drone_attr, effect},
    engine::{DroneInfo, ItemRef, Stats},
    fit::{DroneStack, mutation::display_name},
    icons::Icon,
    prices::Prices,
    ui::{
        Action, AddKind, Efa, Loaded,
        columns::{ICON_W, STATE_W, text_width},
        drag::{DragPreview, DragSource, Dragged},
        stats::format::{self, capitalize, format_list},
        validation::flag,
        widgets::{color, icon, icon_box},
    },
};

/// pyfa's drone order: by the market group a drone is listed under, then by name. Drones in any
/// other group go last.
const DRONE_ORDER: [&str; 12] = [
    "Light Scout Drones",
    "Light Hybrid Drones",
    "Medium Scout Drones",
    "Medium Hybrid Drones",
    "Heavy Attack Drones",
    "Heavy Hybrid Drones",
    "Sentry Drones",
    "Combat Utility Drones",
    "Electronic Warfare Drones",
    "Logistic Drones",
    "Mining Drones",
    "Salvage Drones",
];

/// Drone groups whose misc column says something, by group ID.
mod group {
    pub const COMBAT: i32 = 100;
    pub const MINING: i32 = 101;
    pub const NEUTRALIZER: i32 = 544;
    pub const EWAR: i32 = 639;
    pub const LOGISTIC: i32 = 640;
    pub const WEB: i32 = 641;
    pub const SALVAGE: i32 = 1159;
    /// Combat drones that also carry an electronic warfare effect. pyfa's data predates the
    /// group and has them as combat drones.
    pub const HYBRID: i32 = 5239;
}

/// Icon IDs of the attributes the columns show, as pyfa labels its headers.
const SPEED_ICON: i32 = 1389;
/// Room pyfa adds to a column's widest text when sizing it.
const AUTO_PADDING: f32 = 24.;
const NAME_MIN: f32 = 80.;
/// A row's gap between cells and its padding on both sides, as `gap_1` and `px_1` set them.
const ROW_GAP: f32 = 4.;
const ROW_PADDING: f32 = 8.;
const MIN_WIDTH: f32 = 28.;
const HANDLE_W: f32 = 6.;
const HEADER_ICON: f32 = 18.;

/// The drone pane's columns after the state mark and the icon, as in pyfa's `DEFAULT_COLS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Range,
    Misc,
    Speed,
    Hp,
    Regen,
    Price,
}

impl Col {
    const ALL: [Self; 7] = [
        Self::Name,
        Self::Range,
        Self::Misc,
        Self::Speed,
        Self::Hp,
        Self::Regen,
        Self::Price,
    ];

    /// Name the width is saved under, next to the fitting view's.
    fn key(self) -> &'static str {
        match self {
            Self::Name => "drones-name",
            Self::Range => "drones-range",
            Self::Misc => "drones-misc",
            Self::Speed => "drones-speed",
            Self::Hp => "drones-hp",
            Self::Regen => "drones-regen",
            Self::Price => "drones-price",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }

    fn header_icon(self) -> Option<(i32, &'static str)> {
        match self {
            Self::Range => Some((icon::RANGE, "Optimal + Falloff")),
            Self::Speed => Some((SPEED_ICON, "Maximum Velocity")),
            Self::Hp => Some((icon::SHIELD, "Effective HP")),
            Self::Regen => Some((icon::RECHARGE, "Effective Shield Regeneration")),
            Self::Name | Self::Misc | Self::Price => None,
        }
    }

    fn header_text(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Misc => "Misc data",
            Self::Price => "Price",
            Self::Range | Self::Speed | Self::Hp | Self::Regen => "",
        }
    }

    fn text(
        self,
        catalog: &Catalog,
        prices: &Prices,
        stack: &DroneStack,
        info: &DroneInfo,
        expanded: bool,
    ) -> Option<String> {
        match self {
            Self::Name => Some(format!(
                "{}x {}",
                stack.count,
                display_name(catalog, stack.type_id, stack.mutation.as_ref(), expanded)
            )),
            Self::Range => range_text(info),
            Self::Misc => misc(catalog, stack, info).map(|(text, _)| text),
            Self::Speed => info
                .attr(drone_attr::MAX_VELOCITY)
                .map(|v| format_amount(v, 3, false)),
            Self::Hp => info.ehp.map(|v| format_amount(v, 9, false)),
            Self::Regen => info
                .shield_regen
                .map(|v| format!("{}/s", format_amount(v, 9, false))),
            Self::Price => price_text(prices, stack.type_id, stack.count),
        }
    }
}

/// Column widths for this frame: what the user dragged them to, or the widest cell.
#[derive(Debug, Clone, Copy)]
struct Layout {
    widths: [f32; Col::ALL.len()],
    name_fixed: bool,
}

impl Layout {
    fn measure(
        saved: &std::collections::BTreeMap<String, f32>,
        rows: &[[Option<String>; Col::ALL.len()]],
        window: &Window,
    ) -> Self {
        let mut layout = Self {
            widths: [0.; Col::ALL.len()],
            name_fixed: false,
        };
        for col in Col::ALL {
            if let Some(&width) = saved.get(col.key()) {
                layout.widths[col.index()] = width;
                layout.name_fixed |= col == Col::Name;
                continue;
            }
            let header = match col.header_icon() {
                Some(_) => HEADER_ICON,
                None => text_width(window, col.header_text()),
            };
            let widest = rows
                .iter()
                .filter_map(|cells| cells[col.index()].as_deref())
                .map(|text| text_width(window, text))
                .fold(header, f32::max);
            layout.widths[col.index()] = widest + AUTO_PADDING;
        }
        layout
    }

    /// The narrowest a row can be: below it the list scrolls sideways.
    fn min_width(&self) -> f32 {
        let columns: f32 = Col::ALL
            .iter()
            .map(|&col| {
                if col == Col::Name && !self.name_fixed {
                    NAME_MIN
                } else {
                    self.widths[col.index()]
                }
            })
            .sum();
        // Each row: padding on both sides, and a gap between the state, the icon and the cells.
        let gaps = ROW_GAP * (Col::ALL.len() + 1) as f32;
        STATE_W + ICON_W + columns + gaps + ROW_PADDING
    }

    fn cell(&self, col: Col) -> Div {
        let width = px(self.widths[col.index()]);
        if col == Col::Name && !self.name_fixed {
            div()
                .flex_1()
                .flex_basis(width)
                .min_w(px(NAME_MIN))
                .overflow_hidden()
        } else {
            div().flex_none().w(width).overflow_hidden()
        }
    }
}

/// Being dragged: the edge of a drone column's header.
#[derive(Debug, Clone, Copy)]
struct ColumnResize(Col);

/// Resizing a column needs a drag preview; there is nothing to show.
struct NoPreview;

impl Render for NoPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Stack indices in display order.
pub(super) fn display_order(catalog: &Catalog, drones: &[DroneStack]) -> Vec<usize> {
    // Mutated drones go by their base drone's group, after the plain ones, as in pyfa.
    let rank = |stack: &DroneStack| {
        let group = catalog
            .get(stack.base_type())
            .and_then(|t| t.market_group)
            .and_then(|g| catalog.market_groups.get(&g))
            .map(|g| g.name.as_str());
        (order_rank(group), stack.mutation.is_some())
    };
    let name = |stack: &DroneStack| match &stack.mutation {
        Some(m) => m.full_name(catalog, stack.type_id),
        None => catalog.name(stack.type_id),
    };
    let mut order: Vec<usize> = (0..drones.len()).collect();
    order.sort_by_cached_key(|&i| (rank(&drones[i]), name(&drones[i])));
    order
}

fn order_rank(market_group: Option<&str>) -> usize {
    market_group
        .and_then(|name| DRONE_ORDER.iter().position(|&o| o == name))
        .unwrap_or(DRONE_ORDER.len())
}

/// pyfa's formatAmount with three significant digits: 1234 -> 1.23k. Values of a thousand and up
/// get a suffix, up to 10^`highest`.
pub(super) fn format_amount(value: f64, highest: i32, force_sign: bool) -> String {
    if force_sign {
        format::signed(value, 3, 0, highest)
    } else {
        format::amount(value, 3, 0, highest)
    }
}

/// Range then falloff, the way pyfa's max range column writes them.
fn range_text(info: &DroneInfo) -> Option<String> {
    use drone_attr::{
        ECM_BURST_RANGE, ECM_RANGE_OPTIMAL, EMP_FIELD_RANGE, FALLOFF, FALLOFF_EFFECTIVENESS,
        MAX_RANGE, POWER_TRANSFER_RANGE, SHIELD_TRANSFER_RANGE,
    };
    let range = [
        SHIELD_TRANSFER_RANGE,
        POWER_TRANSFER_RANGE,
        EMP_FIELD_RANGE,
        ECM_BURST_RANGE,
        MAX_RANGE,
        ECM_RANGE_OPTIMAL,
    ]
    .into_iter()
    .find_map(|a| info.attr(a));
    let falloff = [FALLOFF, FALLOFF_EFFECTIVENESS]
        .into_iter()
        .find_map(|a| info.attr(a))
        .filter(|&f| f >= 5.0)
        .map(|f| format!("+{}m", format_amount(f, 3, false)));
    match (range, falloff) {
        (Some(r), f) => Some(format!(
            "{}m{}",
            format_amount(r, 3, false),
            f.unwrap_or_default()
        )),
        (None, f) => f,
    }
}

/// The biggest of some bonuses by size, with the names of those that reach it.
fn strongest<'a>(bonuses: &[(Option<f64>, &'a str)]) -> Option<(f64, Vec<&'a str>)> {
    let display = bonuses
        .iter()
        .filter_map(|&(v, _)| v)
        .fold(
            0.0_f64,
            |best, v| if v.abs() > best.abs() { v } else { best },
        );
    if display == 0.0 {
        return None;
    }
    let names = bonuses
        .iter()
        .filter(|&&(v, _)| v == Some(display))
        .map(|&(_, name)| name)
        .collect();
    Some((display, names))
}

/// What pyfa's Miscellanea column shows for a drone stack, with its tooltip.
fn misc(catalog: &Catalog, stack: &DroneStack, info: &DroneInfo) -> Option<(String, String)> {
    use drone_attr::{
        ACCESS_DIFFICULTY_BONUS, FALLOFF_BONUS, MAX_RANGE_BONUS, MAX_TARGET_RANGE_BONUS,
        NEUT_AMOUNT, NEUT_DURATION, OPTIMAL_SIG_RADIUS, SCAN_GRAVIMETRIC_BONUS, SCAN_LADAR_BONUS,
        SCAN_MAGNETOMETRIC_BONUS, SCAN_RADAR_BONUS, SCAN_RESOLUTION_BONUS, SIGNATURE_RADIUS_BONUS,
        SPEED_FACTOR, TRACKING_SPEED, TRACKING_SPEED_BONUS,
    };
    let group = catalog.get(stack.type_id)?.group_id;
    let a = |id| info.attr(id);
    let neut = || {
        let per_sec = -a(NEUT_AMOUNT)? * 1000.0 / a(NEUT_DURATION)?;
        Some(format!("{}/s", format_amount(per_sec, 3, false)))
    };
    let single = |text: String, tooltip: &str| Some((text, tooltip.to_owned()));
    match group {
        group::COMBAT | group::HYBRID | group::EWAR => {
            // A part per electronic warfare effect the drone carries, in pyfa's order. Asking for
            // the effect matters: speed factor, for one, reads 1 on every drone.
            let mut parts: Vec<(String, String)> = Vec::new();
            if group != group::EWAR
                && let (Some(tracking), Some(sig)) = (a(TRACKING_SPEED), a(OPTIMAL_SIG_RADIUS))
            {
                let normalized = tracking * 40_000.0 / sig;
                parts.push((format_amount(normalized, 3, false), "Tracking speed".into()));
            }
            if info.has_effect(effect::ENTITY_NEUTRALIZER)
                && let Some(text) = neut()
            {
                parts.push((text, "Energy neutralization per second".into()));
            }
            if info.has_effect(effect::ENTITY_WEAPON_DISRUPTOR)
                && let Some((display, names)) = strongest(&[
                    (a(MAX_RANGE_BONUS), "optimal range"),
                    (a(FALLOFF_BONUS), "falloff range"),
                    (a(TRACKING_SPEED_BONUS), "tracking speed"),
                ])
            {
                parts.push((
                    format!("{}%", format_amount(display, 3, false)),
                    capitalize(&format!("{} disruption", format_list(&names))),
                ));
            }
            if info.has_effect(effect::ENTITY_ECM)
                && let Some((display, names)) = strongest(&[
                    (a(SCAN_GRAVIMETRIC_BONUS), "gravimetric"),
                    (a(SCAN_LADAR_BONUS), "ladar"),
                    (a(SCAN_MAGNETOMETRIC_BONUS), "magnetometric"),
                    (a(SCAN_RADAR_BONUS), "radar"),
                ])
            {
                let plural = if names.len() == 1 { "" } else { "s" };
                parts.push((
                    format_amount(display, 3, false),
                    capitalize(&format!("{} strength{plural}", format_list(&names))),
                ));
            }
            if info.has_effect(effect::ENTITY_SENSOR_DAMPENER)
                && let Some((display, names)) = strongest(&[
                    (a(MAX_TARGET_RANGE_BONUS), "lock range"),
                    (a(SCAN_RESOLUTION_BONUS), "scan resolution"),
                ])
            {
                parts.push((
                    format!("{}%", format_amount(display, 3, true)),
                    capitalize(&format!("{} dampening", format_list(&names))),
                ));
            }
            if info.has_effect(effect::ENTITY_WEBIFIER)
                && let Some(speed) = a(SPEED_FACTOR)
            {
                parts.push((
                    format!("{}%", format_amount(speed, 3, false)),
                    "Speed reduction".into(),
                ));
            }
            if info.has_effect(effect::ENTITY_TARGET_PAINTER)
                && let Some(bonus) = a(SIGNATURE_RADIUS_BONUS)
            {
                parts.push((
                    format!("{}%", format_amount(bonus, 3, true)),
                    "Signature radius increase".into(),
                ));
            }
            if parts.is_empty() {
                return None;
            }
            let (texts, tooltips): (Vec<String>, Vec<String>) = parts.into_iter().unzip();
            Some((texts.join(" | "), tooltips.join("\n")))
        }
        group::NEUTRALIZER => single(neut()?, "Energy neutralization per second"),
        group::WEB => single(
            format!("{}%", format_amount(a(SPEED_FACTOR)?, 3, false)),
            "Speed reduction",
        ),
        group::SALVAGE => single(
            format!("{}%", format_amount(a(ACCESS_DIFFICULTY_BONUS)?, 3, false)),
            "Item retrieval chance",
        ),
        group::MINING => {
            let count = f64::from(stack.count);
            let (yield_, drain) = (info.mining.0 * count, info.mining.1 * count);
            if yield_ <= 0.0 {
                return None;
            }
            let mut texts = vec![format!("{} m³/s", format_amount(yield_, 3, false))];
            let mut lines = vec![
                format!(
                    "{} m³ mining yield per second ({} m³ per hour)",
                    format_amount(yield_, 3, false),
                    format_amount(yield_ * 3600.0, 3, false)
                ),
                format!(
                    "{} m³ mining drain per second ({} m³ per hour)",
                    format_amount(drain, 3, false),
                    format_amount(drain * 3600.0, 3, false)
                ),
            ];
            let efficiency = if drain > 0.0 { yield_ / drain } else { 0.0 };
            if (efficiency - 1.0).abs() > 1e-6 {
                let text = format!("{}%", format_amount(efficiency * 100.0, 3, false));
                lines.push(format!("{text} mining efficiency"));
                texts.push(text);
            }
            Some((texts.join(" | "), lines.join("\n")))
        }
        group::LOGISTIC => {
            let count = f64::from(stack.count);
            let (layer, per_drone) = ["Shield", "Armor", "Hull"]
                .into_iter()
                .zip(info.reps)
                .find(|&(_, r)| r > 0.0)?;
            let total = per_drone * count;
            Some((
                format!("{}/s", format_amount(total, 3, false)),
                format!(
                    "{layer} HP repaired per second\n{} HP/s per drone",
                    format_amount(per_drone, 3, false)
                ),
            ))
        }
        _ => None,
    }
}

impl Efa {
    pub(in crate::ui) fn drones_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(current) = self.saved.settings.current else {
            return div().into_any_element();
        };
        let catalog = &loaded.catalog;
        let no_stats = Stats::default();
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let fit = &self.saved.fits[current];
        let order = display_order(catalog, &fit.drones);
        let no_info = DroneInfo::default();
        let info = |i: usize| stats.drones.get(i).unwrap_or(&no_info);
        let expanded = self
            .saved
            .settings
            .preferences
            .general
            .expanded_mutant_names;
        let cells: Vec<[Option<String>; Col::ALL.len()]> = order
            .iter()
            .map(|&i| {
                Col::ALL
                    .map(|col| col.text(catalog, &self.prices, &fit.drones[i], info(i), expanded))
            })
            .collect();
        let layout = Layout::measure(&self.saved.settings.columns, &cells, window);

        let mut rows: Vec<AnyElement> = Vec::with_capacity(order.len() + 1);
        if order.is_empty() {
            rows.push(
                empty_line("Double-click drones in the market to add them.", cx)
                    .id("drones-empty")
                    .context_menu({
                        let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
                        move |menu, window, cx| {
                            misc_entries(menu, &this, &loaded, AddKind::Drones, None, window, cx)
                        }
                    })
                    .into_any_element(),
            );
        }
        for (n, (&i, cells)) in order.iter().zip(cells).enumerate() {
            let stack = &fit.drones[i];
            let tooltip = misc(catalog, stack, info(i)).map(|(_, tip)| tip);
            rows.push(self.drone_row(loaded, layout, n, i, stack, cells, tooltip, cx));
        }

        let theme = cx.theme();
        let list = v_flex()
            .id("drones-list")
            .track_focus(&self.drones_focus)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
            .on_key_down(cx.listener(move |g, event: &KeyDownEvent, _, cx| {
                g.drone_key(event, cx);
            }))
            .drop_highlight::<Dragged>()
            .on_drop(cx.listener(|g, dragged: &Dragged, window, cx| {
                g.drop_on_drones(None, dragged, window.modifiers(), cx);
            }));
        // Narrower than its columns, the list scrolls sideways instead of cutting them off.
        div()
            .id("drones-scroll-x")
            .flex_1()
            .min_h_0()
            .overflow_x_scroll()
            .bg(theme.background)
            .child(
                v_flex()
                    .h_full()
                    .min_w(px(layout.min_width()))
                    .child(Self::drone_header(loaded, layout, cx))
                    .child(list),
            )
            .into_any_element()
    }

    #[expect(clippy::too_many_arguments, reason = "one row needs all of these")]
    fn drone_row(
        &self,
        loaded: &Rc<Loaded>,
        layout: Layout,
        n: usize,
        i: usize,
        stack: &DroneStack,
        cells: [Option<String>; Col::ALL.len()],
        tooltip: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let selected = self.additions_selection.contains(&n);
        let (mark, mark_color) = if stack.active > 0 {
            ("✓", self.palette.green)
        } else {
            ("○", self.palette.muted)
        };
        let state = div()
            .id(SharedString::from(format!("drone-state-{i}")))
            .w(px(STATE_W))
            .flex_none()
            .text_color(color(mark_color))
            .child(mark)
            .on_click(cx.listener(move |g, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                let targets = g.kind_targets(AddKind::Drones, n);
                g.apply(Action::ToggleDrones(i, targets), cx);
            }));
        let [name, range, misc_text, speed, hp, regen, price] = cells;
        let misc_cell = layout
            .cell(Col::Misc)
            .id(SharedString::from(format!("drone-misc-{i}")))
            .truncate()
            .children(misc_text)
            .when_some(tooltip, super::super::widgets::TipExt::tip);
        let mut row: Stateful<Div> = h_flex()
            .id(SharedString::from(format!("drone-{i}")))
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_1()
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .when_some(self.problems_of(loaded, ItemRef::Drone(i)), |row, why| {
                flag(row, why, theme.danger)
            })
            .child(state)
            .child(div().w(px(ICON_W)).flex_none().child(icon_box(
                catalog,
                &loaded.icons,
                Icon::Type(stack.type_id),
                22.,
            )))
            .child(layout.cell(Col::Name).truncate().children(name))
            .child(layout.cell(Col::Range).truncate().children(range))
            .child(misc_cell)
            .child(layout.cell(Col::Speed).truncate().children(speed))
            .child(layout.cell(Col::Hp).truncate().children(hp))
            .child(layout.cell(Col::Regen).truncate().children(regen))
            .child(layout.cell(Col::Price).truncate().children(price));
        if let Some(dragged) = Dragged::of(loaded, DragSource::Drone(i), Some(stack.type_id)) {
            row = row.on_drag(dragged, |dragged, _, _, cx| {
                cx.new(|_| DragPreview(dragged.clone()))
            });
        }
        row.drop_highlight::<Dragged>()
            .on_drop(cx.listener(move |g, dragged: &Dragged, window, cx| {
                cx.stop_propagation();
                g.drop_on_drones(Some(i), dragged, window.modifiers(), cx);
            }))
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.drones_focus, cx);
                if event.click_count() >= 2 {
                    // pyfa takes out one drone, or the whole stack with Alt.
                    let action = if event.modifiers().alt {
                        Action::RemoveAdditions(AddKind::Drones, vec![i])
                    } else {
                        Action::RemoveDrone(i)
                    };
                    g.apply(action, cx);
                } else {
                    g.select_addition(n, event.modifiers(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |g, _: &MouseDownEvent, _, cx| {
                    if g.additions_selection.right_click(n) {
                        cx.notify();
                    }
                }),
            )
            .context_menu({
                let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
                move |menu, window, cx| {
                    addition_menu(menu, &this, &loaded, AddKind::Drones, n, window, cx)
                }
            })
            .into_any_element()
    }

    fn drone_header(loaded: &Loaded, layout: Layout, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .h(px(24.))
            .px_1()
            .gap_1()
            .flex_none()
            .bg(theme.table_head)
            .text_color(theme.table_head_foreground)
            .child(div().w(px(STATE_W + ICON_W)).flex_none())
            .children(Col::ALL.into_iter().map(|col| {
                let content = match col.header_icon() {
                    Some((icon_id, _)) => icon_box(
                        &loaded.catalog,
                        &loaded.icons,
                        Icon::Id(icon_id),
                        HEADER_ICON,
                    )
                    .into_any_element(),
                    None => div().child(col.header_text()).into_any_element(),
                };
                let handle = div()
                    .id(SharedString::from(format!("resize-{}", col.key())))
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .w(px(HANDLE_W))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|s| s.bg(cx.theme().border))
                    .on_drag(ColumnResize(col), |_, _, _, cx| cx.new(|_| NoPreview))
                    .on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if event.click_count() >= 2 {
                            g.saved.settings.columns.remove(col.key());
                            g.save();
                            cx.notify();
                        }
                    }));
                layout
                    .cell(col)
                    .id(SharedString::from(format!("column-{}", col.key())))
                    .relative()
                    .h_full()
                    .flex()
                    .items_center()
                    .when_some(col.header_icon(), |cell, (_, tip)| cell.tip(tip))
                    .child(content)
                    .child(handle)
                    .on_drag_move(cx.listener(
                        move |g, event: &DragMoveEvent<ColumnResize>, _, cx| {
                            if event.drag(cx).0 != col {
                                return;
                            }
                            let width = f32::from(event.event.position.x - event.bounds.left())
                                .max(MIN_WIDTH);
                            g.saved.settings.columns.insert(col.key().to_owned(), width);
                            g.unsaved = true;
                            cx.notify();
                        },
                    ))
            }))
            .into_any_element()
    }

    /// pyfa's keys in the drone pane: Escape clears the selection, Ctrl+A selects every stack,
    /// Delete removes the selected stacks.
    fn drone_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = self.fit().map_or(0, |f| f.drones.len());
        self.additions_key(event, AddKind::Drones, count, true, cx);
    }

    /// pyfa's drops on the drone pane: a drone from the market is added (a full stack with Alt),
    /// a stack dropped on another of the same drone merges into it, and with Ctrl it is copied
    /// into a new stack instead.
    fn drop_on_drones(
        &mut self,
        on: Option<usize>,
        dragged: &Dragged,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        match dragged.source {
            DragSource::Drone(from) if modifiers.control => {
                self.apply(Action::CloneDrones(from), cx);
            }
            DragSource::Drone(from) => {
                if let Some(to) = on {
                    self.apply(Action::MergeDrones { from, to }, cx);
                }
            }
            DragSource::Market => {
                let Some(loaded) = self.loaded() else { return };
                for id in dragged.types() {
                    if loaded.catalog.get(id).is_none_or(|t| t.kind != Kind::Drone) {
                        continue;
                    }
                    let amount = if modifiers.alt {
                        self.drone_stack_size(&loaded, id).max(1)
                    } else {
                        1
                    };
                    self.apply(Action::AddDrones(id, amount), cx);
                }
            }
            DragSource::Row(_) | DragSource::Cargo | DragSource::Listed => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_match_pyfa() {
        for (value, highest, want) in [
            (0.0, 3, "0"),
            (2.178, 3, "2.18"),
            (12.5, 3, "12.5"),
            (630.0, 3, "630"),
            (2850.0, 3, "2.85k"),
            (999_999.0, 3, "1000k"),
            (999_999.0, 6, "1M"),
            (16_590.0, 9, "16.6k"),
            (2_250_000.0, 9, "2.25M"),
            (-12.345, 3, "-12.3"),
        ] {
            assert_eq!(format_amount(value, highest, false), want, "value {value}");
        }
        assert_eq!(format_amount(8.0, 3, true), "+8");
        assert_eq!(format_amount(-25.0, 3, true), "-25");
    }

    #[test]
    fn lists_read_like_pyfa() {
        assert_eq!(format_list(&["ladar"]), "ladar");
        assert_eq!(format_list(&["a", "b", "c"]), "a, b and c");
        assert_eq!(capitalize("lock range dampening"), "Lock range dampening");
    }

    #[test]
    fn strongest_bonus_names_every_tie() {
        let (value, names) = strongest(&[
            (Some(-12.0), "optimal range"),
            (Some(-12.0), "falloff range"),
            (None, "tracking speed"),
        ])
        .expect("there is a bonus");
        assert!((value + 12.0).abs() < f64::EPSILON);
        assert_eq!(names, ["optimal range", "falloff range"]);
        assert!(strongest(&[(None, "a"), (Some(0.0), "b")]).is_none());
    }

    #[test]
    fn range_reads_optimal_then_falloff() {
        let mut info = DroneInfo::default();
        info.attrs.insert(drone_attr::MAX_RANGE, 2100.0);
        info.attrs.insert(drone_attr::FALLOFF, 2000.0);
        assert_eq!(range_text(&info).as_deref(), Some("2.1km+2km"));
        // Falloff under 5 m is noise and left out.
        info.attrs.insert(drone_attr::FALLOFF, 3.0);
        assert_eq!(range_text(&info).as_deref(), Some("2.1km"));
        assert_eq!(range_text(&DroneInfo::default()), None);
    }

    #[test]
    fn drone_classes_sort_in_pyfa_order() {
        assert!(order_rank(Some("Light Scout Drones")) < order_rank(Some("Sentry Drones")));
        assert!(order_rank(Some("Salvage Drones")) < order_rank(Some("Something New")));
        assert_eq!(order_rank(None), DRONE_ORDER.len());
    }
}
