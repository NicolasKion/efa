//! The Fighters tab, pyfa's fighter pane: squadrons by class, a state column that launches and
//! recalls them, their speed, EHP, regeneration and active abilities, and the light, heavy and
//! support tubes underneath.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{MENU_MAX_H, PRICE_W, ROW_HEIGHT, drones::format_amount, price_text};
use crate::{
    data::{Catalog, Kind, TypeId, projection::FighterClass},
    dogma::attr as drone_attr,
    engine::{FighterInfo, ItemRef, Slots, Stats},
    fit::{
        Fit,
        fighters::{self, Fighter},
        split_count,
    },
    icons::Icon,
    ui::{
        Efa, Loaded,
        columns::{ICON_W, STATE_W},
        fitting::{DragSource, Dragged},
        validation::flag,
        widgets::{color, entry, icon, icon_box, icon_label, item_entry},
    },
};

const NUMBER_W: f32 = 64.;
const SPEED_ICON: i32 = 1389;
const HEADER_ICON: f32 = 18.;
const FOOTER_H: f32 = 24.;

/// Squadron indices in display order: by class, then by abilities for the fighting classes,
/// then by name, as pyfa's `fighterKey` sorts them.
fn display_order(catalog: &Catalog, fighters: &[Fighter], stats: Option<&Stats>) -> Vec<usize> {
    let key = |i: usize| {
        let fighter = &fighters[i];
        let group = catalog
            .get(fighter.type_id)
            .and_then(|t| catalog.group_names.get(&t.group_id))
            .map_or("", String::as_str);
        let rank = fighters::group_rank(catalog, fighter.type_id);
        let mut abilities: Vec<i32> = if group == "Support Fighter" {
            Vec::new()
        } else {
            stats
                .and_then(|s| s.fighters.get(i))
                .map_or_default(|info| info.abilities.iter().map(|a| a.id).collect())
        };
        abilities.sort_unstable();
        (rank, abilities, catalog.name(fighter.type_id))
    };
    let mut order: Vec<usize> = (0..fighters.len()).collect();
    order.sort_by_cached_key(|&i| key(i));
    order
}

/// "Name xN" lines, the way pyfa copies fighters.
fn export_lines(
    catalog: &Catalog,
    fighters: &[&Fighter],
    stats: &[Option<&FighterInfo>],
) -> String {
    fighters
        .iter()
        .zip(stats)
        .map(|(f, info)| {
            let size = f
                .amount
                .or_else(|| info.and_then(FighterInfo::max_size))
                .or_else(|| catalog.projection.squadron_size(f.type_id))
                .unwrap_or(1);
            format!("{} x{size}", catalog.name(f.type_id))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fighters from pasted lines; anything that is not a fighter is skipped.
pub(in crate::ui) fn parse_lines(catalog: &Catalog, text: &str) -> Vec<Fighter> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('['))
        .filter_map(|line| {
            let (name, count) = split_count(line);
            let t = catalog.find(name).filter(|t| t.kind == Kind::Fighter)?;
            Some(count.map_or_else(
                || Fighter::new(t.id),
                |n| Fighter::with_size(catalog, t.id, n),
            ))
        })
        .collect()
}

/// pyfa's Base Name for a squadron: "6/9 Templar II".
fn name_text(catalog: &Catalog, fighter: &Fighter, info: Option<&FighterInfo>) -> String {
    let max = info
        .and_then(FighterInfo::max_size)
        .or_else(|| catalog.projection.squadron_size(fighter.type_id));
    let amount = fighter.amount.or(max).unwrap_or(0);
    match max {
        Some(max) => format!("{amount}/{max} {}", catalog.name(fighter.type_id)),
        None => format!("{amount} {}", catalog.name(fighter.type_id)),
    }
}

/// pyfa's Active Abilities column.
fn abilities_text(catalog: &Catalog, info: Option<&FighterInfo>) -> String {
    let active: Vec<String> = info
        .into_iter()
        .flat_map(|i| &i.abilities)
        .filter(|a| a.active)
        .map(|a| catalog.projection.ability_name(a.id))
        .collect();
    if active.is_empty() {
        "None".to_owned()
    } else {
        active.join(", ")
    }
}

impl Efa {
    fn fighter_order(&self, loaded: &Loaded) -> Vec<usize> {
        self.fit().map_or_default(|fit| {
            display_order(&loaded.catalog, &fit.fighters, self.stats.as_ref())
        })
    }

    /// The squadrons, by index in the fit, a click on display row `n` acts on.
    fn fighter_targets(&self, loaded: &Loaded, n: usize) -> Vec<usize> {
        let order = self.fighter_order(loaded);
        self.addition_targets(n)
            .into_iter()
            .filter_map(|r| order.get(r).copied())
            .collect()
    }

    /// Every squadron of the clicked one's type, for Ctrl and Alt.
    fn similar_fighter_targets(&self, loaded: &Loaded, n: usize) -> Vec<usize> {
        let order = self.fighter_order(loaded);
        let (Some(fit), Some(&i)) = (self.fit(), order.get(n)) else {
            return Vec::new();
        };
        Fit::similar_fighters(&fit.fighters, i)
    }

    /// Add squadrons the way pyfa does: refused when the ship has no tubes of their class,
    /// launched when one is free.
    pub(in crate::ui) fn add_fighter_squadrons(
        &mut self,
        fighters: Vec<Fighter>,
        cx: &mut Context<Self>,
    ) {
        if fighters.is_empty() {
            return;
        }
        let stats = self.stats.clone().unwrap_or_default();
        let mut refused = None;
        self.edit_current(cx, |fit, catalog| {
            let mut tubes = stats.fighter_tubes;
            let mut classes = stats.fighter_classes;
            for mut fighter in fighters {
                let class = match catalog.projection.fighter_class(fighter.type_id) {
                    Some(FighterClass::Light) => &mut classes.light,
                    Some(FighterClass::Heavy) => &mut classes.heavy,
                    Some(FighterClass::Support) => &mut classes.support,
                    None => continue,
                };
                if class.total.unwrap_or(0) == 0 {
                    refused = Some(catalog.name(fighter.type_id));
                    continue;
                }
                let room = |s: &Slots| s.used < s.total.unwrap_or(0);
                fighter.active = room(&tubes) && room(class);
                if fighter.active {
                    tubes.used += 1;
                    class.used += 1;
                }
                fit.fighters.push(fighter);
            }
            true
        });
        match refused {
            Some(name) => self.set_error(format!("Nothing on this fit takes {name}")),
            // pyfa brings the Fighters tab forward when a squadron goes in.
            None => self.show_additions_tab(super::super::AdditionsTab::Fighters),
        }
    }

    fn remove_fighters(&mut self, rows: Vec<usize>, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            let mut rows = rows;
            rows.sort_unstable();
            rows.dedup();
            for i in rows.into_iter().rev() {
                if i < fit.fighters.len() {
                    fit.fighters.remove(i);
                }
            }
            true
        });
    }

    /// pyfa's ability toggle: the clicked squadron's ability flips, and every target with the
    /// same ability follows it.
    fn toggle_fighter_ability(
        &mut self,
        main: usize,
        rows: Vec<usize>,
        ability: i32,
        cx: &mut Context<Self>,
    ) {
        let Some(computed) = self.stats.clone() else {
            return;
        };
        let state = |i: usize| {
            computed
                .fighters
                .get(i)
                .and_then(|info| info.abilities.iter().find(|a| a.id == ability))
                .map(|a| a.active)
        };
        let Some(on) = state(main).map(|active| !active) else {
            return;
        };
        self.edit_current(cx, |fit, _| {
            for i in rows.into_iter().chain([main]) {
                if state(i).is_some()
                    && let Some(fighter) = fit.fighters.get_mut(i)
                {
                    fighter.set_ability(ability, on);
                }
            }
            false
        });
    }

    fn set_fighter_amount(&mut self, rows: Vec<usize>, amount: u32, cx: &mut Context<Self>) {
        let maxes: Vec<Option<u32>> = rows
            .iter()
            .map(|&i| {
                self.stats
                    .as_ref()
                    .and_then(|s| s.fighters.get(i))
                    .and_then(FighterInfo::max_size)
            })
            .collect();
        if amount == 0 {
            self.remove_fighters(rows, cx);
            return;
        }
        self.edit_current(cx, |fit, catalog| {
            for (i, max) in rows.into_iter().zip(maxes) {
                if let Some(fighter) = fit.fighters.get_mut(i) {
                    let max = max.or_else(|| catalog.projection.squadron_size(fighter.type_id));
                    fighter.set_amount(amount, max);
                }
            }
            false
        });
    }

    fn swap_fighters(&mut self, rows: Vec<usize>, type_id: TypeId, cx: &mut Context<Self>) {
        self.edit_current(cx, |fit, _| {
            for i in rows {
                if let Some(fighter) = fit.fighters.get_mut(i) {
                    fighter.type_id = type_id;
                }
            }
            false
        });
    }

    pub(in crate::ui) fn fighters_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let catalog = &loaded.catalog;
        let order = display_order(catalog, &fit.fighters, self.stats.as_ref());
        let theme = cx.theme();
        let mut rows: Vec<AnyElement> = Vec::with_capacity(order.len() + 1);
        if order.is_empty() {
            rows.push(
                div()
                    .id("fighters-empty")
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child("Double-click fighters in the market to add them.")
                    .context_menu({
                        let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
                        move |menu, _, cx| fighter_misc(menu, &this, &loaded, None, cx)
                    })
                    .into_any_element(),
            );
        }
        for (n, &i) in order.iter().enumerate() {
            rows.push(self.fighter_row(loaded, n, i, cx));
        }
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_h_0()
            .bg(theme.background)
            .child(Self::fighter_header(loaded, cx))
            .child(
                v_flex()
                    .id("fighters-list")
                    .track_focus(&self.additions_focus)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| {
                        g.fighter_key(event, cx);
                    }))
                    .drag_over::<Dragged>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(|g, dragged: &Dragged, _, cx| {
                        g.drop_on_fighters(dragged, cx);
                    })),
            )
            .child(self.fighter_footer(cx))
            .into_any_element()
    }

    fn fighter_header(loaded: &Loaded, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let header_icon = |id: i32, tip: &'static str| {
            div()
                .id(SharedString::from(format!("fighter-header-{id}")))
                .w(px(NUMBER_W))
                .flex_none()
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .child(icon_box(
                    &loaded.catalog,
                    &loaded.icons,
                    Icon::Id(id),
                    HEADER_ICON,
                ))
        };
        h_flex()
            .h(px(24.))
            .px_1()
            .gap_1()
            .flex_none()
            .bg(theme.table_head)
            .text_color(theme.table_head_foreground)
            .child(div().w(px(STATE_W + ICON_W)).flex_none())
            .child(div().flex_1().min_w_0().child("Name"))
            .child(header_icon(SPEED_ICON, "Maximum Velocity"))
            .child(header_icon(icon::SHIELD, "Effective HP"))
            .child(header_icon(icon::RECHARGE, "Effective Shield Regeneration"))
            .child(div().flex_1().min_w_0().child("Active Abilities"))
            .child(div().w(px(PRICE_W)).flex_none().text_right().child("Price"))
            .into_any_element()
    }

    /// pyfa's tube counts under the list, in the warning colour once a class is overfilled.
    fn fighter_footer(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let classes = self.stats.as_ref().map_or_default(|s| s.fighter_classes);
        let part = |label: &'static str, slots: Slots| {
            let total = slots.total.unwrap_or(0);
            h_flex()
                .gap_1()
                .when(slots.used > total, |d| d.text_color(theme.danger))
                .child(label)
                .child(format!("{}/{total}", slots.used))
        };
        h_flex()
            .h(px(FOOTER_H))
            .flex_none()
            .justify_around()
            .border_t_1()
            .border_color(theme.border)
            .child(part("Light", classes.light))
            .child(part("Heavy", classes.heavy))
            .child(part("Support", classes.support))
            .into_any_element()
    }

    fn fighter_row(
        &self,
        loaded: &Rc<Loaded>,
        n: usize,
        i: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let Some(fighter) = self.fit().and_then(|f| f.fighters.get(i)) else {
            return div().into_any_element();
        };
        let info = self.stats.as_ref().and_then(|s| s.fighters.get(i));
        let selected = self.additions_selection.contains(&n);
        let (mark, mark_color) = if fighter.active {
            ("✓", self.palette.green)
        } else {
            ("○", self.palette.muted)
        };
        let number =
            |text: Option<String>| div().w(px(NUMBER_W)).flex_none().truncate().children(text);
        let speed = info
            .and_then(|i| i.stats.attr(drone_attr::MAX_VELOCITY))
            .map(|v| format_amount(v, 3, false));
        let ehp = info
            .and_then(|i| i.stats.ehp)
            .map(|v| format_amount(v, 9, false));
        let regen = info
            .and_then(|i| i.stats.shield_regen)
            .map(|v| format!("{}/s", format_amount(v, 9, false)));
        let type_id = fighter.type_id;
        let squadron = fighter
            .amount
            .or_else(|| catalog.projection.squadron_size(type_id))
            .unwrap_or(1);
        let price = price_text(&self.prices, type_id, squadron);
        h_flex()
            .id(SharedString::from(format!("fighter-{n}")))
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_1()
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .when_some(self.problems_of(loaded, ItemRef::Fighter(i)), |row, why| {
                flag(row, why, theme.danger)
            })
            .child(
                div()
                    .id(SharedString::from(format!("fighter-state-{n}")))
                    .w(px(STATE_W))
                    .flex_none()
                    .text_color(color(mark_color))
                    .child(mark)
                    .on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        let Some(loaded) = g.loaded() else { return };
                        let rows = if event.modifiers().alt {
                            g.similar_fighter_targets(&loaded, n)
                        } else {
                            g.fighter_targets(&loaded, n)
                        };
                        g.edit_current(cx, |fit, _| {
                            fit.toggle_fighters(i, &rows);
                            false
                        });
                    })),
            )
            .child(div().w(px(ICON_W)).flex_none().child(icon_box(
                catalog,
                &loaded.icons,
                Icon::Type(type_id),
                22.,
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(name_text(catalog, fighter, info)),
            )
            .child(number(speed))
            .child(number(ehp))
            .child(number(regen))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(abilities_text(catalog, info)),
            )
            .child(
                div()
                    .w(px(PRICE_W))
                    .flex_none()
                    .truncate()
                    .text_right()
                    .children(price),
            )
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.additions_focus, cx);
                let Some(loaded) = g.loaded() else { return };
                if event.click_count() >= 2 {
                    let rows = if event.modifiers().alt {
                        g.similar_fighter_targets(&loaded, n)
                    } else {
                        vec![i]
                    };
                    g.remove_fighters(rows, cx);
                } else {
                    g.select_addition(n, event.modifiers(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |g, _: &MouseDownEvent, _, cx| {
                    if g.additions_selection.right_click(n) {
                        cx.notify();
                    }
                }),
            )
            .context_menu({
                let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
                move |menu, window, cx| {
                    let menu = fighter_menu(menu, &this, &loaded, n, i, window, cx);
                    fighter_misc(menu.separator(), &this, &loaded, Some(n), cx)
                }
            })
            .into_any_element()
    }

    /// pyfa's keys: Escape clears the selection, Ctrl+A selects every squadron, Delete removes.
    fn fighter_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let count = self.fit().map_or(0, |f| f.fighters.len());
        self.list_keys(
            event,
            count,
            true,
            |g, places, cx| {
                let Some(loaded) = g.loaded() else { return };
                let order = g.fighter_order(&loaded);
                let rows = places
                    .iter()
                    .filter_map(|&n| order.get(n).copied())
                    .collect();
                g.remove_fighters(rows, cx);
            },
            cx,
        );
    }

    /// A fighter from the market goes in like one double-clicked there.
    fn drop_on_fighters(&mut self, dragged: &Dragged, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let squadrons: Vec<_> = dragged
            .types()
            .filter(|&id| {
                loaded
                    .catalog
                    .get(id)
                    .is_some_and(|t| t.kind == Kind::Fighter)
            })
            .map(Fighter::new)
            .collect();
        if matches!(dragged.source, DragSource::Market) && !squadrons.is_empty() {
            self.add_fighter_squadrons(squadrons, cx);
        }
    }
}

/// pyfa's fighterItem menu.
fn fighter_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    n: usize,
    i: usize,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let Some((fighter, info)) = this.upgrade().and_then(|g| {
        let g = g.read(cx);
        let fighter = g.fit()?.fighters.get(i)?.clone();
        let info = g.stats.as_ref().and_then(|s| s.fighters.get(i)).cloned();
        Some((fighter, info))
    }) else {
        return menu;
    };
    let type_id = fighter.type_id;
    let mut menu = menu;
    let abilities = info.as_ref().map_or_default(|i| i.abilities.clone());
    if !abilities.is_empty() {
        let this = this.clone();
        let names: Vec<String> = abilities
            .iter()
            .map(|a| catalog.projection.ability_name(a.id))
            .collect();
        menu = menu.submenu_with_element(
            "Abilities",
            icon_label(IconName::Settings2, "Abilities"),
            window,
            cx,
            move |menu, _, _| {
                abilities
                    .iter()
                    .zip(&names)
                    .fold(menu, |menu, (ability, name)| {
                        let (this, id) = (this.clone(), ability.id);
                        menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(ability.active)
                                .on_click(move |_, window, cx| {
                                    let similar =
                                        window.modifiers().alt || window.modifiers().control;
                                    Efa::app_update(&this, cx, |g, cx| {
                                        let Some(loaded) = g.loaded() else { return };
                                        let rows = if similar {
                                            g.similar_fighter_targets(&loaded, n)
                                        } else {
                                            g.fighter_targets(&loaded, n)
                                        };
                                        g.toggle_fighter_ability(i, rows, id, cx);
                                    });
                                }),
                        )
                    })
            },
        );
    }
    menu = menu.item(crate::ui::item_stats::stats_entry(
        this,
        "Fighter",
        crate::ui::item_stats::StatsOf::Fitted(crate::engine::FitItem::Fighter(i)),
    ));
    if catalog
        .get(type_id)
        .and_then(|t| t.market_group)
        .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        let this = this.clone();
        menu = menu.item(entry(IconName::Search, "Fighter Market Group").on_click(
            move |_, window, cx| {
                Efa::app_update(&this, cx, |g, cx| g.jump_to_market(type_id, window, cx));
            },
        ));
    }
    {
        let this = this.clone();
        menu = menu.item(entry(IconName::Delete, "Remove Fighter").on_click(
            move |_, window, cx| {
                let similar = window.modifiers().alt || window.modifiers().control;
                Efa::app_update(&this, cx, |g, cx| {
                    let Some(loaded) = g.loaded() else { return };
                    let rows = if similar {
                        g.similar_fighter_targets(&loaded, n)
                    } else {
                        g.fighter_targets(&loaded, n)
                    };
                    g.remove_fighters(rows, cx);
                });
            },
        ));
    }
    {
        let this = this.clone();
        let current = fighter
            .amount
            .or_else(|| info.as_ref().and_then(FighterInfo::max_size))
            .unwrap_or(0);
        menu = menu.item(
            entry(IconName::Settings2, "Change Fighter Quantity").on_click(move |_, window, cx| {
                Efa::app_update(&this, cx, |_, cx| {
                    Efa::open_input_dialog(
                        "Change Amount",
                        "New Amount:",
                        current.to_string(),
                        window,
                        cx,
                        move |g, amount: u32, cx| {
                            let Some(loaded) = g.loaded() else { return };
                            let rows = g.fighter_targets(&loaded, n);
                            g.set_fighter_amount(rows, amount, cx);
                        },
                    );
                });
            }),
        );
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
            move |menu, window, _| {
                entries.iter().fold(
                    menu.scrollable(true)
                        .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H)),
                    |menu, (v, name)| {
                        let (this, v) = (this.clone(), *v);
                        menu.item(
                            item_entry(&loaded, v, name.clone())
                                .checked(v == type_id)
                                .on_click(move |_, window, cx| {
                                    let similar =
                                        window.modifiers().alt || window.modifiers().control;
                                    Efa::app_update(&this, cx, |g, cx| {
                                        let Some(loaded) = g.loaded() else { return };
                                        let rows = if similar {
                                            g.similar_fighter_targets(&loaded, n)
                                        } else {
                                            g.fighter_targets(&loaded, n)
                                        };
                                        g.swap_fighters(rows, v, cx);
                                    });
                                }),
                        )
                    },
                )
            },
        );
    }
    crate::ui::characters::skills_submenu(
        menu,
        this,
        loaded,
        crate::engine::SkillTarget::Fighter(type_id),
        window,
        cx,
    )
}

/// pyfa's fighterItemMisc: copy the selection or every squadron, paste squadrons.
fn fighter_misc(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    clicked: Option<usize>,
    cx: &App,
) -> PopupMenu {
    let has_rows = this
        .upgrade()
        .and_then(|g| g.read(cx).fit().map(|f| !f.fighters.is_empty()))
        .unwrap_or(false);
    let can_paste = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .is_some_and(|text| !text.trim().is_empty());
    let copy = |rows: Option<usize>| {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Efa::app_update(&this, cx, |g, cx| {
                let Some(fit) = g.fit() else { return };
                let indices: Vec<usize> = match rows {
                    Some(n) => g.fighter_targets(&loaded, n),
                    None => (0..fit.fighters.len()).collect(),
                };
                let fighters: Vec<&Fighter> = indices
                    .iter()
                    .filter_map(|&i| fit.fighters.get(i))
                    .collect();
                let infos: Vec<Option<&FighterInfo>> = indices
                    .iter()
                    .map(|&i| g.stats.as_ref().and_then(|s| s.fighters.get(i)))
                    .collect();
                let text = export_lines(&loaded.catalog, &fighters, &infos);
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            });
        }
    };
    let mut menu = menu;
    if let Some(n) = clicked {
        menu = menu.item(entry(IconName::Copy, "Copy Selected Fighters").on_click(copy(Some(n))));
    }
    if has_rows {
        menu = menu.item(entry(IconName::Copy, "Copy All Fighters").on_click(copy(None)));
    }
    if can_paste {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.item(
            entry(IconName::FileText, "Paste Fighters").on_click(move |_, _, cx| {
                let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return;
                };
                let fighters = parse_lines(&loaded.catalog, &text);
                Efa::app_update(&this, cx, |g, cx| g.add_fighter_squadrons(fighters, cx));
            }),
        );
    }
    menu
}
