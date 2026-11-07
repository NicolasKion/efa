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
#[cfg(test)]
mod tests;
mod text;
mod tree;

use std::{cell::Cell, collections::HashSet, rc::Rc, time::Duration};

use gpui_kit::component::{
    ActiveTheme, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::ContextMenuExt,
    text::TextView,
    v_flex,
};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, Bounds, ClickEvent, ClipboardItem, Context, Div,
    Entity, FocusHandle, Global, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, TitlebarOptions,
    WeakEntity, Window, WindowBounds, WindowOptions, div, img, prelude::FluentBuilder, px, size,
};

pub use menu::{StatsOf, stats_entry};

use super::{Hub, Loaded, widgets::entry};
use crate::{
    data::{Kind, TypeId},
    engine::{FitItem, Inspection},
    fit::Fit,
    icons::Icon,
    item_data::{ItemData, TypeInfo},
};
use tree::{Node, Openness};

/// pyfa's stats window size on GTK, where tabs take more room.
const WINDOW_W: f32 = 640.;
const WINDOW_H: f32 = 600.;
/// How far each new stats window sits from the previous one.
const CASCADE: f32 = 30.;
/// How often a window checks for icons that finished downloading.
const ICON_POLL: Duration = Duration::from_millis(500);
/// Checks before a window stops waiting for icons that may never come.
const ICON_POLLS: u32 = 40;
const ROW_H: f32 = 22.;
const ICON: f32 = 16.;
const INDENT: f32 = 16.;
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

/// The extra data, loaded once for all stats windows the first time one opens.
enum DataState {
    Loading,
    Ready(Rc<ItemData>),
    Failed(SharedString),
}

struct ItemDataGlobal(DataState);

impl Global for ItemDataGlobal {}

/// Start loading the stats window's data unless it is loaded or on its way.
fn ensure_data(loaded: &Rc<Loaded>, cx: &mut App) {
    if cx.has_global::<ItemDataGlobal>() {
        return;
    }
    cx.set_global(ItemDataGlobal(DataState::Loading));
    let source = loaded.source.clone();
    let wanted: HashSet<TypeId> = loaded.catalog.types.keys().copied().collect();
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { source.item_data(&wanted) })
            .await;
        cx.update(|cx| {
            let state = match result {
                Ok(data) => DataState::Ready(Rc::new(data)),
                Err(e) => DataState::Failed(e.into()),
            };
            cx.set_global(ItemDataGlobal(state));
        });
    })
    .detach();
}

/// The stats window's data for hover text, such as descriptions and traits. Starts loading it
/// the first time; None until it is in.
pub fn hover_data(loaded: &Rc<Loaded>, cx: &mut App) -> Option<Rc<ItemData>> {
    ensure_data(loaded, cx);
    match &cx.try_global::<ItemDataGlobal>()?.0 {
        DataState::Ready(data) => Some(Rc::clone(data)),
        DataState::Loading | DataState::Failed(_) => None,
    }
}

/// The stats window's data as far as it is loaded: None while loading.
pub fn data(cx: &App) -> Option<Result<Rc<ItemData>, SharedString>> {
    match &cx.try_global::<ItemDataGlobal>()?.0 {
        DataState::Loading => None,
        DataState::Ready(data) => Some(Ok(Rc::clone(data))),
        DataState::Failed(e) => Some(Err(e.clone())),
    }
}

/// Redraw a view once the stats window's data is in.
pub fn observe_data<V: 'static>(cx: &mut Context<V>) -> Subscription {
    cx.observe_global::<ItemDataGlobal>(|_, cx| cx.notify())
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

/// The open stats windows, for Shift to replace the last one used.
#[derive(Default)]
struct StatsWindows {
    open: Vec<AnyWindowHandle>,
    active: Option<AnyWindowHandle>,
    /// How many windows deep the cascade is.
    counter: u32,
}

impl Global for StatsWindows {}

impl StatsWindows {
    /// The window Shift replaces: the one last focused, else the newest still open.
    fn last(&mut self, cx: &App) -> Option<AnyWindowHandle> {
        let alive = cx.windows();
        self.open.retain(|w| alive.contains(w));
        self.active
            .filter(|w| self.open.contains(w))
            .or_else(|| self.open.last().copied())
    }
}

/// Open a stats window, or with `reuse` put it where the last one was and close that one.
pub fn open(
    loaded: &Rc<Loaded>,
    hub: &Entity<Hub>,
    request: Request,
    reuse: bool,
    window: &mut Window,
    cx: &mut App,
) {
    ensure_data(loaded, cx);
    let type_id = match request.target {
        Target::Type(id) => Some(id),
        Target::Fitted { item, .. } => request.fit.as_ref().and_then(|f| {
            // The pilot's implants only matter for an implant, which the fit lists itself
            // unless it uses the character's.
            let pilot = hub.read(cx).characters.pilot(f.character, &loaded.catalog);
            item.type_in(f, &pilot.implants)
        }),
    };
    let Some(type_id) = type_id else { return };

    let mut windows = cx
        .try_global::<StatsWindows>()
        .map_or_default(|w| StatsWindows {
            open: w.open.clone(),
            active: w.active,
            counter: w.counter,
        });
    let reused = if reuse {
        windows.last(cx).and_then(|last| {
            if last == window.window_handle() {
                let bounds = window.window_bounds();
                window.remove_window();
                Some(bounds)
            } else {
                last.update(cx, |_, window, _| {
                    let bounds = window.window_bounds();
                    window.remove_window();
                    bounds
                })
                .ok()
            }
        })
    } else {
        None
    };
    let bounds = reused.unwrap_or_else(|| {
        let parent = window.bounds();
        windows.counter += 1;
        let step = CASCADE * windows.counter as f32;
        let fits = step + WINDOW_W < f32::from(parent.size.width)
            && step + WINDOW_H < f32::from(parent.size.height);
        if !fits {
            windows.counter = 1;
        }
        let step = px(CASCADE * windows.counter as f32);
        WindowBounds::Windowed(Bounds {
            origin: gpui_kit::point(parent.origin.x + step, parent.origin.y + step),
            size: size(px(WINDOW_W), px(WINDOW_H)),
        })
    });

    let title = format!("efa: {} stats", loaded.catalog.name(type_id));
    let options = WindowOptions {
        window_bounds: Some(bounds),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        app_id: Some("efa".into()),
        ..Default::default()
    };
    let (loaded, hub) = (Rc::clone(loaded), hub.clone());
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ItemStats::new(loaded, hub, request, type_id, window, cx))
    });
    match opened {
        Ok((handle, _)) => {
            windows.open.push(handle);
            windows.active = Some(handle);
            cx.set_global(windows);
        }
        Err(e) => eprintln!("opening the stats window: {e}"),
    }
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
    /// The characters the pilot was built from; a new list means the pilot is out of date.
    characters: Option<Rc<crate::character::Characters>>,
    /// Who flies the fit, for skill-dependent values.
    pilot: Rc<crate::engine::Pilot>,
    /// The hub generation the fit was taken from.
    synced: u64,
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
    focus: FocusHandle,
    /// An icon was not on disk yet while drawing.
    icon_missing: Cell<bool>,
    icon_polls: u32,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl ItemStats {
    fn new(
        loaded: Rc<Loaded>,
        hub: Entity<Hub>,
        request: Request,
        type_id: TypeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let synced = hub.read(cx).generation;
        let subscriptions = vec![
            cx.observe(&hub, |this, _, cx| this.pull(cx)),
            cx.observe_global::<ItemDataGlobal>(|_, cx| cx.notify()),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    let handle = window.window_handle();
                    cx.default_global::<StatsWindows>().active = Some(handle);
                }
            }),
        ];
        let poll = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(ICON_POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if this.icon_missing.take() && this.icon_polls < ICON_POLLS {
                        this.icon_polls += 1;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            loaded,
            hub,
            target: request.target,
            type_id,
            fit: request.fit,
            synced,
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
            focus,
            icon_missing: Cell::new(false),
            icon_polls: 0,
            _subscriptions: subscriptions,
            _tasks: vec![poll],
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
                cx.notify();
            }
            return;
        }
        if let Target::Fitted { fit, .. } = &mut self.target {
            let mut gone = false;
            for &(generation, deleted) in &hub.deletions {
                if generation <= self.synced || gone {
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
            } else if let Some(f) = hub.saved.fits.get(*fit) {
                self.pilot = Rc::new(hub.characters.pilot(f.character, &self.loaded.catalog));
                self.fit = Some(f.clone());
            }
        }
        self.synced = hub.generation;
        self.recompute();
        cx.notify();
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
        if let Some(inspection) = self.loaded.engine.inspect(fit, &self.pilot, item) {
            self.inspection = Some(inspection);
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

    fn data(cx: &App) -> Option<Result<Rc<ItemData>, SharedString>> {
        match &cx.try_global::<ItemDataGlobal>()?.0 {
            DataState::Loading => None,
            DataState::Ready(data) => Some(Ok(Rc::clone(data))),
            DataState::Failed(e) => Some(Err(e.clone())),
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
        if let Some(path) = self.loaded.icons.path(&self.loaded.catalog, icon) {
            slot.child(img(path).size(px(size)))
        } else {
            self.icon_missing.set(true);
            slot
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
        let weak = cx.entity().downgrade();
        let rows: Vec<AnyElement> = tree::lines(nodes, openness)
            .into_iter()
            .enumerate()
            .map(|(n, line)| {
                let node = line.node;
                let has_children = !node.children.is_empty();
                let chevron = if has_children {
                    let icon = if line.open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    };
                    div().flex_none().size(px(ICON)).child(
                        gpui_kit::component::Icon::new(icon)
                            .size(px(12.))
                            .text_color(theme.muted_foreground),
                    )
                } else {
                    div().flex_none().size(px(ICON))
                };
                let row = h_flex()
                    .id(SharedString::from(format!("{which:?}-{n}")))
                    .h(px(ROW_H))
                    .px_1()
                    .gap_1()
                    .when(n % 2 == 1, |r| r.bg(theme.table_even))
                    .hover(|s| s.bg(theme.table_hover))
                    .child(div().flex_none().w(px(INDENT * line.depth as f32)))
                    .child(chevron)
                    .when_some(node.icon, |r, icon| r.child(self.icon(Some(icon), ICON)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(node.label.clone()),
                    )
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

    fn attributes_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let catalog = &self.loaded.catalog;
        let merged = self.mutated_info(data);
        let info = merged.as_ref().or_else(|| data.type_info(self.type_id));
        let values = self.values(info);
        let overridden = if self.fitted() {
            self.loaded.engine.overridden_attrs(self.type_id)
        } else {
            Vec::new()
        };
        let describe = attributes::Describe {
            data,
            catalog,
            own: info,
            fitted: self.fitted(),
            overridden: &overridden,
            raw: self.raw,
        };
        let category = catalog.get(self.type_id).map_or(0, |t| t.category_id);
        let nodes = attributes::tree(&describe, category, &values);
        let theme = cx.theme();
        let head = h_flex()
            .h(px(ROW_H))
            .px_1()
            .gap_1()
            .bg(theme.table_head)
            .text_color(theme.table_head_foreground)
            .child(div().flex_1().child("Attribute"))
            .child(div().flex_none().w(px(VALUE_W)).child("Current Value"))
            .when(self.fitted(), |h| {
                h.child(div().flex_none().w(px(VALUE_W)).child("Base Value"))
            });
        let csv = attributes::csv(data, info, &values);
        let file_name = format!("{} ({}).csv", catalog.name(self.type_id), self.type_id);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(head)
            .child(self.tree_view(TreeId::Attributes, &nodes, cx))
            .child(
                Self::button_row(cx)
                    .child(
                        Button::new("raw")
                            .small()
                            .ghost()
                            .label("View Raw Data")
                            .selected(self.raw)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.raw = !this.raw;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("export")
                            .small()
                            .ghost()
                            .label("Export Item Stats")
                            .on_click(move |_, _, cx| export_csv(&file_name, csv.clone(), cx)),
                    ),
            )
            .into_any_element()
    }

    fn requirements_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let nodes = requirements(&self.loaded.catalog, data, self.type_id);
        if nodes.is_empty() {
            return Self::empty("No skills needed.", cx);
        }
        self.tree_view(TreeId::Requirements, &nodes, cx)
            .into_any_element()
    }

    fn dependents_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let nodes = dependents(&self.loaded.catalog, data, self.type_id);
        if nodes.is_empty() {
            return Self::empty("Nothing needs this skill.", cx);
        }
        self.tree_view(TreeId::Dependents, &nodes, cx)
            .into_any_element()
    }

    fn effects_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let info = data.type_info(self.type_id);
        let mut effects: Vec<(String, String, String)> = info
            .map_or(&[][..], |i| i.effects.as_slice())
            .iter()
            .map(|&(id, _)| {
                let effect = data.effects.get(&id);
                let name = effect.map_or_else(|| id.to_string(), |e| e.name.clone());
                let active = self
                    .inspection
                    .as_ref()
                    .and_then(|i| i.effects.iter().find(|&&(e, _)| e == id))
                    .map_or("", |&(_, running)| if running { "Yes" } else { "No" });
                let kind = effect.map_or("", |e| effect_type(e.category));
                (name, active.to_owned(), kind.to_owned())
            })
            .collect();
        effects.sort();
        let cell = |w: Option<f32>, text: String| match w {
            Some(w) => div().flex_none().w(px(w)).truncate().child(text),
            None => div().flex_1().min_w_0().truncate().child(text),
        };
        v_flex()
            .id("effects")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                h_flex()
                    .h(px(ROW_H))
                    .px_1()
                    .bg(theme.table_head)
                    .text_color(theme.table_head_foreground)
                    .child(cell(None, "Name".into()))
                    .child(cell(Some(60.), "Active".into()))
                    .child(cell(Some(120.), "Type".into())),
            )
            .children(
                effects
                    .into_iter()
                    .enumerate()
                    .map(|(n, (name, active, kind))| {
                        h_flex()
                            .h(px(ROW_H))
                            .px_1()
                            .when(n % 2 == 1, |r| r.bg(theme.table_even))
                            .child(cell(None, name))
                            .child(cell(Some(60.), active))
                            .child(cell(Some(120.), kind))
                    }),
            )
            .into_any_element()
    }

    fn affected_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let Some(inspection) = &self.inspection else {
            return Self::empty("Not part of a fit.", cx);
        };
        let names = affected::Names {
            catalog: &self.loaded.catalog,
            data,
        };
        let nodes = affected::tree(&names, inspection, self.affected_view, self.fit_index());
        let view = self.affected_view;
        let expanded = self.affected_open.default_open;
        v_flex()
            .flex_1()
            .min_h_0()
            .child(if nodes.is_empty() {
                Self::empty("Nothing changes this item.", cx)
            } else {
                self.tree_view(TreeId::AffectedBy, &nodes, cx)
                    .into_any_element()
            })
            .child(
                Self::button_row(cx)
                    .child(
                        Button::new("expand")
                            .small()
                            .ghost()
                            .label("Expand All")
                            .selected(expanded)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.affected_open.set_all(!expanded);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("names")
                            .small()
                            .ghost()
                            .label("Toggle Names")
                            .selected(view.real_names)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.affected_view.real_names = !this.affected_view.real_names;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("view")
                            .small()
                            .ghost()
                            .label("Toggle View")
                            .selected(view.by_attribute)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.affected_view.by_attribute = !this.affected_view.by_attribute;
                                // The tree is rebuilt, so it starts out as Expand All says.
                                let open = this.affected_open.default_open;
                                this.affected_open.set_all(open);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn properties_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let rows = properties(
            &self.loaded.catalog,
            data,
            self.type_id,
            self.fit.as_ref(),
            self.target,
        );
        let theme = cx.theme();
        let count = rows.len();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                v_flex()
                    .id("properties")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows.into_iter().enumerate().map(|(n, (name, value))| {
                        h_flex()
                            .h(px(ROW_H))
                            .px_1()
                            .gap_2()
                            .when(n % 2 == 1, |r| r.bg(theme.table_even))
                            .child(div().flex_none().w(px(160.)).truncate().child(name))
                            .child(div().flex_1().min_w_0().truncate().child(value))
                    })),
            )
            .child(Self::button_row(cx).child(format!("{count} attributes.")))
            .into_any_element()
    }

    fn empty(text: &'static str, cx: &App) -> AnyElement {
        div()
            .p_3()
            .text_color(cx.theme().muted_foreground)
            .child(text)
            .into_any_element()
    }
}

impl Render for ItemStats {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                let m = event.keystroke.modifiers;
                let plain = !(m.control || m.alt || m.shift || m.platform);
                if event.keystroke.key == "escape" && plain {
                    window.remove_window();
                }
            }))
            .child(self.header(cx));
        let data = match Self::data(cx) {
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

/// Roman numerals for skill levels, as pyfa writes them.
const ROMAN: [&str; 6] = ["0", "I", "II", "III", "IV", "V"];

fn roman(level: u8) -> &'static str {
    ROMAN.get(usize::from(level)).copied().unwrap_or("?")
}

/// pyfa's requirements tree: each skill with what it needs in turn. A skill's own needs are
/// spelled out once; later mentions stand alone.
fn requirements(catalog: &crate::data::Catalog, data: &ItemData, type_id: TypeId) -> Vec<Node> {
    fn walk(
        catalog: &crate::data::Catalog,
        data: &ItemData,
        type_id: TypeId,
        path: &str,
        seen: &mut Vec<TypeId>,
    ) -> Vec<Node> {
        let needs = data
            .type_info(type_id)
            .map_or_default(TypeInfo::required_skills);
        needs
            .into_iter()
            .map(|(skill, level)| {
                let key = format!("{path}/{skill}");
                let mut node = Node::new(
                    key.clone(),
                    format!("{}  {}", catalog.name(skill), roman(level)),
                )
                .icon(Some(Icon::Type(skill)));
                if !seen.contains(&skill) {
                    node.children = walk(catalog, data, skill, &key, seen);
                    seen.push(skill);
                }
                node
            })
            .collect()
    }
    walk(catalog, data, type_id, "", &mut Vec::new())
}

/// pyfa's dependents tree: what a skill unlocks, by the level it needs.
fn dependents(catalog: &crate::data::Catalog, data: &ItemData, skill: TypeId) -> Vec<Node> {
    let Some(users) = data.required_for.get(&skill) else {
        return Vec::new();
    };
    let mut by_level: std::collections::BTreeMap<u8, Vec<TypeId>> =
        std::collections::BTreeMap::new();
    for &(id, level) in users {
        if catalog.get(id).is_some_and(|t| t.published) {
            by_level.entry(level).or_default().push(id);
        }
    }
    by_level
        .into_iter()
        .map(|(level, mut ids)| {
            ids.sort_by_key(|&id| catalog.name(id));
            let mut node = Node::new(format!("l{level}"), format!("Level {}", roman(level)))
                .icon(Some(Icon::Type(skill)));
            node.children = ids
                .into_iter()
                .map(|id| {
                    Node::new(format!("l{level}/{id}"), catalog.name(id)).icon(Some(Icon::Type(id)))
                })
                .collect();
            node
        })
        .collect()
}

/// What pyfa calls an effect's type, from its dogma category.
fn effect_type(category: i32) -> &'static str {
    match category {
        0 => "passive",
        1 => "active",
        2 => "active projected",
        3 => "area",
        4 => "online",
        5 => "overheat",
        6 => "dungeon",
        7 => "system",
        _ => "",
    }
}

/// The type's raw facts, sorted by name, like pyfa's debug Properties tab.
fn properties(
    catalog: &crate::data::Catalog,
    data: &ItemData,
    type_id: TypeId,
    fit: Option<&Fit>,
    target: Target,
) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = vec![("Id".into(), type_id.to_string())];
    let named = |id: Option<i32>, names: &std::collections::HashMap<i32, String>| {
        id.map(|i| {
            names
                .get(&i)
                .map_or_else(|| i.to_string(), |n| format!("{n} ({i})"))
        })
    };
    if let Some(t) = catalog.get(type_id) {
        rows.push(("Name".into(), t.name.clone()));
        rows.push((
            "Category".into(),
            format!("{} ({})", t.category_label(), t.category_id),
        ));
        if let Some(group) = named(Some(t.group_id), &catalog.group_names) {
            rows.push(("Group".into(), group));
        }
        if let Some(market) = t.market_group {
            let name = catalog
                .market_groups
                .get(&market)
                .map_or_else(|| market.to_string(), |g| format!("{} ({market})", g.name));
            rows.push(("Market Group".into(), name));
        }
        if let Some(meta) = named(t.meta_group, &catalog.meta_group_names) {
            rows.push(("Meta Group".into(), meta));
        }
        rows.push(("Meta Level".into(), t.meta_level.to_string()));
        rows.push(("Published".into(), t.published.to_string()));
        rows.push(("Volume".into(), format::raw(t.volume)));
        rows.push(("Capacity".into(), format::raw(t.capacity)));
        if let Some(parent) = t.variation_parent {
            rows.push((
                "Variation Parent".into(),
                format!("{} ({parent})", catalog.name(parent)),
            ));
        }
        if let Some(icon) = &t.icon {
            rows.push(("Icon".into(), icon.clone()));
        }
    }
    if let Some(info) = data.type_info(type_id) {
        let mut push = |name: &str, value: Option<String>| {
            if let Some(value) = value {
                rows.push((name.to_owned(), value));
            }
        };
        push("Base Price", info.base_price.map(format::raw));
        push("Portion Size", info.portion_size.map(|v| v.to_string()));
        push("Tech Level", info.tech_level.map(|v| v.to_string()));
        push("Race Id", info.race_id.map(|v| v.to_string()));
        push("Faction Id", info.faction_id.map(|v| v.to_string()));
        push("Effects", Some(info.effects.len().to_string()));
        push("Attributes", Some(info.attrs.len().to_string()));
    }
    if let (Target::Fitted { item, .. }, Some(fit)) = (target, fit) {
        rows.push(("Fit".into(), fit.name.clone()));
        rows.push(("Position".into(), format!("{item:?}")));
        if let FitItem::Module(slot, i) = item
            && let Some(Some(m)) = fit.rack(slot).get(i)
        {
            rows.push(("State".into(), m.state.label().to_owned()));
            if let Some(charge) = m.charge {
                rows.push(("Charge".into(), catalog.name(charge)));
            }
        }
        if let FitItem::Drone(i) = item
            && let Some(stack) = fit.drones.get(i)
        {
            rows.push(("Amount".into(), stack.count.to_string()));
            rows.push(("Amount Active".into(), stack.active.to_string()));
        }
    }
    rows.sort();
    rows
}
