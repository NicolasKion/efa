//! The tank section: what to aim for (max EHP, max resist multiplier or max repair in EHP/s),
//! the layer and what the search may use on the left; the best tanks it found as rows in the
//! middle, each with the modules it fits, its goal number and the change against the fit; and
//! the picked one on the right in the main window's stats sections, with what it changes and the
//! modules it puts on and takes off. The search runs off the UI thread, after the prices of
//! everything it may try are in.

use std::{rc::Rc, sync::Arc, time::Duration};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Efa, Layout, Segment, Shared, Stat, Target, divider, field, fieldset, join_and, muted, notice,
    plural, segmented,
};
use crate::{
    data::{Slot, TypeId},
    engine::{Resource, Stats},
    fit::{DamageProfile, Fit},
    icons::Icon,
    prices::Prices,
    theme::Palette,
    ui::{
        columns,
        editor_kit::empty_state,
        side,
        stats::{self, SectionKind},
        widgets,
    },
    wizard::{
        self, Budget, Change, Layer, SearchGroup,
        tank::{Choices, Found, Goal, Limit, Numbers, Outcome, Problem, Progress},
    },
};

/// Tanks the search lists.
const RESULTS: usize = 6;
/// The controls column in a wide window.
const CONTROLS_W: f32 = 250.;
/// The picked tank's column: the main window's stats pane with its padding.
const DETAIL_W: f32 = stats::PANE_WIDTH + 16.;
/// Icons in a result row.
const ICON: f32 = 18.;
/// Result columns: the goal number with its change, the other two numbers, fitting and price.
const GOAL_W: f32 = 96.;
const OTHER_W: f32 = 52.;
const FITTING_W: f32 = 44.;
const PRICE_W: f32 = 56.;
/// The most repairers the stepper goes to.
const MAX_REPAIRERS: u8 = 6;

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

/// What a module line in a preview is to the fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Added,
    Kept,
    Removed,
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
        // Another layer makes the found tanks moot.
        if self.choices.layer != before.layer {
            self.searched = None;
            self.selected = None;
            self.after = None;
            self.note = None;
            self.search = None;
            self.stop_fetching();
        }
        if self.choices.layer != before.layer || self.choices.refit != before.refit {
            self.measure();
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
        self.shared.remember(cx, move |s| s.wizard.tank = choices);
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
        self.shared.remember(cx, move |s| s.wizard.tank = choices);
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

    // The controls.

    fn controls(&self, across: bool, cx: &mut Context<Self>) -> Div {
        let _ = across;
        let c = self.choices;
        let mut search = vec![
            field("Goal", self.goals(cx), cx).into_any_element(),
            field("Layer", self.layers(cx), cx).into_any_element(),
        ];
        if c.goal == Goal::Repair {
            search.push(field("Repairers", self.repairers(cx), cx).into_any_element());
        }
        search.push(
            field(
                "Slots",
                segmented(
                    "wizard-slots",
                    vec![
                        Segment::new("Refit", c.refit).tip(
                            "Take off the tank modules and rigs fitted on the layer, and \
                             search their slots too",
                        ),
                        Segment::new("Free only", !c.refit)
                            .tip("Keep what is fitted and search only the free slots"),
                    ],
                    cx,
                    |this: &mut Self, i, window, cx| {
                        this.change(cx, |c| c.refit = i == 0);
                        this.refocus(window, cx);
                    },
                ),
                cx,
            )
            .into_any_element(),
        );
        v_flex()
            .gap_4()
            .child(fieldset("Search", search, cx))
            .child(fieldset("Module kinds", vec![self.kinds(cx)], cx))
            .child(fieldset(
                "Budget",
                vec![
                    self.budget(false, cx).into_any_element(),
                    self.budget(true, cx).into_any_element(),
                ],
                cx,
            ))
    }

    /// The goals side by side; what each aims for is in its tooltip.
    fn goals(&self, cx: &mut Context<Self>) -> AnyElement {
        let segments = Goal::ALL
            .into_iter()
            .map(|goal| {
                Segment::new(goal.short_label(), goal == self.choices.goal).tip(goal_text(goal))
            })
            .collect();
        segmented(
            "wizard-goal",
            segments,
            cx,
            |this: &mut Self, i, window, cx| {
                this.change(cx, |c| c.goal = Goal::ALL[i]);
                this.refocus(window, cx);
            },
        )
    }

    fn layers(&self, cx: &mut Context<Self>) -> AnyElement {
        let facts = &self.shared.facts;
        let segments = Layer::ALL
            .iter()
            .map(|&l| {
                let slots = facts.total(l.slot());
                Segment::new(l.label(), l == self.choices.layer).tip(format!(
                    "{} {} slots on this hull",
                    slots,
                    slot_word(l.slot())
                ))
            })
            .collect();
        segmented(
            "wizard-layer",
            segments,
            cx,
            |this: &mut Self, i, window, cx| {
                if this.shared.facts.total(Layer::ALL[i].slot()) > 0 {
                    this.change(cx, |c| c.layer = Layer::ALL[i]);
                }
                this.refocus(window, cx);
            },
        )
    }

    /// The number of repairers as a stepper: Auto, then 1 up.
    fn repairers(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let n = self.choices.repairers;
        let step = |id: &'static str, icon: IconName, back: bool, disabled: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .tab_stop(false)
                .icon(icon)
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.change(cx, |c| {
                        c.repairers = match (c.repairers, back) {
                            (None, false) => Some(1),
                            (Some(1) | None, true) => None,
                            (Some(n), true) => Some(n - 1),
                            (Some(n), false) => Some((n + 1).min(MAX_REPAIRERS)),
                        };
                    });
                    this.refocus(window, cx);
                }))
        };
        let tip = if self.choices.refit {
            "How many the tank ends up with; Auto lets the search decide"
        } else {
            "Repairers already fitted count"
        };
        h_flex()
            .id("wizard-repairers")
            .flex_none()
            .w_full()
            .gap_px()
            .rounded(px(4.))
            .border_1()
            .border_color(theme.border)
            .child(step("wizard-rep-less", IconName::Minus, true, n.is_none()))
            .child(
                div()
                    .flex_1()
                    .text_center()
                    .text_xs()
                    .child(n.map_or_else(|| "Auto".to_owned(), |n| n.to_string())),
            )
            .child(step(
                "wizard-rep-more",
                IconName::Plus,
                false,
                n == Some(MAX_REPAIRERS),
            ))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .into_any_element()
    }

    /// What the search may use, as checkboxes in two columns.
    fn kinds(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.weak_entity();
        let boxes = SearchGroup::ALL.into_iter().zip(KIND_TIPS).map(|(g, tip)| {
            let this = this.clone();
            div()
                .id(SharedString::from(format!("wizard-kind-tip-{g:?}")))
                .child(
                    Checkbox::new(SharedString::from(format!("wizard-kind-{g:?}")))
                        .small()
                        .label(g.label())
                        .checked(self.choices.search_tiers.has(g))
                        .on_click(move |_, window, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.change(cx, |c| c.search_tiers.toggle(g));
                                this.refocus(window, cx);
                            });
                        }),
                )
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        });
        div()
            .grid()
            .grid_cols(2)
            .gap_x_2()
            .gap_y_1()
            .pt_0p5()
            .children(boxes)
            .into_any_element()
    }

    /// One budget on one line: a field in ISK, with the presets in a menu beside it.
    fn budget(&self, total: bool, cx: &mut Context<Self>) -> Div {
        let (field_state, presets, current, title) = if total {
            (
                self.budget_total.clone(),
                Budget::TOTAL,
                self.choices.budget.total,
                "Whole tank",
            )
        } else {
            (
                self.budget_each.clone(),
                Budget::EACH,
                self.choices.budget.each,
                "Per module",
            )
        };
        let this = cx.weak_entity();
        let menu = Button::new(SharedString::from(format!("wizard-budget-presets-{total}")))
            .ghost()
            .xsmall()
            .tab_stop(false)
            .icon(IconName::ChevronDown)
            .tooltip("Presets")
            .dropdown_menu(move |mut menu, _, _| {
                for preset in presets {
                    let (this, field_state) = (this.clone(), field_state.clone());
                    menu = menu.item(
                        PopupMenuItem::new(Budget::label(preset))
                            .checked(preset == current)
                            .on_click(move |_, window, cx| {
                                let text =
                                    preset.map_or_else(String::new, |p| Budget::label(Some(p)));
                                field_state.update(cx, |f, cx| f.set_value(text, window, cx));
                                let _ = this.update(cx, |this, cx| {
                                    this.change(cx, |c| {
                                        if total {
                                            c.budget.total = preset;
                                        } else {
                                            c.budget.each = preset;
                                        }
                                    });
                                });
                            }),
                    );
                }
                menu
            });
        let input = if total {
            &self.budget_total
        } else {
            &self.budget_each
        };
        field(
            title,
            h_flex()
                .gap_0p5()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(input).xsmall().suffix(muted("ISK", cx).pr_1())),
                )
                .child(menu),
            cx,
        )
    }

    /// The search button, or its progress while it runs, and what it covers.
    fn search_block(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.shared.palette;
        let cancel = Self::cancel_button(cx);
        let theme = cx.theme();
        let mut block = v_flex().gap_1p5();
        if let Some(search) = &self.search {
            let fraction = search.progress.fraction();
            block = block.child(
                v_flex()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(
                                UiIcon::new(AssetIcon::LoaderCircle)
                                    .small()
                                    .text_color(theme.primary),
                            )
                            .child(div().flex_1().child("Searching tanks"))
                            .child(cancel),
                    )
                    .child(
                        widgets::gauge(
                            f64::from(fraction),
                            palette.accent,
                            format!("{:.0}%", fraction * 100.0),
                            0.,
                            cx,
                        )
                        .w_full(),
                    ),
            );
        } else {
            let fetching = self.fetching.is_some();
            block = block
                .child(
                    Button::new("wizard-find")
                        .small()
                        .w_full()
                        .tab_stop(false)
                        .loading(fetching)
                        .icon(AssetIcon::Search)
                        .label(if fetching {
                            "Fetching prices"
                        } else if self.searched.is_some() {
                            "Search again"
                        } else {
                            "Find best tanks"
                        })
                        .tooltip("F")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.find_best(cx);
                            this.refocus(window, cx);
                        })),
                )
                .when(fetching, |d| d.child(cancel));
        }
        let text = self
            .note
            .clone()
            .unwrap_or_else(|| self.search_text().into());
        let warn = self.note.is_some();
        block.child(if warn {
            notice(AssetIcon::TriangleAlert, palette.yellow, text, cx)
        } else {
            muted(text, cx)
        })
    }

    fn cancel_button(cx: &mut Context<Self>) -> Button {
        Button::new("wizard-cancel")
            .xsmall()
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

    /// What the search covers, in a sentence.
    fn search_text(&self) -> String {
        let r = self.room;
        let rack = slot_word(self.choices.layer.slot());
        let pattern = match &self.shared.fit.damage_profile {
            DamageProfile::Uniform => "uniform damage".to_owned(),
            DamageProfile::SelectedAmmo => "the selected ammo's damage".to_owned(),
            DamageProfile::Pattern { name, .. } => format!("the {name} pattern"),
        };
        let mut room = Vec::new();
        if r.slots > 0 {
            room.push(plural(r.slots, &format!("{rack} slot")));
        }
        if r.rigs > 0 {
            room.push(plural(r.rigs, "rig slot"));
        }
        if room.is_empty() {
            return format!("No free {rack} slots or rigs.");
        }
        let mut off = Vec::new();
        if r.modules_off > 0 {
            off.push(plural(r.modules_off, "tank module"));
        }
        if r.rigs_off > 0 {
            off.push(plural(r.rigs_off, "rig"));
        }
        let off = if off.is_empty() {
            String::new()
        } else {
            format!(" after taking off {}", join_and(&off))
        };
        format!(
            "Searches {}{off}, against {pattern} with the guns idle.",
            join_and(&room)
        )
    }

    // The results.

    fn results(&self, cx: &mut Context<Self>) -> Div {
        let palette = self.shared.palette;
        let Some(searched) = &self.searched else {
            return self.empty_results(cx);
        };
        let busy = self.busy();
        let goal = searched.choices.goal;
        let rows: Vec<AnyElement> = self
            .order()
            .into_iter()
            .enumerate()
            .map(|(rank, i)| self.result_row(rank, i, &searched.found.outcomes[i], goal, cx))
            .collect();
        let found = &searched.found;
        let notes: Vec<SharedString> = [
            found.repairers_short.map(|got| {
                SharedString::from(match got {
                    0 => "No repairer fits with the rest of the fit; these tank without one."
                        .to_owned(),
                    1 => "Only 1 more repairer fits with the rest of the fit.".to_owned(),
                    n => format!("Only {n} more repairers fit with the rest of the fit."),
                })
            }),
            found.rejected.limit().map(limit_text),
        ]
        .into_iter()
        .flatten()
        .collect();
        v_flex()
            .gap_2()
            .when(self.stale() && !busy, |d| {
                d.child(div().px_2().child(notice(
                    AssetIcon::RotateCcw,
                    palette.yellow,
                    "The choices changed since these were found; search again for fresh ones.",
                    cx,
                )))
            })
            .child(
                v_flex()
                    // While a new search runs the old results stay, dimmed.
                    .when(busy, |d| d.opacity(0.45))
                    .child(self.results_head(goal, cx))
                    .children(rows),
            )
            .children(notes.into_iter().map(|n| {
                div()
                    .px_2()
                    .child(notice(AssetIcon::Info, palette.blue, n, cx))
            }))
    }

    /// The column titles in the fitting view's table head; those the list sorts by are buttons.
    fn results_head(&self, goal: Goal, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let right = |text: &'static str, width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .text_right()
                .child(text)
                .into_any_element()
        };
        let sort_head = |sort: Sort, text: &'static str, width: f32| {
            let on = self.sort == sort;
            h_flex()
                .id(SharedString::from(format!("wizard-sort-{sort:?}")))
                .w(px(width))
                .flex_none()
                .justify_end()
                .gap_0p5()
                .cursor_pointer()
                .when(on, |d| d.text_color(theme.foreground))
                .hover(|h| h.text_color(theme.foreground))
                .when(on, |d| d.child(UiIcon::new(AssetIcon::ArrowDown).xsmall()))
                .child(text)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &gpui_kit::MouseDownEvent, window, cx| {
                        this.sort = sort;
                        cx.notify();
                        this.refocus(window, cx);
                    }),
                )
                .into_any_element()
        };
        let others: Vec<AnyElement> = Goal::ALL
            .into_iter()
            .filter(|&g| g != goal)
            .map(|g| match g {
                Goal::Ehp => sort_head(Sort::Ehp, "EHP", OTHER_W),
                g => right(goal_unit(g), OTHER_W),
            })
            .collect();
        widgets::table_head(cx)
            .child(div().w(px(columns::STATE_W)).flex_none().child("#"))
            .child(div().flex_1().min_w_0().child("Modules"))
            .child(sort_head(Sort::Goal, goal_unit(goal), GOAL_W))
            .children(others)
            .child(right("Fitting", FITTING_W))
            .child(sort_head(Sort::Price, "Price", PRICE_W))
    }

    fn empty_results(&self, cx: &App) -> Div {
        if self.busy() {
            empty_state(
                AssetIcon::LoaderCircle,
                "Searching",
                "The best tanks show up here.",
                cx,
            )
        } else {
            empty_state(
                AssetIcon::Sparkles,
                "No tanks yet",
                "Pick a goal and what the search may use, then Find best tanks.",
                cx,
            )
        }
    }

    /// One found tank as a list row: what it puts on, its goal number with the change against
    /// now, the other two numbers, how full the fitting gets and the price.
    fn result_row(
        &self,
        rank: usize,
        i: usize,
        o: &Outcome,
        goal: Goal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let palette = self.shared.palette;
        let n = &o.numbers;
        let value = n.value(goal);
        let (delta_text, delta_color) = delta(goal, self.now.value(goal), value, palette, cx);
        let others: Vec<String> = Goal::ALL
            .into_iter()
            .filter(|&g| g != goal)
            .map(|g| goal_number(g, n.value(g)))
            .collect();
        // The fuller of CPU and powergrid, as a share of the ship's output.
        let share = |r: &Resource| r.output.filter(|&o| o > 0.0).map_or(0.0, |o| r.used / o);
        let full = share(&n.cpu).max(share(&n.powergrid));
        let fitting_tip = SharedString::from(format!(
            "CPU {:.0} / {:.0}, powergrid {:.0} / {:.0}",
            n.cpu.used,
            n.cpu.output.unwrap_or(0.0),
            n.powergrid.used,
            n.powergrid.output.unwrap_or(0.0)
        ));
        let price = if o.price > 0.0 {
            format!(
                "{}{}",
                if o.priced { "" } else { "~" },
                widgets::short(o.price)
            )
        } else {
            "?".to_owned()
        };
        widgets::list_row(
            SharedString::from(format!("wizard-row-{i}")),
            self.selected == Some(i),
            cx,
        )
        .cursor_pointer()
        .child(
            div()
                .w(px(columns::STATE_W))
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(format!("{}", rank + 1)),
        )
        .child(self.module_strip(i, o, cx))
        .child(
            h_flex()
                .w(px(GOAL_W))
                .flex_none()
                .justify_end()
                .gap_1()
                .child(div().text_xs().text_color(delta_color).child(delta_text))
                .child(goal_number(goal, value)),
        )
        .children(others.into_iter().map(|v| {
            div()
                .w(px(OTHER_W))
                .flex_none()
                .text_right()
                .text_color(theme.muted_foreground)
                .child(v)
        }))
        .child(
            div()
                .id(SharedString::from(format!("wizard-row-{i}-fit")))
                .w(px(FITTING_W))
                .flex_none()
                .text_right()
                .when(full > 0.99, |d| {
                    d.text_color(widgets::color(palette.yellow))
                })
                .when(full <= 0.99, |d| d.text_color(theme.muted_foreground))
                .child(format!("{:.0}%", full * 100.0))
                .tooltip(move |window, cx| Tooltip::new(fitting_tip.clone()).build(window, cx)),
        )
        .child(
            div()
                .w(px(PRICE_W))
                .flex_none()
                .text_right()
                .when(!o.priced, |d| d.text_color(theme.muted_foreground))
                .child(price),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.select(i, cx);
            this.refocus(window, cx);
        }))
        .into_any_element()
    }

    /// What a found tank puts on, as icons with their counts, rigs after the modules;
    /// hovering one shows the item.
    fn module_strip(&self, i: usize, o: &Outcome, cx: &App) -> Div {
        let theme = cx.theme();
        let loaded = &self.shared.loaded;
        let mut lines: Vec<(TypeId, usize)> = Vec::new();
        for &id in o.types.iter().chain(&o.rig_types) {
            match lines.iter_mut().find(|(t, _)| *t == id) {
                Some((_, n)) => *n += 1,
                None => lines.push((id, 1)),
            }
        }
        h_flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .gap_2()
            .children(lines.into_iter().map(|(id, n)| {
                let tip = Rc::clone(loaded);
                let price = self.prices.get(id);
                h_flex()
                    .id(SharedString::from(format!("wizard-row-{i}-{id}")))
                    .flex_none()
                    .gap_0p5()
                    .child(widgets::icon_box(
                        &loaded.catalog,
                        &loaded.icons,
                        Icon::Type(id),
                        ICON,
                    ))
                    .when(n > 1, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{n}")),
                        )
                    })
                    .tooltip(move |window, cx| side::item_tooltip(&tip, id, price, window, cx))
            }))
    }

    // The picked tank.

    /// The picked tank: what it changes against the fit as it is, then the fit with it in the
    /// main window's resources, resistances and recharge sections, then its modules: under them
    /// in a `column`, else beside them.
    fn detail(&self, column: bool, cx: &App) -> Div {
        let (Some(picked), Some(i)) = (&self.after, self.selected) else {
            return empty_state(
                AssetIcon::MousePointerClick,
                "No tank picked",
                "Pick a result to see the fit with it.",
                cx,
            );
        };
        let shared = &self.shared;
        let pane = stats::Pane::plain(
            &shared.loaded,
            &picked.stats,
            &picked.fit,
            shared.palette,
            &self.prices,
        );
        let sections = stats::render_sections(
            &pane,
            &[
                SectionKind::Resources,
                SectionKind::Resistances,
                SectionKind::Recharge,
            ],
            None,
            cx,
        );
        // Under the results the stats keep the stats pane's width, with the modules beside them
        // when there is room.
        let stats = v_flex()
            .gap_2()
            .text_size(px(stats::TEXT_SIZE))
            .child(self.changes(i, picked, cx))
            .children(sections);
        let modules = self.module_list(&picked.change, cx);
        if column {
            v_flex().gap_2().child(stats).child(modules)
        } else {
            h_flex()
                .items_start()
                .flex_wrap()
                .gap_x_4()
                .gap_y_2()
                .child(stats.w(px(stats::PANE_WIDTH)).flex_none())
                .child(modules.flex_1().min_w(px(stats::PANE_WIDTH)))
        }
    }

    /// The picked tank's numbers against the fit's now, in a line under its name.
    fn changes(&self, i: usize, picked: &Picked, cx: &App) -> Div {
        let theme = cx.theme();
        let palette = self.shared.palette;
        let layer = self
            .searched
            .as_ref()
            .map_or(self.choices.layer, |s| s.choices.layer)
            .label();
        let rank = self.order().iter().position(|&o| o == i).unwrap_or(i) + 1;
        let (now, after) = (&self.now, &picked.numbers);
        let item = |goal: Goal, label: String| {
            let (before, value) = (now.value(goal), after.value(goal));
            let (text, color) = delta(goal, before, value, palette, cx);
            let tip = SharedString::from(format!(
                "{label}: {} now, {} with this tank",
                goal_number(goal, before),
                goal_number(goal, value)
            ));
            h_flex()
                .id(SharedString::from(format!("wizard-change-{goal:?}")))
                .gap_1()
                .child(div().text_color(theme.muted_foreground).child(label))
                .child(div().text_color(color).child(text))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        };
        let price = self
            .outcomes()
            .get(i)
            .map_or_default(|o| (o.price, o.priced));
        let cap_lost = now.cap_stable && !after.cap_stable;
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_1()
                    .child(widgets::section_title(format!("Tank {rank}")))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(goal_text_short(self.choices.goal)),
                    )
                    .child(div().flex_1())
                    .when(price.0 > 0.0, |d| {
                        d.child(format!(
                            "{}{} ISK",
                            if price.1 { "" } else { "~" },
                            widgets::short(price.0)
                        ))
                    }),
            )
            .child(
                h_flex()
                    .pl_1()
                    .flex_wrap()
                    .gap_x_3()
                    .child(item(Goal::Ehp, "EHP".to_owned()))
                    .child(item(Goal::Resists, format!("{layer} resists")))
                    .child(item(Goal::Repair, format!("{layer} repair")))
                    .when(cap_lost, |d| {
                        d.child(
                            div()
                                .text_color(widgets::color(palette.yellow))
                                .child("Capacitor runs out"),
                        )
                    }),
            )
    }

    /// The racks the change touches under the fitting view's rack labels: what goes on, what
    /// stays and what comes off, with the price of what goes on.
    fn module_list(&self, change: &Change, cx: &App) -> Div {
        let shared = &self.shared;
        let fit = &shared.fit;
        let slot = self.choices.layer.slot();
        let removed = |s: Slot, at: usize| change.removed.contains(&(s, at));
        let placed = |s: Slot| change.placements.iter().filter(move |p| p.0 == s);
        let mut out = v_flex();
        for rack in [slot, Slot::Rig] {
            let fitted: Vec<(TypeId, bool)> = if rack == Slot::Rig {
                fit.rigs
                    .iter()
                    .enumerate()
                    .filter_map(|(at, r)| Some(((*r)?, removed(rack, at))))
                    .collect()
            } else {
                fit.rack(rack)
                    .iter()
                    .enumerate()
                    .filter_map(|(at, m)| Some((m.as_ref()?.type_id, removed(rack, at))))
                    .collect()
            };
            let mut lines: Vec<(TypeId, Mark, usize)> = Vec::new();
            let mut add = |id: TypeId, mark: Mark| match lines
                .iter_mut()
                .find(|l| l.0 == id && l.1 == mark)
            {
                Some(l) => l.2 += 1,
                None => lines.push((id, mark, 1)),
            };
            for p in placed(rack) {
                add(p.2.type_id, Mark::Added);
            }
            for &(id, off) in &fitted {
                add(id, if off { Mark::Removed } else { Mark::Kept });
            }
            // A module that comes off and goes back on stays.
            let mut net: Vec<(TypeId, Mark, usize)> = Vec::new();
            for &(id, mark, n) in &lines {
                let opposite = match mark {
                    Mark::Added => Mark::Removed,
                    Mark::Removed => Mark::Added,
                    Mark::Kept => Mark::Kept,
                };
                let other = if mark == Mark::Kept {
                    0
                } else {
                    lines
                        .iter()
                        .find(|l| l.0 == id && l.1 == opposite)
                        .map_or(0, |l| l.2)
                };
                let same = n.min(other);
                if n > same {
                    net.push((id, mark, n - same));
                }
                if mark == Mark::Added && same > 0 {
                    net.push((id, Mark::Kept, same));
                }
            }
            if net.is_empty() {
                continue;
            }
            let total = u32::try_from(shared.facts.total(rack)).unwrap_or(u32::MAX);
            out = out
                .child(widgets::band(
                    h_flex()
                        .h(px(widgets::ROW_H))
                        .px_1()
                        .gap_1()
                        .mt_2()
                        .child(widgets::rack_band_label(&shared.loaded, rack, total, cx)),
                    cx,
                ))
                .children(
                    net.into_iter()
                        .enumerate()
                        .map(|(k, (id, mark, n))| self.module_line(rack, k, id, mark, n, cx)),
                );
        }
        out
    }

    /// A module the change puts on, keeps or takes off, as a row of the fitting view.
    fn module_line(
        &self,
        rack: Slot,
        k: usize,
        id: TypeId,
        mark: Mark,
        n: usize,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let palette = self.shared.palette;
        let loaded = &self.shared.loaded;
        let price = self.prices.get(id);
        let (icon, color, tag) = match mark {
            Mark::Added => (IconName::Plus, widgets::color(palette.green), "on"),
            Mark::Kept => (IconName::Check, theme.muted_foreground, "kept"),
            Mark::Removed => (IconName::Minus, widgets::color(palette.red), "off"),
        };
        let item_tip = Rc::clone(loaded);
        widgets::list_row(
            SharedString::from(format!("wizard-line-{rack:?}-{k}")),
            false,
            cx,
        )
        .child(
            div()
                .w(px(columns::STATE_W))
                .flex_none()
                .flex()
                .items_center()
                .child(UiIcon::new(icon).size(px(15.)).text_color(color)),
        )
        .child(widgets::icon_box(
            &loaded.catalog,
            &loaded.icons,
            Icon::Type(id),
            22.,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(mark == Mark::Removed, |d| {
                    d.line_through().text_color(theme.muted_foreground)
                })
                .child(if n > 1 {
                    format!("{n}x {}", loaded.catalog.name(id))
                } else {
                    loaded.catalog.name(id)
                }),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(match (mark, price) {
                    (Mark::Added, Some(p)) => widgets::short(p * n as f64),
                    _ => tag.to_owned(),
                }),
        )
        .tooltip(move |window, cx| side::item_tooltip(&item_tip, id, price, window, cx))
        .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let rank = self
            .selected
            .and_then(|s| self.order().iter().position(|&o| o == s));
        let label = match rank {
            Some(r) if self.after.is_some() => format!("Fit tank {}", r + 1),
            _ => "Fit tank".to_owned(),
        };
        h_flex()
            .flex_none()
            .justify_end()
            .gap_2()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .child(
                Button::new("wizard-apply")
                    .small()
                    .primary()
                    .tab_stop(false)
                    .icon(IconName::Check)
                    .label(label)
                    .tooltip(if self.target == Target::Gone {
                        "The fit was deleted"
                    } else if self.after.is_some() {
                        "Enter: fit this tank on the fit, as one undo step"
                    } else {
                        "Find tanks and pick one first"
                    })
                    .disabled(!self.can_apply())
                    .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
            )
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

/// The controls in a column that scrolls, the search pinned at its foot.
fn controls_column(controls: Div, search: Div, width: f32, side: Hsla, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w(px(width))
        .flex_none()
        .h_full()
        .bg(side)
        .border_r_1()
        .border_color(theme.border)
        .child(
            div()
                .id("wizard-controls")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .child(controls),
        )
        .child(
            div()
                .flex_none()
                .p_2()
                .border_t_1()
                .border_color(theme.border)
                .child(search),
        )
}

/// A goal's number in a results column.
fn goal_number(goal: Goal, value: f64) -> String {
    match goal {
        Goal::Ehp => widgets::short(value),
        Goal::Resists => format!("{value:.2}x"),
        Goal::Repair => format!("{value:.0}"),
    }
}

/// A goal's column title, which names its unit.
fn goal_unit(goal: Goal) -> &'static str {
    match goal {
        Goal::Ehp => "EHP",
        Goal::Resists => "Resists",
        Goal::Repair => "EHP/s",
    }
}

/// How a goal's number moves from `now` to `after`, green up and red down.
fn delta(goal: Goal, now: f64, after: f64, palette: Palette, cx: &App) -> (String, Hsla) {
    let change = after - now;
    let margin = 1e-6 * after.abs().max(1.0);
    if change.abs() <= margin {
        return ("same".to_owned(), cx.theme().muted_foreground);
    }
    let text = match goal {
        Goal::Resists => format!("{change:+.2}x"),
        Goal::Ehp => format!(
            "{}{}",
            if change >= 0.0 { "+" } else { "-" },
            widgets::short(change.abs())
        ),
        Goal::Repair => format!("{change:+.0}"),
    };
    let color = if change > 0.0 {
        palette.green
    } else {
        palette.red
    };
    (text, widgets::color(color))
}

/// What a goal aims for, in a sentence.
fn goal_text(goal: Goal) -> &'static str {
    match goal {
        Goal::Ehp => "The most effective HP over all layers, however it gets there.",
        Goal::Resists => {
            "The highest resist multiplier on the layer, so every repair lands hardest; EHP \
             decides between equals."
        }
        Goal::Repair => {
            "The most effective HP repaired a second: the repair the capacitor keeps up, times \
             the resists."
        }
    }
}

fn goal_text_short(goal: Goal) -> &'static str {
    match goal {
        Goal::Ehp => "for max EHP",
        Goal::Resists => "for max resists",
        Goal::Repair => "for max repair",
    }
}

/// What each kind of module is, in the order of [`SearchGroup::ALL`].
const KIND_TIPS: [&str; 8] = [
    "Tech 1 modules",
    "Meta and compact variations of the Tech 1 module",
    "Tech 2 modules",
    "Navy and pirate modules",
    "Storyline modules",
    "Deadspace modules, which get expensive",
    "Officer modules, which get very expensive",
    "Tech 2 rigs; Tech 1 rigs are always tried",
];

/// A slot as a word in a sentence: "mid", "low".
fn slot_word(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "high",
        Slot::Mid => "mid",
        Slot::Low => "low",
        _ => "rig",
    }
}

/// Why the search's better tanks did not make it.
fn limit_text(limit: Limit) -> SharedString {
    let rest = match limit {
        Limit::Cpu | Limit::Powergrid => "better modules did not fit.",
        Limit::Calibration => "better rigs did not fit.",
        Limit::Budget => {
            return "The budget is the limit: better tanks cost more. Raise it to see them.".into();
        }
    };
    format!("{} is the limit: {rest}", limit.label()).into()
}
