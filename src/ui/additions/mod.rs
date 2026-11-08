//! The additions pane under the fit: drones, implants, boosters, cargo and notes, with pyfa's
//! per-tab context menus. Shared pieces live here; each tab has its own module.

mod boosters;
mod cargo;
mod command;
mod drones;
pub(in crate::ui) mod fighters;
mod implants;
mod linked;
mod notes;
mod projected;

pub(super) use cargo::{amount, drop_transfer, goes_to_cargo};
pub(super) use linked::{FitDrag, Link};

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::PopupMenu,
    v_flex,
};
use gpui_kit::{
    App, AppContext, ClickEvent, ClipboardItem, Context, InteractiveElement, IntoElement,
    KeyDownEvent, Modifiers, ParentElement, StatefulInteractiveElement, Styled, WeakEntity, Window,
    div, prelude::FluentBuilder, px,
};

use super::{
    Action, AddKind, AdditionsTab, Efa, Loaded,
    item_stats::{StatsOf, stats_entry},
    stats::format,
    widgets::{entry, icon_label, item_entry},
};
use crate::{
    data::{Catalog, ItemType, Kind, TypeId},
    engine::FitItem,
    fit::{Fit, split_count},
    preferences::AdditionsLabels,
    prices::Prices,
};

const ROW_HEIGHT: f32 = 28.;
/// Width of the price cell in the tabs with fixed-width columns.
const PRICE_W: f32 = 64.;

/// pyfa's price column (builtinViewColumns/price.py): the price of `amount` of the item, blank
/// when it has none.
fn price_text(prices: &Prices, type_id: TypeId, amount: u32) -> Option<String> {
    prices
        .get(type_id)
        .filter(|&p| p > 0.0)
        .map(|p| format::currency(p * f64::from(amount)))
}
/// Tallest a long menu grows before it scrolls.
const MENU_MAX_H: f32 = 480.;

impl AddKind {
    /// The word pyfa uses for the tab's items in menu labels.
    fn noun(self) -> &'static str {
        match self {
            Self::Drones => "Drones",
            Self::Implants => "Implants",
            Self::Boosters => "Boosters",
            Self::Cargo => "Cargo Items",
        }
    }

    /// Whether an item can go into this tab.
    fn accepts(self, kind: Kind) -> bool {
        match self {
            Self::Drones => matches!(kind, Kind::Drone),
            Self::Implants => matches!(kind, Kind::Implant),
            Self::Boosters => matches!(kind, Kind::Booster),
            Self::Cargo => true,
        }
    }

    /// Rows of the tab as (type, amount).
    fn rows(self, fit: &Fit) -> Vec<(TypeId, u32)> {
        match self {
            Self::Drones => fit.drones.iter().map(|d| (d.type_id, d.count)).collect(),
            Self::Implants => fit.implants.iter().map(|&t| (t, 1)).collect(),
            Self::Boosters => fit.boosters.iter().map(|b| (b.type_id, 1)).collect(),
            Self::Cargo => fit.cargo.clone(),
        }
    }
}

/// The number in an additions tab's name, as pyfa's "Extra info in Additions panel tab names"
/// picks it: active items, all items, or none. Cargo counts stacks either way, as in pyfa.
fn tab_count(
    fit: &Fit,
    tab: AdditionsTab,
    labels: AdditionsLabels,
    character_implants: &[TypeId],
) -> usize {
    let active = labels == AdditionsLabels::Active;
    match (labels, tab) {
        (AdditionsLabels::None, _) | (_, AdditionsTab::Notes) => 0,
        (_, AdditionsTab::Drones) => fit
            .drones
            .iter()
            .map(|d| if active { d.active } else { d.count } as usize)
            .sum(),
        (_, AdditionsTab::Implants) => fit
            .applied_implants(character_implants)
            .filter(|&(_, on)| on || !active)
            .count(),
        (_, AdditionsTab::Boosters) => fit.boosters.iter().filter(|b| b.active || !active).count(),
        (_, AdditionsTab::Cargo) => fit.cargo.len(),
        (_, AdditionsTab::Fighters) => fit.fighters.iter().filter(|f| f.active || !active).count(),
        (_, AdditionsTab::Projected) => fit.projected.len(),
        (_, AdditionsTab::Command) => fit.command.iter().filter(|c| c.active || !active).count(),
    }
}

/// Lines pyfa copies for additions: "Name xN" for stacks, the bare name for single items.
fn export_lines(catalog: &Catalog, kind: AddKind, rows: &[(TypeId, u32)]) -> String {
    rows.iter()
        .map(|&(t, n)| match kind {
            AddKind::Drones | AddKind::Cargo => format!("{} x{n}", catalog.name(t)),
            AddKind::Implants | AddKind::Boosters => catalog.name(t),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rows of a tab as text for the clipboard; drones bring their mutations along, as in pyfa.
fn copy_text(catalog: &Catalog, fit: &Fit, kind: AddKind, picked: &[usize]) -> String {
    if kind == AddKind::Drones {
        let stacks: Vec<_> = picked
            .iter()
            .filter_map(|&i| fit.drones.get(i).cloned())
            .collect();
        return crate::port::eft::export_drones(catalog, &stacks);
    }
    let rows = kind.rows(fit);
    let picked: Vec<(TypeId, u32)> = picked
        .iter()
        .filter_map(|&r| rows.get(r).copied())
        .collect();
    export_lines(catalog, kind, &picked)
}

/// Items from pasted lines that belong in the tab; anything else is skipped.
fn parse_lines(catalog: &Catalog, kind: AddKind, text: &str) -> Vec<(TypeId, u32)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('['))
        .filter_map(|line| {
            let (name, count) = split_count(line);
            let t = catalog.find(name)?;
            kind.accepts(t.kind).then_some((t.id, count.unwrap_or(1)))
        })
        .collect()
}

impl Efa {
    pub(super) fn additions(
        &mut self,
        loaded: &Rc<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(current) = self.saved.current else {
            return div().into_any_element();
        };
        let fit = &self.saved.fits[current];
        // pyfa greys out the tabs a structure has no use for.
        let structure = fit
            .ship
            .and_then(|s| loaded.catalog.get(s))
            .is_some_and(ItemType::is_structure);
        let enabled = move |tab: AdditionsTab| !(structure && tab.ship_only());
        if !enabled(self.additions_tab) {
            self.additions_tab = AdditionsTab::Fighters;
        }
        // More in the hold than it takes, implants and boosters included when they count.
        let overfull = self
            .stats
            .as_ref()
            .and_then(|s| s.cargo_capacity)
            .is_some_and(|capacity| {
                let used = fit.hold_volume(
                    &loaded.catalog,
                    self.saved.preferences.general.implants_in_cargo,
                );
                used > capacity + 1e-6
            });
        let theme = cx.theme();
        let labels = self.saved.preferences.general.additions_labels;
        let tabs = AdditionsTab::ORDER.map(|tab| {
            (
                tab,
                tab.label(),
                tab_count(fit, tab, labels, self.character_implants()),
            )
        });
        // The tabs scroll sideways when the drawer is too narrow for them all.
        let tab_bar = h_flex()
            .id("additions-tabs")
            .flex_none()
            .overflow_x_scroll()
            .gap_1()
            .p_1()
            .bg(theme.tab_bar)
            .border_t_1()
            .border_b_1()
            .border_color(theme.border)
            .children(tabs.into_iter().map(|(tab, label, count)| {
                let selected = self.additions_tab == tab;
                let on = enabled(tab);
                let text = if tab == AdditionsTab::Notes {
                    if labels == AdditionsLabels::None {
                        label.to_owned()
                    } else {
                        notes::tab_title(fit)
                    }
                } else if count > 0 {
                    format!("{label} ({count})")
                } else {
                    label.to_owned()
                };
                div()
                    .id(label)
                    .flex_none()
                    .whitespace_nowrap()
                    .px_3()
                    .py_0p5()
                    .rounded(px(4.))
                    .when(selected, |t| {
                        t.bg(theme.tab_active)
                            .text_color(theme.tab_active_foreground)
                    })
                    .when(!selected, |t| t.text_color(theme.tab_foreground))
                    .when(!on, |t| t.text_color(theme.muted_foreground).opacity(0.5))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(text)
                    .when(tab == AdditionsTab::Cargo && overfull, |t| {
                        t.child(
                            gpui_kit::component::Icon::new(
                                gpui_kit::component::IconName::TriangleAlert,
                            )
                            .size(px(13.))
                            .text_color(theme.danger),
                        )
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new("The cargo hold is overfull")
                                .build(window, cx)
                        })
                    })
                    .when(on, |t| {
                        // Tabs switch on press, as desktop tab bars do.
                        t.on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                                this.show_additions_tab(tab);
                                cx.notify();
                            }),
                        )
                    })
            }));

        let content = match self.additions_tab {
            AdditionsTab::Drones => self.drones_tab(loaded, window, cx),
            AdditionsTab::Implants => self.implants_tab(loaded, cx),
            AdditionsTab::Boosters => self.boosters_tab(loaded, cx),
            AdditionsTab::Cargo => self.cargo_tab(loaded, cx),
            AdditionsTab::Notes => self.notes_tab(loaded, cx),
            AdditionsTab::Fighters => self.fighters_tab(loaded, cx),
            AdditionsTab::Projected => self.projected_tab(loaded, cx),
            AdditionsTab::Command => self.command_tab(loaded, cx),
        };
        v_flex()
            .size_full()
            .child(tab_bar)
            .child(content)
            .into_any_element()
    }

    /// Bring an additions tab forward. Its selection goes when the tab changes, since it holds
    /// another tab's rows.
    pub(in crate::ui) fn show_additions_tab(&mut self, tab: AdditionsTab) {
        if self.additions_tab != tab {
            self.additions_tab = tab;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
    }

    fn select_addition(&mut self, i: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        if modifiers.shift
            && let Some(anchor) = self.additions_anchor
        {
            self.additions_selection = (anchor.min(i)..=anchor.max(i)).collect();
        } else if modifiers.control {
            if let Some(at) = self.additions_selection.iter().position(|&r| r == i) {
                self.additions_selection.remove(at);
            } else {
                self.additions_selection.push(i);
            }
            self.additions_anchor = Some(i);
        } else {
            self.additions_selection = vec![i];
            self.additions_anchor = Some(i);
        }
        cx.notify();
    }

    /// The selection when it includes the clicked row, the clicked row otherwise.
    fn addition_targets(&self, clicked: usize) -> Vec<usize> {
        if self.additions_selection.contains(&clicked) {
            self.additions_selection.clone()
        } else {
            vec![clicked]
        }
    }

    /// pyfa's list keys (implantView.py, boosterView.py kbEvent): Delete removes the selection
    /// when the rows are `removable` as `kind`, Ctrl+A selects all `len` rows, Escape clears.
    fn list_key(
        &mut self,
        event: &KeyDownEvent,
        kind: AddKind,
        len: usize,
        removable: bool,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let plain = !(m.control || m.alt || m.shift || m.platform);
        match keystroke.key.as_str() {
            "delete" if plain && removable && !self.additions_selection.is_empty() => {
                let rows = std::mem::take(&mut self.additions_selection);
                self.additions_anchor = None;
                self.apply(Action::RemoveAdditions(kind, rows), cx);
            }
            "a" if m.control && !m.alt && !m.shift => {
                self.additions_selection = (0..len).collect();
                cx.notify();
            }
            "escape" if plain => {
                self.additions_selection.clear();
                self.additions_anchor = None;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Ask for a value and turn it into an action. Input that does not parse leaves the dialog
    /// open.
    pub(super) fn open_value_dialog<T: std::str::FromStr + 'static>(
        title: &'static str,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl Fn(T) -> Action + 'static,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let this = cx.entity().downgrade();
        let action = Rc::new(action);
        window.open_dialog(cx, move |dialog, _, _| {
            let (input, this, action) = (input.clone(), this.clone(), Rc::clone(&action));
            // The OK button and Enter both submit; it closes the dialog itself when done.
            let submit: crate::ui::widgets::Submit = {
                let (input, this, action) = (input.clone(), this.clone(), action.clone());
                Rc::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let Ok(amount) = input.read(cx).value().trim().parse::<T>() else {
                        return;
                    };
                    Self::app_update(&this, cx, |g, cx| g.apply(action(amount), cx));
                    window.close_dialog(cx);
                })
            };
            dialog
                .on_ok({
                    let submit = Rc::clone(&submit);
                    move |e, w, cx| {
                        submit(e, w, cx);
                        false
                    }
                })
                .title(title)
                .w(px(320.))
                .child(v_flex().gap_3().child(Input::new(&input)).child(
                    h_flex().justify_end().child(
                        Button::new("amount-ok").primary().label("OK").on_click({
                            let submit = Rc::clone(&submit);
                            move |e: &ClickEvent, w: &mut Window, cx: &mut App| submit(e, w, cx)
                        }),
                    ),
                ))
        });
    }
}

/// pyfa's droneItem, cargoItem, implantItem and boosterItem menus, then the tab's misc entries.
fn addition_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    kind: AddKind,
    i: usize,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let Some((type_id, amount)) = this
        .upgrade()
        .and_then(|g| g.read(cx).fit().map(|f| kind.rows(f)))
        .and_then(|rows| rows.get(i).copied())
    else {
        return menu;
    };
    let Some(t) = catalog.get(type_id) else {
        return menu;
    };
    let category = t.category_label();
    let mutation = (kind == AddKind::Drones)
        .then(|| {
            this.upgrade()
                .and_then(|g| g.read(cx).fit()?.drones.get(i)?.mutation.clone())
        })
        .flatten();
    let targeted = |make: fn(Vec<usize>) -> Action| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Efa::app_update(&this, cx, |g, cx| {
                let targets = g.addition_targets(i);
                g.apply(make(targets), cx);
            });
        }
    };

    let stats_of = match kind {
        AddKind::Drones => StatsOf::Fitted(FitItem::Drone(i)),
        AddKind::Implants => StatsOf::Fitted(FitItem::Implant(type_id)),
        AddKind::Boosters => StatsOf::Fitted(FitItem::Booster(i)),
        AddKind::Cargo => StatsOf::Type(type_id),
    };
    let mut menu = menu.item(stats_entry(this, category, stats_of));
    // A mutated drone has no market group of its own; pyfa jumps to its base drone's.
    let market_type = mutation.as_ref().map_or(type_id, |m| m.base);
    if catalog
        .get(market_type)
        .and_then(|t| t.market_group)
        .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Search, format!("{category} Market Group")).on_click(
                move |_, window, cx| {
                    Efa::app_update(&this, cx, |g, cx| {
                        g.jump_to_market(market_type, window, cx);
                    });
                },
            ),
        );
    }
    let remove_label = match kind {
        AddKind::Drones => "Remove Drone Stack".to_owned(),
        AddKind::Cargo => format!("Remove {category} Stack"),
        AddKind::Implants | AddKind::Boosters => format!("Remove {category}"),
    };
    menu = menu.item(entry(IconName::Delete, remove_label).on_click(match kind {
        AddKind::Drones => targeted(|rows| Action::RemoveAdditions(AddKind::Drones, rows)),
        AddKind::Implants => targeted(|rows| Action::RemoveAdditions(AddKind::Implants, rows)),
        AddKind::Boosters => targeted(|rows| Action::RemoveAdditions(AddKind::Boosters, rows)),
        AddKind::Cargo => targeted(|rows| Action::RemoveAdditions(AddKind::Cargo, rows)),
    }));
    if matches!(kind, AddKind::Drones | AddKind::Cargo) {
        let label = if kind == AddKind::Drones {
            "Change Drone Quantity"
        } else {
            "Change Selection Quantity"
        };
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Settings2, label).on_click(move |_, window, cx| {
                Efa::app_update(&this, cx, |g, cx| {
                    let targets = g.addition_targets(i);
                    Efa::open_value_dialog(
                        "New Amount",
                        amount.to_string(),
                        window,
                        cx,
                        move |n| Action::SetAmount(kind, targets.clone(), n),
                    );
                });
            }),
        );
    }
    if kind == AddKind::Drones && amount > 1 {
        let this = this.clone();
        menu = menu.item(entry(IconName::Minus, "Split Drone Stack").on_click(
            move |_, window, cx| {
                Efa::app_update(&this, cx, |_, cx| {
                    Efa::open_value_dialog(
                        "Split Off",
                        (amount / 2).to_string(),
                        window,
                        cx,
                        move |n| Action::SplitDrones(i, n),
                    );
                });
            },
        ));
    }
    let variations = catalog.variations_of(type_id);
    if variations.len() > 1 && super::hub::shown_menus(window, cx).variations {
        let entries: Vec<(TypeId, String)> =
            variations.iter().map(|&v| (v, catalog.name(v))).collect();
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.submenu_with_element(
            "Variations",
            icon_label(IconName::Replace, "Variations"),
            window,
            cx,
            move |menu, window, _| {
                entries.iter().fold(
                    menu.scrollable(true)
                        .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H)),
                    |menu, (v, name)| {
                        let (this, v) = (this.clone(), *v);
                        menu.item(
                            item_entry(&loaded, v, name.clone())
                                .checked(v == type_id)
                                .on_click(move |_, _, cx| {
                                    Efa::app_update(&this, cx, |g, cx| {
                                        let targets = g.addition_targets(i);
                                        g.apply(Action::SwapAdditions(kind, targets, v), cx);
                                    });
                                }),
                        )
                    },
                )
            },
        );
    }
    if kind == AddKind::Drones {
        let target = super::actions::MutaTarget::Drones(i);
        menu = super::mutations::mutation_entries(
            menu,
            this,
            loaded,
            type_id,
            mutation.as_ref(),
            &target,
            window,
            cx,
        );
    }
    if kind == AddKind::Drones
        && let Some(target) = super::characters::stack_target(loaded, market_type)
    {
        menu = super::characters::skills_submenu(menu, this, loaded, target, window, cx);
    }
    if kind == AddKind::Cargo && super::hub::shown_menus(window, cx).cargo_fill {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Inbox, format!("Fill Cargo With {category}")).on_click(
                move |_, _, cx| {
                    Efa::app_update(&this, cx, |g, cx| {
                        let Some(loaded) = g.loaded() else { return };
                        let room = g.cargo_room_for(&loaded, type_id);
                        if room > 0 {
                            g.apply(Action::AddCargo(type_id, room), cx);
                        }
                    });
                },
            ),
        );
    }
    if kind == AddKind::Cargo {
        menu = super::cargo_presets::menu_entries(menu.separator(), this, window, cx);
    }
    misc_entries(menu.separator(), this, loaded, kind, Some(i), window, cx)
}

/// pyfa's *`ItemMisc` entries: copy the selection or the whole tab, paste into it.
fn misc_entries(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    kind: AddKind,
    clicked: Option<usize>,
    window: &Window,
    cx: &App,
) -> PopupMenu {
    if !super::hub::shown_menus(window, cx).additions_copy_paste {
        return menu;
    }
    let noun = kind.noun();
    let has_rows = this
        .upgrade()
        .and_then(|g| g.read(cx).fit().map(|f| !kind.rows(f).is_empty()))
        .unwrap_or(false);
    let can_paste = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .is_some_and(|text| !text.trim().is_empty());
    let mut menu = menu;
    if let Some(i) = clicked {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.item(
            entry(IconName::Copy, format!("Copy Selected {noun}")).on_click(move |_, _, cx| {
                Efa::app_update(&this, cx, |g, cx| {
                    let Some(fit) = g.fit() else { return };
                    let picked = g.addition_targets(i);
                    let text = copy_text(&loaded.catalog, fit, kind, &picked);
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                });
            }),
        );
    }
    if has_rows {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.item(entry(IconName::Copy, format!("Copy All {noun}")).on_click(
            move |_, _, cx| {
                Efa::app_update(&this, cx, |g, cx| {
                    let Some(fit) = g.fit() else { return };
                    let all: Vec<usize> = (0..kind.rows(fit).len()).collect();
                    let text = copy_text(&loaded.catalog, fit, kind, &all);
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                });
            },
        ));
    }
    if can_paste {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.item(entry(IconName::FileText, format!("Paste {noun}")).on_click(
            move |_, _, cx| {
                let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return;
                };
                if kind == AddKind::Drones {
                    let stacks = crate::port::eft::parse_drones(&loaded.catalog, &text);
                    if !stacks.is_empty() {
                        Efa::app_update(&this, cx, |g, cx| {
                            g.apply(Action::AddDroneStacks(stacks), cx);
                        });
                    }
                    return;
                }
                let items = parse_lines(&loaded.catalog, kind, &text);
                if !items.is_empty() {
                    Efa::app_update(&this, cx, |g, cx| {
                        g.apply(Action::AddAdditions(kind, items), cx);
                    });
                }
            },
        ));
    }
    menu
}
