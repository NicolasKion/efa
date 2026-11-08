//! pyfa's item stats window (gui/itemStats.py and gui/builtinItemStatsViews): a window of its own
//! per item, with traits, description, attributes, variations side by side, requirements, what a
//! skill unlocks, effects and what affects the item.
//!
//! Each window follows its fit through the hub, so the numbers stay current while the fit is
//! edited. Holding Shift when opening one replaces the last stats window, as in pyfa.

mod affected;
mod attributes;
mod compare;
mod format;
mod menu;
mod mutator;
mod tabs;
#[cfg(test)]
mod tests;
mod text;
mod tree;
mod window;

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex, menu::ContextMenuExt, text::TextView, v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Div, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, img, prelude::FluentBuilder, px,
};

pub use menu::{StatsOf, stats_entry};
pub use window::open;

use super::{
    Hub, Loaded, item_data,
    widgets::{entry, tree_row},
};
use crate::{
    data::{Kind, TypeId},
    engine::{FitItem, Inspection},
    fit::Fit,
    icons::Icon,
    item_data::{ItemData, TypeInfo},
};
use tree::{Node, Openness};
use window::StatsWindows;

const ROW_H: f32 = 22.;
const ICON: f32 = 16.;
/// Width of the value columns of the Attributes tab.
const VALUE_W: f32 = 150.;

/// What a stats window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A type on its own, with its unmodified attributes, as the market shows it.
    Type(TypeId),
    /// An item of a fit, with its attributes after bonuses.
    Fitted { fit: usize, item: FitItem },
}

/// Everything a new stats window starts from. It never reads the window that opened it.
pub struct Request {
    pub target: Target,
    /// The fit as the opening window has it, which may be newer than the hub's copy.
    pub fit: Option<Fit>,
}

/// A ship's bonuses as plain text, the way pyfa's browser tooltips show them.
pub fn traits_text(loaded: &Loaded, data: &ItemData, ship: TypeId) -> Option<String> {
    let traits = data.type_info(ship)?.traits.as_ref()?;
    Some(text::plain(&text::traits_html(&loaded.catalog, traits)))
}

/// An item's description as plain text.
pub fn description_text(data: &ItemData, id: TypeId) -> Option<String> {
    let description = data.type_info(id)?.description.as_ref()?;
    Some(text::plain(&text::description_html(description)))
}

/// The window's tabs, in pyfa's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Traits,
    Mutations,
    Description,
    Attributes,
    Compare,
    Requirements,
    Dependents,
    Effects,
    AffectedBy,
    Properties,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Traits => "Traits",
            Self::Mutations => "Mutations",
            Self::Description => "Description",
            Self::Attributes => "Attributes",
            Self::Compare => "Compare",
            Self::Requirements => "Requirements",
            Self::Dependents => "Dependents",
            Self::Effects => "Effects",
            Self::AffectedBy => "Affected by",
            Self::Properties => "Properties",
        }
    }
}

/// Which tree a click opens or closes a node of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeId {
    Attributes,
    AffectedBy,
    Requirements,
    Dependents,
}

pub struct ItemStats {
    loaded: Rc<Loaded>,
    hub: Entity<Hub>,
    target: Target,
    type_id: TypeId,
    /// The fit the item is part of, as last seen.
    fit: Option<Fit>,
    /// The saved fits boosting or projecting onto it, as last seen.
    surroundings: Vec<Rc<Fit>>,
    /// The characters the pilot was built from; a new list means the pilot is out of date.
    characters: Option<Rc<crate::character::Characters>>,
    /// Who flies the fit, for skill-dependent values.
    pilot: Rc<crate::engine::Pilot>,
    /// The hub generation the fit was taken from.
    synced: u64,
    /// The hub's revision of the fit and its engine generation when the fit was taken, so the
    /// item is only inspected again when either moved on.
    fit_revision: Option<u64>,
    engine_generation: u64,
    /// The item's attributes after bonuses; kept when the item leaves the fit, like pyfa does.
    inspection: Option<Inspection>,
    tab: Option<Tab>,
    raw: bool,
    attr_open: Openness,
    affected_view: affected::View,
    affected_open: Openness,
    req_open: Openness,
    dep_open: Openness,
    compare: compare::State,
    /// The Mutations tab's sliders, built once it is first shown.
    mutator: Option<mutator::Panel>,
    /// The tree the open tab shows, with what it was built for; dropped when the item's
    /// numbers change.
    tree_cache: Option<(TreeKey, Rc<[Node]>)>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// What a cached tree was built for: the tab, raw values or not, and the Affected by tab's view.
type TreeKey = (Tab, bool, affected::View);

impl ItemStats {
    fn new(
        loaded: Rc<Loaded>,
        hub: Entity<Hub>,
        request: Request,
        type_id: TypeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (synced, engine_generation) = {
            let hub = hub.read(cx);
            (hub.generation, hub.engine_generation)
        };
        let surroundings = request.fit.as_ref().map_or_default(|f| {
            crate::engine::surrounding_fits(f, &hub.read(cx).saved.fits)
                .cloned()
                .collect()
        });
        let subscriptions = vec![
            cx.observe(&hub, |this, _, cx| this.pull(cx)),
            item_data::observe_data(cx),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    let handle = window.window_handle();
                    cx.default_global::<StatsWindows>().active = Some(handle);
                }
            }),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            loaded,
            hub,
            target: request.target,
            type_id,
            surroundings,
            fit: request.fit,
            synced,
            fit_revision: None,
            engine_generation,
            characters: None,
            pilot: Rc::new(crate::engine::Pilot::default()),
            inspection: None,
            tab: None,
            raw: false,
            attr_open: Openness::new(true),
            affected_view: affected::View::default(),
            affected_open: Openness::new(false),
            req_open: Openness::new(true),
            dep_open: Openness::new(false),
            compare: compare::State::default(),
            mutator: None,
            tree_cache: None,
            focus,
            _subscriptions: subscriptions,
        };
        this.recompute();
        this
    }

    /// Take in fits other windows changed, following ours when fits before it are deleted.
    fn pull(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.read(cx);
        // Characters change without the fits changing, so they are checked on their own.
        let characters_changed = self
            .characters
            .as_ref()
            .is_none_or(|c| !Rc::ptr_eq(c, &hub.characters));
        if characters_changed {
            self.characters = Some(Rc::clone(&hub.characters));
            if let Some(fit) = &self.fit {
                self.pilot = Rc::new(hub.characters.pilot(fit.character, &self.loaded.catalog));
            }
        }
        if hub.generation == self.synced {
            if characters_changed {
                self.recompute();
                self.tree_cache = None;
                cx.notify();
            }
            return;
        }
        let synced = self.synced;
        self.synced = hub.generation;
        let mut changed = characters_changed;
        if let Target::Fitted { fit, .. } = &mut self.target {
            let mut gone = false;
            for &(generation, deleted) in &hub.deletions {
                if generation <= synced || gone {
                    continue;
                }
                match deleted.cmp(fit) {
                    std::cmp::Ordering::Less => *fit -= 1,
                    std::cmp::Ordering::Equal => gone = true,
                    std::cmp::Ordering::Greater => {}
                }
            }
            if gone {
                // The fit is gone; the window keeps what it last showed.
                self.target = Target::Type(self.type_id);
                changed = true;
            } else if let Some(f) = hub.saved.fits.get(*fit) {
                // The item's numbers follow the fit, the fits around it and the engine's data;
                // when none of them moved on there is nothing to inspect again.
                let revision = hub.fit_revision(*fit);
                let surroundings: Vec<Rc<Fit>> =
                    crate::engine::surrounding_fits(f, &hub.saved.fits)
                        .cloned()
                        .collect();
                let same = self.fit_revision == Some(revision)
                    && self.engine_generation == hub.engine_generation
                    && surroundings.len() == self.surroundings.len()
                    && surroundings
                        .iter()
                        .zip(&self.surroundings)
                        .all(|(a, b)| Rc::ptr_eq(a, b));
                if !same {
                    self.fit_revision = Some(revision);
                    self.engine_generation = hub.engine_generation;
                    self.pilot = Rc::new(hub.characters.pilot(f.character, &self.loaded.catalog));
                    self.surroundings = surroundings;
                    self.fit = Some((**f).clone());
                    changed = true;
                }
            }
        }
        if changed {
            self.recompute();
            self.tree_cache = None;
            cx.notify();
        }
    }

    fn recompute(&mut self) {
        let Target::Fitted { item, .. } = self.target else {
            return;
        };
        let Some(fit) = &self.fit else { return };
        // Something else in that place now: keep showing the old item, as pyfa does.
        if item.type_in(fit, &self.pilot.implants) != Some(self.type_id) {
            return;
        }
        if let Some(inspection) =
            self.loaded
                .engine
                .inspect_among(fit, &self.surroundings, &self.pilot, item)
        {
            self.inspection = Some(inspection);
            self.tree_cache = None;
        }
    }

    fn fitted(&self) -> bool {
        self.inspection.is_some()
    }

    fn fit_index(&self) -> Option<usize> {
        match self.target {
            Target::Fitted { fit, .. } => Some(fit),
            Target::Type(_) => None,
        }
    }

    fn tabs(&self, data: &ItemData) -> Vec<Tab> {
        let info = data.type_info(self.type_id);
        let kind = self.loaded.catalog.get(self.type_id).map(|t| t.kind);
        let mut tabs = Vec::new();
        if info.is_some_and(|i| i.traits.is_some()) {
            tabs.push(Tab::Traits);
        }
        if self.mutation().is_some() {
            tabs.push(Tab::Mutations);
        }
        if info.is_some_and(|i| i.description.is_some()) {
            tabs.push(Tab::Description);
        }
        tabs.push(Tab::Attributes);
        if self.loaded.catalog.variations_of(self.type_id).len() > 1 {
            tabs.push(Tab::Compare);
        }
        tabs.push(Tab::Requirements);
        if kind == Some(Kind::Skill) {
            tabs.push(Tab::Dependents);
        }
        tabs.push(Tab::Effects);
        if self.fitted() {
            tabs.push(Tab::AffectedBy);
        }
        tabs.push(Tab::Properties);
        tabs
    }

    fn openness_mut(&mut self, tree: TreeId) -> &mut Openness {
        match tree {
            TreeId::Attributes => &mut self.attr_open,
            TreeId::AffectedBy => &mut self.affected_open,
            TreeId::Requirements => &mut self.req_open,
            TreeId::Dependents => &mut self.dep_open,
        }
    }

    /// An icon of the given size, or room for it while it downloads.
    fn icon(&self, icon: Option<Icon>, size: f32) -> Div {
        let slot = div().flex_none().size(px(size));
        let Some(icon) = icon else { return slot };
        // One that is still downloading shows once the hub redraws every window for it.
        match self.loaded.icons.path(&self.loaded.catalog, icon) {
            Some(path) => slot.child(img(path).size(px(size))),
            None => slot,
        }
    }

    /// A mutated item's attributes, which the abyssal type mostly leaves to its base item: the
    /// base item's, with the abyssal type's own on top where the base has none, as refine and
    /// pyfa merge them.
    fn mutated_info(&self, data: &ItemData) -> Option<TypeInfo> {
        let base = data.type_info(self.mutation()?.base)?;
        let mut merged = data.type_info(self.type_id).cloned().unwrap_or_default();
        for &(attr, value) in &base.attrs {
            match merged.attrs.binary_search_by_key(&attr, |&(a, _)| a) {
                Ok(at) => merged.attrs[at].1 = value,
                Err(at) => merged.attrs.insert(at, (attr, value)),
            }
        }
        Some(merged)
    }

    /// Attribute values as the Attributes tab lists them.
    fn values(&self, info: Option<&TypeInfo>) -> Vec<attributes::Value> {
        match &self.inspection {
            Some(inspection) => inspection
                .attrs
                .iter()
                .map(|a| attributes::Value {
                    id: a.id,
                    current: a.modified,
                    base: a.base,
                })
                .collect(),
            None => info.map_or_default(|i| {
                i.attrs
                    .iter()
                    .map(|&(id, v)| attributes::Value {
                        id,
                        current: v,
                        base: v,
                    })
                    .collect()
            }),
        }
    }

    fn header(&self, cx: &App) -> impl IntoElement {
        let catalog = &self.loaded.catalog;
        let t = catalog.get(self.type_id);
        let price = self.hub.read(cx).prices.get(self.type_id);
        let sub = t.map_or_else(String::new, |t| {
            let group = catalog
                .group_names
                .get(&t.group_id)
                .cloned()
                .unwrap_or_default();
            format!("{} \u{2022} {group}", t.category_label())
        });
        h_flex()
            .gap_3()
            .p_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.icon(Some(Icon::Type(self.type_id)), 40.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_base()
                            .truncate()
                            .child(catalog.name(self.type_id)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(sub),
                    ),
            )
            .children(price.map(|p| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} ISK", format::isk(p)))
            }))
    }

    fn tab_bar(tabs: &[Tab], current: Tab, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_1()
            .p_1()
            .flex_wrap()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .children(tabs.iter().map(|&tab| {
                let selected = tab == current;
                div()
                    .id(tab.label())
                    .px_3()
                    .py_0p5()
                    .rounded(px(4.))
                    .when(selected, |t| {
                        t.bg(theme.tab_active)
                            .text_color(theme.tab_active_foreground)
                    })
                    .when(!selected, |t| t.text_color(theme.tab_foreground))
                    .child(tab.label())
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                            this.tab = Some(tab);
                            cx.notify();
                        }),
                    )
            }))
    }

    /// A tree, one line per visible node; lines with children open and close on click, and
    /// lines that stand for an item offer its stats on right-click.
    fn tree_view(
        &self,
        which: TreeId,
        nodes: &[Node],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let openness = match which {
            TreeId::Attributes => &self.attr_open,
            TreeId::AffectedBy => &self.affected_open,
            TreeId::Requirements => &self.req_open,
            TreeId::Dependents => &self.dep_open,
        };
        let theme = cx.theme().clone();
        let weak = cx.weak_entity();
        let rows: Vec<AnyElement> = tree::lines(nodes, openness)
            .into_iter()
            .enumerate()
            .map(|(n, line)| {
                let node = line.node;
                let has_children = !node.children.is_empty();
                let icon = node
                    .icon
                    .map(|icon| self.icon(Some(icon), ICON).into_any_element());
                let row = tree_row(
                    SharedString::from(format!("{which:?}-{n}")),
                    line.depth,
                    has_children.then_some(line.open),
                    icon,
                    node.label.clone(),
                    false,
                    cx,
                )
                .when(n % 2 == 1, |r| r.bg(theme.table_even))
                .children(node.cells.iter().map(|cell| {
                    div()
                        .flex_none()
                        .w(px(VALUE_W))
                        .truncate()
                        .child(cell.clone())
                }));
                let row = if has_children {
                    let key = node.key.clone();
                    row.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.openness_mut(which).toggle(&key);
                        cx.notify();
                    }))
                } else {
                    row
                };
                match node.link.clone() {
                    Some((label, target)) => {
                        let weak = weak.clone();
                        row.context_menu(move |menu, _, _| {
                            let weak = weak.clone();
                            menu.item(entry(IconName::Info, format!("{label} Stats")).on_click(
                                move |event: &ClickEvent, window, cx| {
                                    let Some(this) = weak.upgrade() else { return };
                                    let reuse = event.modifiers().shift;
                                    // Read first: Shift may close this very window.
                                    let (loaded, hub, fit) = {
                                        let this = this.read(cx);
                                        (
                                            Rc::clone(&this.loaded),
                                            this.hub.clone(),
                                            this.fit.clone(),
                                        )
                                    };
                                    open(&loaded, &hub, Request { target, fit }, reuse, window, cx);
                                },
                            ))
                        })
                        .into_any_element()
                    }
                    None => row.into_any_element(),
                }
            })
            .collect();
        v_flex()
            .id(SharedString::from(format!("{which:?}-tree")))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
    }

    /// A row of buttons under a tab, right-aligned as in pyfa.
    fn button_row(cx: &App) -> Div {
        h_flex()
            .justify_end()
            .gap_1()
            .p_1()
            .border_t_1()
            .border_color(cx.theme().border)
    }

    fn html_tab(id: &'static str, html: String) -> AnyElement {
        let plain = text::plain(&html);
        div()
            .id(SharedString::from(format!("{id}-scroll")))
            .flex_1()
            .min_h_0()
            .p_2()
            .overflow_y_scroll()
            .child(TextView::html(id, html).selectable(true))
            .context_menu(move |menu, _, _| {
                let plain = plain.clone();
                menu.item(entry(IconName::Copy, "Copy").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(plain.clone()));
                }))
            })
            .into_any_element()
    }

    /// Save the Attributes tab as CSV, as pyfa's Export Item Stats does.
    fn export(&self, cx: &mut App) {
        let Some(Ok(data)) = item_data::data(cx) else {
            return;
        };
        let (info, values) = self.attribute_values(&data);
        let csv = attributes::csv(&data, info.as_ref(), &values);
        let file_name = format!(
            "{} ({}).csv",
            self.loaded.catalog.name(self.type_id),
            self.type_id
        );
        export_csv(&file_name, csv, cx);
    }
}

impl Render for ItemStats {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                let m = event.keystroke.modifiers;
                let plain = !(m.control || m.alt || m.shift || m.platform);
                if event.keystroke.key == "escape" && plain {
                    window.remove_window();
                }
            }))
            .child(self.header(cx));
        let data = match item_data::data(cx) {
            None => return root.child(Self::empty("Loading item data…", cx)),
            Some(Err(e)) => {
                return root.child(
                    div()
                        .p_3()
                        .text_color(cx.theme().danger)
                        .child(format!("Could not load item data: {e}")),
                );
            }
            Some(Ok(data)) => data,
        };
        let tabs = self.tabs(&data);
        let tab = self.tab.filter(|t| tabs.contains(t)).unwrap_or(tabs[0]);
        let info = data.type_info(self.type_id);
        let content = match tab {
            Tab::Traits => {
                let html = info
                    .and_then(|i| i.traits.as_ref())
                    .map_or_default(|t| text::traits_html(&self.loaded.catalog, t));
                Self::html_tab("traits", html)
            }
            Tab::Description => {
                let html = info
                    .and_then(|i| i.description.as_deref())
                    .map_or_default(text::description_html);
                Self::html_tab("description", html)
            }
            Tab::Mutations => {
                self.sync_mutator(window, cx);
                self.mutations_tab(&data, window, cx)
            }
            Tab::Attributes => self.attributes_tab(&data, cx),
            Tab::Compare => self.compare_tab(&data, cx),
            Tab::Requirements => self.requirements_tab(&data, cx),
            Tab::Dependents => self.dependents_tab(&data, cx),
            Tab::Effects => self.effects_tab(&data, cx),
            Tab::AffectedBy => self.affected_tab(&data, cx),
            Tab::Properties => self.properties_tab(&data, cx),
        };
        root.child(Self::tab_bar(&tabs, tab, cx))
            .child(v_flex().flex_1().min_h_0().child(content))
    }
}

/// Ask where to save the CSV and write it there.
fn export_csv(file_name: &str, csv: String, cx: &mut App) {
    let dir = crate::paths::home_dir();
    let chosen = cx.prompt_for_new_path(&dir, Some(file_name));
    cx.background_spawn(async move {
        if let Ok(Ok(Some(path))) = chosen.await
            && let Err(e) = std::fs::write(&path, csv)
        {
            eprintln!("{}: {e}", path.display());
        }
    })
    .detach();
}
