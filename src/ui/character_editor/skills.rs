//! The Skills tab: the skill tree by group, levels and the Alpha clone choice.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::Button,
    h_flex,
    input::Input,
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};

use super::CharacterEditor;
use crate::ui::{
    Loaded,
    widgets::{entry, tree_row},
};
use crate::{character::MAX_LEVEL, data::TypeId};

/// The clone choice's label for no Alpha clone.
const OMEGA: &str = "Omega Clone";

/// A skill group of the tree, with its skills by name.
pub(super) struct Group {
    id: i32,
    name: String,
    skills: Vec<(TypeId, String)>,
}

impl CharacterEditor {
    /// The level column's text, pyfa's "Level N" or "Not learned", with the trained level
    /// after it when an Alpha clone caps the skill.
    fn level_text(&self, skill: TypeId) -> (String, bool) {
        let id = self.selected;
        let trained = self.characters.level(id, skill);
        let flown = self.characters.flown_level(id, skill, &self.loaded.catalog);
        match (trained, flown) {
            (Some(trained), Some(flown)) if flown < trained => {
                (format!("Level {flown} (of {trained})"), true)
            }
            (Some(level), _) => (format!("Level {level}"), false),
            (None, _) => ("Not learned".to_owned(), false),
        }
    }

    fn set_alpha_clone(&mut self, clone: Option<i32>, cx: &mut Context<Self>) {
        let id = self.selected;
        if id.is_builtin() {
            return;
        }
        self.edit(cx, |c, _| c.set_alpha_clone(id, clone));
    }

    /// pyfa's clone choice: Omega, or an Alpha clone that caps the skills.
    fn clone_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.characters.alpha_clone(self.selected);
        let catalog = &self.loaded.catalog;
        let label = current
            .and_then(|c| catalog.alpha_clone(c))
            .map_or_else(|| OMEGA.to_owned(), |c| c.name.clone());
        let choices: Vec<(Option<i32>, String)> = std::iter::once((None, OMEGA.to_owned()))
            .chain(
                catalog
                    .alpha_clones
                    .iter()
                    .map(|c| (Some(c.id), c.name.clone())),
            )
            .collect();
        let this = cx.weak_entity();
        Button::new("alpha-clone")
            .small()
            .outline()
            .label(label)
            .tooltip(
                "Setting an Alpha clone does not replace the character's skills, \
                 but rather caps them to Alpha levels.",
            )
            .disabled(self.selected.is_builtin())
            .dropdown_menu(move |menu, _, _| {
                choices.iter().fold(menu, |menu, (clone, name)| {
                    let (this, clone) = (this.clone(), *clone);
                    menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(clone == current)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |e, cx| e.set_alpha_clone(clone, cx));
                            }),
                    )
                })
            })
    }

    fn skill_row(
        &self,
        skill: TypeId,
        name: &str,
        indent: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dirty = self.characters.is_skill_dirty(self.selected, skill);
        let selected = self.skill == Some(skill);
        let builtin = self.selected.is_builtin();
        let icon = skill_icon(dirty, cx);
        let row = tree_row(
            SharedString::from(format!("skill-{skill}")),
            usize::from(indent),
            None,
            Some(icon),
            name.to_owned(),
            selected,
            cx,
        );
        let theme = cx.theme();
        let row = row
            .child({
                let (text, capped) = self.level_text(skill);
                div()
                    .w(px(130.))
                    .text_color(if dirty {
                        theme.danger
                    } else if capped {
                        theme.warning
                    } else {
                        theme.foreground
                    })
                    .child(text)
            })
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
        let this = cx.weak_entity();
        row.context_menu(move |menu, _, _| {
            let stats = {
                let this = this.clone();
                entry(IconName::Info, "Skill Stats").on_click(move |event, window, cx| {
                    if let Some(e) = this.upgrade() {
                        let (loaded, hub) = {
                            let e = e.read(cx);
                            (Rc::clone(&e.loaded), e.hub.clone())
                        };
                        let request = crate::ui::item_stats::Request {
                            target: crate::ui::item_stats::Target::Type(skill),
                            fit: None,
                        };
                        let reuse = event.modifiers().shift;
                        crate::ui::item_stats::open(&loaded, &hub, request, reuse, window, cx);
                    }
                })
            };
            let menu = menu.item(stats);
            if builtin {
                return menu;
            }
            let set = |level: Option<u8>| {
                let this = this.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |e, cx| e.set_level(skill, level, cx));
                }
            };
            let menu = menu
                .separator()
                .item(PopupMenuItem::new("Unlearn").on_click(set(None)));
            (0..=MAX_LEVEL).fold(menu, |menu, level| {
                menu.item(PopupMenuItem::new(format!("Level {level}")).on_click(set(Some(level))))
            })
        })
        .into_any_element()
    }

    pub(super) fn skills_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    tree_row(
                        SharedString::from(format!("skill-group-{id}")),
                        0,
                        Some(open),
                        Some(skill_icon(dirty, cx)),
                        group.name.clone(),
                        false,
                        cx,
                    )
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
                    .child(self.clone_picker(cx))
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
}

/// The skill tree: groups by name, each with its skills by name, as pyfa's tree lists them.
pub(super) fn skill_groups(loaded: &Loaded) -> Vec<Group> {
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

/// A skill's or group's book, red while it has unsaved levels.
fn skill_icon(dirty: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    gpui_kit::component::Icon::new(IconName::BookOpen)
        .size(px(14.))
        .text_color(if dirty {
            theme.danger
        } else {
            theme.muted_foreground
        })
        .into_any_element()
}
