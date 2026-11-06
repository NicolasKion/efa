//! The middle: the fit's slots as a table, and the additions pane under it.

use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Action, AdditionsTab, Gyfa, Loaded,
    widgets::{color, icon, icon_box, km, short},
};
use crate::{
    data::{Catalog, Slot, TypeId},
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

#[derive(Debug, Clone, Copy)]
enum Row {
    Header {
        label: &'static str,
        icon_id: i32,
        total: u32,
    },
    Module(Slot, usize),
    Empty(Slot),
    Rig(usize),
    Subsystem(usize),
}

impl Gyfa {
    pub(super) fn fitting_view(
        &mut self,
        loaded: &Loaded,
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
        for m in self.saved.fits[current].modules() {
            self.charges_for
                .entry(m.type_id)
                .or_insert_with(|| compatible_charges(&loaded.catalog, m.type_id));
        }
        let fit = &self.saved.fits[current];
        let no_stats = Stats::default();
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let rows = fitting_rows(fit, stats);

        let header = h_flex()
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
            .child(div().w(px(260.)).child(self.name_field()));

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
            .map(|(n, &row)| self.fitting_row(loaded, fit, stats, row, n, cx))
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

    fn fitting_row(
        &self,
        loaded: &Loaded,
        fit: &Fit,
        stats: &Stats,
        row: Row,
        n: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let base = h_flex()
            .id(SharedString::from(format!("fit-row-{n}")))
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_1()
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
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
                .into_any_element(),
            Row::Empty(slot) => base
                .child(div().w(px(STATE_W + ICON_W)).flex_none())
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("[Empty {} slot]", slot_name(slot))),
                )
                .into_any_element(),
            Row::Rig(i) | Row::Subsystem(i) => {
                let (id, remove) = match row {
                    Row::Rig(_) => (fit.rigs[i], Action::RemoveRig(i)),
                    _ => (fit.subsystems[i], Action::RemoveSubsystem(i)),
                };
                removable(
                    base.child(div().w(px(STATE_W)).flex_none())
                        .child(icon_box(catalog, &loaded.icons, Icon::Type(id), 22.))
                        .child(div().flex_1().truncate().child(catalog.name(id))),
                    remove,
                    cx,
                )
            }
            Row::Module(slot, i) => {
                let m = &fit.rack(slot)[i];
                let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                let charges = self
                    .charges_for
                    .get(&m.type_id)
                    .cloned()
                    .unwrap_or_default();
                self.module_row(base, loaded, (slot, i), m, info, &charges, cx)
            }
        }
    }

    #[expect(clippy::too_many_arguments, reason = "one row needs all of these")]
    fn module_row(
        &self,
        base: gpui_kit::Stateful<Div>,
        loaded: &Loaded,
        (slot, i): (Slot, usize),
        m: &Module,
        info: ModuleInfo,
        charges: &[TypeId],
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
        let names: Vec<(TypeId, String)> = charges.iter().map(|&c| (c, catalog.name(c))).collect();
        let current_state = m.state;
        let has_charge = m.charge.is_some();

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
                .child(catalog.name(m.type_id)),
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
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            if event.click_count() >= 2 {
                this.apply(Action::RemoveModule(slot, i), cx);
            }
        }))
        .context_menu({
            let this = Self::weak(cx);
            move |menu, window, cx| {
                module_menu(
                    menu,
                    &this,
                    (slot, i),
                    current_state,
                    has_charge,
                    &names,
                    window,
                    cx,
                )
            }
        })
        .into_any_element()
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
            AdditionsTab::Cargo => fit
                .cargo
                .iter()
                .enumerate()
                .map(|(i, &(id, count))| {
                    simple(
                        id,
                        format!("{} x{count}", catalog.name(id)),
                        i,
                        Action::RemoveCargo(i),
                        cx,
                    )
                })
                .collect(),
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

#[expect(
    clippy::too_many_arguments,
    reason = "the menu needs the row's whole state"
)]
fn module_menu(
    menu: PopupMenu,
    this: &gpui_kit::WeakEntity<Gyfa>,
    (slot, i): (Slot, usize),
    state: ModuleState,
    has_charge: bool,
    charges: &[(TypeId, String)],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let action = |a: Action| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| g.apply(a, cx));
        }
    };
    let menu = menu.submenu("State", window, cx, {
        let this = this.clone();
        move |menu, _, _| {
            [
                ModuleState::Offline,
                ModuleState::Online,
                ModuleState::Active,
                ModuleState::Overload,
            ]
            .into_iter()
            .fold(menu, |menu, s| {
                let this = this.clone();
                menu.item(PopupMenuItem::new(s.label()).checked(s == state).on_click(
                    move |_, _, cx| {
                        Gyfa::app_update(&this, cx, |g, cx| {
                            g.apply(Action::SetState(slot, i, s), cx);
                        });
                    },
                ))
            })
        }
    });
    let menu = if charges.is_empty() {
        menu
    } else {
        let charges = charges.to_vec();
        let this = this.clone();
        menu.submenu("Charge", window, cx, move |menu, _, _| {
            charges
                .iter()
                .fold(menu.scrollable(true).max_h(px(420.)), |menu, (id, name)| {
                    let (this, id) = (this.clone(), *id);
                    menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                        Gyfa::app_update(&this, cx, |g, cx| {
                            g.apply(Action::SetCharge(slot, i, Some(id)), cx);
                        });
                    }))
                })
        })
    };
    menu.when(has_charge, |menu| {
        menu.item(
            PopupMenuItem::new("Unload charge").on_click(action(Action::SetCharge(slot, i, None))),
        )
    })
    .separator()
    .item(PopupMenuItem::new("Remove").on_click(action(Action::RemoveModule(slot, i))))
}

/// Double-click or right-click > Remove on a row runs `remove`.
fn removable(row: gpui_kit::Stateful<Div>, remove: Action, cx: &mut Context<Gyfa>) -> AnyElement {
    let this = Gyfa::weak(cx);
    row.on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
        if event.click_count() >= 2 {
            g.apply(remove, cx);
        }
    }))
    .context_menu(move |menu, _, _| {
        let this = this.clone();
        menu.item(PopupMenuItem::new("Remove").on_click(move |_, _, cx| {
            Gyfa::app_update(&this, cx, |g, cx| g.apply(remove, cx));
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
        rows.extend((0..used).map(|i| match slot {
            Slot::Rig => Row::Rig(i),
            Slot::Subsystem => Row::Subsystem(i),
            Slot::High | Slot::Mid | Slot::Low => Row::Module(slot, i),
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
