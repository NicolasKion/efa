//! pyfa's graph window: a graph picked from the list, its axes and inputs, the fits to plot and
//! the targets to plot them against, drawn with a legend and a readout at the marked X.
//!
//! It is a window of its own reading fits from the [`Hub`]; any change to the fits there
//! redraws it.

mod canvas;
mod lists;
mod panel;
mod style;
mod vector;
mod view;

use std::{cell::Cell, collections::HashMap, rc::Rc};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, FocusHandle, Global, Pixels, Point,
    SharedString, Subscription, WeakEntity, Window, WindowBounds, WindowOptions, px, size,
};

use self::{
    canvas::{Frame, PlotLine},
    lists::{AmmoChoice, Source, Target, TargetKind},
    style::LineStyle,
    vector::{Vector, Which},
};
use super::{
    Loaded,
    hub::{Hub, HubLoading},
};
use crate::{
    fit::Fit,
    graphs::{
        ammo::{self, AmmoQuality, AmmoStyle, WeaponGroup},
        defs::{Checkbox as CheckboxKey, Graph, InputDef, Param, Spec, XDef, YDef, conditions_met},
        plot::{self, Options, Request, TargetData},
        profiles::TargetProfile,
        settings::GraphSettings,
        snapshot::FitSnapshot,
    },
};

/// Something read from a fit, with the fit it was read from to tell when it is stale.
type Cached<T> = (String, Rc<T>);

/// The open graph window, so a second "Graphs" only brings it forward, as in pyfa.
#[derive(Default)]
struct OpenGraphs(Option<(AnyWindowHandle, WeakEntity<GraphsWindow>)>);

impl Global for OpenGraphs {}

/// Open the graph window, or bring it forward, with `fit` in its fit list when opened fresh.
/// `hidden` also lists the graphs pyfa keeps behind Ctrl+Alt+G.
pub fn open(hub: Entity<Hub>, fit: Option<usize>, hidden: bool, cx: &mut App) {
    if let Some((handle, view)) = cx.try_global::<OpenGraphs>().and_then(|g| g.0.clone())
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        if hidden {
            let _ = view.update(cx, |g, cx| {
                g.include_hidden = true;
                cx.notify();
            });
        }
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1180.), px(900.)), cx)),
        app_id: Some("gyfa".into()),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        window.set_window_title("gyfa: Graphs");
        cx.new(|cx| GraphsWindow::new(hub, fit, hidden, window, cx))
    });
    match opened {
        Ok((handle, view)) => cx.set_global(OpenGraphs(Some((handle, view.downgrade())))),
        Err(e) => eprintln!("opening the graph window: {e}"),
    }
}

impl super::Gyfa {
    /// pyfa's Fit > Graphs (Ctrl+G), starting with the current fit. The new window only reads the
    /// hub, never this window, so opening it from inside this window's update is safe.
    pub(super) fn open_graphs(&mut self, cx: &mut App) {
        open(self.hub.clone(), self.saved.current, false, cx);
    }

    /// pyfa's Ctrl+Alt+G: the graph window with its hidden graphs listed too.
    pub(super) fn open_hidden_graphs(&mut self, cx: &mut App) {
        open(self.hub.clone(), self.saved.current, true, cx);
    }
}

/// A misc input field as currently laid out.
struct Field {
    def: InputDef,
    state: Entity<InputState>,
}

pub struct GraphsWindow {
    hub: Entity<Hub>,
    loaded: Option<Rc<Loaded>>,
    fits: Vec<Fit>,
    /// The main window's tabs, for "Add Currently Open Fit".
    open_tabs: Vec<usize>,
    synced: u64,
    /// The characters the snapshots were flown with; a new list means they are out of date.
    characters: Option<Rc<crate::character::Characters>>,
    /// Snapshots by fit and ammo override, with the fit they were read from, so only fits that
    /// changed are read again.
    snapshots: HashMap<(usize, Option<AmmoChoice>), Cached<FitSnapshot>>,
    /// The Application Profile graph's weapon groups with every charge loaded, likewise.
    ammo_groups: HashMap<(usize, AmmoQuality), Cached<Vec<WeaponGroup>>>,
    graph: Graph,
    x: usize,
    y: usize,
    /// The remembered options, saved with the fits.
    settings: GraphSettings,
    /// Shield graphs count EHP unless the main window's stats pane shows raw HP.
    effective_hp: bool,
    /// List the hidden ECM burst graph too, as pyfa's Ctrl+Alt+G does.
    include_hidden: bool,
    checkboxes: HashMap<CheckboxKey, bool>,
    main_input: Option<Field>,
    fields: Vec<Field>,
    /// Field texts by quantity, kept while switching axes like pyfa does.
    stored: HashMap<Spec, String>,
    atk_vector: Vector,
    tgt_vector: Vector,
    pub(super) drag_vector: Option<Which>,
    sources: Vec<Source>,
    targets: Vec<Target>,
    selected_source: Option<usize>,
    selected_target: Option<usize>,
    /// The lists take Delete for removing their selected row, as in pyfa.
    sources_focus: FocusHandle,
    targets_focus: FocusHandle,
    lines: Vec<PlotLine>,
    frame: Frame,
    /// Where the pointer is over the plot.
    pub(super) hover_mark: Option<f64>,
    /// Where the plot was last clicked; stays when the pointer leaves.
    pub(super) pinned_mark: Option<f64>,
    plot_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    status: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
    field_subscriptions: Vec<Subscription>,
}

impl GraphsWindow {
    fn new(
        hub: Entity<Hub>,
        fit: Option<usize>,
        include_hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&hub, |this, _, cx| this.pull(cx)),
            cx.on_release(|_, cx| cx.set_global(OpenGraphs(None))),
        ];
        let (settings, effective_hp) = {
            let hub = hub.read(cx);
            (hub.saved.graphs, !hub.raw_hp)
        };
        let mut this = Self {
            hub,
            loaded: None,
            fits: Vec::new(),
            open_tabs: Vec::new(),
            synced: u64::MAX,
            characters: None,
            snapshots: HashMap::new(),
            ammo_groups: HashMap::new(),
            graph: Graph::DamageStats,
            x: 0,
            y: 0,
            settings,
            effective_hp,
            include_hidden,
            checkboxes: HashMap::new(),
            main_input: None,
            fields: Vec::new(),
            stored: HashMap::new(),
            atk_vector: Vector::new(1.0, 0.0),
            tgt_vector: Vector::new(1.0, 90.0),
            drag_vector: None,
            sources: Vec::new(),
            targets: vec![Target::new(
                TargetKind::Profile(TargetProfile::ideal()),
                &[],
            )],
            selected_source: None,
            selected_target: None,
            sources_focus: cx.focus_handle(),
            targets_focus: cx.focus_handle(),
            lines: Vec::new(),
            frame: Frame::default(),
            hover_mark: None,
            pinned_mark: None,
            plot_bounds: Rc::default(),
            status: None,
            _subscriptions: subscriptions,
            field_subscriptions: Vec::new(),
        };
        this.pull(cx);
        if let Some(fit) = fit.filter(|&i| i < this.fits.len()) {
            this.add_source(fit, None);
        }
        this.layout_inputs(window, cx);
        this.recompute(cx);
        this
    }

    /// Take in new data and fits from the hub; fits deleted elsewhere leave the lists.
    fn pull(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.read(cx);
        let mut changed = false;
        if self.loaded.is_none()
            && let HubLoading::Ready(loaded) = &hub.loading
        {
            self.loaded = Some(Rc::clone(loaded));
            changed = true;
        }
        if self
            .characters
            .as_ref()
            .is_none_or(|c| !Rc::ptr_eq(c, &hub.characters))
        {
            self.characters = Some(Rc::clone(&hub.characters));
            self.snapshots.clear();
            self.ammo_groups.clear();
            changed = true;
        }
        if self.effective_hp == hub.raw_hp {
            self.effective_hp = !hub.raw_hp;
            changed = true;
        }
        if hub.generation == self.synced {
            if changed {
                self.recompute(cx);
                cx.notify();
            }
            return;
        }
        let deleted: Vec<usize> = hub
            .deletions
            .iter()
            .filter(|(generation, _)| self.synced != u64::MAX && *generation > self.synced)
            .map(|&(_, index)| index)
            .collect();
        self.fits = hub.saved.fits.clone();
        // A user profile edited or deleted in the profile editor follows here, as in pyfa.
        let user = &hub.saved.target_profiles;
        self.targets.retain_mut(|t| match &mut t.kind {
            TargetKind::Profile(p) if p.user => match user.iter().find(|u| u.name == p.name) {
                Some(u) => {
                    *p = TargetProfile::from(u);
                    true
                }
                None => false,
            },
            _ => true,
        });
        self.open_tabs = hub.saved.open.clone();
        self.synced = hub.generation;
        for index in deleted {
            self.forget_fit(index);
        }
        let count = self.fits.len();
        self.sources.retain(|s| s.fit < count);
        self.targets
            .retain(|t| !matches!(t.kind, TargetKind::Fit(i) if i >= count));
        // Snapshots of fits that changed are read again when next asked for.
        self.recompute(cx);
        cx.notify();
    }

    /// A fit was deleted: drop its entries and shift later indices down.
    fn forget_fit(&mut self, index: usize) {
        self.sources.retain(|s| s.fit != index);
        self.targets
            .retain(|t| !matches!(t.kind, TargetKind::Fit(i) if i == index));
        let shift = |i: &mut usize| {
            if *i > index {
                *i -= 1;
            }
        };
        self.sources.iter_mut().for_each(|s| shift(&mut s.fit));
        for t in &mut self.targets {
            if let TargetKind::Fit(i) = &mut t.kind {
                shift(i);
            }
        }
    }

    pub(super) fn snapshot(&mut self, fit: usize) -> Option<Rc<FitSnapshot>> {
        self.snapshot_with(fit, None)
    }

    /// The fit with `ammo` loaded into every module of its type, as "Plot with Different Ammo"
    /// asks for.
    fn fit_with(&self, fit: usize, ammo: Option<AmmoChoice>) -> Option<Fit> {
        let mut f = self.fits.get(fit)?.clone();
        if let Some(choice) = ammo {
            for m in [&mut f.high, &mut f.mid, &mut f.low]
                .into_iter()
                .flatten()
                .flatten()
                .filter(|m| m.type_id == choice.module)
            {
                m.charge = Some(choice.charge);
            }
        }
        Some(f)
    }

    fn snapshot_with(&mut self, fit: usize, ammo: Option<AmmoChoice>) -> Option<Rc<FitSnapshot>> {
        let loaded = self.loaded.clone()?;
        let f = self.fit_with(fit, ammo)?;
        let key = serde_json::to_string(&f).unwrap_or_default();
        if let Some((cached, snap)) = self.snapshots.get(&(fit, ammo))
            && *cached == key
        {
            return Some(Rc::clone(snap));
        }
        let pilot = self.pilot(&f, &loaded);
        let snap = Rc::new(loaded.engine.graph_snapshot(&f, &pilot, &loaded.catalog));
        self.snapshots.insert((fit, ammo), (key, Rc::clone(&snap)));
        Some(snap)
    }

    /// Each fit flies with its own character, as in the main window.
    fn pilot(&self, f: &Fit, loaded: &Loaded) -> crate::engine::Pilot {
        self.characters.as_ref().map_or_else(
            || loaded.engine.all5().clone(),
            |c| c.pilot(f.character, &loaded.catalog.skills),
        )
    }

    fn ammo_groups(&mut self, fit: usize) -> Option<Rc<Vec<WeaponGroup>>> {
        let loaded = self.loaded.clone()?;
        let quality = self.settings.ammo_quality;
        let f = self.fits.get(fit)?.clone();
        let key = serde_json::to_string(&f).unwrap_or_default();
        if let Some((cached, groups)) = self.ammo_groups.get(&(fit, quality))
            && *cached == key
        {
            return Some(Rc::clone(groups));
        }
        let pilot = self.pilot(&f, &loaded);
        let groups = Rc::new(
            loaded
                .engine
                .graph_ammo(&f, &pilot, &loaded.catalog, quality),
        );
        self.ammo_groups
            .insert((fit, quality), (key, Rc::clone(&groups)));
        Some(groups)
    }

    fn options(&self) -> Options {
        self.settings.options(self.graph, self.effective_hp)
    }

    /// Recompute with changed settings and remember them.
    pub(super) fn settings_changed(&mut self, cx: &mut Context<Self>) {
        self.recompute(cx);
        cx.notify();
        let settings = self.settings;
        self.hub
            .update(cx, |hub, cx| hub.update_saved(|s| s.graphs = settings, cx));
    }

    fn x_defs(&self) -> Vec<XDef> {
        self.graph.x_defs(self.effective_hp)
    }

    fn y_defs(&self) -> Vec<YDef> {
        let options = self.options();
        self.graph.y_defs(options.ignore_resists, self.effective_hp)
    }

    fn x_def(&self) -> XDef {
        let defs = self.x_defs();
        defs[self.x.min(defs.len() - 1)]
    }

    fn y_def(&self) -> YDef {
        let defs = self.y_defs();
        defs[self.y.min(defs.len() - 1)]
    }

    /// Lay out the input fields for the current graph and axes, keeping typed values.
    fn layout_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store_fields(cx);
        let (x, y) = (self.x_def(), self.y_def());
        let (x_spec, y_spec) = ((x.param, x.unit), (y.param, y.unit));
        let inputs = self.graph.inputs();
        let main_def = inputs
            .iter()
            .copied()
            .find(|i| (i.param, i.unit) == x.main_input);
        let mut handled = vec![x.main_input.0];
        // Vectors take the place of their speed and heading fields.
        for v in [self.graph.src_vector(), self.graph.tgt_vector()]
            .into_iter()
            .flatten()
        {
            handled.extend([v.length, v.angle]);
        }
        self.tgt_vector.direction_only = x.main_input.0 == Param::TgtSpeed;
        self.atk_vector.direction_only = x.main_input.0 == Param::AtkSpeed;

        let mut subscriptions = Vec::new();
        let mut make = |def: InputDef,
                        main: bool,
                        stored: &HashMap<Spec, String>,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            let text = stored
                .get(&(def.param, def.unit))
                .cloned()
                .unwrap_or_else(|| {
                    if main {
                        format!(
                            "{}-{}",
                            canvas::format(def.default_range.0),
                            canvas::format(def.default_range.1)
                        )
                    } else {
                        def.default.map_or_else(String::new, canvas::format)
                    }
                });
            let state = cx.new(|cx| InputState::new(window, cx).default_value(text));
            subscriptions.push(cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.recompute(cx);
                    cx.notify();
                }
            }));
            Field { def, state }
        };
        self.main_input = main_def.map(|def| make(def, true, &self.stored, window, cx));
        let mut fields = Vec::new();
        for def in inputs {
            if handled.contains(&def.param) || !conditions_met(def.conditions, x_spec, y_spec) {
                continue;
            }
            handled.push(def.param);
            fields.push(make(def, false, &self.stored, window, cx));
        }
        self.fields = fields;
        self.field_subscriptions = subscriptions;
    }

    fn store_fields(&mut self, cx: &App) {
        for field in self.main_input.iter().chain(&self.fields) {
            self.stored.insert(
                (field.def.param, field.def.unit),
                field.state.read(cx).value().to_string(),
            );
        }
    }

    fn set_graph(&mut self, graph: Graph, window: &mut Window, cx: &mut Context<Self>) {
        if graph == self.graph {
            return;
        }
        self.graph = graph;
        self.x = 0;
        self.y = 0;
        // pyfa starts every graph from its defaults.
        self.stored.clear();
        self.main_input = None;
        self.fields.clear();
        self.checkboxes.clear();
        self.atk_vector = Vector::new(1.0, 0.0);
        self.tgt_vector = Vector::new(1.0, 90.0);
        self.pinned_mark = None;
        self.layout_inputs(window, cx);
        self.recompute(cx);
        cx.notify();
    }

    fn set_axes(&mut self, x: usize, y: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.x = x;
        self.y = y;
        self.pinned_mark = None;
        self.layout_inputs(window, cx);
        self.recompute(cx);
        cx.notify();
    }

    pub(super) fn set_vector(
        &mut self,
        which: Which,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let vector = match which {
            Which::Attacker => &mut self.atk_vector,
            Which::Target => &mut self.tgt_vector,
        };
        if vector.set_from(position) {
            self.recompute(cx);
            cx.notify();
        }
    }

    pub(super) fn hover_at(&mut self, position: Point<Pixels>, pin: bool, cx: &mut Context<Self>) {
        let Some(x) = canvas::data_x(self.plot_bounds.get(), position, self.frame.x_range) else {
            return;
        };
        let x = canvas::clamp_mark(x, self.frame.x_range);
        self.hover_mark = Some(x);
        if pin {
            self.pinned_mark = Some(x);
        }
        cx.notify();
    }

    fn checkbox(&self, key: CheckboxKey) -> bool {
        self.checkboxes.get(&key).copied().unwrap_or_else(|| {
            self.graph
                .checkboxes()
                .iter()
                .find(|c| c.key == key)
                .is_none_or(|c| c.default)
        })
    }

    /// The values typed into the fields, the vectors, and the main range.
    fn request(&self, cx: &App) -> Request {
        let (x, y) = (self.x_def(), self.y_def());
        let main = self.main_input.as_ref();
        let range = main
            .and_then(|f| parse_range(&f.state.read(cx).value()))
            .or_else(|| main.map(|f| f.def.default_range))
            .unwrap_or((0.0, 100.0));
        let mut misc: HashMap<Param, Option<f64>> = self
            .fields
            .iter()
            .map(|f| (f.def.param, parse_number(&f.state.read(cx).value())))
            .collect();
        if self.graph.src_vector().is_some() {
            misc.insert(Param::AtkSpeed, Some(self.atk_vector.length * 100.0));
            misc.insert(Param::AtkAngle, Some(self.atk_vector.angle));
        }
        if self.graph.tgt_vector().is_some() {
            if x.main_input.0 != Param::TgtSpeed {
                misc.insert(Param::TgtSpeed, Some(self.tgt_vector.length * 100.0));
            }
            misc.insert(Param::TgtAngle, Some(self.tgt_vector.angle));
        }
        let checkboxes = self
            .graph
            .checkboxes()
            .iter()
            .map(|c| (c.key, self.checkbox(c.key)))
            .collect();
        Request {
            graph: self.graph,
            x,
            y,
            range,
            misc,
            checkboxes,
            options: self.options(),
        }
    }

    /// Recalculate every line. Snapshots are cached per fit, so this only samples.
    fn recompute(&mut self, cx: &App) {
        self.lines.clear();
        if self.loaded.is_none() {
            return;
        }
        let req = self.request(cx);
        self.plot_lines(&req);
    }

    fn plot_lines(&mut self, req: &Request) {
        let sources: Vec<Source> = self.sources.clone();
        let targets: Vec<Target> = self.targets.clone();
        let mut lines = Vec::new();
        let mut ammo_names: Vec<String> = Vec::new();
        for source in sources {
            let (fit, color) = (source.fit, source.color);
            let Some(src) = self.snapshot_with(fit, source.ammo) else {
                continue;
            };
            let name = self.source_name(&source);
            if self.graph.has_segments() {
                let Some(groups) = self.ammo_groups(fit) else {
                    continue;
                };
                for target in &targets {
                    let Some(data) = self.target_data(target) else {
                        continue;
                    };
                    let label = format!("{name} vs {}", self.target_name(target));
                    let segments = ammo::segments(req, &src, &groups, &data);
                    let style = self.settings.ammo_style;
                    for (n, segment) in segments.into_iter().enumerate() {
                        let index = ammo_names
                            .iter()
                            .position(|a| *a == segment.ammo)
                            .unwrap_or_else(|| {
                                ammo_names.push(segment.ammo.clone());
                                ammo_names.len() - 1
                            });
                        lines.push(PlotLine {
                            label: match style {
                                AmmoStyle::None => label.clone(),
                                _ => segment.ammo.clone(),
                            },
                            color,
                            lightness: target.lightness,
                            style: match style {
                                AmmoStyle::Pattern => LineStyle::ALL[index % LineStyle::ALL.len()],
                                _ => target.style,
                            },
                            hue: (style == AmmoStyle::Color).then(|| canvas::ammo_hue(index)),
                            // One legend entry per charge, or one per line without ammo styling.
                            in_legend: match style {
                                AmmoStyle::None => n == 0,
                                _ => !lines.iter().any(|l: &PlotLine| l.label == segment.ammo),
                            },
                            series: plot::Series {
                                xs: segment.xs,
                                ys: segment.ys,
                            },
                        });
                    }
                }
            } else if self.graph.has_targets() {
                for target in &targets {
                    let Some(data) = self.target_data(target) else {
                        continue;
                    };
                    if let Some(series) = plot::plot(req, &src, Some(&data)) {
                        lines.push(PlotLine {
                            label: format!("{name} vs {}", self.target_name(target)),
                            color,
                            lightness: target.lightness,
                            style: target.style,
                            hue: None,
                            in_legend: true,
                            series,
                        });
                    }
                }
            } else if let Some(series) = plot::plot(req, &src, None) {
                lines.push(PlotLine {
                    label: name,
                    color,
                    lightness: style::Lightness::Normal,
                    style: LineStyle::Solid,
                    hue: None,
                    in_legend: true,
                    series,
                });
            }
        }
        // pyfa just draws nothing in these cases; say why instead.
        self.status = if self.sources.is_empty() {
            Some("Add a fit with the Add button over the fit list, or right-click it.".into())
        } else if self.graph.has_targets() && self.targets.is_empty() {
            Some("Add a target with the Add button over the target list.".into())
        } else if self.graph.has_segments() && lines.is_empty() {
            Some("No turrets or launchers with charges to compare.".into())
        } else if req.y.param == Param::Damage
            && req.x.param != Param::Time
            && req.misc.get(&Param::Time).copied().flatten().is_none()
        {
            Some("Damage inflicted needs a time.".into())
        } else {
            None
        };
        self.lines = lines;
        self.frame = self.frame_for(req);
    }

    fn frame_for(&self, req: &Request) -> Frame {
        let x_main = req.x.main_input.1 == req.x.unit;
        let mut xs: Vec<f64> = self
            .lines
            .iter()
            .flat_map(|l| l.series.xs.iter().copied())
            .collect();
        if x_main || xs.is_empty() {
            xs.extend([req.range.0, req.range.1]);
        }
        let mut ys: Vec<f64> = self
            .lines
            .iter()
            .flat_map(|l| l.series.ys.iter().copied())
            .collect();
        if self.settings.show_y0 {
            ys.push(0.0);
        }
        Frame {
            x_label: req.x.label.to_owned(),
            y_label: req.y.label.to_owned(),
            x_unit: req.x.unit.label(),
            y_unit: req.y.unit.label(),
            x_range: canvas::limits(xs.into_iter(), (0.02, 0.02), false),
            y_range: canvas::limits(ys.into_iter(), (0.05, 0.03), true),
            legend: self.settings.show_legend,
        }
    }

    pub(super) fn fit_name(&self, fit: usize) -> String {
        let Some(f) = self.fits.get(fit) else {
            return String::new();
        };
        match (f.ship, &self.loaded) {
            (Some(ship), Some(loaded)) => format!("{} ({})", f.name, loaded.catalog.name(ship)),
            _ => f.name.clone(),
        }
    }

    /// A fit's name, with the charge it is plotted with when that was picked.
    pub(super) fn source_name(&self, source: &Source) -> String {
        let name = self.fit_name(source.fit);
        match (source.ammo, &self.loaded) {
            (Some(choice), Some(loaded)) => {
                format!("{name} [{}]", loaded.catalog.name(choice.charge))
            }
            _ => name,
        }
    }

    pub(super) fn target_name(&self, target: &Target) -> String {
        match &target.kind {
            TargetKind::Fit(i) => self.fit_name(*i),
            TargetKind::Profile(p) => p.short_name().to_owned(),
        }
    }

    pub(super) fn target_data(&mut self, target: &Target) -> Option<TargetData> {
        Some(match &target.kind {
            TargetKind::Fit(i) => {
                let snap = self.snapshot(*i)?;
                let ship = &snap.ship;
                TargetData {
                    max_velocity: ship.max_velocity,
                    sig_radius: ship.sig_radius,
                    radius: ship.radius,
                    resists: target.resist_mode.resists(ship),
                    hp: Some(ship.hp.iter().sum()),
                    breacher_resist: ship.breacher_resist,
                    scrammed: ship.scrammed,
                }
            }
            TargetKind::Profile(p) => TargetData::profile(
                p.max_velocity,
                p.sig_radius.unwrap_or(f64::INFINITY),
                p.radius,
                p.resists,
                p.hp,
            ),
        })
    }

    pub(super) fn add_source(&mut self, fit: usize, ammo: Option<AmmoChoice>) {
        if self.sources.iter().any(|s| s.fit == fit && s.ammo == ammo) {
            return;
        }
        let color = style::LineColor::least_used(self.sources.iter().map(|s| s.color));
        self.sources.push(Source { fit, color, ammo });
    }

    pub(super) fn add_target(&mut self, kind: TargetKind) {
        let exists = self.targets.iter().any(|t| match (&t.kind, &kind) {
            (TargetKind::Fit(a), TargetKind::Fit(b)) => a == b,
            (TargetKind::Profile(a), TargetKind::Profile(b)) => a.name == b.name,
            _ => false,
        });
        if !exists {
            let target = Target::new(kind, &self.targets);
            self.targets.push(target);
        }
    }

    pub(super) fn remove_target(&mut self, index: usize) {
        if index < self.targets.len() {
            self.targets.remove(index);
        }
        // pyfa resets the last target left to the plain style.
        if let [only] = self.targets.as_mut_slice() {
            only.lightness = style::Lightness::Normal;
            only.style = LineStyle::Solid;
        }
        self.selected_target = None;
    }

    /// Ask the main window to open a fit as a tab, pyfa's "Open Fit in New Tab".
    pub(super) fn open_in_main(hub: &Entity<Hub>, fit: usize, cx: &mut App) {
        hub.update(cx, |hub, cx| {
            hub.open_requests.push(fit);
            cx.notify();
        });
    }
}

/// pyfa's range field: "0-100", "10 - 50", or one number for both ends.
fn parse_range(text: &str) -> Option<(f64, f64)> {
    let text = text.trim();
    // A dash after the first character separates; a leading one is a minus sign.
    let split = text
        .char_indices()
        .skip(1)
        .find(|&(i, c)| c == '-' && !text[..i].trim_end().ends_with(['e', 'E', '-']))
        .map(|(i, _)| i);
    let (a, b) = if let Some(i) = split {
        (parse_number(&text[..i])?, parse_number(&text[i + 1..])?)
    } else {
        let v = parse_number(text)?;
        (v, v)
    };
    Some((a.min(b), a.max(b)))
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_fields_parse_like_pyfa() {
        assert_eq!(parse_range("0-100"), Some((0.0, 100.0)));
        assert_eq!(parse_range(" 10 - 50 "), Some((10.0, 50.0)));
        assert_eq!(parse_range("-5-5"), Some((-5.0, 5.0)));
        assert_eq!(parse_range("80"), Some((80.0, 80.0)));
        assert_eq!(parse_range("50-0"), Some((0.0, 50.0)));
        assert_eq!(parse_range("abc"), None);
        assert_eq!(parse_number("0,5"), Some(0.5));
    }
}
