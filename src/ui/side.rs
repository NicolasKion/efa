//! The left side: market browser and fittings browser.

use std::{collections::HashSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as LucideIcon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, ClickEvent, Context, InteractiveElement, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{
    Action, AdditionsTab, BrowserTab, Gyfa, Loaded,
    additions::{FitDrag, Link, goes_to_cargo},
    fitting::{DragPreview, DragSource, Dragged},
    item_stats,
    stats::format,
    widgets::{entry, icon_box},
};
use crate::{
    data::{Kind, Slot, TypeId},
    icons::Icon,
    market::{MetaCategory, charges_for_fit},
    ship_browser::{self, Race, Stage},
};

/// Search results shown at most, to keep the list quick to draw.
const SEARCH_LIMIT: usize = 300;
const INDENT: f32 = 14.;
/// Drones that can be in space at once without skills, the floor for a stack's size.
const BASE_ACTIVE_DRONES: u32 = 5;
/// Charges "Add to Cargo" puts in at once, and the smaller batch for scan probes.
const AMMO_BATCH: u32 = 1000;
const PROBE_BATCH: u32 = 8;
/// pyfa's market tree entries that are not market groups, kept apart by negative IDs.
const RECENT_GROUP: i32 = -1;
const CHARGES_GROUP: i32 = -2;
/// The Omega clone icon pyfa marks hulls and items with.
const OMEGA_ICON: i32 = 25874;
/// Widest a hover text gets before it wraps.
const TOOLTIP_W: f32 = 380.;
/// Longest description excerpt a market tooltip shows.
const DESCRIPTION_MAX: usize = 400;

/// Whether the market list shows search results or browses groups, as pyfa tells them apart:
/// recently used items count as a search, charges for the fit as a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketContext {
    Group,
    Search,
}

/// A line of the market list before it is drawn, so the meta buttons can look at what it holds.
enum MarketEntry {
    Group {
        id: i32,
        depth: usize,
        name: SharedString,
        icon: GroupIcon,
    },
    Item {
        id: TypeId,
        depth: usize,
        context: MarketContext,
    },
}

enum GroupIcon {
    Game(Option<i32>),
    Lucide(IconName),
}

impl Gyfa {
    pub(super) fn browser(&mut self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.browser_tab {
            BrowserTab::Market => self.market(loaded, cx).into_any_element(),
            BrowserTab::Fittings => self.fittings(loaded, cx).into_any_element(),
        };
        let theme = cx.theme();
        let tab = |id: &'static str, label: &'static str, which: BrowserTab| {
            let selected = self.browser_tab == which;
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded(px(4.))
                .when(selected, |t| {
                    t.bg(theme.tab_active)
                        .text_color(theme.tab_active_foreground)
                })
                .when(!selected, |t| t.text_color(theme.tab_foreground))
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.browser_tab = which;
                    cx.notify();
                }))
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
                    .child(tab("tab-market", "Market", BrowserTab::Market))
                    .child(tab("tab-fittings", "Fittings", BrowserTab::Fittings)),
            )
            .child(content)
    }

    /// Remember an item taken from the market, for pyfa's Recently Used Items.
    pub(super) fn used_from_market(&mut self, id: TypeId) {
        crate::market::remember(&mut self.saved.recent_items, id);
        self.save();
    }

    /// The market search's results, searched again only when the text changed.
    fn search_results(&mut self, loaded: &Loaded) -> Option<Rc<Vec<TypeId>>> {
        let text = self.search_text.trim();
        // pyfa goes back to the tree while there is nothing but wildcards.
        if text.replace(['*', '?'], "").is_empty() {
            return None;
        }
        if let Some((cached, results)) = &self.search_results
            && cached == text
        {
            return Some(Rc::clone(results));
        }
        let results = Rc::new(loaded.catalog.search(text, SEARCH_LIMIT));
        self.search_results = Some((text.to_owned(), Rc::clone(&results)));
        Some(results)
    }

    fn market_entries(&mut self, loaded: &Loaded) -> Vec<MarketEntry> {
        let mut entries = Vec::new();
        if let Some(results) = self.search_results(loaded) {
            entries.extend(results.iter().map(|&id| MarketEntry::Item {
                id,
                depth: 0,
                context: MarketContext::Search,
            }));
            return entries;
        }
        for &root in &loaded.catalog.market_roots {
            self.market_group(loaded, root, 0, &mut entries);
        }
        entries.push(MarketEntry::Group {
            id: RECENT_GROUP,
            depth: 0,
            name: "Recently Used Items".into(),
            icon: GroupIcon::Lucide(IconName::Calendar),
        });
        if self.expanded_groups.contains(&RECENT_GROUP) {
            entries.extend(
                self.saved
                    .recent_items
                    .iter()
                    .filter(|&&id| loaded.catalog.get(id).is_some())
                    .map(|&id| MarketEntry::Item {
                        id,
                        depth: 1,
                        context: MarketContext::Search,
                    }),
            );
        }
        entries.push(MarketEntry::Group {
            id: CHARGES_GROUP,
            depth: 0,
            name: "Charges For Active Fit".into(),
            icon: GroupIcon::Lucide(IconName::Asterisk),
        });
        if self.expanded_groups.contains(&CHARGES_GROUP)
            && let Some(fit) = self.fit()
        {
            entries.extend(charges_for_fit(&loaded.catalog, fit).into_iter().map(|id| {
                MarketEntry::Item {
                    id,
                    depth: 1,
                    context: MarketContext::Group,
                }
            }));
        }
        entries
    }

    fn market_group(&self, loaded: &Loaded, id: i32, depth: usize, out: &mut Vec<MarketEntry>) {
        let Some(group) = loaded.catalog.market_groups.get(&id) else {
            return;
        };
        out.push(MarketEntry::Group {
            id,
            depth,
            name: group.name.clone().into(),
            icon: GroupIcon::Game(group.icon_id),
        });
        if !self.expanded_groups.contains(&id) {
            return;
        }
        for &child in &group.children {
            self.market_group(loaded, child, depth + 1, out);
        }
        out.extend(group.types.iter().map(|&t| MarketEntry::Item {
            id: t,
            depth: depth + 1,
            context: MarketContext::Group,
        }));
    }

    fn market(&mut self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = self.search_results(loaded).is_some();
        let entries = self.market_entries(loaded);
        let present: HashSet<MetaCategory> = entries
            .iter()
            .filter_map(|e| match e {
                MarketEntry::Item { id, .. } => loaded.catalog.get(*id),
                MarketEntry::Group { .. } => None,
            })
            .map(MetaCategory::of_item)
            .collect();
        // pyfa never leaves a list empty because of the buttons.
        self.meta_filter.show_something(&present);
        let filter = self.meta_filter;
        let rows: Vec<(Option<TypeId>, AnyElement)> = entries
            .into_iter()
            .filter_map(|entry| match entry {
                MarketEntry::Group {
                    id,
                    depth,
                    name,
                    icon,
                } => Some((None, self.group_row(loaded, id, depth, name, icon, cx))),
                MarketEntry::Item { id, depth, context } => {
                    let t = loaded.catalog.get(id)?;
                    filter
                        .allows(t)
                        .then(|| (Some(id), self.item_row(loaded, id, depth, context, cx)))
                }
            })
            .collect();
        if self.market_scroll_pending {
            self.market_scroll_pending = false;
            let target = self.market_highlight;
            if let Some(index) = rows
                .iter()
                .position(|row| row.0.is_some() && row.0 == target)
            {
                self.market_scroll.scroll_to_item(index);
            }
        }
        let empty_search = searching && rows.is_empty();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(div().p_1().child(self.search_input()))
            .child(
                v_flex()
                    .id("market-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.market_scroll)
                    .when(empty_search, |list| {
                        list.child(
                            div()
                                .p_2()
                                .text_color(cx.theme().muted_foreground)
                                .child("No matching items."),
                        )
                    })
                    .children(rows.into_iter().map(|row| row.1)),
            )
            .child(self.meta_buttons(&present, cx))
    }

    /// pyfa's meta group toggles under the market list. A click shows one category, Ctrl+click
    /// adds or removes one; categories the list does not hold are greyed out.
    fn meta_buttons(&self, present: &HashSet<MetaCategory>, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_1()
            .p_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .children(MetaCategory::ALL.into_iter().map(|category| {
                div().flex_1().child(
                    Button::new(SharedString::from(format!("meta-{}", category.label())))
                        .xsmall()
                        .w_full()
                        .label(category.label())
                        .toggled(self.meta_filter.is_on(category))
                        .selected(self.meta_filter.is_on(category))
                        .disabled(!present.contains(&category))
                        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                            let ctrl = event.modifiers().control;
                            this.meta_filter.click(category, ctrl);
                            cx.notify();
                        })),
                )
            }))
            .into_any_element()
    }

    fn group_row(
        &self,
        loaded: &Loaded,
        id: i32,
        depth: usize,
        name: SharedString,
        icon: GroupIcon,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.expanded_groups.contains(&id);
        h_flex()
            .id(SharedString::from(format!("group-{id}")))
            .gap_1()
            .h(px(22.))
            .pl(px(4. + INDENT * depth as f32))
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(
                div()
                    .w(px(10.))
                    .text_xs()
                    .child(if expanded { "▾" } else { "▸" }),
            )
            .map(|row| match icon {
                GroupIcon::Game(Some(icon_id)) => row.child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Id(icon_id),
                    18.,
                )),
                GroupIcon::Game(None) => row,
                GroupIcon::Lucide(name) => row.child(
                    div()
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(LucideIcon::new(name).size(px(14.))),
                ),
            })
            .child(name)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.expanded_groups.remove(&id) {
                    this.expanded_groups.insert(id);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// The Omega marker, when the preference shows it and the type needs Omega.
    fn omega_marker(&self, loaded: &Loaded, id: TypeId, size: f32) -> Option<AnyElement> {
        let omega = loaded.catalog.get(id).is_some_and(|t| t.omega);
        (omega && self.saved.preferences.general.omega_icons).then(|| {
            icon_box(&loaded.catalog, &loaded.icons, Icon::Id(OMEGA_ICON), size).into_any_element()
        })
    }

    fn item_row(
        &self,
        loaded: &Loaded,
        id: TypeId,
        depth: usize,
        context: MarketContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = loaded.catalog.name(id);
        let highlighted = self.market_highlight == Some(id);
        let price = self.prices.get(id).filter(|&p| p > 0.0);
        let weak = Self::weak(cx);
        h_flex()
            .id(SharedString::from(format!("item-{id}")))
            .gap_2()
            .h(px(26.))
            .pl(px(18. + INDENT * depth as f32))
            .when(highlighted, |row| row.bg(cx.theme().list_active))
            .hover(|s| s.bg(cx.theme().list_hover))
            .children(self.omega_marker(loaded, id, 16.))
            .child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(id),
                24.,
            ))
            .child(div().truncate().child(name))
            .when_some(self.loaded(), |row, loaded| {
                row.tooltip(move |window, cx| item_tooltip(&loaded, id, price, window, cx))
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.used_from_market(id);
                    this.market_double_click(id, event.modifiers(), cx);
                }
            }))
            .when_some(
                Dragged::of(loaded, DragSource::Market, Some(id)),
                |row, dragged| {
                    row.on_drag(dragged, move |dragged, _, _, cx| {
                        let _ = weak.update(cx, |g, _| g.used_from_market(id));
                        cx.new(|_| DragPreview(dragged.clone()))
                    })
                },
            )
            .context_menu({
                let this = Self::weak(cx);
                move |menu, _, cx| market_menu(menu, &this, id, context, cx)
            })
            .into_any_element()
    }

    /// Search ships and fits from the fittings browser's box; clearing it goes back.
    pub(super) fn search_fittings(&mut self, text: String, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            if matches!(self.ship_browser.stage, Stage::Search(_)) {
                self.ship_browser.back();
            }
        } else {
            self.ship_browser.go(Stage::Search(text));
        }
        cx.notify();
    }

    /// A new fit for a ship from the fittings browser, named the way pyfa's browser names it.
    fn new_fit_for_ship(&mut self, ship: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let mut fit = self.new_fit(format!("{} fit", loaded.catalog.name(ship)));
        fit.ship = Some(ship);
        fit.mode = loaded.catalog.modes_for(ship).first().copied();
        self.new_fit_defaults(&mut fit);
        self.ship_browser.go(Stage::Fits(ship));
        self.add_fit(fit, cx);
    }

    fn fittings(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let counts = ship_browser::fit_counts(&self.saved.fits);
        let stage = self.ship_browser.stage.clone();
        let mut races = Vec::new();
        let rows: Vec<AnyElement> = match &stage {
            Stage::Groups => loaded
                .catalog
                .ship_groups
                .iter()
                .filter_map(|(group, _)| {
                    let count = ship_browser::group_fit_count(&loaded.catalog, *group, &counts);
                    (!self.ship_browser.hide_empty || count > 0)
                        .then(|| Self::ship_group_row(loaded, *group, count, cx))
                })
                .collect(),
            Stage::Ships(group) => {
                let ships = loaded.catalog.ships_in(*group);
                let present: HashSet<Race> = ships
                    .iter()
                    .filter_map(|&s| loaded.catalog.get(s)?.race)
                    .collect();
                races = present.iter().copied().collect();
                races.sort();
                ships
                    .iter()
                    .filter_map(|&ship| {
                        let count = counts.get(&ship).copied().unwrap_or(0);
                        let race = loaded.catalog.get(ship).and_then(|t| t.race);
                        let shown = self.ship_browser.shows_race(race, &present)
                            && (!self.ship_browser.hide_empty || count > 0);
                        shown.then(|| self.ship_row(loaded, ship, count, cx))
                    })
                    .collect()
            }
            Stage::Fits(ship) => ship_browser::fits_of(&self.saved.fits, *ship)
                .into_iter()
                .map(|i| self.fit_row(loaded, i, cx))
                .collect(),
            Stage::Search(query) => {
                let ships = ship_browser::search_ships(&loaded.catalog, query);
                let fits = ship_browser::search_fits(&self.saved.fits, query);
                let mut rows: Vec<AnyElement> = ships
                    .into_iter()
                    .map(|ship| {
                        let count = counts.get(&ship).copied().unwrap_or(0);
                        self.ship_row(loaded, ship, count, cx)
                    })
                    .collect();
                rows.extend(fits.into_iter().map(|i| self.fit_row(loaded, i, cx)));
                rows
            }
            Stage::Recent => ship_browser::recent_fits(&self.saved.fits)
                .into_iter()
                .map(|i| self.fit_row(loaded, i, cx))
                .collect(),
        };
        let empty = match &stage {
            Stage::Groups | Stage::Ships(_) if self.ship_browser.hide_empty => "No fits yet.",
            Stage::Groups | Stage::Ships(_) => "Nothing here.",
            Stage::Fits(_) => "No fits for this ship yet.",
            Stage::Search(_) => "No matching results.",
            Stage::Recent => "No fits yet.",
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.fittings_toolbar(&stage, cx))
            .child(Self::fittings_title(loaded, &stage, cx))
            .child(
                v_flex()
                    .id("fittings-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .p_2()
                                .text_color(cx.theme().muted_foreground)
                                .child(empty),
                        )
                    })
                    .children(rows),
            )
            .when(!races.is_empty(), |pane| {
                pane.child(self.race_filter(&races, cx))
            })
    }

    /// pyfa's navigation panel: ship groups, back, recent fits, empty groups, new fit, and the
    /// search box.
    fn fittings_toolbar(&self, stage: &Stage, cx: &mut Context<Self>) -> impl IntoElement {
        let browser = &self.ship_browser;
        let browsing = matches!(stage, Stage::Groups | Stage::Ships(_));
        let ship = match stage {
            Stage::Fits(ship) => Some(*ship),
            _ => None,
        };
        let tool = |id: &'static str, icon: IconName, tip: &'static str| {
            Button::new(id).ghost().xsmall().icon(icon).tooltip(tip)
        };
        v_flex()
            .gap_1()
            .p_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_0p5()
                    .child(
                        tool("fittings-home", IconName::LayoutDashboard, "Ship groups").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.ship_browser.go(Stage::Groups);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        tool("fittings-back", IconName::ArrowLeft, "Back")
                            .disabled(!browser.can_go_back())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ship_browser.back();
                                cx.notify();
                            })),
                    )
                    .child(
                        tool(
                            "fittings-recent",
                            IconName::Calendar,
                            if *stage == Stage::Recent {
                                "Hide Recent Fits"
                            } else {
                                "Recent Fits"
                            },
                        )
                        .selected(*stage == Stage::Recent)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.ship_browser.stage == Stage::Recent {
                                this.ship_browser.go(Stage::Groups);
                            } else {
                                this.ship_browser.go(Stage::Recent);
                            }
                            cx.notify();
                        })),
                    )
                    .when(browsing, |bar| {
                        let hide = browser.hide_empty;
                        bar.child(
                            tool(
                                "fittings-empty",
                                if hide {
                                    IconName::EyeOff
                                } else {
                                    IconName::Eye
                                },
                                if hide {
                                    "Show empty ship groups"
                                } else {
                                    "Hide empty ship groups"
                                },
                            )
                            .selected(hide)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ship_browser.hide_empty = !this.ship_browser.hide_empty;
                                cx.notify();
                            })),
                        )
                    })
                    .when_some(ship, |bar, ship| {
                        bar.child(tool("fittings-new", IconName::Plus, "New fit").on_click(
                            cx.listener(move |this, _, _, cx| this.new_fit_for_ship(ship, cx)),
                        ))
                    }),
            )
            .child(self.fit_search_input())
    }

    /// Where the browser is: the group, ship, search or recent fits.
    fn fittings_title(loaded: &Loaded, stage: &Stage, cx: &mut Context<Self>) -> AnyElement {
        let title = match stage {
            Stage::Groups => "Ship groups".to_owned(),
            Stage::Ships(group) => loaded
                .catalog
                .group_names
                .get(group)
                .cloned()
                .unwrap_or_default(),
            Stage::Fits(ship) => loaded.catalog.name(*ship),
            Stage::Search(query) => format!("Search: {}", query.trim()),
            Stage::Recent => "Recent fits".to_owned(),
        };
        let row = h_flex()
            .id("fittings-title")
            .gap_2()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .border_b_1()
            .border_color(cx.theme().border);
        match stage {
            // pyfa's baseShip menu, on the ship whose fits are listed.
            Stage::Fits(ship) => {
                let ship = *ship;
                row.child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Type(ship),
                    18.,
                ))
                .child(title)
                .context_menu({
                    let this = Self::weak(cx);
                    move |menu, _, _| ship_menu(menu, &this, ship)
                })
                .into_any_element()
            }
            _ => row.child(title).into_any_element(),
        }
    }

    /// The race toggles under a group's ships, pyfa's race selector.
    fn race_filter(&self, races: &[Race], cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_wrap()
            .gap_1()
            .p_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .children(races.iter().map(|&race| {
                Button::new(SharedString::from(format!("race-{race:?}")))
                    .xsmall()
                    .label(race.label())
                    .toggled(self.ship_browser.races.contains(&race))
                    .selected(self.ship_browser.races.contains(&race))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ship_browser.toggle_race(race);
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }

    fn ship_group_row(
        loaded: &Loaded,
        group: i32,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = loaded
            .catalog
            .group_names
            .get(&group)
            .cloned()
            .unwrap_or_default();
        h_flex()
            .id(SharedString::from(format!("ship-group-{group}")))
            .gap_2()
            .px_2()
            .h(px(26.))
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(div().flex_1().truncate().child(name))
            .when(count > 0, |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(fits_label(count)),
                )
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.ship_browser.go(Stage::Ships(group));
                cx.notify();
            }))
            .into_any_element()
    }

    fn ship_row(
        &self,
        loaded: &Loaded,
        ship: TypeId,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tooltips = self.saved.preferences.general.ship_browser_tooltips;
        h_flex()
            .id(SharedString::from(format!("ship-{ship}")))
            .gap_2()
            .p_1()
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(ship),
                32.,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(loaded.catalog.name(ship)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(fits_label(count)),
                    ),
            )
            .children(self.omega_marker(loaded, ship, 16.))
            .when_some(self.loaded().filter(|_| tooltips), |row, loaded| {
                row.tooltip(move |window, cx| {
                    ship_tooltip(&loaded, ship, TipHead::Traits, window, cx)
                })
            })
            // A ship without fits gets one, as clicking it in pyfa starts one.
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if count > 0 {
                    this.ship_browser.go(Stage::Fits(ship));
                    cx.notify();
                } else {
                    this.new_fit_for_ship(ship, cx);
                }
            }))
            .context_menu({
                let this = Self::weak(cx);
                move |menu, _, _| ship_menu(menu, &this, ship)
            })
            .into_any_element()
    }

    fn fit_row(&self, loaded: &Loaded, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let fit = &self.saved.fits[i];
        let ship_name = fit
            .ship
            .map_or_else(String::new, |s| loaded.catalog.name(s));
        let notes = Self::fitting_tooltip(fit, &ship_name).map(|n| n.to_string());
        let tooltips = self.saved.preferences.general.ship_browser_tooltips;
        let subtitle = match ship_browser::fit_date(fit.modified) {
            Some(date) => format!("{ship_name}  {date}"),
            None => ship_name.clone(),
        };
        let open = self.saved.current == Some(i);
        h_flex()
            .id(SharedString::from(format!("fitting-{i}")))
            .gap_2()
            .p_1()
            .when(open, |row| row.bg(cx.theme().list_active))
            .hover(|s| s.bg(cx.theme().list_hover))
            .map(|row| match (fit.ship, self.loaded().filter(|_| tooltips)) {
                (Some(ship), Some(loaded)) => row.tooltip(move |window, cx| {
                    ship_tooltip(&loaded, ship, TipHead::Fit(notes.clone()), window, cx)
                }),
                _ => match notes {
                    Some(notes) => {
                        let tip: SharedString = notes.into();
                        row.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    }
                    None => row,
                },
            })
            .when_some(fit.ship, |row, ship| {
                row.child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Type(ship),
                    32.,
                ))
            })
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().truncate().child(fit.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    ),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_from_browser(i, cx);
            }))
            // A middle click opens the fit in a tab of its own, as in pyfa.
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| this.open_fit(i, cx)),
            )
            // The Projected and Command tabs take fits dragged from here.
            .on_drag(
                FitDrag {
                    index: i,
                    label: fit.name.clone().into(),
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .context_menu({
                let this = Self::weak(cx);
                let has_fit = self.saved.current.is_some();
                move |menu, _, _| fit_menu(menu, &this, i, has_fit)
            })
            .into_any_element()
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
        let free = stats.cargo_capacity.unwrap_or(0.0) - fit.cargo_volume(&loaded.catalog);
        if volume <= 0.0 || free <= 0.0 {
            return 0;
        }
        (free / volume).floor() as u32
    }
}

/// "No fits", "1 fit", "3 fits", as pyfa's ship rows count them.
fn fits_label(count: usize) -> String {
    match count {
        0 => "No fits".to_owned(),
        1 => "1 fit".to_owned(),
        n => format!("{n} fits"),
    }
}

/// Hover text as lines in a box that wraps.
fn text_tooltip(lines: Vec<String>, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        v_flex()
            .max_w(px(TOOLTIP_W))
            .children(lines.iter().map(|line| {
                if line.is_empty() {
                    div().h(px(8.))
                } else {
                    div().child(line.clone())
                }
            }))
    })
    .build(window, cx)
}

/// A market item's hover text: name, Omega, price and the start of its description.
fn item_tooltip(
    loaded: &Rc<Loaded>,
    id: TypeId,
    price: Option<f64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let mut lines = vec![loaded.catalog.name(id)];
    if loaded.catalog.get(id).is_some_and(|t| t.omega) {
        lines.push("Omega clones only".to_owned());
    }
    if let Some(price) = price {
        lines.push(format!("Price: {}", format::currency(price)));
    }
    let description =
        item_stats::hover_data(loaded, cx).and_then(|data| item_stats::description_text(&data, id));
    if let Some(description) = description {
        lines.push(String::new());
        let mut excerpt: String = description.chars().take(DESCRIPTION_MAX).collect();
        if excerpt.len() < description.len() {
            excerpt.push_str("...");
        }
        lines.extend(excerpt.lines().map(str::to_owned));
    }
    text_tooltip(lines, window, cx)
}

/// What a fittings browser tooltip starts with before the traits.
enum TipHead {
    /// Nothing: a ship row shows only the traits.
    Traits,
    /// A fit row: the ship, or the ship with the notes when there are any.
    Fit(Option<String>),
}

/// pyfa's ship and fit hover text: for a fit the ship and its notes, then the ship's traits.
fn ship_tooltip(
    loaded: &Rc<Loaded>,
    ship: TypeId,
    head: TipHead,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let rule = "─".repeat(20);
    let traits = item_stats::hover_data(loaded, cx)
        .and_then(|data| item_stats::traits_text(loaded, &data, ship));
    let mut text = match head {
        TipHead::Fit(Some(notes)) => notes,
        TipHead::Fit(None) => loaded.catalog.name(ship),
        TipHead::Traits => String::new(),
    };
    if let Some(traits) = traits {
        if !text.is_empty() {
            text.push('\n');
            text.push_str(&rule);
            text.push('\n');
        }
        text.push_str(&traits);
    }
    if text.is_empty() {
        text = loaded.catalog.name(ship);
    }
    text_tooltip(text.lines().map(str::to_owned).collect(), window, cx)
}

/// pyfa's baseShip menu, with the browser's New Fit.
fn ship_menu(menu: PopupMenu, this: &WeakEntity<Gyfa>, ship: TypeId) -> PopupMenu {
    let new_fit = this.clone();
    menu.item(item_stats::stats_entry(
        this,
        "Ship Basic",
        item_stats::StatsOf::Type(ship),
    ))
    .item(entry(IconName::Plus, "New Fit").on_click(move |_, _, cx| {
        Gyfa::app_update(&new_fit, cx, |g, cx| g.new_fit_for_ship(ship, cx));
    }))
}

/// pyfa's fit row menu, plus the copy, rename and delete buttons pyfa has on the row.
fn fit_menu(menu: PopupMenu, this: &WeakEntity<Gyfa>, i: usize, has_fit: bool) -> PopupMenu {
    let on = |f: fn(&mut Gyfa, usize, &mut Context<Gyfa>)| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| f(g, i, cx));
        }
    };
    let link = |link: Link| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| g.link(link, &[i], cx));
        }
    };
    let mut menu =
        menu.item(entry(IconName::FolderOpen, "Open in new tab").on_click(on(Gyfa::open_fit)));
    if has_fit {
        menu = menu
            .item(entry(IconName::Globe, "Project onto Active Fit").on_click(link(Link::Projected)))
            .item(entry(IconName::Network, "Add Command Booster").on_click(link(Link::Command)));
    }
    let rename = this.clone();
    menu.separator()
        .item(entry(IconName::Copy, "Copy").on_click(on(Gyfa::copy_fit)))
        .item(
            entry(IconName::Replace, "Rename").on_click(move |_, window, cx| {
                let _ = rename.update(cx, |g, cx| {
                    g.open_from_browser(i, cx);
                    g.start_rename(window, cx);
                });
            }),
        )
        .item(entry(IconName::Delete, "Delete").on_click(on(Gyfa::delete_fit)))
}

/// pyfa's marketItemGroup / marketItemMisc menu.
fn market_menu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    id: TypeId,
    context: MarketContext,
    cx: &App,
) -> PopupMenu {
    let Some(g) = this.upgrade() else { return menu };
    let g = g.read(cx);
    let Some(loaded) = g.loaded() else {
        return menu;
    };
    let Some(t) = loaded.catalog.get(id) else {
        return menu;
    };
    let category = t.category_label();
    let has_fit = g.fit().is_some_and(|f| f.ship.is_some());
    let structure = g
        .fit()
        .and_then(|f| f.ship)
        .and_then(|s| loaded.catalog.get(s))
        .is_some_and(crate::data::ItemType::is_structure);
    let on = |f: fn(&mut Gyfa, TypeId, &mut Window, &mut Context<Gyfa>)| {
        let this = this.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| f(g, id, window, cx));
        }
    };
    // Adding an item from here counts as using it, as pyfa's commands remember it.
    let use_with = |f: fn(&mut Gyfa, TypeId, &mut Window, &mut Context<Gyfa>)| {
        let this = this.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| {
                g.used_from_market(id);
                f(g, id, window, cx);
            });
        }
    };
    let apply = |action: Action| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| {
                g.used_from_market(id);
                g.apply(action.clone(), cx);
            });
        }
    };

    let mut menu = menu.item(item_stats::stats_entry(
        this,
        category,
        item_stats::StatsOf::Type(id),
    ));
    // Ice products are found by search but sit outside the market tree, so there is no group.
    let listed = t
        .market_group
        .is_some_and(|g| loaded.catalog.market_groups.contains_key(&g));
    if context == MarketContext::Search && listed {
        menu = menu.item(
            entry(IconName::Search, format!("{category} Market Group"))
                .on_click(on(super::Gyfa::jump_to_market)),
        );
    }
    if !has_fit {
        return menu.item(
            entry(IconName::Plus, format!("Add {category} to Fit"))
                .on_click(use_with(|g, id, _, cx| g.add_item(id, cx))),
        );
    }
    if let Kind::Module(slot) = t.kind
        && slot != Slot::Subsystem
        && g.saved.preferences.context_menus.module_fill
    {
        menu = menu.item(
            entry(IconName::Plus, "Fill With Module").on_click(apply(Action::FillRack(slot, id))),
        );
    }
    if t.kind == Kind::Drone {
        let amount = g.drone_stack_size(&loaded, id);
        if amount >= 1 {
            let label = if amount == 1 {
                format!("Add {} to Drone Bay", t.name)
            } else {
                format!("Add {} to Drone Bay (x{amount})", t.name)
            };
            menu = menu
                .item(entry(IconName::Plus, label).on_click(apply(Action::AddDrones(id, amount))));
        }
    }
    let to_cargo = |count: u32| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| {
                g.used_from_market(id);
                g.add_to_cargo(id, count, cx);
            });
        }
    };
    // pyfa offers structures only charges for their cargo.
    if !structure || t.kind == Kind::Charge {
        menu = menu
            .item(entry(IconName::Inbox, format!("Add {category} to Cargo")).on_click(to_cargo(1)));
    }
    if goes_to_cargo(t.kind) && g.saved.preferences.context_menus.cargo_fill {
        menu = menu.item(
            entry(IconName::Inbox, format!("Fill Cargo With {category}"))
                .on_click(use_with(|g, id, _, cx| g.fill_cargo_with(id, cx))),
        );
    }
    if t.kind == Kind::Charge {
        let is_probe = t
            .market_group
            .and_then(|m| loaded.catalog.market_groups.get(&m))
            .is_some_and(|m| m.name == "Scan Probes");
        let batch = if is_probe { PROBE_BATCH } else { AMMO_BATCH };
        menu = menu.item(
            entry(
                IconName::Inbox,
                format!("Add {category} to Cargo (x{batch})"),
            )
            .on_click(to_cargo(batch)),
        );
    }
    let shown = super::hub::preferences(cx).context_menus;
    if shown.ammo_pattern && t.damage.iter().any(|&d| d > 0.0) {
        let profile = crate::fit::DamageProfile::Pattern {
            name: crate::damage_pattern::SELECTED_AMMO.to_owned(),
            amounts: t.damage,
        };
        let this = this.clone();
        menu = menu.item(
            entry(
                IconName::ChartPie,
                format!("Set {category} as Damage Pattern"),
            )
            .on_click(move |_, _, cx| {
                Gyfa::app_update(&this, cx, |g, cx| {
                    g.apply(Action::SetDamageProfile(profile.clone()), cx);
                });
            }),
        );
    }
    if shown.project && !structure && loaded.catalog.projection.projectable.contains(&id) {
        menu = menu.item(
            entry(IconName::Globe, format!("Project {category} onto Fit")).on_click(on(
                |g, id, _, cx| {
                    g.project_item(id, cx);
                    if g.additions_tab != AdditionsTab::Projected {
                        g.additions_tab = AdditionsTab::Projected;
                        g.clear_selection();
                        cx.notify();
                    }
                },
            )),
        );
    }
    menu
}
