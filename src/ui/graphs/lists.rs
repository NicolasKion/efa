//! The fit list and the target list under the plot, with pyfa's columns and context menus.

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity,
    Window, div, prelude::FluentBuilder, px,
};

use super::{
    GraphsWindow, canvas,
    style::{self, Lightness, LineColor, LineStyle},
};
use crate::data::{Hardpoint, TypeId};
use crate::graphs::{
    ammo::{self, AmmoQuality},
    defs::Column,
    defs::Graph,
    plot::ResistMode,
    profiles::TargetProfile,
    snapshot::Ship,
    snapshot::{EwarKind, FitSnapshot},
};
use crate::icons::Icon;
use crate::ui::{
    profile_editor::open_target_profile_editor,
    profile_menus::{FirstProfile, PickProfile, target_profile_menu},
    widgets::icon_box,
};

/// A weapon type with its name and the charges it takes, each with its name.
type WeaponCharges = (TypeId, String, Vec<(TypeId, String)>);

/// A fit plotted, in its color.
#[derive(Debug, Clone)]
pub struct Source {
    pub fit: usize,
    pub color: LineColor,
    /// A charge loaded into every module of a type, from "Plot with Different Ammo".
    pub ammo: Option<AmmoChoice>,
}

/// A charge to plot a fit's weapons of one type with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AmmoChoice {
    pub module: TypeId,
    pub charge: TypeId,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TargetKind {
    Fit(usize),
    Profile(TargetProfile),
}

/// Something the fits are plotted against, with the shade and dash its lines get.
#[derive(Debug, Clone)]
pub struct Target {
    pub kind: TargetKind,
    pub lightness: Lightness,
    pub style: LineStyle,
    pub resist_mode: ResistMode,
}

impl Target {
    /// A new target takes the least used shade and dash, independently, as in pyfa.
    pub fn new(kind: TargetKind, existing: &[Target]) -> Self {
        let shades: Vec<Lightness> = existing.iter().map(|t| t.lightness).collect();
        let dashes: Vec<LineStyle> = existing.iter().map(|t| t.style).collect();
        Self {
            kind,
            lightness: style::least_used(&Lightness::ALL, &shades),
            style: style::least_used(&LineStyle::ALL, &dashes),
            resist_mode: ResistMode::Auto,
        }
    }
}

const ROW_H: f32 = 24.;
/// The line sample's column, and the ship icon's.
const SWATCH_W: f32 = 36.;
const ICON: f32 = 20.;
/// A value column.
const COL_W: f32 = 80.;

fn is_delete(event: &KeyDownEvent) -> bool {
    let key = &event.keystroke;
    !key.modifiers.modified() && matches!(key.key.as_str(), "delete" | "backspace")
}

type Weak = WeakEntity<GraphsWindow>;

fn with(this: &Weak, cx: &mut App, f: impl FnOnce(&mut GraphsWindow, &mut Context<GraphsWindow>)) {
    let _ = this.update(cx, |g, cx| {
        f(g, cx);
        g.recompute(cx);
        cx.notify();
    });
}

/// A column value for a fit row.
fn ship_column(col: Column, ship: &Ship) -> String {
    let f = canvas::format;
    match col {
        Column::Dps => f(ship.dps),
        Column::Volley => f(ship.volley),
        Column::Speed => format!("{} m/s", f(ship.max_velocity)),
        Column::Radius => format!("{} m", f(ship.radius)),
        Column::ShieldRr => format!("{} HP/s", f(ship.rr[0])),
        Column::ArmorRr => format!("{} HP/s", f(ship.rr[1])),
        Column::HullRr => format!("{} HP/s", f(ship.rr[2])),
        Column::CapAmount => format!("{} GJ", f(ship.cap_capacity)),
        Column::CapTime => match (ship.cap_stable, ship.cap_lasts) {
            (Some(level), _) => format!("Stable {:.1}%", level * 100.0),
            (None, Some(secs)) => format!("{} s", f(secs)),
            _ => String::new(),
        },
        Column::Agility => format!("{} s", f(ship.align_time)),
        Column::WarpSpeed => format!("{} AU/s", f(ship.warp_speed)),
        Column::WarpDistance => format!("{} AU", f(ship.max_warp_distance)),
        Column::ScanResolution => format!("{} mm", f(ship.scan_res)),
        Column::ShieldAmount => format!("{} HP", f(ship.shield_capacity)),
        Column::ShieldTime => format!("{} s", f(ship.shield_recharge)),
        Column::SigRadius => format!("{} m", f(ship.sig_radius)),
        Column::FullHp => f(ship.hp.iter().sum()),
        Column::TargetResists | Column::DampScanRes => String::new(),
    }
}

/// How much the fit's dampeners cut scan resolution, pyfa's "Damp `ScanRes`" column.
fn damp_scan_res(snap: &FitSnapshot) -> String {
    let mults: Vec<f64> = snap
        .ewar
        .iter()
        .filter(|e| e.kind == EwarKind::Damp)
        .map(|e| 1.0 + e.strength2 / 100.0)
        .collect();
    format!(
        "{:.0}%",
        (crate::graphs::math::stacked_multiplier(&mults) - 1.0) * 100.0
    )
}

fn resists_text(resists: &[f64; 4]) -> String {
    resists
        .iter()
        .map(|r| format!("{:.0}", r * 100.0))
        .collect::<Vec<_>>()
        .join("/")
}

/// The column names over a list's rows.
fn header(cols: &[Column], first: &'static str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .flex_none()
        .h(px(22.))
        .px_2()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(div().flex_none().w(px(SWATCH_W + ICON)).mr_2())
        .child(div().flex_1().child(first))
        .children(cols.iter().map(|c| value_cell().child(c.header())))
}

/// A right-aligned value column.
fn value_cell() -> Div {
    h_flex()
        .flex_none()
        .w(px(COL_W))
        .justify_end()
        .whitespace_nowrap()
        .overflow_hidden()
}

/// A list card's title bar: its icon, name and count, then its buttons.
fn title_bar(
    icon: AssetIcon,
    title: &'static str,
    count: usize,
    actions: [AnyElement; 2],
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .flex_none()
        .gap_2()
        .pl_2()
        .pr_1()
        .py_0p5()
        .border_b_1()
        .border_color(theme.border)
        .child(
            UiIcon::new(icon)
                .size(px(14.))
                .text_color(theme.muted_foreground),
        )
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(
            div()
                .px_1p5()
                .rounded_full()
                .bg(theme.secondary)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(count.to_string()),
        )
        .child(div().flex_1())
        .children(actions)
}

/// What an empty list says, in place of its rows.
fn empty_state(icon: AssetIcon, title: &'static str, hint: &'static str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .items_center()
        .justify_center()
        .gap_0p5()
        .py_2()
        .child(
            UiIcon::new(icon)
                .size(px(16.))
                .text_color(theme.muted_foreground),
        )
        .child(div().font_weight(FontWeight::MEDIUM).child(title))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint),
        )
}

/// A list's card: a hairline border on the sidebar color, like the settings cards.
fn card(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .rounded(px(4.))
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
}

/// A ship's icon, or `fallback` drawn in its place for what has no ship.
fn ship_icon(g: &GraphsWindow, fit: Option<usize>, fallback: AssetIcon, cx: &App) -> AnyElement {
    let ship = fit.and_then(|i| g.fits.get(i)).and_then(|f| f.ship);
    match (ship, &g.loaded) {
        (Some(ship), Some(loaded)) => {
            icon_box(&loaded.catalog, &loaded.icons, Icon::Type(ship), ICON)
                .rounded(px(3.))
                .overflow_hidden()
                .into_any_element()
        }
        _ => h_flex()
            .flex_none()
            .size(px(ICON))
            .justify_center()
            .child(
                UiIcon::new(fallback)
                    .size(px(14.))
                    .text_color(cx.theme().muted_foreground),
            )
            .into_any_element(),
    }
}

/// A list row's frame: hover and selection like the fittings browser's rows.
fn row_frame(id: SharedString, selected: bool, cx: &App) -> gpui_kit::Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .flex_none()
        .h(px(ROW_H))
        .mx_1()
        .px_2()
        .gap_2()
        .text_xs()
        .when(selected, |r| r.bg(theme.list_active))
        .when(!selected, |r| r.hover(|s| s.bg(theme.list_hover)))
}

pub fn sources(g: &mut GraphsWindow, cx: &mut Context<GraphsWindow>) -> AnyElement {
    let this = cx.entity().downgrade();
    let cols = g.graph.src_columns();
    let (fits, tabs) = fit_choices(g);
    let rows: Vec<AnyElement> = (0..g.sources.len())
        .map(|i| source_row(g, i, cols, &this, &fits, &tabs, cx))
        .collect();
    let empty = rows.is_empty();
    let add = {
        let (this, fits, tabs) = (this.clone(), Rc::clone(&fits), Rc::clone(&tabs));
        Button::new("graph-add-fit")
            .ghost()
            .xsmall()
            .icon(AssetIcon::Plus)
            .label("Add")
            .dropdown_menu(move |menu, window, cx| {
                source_menu(menu, &this, &fits, &tabs, None, window, cx)
            })
            .into_any_element()
    };
    let remove = Button::new("graph-remove-fit")
        .ghost()
        .xsmall()
        .icon(AssetIcon::Trash)
        .tooltip("Remove the selected fit (Delete)")
        .disabled(g.selected_source.is_none_or(|i| i >= g.sources.len()))
        .on_click(cx.listener(|g, _: &ClickEvent, _, cx| {
            if let Some(i) = g.selected_source.take()
                && i < g.sources.len()
            {
                g.sources.remove(i);
                g.recompute(cx);
                cx.notify();
            }
        }))
        .into_any_element();
    let body = v_flex()
        .id("graph-sources")
        .track_focus(&g.sources_focus)
        .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| {
            if is_delete(event)
                && let Some(i) = g.selected_source.take()
                && i < g.sources.len()
            {
                g.sources.remove(i);
                g.recompute(cx);
                cx.notify();
            }
        }))
        .flex_1()
        .min_h_0()
        .py_1()
        .overflow_y_scroll()
        .children(rows)
        .when(empty, |list| {
            list.child(empty_state(
                AssetIcon::Rocket,
                "No fits yet",
                "Add a fit with the Add button above, or right-click here.",
                cx,
            ))
        })
        .child(div().flex_1().min_h(px(ROW_H)).id("graph-sources-rest"))
        .context_menu(move |menu, window, cx| {
            source_menu(menu, &this, &fits, &tabs, None, window, cx)
        });
    card(cx)
        .child(title_bar(
            AssetIcon::Rocket,
            "Fits",
            g.sources.len(),
            [add, remove],
            cx,
        ))
        .child(header(cols, "Fit", cx))
        .child(body)
        .into_any_element()
}

/// Fits by index and name, for the add menus.
type Choices = Rc<Vec<(usize, String)>>;

/// Every fit by name, and the main window's open tabs, for the add menus.
fn fit_choices(g: &GraphsWindow) -> (Choices, Choices) {
    let mut fits: Vec<(usize, String)> = (0..g.fits.len()).map(|i| (i, g.fit_name(i))).collect();
    fits.sort_by_cached_key(|f| f.1.to_lowercase());
    let tabs = g
        .open_tabs
        .iter()
        .filter(|&&i| i < g.fits.len())
        .map(|&i| (i, g.fit_name(i)))
        .collect();
    (Rc::new(fits), Rc::new(tabs))
}

fn add_fit_submenus(
    menu: PopupMenu,
    this: &Weak,
    fits: &Choices,
    tabs: &Choices,
    target: bool,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let add = move |this: Weak, fit: usize| {
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            with(&this, cx, |g, _| {
                if target {
                    g.add_target(TargetKind::Fit(fit));
                } else {
                    g.add_source(fit, None);
                }
            });
        }
    };
    let (this_a, fits) = (this.clone(), Rc::clone(fits));
    let menu = menu.submenu("Add Fit...", window, cx, move |menu, _, _| {
        fits.iter().fold(menu, |menu, (i, name)| {
            menu.item(PopupMenuItem::new(name.clone()).on_click(add(this_a.clone(), *i)))
        })
    });
    let (this_b, tabs) = (this.clone(), Rc::clone(tabs));
    menu.submenu("Add Currently Open Fit", window, cx, move |menu, _, _| {
        tabs.iter().fold(menu, |menu, (i, name)| {
            menu.item(PopupMenuItem::new(name.clone()).on_click(add(this_b.clone(), *i)))
        })
    })
}

fn source_menu(
    menu: PopupMenu,
    this: &Weak,
    fits: &Choices,
    tabs: &Choices,
    selected: Option<usize>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = add_fit_submenus(menu, this, fits, tabs, false, window, cx);
    if let Some(row) = selected {
        menu = ammo_menu(menu, this, row, window, cx);
        let (remove, open) = (this.clone(), this.clone());
        menu = menu
            .separator()
            .item(PopupMenuItem::new("Remove Fit").on_click(move |_, _, cx| {
                with(&remove, cx, |g, _| {
                    if row < g.sources.len() {
                        g.sources.remove(row);
                    }
                    g.selected_source = None;
                });
            }))
            .item(
                PopupMenuItem::new("Open Fit in New Tab").on_click(move |_, _, cx| {
                    let Some(g) = open.upgrade() else { return };
                    let (hub, fit) = {
                        let g = g.read(cx);
                        (g.hub.clone(), g.sources.get(row).map(|s| s.fit))
                    };
                    if let Some(fit) = fit {
                        GraphsWindow::open_in_main(&hub, fit, cx);
                    }
                }),
            );
    }
    menu
}

/// pyfa's "Plot with Different Ammo": the damage graph plots the fit again with another charge
/// in one type of weapon. pyfa's own picker dialog is switched off in its code; this offers the
/// same choice as a menu.
fn ammo_menu(
    menu: PopupMenu,
    this: &Weak,
    row: usize,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(g) = this.upgrade() else { return menu };
    let (groups, fit) = {
        let g = g.read(cx);
        if g.graph != Graph::DamageStats {
            return menu;
        }
        let (Some(source), Some(loaded)) = (g.sources.get(row), g.loaded.clone()) else {
            return menu;
        };
        let Some(fit) = g.fits.get(source.fit) else {
            return menu;
        };
        let catalog = &loaded.catalog;
        let mut types: Vec<TypeId> = Vec::new();
        for m in fit.modules() {
            let weapon = catalog
                .get(m.type_id)
                .is_some_and(|t| t.hardpoint != Hardpoint::None && !t.mining);
            if weapon && !types.contains(&m.type_id) {
                types.push(m.type_id);
            }
        }
        let groups: Vec<WeaponCharges> = types
            .into_iter()
            .map(|t| {
                let charges = ammo::charges_for(catalog, t, AmmoQuality::All)
                    .into_iter()
                    .map(|c| (c, catalog.name(c)))
                    .collect();
                (t, catalog.name(t), charges)
            })
            .filter(|(_, _, charges): &(_, _, Vec<_>)| !charges.is_empty())
            .collect();
        (groups, source.fit)
    };
    if groups.is_empty() {
        return menu;
    }
    let this = this.clone();
    menu.separator().submenu(
        "Plot with Different Ammo",
        window,
        cx,
        move |menu, window, cx| {
            if let [(module, _, charges)] = groups.as_slice() {
                return charges.iter().fold(menu, |menu, (charge, name)| {
                    menu.item(
                        PopupMenuItem::new(name.clone())
                            .on_click(pick_ammo(&this, fit, *module, *charge)),
                    )
                });
            }
            groups
                .iter()
                .fold(menu, |menu, (module, module_name, charges)| {
                    let (module, charges, this) = (*module, charges.clone(), this.clone());
                    menu.submenu(module_name.clone(), window, cx, move |menu, _, _| {
                        charges.iter().fold(menu, |menu, (charge, name)| {
                            menu.item(
                                PopupMenuItem::new(name.clone())
                                    .on_click(pick_ammo(&this, fit, module, *charge)),
                            )
                        })
                    })
                })
        },
    )
}

fn pick_ammo(
    this: &Weak,
    fit: usize,
    module: TypeId,
    charge: TypeId,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let this = this.clone();
    move |_, _, cx| {
        with(&this, cx, |g, _| {
            g.add_source(fit, Some(AmmoChoice { module, charge }));
        });
    }
}

fn source_row(
    g: &mut GraphsWindow,
    i: usize,
    cols: &[Column],
    this: &Weak,
    fits: &Choices,
    tabs: &Choices,
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let source = g.sources[i].clone();
    let snap = g.snapshot(source.fit);
    let name = g.source_name(&source);
    let color = source.color.color(cx.theme());
    let selected = g.selected_source == Some(i);
    let picker = {
        let this = this.clone();
        Button::new(SharedString::from(format!("src-color-{i}")))
            .ghost()
            .xsmall()
            .w(px(SWATCH_W))
            .tooltip("Line color")
            .child(canvas::swatch(color, LineStyle::Solid))
            .dropdown_menu(move |menu, _, _| {
                LineColor::ALL.iter().fold(menu, |menu, &c| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(c.label())
                            .checked(c == source.color)
                            .on_click(move |_, _, cx| {
                                with(&this, cx, |g, _| {
                                    if let Some(s) = g.sources.get_mut(i) {
                                        s.color = c;
                                    }
                                });
                            }),
                    )
                })
            })
    };
    let icon = ship_icon(g, Some(source.fit), AssetIcon::Rocket, cx);
    row_frame(SharedString::from(format!("src-row-{i}")), selected, cx)
        .child(picker)
        .child(icon)
        .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
        .children(cols.iter().map(|&c| {
            value_cell().child(snap.as_ref().map_or_else(String::new, |s| match c {
                Column::DampScanRes => damp_scan_res(s),
                _ => ship_column(c, &s.ship),
            }))
        }))
        .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
            if event.click_count() >= 2 {
                // pyfa removes a row on double-click.
                if i < g.sources.len() {
                    g.sources.remove(i);
                }
                g.selected_source = None;
                g.recompute(cx);
            } else {
                g.selected_source = Some(i);
                window.focus(&g.sources_focus, cx);
            }
            cx.notify();
        }))
        .context_menu({
            let (this, fits, tabs) = (this.clone(), Rc::clone(fits), Rc::clone(tabs));
            move |menu, window, cx| source_menu(menu, &this, &fits, &tabs, Some(i), window, cx)
        })
        .into_any_element()
}

pub fn targets(g: &mut GraphsWindow, cx: &mut Context<GraphsWindow>) -> AnyElement {
    let this = cx.entity().downgrade();
    let cols = g.graph.tgt_columns(g.options().ignore_resists);
    let (fits, tabs) = fit_choices(g);
    let ignore_resists = g.options().ignore_resists;
    let rows: Vec<AnyElement> = (0..g.targets.len())
        .map(|i| target_row(g, i, &cols, &this, &fits, &tabs, ignore_resists, cx))
        .collect();
    let empty = rows.is_empty();
    let add = {
        let (this, fits, tabs) = (this.clone(), Rc::clone(&fits), Rc::clone(&tabs));
        Button::new("graph-add-target")
            .ghost()
            .xsmall()
            .icon(AssetIcon::Plus)
            .label("Add")
            .dropdown_menu(move |menu, window, cx| {
                target_menu(menu, &this, &fits, &tabs, None, window, cx)
            })
            .into_any_element()
    };
    let remove = Button::new("graph-remove-target")
        .ghost()
        .xsmall()
        .icon(AssetIcon::Trash)
        .tooltip("Remove the selected target (Delete)")
        .disabled(g.selected_target.is_none_or(|i| i >= g.targets.len()))
        .on_click(cx.listener(|g, _: &ClickEvent, _, cx| {
            if let Some(i) = g.selected_target {
                g.remove_target(i);
                g.recompute(cx);
                cx.notify();
            }
        }))
        .into_any_element();
    let body = v_flex()
        .id("graph-targets")
        .track_focus(&g.targets_focus)
        .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| {
            if is_delete(event)
                && let Some(i) = g.selected_target
            {
                g.remove_target(i);
                g.recompute(cx);
                cx.notify();
            }
        }))
        .flex_1()
        .min_h_0()
        .py_1()
        .overflow_y_scroll()
        .children(rows)
        .when(empty, |list| {
            list.child(empty_state(
                AssetIcon::Crosshair,
                "No targets",
                "Add a fit or a target profile with the Add button above.",
                cx,
            ))
        })
        .child(div().flex_1().min_h(px(ROW_H)).id("graph-targets-rest"))
        .context_menu(move |menu, window, cx| {
            target_menu(menu, &this, &fits, &tabs, None, window, cx)
        });
    card(cx)
        .child(title_bar(
            AssetIcon::Crosshair,
            "Targets",
            g.targets.len(),
            [add, remove],
            cx,
        ))
        .child(header(&cols, "Target", cx))
        .child(body)
        .into_any_element()
}

/// The row a menu was opened on: its index, whether it is a fit, and the resist mode to offer
/// (fits only, while resists count).
#[derive(Debug, Clone)]
struct TargetRow {
    index: usize,
    fit: bool,
    resist_mode: Option<ResistMode>,
    /// A user profile's name, for "Edit Target Profile".
    user_profile: Option<SharedString>,
}

fn target_menu(
    menu: PopupMenu,
    this: &Weak,
    fits: &Choices,
    tabs: &Choices,
    row: Option<TargetRow>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = add_fit_submenus(menu, this, fits, tabs, true, window, cx);
    let profiles = this.clone();
    let user: Rc<Vec<crate::target_profile::TargetProfile>> =
        Rc::new(this.upgrade().map_or_else(Vec::new, |g| {
            g.read(cx).hub.read(cx).saved.target_profiles.clone()
        }));
    menu = menu.submenu("Add Target Profile", window, cx, move |menu, window, cx| {
        let this = profiles.clone();
        let pick: PickProfile = Rc::new(move |profile, _, cx| {
            let profile = profile.unwrap_or_else(crate::target_profile::TargetProfile::ideal);
            with(&this, cx, |g, _| {
                g.add_target(TargetKind::Profile(TargetProfile::from(&profile)));
            });
        });
        target_profile_menu(menu, &user, None, FirstProfile::Ideal, &pick, window, cx)
    });
    if let Some(TargetRow {
        index: row,
        fit: is_fit,
        resist_mode,
        user_profile,
    }) = row
    {
        if let Some(name) = user_profile {
            let this = this.clone();
            menu = menu
                .separator()
                .item(
                    PopupMenuItem::new("Edit Target Profile").on_click(move |_, _, cx| {
                        let Some(g) = this.upgrade() else { return };
                        let hub = g.read(cx).hub.clone();
                        open_target_profile_editor(hub, Some(name.to_string()), cx);
                    }),
                );
        }
        if let Some(mode) = resist_mode {
            let this = this.clone();
            menu = menu
                .separator()
                .submenu("Resist Mode", window, cx, move |menu, _, _| {
                    ResistMode::ALL.iter().fold(menu, |menu, &m| {
                        let this = this.clone();
                        menu.item(PopupMenuItem::new(m.label()).checked(m == mode).on_click(
                            move |_, _, cx| {
                                with(&this, cx, |g, _| {
                                    if let Some(t) = g.targets.get_mut(row) {
                                        t.resist_mode = m;
                                    }
                                });
                            },
                        ))
                    })
                });
        }
        let (remove, open) = (this.clone(), this.clone());
        menu =
            menu.separator().item(
                PopupMenuItem::new("Remove Target").on_click(move |_, _, cx| {
                    with(&remove, cx, |g, _| g.remove_target(row));
                }),
            );
        if is_fit {
            menu = menu.item(PopupMenuItem::new("Open Fit in New Tab").on_click(
                move |_, _, cx| {
                    let Some(g) = open.upgrade() else { return };
                    let (hub, fit) = {
                        let g = g.read(cx);
                        let fit = match g.targets.get(row).map(|t| &t.kind) {
                            Some(TargetKind::Fit(fit)) => Some(*fit),
                            _ => None,
                        };
                        (g.hub.clone(), fit)
                    };
                    if let Some(fit) = fit {
                        GraphsWindow::open_in_main(&hub, fit, cx);
                    }
                },
            ));
        }
    }
    menu
}

#[expect(
    clippy::too_many_arguments,
    reason = "a row with its menus' inputs spelled out"
)]
fn target_row(
    g: &mut GraphsWindow,
    i: usize,
    cols: &[Column],
    this: &Weak,
    fits: &Choices,
    tabs: &Choices,
    ignore_resists: bool,
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let target = g.targets[i].clone();
    let data = g.target_data(&target);
    let snap = match target.kind {
        TargetKind::Fit(fit) => g.snapshot(fit),
        TargetKind::Profile(_) => None,
    };
    // A target's swatch shows its shade of the first fit's color.
    let base = g
        .sources
        .first()
        .map_or(LineColor::Blue, |s| s.color)
        .color(cx.theme());
    let color = target.lightness.apply(base);
    let selected = g.selected_target == Some(i);
    let picker = {
        let this = this.clone();
        Button::new(SharedString::from(format!("tgt-style-{i}")))
            .ghost()
            .xsmall()
            .w(px(SWATCH_W))
            .tooltip("Line lightness and style")
            .child(canvas::swatch(color, target.style))
            .dropdown_menu(move |menu, window, cx| {
                let (shade_this, dash_this) = (this.clone(), this.clone());
                let (lightness, style) = (target.lightness, target.style);
                menu.submenu("Lightness", window, cx, move |menu, _, _| {
                    Lightness::ALL.iter().fold(menu, |menu, &l| {
                        let this = shade_this.clone();
                        menu.item(
                            PopupMenuItem::new(l.label())
                                .checked(l == lightness)
                                .on_click(move |_, _, cx| {
                                    with(&this, cx, |g, _| {
                                        if let Some(t) = g.targets.get_mut(i) {
                                            t.lightness = l;
                                        }
                                    });
                                }),
                        )
                    })
                })
                .submenu("Line Style", window, cx, move |menu, _, _| {
                    LineStyle::ALL.iter().fold(menu, |menu, &s| {
                        let this = dash_this.clone();
                        menu.item(PopupMenuItem::new(s.label()).checked(s == style).on_click(
                            move |_, _, cx| {
                                with(&this, cx, |g, _| {
                                    if let Some(t) = g.targets.get_mut(i) {
                                        t.style = s;
                                    }
                                });
                            },
                        ))
                    })
                })
            })
    };
    let value = |c: Column| -> String {
        let fmt = canvas::format;
        match (c, &data, &snap) {
            (Column::TargetResists, Some(d), _) => resists_text(&d.resists),
            (Column::Speed, Some(d), _) => format!("{} m/s", fmt(d.max_velocity)),
            (Column::SigRadius, Some(d), _) if d.sig_radius.is_finite() => {
                format!("{} m", fmt(d.sig_radius))
            }
            (Column::SigRadius, Some(_), _) => "∞".to_owned(),
            (Column::Radius, Some(d), _) => format!("{} m", fmt(d.radius)),
            (Column::FullHp, Some(d), _) => d.hp.map_or_else(String::new, fmt),
            (c, _, Some(s)) => ship_column(c, &s.ship),
            _ => String::new(),
        }
    };
    let name = g.target_name(&target);
    let fit = match target.kind {
        TargetKind::Fit(fit) => Some(fit),
        TargetKind::Profile(_) => None,
    };
    let icon = ship_icon(g, fit, AssetIcon::Target, cx);
    row_frame(SharedString::from(format!("tgt-row-{i}")), selected, cx)
        .child(picker)
        .child(icon)
        .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
        .children(cols.iter().map(|&c| value_cell().child(value(c))))
        .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
            if event.click_count() >= 2 {
                g.remove_target(i);
                g.recompute(cx);
            } else {
                g.selected_target = Some(i);
                window.focus(&g.targets_focus, cx);
            }
            cx.notify();
        }))
        .context_menu({
            let (this, fits, tabs) = (this.clone(), Rc::clone(fits), Rc::clone(tabs));
            let is_fit = matches!(target.kind, TargetKind::Fit(_));
            let row = TargetRow {
                index: i,
                fit: is_fit,
                resist_mode: (is_fit && !ignore_resists).then_some(target.resist_mode),
                user_profile: match &target.kind {
                    TargetKind::Profile(p) if p.user => Some(p.name.clone().into()),
                    _ => None,
                },
            };
            move |menu, window, cx| {
                target_menu(menu, &this, &fits, &tabs, Some(row.clone()), window, cx)
            }
        })
        .into_any_element()
}
