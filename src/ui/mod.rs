//! The main window, laid out like pyfa: browsers on the left, the fit in the middle with its
//! additions underneath, stats on the right.

mod actions;
mod additions;
mod cargo_presets;
mod character_editor;
mod characters;
pub mod chrome;
mod columns;
mod dialogs;
mod drag;
mod editor_kit;
mod error_dialog;
mod esi;
mod export;
mod fits;
mod fitting;
mod graphs;
mod history;
mod hub;
mod implant_sets;
mod item_data;
mod item_stats;
mod menu;
mod mutations;
mod overrides;
mod port;
mod preferences;
mod prices;
mod profile_editor;
mod profile_menus;
mod selection;
mod side;
mod splash;
mod stats;
mod status_bar;
mod sync;
mod tabs;
mod tooltips;
mod updates;
mod validation;
mod widgets;
mod wizard;

use std::{
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::component::{
    ActiveTheme, Sizable, h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    resizable::{h_resizable, resizable_panel, v_resizable},
    v_flex,
};
use gpui_kit::{
    App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, MouseUpEvent,
    ParentElement, Render, ScrollHandle, ScrollStrategy, SharedString, Styled, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

pub use actions::{Action, AddKind, RowRef, Scope};
pub use hub::Hub;
pub use menu::init as init_menus;
use prices::PriceWaiter;
use selection::ListSelection;

/// Which window this is: the main one, whose tabs are remembered, or one torn off for a fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Main,
    Detached(usize),
}

use crate::{
    character::{CharacterId, Characters},
    data::{Catalog, TypeId},
    engine::{Engine, Pilot, Stats},
    fit::Fit,
    icons::Icons,
    prices::Prices,
    store::Saved,
    theme::Palette,
};

/// How often background work is checked: prices, errors to show, changes to publish.
const POLL: Duration = Duration::from_millis(250);

pub struct Loaded {
    pub catalog: Catalog,
    /// Shared so long searches, like the fitting wizard's tank optimiser, can run off the UI
    /// thread.
    pub engine: std::sync::Arc<Engine>,
    pub icons: Icons,
    /// Where downloads are kept: icons, prices, the CDN index.
    pub cache_dir: PathBuf,
    /// The pack or dump the data came from, for what is only read when first needed.
    pub source: crate::pack::Source,
    /// How long reading the data took, for the status bar.
    pub load_time: std::time::Duration,
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

impl AdditionsTab {
    /// The tabs as the pane lists them, which Ctrl+1..9 count along.
    const ORDER: [Self; 8] = [
        Self::Drones,
        Self::Fighters,
        Self::Implants,
        Self::Boosters,
        Self::Cargo,
        Self::Projected,
        Self::Command,
        Self::Notes,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Drones => "Drones",
            Self::Fighters => "Fighters",
            Self::Implants => "Implants",
            Self::Boosters => "Boosters",
            Self::Cargo => "Cargo",
            Self::Projected => "Projected",
            Self::Command => "Command",
            Self::Notes => "Notes",
        }
    }

    /// Tabs pyfa disables for structures: they fly no drones, take no implants or boosters, give
    /// no fleet boosts and keep no notes.
    fn ship_only(self) -> bool {
        matches!(
            self,
            Self::Drones | Self::Implants | Self::Boosters | Self::Command | Self::Notes
        )
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent view flags of one window, not a state machine"
)]
pub struct Efa {
    hub: Entity<Hub>,
    role: Role,
    /// The hub generation this window's fits match.
    synced: u64,
    /// The hub's engine generation the stats were calculated with.
    engine_synced: u64,
    /// The strip announcing a new release, in the main window.
    update_notice: Option<Entity<updates::UpdateNotice>>,
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
    /// Whether the browser column shows the stats, which it does in place of the stats pane
    /// while the window is too narrow for that pane.
    side_stats: bool,
    additions_tab: AdditionsTab,
    search: Entity<InputState>,
    search_text: String,
    name_input: Entity<InputState>,
    notes_input: Entity<TextareaState>,
    /// The fit whose name and notes the inputs currently show.
    inputs_show: Option<usize>,
    expanded_groups: HashSet<i32>,
    /// What the market's results pane lists while the search box is empty.
    listing: Option<crate::market::Listing>,
    /// Item to scroll the market to and highlight, after a market group jump.
    market_highlight: Option<TypeId>,
    /// Items picked in the market results, for dragging or acting on several at once.
    market_selection: ListSelection<TypeId>,
    /// The items the results pane lists, top to bottom, for Shift-click ranges.
    market_order: Rc<Vec<TypeId>>,
    market_scroll_pending: bool,
    market_scroll: UniformListScrollHandle,
    market_tree_scroll: ScrollHandle,
    /// Which market items the meta group buttons let through.
    meta_filter: crate::market::MetaFilter,
    /// The results pane's own filter box, which narrows what it lists.
    results_filter: Entity<InputState>,
    results_filter_text: String,
    /// The results pane hides what the current fit cannot use.
    fits_ship_only: bool,
    /// What the results pane last listed before its filters, so a redraw does not search or
    /// sort again.
    results_cache: Option<(side::ResultsKey, Rc<Vec<crate::market::Section>>)>,
    /// Where the fittings browser is.
    ship_browser: crate::ship_browser::Browser,
    /// The fittings browser's search box, for ships and fits by name.
    fit_search: Entity<InputState>,
    /// Rows picked in the fitting view.
    selection: ListSelection<RowRef>,
    /// Changes not written yet; the poll saves them, so dragging does not write every frame.
    unsaved: bool,
    /// The fitting view's column widths for the frame being drawn.
    column_layout: columns::Layout,
    /// Bumped by every recalculation, which every change to the fits makes: what is worked out
    /// from the fits for drawing is kept until it moves on.
    calc_generation: u64,
    /// The fitting view's column widths and Miscellanea texts, as last worked out.
    fitting_cache: Option<columns::FittingCache>,
    /// The types every saved fit uses, as of a calculation generation.
    fit_types: Option<(u64, Rc<HashSet<TypeId>>)>,
    /// The fittings browser's lines, and what they were listed from.
    browser_cache: Option<(side::BrowserKey, Rc<side::BrowserRows>)>,
    /// The market results after the pane's filters, and what they were filtered with.
    filtered_cache: Option<(side::FilterKey, Rc<crate::market::Filtered>)>,
    /// Selected rows of the open additions tab, by their place in the list as shown. Tabs that
    /// sort their rows map these to the fit's own indices before acting on them.
    additions_selection: ListSelection<usize>,
    /// Keyboard focus of the additions list, for Delete, Ctrl+A and Escape.
    additions_focus: FocusHandle,
    /// Takes the drones pane's keys: Delete, Escape, Ctrl+A.
    drones_focus: FocusHandle,
    /// Takes the fitting view's keys: Delete, Escape, Ctrl+A.
    fitting_focus: FocusHandle,
    /// The whole window's, where the menu bar's commands are dispatched to.
    root_focus: FocusHandle,
    collapsed_sections: HashSet<&'static str>,
    /// Undo and redo steps per fit.
    history: history::History,
    /// The market search waiting out the search delay preference.
    search_debounce: Option<Task<()>>,
    stats_view: stats::View,
    status: Option<status_bar::Status>,
    /// The hub's theme colors, as of its last change.
    palette: Palette,
    /// Item prices; replaced, not mutated, when a refresh lands.
    prices: Rc<Prices>,
    prices_fetch: Option<Task<()>>,
    /// When a failed price fetch may be tried again.
    prices_retry: Option<Instant>,
    /// Work waiting for prices of particular types, with those types.
    price_waiters: Vec<(HashSet<TypeId>, PriceWaiter)>,
    /// The hub's characters as of its last change.
    characters: Rc<Characters>,
    /// The pilot last built, and from which characters and character.
    pilot_cache: Option<(Rc<Characters>, CharacterId, Rc<Pilot>)>,
    /// The character last picked, which new fits get, as in pyfa.
    last_character: CharacterId,
    /// The damage pattern last picked, which every fit takes with the global damage pattern
    /// preference. Uniform until one is picked, as in pyfa.
    last_damage_profile: crate::fit::DamageProfile,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Efa {
    pub fn new(hub: Entity<Hub>, role: Role, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let palette = hub.read(cx).palette;

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search items"));
        let fit_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search ships and fits"));
        let results_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter these items"));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Fit name"));
        let notes_input = cx.new(|cx| TextareaState::new(window, cx).rows(2).placeholder("Notes"));
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.search_after_delay(text, cx);
                }
            }),
            cx.subscribe(&results_filter, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.results_filter_text = input.read(cx).value().to_string();
                    this.market_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
            }),
            cx.subscribe(&fit_search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.search_fittings(text, cx);
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
                    && let Some(i) = this.saved.settings.current
                {
                    let name = input.read(cx).value().to_string();
                    if this.saved.fits[i].name == name {
                        return;
                    }
                    let fit = this.saved.fit_mut(i);
                    fit.name = name;
                    fit.modified = crate::fit::now();
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
                if this.update(cx, Efa::poll).is_err() {
                    break;
                }
            }
        });

        let mut subscriptions = subscriptions;
        subscriptions.push(cx.observe(&hub, |g, _, cx| g.pull(cx)));
        let update_notice = (role == Role::Main).then(|| {
            let updates = hub.read(cx).updates.clone();
            cx.new(|cx| updates::UpdateNotice::new(updates, cx))
        });
        let (mut saved, synced, characters) = {
            let hub = hub.read(cx);
            (
                hub.saved.clone(),
                hub.generation,
                Rc::clone(&hub.characters),
            )
        };
        if let Role::Detached(fit) = role {
            saved.settings.open = vec![fit];
            saved.settings.current = Some(fit);
        }
        let mut this = Self {
            hub,
            role,
            synced,
            engine_synced: 0,
            update_notice,
            pending_deletions: Vec::new(),
            renaming: false,
            tab_dragging: false,
            window_title: String::new(),
            loading: Loading::Pending,
            saved,
            stats: None,
            browser_tab: BrowserTab::Market,
            side_stats: false,
            additions_tab: AdditionsTab::Drones,
            search,
            search_text: String::new(),
            name_input,
            notes_input,
            inputs_show: None,
            expanded_groups: HashSet::new(),
            listing: None,
            market_highlight: None,
            market_selection: ListSelection::default(),
            market_order: Rc::default(),
            market_scroll_pending: false,
            market_scroll: UniformListScrollHandle::new(),
            market_tree_scroll: ScrollHandle::new(),
            meta_filter: crate::market::MetaFilter::default(),
            results_filter,
            results_filter_text: String::new(),
            fits_ship_only: false,
            results_cache: None,
            ship_browser: crate::ship_browser::Browser::default(),
            fit_search,
            selection: ListSelection::default(),
            unsaved: false,
            column_layout: columns::Layout::default(),
            calc_generation: 0,
            fitting_cache: None,
            fit_types: None,
            filtered_cache: None,
            browser_cache: None,
            additions_selection: ListSelection::default(),
            additions_focus: cx.focus_handle(),
            drones_focus: cx.focus_handle(),
            fitting_focus: cx.focus_handle(),
            root_focus: cx.focus_handle(),
            // Which sections show at all is a preference; bombing is off by default, as in pyfa.
            collapsed_sections: HashSet::new(),
            history: history::History::default(),
            search_debounce: None,
            stats_view: stats::View::default(),
            status: None,
            palette,
            prices: Rc::new(Prices::default()),
            prices_fetch: None,
            prices_retry: None,
            price_waiters: Vec::new(),
            characters,
            pilot_cache: None,
            last_character: CharacterId::default(),
            last_damage_profile: crate::fit::DamageProfile::default(),
            _subscriptions: subscriptions,
            _tasks: vec![poller],
        };
        this.pull(cx);
        this
    }

    /// Let a window open, and say in the status line why it could not.
    fn report_open(opening: Task<Result<(), SharedString>>, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            if let Err(e) = opening.await {
                let _ = this.update(cx, |g, cx| {
                    g.set_error(e);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn loaded(&self) -> Option<Rc<Loaded>> {
        match &self.loading {
            Loading::Ready(loaded) => Some(Rc::clone(loaded)),
            Loading::Pending | Loading::Failed(_) => None,
        }
    }

    fn fit(&self) -> Option<&Fit> {
        self.saved.settings.current.map(|i| &*self.saved.fits[i])
    }

    fn recompute(&mut self) {
        let Some(loaded) = self.loaded() else { return };
        self.calc_generation += 1;
        self.follow_globals();
        let options = crate::engine::CalcOptions {
            factor_reload: self.saved.settings.factor_reload,
            default_spool: Some(self.saved.settings.preferences.engine.default_spool),
        };
        // Older saves kept fighters as drone stacks; links can point at fits deleted since.
        if let Some(i) = self.saved.settings.current {
            let fighters = self.saved.fits[i].has_fighter_stacks(&loaded.catalog);
            if fighters {
                self.saved
                    .fit_mut(i)
                    .move_fighters_out_of_drones(&loaded.catalog);
            }
            if fighters | self.saved.tidy_links(i) {
                self.unsaved = true;
            }
        }
        let pilot = self.pilot(&loaded);
        self.stats = self.fit().map(|fit| {
            loaded
                .engine
                .compute_among(fit, &self.saved.fits, &pilot, options)
        });
    }

    /// With pyfa's "Use global character" and "Use global damage pattern", the current fit
    /// takes the character and damage pattern picked last, as pyfa's `switchFit` does.
    fn follow_globals(&mut self) {
        let general = &self.saved.settings.preferences.general;
        let (character, pattern) = (general.global_character, general.global_damage_pattern);
        let Some(i) = self.saved.settings.current else {
            return;
        };
        let fit = &self.saved.fits[i];
        if character && fit.character != self.last_character {
            self.saved.fit_mut(i).character = self.last_character;
            self.unsaved = true;
        }
        let fit = &self.saved.fits[i];
        if pattern && fit.damage_profile != self.last_damage_profile {
            self.saved.fit_mut(i).damage_profile = self.last_damage_profile.clone();
            self.unsaved = true;
        }
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

    /// Keep `before` for undo and date the fit, when the edit changed it.
    fn record_edit(&mut self, index: usize, before: Rc<Fit>) {
        if self.history.record(index, before, &self.saved.fits[index]) {
            self.saved.fit_mut(index).modified = crate::fit::now();
        }
    }

    /// Point the name and notes inputs at the current fit when it changes.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inputs_show == self.saved.settings.current {
            return;
        }
        self.inputs_show = self.saved.settings.current;
        let (name, notes) = self
            .fit()
            .map_or_default(|f| (f.name.clone(), f.notes.clone()));
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.notes_input
            .update(cx, |input, cx| input.set_value(notes, window, cx));
    }
}

fn load(
    overrides: &crate::overrides::Values,
    proxy: &crate::preferences::Proxy,
) -> Result<Loaded, String> {
    let start = std::time::Instant::now();
    let source = crate::pack::Source::detect()?;
    let catalog = source.catalog()?;
    let engine = Engine::from_source(source.engine(), catalog.skills.clone())?;
    let load_time = start.elapsed();
    eprintln!(
        "loaded {} in {} ms",
        source.describe(),
        load_time.as_millis()
    );
    if !overrides.is_empty() {
        engine.apply_overrides(overrides)?;
    }
    let cache_dir = crate::paths::cache_dir();
    // A proxy setting that does not parse leaves icons to what is on disk.
    let icons = Icons::new(source.dump_dir(), &cache_dir, proxy.client().ok());
    Ok(Loaded {
        catalog,
        engine: std::sync::Arc::new(engine),
        icons,
        cache_dir,
        source,
        load_time,
    })
}

/// Below this window width the stats pane moves into the browser column as a tab, so the fit
/// keeps room enough to read.
const NARROW_WIDTH: f32 = 900.;

impl Render for Efa {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm();
        let root = self.bind_actions(root, cx);
        let loaded = match &self.loading {
            Loading::Ready(loaded) => Rc::clone(loaded),
            // The menu bar stays, so the window can still be moved and closed when efa draws
            // its own window buttons.
            Loading::Pending | Loading::Failed(_) => {
                let failed = match &self.loading {
                    Loading::Failed(e) => Some(e.clone()),
                    _ => None,
                };
                return root
                    .child(self.menu_bar(window, cx))
                    .child(splash::render(failed, cx))
                    .child(self.status_bar(cx))
                    .into_any_element();
            }
        };
        self.sync_inputs(window, cx);

        let center = v_flex()
            .size_full()
            .child(self.fit_tabs(&loaded, cx))
            .child(if self.fit().is_some() {
                // Under the tabs: the split fills what they leave, not the whole column.
                div()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_resizable("center")
                            .child(resizable_panel().child(self.fitting_view(&loaded, window, cx)))
                            .child(
                                // The additions drawer keeps its height as the window resizes;
                                // the fitting view above takes the change and scrolls.
                                resizable_panel()
                                    .size(px(200.))
                                    .size_range(px(80.)..px(600.))
                                    .fixed()
                                    .child(self.additions(&loaded, window, cx)),
                            ),
                    )
                    .into_any_element()
            } else {
                // The hint wraps within the column instead of pushing past it.
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_4()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().min_w_0().max_w_full().text_center().child(
                        "Double-click a ship in the market to start a fit, or import one \
                             from the File menu.",
                    ))
                    .into_any_element()
            })
            .relative()
            .when(self.tab_dragging, |center| {
                center.child(Self::tear_off_zone(cx))
            });

        let title = match self.fit() {
            Some(fit) => match fit.ship {
                Some(ship) => format!("efa: {} ({})", fit.name, loaded.catalog.name(ship)),
                None => format!("efa: {}", fit.name),
            },
            None => "efa".to_owned(),
        };
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }

        let narrow = window.viewport_size().width < px(NARROW_WIDTH);
        root.capture_any_mouse_up(cx.listener(|g, _: &MouseUpEvent, _, cx| {
            // A tab let go anywhere, dropped or not, ends the drag; the zone goes away.
            if g.tab_dragging {
                g.tab_dragging = false;
                cx.notify();
            }
        }))
        .child(self.menu_bar(window, cx))
        .children(self.update_notice.clone())
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
                                    .child(self.browser(&loaded, narrow, cx)),
                            )
                            .child(resizable_panel().child(center)),
                    ),
                )
                // pyfa's stats pane is as wide as its content and does not resize. A narrow
                // window has no room for it next to the fit; it is a browser tab there.
                .when(!narrow, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .w(px(stats::PANE_WIDTH))
                            .h_full()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .child(self.stats_panel(&loaded, cx)),
                    )
                }),
        )
        .child(self.status_bar(cx))
        .into_any_element()
    }
}

/// Inputs that the views embed, exposed for them.
impl Efa {
    fn search_input(&self) -> Input {
        Input::new(&self.search).small()
    }

    fn results_filter_input(&self) -> Input {
        Input::new(&self.results_filter).small().cleanable(true)
    }

    fn fit_search_input(&self) -> Input {
        Input::new(&self.fit_search).small()
    }

    fn name_field(&self) -> Input {
        Input::new(&self.name_input).small()
    }

    fn notes_field(&self) -> Textarea {
        Textarea::new(&self.notes_input)
    }

    fn app_update(
        weak: &WeakEntity<Self>,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        let _ = weak.update(cx, f);
    }
}
