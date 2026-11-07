//! pyfa's Character Editor (gui/characterEditor.py), in a window of its own: pick, create,
//! rename, copy and delete characters, set skill levels in the skill tree, plug in implants,
//! set the security status, and save or revert pending changes.
//!
//! The characters live in the [`Hub`]; every change goes through it, so the fitting windows
//! recalculate with it right away.

use std::{collections::HashSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
};

use super::{
    Hub, Loaded,
    characters::name_dialog,
    widgets::{entry, icon_box, icon_label},
};
use crate::{
    character::{CharacterId, Characters, Keep, MAX_LEVEL},
    data::{Kind, TypeId},
    icons::Icon,
};

const ROW_HEIGHT: f32 = 24.;
/// Most implant search hits listed, so a one-letter query stays cheap to draw.
const SEARCH_LIMIT: usize = 200;
const MENU_MAX_H: f32 = 480.;
/// What pyfa says before importing skills.
const IMPORT_NOTICE: &str = "Imported skills are pending: click Save to keep them.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Skills,
    Implants,
}

/// A skill group of the tree, with its skills by name.
struct Group {
    id: i32,
    name: String,
    skills: Vec<(TypeId, String)>,
}

pub struct CharacterEditor {
    hub: Entity<Hub>,
    loaded: Rc<Loaded>,
    /// The hub's characters as of the last change it announced.
    characters: Rc<Characters>,
    selected: CharacterId,
    tab: Tab,
    groups: Vec<Group>,
    expanded: HashSet<i32>,
    /// The skill the level keys apply to.
    skill: Option<TypeId>,
    search: Entity<InputState>,
    implant_search: Entity<InputState>,
    tree_focus: FocusHandle,
    notice: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl CharacterEditor {
    pub fn new(
        hub: Entity<Hub>,
        loaded: Rc<Loaded>,
        select: CharacterId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        window.set_window_title("gyfa: Character Editor");
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        let implant_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search implants"));
        let subscriptions = vec![
            cx.observe(&hub, |this: &mut Self, hub, cx| {
                let characters = &hub.read(cx).characters;
                if !Rc::ptr_eq(characters, &this.characters) {
                    this.characters = Rc::clone(characters);
                    cx.notify();
                }
            }),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&implant_search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let characters = Rc::clone(&hub.read(cx).characters);
        let groups = skill_groups(&loaded);
        Self {
            selected: characters.resolve(select),
            characters,
            hub,
            loaded,
            tab: Tab::Skills,
            groups,
            expanded: HashSet::new(),
            skill: None,
            search,
            implant_search,
            tree_focus: cx.focus_handle(),
            notice: None,
            _subscriptions: subscriptions,
        }
    }

    /// Show another character, as picking it in the list does.
    pub fn select(&mut self, id: CharacterId, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = self.characters.resolve(id);
        self.skill = None;
        self.notice = None;
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn edit<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Characters, &Loaded) -> R,
    ) -> R {
        let loaded = Rc::clone(&self.loaded);
        let result = self
            .hub
            .update(cx, |hub, cx| hub.edit_characters(cx, |c| edit(c, &loaded)));
        self.characters = Rc::clone(&self.hub.read(cx).characters);
        cx.notify();
        result
    }

    fn set_level(&mut self, skill: TypeId, level: Option<u8>, cx: &mut Context<Self>) {
        let id = self.selected;
        if id.is_builtin() {
            return;
        }
        let strict = self
            .hub
            .read(cx)
            .saved
            .preferences
            .engine
            .strict_skill_levels;
        self.edit(cx, |c, loaded| {
            c.change_level_with(id, skill, level, Keep::Saved, strict, &loaded.catalog);
        });
    }

    /// The number keys set the selected skill's level, as in pyfa.
    fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let Some(skill) = self.skill else { return };
        let Ok(level) = event.keystroke.key.parse::<u8>() else {
            return;
        };
        if level <= MAX_LEVEL {
            self.set_level(skill, Some(level), cx);
            cx.stop_propagation();
        }
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        name_dialog(
            &self.hub,
            "New Character",
            "Enter a name for your new Character:",
            String::new(),
            window,
            cx,
            move |name, cx| {
                let _ = this.update(cx, |e, cx| {
                    let id = e.edit(cx, |c, _| c.create(&name));
                    e.selected = id;
                });
            },
        );
    }

    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.selected;
        let this = cx.entity().downgrade();
        name_dialog(
            &self.hub,
            "Copy Character",
            "Enter a name for your Character copy:",
            format!("{} Copy", self.characters.name(from)),
            window,
            cx,
            move |name, cx| {
                let _ = this.update(cx, |e, cx| {
                    let id = e.edit(cx, |c, loaded| c.copy(from, &name, &loaded.catalog));
                    e.selected = id;
                });
            },
        );
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.selected;
        if id.is_builtin() {
            return;
        }
        let this = cx.entity().downgrade();
        name_dialog(
            &self.hub,
            "Rename Character",
            "Enter a new name for your Character:",
            self.characters.name(id),
            window,
            cx,
            move |name, cx| {
                let _ = this.update(cx, |e, cx| e.edit(cx, |c, _| c.rename(id, &name)));
            },
        );
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.selected;
        if id.is_builtin() {
            return;
        }
        let question = format!(
            "Do you really want to delete the {} Character?",
            self.characters.name(id)
        );
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (yes, no) = (this.clone(), question.clone());
            dialog.title("Confirm Delete").w(px(360.)).child(
                v_flex().gap_3().child(no).child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("delete-no")
                                .label("No")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("delete-yes").danger().label("Yes").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = yes.update(cx, |e, cx| {
                                    e.edit(cx, |c, _| c.delete(id));
                                    e.selected = CharacterId::All5;
                                    e.skill = None;
                                });
                            },
                        )),
                ),
            )
        });
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, _| c.save_levels(id));
    }

    fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.selected;
        let this = cx.entity().downgrade();
        name_dialog(
            &self.hub,
            "Save Character As...",
            "Enter a name for your new Character:",
            format!("{} Copy", self.characters.name(id)),
            window,
            cx,
            move |name, cx| {
                let _ = this.update(cx, |e, cx| {
                    e.edit(cx, |c, loaded| c.save_as(id, &name, &loaded.catalog));
                });
            },
        );
    }

    fn revert(&mut self, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, _| c.revert_levels(id));
    }

    fn import_skills(&mut self, cx: &mut Context<Self>) {
        let id = self.selected;
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.notice = Some("There are no skills on the clipboard".into());
            cx.notify();
            return;
        };
        let result = self.edit(cx, |c, loaded| c.import_skills(id, &text, &loaded.catalog));
        self.notice = Some(match result {
            Ok(0) => IMPORT_NOTICE.into(),
            Ok(errors) => format!("{IMPORT_NOTICE} {errors} line(s) could not be read.").into(),
            Err(e) => e.into(),
        });
        cx.notify();
    }

    fn export_skills(&mut self, cx: &mut Context<Self>) {
        let text = self
            .characters
            .export_skills(self.selected, &self.loaded.catalog);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice = Some("Skills copied to the clipboard".into());
        cx.notify();
    }

    fn sec_status_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.selected;
        let current = self.characters.sec_status(id);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(format!("{current:.2}")));
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (input, this) = (input.clone(), this.clone());
            // The OK button and Enter both submit; it closes the dialog itself when done.
            let submit: crate::ui::widgets::Submit = {
                let (input, this) = (input.clone(), this.clone());
                Rc::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let Ok(value) = input.read(cx).value().trim().parse::<f64>() else {
                        return;
                    };
                    window.close_dialog(cx);
                    let _ = this.update(cx, |e, cx| {
                        e.edit(cx, |c, _| c.set_sec_status(id, value));
                    });
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
                .title("Set Security Status")
                .w(px(320.))
                .child(
                    v_flex()
                        .gap_3()
                        .child("Security Status is used in some CONCORD hull calculations")
                        .child(Input::new(&input))
                        .child(
                            h_flex().justify_end().child(
                                Button::new("sec-status-ok")
                                    .primary()
                                    .label("OK")
                                    .on_click({
                                        let submit = Rc::clone(&submit);
                                        move |e: &ClickEvent, w: &mut Window, cx: &mut App| {
                                            submit(e, w, cx);
                                        }
                                    }),
                            ),
                        ),
                )
        });
    }

    fn add_implant(&mut self, implant: TypeId, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, loaded| c.add_implant(id, &loaded.catalog, implant));
    }

    fn remove_implant(&mut self, implant: TypeId, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, _| c.remove_implant(id, implant));
    }

    fn apply_implant_set(&mut self, implants: &[TypeId], cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, loaded| {
            for &implant in implants {
                c.add_implant(id, &loaded.catalog, implant);
            }
        });
    }

    /// The character list and pyfa's `EntityEditor` buttons.
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let builtin = self.selected.is_builtin();
        let current = self.selected;
        let choices: Vec<(CharacterId, String)> = self
            .characters
            .ordered()
            .into_iter()
            .map(|id| (id, self.characters.name(id)))
            .collect();
        let this = cx.entity().downgrade();
        let picker = Button::new("editor-character")
            .small()
            .outline()
            .label(self.characters.name(current))
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
                for (id, name) in &choices {
                    let (this, id) = (this.clone(), *id);
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(id == current)
                            .on_click(move |_, window, cx| {
                                let _ = this.update(cx, |e, cx| e.select(id, window, cx));
                            }),
                    );
                }
                menu
            });
        h_flex()
            .gap_1()
            .child(div().flex_1().child(picker))
            .child(
                Button::new("character-new")
                    .small()
                    .icon(IconName::Plus)
                    .tooltip("New Character")
                    .on_click(cx.listener(|e, _, window, cx| e.create(window, cx))),
            )
            .child(
                Button::new("character-rename")
                    .small()
                    .icon(IconName::Replace)
                    .tooltip("Rename Character")
                    .disabled(builtin)
                    .on_click(cx.listener(|e, _, window, cx| e.rename(window, cx))),
            )
            .child(
                Button::new("character-copy")
                    .small()
                    .icon(IconName::Copy)
                    .tooltip("Copy Character")
                    .on_click(cx.listener(|e, _, window, cx| e.copy(window, cx))),
            )
            .child(
                Button::new("character-delete")
                    .small()
                    .icon(IconName::Delete)
                    .tooltip("Delete Character")
                    .disabled(builtin)
                    .on_click(cx.listener(|e, _, window, cx| e.delete(window, cx))),
            )
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .children(
                [(Tab::Skills, "Skills"), (Tab::Implants, "Implants")].map(|(tab, label)| {
                    let selected = self.tab == tab;
                    div()
                        .id(SharedString::from(format!("editor-tab-{label}")))
                        .px_3()
                        .py_1()
                        .rounded_t(px(4.))
                        .when(selected, |d| d.bg(theme.secondary))
                        .when(!selected, |d| d.text_color(theme.muted_foreground))
                        .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                        .child(label)
                        .on_click(cx.listener(move |e, _: &ClickEvent, _, cx| {
                            e.tab = tab;
                            cx.notify();
                        }))
                }),
            )
    }

    /// The level column's text, pyfa's "Level N" or "Not learned".
    fn level_text(&self, skill: TypeId) -> String {
        match self.characters.level(self.selected, skill) {
            Some(level) => format!("Level {level}"),
            None => "Not learned".to_owned(),
        }
    }

    fn skill_row(
        &self,
        skill: TypeId,
        name: &str,
        indent: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let dirty = self.characters.is_skill_dirty(self.selected, skill);
        let selected = self.skill == Some(skill);
        let builtin = self.selected.is_builtin();
        let row = h_flex()
            .id(SharedString::from(format!("skill-{skill}")))
            .h(px(ROW_HEIGHT))
            .px_2()
            .gap_2()
            .when(indent, Styled::pl_6)
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .child(
                gpui_kit::component::Icon::new(IconName::BookOpen)
                    .size(px(14.))
                    .text_color(if dirty {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    }),
            )
            .child(div().flex_1().truncate().child(name.to_owned()))
            .child(
                div()
                    .w(px(90.))
                    .text_color(if dirty {
                        theme.danger
                    } else {
                        theme.foreground
                    })
                    .child(self.level_text(skill)),
            )
            .on_click(cx.listener(move |e, _: &ClickEvent, window, cx| {
                e.skill = Some(skill);
                window.focus(&e.tree_focus, cx);
                cx.notify();
            }))
            .on_mouse_down(
                gpui_kit::MouseButton::Right,
                cx.listener(move |e, _, _, cx| {
                    e.skill = Some(skill);
                    cx.notify();
                }),
            );
        if builtin {
            return row.into_any_element();
        }
        let this = cx.entity().downgrade();
        row.context_menu(move |menu, _, _| {
            let set = |level: Option<u8>| {
                let this = this.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |e, cx| e.set_level(skill, level, cx));
                }
            };
            let menu = menu.item(PopupMenuItem::new("Unlearn").on_click(set(None)));
            (0..=MAX_LEVEL).fold(menu, |menu, level| {
                menu.item(PopupMenuItem::new(format!("Level {level}")).on_click(set(Some(level))))
            })
        })
        .into_any_element()
    }

    fn skills_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let mut rows: Vec<AnyElement> = Vec::new();
        if query.is_empty() {
            for group in &self.groups {
                let open = self.expanded.contains(&group.id);
                let dirty = group
                    .skills
                    .iter()
                    .any(|&(s, _)| self.characters.is_skill_dirty(self.selected, s));
                let id = group.id;
                rows.push(
                    h_flex()
                        .id(SharedString::from(format!("skill-group-{id}")))
                        .h(px(ROW_HEIGHT))
                        .px_2()
                        .gap_2()
                        .hover(|s| s.bg(theme.table_hover))
                        .child(div().w(px(10.)).child(if open { "▾" } else { "▸" }))
                        .child(
                            gpui_kit::component::Icon::new(IconName::BookOpen)
                                .size(px(14.))
                                .text_color(if dirty {
                                    theme.danger
                                } else {
                                    theme.muted_foreground
                                }),
                        )
                        .child(group.name.clone())
                        .on_click(cx.listener(move |e, _: &ClickEvent, _, cx| {
                            if !e.expanded.remove(&id) {
                                e.expanded.insert(id);
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                );
                if open {
                    for (skill, name) in &group.skills {
                        rows.push(self.skill_row(*skill, name, true, cx));
                    }
                }
            }
        } else {
            let mut hits: Vec<&(TypeId, String)> = self
                .groups
                .iter()
                .flat_map(|g| &g.skills)
                .filter(|(_, name)| name.to_lowercase().contains(&query))
                .collect();
            hits.sort_by(|a, b| a.1.cmp(&b.1));
            for (skill, name) in hits {
                rows.push(self.skill_row(*skill, name, false, cx));
            }
        }
        let builtin = self.selected.is_builtin();
        let sec_status = self.characters.sec_status(self.selected);
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(Input::new(&self.search).small())
            .child(
                v_flex()
                    .id("skill-tree")
                    .track_focus(&self.tree_focus)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(px(4.))
                    .children(rows)
                    .on_key_down(cx.listener(|e, event: &KeyDownEvent, _, cx| e.key(event, cx))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("sec-status")
                            .small()
                            .label(format!("Sec Status: {sec_status:.2}"))
                            .disabled(builtin)
                            .on_click(
                                cx.listener(|e, _, window, cx| e.sec_status_dialog(window, cx)),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("skills-import")
                            .small()
                            .icon(IconName::FolderOpen)
                            .tooltip("Import skills from clipboard")
                            .disabled(builtin)
                            .on_click(cx.listener(|e, _, _, cx| e.import_skills(cx))),
                    )
                    .child(
                        Button::new("skills-export")
                            .small()
                            .icon(IconName::Copy)
                            .tooltip("Export skills to clipboard")
                            .on_click(cx.listener(|e, _, _, cx| e.export_skills(cx))),
                    ),
            )
    }

    fn implant_row(
        &self,
        id: SharedString,
        implant: TypeId,
        trailing: Option<AnyElement>,
        cx: &Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let catalog = &self.loaded.catalog;
        let slot = catalog.get(implant).and_then(|t| t.implant_slot);
        h_flex()
            .id(id)
            .gap_2()
            .h(px(ROW_HEIGHT + 2.))
            .px_2()
            .hover(|s| s.bg(cx.theme().table_hover))
            .child(
                div()
                    .w(px(18.))
                    .text_color(cx.theme().muted_foreground)
                    .child(slot.map_or_default(|s| s.to_string())),
            )
            .child(icon_box(
                catalog,
                &self.loaded.icons,
                Icon::Type(implant),
                20.,
            ))
            .child(div().flex_1().truncate().child(catalog.name(implant)))
            .children(trailing)
    }

    fn implants_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        if self.selected.is_builtin() {
            return div()
                .p_4()
                .text_color(theme.muted_foreground)
                .child("All 0 and All 5 have no implants. Pick or make another character.")
                .into_any_element();
        }
        let catalog = &self.loaded.catalog;
        let query = self.implant_search.read(cx).value().to_string();
        let hits: Vec<TypeId> = catalog
            .search(&query, SEARCH_LIMIT)
            .into_iter()
            .filter(|&t| catalog.get(t).is_some_and(|t| t.kind == Kind::Implant))
            .collect();
        let available: Vec<AnyElement> = hits
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                self.implant_row(SharedString::from(format!("available-{i}")), t, None, cx)
                    .on_click(cx.listener(move |e, _: &ClickEvent, _, cx| e.add_implant(t, cx)))
                    .into_any_element()
            })
            .collect();
        let plugged: Vec<AnyElement> = self
            .characters
            .implants(self.selected)
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let remove = Button::new(SharedString::from(format!("implant-remove-{i}")))
                    .ghost()
                    .xsmall()
                    .label("×")
                    .tooltip("Remove implant")
                    .on_click(cx.listener(move |e, _, _, cx| e.remove_implant(t, cx)));
                self.implant_row(
                    SharedString::from(format!("plugged-{i}")),
                    t,
                    Some(remove.into_any_element()),
                    cx,
                )
                .on_click(cx.listener(move |e, event: &ClickEvent, _, cx| {
                    if event.click_count() >= 2 {
                        e.remove_implant(t, cx);
                    }
                }))
                .into_any_element()
            })
            .collect();
        let hint = query
            .trim()
            .is_empty()
            .then_some("Search for implants, then click one to plug it in.");
        let sets: Vec<(String, Vec<TypeId>)> = self
            .hub
            .read(cx)
            .saved
            .implant_sets
            .iter()
            .map(|s| (s.name.clone(), s.implants.clone()))
            .collect();
        let this = cx.entity().downgrade();
        h_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .gap_1()
                    .child(Input::new(&self.implant_search).small())
                    .child(
                        v_flex()
                            .id("editor-available")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .border_1()
                            .border_color(theme.border)
                            .rounded(px(4.))
                            .when_some(hint, |pane, hint| {
                                pane.child(
                                    div().p_2().text_color(theme.muted_foreground).child(hint),
                                )
                            })
                            .children(available),
                    ),
            )
            .child(
                v_flex()
                    .id("editor-plugged")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(px(4.))
                    .when(plugged.is_empty(), |pane| {
                        pane.child(
                            div()
                                .p_2()
                                .text_color(theme.muted_foreground)
                                .child("No implants plugged in."),
                        )
                    })
                    .children(plugged)
                    // pyfa's implantEditor menu.
                    .context_menu(move |menu, window, cx| {
                        apply_set_menu(menu, &this, &sets, window, cx)
                    }),
            )
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let builtin = self.selected.is_builtin();
        let dirty = self.characters.is_dirty(self.selected);
        h_flex()
            .gap_2()
            .child(
                Button::new("character-save")
                    .small()
                    .label("Save")
                    .disabled(builtin || !dirty)
                    .on_click(cx.listener(|e, _, _, cx| e.save(cx))),
            )
            .child(
                Button::new("character-save-as")
                    .small()
                    .label("Save As…")
                    .disabled(!dirty)
                    .on_click(cx.listener(|e, _, window, cx| e.save_as(window, cx))),
            )
            .child(
                Button::new("character-revert")
                    .small()
                    .label("Revert")
                    .disabled(!dirty)
                    .on_click(cx.listener(|e, _, _, cx| e.revert(cx))),
            )
            .when_some(self.notice.clone(), |row, notice| {
                row.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(notice),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("character-ok")
                    .small()
                    .primary()
                    .label("OK")
                    .on_click(|_, window, _| window.remove_window()),
            )
    }
}

fn apply_set_menu(
    menu: PopupMenu,
    this: &gpui_kit::WeakEntity<CharacterEditor>,
    sets: &[(String, Vec<TypeId>)],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if sets.is_empty() {
        return menu;
    }
    let (this, sets) = (this.clone(), sets.to_vec());
    menu.submenu_with_element(
        "Apply Implant Set",
        icon_label(IconName::Plus, "Apply Implant Set"),
        window,
        cx,
        move |menu, _, _| {
            sets.iter().fold(menu, |menu, (name, implants)| {
                let (this, implants) = (this.clone(), implants.clone());
                menu.item(
                    entry(IconName::Plus, name.clone()).on_click(move |_, _, cx| {
                        let _ = this.update(cx, |e, cx| e.apply_implant_set(&implants, cx));
                    }),
                )
            })
        },
    )
}

/// The skill tree: groups by name, each with its skills by name, as pyfa's tree lists them.
fn skill_groups(loaded: &Loaded) -> Vec<Group> {
    let catalog = &loaded.catalog;
    let mut groups: Vec<Group> = Vec::new();
    for &skill in &catalog.skills {
        let Some(t) = catalog.get(skill) else {
            continue;
        };
        let entry = (skill, t.name.clone());
        match groups.iter_mut().find(|g| g.id == t.group_id) {
            Some(group) => group.skills.push(entry),
            None => groups.push(Group {
                id: t.group_id,
                name: catalog
                    .group_names
                    .get(&t.group_id)
                    .cloned()
                    .unwrap_or_default(),
                skills: vec![entry],
            }),
        }
    }
    for group in &mut groups {
        group.skills.sort_by(|a, b| a.1.cmp(&b.1));
    }
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    groups
}

impl Render for CharacterEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Another window may have deleted the character shown here.
        let resolved = self.characters.resolve(self.selected);
        if resolved != self.selected {
            self.selected = resolved;
            self.skill = None;
        }
        let body = match self.tab {
            Tab::Skills => self.skills_tab(cx).into_any_element(),
            Tab::Implants => self.implants_tab(cx),
        };
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(self.header(cx))
            .child(self.tab_bar(cx))
            .child(v_flex().flex_1().min_h_0().child(body))
            .child(self.footer(cx))
            // Escape closes the editor, as in pyfa.
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                let key = &event.keystroke;
                if key.key == "escape" && !key.modifiers.modified() {
                    window.remove_window();
                }
            }))
    }
}
