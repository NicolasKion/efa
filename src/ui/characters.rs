//! The fit's character in the main window: the picker and missing-skills icon at the top of the
//! stats pane (gui/characterSelection.py), the Change Skills menus (skillAffectors.py) and the
//! Character menu (mainMenuBar.py).

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement, Styled, WeakEntity,
    Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder, px, size,
};

use super::{
    Gyfa, Hub, Loaded,
    character_editor::CharacterEditor,
    widgets::{entry, icon_label},
};
use crate::{
    character::{
        CharacterId, Characters, MAX_LEVEL,
        evemon::{self, PlanFormat},
        requirements::{self, ItemRequirements, Numerals, TooltipStyle},
    },
    data::{Kind, TypeId},
    engine::{Pilot, SkillTarget},
};

/// Tallest a skills menu gets before it scrolls.
const MENU_MAX_H: f32 = 480.;

/// The three ways pyfa copies missing skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyStyle {
    /// Every missing skill once, "Name level".
    List,
    /// Only skills no other listed skill needs at that level anyway.
    Condensed,
    /// Like `List`, with Roman numerals for `EVEMon`.
    EveMon,
}

impl Gyfa {
    /// The current fit's character, or All 5 without a fit or once its character is deleted.
    pub(super) fn fit_character(&self) -> CharacterId {
        self.fit()
            .map_or(CharacterId::All5, |f| self.characters.resolve(f.character))
    }

    /// Implants plugged into the current fit's character.
    pub(super) fn character_implants(&self) -> &[TypeId] {
        self.characters.implants(self.fit_character())
    }

    /// The pilot to calculate the current fit with, built again only when the character or the
    /// characters changed.
    pub(super) fn pilot(&mut self, loaded: &Loaded) -> Rc<Pilot> {
        let id = self.fit_character();
        if let Some((characters, cached, pilot)) = &self.pilot_cache
            && Rc::ptr_eq(characters, &self.characters)
            && *cached == id
        {
            return Rc::clone(pilot);
        }
        let pilot = Rc::new(self.characters.pilot(id, &loaded.catalog));
        self.pilot_cache = Some((Rc::clone(&self.characters), id, Rc::clone(&pilot)));
        pilot
    }

    /// Change the characters through the hub, which saves them and tells every window, then
    /// recalculate with the result.
    pub(super) fn edit_characters<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Characters, &Loaded) -> R,
    ) -> Option<R> {
        let loaded = self.loaded()?;
        let result = self
            .hub
            .update(cx, |hub, cx| hub.edit_characters(cx, |c| edit(c, &loaded)));
        self.characters = Rc::clone(&self.hub.read(cx).characters);
        self.recompute();
        cx.notify();
        Some(result)
    }

    /// Fly the current fit with another character. New fits get it too, as in pyfa.
    fn set_fit_character(&mut self, id: CharacterId, cx: &mut Context<Self>) {
        let Some(i) = self.saved.current else { return };
        self.last_character = id;
        self.saved.fits[i].character = id;
        self.changed(cx);
    }

    /// Set a skill of the fit's character, pending until the character is saved, as pyfa's
    /// Change Skills menus do.
    fn change_skill_level(&mut self, skill: TypeId, level: Option<u8>, cx: &mut Context<Self>) {
        let id = self.fit_character();
        let strict = self.saved.preferences.engine.strict_skill_levels;
        self.edit_characters(cx, |c, loaded| {
            c.change_level_with(
                id,
                skill,
                level,
                crate::character::Keep::Pending,
                strict,
                &loaded.catalog,
            );
        });
    }

    /// What the current fit needs that its character lacks.
    pub(super) fn missing_skills(&self, loaded: &Loaded) -> Vec<ItemRequirements> {
        let Some(fit) = self.fit() else {
            return Vec::new();
        };
        let id = self.fit_character();
        requirements::missing(fit, &loaded.catalog, self.character_implants(), |s| {
            self.characters.flown_level_or_zero(id, s, &loaded.catalog)
        })
    }

    fn grant_missing_skills(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let skills = requirements::condensed(&self.missing_skills(&loaded), &loaded.catalog);
        let id = self.fit_character();
        self.edit_characters(cx, |c, loaded| {
            for (skill, level) in skills {
                c.raise_level(id, skill, level, &loaded.catalog);
            }
        });
    }

    fn copy_missing_skills(&mut self, style: CopyStyle, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let catalog = &loaded.catalog;
        let missing = self.missing_skills(&loaded);
        let text = match style {
            CopyStyle::List => requirements::lines(
                &requirements::condensed(&missing, catalog),
                catalog,
                Numerals::Arabic,
            ),
            CopyStyle::Condensed => requirements::lines(
                &requirements::super_condensed(&missing, catalog),
                catalog,
                Numerals::Arabic,
            ),
            CopyStyle::EveMon => requirements::lines(
                &requirements::condensed(&missing, catalog),
                catalog,
                Numerals::Roman,
            ),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Open the character editor in a window of its own, or bring the open one forward.
    pub(super) fn open_character_editor(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let select = self.fit_character();
        let open = self.hub.read(cx).character_editor.clone();
        if let Some((handle, editor)) = open {
            let shown = handle.update(cx, |_, window, cx| {
                let _ = editor.update(cx, |e, cx| e.select(select, window, cx));
                window.activate_window();
            });
            if shown.is_ok() {
                return;
            }
        }
        let hub = self.hub.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(950.), px(680.)), cx)),
            app_id: Some("gyfa".into()),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| CharacterEditor::new(hub, loaded, select, window, cx))
        });
        match opened {
            Ok((handle, editor)) => self.hub.update(cx, |hub, _| {
                hub.character_editor = Some((handle, editor.downgrade()));
            }),
            Err(e) => {
                self.status = Some(format!("Could not open the character editor: {e}").into());
                cx.notify();
            }
        }
    }

    /// pyfa's Import Character File: `EVEMon` character sheets, each a new character.
    fn import_character_files(cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let _ = this.update(cx, |g, cx| {
                let (mut imported, mut failed) = (0, Vec::new());
                let sheets: Vec<_> = paths
                    .iter()
                    .map(|path| {
                        (
                            path,
                            std::fs::read_to_string(path).map_err(|e| e.to_string()),
                        )
                    })
                    .collect();
                g.edit_characters(cx, |c, loaded| {
                    for (path, text) in sheets {
                        match text.and_then(|t| evemon::import_sheet(&t, &loaded.catalog)) {
                            Ok(character) => {
                                c.add(character);
                                imported += 1;
                            }
                            Err(e) => failed.push(format!("{}: {e}", path.display())),
                        }
                    }
                });
                g.status = Some(if failed.is_empty() {
                    format!("Imported {imported} character(s)").into()
                } else {
                    format!("Could not import {}", failed.join("; ")).into()
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// pyfa's Export Skills Needed: the fit's missing skills as an `EVEMon` plan (`.emp`, `.xml`)
    /// or text (`.txt`), by the extension given.
    fn export_skills_needed(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let catalog = &loaded.catalog;
        let name = self.characters.name(self.fit_character());
        let plan = requirements::plan(&self.missing_skills(&loaded), catalog);
        let (xml, text) = (
            requirements::plan_xml(&name, &plan),
            requirements::plan_text(&name, &plan),
        );
        let dir = std::env::var_os("HOME").map_or_else(std::path::PathBuf::new, Into::into);
        let path = cx.prompt_for_new_path(&dir, Some("skills.emp"));
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let written = evemon::plan_file(PlanFormat::for_path(&path), &xml, &text)
                .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()));
            let _ = this.update(cx, |g, cx| {
                g.status = Some(match written {
                    Ok(()) => format!("Saved skills to {}", path.display()).into(),
                    Err(e) => format!("Could not save skills: {e}").into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// The picker row at the top of the stats pane.
    pub(super) fn character_bar(&self, loaded: &Loaded, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let has_fit = self.fit().is_some();
        let current = self.fit_character();
        let name = self.characters.name(current);
        let missing = self.missing_skills(loaded);
        let needs_skills = has_fit && !missing.is_empty();
        let tip: SharedString = if has_fit {
            let style = if self.saved.preferences.general.compact_skills {
                TooltipStyle::Condensed
            } else {
                TooltipStyle::Full
            };
            requirements::tooltip(&missing, &loaded.catalog, style).into()
        } else {
            "No active fit".into()
        };
        let color = match (has_fit, needs_skills) {
            (false, _) => theme.muted_foreground,
            (true, true) => theme.danger,
            (true, false) => theme.success,
        };
        let this = Self::weak(cx);
        let choices: Vec<(CharacterId, String)> = self
            .characters
            .ordered()
            .into_iter()
            .map(|id| (id, self.characters.name(id)))
            .collect();
        let picker = Button::new("character-picker")
            .small()
            .outline()
            .label(name)
            .disabled(!has_fit)
            .dropdown_menu({
                let this = this.clone();
                move |menu, _, _| {
                    let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
                    for (id, name) in &choices {
                        let (this, id) = (this.clone(), *id);
                        menu = menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(id == current)
                                .on_click(move |_, _, cx| {
                                    Gyfa::app_update(&this, cx, |g, cx| {
                                        g.set_fit_character(id, cx);
                                    });
                                }),
                        );
                    }
                    let this = this.clone();
                    menu.separator()
                        .item(entry(IconName::User, "Open Character Editor").on_click(
                            move |_, _, cx| {
                                Gyfa::app_update(&this, cx, Gyfa::open_character_editor);
                            },
                        ))
                }
            });
        let icon = div()
            .id("missing-skills")
            .flex_none()
            .child(
                gpui_kit::component::Icon::new(IconName::BookOpen)
                    .size(px(16.))
                    .text_color(color),
            )
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
        let icon = if needs_skills {
            icon.context_menu(move |menu, _, _| missing_skills_menu(menu, &this))
                .into_any_element()
        } else {
            icon.into_any_element()
        };
        h_flex()
            .gap_2()
            .p_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child("Character:")
            .child(div().flex_1().min_w_0().child(picker))
            .child(icon)
            .into_any_element()
    }

    /// The Character menu of the menu bar.
    pub(super) fn character_menu(menu: PopupMenu, this: &WeakEntity<Self>, cx: &App) -> PopupMenu {
        let (has_fit, dirty, builtin) = this.upgrade().map_or((false, false, true), |g| {
            let g = g.read(cx);
            let id = g.fit_character();
            (
                g.fit().is_some(),
                g.characters.is_dirty(id),
                id.is_builtin(),
            )
        });
        let on = |f: fn(&mut Gyfa, &mut Window, &mut Context<Gyfa>)| {
            let this = this.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |g, cx| f(g, window, cx));
            }
        };
        menu.item(
            entry(IconName::User, "Character Editor…")
                .on_click(on(|g, _, cx| g.open_character_editor(cx))),
        )
        .separator()
        .item(
            entry(IconName::Check, "Save Character")
                .disabled(builtin || !dirty)
                .on_click(on(|g, _, cx| {
                    let id = g.fit_character();
                    g.edit_characters(cx, |c, _| c.save_levels(id));
                })),
        )
        .item(
            entry(IconName::Copy, "Save Character As…")
                .disabled(!dirty)
                .on_click(on(|g, window, cx| {
                    let id = g.fit_character();
                    let initial = format!("{} Copy", g.characters.name(id));
                    let this = cx.entity().downgrade();
                    name_dialog(
                        &g.hub,
                        "Save Character As...",
                        "Enter a name for your new Character:",
                        initial,
                        window,
                        cx,
                        move |name, cx| {
                            Gyfa::app_update(&this, cx, |g, cx| {
                                g.edit_characters(cx, |c, loaded| {
                                    c.save_as(id, &name, &loaded.catalog);
                                });
                            });
                        },
                    );
                })),
        )
        .item(
            entry(IconName::Undo2, "Revert Character")
                .disabled(!dirty)
                .on_click(on(|g, _, cx| {
                    let id = g.fit_character();
                    g.edit_characters(cx, |c, _| c.revert_levels(id));
                })),
        )
        .separator()
        .item(
            entry(IconName::FolderOpen, "Import Character File…")
                .on_click(on(|_, _, cx| Gyfa::import_character_files(cx))),
        )
        .item(
            entry(IconName::FileText, "Export Skills Needed…")
                .disabled(!has_fit)
                .on_click(on(|g, _, cx| g.export_skills_needed(cx))),
        )
    }
}

/// The missing-skills icon's menu.
fn missing_skills_menu(menu: PopupMenu, this: &WeakEntity<Gyfa>) -> PopupMenu {
    let copy = |label: &'static str, style: CopyStyle| {
        let this = this.clone();
        entry(IconName::Copy, label).on_click(move |_, _, cx| {
            Gyfa::app_update(&this, cx, |g, cx| g.copy_missing_skills(style, cx));
        })
    };
    let grant = this.clone();
    menu.item(
        entry(IconName::Plus, "Grant Missing Skills").on_click(move |_, _, cx| {
            Gyfa::app_update(&grant, cx, Gyfa::grant_missing_skills);
        }),
    )
    .item(copy("Copy Missing Skills", CopyStyle::List))
    .item(copy(
        "Copy Missing Skills (condensed)",
        CopyStyle::Condensed,
    ))
    .item(copy("Copy Missing Skills (EVEMon)", CopyStyle::EveMon))
}

/// pyfa's "Change {X} Skills" submenu: the skills that change the item, each with its levels.
/// Nothing is added when no skill changes it.
pub(super) fn skills_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    loaded: &Rc<Loaded>,
    target: SkillTarget,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if !super::hub::preferences(cx).context_menus.change_skills {
        return menu;
    }
    let Some(g) = this.upgrade() else { return menu };
    let (ship, character, characters) = {
        let g = g.read(cx);
        let Some(fit) = g.fit() else { return menu };
        (fit.ship, g.fit_character(), Rc::clone(&g.characters))
    };
    let catalog = &loaded.catalog;
    let mut skills: Vec<(TypeId, String)> = loaded
        .engine
        .affecting_skills(ship, target)
        .into_iter()
        .map(|s| (s, catalog.name(s)))
        .collect();
    if skills.is_empty() {
        return menu;
    }
    skills.sort_by(|a, b| a.1.cmp(&b.1));
    let structure = ship
        .and_then(|s| catalog.get(s))
        .is_some_and(crate::data::ItemType::is_structure);
    let noun = match target {
        SkillTarget::Ship if structure => "Citadel",
        SkillTarget::Ship => "Ship",
        SkillTarget::Module(..) => "Module",
        SkillTarget::Charge { .. } => "Charge",
        SkillTarget::Drone(_) => "Drone",
        SkillTarget::Fighter(_) => "Fighter",
    };
    let title = format!("Change {noun} Skills");
    let this = this.clone();
    menu.submenu_with_element(
        title.clone(),
        icon_label(IconName::BookOpen, title),
        window,
        cx,
        move |menu, window, cx| {
            let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
            for (skill, name) in &skills {
                let skill = *skill;
                let level = characters.level(character, skill);
                let label = match level {
                    Some(level) => format!("{name} ({level})"),
                    None => name.clone(),
                };
                let this = this.clone();
                menu = menu.submenu(label, window, cx, move |menu, _, _| {
                    let set = |l: Option<u8>| {
                        let this = this.clone();
                        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                            Gyfa::app_update(&this, cx, |g, cx| g.change_skill_level(skill, l, cx));
                        }
                    };
                    // The built-ins keep their levels whatever is picked, as in pyfa.
                    let menu = menu.item(
                        PopupMenuItem::new("Not Learned")
                            .checked(level.is_none())
                            .disabled(character.is_builtin())
                            .on_click(set(None)),
                    );
                    (0..=MAX_LEVEL).fold(menu, |menu, l| {
                        menu.item(
                            PopupMenuItem::new(format!("Level {l}"))
                                .checked(level == Some(l))
                                .disabled(character.is_builtin())
                                .on_click(set(Some(l))),
                        )
                    })
                });
            }
            menu
        },
    )
}

/// The skill target for a drone-pane stack: a drone or a fighter.
pub(super) fn stack_target(loaded: &Loaded, type_id: TypeId) -> Option<SkillTarget> {
    match loaded.catalog.get(type_id)?.kind {
        Kind::Drone => Some(SkillTarget::Drone(type_id)),
        Kind::Fighter => Some(SkillTarget::Fighter(type_id)),
        _ => None,
    }
}

/// Ask for a character name and hand it on once it is one no character has, pyfa's
/// `TextEntryValidatedDialog`.
pub(super) fn name_dialog(
    hub: &Entity<Hub>,
    title: &'static str,
    prompt: &'static str,
    initial: String,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(String, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
    let error = cx.new(|_| None::<SharedString>);
    let hub = hub.downgrade();
    let on_ok = Rc::new(on_ok);
    window.open_dialog(cx, move |dialog, _, cx| {
        let (input, error, hub, on_ok) =
            (input.clone(), error.clone(), hub.clone(), Rc::clone(&on_ok));
        let message = error.read(cx).clone();
        // The OK button and Enter both submit; it closes the dialog itself when done.
        let submit: crate::ui::widgets::Submit = {
            let (input, error, hub, on_ok) =
                (input.clone(), error.clone(), hub.clone(), on_ok.clone());
            Rc::new(
                move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
                    let name = input.read(cx).value().trim().to_owned();
                    let problem = hub
                        .upgrade()
                        .and_then(|h| h.read(cx).characters.name_problem(&name));
                    if let Some(problem) = problem {
                        error.update(cx, |e, cx| {
                            *e = Some(problem.into());
                            cx.notify();
                        });
                        window.refresh();
                        return;
                    }
                    window.close_dialog(cx);
                    on_ok(name, cx);
                },
            )
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
            .w(px(360.))
            .child(
                v_flex()
                    .gap_3()
                    .child(prompt)
                    .child(Input::new(&input))
                    .when_some(message, |col, message| {
                        col.child(div().text_sm().text_color(cx.theme().danger).child(message))
                    })
                    .child(
                        h_flex().justify_end().child(
                            Button::new("character-name-ok")
                                .primary()
                                .label("OK")
                                .on_click({
                                    let submit = Rc::clone(&submit);
                                    move |e: &gpui_kit::ClickEvent, w: &mut Window, cx: &mut App| {
                                        submit(e, w, cx);
                                    }
                                }),
                        ),
                    ),
            )
    });
}
