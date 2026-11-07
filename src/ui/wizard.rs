//! The fitting wizard: one dialog over the fit with every choice on it, laid out like the
//! preferences window: titled sections of rows, each with its text on the left and its controls
//! on the right. Every module row picks its own variation; the tank section can search for the
//! best tank off the UI thread. A live preview shows what goes where, what it costs and how it
//! tanks. The picking is in [`crate::wizard`].

use std::{rc::Rc, sync::Arc, time::Duration};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity, Window, div,
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
        self, Ammo, Category, Choices, DamageMods, Facts, Family, Goal, Layer, Pick, Prop, Rigs,
        Setup, TankStyle, Tier, Utility,
        tank::{self, Outcome, Progress},
    },
};

const WIDTH: f32 = 1240.;
const PREVIEW_W: f32 = 300.;
const ICON: f32 = 16.;
const VARIANT_W: f32 = 200.;
/// Tanks the optimiser shows.
const RESULTS: usize = 5;

/// Dogma attributes the rows describe.
mod attr {
    pub const CAP_NEED: i32 = 6;
    pub const SPEED_FACTOR: i32 = 20;
    pub const SIG_BLOOM: i32 = 554;
    pub const UPGRADE_COST: i32 = 1153;
}

/// The rows of choices, in keyboard order: the left column, then the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Tier,
    Racks,
    Weapon,
    Ammo,
    Prop,
    Utility(Utility),
    Style,
    Layer,
    Goal,
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
    focus: FocusHandle,
    focused: bool,
    palette: Palette,
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
        let gyfa = cx.entity().downgrade();
        let palette = self.palette;
        let prices = Rc::clone(&self.prices);
        let title = format!("Fitting Wizard: {}", loaded.catalog.name(facts.ship));
        let panel = cx.new(|cx| {
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
                focus: cx.focus_handle(),
                focused: false,
                palette,
            };
            panel.replan();
            panel
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .w(px(WIDTH))
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
            .placements(&self.fit, &picks, self.choices.clear);
        self.stats = wizard::preview(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &placements,
            self.choices.clear,
        );
        self.picks = picks;
    }

    fn make_it_fit(&mut self, cx: &mut Context<Self>) {
        let picks = self.setup().make_it_fit(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &self.picks,
            self.choices.clear,
        );
        self.set_picks(picks);
        self.made_to_fit = true;
        cx.notify();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        let clear = self.choices.clear;
        let placements = self.setup().placements(&self.fit, &self.picks, clear);
        let bling = self.choices.bling.clone();
        window.close_dialog(cx);
        Gyfa::app_update(&self.gyfa, cx, |g, cx| {
            g.saved.wizard = bling;
            g.apply(Action::ApplyWizard { placements, clear }, cx);
        });
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Choices)) {
        let before = self.choices.clone();
        f(&mut self.choices);
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
        let Some(problem) =
            self.setup()
                .tank_problem(&self.loaded.engine, &self.pilot, &self.fit, &self.choices)
        else {
            return;
        };
        if problem.positions.is_empty() && problem.rig_positions.is_empty() {
            self.note = Some("No free slots or rigs are left for the tank.".into());
            cx.notify();
            return;
        }
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
            let outcomes = work.await;
            drop(ticker);
            let _ = this.update(cx, |p, cx| p.finish(generation, outcomes, cx));
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

    fn finish(&mut self, generation: u64, outcomes: Vec<Outcome>, cx: &mut Context<Self>) {
        if generation != self.generation || self.search.is_none() {
            return;
        }
        self.search = None;
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
        self.change(cx, |c| c.tank_fit = Some(found));
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

    fn left_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Tier, Row::Racks, Row::Weapon];
        if self.choices.family.is_some() {
            rows.push(Row::Ammo);
        }
        rows.push(Row::Prop);
        rows.extend(Utility::ALL.into_iter().map(Row::Utility));
        rows
    }

    fn right_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Style];
        if self.choices.tank != TankStyle::None {
            rows.extend([Row::Layer, Row::Goal, Row::Search]);
            if !self.results.is_empty() {
                rows.push(Row::Results);
            }
        }
        rows.extend(self.module_rows().into_iter().map(|(b, _)| Row::Module(b)));
        rows.extend([Row::Damage, Row::Rigs]);
        rows.extend(self.rig_rows().into_iter().map(|(b, _)| Row::Rig(b)));
        rows
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = self.left_rows();
        rows.extend(self.right_rows());
        rows
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
            Row::Racks => (
                vec![("Fill free".into(), true), ("Clear first".into(), true)],
                usize::from(c.clear),
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
            _ => self.change(cx, |c| match row {
                Row::Tier => c.bling.set_tier(Tier::ALL[i]),
                Row::Racks => c.clear = i == 1,
                Row::Weapon => c.family = (i < families).then_some(i),
                Row::Ammo => c.ammo = Ammo::ALL[i],
                Row::Prop => c.prop = Prop::ALL[i],
                Row::Style => c.tank = TankStyle::ALL[i],
                Row::Layer => c.layer = Layer::ALL[i],
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
                    self.change(cx, |c| c.toggle(u));
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
        self.change(cx, |c| c.bling.choose(category, base, id, tier));
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
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.alt {
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
                let selected = i == current;
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
                .w(px(VARIANT_W))
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
            .w(px(VARIANT_W))
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
            Row::Racks => (
                "Racks".into(),
                if c.clear {
                    "Empty the racks and rigs, then fit everything anew.".into()
                } else {
                    "Keep what is fitted and fill the free slots.".into()
                },
            ),
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
                    TankStyle::Brick => "Nothing but plates or extenders.",
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
                    _ => "Effective HP, favouring resists over plates when they are close.",
                }
                .into(),
            ),
            Row::Search => ("Find the best tank".into(), self.search_detail()),
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

    /// The controls on a row's right.
    fn controls(&self, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let pair = |a: AnyElement, b: Option<AnyElement>| {
            h_flex()
                .gap_2()
                .flex_wrap()
                .justify_end()
                .child(a)
                .children(b)
                .into_any_element()
        };
        match row {
            Row::Weapon => pair(self.family_button(cx), self.variant_button(row, cx)),
            Row::Prop | Row::Damage => pair(self.segmented(row, cx), self.variant_button(row, cx)),
            Row::Utility(u) => {
                let usable = self.utility_usable(u);
                let switch = Switch::new(SharedString::from(format!("wizard-{}", row.key())))
                    .checked(self.choices.has(u))
                    .disabled(!usable)
                    .on_click(cx.listener(move |this, _: &bool, window, cx| {
                        this.focus_row(row, cx);
                        this.change(cx, |c| c.toggle(u));
                        this.refocus(window, cx);
                    }))
                    .into_any_element();
                h_flex()
                    .gap_2()
                    .children(self.variant_button(row, cx))
                    .child(switch)
                    .into_any_element()
            }
            Row::Module(_) | Row::Rig(_) => self
                .variant_button(row, cx)
                .unwrap_or_else(|| div().into_any_element()),
            Row::Goal if self.choices.tank == TankStyle::None => div().into_any_element(),
            Row::Search => self.search_controls(cx),
            Row::Results => div().into_any_element(),
            _ => self.segmented(row, cx),
        }
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

    /// The optimiser's tanks, as cards to pick from.
    fn result_cards(&self, cx: &mut Context<Self>) -> AnyElement {
        let cards: Vec<AnyElement> = self
            .results
            .iter()
            .enumerate()
            .map(|(i, outcome)| self.result_card(i, outcome, cx))
            .collect();
        v_flex().gap_1p5().pt_2().children(cards).into_any_element()
    }

    fn result_card(&self, i: usize, outcome: &Outcome, cx: &mut Context<Self>) -> AnyElement {
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
        let modules = h_flex()
            .flex_wrap()
            .gap_x_2()
            .gap_y_0p5()
            .children(lines.into_iter().map(|(id, n)| {
                h_flex()
                    .gap_1()
                    .text_xs()
                    .child(widgets::icon_box(
                        catalog,
                        &self.loaded.icons,
                        Icon::Type(id),
                        ICON,
                    ))
                    .child(format!("{n}x {}", catalog.name(id)))
            }));
        let number = |label: &str, value: String| {
            h_flex()
                .gap_1()
                .text_xs()
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(label.to_owned()),
                )
                .child(div().font_weight(FontWeight::MEDIUM).child(value))
        };
        let (cap_icon, cap_color, cap_text) = if outcome.cap_stable {
            (AssetIcon::BatteryFull, self.palette.green, "Cap stable")
        } else {
            (AssetIcon::BatteryLow, self.palette.yellow, "Cap runs out")
        };
        let numbers = h_flex()
            .flex_wrap()
            .gap_3()
            .child(number("EHP", widgets::short(outcome.ehp)))
            .child(number("Resist", format!("{:.1}%", outcome.resist * 100.0)))
            .when(outcome.repair > 0.0, |d| {
                d.child(number("Repair", format!("{:.0} HP/s", outcome.repair)))
            })
            .child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(widgets::color(cap_color))
                    .child(UiIcon::new(cap_icon).xsmall())
                    .child(cap_text),
            );
        v_flex()
            .id(SharedString::from(format!("wizard-result-{i}")))
            .gap_1()
            .p_2()
            .rounded(px(6.))
            .border_1()
            .border_color(if selected {
                theme.primary
            } else {
                theme.border
            })
            .bg(if selected {
                theme.primary.opacity(0.08)
            } else {
                theme.background
            })
            .cursor_pointer()
            .hover(|s| s.border_color(theme.primary.opacity(0.6)))
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("#{}", i + 1)),
                    )
                    .child(numbers),
            )
            .child(modules)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.focus_row(Row::Results, cx);
                this.select(i, cx);
                this.refocus(window, cx);
            }))
            .into_any_element()
    }

    /// One row: its text on the left, its controls on the right (or under the text for wide
    /// ones), marked while the keyboard is on it.
    fn row_element(&self, row: Row, cursor: Option<Row>, cx: &mut Context<Self>) -> AnyElement {
        let (title, description) = self.text(row);
        let disabled = match row {
            Row::Utility(u) => !self.utility_usable(u),
            _ => false,
        };
        let controls = self.controls(row, cx);
        let results = (row == Row::Results).then(|| self.result_cards(cx));
        let theme = cx.theme();
        let active = cursor == Some(row);
        let stacked = matches!(row, Row::Tier | Row::Weapon)
            || row == Row::Goal && self.choices.tank != TankStyle::None;
        let text = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .when(disabled, |d| d.opacity(0.5))
            .child(div().font_weight(FontWeight::MEDIUM).child(title))
            .when(!description.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(description),
                )
            });
        let body = if stacked {
            v_flex()
                .gap_2()
                .child(text)
                .child(h_flex().child(controls))
                .into_any_element()
        } else {
            h_flex()
                .gap_4()
                .child(text)
                .child(div().flex_none().child(controls))
                .into_any_element()
        };
        v_flex()
            .id(SharedString::from(format!("wizard-row-{}", row.key())))
            .px_4()
            .py_2()
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
            .child(body)
            .children(results)
            .into_any_element()
    }

    fn sections(&self, rows: &[Row], cursor: Option<Row>, cx: &mut Context<Self>) -> Div {
        let mut groups: Vec<(&'static str, Vec<Row>)> = Vec::new();
        for &row in rows {
            let title = section_of(row);
            match groups.last_mut() {
                Some((t, rows)) if *t == title => rows.push(row),
                _ => groups.push((title, vec![row])),
            }
        }
        let mut column = v_flex().flex_1().min_w_0().gap_4();
        for (title, rows) in groups {
            let elements: Vec<AnyElement> = rows
                .into_iter()
                .map(|r| self.row_element(r, cursor, cx))
                .collect();
            column = column.child(section(title, elements, cx));
        }
        column
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
            let kept = if self.choices.clear {
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
            .gap_3()
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
            .child(caption("Tank", cx))
            .child(self.tank_summary(cx))
            .child(caption("Damage", cx))
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
        Row::Tier | Row::Racks => "General",
        Row::Weapon | Row::Ammo => "Weapons",
        Row::Prop => "Propulsion",
        Row::Utility(_) => "Tackle & utility",
        Row::Style | Row::Layer | Row::Goal | Row::Search | Row::Results | Row::Module(_) => "Tank",
        Row::Damage | Row::Rigs | Row::Rig(_) => "Damage & rigs",
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

/// A small uppercase caption over a group of rows.
fn caption(title: &str, cx: &App) -> Div {
    div()
        .px_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.to_uppercase())
}

/// A titled card of rows, divided by hairlines.
fn section(title: &'static str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut card = v_flex()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.sidebar)
        .overflow_hidden();
    for (n, row) in rows.into_iter().enumerate() {
        if n > 0 {
            card = card.child(div().h_px().mx_4().bg(theme.border));
        }
        card = card.child(row);
    }
    v_flex()
        .gap_2()
        .child(caption(title, cx))
        .child(card)
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
        let left = self.sections(&self.left_rows(), cursor, cx);
        let right = self.sections(&self.right_rows(), cursor, cx);
        let count = self.picks.len();
        let preview = self.preview(cx);
        let height = (window.viewport_size().height * 0.72).max(px(360.));
        let theme = cx.theme();
        let hint = div().text_xs().text_color(theme.muted_foreground).child(
            "Up/Down: row, Left/Right or 1-9: choose, [ ]: variant, Space: toggle, F: find the \
             best tank, Enter: apply",
        );
        let footer = h_flex().justify_between().gap_2().child(hint).child(
            h_flex()
                .gap_2()
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
                ),
        );
        v_flex()
            .track_focus(&self.focus)
            .key_context("FittingWizard")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.key(event, window, cx);
            }))
            .gap_3()
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .child(
                        h_flex()
                            .id("wizard-choices")
                            .flex_1()
                            .min_w_0()
                            .max_h(height)
                            .overflow_y_scroll()
                            .items_start()
                            .gap_4()
                            .pr_1()
                            .child(left)
                            .child(right),
                    )
                    .child(
                        v_flex()
                            .id("wizard-preview")
                            .w(px(PREVIEW_W))
                            .flex_none()
                            .max_h(height)
                            .overflow_y_scroll()
                            .p_3()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.sidebar)
                            .child(preview),
                    ),
            )
            .child(div().h_px().bg(theme.border))
            .child(footer)
    }
}
