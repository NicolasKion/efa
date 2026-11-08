//! The left side: market browser and fittings browser.

mod fittings;
mod market;
mod menus;

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::{BrowserTab, Efa, Loaded};
use crate::data::TypeId;

pub(super) use fittings::{BrowserKey, BrowserRows};
pub(super) use market::{FilterKey, ResultsKey};

/// Drones that can be in space at once without skills, the floor for a stack's size.
const BASE_ACTIVE_DRONES: u32 = 5;

impl Efa {
    /// The market and fittings browsers. In a `narrow` window the stats pane is a third tab
    /// here instead of a column of its own.
    pub(super) fn browser(
        &mut self,
        loaded: &Rc<Loaded>,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let stats_shown = narrow && self.side_stats;
        let content = match self.browser_tab {
            // The stats are laid out for their own pane's width; narrower, they scroll sideways.
            _ if stats_shown => div()
                .id("side-stats")
                .size_full()
                .overflow_x_scroll()
                .child(
                    div()
                        .h_full()
                        .min_w(px(super::stats::PANE_WIDTH))
                        .child(self.stats_panel(loaded, cx)),
                )
                .into_any_element(),
            BrowserTab::Market => self.market(loaded, cx).into_any_element(),
            BrowserTab::Fittings => self.fittings(loaded, cx).into_any_element(),
        };
        let theme = cx.theme();
        let tab = |id: &'static str, label: &'static str, which: Option<BrowserTab>| {
            let selected = match which {
                Some(which) => !stats_shown && self.browser_tab == which,
                None => stats_shown,
            };
            div()
                .id(id)
                .min_w_0()
                .truncate()
                .when(narrow, Styled::px_2)
                .when(!narrow, Styled::px_3)
                .py_1()
                .rounded(px(4.))
                .when(selected, |t| {
                    t.bg(theme.tab_active)
                        .text_color(theme.tab_active_foreground)
                })
                .when(!selected, |t| t.text_color(theme.tab_foreground))
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        match which {
                            Some(which) => {
                                this.browser_tab = which;
                                this.side_stats = false;
                            }
                            None => this.side_stats = true,
                        }
                        cx.notify();
                    }),
                )
        };
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .gap_1()
                    .p_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(tab("tab-market", "Market", Some(BrowserTab::Market)))
                    .child(tab("tab-fittings", "Fittings", Some(BrowserTab::Fittings)))
                    .when(narrow, |bar| bar.child(tab("tab-stats", "Stats", None))),
            )
            .child(content)
    }

    /// How many of a drone one "Add to Drone Bay" adds, as pyfa sizes it: no more than can be in
    /// space at once, and no more than the bandwidth can run.
    pub(super) fn drone_stack_size(&self, loaded: &Loaded, drone: TypeId) -> u32 {
        let Some(stats) = &self.stats else { return 0 };
        let hard_limit = BASE_ACTIVE_DRONES.max(stats.drones_launched.total.unwrap_or(0));
        let bandwidth = stats.drone_bandwidth.output.unwrap_or(0.0);
        let per_drone = loaded.catalog.get(drone).map_or(0.0, |t| t.drone_bandwidth);
        if per_drone > 0.0 {
            hard_limit.min((bandwidth / per_drone).floor() as u32)
        } else {
            hard_limit
        }
    }

    /// Units of an item that still fit in the cargo hold.
    pub(super) fn cargo_room_for(&self, loaded: &Loaded, item: TypeId) -> u32 {
        let (Some(fit), Some(stats)) = (self.fit(), &self.stats) else {
            return 0;
        };
        let volume = loaded.catalog.get(item).map_or(0.0, |t| t.volume);
        let used = fit.hold_volume(
            &loaded.catalog,
            self.saved.settings.preferences.general.implants_in_cargo,
        );
        let free = stats.cargo_capacity.unwrap_or(0.0) - used;
        if volume <= 0.0 || free <= 0.0 {
            return 0;
        }
        (free / volume).floor() as u32
    }
}
