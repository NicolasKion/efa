//! The fitting wizard: an overlay over nearly the whole window that walks through three steps,
//! the loadout, the tank and the rigs, with a live preview beside them. A step adds to the fit
//! only once something on it changed, so the tank step alone fills the free slots around what is
//! fitted. Each step opens with a few plain words on what the fit has and what the step fills,
//! then reads top to bottom as sections of rows: a fixed label column on the left and the
//! controls filling the rest. Every module row picks its own variation; the tank step can search
//! for the best tank off the UI thread and lists what it found as a table. The preview shows the
//! numbers before and after, what goes where and what it costs. The picking is in
//! [`crate::wizard`].

use std::{fmt::Write as _, rc::Rc, sync::Arc, time::Duration};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{Action, Efa, Loaded, item_stats, widgets};
use crate::{
    data::{Slot, TypeId},
    engine::{Cap, Layer as LayerStats, Pilot, Resource, Stats},
    fit::{DamageProfile, Fit},
    icons::Icon,
    item_data::ItemData,
    prices::Prices,
    theme::Palette,
    wizard::{
        self, Ammo, Budget, Category, Choices, Cleared, DamageMods, Facts, Family, Goal, Layer,
        Pick, Prop, Rigs, SearchGroup, Setup, Tab, Tabs, TankStyle, Tier, Utility,
        tank::{self, Found, Limit, Outcome, Progress},
    },
};

/// Room the overlay leaves to each edge of the window, and in a small window.
const INSET: f32 = 24.;
/// The dialog keeps at least 16 px clear of every edge itself.
const SMALL_INSET: f32 = 16.;
const PREVIEW_W: f32 = 320.;
/// Below this window width the preview folds behind a button in the footer.
const FOLD_PREVIEW_W: f32 = 900.;
/// Below this window width rows put their controls under their names, and the step list and
/// footer keep only what they need.
const COMPACT_W: f32 = 640.;
/// What the ship's name keeps beside the steps before they drop under it.
const SHIP_MIN_W: f32 = 180.;

/// Room the overlay leaves to the window's edges: less in a small window.
fn inset(window: &Window) -> f32 {
    let size = window.viewport_size();
    if size.width < px(COMPACT_W) || size.height < px(480.) {
        SMALL_INSET
    } else {
        INSET
    }
}
const ICON: f32 = 16.;
/// Icons in the results table's module strip.
const STRIP_ICON: f32 = 20.;
/// The label column every row lines up on.
const LABEL_W: f32 = 150.;
/// The height of a line of controls, which the label centres on.
const LINE_H: f32 = 28.;
/// The narrowest a variant dropdown gets before it wraps under the row's other controls.
const VARIANT_MIN_W: f32 = 180.;
/// The widest a variant dropdown gets, so a long row does not stretch it across the window.
const VARIANT_MAX_W: f32 = 380.;
/// A CPU or powergrid cell of the results table.
const RESOURCE_W: f32 = 64.;
/// A value column of the before and after table.
const VALUE_W: f32 = 64.;
/// Tanks the optimiser shows.
const RESULTS: usize = 5;
/// Groups of fitted modules an intro names before it says "and N more".
const INTRO_NAMES: usize = 3;

/// Dogma attributes the rows describe.
mod attr {
    pub const CAP_NEED: i32 = 6;
    pub const SPEED_FACTOR: i32 = 20;
    pub const SIG_BLOOM: i32 = 554;
    pub const UPGRADE_COST: i32 = 1153;
}

/// The rows of choices, in keyboard order down a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Tier,
    /// Whether a step fills the free slots or empties its racks first.
    Racks(Tab),
    Weapon,
    Ammo,
    Prop,
    Utility(Utility),
    Style,
    Layer,
    Goal,
    /// How many repairers or boosters an active tank gets.
    Repairers,
    /// Which variations the tank search tries.
    Tiers,
    /// The most one tank module or rig may cost.
    BudgetEach,
    /// The most the whole tank may cost.
    BudgetTotal,
    Search,
    Results,
    /// Opens or closes the tank's module rows.
    FineTune,
    /// A tank module, by its T1 item.
    Module(TypeId),
    Damage,
    Rigs,
    /// A rig, by its T1 item.
    Rig(TypeId),
}

impl Row {
    fn key(self) -> String {
        format!("{self:?}")
    }

    /// The step a change on the row puts on the fit; None for the bling, which every step uses.
    fn tab(self) -> Option<Tab> {
        match self {
            Row::Tier => None,
            Row::Racks(tab) => Some(tab),
            Row::Weapon | Row::Ammo | Row::Prop | Row::Utility(_) | Row::Damage => {
                Some(Tab::Loadout)
            }
            Row::Style
            | Row::Layer
            | Row::Goal
            | Row::Repairers
            | Row::Tiers
            | Row::BudgetEach
            | Row::BudgetTotal
            | Row::Search
            | Row::Results
            | Row::FineTune
            | Row::Module(_) => Some(Tab::Tank),
            Row::Rigs | Row::Rig(_) => Some(Tab::Rigs),
        }
    }

    /// Whether the row's description goes under its controls rather than in a tooltip.
    fn describes_inline(self) -> bool {
        matches!(
            self,
            Row::Racks(_)
                | Row::Weapon
                | Row::Ammo
                | Row::Prop
                | Row::Style
                | Row::Goal
                | Row::Search
                | Row::Damage
                | Row::Rigs
        )
    }
}

/// A titled group of rows on a step. Numbered sections are the steps' sequence; the others are
/// the setup above them and what comes out below.
struct Section {
    number: Option<u8>,
    title: &'static str,
    /// A line under the title, or what the section says in place of rows.
    note: Option<SharedString>,
    /// A row the title itself stands for, which opens and closes the section.
    toggle: Option<Row>,
    rows: Vec<Row>,
}

impl Section {
    fn new(number: Option<u8>, title: &'static str, rows: Vec<Row>) -> Self {
        Self {
            number,
            title,
            note: None,
            toggle: None,
            rows,
        }
    }

    fn note(mut self, note: impl Into<SharedString>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// Where a step stands, for the step indicator.
enum StepState {
    /// Untouched: the step adds nothing.
    Skipped,
    /// What the step adds, as "Adds 6 highs and 3 mids".
    Adds(String),
    /// The step's racks are full.
    NothingFree,
    /// Touched, but the choices add nothing.
    Nothing,
}

impl StepState {
    fn text(&self) -> SharedString {
        match self {
            StepState::Skipped => "Skipped".into(),
            StepState::Adds(text) => text.clone().into(),
            StepState::NothingFree => "Nothing free".into(),
            StepState::Nothing => "Adds nothing".into(),
        }
    }
}

/// A running tank search. Dropping it calls the search off.
struct Search {
    progress: Arc<Progress>,
    _task: Task<()>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

pub(super) struct WizardPanel {
    efa: WeakEntity<Efa>,
    loaded: Rc<Loaded>,
    data: Rc<ItemData>,
    pilot: Rc<Pilot>,
    prices: Rc<Prices>,
    /// The fit as it was when the wizard opened.
    fit: Fit,
    /// Its stats, for the preview's before column.
    before: Stats,
    facts: Facts,
    families: Vec<Family>,
    choices: Choices,
    picks: Vec<Pick>,
    stats: Stats,
    /// "Make it fit" changed the picks; a new choice starts over from the choices.
    made_to_fit: bool,
    /// The row the keyboard is on, an index into [`WizardPanel::rows`].
    row: usize,
    search: Option<Search>,
    /// Prices are being fetched ahead of the search of this generation.
    fetching: Option<u64>,
    /// Tells a finished search from one that was called off or replaced.
    generation: u64,
    results: Vec<Outcome>,
    /// The choices the results were found for, without the tank they picked.
    results_for: Option<Choices>,
    selected: Option<usize>,
    /// Why the last search found nothing, or what it is waiting for.
    note: Option<SharedString>,
    /// What the last search dropped the most tanks for not fitting.
    limit: Option<Limit>,
    /// Fewer repairers than chosen fit, and this many did.
    repairers_short: Option<usize>,
    /// The tank's module rows are open.
    fine_tune: bool,
    /// The window is too narrow for the preview beside the step, and too narrow for rows'
    /// names beside their controls; set on every render.
    fold_preview: bool,
    compact: bool,
    /// With the preview folded, it shows in place of the step.
    show_preview: bool,
    focus: FocusHandle,
    focused: bool,
    palette: Palette,
    ship: SharedString,
    tab: Tab,
    /// Custom budgets, typed as "75m" or "1.5b", next to the presets.
    budget_each: Entity<InputState>,
    budget_total: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Efa {
    /// Open the wizard over the current fit. It needs a ship, and the item data for the hull's
    /// bonuses and the modules' fitting costs, which may still be loading the first time.
    pub(super) fn open_wizard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        if self.fit().and_then(|f| f.ship).is_none() {
            self.status = Some("The fitting wizard needs a ship first".into());
            cx.notify();
            return;
        }
        let Some(data) = item_stats::hover_data(&loaded, cx) else {
            self.status = Some("Loading item data for the fitting wizard…".into());
            cx.notify();
            let this = cx.entity().downgrade();
            let window_handle = window.window_handle();
            cx.spawn(async move |_, cx| {
                for _ in 0..600 {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let ready = cx.update(|cx| item_stats::hover_data(&loaded, cx).is_some());
                    if ready {
                        let _ = window_handle.update(cx, |_, window, cx| {
                            Efa::app_update(&this, cx, |g, cx| {
                                g.status = None;
                                g.open_wizard(window, cx);
                            });
                        });
                        return;
                    }
                }
            })
            .detach();
            return;
        };
        let Some(fit) = self.fit().cloned() else {
            return;
        };
        let pilot = self.pilot(&loaded);
        let Some(facts) = Facts::gather(&loaded.engine, &data, &loaded.catalog, &fit, &pilot)
        else {
            return;
        };
        let families = wizard::families(&loaded.catalog, &data, &facts);
        let mut choices = Setup::new(&loaded.catalog, &data, &facts, &families).defaults();
        choices.bling = self.saved.wizard.clone();
        // Nothing goes on until a step's choices change.
        choices.touched = Tabs::NONE;
        let tab = self.saved.wizard_tab;
        let efa = cx.entity().downgrade();
        let palette = self.palette;
        let prices = Rc::clone(&self.prices);
        let ship = loaded.catalog.name(facts.ship).into();
        let before = wizard::preview(&loaded.engine, &pilot, &fit, &[], Cleared::NONE);
        let panel = cx.new(|cx| {
            let input = |cx: &mut Context<WizardPanel>, window: &mut Window| {
                cx.new(|cx| InputState::new(window, cx).placeholder("Custom, e.g. 75m"))
            };
            let budget_each = input(cx, window);
            let budget_total = input(cx, window);
            // Typing a custom budget sets it; text that is not an amount leaves it as it was.
            let on_type = |cx: &mut Context<WizardPanel>,
                           field: &Entity<InputState>,
                           set: fn(&mut Choices, Option<u64>)| {
                cx.subscribe(
                    field,
                    move |this: &mut WizardPanel, input, event: &InputEvent, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let text = input.read(cx).value().to_string();
                        let amount = if text.trim().is_empty() {
                            None
                        } else {
                            match Budget::parse(&text) {
                                Some(isk) => Some(isk),
                                None => return,
                            }
                        };
                        this.change(Some(Tab::Tank), cx, |c| set(c, amount));
                    },
                )
            };
            let subscriptions = vec![
                on_type(cx, &budget_each, |c, v| c.budget.each = v),
                on_type(cx, &budget_total, |c, v| c.budget.total = v),
            ];
            let mut panel = WizardPanel {
                efa,
                loaded,
                data,
                pilot,
                prices,
                fit,
                before,
                facts,
                families,
                choices,
                picks: Vec::new(),
                stats: Stats::default(),
                made_to_fit: false,
                row: 0,
                search: None,
                fetching: None,
                generation: 0,
                results: Vec::new(),
                results_for: None,
                selected: None,
                note: None,
                limit: None,
                repairers_short: None,
                fine_tune: false,
                fold_preview: false,
                compact: false,
                show_preview: false,
                focus: cx.focus_handle(),
                focused: false,
                palette,
                ship,
                tab,
                budget_each,
                budget_total,
                _subscriptions: subscriptions,
            };
            panel.replan();
            panel
        });
        // The panel draws its own header and padding, so the overlay is all of it.
        window.open_dialog(cx, move |dialog, window, _| {
            let inset = inset(window);
            dialog
                .w(window.viewport_size().width - px(2. * inset))
                .margin_top(px(inset))
                .p_0()
                .child(panel.clone())
        });
    }
}

impl WizardPanel {
    fn setup(&self) -> Setup<'_> {
        Setup::new(
            &self.loaded.catalog,
            &self.data,
            &self.facts,
            &self.families,
        )
    }

    /// Plan again from the choices and preview the result.
    fn replan(&mut self) {
        let picks = self.setup().plan(&self.fit, &self.choices);
        self.set_picks(picks);
        self.made_to_fit = false;
    }

    fn set_picks(&mut self, picks: Vec<Pick>) {
        let placements = self
            .setup()
            .placements(&self.fit, &picks, self.choices.cleared());
        self.stats = wizard::preview(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &placements,
            self.choices.cleared(),
        );
        self.picks = picks;
    }

    fn make_it_fit(&mut self, cx: &mut Context<Self>) {
        let picks = self.setup().make_it_fit(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &self.picks,
            self.choices.cleared(),
        );
        self.set_picks(picks);
        self.made_to_fit = true;
        cx.notify();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        let clear = self.choices.cleared();
        let placements = self.setup().placements(&self.fit, &self.picks, clear);
        let bling = self.choices.bling.clone();
        window.close_dialog(cx);
        Efa::app_update(&self.efa, cx, |g, cx| {
            g.saved.wizard = bling;
            g.apply(Action::ApplyWizard { placements, clear }, cx);
        });
    }

    /// Change the choices; `touch` is the step whose choices that puts on the fit.
    fn change(&mut self, touch: Option<Tab>, cx: &mut Context<Self>, f: impl FnOnce(&mut Choices)) {
        let before = self.choices.clone();
        f(&mut self.choices);
        if let Some(tab) = touch {
            self.choices.touched.set(tab, true);
        }
        // A new weapon kind brings its own damage modules.
        if self.choices.family != before.family && self.choices.damage == DamageMods::None {
            self.choices.damage = before.damage;
        }
        // Another kind of tank makes the found ones moot.
        if self.choices.tank != before.tank || self.choices.layer != before.layer {
            self.choices.tank_fit = None;
            self.results.clear();
            self.results_for = None;
            self.selected = None;
            self.note = None;
            self.limit = None;
            self.search = None;
            self.stop_fetching();
        }
        if self.choices != before || self.made_to_fit {
            self.replan();
        }
        cx.notify();
    }

    /// The choices without the tank the optimiser picked, to tell whether results still hold.
    fn search_key(choices: &Choices) -> Choices {
        Choices {
            tank_fit: None,
            ..choices.clone()
        }
    }

    fn stale(&self) -> bool {
        self.results_for
            .as_ref()
            .is_some_and(|c| *c != Self::search_key(&self.choices))
    }

    // The tank search.

    fn busy(&self) -> bool {
        self.search.is_some() || self.fetching.is_some()
    }

    fn find_best(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        // Searching is a tank choice: the tank step goes on the fit.
        if !self.choices.touched.has(Tab::Tank) {
            self.change(Some(Tab::Tank), cx, |_| {});
        }
        let Some(problem) = self.setup().tank_problem(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &self.choices,
            &self.prices,
        ) else {
            return;
        };
        if problem.positions.is_empty() && problem.rig_positions.is_empty() {
            self.note = Some("No free slots or rigs are left for the tank.".into());
            cx.notify();
            return;
        }
        // Prices first, for every module and rig the search may try, so budgets and the price
        // column go by real numbers rather than whatever happened to be fetched already.
        let types: std::collections::HashSet<TypeId> = problem
            .candidates
            .iter()
            .map(|c| c.module.type_id)
            .chain(problem.rigs.iter().map(|r| r.type_id))
            .collect();
        self.generation += 1;
        let generation = self.generation;
        self.fetching = Some(generation);
        self.note = Some("Fetching prices for the modules the search may try...".into());
        cx.notify();
        let wizard = cx.entity().downgrade();
        Efa::app_update(&self.efa, cx, move |g, cx| {
            g.with_prices_for(types, cx, move |g, cx| {
                let prices = Rc::clone(&g.prices);
                // The wizard may be the one asking, so it is updated once this is over.
                cx.defer(move |cx| {
                    let _ = wizard.update(cx, |w, cx| {
                        if w.fetching == Some(generation) {
                            w.fetching = None;
                            w.prices = prices;
                            w.run_search(cx);
                        }
                    });
                });
            });
        });
    }

    /// The search itself, once prices are in.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }
        let Some(problem) = self.setup().tank_problem(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &self.choices,
            &self.prices,
        ) else {
            self.note = None;
            cx.notify();
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        let engine = Arc::clone(&self.loaded.engine);
        let progress = Arc::new(Progress::default());
        let shared = Arc::clone(&progress);
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let work =
                cx.background_spawn(
                    async move { tank::optimise(&engine, &problem, &shared, RESULTS) },
                );
            // Redraw the progress bar while the search runs.
            let ticker = {
                let this = this.clone();
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let running = this.update(cx, |p, cx| {
                            cx.notify();
                            p.search.is_some() && p.generation == generation
                        });
                        if !matches!(running, Ok(true)) {
                            break;
                        }
                    }
                })
            };
            let found = work.await;
            drop(ticker);
            let _ = this.update(cx, |p, cx| p.finish(generation, found, cx));
        });
        self.search = Some(Search {
            progress,
            _task: task,
        });
        self.note = None;
        cx.notify();
    }

    /// Stop waiting for prices: the answer, when it comes, starts no search.
    fn stop_fetching(&mut self) {
        if self.fetching.take().is_some() {
            self.generation += 1;
            self.note = None;
        }
    }

    fn cancel_search(&mut self, cx: &mut Context<Self>) {
        let fetching = self.fetching.is_some();
        self.stop_fetching();
        if self.search.take().is_some() || fetching {
            cx.notify();
        }
    }

    fn finish(&mut self, generation: u64, found: Found, cx: &mut Context<Self>) {
        if generation != self.generation || self.search.is_none() {
            return;
        }
        self.search = None;
        self.limit = found.rejected.limit();
        self.repairers_short = found.repairers_short;
        let outcomes = found.outcomes;
        if outcomes.is_empty() {
            self.note = Some(
                "No tank fits in the CPU, powergrid and calibration left. Try a lower bling \
                 tier, or fewer utilities."
                    .into(),
            );
            cx.notify();
            return;
        }
        self.results = outcomes;
        self.results_for = Some(Self::search_key(&self.choices));
        self.select(0, cx);
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(found) = self.results.get(i).map(Outcome::tank_fit) else {
            return;
        };
        self.selected = Some(i);
        self.change(Some(Tab::Tank), cx, |c| c.tank_fit = Some(found));
    }

    // Rows and what they hold.

    fn kind(&self) -> Option<wizard::WeaponKind> {
        self.setup().family(&self.choices).map(|f| f.kind)
    }

    fn base(&self, name: &str) -> Option<TypeId> {
        self.loaded.catalog.find(name).map(|t| t.id)
    }

    fn utility_base(&self, utility: Utility) -> Option<TypeId> {
        self.base(&self.setup().utility_name(utility, self.kind()))
    }

    fn utility_usable(&self, utility: Utility) -> bool {
        self.facts.total(utility.slot()) > 0
            && utility.fits(self.facts.size)
            && self.utility_base(utility).is_some()
    }

    fn prop_base(&self) -> Option<TypeId> {
        self.setup()
            .prop_name(self.choices.prop)
            .and_then(|n| self.base(&n))
    }

    fn damage_base(&self) -> Option<TypeId> {
        self.setup()
            .damage_mod_name(self.choices.damage, self.kind())
            .and_then(|n| self.base(n))
    }

    /// Distinct T1 items of the picks that `keep` lets through, in plan order, with their count.
    fn distinct(&self, keep: impl Fn(&Pick) -> bool) -> Vec<(TypeId, usize)> {
        let mut out: Vec<(TypeId, usize)> = Vec::new();
        for pick in self.picks.iter().filter(|p| keep(p)) {
            match out.iter_mut().find(|(b, _)| *b == pick.base) {
                Some((_, n)) => *n += 1,
                None => out.push((pick.base, 1)),
            }
        }
        out
    }

    fn count(&self, base: TypeId) -> usize {
        self.picks.iter().filter(|p| p.base == base).count()
    }

    /// Modules the other rows do not cover: the tank, and a damage control or cap booster the
    /// wizard adds on its own.
    fn module_rows(&self) -> Vec<(TypeId, usize)> {
        let covered: Vec<TypeId> = self
            .setup()
            .family(&self.choices)
            .map(|f| f.base)
            .into_iter()
            .chain(self.prop_base())
            .chain(self.damage_base())
            .chain(
                self.choices
                    .utilities
                    .iter()
                    .filter_map(|&u| self.utility_base(u)),
            )
            .collect();
        self.distinct(|p| p.slot != Slot::Rig && !covered.contains(&p.base))
    }

    fn rig_rows(&self) -> Vec<(TypeId, usize)> {
        self.distinct(|p| p.slot == Slot::Rig)
    }

    /// A step's sections, top to bottom. Whatever does not apply to the choices is left out.
    fn sections(&self, tab: Tab) -> Vec<Section> {
        let c = &self.choices;
        let setup = Section::new(None, "Starting point", vec![Row::Racks(tab), Row::Tier]);
        let mut out = vec![setup];
        match tab {
            Tab::Loadout => {
                let mut weapons = vec![Row::Weapon];
                if c.family.is_some() {
                    weapons.push(Row::Ammo);
                }
                out.push(Section::new(Some(1), "Weapons", weapons));
                out.push(Section::new(Some(2), "Propulsion", vec![Row::Prop]));
                out.push(Section::new(
                    Some(3),
                    "Tackle and utility",
                    Utility::ALL.into_iter().map(Row::Utility).collect(),
                ));
                out.push(Section::new(Some(4), "Damage modules", vec![Row::Damage]));
            }
            Tab::Tank => {
                if c.tank == TankStyle::None {
                    out.push(
                        Section::new(Some(1), "What kind of tank", vec![Row::Style]).note(
                            "Pick Buffer or Active to set up a tank; the goal, the search and \
                             its results follow.",
                        ),
                    );
                } else {
                    out.push(Section::new(
                        Some(1),
                        "What kind of tank",
                        vec![Row::Style, Row::Layer],
                    ));
                    let mut aim = vec![Row::Goal];
                    if c.tank == TankStyle::Active {
                        aim.push(Row::Repairers);
                    }
                    out.push(Section::new(Some(2), "What to aim for", aim));
                    out.push(Section::new(
                        Some(3),
                        "What it may use",
                        vec![Row::Tiers, Row::BudgetEach, Row::BudgetTotal],
                    ));
                    out.push(Section::new(
                        Some(4),
                        "Find the best tank",
                        vec![Row::Search],
                    ));
                    out.push(if self.results.is_empty() {
                        Section::new(None, "Results", Vec::new()).note(self.empty_results())
                    } else {
                        Section::new(None, "Results", vec![Row::Results])
                    });
                }
                let modules = self.module_rows();
                if !modules.is_empty() {
                    let n: usize = modules.iter().map(|(_, n)| n).sum();
                    let mut rows = vec![Row::FineTune];
                    if self.fine_tune {
                        rows.extend(modules.into_iter().map(|(b, _)| Row::Module(b)));
                    }
                    out.push(Section {
                        toggle: Some(Row::FineTune),
                        ..Section::new(None, "Fine tune", rows).note(format!(
                            "{} going on. Pick the variation of each.",
                            plural(n, "tank module")
                        ))
                    });
                }
            }
            Tab::Rigs => {
                if self.facts.total(Slot::Rig) == 0 {
                    out.push(
                        Section::new(Some(1), "What the rigs are for", Vec::new())
                            .note("This hull has no rig slots, so this step has nothing to do."),
                    );
                } else {
                    out.push(Section::new(
                        Some(1),
                        "What the rigs are for",
                        vec![Row::Rigs],
                    ));
                    let rigs: Vec<Row> = self
                        .rig_rows()
                        .into_iter()
                        .map(|(b, _)| Row::Rig(b))
                        .collect();
                    if rigs.is_empty() {
                        if c.rigs != Rigs::None {
                            out.push(
                                Section::new(Some(2), "The rigs", Vec::new())
                                    .note("No rigs go on with these choices."),
                            );
                        }
                    } else {
                        out.push(Section::new(Some(2), "The rigs", rigs));
                    }
                }
            }
        }
        out
    }

    /// What the results section says before there are results.
    fn empty_results(&self) -> SharedString {
        if self.busy() {
            "Searching. The best tanks show up here.".into()
        } else if let Some(note) = &self.note {
            note.clone()
        } else {
            "No results yet. Pick what to aim for, then Find best.".into()
        }
    }

    /// The current step's rows, in keyboard order.
    fn rows(&self) -> Vec<Row> {
        self.sections(self.tab)
            .into_iter()
            .flat_map(|s| s.rows)
            .collect()
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        self.row = 0;
        Efa::app_update(&self.efa, cx, |g, _| {
            g.saved.wizard_tab = tab;
            g.save();
        });
        cx.notify();
    }

    /// The step before or after the current one, if there is one.
    fn neighbour(&self, forward: bool) -> Option<Tab> {
        let at = Tab::ALL.iter().position(|&t| t == self.tab)?;
        if forward {
            Tab::ALL.get(at + 1).copied()
        } else {
            at.checked_sub(1).map(|i| Tab::ALL[i])
        }
    }

    /// The picks a step adds: the loadout's modules, the tank with its rigs, or every rig.
    fn tab_picks(&self, tab: Tab) -> impl Iterator<Item = &Pick> {
        self.picks.iter().filter(move |p| match tab {
            Tab::Loadout => !p.tank && p.slot != Slot::Rig,
            Tab::Tank => p.tank,
            Tab::Rigs => p.slot == Slot::Rig,
        })
    }

    /// Where a step stands: what it adds, or why it adds nothing.
    fn step_state(&self, tab: Tab) -> StepState {
        let counts: Vec<String> = [Slot::High, Slot::Mid, Slot::Low, Slot::Rig]
            .into_iter()
            .filter_map(|slot| {
                let n = self.tab_picks(tab).filter(|p| p.slot == slot).count();
                (n > 0).then(|| plural(n, slot_word(slot)))
            })
            .collect();
        if !counts.is_empty() {
            return StepState::Adds(format!("Adds {}", join_and(&counts, "and")));
        }
        let cleared = self.choices.cleared();
        let free = self.choices.racks(tab).iter().any(|&slot| {
            let others = self
                .picks
                .iter()
                .filter(|p| p.slot == slot && !self.tab_picks(tab).any(|q| std::ptr::eq(q, *p)))
                .count();
            self.setup().free(&self.fit, slot, cleared).len() > others
        });
        if !free {
            StepState::NothingFree
        } else if !self.choices.touched.has(tab) {
            StepState::Skipped
        } else {
            StepState::Nothing
        }
    }

    /// The racks a step fills: the tank's rack takes the rigs too when the tank takes them.
    fn step_racks(&self, tab: Tab) -> Vec<Slot> {
        let mut racks = self.choices.racks(tab).to_vec();
        if tab == Tab::Tank && Setup::tank_takes_rigs(&self.choices, self.kind()) {
            racks.push(Slot::Rig);
        }
        racks
    }

    /// The modules fitted in some racks, most first, by item and count.
    fn fitted(&self, racks: &[Slot]) -> Vec<(TypeId, usize)> {
        let mut out: Vec<(TypeId, usize)> = Vec::new();
        for &slot in racks {
            let ids: Vec<TypeId> = if slot == Slot::Rig {
                self.fit.rigs.iter().flatten().copied().collect()
            } else {
                self.fit
                    .rack(slot)
                    .iter()
                    .flatten()
                    .map(|m| m.type_id)
                    .collect()
            };
            for id in ids {
                match out.iter_mut().find(|(t, _)| *t == id) {
                    Some((_, n)) => *n += 1,
                    None => out.push((id, 1)),
                }
            }
        }
        out.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        out
    }

    /// A few plain sentences that open a step: what the fit already has where the step goes,
    /// and what the step fills.
    fn intro(&self, tab: Tab) -> String {
        let racks = self.step_racks(tab);
        let rack_words: Vec<String> = racks.iter().map(|&s| rack_name(s).to_owned()).collect();
        let fitted = self.fitted(&racks);
        let has = if fitted.is_empty() {
            format!(
                "Your fit has nothing in its {} yet.",
                join_and(&rack_words, "or")
            )
        } else {
            let catalog = &self.loaded.catalog;
            let mut names: Vec<String> = fitted
                .iter()
                .take(INTRO_NAMES)
                .map(|&(id, n)| format!("{n}x {}", catalog.name(id)))
                .collect();
            let rest: usize = fitted.iter().skip(INTRO_NAMES).map(|(_, n)| n).sum();
            if rest > 0 {
                names.push(format!("{rest} more"));
            }
            format!(
                "Your fit has {} in its {}.",
                join_and(&names, "and"),
                join_and(&rack_words, "and")
            )
        };
        let purpose = match tab {
            Tab::Loadout => "weapons, propulsion, tackle and damage modules".to_owned(),
            Tab::Tank => format!(
                "{} {} tank",
                article(self.choices.tank.label()),
                self.choices.layer.label().to_lowercase()
            )
            .to_lowercase(),
            Tab::Rigs => "rigs for damage or for the tank".to_owned(),
        };
        let fills = if tab == Tab::Tank && self.choices.tank == TankStyle::None {
            format!(
                "With no tank picked, this step adds nothing and leaves the {} to the loadout.",
                join_and(&rack_words, "and")
            )
        } else if self.choices.clear.has(tab) {
            let total: usize = racks.iter().map(|&s| self.facts.total(s)).sum();
            format!(
                "This step empties the {} first and fills all {} anew with {purpose}.",
                join_and(&rack_words, "and"),
                plural(total, "slot")
            )
        } else {
            let parts: Vec<String> = racks
                .iter()
                .filter_map(|&slot| {
                    let n = self.setup().free(&self.fit, slot, Cleared::NONE).len();
                    (n > 0).then(|| match slot {
                        Slot::Rig => format!("the {}", plural(n, "free rig slot")),
                        _ => format!(
                            "the {}",
                            plural(n, &format!("free {} slot", slot_word(slot)))
                        ),
                    })
                })
                .collect();
            if parts.is_empty() {
                format!(
                    "Every slot here is taken. Set Slots to Clear first to refit the {}.",
                    join_and(&rack_words, "and")
                )
            } else {
                format!(
                    "This step fills {} with {purpose}.",
                    join_and(&parts, "and")
                )
            }
        };
        let mut text = format!("{has} {fills}");
        if tab == Tab::Rigs && Setup::tank_takes_rigs(&self.choices, self.kind()) {
            text.push_str(" With these choices the tank step picks the rigs.");
        }
        if tab == Tab::Rigs && self.facts.total(Slot::Rig) > 0 {
            let cal = &self.before.calibration;
            let left = if self.choices.clear.has(Tab::Rigs) {
                cal.output.unwrap_or(0.0)
            } else {
                cal.output.unwrap_or(0.0) - cal.used
            };
            let _ = write!(text, " {left:.0} calibration is free.");
        }
        text
    }

    fn current_row(&self) -> Option<Row> {
        let rows = self.rows();
        rows.get(self.row.min(rows.len().saturating_sub(1)))
            .copied()
    }

    /// A row's options as a segmented control: labels, which can be picked, and the current one.
    fn segments(&self, row: Row) -> Option<(Vec<(SharedString, bool)>, usize)> {
        let c = &self.choices;
        let has_mids = self.facts.total(Slot::Mid) > 0;
        Some(match row {
            Row::Tier => (
                Tier::ALL.iter().map(|t| (t.label().into(), true)).collect(),
                Tier::ALL.iter().position(|&t| t == c.bling.tier)?,
            ),
            Row::Racks(tab) => (
                vec![("Fill free".into(), true), ("Clear first".into(), true)],
                usize::from(c.clear.has(tab)),
            ),
            Row::Ammo => (
                vec![
                    ("Faction".into(), true),
                    ("T1".into(), true),
                    ("None".into(), true),
                ],
                Ammo::ALL.iter().position(|&a| a == c.ammo)?,
            ),
            Row::Prop => (
                vec![
                    ("MWD".into(), has_mids),
                    ("AB".into(), has_mids),
                    ("None".into(), true),
                ],
                Prop::ALL.iter().position(|&p| p == c.prop)?,
            ),
            Row::Style => (
                TankStyle::ALL
                    .iter()
                    .map(|s| (s.label().into(), true))
                    .collect(),
                TankStyle::ALL.iter().position(|&s| s == c.tank)?,
            ),
            Row::Layer => (
                Layer::ALL
                    .iter()
                    .map(|l| (l.label().into(), self.facts.total(l.slot()) > 0))
                    .collect(),
                Layer::ALL.iter().position(|&l| l == c.layer)?,
            ),
            Row::BudgetEach if c.tank != TankStyle::None => (
                Budget::EACH
                    .iter()
                    .map(|&b| (Budget::label(b).into(), true))
                    .collect(),
                Budget::EACH
                    .iter()
                    .position(|&b| b == c.budget.each)
                    .unwrap_or(usize::MAX),
            ),
            Row::BudgetTotal if c.tank != TankStyle::None => (
                Budget::TOTAL
                    .iter()
                    .map(|&b| (Budget::label(b).into(), true))
                    .collect(),
                Budget::TOTAL
                    .iter()
                    .position(|&b| b == c.budget.total)
                    .unwrap_or(usize::MAX),
            ),
            Row::Repairers if c.tank == TankStyle::Active => (
                REPAIRER_CHOICES
                    .iter()
                    .map(|&n| (repairer_label(n).into(), true))
                    .collect(),
                REPAIRER_CHOICES
                    .iter()
                    .position(|&n| n == c.repairers)
                    .unwrap_or(0),
            ),
            // Switches rather than one choice: each group is on or off.
            Row::Tiers if c.tank != TankStyle::None => (
                SearchGroup::ALL
                    .iter()
                    .map(|g| (g.label().into(), true))
                    .collect(),
                0,
            ),
            Row::Goal if c.tank != TankStyle::None => {
                let goals = Goal::for_style(c.tank);
                (
                    goals.iter().map(|g| (g.label().into(), true)).collect(),
                    goals.iter().position(|&g| g == c.goal).unwrap_or(0),
                )
            }
            Row::Damage => {
                let kind = self.kind();
                let lows = self.facts.total(Slot::Low) > 0;
                (
                    DamageMods::ALL
                        .iter()
                        .map(|&d| {
                            let label = match d {
                                DamageMods::Weapon => "Weapon",
                                DamageMods::Drone => "Drone",
                                DamageMods::None => "None",
                            };
                            let usable = d == DamageMods::None
                                || lows && self.setup().damage_mod_name(d, kind).is_some();
                            (label.into(), usable)
                        })
                        .collect(),
                    DamageMods::ALL.iter().position(|&d| d == c.damage)?,
                )
            }
            Row::Rigs => (
                Rigs::ALL
                    .iter()
                    .map(|&r| {
                        (
                            r.label().into(),
                            r == Rigs::None || self.facts.total(Slot::Rig) > 0,
                        )
                    })
                    .collect(),
                Rigs::ALL.iter().position(|&r| r == c.rigs)?,
            ),
            Row::Weapon => (
                self.families
                    .iter()
                    .map(|f| (f.name.clone().into(), true))
                    .chain(std::iter::once(("None".into(), true)))
                    .collect(),
                c.family.unwrap_or(self.families.len()),
            ),
            Row::Results => (
                (1..=self.results.len())
                    .map(|i| (i.to_string().into(), true))
                    .collect(),
                self.selected.unwrap_or(0),
            ),
            _ => return None,
        })
    }

    /// What an option of a segmented row means, when its label is short or it cannot be picked.
    fn segment_tip(&self, row: Row, i: usize, usable: bool) -> Option<&'static str> {
        if !usable {
            return Some(match row {
                Row::Prop => "This hull has no mid slots.",
                Row::Layer if Layer::ALL[i] == Layer::Shield => "This hull has no mid slots.",
                Row::Layer => "This hull has no low slots.",
                Row::Damage if self.facts.total(Slot::Low) == 0 => "This hull has no low slots.",
                Row::Damage => "No damage module goes with these weapons.",
                Row::Rigs => "This hull has no rig slots.",
                _ => return None,
            });
        }
        Some(match row {
            Row::Prop => ["Microwarpdrive", "Afterburner", "No propulsion module"][i],
            Row::Ammo => [
                "Navy or pirate ammo",
                "Tech 1 ammo",
                "The weapons go on unloaded",
            ][i],
            Row::Tier => [
                "Tech 1",
                "The best meta variation, usually the compact one",
                "Tech 2",
                "Navy, pirate and storyline modules",
                "Deadspace modules",
                "Officer modules",
            ][i],
            Row::Tiers => [
                "Tech 1 modules",
                "Meta and compact variations of the Tech 1 module",
                "Tech 2 modules",
                "Navy and pirate modules",
                "Storyline modules",
                "Deadspace modules, which get expensive",
                "Officer modules, which get very expensive",
                "Tech 2 rigs; Tech 1 rigs are always tried",
            ][i],
            Row::BudgetEach | Row::BudgetTotal if i == 0 => "No limit",
            Row::Repairers if i == 0 => "The search decides how many",
            Row::Results => "Fit this tank",
            _ => return None,
        })
    }

    /// Pick option `i` of a row.
    fn set_segment(&mut self, row: Row, i: usize, cx: &mut Context<Self>) {
        if self
            .segments(row)
            .is_none_or(|(options, _)| !options.get(i).is_some_and(|o| o.1))
        {
            return;
        }
        let families = self.families.len();
        match row {
            Row::Results => self.select(i, cx),
            _ => self.change(row.tab(), cx, |c| match row {
                Row::Tier => c.bling.set_tier(Tier::ALL[i]),
                Row::Racks(tab) => c.clear.set(tab, i == 1),
                Row::Weapon => c.family = (i < families).then_some(i),
                Row::Ammo => c.ammo = Ammo::ALL[i],
                Row::Prop => c.prop = Prop::ALL[i],
                Row::Style => c.tank = TankStyle::ALL[i],
                Row::Layer => c.layer = Layer::ALL[i],
                Row::Tiers => c.search_tiers.toggle(SearchGroup::ALL[i]),
                Row::Repairers => c.repairers = REPAIRER_CHOICES[i],
                Row::BudgetEach => c.budget.each = Budget::EACH[i],
                Row::BudgetTotal => c.budget.total = Budget::TOTAL[i],
                Row::Goal => {
                    if let Some(&goal) = Goal::for_style(c.tank).get(i) {
                        c.goal = goal;
                    }
                }
                Row::Damage => c.damage = DamageMods::ALL[i],
                Row::Rigs => c.rigs = Rigs::ALL[i],
                _ => {}
            }),
        }
    }

    /// One option back or forward, around the ends, past what cannot be picked.
    fn step(&mut self, row: Row, back: bool, cx: &mut Context<Self>) {
        match row {
            Row::Utility(u) => {
                if self.utility_usable(u) {
                    self.change(row.tab(), cx, |c| c.toggle(u));
                }
                return;
            }
            Row::Module(_) | Row::Rig(_) => {
                self.step_variant(row, back, cx);
                return;
            }
            Row::Search => {
                self.find_best(cx);
                return;
            }
            Row::FineTune => {
                self.fine_tune = !self.fine_tune;
                cx.notify();
                return;
            }
            _ => {}
        }
        let Some((options, current)) = self.segments(row) else {
            return;
        };
        let len = options.len().max(1);
        let mut at = current;
        for _ in 0..len {
            at = if back {
                (at + len - 1) % len
            } else {
                (at + 1) % len
            };
            if options.get(at).is_some_and(|o| o.1) {
                break;
            }
        }
        self.set_segment(row, at, cx);
    }

    /// The module a row picks a variation for: its kind, T1 item and the variation planned.
    fn variant_of(&self, row: Row) -> Option<(Category, TypeId, TypeId)> {
        let (category, base) = match row {
            Row::Weapon => (Category::Weapon, self.setup().family(&self.choices)?.base),
            Row::Prop => (Category::Propulsion, self.prop_base()?),
            Row::Utility(u) if self.choices.has(u) => (u.category(), self.utility_base(u)?),
            Row::Module(base) => (tank::category_of(&self.loaded.catalog, base), base),
            Row::Damage => (Category::Damage, self.damage_base()?),
            Row::Rig(base) => (Category::Rig, base),
            _ => return None,
        };
        let current = self.picks.iter().find(|p| p.base == base).map_or_else(
            || self.setup().pick(base, category, &self.choices.bling),
            |p| p.type_id,
        );
        Some((category, base, current))
    }

    fn set_variant(&mut self, row: Row, id: TypeId, cx: &mut Context<Self>) {
        let Some((category, base, _)) = self.variant_of(row) else {
            return;
        };
        let tier = Tier::of(&self.loaded.catalog, id, base);
        self.change(row.tab(), cx, |c| c.bling.choose(category, base, id, tier));
    }

    fn step_variant(&mut self, row: Row, back: bool, cx: &mut Context<Self>) {
        let Some((_, base, current)) = self.variant_of(row) else {
            return;
        };
        let variants = self.setup().variants(base);
        let len = variants.len();
        if len < 2 {
            return;
        }
        let at = variants.iter().position(|&v| v == current).unwrap_or(0);
        let next = if back {
            (at + len - 1) % len
        } else {
            (at + 1) % len
        };
        self.set_variant(row, variants[next], cx);
    }

    fn focus_row(&mut self, row: Row, cx: &mut Context<Self>) {
        if let Some(i) = self.rows().iter().position(|&r| r == row) {
            self.row = i;
            cx.notify();
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Keys typed into a text field are the field's, not the wizard's shortcuts.
        if !self.focus.is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.modifiers.alt {
            return;
        }
        if keystroke.modifiers.control {
            let at = Tab::ALL.iter().position(|&t| t == self.tab).unwrap_or(0);
            let len = Tab::ALL.len();
            let tab = match keystroke.key.as_str() {
                "tab" if keystroke.modifiers.shift => Tab::ALL[(at + len - 1) % len],
                "tab" => Tab::ALL[(at + 1) % len],
                key => match key.parse::<usize>() {
                    Ok(n @ 1..=3) => Tab::ALL[n - 1],
                    _ => return,
                },
            };
            self.set_tab(tab, cx);
            cx.stop_propagation();
            return;
        }
        let rows = self.rows();
        let count = rows.len().max(1);
        self.row = self.row.min(count - 1);
        let Some(row) = rows.get(self.row).copied() else {
            return;
        };
        match keystroke.key.as_str() {
            "enter" => self.apply(window, cx),
            "up" => {
                self.row = (self.row + count - 1) % count;
                cx.notify();
            }
            "down" | "tab" => {
                self.row = (self.row + 1) % count;
                cx.notify();
            }
            "left" | "right" => self.step(row, keystroke.key == "left", cx),
            "space" => match row {
                Row::Utility(_) | Row::Search | Row::FineTune => self.step(row, false, cx),
                _ => return,
            },
            "[" | "]" => self.step_variant(row, keystroke.key == "[", cx),
            "f" if self.choices.tank != TankStyle::None => self.find_best(cx),
            "escape" if self.busy() => self.cancel_search(cx),
            "m" if !wizard::fits(&self.stats) => self.make_it_fit(cx),
            key => {
                if let Ok(n) = key.parse::<usize>()
                    && n >= 1
                {
                    self.set_segment(row, n - 1, cx);
                } else {
                    return;
                }
            }
        }
        cx.stop_propagation();
    }

    // Drawing.

    /// Give the keyboard back to the panel after a click on a control.
    fn refocus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    fn segmented(&self, row: Row, cx: &Context<Self>) -> AnyElement {
        let Some((options, current)) = self.segments(row) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let segments: Vec<AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(i, (label, usable))| {
                let selected = if row == Row::Tiers {
                    self.choices.search_tiers.has(SearchGroup::ALL[i])
                } else {
                    i == current
                };
                let tip = self.segment_tip(row, i, usable);
                div()
                    .id(SharedString::from(format!("wizard-{}-{i}", row.key())))
                    .px_2p5()
                    .py_0p5()
                    .rounded(px(5.))
                    .text_xs()
                    .when(selected, |d| {
                        d.bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!selected && usable, |d| {
                        d.text_color(theme.muted_foreground)
                            .hover(|s| s.bg(theme.secondary).text_color(theme.foreground))
                    })
                    .when(!usable, |d| {
                        d.text_color(theme.muted_foreground).opacity(0.4)
                    })
                    // Search kinds are switches: a check marks those that are on.
                    .when(row == Row::Tiers, |d| {
                        d.flex().items_center().gap_1().child(
                            UiIcon::new(if selected {
                                IconName::Check
                            } else {
                                IconName::Plus
                            })
                            .xsmall(),
                        )
                    })
                    .child(label)
                    .when_some(tip, |d, tip| {
                        d.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                    })
                    .when(usable, |d| {
                        d.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &gpui_kit::MouseDownEvent, window, cx| {
                                this.focus_row(row, cx);
                                this.set_segment(row, i, cx);
                                this.refocus(window, cx);
                            }),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        h_flex()
            .flex_wrap()
            .gap_0p5()
            .p_0p5()
            .rounded(px(7.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .children(segments)
            .into_any_element()
    }

    /// A dropdown of a module's variations, with tier, meta level and price.
    fn variant_button(&self, row: Row, cx: &Context<Self>) -> Option<AnyElement> {
        let (_, base, current) = self.variant_of(row)?;
        let catalog = &self.loaded.catalog;
        let variants = self.setup().variants(base);
        let entries: Vec<(TypeId, String, String, Option<f64>)> = variants
            .iter()
            .map(|&id| {
                let level = catalog.get(id).map_or(0, |t| t.meta_level);
                (
                    id,
                    catalog.name(id),
                    format!("{}, meta {level}", Tier::of(catalog, id, base).label()),
                    self.prices.get(id),
                )
            })
            .collect();
        let this = cx.entity().downgrade();
        let loaded = Rc::clone(&self.loaded);
        let price = self
            .prices
            .get(current)
            .map(|p| format!("{} ISK each", widgets::short(p)));
        let tip: SharedString = match (&price, variants.len() < 2) {
            (_, true) => "The only variation".into(),
            (Some(price), false) => format!("{price}. [ and ] step through the variations.").into(),
            (None, false) => "[ and ] step through the variations.".into(),
        };
        Some(
            Button::new(SharedString::from(format!("wizard-variant-{}", row.key())))
                .outline()
                .xsmall()
                .tab_stop(false)
                .dropdown_caret(true)
                .flex_1()
                .min_w(px(VARIANT_MIN_W))
                .max_w(px(VARIANT_MAX_W))
                .label(catalog.name(current))
                .tooltip(tip)
                .disabled(variants.len() < 2)
                .dropdown_menu(move |menu, _, _| {
                    entries
                        .iter()
                        .fold(menu, |menu, (id, name, detail, price)| {
                            let id = *id;
                            let this = this.clone();
                            let entry = variant_entry(&loaded, id, name, detail, *price);
                            menu.item(entry.checked(id == current).on_click(
                                move |_, window, cx| {
                                    let _ = this.update(cx, |p, cx| {
                                        p.focus_row(row, cx);
                                        p.set_variant(row, id, cx);
                                        p.refocus(window, cx);
                                    });
                                },
                            ))
                        })
                })
                .into_any_element(),
        )
    }

    /// The weapon line, as a dropdown.
    fn family_button(&self, cx: &Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let loaded = Rc::clone(&self.loaded);
        let current = self.choices.family;
        let lines: Vec<(Option<usize>, Option<TypeId>, String)> = self
            .families
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let bonus = match f.bonus {
                    3 => ", mastery",
                    2 => ", bonused",
                    _ => "",
                };
                (
                    Some(i),
                    Some(f.base),
                    format!("{}x {}{bonus}", f.count, f.name),
                )
            })
            .chain(std::iter::once((None, None, "No weapons".to_owned())))
            .collect();
        let label = self
            .setup()
            .family(&self.choices)
            .map_or_else(|| "No weapons".to_owned(), |f| f.name.clone());
        let families = self.families.len();
        Button::new("wizard-family")
            .outline()
            .xsmall()
            .tab_stop(false)
            .dropdown_caret(true)
            .flex_1()
            .min_w(px(VARIANT_MIN_W))
            .max_w(px(VARIANT_MAX_W))
            .label(label)
            .tooltip("The kind of weapon. Left and Right step through them.")
            .dropdown_menu(move |menu, _, _| {
                lines.iter().fold(menu, |menu, (i, icon, text)| {
                    let i = *i;
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::element(widgets::item_label(&loaded, *icon, text.clone()))
                            .checked(i == current)
                            .on_click(move |_, window, cx| {
                                let _ = this.update(cx, |p, cx| {
                                    p.focus_row(Row::Weapon, cx);
                                    p.set_segment(Row::Weapon, i.unwrap_or(families), cx);
                                    p.refocus(window, cx);
                                });
                            }),
                    )
                })
            })
            .into_any_element()
    }

    /// A row's title and description.
    fn text(&self, row: Row) -> (SharedString, SharedString) {
        let c = &self.choices;
        let catalog = &self.loaded.catalog;
        let (title, description): (String, String) = match row {
            Row::Tier => (
                "Bling".into(),
                "The tier every module starts at, on every step. Each module row can pick its \
                 own variation."
                    .into(),
            ),
            Row::Racks(tab) => ("Slots".into(), {
                let racks = match (tab, c.layer) {
                    (Tab::Loadout, _) => "high, mid and low racks",
                    (Tab::Tank, Layer::Shield) => "mid rack",
                    (Tab::Tank, Layer::Armor) => "low rack",
                    (Tab::Rigs, _) => "rigs",
                };
                if c.clear.has(tab) {
                    format!("Empty the {racks}, then fit them anew.")
                } else {
                    format!("Keep what is fitted in the {racks} and fill the free slots.")
                }
            }),
            Row::Weapon => (
                "Weapons".into(),
                match self.setup().family(c) {
                    Some(f) => format!(
                        "{}x {}{}",
                        f.count,
                        f.name,
                        if f.bonus >= 2 {
                            ", which the hull is built for."
                        } else {
                            "."
                        }
                    ),
                    None => "The high slots stay free for utilities.".into(),
                },
            ),
            Row::Ammo => (
                "Ammo".into(),
                self.picks
                    .iter()
                    .find_map(|p| p.charge.filter(|_| p.slot == Slot::High))
                    .map_or_else(|| "Unloaded.".into(), |a| catalog.name(a)),
            ),
            Row::Prop => ("Propulsion".into(), self.prop_detail()),
            Row::Utility(u) => {
                let (title, text) = utility_text(u);
                (title.into(), self.utility_reason(u).unwrap_or(text).into())
            }
            Row::Style => (
                "Style".into(),
                match c.tank {
                    TankStyle::Buffer => "Plates or extenders with resists.",
                    TankStyle::Active => "Repairers or boosters with resists.",
                    TankStyle::None => "Leave the tank slots to the rest.",
                }
                .into(),
            ),
            Row::Layer => (
                "Layer".into(),
                "Shield goes in the mid slots, armor in the lows.".into(),
            ),
            Row::Goal => (
                "Goal".into(),
                match (c.tank, c.goal) {
                    (TankStyle::Active, Goal::Resists) => {
                        "The best resists against the damage pattern, with a repairer."
                    }
                    (TankStyle::Active, Goal::Repair) => {
                        "The most HP repaired per second the capacitor keeps up, times resists."
                    }
                    (TankStyle::Active, _) => "Repair and buffer together.",
                    (_, Goal::Ehp) => "The most raw effective HP, however it resists.",
                    (_, Goal::Resists) => {
                        "The highest resist multiplier on the tanked layer, so logistics repairs \
                         land hardest; EHP decides between equals."
                    }
                    _ => "Effective HP, favouring resists over plates when they are close.",
                }
                .into(),
            ),
            Row::Tiers => (
                "Kinds of module".into(),
                "Which kinds of each module the search tries, the strongest few of each kind \
                 switched on. Officer and deadspace modules get expensive; the budgets keep \
                 them in check."
                    .into(),
            ),
            Row::Repairers => (
                "Repairers".into(),
                "How many repairers or boosters the tank ends up with, the fitted ones counted. \
                 Auto lets the search decide."
                    .into(),
            ),
            Row::BudgetEach => (
                "Per module".into(),
                "The most any one tank module or rig may cost. With a budget set, items the \
                 market has no price for are left out."
                    .into(),
            ),
            Row::BudgetTotal => (
                "Whole tank".into(),
                "The most the whole tank, modules and rigs, may cost.".into(),
            ),
            Row::Search => ("Search".into(), self.search_detail()),
            Row::Results => (
                "Results".into(),
                if self.stale() {
                    "The choices changed since; search again for fresh numbers.".into()
                } else {
                    "Pick one to fit it.".into()
                },
            ),
            Row::FineTune => (
                "Fine tune".into(),
                "Open the tank's module rows to pick each one's variation.".into(),
            ),
            Row::Module(base) => (
                category_text(tank::category_of(catalog, base)).into(),
                format!("{}x {}", self.count(base), base_name(&catalog.name(base))),
            ),
            Row::Damage => (
                "Damage modules".into(),
                match self.damage_base() {
                    Some(base) => format!("{}x in the low slots.", self.count(base)),
                    None => "No damage modules.".into(),
                },
            ),
            Row::Rigs => (
                "Rigs".into(),
                match c.rigs {
                    Rigs::Damage => "Rigs that add to the weapons' damage.",
                    Rigs::Tank => "Rigs for the tank; the tank step's search picks them.",
                    Rigs::None => "The rig slots stay free.",
                }
                .into(),
            ),
            Row::Rig(base) => {
                let id = self.variant_of(row).map_or(base, |(_, _, current)| current);
                let cost = self
                    .data
                    .type_info(id)
                    .and_then(|i| i.attr(attr::UPGRADE_COST))
                    .unwrap_or(0.0);
                (
                    "Rig".into(),
                    format!(
                        "{}x {}, {cost:.0} calibration each",
                        self.count(base),
                        base_name(&catalog.name(base))
                    ),
                )
            }
        };
        (title.into(), description.into())
    }

    /// Why a utility cannot go on this hull, if it cannot.
    fn utility_reason(&self, u: Utility) -> Option<&'static str> {
        if !u.fits(self.facts.size) {
            Some("Battleships and bigger only.")
        } else if self.facts.total(u.slot()) == 0 {
            Some("No slot for it on this hull.")
        } else if self.utility_base(u).is_none() {
            Some("None goes with these weapons.")
        } else {
            None
        }
    }

    fn prop_detail(&self) -> String {
        let Some((_, _, id)) = self.variant_of(Row::Prop) else {
            return "No propulsion module.".into();
        };
        let get = |a| {
            self.data
                .type_info(id)
                .and_then(|i| i.attr(a))
                .unwrap_or(0.0)
        };
        let mut parts = vec![format!("+{:.0}% speed", get(attr::SPEED_FACTOR))];
        let bloom = get(attr::SIG_BLOOM);
        if bloom.abs() > 0.5 {
            parts.push(format!("+{bloom:.0}% signature"));
        }
        parts.push(format!("{:.0} GJ a cycle", get(attr::CAP_NEED)));
        parts.join(", ") + "."
    }

    fn search_detail(&self) -> String {
        let pattern = match &self.fit.damage_profile {
            DamageProfile::Uniform => "uniform damage".to_owned(),
            DamageProfile::SelectedAmmo => "the selected ammo's damage".to_owned(),
            DamageProfile::Pattern { name, .. } => format!("the {name} pattern"),
        };
        let rack = slot_word(self.choices.layer.slot());
        let rigs = if Setup::tank_takes_rigs(&self.choices, self.kind()) {
            " and rigs"
        } else {
            ""
        };
        format!(
            "Tries tanks for the free {rack} slots{rigs} against {pattern}, with the weapons \
             idle, and lists the best {RESULTS}."
        )
    }

    /// The controls right of a row's label, which wrap when the room gets narrow. A variant
    /// dropdown comes last and takes the room left.
    fn controls(&self, row: Row, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        match row {
            Row::Weapon => out.push(self.family_button(cx)),
            Row::Utility(u) => {
                out.push(
                    Switch::new(SharedString::from(format!("wizard-{}", row.key())))
                        .checked(self.choices.has(u))
                        .disabled(!self.utility_usable(u))
                        .on_click(cx.listener(move |this, _: &bool, window, cx| {
                            this.focus_row(row, cx);
                            this.change(row.tab(), cx, |c| c.toggle(u));
                            this.refocus(window, cx);
                        }))
                        .into_any_element(),
                );
                // Off, the row says what the module is for; on, its variation takes the room.
                if !self.choices.has(u) {
                    out.push(muted(self.text(row).1, cx).into_any_element());
                }
            }
            Row::Module(base) | Row::Rig(base) => {
                out.push(count_cell(self.count(base), cx));
            }
            Row::Search => {
                out.push(self.search_controls(cx));
                // With no results the results section says it; otherwise it goes here.
                if let Some(note) = &self.note
                    && (self.fetching.is_some() || !self.results.is_empty())
                {
                    out.push(muted(note.clone(), cx).into_any_element());
                }
            }
            Row::BudgetEach | Row::BudgetTotal => {
                out.push(self.segmented(row, cx));
                let field = if row == Row::BudgetEach {
                    &self.budget_each
                } else {
                    &self.budget_total
                };
                out.push(
                    div()
                        .w(px(128.))
                        .child(Input::new(field).small())
                        .into_any_element(),
                );
            }
            _ => out.push(self.segmented(row, cx)),
        }
        out.extend(self.variant_button(row, cx));
        if let Row::Module(base) | Row::Rig(base) = row
            && let Some(price) = self
                .variant_of(row)
                .and_then(|(_, _, id)| self.prices.get(id))
        {
            let n = self.count(base) as f64;
            out.push(muted(format!("{} ISK", widgets::short(price * n)), cx).into_any_element());
        }
        out
    }

    fn search_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(search) = &self.search {
            let fraction = search.progress.fraction();
            return h_flex()
                .gap_2()
                .child(div().w(px(240.)).child(widgets::gauge(
                    f64::from(fraction),
                    self.palette.accent,
                    format!("Searching, {:.0}%", fraction * 100.0),
                    0.,
                    cx,
                )))
                .child(Self::cancel_button(cx))
                .into_any_element();
        }
        let again = !self.results.is_empty();
        h_flex()
            .gap_2()
            .child(
                Button::new("wizard-find")
                    .small()
                    .primary()
                    .tab_stop(false)
                    .disabled(self.choices.tank == TankStyle::None)
                    .loading(self.fetching.is_some())
                    .icon(AssetIcon::Search)
                    .label(if self.fetching.is_some() {
                        "Fetching prices"
                    } else if again {
                        "Search again"
                    } else {
                        "Find best"
                    })
                    .tooltip("F")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus_row(Row::Search, cx);
                        this.find_best(cx);
                        this.refocus(window, cx);
                    })),
            )
            .when(self.fetching.is_some(), |d| {
                d.child(Self::cancel_button(cx))
            })
            .into_any_element()
    }

    fn cancel_button(cx: &mut Context<Self>) -> Button {
        Button::new("wizard-cancel")
            .small()
            .ghost()
            .tab_stop(false)
            .icon(AssetIcon::X)
            .label("Cancel")
            .tooltip("Escape")
            .on_click(cx.listener(|this, _, window, cx| {
                this.cancel_search(cx);
                this.refocus(window, cx);
            }))
    }

    /// The optimiser's tanks as a table: rank, numbers, and the modules as a strip of icons.
    /// The keyboard and clicks pick one to fit.
    fn results_table(&self, cursor: Option<Row>, cx: &mut Context<Self>) -> AnyElement {
        let repair = self.results.iter().any(|o| o.repair > 0.0);
        // Aiming for resists, the table shows the resist multiplier: EHP per raw HP.
        let multiplier = self.choices.goal == Goal::Resists;
        let rows: Vec<AnyElement> = self
            .results
            .iter()
            .enumerate()
            .map(|(i, outcome)| self.result_row(i, outcome, repair, multiplier, cx))
            .collect();
        let stale = self.stale().then(|| self.text(Row::Results).1);
        let limit = self.limit.map(limit_text);
        let short = self.repairers_short.map(|got| {
            SharedString::from(match got {
                0 => "No repairer fits with the rest of the fit; the results tank without one."
                    .to_owned(),
                1 => "Only 1 more repairer fits with the rest of the fit.".to_owned(),
                n => format!("Only {n} more repairers fit with the rest of the fit."),
            })
        });
        let layer = self.choices.layer.label();
        let theme = cx.theme();
        let header = h_flex()
            .h(px(24.))
            .gap_3()
            .px_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().w(px(20.)).flex_none().child("#"))
            .child(head_cell(
                "ehp",
                "EHP",
                "Effective hit points against the damage pattern",
                52.,
            ))
            .child(if multiplier {
                head_cell(
                    "resist",
                    "Multiplier",
                    format!("EHP per raw HP of the {layer} layer"),
                    60.,
                )
            } else {
                head_cell(
                    "resist",
                    "Resist",
                    format!("The {layer} layer's resist against the damage pattern"),
                    60.,
                )
            })
            .when(repair, |d| {
                d.child(head_cell(
                    "repair",
                    "HP/s",
                    "Hit points repaired a second, as long as the capacitor keeps up",
                    48.,
                ))
            })
            .child(head_cell(
                "cpu",
                "CPU",
                "CPU used of the ship's output, with this tank",
                RESOURCE_W,
            ))
            .child(head_cell(
                "pg",
                "PG",
                "Powergrid used of the ship's output, with this tank",
                RESOURCE_W,
            ))
            .child(head_cell(
                "price",
                "Price",
                "What the tank's modules and rigs cost; ~ when some have no price",
                56.,
            ))
            .child(
                div()
                    .id("wizard-head-cap")
                    .w(px(24.))
                    .flex_none()
                    .text_center()
                    .child("Cap")
                    .tooltip(|window, cx| {
                        Tooltip::new("Whether the capacitor stays stable").build(window, cx)
                    }),
            )
            .child(div().flex_1().min_w_0().child("Modules"));
        let active = cursor == Some(Row::Results);
        let palette = self.palette;
        v_flex()
            .id(SharedString::from(format!(
                "wizard-row-{}",
                Row::Results.key()
            )))
            .py_1()
            .border_l_2()
            .border_color(if active {
                theme.primary
            } else {
                theme.primary.opacity(0.)
            })
            .when(active, |d| d.bg(theme.secondary.opacity(0.5)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.focus_row(Row::Results, cx)),
            )
            .when_some(stale, |d, stale| {
                d.child(notice(AssetIcon::RotateCcw, palette.yellow, stale, cx).mb_1())
            })
            .child(header)
            .children(rows)
            .children(
                [short, limit]
                    .into_iter()
                    .flatten()
                    .map(|note| notice(AssetIcon::Info, palette.blue, note, cx).mt_1()),
            )
            .into_any_element()
    }

    fn result_row(
        &self,
        i: usize,
        outcome: &Outcome,
        repair: bool,
        multiplier: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let catalog = &self.loaded.catalog;
        let selected = self.selected == Some(i);
        let mut lines: Vec<(TypeId, usize)> = Vec::new();
        for &id in outcome.types.iter().chain(&outcome.rig_types) {
            match lines.iter_mut().find(|(t, _)| *t == id) {
                Some((_, n)) => *n += 1,
                None => lines.push((id, 1)),
            }
        }
        let strip = h_flex()
            .flex_1()
            .min_w_0()
            .flex_wrap()
            .gap_x_2()
            .gap_y_0p5()
            .children(lines.into_iter().map(|(id, n)| {
                let tip = SharedString::from(format!("{n}x {}", catalog.name(id)));
                h_flex()
                    .id(SharedString::from(format!("wizard-result-{i}-{id}")))
                    .gap_0p5()
                    .child(widgets::icon_box(
                        catalog,
                        &self.loaded.icons,
                        Icon::Type(id),
                        STRIP_ICON,
                    ))
                    .when(n > 1, |d| {
                        d.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("{n}")),
                        )
                    })
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            }));
        let (cap_icon, cap_color, cap_text) = if outcome.cap_stable {
            (AssetIcon::BatteryFull, self.palette.green, "Cap stable")
        } else {
            (AssetIcon::BatteryLow, self.palette.yellow, "Cap runs out")
        };
        let cap = h_flex()
            .id(SharedString::from(format!("wizard-result-{i}-cap")))
            .w(px(24.))
            .flex_none()
            .justify_center()
            .text_color(widgets::color(cap_color))
            .child(UiIcon::new(cap_icon).xsmall())
            .tooltip(move |window, cx| Tooltip::new(cap_text).build(window, cx));
        h_flex()
            .id(SharedString::from(format!("wizard-result-{i}")))
            .min_h(px(28.))
            .gap_3()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .text_xs()
            .cursor_pointer()
            .when(selected, |d| d.bg(theme.primary.opacity(0.15)))
            .when(!selected, |d| d.hover(|s| s.bg(theme.secondary)))
            .child(
                div()
                    .w(px(20.))
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(selected, |d| d.text_color(theme.primary))
                    .when(!selected, |d| d.text_color(theme.muted_foreground))
                    .child(format!("{}", i + 1)),
            )
            .child(cell(widgets::short(outcome.ehp), 52.).font_weight(FontWeight::MEDIUM))
            .child(cell(
                if multiplier {
                    format!("{:.2}x", 1.0 / (1.0 - outcome.resist).max(1e-9))
                } else {
                    format!("{:.1}%", outcome.resist * 100.0)
                },
                60.,
            ))
            .when(repair, |d| {
                d.child(cell(
                    if outcome.repair > 0.0 {
                        format!("{:.0}", outcome.repair)
                    } else {
                        String::new()
                    },
                    48.,
                ))
            })
            .child(self.resource_cell(&outcome.cpu))
            .child(self.resource_cell(&outcome.powergrid))
            .child(
                cell(
                    if outcome.price > 0.0 {
                        let sign = if outcome.priced { "" } else { "~" };
                        format!("{sign}{}", widgets::short(outcome.price))
                    } else {
                        "?".to_owned()
                    },
                    56.,
                )
                .when(!outcome.priced, |d| d.text_color(theme.muted_foreground)),
            )
            .child(cap)
            .child(strip)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.focus_row(Row::Results, cx);
                this.select(i, cx);
                this.refocus(window, cx);
            }))
            .into_any_element()
    }

    /// Used of output, red when within 1% of the limit.
    fn resource_cell(&self, r: &Resource) -> Div {
        let out = r.output.unwrap_or(0.0);
        cell(format!("{:.0}/{out:.0}", r.used), RESOURCE_W).when(r.used >= out * 0.99, |d| {
            d.text_color(widgets::color(self.palette.red))
        })
    }

    /// One row: a fixed label column and the controls filling the rest, its description under
    /// them or in a tooltip on the label. Marked while the keyboard is on it.
    fn row_element(&self, row: Row, cursor: Option<Row>, cx: &mut Context<Self>) -> AnyElement {
        if row == Row::Results {
            return self.results_table(cursor, cx);
        }
        let (title, description) = self.text(row);
        let disabled = matches!(row, Row::Utility(u) if !self.utility_usable(u));
        let inline = row.describes_inline();
        let controls = self.controls(row, cx);
        let theme = cx.theme();
        let active = cursor == Some(row);
        let tip = (!inline && !matches!(row, Row::Utility(u) if !self.choices.has(u)))
            .then_some(description.clone());
        let compact = self.compact;
        let label = h_flex()
            .id(SharedString::from(format!("wizard-label-{}", row.key())))
            .when(compact, |d| d.w_full())
            .when(!compact, |d| d.w(px(LABEL_W)))
            .h(px(LINE_H))
            .flex_none()
            .gap_1()
            .when(disabled, |d| d.opacity(0.5))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .when(active, |d| d.text_color(theme.foreground))
                    .child(title),
            )
            .when_some(tip.filter(|t| !t.is_empty()), |d, tip| {
                d.child(
                    UiIcon::new(AssetIcon::Info)
                        .xsmall()
                        .flex_none()
                        .text_color(theme.muted_foreground.opacity(0.6)),
                )
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            });
        // A compact row puts its controls under its name.
        h_flex()
            .id(SharedString::from(format!("wizard-row-{}", row.key())))
            .when(compact, |d| d.flex_col().items_stretch().gap_0p5())
            .when(!compact, |d| d.items_start().gap_4())
            .px_3()
            .py_1()
            .rounded_r(px(4.))
            .text_sm()
            .border_l_2()
            .border_color(if active {
                theme.primary
            } else {
                theme.primary.opacity(0.)
            })
            .when(active, |d| d.bg(theme.secondary.opacity(0.5)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.focus_row(row, cx)),
            )
            .child(label)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .min_h(px(LINE_H))
                            .flex_wrap()
                            .gap_2()
                            .children(controls),
                    )
                    .when(inline && !description.is_empty(), |d| {
                        d.child(muted(description, cx).pb_0p5())
                    }),
            )
            .into_any_element()
    }

    /// The title of a section: its number, an uppercase caption and a hairline. A section
    /// that opens and closes has its title as its row.
    fn section_title(&self, section: &Section, cursor: Option<Row>, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let badge = section.number.map(|n| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(18.))
                .rounded_full()
                .bg(theme.primary.opacity(0.15))
                .text_color(theme.primary)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .child(n.to_string())
        });
        let caption = h_flex().gap_2().children(badge).child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(section.title.to_uppercase()),
        );
        let Some(toggle) = section.toggle else {
            return div()
                .px_3()
                .pb_1p5()
                .mb_1()
                .border_b_1()
                .border_color(theme.border)
                .child(caption);
        };
        let active = cursor == Some(toggle);
        let open = self.fine_tune;
        div().mb_1().border_b_1().border_color(theme.border).child(
            h_flex()
                .id(SharedString::from(format!("wizard-row-{}", toggle.key())))
                .gap_2()
                .px_3()
                .py_1p5()
                .border_l_2()
                .border_color(if active {
                    theme.primary
                } else {
                    theme.primary.opacity(0.)
                })
                .when(active, |d| d.bg(theme.secondary.opacity(0.5)))
                .cursor_pointer()
                .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                .child(
                    UiIcon::new(if open {
                        AssetIcon::ChevronDown
                    } else {
                        AssetIcon::ChevronRight
                    })
                    .xsmall()
                    .text_color(theme.muted_foreground),
                )
                .child(caption)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if open { "Hide" } else { "Show" }),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.focus_row(toggle, cx);
                    this.fine_tune = !this.fine_tune;
                    cx.notify();
                    this.refocus(window, cx);
                })),
        )
    }

    /// The current step: its intro, then its sections top to bottom, scrolling on its own when
    /// the window is too short.
    fn step_body(&self, cursor: Option<Row>, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.tab;
        let intro = self.intro(tab);
        let untouched = !self.choices.touched.has(tab);
        let mut body = v_flex()
            .id("wizard-step")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .gap_6()
            .pr_2()
            .pb_6();
        let theme = cx.theme();
        let icon = match tab {
            Tab::Loadout => AssetIcon::Crosshair,
            Tab::Tank => AssetIcon::Shield,
            Tab::Rigs => AssetIcon::Wrench,
        };
        let number = Tab::ALL.iter().position(|&t| t == tab).unwrap_or(0) + 1;
        body = body.child(
            h_flex()
                .items_start()
                .gap_3()
                .px_3()
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(36.))
                        .rounded(px(8.))
                        .bg(theme.primary.opacity(0.15))
                        .child(UiIcon::new(icon).size(px(18.)).text_color(theme.primary)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("Step {number}: {}", tab.label())),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(intro),
                        )
                        .when(untouched, |d| {
                            d.child(
                                h_flex()
                                    .gap_1p5()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(UiIcon::new(AssetIcon::Info).xsmall().flex_none())
                                    .child(div().flex_1().min_w_0().child(if tab == Tab::Tank {
                                        "Nothing from this step goes on until you change \
                                         something here or find a tank."
                                    } else {
                                        "Nothing from this step goes on until you change \
                                         something here."
                                    })),
                            )
                        }),
                ),
        );
        for section in self.sections(tab) {
            let title = self.section_title(&section, cursor, cx);
            let rows: Vec<AnyElement> = section
                .rows
                .iter()
                .filter(|&&r| Some(r) != section.toggle)
                .map(|&r| self.row_element(r, cursor, cx))
                .collect();
            let note = section.note.clone().map(|note| {
                let empty = section.rows.is_empty();
                h_flex()
                    .gap_1p5()
                    .px_3()
                    .when(empty, Styled::py_2)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .when(empty, |d| d.child(UiIcon::new(AssetIcon::Info).xsmall()))
                    .child(note)
            });
            // A note heads a section that opens and closes, and follows the rows of the rest.
            let (above, below) = if section.toggle.is_some() {
                (note, None)
            } else {
                (None, note)
            };
            body = body.child(
                v_flex()
                    .gap_0p5()
                    .child(title)
                    .children(above)
                    .children(rows)
                    .children(below),
            );
        }
        body.into_any_element()
    }

    /// The steps as a sequence across the header: number or check, name and what it adds.
    fn steps(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut out = h_flex().flex_none().gap_1();
        for (i, tab) in Tab::ALL.into_iter().enumerate() {
            let current = tab == self.tab;
            let state = self.step_state(tab);
            let adds = matches!(state, StepState::Adds(_));
            let theme = cx.theme();
            if i > 0 {
                out = out.child(div().w(px(20.)).h_px().flex_none().bg(theme.border));
            }
            let badge = div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(22.))
                .rounded_full()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .when(current, |d| {
                    d.bg(theme.primary).text_color(theme.primary_foreground)
                })
                .when(!current && adds, |d| {
                    d.bg(theme.primary.opacity(0.15)).text_color(theme.primary)
                })
                .when(!current && !adds, |d| {
                    d.border_1()
                        .border_color(theme.border)
                        .text_color(theme.muted_foreground)
                })
                .map(|d| {
                    if adds && !current {
                        d.child(UiIcon::new(IconName::Check).xsmall())
                    } else {
                        d.child((i + 1).to_string())
                    }
                });
            let text = state.text();
            let tip = SharedString::from(format!("Ctrl+{}: {}", i + 1, text));
            out = out.child(
                h_flex()
                    .id(SharedString::from(format!("wizard-step-{tab:?}")))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(current, |d| d.bg(theme.secondary))
                    .when(!current, |d| {
                        d.hover(|s| s.bg(theme.secondary.opacity(0.5)))
                    })
                    .child(badge)
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .text_sm()
                                    .when(current, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .when(!current, |d| d.text_color(theme.muted_foreground))
                                    .child(tab.label()),
                            )
                            .when(!self.compact, |d| {
                                d.child(
                                    div()
                                        .max_w(px(200.))
                                        .truncate()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .when(adds, |d| d.text_color(theme.foreground.opacity(0.8)))
                                        .child(text),
                                )
                            }),
                    )
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &gpui_kit::MouseDownEvent, window, cx| {
                            this.set_tab(tab, cx);
                            this.refocus(window, cx);
                        }),
                    ),
            );
        }
        out.into_any_element()
    }

    // The preview.

    /// Each rack: what is fitted and stays, muted, then what the wizard adds, marked.
    fn racks(&self, cx: &App) -> Div {
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let cleared = self.choices.cleared();
        let mut racks = v_flex().gap_1();
        for slot in [Slot::High, Slot::Mid, Slot::Low, Slot::Rig] {
            let total = self.facts.total(slot);
            if total == 0 {
                continue;
            }
            let picks: Vec<&Pick> = self.picks.iter().filter(|p| p.slot == slot).collect();
            let kept = if cleared.has(slot) {
                Vec::new()
            } else {
                self.fitted(&[slot])
            };
            let kept_n: usize = kept.iter().map(|(_, n)| n).sum();
            // Same module and charge on one line, in the order they go on.
            let mut lines: Vec<((TypeId, Option<TypeId>), usize)> = Vec::new();
            for p in &picks {
                match lines.iter_mut().find(|(k, _)| *k == (p.type_id, p.charge)) {
                    Some((_, n)) => *n += 1,
                    None => lines.push(((p.type_id, p.charge), 1)),
                }
            }
            let used = (kept_n + picks.len()).min(total);
            let status = if cleared.has(slot) {
                format!("{used}/{total}, emptied first")
            } else {
                format!("{used}/{total}")
            };
            racks = racks.child(
                h_flex()
                    .justify_between()
                    .mt_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(rack_title(slot))
                    .child(status),
            );
            let line = |id: TypeId, n: usize, charge: Option<TypeId>, added: bool| {
                h_flex()
                    .gap_1p5()
                    .min_w_0()
                    .text_xs()
                    .child(
                        div()
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .when(added, |d| d.bg(theme.primary)),
                    )
                    .child(
                        widgets::icon_box(catalog, &self.loaded.icons, Icon::Type(id), ICON)
                            .when(!added, |d| d.opacity(0.6)),
                    )
                    .child(
                        div()
                            .truncate()
                            .when(!added, |d| d.text_color(theme.muted_foreground))
                            .child(format!("{n}x {}", catalog.name(id))),
                    )
                    .when_some(charge, |row, c| {
                        row.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(catalog.name(c)),
                        )
                    })
            };
            for (id, n) in kept {
                racks = racks.child(line(id, n, None, false));
            }
            for ((id, charge), n) in lines {
                racks = racks.child(line(id, n, charge, true));
            }
        }
        racks
    }

    fn preview(&self, cx: &App) -> Div {
        let s = &self.stats;
        let b = &self.before;
        let theme = cx.theme();
        let ehp = |s: &Stats| -> f64 {
            [s.shield, s.armor, s.hull]
                .iter()
                .map(|l| l.ehp.unwrap_or(l.hp))
                .sum()
        };
        let layer = self.choices.layer;
        let of = |s: &Stats| match layer {
            Layer::Shield => s.shield,
            Layer::Armor => s.armor,
        };
        let repair = |s: &Stats| match layer {
            Layer::Shield => s.sustained.shield,
            Layer::Armor => s.sustained.armor,
        };
        let mult = |l: LayerStats| l.mult.unwrap_or(1.0).max(1.0);
        let name = layer.label();
        let multiplier = self.choices.tank != TankStyle::None && self.choices.goal == Goal::Resists;
        let cost: f64 = self
            .picks
            .iter()
            .filter_map(|p| self.prices.get(p.type_id))
            .sum();
        let unpriced = self
            .picks
            .iter()
            .any(|p| self.prices.get(p.type_id).is_none());
        let resist = if multiplier {
            self.compare(
                &format!("{name} multiplier"),
                format!("EHP per raw HP of the {} layer", name.to_lowercase()),
                (mult(of(b)), mult(of(s))),
                |v| format!("{v:.2}x"),
                cx,
            )
        } else {
            self.compare(
                &format!("{name} resist"),
                format!(
                    "The {} layer's resist against the damage pattern",
                    name.to_lowercase()
                ),
                (1.0 - 1.0 / mult(of(b)), 1.0 - 1.0 / mult(of(s))),
                |v| format!("{:.1}%", v * 100.0),
                cx,
            )
        };
        let numbers = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1())
                    .child(div().w(px(VALUE_W)).text_right().child("Now"))
                    .child(div().w(px(16.)))
                    .child(div().w(px(VALUE_W)).text_right().child("After")),
            )
            .child(self.compare(
                "Effective HP",
                "Effective hit points of all three layers against the damage pattern".into(),
                (ehp(b), ehp(s)),
                widgets::short,
                cx,
            ))
            .child(resist)
            .child(self.compare(
                &format!("{name} repair"),
                "Hit points repaired a second that the capacitor keeps up".into(),
                (repair(b), repair(s)),
                |v| format!("{v:.0} HP/s"),
                cx,
            ))
            .child(self.compare(
                "DPS",
                "Damage a second of the weapons and drones".into(),
                (b.dps.total(), s.dps.total()),
                |v| format!("{v:.0}"),
                cx,
            ));
        let fits = wizard::fits(s);
        let gauges = v_flex()
            .gap_2()
            .child(self.resource("CPU", &b.cpu, &s.cpu, " tf", cx))
            .child(self.resource("Powergrid", &b.powergrid, &s.powergrid, " MW", cx))
            .child(self.resource("Calibration", &b.calibration, &s.calibration, "", cx))
            .when(!fits, |d| {
                d.child(notice(
                    AssetIcon::TriangleAlert,
                    self.palette.red,
                    "Over what the ship has. Make it fit (M) swaps in variations that fit.",
                    cx,
                ))
            })
            .when(fits && self.made_to_fit, |d| {
                d.child(notice(
                    AssetIcon::CircleCheck,
                    self.palette.green,
                    "Made to fit: some modules use a cheaper variation.",
                    cx,
                ))
            });
        let count = self.picks.len();
        let cost_line = h_flex()
            .justify_between()
            .text_xs()
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} added", plural(count, "module"))),
            )
            .child(if cost > 0.0 {
                format!(
                    "{}{} ISK",
                    if unpriced { "~" } else { "" },
                    widgets::short(cost)
                )
            } else if count > 0 {
                "No prices yet".to_owned()
            } else {
                "Nothing yet".to_owned()
            });
        let legend = h_flex()
            .gap_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                h_flex()
                    .gap_1p5()
                    .child(div().size(px(6.)).rounded_full().bg(theme.primary))
                    .child("Added"),
            )
            .child("Grey: already fitted");
        v_flex()
            .gap_2()
            .child(caption("Before and after", cx))
            .child(numbers)
            .child(cost_line)
            .child(caption("Fitting", cx).mt_3())
            .child(gauges)
            .child(caption("Modules", cx).mt_3())
            .child(legend)
            .child(self.racks(cx))
            .child(caption("Tank after", cx).mt_3())
            .child(self.tank_summary(cx))
    }

    /// A number before and after, the after green when it got better and red when worse.
    fn compare(
        &self,
        label: &str,
        tip: String,
        (before, after): (f64, f64),
        format: fn(f64) -> String,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let changed = (after - before).abs() > 1e-6 * before.abs().max(1.0);
        let tip = SharedString::from(tip);
        h_flex()
            .id(SharedString::from(format!("wizard-compare-{label}")))
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.muted_foreground)
                    .child(label.to_owned()),
            )
            .child(
                div()
                    .w(px(VALUE_W))
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .child(format(before)),
            )
            .child(
                div().w(px(16.)).flex().justify_center().child(
                    UiIcon::new(AssetIcon::ArrowRight)
                        .xsmall()
                        .text_color(theme.muted_foreground.opacity(0.6)),
                ),
            )
            .child(
                div()
                    .w(px(VALUE_W))
                    .text_right()
                    .font_weight(FontWeight::MEDIUM)
                    .when(changed && after > before, |d| {
                        d.text_color(widgets::color(self.palette.green))
                    })
                    .when(changed && after < before, |d| {
                        d.text_color(widgets::color(self.palette.red))
                    })
                    .when(!changed, |d| d.text_color(theme.muted_foreground))
                    .child(format(after)),
            )
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .into_any_element()
    }

    fn tank_summary(&self, cx: &App) -> Div {
        let s = &self.stats;
        let catalog = &self.loaded.catalog;
        let icons = &self.loaded.icons;
        let layer_row =
            |icon_id: i32, layer: LayerStats| {
                h_flex()
                    .gap_1()
                    .child(widgets::icon_box(catalog, icons, Icon::Id(icon_id), ICON))
                    .children(layer.resists.into_iter().zip(widgets::DAMAGE_COLORS).map(
                        |(r, fill)| widgets::gauge(r, fill, format!("{:.0}%", r * 100.0), 0., cx),
                    ))
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .text_xs()
                            .text_right()
                            .child(widgets::short(layer.ehp.unwrap_or(layer.hp))),
                    )
            };
        let cap = match s.cap {
            Some(Cap::Stable(level)) => format!("Stable at {:.0}%", level * 100.0),
            Some(Cap::Lasts(secs)) => {
                let secs = secs.max(0.0) as u64;
                format!("Lasts {}m {:02}s", secs / 60, secs % 60)
            }
            None => "None".into(),
        };
        v_flex()
            .gap_1()
            .child(layer_row(widgets::icon::SHIELD, s.shield))
            .child(layer_row(widgets::icon::ARMOR, s.armor))
            .child(layer_row(widgets::icon::HULL, s.hull))
            .when(s.tank.passive_shield > 0.0, |d| {
                d.child(line(
                    "Passive shield regen",
                    format!("{:.0} HP/s", s.tank.passive_shield),
                    cx,
                ))
            })
            .child(line("Capacitor, all active", cap, cx))
            .child(line("Volley", format!("{:.0}", s.volley.total()), cx))
            .when(s.drone_dps > 0.0, |d| {
                d.child(line("DPS from drones", format!("{:.0}", s.drone_dps), cx))
            })
    }

    /// A fitting resource: used before and after of the output, with a gauge of the after.
    fn resource(&self, label: &str, before: &Resource, r: &Resource, unit: &str, cx: &App) -> Div {
        let out = r.output.unwrap_or(0.0);
        let fraction = if out > 0.0 { r.used / out } else { 0.0 };
        let over = r.used > out + 1e-6;
        let fill = if over {
            self.palette.red
        } else {
            self.palette.green
        };
        let theme = cx.theme();
        let changed = (r.used - before.used).abs() > 1e-6;
        v_flex()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child(label.to_owned()))
                    .when(changed, |d| {
                        d.child(format!("{:.1}", before.used)).child(
                            UiIcon::new(AssetIcon::ArrowRight)
                                .xsmall()
                                .text_color(theme.muted_foreground.opacity(0.6)),
                        )
                    })
                    .child(
                        div()
                            .when(over, |d| d.text_color(widgets::color(self.palette.red)))
                            .when(!over && changed, |d| d.text_color(theme.foreground))
                            .child(format!("{:.1} / {out:.1}{unit}", r.used)),
                    ),
            )
            .child(
                widgets::gauge(fraction, fill, format!("{:.1}%", fraction * 100.0), 0., cx)
                    .w_full(),
            )
    }

    /// The footer: key hints, then moving between steps, then making it fit and applying.
    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let fits = wizard::fits(&self.stats);
        let count = self.picks.len();
        let back = self.neighbour(false);
        let next = self.neighbour(true);
        let tank = self.tab == Tab::Tank && self.choices.tank != TankStyle::None;
        let mut keys: Vec<(&str, &str)> = vec![
            ("Up Down", "row"),
            ("Left Right", "choose"),
            ("[ ]", "variation"),
            ("Space", "switch"),
        ];
        if tank {
            keys.push(("F", "find"));
        }
        if !fits {
            keys.push(("M", "make it fit"));
        }
        keys.extend([("Ctrl+Tab", "step"), ("Enter", "apply")]);
        let theme = cx.theme();
        let hints = h_flex()
            .id("wizard-keys")
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .gap_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .children(keys.into_iter().map(|(key, what)| {
                h_flex()
                    .flex_none()
                    .gap_1()
                    .child(
                        div()
                            .px_1()
                            .rounded(px(3.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary.opacity(0.5))
                            .child(key),
                    )
                    .child(what)
            }))
            .tooltip(|window, cx| {
                Tooltip::new(
                    "Up and Down pick a row. Left, Right or 1 to 9 choose on it. [ and ] step \
                     through a module's variations. Space flips a switch or runs the search. F \
                     finds the best tank. M makes it fit. Ctrl+Tab or Ctrl+1 to 3 change step. \
                     Escape cancels a search. Enter applies.",
                )
                .build(window, cx)
            });
        let show_preview = self.show_preview;
        let preview_toggle = self.fold_preview.then(|| {
            Button::new("wizard-preview-toggle")
                .small()
                .ghost()
                .tab_stop(false)
                .icon(if show_preview {
                    AssetIcon::ListChecks
                } else {
                    AssetIcon::ChartColumn
                })
                .label(if show_preview { "Choices" } else { "Preview" })
                .tooltip(if show_preview {
                    "Back to the step's choices"
                } else {
                    "What the fit looks like with these modules"
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_preview = !this.show_preview;
                    cx.notify();
                    this.refocus(window, cx);
                }))
        });
        // A compact footer drops the key hints and wraps its buttons rather than pushing them
        // out of the window.
        h_flex()
            .flex_none()
            .flex_wrap()
            .justify_end()
            .gap_2()
            .when(self.compact, |d| d.px_3())
            .when(!self.compact, |d| d.px_5())
            .py_2p5()
            .border_t_1()
            .border_color(theme.border)
            .when(!self.compact, |d| d.child(hints))
            .children(preview_toggle)
            .child(
                Button::new("wizard-back")
                    .small()
                    .ghost()
                    .tab_stop(false)
                    .icon(AssetIcon::ArrowLeft)
                    .label("Back")
                    .disabled(back.is_none())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(tab) = back {
                            this.set_tab(tab, cx);
                        }
                        this.refocus(window, cx);
                    })),
            )
            .child(
                Button::new("wizard-next")
                    .small()
                    .outline()
                    .tab_stop(false)
                    .label(next.map_or_else(
                        || SharedString::from("Next"),
                        |t| format!("Next: {}", t.label()).into(),
                    ))
                    .disabled(next.is_none())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(tab) = next {
                            this.set_tab(tab, cx);
                        }
                        this.refocus(window, cx);
                    })),
            )
            .when(!fits, |f| {
                f.child(
                    Button::new("wizard-make-fit")
                        .small()
                        .warning()
                        .tab_stop(false)
                        .icon(AssetIcon::Minimize2)
                        .label("Make it fit")
                        .tooltip(
                            "M: swap in variations that fit the CPU, powergrid and calibration",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.make_it_fit(cx);
                            this.refocus(window, cx);
                        })),
                )
            })
            .child(
                Button::new("wizard-apply")
                    .small()
                    .primary()
                    .tab_stop(false)
                    .icon(IconName::Check)
                    .label(if count == 0 {
                        "Apply".to_owned()
                    } else {
                        format!("Apply {}", plural(count, "module"))
                    })
                    .tooltip(if count == 0 {
                        "Nothing to add yet: change something on a step first"
                    } else {
                        "Enter: add these to the fit, as one undo step"
                    })
                    .disabled(count == 0)
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
            )
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let steps = self.steps(cx);
        let theme = cx.theme();
        let catalog = &self.loaded.catalog;
        // The steps drop under the ship when the two don't fit on a line.
        h_flex()
            .flex_none()
            .flex_wrap()
            .gap_x_6()
            .gap_y_1()
            .when(self.compact, |d| d.px_3())
            .when(!self.compact, |d| d.px_5())
            .py_2p5()
            // Room for the dialog's close button.
            .pr_12()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(SHIP_MIN_W))
                    .gap_3()
                    .child(widgets::icon_box(
                        catalog,
                        &self.loaded.icons,
                        Icon::Type(self.facts.ship),
                        32.,
                    ))
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("Fitting wizard"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.ship.clone()),
                            ),
                    ),
            )
            .child(steps)
    }
}

/// A utility's row title and what it is for.
fn utility_text(utility: Utility) -> (&'static str, &'static str) {
    match utility {
        Utility::Scram => ("Warp scrambler", "Short range tackle that also stops MWDs."),
        Utility::Point => ("Warp disruptor", "Long range tackle."),
        Utility::Web => ("Stasis webifier", "Slows the target down."),
        Utility::Grappler => ("Stasis grappler", "A strong web at very short range."),
        Utility::Neut => ("Energy neutralizer", "Drains the target's capacitor."),
        Utility::Nos => ("Energy nosferatu", "Steals capacitor from the target."),
        Utility::SensorBooster => ("Sensor booster", "Faster locks and longer range."),
        Utility::Computer => ("Weapon computer", "Tracking or missile guidance."),
        Utility::CapBooster => ("Capacitor booster", "Capacitor on demand, from charges."),
        Utility::Enhancer => ("Weapon enhancer", "Tracking or guidance, in a low slot."),
    }
}

fn category_text(category: Category) -> &'static str {
    match category {
        Category::Repairer => "Repairer",
        Category::Buffer => "Buffer",
        Category::Hardener => "Resists",
        Category::Electronics => "Capacitor",
        _ => "Module",
    }
}

/// A T1 name without its " I", for text that names a module of any tier.
fn base_name(name: &str) -> String {
    name.strip_suffix(" I").unwrap_or(name).to_owned()
}

/// A slot as a word in a sentence: "high", "rig".
fn slot_word(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "high",
        Slot::Mid => "mid",
        Slot::Low => "low",
        _ => "rig",
    }
}

/// A rack as the intros name it.
fn rack_name(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "high slots",
        Slot::Mid => "mid slots",
        Slot::Low => "low slots",
        _ => "rig slots",
    }
}

/// A rack as the preview titles it.
fn rack_title(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "High",
        Slot::Mid => "Mid",
        Slot::Low => "Low",
        _ => "Rigs",
    }
}

/// "1 slot", "3 slots".
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// "a buffer", "an active".
fn article(word: &str) -> String {
    let an = word
        .chars()
        .next()
        .is_some_and(|c| "aeiouAEIOU".contains(c));
    format!("{} {word}", if an { "an" } else { "a" })
}

/// A label on the left and its value on the right, in small type.
fn line(label: &str, value: String, cx: &App) -> Div {
    h_flex()
        .justify_between()
        .gap_2()
        .text_xs()
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_owned()),
        )
        .child(value)
}

/// A small uppercase caption over a group, with a hairline under it.
fn caption(title: &str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .pb_1()
        .mb_1()
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(title.to_uppercase())
}

/// Small muted text that wraps.
fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A one line notice with an icon in a colour of the palette.
fn notice(
    icon: AssetIcon,
    color: crate::theme::Rgb,
    text: impl Into<SharedString>,
    cx: &App,
) -> Div {
    h_flex()
        .items_start()
        .gap_1p5()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(
            div()
                .flex_none()
                .pt(px(1.))
                .text_color(widgets::color(color))
                .child(UiIcon::new(icon).xsmall()),
        )
        .child(div().flex_1().min_w_0().child(text.into()))
}

/// "a, b and c".
fn join_and(parts: &[String], word: &str) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} {word} {last}", rest.join(", ")),
    }
}

/// Why the search's better tanks did not make it.
fn limit_text(limit: Limit) -> SharedString {
    let rest = match limit {
        Limit::Cpu | Limit::Powergrid => {
            "better modules did not fit. Try Make it fit, lower tier weapons or fewer utilities."
        }
        Limit::Calibration => "better rigs did not fit.",
        Limit::Budget => {
            return "The budget is the limit: better tanks cost more. Raise it to see them.".into();
        }
    };
    format!("{} is the limit: {rest}", limit.label()).into()
}

/// A numeric cell of the results table, right aligned.
fn cell(value: impl Into<SharedString>, width: f32) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .text_right()
        .child(value.into())
}

/// A column title of the results table, right aligned, what it means in a tooltip.
fn head_cell(
    id: &'static str,
    label: &'static str,
    tip: impl Into<SharedString>,
    width: f32,
) -> AnyElement {
    let tip = tip.into();
    div()
        .id(SharedString::from(format!("wizard-head-{id}")))
        .w(px(width))
        .flex_none()
        .text_right()
        .child(label)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .into_any_element()
}

/// How many of a module go on, ahead of its variant dropdown.
fn count_cell(n: usize, cx: &App) -> AnyElement {
    div()
        .w(px(24.))
        .flex_none()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(format!("{n}x"))
        .into_any_element()
}

/// A variation in the variant menu: icon, name, tier and meta level, and price when known.
fn variant_entry(
    loaded: &Rc<Loaded>,
    id: TypeId,
    name: &str,
    detail: &str,
    price: Option<f64>,
) -> PopupMenuItem {
    let loaded = Rc::clone(loaded);
    let name = SharedString::from(name.to_owned());
    let detail = SharedString::from(detail.to_owned());
    let price = SharedString::from(
        price.map_or_else(String::new, |p| format!("{} ISK", widgets::short(p))),
    );
    PopupMenuItem::element(move |_, cx: &mut App| {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .gap_2()
            .py(px(3.))
            .w(px(440.))
            .child(widgets::icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(id),
                20.,
            ))
            .child(div().flex_1().min_w_0().truncate().child(name.clone()))
            .child(
                div()
                    .w(px(110.))
                    .text_xs()
                    .text_color(muted)
                    .child(detail.clone()),
            )
            .child(
                div()
                    .w(px(70.))
                    .text_xs()
                    .text_right()
                    .text_color(muted)
                    .child(price.clone()),
            )
    })
}

impl Render for WizardPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused {
            self.focused = true;
            let focus = self.focus.clone();
            window.defer(cx, move |window, cx| window.focus(&focus, cx));
        }
        let viewport = window.viewport_size();
        self.fold_preview = viewport.width < px(FOLD_PREVIEW_W);
        self.compact = viewport.width < px(COMPACT_W);
        if !self.fold_preview {
            self.show_preview = false;
        }
        // Folded, the preview shows in place of the step when asked for.
        let (show_body, show_preview) = match (self.fold_preview, self.show_preview) {
            (false, _) => (true, true),
            (true, preview) => (!preview, preview),
        };
        let cursor = self.current_row();
        let header = self.header(cx);
        let body = show_body.then(|| self.step_body(cursor, cx));
        let footer = self.footer(cx);
        let preview = show_preview.then(|| self.preview(cx));
        let height = viewport.height - px(2. * inset(window));
        let theme = cx.theme();
        let folded = self.fold_preview;
        let compact = self.compact;
        v_flex()
            .track_focus(&self.focus)
            .key_context("FittingWizard")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.key(event, window, cx);
            }))
            .w_full()
            .h(height)
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .children(body.map(|body| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .when(compact, |d| d.px_1().pt_3())
                            .when(!compact, |d| d.px_3().pt_5())
                            .child(body)
                    }))
                    .children(preview.map(|preview| {
                        v_flex()
                            .id("wizard-preview")
                            .when(folded, |d| d.flex_1().min_w_0())
                            .when(!folded, |d| {
                                d.w(px(PREVIEW_W))
                                    .flex_none()
                                    .border_l_1()
                                    .border_color(theme.border)
                            })
                            .h_full()
                            .overflow_y_scroll()
                            .when(compact, |d| d.px_3().pt_3())
                            .when(!compact, |d| d.px_5().pt_5())
                            .pb_4()
                            .child(preview)
                    })),
            )
            .child(footer)
    }
}

/// The numbers of repairers the tank step offers; None is Auto.
const REPAIRER_CHOICES: [Option<u8>; 5] = [None, Some(1), Some(2), Some(3), Some(4)];

fn repairer_label(n: Option<u8>) -> String {
    n.map_or_else(|| "Auto".to_owned(), |n| n.to_string())
}
