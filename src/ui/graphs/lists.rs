//! The fit list and the target list under the plot, with pyfa's columns and context menus.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    GraphsWindow, canvas,
    style::{self, Lightness, LineColor, LineStyle},
};
use crate::graphs::{defs::Column, plot::ResistMode, profiles::TargetProfile, snapshot::Ship};

/// A fit plotted, in its color.
#[derive(Debug, Clone)]
pub struct Source {
    pub fit: usize,
    pub color: LineColor,
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

const ROW_H: f32 = 22.;

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
        Column::TargetResists => String::new(),
    }
}

fn resists_text(resists: &[f64; 4]) -> String {
    resists
        .iter()
        .map(|r| format!("{:.0}", r * 100.0))
        .collect::<Vec<_>>()
        .join("/")
}

fn header(cols: &[Column], first: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .h(px(ROW_H))
        .px_2()
        .gap_2()
        .bg(cx.theme().table_head)
        .text_xs()
        .text_color(cx.theme().table_head_foreground)
        .child(div().w(px(56.)))
        .child(div().flex_1().child(first))
        .children(cols.iter().map(|c| div().w(px(84.)).child(c.header())))
}

pub fn sources(g: &mut GraphsWindow, cx: &mut Context<GraphsWindow>) -> AnyElement {
    let this = cx.entity().downgrade();
    let cols = g.graph.src_columns();
    let (fits, tabs) = fit_choices(g);
    let rows: Vec<AnyElement> = (0..g.sources.len())
        .map(|i| source_row(g, i, cols, &this, &fits, &tabs, cx))
        .collect();
    let empty = rows.is_empty();
    v_flex()
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
        .h_full()
        .border_1()
        .border_color(cx.theme().border)
        .overflow_y_scroll()
        .child(header(cols, "Fit", cx))
        .children(rows)
        .when(empty, |list| {
            list.child(
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Right-click to add fits to graph"),
            )
        })
        .child(div().flex_1().min_h(px(ROW_H)).id("graph-sources-rest"))
        .context_menu(move |menu, window, cx| {
            source_menu(menu, &this, &fits, &tabs, None, window, cx)
        })
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
                    g.add_source(fit);
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
    let theme = cx.theme();
    let color = source.color.color(theme);
    let selected = g.selected_source == Some(i);
    let picker = {
        let this = this.clone();
        Button::new(SharedString::from(format!("src-color-{i}")))
            .ghost()
            .xsmall()
            .w(px(56.))
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
    h_flex()
        .id(SharedString::from(format!("src-row-{i}")))
        .h(px(ROW_H))
        .px_2()
        .gap_2()
        .text_xs()
        .when(i % 2 == 1, |r| r.bg(theme.table_even))
        .when(selected, |r| r.bg(theme.table_active))
        .hover(|s| s.bg(theme.table_hover))
        .child(picker)
        .child(div().flex_1().truncate().child(g.fit_name(source.fit)))
        .children(cols.iter().map(|&c| {
            div().w(px(84.)).truncate().child(
                snap.as_ref()
                    .map_or_else(String::new, |s| ship_column(c, &s.ship)),
            )
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
    let cols = g.graph.tgt_columns(g.options.ignore_resists);
    let (fits, tabs) = fit_choices(g);
    let ignore_resists = g.options.ignore_resists;
    let rows: Vec<AnyElement> = (0..g.targets.len())
        .map(|i| target_row(g, i, &cols, &this, &fits, &tabs, ignore_resists, cx))
        .collect();
    v_flex()
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
        .h_full()
        .border_1()
        .border_color(cx.theme().border)
        .overflow_y_scroll()
        .child(header(&cols, "Target", cx))
        .children(rows)
        .child(div().flex_1().min_h(px(ROW_H)).id("graph-targets-rest"))
        .context_menu(move |menu, window, cx| {
            target_menu(menu, &this, &fits, &tabs, None, window, cx)
        })
        .into_any_element()
}

/// The row a menu was opened on: its index, whether it is a fit, and the resist mode to offer
/// (fits only, while resists count).
#[derive(Debug, Clone, Copy)]
struct TargetRow {
    index: usize,
    fit: bool,
    resist_mode: Option<ResistMode>,
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
    menu = menu.submenu("Add Target Profile", window, cx, move |menu, window, cx| {
        let add = |p: TargetProfile, this: Weak| {
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let p = p.clone();
                with(&this, cx, |g, _| g.add_target(TargetKind::Profile(p)));
            }
        };
        let menu = menu
            .item(
                PopupMenuItem::new("Ideal Target")
                    .on_click(add(TargetProfile::ideal(), profiles.clone())),
            )
            .separator();
        profile_tree(menu, &profiles, &[], window, cx, &add)
    });
    if let Some(TargetRow {
        index: row,
        fit: is_fit,
        resist_mode,
    }) = row
    {
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

/// pyfa's builtin profiles as a tree of their `[Category]` prefixes.
fn profile_tree<F, H>(
    menu: PopupMenu,
    this: &Weak,
    path: &[String],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    add: &F,
) -> PopupMenu
where
    F: Fn(TargetProfile, Weak) -> H + Clone + 'static,
    H: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let mut menu = menu;
    let mut seen: Vec<String> = Vec::new();
    for profile in TargetProfile::builtins() {
        let cats = profile.categories();
        if cats.len() < path.len() || cats[..path.len()] != *path {
            continue;
        }
        if cats.len() == path.len() {
            menu = menu.item(
                PopupMenuItem::new(profile.short_name().to_owned())
                    .on_click(add(profile.clone(), this.clone())),
            );
            continue;
        }
        let next = cats[path.len()].to_owned();
        if seen.contains(&next) {
            continue;
        }
        seen.push(next.clone());
        let mut sub_path = path.to_vec();
        sub_path.push(next.clone());
        let (this, add) = (this.clone(), add.clone());
        menu = menu.submenu(next, window, cx, move |menu, window, cx| {
            profile_tree(menu, &this, &sub_path, window, cx, &add)
        });
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
    let theme = cx.theme();
    // A target's swatch shows its shade of the first fit's color.
    let base = g
        .sources
        .first()
        .map_or(LineColor::Blue, |s| s.color)
        .color(theme);
    let color = target.lightness.apply(base);
    let selected = g.selected_target == Some(i);
    let picker = {
        let this = this.clone();
        Button::new(SharedString::from(format!("tgt-style-{i}")))
            .ghost()
            .xsmall()
            .w(px(56.))
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
    h_flex()
        .id(SharedString::from(format!("tgt-row-{i}")))
        .h(px(ROW_H))
        .px_2()
        .gap_2()
        .text_xs()
        .when(i % 2 == 1, |r| r.bg(theme.table_even))
        .when(selected, |r| r.bg(theme.table_active))
        .hover(|s| s.bg(theme.table_hover))
        .child(picker)
        .child(div().flex_1().truncate().child(name))
        .children(
            cols.iter()
                .map(|&c| div().w(px(84.)).truncate().child(value(c))),
        )
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
            };
            move |menu, window, cx| target_menu(menu, &this, &fits, &tabs, Some(row), window, cx)
        })
        .into_any_element()
}
