//! The left side: market browser and fittings browser.

use std::{collections::HashSet, ops::Range, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as LucideIcon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu},
    resizable::{resizable_panel, v_resizable},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, ClickEvent, Context, InteractiveElement, IntoElement,
    MouseButton, MouseUpEvent, ParentElement, ScrollStrategy, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
    uniform_list,
};

use super::{
    Action, AdditionsTab, BrowserTab, Gyfa, Loaded,
    additions::{FitDrag, Link, goes_to_cargo},
    fitting::{
        ChargeMenu, DragPreview, DragSource, Dragged, compatible_charges, structure_charges,
    },
    item_stats,
    stats::format,
    widgets::{entry, icon_box},
};
use crate::{
    data::{Catalog, Kind, Slot, TypeId},
    icons::Icon,
    market::{
        Filtered, Listing, MetaCategory, ResultRow, Section, ShipRoom, TextFilter, charges_for_fit,
        filter_results, group_items, jargon::Jargon,
    },
    ship_browser::{self, Race, Stage},
};

/// Search results shown at most, to keep the list quick to build.
const SEARCH_LIMIT: usize = 500;
const INDENT: f32 = 14.;
/// Height of every results row, items and headings alike, as the list needs.
const RESULT_ROW_H: f32 = 26.;
/// Height of the market tree over the results until the user drags the divider.
const DEFAULT_SPLIT: f32 = 260.;
/// Least height either half of the market keeps.
const MIN_PANE: f32 = 80.;
/// Drones that can be in space at once without skills, the floor for a stack's size.
const BASE_ACTIVE_DRONES: u32 = 5;
/// Charges "Add to Cargo" puts in at once, and the smaller batch for scan probes.
const AMMO_BATCH: u32 = 1000;
const PROBE_BATCH: u32 = 8;
/// pyfa's market tree entries that are not market groups, kept apart by negative IDs.
const RECENT_GROUP: i32 = -1;
const CHARGES_GROUP: i32 = -2;
const MODULE_CHARGES_GROUP: i32 = -3;
/// The Omega clone icon pyfa marks hulls and items with.
const OMEGA_ICON: i32 = 25874;
/// Widest a hover text gets before it wraps.
const TOOLTIP_W: f32 = 380.;
/// Longest description excerpt a market tooltip shows.
const DESCRIPTION_MAX: usize = 400;

/// Whether an item is listed in its own market group or away from it, as pyfa tells its
/// marketItemGroup and marketItemMisc menus apart: only the latter jump to the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketContext {
    Group,
    Search,
}

/// What the results pane's list was built from, to know when to build it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResultsKey {
    Search(String),
    Listing(Listing),
}

/// A line of the market group tree: a market group or one of pyfa's virtual entries.
struct TreeEntry {
    id: i32,
    depth: usize,
    name: SharedString,
    icon: GroupIcon,
    /// Has subgroups to open.
    branch: bool,
    listing: Listing,
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

    /// The market's results before the pane's filters: the search's matches while there is a
    /// search, otherwise whatever the tree picked. Searches and group lists are cached.
    fn market_results(&mut self, loaded: &Loaded) -> Option<(ResultsKey, Rc<Vec<Section>>)> {
        let text = self.search_text.trim();
        // pyfa goes back to the tree while there is nothing but wildcards.
        let key = if text.replace(['*', '?'], "").is_empty() {
            ResultsKey::Listing(self.listing?)
        } else {
            ResultsKey::Search(text.to_owned())
        };
        // The fit and the recent items change under these, and they are quick to list.
        let cached = !matches!(
            key,
            ResultsKey::Listing(Listing::Recent | Listing::FitCharges)
        );
        if cached
            && let Some((last, sections)) = &self.results_cache
            && *last == key
        {
            return Some((key, Rc::clone(sections)));
        }
        let catalog = &loaded.catalog;
        let sections = Rc::new(match &key {
            ResultsKey::Search(text) => {
                vec![Section::untitled(catalog.search(text, SEARCH_LIMIT))]
            }
            ResultsKey::Listing(Listing::Group(group)) => {
                vec![Section::untitled(group_items(catalog, *group))]
            }
            ResultsKey::Listing(Listing::Recent) => vec![Section::untitled(
                self.saved
                    .recent_items
                    .iter()
                    .copied()
                    .filter(|&id| catalog.get(id).is_some())
                    .collect(),
            )],
            ResultsKey::Listing(Listing::FitCharges) => vec![Section::untitled(
                self.fit()
                    .map_or_default(|fit| charges_for_fit(catalog, fit)),
            )],
            ResultsKey::Listing(Listing::ModuleCharges(module)) => {
                module_charge_sections(catalog, *module)
            }
        });
        if cached {
            self.results_cache = Some((key.clone(), Rc::clone(&sections)));
        }
        Some((key, sections))
    }

    /// What the current fit has room for, for the "Fits current ship" filter.
    fn ship_room<'a>(&self, catalog: &'a Catalog) -> Option<ShipRoom<'a>> {
        let fit = self.fit()?;
        let ship = catalog.get(fit.ship?)?;
        let stats = self.stats.as_ref()?;
        let total = |slots: crate::engine::Slots| slots.total.unwrap_or(0);
        let holders = [Slot::High, Slot::Mid, Slot::Low]
            .into_iter()
            .flat_map(|slot| fit.rack(slot).iter().flatten())
            .filter_map(|m| catalog.get(m.type_id))
            .filter(|m| !m.charge_groups.is_empty())
            .collect();
        Some(ShipRoom {
            ship,
            slots: [
                Slot::High,
                Slot::Mid,
                Slot::Low,
                Slot::Rig,
                Slot::Subsystem,
                Slot::Service,
            ]
            .map(|slot| total(stats.slots(slot))),
            turrets: total(stats.turret),
            launchers: total(stats.launcher),
            drones: stats.drone_bay.output.is_some_and(|v| v > 0.0),
            fighters: total(stats.fighter_tubes) > 0,
            holders,
        })
    }

    /// The market tab: the search box, then the group tree over the results pane, split by a
    /// divider the user can drag.
    fn market(&mut self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let jump = std::mem::take(&mut self.market_scroll_pending);
        let results = self.results_pane(loaded, jump, cx);
        let tree = self.market_tree(loaded, jump, cx);
        let split = self.saved.market_split.unwrap_or(DEFAULT_SPLIT);
        let weak = Self::weak(cx);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(div().p_1().child(self.search_input()))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_resizable("market-split")
                            .on_resize(move |state, _, cx| {
                                let Some(&height) = state.read(cx).sizes().first() else {
                                    return;
                                };
                                let _ = weak.update(cx, |g, _| {
                                    g.saved.market_split = Some(f32::from(height));
                                    g.save();
                                });
                            })
                            .child(
                                resizable_panel()
                                    .size(px(split))
                                    .size_range(px(MIN_PANE)..px(f32::MAX))
                                    .child(tree),
                            )
                            .child(
                                resizable_panel()
                                    .size_range(px(MIN_PANE)..px(f32::MAX))
                                    .child(results),
                            ),
                    ),
            )
    }

    fn market_tree(&self, loaded: &Loaded, jump: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut entries = Vec::new();
        for &root in &loaded.catalog.market_roots {
            self.tree_group(loaded, root, 0, &mut entries);
        }
        entries.push(TreeEntry {
            id: RECENT_GROUP,
            depth: 0,
            name: "Recently Used Items".into(),
            icon: GroupIcon::Lucide(IconName::Calendar),
            branch: false,
            listing: Listing::Recent,
        });
        entries.push(TreeEntry {
            id: CHARGES_GROUP,
            depth: 0,
            name: "Charges For Active Fit".into(),
            icon: GroupIcon::Lucide(IconName::Asterisk),
            branch: false,
            listing: Listing::FitCharges,
        });
        if let Some(listing @ Listing::ModuleCharges(module)) = self.listing {
            entries.push(TreeEntry {
                id: MODULE_CHARGES_GROUP,
                depth: 0,
                name: format!("Charges for {}", loaded.catalog.name(module)).into(),
                icon: GroupIcon::Lucide(IconName::BatteryCharging),
                branch: false,
                listing,
            });
        }
        if jump && let Some(index) = entries.iter().position(|e| Some(e.listing) == self.listing) {
            self.market_tree_scroll.scroll_to_item(index);
        }
        v_flex()
            .id("market-tree")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.market_tree_scroll)
            .children(
                entries
                    .into_iter()
                    .map(|entry| self.group_row(loaded, entry, cx)),
            )
            .into_any_element()
    }

    fn tree_group(&self, loaded: &Loaded, id: i32, depth: usize, out: &mut Vec<TreeEntry>) {
        let Some(group) = loaded.catalog.market_groups.get(&id) else {
            return;
        };
        out.push(TreeEntry {
            id,
            depth,
            name: group.name.clone().into(),
            icon: GroupIcon::Game(group.icon_id),
            branch: !group.children.is_empty(),
            listing: Listing::Group(id),
        });
        if self.expanded_groups.contains(&id) {
            for &child in &group.children {
                self.tree_group(loaded, child, depth + 1, out);
            }
        }
    }

    fn group_row(&self, loaded: &Loaded, entry: TreeEntry, cx: &mut Context<Self>) -> AnyElement {
        let TreeEntry {
            id,
            depth,
            name,
            icon,
            branch,
            listing,
        } = entry;
        let expanded = self.expanded_groups.contains(&id);
        let selected = self.listing == Some(listing);
        h_flex()
            .id(SharedString::from(format!("group-{id}")))
            .gap_1()
            .h(px(22.))
            .flex_none()
            .pl(px(4. + INDENT * depth as f32))
            .when(selected, |row| row.bg(cx.theme().list_active))
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(
                div()
                    .w(px(10.))
                    .flex_none()
                    .text_xs()
                    .when(branch, |d| d.child(if expanded { "▾" } else { "▸" })),
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
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .child(LucideIcon::new(name).size(px(14.))),
                ),
            })
            .child(div().truncate().child(name))
            // A click lists the entry's items and opens it; a second click closes it again.
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                if branch && (!selected || !this.expanded_groups.remove(&id)) {
                    this.expanded_groups.insert(id);
                }
                this.select_listing(listing, window, cx);
            }))
            .into_any_element()
    }

    /// List something in the results pane, leaving the search so it shows.
    fn select_listing(&mut self, listing: Listing, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_text.is_empty() || self.search_debounce.is_some() {
            self.search_text.clear();
            self.search_debounce = None;
            self.search
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        if self.listing != Some(listing) {
            self.listing = Some(listing);
            self.market_highlight = None;
            self.market_scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// The results pane: what it lists, its filter bar, and the list itself.
    fn results_pane(&mut self, loaded: &Loaded, jump: bool, cx: &mut Context<Self>) -> AnyElement {
        let catalog = &loaded.catalog;
        let results = self.market_results(loaded);
        let room = self.ship_room(catalog);
        let can_filter_ship = room.is_some();
        let room = room.filter(|_| self.fits_ship_only);
        let text = TextFilter::new(&self.results_filter_text, Jargon::global());
        let mut meta = self.meta_filter;
        let filtered = results.as_ref().map_or_default(|(_, sections)| {
            filter_results(catalog, sections, text.as_ref(), room.as_ref(), &mut meta)
        });
        self.meta_filter = meta;
        let key = results.map(|(key, _)| key);
        if jump
            && let Some(target) = self.market_highlight
            && let Some(index) = filtered
                .rows
                .iter()
                .position(|row| *row == ResultRow::Item(target))
        {
            self.market_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
        let title = results_title(catalog, key.as_ref());
        let count = count_label(&filtered);
        let body = if filtered.rows.is_empty() {
            self.results_empty(key.as_ref(), &filtered, cx)
        } else {
            let rows = Rc::new(filtered.rows);
            let group = match key {
                Some(ResultsKey::Listing(Listing::Group(group))) => Some(group),
                _ => None,
            };
            uniform_list(
                "market-results",
                rows.len(),
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    let Some(loaded) = this.loaded() else {
                        return Vec::new();
                    };
                    rows[range]
                        .iter()
                        .map(|row| match row {
                            ResultRow::Heading(title) => heading_row(title, cx),
                            ResultRow::Item(id) => {
                                let own = loaded.catalog.get(*id).and_then(|t| t.market_group);
                                let context = if group.is_some() && own == group {
                                    MarketContext::Group
                                } else {
                                    MarketContext::Search
                                };
                                this.item_row(&loaded, *id, context, cx)
                            }
                        })
                        .collect()
                }),
            )
            .track_scroll(&self.market_scroll)
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .border_b_1()
                    .border_color(border)
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .children(count.map(|c| div().flex_none().child(c))),
            )
            .child(
                h_flex()
                    .gap_1()
                    .px_1()
                    .pt_1()
                    .child(div().flex_1().min_w_0().child(self.results_filter_input()))
                    .child(
                        Button::new("market-fits-ship")
                            .ghost()
                            .xsmall()
                            .icon(LucideIcon::new(AssetIcon::Rocket))
                            .tooltip(if can_filter_ship {
                                "Fits current ship: hide what this fit cannot use"
                            } else {
                                "Fits current ship: needs a fit with a ship"
                            })
                            .selected(self.fits_ship_only && can_filter_ship)
                            .disabled(!can_filter_ship)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fits_ship_only = !this.fits_ship_only;
                                cx.notify();
                            })),
                    ),
            )
            .child(self.meta_buttons(&filtered.present, cx))
            .child(body)
            .into_any_element()
    }

    /// What the results pane says when it has nothing to show.
    fn results_empty(
        &self,
        key: Option<&ResultsKey>,
        filtered: &Filtered,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message = match key {
            _ if filtered.total > 0 => {
                let n = filtered.total;
                let items = if n == 1 { "item" } else { "items" };
                format!("The filters hide all {n} {items}.")
            }
            None => "Pick a market group, or search the market.".to_owned(),
            Some(ResultsKey::Search(_)) => "No matching items.".to_owned(),
            Some(ResultsKey::Listing(Listing::Group(_))) => "Nothing in this group.".to_owned(),
            Some(ResultsKey::Listing(Listing::Recent)) => {
                "Items you add from the market show up here.".to_owned()
            }
            Some(ResultsKey::Listing(Listing::FitCharges)) => {
                "The fit has nothing that takes charges.".to_owned()
            }
            Some(ResultsKey::Listing(Listing::ModuleCharges(_))) => {
                "This module takes no charges.".to_owned()
            }
        };
        let filters_on = !self.results_filter_text.trim().is_empty() || self.fits_ship_only;
        v_flex()
            .flex_1()
            .gap_2()
            .p_2()
            .items_start()
            .text_color(cx.theme().muted_foreground)
            .child(message)
            .when(filtered.total > 0 && filters_on, |pane| {
                pane.child(
                    Button::new("market-clear-filters")
                        .xsmall()
                        .label("Clear filters")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.results_filter_text.clear();
                            this.results_filter
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            this.fits_ship_only = false;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    /// pyfa's meta group toggles, over the results. A click shows one category, Ctrl+click
    /// adds or removes one; categories the results do not hold are greyed out.
    fn meta_buttons(&self, present: &HashSet<MetaCategory>, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_1()
            .p_1()
            .border_b_1()
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
            .h(px(RESULT_ROW_H))
            .px_1()
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
            Stage::Groups => {
                // Ships first, then structures in a section of their own.
                let shown = |structures: bool| {
                    loaded
                        .catalog
                        .ship_groups
                        .iter()
                        .filter(move |(_, types)| {
                            types
                                .first()
                                .and_then(|&t| loaded.catalog.get(t))
                                .is_some_and(|t| t.is_structure() == structures)
                        })
                        .filter_map(|(group, _)| {
                            let count =
                                ship_browser::group_fit_count(&loaded.catalog, *group, &counts);
                            (!self.ship_browser.hide_empty || count > 0).then_some((*group, count))
                        })
                        .collect::<Vec<_>>()
                };
                let (ships, structures) = (shown(false), shown(true));
                let mut rows = Vec::new();
                for (title, groups) in [("Ships", ships), ("Structures", structures)] {
                    if groups.is_empty() {
                        continue;
                    }
                    rows.push(section_header(title, cx));
                    rows.extend(
                        groups
                            .into_iter()
                            .map(|(group, count)| Self::ship_group_row(loaded, group, count, cx)),
                    );
                }
                rows
            }
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
                pane.child(self.race_filter(loaded, &races, cx))
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
            Stage::Ships(group) => row
                .children(crate::brackets::ship_bracket(*group).map(|b| {
                    icon_box(&loaded.catalog, &loaded.icons, Icon::Bracket(b), 16.)
                }))
                .child(title)
                .into_any_element(),
            _ => row.child(title).into_any_element(),
        }
    }

    /// The race toggles under a group's ships, pyfa's race selector.
    /// The race filter under a group's ships: one faction logo per race, the name on hover.
    fn race_filter(&self, loaded: &Loaded, races: &[Race], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let buttons: Vec<AnyElement> = races
            .iter()
            .map(|&race| {
                let on = self.ship_browser.races.contains(&race);
                let label = race.label();
                let button = div()
                    .id(SharedString::from(format!("race-{race:?}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(30.))
                    .min_w(px(30.))
                    .rounded(px(5.))
                    .border_1()
                    .when(on, |d| d.bg(theme.secondary).border_color(theme.primary))
                    .when(!on, |d| {
                        d.border_color(gpui_kit::transparent_black())
                            .opacity(0.6)
                            .hover(|s| s.opacity(1.).bg(theme.secondary))
                    })
                    .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.ship_browser.toggle_race(race);
                        cx.notify();
                    }));
                match race.logo_corp() {
                    Some(corp) => button
                        .child(icon_box(
                            &loaded.catalog,
                            &loaded.icons,
                            Icon::CorpLogo(corp),
                            24.,
                        ))
                        .into_any_element(),
                    None => button.px_2().text_xs().child(label).into_any_element(),
                }
            })
            .collect();
        h_flex()
            .flex_wrap()
            .gap_1()
            .p_1()
            .border_t_1()
            .border_color(theme.border)
            .children(buttons)
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
            .child(
                div().w(px(16.)).flex_none().children(
                    crate::brackets::ship_bracket(group).map(|b| {
                        icon_box(&loaded.catalog, &loaded.icons, Icon::Bracket(b), 16.)
                    }),
                ),
            )
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

/// What the results pane shows, for its title.
fn results_title(catalog: &Catalog, key: Option<&ResultsKey>) -> String {
    match key {
        None => "Items".to_owned(),
        Some(ResultsKey::Search(text)) => format!("Search: {text}"),
        Some(ResultsKey::Listing(Listing::Group(group))) => catalog
            .market_groups
            .get(group)
            .map_or_else(String::new, |g| g.name.clone()),
        Some(ResultsKey::Listing(Listing::Recent)) => "Recently Used Items".to_owned(),
        Some(ResultsKey::Listing(Listing::FitCharges)) => "Charges For Active Fit".to_owned(),
        Some(ResultsKey::Listing(Listing::ModuleCharges(module))) => {
            format!("Charges for {}", catalog.name(*module))
        }
    }
}

/// "12 items", or "3 of 12" while filters hide some.
fn count_label(filtered: &Filtered) -> Option<String> {
    match (filtered.count, filtered.total) {
        (_, 0) => None,
        (1, 1) => Some("1 item".to_owned()),
        (shown, total) if shown == total => Some(format!("{total} items")),
        (shown, total) => Some(format!("{shown} of {total}")),
    }
}

/// A heading over a run of results, such as a charge family.
fn heading_row(title: &str, cx: &App) -> AnyElement {
    div()
        .h(px(RESULT_ROW_H))
        .flex()
        .items_end()
        .px_2()
        .pb_0p5()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(title.to_owned())
        .into_any_element()
}

/// Every charge a module loads, grouped the way its Charge menu groups them: turret ammo by
/// family from long range to short, missiles by damage type, anything else in one list.
fn module_charge_sections(catalog: &Catalog, module: TypeId) -> Vec<Section> {
    let Some(m) = catalog.get(module) else {
        return Vec::new();
    };
    let charges = compatible_charges(catalog, module);
    match structure_charges(catalog, m, &charges) {
        ChargeMenu::Turret(families) => {
            let (grades, singles): (Vec<_>, Vec<_>) =
                families.into_iter().partition(|f| f.len() > 1);
            let mut sections: Vec<Section> = grades
                .into_iter()
                .map(|family| Section {
                    title: Some(catalog.name(family[0])),
                    items: family,
                })
                .collect();
            let singles: Vec<TypeId> = singles.into_iter().flatten().collect();
            if !singles.is_empty() {
                sections.push(Section {
                    title: Some("Other charges".to_owned()),
                    items: singles,
                });
            }
            sections
        }
        ChargeMenu::Missile(by_type) => by_type
            .into_iter()
            .map(|(label, items)| Section {
                title: Some(label.to_owned()),
                items,
            })
            .collect(),
        ChargeMenu::Flat(list) => vec![Section::untitled(list)],
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

/// A band over a run of rows in the fittings browser, like the fitting view's rack labels.
fn section_header(title: &'static str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .mt_2()
        .px_2()
        .py_1()
        .bg(theme.secondary)
        .border_t_1()
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(title)
        .into_any_element()
}
