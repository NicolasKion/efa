//! The Command tab, pyfa's command pane: fits boosting this one from its fleet, switched on and
//! off from the state column, added from the fittings browser, open tabs or a list of every fit
//! that carries a command burst, and pyfa's generic links below them.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity,
    Window, div, prelude::FluentBuilder, px,
};

use super::{
    MENU_MAX_H, ROW_HEIGHT,
    linked::{Link, add_fit_entries},
};
use crate::ui::widgets::DropHighlight;
use crate::{
    data::Catalog,
    fit::{
        Fit,
        links::{CommandLink, LinkCategory, LinkSet, STRENGTHS, link_name},
    },
    icons::Icon,
    store::Saved,
    ui::{
        Efa, Loaded,
        columns::{ICON_W, STATE_W},
        widgets::{color, entry, icon_box, icon_label, on_efa},
    },
};

/// Past this many command fits pyfa groups its menu by ship.
const GROUP_BY_SHIP_FROM: usize = 15;

/// A row of the pane.
#[derive(Debug, Clone, Copy)]
enum Row {
    /// A command entry's index and the saved fit it points at.
    Fit { entry: usize, fit: usize },
    /// A generic link's index.
    Link(usize),
}

/// The command entries and generic links some rows stand for.
#[derive(Debug, Default)]
struct Entries {
    fits: Vec<usize>,
    links: Vec<usize>,
}

impl Entries {
    fn of(rows: impl IntoIterator<Item = Row>) -> Self {
        let mut entries = Self::default();
        for row in rows {
            match row {
                Row::Fit { entry, .. } => entries.fits.push(entry),
                Row::Link(i) => entries.links.push(i),
            }
        }
        entries
    }
}

/// Command fits in pyfa's order, by fit name, then the generic links. Entries whose fit was
/// deleted are left out.
fn rows(saved: &Saved, fit: &Fit) -> Vec<Row> {
    let mut fits: Vec<(usize, usize)> = fit
        .command
        .iter()
        .enumerate()
        .filter_map(|(entry, c)| Some((entry, saved.fit_index(c.fit)?)))
        .collect();
    fits.sort_by(|a, b| saved.fits[a.1].name.cmp(&saved.fits[b.1].name));
    fits.into_iter()
        .map(|(entry, fit)| Row::Fit { entry, fit })
        .chain((0..fit.command_links.len()).map(Row::Link))
        .collect()
}

/// Removes `indices` from `list`, each once.
fn remove_indices<T>(list: &mut Vec<T>, mut indices: Vec<usize>) {
    indices.sort_unstable();
    indices.dedup();
    for i in indices.into_iter().rev() {
        if i < list.len() {
            list.remove(i);
        }
    }
}

/// How the command pane names a fit: "Fit (Ship)".
fn row_label(catalog: &Catalog, fit: &Fit) -> String {
    match fit.ship {
        Some(ship) => format!("{} ({})", fit.name, catalog.name(ship)),
        None => fit.name.clone(),
    }
}

impl Efa {
    /// The rows at `places`.
    fn command_entries(&self, places: &[usize]) -> Entries {
        let Some(fit) = self.fit() else {
            return Entries::default();
        };
        let rows = rows(&self.saved, fit);
        Entries::of(places.iter().filter_map(|&n| rows.get(n).copied()))
    }

    /// The rows behind the clicked row and the selection it belongs to.
    fn command_targets(&self, clicked: usize) -> Entries {
        self.command_entries(&self.addition_targets(clicked))
    }

    fn remove_command_entries(&mut self, entries: Entries, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            remove_indices(&mut fit.command, entries.fits);
            remove_indices(&mut fit.command_links, entries.links);
            true
        });
    }

    /// pyfa's toggle: the clicked row's state decides whether all of them go on or off.
    fn toggle_command_entries(&mut self, clicked: Row, entries: Entries, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            let on = match clicked {
                Row::Fit { entry, .. } => fit.command.get(entry).map(|c| !c.active),
                Row::Link(i) => fit.command_links.get(i).map(|l| !l.active),
            };
            let Some(on) = on else { return false };
            let (mut fits, mut links) = (entries.fits, entries.links);
            match clicked {
                Row::Fit { entry, .. } => fits.push(entry),
                Row::Link(i) => links.push(i),
            }
            for i in fits {
                if let Some(c) = fit.command.get_mut(i) {
                    c.active = on;
                }
            }
            for i in links {
                if let Some(l) = fit.command_links.get_mut(i) {
                    l.active = on;
                }
            }
            false
        });
    }

    /// Add a generic link, unless the same one is there.
    fn add_command_link(&mut self, link: CommandLink, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            if fit.command_links.contains(&link) {
                return false;
            }
            fit.command_links.push(link);
            true
        });
    }

    pub(in crate::ui) fn command_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let rows = rows(&self.saved, fit);
        let mut content: Vec<AnyElement> = Vec::with_capacity(rows.len() + 1);
        if rows.is_empty() {
            content.push(
                div()
                    .p_2()
                    .text_color(cx.theme().muted_foreground)
                    .child("Drag a fit to this area")
                    .into_any_element(),
            );
        }
        for (n, row) in rows.iter().enumerate() {
            content.push(self.command_row(loaded, n, *row, cx));
        }
        v_flex()
            .id("command-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .track_focus(&self.additions_focus)
            .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| g.command_key(event, cx)))
            .children(content)
            .drop_highlight::<super::FitDrag>()
            .on_drop(cx.listener(|g, drag: &super::FitDrag, _, cx| {
                g.link(Link::Command, &[drag.index], cx);
            }))
            .context_menu({
                let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
                move |menu, window, cx| command_view_menu(menu, &this, &loaded, window, cx)
            })
            .into_any_element()
    }

    fn command_row(
        &self,
        loaded: &Rc<Loaded>,
        n: usize,
        row: Row,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let catalog = &loaded.catalog;
        let (active, icon, label) = match row {
            Row::Fit { entry, fit: index } => {
                let target = &self.saved.fits[index];
                (
                    fit.command.get(entry).is_some_and(|c| c.active),
                    target.ship,
                    row_label(catalog, target),
                )
            }
            Row::Link(i) => {
                let Some(link) = fit.command_links.get(i) else {
                    return div().into_any_element();
                };
                let category = link.set.charges().first().map(|&(c, _)| c);
                (
                    link.active,
                    category.map(LinkCategory::burst),
                    link.label(catalog),
                )
            }
        };
        let (mark, mark_color) = if active {
            ("✓", self.palette.green)
        } else {
            ("○", self.palette.muted)
        };
        let selected = self.additions_selection.contains(&n);
        h_flex()
            .id(SharedString::from(format!("command-{n}")))
            .gap_1()
            .px_1()
            .h(px(ROW_HEIGHT))
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .child(
                div()
                    .id(SharedString::from(format!("command-state-{n}")))
                    .w(px(STATE_W))
                    .flex_none()
                    .text_color(color(mark_color))
                    .child(mark)
                    .on_click(cx.listener(move |g, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        let entries = g.command_targets(n);
                        g.toggle_command_entries(row, entries, cx);
                    })),
            )
            .child(
                div()
                    .w(px(ICON_W))
                    .flex_none()
                    .children(icon.map(|t| icon_box(catalog, &loaded.icons, Icon::Type(t), 22.))),
            )
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.additions_focus, cx);
                if event.click_count() >= 2 {
                    g.remove_command_entries(Entries::of([row]), cx);
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
                    let menu = match row {
                        Row::Fit { fit, .. } => command_fit_menu(menu, &this, n, fit, cx),
                        Row::Link(_) => {
                            menu.item(entry(IconName::Delete, "Remove Generic Link").on_click(
                                on_efa(&this, move |g, _, _, cx| {
                                    let entries = g.command_targets(n);
                                    g.remove_command_entries(entries, cx);
                                }),
                            ))
                        }
                    };
                    command_view_menu(menu.separator(), &this, &loaded, window, cx)
                }
            })
            .into_any_element()
    }

    /// pyfa's keys: Escape clears the selection, Ctrl+A selects everything, Delete removes.
    fn command_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = self.fit().map_or(0, |f| rows(&self.saved, f).len());
        self.list_keys(
            event,
            count,
            true,
            |g, places, cx| {
                let entries = g.command_entries(&places);
                g.remove_command_entries(entries, cx);
            },
            cx,
        );
    }
}

/// pyfa's commandFit entries.
fn command_fit_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    n: usize,
    fit_index: usize,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let is_current = this
        .upgrade()
        .is_some_and(|g| g.read(cx).saved.settings.current == Some(fit_index));
    let mut menu = menu;
    if !is_current {
        menu = menu.item(
            entry(IconName::ExternalLink, "Open Fit in New Tab")
                .on_click(on_efa(this, move |g, _, _, cx| g.open_fit(fit_index, cx))),
        );
    }
    menu.item(
        entry(IconName::Delete, "Remove Command Fit").on_click(on_efa(this, move |g, _, _, cx| {
            let entries = g.command_targets(n);
            g.remove_command_entries(entries, cx);
        })),
    )
}

/// pyfa's commandView entries: add fits from the browser, from open tabs, or from every fit
/// with a command burst, and generic links.
fn command_view_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = add_fit_entries(menu, this, loaded, Link::Command, window, cx);
    let menu = generic_links_menu(menu, this, loaded, window, cx);
    let Some(g) = this.upgrade() else { return menu };
    let catalog = &loaded.catalog;
    let mut boosters: Vec<(usize, String, String)> = g
        .read(cx)
        .saved
        .fits
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f.modules()
                .any(|m| catalog.projection.command_bursts.contains(&m.type_id))
        })
        .map(|(i, f)| {
            let ship = f.ship.map_or_default(|s| catalog.name(s));
            (i, ship, f.name.clone())
        })
        .collect();
    if boosters.is_empty() {
        return menu;
    }
    boosters.sort_by(|a, b| a.2.cmp(&b.2));
    let this = this.clone();
    let pick = move |index: usize| {
        on_efa(&this, move |g, _, _, cx| {
            g.link(Link::Command, &[index], cx);
        })
    };
    menu.submenu_with_element(
        "Command Fits",
        icon_label(IconName::Network, "Command Fits"),
        window,
        cx,
        move |menu, window, cx| {
            let menu = menu
                .scrollable(true)
                .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H));
            if boosters.len() < GROUP_BY_SHIP_FROM {
                return boosters.iter().fold(menu, |menu, (i, ship, name)| {
                    menu.item(PopupMenuItem::new(format!("({ship}) {name}")).on_click(pick(*i)))
                });
            }
            let mut ships: Vec<&str> = boosters.iter().map(|b| b.1.as_str()).collect();
            ships.sort_unstable();
            ships.dedup();
            ships.into_iter().fold(menu, |menu, ship| {
                let fits: Vec<(usize, String)> = boosters
                    .iter()
                    .filter(|b| b.1 == ship)
                    .map(|b| (b.0, b.2.clone()))
                    .collect();
                let pick = pick.clone();
                menu.submenu(ship.to_owned(), window, cx, move |menu, _, _| {
                    fits.iter().fold(menu, |menu, (i, name)| {
                        menu.item(PopupMenuItem::new(name.clone()).on_click(pick(*i)))
                    })
                })
            })
        },
    )
}

/// pyfa's "Generic Links": every link at its strongest in one click, all links, or a category's
/// or a single link, each at a ship bonus with or without a mindlink.
fn generic_links_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = this.clone();
    let loaded = Rc::clone(loaded);
    let add = move |set: LinkSet, strength: u8, mindlink: bool| {
        on_efa(&this, move |g, _, _, cx| {
            g.add_command_link(CommandLink::new(set, strength, mindlink), cx);
        })
    };
    let max = add.clone();
    let strengths = move |menu: PopupMenu, set: LinkSet| {
        let mindlinks = STRENGTHS.iter().fold(menu, |menu, &s| {
            menu.item(
                PopupMenuItem::new(format!("{s}%/lvl + Mindlink")).on_click(add(set, s, true)),
            )
        });
        STRENGTHS.iter().fold(mindlinks.separator(), |menu, &s| {
            menu.item(PopupMenuItem::new(format!("{s}%/lvl")).on_click(add(set, s, false)))
        })
    };
    menu.submenu_with_element(
        "Generic Links",
        icon_label(IconName::Plus, "Generic Links"),
        window,
        cx,
        move |menu, window, cx| {
            let max =
                PopupMenuItem::new("Max Links").on_click(max(LinkSet::All, STRENGTHS[0], true));
            let strengths_all = strengths.clone();
            let mut menu = menu
                .item(max)
                .separator()
                .submenu("All Links", window, cx, move |menu, _, _| {
                    strengths_all(menu, LinkSet::All)
                })
                .separator();
            for category in LinkCategory::ALL {
                let strengths = strengths.clone();
                let loaded = Rc::clone(&loaded);
                menu = menu.submenu(category.label(), window, cx, move |menu, window, cx| {
                    let mut sets = vec![(
                        LinkSet::Category(category),
                        format!("All {} Links", category.label()),
                    )];
                    sets.extend(
                        category
                            .charges()
                            .iter()
                            .map(|&t| (LinkSet::Charge(t), link_name(&loaded.catalog, t))),
                    );
                    sets.into_iter().fold(menu, |menu, (set, label)| {
                        let strengths = strengths.clone();
                        menu.submenu(label, window, cx, move |menu, _, _| strengths(menu, set))
                    })
                });
            }
            menu
        },
    )
}
