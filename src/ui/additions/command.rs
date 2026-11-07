//! The Command tab, pyfa's command pane: fits boosting this one from its fleet, switched on and
//! off from the state column, added from the fittings browser, open tabs or a list of every fit
//! that carries a command burst.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{
    MENU_MAX_H, ROW_HEIGHT,
    linked::{Link, add_fit_entries},
};
use crate::{
    data::Catalog,
    fit::Fit,
    icons::Icon,
    store::Saved,
    ui::{
        Efa, Loaded,
        columns::{ICON_W, STATE_W},
        widgets::{color, entry, icon_box, icon_label},
    },
};

/// Past this many command fits pyfa groups its menu by ship.
const GROUP_BY_SHIP_FROM: usize = 15;

/// A row of the pane: the command entry's index and the saved fit it points at.
#[derive(Debug, Clone, Copy)]
struct Row {
    entry: usize,
    fit: usize,
}

/// Command fits in pyfa's order, by fit name. Entries whose fit was deleted are left out.
fn rows(saved: &Saved, fit: &Fit) -> Vec<Row> {
    let mut rows: Vec<Row> = fit
        .command
        .iter()
        .enumerate()
        .filter_map(|(entry, c)| {
            Some(Row {
                entry,
                fit: saved.fit_index(c.fit)?,
            })
        })
        .collect();
    rows.sort_by(|a, b| saved.fits[a.fit].name.cmp(&saved.fits[b.fit].name));
    rows
}

/// How the command pane names a fit: "Fit (Ship)".
fn row_label(catalog: &Catalog, fit: &Fit) -> String {
    match fit.ship {
        Some(ship) => format!("{} ({})", fit.name, catalog.name(ship)),
        None => fit.name.clone(),
    }
}

impl Efa {
    /// The command entries behind the clicked row and the selection it belongs to.
    fn command_targets(&self, clicked: usize) -> Vec<usize> {
        let Some(fit) = self.fit() else {
            return Vec::new();
        };
        let rows = rows(&self.saved, fit);
        self.addition_targets(clicked)
            .into_iter()
            .filter_map(|n| rows.get(n).map(|r| r.entry))
            .collect()
    }

    fn remove_command_fits(&mut self, entries: Vec<usize>, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            let mut entries = entries;
            entries.sort_unstable();
            entries.dedup();
            for i in entries.into_iter().rev() {
                if i < fit.command.len() {
                    fit.command.remove(i);
                }
            }
            true
        });
    }

    /// pyfa's toggle: the clicked fit's state decides whether all of them go on or off.
    fn toggle_command_fits(&mut self, clicked: usize, entries: Vec<usize>, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            let Some(on) = fit.command.get(clicked).map(|c| !c.active) else {
                return false;
            };
            for i in entries.into_iter().chain([clicked]) {
                if let Some(c) = fit.command.get_mut(i) {
                    c.active = on;
                }
            }
            false
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
            .drag_over::<super::FitDrag>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(|g, drag: &super::FitDrag, _, cx| {
                g.link(Link::Command, &[drag.index], cx);
            }))
            .context_menu({
                let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
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
        let target = &self.saved.fits[row.fit];
        let active = self
            .fit()
            .and_then(|f| f.command.get(row.entry))
            .is_some_and(|c| c.active);
        let (mark, mark_color) = if active {
            ("✓", self.palette.green)
        } else {
            ("○", self.palette.muted)
        };
        let selected = self.additions_selection.contains(&n);
        let entry = row.entry;
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
                        g.toggle_command_fits(entry, entries, cx);
                    })),
            )
            .child(
                div().w(px(ICON_W)).flex_none().children(
                    target.ship.map(|ship| {
                        icon_box(&loaded.catalog, &loaded.icons, Icon::Type(ship), 22.)
                    }),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(row_label(&loaded.catalog, target)),
            )
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.additions_focus, cx);
                if event.click_count() >= 2 {
                    g.remove_command_fits(vec![entry], cx);
                } else {
                    g.select_addition(n, event.modifiers(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |g, _: &MouseDownEvent, _, cx| {
                    if !g.additions_selection.contains(&n) {
                        g.additions_selection = vec![n];
                        g.additions_anchor = Some(n);
                        cx.notify();
                    }
                }),
            )
            .context_menu({
                let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                let fit_index = row.fit;
                move |menu, window, cx| {
                    let menu = command_fit_menu(menu, &this, n, fit_index, cx);
                    command_view_menu(menu.separator(), &this, &loaded, window, cx)
                }
            })
            .into_any_element()
    }

    /// pyfa's keys: Escape clears the selection, Ctrl+A selects everything, Delete removes.
    fn command_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let plain = !keystroke.modifiers.modified();
        match keystroke.key.as_str() {
            "escape" if plain => {
                self.additions_selection.clear();
                self.additions_anchor = None;
                cx.notify();
            }
            "a" if keystroke.modifiers == Modifiers::control() => {
                let count = self.fit().map_or(0, |f| rows(&self.saved, f).len());
                self.additions_selection = (0..count).collect();
                cx.notify();
            }
            "delete" if plain && !self.additions_selection.is_empty() => {
                let Some(fit) = self.fit() else { return };
                let rows = rows(&self.saved, fit);
                let entries = self
                    .additions_selection
                    .iter()
                    .filter_map(|&n| rows.get(n).map(|r| r.entry))
                    .collect();
                self.remove_command_fits(entries, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
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
        .is_some_and(|g| g.read(cx).saved.current == Some(fit_index));
    let mut menu = menu;
    if !is_current {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::ExternalLink, "Open Fit in New Tab").on_click(move |_, _, cx| {
                Efa::app_update(&this, cx, |g, cx| g.open_fit(fit_index, cx));
            }),
        );
    }
    let this = this.clone();
    menu.item(
        entry(IconName::Delete, "Remove Command Fit").on_click(move |_, _, cx| {
            Efa::app_update(&this, cx, |g, cx| {
                let entries = g.command_targets(n);
                g.remove_command_fits(entries, cx);
            });
        }),
    )
}

/// pyfa's commandView entries: add fits from the browser, from open tabs, or from every fit
/// with a command burst. Its Generic Links need boosts without a fit, which refine cannot do.
fn command_view_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = add_fit_entries(menu, this, loaded, Link::Command, window, cx);
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
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut gpui_kit::App| {
            Efa::app_update(&this, cx, |g, cx| g.link(Link::Command, &[index], cx));
        }
    };
    menu.submenu_with_element(
        "Command Fits",
        icon_label(IconName::Network, "Command Fits"),
        window,
        cx,
        move |menu, window, cx| {
            let menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
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
