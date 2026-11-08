//! The fitting wizard: a window of its own over one fit, with its sections, one per concern of
//! the fit, as tabs in its header. Each section works on the fit as it was when the wizard
//! opened, leaves everything outside its concern alone, and hands its change to the main window
//! through the hub, which makes it as one undo step. A section is its own entity in its own
//! module, so another one slots in beside the tank without touching it. The logic is in
//! [`crate::wizard`].

mod tank;

use std::{rc::Rc, time::Duration};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, Context, Div, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{Efa, Loaded, hub::Hub, item_stats, widgets};
use crate::{
    engine::Pilot,
    fit::Fit,
    icons::Icon,
    item_data::ItemData,
    theme::{Palette, Rgb},
    wizard::{self, Change, Facts, Section},
};

/// From this window width a section lays its parts out in three columns.
const WIDE_W: f32 = 1000.;
/// From this width in two.
const SPLIT_W: f32 = 700.;

/// How a section fits the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Controls, results and the picked result side by side.
    Wide,
    /// Controls beside the results, the picked result under them.
    Split,
    /// One column.
    Narrow,
}

impl Layout {
    fn of(window: &Window) -> Self {
        let width = window.viewport_size().width;
        if width >= px(WIDE_W) {
            Self::Wide
        } else if width >= px(SPLIT_W) {
            Self::Split
        } else {
            Self::Narrow
        }
    }
}

/// What every section works from: the fit as it was when the wizard opened, and the hull.
struct Shared {
    hub: Entity<Hub>,
    /// The window the wizard was opened from, which fetches prices for it.
    efa: WeakEntity<Efa>,
    loaded: Rc<Loaded>,
    data: Rc<ItemData>,
    pilot: Rc<Pilot>,
    /// The fit's identity, which outlives its place in the list.
    uid: u64,
    fit: Fit,
    /// The fit as saved, to tell when it changes.
    saved: String,
    facts: Facts,
    palette: Palette,
}

impl Shared {
    fn setup(&self) -> wizard::Setup<'_> {
        wizard::Setup::new(&self.loaded.catalog, &self.data, &self.facts)
    }

    /// Hand a section's change to the main window, which makes it as one undo step, and close
    /// the wizard.
    fn apply(&self, change: Change, window: &mut Window, cx: &mut App) {
        let edit = super::hub::WizardEdit {
            uid: self.uid,
            change,
        };
        self.hub.update(cx, |hub, cx| {
            hub.inbox.wizard_edits.push(edit);
            cx.notify();
        });
        window.remove_window();
    }

    /// Remember settings for the next time, through the hub.
    fn remember(&self, cx: &mut App, edit: impl FnOnce(&mut crate::store::Saved) + 'static) {
        let hub = self.hub.clone();
        // The hub may be the one being updated when this runs.
        cx.defer(move |cx| hub.update(cx, |hub, cx| hub.update_saved(edit, cx)));
    }
}

/// How the fit stands against the copy the wizard works on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Same,
    /// Edited since: the results go by the old fit.
    Changed,
    /// No longer open in the main window.
    Closed,
    /// Deleted: nothing to apply to.
    Gone,
}

/// A number a section shows in the header: what the fit has now.
struct Stat {
    label: SharedString,
    value: SharedString,
    tip: SharedString,
}

/// A section's entity.
enum Panel {
    Tank(Entity<tank::TankSection>),
}

impl Panel {
    fn new(section: Section, shared: &Rc<Shared>, window: &mut Window, cx: &mut App) -> Self {
        match section {
            Section::Tank => {
                let hub = shared.hub.read(cx);
                let mut choices = hub.saved.settings.wizard.tank;
                choices.layer = shared.facts.layer();
                let prices = Rc::clone(&hub.prices);
                let shared = Rc::clone(shared);
                Panel::Tank(
                    cx.new(|cx| tank::TankSection::new(shared, choices, prices, window, cx)),
                )
            }
        }
    }

    fn section(&self) -> Section {
        match self {
            Panel::Tank(_) => Section::Tank,
        }
    }

    fn view(&self) -> AnyView {
        match self {
            Panel::Tank(e) => e.clone().into(),
        }
    }

    fn focus(&self, cx: &App) -> FocusHandle {
        match self {
            Panel::Tank(e) => e.read(cx).focus.clone(),
        }
    }

    /// A few words on where the section stands, in its tab's tooltip.
    fn summary(&self, cx: &App) -> SharedString {
        match self {
            Panel::Tank(e) => e.read(cx).summary(),
        }
    }

    /// The fit's numbers the section works on, for the header.
    fn stats(&self, cx: &App) -> Vec<Stat> {
        match self {
            Panel::Tank(e) => e.read(cx).stats(),
        }
    }

    fn set_target(&self, target: Target, cx: &mut App) {
        match self {
            Panel::Tank(e) => e.update(cx, |t, cx| t.set_target(target, cx)),
        }
    }

    fn icon(&self) -> AssetIcon {
        match self {
            Panel::Tank(_) => AssetIcon::Shield,
        }
    }
}

pub(super) struct WizardDialog {
    shared: Rc<Shared>,
    panels: Vec<Panel>,
    section: Section,
    target: Target,
    /// The hub has had the fit under its identity: a fit just given one is not there until its
    /// window publishes, which is no reason to call it deleted.
    seen: bool,
    focused: bool,
    _hub: Subscription,
}

impl Efa {
    /// Open the wizard over the current fit in a window of its own, or bring the open one
    /// forward with this fit. It needs a ship, and the item data for the hull's bonuses and the
    /// modules' fitting costs, which may still be loading the first time.
    pub(super) fn open_wizard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        if self.fit().and_then(|f| f.ship).is_none() {
            self.set_status("The fitting wizard needs a ship first");
            cx.notify();
            return;
        }
        let Some(data) = item_stats::hover_data(&loaded, cx) else {
            self.set_status("Loading item data for the fitting wizard…");
            cx.notify();
            let this = cx.weak_entity();
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
        let Some(current) = self.saved.settings.current else {
            return;
        };
        let Some(uid) = self.saved.ensure_uid(current) else {
            return;
        };
        self.save();
        let fit = (*self.saved.fits[current]).clone();
        let pilot = self.pilot(&loaded);
        let Some(shared) = Shared::new(
            self.hub.clone(),
            cx.weak_entity(),
            loaded,
            data,
            pilot,
            uid,
            fit,
            self.palette,
        ) else {
            return;
        };
        let shared = Rc::new(shared);
        let again = Rc::clone(&shared);
        let opening = super::chrome::open_single(
            "Fitting Wizard",
            (1100., 760.),
            cx,
            move |d: &mut WizardDialog, window, cx| d.load(again, window, cx),
            move |window, cx| WizardDialog::new(shared, window, cx),
        );
        Self::report_open(opening, cx);
    }
}

impl Shared {
    #[allow(clippy::too_many_arguments)]
    fn new(
        hub: Entity<Hub>,
        efa: WeakEntity<Efa>,
        loaded: Rc<Loaded>,
        data: Rc<ItemData>,
        pilot: Rc<Pilot>,
        uid: u64,
        fit: Fit,
        palette: Palette,
    ) -> Option<Self> {
        let facts = Facts::gather(&loaded.engine, &data, &loaded.catalog, &fit, &pilot)?;
        Some(Self {
            hub,
            efa,
            loaded,
            data,
            pilot,
            uid,
            saved: fit_text(&fit),
            fit,
            facts,
            palette,
        })
    }
}

/// A fit as text, to compare copies by.
fn fit_text(fit: &Fit) -> String {
    serde_json::to_string(fit).unwrap_or_default()
}

impl WizardDialog {
    fn new(shared: Rc<Shared>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let section = shared.hub.read(cx).saved.settings.wizard_tab;
        let hub = shared.hub.clone();
        let mut dialog = Self {
            panels: Vec::new(),
            section,
            target: Target::Same,
            seen: false,
            focused: false,
            _hub: cx.observe(&hub, |d: &mut Self, _, cx| d.check(cx)),
            shared: Rc::clone(&shared),
        };
        dialog.load(shared, window, cx);
        dialog
    }

    /// Work on `shared`'s fit from scratch.
    fn load(&mut self, shared: Rc<Shared>, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title(&format!(
            "efa: Fitting Wizard, {}",
            shared.loaded.catalog.name(shared.facts.ship)
        ));
        self.panels = Section::ALL
            .iter()
            .map(|&s| Panel::new(s, &shared, window, cx))
            .collect();
        self.shared = shared;
        self.target = Target::Same;
        self.seen = false;
        self.focused = false;
        cx.notify();
    }

    /// Start over from the fit as it is now.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = &self.shared;
        let Some(fit) = s
            .hub
            .read(cx)
            .saved
            .fits
            .iter()
            .find(|f| f.uid == s.uid)
            .map(|f| (**f).clone())
        else {
            return;
        };
        let Some(shared) = Shared::new(
            s.hub.clone(),
            s.efa.clone(),
            Rc::clone(&s.loaded),
            Rc::clone(&s.data),
            Rc::clone(&s.pilot),
            s.uid,
            fit,
            s.palette,
        ) else {
            return;
        };
        self.load(Rc::new(shared), window, cx);
    }

    /// Look at the fit in the hub: still there, still open, still the same.
    fn check(&mut self, cx: &mut Context<Self>) {
        let hub = self.shared.hub.read(cx);
        let found = hub.saved.fits.iter().position(|f| f.uid == self.shared.uid);
        self.seen |= found.is_some();
        let target = match found {
            None if !self.seen => Target::Same,
            None => Target::Gone,
            Some(i) if fit_text(&hub.saved.fits[i]) != self.shared.saved => Target::Changed,
            Some(i) if !hub.saved.settings.open.contains(&i) => Target::Closed,
            Some(_) => Target::Same,
        };
        if target != self.target {
            self.target = target;
            for panel in &self.panels {
                panel.set_target(target, cx);
            }
            cx.notify();
        }
    }

    fn panel(&self) -> Option<&Panel> {
        self.panels
            .iter()
            .find(|p| p.section() == self.section)
            .or_else(|| self.panels.first())
    }

    fn set_section(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        if section != self.section {
            self.section = section;
            self.shared
                .remember(cx, move |s| s.settings.wizard_tab = section);
        }
        if let Some(focus) = self.panel().map(|p| p.focus(cx)) {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// Ctrl+Tab and Ctrl+1 to 9 move between sections.
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if !keystroke.modifiers.control || keystroke.modifiers.alt {
            return;
        }
        let all = Section::ALL;
        let at = all.iter().position(|&s| s == self.section).unwrap_or(0);
        let section = match keystroke.key.as_str() {
            "tab" if keystroke.modifiers.shift => all[(at + all.len() - 1) % all.len()],
            "tab" => all[(at + 1) % all.len()],
            key => match key.parse::<usize>() {
                Ok(n) if (1..=all.len()).contains(&n) => all[n - 1],
                _ => return,
            },
        };
        self.set_section(section, window, cx);
        cx.stop_propagation();
    }

    /// A section's tab: icon and name, underlined in the accent while open.
    fn tab(&self, panel: &Panel, cx: &mut Context<Self>) -> AnyElement {
        let section = panel.section();
        let current = section == self.section;
        let summary = panel.summary(cx);
        let theme = cx.theme();
        let n = Section::ALL.iter().position(|&s| s == section).unwrap_or(0) + 1;
        let tip = SharedString::from(format!("Ctrl+{n}: {summary}"));
        h_flex()
            .id(SharedString::from(format!("wizard-section-{section:?}")))
            .h(px(24.))
            .gap_1p5()
            .px_2()
            .rounded(px(4.))
            .cursor_pointer()
            .text_sm()
            .when(current, |d| {
                d.bg(theme.secondary).text_color(theme.foreground)
            })
            .when(!current, |d| {
                d.text_color(theme.muted_foreground)
                    .hover(|h| h.text_color(theme.foreground))
            })
            .child(UiIcon::new(panel.icon()).xsmall())
            .child(section.label())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &gpui_kit::MouseDownEvent, window, cx| {
                    this.set_section(section, window, cx);
                }),
            )
            .into_any_element()
    }

    /// The ship, the sections, and what the fit has now where the open section works.
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let tabs: Vec<AnyElement> = self.panels.iter().map(|p| self.tab(p, cx)).collect();
        let stats = self.panel().map_or_default(|p| p.stats(cx));
        let theme = cx.theme();
        let catalog = &self.shared.loaded.catalog;
        let stats = stats.into_iter().enumerate().map(|(i, s)| {
            let tip = s.tip.clone();
            h_flex()
                .id(SharedString::from(format!("wizard-stat-{i}")))
                .gap_1p5()
                .child(div().text_color(theme.muted_foreground).child(s.label))
                .child(s.value)
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        });
        h_flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_x_4()
            .gap_y_1()
            .px_2()
            .py_1()
            .min_h(px(32.))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .text_sm()
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1p5()
                    .child(widgets::icon_box(
                        catalog,
                        &self.shared.loaded.icons,
                        Icon::Type(self.shared.facts.ship),
                        20.,
                    ))
                    .child(div().truncate().child(self.shared.fit.name.clone()))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(catalog.name(self.shared.facts.ship)),
                    ),
            )
            .child(h_flex().gap_1().children(tabs))
            .child(div().flex_1())
            .child(h_flex().gap_4().children(stats))
    }

    /// A strip under the header when the fit is no longer what the wizard works on.
    fn banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let palette = self.shared.palette;
        let (color, text, reload) = match self.target {
            Target::Same => return None,
            Target::Changed => (
                palette.yellow,
                "The fit changed since the wizard opened. The results go by the old fit.",
                true,
            ),
            Target::Closed => (
                palette.yellow,
                "The fit is no longer open in the main window. Fitting a result still changes it.",
                false,
            ),
            Target::Gone => (
                palette.red,
                "The fit was deleted, so there is nothing to fit a result to.",
                false,
            ),
        };
        let fill = widgets::color(color);
        Some(
            h_flex()
                .flex_none()
                .gap_2()
                .px_2()
                .py_1()
                .bg(fill.opacity(0.1))
                .border_b_1()
                .border_color(fill.opacity(0.3))
                .text_xs()
                .child(
                    UiIcon::new(AssetIcon::TriangleAlert)
                        .xsmall()
                        .text_color(fill),
                )
                .child(div().flex_1().min_w_0().child(text))
                .when(reload, |d| {
                    d.child(
                        Button::new("wizard-reload")
                            .xsmall()
                            .ghost()
                            .icon(AssetIcon::RotateCcw)
                            .label("Start over from the fit")
                            .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                    )
                }),
        )
    }
}

impl Render for WizardDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused {
            self.focused = true;
            if let Some(focus) = self.panel().map(|p| p.focus(cx)) {
                window.defer(cx, move |window, cx| window.focus(&focus, cx));
            }
        }
        let header = self.header(cx);
        let banner = self.banner(cx);
        let view = self.panel().map(Panel::view);
        let theme = cx.theme();
        v_flex()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.key(event, window, cx);
            }))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(header)
            .children(banner)
            .child(div().flex_1().min_h_0().children(view))
    }
}

// Pieces the sections share.

/// Width of the label column in the controls.
const FIELD_LABEL_W: f32 = 72.;

/// A control with its label to the left, on one line.
fn field(title: &str, control: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .w_full()
        .min_h(px(24.))
        .gap_2()
        .child(
            div()
                .w(px(FIELD_LABEL_W))
                .flex_none()
                .whitespace_nowrap()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(title.to_owned()),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// A titled set of fields, the title over a hairline like the stats pane's headers.
fn fieldset(title: &str, rows: Vec<AnyElement>, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(
            div()
                .pb_0p5()
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .children(rows)
}

/// Small muted text that wraps.
fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A line of small text with an icon in a colour of the palette.
fn notice(icon: AssetIcon, color: Rgb, text: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_1p5()
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

/// A hairline between parts of a pane.
fn divider(cx: &App) -> Div {
    div().h_px().w_full().bg(cx.theme().border)
}

/// "1 slot", "3 slots".
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// "a, b and c".
fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
