//! The fit tabs: the strip over the fit, renaming, reordering and tearing a tab off into its own
//! window.

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::ContextMenuExt,
};
use gpui_kit::{
    AnyElement, AppContext, Context, InteractiveElement, IntoElement, MouseButton, MouseUpEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::{Efa, Loaded, Role, chrome, fitting, widgets};
use crate::icons::Icon;
use crate::ui::widgets::{DropHighlight, TipExt};

/// Size of a fit tab, and of the name field while renaming one.
const TAB_H: f32 = 26.;
const TAB_MAX_W: f32 = 300.;
const TAB_RENAME_W: f32 = 200.;

/// A fit tab being dragged: to reorder tabs, or onto the drop zone to open a new window.
#[derive(Debug, Clone)]
struct TabDrag {
    index: usize,
    label: SharedString,
}

/// What follows the pointer while a tab is dragged.
struct TabPreview(SharedString);

impl Render for TabPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(cx.theme().tab_active)
            .border_1()
            .border_color(cx.theme().ring)
            .shadow_md()
            .text_sm()
            .child(self.0.clone())
    }
}

impl Efa {
    /// Open a fit in a window of its own and close its tab here, the way tearing a tab off works.
    fn open_in_new_window(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.saved.fits.len() {
            return;
        }
        // The new window starts from the hub, so it has to have this window's latest.
        self.publish(cx);
        let hub = self.hub.clone();
        let options = chrome::window_options(1400., 860., cx);
        let opened = gpui_kit::open_window(chrome::options(options), cx, move |window, cx| {
            cx.new(|cx| Efa::new(hub, Role::Detached(index), window, cx))
        });
        match opened {
            Ok(_) => self.close_fit(index, cx),
            Err(e) => {
                self.set_error(format!("Could not open a window: {e}"));
                cx.notify();
            }
        }
    }

    /// Move a tab to where another one is.
    fn move_tab(&mut self, from: usize, onto: usize, cx: &mut Context<Self>) {
        let open = &mut self.saved.settings.open;
        let (Some(a), Some(b)) = (
            open.iter().position(|&i| i == from),
            open.iter().position(|&i| i == onto),
        ) else {
            return;
        };
        let tab = open.remove(a);
        open.insert(b, tab);
        self.save();
        cx.notify();
    }

    /// The open fits as slim tabs: ship icon, fit name and hull on one line.
    pub(super) fn fit_tabs(&self, loaded: &Loaded, cx: &mut Context<Self>) -> AnyElement {
        let tabs: Vec<AnyElement> = self
            .saved
            .settings
            .open
            .iter()
            .map(|&i| self.fit_tab(loaded, i, cx))
            .collect();
        let theme = cx.theme();
        h_flex()
            .id("fit-tabs")
            .flex_none()
            .items_center()
            .gap_0p5()
            .px_1()
            .py_1()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .overflow_x_scroll()
            .children(tabs)
            .child(
                Button::new("new-fit")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip("New fit")
                    .on_click(cx.listener(|this, _, _, cx| this.new_tab(cx))),
            )
            .into_any_element()
    }

    fn fit_tab(&self, loaded: &Loaded, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let fit = &self.saved.fits[i];
        let selected = self.saved.settings.current == Some(i);
        let renaming = selected && self.renaming;
        let ship_name = fit.ship.map(|s| loaded.catalog.name(s));
        let label = match &ship_name {
            Some(ship) => format!("{ship}: {}", fit.name),
            None => fit.name.clone(),
        };
        // The hull only adds something when the fit is not just named after it.
        let hull = ship_name.filter(|ship| *ship != fit.name);
        let group = SharedString::from(format!("tab-{i}"));
        let name: AnyElement = if renaming {
            div()
                .w(px(TAB_RENAME_W))
                .child(self.name_field())
                .into_any_element()
        } else {
            h_flex()
                .min_w_0()
                .gap_1p5()
                .child(div().min_w_0().truncate().child(fit.name.clone()))
                .when_some(hull, |row, hull| {
                    row.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(hull),
                    )
                })
                .into_any_element()
        };
        h_flex()
            .id(group.clone())
            .group(group.clone())
            .when(
                self.saved.settings.preferences.general.tab_tooltips,
                |tab| {
                    let tip: SharedString = label.clone().into();
                    tab.tip(tip)
                },
            )
            .flex_none()
            .max_w(px(TAB_MAX_W))
            .h(px(TAB_H))
            .items_center()
            .gap_1p5()
            .pl_2()
            .pr_1()
            .rounded(px(5.))
            .text_sm()
            .when(selected, |tab| {
                tab.bg(theme.secondary).text_color(theme.foreground)
            })
            .when(!selected, |tab| {
                tab.text_color(theme.tab_foreground)
                    .hover(|s| s.bg(theme.secondary.opacity(0.5)))
            })
            .when_some(fit.ship, |tab, ship| {
                tab.child(widgets::icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Type(ship),
                    16.,
                ))
            })
            .child(name)
            .child(
                div()
                    // The close button stays out of the way until the tab is active or hovered.
                    .when(!selected, |d| {
                        d.opacity(0.).group_hover(group, |s| s.opacity(1.))
                    })
                    .child(
                        Button::new(SharedString::from(format!("tab-close-{i}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Close tab")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_fit(i, cx);
                            })),
                    )
                    // Pressing the close button must not bring its tab up first.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
            // A tab comes up on press; a second click on it renames the fit.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                    if this.saved.settings.current == Some(i) && event.click_count >= 2 {
                        this.start_rename(window, cx);
                        return;
                    }
                    if this.saved.settings.current != Some(i) {
                        this.renaming = false;
                        this.saved.settings.current = Some(i);
                        this.clear_selection();
                        this.changed(cx);
                    }
                }),
            )
            // chrome_tabs.py OnMiddleUp: a middle click closes the tab.
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                    cx.stop_propagation();
                    this.close_fit(i, cx);
                }),
            )
            .on_drag(
                TabDrag {
                    index: i,
                    label: label.into(),
                },
                {
                    let this = cx.weak_entity();
                    move |drag, _, _, cx| {
                        // The drop zone only shows while a tab is in flight.
                        let _ = this.update(cx, |g, cx| {
                            g.tab_dragging = true;
                            cx.notify();
                        });
                        cx.new(|_| TabPreview(drag.label.clone()))
                    }
                },
            )
            .drop_highlight::<TabDrag>()
            .on_drop(cx.listener(move |g, drag: &TabDrag, _, cx| {
                g.tab_dragging = false;
                g.move_tab(drag.index, i, cx);
            }))
            .context_menu({
                let this = cx.weak_entity();
                let ship = fit.ship.filter(|_| selected);
                move |menu, window, cx| {
                    let on = |f: fn(&mut Efa, usize, &mut Context<Efa>)| {
                        widgets::on_efa(&this, move |g, _, _, cx| f(g, i, cx))
                    };
                    let menu = menu
                        .item(widgets::entry(IconName::Replace, "Rename").on_click(
                            widgets::on_efa(&this, move |g, _, window, cx| {
                                g.saved.settings.current = Some(i);
                                g.changed(cx);
                                g.start_rename(window, cx);
                            }),
                        ))
                        .item(
                            widgets::entry(IconName::ExternalLink, "Open in New Window")
                                .on_click(on(Efa::open_in_new_window)),
                        )
                        .separator()
                        .item(widgets::entry(IconName::Close, "Close").on_click(on(Efa::close_fit)))
                        .item(
                            widgets::entry(IconName::Close, "Close Others")
                                .on_click(on(Efa::close_other_fits)),
                        );
                    // The ship's own menu, which used to sit on the header, for the active fit.
                    match ship {
                        Some(ship) => {
                            fitting::ship_entries(menu.separator(), &this, ship, window, cx)
                        }
                        None => menu,
                    }
                }
            })
            .into_any_element()
    }

    /// Swap the active tab's name for the name field, focused.
    pub(super) fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = true;
        self.inputs_show = None;
        self.sync_inputs(window, cx);
        self.name_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Where a dragged tab is dropped to get a window of its own. Wayland does not tell a window
    /// where a drag outside it ends, so tearing off happens onto this zone instead.
    pub(super) fn tear_off_zone(cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .id("tear-off-zone")
            .absolute()
            .left_4()
            .right_4()
            .bottom_4()
            .h(px(120.))
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .rounded(px(8.))
            .border_2()
            .border_dashed()
            .border_color(theme.ring)
            .bg(theme.popover.opacity(0.92))
            .text_color(theme.foreground)
            .child(gpui_kit::component::Icon::new(IconName::ExternalLink).size(px(18.)))
            .child("Drop here to open this fit in a new window")
            .drop_highlight::<TabDrag>()
            .on_drop(cx.listener(|g, drag: &TabDrag, _, cx| {
                g.tab_dragging = false;
                g.open_in_new_window(drag.index, cx);
            }))
    }
}
