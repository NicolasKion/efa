//! The Boosters tab: pyfa's booster pane, with its state, slot, side effect and duration columns,
//! its menu and drops from the market.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, WeakEntity,
    Window, div, prelude::FluentBuilder, px,
};

use super::{MENU_MAX_H, PRICE_W, ROW_HEIGHT, misc_entries, price_text};
use crate::{
    data::{Catalog, ItemType, Kind, TypeId},
    engine::{ItemRef, Stats},
    fit::{Booster, Fit},
    icons::Icon,
    ui::{
        Action, AddKind, Gyfa, Loaded,
        columns::STATE_W,
        fitting::{DragSource, Dragged},
        validation::flag,
        widgets::{color, entry, icon_box, icon_label, item_entry, short},
    },
};

const SLOT_W: f32 = 36.;
const DURATION_W: f32 = 64.;
/// Past this many variations pyfa lists them by name instead of by side effect chance.
const RANK_BY_CHANCE_MAX: usize = 7;

/// pyfa's names for the side effects it implements, by effect ID (eos/effects.py).
const SIDE_EFFECT_NAMES: [(i32, &str); 12] = [
    (2735, "Armor Capacity"),
    (2736, "Armor Repair Amount"),
    (2737, "Shield Capacity"),
    (2739, "Turret Optimal Range"),
    (2741, "Turret Falloff"),
    (2745, "Cap Capacity"),
    (2746, "Velocity"),
    (2747, "Turret Tracking"),
    (2748, "Missile Velocity"),
    (2749, "Missile Explosion Velocity"),
    (2791, "Missile Explosion Radius"),
    (4970, "Shield Boost"),
];

/// A side effect as the pane shows it.
#[derive(Debug, Clone, PartialEq)]
struct SideEffectRow {
    effect_id: i32,
    label: String,
    active: bool,
}

/// pyfa's label for a side effect: "-10.875% Armor Capacity". Without engine numbers (no ship
/// yet) only the name is known.
fn side_effect_label(effect_id: i32, strength: Option<f64>) -> String {
    let name = SIDE_EFFECT_NAMES
        .iter()
        .find(|(id, _)| *id == effect_id)
        .map_or_else(
            || format!("Side Effect {effect_id}"),
            |(_, n)| (*n).to_owned(),
        );
    match strength {
        Some(s) => format!("{}% {name}", round_to_prec(s, 5)),
        None => name,
    }
}

/// pyfa's `roundToPrec`: round to `digits` significant digits, and drop a zero fraction.
fn round_to_prec(value: f64, digits: i32) -> String {
    if value == 0.0 || !value.is_finite() {
        return format!("{value}");
    }
    let decimals = (digits - value.abs().log10().floor() as i32 - 1).max(0);
    let scale = 10f64.powi(decimals);
    let rounded = (value * scale).round() / scale;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded}")
    }
}

/// The side effects of the fit's booster at `index`, with the engine's strengths when it has
/// them for this booster.
fn side_effects(
    catalog: &Catalog,
    stats: Option<&Stats>,
    fit: &Fit,
    index: usize,
) -> Vec<SideEffectRow> {
    let Some(booster) = fit.boosters.get(index) else {
        return Vec::new();
    };
    let active = |id: i32| booster.side_effects.contains(&id);
    let computed = stats
        .filter(|s| s.boosters.len() == fit.boosters.len())
        .and_then(|s| s.boosters.get(index));
    match computed {
        Some(info) => info
            .side_effects
            .iter()
            .map(|se| SideEffectRow {
                effect_id: se.effect_id,
                label: side_effect_label(se.effect_id, se.strength),
                active: active(se.effect_id),
            })
            .collect(),
        None => catalog
            .get(booster.type_id)
            .map_or_default(|t| t.side_effects.as_slice())
            .iter()
            .map(|&id| SideEffectRow {
                effect_id: id,
                label: side_effect_label(id, None),
                active: active(id),
            })
            .collect(),
    }
}

/// Whether pyfa's database has this booster: its data update hides a few published ones and
/// publishes the AIR boosters (`db_update.py`).
fn listed_by_pyfa(t: &ItemType) -> bool {
    let name = t.name.as_str();
    let after = |prefix: &str, rest: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|tail| tail.contains(rest))
    };
    if after("AIR ", "Booster") {
        return true;
    }
    let hidden = name.starts_with("Limited Synth ")
        || name.starts_with("Expired ")
        || after("Grand Prix ", "Booster")
        || after("Mining Blitz ", " Booster Dose ");
    t.published && !hidden
}

/// A booster's variations the way pyfa finds them. The data has no variation parents for
/// boosters, so pyfa takes every booster of the same slot sold in the same market group (or in
/// none), and ranks them by the chance of their first side effect, or by name when there are
/// many.
fn booster_variations(catalog: &Catalog, type_id: TypeId) -> Vec<TypeId> {
    let Some(base) = catalog.get(type_id) else {
        return Vec::new();
    };
    let Some(slot) = base.booster_slot else {
        return Vec::new();
    };
    let mut items: Vec<&ItemType> = catalog
        .types
        .values()
        .filter(|t| {
            t.id == type_id
                || (t.kind == Kind::Booster
                    && t.booster_slot == Some(slot)
                    && (t.market_group.is_none() || t.market_group == base.market_group)
                    && listed_by_pyfa(t))
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));
    if items.len() <= RANK_BY_CHANCE_MAX {
        // Synth boosters have no side effects, which ranks them first.
        items.sort_by(|a, b| {
            let rank = |t: &ItemType| t.booster_chance.unwrap_or(0.0);
            rank(a).total_cmp(&rank(b))
        });
    }
    items.into_iter().map(|t| t.id).collect()
}

impl Gyfa {
    pub(in crate::ui) fn boosters_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let mut content: Vec<AnyElement> = Vec::new();
        if fit.boosters.is_empty() {
            content.push(
                div()
                    .id("additions-empty")
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child("Double-click boosters in the market, or drag them here, to add them.")
                    .context_menu({
                        let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                        move |menu, _, cx| {
                            misc_entries(menu, &this, &loaded, AddKind::Boosters, None, cx)
                        }
                    })
                    .into_any_element(),
            );
        } else {
            content.push(Self::booster_header(cx));
            for i in 0..fit.boosters.len() {
                content.push(self.booster_row(loaded, fit, i, cx));
            }
        }
        v_flex()
            .id("additions-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(content)
            .drag_over::<Dragged>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            // pyfa treats a market item dropped here like one double-clicked in the market.
            .on_drop(cx.listener(|g, dragged: &Dragged, _, cx| {
                if matches!(dragged.source, DragSource::Market) {
                    cx.stop_propagation();
                    for id in dragged.types() {
                        g.add_item(id, cx);
                    }
                }
            }))
            .into_any_element()
    }

    fn booster_header(cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .h(px(ROW_HEIGHT - 6.))
            .px_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(px(STATE_W + 22.)).flex_none())
            .child(div().w(px(SLOT_W)).flex_none().child("Slot"))
            .child(div().flex_1().min_w_0().child("Name"))
            .child(div().flex_1().min_w_0().child("Active Side Effects"))
            .child(div().w(px(PRICE_W)).flex_none().text_right().child("Price"))
            .child(div().w(px(DURATION_W)).flex_none().child("Duration"))
            .into_any_element()
    }

    fn booster_row(
        &self,
        loaded: &Rc<Loaded>,
        fit: &Fit,
        i: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let booster: &Booster = &fit.boosters[i];
        let selected = self.additions_selection.contains(&i);
        let (mark, mark_color) = if booster.active {
            ("✓", self.palette.green)
        } else {
            ("○", self.palette.muted)
        };
        let slot = catalog
            .get(booster.type_id)
            .and_then(|t| t.booster_slot)
            .map_or_default(|s| s.to_string());
        let active_effects: Vec<String> = side_effects(catalog, self.stats.as_ref(), fit, i)
            .into_iter()
            .filter(|se| se.active)
            .map(|se| se.label)
            .collect();
        let effects_text = if active_effects.is_empty() {
            "None".to_owned()
        } else {
            active_effects.join(", ")
        };
        let duration = self
            .stats
            .as_ref()
            .filter(|s| s.boosters.len() == fit.boosters.len())
            .and_then(|s| s.boosters.get(i))
            .and_then(|b| b.duration)
            .map_or_default(|secs| format!("{} min", short(secs / 60.0)));

        h_flex()
            .id(SharedString::from(format!("addition-{i}")))
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .when(i % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .when_some(self.problems_of(loaded, ItemRef::Booster(i)), |row, why| {
                flag(row, why, theme.danger)
            })
            .child(
                div()
                    .id(SharedString::from(format!("booster-state-{i}")))
                    .w(px(STATE_W))
                    .flex_none()
                    .text_color(color(mark_color))
                    .child(mark)
                    .on_click(cx.listener(move |g, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        let rows = g.addition_targets(i);
                        g.apply(Action::ToggleBoosters(i, rows), cx);
                    })),
            )
            .child(icon_box(
                catalog,
                &loaded.icons,
                Icon::Type(booster.type_id),
                22.,
            ))
            .child(div().w(px(SLOT_W)).flex_none().child(slot))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(catalog.name(booster.type_id)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(active_effects.is_empty(), |d| {
                        d.text_color(theme.muted_foreground)
                    })
                    .child(effects_text),
            )
            .child(
                div()
                    .w(px(PRICE_W))
                    .flex_none()
                    .text_right()
                    .truncate()
                    .children(price_text(&self.prices, booster.type_id, 1)),
            )
            .child(div().w(px(DURATION_W)).flex_none().child(duration))
            .on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    g.apply(Action::RemoveAdditions(AddKind::Boosters, vec![i]), cx);
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
                move |menu, window, cx| booster_menu(menu, &this, &loaded, i, window, cx)
            })
            .into_any_element()
    }
}

/// pyfa's boosterItem menu, then the boosterItemMisc entries.
fn booster_menu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    i: usize,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let Some((type_id, effects)) = this.upgrade().and_then(|g| {
        let g = g.read(cx);
        let fit = g.fit()?;
        let booster = fit.boosters.get(i)?;
        Some((
            booster.type_id,
            side_effects(catalog, g.stats.as_ref(), fit, i),
        ))
    }) else {
        return menu;
    };
    let Some(t) = catalog.get(type_id) else {
        return menu;
    };
    let mut menu = menu;

    if !effects.is_empty() {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Side Effects",
            icon_label(IconName::TriangleAlert, "Side Effects"),
            window,
            cx,
            move |menu, _, _| {
                effects.iter().fold(menu, |menu, se| {
                    let (this, effect_id) = (this.clone(), se.effect_id);
                    menu.item(
                        PopupMenuItem::new(se.label.clone())
                            .checked(se.active)
                            .on_click(move |_, _, cx| {
                                Gyfa::app_update(&this, cx, |g, cx| {
                                    g.apply(Action::ToggleBoosterSideEffect(i, effect_id), cx);
                                });
                            }),
                    )
                })
            },
        );
    }
    menu = menu.item(super::super::item_stats::stats_entry(
        this,
        "Booster",
        super::super::item_stats::StatsOf::Fitted(crate::engine::FitItem::Booster(type_id)),
    ));
    if t.market_group.is_some() {
        let this = this.clone();
        menu = menu.item(entry(IconName::Search, "Booster Market Group").on_click(
            move |_, window, cx| {
                Gyfa::app_update(&this, cx, |g, cx| g.jump_to_market(type_id, window, cx));
            },
        ));
    }
    {
        let this = this.clone();
        menu = menu.item(entry(IconName::Delete, "Remove Booster").on_click(
            move |_, _: &mut Window, cx: &mut App| {
                Gyfa::app_update(&this, cx, |g, cx| {
                    let rows = g.addition_targets(i);
                    g.apply(Action::RemoveAdditions(AddKind::Boosters, rows), cx);
                });
            },
        ));
    }
    let variations = booster_variations(catalog, type_id);
    if variations.len() > 1 && crate::ui::hub::preferences(cx).context_menus.variations {
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
                        // pyfa swaps only the clicked booster: one per slot means a selection
                        // could only collapse into a single booster anyway.
                        menu.item(
                            item_entry(&loaded, v, name.clone())
                                .checked(v == type_id)
                                .on_click(move |_, _, cx| {
                                    Gyfa::app_update(&this, cx, |g, cx| {
                                        g.apply(
                                            Action::SwapAdditions(AddKind::Boosters, vec![i], v),
                                            cx,
                                        );
                                    });
                                }),
                        )
                    },
                )
            },
        );
    }
    misc_entries(
        menu.separator(),
        this,
        loaded,
        AddKind::Boosters,
        Some(i),
        cx,
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const DATA: &str = "data/eve";

    fn catalog() -> Option<Catalog> {
        if !Path::new(DATA).join("phobos/metadata.json").exists() {
            eprintln!("no EVE data in {DATA}, skipping");
            return None;
        }
        Some(
            Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads"),
        )
    }

    #[test]
    fn rounds_like_pyfa() {
        for (value, want) in [
            (-25.0, "-25"),
            (-10.875, "-10.875"),
            (-12.345_678, "-12.346"),
            (0.0, "0"),
        ] {
            assert_eq!(round_to_prec(value, 5), want, "value {value}");
        }
    }

    #[test]
    fn labels_side_effects_like_pyfa() {
        assert_eq!(
            side_effect_label(2735, Some(-10.875)),
            "-10.875% Armor Capacity"
        );
        assert_eq!(side_effect_label(2746, None), "Velocity");
    }

    #[test]
    fn pyfa_hides_some_boosters() {
        let Some(catalog) = catalog() else { return };
        let listed = |name: &str| listed_by_pyfa(catalog.find(name).expect("booster exists"));
        assert!(listed("Strong Blue Pill Booster"));
        assert!(!listed("Limited Synth Blue Pill Booster"));
    }

    #[test]
    fn variations_rank_by_side_effect_chance() {
        let Some(catalog) = catalog() else { return };
        let id = |name: &str| catalog.find(name).expect("booster exists").id;
        let names: Vec<String> = booster_variations(&catalog, id("Improved Blue Pill Booster"))
            .into_iter()
            .map(|v| catalog.name(v))
            .collect();
        assert!(names.len() <= RANK_BY_CHANCE_MAX, "variations: {names:?}");
        let position = |name: &str| {
            names
                .iter()
                .position(|n| n == name)
                .unwrap_or_else(|| panic!("{name} missing from {names:?}"))
        };
        assert!(position("Synth Blue Pill Booster") < position("Standard Blue Pill Booster"));
        assert!(position("Standard Blue Pill Booster") < position("Strong Blue Pill Booster"));
        assert!(!names.iter().any(|n| n.starts_with("Limited ")));
    }

    #[test]
    fn many_variations_go_by_name() {
        let Some(catalog) = catalog() else { return };
        let id = catalog
            .find("Imperial Electronics Booster I")
            .expect("booster exists")
            .id;
        let names: Vec<String> = booster_variations(&catalog, id)
            .into_iter()
            .map(|v| catalog.name(v))
            .collect();
        assert!(names.len() > RANK_BY_CHANCE_MAX, "variations: {names:?}");
        assert!(names.is_sorted(), "not by name: {names:?}");
    }
}
