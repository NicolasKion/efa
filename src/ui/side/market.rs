//! The market tab: the group tree, the results pane with its filters, and the item rows.

use std::{collections::HashSet, ops::Range, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as LucideIcon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::ContextMenuExt,
    resizable::{resizable_panel, v_resizable},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, InteractiveElement, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, ParentElement, ScrollStrategy, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, uniform_list,
};

use super::menus::market_menu;
use crate::market::charges::{ChargeMenu, compatible_charges, structure_charges};
use crate::ui::BrowserTab;
use crate::ui::tooltips::item_tooltip;
use crate::ui::{
    Efa, Loaded,
    drag::{DragPreview, DragSource, Dragged},
    widgets::{count, icon_box, list_row, tree_row},
};
use crate::{
    data::{Catalog, Slot, TypeId},
    icons::Icon,
    market::{
        Filtered, Listing, MetaCategory, ResultRow, Section, ShipRoom, TextFilter, charges_for_fit,
        filter_results, group_items, jargon::Jargon,
    },
};

/// Search results shown at most, to keep the list quick to build.
const SEARCH_LIMIT: usize = 500;
/// Height of every results row, items and headings alike, as the list needs.
const RESULT_ROW_H: f32 = 26.;
/// Height of the market results pane until the user drags the divider.
const DEFAULT_RESULTS: f32 = 420.;
/// Least height either half of the market keeps.
const MIN_PANE: f32 = 80.;

/// pyfa's market tree entries that are not market groups, kept apart by negative IDs.
const RECENT_GROUP: i32 = -1;
const CHARGES_GROUP: i32 = -2;
const MODULE_CHARGES_GROUP: i32 = -3;

/// The Omega clone icon pyfa marks hulls and items with.
const OMEGA_ICON: i32 = 25874;

/// Whether an item is listed in its own market group or away from it, as pyfa tells its
/// marketItemGroup and marketItemMisc menus apart: only the latter jump to the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarketContext {
    Group,
    Search,
}

/// What the results pane's filters ran over and with, to know when to run them again.
pub(in crate::ui) struct FilterKey {
    /// The listed sections, kept so the next ones are told apart by address.
    sections: Option<Rc<Vec<Section>>>,
    text: String,
    meta: crate::market::MetaFilter,
    /// The calculation the ship filter used, when it is on.
    ship: Option<u64>,
    /// The current hull's rig size, which rigs are kept to.
    rig_size: Option<f64>,
}

impl FilterKey {
    fn same(&self, other: &Self) -> bool {
        let sections = match (&self.sections, &other.sections) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        sections
            && self.text == other.text
            && self.meta == other.meta
            && self.ship == other.ship
            && self.rig_size == other.rig_size
    }
}

/// What the results pane's list was built from, to know when to build it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum ResultsKey {
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

impl Efa {
    /// Remember an item taken from the market, for pyfa's Recently Used Items.
    pub(in crate::ui) fn used_from_market(&mut self, id: TypeId) {
        crate::market::remember(&mut self.saved.settings.recent_items, id);
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
                vec![Section::untitled(crate::market::search(
                    catalog,
                    text,
                    SEARCH_LIMIT,
                ))]
            }
            ResultsKey::Listing(Listing::Group(group)) => {
                vec![Section::untitled(group_items(catalog, *group))]
            }
            ResultsKey::Listing(Listing::Recent) => vec![Section::untitled(
                self.saved
                    .settings
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
    pub(super) fn market(&mut self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let jump = std::mem::take(&mut self.market_scroll_pending);
        let results = self.results_pane(loaded, jump, cx);
        let tree = self.market_tree(loaded, jump, cx);
        let results_height = self
            .saved
            .settings
            .market_results
            .unwrap_or(DEFAULT_RESULTS);
        let weak = cx.weak_entity();
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
                                let Some(&height) = state.read(cx).sizes().last() else {
                                    return;
                                };
                                let _ = weak.update(cx, |g, _| {
                                    g.saved.settings.market_results = Some(f32::from(height));
                                    g.save();
                                });
                            })
                            .child(
                                resizable_panel()
                                    .size_range(px(MIN_PANE)..px(f32::MAX))
                                    .child(tree),
                            )
                            // The results keep their height as the window resizes.
                            .child(
                                resizable_panel()
                                    .size(px(results_height))
                                    .size_range(px(MIN_PANE)..px(f32::MAX))
                                    .fixed()
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
        let icon = match icon {
            GroupIcon::Game(Some(icon_id)) => Some(
                icon_box(&loaded.catalog, &loaded.icons, Icon::Id(icon_id), 18.).into_any_element(),
            ),
            GroupIcon::Game(None) => None,
            GroupIcon::Lucide(name) => Some(
                div()
                    .size(px(18.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .child(LucideIcon::new(name).size(px(14.)))
                    .into_any_element(),
            ),
        };
        tree_row(
            SharedString::from(format!("group-{id}")),
            depth,
            branch.then_some(expanded),
            icon,
            name,
            selected,
            cx,
        )
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
        let filter_key = FilterKey {
            sections: results.as_ref().map(|(_, sections)| Rc::clone(sections)),
            text: self.results_filter_text.clone(),
            meta: self.meta_filter,
            ship: room.is_some().then_some(self.calc_generation),
            rig_size: self
                .fit()
                .and_then(|f| catalog.get(f.ship?))
                .and_then(|s| s.rig_size),
        };
        let filtered = match &self.filtered_cache {
            Some((last, filtered)) if last.same(&filter_key) => Rc::clone(filtered),
            _ => {
                let text = TextFilter::new(&self.results_filter_text, Jargon::global());
                let filtered = Rc::new(results.as_ref().map_or_default(|(_, sections)| {
                    filter_results(
                        catalog,
                        sections,
                        text.as_ref(),
                        room.as_ref(),
                        filter_key.rig_size,
                        self.meta_filter,
                    )
                }));
                self.market_order = Rc::new(
                    filtered
                        .rows
                        .iter()
                        .filter_map(|row| match row {
                            ResultRow::Item(id) => Some(*id),
                            ResultRow::Heading(_) => None,
                        })
                        .collect(),
                );
                // Only what is listed stays selected, so nothing hidden is dragged along.
                let listed: HashSet<TypeId> = self.market_order.iter().copied().collect();
                self.market_selection.retain(|id| listed.contains(id));
                self.filtered_cache = Some((filter_key, Rc::clone(&filtered)));
                filtered
            }
        };
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
            let rows = Rc::clone(&filtered);
            let group = match key {
                Some(ResultsKey::Listing(Listing::Group(group))) => Some(group),
                _ => None,
            };
            uniform_list(
                "market-results",
                rows.rows.len(),
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    let Some(loaded) = this.loaded() else {
                        return Vec::new();
                    };
                    rows.rows[range]
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
                format!(
                    "The filters hide all {}.",
                    count(filtered.total, "item", "items")
                )
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
                            this.meta_filter.clear();
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    /// Meta group toggles over the results. A click shows only that one (a second click shows
    /// everything again), Ctrl+click adds or removes one; with none pressed everything shows. Categories the results do not hold
    /// are greyed out, unless pressed, so they can still be switched off.
    fn meta_buttons(&self, present: &HashSet<MetaCategory>, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_wrap()
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
                        .disabled(!present.contains(&category) && !self.meta_filter.is_on(category))
                        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                            let m = event.modifiers();
                            this.meta_filter.click(category, !(m.control || m.platform));
                            cx.notify();
                        })),
                )
            }))
            .into_any_element()
    }

    /// The Omega marker, when the preference shows it and the type needs Omega.
    pub(super) fn omega_marker(
        &self,
        loaded: &Loaded,
        id: TypeId,
        size: f32,
    ) -> Option<AnyElement> {
        let omega = loaded.catalog.get(id).is_some_and(|t| t.omega);
        (omega && self.saved.settings.preferences.general.omega_icons).then(|| {
            icon_box(&loaded.catalog, &loaded.icons, Icon::Id(OMEGA_ICON), size).into_any_element()
        })
    }

    fn item_row(
        &self,
        loaded: &Rc<Loaded>,
        id: TypeId,
        context: MarketContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = loaded.catalog.name(id);
        let selected = self.market_selection.contains(&id);
        let highlighted = selected || self.market_highlight == Some(id);
        // Dragging a selected item takes the rest of the selection along.
        let more: Vec<TypeId> = if selected {
            self.market_selection
                .items()
                .iter()
                .copied()
                .filter(|&other| other != id)
                .collect()
        } else {
            Vec::new()
        };
        let price = self.prices.get(id).filter(|&p| p > 0.0);
        let weak = cx.weak_entity();
        list_row(SharedString::from(format!("item-{id}")), highlighted, cx)
            .w_full()
            .gap_2()
            .h(px(RESULT_ROW_H))
            .children(self.omega_marker(loaded, id, 16.))
            .child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(id),
                24.,
            ))
            .child(div().flex_1().min_w_0().truncate().child(name))
            .when_some(self.loaded(), |row, loaded| {
                row.tooltip(move |window, cx| item_tooltip(&loaded, id, price, window, cx))
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.used_from_market(id);
                    this.market_double_click(id, event.modifiers(), cx);
                } else {
                    this.select_market_item(id, event.modifiers());
                    cx.notify();
                }
            }))
            // A right-click on an unselected item makes it the selection, as list views do.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    if this.market_selection.right_click(id) {
                        cx.notify();
                    }
                }),
            )
            .when_some(
                Dragged::of(loaded, DragSource::Market, Some(id)).map(|d| d.with_more(more)),
                |row, dragged| {
                    row.on_drag(dragged, move |dragged, _, _, cx| {
                        let _ = weak.update(cx, |g, _| g.used_from_market(id));
                        cx.new(|_| DragPreview(dragged.clone()))
                    })
                },
            )
            .context_menu({
                let this = cx.weak_entity();
                move |menu, window, cx| market_menu(menu, &this, id, context, window, cx)
            })
            .into_any_element()
    }

    /// Click selection in the market results: a plain click picks one item, Ctrl adds or
    /// removes one, Shift takes the range from the last clicked item.
    fn select_market_item(&mut self, id: TypeId, modifiers: Modifiers) {
        let order = Rc::clone(&self.market_order);
        self.market_selection.click(id, modifiers, &order);
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
        (shown, total) if shown == total => Some(count(total, "item", "items")),
        (shown, total) => Some(format!("{shown} of {total}")),
    }
}

/// A heading over a run of results, such as a charge family.
fn heading_row(title: &str, cx: &App) -> AnyElement {
    div()
        .w_full()
        .h(px(RESULT_ROW_H))
        .flex()
        .items_end()
        .px_2()
        .pb_0p5()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(div().min_w_0().truncate().child(title.to_owned()))
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
impl Efa {
    /// Show an item in the market browser: clear the search, open its groups, scroll to it.
    pub(in crate::ui) fn jump_to_market(
        &mut self,
        id: TypeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        self.show_in_market(window, cx);
        let path = loaded.catalog.market_path(id);
        if let Some(&group) = path.last() {
            self.listing = Some(crate::market::Listing::Group(group));
        }
        self.expanded_groups.extend(path);
        // The item would not show with its meta button off, or with the ship filter on.
        if let Some(t) = loaded.catalog.get(id) {
            self.meta_filter
                .enable(crate::market::MetaCategory::of_item(t));
        }
        self.fits_ship_only = false;
        self.market_highlight = Some(id);
        self.market_scroll_pending = true;
        cx.notify();
    }

    /// List every charge a module can load in the market's results pane.
    pub(in crate::ui) fn show_charges_for(
        &mut self,
        module: TypeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_in_market(window, cx);
        self.listing = Some(crate::market::Listing::ModuleCharges(module));
        self.market_highlight = None;
        self.market_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// Bring up the market tab with its search and the results filter cleared, so what is
    /// listed next shows in full.
    fn show_in_market(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.browser_tab = BrowserTab::Market;
        self.side_stats = false;
        self.search_text.clear();
        self.search_debounce = None;
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.results_filter_text.clear();
        self.results_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
    }
}
