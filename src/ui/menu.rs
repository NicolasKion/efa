//! The menu bar and its keyboard shortcuts, laid out like pyfa's (gui/mainMenuBar.py): File,
//! Fit, Character, Global, Editors, Help.
//!
//! Left out: ESI fittings and character management (they need EVE SSO), attribute overrides and
//! Optimize Fit Price. Entries that belong to windows built
//! elsewhere are marked "Hook:" below and stay disabled until those windows exist.

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, Div, InteractiveElement, IntoElement, KeyBinding, ParentElement,
    SharedString, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::Gyfa;

/// Key context of a fit window, so text fields keep their own Ctrl+C, Ctrl+V and Ctrl+Z.
const CONTEXT: &str = "Gyfa";

const WIKI_URL: &str = "https://github.com/pyfa-org/Pyfa/wiki";
const FORUM_URL: &str = "https://forums.eveonline.com/t/27156";

gpui_kit::actions!(
    gyfa,
    [
        NewTab,
        CloseTab,
        CloseAllTabs,
        Undo,
        Redo,
        CopyFit,
        PasteFit,
        ImportFits,
        ExportFit,
        OpenPreferences,
        ImplantSetEditor,
        CharacterEditor,
        Graphs,
        HiddenGraphs,
        Quit,
    ]
);

/// Bind pyfa's shortcuts. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-t", NewTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-w", CloseTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-w", CloseAllTabs, Some(CONTEXT)),
        KeyBinding::new("ctrl-z", Undo, Some(CONTEXT)),
        KeyBinding::new("ctrl-y", Redo, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", CopyFit, Some(CONTEXT)),
        KeyBinding::new("ctrl-v", PasteFit, Some(CONTEXT)),
        KeyBinding::new("ctrl-o", ImportFits, Some(CONTEXT)),
        KeyBinding::new("ctrl-s", ExportFit, Some(CONTEXT)),
        KeyBinding::new("ctrl-p", OpenPreferences, Some(CONTEXT)),
        KeyBinding::new("ctrl-i", ImplantSetEditor, Some(CONTEXT)),
        KeyBinding::new("ctrl-k", CharacterEditor, Some(CONTEXT)),
        KeyBinding::new("ctrl-q", Quit, Some(CONTEXT)),
        KeyBinding::new("ctrl-g", Graphs, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-g", HiddenGraphs, Some(CONTEXT)),
    ]);
}

/// A menu entry that runs `f` on the window and shows `action`'s shortcut.
fn command(
    this: &WeakEntity<Gyfa>,
    icon: IconName,
    label: &'static str,
    action: Option<Box<dyn gpui_kit::Action>>,
    f: fn(&mut Gyfa, &mut Window, &mut Context<Gyfa>),
) -> PopupMenuItem {
    let this = this.clone();
    let item = PopupMenuItem::new(label)
        .icon(icon)
        .on_click(move |_: &ClickEvent, window, cx| {
            let _ = this.update(cx, |g, cx| f(g, window, cx));
        });
    match action {
        Some(action) => item.action(action),
        None => item,
    }
}

impl Gyfa {
    /// The root element with the menu's shortcuts wired to this window.
    pub(super) fn bind_actions(root: Div, cx: &mut Context<Self>) -> Div {
        root.key_context(CONTEXT)
            .on_action(cx.listener(|g, _: &NewTab, _, cx| g.new_tab(cx)))
            .on_action(cx.listener(|g, _: &CloseTab, _, cx| {
                if let Some(current) = g.saved.current {
                    g.close_fit(current, cx);
                }
            }))
            .on_action(cx.listener(|g, _: &CloseAllTabs, _, cx| g.close_all_tabs(cx)))
            .on_action(cx.listener(|g, _: &Undo, _, cx| g.undo(cx)))
            .on_action(cx.listener(|g, _: &Redo, _, cx| g.redo(cx)))
            .on_action(cx.listener(|g, _: &CopyFit, _, cx| g.open_export_window(cx)))
            .on_action(cx.listener(|g, _: &PasteFit, _, cx| g.paste_from_clipboard(cx)))
            .on_action(cx.listener(|_, _: &ImportFits, _, cx| Gyfa::import_files_dialog(cx)))
            .on_action(cx.listener(|g, _: &ExportFit, _, cx| g.save_fit_dialog(cx)))
            .on_action(cx.listener(|g, _: &OpenPreferences, _, cx| g.open_preferences(cx)))
            .on_action(cx.listener(|g, _: &CharacterEditor, _, cx| {
                g.open_character_editor(cx);
            }))
            .on_action(cx.listener(|g, _: &ImplantSetEditor, window, cx| {
                g.open_implant_set_editor(None, window, cx);
            }))
            .on_action(cx.listener(|g, _: &Graphs, _, cx| g.open_graphs(cx)))
            .on_action(cx.listener(|g, _: &HiddenGraphs, _, cx| g.open_hidden_graphs(cx)))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
    }

    pub(super) fn new_tab(&mut self, cx: &mut Context<Self>) {
        let mut fit = self.new_fit("New fit");
        self.new_fit_defaults(&mut fit);
        self.add_fit(fit, cx);
    }

    fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.open.clear();
        self.saved.current = None;
        self.changed(cx);
    }

    pub(super) fn menu_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let has_fit = self.saved.current.is_some();
        let has_fits = !self.saved.fits.is_empty();
        let unrestricted = self.fit().is_some_and(|f| f.ignore_restrictions);
        let (can_undo, can_redo) = (self.can_undo(), self.can_redo());
        let menu =
            |id: &'static str, label: &'static str, build: Box<dyn Fn(PopupMenu) -> PopupMenu>| {
                Button::new(id)
                    .ghost()
                    .small()
                    .label(label)
                    .dropdown_menu(move |menu, _, _| build(menu.min_w(px(220.))))
            };

        let file = {
            let this = this.clone();
            Box::new(move |menu: PopupMenu| {
                menu.item(command(
                    &this,
                    IconName::Plus,
                    "New Tab",
                    Some(Box::new(NewTab)),
                    |g, _, cx| g.new_tab(cx),
                ))
                .item(
                    command(
                        &this,
                        IconName::Close,
                        "Close Tab",
                        Some(Box::new(CloseTab)),
                        |g, _, cx| {
                            if let Some(current) = g.saved.current {
                                g.close_fit(current, cx);
                            }
                        },
                    )
                    .disabled(!has_fit),
                )
                .item(
                    command(
                        &this,
                        IconName::Close,
                        "Close All Tabs",
                        Some(Box::new(CloseAllTabs)),
                        |g, _, cx| g.close_all_tabs(cx),
                    )
                    .disabled(!has_fit),
                )
                .separator()
                .item(
                    command(
                        &this,
                        IconName::Folder,
                        "Backup All Fittings",
                        None,
                        |g, _, cx| g.backup_dialog(cx),
                    )
                    .disabled(!has_fits),
                )
                .item(
                    command(
                        &this,
                        IconName::Globe,
                        "Export All Fittings to HTML",
                        None,
                        |g, _, cx| g.export_html(cx),
                    )
                    .disabled(!has_fits),
                )
                .separator()
                .item(command(
                    &this,
                    IconName::WindowClose,
                    "Quit",
                    Some(Box::new(Quit)),
                    |_, _, cx| cx.quit(),
                ))
            }) as Box<dyn Fn(PopupMenu) -> PopupMenu>
        };

        let fit = {
            let this = this.clone();
            Box::new(move |menu: PopupMenu| {
                menu.item(
                    command(
                        &this,
                        IconName::Undo,
                        "Undo",
                        Some(Box::new(Undo)),
                        |g, _, cx| g.undo(cx),
                    )
                    .disabled(!can_undo),
                )
                .item(
                    command(
                        &this,
                        IconName::Redo,
                        "Redo",
                        Some(Box::new(Redo)),
                        |g, _, cx| g.redo(cx),
                    )
                    .disabled(!can_redo),
                )
                .separator()
                .item(
                    command(
                        &this,
                        IconName::Copy,
                        "To Clipboard",
                        Some(Box::new(CopyFit)),
                        |g, _, cx| g.open_export_window(cx),
                    )
                    .disabled(!has_fit),
                )
                .item(command(
                    &this,
                    IconName::FileText,
                    "From Clipboard",
                    Some(Box::new(PasteFit)),
                    |g, _, cx| g.paste_from_clipboard(cx),
                ))
                .item(command(
                    &this,
                    IconName::FileText,
                    "From Text…",
                    None,
                    |_, window, cx| Gyfa::open_import_dialog(window, cx),
                ))
                .separator()
                .item(command(
                    &this,
                    IconName::FolderOpen,
                    "Import Fittings",
                    Some(Box::new(ImportFits)),
                    |_, _, cx| Gyfa::import_files_dialog(cx),
                ))
                .item(
                    command(
                        &this,
                        IconName::File,
                        "Export Fitting",
                        Some(Box::new(ExportFit)),
                        |g, _, cx| g.save_fit_dialog(cx),
                    )
                    .disabled(!has_fit),
                )
                .separator()
                .item(command(
                    &this,
                    IconName::ChartPie,
                    "Graphs",
                    Some(Box::new(Graphs)),
                    |g, _, cx| g.open_graphs(cx),
                ))
                .separator()
                .item(
                    command(
                        &this,
                        IconName::TriangleAlert,
                        if unrestricted {
                            "Enable Fitting Restrictions"
                        } else {
                            "Disable Fitting Restrictions"
                        },
                        None,
                        Gyfa::confirm_restrictions_toggle,
                    )
                    .disabled(!has_fit),
                )
                .separator()
                .item(
                    command(&this, IconName::Copy, "Duplicate Fit", None, |g, _, cx| {
                        g.duplicate_fit(cx);
                    })
                    .disabled(!has_fit),
                )
                .item(
                    command(&this, IconName::Delete, "Delete Fit", None, |g, _, cx| {
                        if let Some(current) = g.saved.current {
                            g.delete_fit(current, cx);
                        }
                    })
                    .disabled(!has_fit),
                )
            }) as Box<dyn Fn(PopupMenu) -> PopupMenu>
        };

        // The character menu reads the window's state when it opens, so it needs the app.
        let character = {
            let this = this.clone();
            Button::new("menu-character")
                .ghost()
                .small()
                .label("Character")
                .dropdown_menu(move |menu, _, cx| {
                    Self::character_menu(menu.min_w(px(220.)), &this, cx)
                })
        };

        let global = {
            let this = this.clone();
            Box::new(move |menu: PopupMenu| {
                menu.item(command(
                    &this,
                    IconName::Settings,
                    "Preferences",
                    Some(Box::new(OpenPreferences)),
                    |g, _, cx| g.open_preferences(cx),
                ))
            }) as Box<dyn Fn(PopupMenu) -> PopupMenu>
        };

        let editors = {
            let this = this.clone();
            let hub = self.hub.clone();
            Box::new(move |menu: PopupMenu| {
                let (patterns, profiles) = (hub.clone(), hub.clone());
                menu.item(command(
                    &this,
                    IconName::CircleUser,
                    "Character Editor",
                    Some(Box::new(CharacterEditor)),
                    |g, _, cx| g.open_character_editor(cx),
                ))
                .item(command(
                    &this,
                    IconName::Settings2,
                    "Implant Set Editor",
                    Some(Box::new(ImplantSetEditor)),
                    |g, window, cx| g.open_implant_set_editor(None, window, cx),
                ))
                .item(
                    PopupMenuItem::new("Damage Pattern Editor")
                        .icon(IconName::Settings2)
                        .on_click(move |_, _, cx| {
                            super::profile_editor::open_damage_pattern_editor(
                                patterns.clone(),
                                None,
                                cx,
                            );
                        }),
                )
                .item(
                    PopupMenuItem::new("Target Profile Editor")
                        .icon(IconName::Settings2)
                        .on_click(move |_, _, cx| {
                            super::profile_editor::open_target_profile_editor(
                                profiles.clone(),
                                None,
                                cx,
                            );
                        }),
                )
            }) as Box<dyn Fn(PopupMenu) -> PopupMenu>
        };

        let help = {
            let this = this.clone();
            Box::new(move |menu: PopupMenu| {
                menu.item(
                    PopupMenuItem::new("Wiki")
                        .icon(IconName::BookOpen)
                        .on_click(|_, _, cx| cx.open_url(WIKI_URL)),
                )
                .item(
                    PopupMenuItem::new("Forums")
                        .icon(IconName::Globe)
                        .on_click(|_, _, cx| cx.open_url(FORUM_URL)),
                )
                .separator()
                .item(command(
                    &this,
                    IconName::Info,
                    "About",
                    None,
                    |_, window, cx| Gyfa::open_about(window, cx),
                ))
            }) as Box<dyn Fn(PopupMenu) -> PopupMenu>
        };

        h_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().title_bar)
            .child(menu("menu-file", "File", file))
            .child(menu("menu-fit", "Fit", fit))
            .child(character)
            .child(menu("menu-global", "Global", global))
            .child(menu("menu-editors", "Editors", editors))
            .child(menu("menu-help", "Help", help))
            .when_some(self.status.clone(), |bar, status| {
                bar.child(
                    div()
                        .ml_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(status),
                )
            })
    }

    fn open_about(window: &mut Window, cx: &mut Context<Self>) {
        let version: SharedString = format!("gyfa {}", env!("CARGO_PKG_VERSION")).into();
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            dialog.title("About gyfa").w(px(420.)).child(
                v_flex()
                    .gap_2()
                    .child(div().text_lg().child(version.clone()))
                    .child(env!("CARGO_PKG_DESCRIPTION"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Calculations by refine. Built to match pyfa feature for feature. Licensed AGPL-3.0-or-later."),
                    ),
            )
        });
    }
}
