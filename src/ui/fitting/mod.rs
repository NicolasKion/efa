//! The middle: the fit's slots as a table, and the additions pane under it.

mod menu;

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Icon as UiIcon, h_flex, menu::ContextMenuExt, v_flex};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};

use super::drag::{DragPreview, DragSource, Dragged, DropOn, drop_target};
use super::widgets::TipExt;
use super::{
    Action, Efa, Loaded, RowRef, Scope,
    columns::{Col, ICON_W, STATE_W},
    item_stats::{StatsOf, stats_entry},
    selection::{ListKey, list_key},
    validation::flag,
    widgets::{ROW_H, band, color, icon_box, rack_band_label, short},
};
use menu::{MenuSnapshot, item_menu, ship_menu_builder};

pub(super) use menu::ship_entries;

use crate::{
    data::{Slot, TypeId},
    engine::{FitItem, ItemRef, ModuleInfo, Stats},
    fit::{Fit, Module, ModuleState},
    icons::Icon,
};

#[derive(Debug, Clone, Copy)]
enum Row {
    /// A rack's label, pyfa's "Show Rack Labels".
    Header {
        slot: Slot,
        total: u32,
    },
    /// A blank row between racks, when they are set apart without labels.
    Separator,
    /// The label over a tactical destroyer's mode.
    ModeHeader,
    Item(RowRef),
    Empty(Slot, usize),
    /// A tactical destroyer's mode, shown under the racks like pyfa does.
    Mode(TypeId),
}

impl Efa {
    /// The rows that hold something, top to bottom, for Shift-click ranges.
    fn row_order(&self) -> Vec<RowRef> {
        let (Some(fit), Some(stats)) = (self.fit(), self.stats.as_ref()) else {
            return Vec::new();
        };
        fitting_rows(fit, stats, self.rack_style())
            .into_iter()
            .filter_map(|row| match row {
                Row::Item(r) => Some(r),
                Row::Header { .. }
                | Row::Separator
                | Row::ModeHeader
                | Row::Empty(..)
                | Row::Mode(_) => None,
            })
            .collect()
    }

    pub(super) fn fitting_view(
        &mut self,
        loaded: &Rc<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(current) = self.saved.settings.current else {
            return div().into_any_element();
        };
        let Some(ship) = self.saved.fits[current].ship else {
            return div()
                .p_4()
                .min_w_0()
                .max_w_full()
                .overflow_hidden()
                .text_color(cx.theme().muted_foreground)
                .child("Double-click a ship in the market to put it under this fit.")
                .into_any_element();
        };
        let no_stats = Stats::default();
        self.refresh_fitting_cache(loaded, window);
        let layout = self.column_layout;
        let fit = &self.saved.fits[current];
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let rows = fitting_rows(fit, stats, self.rack_style());

        let table_head = Self::column_header(loaded, layout, cx);

        let body: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(n, &row)| self.fitting_row(loaded, fit, stats, ship, row, n, cx))
            .collect();

        // When the center column is narrower than the columns need, the table scrolls sideways
        // instead of squeezing them; rows scroll vertically under a header that stays put.
        div()
            .id("fitting-scroll-x")
            .size_full()
            .overflow_x_scroll()
            .child(
                v_flex()
                    .h_full()
                    .min_w(px(layout.min_width()))
                    .child(table_head)
                    .child(
                        v_flex()
                            .id("fitting-rows")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_focus(&self.fitting_focus)
                            // A click beside the modules clears the selection; rows stop theirs.
                            .on_click(cx.listener(|g, _: &ClickEvent, window, cx| {
                                window.focus(&g.fitting_focus, cx);
                                if !g.selection.is_empty() {
                                    g.selection.clear();
                                    cx.notify();
                                }
                            }))
                            .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| {
                                g.fitting_key(event, cx);
                            }))
                            .children(body)
                            .children(stats.problems.iter().map(|p| {
                                div().px_2().text_color(cx.theme().danger).child(p.clone())
                            })),
                    ),
            )
            .into_any_element()
    }

    /// pyfa's keys in the fitting view: Delete removes the selection, Ctrl+A selects every
    /// module, Escape clears the selection.
    fn fitting_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        match list_key(event) {
            Some(ListKey::Delete) if !self.selection.is_empty() => {
                let rows = self.selection.take();
                self.apply(Action::Remove(rows), cx);
            }
            Some(ListKey::SelectAll) => {
                self.selection.select_all(self.row_order());
                cx.notify();
            }
            Some(ListKey::Clear) => {
                self.selection.clear();
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    #[expect(clippy::too_many_arguments, reason = "one row needs all of these")]
    fn fitting_row(
        &self,
        loaded: &Rc<Loaded>,
        fit: &Fit,
        stats: &Stats,
        ship: TypeId,
        row: Row,
        n: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        // In a grouped rack a module is linked to the others of its type: selecting one shows
        // them all selected, since whatever is done to one is done to all.
        let group = match row {
            Row::Item(RowRef::Module(slot, i)) => self
                .fit()
                .map_or_else(Vec::new, |f| f.group(slot, i))
                .into_iter()
                .map(|j| RowRef::Module(slot, j))
                .collect(),
            Row::Item(r) => vec![r],
            _ => Vec::new(),
        };
        let selected = group.iter().any(|r| self.selection.contains(r));
        let linked = group.len() > 1;
        // pyfa's "Color fitting view by slot": a faint tint per rack.
        let slot_tint = match row {
            Row::Item(RowRef::Module(slot, _)) | Row::Empty(slot, _) => Some(slot),
            Row::Item(RowRef::Rig(_)) => Some(Slot::Rig),
            Row::Item(RowRef::Subsystem(_)) => Some(Slot::Subsystem),
            Row::Header { .. } | Row::Separator | Row::ModeHeader | Row::Mode(_) => None,
        }
        .filter(|_| self.saved.settings.preferences.general.color_by_slot)
        .and_then(|slot| {
            let tint = match slot {
                Slot::High => theme.danger,
                Slot::Mid => theme.info,
                Slot::Low => theme.success,
                Slot::Rig | Slot::Subsystem => theme.warning,
                // pyfa leaves services untinted.
                Slot::Service => return None,
            };
            Some(tint.opacity(0.12))
        });
        let base = h_flex()
            .id(SharedString::from(format!("fit-row-{n}")))
            .h(px(ROW_H))
            .px_1()
            .gap_1()
            .when_some(slot_tint, Styled::bg)
            .when(selected, |r| r.bg(theme.table_active))
            // A bar down the left edge marks modules linked by grouping.
            .when(linked, |r| {
                r.relative().child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(px(2.))
                        .bg(theme.accent),
                )
            });
        // Rack labels are section bands, set off from the rack above; without labels, racks are
        // only set apart by the gap and border. Only real rows highlight.
        let base = match row {
            Row::Header { .. } | Row::ModeHeader => band(base.when(n > 0, Styled::mt_2), cx),
            Row::Separator => base
                .h(px(1.))
                .mt_2()
                .border_t_1()
                .border_color(theme.border),
            _ => base.hover(|s| s.bg(theme.table_hover)),
        };
        match row {
            Row::Header { slot, total } => base
                .child(rack_band_label(loaded, slot, total, cx))
                .when(matches!(slot, Slot::High | Slot::Mid | Slot::Low), |row| {
                    row.child(self.group_toggle(slot, cx))
                })
                .context_menu(ship_menu_builder(cx.weak_entity(), ship))
                .into_any_element(),
            Row::Separator => base
                .context_menu(ship_menu_builder(cx.weak_entity(), ship))
                .into_any_element(),
            Row::ModeHeader => base
                .child(div().w(px(STATE_W + ICON_W - 4.)).flex_none())
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Tactical Mode"),
                )
                .context_menu(ship_menu_builder(cx.weak_entity(), ship))
                .into_any_element(),
            Row::Mode(mode) => base
                .when_some(self.problems_of(loaded, ItemRef::Mode), |row, why| {
                    flag(row, why, theme.danger)
                })
                .child(div().w(px(STATE_W)).flex_none())
                .child(icon_box(catalog, &loaded.icons, Icon::Type(mode), 22.))
                .child(
                    self.column_layout
                        .cell(Col::Name)
                        .truncate()
                        .child(catalog.name(mode)),
                )
                .context_menu({
                    // pyfa's fittingMode entries come before the ship's.
                    let ship_menu = ship_menu_builder(cx.weak_entity(), ship);
                    let this = cx.weak_entity();
                    move |menu, window, cx| {
                        let menu = menu
                            .item(stats_entry(
                                &this,
                                "Tactical Mode",
                                StatsOf::Fitted(FitItem::Mode),
                            ))
                            .separator();
                        ship_menu(menu, window, cx)
                    }
                })
                .into_any_element(),
            Row::Empty(slot, pos) => {
                let row = base.child(div().w(px(STATE_W + ICON_W)).flex_none()).child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("[Empty {} slot]", slot_name(slot))),
                );
                drop_target(row, DropOn::Empty(slot, pos), cx)
                    .context_menu(ship_menu_builder(cx.weak_entity(), ship))
                    .into_any_element()
            }
            Row::Item(item) => {
                let base = match item {
                    RowRef::Module(slot, i) => {
                        let Some(m) = &fit.rack(slot)[i] else {
                            return div().into_any_element();
                        };
                        let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                        let misc = self.cached_misc(FitItem::Module(slot, i));
                        self.module_cells(base, loaded, (slot, i), m, info, misc, cx)
                    }
                    RowRef::Rig(i) | RowRef::Subsystem(i) => {
                        let id = match item {
                            RowRef::Rig(_) => fit.rigs[i].unwrap_or_default(),
                            _ => fit.subsystems[i],
                        };
                        let off = matches!(item, RowRef::Rig(_)) && !fit.rig_active(i);
                        let muted = theme.muted_foreground;
                        base.child(self.rig_state(item, off, cx))
                            .child(icon_box(catalog, &loaded.icons, Icon::Type(id), 22.))
                            .child(
                                self.column_layout
                                    .cell(Col::Name)
                                    .truncate()
                                    .when(off, |d| d.text_color(muted))
                                    .child(catalog.name(id)),
                            )
                            // Empty cells up to the price, so it lines up with the modules'.
                            .children(
                                [Col::Powergrid, Col::Cpu, Col::Capacitor, Col::Range]
                                    .map(|col| self.column_layout.cell(col)),
                            )
                            .child(misc_cell(
                                self.column_layout,
                                format!("misc-{item:?}"),
                                match item {
                                    RowRef::Subsystem(i) => {
                                        self.cached_misc(FitItem::Subsystem(i)).cloned()
                                    }
                                    _ => None,
                                },
                            ))
                            .child(
                                self.column_layout
                                    .cell(Col::Price)
                                    .truncate()
                                    .children(self.prices.get(id).map(short)),
                            )
                            // And after it, so the name takes the same room as a module's.
                            .children(
                                [Col::Charge, Col::Heat].map(|col| self.column_layout.cell(col)),
                            )
                    }
                };
                // pyfa's red rows: whatever breaks a fitting rule or needs skills the
                // character lacks, with the reasons on hover.
                let danger = cx.theme().danger;
                let base = base.when_some(self.problems_of(loaded, item.into()), |row, why| {
                    flag(row, why, danger)
                });
                let dragged = Dragged::of(loaded, DragSource::Row(item), self.row_type(item));
                let base = drop_target(base, DropOn::Item(item), cx);
                let base = match dragged {
                    Some(dragged) => base.on_drag(dragged, |dragged, _, _, cx| {
                        cx.new(|_| DragPreview(dragged.clone()))
                    }),
                    None => base,
                };
                item_row_behaviour(base, loaded, item, ship, cx)
            }
        }
    }

    /// The rack header's switch for grouping its modules by type.
    fn group_toggle(&self, slot: Slot, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let grouped = self.fit().is_some_and(|f| f.grouped.contains(&slot));
        let tip = if grouped {
            "Modules of a type are grouped and change together. Click to ungroup."
        } else {
            "Group modules of a type, so changing one changes them all"
        };
        div()
            .id(SharedString::from(format!("group-{slot:?}")))
            .ml_1()
            .p_0p5()
            .rounded_sm()
            .when(grouped, |d| d.bg(theme.accent))
            .hover(|d| d.bg(theme.table_hover))
            .child(
                UiIcon::new(AssetIcon::Group)
                    .size(px(14.))
                    .text_color(if grouped {
                        theme.accent_foreground
                    } else {
                        theme.muted_foreground
                    }),
            )
            .tip(tip)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.apply(Action::ToggleGrouped(slot), cx);
            }))
    }

    /// A rig's on/off mark, which a click flips for it and the selected rigs; subsystems have
    /// none.
    fn rig_state(&self, item: RowRef, off: bool, cx: &mut Context<Self>) -> AnyElement {
        let cell = div().w(px(STATE_W)).flex_none();
        let RowRef::Rig(i) = item else {
            return cell.into_any_element();
        };
        let (mark, mark_color) = if off {
            (AssetIcon::Power, self.palette.muted)
        } else {
            (AssetIcon::Circle, self.palette.yellow)
        };
        cell.id(SharedString::from(format!("state-rig-{i}")))
            .flex()
            .items_center()
            .child(
                UiIcon::new(mark)
                    .size(px(15.))
                    .text_color(color(mark_color)),
            )
            .tip(if off { "Off" } else { "On" })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                let others = this
                    .targets(item, Scope::Selection)
                    .into_iter()
                    .filter_map(|r| match r {
                        RowRef::Rig(j) => Some(j),
                        _ => None,
                    })
                    .collect();
                this.apply(Action::ToggleRigs(i, others), cx);
            }))
            .into_any_element()
    }

    #[expect(clippy::too_many_arguments, reason = "one row needs all of these")]
    fn module_cells(
        &self,
        base: Stateful<Div>,
        loaded: &Loaded,
        (slot, i): (Slot, usize),
        m: &Module,
        info: ModuleInfo,
        misc: Option<&(String, String)>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let palette = &self.palette;
        // pyfa's state icons: a flame marks an overheated module.
        let (mark, mark_color) = match m.state {
            ModuleState::Offline => (AssetIcon::Power, palette.muted),
            ModuleState::Online => (AssetIcon::Circle, palette.yellow),
            ModuleState::Active => (AssetIcon::CircleCheck, palette.green),
            ModuleState::Overload => (AssetIcon::Flame, palette.orange),
        };
        let overheats = catalog.get(m.type_id).is_some_and(|t| t.overloadable);
        let layout = self.column_layout;
        let prices = &self.prices;
        let expanded = self
            .saved
            .settings
            .preferences
            .general
            .expanded_mutant_names;
        let misc_text = misc.map(|(text, _)| text.as_str());
        let text = |col: Col| col.text(catalog, prices, m, &info, expanded, misc_text);

        base.child(
            div()
                .id(SharedString::from(format!("state-{slot:?}-{i}")))
                .w(px(STATE_W))
                .flex_none()
                .flex()
                .items_center()
                .child(
                    UiIcon::new(mark)
                        .size(px(15.))
                        .text_color(color(mark_color)),
                )
                .tip(m.state.label())
                // A right click overheats, or stops overheating; other modules keep the menu.
                .when(overheats, |cell| {
                    cell.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let row = RowRef::Module(slot, i);
                            let others = if event.modifiers.alt {
                                this.targets(row, Scope::Similar)
                            } else {
                                this.targets(row, Scope::Selection)
                            };
                            this.apply(
                                Action::ToggleOverheat {
                                    main: (slot, i),
                                    others,
                                },
                                cx,
                            );
                        }),
                    )
                })
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    // Like pyfa: a click on a selected module takes the selection along, Alt
                    // takes every module of its kind, and Ctrl switches them off.
                    let modifiers = event.modifiers();
                    let row = RowRef::Module(slot, i);
                    let others = if modifiers.alt {
                        this.targets(row, Scope::Similar)
                    } else {
                        this.targets(row, Scope::Selection)
                    };
                    this.apply(
                        Action::CycleState {
                            main: (slot, i),
                            others,
                            offline: modifiers.control,
                        },
                        cx,
                    );
                })),
        )
        .child(icon_box(catalog, &loaded.icons, Icon::Type(m.type_id), 22.))
        .child(
            layout
                .cell(Col::Name)
                .truncate()
                .when(m.state == ModuleState::Offline, |d| {
                    d.text_color(theme.muted_foreground)
                })
                .children(text(Col::Name)),
        )
        .children(
            [Col::Powergrid, Col::Cpu, Col::Capacitor]
                .map(|col| layout.cell(col).truncate().children(text(col))),
        )
        .child(
            layout
                .cell(Col::Range)
                .id(SharedString::from(format!("range-{slot:?}-{i}")))
                .truncate()
                .children(text(Col::Range))
                .when_some(super::columns::range_tooltip(&info), |cell, tip| {
                    cell.tip(SharedString::from(tip))
                }),
        )
        .child(misc_cell(
            layout,
            format!("misc-{slot:?}-{i}"),
            misc.cloned(),
        ))
        .child(
            layout
                .cell(Col::Price)
                .id(SharedString::from(format!("price-{slot:?}-{i}")))
                .truncate()
                .children(text(Col::Price))
                .when_some(prices.source_label(m.type_id), |cell, source| {
                    let source = SharedString::from(format!("Price from {source}"));
                    cell.tip(source)
                }),
        )
        .child(layout.cell(Col::Charge).child(h_flex().gap_1().when_some(
            m.charge,
            |cell, charge| {
                cell.child(icon_box(catalog, &loaded.icons, Icon::Type(charge), 20.))
                    .child(div().min_w_0().truncate().children(text(Col::Charge)))
            },
        )))
        .child(
            layout
                .cell(Col::Heat)
                .id(SharedString::from(format!("heat-{slot:?}-{i}")))
                .truncate()
                .children(text(Col::Heat))
                .when(info.burnout_from_neighbours, |cell| {
                    cell.text_color(cx.theme().muted_foreground)
                })
                .when(info.burnout.is_some(), |cell| {
                    let tip = if info.burnout_from_neighbours {
                        "Estimated time til burnout from the overheated modules in its rack"
                    } else {
                        "Estimated time til burnout"
                    };
                    cell.tip(tip)
                }),
        )
    }
}

/// The Miscellanea cell, with its tooltip saying what the figure is.
fn misc_cell(
    layout: super::columns::Layout,
    id: String,
    misc: Option<(String, String)>,
) -> Stateful<Div> {
    let cell = layout.cell(Col::Misc).id(SharedString::from(id)).truncate();
    match misc {
        Some((text, tooltip)) => cell.child(text).when(!tooltip.is_empty(), |cell| {
            let tooltip = SharedString::from(tooltip);
            cell.tip(tooltip)
        }),
        None => cell,
    }
}

/// Selection, double-click removal and the context menu, shared by every item row.
fn item_row_behaviour(
    base: Stateful<Div>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    ship: TypeId,
    cx: &mut Context<Efa>,
) -> AnyElement {
    let this = cx.weak_entity();
    let loaded = Rc::clone(loaded);
    base.on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
        cx.stop_propagation();
        window.focus(&g.fitting_focus, cx);
        if event.click_count() >= 2 {
            g.apply(Action::Remove(vec![item]), cx);
        } else {
            let order = g.row_order();
            g.selection.click(item, event.modifiers(), &order);
            cx.notify();
        }
    }))
    // A right-click on an unselected row makes it the selection, as list views do.
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |g, _: &MouseDownEvent, _, cx| {
            if g.selection.right_click(item) {
                cx.notify();
            }
        }),
    )
    .context_menu(move |menu, window, cx| {
        let Some(snapshot) = MenuSnapshot::take(&this, item, cx) else {
            return menu;
        };
        item_menu(menu, &this, &loaded, &snapshot, ship, window, cx)
    })
    .into_any_element()
}

/// How the fitting view sets racks apart: pyfa's "Separate Racks" and "Show Rack Labels".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RackStyle {
    /// One list, racks back to back.
    Plain,
    /// A blank row between racks.
    Separators,
    /// A labelled row over each rack.
    Labels,
}

impl Efa {
    fn rack_style(&self) -> RackStyle {
        let general = &self.saved.settings.preferences.general;
        match (general.rack_slots, general.rack_labels) {
            (false, _) => RackStyle::Plain,
            (true, false) => RackStyle::Separators,
            (true, true) => RackStyle::Labels,
        }
    }
}

/// The fitting view's rows: each rack padded with empty slots to what the ship has, set apart
/// the way `style` says.
fn fitting_rows(fit: &Fit, stats: &Stats, style: RackStyle) -> Vec<Row> {
    let mut rows = Vec::new();
    // pyfa puts a separator before every rack but the first, and a label before every one.
    let set_apart = |rows: &mut Vec<Row>, header: Row| match style {
        RackStyle::Plain => {}
        RackStyle::Separators if rows.is_empty() => {}
        RackStyle::Separators => rows.push(Row::Separator),
        RackStyle::Labels => rows.push(header),
    };
    let rack = |rows: &mut Vec<Row>, slot: Slot, slots: crate::engine::Slots, filled: Vec<Cell>| {
        let total = slots.total.unwrap_or(0);
        if total == 0 && !filled.iter().any(|&c| c != Cell::Empty) {
            return;
        }
        set_apart(rows, Row::Header { slot, total });
        let positions = filled.len().max(total as usize);
        for pos in 0..positions {
            match filled.get(pos).copied().unwrap_or(Cell::Empty) {
                Cell::Item => rows.push(Row::Item(match slot {
                    Slot::Rig => RowRef::Rig(pos),
                    Slot::Subsystem => RowRef::Subsystem(pos),
                    Slot::High | Slot::Mid | Slot::Low | Slot::Service => RowRef::Module(slot, pos),
                })),
                Cell::Empty => rows.push(Row::Empty(slot, pos)),
            }
        }
    };
    for (slot, slots) in [
        (Slot::High, stats.high),
        (Slot::Mid, stats.mid),
        (Slot::Low, stats.low),
    ] {
        rack(&mut rows, slot, slots, rack_cells(fit, slot));
    }
    rack(
        &mut rows,
        Slot::Rig,
        stats.rig,
        fit.rigs.iter().map(|r| Cell::of(r.is_some())).collect(),
    );
    rack(
        &mut rows,
        Slot::Subsystem,
        stats.subsystem,
        vec![Cell::Item; fit.subsystems.len()],
    );
    rack(
        &mut rows,
        Slot::Service,
        stats.service,
        rack_cells(fit, Slot::Service),
    );
    if let Some(mode) = fit.mode {
        set_apart(&mut rows, Row::ModeHeader);
        rows.push(Row::Mode(mode));
    }
    rows
}

/// What a rack position shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    Item,
}

impl Cell {
    fn of(filled: bool) -> Self {
        if filled { Self::Item } else { Self::Empty }
    }
}

/// A module rack's positions.
fn rack_cells(fit: &Fit, slot: Slot) -> Vec<Cell> {
    fit.rack(slot)
        .iter()
        .map(|m| Cell::of(m.is_some()))
        .collect()
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "High",
        Slot::Mid => "Med",
        Slot::Low => "Low",
        Slot::Rig => "Rig",
        Slot::Subsystem => "Subsystem",
        Slot::Service => "Service",
    }
}
