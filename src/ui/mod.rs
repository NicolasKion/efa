//! The main window, laid out like pyfa: browsers on the left, the fit in the middle with its
//! additions underneath, stats on the right.

mod actions;
mod additions;
mod character_editor;
mod characters;
mod columns;
mod export;
mod fitting;
mod history;
mod hub;
mod implant_sets;
mod item_stats;
mod menu;
mod port;
mod preferences;
pub mod profile_editor;
pub mod profile_menus;
mod side;
mod stats;
mod widgets;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::ContextMenuExt,
    resizable::{h_resizable, resizable_panel, v_resizable},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, MouseUpEvent, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity, Window, WindowBounds,
    WindowOptions, div, prelude::FluentBuilder, px, size,
};

pub use actions::{Action, AddKind, RowRef, Scope};
pub use hub::Hub;
pub use menu::init as init_menus;

/// How pyfa names a rack in messages.
fn slot_label(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "high",
        Slot::Mid => "med",
        Slot::Low => "low",
        Slot::Rig => "rig",
        Slot::Subsystem => "subsystem",
    }
}

/// Which window this is: the main one, whose tabs are remembered, or one torn off for a fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Main,
    Detached(usize),
}

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

use crate::{
    character::{CharacterId, Characters},
    data::{Catalog, Kind, Slot, TypeId},
    engine::{Engine, Pilot, Stats},
    fit::Fit,
    icons::{Icon, Icons},
    prices::Prices,
    store::Saved,
    theme::{self, Palette},
};

/// How long to wait before asking ESI again after a failed price fetch.
const PRICE_RETRY: Duration = Duration::from_secs(600);

/// How often background work is checked: icons that finished downloading, theme switches.
const POLL: Duration = Duration::from_millis(250);

pub struct Loaded {
    pub catalog: Catalog,
    pub engine: Engine,
    pub icons: Icons,
    /// Where downloads and derived data are kept: icons, prices, the catalog cache.
    pub cache_dir: PathBuf,
    /// The dump the data came from, for what is only read when first needed.
    pub data_dir: PathBuf,
}

enum Loading {
    Pending,
    Ready(Rc<Loaded>),
    Failed(SharedString),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserTab {
    Market,
    Fittings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdditionsTab {
    Drones,
    Implants,
    Boosters,
    Cargo,
    Notes,
    Fighters,
    Projected,
    Command,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent view flags of one window, not a state machine"
)]
pub struct Gyfa {
    hub: Entity<Hub>,
    role: Role,
    /// The hub generation this window's fits match.
    synced: u64,
    /// Fits this window deleted since it last published, for the other windows' tabs.
    pending_deletions: Vec<usize>,
    /// The active tab shows the name field instead of the name.
    renaming: bool,
    /// A tab is being dragged, so the tear-off drop zone shows.
    tab_dragging: bool,
    /// The title last given to the window, so it is only set when it changes.
    window_title: String,
    loading: Loading,
    saved: Saved,
    stats: Option<Stats>,
    browser_tab: BrowserTab,
    additions_tab: AdditionsTab,
    search: Entity<InputState>,
    search_text: String,
    name_input: Entity<InputState>,
    notes_input: Entity<TextareaState>,
    /// The fit whose name and notes the inputs currently show.
    inputs_show: Option<usize>,
    expanded_groups: HashSet<i32>,
    /// Item to scroll the market to and highlight, after a market group jump.
    market_highlight: Option<TypeId>,
    market_scroll_pending: bool,
    market_scroll: ScrollHandle,
    /// Ship whose fits the fittings browser is narrowed to.
    fittings_ship: Option<TypeId>,
    selection: Vec<RowRef>,
    /// Where a Shift-click selection range starts.
    anchor: Option<RowRef>,
    /// Changes not written yet; the poll saves them, so dragging does not write every frame.
    unsaved: bool,
    /// The fitting view's column widths for the frame being drawn.
    column_layout: columns::Layout,
    /// Selected rows of the open additions tab.
    additions_selection: Vec<usize>,
    additions_anchor: Option<usize>,
    /// Keyboard focus of the additions list, for Delete, Ctrl+A and Escape.
    additions_focus: FocusHandle,
    /// Takes the drones pane's keys: Delete, Escape, Ctrl+A.
    drones_focus: FocusHandle,
    collapsed_sections: HashSet<&'static str>,
    /// Undo and redo steps per fit.
    history: history::History,
    /// The market search waiting out the search delay preference.
    search_debounce: Option<Task<()>>,
    stats_view: stats::View,
    status: Option<SharedString>,
    palette: Palette,
    theme_generation: Option<String>,
    /// Item prices; replaced, not mutated, when a refresh lands.
    prices: Rc<Prices>,
    prices_fetch: Option<Task<()>>,
    /// When a failed price fetch may be tried again.
    prices_retry: Option<Instant>,
    /// The hub's characters as of its last change.
    characters: Rc<Characters>,
    /// The pilot last built, and from which characters and character.
    pilot_cache: Option<(Rc<Characters>, CharacterId, Rc<Pilot>)>,
    /// The character last picked, which new fits get, as in pyfa.
    last_character: CharacterId,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Gyfa {
    pub fn new(hub: Entity<Hub>, role: Role, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let palette = Palette::current().unwrap_or_default();
        widgets::apply_palette(&palette, cx);

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search items"));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Fit name"));
        let notes_input = cx.new(|cx| TextareaState::new(window, cx).rows(6).placeholder("Notes"));
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.search_after_delay(text, cx);
                }
            }),
            cx.subscribe(&name_input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                    && this.renaming
                {
                    this.renaming = false;
                    cx.notify();
                }
            }),
            cx.subscribe(&name_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(i) = this.saved.current
                {
                    this.saved.fits[i].name = input.read(cx).value().to_string();
                    this.save();
                    cx.notify();
                }
            }),
            cx.subscribe(&notes_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.edit_notes(input.read(cx).value().to_string(), cx);
                }
            }),
        ];

        let poller = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, Gyfa::poll).is_err() {
                    break;
                }
            }
        });

        let mut subscriptions = subscriptions;
        subscriptions.push(cx.observe(&hub, |g, _, cx| g.pull(cx)));
        let (mut saved, synced, characters) = {
            let hub = hub.read(cx);
            (
                hub.saved.clone(),
                hub.generation,
                Rc::clone(&hub.characters),
            )
        };
        if let Role::Detached(fit) = role {
            saved.open = vec![fit];
            saved.current = Some(fit);
        }
        let mut this = Self {
            hub,
            role,
            synced,
            pending_deletions: Vec::new(),
            renaming: false,
            tab_dragging: false,
            window_title: String::new(),
            loading: Loading::Pending,
            saved,
            stats: None,
            browser_tab: BrowserTab::Market,
            additions_tab: AdditionsTab::Drones,
            search,
            search_text: String::new(),
            name_input,
            notes_input,
            inputs_show: None,
            expanded_groups: HashSet::new(),
            market_highlight: None,
            market_scroll_pending: false,
            market_scroll: ScrollHandle::new(),
            fittings_ship: None,
            selection: Vec::new(),
            anchor: None,
            unsaved: false,
            column_layout: columns::Layout::default(),
            additions_selection: Vec::new(),
            additions_anchor: None,
            additions_focus: cx.focus_handle(),
            drones_focus: cx.focus_handle(),
            // Which sections show at all is a preference; bombing is off by default, as in pyfa.
            collapsed_sections: HashSet::new(),
            history: history::History::default(),
            search_debounce: None,
            stats_view: stats::View::default(),
            status: None,
            palette,
            theme_generation: theme::generation(),
            prices: Rc::new(Prices::default()),
            prices_fetch: None,
            prices_retry: None,
            characters,
            pilot_cache: None,
            last_character: CharacterId::default(),
            _subscriptions: subscriptions,
            _tasks: vec![poller],
        };
        this.pull(cx);
        this
    }

    /// Take in what changed in the hub: data finishing loading, new prices, and fits other
    /// windows changed.
    fn pull(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.read(cx);
        let mut changed = false;
        if matches!(self.loading, Loading::Pending) {
            match &hub.loading {
                hub::HubLoading::Ready(loaded) => {
                    self.loading = Loading::Ready(Rc::clone(loaded));
                    changed = true;
                }
                hub::HubLoading::Failed(e) => {
                    self.loading = Loading::Failed(e.clone());
                    changed = true;
                }
                hub::HubLoading::Pending => {}
            }
        }
        if !Rc::ptr_eq(&self.prices, &hub.prices) {
            self.prices = Rc::clone(&hub.prices);
            changed = true;
        }
        if !Rc::ptr_eq(&self.characters, &hub.characters) {
            self.characters = Rc::clone(&hub.characters);
            changed = true;
        }
        if hub.generation != self.synced {
            let deleted: Vec<usize> = hub
                .deletions
                .iter()
                .filter(|(generation, _)| *generation > self.synced)
                .map(|&(_, index)| index)
                .collect();
            let (open, current) = (self.saved.open.clone(), self.saved.current);
            // Edits not published yet survive a settings change from the preferences window.
            let unpublished =
                (self.unsaved && deleted.is_empty()).then(|| std::mem::take(&mut self.saved.fits));
            self.saved = hub.saved.clone();
            self.saved.open = open;
            self.saved.current = current;
            if let Some(fits) = unpublished {
                self.saved.fits = fits;
            }
            for index in deleted {
                hub::shift_tabs(&mut self.saved.open, &mut self.saved.current, index);
                self.history.forget(index);
            }
            self.synced = hub.generation;
            self.inputs_show = None;
            changed = true;
        }
        let import = (self.role == Role::Main && self.loaded().is_some())
            .then(|| self.hub.update(cx, |hub, _| hub.import.take()))
            .flatten();
        if let Some(text) = import {
            self.import(&text);
            self.save();
        }
        if changed {
            self.recompute();
            cx.notify();
        }
    }

    /// Hand this window's fits to the hub, which writes them out and updates the other windows.
    fn publish(&mut self, cx: &mut Context<Self>) {
        let saved = self.saved.clone();
        let deleted = std::mem::take(&mut self.pending_deletions);
        let main = self.role == Role::Main;
        self.hub
            .update(cx, |hub, cx| hub.publish(&saved, main, &deleted, cx));
        self.synced = self.hub.read(cx).generation;
    }

    /// Open a fit in a window of its own and close its tab here, the way tearing a tab off works.
    fn open_in_new_window(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.saved.fits.len() {
            return;
        }
        // The new window starts from the hub, so it has to have this window's latest.
        self.publish(cx);
        let hub = self.hub.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1400.), px(860.)), cx)),
            app_id: Some("gyfa".into()),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Gyfa::new(hub, Role::Detached(index), window, cx))
        });
        match opened {
            Ok(_) => self.close_fit(index, cx),
            Err(e) => {
                self.status = Some(format!("Could not open a window: {e}").into());
                cx.notify();
            }
        }
    }

    /// Move a tab to where another one is.
    fn move_tab(&mut self, from: usize, onto: usize, cx: &mut Context<Self>) {
        let open = &mut self.saved.open;
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

    fn loaded(&self) -> Option<Rc<Loaded>> {
        match &self.loading {
            Loading::Ready(loaded) => Some(Rc::clone(loaded)),
            Loading::Pending | Loading::Failed(_) => None,
        }
    }

    /// Redraw for downloaded icons, restyle for a theme switch.
    /// Fetch newer prices in the background when the ones we have are stale. After a failure it
    /// waits a while before trying again, so being offline does not mean a request every poll.
    fn refresh_prices(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let preferences = &self.saved.preferences;
        if !preferences.network.enabled || !preferences.network.prices {
            return;
        }
        let waiting = self.prices_retry.is_some_and(|at| Instant::now() < at);
        if self.prices_fetch.is_some() || waiting {
            return;
        }
        let origin = crate::prices::Origin {
            source: preferences.market.price_source,
            system: preferences.market.price_system,
        };
        // Sources priced per type are asked about what the saved fits use.
        let types: HashSet<TypeId> = if origin.source.uses_system() {
            self.saved
                .fits
                .iter()
                .flat_map(crate::port::fit_types)
                .collect()
        } else {
            HashSet::new()
        };
        if !self.prices.is_stale_for(origin, &types) {
            return;
        }
        let prices = (*self.prices).clone();
        let cache_dir = loaded.cache_dir.clone();
        let proxy = preferences.network.proxy.clone();
        self.prices_fetch = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx
                .background_spawn(async move {
                    let http = proxy.client(crate::prices::user_agent())?;
                    prices.refresh_from(&cache_dir, origin, &types, &http)
                })
                .await;
            let _ = this.update(cx, |g, cx| {
                g.prices_fetch = None;
                match result {
                    Ok(prices) => {
                        let prices = Rc::new(prices);
                        g.prices = Rc::clone(&prices);
                        g.prices_retry = None;
                        g.hub.update(cx, |hub, cx| hub.set_prices(prices, cx));
                        cx.notify();
                    }
                    Err(e) => {
                        eprintln!("prices: {e}");
                        g.prices_retry = Some(Instant::now() + PRICE_RETRY);
                    }
                }
            });
        }));
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        // One window fetching prices is enough; the hub hands them to the rest.
        if self.role == Role::Main {
            self.refresh_prices(cx);
        }
        if std::mem::take(&mut self.unsaved) {
            self.publish(cx);
        }
        if self.loaded().is_some_and(|l| l.icons.take_updated()) {
            cx.notify();
        }
        let generation = theme::generation();
        if generation != self.theme_generation {
            self.theme_generation = generation;
            if let Some(palette) = Palette::current() {
                self.palette = palette;
                widgets::apply_palette(&palette, cx);
            }
        }
    }

    fn fit(&self) -> Option<&Fit> {
        self.saved.current.map(|i| &self.saved.fits[i])
    }

    fn recompute(&mut self) {
        let Some(loaded) = self.loaded() else { return };
        let options = crate::engine::CalcOptions {
            factor_reload: self.saved.factor_reload,
            default_spool: Some(self.saved.preferences.engine.default_spool),
        };
        // Older saves kept fighters as drone stacks; links can point at fits deleted since.
        if let Some(i) = self.saved.current
            && (self.saved.fits[i].move_fighters_out_of_drones(&loaded.catalog)
                | self.saved.tidy_links(i))
        {
            self.unsaved = true;
        }
        let pilot = self.pilot(&loaded);
        self.stats = self.fit().map(|fit| {
            loaded
                .engine
                .compute_among(fit, &self.saved.fits, &pilot, options)
        });
    }

    /// Mark the fits as changed. The poll publishes them to the hub, which writes them out and
    /// passes them on to the other windows.
    fn save(&mut self) {
        self.unsaved = true;
    }

    /// Recalculate, save and redraw after the fits changed.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.recompute();
        self.save();
        cx.notify();
    }

    fn open_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        if !self.saved.open.contains(&index) {
            self.saved.open.push(index);
        }
        self.saved.current = Some(index);
        self.changed(cx);
    }

    /// An empty fit flown by the character picked last.
    fn new_fit(&self, name: impl Into<String>) -> Fit {
        Fit {
            character: self.last_character,
            ..Fit::new(name)
        }
    }

    fn add_fit(&mut self, fit: Fit, cx: &mut Context<Self>) {
        self.saved.fits.push(fit);
        self.open_fit(self.saved.fits.len() - 1, cx);
    }

    fn close_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.open.retain(|&i| i != index);
        if self.saved.current == Some(index) {
            self.saved.current = self.saved.open.last().copied();
        }
        self.changed(cx);
    }

    fn close_other_fits(&mut self, keep: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.open.retain(|&i| i == keep);
        self.saved.current = Some(keep);
        self.changed(cx);
    }

    fn delete_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.open.retain(|&i| i != index);
        if self.saved.current == Some(index) {
            self.saved.current = self.saved.open.last().copied();
        }
        self.saved.fits.remove(index);
        self.history.forget(index);
        self.pending_deletions.push(index);
        let shift = |i: usize| if i > index { i - 1 } else { i };
        self.saved.open = self.saved.open.iter().map(|&i| shift(i)).collect();
        self.saved.current = self.saved.current.map(shift);
        self.inputs_show = None;
        self.changed(cx);
    }

    /// Import fits in any format as new open fits. Problems end up in the status line.
    fn import(&mut self, text: &str) {
        self.import_fits(text, None);
    }

    /// Add an item from the market the way double-clicking it in pyfa does.
    fn add_item(&mut self, id: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let catalog = &loaded.catalog;
        let is_ship = catalog.get(id).is_some_and(|t| t.kind == Kind::Ship);
        // A ship starts a new fit rather than swapping the hull under an existing one.
        if is_ship {
            let mut fit = self.new_fit(catalog.name(id));
            fit.ship = Some(id);
            fit.mode = catalog.modes_for(id).first().copied();
            self.new_fit_defaults(&mut fit);
            self.status = None;
            self.add_fit(fit, cx);
            return;
        }
        let before = self.saved.current.map(|i| self.saved.fits[i].clone());
        let Some(i) = self.saved.current else {
            self.status = Some("Pick a ship first".into());
            cx.notify();
            return;
        };
        // Like pyfa, a module only goes on when its rack has a free slot.
        if let Some(Kind::Module(slot)) = catalog.get(id).map(|t| t.kind)
            && self.free_slots(slot) == 0
        {
            self.status = Some(format!("No free {} slot", slot_label(slot)).into());
            cx.notify();
            return;
        }
        // pyfa launches a new squadron only when a tube of its class is free.
        if catalog.get(id).is_some_and(|t| t.kind == Kind::Fighter) {
            self.status = None;
            self.add_fighter_squadrons(vec![crate::fit::fighters::Fighter::new(id)], cx);
            return;
        }
        let added = self.saved.fits[i].add(catalog, id);
        // pyfa brings the Boosters tab up even when the booster was already there.
        if catalog.get(id).is_some_and(|t| t.kind == Kind::Booster)
            && self.additions_tab != AdditionsTab::Boosters
        {
            self.additions_tab = AdditionsTab::Boosters;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
        if catalog.get(id).is_some_and(|t| t.kind == Kind::Implant)
            && self.additions_tab != AdditionsTab::Implants
        {
            self.additions_tab = AdditionsTab::Implants;
            self.clear_selection();
        }
        // pyfa brings the drones pane forward when a drone goes in.
        if added
            && self.additions_tab != AdditionsTab::Drones
            && catalog.get(id).is_some_and(|t| t.kind == Kind::Drone)
        {
            self.additions_tab = AdditionsTab::Drones;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
        self.status =
            (!added).then(|| format!("Nothing on this fit takes {}", catalog.name(id)).into());
        if let Some(before) = before {
            self.history.record(i, before, &self.saved.fits[i]);
        }
        self.changed(cx);
    }

    /// Empty slots left in a rack: the ship's total after bonuses, which subsystems change, less
    /// what is fitted.
    pub(super) fn free_slots(&self, slot: Slot) -> usize {
        let (Some(fit), Some(stats)) = (self.fit(), &self.stats) else {
            return 0;
        };
        let (total, used) = match slot {
            Slot::High | Slot::Mid | Slot::Low => (
                match slot {
                    Slot::High => stats.high.total,
                    Slot::Mid => stats.mid.total,
                    _ => stats.low.total,
                },
                fit.rack(slot).iter().flatten().count(),
            ),
            Slot::Rig => (stats.rig.total, fit.rigs.iter().flatten().count()),
            Slot::Subsystem => (stats.subsystem.total, fit.subsystems.len()),
        };
        (total.unwrap_or(0) as usize).saturating_sub(used)
    }

    fn open_import_dialog(window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(16)
                .placeholder("[Vexor, My Vexor]\nDrone Damage Amplifier II\n…")
        });
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (text, this) = (text.clone(), this.clone());
            dialog.title("Import Fits").w(px(560.)).child(
                v_flex()
                    .gap_3()
                    .child(Textarea::new(&text).h(px(320.)))
                    .child(
                        h_flex().justify_end().child(
                            Button::new("import-eft")
                                .primary()
                                .label("Import")
                                .on_click(move |_, window, cx| {
                                    let value = text.read(cx).value().to_string();
                                    let _ = this.update(cx, |this, cx| {
                                        this.import(&value);
                                        this.changed(cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                    ),
            )
        });
    }

    /// Point the name and notes inputs at the current fit when it changes.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inputs_show == self.saved.current {
            return;
        }
        self.inputs_show = self.saved.current;
        let (name, notes) = self
            .fit()
            .map_or_default(|f| (f.name.clone(), f.notes.clone()));
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.notes_input
            .update(cx, |input, cx| input.set_value(notes, window, cx));
    }

    fn duplicate_fit(&mut self, cx: &mut Context<Self>) {
        let Some(mut copy) = self.fit().cloned() else {
            return;
        };
        copy.name = format!("{} copy", copy.name);
        self.add_fit(copy, cx);
    }

    /// The open fits as slim tabs: ship icon, fit name and hull on one line.
    fn fit_tabs(&self, loaded: &Loaded, cx: &mut Context<Self>) -> AnyElement {
        let tabs: Vec<AnyElement> = self
            .saved
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
        let selected = self.saved.current == Some(i);
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
                .child(div().truncate().child(fit.name.clone()))
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
            .when(self.saved.preferences.general.tab_tooltips, |tab| {
                let tip: SharedString = label.clone().into();
                tab.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            })
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
                    ),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if this.saved.current == Some(i) && event.click_count() >= 2 {
                    this.start_rename(window, cx);
                    return;
                }
                if this.saved.current != Some(i) {
                    this.renaming = false;
                    this.saved.current = Some(i);
                    this.clear_selection();
                    this.changed(cx);
                }
            }))
            .on_drag(
                TabDrag {
                    index: i,
                    label: label.into(),
                },
                {
                    let this = cx.entity().downgrade();
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
            .drag_over::<TabDrag>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(move |g, drag: &TabDrag, _, cx| {
                g.tab_dragging = false;
                g.move_tab(drag.index, i, cx);
            }))
            .context_menu({
                let this = cx.entity().downgrade();
                let ship = fit.ship.filter(|_| selected);
                move |menu, window, cx| {
                    let on = |f: fn(&mut Gyfa, usize, &mut Context<Gyfa>)| {
                        let this = this.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let _ = this.update(cx, |g, cx| f(g, i, cx));
                        }
                    };
                    let rename = this.clone();
                    let menu = menu
                        .item(widgets::entry(IconName::Replace, "Rename").on_click(
                            move |_, window, cx| {
                                let _ = rename.update(cx, |g, cx| {
                                    g.saved.current = Some(i);
                                    g.changed(cx);
                                    g.start_rename(window, cx);
                                });
                            },
                        ))
                        .item(
                            widgets::entry(IconName::ExternalLink, "Open in New Window")
                                .on_click(on(Gyfa::open_in_new_window)),
                        )
                        .separator()
                        .item(
                            widgets::entry(IconName::Close, "Close").on_click(on(Gyfa::close_fit)),
                        )
                        .item(
                            widgets::entry(IconName::Close, "Close Others")
                                .on_click(on(Gyfa::close_other_fits)),
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
    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = true;
        self.inputs_show = None;
        self.sync_inputs(window, cx);
        self.name_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
}

impl Gyfa {
    /// Where a dragged tab is dropped to get a window of its own. Wayland does not tell a window
    /// where a drag outside it ends, so tearing off happens onto this zone instead.
    fn tear_off_zone(cx: &mut Context<Self>) -> impl IntoElement {
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
            .drag_over::<TabDrag>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(|g, drag: &TabDrag, _, cx| {
                g.tab_dragging = false;
                g.open_in_new_window(drag.index, cx);
            }))
    }

    /// Show an item in the market browser: clear the search, open its groups, scroll to it.
    pub(super) fn jump_to_market(
        &mut self,
        id: TypeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        self.browser_tab = BrowserTab::Market;
        self.search_text.clear();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.expanded_groups.extend(loaded.catalog.market_path(id));
        self.market_highlight = Some(id);
        self.market_scroll_pending = true;
        cx.notify();
    }

    /// Show the fittings browser narrowed to one ship's fits.
    pub(super) fn jump_to_fittings(&mut self, ship: TypeId, cx: &mut Context<Self>) {
        self.browser_tab = BrowserTab::Fittings;
        self.fittings_ship = Some(ship);
        cx.notify();
    }
}

fn load(data_dir: &Path) -> Result<Loaded, String> {
    if !data_dir.join("phobos/metadata.json").exists() {
        return Err(format!(
            "No EVE data in {}.\nRun scripts/fetch-data.sh, or point GYFA_DATA at a deimos dump.",
            data_dir.display()
        ));
    }
    let cache_dir = data_dir.parent().unwrap_or(data_dir).join("cache");
    let catalog = Catalog::load_cached(data_dir, &cache_dir)?;
    let engine = Engine::load(data_dir, &cache_dir, catalog.skills.clone())?;
    let icons = Icons::new(data_dir, &cache_dir);
    Ok(Loaded {
        catalog,
        engine,
        icons,
        cache_dir,
        data_dir: data_dir.to_owned(),
    })
}

impl Render for Gyfa {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm();
        let root = Self::bind_actions(root, cx);
        let loaded = match &self.loading {
            Loading::Ready(loaded) => Rc::clone(loaded),
            Loading::Pending => {
                return root
                    .items_center()
                    .justify_center()
                    .child("Loading EVE data…")
                    .into_any_element();
            }
            Loading::Failed(e) => {
                return root
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().danger)
                    .child(e.clone())
                    .into_any_element();
            }
        };
        self.sync_inputs(window, cx);

        let center = v_flex()
            .size_full()
            .child(self.fit_tabs(&loaded, cx))
            .child(if self.fit().is_some() {
                v_resizable("center")
                    .child(resizable_panel().child(self.fitting_view(&loaded, window, cx)))
                    .child(
                        resizable_panel()
                            .size(px(200.))
                            .size_range(px(80.)..px(600.))
                            .child(self.additions(&loaded, window, cx)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child("Double-click a ship in the market to start a fit, or import one from the File menu.")
                    .into_any_element()
            })
            .relative()
            .when(self.tab_dragging, |center| center.child(Self::tear_off_zone(cx)));

        let title = match self.fit() {
            Some(fit) => match fit.ship {
                Some(ship) => format!("gyfa: {} ({})", fit.name, loaded.catalog.name(ship)),
                None => format!("gyfa: {}", fit.name),
            },
            None => "gyfa".to_owned(),
        };
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }

        root.capture_any_mouse_up(cx.listener(|g, _: &MouseUpEvent, _, cx| {
            // A tab let go anywhere, dropped or not, ends the drag; the zone goes away.
            if g.tab_dragging {
                g.tab_dragging = false;
                cx.notify();
            }
        }))
        .child(self.menu_bar(cx))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .child(
                    // Only this part gives up room when the window narrows; the stats pane
                    // keeps its width and the market keeps its own minimum.
                    div().flex_1().min_w_0().h_full().child(
                        h_resizable("main")
                            .child(
                                resizable_panel()
                                    .size(px(280.))
                                    .size_range(px(200.)..px(520.))
                                    .child(self.browser(&loaded, cx)),
                            )
                            .child(resizable_panel().child(center)),
                    ),
                )
                // pyfa's stats pane is as wide as its content and does not resize.
                .child(
                    div()
                        .flex_none()
                        .w(px(stats::PANE_WIDTH))
                        .h_full()
                        .border_l_1()
                        .border_color(cx.theme().border)
                        .child(self.stats_panel(&loaded, cx)),
                ),
        )
        .into_any_element()
    }
}

/// Inputs that the views embed, exposed for them.
impl Gyfa {
    fn search_input(&self) -> Input {
        Input::new(&self.search).small()
    }

    fn name_field(&self) -> Input {
        Input::new(&self.name_input).small()
    }

    fn notes_field(&self) -> Textarea {
        Textarea::new(&self.notes_input)
    }

    fn weak(cx: &Context<Self>) -> WeakEntity<Self> {
        cx.entity().downgrade()
    }

    fn app_update(
        weak: &WeakEntity<Self>,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        let _ = weak.update(cx, f);
    }
}
