//! The middle: the fit's slots as a table, and the additions pane under it.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, SharedString, Stateful, StatefulInteractiveElement,
    Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{
    Action, AdditionsTab, Gyfa, Loaded, RowRef, Scope,
    widgets::{color, icon, icon_box, km, short},
};
use crate::{
    data::{Catalog, Hardpoint, ItemType, Slot, TypeId},
    engine::{ModuleInfo, Stats},
    fit::{Fit, Module, ModuleState},
    icons::Icon,
};

const ROW_HEIGHT: f32 = 24.;
/// Widths of the fixed columns: state, icon, then PG, CPU, cap, range, charge.
const STATE_W: f32 = 22.;
const ICON_W: f32 = 26.;
const NUMBER_W: f32 = 56.;
const CAP_W: f32 = 66.;
const RANGE_W: f32 = 106.;
const CHARGE_W: f32 = 190.;
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
    Empty(Slot),
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
                Row::Header { .. } | Row::Empty(_) => None,
            })
            .collect()
    }

    pub(super) fn fitting_view(
        &mut self,
        loaded: &Rc<Loaded>,
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
        let fit = &self.saved.fits[current];
        let no_stats = Stats::default();
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let rows = fitting_rows(fit, stats);

        let header = h_flex()
            .id("fit-header")
            .gap_2()
            .p_2()
            .child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(ship),
                32.,
            ))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(loaded.catalog.name(ship)),
            )
            .child(div().w(px(260.)).child(self.name_field()))
            .context_menu(ship_menu_builder(Self::weak(cx), ship));

        let column_head = |icon_id: i32, width: f32| {
            h_flex().w(px(width)).flex_none().child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Id(icon_id),
                18.,
            ))
        };
        let table_head = h_flex()
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_1()
            .bg(cx.theme().table_head)
            .text_color(cx.theme().table_head_foreground)
            .child(div().w(px(STATE_W + ICON_W)).flex_none())
            .child(div().flex_1().min_w(px(140.)).child("Name"))
            .child(column_head(icon::POWERGRID, NUMBER_W))
            .child(column_head(icon::CPU, NUMBER_W))
            .child(column_head(icon::CAPACITOR, CAP_W))
            .child(column_head(icon::RANGE, RANGE_W))
            .child(div().w(px(CHARGE_W)).flex_none().child("Charge"));

        let body: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(n, &row)| self.fitting_row(loaded, fit, stats, ship, row, n, cx))
            .collect();

        v_flex()
            .size_full()
            .child(header)
            .child(table_head)
            .child(
                v_flex()
                    .id("fitting-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(body)
                    .children(
                        stats
                            .problems
                            .iter()
                            .map(|p| div().px_2().text_color(cx.theme().danger).child(p.clone())),
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
                .child(icon_box(catalog, &loaded.icons, Icon::Id(icon_id), 18.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("— {total} {label} —")),
                )
                .context_menu(ship_menu_builder(Self::weak(cx), ship))
                .into_any_element(),
            Row::Empty(slot) => base
                .child(div().w(px(STATE_W + ICON_W)).flex_none())
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("[Empty {} slot]", slot_name(slot))),
                )
                .context_menu(ship_menu_builder(Self::weak(cx), ship))
                .into_any_element(),
            Row::Item(item) => {
                let base = match item {
                    RowRef::Module(slot, i) => {
                        let m = &fit.rack(slot)[i];
                        let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                        self.module_cells(base, loaded, (slot, i), m, info, cx)
                    }
                    RowRef::Rig(i) | RowRef::Subsystem(i) => {
                        let id = match item {
                            RowRef::Rig(_) => fit.rigs[i],
                            _ => fit.subsystems[i],
                        };
                        base.child(div().w(px(STATE_W)).flex_none())
                            .child(icon_box(catalog, &loaded.icons, Icon::Type(id), 22.))
                            .child(div().flex_1().truncate().child(catalog.name(id)))
                    }
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
        let (mark, mark_color) = match m.state {
            ModuleState::Offline => ("○", palette.muted),
            ModuleState::Online => ("○", palette.yellow),
            ModuleState::Active => ("✓", palette.green),
            ModuleState::Overload => ("●", palette.orange),
        };
        let number = |value: Option<String>, width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .truncate()
                .when_some(value, ParentElement::child)
        };
        let range = match (info.optimal, info.falloff) {
            (Some(o), Some(f)) => Some(format!("{}+{}", km(o), km(f))),
            (Some(o), None) => Some(km(o)),
            _ => None,
        };
        let mut name = catalog.name(m.type_id);
        if let (Some(cycles), Some(max)) = (m.spool, info.spool_max) {
            name = format!("{name} ({cycles}/{max} cycles)");
        }
        base.child(
            div()
                .id(SharedString::from(format!("state-{slot:?}-{i}")))
                .w(px(STATE_W))
                .flex_none()
                .text_color(color(mark_color))
                .child(mark)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.apply(Action::CycleState(slot, i), cx);
                })),
        )
        .child(icon_box(catalog, &loaded.icons, Icon::Type(m.type_id), 22.))
        .child(
            div()
                .flex_1()
                .min_w(px(140.))
                .truncate()
                .when(m.state == ModuleState::Offline, |d| {
                    d.text_color(theme.muted_foreground)
                })
                .child(name),
        )
        .child(number(info.powergrid.map(short), NUMBER_W))
        .child(number(info.cpu.map(short), NUMBER_W))
        .child(number(
            info.cap_per_sec.map(|v| format!("{:+.1}/s", -v)),
            CAP_W,
        ))
        .child(number(range, RANGE_W))
        .child(
            h_flex()
                .w(px(CHARGE_W))
                .flex_none()
                .gap_1()
                .when_some(m.charge, |cell, charge| {
                    cell.child(icon_box(catalog, &loaded.icons, Icon::Type(charge), 20.))
                        .child(div().truncate().child(catalog.name(charge)))
                }),
        )
    }

    pub(super) fn additions(
        &mut self,
        loaded: &Loaded,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(current) = self.saved.current else {
            return div().into_any_element();
        };
        let fit = &self.saved.fits[current];
        let theme = cx.theme();
        let tabs = [
            (AdditionsTab::Drones, "Drones", fit.drones.len()),
            (AdditionsTab::Implants, "Implants", fit.implants.len()),
            (AdditionsTab::Boosters, "Boosters", fit.boosters.len()),
            (AdditionsTab::Cargo, "Cargo", fit.cargo.len()),
            (AdditionsTab::Notes, "Notes", 0),
        ];
        let tab_bar = h_flex()
            .gap_1()
            .p_1()
            .bg(theme.tab_bar)
            .border_t_1()
            .border_b_1()
            .border_color(theme.border)
            .children(tabs.into_iter().map(|(tab, label, count)| {
                let selected = self.additions_tab == tab;
                let text = if count > 0 {
                    format!("{label} ({count})")
                } else {
                    label.to_owned()
                };
                div()
                    .id(label)
                    .px_3()
                    .py_0p5()
                    .rounded(px(4.))
                    .when(selected, |t| {
                        t.bg(theme.tab_active)
                            .text_color(theme.tab_active_foreground)
                    })
                    .when(!selected, |t| t.text_color(theme.tab_foreground))
                    .child(text)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.additions_tab = tab;
                        cx.notify();
                    }))
            }));

        let catalog = &loaded.catalog;
        let simple =
            |id: TypeId, label: String, n: usize, remove: Action, cx: &mut Context<Self>| {
                removable(
                    h_flex()
                        .id(SharedString::from(format!("addition-{n}")))
                        .gap_2()
                        .h(px(26.))
                        .px_2()
                        .hover(|s| s.bg(cx.theme().list_hover))
                        .child(icon_box(catalog, &loaded.icons, Icon::Type(id), 22.))
                        .child(label),
                    remove,
                    cx,
                )
            };
        let content: Vec<AnyElement> = match self.additions_tab {
            AdditionsTab::Drones if fit.drones.is_empty() => vec![
                div()
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child("Double-click drones in the market to add them.")
                    .into_any_element(),
            ],
            AdditionsTab::Drones => fit
                .drones
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let (active, count) = (d.active, d.count);
                    removable(
                        h_flex()
                            .id(SharedString::from(format!("drone-{i}")))
                            .gap_2()
                            .h(px(28.))
                            .px_2()
                            .hover(|s| s.bg(cx.theme().list_hover))
                            .child(
                                Button::new(SharedString::from(format!("drone-less-{i}")))
                                    .ghost()
                                    .xsmall()
                                    .label("−")
                                    .disabled(active == 0)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.apply(
                                            Action::SetDronesActive(i, active.saturating_sub(1)),
                                            cx,
                                        );
                                    })),
                            )
                            .child(div().w(px(36.)).child(format!("{active}/{count}")))
                            .child(
                                Button::new(SharedString::from(format!("drone-more-{i}")))
                                    .ghost()
                                    .xsmall()
                                    .label("+")
                                    .disabled(active >= count)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.apply(Action::SetDronesActive(i, active + 1), cx);
                                    })),
                            )
                            .child(icon_box(catalog, &loaded.icons, Icon::Type(d.type_id), 22.))
                            .child(catalog.name(d.type_id)),
                        Action::RemoveDrones(i),
                        cx,
                    )
                })
                .collect(),
            AdditionsTab::Implants => fit
                .implants
                .iter()
                .enumerate()
                .map(|(i, &id)| simple(id, catalog.name(id), i, Action::RemoveImplant(i), cx))
                .collect(),
            AdditionsTab::Boosters => fit
                .boosters
                .iter()
                .enumerate()
                .map(|(i, &id)| simple(id, catalog.name(id), i, Action::RemoveBooster(i), cx))
                .collect(),
            AdditionsTab::Cargo => {
                let used = fit.cargo_volume(catalog);
                let capacity = self
                    .stats
                    .as_ref()
                    .and_then(|s| s.cargo_capacity)
                    .unwrap_or(0.0);
                let summary = div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .when(used > capacity + 1e-6, |d| d.text_color(theme.danger))
                    .when(used <= capacity + 1e-6, |d| {
                        d.text_color(theme.muted_foreground)
                    })
                    .child(format!("{} / {} m³", short(used), short(capacity)))
                    .into_any_element();
                std::iter::once(summary)
                    .chain(fit.cargo.iter().enumerate().map(|(i, &(id, count))| {
                        simple(
                            id,
                            format!("{} x{count}", catalog.name(id)),
                            i,
                            Action::RemoveCargo(i),
                            cx,
                        )
                    }))
                    .collect()
            }
            AdditionsTab::Notes => vec![
                div()
                    .p_2()
                    .size_full()
                    .child(self.notes_field())
                    .into_any_element(),
            ],
        };

        v_flex()
            .size_full()
            .child(tab_bar)
            .child(
                v_flex()
                    .id("additions-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(content),
            )
            .into_any_element()
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
                let m = fit.rack(slot).get(i)?.clone();
                (m.type_id, Some(m))
            }
            RowRef::Rig(i) => (*fit.rigs.get(i)?, None),
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
    if t.market_group.is_some() {
        menu = menu.item(PopupMenuItem::new("Module Market Group").on_click(handler(
            this,
            move |g, _, window, cx| {
                g.jump_to_market(type_id, window, cx);
            },
        )));
    }
    menu = menu.item(PopupMenuItem::new("Remove Module").on_click(scoped(
        this,
        item,
        Scope::Selection,
        Action::Remove,
    )));
    let variations = catalog.variations_of(type_id);
    if variations.len() > 1 {
        menu = variations_submenu(menu, this, catalog, item, type_id, variations, window, cx);
    }
    let slot = match item {
        RowRef::Module(slot, _) => Some(slot),
        RowRef::Rig(_) => Some(Slot::Rig),
        RowRef::Subsystem(_) => None,
    };
    if let Some(slot) = slot {
        menu = menu.item(
            PopupMenuItem::new(format!("Fill With {}", t.name)).on_click(handler(
                this,
                move |g, _, _, cx| {
                    g.apply(Action::FillRack(slot, type_id), cx);
                },
            )),
        );
    }

    if let Some(charge) = snapshot.module.as_ref().and_then(|m| m.charge)
        && catalog
            .get(charge)
            .is_some_and(|c| c.market_group.is_some())
    {
        menu = menu
            .separator()
            .item(PopupMenuItem::new("Charge Market Group").on_click(handler(
                this,
                move |g, _, window, cx| {
                    g.jump_to_market(charge, window, cx);
                },
            )));
    }

    ship_entries(menu.separator(), this, ship)
}

fn ship_menu_builder(
    this: WeakEntity<Gyfa>,
    ship: TypeId,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, _| ship_entries(menu, &this, ship)
}

/// pyfa's fittingShip menu.
fn ship_entries(menu: PopupMenu, this: &WeakEntity<Gyfa>, ship: TypeId) -> PopupMenu {
    menu.item(
        PopupMenuItem::new("Open in Fitting Browser").on_click(handler(
            this,
            move |g, _, _, cx| {
                g.jump_to_fittings(ship, cx);
            },
        )),
    )
}

/// A charge entry: loads it into every similar module, or only the selection with Ctrl or Alt.
fn charge_entry(
    this: &WeakEntity<Gyfa>,
    catalog: &Catalog,
    item: RowRef,
    id: TypeId,
    loaded_charge: Option<TypeId>,
) -> PopupMenuItem {
    PopupMenuItem::new(catalog.name(id))
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
    menu.submenu(title, window, cx, move |menu, _, _| {
        charges
            .iter()
            .fold(menu.label("─ Less Damage ─"), |menu, &c| {
                menu.item(charge_entry(&this, &loaded.catalog, item, c, loaded_charge))
            })
            .label("─ More Damage ─")
    })
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
    menu.submenu("Charge", window, cx, move |menu, window, cx| {
        let catalog = &loaded.catalog;
        let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
        match &structure {
            ChargeMenu::Turret(families) => {
                menu = menu.label("─ Long Range ─");
                for family in families {
                    menu = match family.as_slice() {
                        [only] => {
                            menu.item(charge_entry(&this, catalog, item, *only, loaded_charge))
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
                    menu = menu.item(charge_entry(&this, catalog, item, c, loaded_charge));
                }
            }
        }
        menu.item(PopupMenuItem::new("Empty").on_click(scoped(
            &this,
            item,
            Scope::Similar,
            |rows| Action::SetCharge(rows, None),
        )))
    })
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
    menu.submenu("Spoolup Cycles", window, cx, move |menu, _, _| {
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
            .item(PopupMenuItem::new("Reset").on_click(scoped(
                &this,
                item,
                Scope::Selection,
                |rows| Action::SetSpool(rows, None),
            )))
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn variations_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    catalog: &Catalog,
    item: RowRef,
    current: TypeId,
    variations: &[TypeId],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
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
    let this = this.clone();
    menu.submenu("Variations", window, cx, move |menu, _, _| {
        let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
        for (group, members) in &groups {
            menu = menu.label(group.clone());
            for (id, name) in members {
                let id = *id;
                menu = menu.item(
                    PopupMenuItem::new(name.clone())
                        .checked(id == current)
                        .on_click(scoped(&this, item, Scope::Selection, move |rows| {
                            Action::SwapType(rows, id)
                        })),
                );
            }
        }
        menu
    })
}

/// A module's charges, structured the way pyfa's Charge menu shows them.
enum ChargeMenu {
    /// Families of the same charge in different grades, from long range to short.
    Turret(Vec<Vec<TypeId>>),
    /// By damage type, each from less damage to more.
    Missile(Vec<(&'static str, Vec<TypeId>)>),
    Flat(Vec<TypeId>),
}

fn structure_charges(catalog: &Catalog, module: &ItemType, charges: &[TypeId]) -> ChargeMenu {
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

/// Double-click or right-click > Remove on a row runs `remove`.
fn removable(row: Stateful<Div>, remove: Action, cx: &mut Context<Gyfa>) -> AnyElement {
    let this = Gyfa::weak(cx);
    let on_double = remove.clone();
    row.on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
        if event.click_count() >= 2 {
            g.apply(on_double.clone(), cx);
        }
    }))
    .context_menu(move |menu, _, _| {
        let (this, remove) = (this.clone(), remove.clone());
        menu.item(PopupMenuItem::new("Remove").on_click(move |_, _, cx| {
            Gyfa::app_update(&this, cx, |g, cx| g.apply(remove.clone(), cx));
        }))
    })
    .into_any_element()
}

/// The fitting view's rows: each rack under its header, padded with empty slots to what the ship
/// has.
fn fitting_rows(fit: &Fit, stats: &Stats) -> Vec<Row> {
    let racks = [
        (
            Slot::High,
            "High Slots",
            icon::HIGH,
            stats.high,
            fit.high.len(),
        ),
        (Slot::Mid, "Med Slots", icon::MID, stats.mid, fit.mid.len()),
        (Slot::Low, "Low Slots", icon::LOW, stats.low, fit.low.len()),
        (Slot::Rig, "Rig Slots", icon::RIG, stats.rig, fit.rigs.len()),
        (
            Slot::Subsystem,
            "Subsystem Slots",
            icon::SUBSYSTEM,
            stats.subsystem,
            fit.subsystems.len(),
        ),
    ];
    let mut rows = Vec::new();
    for (slot, label, icon_id, slots, used) in racks {
        let total = slots.total.unwrap_or(0);
        if total == 0 && used == 0 {
            continue;
        }
        rows.push(Row::Header {
            label,
            icon_id,
            total,
        });
        rows.extend((0..used).map(|i| {
            Row::Item(match slot {
                Slot::Rig => RowRef::Rig(i),
                Slot::Subsystem => RowRef::Subsystem(i),
                Slot::High | Slot::Mid | Slot::Low => RowRef::Module(slot, i),
            })
        }));
        rows.extend((used..total as usize).map(|_| Row::Empty(slot)));
    }
    rows
}

/// Published charges that fit a module type, by name.
fn compatible_charges(catalog: &Catalog, module: TypeId) -> Vec<TypeId> {
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
