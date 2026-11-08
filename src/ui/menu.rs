//! The menu bar and its keyboard shortcuts, laid out like pyfa's (gui/mainMenuBar.py): File,
//! Fit, Character, Global, Editors, Help.
//!
//! Every command is an action, handled once in [`Efa::bind_actions`]: a shortcut and the menu
//! entry both dispatch it. The Character menu reads the window when it opens and is built in
//! `characters.rs`.

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    App, Context, Div, InteractiveElement, IntoElement, KeyBinding, ParentElement, SharedString,
    Styled, Window, div, prelude::FluentBuilder, px,
};

use super::{AdditionsTab, BrowserTab, Efa, actions::Action, widgets};
use crate::prices::optimize;

/// Key context of a fit window, so text fields keep their own Ctrl+C, Ctrl+V and Ctrl+Z.
const CONTEXT: &str = "Efa";

const WIKI_URL: &str = "https://github.com/pyfa-org/Pyfa/wiki";
const FORUM_URL: &str = "https://forums.eveonline.com/t/27156";

gpui_kit::actions!(
    efa,
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
        BrowseEsiFittings,
        ExportToEve,
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
        BackupFits,
        ExportHtml,
        ImportText,
        ToggleRestrictions,
        DuplicateFit,
        DeleteFit,
        CargoPresets,
        DamagePatternEditor,
        TargetProfileEditor,
        AttributeOverrides,
        ToggleOverrides,
        About,
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
    super::implant_sets::bind_keys(cx);
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
        KeyBinding::new("ctrl-b", BrowseEsiFittings, Some(CONTEXT)),
        KeyBinding::new("ctrl-e", ExportToEve, Some(CONTEXT)),
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

/// A menu bar entry that dispatches `action`, showing its shortcut if it has one.
fn command(
    icon: impl Into<gpui_kit::component::Icon>,
    label: impl Into<SharedString>,
    action: impl gpui_kit::Action,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(icon)
        .action(Box::new(action))
}

impl Efa {
    /// The root element with the menu's shortcuts wired to this window.
    pub(super) fn bind_actions(&self, root: Div, cx: &mut Context<Self>) -> Div {
        let root = root
            .key_context(CONTEXT)
            .track_focus(&self.root_focus)
            .on_action(cx.listener(|g, _: &NewTab, _, cx| g.new_tab(cx)))
            .on_action(cx.listener(|g, _: &CloseTab, _, cx| g.close_current_tab(cx)))
            .on_action(cx.listener(|g, _: &CloseAllTabs, _, cx| g.close_all_tabs(cx)))
            .on_action(cx.listener(|g, _: &Undo, _, cx| g.undo(cx)))
            .on_action(cx.listener(|g, _: &Redo, _, cx| g.redo(cx)))
            .on_action(cx.listener(|g, _: &CopyFit, _, cx| g.open_export_window(cx)))
            .on_action(cx.listener(|g, _: &PasteFit, _, cx| g.paste_from_clipboard(cx)))
            .on_action(cx.listener(|_, _: &ImportFits, _, cx| Efa::import_files_dialog(cx)))
            .on_action(cx.listener(|_, _: &ImportText, window, cx| {
                Efa::open_import_dialog(window, cx);
            }))
            .on_action(cx.listener(|g, _: &ExportFit, _, cx| g.save_fit_dialog(cx)))
            .on_action(cx.listener(|g, _: &BackupFits, _, cx| g.backup_dialog(cx)))
            .on_action(cx.listener(|g, _: &ExportHtml, _, cx| g.export_html(cx)))
            .on_action(cx.listener(|g, _: &OpenPreferences, _, cx| g.open_preferences(cx)))
            .on_action(cx.listener(|g, _: &CharacterEditor, _, cx| {
                g.open_character_editor(cx);
            }))
            .on_action(cx.listener(|g, _: &ImplantSetEditor, window, cx| {
                g.open_implant_set_editor(None, window, cx);
            }))
            .on_action(cx.listener(|g, _: &CargoPresets, _, cx| {
                g.open_cargo_preset_editor(None, cx);
            }))
            .on_action(cx.listener(|g, _: &DamagePatternEditor, _, cx| {
                super::profile_editor::open_damage_pattern_editor(g.hub.clone(), None, cx);
            }))
            .on_action(cx.listener(|g, _: &TargetProfileEditor, _, cx| {
                super::profile_editor::open_target_profile_editor(g.hub.clone(), None, cx);
            }))
            .on_action(cx.listener(|g, _: &AttributeOverrides, _, cx| g.open_overrides(cx)))
            .on_action(cx.listener(|g, _: &ToggleOverrides, _, cx| g.toggle_overrides(cx)))
            .on_action(cx.listener(|g, _: &Graphs, _, cx| g.open_graphs(cx)))
            .on_action(cx.listener(|g, _: &HiddenGraphs, _, cx| g.open_hidden_graphs(cx)))
            .on_action(cx.listener(|g, _: &FittingWizard, window, cx| {
                g.open_wizard(window, cx);
            }))
            .on_action(cx.listener(|g, _: &BrowseEsiFittings, _, cx| g.open_esi_fittings(cx)))
            .on_action(cx.listener(|g, _: &ExportToEve, _, cx| g.open_export_to_eve(cx)))
            .on_action(cx.listener(|g, _: &OptimizeFitPrice, _, cx| g.optimize_fit_price(cx)))
            .on_action(cx.listener(|g, _: &ToggleRestrictions, window, cx| {
                g.confirm_restrictions_toggle(window, cx);
            }))
            .on_action(cx.listener(|g, _: &DuplicateFit, _, cx| g.duplicate_fit(cx)))
            .on_action(cx.listener(|g, _: &DeleteFit, window, cx| {
                if let Some(current) = g.saved.settings.current {
                    let shift = window.modifiers().shift;
                    g.confirm_delete_fit(current, shift, window, cx);
                }
            }))
            .on_action(cx.listener(|_, _: &About, window, cx| Efa::open_about(window, cx)))
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
        let Some(next) = cycle_tab(
            &self.saved.settings.open,
            self.saved.settings.current,
            forward,
        ) else {
            return;
        };
        if self.saved.settings.current != Some(next) {
            self.renaming = false;
            self.saved.settings.current = Some(next);
            self.clear_selection();
            self.changed(cx);
        }
    }

    /// Ctrl+F, mainFrame.py toggleSearchBox: the search box of whichever browser shows.
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.side_stats {
            self.side_stats = false;
            cx.notify();
        }
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
        self.side_stats = false;
        cx.notify();
    }

    /// Ctrl+1..9, `mainFrame.py` `AdditionsTabSelect`: the additions tab at that place.
    fn pick_additions_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(&tab) = AdditionsTab::ORDER.get(index) else {
            return;
        };
        self.show_additions_tab(tab);
        cx.notify();
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
        let Some(loaded) = self.loaded() else { return };
        let fit = self.fresh_fit("New fit", None, &loaded.catalog);
        self.add_fit(fit, cx);
    }

    /// pyfa's Optimize Fit Price: price the fit's items and their replacements, then swap in the
    /// cheaper ones as one undo step. The status line says what it saved.
    fn optimize_fit_price(&mut self, cx: &mut Context<Self>) {
        let (Some(loaded), Some(current), Some(fit)) =
            (self.loaded(), self.saved.settings.current, self.fit())
        else {
            return;
        };
        let items = optimize::fit_items(fit);
        let types = optimize::types_to_price(&loaded.catalog, items);
        self.with_prices_for(types, cx, move |g, cx| {
            // The fetch may have taken long enough to switch fits.
            if g.saved.settings.current == Some(current) {
                g.apply(Action::OptimizePrice, cx);
            }
        });
    }

    fn close_current_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(current) = self.saved.settings.current {
            self.close_fit(current, cx);
        }
    }

    fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.settings.open.clear();
        self.saved.settings.current = None;
        self.changed(cx);
    }

    pub(super) fn menu_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.weak_entity();
        let has_fit = self.saved.settings.current.is_some();
        let has_ship = self.fit().is_some_and(|f| f.ship.is_some());
        let has_fits = !self.saved.fits.is_empty();
        let unrestricted = self.fit().is_some_and(|f| f.ignore_restrictions);
        let (can_undo, can_redo) = (self.can_undo(), self.can_redo());
        let overrides_on = self.saved.settings.overrides.enabled;
        let root = self.root_focus.clone();
        let menu =
            |id: &'static str, label: &'static str, build: fn(PopupMenu, Flags) -> PopupMenu| {
                let flags = Flags {
                    has_fit,
                    has_ship,
                    has_fits,
                    unrestricted,
                    can_undo,
                    can_redo,
                    overrides_on,
                };
                // The entries' actions go to the window's root, where they are handled.
                let root = root.clone();
                Button::new(id).ghost().small().label(label).dropdown_menu(
                    move |menu, window, _| {
                        build(fit_window(menu, window).action_context(root.clone()), flags)
                    },
                )
            };

        // The character menu reads the window's state when it opens, so it needs the app.
        let character = Button::new("menu-character")
            .ghost()
            .small()
            .label("Character")
            .dropdown_menu(move |menu, window, cx| {
                Self::character_menu(fit_window(menu, window), &this, cx)
            });

        let builtin = super::chrome::builtin(cx);
        h_flex()
            .gap_1()
            .pl_2()
            .when(builtin, |bar| bar.h(px(super::chrome::HEIGHT)))
            .when(!builtin, |bar| bar.px_2().py_1())
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().title_bar)
            .child(menu("menu-file", "File", file_menu))
            .child(menu("menu-fit", "Fit", fit_menu))
            .child(character)
            .child(menu("menu-global", "Global", global_menu))
            .child(menu("menu-editors", "Editors", editors_menu))
            .child(menu("menu-help", "Help", help_menu))
            // With the window buttons in the bar, the room left over moves the window.
            .when(builtin, |bar| {
                bar.child(
                    super::chrome::drag_area(div().id("menu-bar-drag"))
                        .flex_1()
                        .min_w(px(24.))
                        .h_full(),
                )
                .child(super::chrome::controls(window, cx))
            })
    }

    fn open_about(window: &mut Window, cx: &mut Context<Self>) {
        let version: SharedString = format!("efa {}", env!("CARGO_PKG_VERSION")).into();
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            dialog.title("About efa").w(px(420.)).child(
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

/// What the menu bar's entries depend on, read when the bar is drawn.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about the window, one per entry they enable"
)]
struct Flags {
    has_fit: bool,
    has_ship: bool,
    has_fits: bool,
    unrestricted: bool,
    can_undo: bool,
    can_redo: bool,
    overrides_on: bool,
}

fn file_menu(menu: PopupMenu, f: Flags) -> PopupMenu {
    menu.item(command(IconName::Plus, "New Tab", NewTab))
        .item(command(IconName::Close, "Close Tab", CloseTab).disabled(!f.has_fit))
        .item(command(IconName::Close, "Close All Tabs", CloseAllTabs).disabled(!f.has_fit))
        .separator()
        .item(command(IconName::Folder, "Backup All Fittings", BackupFits).disabled(!f.has_fits))
        .item(
            command(IconName::Globe, "Export All Fittings to HTML", ExportHtml)
                .disabled(!f.has_fits),
        )
        .separator()
        .item(command(IconName::WindowClose, "Quit", Quit))
}

fn fit_menu(menu: PopupMenu, f: Flags) -> PopupMenu {
    let restrictions = if f.unrestricted {
        "Enable Fitting Restrictions"
    } else {
        "Disable Fitting Restrictions"
    };
    menu.item(command(IconName::Undo, "Undo", Undo).disabled(!f.can_undo))
        .item(command(IconName::Redo, "Redo", Redo).disabled(!f.can_redo))
        .separator()
        .item(
            command(AssetIcon::WandSparkles, "Fitting Wizard…", FittingWizard)
                .disabled(!f.has_ship),
        )
        .separator()
        .item(command(IconName::Copy, "To Clipboard", CopyFit).disabled(!f.has_fit))
        .item(command(IconName::FileText, "From Clipboard", PasteFit))
        .item(command(IconName::FileText, "From Text…", ImportText))
        .separator()
        .item(command(IconName::FolderOpen, "Import Fittings", ImportFits))
        .item(command(IconName::File, "Export Fitting", ExportFit).disabled(!f.has_fit))
        .separator()
        .item(
            command(AssetIcon::PiggyBank, "Optimize Fit Price", OptimizeFitPrice)
                .disabled(!f.has_fit),
        )
        .item(command(IconName::ChartPie, "Graphs", Graphs))
        .separator()
        .item(
            command(IconName::TriangleAlert, restrictions, ToggleRestrictions).disabled(!f.has_fit),
        )
        .separator()
        .item(command(
            IconName::Inbox,
            "Browse ESI Fittings…",
            BrowseEsiFittings,
        ))
        .item(command(IconName::ArrowUp, "Export to EVE…", ExportToEve).disabled(!f.has_ship))
        .separator()
        .item(command(IconName::Copy, "Duplicate Fit", DuplicateFit).disabled(!f.has_fit))
        .item(command(IconName::Delete, "Delete Fit", DeleteFit).disabled(!f.has_fit))
}

fn global_menu(menu: PopupMenu, f: Flags) -> PopupMenu {
    let (toggle_icon, toggle_label) = if f.overrides_on {
        (AssetIcon::ToggleRight, "Turn Overrides Off")
    } else {
        (AssetIcon::ToggleLeft, "Turn Overrides On")
    };
    menu.item(command(IconName::Settings, "Preferences", OpenPreferences))
        .separator()
        .item(command(
            UiIcon::new(AssetIcon::SlidersHorizontal),
            "Attribute Overrides",
            AttributeOverrides,
        ))
        .item(command(
            UiIcon::new(toggle_icon),
            toggle_label,
            ToggleOverrides,
        ))
}

fn editors_menu(menu: PopupMenu, _: Flags) -> PopupMenu {
    menu.item(command(
        IconName::CircleUser,
        "Character Editor",
        CharacterEditor,
    ))
    .item(command(
        IconName::Settings2,
        "Implant Set Editor",
        ImplantSetEditor,
    ))
    .item(command(IconName::Inbox, "Cargo Presets", CargoPresets))
    .item(command(
        IconName::Settings2,
        "Damage Pattern Editor",
        DamagePatternEditor,
    ))
    .item(command(
        IconName::Settings2,
        "Target Profile Editor",
        TargetProfileEditor,
    ))
}

fn help_menu(menu: PopupMenu, _: Flags) -> PopupMenu {
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
    .item(command(IconName::Info, "About", About))
}

/// A menu bar menu, which scrolls when the window is too short to show it whole.
fn fit_window(menu: PopupMenu, window: &Window) -> PopupMenu {
    menu.min_w(px(220.))
        .scrollable(true)
        .max_h(widgets::menu_max_h(window, f32::MAX))
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
