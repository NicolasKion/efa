//! The right side: the fit's stats in collapsible sections, in pyfa's order (gui/statsPane.py),
//! with the menus pyfa puts on a section header.

mod defense;
pub(super) mod format;
mod misc;
mod offense;
mod price;
mod resources;
pub(super) mod text;

use std::{collections::HashSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, WeakEntity, Window,
    div, prelude::FluentBuilder, px,
};

use super::{
    AdditionsTab, Efa, Loaded,
    widgets::{icon_box, section_title},
};
use crate::{
    engine::Stats, fit::Fit, icons::Icon, prices::Prices, store::PriceTotals, theme::Palette,
};

/// Pane state that lasts for the session only, like pyfa's.
#[derive(Debug, Default)]
pub struct View {
    /// Show raw HP instead of effective HP, in the resistance and recharge sections.
    pub raw_hp: bool,
    /// Show mining yield where firepower would be.
    pub mining: bool,
}

/// Game icon IDs for stats the shared set in `widgets::icon` does not cover.
mod ids {
    pub const MINING: i32 = 22;
    pub const CARGO: i32 = 71;
    pub const REPAIR: i32 = 80;
    pub const SKILL: i32 = 33;
    pub const FIGHTER_TUBES: i32 = 2677;
    pub const NEUT: i32 = 1400;
    pub const TRANSFER: i32 = 1033;
    /// The Ship Equipment market group's.
    pub const FITTINGS: i32 = 1432;
    /// The Implants & Boosters market group's.
    pub const CHARACTER: i32 = 2563;
    /// The wallet window's.
    pub const ISK: i32 = 25120;
}

/// Everything a section needs to draw itself.
pub(in crate::ui) struct Pane<'a> {
    loaded: &'a Loaded,
    stats: &'a Stats,
    fit: &'a Fit,
    view: &'a View,
    palette: Palette,
    additions_tab: AdditionsTab,
    prices: &'a Prices,
    /// A price fetch is out.
    prices_updating: bool,
    price_totals: PriceTotals,
    /// Firepower counts reloads.
    factor_reload: bool,
    /// The user's patterns and profiles, for the header menus.
    damage_patterns: Rc<Vec<crate::damage_pattern::DamagePattern>>,
    target_profiles: Rc<Vec<crate::target_profile::TargetProfile>>,
    /// Implants plugged into the fit's character.
    character_implants: &'a [crate::data::TypeId],
    /// The fit's implants and boosters count against the cargo hold.
    implants_in_cargo: bool,
}

/// The view a window that only shows sections draws them in: effective HP, firepower.
static PLAIN_VIEW: View = View {
    raw_hp: false,
    mining: false,
};

impl<'a> Pane<'a> {
    /// A fit's sections as another window shows them, with pyfa's defaults for what the main
    /// window lets the user switch.
    pub(in crate::ui) fn plain(
        loaded: &'a Loaded,
        stats: &'a Stats,
        fit: &'a Fit,
        palette: Palette,
        prices: &'a Prices,
    ) -> Self {
        Self {
            loaded,
            stats,
            fit,
            view: &PLAIN_VIEW,
            palette,
            additions_tab: AdditionsTab::Drones,
            prices,
            prices_updating: false,
            price_totals: PriceTotals::default(),
            factor_reload: false,
            damage_patterns: Rc::default(),
            target_profiles: Rc::default(),
            character_implants: &[],
            implants_in_cargo: false,
        }
    }
}

/// The sections, in pyfa's order. Firepower is mining yield while the view is swapped to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum SectionKind {
    Resources,
    Resistances,
    Recharge,
    Firepower,
    Outgoing,
    Capacitor,
    Targeting,
    Bombing,
    Price,
}

impl SectionKind {
    pub(in crate::ui) const ALL: [Self; 9] = [
        Self::Resources,
        Self::Resistances,
        Self::Recharge,
        Self::Firepower,
        Self::Outgoing,
        Self::Capacitor,
        Self::Targeting,
        Self::Bombing,
        Self::Price,
    ];

    /// The name the preferences and the collapsed set know the section by.
    pub(in crate::ui) fn key(self) -> &'static str {
        match self {
            Self::Resources => "Resources",
            Self::Resistances => "Resistances",
            Self::Recharge => "Recharge",
            Self::Firepower => "Firepower",
            Self::Outgoing => "Outgoing",
            Self::Capacitor => "Capacitor",
            Self::Targeting => "Targeting",
            Self::Bombing => "Bombing",
            Self::Price => "Price",
        }
    }
}

/// What the main window adds to its sections: header menus, the toggles and collapsing. A
/// window without them shows the same sections read-only.
pub(in crate::ui) struct Controls<'a> {
    pub efa: WeakEntity<Efa>,
    pub collapsed: &'a HashSet<&'static str>,
}

/// `which` sections of the pane's fit, each under its header.
pub(in crate::ui) fn render_sections(
    pane: &Pane<'_>,
    which: &[SectionKind],
    controls: Option<&Controls<'_>>,
    cx: &App,
) -> Vec<AnyElement> {
    which
        .iter()
        .map(|&kind| {
            let section = match kind {
                SectionKind::Resources => resources::section(pane, cx),
                SectionKind::Resistances => defense::resistances(pane, controls, cx),
                SectionKind::Recharge => defense::recharge(pane),
                SectionKind::Firepower if pane.view.mining => offense::mining(pane, controls),
                SectionKind::Firepower => offense::firepower(pane, controls, cx),
                SectionKind::Outgoing => offense::outgoing(pane),
                SectionKind::Capacitor => misc::capacitor(pane),
                SectionKind::Targeting => misc::targeting(pane, cx),
                SectionKind::Bombing => defense::bombing(pane),
                SectionKind::Price => price::section(pane, controls, cx),
            };
            frame(section, controls)
        })
        .collect()
}

impl Pane<'_> {
    fn icon(&self, icon_id: i32, size: f32) -> Div {
        icon_box(
            &self.loaded.catalog,
            &self.loaded.icons,
            Icon::Id(icon_id),
            size,
        )
    }

    /// A game icon with a tooltip naming what it stands for.
    fn icon_tip(
        &self,
        id: impl Into<ElementId>,
        icon_id: i32,
        size: f32,
        text: &str,
    ) -> AnyElement {
        tip(id, self.icon(icon_id, size), text).into_any_element()
    }
}

/// `child` with a hover text; an empty text adds none, as pyfa clears tooltips.
fn tip(
    id: impl Into<ElementId>,
    child: impl IntoElement,
    text: impl Into<SharedString>,
) -> Stateful<Div> {
    let text: SharedString = text.into();
    div().id(id).child(child).when(!text.is_empty(), |d| {
        d.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
    })
}

fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

fn warn(text: impl Into<SharedString>, warn: bool, cx: &App) -> Div {
    div()
        .when(warn, |d| d.text_color(cx.theme().danger))
        .child(text.into())
}

/// Builds a header menu; shared by the header button and a right-click on the section.
type MenuFn = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

struct Section {
    key: &'static str,
    title: AnyElement,
    body: AnyElement,
    menu: Option<MenuFn>,
    /// A control for the header's right end, before the menu, e.g. the firepower/mining swap.
    action: Option<AnyElement>,
}

/// Width of the stats pane. pyfa sizes its pane to the content and keeps it there.
pub const PANE_WIDTH: f32 = 300.;
/// pyfa draws its stats pane in 8 point text on Linux, about 11 pixels.
pub(in crate::ui) const TEXT_SIZE: f32 = 11.;

impl Efa {
    pub(super) fn stats_panel(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let character = self.character_bar(loaded, cx);
        let theme = cx.theme();
        let (Some(stats), Some(fit)) = (&self.stats, self.fit()) else {
            return v_flex()
                .size_full()
                .bg(theme.sidebar)
                .text_size(px(TEXT_SIZE))
                .child(character)
                .into_any_element();
        };
        let pane = Pane {
            loaded,
            stats,
            fit,
            view: &self.stats_view,
            palette: self.palette,
            additions_tab: self.additions_tab,
            prices: &self.prices,
            prices_updating: self.prices_fetch.is_some(),
            price_totals: self.saved.price_totals,
            factor_reload: self.saved.factor_reload,
            damage_patterns: Rc::new(self.saved.damage_patterns.clone()),
            target_profiles: Rc::new(self.saved.target_profiles.clone()),
            character_implants: self.character_implants(),
            implants_in_cargo: self.saved.preferences.general.implants_in_cargo,
        };
        let which: Vec<SectionKind> = SectionKind::ALL
            .into_iter()
            .filter(|s| self.saved.preferences.stats.shows(s.key()))
            .collect();
        let controls = Controls {
            efa: Self::weak(cx),
            collapsed: &self.collapsed_sections,
        };
        let sections = render_sections(&pane, &which, Some(&controls), cx);
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .text_size(px(TEXT_SIZE))
            .child(character)
            .child(
                v_flex()
                    .id("stats-sections")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_2()
                    .children(sections),
            )
            .into_any_element()
    }
}

/// A section under its header. With the main window's controls, a click on the title folds it
/// and the header carries the section's menu.
fn frame(section: Section, controls: Option<&Controls<'_>>) -> AnyElement {
    let Section {
        key,
        title,
        body,
        menu,
        action,
    } = section;
    let Some(controls) = controls else {
        return v_flex()
            .gap_1()
            .child(h_flex().gap_1().child(title).children(action))
            .child(div().pl_1().child(body))
            .into_any_element();
    };
    let collapsed = controls.collapsed.contains(key);
    let efa = controls.efa.clone();
    let header = h_flex()
        .gap_1()
        .child(
            h_flex()
                .id(SharedString::from(format!("section-{key}")))
                .flex_1()
                .gap_1()
                .child(div().w(px(10.)).child(if collapsed { "▸" } else { "▾" }))
                .child(title)
                .on_click(move |_: &ClickEvent, _, cx| {
                    Efa::app_update(&efa, cx, |this, cx| {
                        if !this.collapsed_sections.remove(key) {
                            this.collapsed_sections.insert(key);
                        }
                        cx.notify();
                    });
                }),
        )
        .children(action)
        .when_some(menu.clone(), |h, menu| {
            h.child(
                Button::new(SharedString::from(format!("section-menu-{key}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Menu)
                    .dropdown_menu(move |m, window, cx| menu(m, window, cx)),
            )
        });
    let body = div()
        .id(SharedString::from(format!("section-body-{key}")))
        .pl_1()
        .child(body);
    let body = match menu {
        Some(menu) => body
            .context_menu(move |m, window, cx| menu(m, window, cx))
            .into_any_element(),
        None => body.into_any_element(),
    };
    v_flex()
        .gap_1()
        .child(header)
        .when(!collapsed, |s| s.child(body))
        .into_any_element()
}

/// A plain section title.
fn title(text: impl Into<SharedString>) -> AnyElement {
    section_title(text).into_any_element()
}
