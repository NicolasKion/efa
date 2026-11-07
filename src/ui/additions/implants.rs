//! The Implants tab, after pyfa's implant pane: state, slot, icon and name columns in slot
//! order, market drops, the fit or character implant switch, and implant sets.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, checkbox::Checkbox, h_flex, menu::ContextMenuExt,
    menu::PopupMenu, radio::Radio, v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{MENU_MAX_H, PRICE_W, ROW_HEIGHT, export_lines, parse_lines, price_text};
use crate::{
    data::{Kind, TypeId},
    engine::ItemRef,
    fit::ImplantSource,
    icons::Icon,
    implant_set::{self, PrecalcSet},
    ui::{
        Action, AddKind, Efa, Loaded,
        fitting::{DragSource, Dragged},
        validation::flag,
        widgets::{entry, icon_box, icon_label, item_entry, item_label},
    },
};

/// What a menu needs from the fit, read when it opens.
struct Snapshot {
    source: ImplantSource,
    /// The applied implants, in the order the pane lists them.
    rows: Vec<TypeId>,
}

impl Snapshot {
    fn take(this: &WeakEntity<Efa>, cx: &App) -> Option<Self> {
        let g = this.upgrade()?;
        let g = g.read(cx);
        let fit = g.fit()?;
        Some(Self {
            source: fit.implant_source,
            rows: fit
                .applied_implants(g.character_implants())
                .map(|(t, _)| t)
                .collect(),
        })
    }
}

impl Efa {
    pub(in crate::ui) fn implants_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(current) = self.saved.current else {
            return div().into_any_element();
        };
        // Older saves kept implants in the order they were added; pyfa lists them by slot.
        if self.saved.fits[current].sort_implants(&loaded.catalog) {
            self.additions_selection.clear();
            self.additions_anchor = None;
            self.unsaved = true;
        }
        let fit = &self.saved.fits[current];
        let source = fit.implant_source;
        let rows: Vec<(TypeId, bool)> = fit.applied_implants(self.character_implants()).collect();

        let empty_text = match source {
            ImplantSource::Fit => "Double-click or drag implants from the market to add them.",
            ImplantSource::Character => "The character has no implants plugged in.",
        };
        let mut content: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(i, &(type_id, active))| self.implant_row(loaded, source, i, type_id, active, cx))
            .collect();
        let theme = cx.theme();
        // The space under the rows takes the pane's own menu, as in pyfa.
        content.push(
            div()
                .id("implants-rest")
                .flex_1()
                .min_h(px(ROW_HEIGHT))
                .p_2()
                .text_color(theme.muted_foreground)
                .when(rows.is_empty(), |d| d.child(empty_text))
                .context_menu({
                    let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                    move |menu, window, cx| implant_menu(menu, &this, &loaded, None, window, cx)
                })
                .into_any_element(),
        );

        let len = rows.len();
        let own = source == ImplantSource::Fit;
        let list = v_flex()
            .id("implants-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_focus(&self.additions_focus)
            .on_key_down(cx.listener(move |g, event: &KeyDownEvent, _, cx| {
                g.list_key(event, AddKind::Implants, len, own, cx);
            }))
            .children(content)
            .drag_over::<Dragged>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(|g, dragged: &Dragged, _, cx| {
                g.drop_implant(dragged, cx);
            }));

        let switch = h_flex()
            .justify_center()
            .gap_4()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Radio::new("implants-from-fit")
                    .label("Use Fit-specific Implants")
                    .checked(source == ImplantSource::Fit)
                    .on_click(cx.listener(|g, _: &bool, _, cx| {
                        g.apply(Action::SetImplantSource(ImplantSource::Fit), cx);
                    })),
            )
            .child(
                Radio::new("implants-from-character")
                    .label("Use Character Implants")
                    .checked(source == ImplantSource::Character)
                    .on_click(cx.listener(|g, _: &bool, _, cx| {
                        g.apply(Action::SetImplantSource(ImplantSource::Character), cx);
                    })),
            );

        v_flex()
            .flex_1()
            .min_h_0()
            .child(list)
            .child(switch)
            .into_any_element()
    }

    /// pyfa only takes implants dropped from the market.
    fn drop_implant(&mut self, dragged: &Dragged, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let implants: Vec<_> = dragged
            .types()
            .filter(|&id| {
                loaded
                    .catalog
                    .get(id)
                    .is_some_and(|t| t.kind == Kind::Implant)
            })
            .collect();
        if matches!(dragged.source, DragSource::Market) && !implants.is_empty() {
            self.apply(Action::AddImplants(implants), cx);
        }
    }

    fn implant_row(
        &self,
        loaded: &Rc<Loaded>,
        source: ImplantSource,
        i: usize,
        type_id: TypeId,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let selected = self.additions_selection.contains(&i);
        let own = source == ImplantSource::Fit;
        let slot = catalog.get(type_id).and_then(|t| t.implant_slot);
        let this = Self::weak(cx);
        h_flex()
            .id(SharedString::from(format!("implant-{i}")))
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .when(i % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .when_some(
                self.problems_of(loaded, ItemRef::Implant(type_id)),
                |row, why| flag(row, why, theme.danger),
            )
            .child(
                Checkbox::new(SharedString::from(format!("implant-state-{i}")))
                    .checked(active)
                    .disabled(!own)
                    .tooltip(if active { "Active" } else { "Inactive" })
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        Efa::app_update(&this, cx, |g, cx| {
                            let targets = g.addition_targets(i);
                            g.apply(Action::ToggleImplants(i, targets), cx);
                        });
                    }),
            )
            .child(
                div()
                    .w(px(18.))
                    .text_color(theme.muted_foreground)
                    .child(slot.map_or_default(|s| s.to_string())),
            )
            .child(icon_box(catalog, &loaded.icons, Icon::Type(type_id), 22.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(!active, |d| d.text_color(theme.muted_foreground))
                    .child(catalog.name(type_id)),
            )
            .child(
                div()
                    .w(px(PRICE_W))
                    .flex_none()
                    .text_right()
                    .truncate()
                    .children(price_text(&self.prices, type_id, 1)),
            )
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.additions_focus, cx);
                if event.click_count() >= 2 && own {
                    g.apply(Action::RemoveAdditions(AddKind::Implants, vec![i]), cx);
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
                move |menu, window, cx| implant_menu(menu, &this, &loaded, Some(i), window, cx)
            })
            .into_any_element()
    }
}

/// pyfa's implantItem (or implantItemChar) entries for the clicked row, then implantItemMisc.
fn implant_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    clicked: Option<usize>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(snapshot) = Snapshot::take(this, cx) else {
        return menu;
    };
    let own = snapshot.source == ImplantSource::Fit;
    let mut menu = menu;
    let clicked_type = clicked.and_then(|i| snapshot.rows.get(i).map(|&t| (i, t)));
    if let Some((i, type_id)) = clicked_type
        && let Some(t) = loaded.catalog.get(type_id)
    {
        menu = menu.item(super::super::item_stats::stats_entry(
            this,
            "Implant",
            super::super::item_stats::StatsOf::Fitted(crate::engine::FitItem::Implant(type_id)),
        ));
        if t.market_group.is_some() {
            let this = this.clone();
            menu = menu.item(entry(IconName::Search, "Implant Market Group").on_click(
                move |_, window, cx| {
                    Efa::app_update(&this, cx, |g, cx| g.jump_to_market(type_id, window, cx));
                },
            ));
        }
        if own {
            menu = item_entries(menu, this, loaded, i, type_id, window, cx);
        }
    }
    menu = menu.separator();
    if own {
        menu = apply_set_entry(menu, this, loaded, window, cx);
    }
    if !snapshot.rows.is_empty() {
        let (this, implants) = (this.clone(), snapshot.rows.clone());
        menu = menu.item(entry(IconName::Star, "Save as New Implant Set").on_click(
            move |_, window, cx| {
                let implants = implants.clone();
                Efa::app_update(&this, cx, |_, cx| {
                    Efa::save_implant_set_dialog(implants, window, cx);
                });
            },
        ));
    }
    copy_paste_entries(
        menu,
        this,
        loaded,
        &snapshot,
        clicked_type.map(|(i, _)| i),
        window,
        cx,
    )
}

/// Remove and Variations, which only make sense for the fit's own implants.
fn item_entries(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    i: usize,
    type_id: TypeId,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let remove = {
        let this = this.clone();
        entry(IconName::Delete, "Remove Implant").on_click(move |_, _, cx| {
            Efa::app_update(&this, cx, |g, cx| {
                let targets = g.addition_targets(i);
                g.apply(Action::RemoveAdditions(AddKind::Implants, targets), cx);
            });
        })
    };
    let mut menu = menu.item(remove);
    let variations = catalog.variations_of(type_id);
    if variations.len() > 1 && crate::ui::hub::shown_menus(window, cx).variations {
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
                                        let action =
                                            Action::SwapAdditions(AddKind::Implants, targets, v);
                                        g.apply(action, cx);
                                    });
                                }),
                        )
                    },
                )
            },
        );
    }
    menu
}

/// "Apply Implant Set": the precalculated sets, with a submenu per grade where there are several,
/// then the user's sets under "Saved Sets".
fn apply_set_entry(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let precalc: Rc<Vec<PrecalcSet>> = Rc::new(implant_set::precalculated(&loaded.catalog));
    let mut saved = this
        .upgrade()
        .map_or_default(|g| g.read(cx).saved.implant_sets.clone());
    saved.sort_by(|a, b| a.name.cmp(&b.name));
    if precalc.is_empty() && saved.is_empty() {
        return menu;
    }
    let saved = Rc::new(saved);
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    menu.submenu_with_element(
        "Apply Implant Set",
        icon_label(IconName::GalleryVerticalEnd, "Apply Implant Set"),
        window,
        cx,
        move |menu, window, cx| {
            let mut menu = menu
                .scrollable(true)
                .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H));
            for set in precalc.iter() {
                let first = set.grades.first().and_then(|(_, ids)| ids.first().copied());
                if let [(_, implants)] = set.grades.as_slice() {
                    menu = menu.item(set_item(&this, &loaded, &set.name, implants));
                    continue;
                }
                let (this, loaded, grades) = (this.clone(), Rc::clone(&loaded), set.grades.clone());
                menu = menu.submenu_with_element(
                    set.name.clone(),
                    item_label(&loaded, first, set.name.clone()),
                    window,
                    cx,
                    move |menu, _, _| {
                        grades.iter().fold(menu, |menu, (grade, implants)| {
                            let label = grade.clone().unwrap_or_default();
                            menu.item(set_item(&this, &loaded, &label, implants))
                        })
                    },
                );
            }
            if !saved.is_empty() {
                menu = menu.separator();
                let (this, loaded, saved) = (this.clone(), Rc::clone(&loaded), Rc::clone(&saved));
                menu = menu.submenu_with_element(
                    "Saved Sets",
                    icon_label(IconName::Star, "Saved Sets"),
                    window,
                    cx,
                    move |menu, window, _| {
                        saved.iter().fold(
                            menu.scrollable(true)
                                .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H)),
                            |menu, set| {
                                menu.item(set_item(&this, &loaded, &set.name, &set.implants))
                            },
                        )
                    },
                );
            }
            let editor = this.clone();
            menu.separator()
                .item(entry(IconName::Settings2, "Implant Set Editor…").on_click(
                    move |_, window, cx| {
                        Efa::app_update(&editor, cx, |g, cx| {
                            g.open_implant_set_editor(None, window, cx);
                        });
                    },
                ))
        },
    )
}

/// One set in the Apply Implant Set menu, shown with its first implant's icon.
fn set_item(
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    label: &str,
    implants: &[TypeId],
) -> gpui_kit::component::menu::PopupMenuItem {
    let (this, implants) = (this.clone(), implants.to_vec());
    let item = match implants.first() {
        Some(&first) => item_entry(loaded, first, label.to_owned()),
        None => entry(IconName::Star, label.to_owned()),
    };
    item.disabled(implants.is_empty())
        .on_click(move |_, _, cx| {
            let implants = implants.clone();
            Efa::app_update(&this, cx, |g, cx| {
                g.apply(Action::AddImplants(implants), cx);
            });
        })
}

/// pyfa's additions copy and paste entries, working on whichever implants the pane shows.
fn copy_paste_entries(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    snapshot: &Snapshot,
    clicked: Option<usize>,
    window: &Window,
    cx: &App,
) -> PopupMenu {
    if !crate::ui::hub::shown_menus(window, cx).additions_copy_paste {
        return menu;
    }
    let kind = AddKind::Implants;
    let lines = |implants: &[TypeId]| {
        let rows: Vec<(TypeId, u32)> = implants.iter().map(|&t| (t, 1)).collect();
        export_lines(&loaded.catalog, kind, &rows)
    };
    let mut menu = menu;
    if let Some(i) = clicked {
        let (this, rows, loaded) = (this.clone(), snapshot.rows.clone(), Rc::clone(loaded));
        menu = menu.item(entry(IconName::Copy, "Copy Selected Implants").on_click(
            move |_, _, cx| {
                let Some(g) = this.upgrade() else { return };
                let picked: Vec<(TypeId, u32)> = g
                    .read(cx)
                    .addition_targets(i)
                    .into_iter()
                    .filter_map(|r| rows.get(r).map(|&t| (t, 1)))
                    .collect();
                let text = export_lines(&loaded.catalog, kind, &picked);
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            },
        ));
    }
    if !snapshot.rows.is_empty() {
        let text = lines(&snapshot.rows);
        menu = menu.item(
            entry(IconName::Copy, "Copy All Implants").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            }),
        );
    }
    let can_paste = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .is_some_and(|text| !text.trim().is_empty());
    if can_paste {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu = menu.item(
            entry(IconName::FileText, "Paste Implants").on_click(move |_, _, cx| {
                let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
                    return;
                };
                let items = parse_lines(&loaded.catalog, kind, &text);
                if !items.is_empty() {
                    Efa::app_update(&this, cx, |g, cx| {
                        g.apply(Action::AddAdditions(kind, items), cx);
                    });
                }
            }),
        );
    }
    menu
}
