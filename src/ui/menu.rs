//! The menu bar and its keyboard shortcuts, laid out like pyfa's (gui/mainMenuBar.py): File,
//! Fit, Character, Global, Editors, Help.
//!
//! Left out: ESI fittings and character management (they need EVE SSO). Entries that belong to
//! windows built elsewhere are marked "Hook:" below and stay disabled until those windows exist.

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, Div, InteractiveElement, IntoElement, KeyBinding, ParentElement,
    SharedString, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{AdditionsTab, BrowserTab, Gyfa, actions::Action};
use crate::prices::optimize;

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
        FittingWizard,
        OptimizeFitPrice,
        Quit,
        NextTab,
        PreviousTab,
        FocusSearch,
        ToggleBrowser,
        AdditionsTab1,
        AdditionsTab2,
        AdditionsTab3,
        AdditionsTab4,
        AdditionsTab5,
        AdditionsTab6,
        AdditionsTab7,
        AdditionsTab8,
        AdditionsTab9,
        MarketItem1,
        MarketItem2,
        MarketItem3,
        MarketItem4,
        MarketItem5,
        MarketItem6,
        MarketItem7,
        MarketItem8,
        MarketItem9,
    ]
);

/// Calls `$m!` with `$args` and then each numbered action pair, its zero-based index and its
/// key, for Ctrl+1..9 and Alt+1..9.
macro_rules! numbered {
    ($m:ident!($($args:tt)*)) => {
        $m!(
            $($args)*;
            (AdditionsTab1, MarketItem1, 0, "1"),
            (AdditionsTab2, MarketItem2, 1, "2"),
            (AdditionsTab3, MarketItem3, 2, "3"),
            (AdditionsTab4, MarketItem4, 3, "4"),
            (AdditionsTab5, MarketItem5, 4, "5"),
            (AdditionsTab6, MarketItem6, 5, "6"),
            (AdditionsTab7, MarketItem7, 6, "7"),
            (AdditionsTab8, MarketItem8, 7, "8"),
            (AdditionsTab9, MarketItem9, 8, "9")
        )
    };
}

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
        KeyBinding::new("ctrl-shift-w", FittingWizard, Some(CONTEXT)),
        KeyBinding::new("ctrl-d", OptimizeFitPrice, Some(CONTEXT)),
        // mainFrame.py's accelerator table.
        KeyBinding::new("ctrl-f4", CloseTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-alt-f4", CloseAllTabs, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-z", Redo, Some(CONTEXT)),
        KeyBinding::new("ctrl-tab", NextTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-pagedown", NextTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-pageup", PreviousTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("ctrl-space", ToggleBrowser, Some(CONTEXT)),
    ]);
    macro_rules! bind_numbered {
        ($cx:ident; $(($tab:ident, $item:ident, $index:expr, $key:literal)),*) => {
            $cx.bind_keys([
                $(
                    KeyBinding::new(concat!("ctrl-", $key), $tab, Some(CONTEXT)),
                    KeyBinding::new(concat!("alt-", $key), $item, Some(CONTEXT)),
                )*
            ]);
        };
    }
    numbered!(bind_numbered!(cx));
}

/// The open tab `step` places from `current`, wrapping around at either end, as pyfa's
/// `chrome_tabs.py` `NextPage` and `PrevPage` do. None without an open tab to land on.
pub fn cycle_tab(open: &[usize], current: Option<usize>, forward: bool) -> Option<usize> {
    let at = open.iter().position(|&i| Some(i) == current)?;
    let next = if forward {
        (at + 1) % open.len()
    } else {
        (at + open.len() - 1) % open.len()
    };
    open.get(next).copied()
}

/// A menu entry that runs `f` on the window and shows `action`'s shortcut.
fn command(
    this: &WeakEntity<Gyfa>,
    icon: impl Into<gpui_kit::component::Icon>,
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
        let root = root
            .key_context(CONTEXT)
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
            .on_action(cx.listener(|g, _: &FittingWizard, window, cx| {
                g.open_wizard(window, cx);
            }))
            .on_action(cx.listener(|g, _: &OptimizeFitPrice, _, cx| g.optimize_fit_price(cx)))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|g, _: &NextTab, _, cx| g.cycle_tab(true, cx)))
            .on_action(cx.listener(|g, _: &PreviousTab, _, cx| g.cycle_tab(false, cx)))
            .on_action(cx.listener(|g, _: &FocusSearch, window, cx| g.focus_search(window, cx)))
            .on_action(cx.listener(|g, _: &ToggleBrowser, _, cx| g.toggle_browser(cx)));
        macro_rules! on_numbered {
            ($root:ident, $cx:ident; $(($tab:ident, $item:ident, $index:expr, $key:literal)),*) => {
                $root$(
                    .on_action($cx.listener(|g, _: &$tab, _, cx| {
                        g.pick_additions_tab($index, cx);
                    }))
                    .on_action($cx.listener(|g, _: &$item, _, cx| {
                        g.add_listed_item($index, cx);
                    }))
                )*
            };
        }
        numbered!(on_numbered!(root, cx))
    }

    /// Ctrl+Tab and friends: the next or previous open fit.
    fn cycle_tab(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(next) = cycle_tab(&self.saved.open, self.saved.current, forward) else {
            return;
        };
        if self.saved.current != Some(next) {
            self.renaming = false;
            self.saved.current = Some(next);
            self.clear_selection();
            self.changed(cx);
        }
    }

    /// Ctrl+F, mainFrame.py toggleSearchBox: the search box of whichever browser shows.
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match self.browser_tab {
            BrowserTab::Market => &self.search,
            BrowserTab::Fittings => &self.fit_search,
        };
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Ctrl+Space, mainFrame.py toggleShipMarket: swap the market and the fittings browser.
    fn toggle_browser(&mut self, cx: &mut Context<Self>) {
        self.browser_tab = match self.browser_tab {
            BrowserTab::Market => BrowserTab::Fittings,
            BrowserTab::Fittings => BrowserTab::Market,
        };
        cx.notify();
    }

    /// Ctrl+1..9, `mainFrame.py` `AdditionsTabSelect`: the additions tab at that place.
    fn pick_additions_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(&tab) = AdditionsTab::ORDER.get(index) else {
            return;
        };
        if self.additions_tab != tab {
            self.additions_tab = tab;
            self.additions_selection.clear();
            self.additions_anchor = None;
            cx.notify();
        }
    }

    /// Alt+1..9, `mainFrame.py` `ItemSelect`: add the market's Nth listed item, as a double-click
    /// without modifiers would.
    fn add_listed_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(&id) = self.market_order.get(index) else {
            return;
        };
        self.used_from_market(id);
        self.market_double_click(id, gpui_kit::Modifiers::default(), cx);
    }

    pub(super) fn new_tab(&mut self, cx: &mut Context<Self>) {
        let mut fit = self.new_fit("New fit");
        self.new_fit_defaults(&mut fit);
        self.add_fit(fit, cx);
    }

    /// pyfa's Optimize Fit Price: price the fit's items and their replacements, then swap in the
    /// cheaper ones as one undo step. The status line says what it saved.
    fn optimize_fit_price(&mut self, cx: &mut Context<Self>) {
        let (Some(loaded), Some(current), Some(fit)) =
            (self.loaded(), self.saved.current, self.fit())
        else {
            return;
        };
        let items = optimize::fit_items(fit);
        let types = optimize::types_to_price(&loaded.catalog, items);
        self.with_prices_for(types, cx, move |g, cx| {
            // The fetch may have taken long enough to switch fits.
            if g.saved.current == Some(current) {
                g.apply(Action::OptimizePrice, cx);
            }
        });
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
        let has_ship = self.fit().is_some_and(|f| f.ship.is_some());
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
                        gpui_kit::assets::IconName::WandSparkles,
                        "Fitting Wizard…",
                        Some(Box::new(FittingWizard)),
                        Gyfa::open_wizard,
                    )
                    .disabled(!has_ship),
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
                .item(
                    command(
                        &this,
                        AssetIcon::PiggyBank,
                        "Optimize Fit Price",
                        Some(Box::new(OptimizeFitPrice)),
                        |g, _, cx| g.optimize_fit_price(cx),
                    )
                    .disabled(!has_fit),
                )
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
                    command(
                        &this,
                        IconName::Delete,
                        "Delete Fit",
                        None,
                        |g, window, cx| {
                            if let Some(current) = g.saved.current {
                                let shift = window.modifiers().shift;
                                g.confirm_delete_fit(current, shift, window, cx);
                            }
                        },
                    )
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

        let overrides_on = self.saved.overrides.enabled;
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
                .separator()
                .item(command(
                    &this,
                    UiIcon::new(AssetIcon::SlidersHorizontal),
                    "Attribute Overrides",
                    None,
                    |g, _, cx| g.open_overrides(cx),
                ))
                .item(command(
                    &this,
                    UiIcon::new(if overrides_on {
                        AssetIcon::ToggleRight
                    } else {
                        AssetIcon::ToggleLeft
                    }),
                    if overrides_on {
                        "Turn Overrides Off"
                    } else {
                        "Turn Overrides On"
                    },
                    None,
                    |g, _, cx| g.toggle_overrides(cx),
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

#[cfg(test)]
mod tests {
    use super::cycle_tab;

    #[test]
    fn tabs_cycle_and_wrap() {
        let open = [4, 1, 7];
        assert_eq!(cycle_tab(&open, Some(4), true), Some(1));
        assert_eq!(cycle_tab(&open, Some(7), true), Some(4));
        assert_eq!(cycle_tab(&open, Some(4), false), Some(7));
        assert_eq!(cycle_tab(&open, Some(1), false), Some(4));
        assert_eq!(cycle_tab(&[3], Some(3), true), Some(3));
        assert_eq!(cycle_tab(&open, None, true), None);
        assert_eq!(cycle_tab(&[], Some(2), false), None);
        // A current fit that is not open (just deleted, say) goes nowhere.
        assert_eq!(cycle_tab(&open, Some(9), true), None);
    }
}
