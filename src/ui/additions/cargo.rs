//! The Cargo tab, after pyfa's cargoView: stacks sorted by category, group and name, with their
//! volume, against the hold's size. Stacks can be dragged onto fitting slots, and fitted items and
//! market items dropped here.

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, h_flex, menu::ContextMenuExt, v_flex};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent,
    Modifiers, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::{PRICE_W, ROW_HEIGHT, addition_menu, misc_entries, price_text};
use crate::{
    data::{CONTAINER_GROUPS, Catalog, Kind, Slot, TypeId},
    fit::cargo::Transfer,
    icons::Icon,
    ui::{
        Action, AddKind, AdditionsTab, Gyfa, Loaded, RowRef,
        fitting::{DragPreview, DragSource, Dragged},
        widgets::{icon_box, short},
    },
};

/// Room for the volume column's widest usual text, "0.01 m³ (10k m³)".
const VOLUME_W: f32 = 140.;
const ICON_SIZE: f32 = 22.;
/// Plain, Ctrl, Alt and Ctrl+Alt double-clicks in the market add this many, as in pyfa.
const BATCH_PLAIN: u32 = 1;
const BATCH_CTRL: u32 = 10;
const BATCH_ALT: u32 = 100;
const BATCH_CTRL_ALT: u32 = 1000;

/// Ctrl held at the drop duplicates instead of moving, as pyfa does it.
pub fn drop_transfer(modifiers: Modifiers) -> Transfer {
    if modifiers.control {
        Transfer::Duplicate
    } else {
        Transfer::Move
    }
}

/// How many units a market double-click puts in cargo. pyfa only scales the batch for exactly
/// these modifier combinations.
fn double_click_batch(m: Modifiers) -> u32 {
    match (m.control, m.alt, m.shift || m.platform || m.function) {
        (true, false, false) => BATCH_CTRL,
        (false, true, false) => BATCH_ALT,
        (true, true, false) => BATCH_CTRL_ALT,
        _ => BATCH_PLAIN,
    }
}

/// Whether a market double-click or "Fill Cargo" applies. pyfa offers both for charges and
/// commodities; gyfa takes every cargo-only item, since nothing else can be done with them.
pub fn goes_to_cargo(kind: Kind) -> bool {
    matches!(kind, Kind::Charge | Kind::Cargo)
}

/// The position a fitting row stands for.
fn row_position(row: RowRef) -> (Slot, usize) {
    match row {
        RowRef::Module(slot, i) => (slot, i),
        RowRef::Rig(i) => (Slot::Rig, i),
        RowRef::Subsystem(i) => (Slot::Subsystem, i),
    }
}

/// pyfa's `formatAmount(value, 3, 0, 6)`: three significant digits, k and M above a thousand.
fn amount(value: f64) -> String {
    fn round_to(value: f64, digits: i32) -> f64 {
        if value.fract() == 0.0 {
            return value;
        }
        let places = (digits - 1 - value.abs().log10().floor() as i32).max(0);
        let scale = 10f64.powi(places);
        (value * scale).round() / scale
    }
    let (mut mantissa, mut suffix) = (value, "");
    if value.abs() >= 1e6 {
        (mantissa, suffix) = (value / 1e6, "M");
    } else if value.abs() >= 1e3 {
        (mantissa, suffix) = (value / 1e3, "k");
        if round_to(mantissa, 3).abs() >= 1e3 {
            (mantissa, suffix) = (mantissa / 1e3, "M");
        }
    }
    let rounded = round_to(mantissa, 3);
    let text = format!("{rounded:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}{suffix}")
}

/// pyfa's volume column: the unit volume, and the stack's when there is more than one.
fn volume_text(unit: f64, count: u32) -> String {
    if unit <= 0.0 {
        String::new()
    } else if count == 1 {
        format!("{} m³", amount(unit))
    } else {
        format!(
            "{} m³ ({} m³)",
            amount(unit),
            amount(unit * f64::from(count))
        )
    }
}

/// pyfa's name column: "5x Name", with a container's capacity after it.
fn name_text(catalog: &Catalog, type_id: TypeId, count: u32) -> String {
    let name = catalog.name(type_id);
    match catalog.get(type_id) {
        Some(t) if CONTAINER_GROUPS.contains(&t.group_id) && t.capacity > 0.0 => {
            format!("{count}x {name} ({} m³)", amount(t.capacity))
        }
        _ => format!("{count}x {name}"),
    }
}

/// Indices into the cargo in the order pyfa lists them: by category, group, then name.
fn display_order(catalog: &Catalog, cargo: &[(TypeId, u32)]) -> Vec<usize> {
    let key = |i: usize| {
        let t = catalog.get(cargo[i].0);
        let category = t.map_or("", |t| t.category_label());
        let group = t
            .and_then(|t| catalog.group_names.get(&t.group_id))
            .map_or("", String::as_str);
        (category, group, catalog.name(cargo[i].0))
    };
    let mut order: Vec<usize> = (0..cargo.len()).collect();
    order.sort_by_cached_key(|&i| key(i));
    order
}

impl Gyfa {
    pub(in crate::ui) fn cargo_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let catalog = &loaded.catalog;
        let used = fit.cargo_volume(catalog);
        let capacity = self
            .stats
            .as_ref()
            .and_then(|s| s.cargo_capacity)
            .unwrap_or(0.0);
        let order = display_order(catalog, &fit.cargo);
        let theme = cx.theme();
        let over = used > capacity + 1e-6;
        let summary = h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .text_xs()
            .border_b_1()
            .border_color(theme.border)
            .text_color(theme.muted_foreground)
            .child(div().flex_1().child("Name"))
            .child(
                div()
                    .w(px(VOLUME_W))
                    .flex_none()
                    .text_right()
                    .when(over, |d| d.text_color(theme.danger))
                    .child(format!("{} / {} m³", short(used), short(capacity))),
            )
            .child(div().w(px(PRICE_W)).flex_none().text_right().child("Price"));

        let mut rows: Vec<AnyElement> = Vec::with_capacity(order.len().max(1));
        if order.is_empty() {
            rows.push(
                div()
                    .id("cargo-empty")
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child("Drag items here, or right-click them in the market.")
                    .context_menu({
                        let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                        move |menu, _, cx| {
                            misc_entries(menu, &this, &loaded, AddKind::Cargo, None, cx)
                        }
                    })
                    .into_any_element(),
            );
        }
        for (n, &i) in order.iter().enumerate() {
            let (type_id, count) = fit.cargo[i];
            rows.push(self.cargo_row(loaded, n, i, type_id, count, cx));
        }

        v_flex()
            .id("cargo-list")
            .flex_1()
            .min_h_0()
            .track_focus(&self.additions_focus)
            .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| {
                g.cargo_key(event, cx);
            }))
            .drag_over::<Dragged>(|style, dragged, _, cx| {
                if matches!(dragged.source, DragSource::Cargo) {
                    style
                } else {
                    style.bg(cx.theme().drop_target)
                }
            })
            .on_drop(cx.listener(|g, dragged: &Dragged, window, cx| {
                g.drop_on_cargo(dragged, None, window.modifiers(), cx);
            }))
            .child(summary)
            .child(
                v_flex()
                    .id("cargo-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    /// One stack: `n` is its place in the list, `i` its index in the fit's cargo.
    fn cargo_row(
        &self,
        loaded: &Rc<Loaded>,
        n: usize,
        i: usize,
        type_id: TypeId,
        count: u32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let selected = self.additions_selection.contains(&i);
        let unit = catalog.get(type_id).map_or(0.0, |t| t.volume);
        let row = h_flex()
            .id(SharedString::from(format!("cargo-{i}")))
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .drag_over::<Dragged>(|style, dragged, _, cx| {
                if matches!(dragged.source, DragSource::Row(_)) {
                    style.bg(cx.theme().drop_target)
                } else {
                    style
                }
            })
            .on_drop(cx.listener(move |g, dragged: &Dragged, window, cx| {
                cx.stop_propagation();
                g.drop_on_cargo(dragged, Some(type_id), window.modifiers(), cx);
            }))
            .child(icon_box(
                catalog,
                &loaded.icons,
                Icon::Type(type_id),
                ICON_SIZE,
            ))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .child(name_text(catalog, type_id, count)),
            )
            .child(
                div()
                    .w(px(VOLUME_W))
                    .flex_none()
                    .text_right()
                    .truncate()
                    .child(volume_text(unit, count)),
            )
            .child(
                div()
                    .w(px(PRICE_W))
                    .flex_none()
                    .text_right()
                    .truncate()
                    .children(price_text(&self.prices, type_id, count)),
            )
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.additions_focus, cx);
                if event.click_count() >= 2 {
                    g.apply(Action::RemoveAdditions(AddKind::Cargo, vec![i]), cx);
                } else {
                    g.select_cargo(i, event.modifiers(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |g, _: &MouseDownEvent, _, cx| {
                    if !g.additions_selection.contains(&i) {
                        g.additions_selection = vec![i];
                        g.additions_anchor = Some(i);
                        cx.notify();
                    }
                }),
            );
        let row = match Dragged::of(loaded, DragSource::Cargo, Some(type_id)) {
            Some(dragged) => row.on_drag(dragged, |dragged, _, _, cx| {
                cx.new(|_| DragPreview(dragged.clone()))
            }),
            None => row,
        };
        row.context_menu({
            let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
            move |menu, window, cx| {
                addition_menu(menu, &this, &loaded, AddKind::Cargo, i, window, cx)
            }
        })
        .into_any_element()
    }

    /// List selection over the displayed order, which is sorted and so differs from the cargo's.
    fn select_cargo(&mut self, i: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let Some(fit) = self.fit() else { return };
        let order = display_order(&loaded.catalog, &fit.cargo);
        let place = |r: usize| order.iter().position(|&o| o == r);
        if modifiers.shift
            && let Some(anchor) = self.additions_anchor
            && let (Some(a), Some(b)) = (place(anchor), place(i))
        {
            self.additions_selection = order[a.min(b)..=a.max(b)].to_vec();
        } else if modifiers.control {
            if let Some(at) = self.additions_selection.iter().position(|&r| r == i) {
                self.additions_selection.remove(at);
            } else {
                self.additions_selection.push(i);
            }
            self.additions_anchor = Some(i);
        } else {
            self.additions_selection = vec![i];
            self.additions_anchor = Some(i);
        }
        cx.notify();
    }

    /// pyfa's cargo keys: Delete removes the selection, Ctrl+A selects all, Escape clears.
    fn cargo_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let plain = !(m.control || m.alt || m.shift || m.platform);
        match keystroke.key.as_str() {
            "delete" if plain && !self.additions_selection.is_empty() => {
                let rows = std::mem::take(&mut self.additions_selection);
                self.apply(Action::RemoveAdditions(AddKind::Cargo, rows), cx);
            }
            "a" if m.control && !m.alt && !m.shift => {
                let len = self.fit().map_or(0, |f| f.cargo.len());
                self.additions_selection = (0..len).collect();
                cx.notify();
            }
            "escape" if plain => {
                self.additions_selection.clear();
                self.additions_anchor = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// pyfa's cargo drop target: fitted items go into the hold (onto a cargo module, they trade
    /// places), market items are added one at a time.
    fn drop_on_cargo(
        &mut self,
        dragged: &Dragged,
        onto: Option<TypeId>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        match dragged.source {
            DragSource::Row(row) => {
                let transfer = drop_transfer(modifiers);
                self.apply(Action::ModuleToCargo(row_position(row), onto, transfer), cx);
            }
            DragSource::Market => self.add_to_cargo(dragged.type_id, 1, cx),
            // Drones dragged off the drones pane stay drones; pyfa does not move them to cargo.
            DragSource::Cargo | DragSource::Drone(_) => {}
        }
    }

    /// Add to cargo and bring the Cargo tab up, as pyfa does after every cargo addition.
    pub(in crate::ui) fn add_to_cargo(
        &mut self,
        type_id: TypeId,
        count: u32,
        cx: &mut Context<Self>,
    ) {
        if count == 0 || self.fit().is_none() {
            return;
        }
        if self.additions_tab != AdditionsTab::Cargo {
            self.additions_tab = AdditionsTab::Cargo;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
        self.apply(Action::AddCargo(type_id, count), cx);
    }

    /// As many units as still fit in the hold, worked out at click time like pyfa's cargoFill.
    pub(in crate::ui) fn fill_cargo_with(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let room = self.cargo_room_for(&loaded, type_id);
        self.add_to_cargo(type_id, room, cx);
    }

    /// A market double-click. Charges and cargo-only items go into the hold, in batches set by
    /// the modifiers; everything else is fitted.
    pub(in crate::ui) fn market_double_click(
        &mut self,
        type_id: TypeId,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let to_cargo = self.loaded().is_some_and(|l| {
            l.catalog
                .get(type_id)
                .is_some_and(|t| goes_to_cargo(t.kind))
        });
        if to_cargo && self.fit().is_some() {
            self.add_to_cargo(type_id, double_click_batch(modifiers), cx);
        } else {
            self.add_item(type_id, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_match_pyfa() {
        for (value, want) in [
            (0.01, "0.01"),
            (0.0125, "0.0125"),
            (5.0, "5"),
            (12.345, "12.3"),
            (999.0, "999"),
            (1234.5, "1.23k"),
            (3900.0, "3.9k"),
            (15_000.0, "15k"),
            (999_999.0, "1M"),
            (2_250_000.0, "2.25M"),
        ] {
            assert_eq!(amount(value), want, "value {value}");
        }
    }

    #[test]
    fn volume_shows_the_stack_total_past_one() {
        assert_eq!(volume_text(0.01, 1), "0.01 m³");
        assert_eq!(volume_text(0.01, 1000), "0.01 m³ (10 m³)");
        assert_eq!(volume_text(0.0, 5), "");
    }

    #[test]
    fn double_click_batches() {
        let with = |control, alt, shift| Modifiers {
            control,
            alt,
            shift,
            ..Modifiers::default()
        };
        assert_eq!(double_click_batch(with(false, false, false)), 1);
        assert_eq!(double_click_batch(with(true, false, false)), 10);
        assert_eq!(double_click_batch(with(false, true, false)), 100);
        assert_eq!(double_click_batch(with(true, true, false)), 1000);
        assert_eq!(double_click_batch(with(true, false, true)), 1);
    }

    #[test]
    fn ctrl_duplicates_drops() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert_eq!(drop_transfer(ctrl), Transfer::Duplicate);
        assert_eq!(drop_transfer(Modifiers::default()), Transfer::Move);
    }
}
