//! pyfa's Character Editor (gui/characterEditor.py), in a window of its own: pick, create,
//! rename, copy and delete characters, set skill levels in the skill tree, plug in implants,
//! set the security status, and save or revert pending changes.
//!
//! The characters live in the [`Hub`]; every change goes through it, so the fitting windows
//! recalculate with it right away.

mod esi;
mod implants;
mod skills;

use std::{collections::HashSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::InputState,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AppContext, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, Styled, Subscription, Task, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Hub, Loaded,
    characters::name_dialog,
    dialogs::{Confirm, Prompt, confirm, prompt_value},
    editor_kit::ItemTree,
    widgets::{count, redraw_on_change},
};
use crate::{
    character::{CharacterId, Characters, Keep, MAX_LEVEL},
    data::TypeId,
};
use implants::is_implant;
use skills::{Group, skill_groups};

const ROW_HEIGHT: f32 = 24.;
const MENU_MAX_H: f32 = 480.;
/// What pyfa says before importing skills.
const IMPORT_NOTICE: &str = "Imported skills are pending: click Save to keep them.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Skills,
    Implants,
    /// pyfa's EVE SSO tab: the linked EVE character and Get Skills.
    Esi,
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
    /// The implants to plug in, as the market's tree.
    implant_tree: ItemTree,
    tree_focus: FocusHandle,
    notice: Option<SharedString>,
    /// A Get Skills request on its way.
    esi_fetch: Option<Task<()>>,
    /// What the last ESI request came to, and whether it failed.
    esi_message: Option<(SharedString, bool)>,
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
        window.set_window_title("efa: Character Editor");
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        let implant_tree =
            ItemTree::new(&loaded.catalog, is_implant, "Search implants", window, cx);
        let subscriptions = vec![
            cx.observe(&hub, |this: &mut Self, hub, cx| {
                let characters = &hub.read(cx).characters;
                if !Rc::ptr_eq(characters, &this.characters) {
                    this.characters = Rc::clone(characters);
                    cx.notify();
                }
            }),
            // Logins and their progress show on the EVE SSO tab.
            cx.observe(&hub, |this: &mut Self, _, cx| {
                if this.tab == Tab::Esi {
                    cx.notify();
                }
            }),
            redraw_on_change(cx, &search),
            redraw_on_change(cx, &implant_tree.search),
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
            implant_tree,
            tree_focus: cx.focus_handle(),
            notice: None,
            esi_fetch: None,
            esi_message: None,
            _subscriptions: subscriptions,
        }
    }

    /// Show another character, as picking it in the list does.
    pub fn select(&mut self, id: CharacterId, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = self.characters.resolve(id);
        self.skill = None;
        self.notice = None;
        self.esi_message = None;
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
            .settings
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
        let this = cx.weak_entity();
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
        let this = cx.weak_entity();
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
        let this = cx.weak_entity();
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
        let this = cx.weak_entity();
        confirm(window, cx, Confirm::delete(question), move |_, cx| {
            let _ = this.update(cx, |e, cx| {
                e.edit(cx, |c, _| c.delete(id));
                e.selected = CharacterId::All5;
                e.skill = None;
            });
        });
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, _| c.save_levels(id));
    }

    fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.selected;
        let this = cx.weak_entity();
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
            Ok(errors) => format!(
                "{IMPORT_NOTICE} {} could not be read.",
                count(errors, "line", "lines")
            )
            .into(),
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
        let this = cx.weak_entity();
        let prompt = Prompt::new("Set Security Status")
            .message("Security Status is used in some CONCORD hull calculations")
            .default(format!("{current:.2}"))
            .width(320.);
        prompt_value(window, cx, prompt, move |value: f64, _, cx| {
            let _ = this.update(cx, |e, cx| {
                e.edit(cx, |c, _| c.set_sec_status(id, value));
            });
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
        let this = cx.weak_entity();
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
                [
                    (Tab::Skills, "Skills"),
                    (Tab::Implants, "Implants"),
                    (Tab::Esi, "EVE SSO"),
                ]
                .map(|(tab, label)| {
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
                        .on_mouse_down(
                            gpui_kit::MouseButton::Left,
                            cx.listener(move |e, _: &gpui_kit::MouseDownEvent, _, cx| {
                                e.tab = tab;
                                cx.notify();
                            }),
                        )
                }),
            )
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
            Tab::Esi => self.esi_tab(cx),
        };
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
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
