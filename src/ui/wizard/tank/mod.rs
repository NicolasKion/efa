//! The tank section: what to aim for (max EHP, max resist multiplier or max repair in EHP/s),
//! the layer and what the search may use on the left; the best tanks it found as rows in the
//! middle, each with the modules it fits, its goal number and the change against the fit; and
//! the picked one on the right in the main window's stats sections, with what it changes and the
//! modules it puts on and takes off. The search runs off the UI thread, after the prices of
//! everything it may try are in.

mod controls;
mod detail;
mod results;
mod text;

use std::{rc::Rc, sync::Arc, time::Duration};

use gpui_kit::component::{
    ActiveTheme, h_flex,
    input::{InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, WeakEntity, Window, div, px,
};

use super::{Efa, Layout, Shared, Stat, Target, divider};
use crate::{
    data::Slot,
    engine::Stats,
    fit::Fit,
    prices::Prices,
    ui::{stats, widgets},
    wizard::{
        self, Budget, Change,
        tank::{Choices, Found, Numbers, Outcome, Problem, Progress},
    },
};
use controls::controls_column;
use text::slot_word;

/// Tanks the search lists.
const RESULTS: usize = 6;
/// The controls column in a wide window.
const CONTROLS_W: f32 = 250.;
/// The picked tank's column: the main window's stats pane with its padding.
const DETAIL_W: f32 = stats::PANE_WIDTH + 16.;

/// How the results are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Goal,
    Price,
    Ehp,
}

/// A running search. Dropping it calls the search off.
struct Search {
    progress: Arc<Progress>,
    _task: Task<()>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

/// The last search: what it was for and what it found.
struct Searched {
    choices: Choices,
    problem: Problem,
    found: Found,
}

/// The section's room on the fit: how many tank modules and rigs come off, and how many
/// positions the search fills.
#[derive(Debug, Clone, Copy, Default)]
struct RoomCounts {
    modules_off: usize,
    rigs_off: usize,
    slots: usize,
    rigs: usize,
}

/// A picked tank: its change, the numbers the fit gets with it, and the fit with it made and its
/// stats, which the preview shows in the main window's sections.
struct Picked {
    change: Change,
    numbers: Numbers,
    fit: Fit,
    stats: Stats,
}

pub(in crate::ui) struct TankSection {
    shared: Rc<Shared>,
    pub(super) focus: FocusHandle,
    prices: Rc<Prices>,
    choices: Choices,
    room: RoomCounts,
    /// The fit's numbers now, on the chosen layer.
    now: Numbers,
    search: Option<Search>,
    /// Prices are being fetched ahead of the search of this generation.
    fetching: Option<u64>,
    /// Tells a finished search from one that was called off or replaced.
    generation: u64,
    searched: Option<Searched>,
    /// Index into the found tanks.
    selected: Option<usize>,
    /// The picked tank, with what the fit becomes.
    after: Option<Picked>,
    /// Why the last search found nothing, or what it is waiting for.
    note: Option<SharedString>,
    sort: Sort,
    /// How the fit stands against the copy the section works on.
    target: Target,
    /// Budgets, typed as "75m" or "1.5b", with presets under them.
    budget_each: Entity<InputState>,
    budget_total: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl TankSection {
    pub(super) fn new(
        shared: Rc<Shared>,
        choices: Choices,
        prices: Rc<Prices>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = |amount: Option<u64>, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder("Any");
                if let Some(isk) = amount {
                    state.set_value(Budget::label(Some(isk)), window, cx);
                }
                state
            })
        };
        let budget_each = input(choices.budget.each, window, cx);
        let budget_total = input(choices.budget.total, window, cx);
        // Typing a budget sets it; text that is not an amount leaves it as it was.
        let on_type = |cx: &mut Context<Self>,
                       field: &Entity<InputState>,
                       set: fn(&mut Choices, Option<u64>)| {
            cx.subscribe(
                field,
                move |this: &mut Self, input, event: &InputEvent, cx| {
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
                    this.change(cx, |c| set(c, amount));
                },
            )
        };
        let subscriptions = vec![
            on_type(cx, &budget_each, |c, v| c.budget.each = v),
            on_type(cx, &budget_total, |c, v| c.budget.total = v),
        ];
        let mut section = Self {
            shared,
            focus: cx.focus_handle(),
            prices,
            choices,
            room: RoomCounts::default(),
            now: Numbers::default(),
            search: None,
            fetching: None,
            generation: 0,
            searched: None,
            selected: None,
            after: None,
            note: None,
            sort: Sort::Goal,
            target: Target::Same,
            budget_each,
            budget_total,
            _subscriptions: subscriptions,
        };
        section.measure();
        section
    }

    /// Where the section stands, for the section list.
    pub(super) fn summary(&self) -> SharedString {
        let goal = self.choices.goal.label().to_lowercase();
        let layer = self.choices.layer.label();
        match self.selected {
            Some(i) => format!("{layer}, {goal}, #{} picked", i + 1).into(),
            None => format!("{layer}, {goal}").into(),
        }
    }

    /// The fit's tank now, for the header.
    pub(super) fn stats(&self) -> Vec<Stat> {
        let n = &self.now;
        let layer = self.choices.layer.label();
        vec![
            Stat {
                label: "EHP".into(),
                value: widgets::short(n.ehp).into(),
                tip: "Effective HP of all layers against the damage pattern, now".into(),
            },
            Stat {
                label: format!("{layer} resists").into(),
                value: format!("{:.2}x", n.multiplier).into(),
                tip: format!("The {layer} layer's EHP per raw HP, now").into(),
            },
            Stat {
                label: format!("{layer} repair").into(),
                value: format!("{:.0} EHP/s", n.repair_ehp).into(),
                tip: "Effective HP repaired a second that the capacitor keeps up, guns idle".into(),
            },
        ]
    }

    /// Work out the room the choices leave and the fit's numbers on the layer.
    fn measure(&mut self) {
        let shared = &self.shared;
        let room = shared.setup().tank_room(&shared.fit, &self.choices);
        let rigs_off = room.removed.iter().filter(|r| r.0 == Slot::Rig).count();
        self.room = RoomCounts {
            modules_off: room.removed.len() - rigs_off,
            rigs_off,
            slots: room.positions.len(),
            rigs: room.rig_positions.len(),
        };
        self.now = Numbers::after(
            &shared.loaded.engine,
            &shared.pilot,
            &shared.fit,
            &Change::default(),
            self.choices.layer,
        );
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Choices)) {
        let before = self.choices;
        f(&mut self.choices);
        if self.choices == before {
            return;
        }
        // Another layer or goal makes the found tanks moot: tanks found for max EHP are not
        // the ones to show under max repair. A goal change searches again right away when
        // there were results, ranked by the new goal.
        let layer = self.choices.layer != before.layer;
        let goal = self.choices.goal != before.goal;
        let had_results = self.searched.is_some();
        if layer || goal {
            self.searched = None;
            self.selected = None;
            self.after = None;
            self.note = None;
            self.search = None;
            self.stop_fetching();
            self.sort = Sort::Goal;
        }
        if layer || self.choices.refit != before.refit {
            self.measure();
        }
        if goal && !layer && had_results {
            self.find_best(cx);
        }
        cx.notify();
    }

    /// The choices changed since the results were found.
    fn stale(&self) -> bool {
        self.searched
            .as_ref()
            .is_some_and(|s| s.choices != self.choices)
    }

    fn busy(&self) -> bool {
        self.search.is_some() || self.fetching.is_some()
    }

    fn outcomes(&self) -> &[Outcome] {
        self.searched
            .as_ref()
            .map_or(&[], |s| s.found.outcomes.as_slice())
    }

    /// The found tanks' indices in the order the sort asks for.
    fn order(&self) -> Vec<usize> {
        let outcomes = self.outcomes();
        let mut order: Vec<usize> = (0..outcomes.len()).collect();
        match self.sort {
            Sort::Goal => {}
            Sort::Price => order.sort_by(|&a, &b| outcomes[a].price.total_cmp(&outcomes[b].price)),
            Sort::Ehp => {
                order.sort_by(|&a, &b| outcomes[b].numbers.ehp.total_cmp(&outcomes[a].numbers.ehp));
            }
        }
        order
    }

    fn problem(&self) -> Problem {
        let shared = &self.shared;
        shared.setup().tank_problem(
            &shared.loaded.engine,
            &shared.pilot,
            &shared.fit,
            &self.choices,
            &self.prices,
        )
    }

    fn find_best(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let problem = self.problem();
        if problem.is_empty() {
            self.note = Some(
                format!(
                    "No free {} slots or rigs. Set Slots to Refit tank to redo the fitted one.",
                    slot_word(self.choices.layer.slot())
                )
                .into(),
            );
            cx.notify();
            return;
        }
        // Prices first, for every module and rig the search may try, so budgets and prices go
        // by real numbers rather than whatever happened to be fetched already.
        let types = problem.types();
        // The next fit starts from what was searched with.
        let choices = self.choices;
        self.shared
            .remember(cx, move |s| s.settings.wizard.tank = choices);
        self.note = None;
        // The window the wizard came from fetches prices; without it, search with what there is.
        if self.shared.efa.upgrade().is_none() {
            self.run_search(cx);
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.fetching = Some(generation);
        cx.notify();
        let section = cx.weak_entity();
        Efa::app_update(&self.shared.efa, cx, move |g, cx| {
            g.with_prices_for(types, cx, move |g, cx| {
                let prices = Rc::clone(&g.prices);
                // The section may be the one asking, so it is updated once this is over.
                cx.defer(move |cx| {
                    let _ = section.update(cx, |s, cx| {
                        if s.fetching == Some(generation) {
                            s.fetching = None;
                            s.prices = prices;
                            s.run_search(cx);
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
        let problem = self.problem();
        let choices = self.choices;
        self.generation += 1;
        let generation = self.generation;
        let engine = Arc::clone(&self.shared.loaded.engine);
        let progress = Arc::new(Progress::default());
        let shared = Arc::clone(&progress);
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let work = cx.background_spawn(async move {
                let found = wizard::tank::optimise(&engine, &problem, &shared, RESULTS);
                (problem, found)
            });
            // Redraw the progress bar while the search runs.
            let ticker = {
                let this = this.clone();
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                        let running = this.update(cx, |s, cx| {
                            cx.notify();
                            s.search.is_some() && s.generation == generation
                        });
                        if !matches!(running, Ok(true)) {
                            break;
                        }
                    }
                })
            };
            let (problem, found) = work.await;
            drop(ticker);
            let _ = this.update(cx, |s, cx| {
                s.finish(generation, choices, problem, found, cx);
            });
        });
        self.search = Some(Search {
            progress,
            _task: task,
        });
        cx.notify();
    }

    /// Stop waiting for prices: the answer, when it comes, starts no search.
    fn stop_fetching(&mut self) {
        if self.fetching.take().is_some() {
            self.generation += 1;
        }
    }

    fn cancel_search(&mut self, cx: &mut Context<Self>) {
        let fetching = self.fetching.is_some();
        self.stop_fetching();
        if self.search.take().is_some() || fetching {
            cx.notify();
        }
    }

    fn finish(
        &mut self,
        generation: u64,
        choices: Choices,
        problem: Problem,
        found: Found,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation || self.search.is_none() {
            return;
        }
        self.search = None;
        if found.outcomes.is_empty() {
            self.note = Some(
                "No tank fits in the CPU, powergrid and calibration. Try other kinds of module, \
                 or a looser budget."
                    .into(),
            );
            cx.notify();
            return;
        }
        self.note = None;
        self.searched = Some(Searched {
            choices,
            problem,
            found,
        });
        self.select(0, cx);
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(searched) = &self.searched else {
            return;
        };
        let Some(outcome) = searched.found.outcomes.get(i) else {
            return;
        };
        let shared = &self.shared;
        let change = outcome.change(&searched.problem, &shared.loaded.catalog);
        let (fit, stats) =
            wizard::tank::stats_after(&shared.loaded.engine, &shared.pilot, &shared.fit, &change);
        let numbers = Numbers::of_stats(&stats, searched.choices.layer);
        self.selected = Some(i);
        self.after = Some(Picked {
            change,
            numbers,
            fit,
            stats,
        });
        cx.notify();
    }

    /// Pick the result before or after the picked one, in the order shown.
    fn step_selection(&mut self, back: bool, cx: &mut Context<Self>) {
        let order = self.order();
        if order.is_empty() {
            return;
        }
        let at = self
            .selected
            .and_then(|s| order.iter().position(|&i| i == s));
        let next = match (at, back) {
            (None, _) => 0,
            (Some(at), true) => at.saturating_sub(1),
            (Some(at), false) => (at + 1).min(order.len() - 1),
        };
        self.select(order[next], cx);
    }

    /// Whether a picked tank can go on: there is one, and the fit is still there.
    fn can_apply(&self) -> bool {
        self.after.is_some() && self.target != Target::Gone
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_apply() {
            return;
        }
        let Some(Picked { change, .. }) = self.after.take() else {
            return;
        };
        self.search = None;
        let choices = self.choices;
        self.shared
            .remember(cx, move |s| s.settings.wizard.tank = choices);
        self.shared.apply(change, window, cx);
    }

    /// How the fit stands, from the window.
    pub(super) fn set_target(&mut self, target: Target, cx: &mut Context<Self>) {
        self.target = target;
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Keys typed into a text field are the field's, not the section's shortcuts.
        if !self.focus.is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.modifiers.alt || keystroke.modifiers.control {
            return;
        }
        match keystroke.key.as_str() {
            "enter" if self.can_apply() => self.apply(window, cx),
            "up" => self.step_selection(true, cx),
            "down" => self.step_selection(false, cx),
            "f" => self.find_best(cx),
            "escape" if self.busy() => self.cancel_search(cx),
            key => match key.parse::<usize>() {
                Ok(n) if n >= 1 => {
                    if let Some(&i) = self.order().get(n - 1) {
                        self.select(i, cx);
                    }
                }
                _ => return,
            },
        }
        cx.stop_propagation();
    }

    /// Give the keyboard back to the section after a click on a control.
    fn refocus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }
}

impl Render for TankSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = Layout::of(window);
        let across = layout == Layout::Narrow && window.viewport_size().width >= px(600.);
        let controls = self.controls(across, cx);
        let search = self.search_block(cx);
        let results = self.results(cx);
        let detail = self.detail(layout == Layout::Wide, cx);
        let theme = cx.theme();
        let scroll = |id: &'static str| v_flex().id(id).h_full().min_h_0().overflow_y_scroll();
        let side = theme.sidebar;
        let body: AnyElement = match layout {
            Layout::Wide => h_flex()
                .size_full()
                .items_start()
                .child(controls_column(controls, search, CONTROLS_W, side, cx))
                .child(scroll("wizard-results").flex_1().min_w_0().child(results))
                .child(
                    scroll("wizard-detail")
                        .w(px(DETAIL_W))
                        .flex_none()
                        .p_2()
                        .bg(side)
                        .border_l_1()
                        .border_color(theme.border)
                        .child(detail),
                )
                .into_any_element(),
            Layout::Split => h_flex()
                .size_full()
                .items_start()
                .child(controls_column(
                    controls,
                    search,
                    CONTROLS_W - 20.,
                    side,
                    cx,
                ))
                .child(
                    scroll("wizard-results")
                        .flex_1()
                        .min_w_0()
                        .pb_2()
                        .gap_2()
                        .child(results)
                        .child(divider(cx))
                        .child(div().px_2().child(detail)),
                )
                .into_any_element(),
            Layout::Narrow => scroll("wizard-all")
                .size_full()
                .py_1()
                .gap_2()
                .child(div().px_2().child(controls.child(search)))
                .child(divider(cx))
                .child(results)
                .child(divider(cx))
                .child(div().px_2().child(detail))
                .into_any_element(),
        };
        v_flex()
            .track_focus(&self.focus)
            .key_context("FittingWizard")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.key(event, window, cx);
            }))
            .size_full()
            .child(div().flex_1().min_h_0().child(body))
            .child(self.footer(cx))
    }
}
