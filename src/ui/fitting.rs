//! The middle: the fit's slots as a table, and the additions pane under it.

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, FontWeight, InteractiveElement,
    IntoElement, Modifiers, MouseButton, MouseDownEvent, ParentElement, Render, SharedString,
    Stateful, StatefulInteractiveElement, Styled, WeakEntity, Window, div, img,
    prelude::FluentBuilder, px,
};

use super::{
    Action, Gyfa, Loaded, RowRef, Scope,
    characters::skills_submenu,
    columns::{Col, ICON_W, STATE_W},
    item_stats::{StatsOf, stats_entry},
    widgets::{color, entry, icon, icon_box, icon_label, item_entry, item_label, short},
};
use crate::{
    data::{Catalog, Hardpoint, ItemType, Kind, Slot, TypeId},
    engine::{FitItem, ModuleInfo, SkillTarget, Stats},
    fit::{Fit, Module, ModuleState},
    icons::Icon,
};

const ROW_HEIGHT: f32 = 24.;
/// Tallest a long menu grows before it scrolls.
const MENU_MAX_H: f32 = 480.;

#[derive(Debug, Clone, Copy)]
enum Row {
    Header {
        label: &'static str,
        icon_id: i32,
        total: u32,
    },
    Item(RowRef),
    Empty(Slot, usize),
    /// A tactical destroyer's mode, shown under the racks like pyfa does.
    Mode(TypeId),
}

impl Gyfa {
    /// The rows that hold something, top to bottom, for Shift-click ranges.
    fn row_order(&self) -> Vec<RowRef> {
        let (Some(fit), Some(stats)) = (self.fit(), self.stats.as_ref()) else {
            return Vec::new();
        };
        fitting_rows(fit, stats)
            .into_iter()
            .filter_map(|row| match row {
                Row::Item(r) => Some(r),
                Row::Header { .. } | Row::Empty(..) | Row::Mode(_) => None,
            })
            .collect()
    }

    pub(super) fn fitting_view(
        &mut self,
        loaded: &Rc<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(current) = self.saved.current else {
            return div().into_any_element();
        };
        let Some(ship) = self.saved.fits[current].ship else {
            return div()
                .p_4()
                .text_color(cx.theme().muted_foreground)
                .child("Double-click a ship in the market to put it under this fit.")
                .into_any_element();
        };
        let no_stats = Stats::default();
        let layout = {
            let stats = self.stats.as_ref().unwrap_or(&no_stats);
            self.column_layout(loaded, &self.saved.fits[current], stats, window)
        };
        self.column_layout = layout;
        let fit = &self.saved.fits[current];
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let rows = fitting_rows(fit, stats);

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
                            .children(body)
                            .children(stats.problems.iter().map(|p| {
                                div().px_2().text_color(cx.theme().danger).child(p.clone())
                            })),
                    ),
            )
            .into_any_element()
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
        let selected = matches!(row, Row::Item(r) if self.selection.contains(&r));
        let base = h_flex()
            .id(SharedString::from(format!("fit-row-{n}")))
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_1()
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover));
        match row {
            Row::Header {
                label,
                icon_id,
                total,
            } => base
                .child(div().w(px(STATE_W)).flex_none())
                .child(div().w(px(ICON_W - 4.)).flex_none().child(icon_box(
                    catalog,
                    &loaded.icons,
                    Icon::Id(icon_id),
                    18.,
                )))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("— {total} {label} —")),
                )
                .context_menu(ship_menu_builder(Self::weak(cx), ship))
                .into_any_element(),
            Row::Mode(mode) => base
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
                    let ship_menu = ship_menu_builder(Self::weak(cx), ship);
                    let this = Self::weak(cx);
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
                    .context_menu(ship_menu_builder(Self::weak(cx), ship))
                    .into_any_element()
            }
            Row::Item(item) => {
                let base = match item {
                    RowRef::Module(slot, i) => {
                        let Some(m) = &fit.rack(slot)[i] else {
                            return div().into_any_element();
                        };
                        let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                        let total = match slot {
                            Slot::High => stats.high.total,
                            Slot::Mid => stats.mid.total,
                            _ => stats.low.total,
                        };
                        let cells = self.module_cells(base, loaded, (slot, i), m, info, cx);
                        // Fits from before slots were enforced can hold more than the ship has.
                        if i >= total.unwrap_or(0) as usize {
                            cells.text_color(cx.theme().danger).tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(
                                    "The ship has no slot for this module",
                                )
                                .build(window, cx)
                            })
                        } else {
                            cells
                        }
                    }
                    RowRef::Rig(i) | RowRef::Subsystem(i) => {
                        let id = match item {
                            RowRef::Rig(_) => fit.rigs[i].unwrap_or_default(),
                            _ => fit.subsystems[i],
                        };
                        base.child(div().w(px(STATE_W)).flex_none())
                            .child(icon_box(catalog, &loaded.icons, Icon::Type(id), 22.))
                            .child(
                                self.column_layout
                                    .cell(Col::Name)
                                    .truncate()
                                    .child(catalog.name(id)),
                            )
                            // Empty cells up to the price, so it lines up with the modules'.
                            .children(
                                [Col::Powergrid, Col::Cpu, Col::Capacitor, Col::Range]
                                    .map(|col| self.column_layout.cell(col)),
                            )
                            .child(
                                self.column_layout
                                    .cell(Col::Price)
                                    .truncate()
                                    .children(self.prices.get(id).map(short)),
                            )
                    }
                };
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

    fn module_cells(
        &self,
        base: Stateful<Div>,
        loaded: &Loaded,
        (slot, i): (Slot, usize),
        m: &Module,
        info: ModuleInfo,
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
        let layout = self.column_layout;
        let prices = &self.prices;
        let text = |col: Col| col.text(catalog, prices, m, &info);
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
                .tooltip({
                    let label = m.state.label();
                    move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(label).build(window, cx)
                    }
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.apply(Action::CycleState(slot, i), cx);
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
            [
                Col::Powergrid,
                Col::Cpu,
                Col::Capacitor,
                Col::Range,
                Col::Price,
            ]
            .map(|col| layout.cell(col).truncate().children(text(col))),
        )
        .child(layout.cell(Col::Charge).child(h_flex().gap_1().when_some(
            m.charge,
            |cell, charge| {
                cell.child(icon_box(catalog, &loaded.icons, Icon::Type(charge), 20.))
                    .child(div().truncate().children(text(Col::Charge)))
            },
        )))
    }
}

/// Selection, double-click removal and the context menu, shared by every item row.
fn item_row_behaviour(
    base: Stateful<Div>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    ship: TypeId,
    cx: &mut Context<Gyfa>,
) -> AnyElement {
    let this = Gyfa::weak(cx);
    let loaded = Rc::clone(loaded);
    base.on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
        if event.click_count() >= 2 {
            g.apply(Action::Remove(vec![item]), cx);
        } else {
            let order = g.row_order();
            g.select(item, event.modifiers(), &order, cx);
        }
    }))
    // A right-click on an unselected row makes it the selection, as list views do.
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |g, _: &MouseDownEvent, _, cx| {
            if !g.selection.contains(&item) {
                g.selection = vec![item];
                g.anchor = Some(item);
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

/// Where a dragged item came from.
#[derive(Debug, Clone, Copy)]
pub enum DragSource {
    Market,
    Row(RowRef),
    /// A stack in the cargo tab.
    Cargo,
    /// A stack of the drones pane, by its index in the fit.
    Drone(usize),
}

/// An item being dragged: from the market onto a slot, or between slots.
#[derive(Debug, Clone)]
pub struct Dragged {
    pub source: DragSource,
    pub type_id: TypeId,
    label: SharedString,
    icon: Option<std::path::PathBuf>,
}

impl Dragged {
    pub fn of(loaded: &Loaded, source: DragSource, type_id: Option<TypeId>) -> Option<Self> {
        let type_id = type_id?;
        Some(Self {
            source,
            type_id,
            label: loaded.catalog.name(type_id).into(),
            icon: loaded.icons.path(&loaded.catalog, Icon::Type(type_id)),
        })
    }
}

/// What follows the pointer while dragging: the item's icon and name.
pub struct DragPreview(pub Dragged);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.ring)
            .shadow_md()
            .opacity(0.9)
            .when_some(self.0.icon.clone(), |row, path| {
                row.child(img(path).size(px(22.)))
            })
            .child(self.0.label.clone())
    }
}

/// A fitting row a drag can end on.
#[derive(Debug, Clone, Copy)]
enum DropOn {
    Item(RowRef),
    Empty(Slot, usize),
}

impl DropOn {
    fn position(self) -> Option<(Slot, usize)> {
        match self {
            Self::Item(RowRef::Module(slot, i)) | Self::Empty(slot, i) => Some((slot, i)),
            Self::Item(RowRef::Rig(i)) => Some((Slot::Rig, i)),
            Self::Item(RowRef::Subsystem(_)) => None,
        }
    }

    fn row(self) -> Option<RowRef> {
        match self {
            Self::Item(row) => Some(row),
            Self::Empty(Slot::Rig, i) => Some(RowRef::Rig(i)),
            Self::Empty(slot @ (Slot::High | Slot::Mid | Slot::Low), i) => {
                Some(RowRef::Module(slot, i))
            }
            Self::Empty(Slot::Subsystem, _) => None,
        }
    }
}

/// Highlight a row while something is dragged over it, and handle the drop.
fn drop_target(row: Stateful<Div>, on: DropOn, cx: &mut Context<Gyfa>) -> Stateful<Div> {
    row.drag_over::<Dragged>(|style, _, _, cx| style.bg(cx.theme().drop_target))
        .on_drop(cx.listener(move |g, dragged: &Dragged, window, cx| {
            cx.stop_propagation();
            g.drop_on(on, dragged, window.modifiers(), cx);
        }))
}

impl Gyfa {
    /// pyfa's drop rules: a module onto its own rack goes to that position, a charge onto a
    /// module loads it, a module dragged within its rack swaps places, anything else is added.
    fn drop_on(
        &mut self,
        on: DropOn,
        dragged: &Dragged,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let Some(t) = loaded.catalog.get(dragged.type_id) else {
            return;
        };
        match dragged.source {
            DragSource::Row(from) => {
                if let Some(to) = on.row() {
                    self.apply(Action::Swap(from, to), cx);
                }
            }
            // Drone stacks only move within the drones pane.
            DragSource::Drone(_) => {}
            DragSource::Market => match (t.kind, on.position()) {
                (Kind::Module(slot), Some((target, pos))) if slot == target => {
                    self.apply(Action::Place(slot, pos, t.id), cx);
                }
                (Kind::Charge, Some(_)) => match on {
                    DropOn::Item(row @ RowRef::Module(..)) => {
                        self.apply(Action::SetCharge(vec![row], Some(t.id)), cx);
                    }
                    _ => self.add_item(t.id, cx),
                },
                _ => self.add_item(t.id, cx),
            },
            DragSource::Cargo => {
                if let Some(onto) = on.position() {
                    let transfer = super::additions::drop_transfer(modifiers);
                    self.apply(Action::CargoToModule(t.id, onto, transfer), cx);
                }
            }
        }
    }
}

/// What a row's menu needs to know, read when the menu opens.
struct MenuSnapshot {
    item: RowRef,
    type_id: TypeId,
    module: Option<Module>,
    info: ModuleInfo,
}

impl MenuSnapshot {
    fn take(this: &WeakEntity<Gyfa>, item: RowRef, cx: &App) -> Option<Self> {
        let g = this.upgrade()?;
        let g = g.read(cx);
        let fit = g.fit()?;
        let (type_id, module) = match item {
            RowRef::Module(slot, i) => {
                let m = fit.rack(slot).get(i)?.clone()?;
                (m.type_id, Some(m))
            }
            RowRef::Rig(i) => ((*fit.rigs.get(i)?)?, None),
            RowRef::Subsystem(i) => (*fit.subsystems.get(i)?, None),
        };
        let info = match item {
            RowRef::Module(slot, i) => g
                .stats
                .as_ref()
                .and_then(|s| s.modules.get(&(slot, i)).copied())
                .unwrap_or_default(),
            RowRef::Rig(_) | RowRef::Subsystem(_) => ModuleInfo::default(),
        };
        Some(Self {
            item,
            type_id,
            module,
            info,
        })
    }
}

/// A menu entry handler that runs `f` on the app.
fn handler(
    this: &WeakEntity<Gyfa>,
    f: impl Fn(&mut Gyfa, &ClickEvent, &mut Window, &mut Context<Gyfa>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let this = this.clone();
    move |event, window, cx| {
        let _ = this.update(cx, |g, cx| f(g, event, window, cx));
    }
}

/// Act on `item` and whatever the click's scope adds to it.
fn scoped(
    this: &WeakEntity<Gyfa>,
    item: RowRef,
    default: Scope,
    action: impl Fn(Vec<RowRef>) -> Action + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    handler(this, move |g, event, _, cx| {
        let targets = g.targets(item, Scope::from_click(default, event));
        g.apply(action(targets), cx);
    })
}

/// pyfa's fittingModule, fittingCharge and fittingShip menus, in its order.
fn item_menu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    snapshot: &MenuSnapshot,
    ship: TypeId,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let (item, type_id) = (snapshot.item, snapshot.type_id);
    let Some(t) = catalog.get(type_id) else {
        return menu;
    };
    let mut menu = menu;

    if let Some(m) = &snapshot.module {
        let charges = compatible_charges(catalog, type_id);
        if !charges.is_empty() {
            let structure = structure_charges(catalog, t, &charges);
            menu = charge_submenu(menu, this, loaded, item, structure, m.charge, window, cx);
        }
        if let Some(max) = snapshot.info.spool_max.filter(|_| t.spools) {
            menu = spool_submenu(menu, this, item, m.spool, max, window, cx);
        }
    }
    let fitted = match item {
        RowRef::Module(slot, i) => FitItem::Module(slot, i),
        RowRef::Rig(i) => FitItem::Rig(i),
        RowRef::Subsystem(i) => FitItem::Subsystem(i),
    };
    menu = menu.item(stats_entry(
        this,
        t.category_label(),
        StatsOf::Fitted(fitted),
    ));
    if t.market_group.is_some() {
        menu = menu.item(
            entry(IconName::Search, "Module Market Group").on_click(handler(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(type_id, window, cx);
                },
            )),
        );
    }
    menu = menu.item(entry(IconName::Delete, "Remove Module").on_click(scoped(
        this,
        item,
        Scope::Selection,
        Action::Remove,
    )));
    let variations = catalog.variations_of(type_id);
    if variations.len() > 1 {
        menu = variations_submenu(menu, this, loaded, item, type_id, variations, window, cx);
    }
    let rack = match item {
        RowRef::Module(slot, _) => slot,
        RowRef::Rig(_) => Slot::Rig,
        RowRef::Subsystem(_) => Slot::Subsystem,
    };
    let slot = (rack != Slot::Subsystem).then_some(rack);
    if let Some(slot) = slot {
        menu = menu.item(
            entry(IconName::Plus, format!("Fill With {}", t.category_label())).on_click(handler(
                this,
                move |g, _, _, cx| {
                    g.apply(Action::FillRack(slot, type_id), cx);
                },
            )),
        );
    }

    menu = skills_submenu(
        menu,
        this,
        loaded,
        SkillTarget::Module(rack, type_id),
        window,
        cx,
    );

    let charge = snapshot.module.as_ref().and_then(|m| m.charge);
    if charge.is_some()
        && let RowRef::Module(slot, i) = item
    {
        menu = menu.separator().item(stats_entry(
            this,
            "Charge",
            StatsOf::Fitted(FitItem::Charge(slot, i)),
        ));
    }
    if let Some(charge) = charge
        && catalog
            .get(charge)
            .is_some_and(|c| c.market_group.is_some())
    {
        menu = menu.item(
            entry(IconName::Search, "Charge Market Group").on_click(handler(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(charge, window, cx);
                },
            )),
        );
    }
    if let Some(charge) = charge {
        let target = SkillTarget::Charge {
            slot: rack,
            module: type_id,
            charge,
        };
        menu = skills_submenu(menu, this, loaded, target, window, cx);
    }

    ship_entries(menu.separator(), this, ship, window, cx)
}

fn ship_menu_builder(
    this: WeakEntity<Gyfa>,
    ship: TypeId,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| ship_entries(menu, &this, ship, window, cx)
}

/// Ships whose bonuses scale with the pilot's security status.
const SEC_STATUS_SHIPS: [&str; 6] = [
    "Pacifier",
    "Enforcer",
    "Marshal",
    "Sidewinder",
    "Cobra",
    "Python",
];
const SEC_STATUS_PRESETS: [f64; 11] = [-10., -8., -6., -4., -2., 0., 1., 2., 3., 4., 5.];

/// pyfa's fittingShip menu.
pub(super) fn ship_entries(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    ship: TypeId,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(g) = this.upgrade() else { return menu };
    let (loaded, mode, sec_status, character_sec) = {
        let g = g.read(cx);
        let fit = g.fit();
        (
            g.loaded(),
            fit.and_then(|f| f.mode),
            fit.and_then(|f| f.sec_status),
            g.characters.sec_status(g.fit_character()),
        )
    };
    let Some(loaded) = loaded else { return menu };
    let ship_name = loaded.catalog.name(ship);
    let mut menu = menu;

    let modes = loaded.catalog.modes_for(ship);
    if !modes.is_empty() {
        let (this, loaded, prefix) = (this.clone(), Rc::clone(&loaded), format!("{ship_name} "));
        menu = menu.submenu_with_element(
            "Tactical Mode",
            icon_label(IconName::Settings2, "Tactical Mode"),
            window,
            cx,
            move |menu, _, _| {
                modes.iter().fold(menu, |menu, &id| {
                    let name = loaded.catalog.name(id);
                    let label = name
                        .strip_prefix(&prefix)
                        .unwrap_or(&name)
                        .trim_end_matches(" Mode")
                        .to_owned();
                    menu.item(
                        item_entry(&loaded, id, label)
                            .checked(mode == Some(id))
                            .on_click(handler(&this, move |g, _, _, cx| {
                                g.apply(Action::SetMode(id), cx);
                            })),
                    )
                })
            },
        );
    }
    menu = menu.item(stats_entry(this, "Ship", StatsOf::Fitted(FitItem::Ship)));
    if SEC_STATUS_SHIPS.contains(&ship_name.as_str()) {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Pilot Security Status",
            icon_label(IconName::CircleUser, "Pilot Security Status"),
            window,
            cx,
            move |menu, _, _| {
                let set = |value: Option<f64>| {
                    handler(&this, move |g, _, _, cx| {
                        g.apply(Action::SetSecStatus(value), cx);
                    })
                };
                let custom = sec_status.filter(|v| !SEC_STATUS_PRESETS.contains(v));
                let custom_label = match custom {
                    Some(v) => format!("Custom ({v})"),
                    None => "Custom…".to_owned(),
                };
                let mut menu = menu
                    .item(
                        PopupMenuItem::new(format!("Character ({})", py_round(character_sec)))
                            .checked(sec_status.is_none())
                            .on_click(set(None)),
                    )
                    .item(
                        PopupMenuItem::new(custom_label)
                            .checked(custom.is_some())
                            .on_click(handler(&this, move |_, _, window, cx| {
                                Gyfa::open_value_dialog(
                                    "Security Status (-10 to 5)",
                                    custom.unwrap_or(0.0).to_string(),
                                    window,
                                    cx,
                                    |v: f64| Action::SetSecStatus(Some(v.clamp(-10.0, 5.0))),
                                );
                            })),
                    )
                    .separator();
                for preset in SEC_STATUS_PRESETS {
                    menu = menu.item(
                        PopupMenuItem::new(format!("{preset}"))
                            .checked(sec_status == Some(preset))
                            .on_click(set(Some(preset))),
                    );
                }
                menu
            },
        );
    }
    let menu = menu.item(
        entry(IconName::FolderOpen, "Open in Fitting Browser").on_click(handler(
            this,
            move |g, _, _, cx| {
                g.jump_to_fittings(ship, cx);
            },
        )),
    );
    skills_submenu(menu, this, &loaded, SkillTarget::Ship, window, cx)
}

/// A number as Python prints `round(x, 2)`, which pyfa shows the character's status as.
fn py_round(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.1}")
    } else {
        format!("{rounded}")
    }
}

/// A charge entry: loads it into every similar module, or only the selection with Ctrl or Alt.
fn charge_entry(
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    id: TypeId,
    loaded_charge: Option<TypeId>,
) -> PopupMenuItem {
    item_entry(loaded, id, loaded.catalog.name(id))
        .checked(loaded_charge == Some(id))
        .on_click(scoped(this, item, Scope::Similar, move |rows| {
            Action::SetCharge(rows, Some(id))
        }))
}

/// A submenu of charges between pyfa's "Less Damage" and "More Damage" markers.
#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn damage_ladder(
    menu: PopupMenu,
    title: String,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    charges: Vec<TypeId>,
    loaded_charge: Option<TypeId>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    menu.submenu_with_element(
        title.clone(),
        item_label(&loaded, charges.first().copied(), title),
        window,
        cx,
        move |menu, _, _| {
            charges
                .iter()
                .fold(menu.label("─ Less Damage ─"), |menu, &c| {
                    menu.item(charge_entry(&this, &loaded, item, c, loaded_charge))
                })
                .label("─ More Damage ─")
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn charge_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    structure: ChargeMenu,
    loaded_charge: Option<TypeId>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    menu.submenu_with_element(
        "Charge",
        icon_label(IconName::BatteryCharging, "Charge"),
        window,
        cx,
        move |menu, window, cx| {
            let catalog = &loaded.catalog;
            let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
            match &structure {
                ChargeMenu::Turret(families) => {
                    menu = menu.label("─ Long Range ─");
                    for family in families {
                        menu = match family.as_slice() {
                            [only] => {
                                menu.item(charge_entry(&this, &loaded, item, *only, loaded_charge))
                            }
                            _ => damage_ladder(
                                menu,
                                catalog.name(family[0]),
                                &this,
                                &loaded,
                                item,
                                family.clone(),
                                loaded_charge,
                                window,
                                cx,
                            ),
                        };
                    }
                    menu = menu.label("─ Short Range ─");
                }
                ChargeMenu::Missile(by_type) => {
                    for (label, charges) in by_type {
                        menu = damage_ladder(
                            menu,
                            (*label).to_owned(),
                            &this,
                            &loaded,
                            item,
                            charges.clone(),
                            loaded_charge,
                            window,
                            cx,
                        );
                    }
                }
                ChargeMenu::Flat(list) => {
                    for &c in list {
                        menu = menu.item(charge_entry(&this, &loaded, item, c, loaded_charge));
                    }
                }
            }
            menu.item(entry(IconName::CircleX, "Empty").on_click(scoped(
                &this,
                item,
                Scope::Similar,
                |rows| Action::SetCharge(rows, None),
            )))
        },
    )
}

fn spool_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    item: RowRef,
    current: Option<u32>,
    max: u32,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = this.clone();
    menu.submenu_with_element(
        "Spoolup Cycles",
        icon_label(IconName::RefreshCw, "Spoolup Cycles"),
        window,
        cx,
        move |menu, _, _| {
            let menu = (0..=max).fold(
                menu.scrollable(true).max_h(px(MENU_MAX_H)),
                |menu, cycles| {
                    let label = if cycles == max {
                        format!("{cycles} (default)")
                    } else {
                        cycles.to_string()
                    };
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(current.unwrap_or(max) == cycles)
                            .on_click(scoped(&this, item, Scope::Selection, move |rows| {
                                Action::SetSpool(rows, Some(cycles))
                            })),
                    )
                },
            );
            menu.separator()
                .item(entry(IconName::Undo2, "Reset").on_click(scoped(
                    &this,
                    item,
                    Scope::Selection,
                    |rows| Action::SetSpool(rows, None),
                )))
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn variations_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    item: RowRef,
    current: TypeId,
    variations: &[TypeId],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    // Grouped under their meta group, groups in the order they first appear by meta level.
    let mut groups: Vec<(String, Vec<(TypeId, String)>)> = Vec::new();
    for &v in variations {
        let Some(t) = catalog.get(v) else { continue };
        let group = t
            .meta_group
            .and_then(|g| catalog.meta_group_names.get(&g))
            .cloned()
            .unwrap_or_else(|| "Other".to_owned());
        match groups.iter_mut().find(|(g, _)| *g == group) {
            Some((_, members)) => members.push((v, t.name.clone())),
            None => groups.push((group, vec![(v, t.name.clone())])),
        }
    }
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    menu.submenu_with_element(
        "Variations",
        icon_label(IconName::Replace, "Variations"),
        window,
        cx,
        move |menu, _, _| {
            let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
            for (group, members) in &groups {
                menu = menu.label(group.clone());
                for (id, name) in members {
                    let id = *id;
                    menu = menu.item(
                        item_entry(&loaded, id, name.clone())
                            .checked(id == current)
                            .on_click(scoped(&this, item, Scope::Selection, move |rows| {
                                Action::SwapType(rows, id)
                            })),
                    );
                }
            }
            menu
        },
    )
}

/// A module's charges, structured the way pyfa's Charge menu shows them.
pub(super) enum ChargeMenu {
    /// Families of the same charge in different grades, from long range to short.
    Turret(Vec<Vec<TypeId>>),
    /// By damage type, each from less damage to more.
    Missile(Vec<(&'static str, Vec<TypeId>)>),
    Flat(Vec<TypeId>),
}

pub(super) fn structure_charges(
    catalog: &Catalog,
    module: &ItemType,
    charges: &[TypeId],
) -> ChargeMenu {
    let charges: Vec<&ItemType> = charges.iter().filter_map(|&c| catalog.get(c)).collect();
    let damage = |c: &ItemType| c.damage.iter().sum::<f64>();
    let name_base = |c: &ItemType| {
        let words: Vec<&str> = c.name.split_whitespace().collect();
        words[words.len().saturating_sub(2)..].join(" ")
    };
    match module.hardpoint {
        Hardpoint::Turret if !module.mining => {
            let range = |c: &ItemType| {
                module.optimal.unwrap_or(0.0) * c.range_multiplier.unwrap_or(1.0)
                    + module.falloff.unwrap_or(0.0) * c.falloff_multiplier.unwrap_or(1.0)
            };
            let mut sorted: Vec<&ItemType> = charges
                .into_iter()
                .filter(|c| !c.name.to_lowercase().contains("civilian"))
                .collect();
            sorted.sort_by(|a, b| {
                range(b)
                    .total_cmp(&range(a))
                    .then_with(|| name_base(a).cmp(&name_base(b)))
                    .then_with(|| damage(a).total_cmp(&damage(b)))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let mut families: Vec<Vec<TypeId>> = Vec::new();
            let mut previous: Option<(String, Option<f64>)> = None;
            for c in sorted {
                let key = (name_base(c), c.range_multiplier);
                match families.last_mut() {
                    Some(last) if previous.as_ref() == Some(&key) => last.push(c.id),
                    _ => families.push(vec![c.id]),
                }
                previous = Some(key);
            }
            ChargeMenu::Turret(families)
        }
        Hardpoint::Launcher => {
            const TYPES: [&str; 4] = ["EM", "Thermal", "Kinetic", "Explosive"];
            // A charge dealing one damage type only files under it; anything else is mixed.
            let damage_type = |c: &ItemType| {
                let total = damage(c);
                c.damage
                    .iter()
                    .position(|&d| d > 0.0 && (d - total).abs() < 1e-9)
                    .unwrap_or(TYPES.len())
            };
            let mut sorted = charges;
            sorted.sort_by(|a, b| {
                damage_type(a)
                    .cmp(&damage_type(b))
                    .then_with(|| damage(a).total_cmp(&damage(b)))
                    .then_with(|| a.name.cmp(&b.name))
            });
            let mut by_type: Vec<(&'static str, Vec<TypeId>)> = Vec::new();
            for c in sorted {
                let label = TYPES.get(damage_type(c)).copied().unwrap_or("Mixed");
                match by_type.last_mut() {
                    Some((l, members)) if *l == label => members.push(c.id),
                    _ => by_type.push((label, vec![c.id])),
                }
            }
            ChargeMenu::Missile(by_type)
        }
        Hardpoint::Turret | Hardpoint::None => {
            ChargeMenu::Flat(charges.iter().map(|c| c.id).collect())
        }
    }
}

/// The fitting view's rows: each rack under its header, padded with empty slots to what the ship
/// has.
fn fitting_rows(fit: &Fit, stats: &Stats) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut rack = |slot: Slot,
                    label: &'static str,
                    icon_id: i32,
                    slots: crate::engine::Slots,
                    filled: Vec<bool>| {
        let total = slots.total.unwrap_or(0);
        if total == 0 && !filled.contains(&true) {
            return;
        }
        rows.push(Row::Header {
            label,
            icon_id,
            total,
        });
        let positions = filled.len().max(total as usize);
        for pos in 0..positions {
            if filled.get(pos).copied().unwrap_or(false) {
                rows.push(Row::Item(match slot {
                    Slot::Rig => RowRef::Rig(pos),
                    Slot::Subsystem => RowRef::Subsystem(pos),
                    Slot::High | Slot::Mid | Slot::Low => RowRef::Module(slot, pos),
                }));
            } else {
                rows.push(Row::Empty(slot, pos));
            }
        }
    };
    for (slot, label, icon_id, slots) in [
        (Slot::High, "High Slots", icon::HIGH, stats.high),
        (Slot::Mid, "Med Slots", icon::MID, stats.mid),
        (Slot::Low, "Low Slots", icon::LOW, stats.low),
    ] {
        rack(
            slot,
            label,
            icon_id,
            slots,
            fit.rack(slot).iter().map(Option::is_some).collect(),
        );
    }
    rack(
        Slot::Rig,
        "Rig Slots",
        icon::RIG,
        stats.rig,
        fit.rigs.iter().map(Option::is_some).collect(),
    );
    rack(
        Slot::Subsystem,
        "Subsystem Slots",
        icon::SUBSYSTEM,
        stats.subsystem,
        vec![true; fit.subsystems.len()],
    );
    if let Some(mode) = fit.mode {
        rows.push(Row::Mode(mode));
    }
    rows
}

/// Published charges that fit a module type, by name.
pub(super) fn compatible_charges(catalog: &Catalog, module: TypeId) -> Vec<TypeId> {
    let Some(m) = catalog.get(module).filter(|m| !m.charge_groups.is_empty()) else {
        return Vec::new();
    };
    let mut charges: Vec<_> = catalog
        .types
        .values()
        .filter(|c| c.published && c.market_group.is_some() && c.fits_in(m))
        .collect();
    charges.sort_by(|a, b| a.name.cmp(&b.name));
    charges.into_iter().map(|c| c.id).collect()
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "High",
        Slot::Mid => "Med",
        Slot::Low => "Low",
        Slot::Rig => "Rig",
        Slot::Subsystem => "Subsystem",
    }
}
