//! The fittings browser: ship groups, ships and their fits, with pyfa's navigation toolbar.

use std::{collections::HashSet, ops::Range, rc::Rc};

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::ContextMenuExt,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, InteractiveElement, IntoElement, MouseButton,
    MouseUpEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, div,
    prelude::FluentBuilder, px, uniform_list,
};

use super::menus::{fit_menu, ship_menu};
use crate::ui::BrowserTab;
use crate::ui::tooltips::{TipHead, ship_tooltip};
use crate::ui::widgets::TipExt;
use crate::ui::{
    Efa, Loaded,
    additions::FitDrag,
    widgets::{band, count, empty_line, icon_box, list_row},
};
use crate::{
    data::TypeId,
    icons::Icon,
    ship_browser::{self, Race, Stage},
};

/// Height of a ship group line in the fittings browser, and of a ship or fit line.
const GROUP_ROW_H: f32 = 26.;
const FIT_ROW_H: f32 = 44.;

/// A line of the fittings browser.
#[derive(Debug, Clone, Copy)]
enum BrowserRow {
    /// A band over the ships or the structures.
    Header(&'static str),
    /// A ship group and how many fits its ships have.
    Group(i32, usize),
    /// A ship and how many fits it has.
    Ship(TypeId, usize),
    /// A saved fit, by index.
    Fit(usize),
}

/// The fittings browser's lines, and the races its race filter offers.
pub(in crate::ui) struct BrowserRows {
    rows: Vec<BrowserRow>,
    races: Vec<Race>,
}

/// What the fittings browser's lines were listed from.
pub(in crate::ui) struct BrowserKey {
    /// The fits, kept so a changed one is told apart by address.
    fits: Vec<Rc<crate::fit::Fit>>,
    stage: Stage,
    hide_empty: bool,
    races: Vec<Race>,
}

impl BrowserKey {
    fn same(&self, other: &Self) -> bool {
        self.stage == other.stage
            && self.hide_empty == other.hide_empty
            && self.races == other.races
            && self.fits.len() == other.fits.len()
            && self
                .fits
                .iter()
                .zip(&other.fits)
                .all(|(a, b)| Rc::ptr_eq(a, b))
    }
}

impl Efa {
    /// Search ships and fits from the fittings browser's box; clearing it goes back.
    pub(in crate::ui) fn search_fittings(&mut self, text: String, cx: &mut Context<Self>) {
        if text.trim().is_empty() {
            if matches!(self.ship_browser.stage, Stage::Search(_)) {
                self.ship_browser.back();
            }
        } else {
            self.ship_browser.go(Stage::Search(text));
        }
        cx.notify();
    }

    /// A new fit for a ship from the fittings browser, named the way pyfa's browser names it.
    pub(super) fn new_fit_for_ship(&mut self, ship: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let name = format!("{} fit", loaded.catalog.name(ship));
        let fit = self.fresh_fit(name, Some(ship), &loaded.catalog);
        self.ship_browser.go(Stage::Fits(ship));
        self.add_fit(fit, cx);
    }

    /// The fittings browser's rows for where it is, worked out again only when the fits, the
    /// place or its filters changed.
    fn browser_rows(&mut self, loaded: &Loaded) -> Rc<BrowserRows> {
        let key = BrowserKey {
            fits: self.saved.fits.clone(),
            stage: self.ship_browser.stage.clone(),
            hide_empty: self.ship_browser.hide_empty,
            races: self.ship_browser.races.iter().copied().collect(),
        };
        if let Some((last, rows)) = &self.browser_cache
            && last.same(&key)
        {
            return Rc::clone(rows);
        }
        let rows = Rc::new(self.list_browser_rows(loaded));
        self.browser_cache = Some((key, Rc::clone(&rows)));
        rows
    }

    fn list_browser_rows(&self, loaded: &Loaded) -> BrowserRows {
        let counts = ship_browser::fit_counts(&self.saved.fits);
        let count_of = |ship: TypeId| counts.get(&ship).copied().unwrap_or(0);
        let mut races = Vec::new();
        let rows: Vec<BrowserRow> = match &self.ship_browser.stage {
            Stage::Groups => {
                // Ships first, then structures in a section of their own.
                let shown = |structures: bool| {
                    loaded
                        .catalog
                        .ship_groups
                        .iter()
                        .filter(move |(_, types)| {
                            types
                                .first()
                                .and_then(|&t| loaded.catalog.get(t))
                                .is_some_and(|t| t.is_structure() == structures)
                        })
                        .filter_map(|(group, _)| {
                            let count =
                                ship_browser::group_fit_count(&loaded.catalog, *group, &counts);
                            (!self.ship_browser.hide_empty || count > 0).then_some((*group, count))
                        })
                        .collect::<Vec<_>>()
                };
                let (ships, structures) = (shown(false), shown(true));
                let mut rows = Vec::new();
                for (title, groups) in [("Ships", ships), ("Structures", structures)] {
                    if groups.is_empty() {
                        continue;
                    }
                    rows.push(BrowserRow::Header(title));
                    rows.extend(
                        groups
                            .into_iter()
                            .map(|(group, count)| BrowserRow::Group(group, count)),
                    );
                }
                rows
            }
            Stage::Ships(group) => {
                let ships = loaded.catalog.ships_in(*group);
                let present: HashSet<Race> = ships
                    .iter()
                    .filter_map(|&s| loaded.catalog.get(s)?.race)
                    .collect();
                races = present.iter().copied().collect();
                races.sort();
                ships
                    .iter()
                    .filter(|&&ship| {
                        let race = loaded.catalog.get(ship).and_then(|t| t.race);
                        self.ship_browser.shows_race(race, &present)
                            && (!self.ship_browser.hide_empty || count_of(ship) > 0)
                    })
                    .map(|&ship| BrowserRow::Ship(ship, count_of(ship)))
                    .collect()
            }
            Stage::Fits(ship) => ship_browser::fits_of(&self.saved.fits, *ship)
                .into_iter()
                .map(BrowserRow::Fit)
                .collect(),
            Stage::Search(query) => ship_browser::search_ships(&loaded.catalog, query)
                .into_iter()
                .map(|ship| BrowserRow::Ship(ship, count_of(ship)))
                .chain(
                    ship_browser::search_fits(&self.saved.fits, query)
                        .into_iter()
                        .map(BrowserRow::Fit),
                )
                .collect(),
            Stage::Recent => ship_browser::recent_fits(&self.saved.fits)
                .into_iter()
                .map(BrowserRow::Fit)
                .collect(),
        };
        BrowserRows { rows, races }
    }

    pub(super) fn fittings(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let stage = self.ship_browser.stage.clone();
        let listed = self.browser_rows(loaded);
        let empty = match &stage {
            Stage::Groups | Stage::Ships(_) if self.ship_browser.hide_empty => "No fits yet.",
            Stage::Groups | Stage::Ships(_) => "Nothing here.",
            Stage::Fits(_) => "No fits for this ship yet.",
            Stage::Search(_) => "No matching results.",
            Stage::Recent => "No fits yet.",
        };
        // Groups are single lines; ships and fits show an icon over two.
        let row_h = if stage == Stage::Groups {
            GROUP_ROW_H
        } else {
            FIT_ROW_H
        };
        let list = if listed.rows.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .child(empty_line(empty, cx))
                .into_any_element()
        } else {
            let rows = Rc::clone(&listed);
            uniform_list(
                "fittings-list",
                rows.rows.len(),
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    let Some(loaded) = this.loaded() else {
                        return Vec::new();
                    };
                    rows.rows[range]
                        .iter()
                        .map(|row| match *row {
                            BrowserRow::Header(title) => section_header(title, cx),
                            BrowserRow::Group(group, count) => {
                                Self::ship_group_row(&loaded, group, count, cx)
                            }
                            BrowserRow::Ship(ship, count) => {
                                this.ship_row(&loaded, ship, count, cx)
                            }
                            BrowserRow::Fit(i) => this.fit_row(&loaded, i, cx),
                        })
                        .map(|row| div().h(px(row_h)).child(row).into_any_element())
                        .collect()
                }),
            )
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.fittings_toolbar(&stage, cx))
            .child(Self::fittings_title(loaded, &stage, cx))
            .child(list)
            .when(!listed.races.is_empty(), |pane| {
                pane.child(self.race_filter(loaded, &listed.races, cx))
            })
    }

    /// pyfa's navigation panel: ship groups, back, recent fits, empty groups, new fit, and the
    /// search box.
    fn fittings_toolbar(&self, stage: &Stage, cx: &mut Context<Self>) -> impl IntoElement {
        let browser = &self.ship_browser;
        let browsing = matches!(stage, Stage::Groups | Stage::Ships(_));
        let ship = match stage {
            Stage::Fits(ship) => Some(*ship),
            _ => None,
        };
        let tool = |id: &'static str, icon: IconName, tip: &'static str| {
            Button::new(id).ghost().xsmall().icon(icon).tooltip(tip)
        };
        v_flex()
            .gap_1()
            .p_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_0p5()
                    .child(
                        tool("fittings-home", IconName::LayoutDashboard, "Ship groups").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.ship_browser.go(Stage::Groups);
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        tool("fittings-back", IconName::ArrowLeft, "Back")
                            .disabled(!browser.can_go_back())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ship_browser.back();
                                cx.notify();
                            })),
                    )
                    .child(
                        tool(
                            "fittings-recent",
                            IconName::Calendar,
                            if *stage == Stage::Recent {
                                "Hide Recent Fits"
                            } else {
                                "Recent Fits"
                            },
                        )
                        .selected(*stage == Stage::Recent)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.ship_browser.stage == Stage::Recent {
                                this.ship_browser.go(Stage::Groups);
                            } else {
                                this.ship_browser.go(Stage::Recent);
                            }
                            cx.notify();
                        })),
                    )
                    .when(browsing, |bar| {
                        let hide = browser.hide_empty;
                        bar.child(
                            tool(
                                "fittings-empty",
                                if hide {
                                    IconName::EyeOff
                                } else {
                                    IconName::Eye
                                },
                                if hide {
                                    "Show empty ship groups"
                                } else {
                                    "Hide empty ship groups"
                                },
                            )
                            .selected(hide)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ship_browser.hide_empty = !this.ship_browser.hide_empty;
                                cx.notify();
                            })),
                        )
                    })
                    .when_some(ship, |bar, ship| {
                        bar.child(tool("fittings-new", IconName::Plus, "New fit").on_click(
                            cx.listener(move |this, _, _, cx| this.new_fit_for_ship(ship, cx)),
                        ))
                    }),
            )
            .child(self.fit_search_input())
    }

    /// Where the browser is: the group, ship, search or recent fits.
    fn fittings_title(loaded: &Loaded, stage: &Stage, cx: &mut Context<Self>) -> AnyElement {
        let title = match stage {
            Stage::Groups => "Ship groups".to_owned(),
            Stage::Ships(group) => loaded
                .catalog
                .group_names
                .get(group)
                .cloned()
                .unwrap_or_default(),
            Stage::Fits(ship) => loaded.catalog.name(*ship),
            Stage::Search(query) => format!("Search: {}", query.trim()),
            Stage::Recent => "Recent fits".to_owned(),
        };
        let title = div().min_w_0().truncate().child(title);
        let row = h_flex()
            .id("fittings-title")
            .gap_2()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .border_b_1()
            .border_color(cx.theme().border);
        match stage {
            // pyfa's baseShip menu, on the ship whose fits are listed.
            Stage::Fits(ship) => {
                let ship = *ship;
                row.child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Type(ship),
                    18.,
                ))
                .child(title)
                .context_menu({
                    let this = cx.weak_entity();
                    move |menu, _, _| ship_menu(menu, &this, ship)
                })
                .into_any_element()
            }
            Stage::Ships(group) => row
                .children(
                    crate::brackets::ship_bracket(*group)
                        .map(|b| icon_box(&loaded.catalog, &loaded.icons, Icon::Bracket(b), 16.)),
                )
                .child(title)
                .into_any_element(),
            _ => row.child(title).into_any_element(),
        }
    }

    /// The race toggles under a group's ships, pyfa's race selector.
    /// The race filter under a group's ships: one faction logo per race, the name on hover.
    fn race_filter(&self, loaded: &Loaded, races: &[Race], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let buttons: Vec<AnyElement> = races
            .iter()
            .map(|&race| {
                let on = self.ship_browser.races.contains(&race);
                let label = race.label();
                let button = div()
                    .id(SharedString::from(format!("race-{race:?}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(30.))
                    .min_w(px(30.))
                    .rounded(px(5.))
                    .border_1()
                    .when(on, |d| d.bg(theme.secondary).border_color(theme.primary))
                    .when(!on, |d| {
                        d.border_color(gpui_kit::transparent_black())
                            .opacity(0.6)
                            .hover(|s| s.opacity(1.).bg(theme.secondary))
                    })
                    .tip(label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.ship_browser.toggle_race(race);
                        cx.notify();
                    }));
                match race.logo_corp() {
                    Some(corp) => button
                        .child(icon_box(
                            &loaded.catalog,
                            &loaded.icons,
                            Icon::CorpLogo(corp),
                            24.,
                        ))
                        .into_any_element(),
                    None => button.px_2().text_xs().child(label).into_any_element(),
                }
            })
            .collect();
        h_flex()
            .flex_wrap()
            .gap_1()
            .p_1()
            .border_t_1()
            .border_color(theme.border)
            .children(buttons)
            .into_any_element()
    }

    fn ship_group_row(
        loaded: &Loaded,
        group: i32,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = loaded
            .catalog
            .group_names
            .get(&group)
            .cloned()
            .unwrap_or_default();
        list_row(SharedString::from(format!("ship-group-{group}")), false, cx)
            .gap_2()
            .px_2()
            .h_full()
            .child(
                div().w(px(16.)).flex_none().children(
                    crate::brackets::ship_bracket(group)
                        .map(|b| icon_box(&loaded.catalog, &loaded.icons, Icon::Bracket(b), 16.)),
                ),
            )
            .child(div().flex_1().min_w_0().truncate().child(name))
            .when(count > 0, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(fits_label(count)),
                )
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.ship_browser.go(Stage::Ships(group));
                cx.notify();
            }))
            .into_any_element()
    }

    fn ship_row(
        &self,
        loaded: &Loaded,
        ship: TypeId,
        count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tooltips = self
            .saved
            .settings
            .preferences
            .general
            .ship_browser_tooltips;
        list_row(SharedString::from(format!("ship-{ship}")), false, cx)
            .h_full()
            .gap_2()
            .p_1()
            .child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Type(ship),
                32.,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(loaded.catalog.name(ship)))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(fits_label(count)),
                    ),
            )
            .children(self.omega_marker(loaded, ship, 16.))
            .when_some(self.loaded().filter(|_| tooltips), |row, loaded| {
                row.tooltip(move |window, cx| {
                    ship_tooltip(&loaded, ship, TipHead::Traits, window, cx)
                })
            })
            // A ship without fits gets one, as clicking it in pyfa starts one.
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if count > 0 {
                    this.ship_browser.go(Stage::Fits(ship));
                    cx.notify();
                } else {
                    this.new_fit_for_ship(ship, cx);
                }
            }))
            .context_menu({
                let this = cx.weak_entity();
                move |menu, _, _| ship_menu(menu, &this, ship)
            })
            .into_any_element()
    }

    fn fit_row(&self, loaded: &Loaded, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let fit = &self.saved.fits[i];
        let ship_name = fit
            .ship
            .map_or_else(String::new, |s| loaded.catalog.name(s));
        let notes = Self::fitting_tooltip(fit, &ship_name).map(|n| n.to_string());
        let tooltips = self
            .saved
            .settings
            .preferences
            .general
            .ship_browser_tooltips;
        let subtitle = match ship_browser::fit_date(fit.modified) {
            Some(date) => format!("{ship_name}  {date}"),
            None => ship_name.clone(),
        };
        let open = self.saved.settings.current == Some(i);
        list_row(SharedString::from(format!("fitting-{i}")), open, cx)
            .h_full()
            .gap_2()
            .p_1()
            .map(|row| match (fit.ship, self.loaded().filter(|_| tooltips)) {
                (Some(ship), Some(loaded)) => row.tooltip(move |window, cx| {
                    ship_tooltip(&loaded, ship, TipHead::Fit(notes.clone()), window, cx)
                }),
                _ => match notes {
                    Some(notes) => {
                        let tip: SharedString = notes.into();
                        row.tip(tip)
                    }
                    None => row,
                },
            })
            .when_some(fit.ship, |row, ship| {
                row.child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Type(ship),
                    32.,
                ))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(fit.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    ),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_from_browser(i, cx);
            }))
            // A middle click opens the fit in a tab of its own, as in pyfa.
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, _, cx| this.open_fit(i, cx)),
            )
            // The Projected and Command tabs take fits dragged from here.
            .on_drag(
                FitDrag {
                    index: i,
                    label: fit.name.clone().into(),
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .context_menu({
                let this = cx.weak_entity();
                let has_fit = self.saved.settings.current.is_some();
                move |menu, _, _| fit_menu(menu, &this, i, has_fit)
            })
            .into_any_element()
    }
}

/// "No fits", "1 fit", "3 fits", as pyfa's ship rows count them.
fn fits_label(n: usize) -> String {
    if n == 0 {
        "No fits".to_owned()
    } else {
        count(n, "fit", "fits")
    }
}

/// A band over a run of rows in the fittings browser, like the fitting view's rack labels.
fn section_header(title: &'static str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    band(div().h_full().flex().items_center().px_2(), cx)
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(title)
        .into_any_element()
}
impl Efa {
    /// Show one ship's fits in the fittings browser, pyfa's shipJump.
    pub(in crate::ui) fn jump_to_fittings(&mut self, ship: TypeId, cx: &mut Context<Self>) {
        self.browser_tab = BrowserTab::Fittings;
        self.side_stats = false;
        self.ship_browser.go(crate::ship_browser::Stage::Fits(ship));
        cx.notify();
    }

    /// Whether the fittings browser shows this ship's fits right now.
    pub(in crate::ui) fn browsing_fits_of(&self, ship: TypeId) -> bool {
        self.browser_tab == BrowserTab::Fittings
            && self.ship_browser.stage == crate::ship_browser::Stage::Fits(ship)
    }
}
