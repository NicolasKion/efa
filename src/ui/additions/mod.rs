//! The additions pane under the fit: drones, implants, boosters, cargo and notes, with pyfa's
//! per-tab context menus. Shared pieces live here; each tab has its own module.

mod boosters;
mod cargo;
mod drones;
mod implants;
mod notes;

pub(super) use cargo::{drop_transfer, goes_to_cargo};

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::{ContextMenuExt, PopupMenu},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Div, InteractiveElement,
    IntoElement, Modifiers, MouseButton, MouseDownEvent, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{
    Action, AddKind, AdditionsTab, Gyfa, Loaded,
    widgets::{entry, icon_box, icon_label, item_entry},
};
use crate::{
    data::{Catalog, Kind, TypeId},
    fit::{Fit, split_count},
    icons::Icon,
};

const ROW_HEIGHT: f32 = 28.;
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
            Self::Boosters => fit.boosters.iter().map(|&t| (t, 1)).collect(),
            Self::Cargo => fit.cargo.clone(),
        }
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

impl Gyfa {
    pub(super) fn additions(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(current) = self.saved.current else {
            return div().into_any_element();
        };
        let fit = &self.saved.fits[current];
        let theme = cx.theme();
        let tabs = [
            (AdditionsTab::Drones, "Drones", fit.drones.len()),
            (AdditionsTab::Implants, "Implants", fit.implants.len()),
            (AdditionsTab::Boosters, "Boosters", fit.boosters.len()),
            (AdditionsTab::Cargo, "Cargo", fit.cargo.len()),
            (AdditionsTab::Notes, "Notes", 0),
        ];
        let tab_bar = h_flex()
            .gap_1()
            .p_1()
            .bg(theme.tab_bar)
            .border_t_1()
            .border_b_1()
            .border_color(theme.border)
            .children(tabs.into_iter().map(|(tab, label, count)| {
                let selected = self.additions_tab == tab;
                let text = if count > 0 {
                    format!("{label} ({count})")
                } else {
                    label.to_owned()
                };
                div()
                    .id(label)
                    .px_3()
                    .py_0p5()
                    .rounded(px(4.))
                    .when(selected, |t| {
                        t.bg(theme.tab_active)
                            .text_color(theme.tab_active_foreground)
                    })
                    .when(!selected, |t| t.text_color(theme.tab_foreground))
                    .child(text)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.additions_tab = tab;
                        this.additions_selection.clear();
                        this.additions_anchor = None;
                        cx.notify();
                    }))
            }));

        let content = match self.additions_tab {
            AdditionsTab::Drones => self.drones_tab(loaded, cx),
            AdditionsTab::Implants => self.implants_tab(loaded, cx),
            AdditionsTab::Boosters => self.boosters_tab(loaded, cx),
            AdditionsTab::Cargo => self.cargo_tab(loaded, cx),
            AdditionsTab::Notes => self.notes_tab(loaded, cx),
        };
        v_flex()
            .size_full()
            .child(tab_bar)
            .child(content)
            .into_any_element()
    }

    /// A tab's rows as a scrolling list, with `before` above them. The shared layout the item
    /// tabs start from.
    pub(super) fn item_list(
        &mut self,
        loaded: &Rc<Loaded>,
        kind: AddKind,
        before: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(current) = self.saved.current else {
            return div().into_any_element();
        };
        let fit = &self.saved.fits[current];
        let theme = cx.theme();
        let rows = kind.rows(fit);
        let mut content = before;
        if rows.is_empty() {
            content.push(
                div()
                    .id("additions-empty")
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child(match kind {
                        AddKind::Drones => "Double-click drones in the market to add them.",
                        AddKind::Implants => "Double-click implants in the market to add them.",
                        AddKind::Boosters => "Double-click boosters in the market to add them.",
                        AddKind::Cargo => "Right-click items in the market to put them in cargo.",
                    })
                    .context_menu({
                        let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                        move |menu, _, cx| misc_entries(menu, &this, &loaded, kind, None, cx)
                    })
                    .into_any_element(),
            );
        }
        for (i, &(type_id, amount)) in rows.iter().enumerate() {
            let row = self.addition_row(loaded, fit, kind, i, type_id, amount, cx);
            content.push(row);
        }
        v_flex()
            .id("additions-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(content)
            .into_any_element()
    }

    #[expect(clippy::too_many_arguments, reason = "one row needs all of these")]
    fn addition_row(
        &self,
        loaded: &Rc<Loaded>,
        fit: &Fit,
        kind: AddKind,
        i: usize,
        type_id: TypeId,
        amount: u32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let selected = self.additions_selection.contains(&i);
        let mut row: Stateful<Div> = h_flex()
            .id(SharedString::from(format!("addition-{i}")))
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .when(i % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover));
        if kind == AddKind::Drones {
            let (active, count) = (fit.drones[i].active, fit.drones[i].count);
            row = row
                .child(
                    Button::new(SharedString::from(format!("drone-less-{i}")))
                        .ghost()
                        .xsmall()
                        .label("−")
                        .disabled(active == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.apply(Action::SetDronesActive(i, active.saturating_sub(1)), cx);
                        })),
                )
                .child(div().w(px(36.)).child(format!("{active}/{count}")))
                .child(
                    Button::new(SharedString::from(format!("drone-more-{i}")))
                        .ghost()
                        .xsmall()
                        .label("+")
                        .disabled(active >= count)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.apply(Action::SetDronesActive(i, active + 1), cx);
                        })),
                );
        }
        let label = match kind {
            AddKind::Cargo => format!("{} x{amount}", catalog.name(type_id)),
            AddKind::Drones | AddKind::Implants | AddKind::Boosters => catalog.name(type_id),
        };
        row.child(icon_box(catalog, &loaded.icons, Icon::Type(type_id), 22.))
            .child(div().flex_1().truncate().child(label))
            .on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    g.apply(Action::RemoveAdditions(kind, vec![i]), cx);
                } else {
                    g.select_addition(i, event.modifiers(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |g, _: &MouseDownEvent, _, cx| {
                    if !g.additions_selection.contains(&i) {
                        g.additions_selection = vec![i];
                        g.additions_anchor = Some(i);
                        cx.notify();
                    }
                }),
            )
            .context_menu({
                let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                move |menu, window, cx| addition_menu(menu, &this, &loaded, kind, i, window, cx)
            })
            .into_any_element()
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
            dialog
                .title(title)
                .w(px(320.))
                .child(v_flex().gap_3().child(Input::new(&input)).child(
                    h_flex().justify_end().child(
                        Button::new("amount-ok").primary().label("OK").on_click(
                            move |_, window, cx| {
                                let Ok(amount) = input.read(cx).value().trim().parse::<T>() else {
                                    return;
                                };
                                Self::app_update(&this, cx, |g, cx| g.apply(action(amount), cx));
                                window.close_dialog(cx);
                            },
                        ),
                    ),
                ))
        });
    }
}

/// pyfa's droneItem, cargoItem, implantItem and boosterItem menus, then the tab's misc entries.
fn addition_menu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
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
    let targeted = |make: fn(Vec<usize>) -> Action| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| {
                let targets = g.addition_targets(i);
                g.apply(make(targets), cx);
            });
        }
    };

    let mut menu = menu;
    if t.market_group
        .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Search, format!("{category} Market Group")).on_click(
                move |_, window, cx| {
                    Gyfa::app_update(&this, cx, |g, cx| g.jump_to_market(type_id, window, cx));
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
                Gyfa::app_update(&this, cx, |g, cx| {
                    let targets = g.addition_targets(i);
                    Gyfa::open_value_dialog(
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
                Gyfa::app_update(&this, cx, |_, cx| {
                    Gyfa::open_value_dialog(
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
    if variations.len() > 1 {
        let entries: Vec<(TypeId, String)> =
            variations.iter().map(|&v| (v, catalog.name(v))).collect();
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.submenu_with_element(
            "Variations",
            icon_label(IconName::Replace, "Variations"),
            window,
            cx,
            move |menu, _, _| {
                entries.iter().fold(
                    menu.scrollable(true).max_h(px(MENU_MAX_H)),
                    |menu, (v, name)| {
                        let (this, v) = (this.clone(), *v);
                        menu.item(
                            item_entry(&loaded, v, name.clone())
                                .checked(v == type_id)
                                .on_click(move |_, _, cx| {
                                    Gyfa::app_update(&this, cx, |g, cx| {
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
    if kind == AddKind::Cargo {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Inbox, format!("Fill Cargo With {category}")).on_click(
                move |_, _, cx| {
                    Gyfa::app_update(&this, cx, |g, cx| {
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
    misc_entries(menu.separator(), this, loaded, kind, Some(i), cx)
}

/// pyfa's *`ItemMisc` entries: copy the selection or the whole tab, paste into it.
fn misc_entries(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    kind: AddKind,
    clicked: Option<usize>,
    cx: &App,
) -> PopupMenu {
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
                Gyfa::app_update(&this, cx, |g, cx| {
                    let Some(fit) = g.fit() else { return };
                    let rows = kind.rows(fit);
                    let picked: Vec<(TypeId, u32)> = g
                        .addition_targets(i)
                        .into_iter()
                        .filter_map(|r| rows.get(r).copied())
                        .collect();
                    let text = export_lines(&loaded.catalog, kind, &picked);
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                });
            }),
        );
    }
    if has_rows {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.item(entry(IconName::Copy, format!("Copy All {noun}")).on_click(
            move |_, _, cx| {
                Gyfa::app_update(&this, cx, |g, cx| {
                    let Some(fit) = g.fit() else { return };
                    let text = export_lines(&loaded.catalog, kind, &kind.rows(fit));
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
                let items = parse_lines(&loaded.catalog, kind, &text);
                if !items.is_empty() {
                    Gyfa::app_update(&this, cx, |g, cx| {
                        g.apply(Action::AddAdditions(kind, items), cx);
                    });
                }
            },
        ));
    }
    menu
}
