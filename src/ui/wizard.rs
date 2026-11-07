//! The fitting wizard: an overlay over nearly the whole window, with tabs for the loadout, the
//! tank and the rigs and a live preview beside them. A tab adds to the fit only once something
//! on it changed, so the tank tab alone fills the free slots around what is fitted. Each tab is
//! captioned sections of rows, a fixed label column on the left (its description in a tooltip)
//! and the controls filling the rest. Every module row picks its own variation; the tank tab can
//! search for the best tank off the UI thread and lists what it found as a table. The preview
//! shows what goes where, what it costs and how it tanks. The picking is in [`crate::wizard`].

use std::{rc::Rc, sync::Arc, time::Duration};

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

use super::{Action, Gyfa, Loaded, item_stats, widgets};
use crate::{
    data::{Slot, TypeId},
    engine::{Cap, Layer as LayerStats, Pilot, Resource, Stats},
    fit::{DamageProfile, Fit},
    icons::Icon,
    item_data::ItemData,
    prices::Prices,
    theme::Palette,
    wizard::{
        self, Ammo, Budget, Category, Choices, DamageMods, Facts, Family, Goal, Layer, Pick, Prop,
        Rigs, SearchGroup, Setup, Tab, Tabs, TankStyle, Tier, Utility,
        tank::{self, Found, Limit, Outcome, Progress},
    },
};

/// Room the overlay leaves to each edge of the window.
const INSET: f32 = 24.;
const PREVIEW_W: f32 = 300.;
const ICON: f32 = 16.;
/// Icons in the results table's module strip.
const STRIP_ICON: f32 = 20.;
const LABEL_W: f32 = 140.;
const ROW_H: f32 = 32.;
/// The narrowest a variant dropdown gets before it wraps under the row's other controls.
const VARIANT_MIN_W: f32 = 160.;
/// A CPU or powergrid cell of the results table.
const RESOURCE_W: f32 = 64.;
/// Tanks the optimiser shows.
const RESULTS: usize = 5;

/// Dogma attributes the rows describe.
mod attr {
    pub const CAP_NEED: i32 = 6;
    pub const SPEED_FACTOR: i32 = 20;
    pub const SIG_BLOOM: i32 = 554;
    pub const UPGRADE_COST: i32 = 1153;
}

/// The rows of choices. A tab's rows go in keyboard order: its left column, then its right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Tier,
    /// Whether a tab fills the free slots or empties its racks first.
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

    /// The tab a change on the row puts on the fit; None for the bling, which every tab uses.
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
            | Row::Module(_) => Some(Tab::Tank),
            Row::Rigs | Row::Rig(_) => Some(Tab::Rigs),
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
    gyfa: WeakEntity<Gyfa>,
    loaded: Rc<Loaded>,
    data: Rc<ItemData>,
    pilot: Rc<Pilot>,
    prices: Rc<Prices>,
    /// The fit as it was when the wizard opened.
    fit: Fit,
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
    /// Tells a finished search from one that was called off or replaced.
    generation: u64,
    results: Vec<Outcome>,
    /// The choices the results were found for, without the tank they picked.
    results_for: Option<Choices>,
    selected: Option<usize>,
    /// Why the last search found nothing.
    note: Option<SharedString>,
    /// What the last search dropped the most tanks for not fitting.
    limit: Option<Limit>,
    /// Fewer repairers than chosen fit, and this many did.
    repairers_short: Option<usize>,
    focus: FocusHandle,
    focused: bool,
    palette: Palette,
    title: SharedString,
    tab: Tab,
    /// Custom budgets, typed as "75m" or "1.5b", next to the presets.
    budget_each: Entity<InputState>,
    budget_total: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Gyfa {
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
                            Gyfa::app_update(&this, cx, |g, cx| {
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
        // Nothing goes on until a tab's choices change.
        choices.touched = Tabs::NONE;
        let tab = self.saved.wizard_tab;
        let gyfa = cx.entity().downgrade();
        let palette = self.palette;
        let prices = Rc::clone(&self.prices);
        let title = format!("Fitting Wizard: {}", loaded.catalog.name(facts.ship)).into();
        let panel = cx.new(|cx| {
            let input = |cx: &mut Context<WizardPanel>, window: &mut Window| {
                cx.new(|cx| InputState::new(window, cx).placeholder("Custom"))
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
                gyfa,
                loaded,
                data,
                pilot,
                prices,
                fit,
                facts,
                families,
                choices,
                picks: Vec::new(),
                stats: Stats::default(),
                made_to_fit: false,
                row: 0,
                search: None,
                generation: 0,
                results: Vec::new(),
                results_for: None,
                selected: None,
                note: None,
                limit: None,
                repairers_short: None,
                focus: cx.focus_handle(),
                focused: false,
                palette,
                title,
                tab,
                budget_each,
                budget_total,
                _subscriptions: subscriptions,
            };
            panel.replan();
            panel
        });
        // The panel draws its own title and padding, so the overlay is all of it.
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .w(window.viewport_size().width - px(2. * INSET))
                .margin_top(px(INSET))
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
        Gyfa::app_update(&self.gyfa, cx, |g, cx| {
            g.saved.wizard = bling;
            g.apply(Action::ApplyWizard { placements, clear }, cx);
        });
    }

    /// Change the choices; `touch` is the tab whose choices that puts on the fit.
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

    fn find_best(&mut self, cx: &mut Context<Self>) {
        if self.search.is_some() {
            return;
        }
        // Searching is a tank choice: the tank tab goes on the fit.
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
        self.note = Some("Fetching prices...".into());
        cx.notify();
        let wizard = cx.entity().downgrade();
        Gyfa::app_update(&self.gyfa, cx, move |g, cx| {
            g.with_prices_for(types, cx, move |g, cx| {
                let prices = Rc::clone(&g.prices);
                // The wizard may be the one asking, so it is updated once this is over.
                cx.defer(move |cx| {
                    let _ = wizard.update(cx, |w, cx| {
                        if w.generation == generation {
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

    fn cancel_search(&mut self, cx: &mut Context<Self>) {
        if self.search.take().is_some() {
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

    /// A tab's rows: its left column and its right one.
    fn tab_rows(&self, tab: Tab) -> (Vec<Row>, Vec<Row>) {
        match tab {
            Tab::Loadout => {
                let mut left = vec![Row::Tier, Row::Racks(tab), Row::Weapon];
                if self.choices.family.is_some() {
                    left.push(Row::Ammo);
                }
                left.push(Row::Prop);
                let mut right: Vec<Row> = Utility::ALL.into_iter().map(Row::Utility).collect();
                right.push(Row::Damage);
                (left, right)
            }
            Tab::Tank => {
                let mut left = vec![Row::Racks(tab), Row::Style];
                if self.choices.tank != TankStyle::None {
                    left.extend([Row::Layer, Row::Goal]);
                    if self.choices.tank == TankStyle::Active {
                        left.push(Row::Repairers);
                    }
                    left.extend([Row::Tiers, Row::BudgetEach, Row::BudgetTotal, Row::Search]);
                }
                left.extend(self.module_rows().into_iter().map(|(b, _)| Row::Module(b)));
                let right = if self.choices.tank != TankStyle::None && !self.results.is_empty() {
                    vec![Row::Results]
                } else {
                    Vec::new()
                };
                (left, right)
            }
            Tab::Rigs => {
                let mut left = vec![Row::Racks(tab), Row::Rigs];
                left.extend(self.rig_rows().into_iter().map(|(b, _)| Row::Rig(b)));
                (left, Vec::new())
            }
        }
    }

    /// The current tab's rows, in keyboard order.
    fn rows(&self) -> Vec<Row> {
        let (mut rows, right) = self.tab_rows(self.tab);
        rows.extend(right);
        rows
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        self.row = 0;
        Gyfa::app_update(&self.gyfa, cx, |g, _| {
            g.saved.wizard_tab = tab;
            g.save();
        });
        cx.notify();
    }

    /// The picks a tab adds: the loadout's modules, the tank with its rigs, or every rig.
    fn tab_picks(&self, tab: Tab) -> impl Iterator<Item = &Pick> {
        self.picks.iter().filter(move |p| match tab {
            Tab::Loadout => !p.tank && p.slot != Slot::Rig,
            Tab::Tank => p.tank,
            Tab::Rigs => p.slot == Slot::Rig,
        })
    }

    /// One line on what a tab adds to the fit, or why it adds nothing.
    fn summary(&self, tab: Tab) -> String {
        let word = |slot: Slot| match slot {
            Slot::High => "high",
            Slot::Mid => "mid",
            Slot::Low => "low",
            _ => "rig",
        };
        let counts: Vec<String> = [Slot::High, Slot::Mid, Slot::Low, Slot::Rig]
            .into_iter()
            .filter_map(|slot| {
                let n = self.tab_picks(tab).filter(|p| p.slot == slot).count();
                (n > 0).then(|| format!("{n} {}{}", word(slot), if n == 1 { "" } else { "s" }))
            })
            .collect();
        if !counts.is_empty() {
            return format!("Adds {}", join_and(&counts, "and"));
        }
        let cleared = self.choices.cleared();
        let racks = self.choices.racks(tab);
        let free = racks.iter().any(|&slot| {
            let others = self
                .picks
                .iter()
                .filter(|p| p.slot == slot && !self.tab_picks(tab).any(|q| std::ptr::eq(q, *p)))
                .count();
            self.setup().free(&self.fit, slot, cleared).len() > others
        });
        if !free {
            let names: Vec<String> = racks.iter().map(|&s| word(s).to_owned()).collect();
            format!("Nothing to add: no free {} slots", join_and(&names, "or"))
        } else if !self.choices.touched.has(tab) {
            "Nothing to add yet: the fit stays as it is until something here changes".into()
        } else {
            "Nothing to add with these choices".into()
        }
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
                Row::Utility(_) | Row::Search => self.step(row, false, cx),
                _ => return,
            },
            "[" | "]" => self.step_variant(row, keystroke.key == "[", cx),
            "f" if self.choices.tank != TankStyle::None => self.find_best(cx),
            "escape" if self.search.is_some() => self.cancel_search(cx),
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
                    .child(label)
                    .when(usable, |d| {
                        d.cursor_pointer().on_click(cx.listener(
                            move |this, _: &ClickEvent, window, cx| {
                                this.focus_row(row, cx);
                                this.set_segment(row, i, cx);
                                this.refocus(window, cx);
                            },
                        ))
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
        Some(
            Button::new(SharedString::from(format!("wizard-variant-{}", row.key())))
                .outline()
                .xsmall()
                .tab_stop(false)
                .dropdown_caret(true)
                .flex_1()
                .min_w(px(VARIANT_MIN_W))
                .label(catalog.name(current))
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
            .label(label)
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

    /// A row's title and short description.
    fn text(&self, row: Row) -> (SharedString, SharedString) {
        let c = &self.choices;
        let catalog = &self.loaded.catalog;
        let (title, description): (String, String) = match row {
            Row::Tier => (
                "Bling".into(),
                "The tier every module starts at. Each module row can pick its own variation."
                    .into(),
            ),
            Row::Racks(tab) => ("Racks".into(), {
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
                            ", which the hull is built for"
                        } else {
                            ""
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
                    .map_or_else(|| "Unloaded".into(), |a| catalog.name(a)),
            ),
            Row::Prop => ("Propulsion".into(), self.prop_detail()),
            Row::Utility(u) => {
                let (title, text) = utility_text(u);
                let text = if !u.fits(self.facts.size) {
                    "Battleships and bigger only."
                } else if self.facts.total(u.slot()) == 0 {
                    "No slot for it on this hull."
                } else {
                    text
                };
                (title.into(), text.into())
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
                "Search tiers".into(),
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
                "Tank budget".into(),
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
            Row::Module(base) => (
                base_name(&catalog.name(base)),
                format!(
                    "{}x, {}",
                    self.count(base),
                    category_text(tank::category_of(catalog, base))
                ),
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
                format!(
                    "{:.0} of {:.0} calibration used.",
                    self.stats.calibration.used,
                    self.stats.calibration.output.unwrap_or(0.0)
                ),
            ),
            Row::Rig(base) => {
                let id = self.variant_of(row).map_or(base, |(_, _, current)| current);
                let cost = self
                    .data
                    .type_info(id)
                    .and_then(|i| i.attr(attr::UPGRADE_COST))
                    .unwrap_or(0.0);
                (
                    base_name(&catalog.name(base)),
                    format!("{}x, {cost:.0} calibration each", self.count(base)),
                )
            }
        };
        (title.into(), description.into())
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
        parts.join(", ")
    }

    fn search_detail(&self) -> String {
        if let Some(note) = &self.note {
            return note.to_string();
        }
        let pattern = match &self.fit.damage_profile {
            DamageProfile::Uniform => "uniform damage".to_owned(),
            DamageProfile::SelectedAmmo => "the selected ammo's damage".to_owned(),
            DamageProfile::Pattern { name, .. } => format!("the {name} pattern"),
        };
        let rack = match self.choices.layer {
            Layer::Shield => "mid",
            Layer::Armor => "low",
        };
        format!("Tries tanks for the free {rack} slots and rigs against {pattern}, guns idle.")
    }

    /// The controls right of a row's label, which wrap when the column gets narrow. A variant
    /// dropdown comes last and takes the room left.
    fn controls(&self, row: Row, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        match row {
            Row::Weapon => out.push(self.family_button(cx)),
            Row::Utility(u) => out.push(
                Switch::new(SharedString::from(format!("wizard-{}", row.key())))
                    .checked(self.choices.has(u))
                    .disabled(!self.utility_usable(u))
                    .on_click(cx.listener(move |this, _: &bool, window, cx| {
                        this.focus_row(row, cx);
                        this.change(row.tab(), cx, |c| c.toggle(u));
                        this.refocus(window, cx);
                    }))
                    .into_any_element(),
            ),
            Row::Module(base) | Row::Rig(base) => {
                out.push(count_cell(self.count(base), cx));
            }
            Row::Search => {
                out.push(self.search_controls(cx));
                if let Some(note) = &self.note {
                    out.push(aside(note.clone(), cx));
                }
            }
            Row::Results => {}
            Row::BudgetEach | Row::BudgetTotal => {
                out.push(self.segmented(row, cx));
                let field = if row == Row::BudgetEach {
                    &self.budget_each
                } else {
                    &self.budget_total
                };
                out.push(
                    div()
                        .w(px(96.))
                        .child(Input::new(field).small())
                        .into_any_element(),
                );
            }
            _ => out.push(self.segmented(row, cx)),
        }
        out.extend(self.variant_button(row, cx));
        match row {
            Row::Ammo => out.push(aside(self.text(row).1, cx)),
            Row::Rigs => out.push(aside(
                format!(
                    "{:.0}/{:.0} calibration",
                    self.stats.calibration.used,
                    self.stats.calibration.output.unwrap_or(0.0)
                ),
                cx,
            )),
            _ => {}
        }
        out
    }

    fn search_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(search) = &self.search {
            let fraction = search.progress.fraction();
            return h_flex()
                .gap_2()
                .child(div().w(px(120.)).child(widgets::gauge(
                    f64::from(fraction),
                    self.palette.accent,
                    format!("{:.0}%", fraction * 100.0),
                    0.,
                    cx,
                )))
                .child(
                    Button::new("wizard-cancel")
                        .xsmall()
                        .outline()
                        .tab_stop(false)
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel_search(cx);
                            this.refocus(window, cx);
                        })),
                )
                .into_any_element();
        }
        Button::new("wizard-find")
            .xsmall()
            .primary()
            .tab_stop(false)
            .disabled(self.choices.tank == TankStyle::None)
            .icon(AssetIcon::Search)
            .label("Find best (F)")
            .on_click(cx.listener(|this, _, window, cx| {
                this.focus_row(Row::Search, cx);
                this.find_best(cx);
                this.refocus(window, cx);
            }))
            .into_any_element()
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
        let theme = cx.theme();
        let header = h_flex()
            .h(px(24.))
            .gap_3()
            .px_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().w(px(20.)).flex_none().child("#"))
            .child(cell("EHP", 52.))
            .child(cell(if multiplier { "Multiplier" } else { "Resist" }, 60.))
            .when(repair, |d| d.child(cell("HP/s", 48.)))
            .child(cell("CPU", RESOURCE_W))
            .child(cell("PG", RESOURCE_W))
            .child(cell("Price", 56.))
            .child(div().w(px(24.)).flex_none().text_center().child("Cap"))
            .child(div().flex_1().min_w_0().child("Modules"));
        let active = cursor == Some(Row::Results);
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
            .child(header)
            .children(rows)
            .children([short, limit, stale].into_iter().flatten().map(|note| {
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(note)
            }))
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

    /// One row: a fixed label column, its description in a tooltip, and the controls filling
    /// the rest. Marked while the keyboard is on it.
    fn row_element(&self, row: Row, cursor: Option<Row>, cx: &mut Context<Self>) -> AnyElement {
        if row == Row::Results {
            return self.results_table(cursor, cx);
        }
        let (title, description) = self.text(row);
        let disabled = matches!(row, Row::Utility(u) if !self.utility_usable(u));
        let controls = self.controls(row, cx);
        let theme = cx.theme();
        let active = cursor == Some(row);
        // Module titles truncate, so their tooltip names them in full.
        let tip: SharedString = match row {
            Row::Module(_) | Row::Rig(_) => format!("{title}: {description}").into(),
            _ => description,
        };
        let label = h_flex()
            .id(SharedString::from(format!("wizard-label-{}", row.key())))
            .w(px(LABEL_W))
            .flex_none()
            .gap_1()
            .when(disabled, |d| d.opacity(0.5))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .when(!tip.is_empty(), |d| {
                d.child(
                    UiIcon::new(AssetIcon::Info)
                        .xsmall()
                        .flex_none()
                        .text_color(theme.muted_foreground.opacity(0.6)),
                )
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            });
        h_flex()
            .id(SharedString::from(format!("wizard-row-{}", row.key())))
            .min_h(px(ROW_H))
            .gap_3()
            .px_2()
            .py_0p5()
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
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .flex_wrap()
                    .gap_2()
                    .children(controls),
            )
            .into_any_element()
    }

    /// A column of captioned sections, which scrolls on its own when the window is too short.
    fn column(
        &self,
        id: &'static str,
        rows: &[Row],
        cursor: Option<Row>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut groups: Vec<(&'static str, Vec<Row>)> = Vec::new();
        for &row in rows {
            let title = section_of(row);
            match groups.last_mut() {
                Some((t, rows)) if *t == title => rows.push(row),
                _ => groups.push((title, vec![row])),
            }
        }
        let mut column = v_flex()
            .id(id)
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .gap_5()
            .pb_4();
        for (title, rows) in groups {
            let elements: Vec<AnyElement> = rows
                .into_iter()
                .map(|r| self.row_element(r, cursor, cx))
                .collect();
            column = column.child(
                v_flex()
                    .gap_0p5()
                    .child(caption(title, cx))
                    .children(elements),
            );
        }
        column.into_any_element()
    }

    /// The tabs, the current one underlined; a dot marks those that add to the fit.
    fn tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let tabs: Vec<AnyElement> = Tab::ALL
            .into_iter()
            .map(|tab| {
                let current = tab == self.tab;
                let touched = self.choices.touched.has(tab);
                h_flex()
                    .id(SharedString::from(format!("wizard-tab-{tab:?}")))
                    .h_full()
                    .gap_1p5()
                    .px_2()
                    .border_b_2()
                    .text_sm()
                    .cursor_pointer()
                    .when(current, |d| {
                        d.border_color(theme.primary)
                            .text_color(theme.foreground)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!current, |d| {
                        d.border_color(theme.primary.opacity(0.))
                            .text_color(theme.muted_foreground)
                            .hover(|s| s.text_color(theme.foreground))
                    })
                    .child(tab.label())
                    .when(touched, |d| {
                        d.child(div().size(px(5.)).rounded_full().bg(theme.primary))
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.set_tab(tab, cx);
                        this.refocus(window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        h_flex().h_full().gap_2().children(tabs).into_any_element()
    }

    // The preview.

    fn racks(&self, cx: &App) -> Div {
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let mut racks = v_flex().gap_1();
        for slot in [Slot::High, Slot::Mid, Slot::Low, Slot::Rig] {
            let total = self.facts.total(slot);
            if total == 0 {
                continue;
            }
            let picks: Vec<&Pick> = self.picks.iter().filter(|p| p.slot == slot).collect();
            let kept = if self.choices.cleared().has(slot) {
                0
            } else if slot == Slot::Rig {
                self.fit.rigs.iter().flatten().count()
            } else {
                self.fit.rack(slot).iter().flatten().count()
            };
            // Same module and charge on one line, in the order they go on.
            let mut lines: Vec<((TypeId, Option<TypeId>), usize)> = Vec::new();
            for p in &picks {
                match lines.iter_mut().find(|(k, _)| *k == (p.type_id, p.charge)) {
                    Some((_, n)) => *n += 1,
                    None => lines.push(((p.type_id, p.charge), 1)),
                }
            }
            let name = match slot {
                Slot::High => "High",
                Slot::Mid => "Mid",
                Slot::Low => "Low",
                _ => "Rigs",
            };
            let used = (kept + picks.len()).min(total);
            racks = racks.child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(name)
                    .child(if kept > 0 {
                        format!("{used}/{total}, {kept} kept")
                    } else {
                        format!("{used}/{total}")
                    }),
            );
            for ((type_id, charge), n) in lines {
                racks = racks.child(
                    h_flex()
                        .gap_1()
                        .min_w_0()
                        .text_xs()
                        .child(widgets::icon_box(
                            catalog,
                            &self.loaded.icons,
                            Icon::Type(type_id),
                            ICON,
                        ))
                        .child(
                            div()
                                .truncate()
                                .child(format!("{n}x {}", catalog.name(type_id))),
                        )
                        .when_some(charge, |row, c| {
                            row.child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(catalog.name(c)),
                            )
                        }),
                );
            }
        }
        racks
    }

    fn preview(&self, cx: &App) -> Div {
        let gauges = v_flex()
            .gap_1()
            .child(self.resource("CPU", &self.stats.cpu, " tf", cx))
            .child(self.resource("Powergrid", &self.stats.powergrid, " MW", cx))
            .child(self.resource("Calibration", &self.stats.calibration, "", cx));
        let cost: f64 = self
            .picks
            .iter()
            .filter_map(|p| self.prices.get(p.type_id))
            .sum();
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .child(caption("Modules", cx))
            .child(self.racks(cx))
            .child(gauges)
            .when(cost > 0.0, |d| {
                d.child(
                    h_flex()
                        .justify_between()
                        .text_xs()
                        .child(div().text_color(muted).child("New modules cost"))
                        .child(format!("{} ISK", widgets::short(cost))),
                )
            })
            .child(caption("Tank", cx).mt_3())
            .child(self.tank_summary(cx))
            .child(caption("Damage", cx).mt_3())
            .child(self.damage_summary(cx))
    }

    fn tank_summary(&self, cx: &App) -> Div {
        let s = &self.stats;
        let catalog = &self.loaded.catalog;
        let icons = &self.loaded.icons;
        let ehp: f64 = [s.shield, s.armor, s.hull]
            .iter()
            .map(|l| l.ehp.unwrap_or(l.hp))
            .sum();
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
        let (layer, repair) = match self.choices.layer {
            Layer::Shield => ("Shield", s.sustained.shield),
            Layer::Armor => ("Armor", s.sustained.armor),
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
            .child(line("Effective HP", widgets::short(ehp), cx))
            .child(layer_row(widgets::icon::SHIELD, s.shield))
            .child(layer_row(widgets::icon::ARMOR, s.armor))
            .child(layer_row(widgets::icon::HULL, s.hull))
            .when(repair > 0.0, |d| {
                d.child(line(
                    &format!("{layer} repair, sustained"),
                    format!("{repair:.0} HP/s"),
                    cx,
                ))
            })
            .when(s.tank.passive_shield > 0.0, |d| {
                d.child(line(
                    "Passive shield regen",
                    format!("{:.0} HP/s", s.tank.passive_shield),
                    cx,
                ))
            })
            .child(line("Capacitor, all active", cap, cx))
    }

    fn damage_summary(&self, cx: &App) -> Div {
        let s = &self.stats;
        v_flex()
            .gap_1()
            .child(line("DPS", format!("{:.0}", s.dps.total()), cx))
            .when(s.drone_dps > 0.0, |d| {
                d.child(line("Of which drones", format!("{:.0}", s.drone_dps), cx))
            })
            .child(line("Volley", format!("{:.0}", s.volley.total()), cx))
    }

    fn resource(&self, label: &str, r: &Resource, unit: &str, cx: &App) -> Div {
        let out = r.output.unwrap_or(0.0);
        let fraction = if out > 0.0 { r.used / out } else { 0.0 };
        let over = r.used > out + 1e-6;
        let fill = if over {
            self.palette.red
        } else {
            self.palette.green
        };
        v_flex()
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_owned())
                    .child(format!("{:.1}/{out:.1}{unit}", r.used)),
            )
            .child(
                widgets::gauge(fraction, fill, format!("{:.1}%", fraction * 100.0), 0., cx)
                    .w_full(),
            )
    }
}

/// The section a row sits in.
fn section_of(row: Row) -> &'static str {
    match row {
        Row::Tier | Row::Racks(_) => "General",
        Row::Weapon | Row::Ammo => "Weapons",
        Row::Prop => "Propulsion",
        Row::Utility(_) => "Tackle & utility",
        Row::Style
        | Row::Layer
        | Row::Goal
        | Row::Repairers
        | Row::Tiers
        | Row::BudgetEach
        | Row::BudgetTotal
        | Row::Search
        | Row::Module(_) => "Tank",
        Row::Results => "Results",
        Row::Damage => "Damage modules",
        Row::Rigs | Row::Rig(_) => "Rigs",
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
        Category::Repairer => "repairs",
        Category::Buffer => "buffer",
        Category::Hardener => "resists",
        Category::Electronics => "capacitor",
        _ => "module",
    }
}

/// A T1 name without its " I", for row titles that name a module of any tier.
fn base_name(name: &str) -> String {
    name.strip_suffix(" I").unwrap_or(name).to_owned()
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

/// A small uppercase caption over a group of rows, with a hairline under it.
fn caption(title: &str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .px_1()
        .pb_1()
        .mb_1()
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(title.to_uppercase())
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

/// Muted text after a row's controls that takes what room is left and truncates.
fn aside(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}

/// How many of a module go on, ahead of its variant dropdown.
fn count_cell(n: usize, cx: &App) -> AnyElement {
    div()
        .w(px(20.))
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
        let fits = wizard::fits(&self.stats);
        let cursor = self.current_row();
        let (left, right) = self.tab_rows(self.tab);
        let left = self.column("wizard-left", &left, cursor, cx);
        let right = self.column("wizard-right", &right, cursor, cx);
        let summary = self.summary(self.tab);
        let tabs = self.tab_bar(cx);
        let count = self.picks.len();
        let preview = self.preview(cx);
        let height = window.viewport_size().height - px(2. * INSET);
        let theme = cx.theme();
        let header = h_flex()
            .flex_none()
            .h(px(44.))
            .px_5()
            // Room for the dialog's close button.
            .pr_12()
            .gap_6()
            .child(
                div()
                    .flex_none()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.title.clone()),
            )
            .child(tabs);
        let hint = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                "Ctrl+Tab or Ctrl+1-3: tab   Up/Down: row   Left/Right or 1-9: choose   \
                 [ ]: variant   Space: toggle   F: find the best tank   Enter: apply",
            );
        let footer = h_flex()
            .flex_none()
            .gap_3()
            .px_5()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .child(hint)
            .when(!fits, |f| {
                f.child(
                    Button::new("wizard-make-fit")
                        .small()
                        .tab_stop(false)
                        .icon(AssetIcon::Minimize2)
                        .label("Make it fit (M)")
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
                    .label(format!("Apply {count} modules"))
                    .disabled(count == 0)
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
            );
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
                    .gap_8()
                    .px_5()
                    .pt_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .gap_3()
                            .child(
                                h_flex()
                                    .flex_none()
                                    .gap_1p5()
                                    .px_2()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(UiIcon::new(AssetIcon::Info).xsmall())
                                    .child(summary),
                            )
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_h_0()
                                    .items_start()
                                    .gap_8()
                                    .child(left)
                                    .child(right),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("wizard-preview")
                            .w(px(PREVIEW_W))
                            .flex_none()
                            .h_full()
                            .overflow_y_scroll()
                            .pb_4()
                            .child(preview),
                    ),
            )
            .child(footer)
    }
}

/// The numbers of repairers the Tank tab offers; None is Auto.
const REPAIRER_CHOICES: [Option<u8>; 5] = [None, Some(1), Some(2), Some(3), Some(4)];

fn repairer_label(n: Option<u8>) -> String {
    n.map_or_else(|| "Auto".to_owned(), |n| n.to_string())
}
