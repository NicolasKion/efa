//! What the universal search knows by name: the menu bar's commands and the preferences that
//! switch on and off.

use gpui_kit::Action;
use gpui_kit::assets::IconName as AssetIcon;

use crate::store::Saved;
use crate::ui::{
    AdditionsTab,
    menu::{self, Flags},
    preferences::Page,
};

/// What picking a command does.
#[derive(Debug, Clone, Copy)]
pub enum CommandRun {
    /// One of the menu bar's actions, sent to the fit window.
    Action(fn() -> Box<dyn Action>),
    Url(&'static str),
    AdditionsTab(AdditionsTab),
}

pub struct CommandDef {
    pub label: &'static str,
    /// The menu that has it, shown next to the name.
    pub menu: &'static str,
    pub icon: AssetIcon,
    /// More words to find it by.
    pub keywords: &'static str,
    pub run: CommandRun,
    pub enabled: fn(Flags) -> bool,
}

fn always(_: Flags) -> bool {
    true
}

fn has_fit(f: Flags) -> bool {
    f.has_fit
}

fn has_ship(f: Flags) -> bool {
    f.has_ship
}

fn has_fits(f: Flags) -> bool {
    f.has_fits
}

macro_rules! command {
    ($menu:literal, $label:literal, $icon:ident, $keywords:literal, $action:ident, $enabled:expr) => {
        CommandDef {
            label: $label,
            menu: $menu,
            icon: AssetIcon::$icon,
            keywords: $keywords,
            run: CommandRun::Action(|| Box::new(menu::$action)),
            enabled: $enabled,
        }
    };
}

macro_rules! additions_tab {
    ($label:literal, $tab:ident, $keywords:literal) => {
        CommandDef {
            label: $label,
            menu: "View",
            icon: AssetIcon::PanelBottom,
            keywords: $keywords,
            run: CommandRun::AdditionsTab(AdditionsTab::$tab),
            enabled: has_fit,
        }
    };
}

pub static COMMANDS: &[CommandDef] = &[
    command!("File", "New Tab", Plus, "empty fit create", NewTab, always),
    command!("File", "Close Tab", X, "fit", CloseTab, has_fit),
    command!("File", "Close All Tabs", X, "fits", CloseAllTabs, has_fit),
    command!(
        "File",
        "Backup All Fittings",
        Save,
        "export xml save",
        BackupFits,
        has_fits
    ),
    command!(
        "File",
        "Export All Fittings to HTML",
        Globe,
        "web page",
        ExportHtml,
        has_fits
    ),
    command!("File", "Quit", LogOut, "exit close efa", Quit, always),
    command!("Fit", "Undo", Undo, "revert back", Undo, |f| f.can_undo),
    command!("Fit", "Redo", Redo, "again", Redo, |f| f.can_redo),
    command!(
        "Fit",
        "Fitting Wizard",
        WandSparkles,
        "tank optimise optimize auto build",
        FittingWizard,
        has_ship
    ),
    command!(
        "Fit",
        "Copy Fit to Clipboard",
        ClipboardCopy,
        "export eft share",
        CopyFit,
        has_fit
    ),
    command!(
        "Fit",
        "Paste Fit from Clipboard",
        ClipboardPaste,
        "import eft",
        PasteFit,
        always
    ),
    command!(
        "Fit",
        "Import Fit from Text",
        FileText,
        "paste eft dna",
        ImportText,
        always
    ),
    command!(
        "Fit",
        "Import Fittings from File",
        FolderOpen,
        "open xml eft load",
        ImportFits,
        always
    ),
    command!(
        "Fit",
        "Export Fitting to File",
        FileDown,
        "save xml",
        ExportFit,
        has_fit
    ),
    command!(
        "Fit",
        "Optimize Fit Price",
        PiggyBank,
        "cheaper isk cost optimise",
        OptimizeFitPrice,
        has_fit
    ),
    command!(
        "Fit",
        "Graphs",
        ChartSpline,
        "dps range chart plot",
        Graphs,
        always
    ),
    command!(
        "Fit",
        "Toggle Fitting Restrictions",
        TriangleAlert,
        "ignore rules disable enable",
        ToggleRestrictions,
        has_fit
    ),
    command!(
        "Fit",
        "Browse ESI Fittings",
        Inbox,
        "eve in-game import saved",
        BrowseEsiFittings,
        always
    ),
    command!(
        "Fit",
        "Export to EVE",
        FileUp,
        "esi upload in-game save",
        ExportToEve,
        has_ship
    ),
    command!(
        "Fit",
        "Duplicate Fit",
        Copy,
        "copy clone",
        DuplicateFit,
        has_fit
    ),
    command!("Fit", "Delete Fit", Trash, "remove", DeleteFit, has_fit),
    command!(
        "Fit",
        "Next Tab",
        ArrowRight,
        "fit switch",
        NextTab,
        has_fit
    ),
    command!(
        "Fit",
        "Previous Tab",
        ArrowLeft,
        "fit switch",
        PreviousTab,
        has_fit
    ),
    command!(
        "Global",
        "Preferences",
        Settings,
        "settings options configure",
        OpenPreferences,
        always
    ),
    command!(
        "Global",
        "Attribute Overrides",
        SlidersHorizontal,
        "edit stats values",
        AttributeOverrides,
        always
    ),
    command!(
        "Global",
        "Toggle Attribute Overrides",
        ToggleRight,
        "on off enable disable",
        ToggleOverrides,
        always
    ),
    command!(
        "Editors",
        "Character Editor",
        CircleUser,
        "skills pilot",
        CharacterEditor,
        always
    ),
    command!(
        "Editors",
        "Implant Set Editor",
        Settings2,
        "pod implants",
        ImplantSetEditor,
        always
    ),
    command!(
        "Editors",
        "Cargo Presets",
        Package,
        "hold ammo editor",
        CargoPresets,
        always
    ),
    command!(
        "Editors",
        "Damage Pattern Editor",
        Target,
        "incoming resist profile",
        DamagePatternEditor,
        always
    ),
    command!(
        "Editors",
        "Target Profile Editor",
        Target,
        "signature resist enemy",
        TargetProfileEditor,
        always
    ),
    command!(
        "View",
        "Search the Market",
        Search,
        "focus items find",
        FocusSearch,
        always
    ),
    command!(
        "View",
        "Switch Between Market and Fittings",
        Store,
        "browser ships sidebar",
        ToggleBrowser,
        always
    ),
    additions_tab!("Show Drones Tab", Drones, "tab bay"),
    additions_tab!("Show Fighters Tab", Fighters, "tab squadrons"),
    additions_tab!("Show Implants Tab", Implants, "tab pod"),
    additions_tab!("Show Boosters Tab", Boosters, "tab drugs"),
    additions_tab!("Show Cargo Tab", Cargo, "tab hold"),
    additions_tab!("Show Projected Tab", Projected, "tab effects remote"),
    additions_tab!("Show Command Tab", Command, "tab fleet boosts bursts"),
    additions_tab!("Show Notes Tab", Notes, "tab text"),
    CommandDef {
        label: "Wiki",
        menu: "Help",
        icon: AssetIcon::BookOpen,
        keywords: "docs manual pyfa",
        run: CommandRun::Url(menu::WIKI_URL),
        enabled: always,
    },
    CommandDef {
        label: "Forums",
        menu: "Help",
        icon: AssetIcon::Globe,
        keywords: "eve online thread",
        run: CommandRun::Url(menu::FORUM_URL),
        enabled: always,
    },
    command!(
        "Help",
        "Check for Updates",
        RefreshCw,
        "version release upgrade",
        CheckUpdates,
        always
    ),
    command!("Help", "About", Info, "version license", About, always),
];

/// Commands offered before anything is typed, by label: the ones for a fit with a ship, and the
/// ones to get to a fit.
pub const WITH_SHIP: [&str; 5] = [
    "Fitting Wizard",
    "Graphs",
    "Copy Fit to Clipboard",
    "Optimize Fit Price",
    "Preferences",
];
pub const WITHOUT_SHIP: [&str; 5] = [
    "Paste Fit from Clipboard",
    "Import Fittings from File",
    "Browse ESI Fittings",
    "Character Editor",
    "Preferences",
];

/// A preference that is on or off.
pub struct SettingDef {
    pub label: &'static str,
    /// What it does, which it is also found by.
    pub hint: &'static str,
    /// The preferences page it is on.
    pub page: Page,
    pub get: fn(&Saved) -> bool,
    pub set: fn(&mut Saved, bool),
}

macro_rules! setting {
    ($page:ident, $label:literal, $hint:literal, $($field:ident).+) => {
        SettingDef {
            label: $label,
            hint: $hint,
            page: Page::$page,
            get: |s| s.settings.$($field).+,
            set: |s, on| s.settings.$($field).+ = on,
        }
    };
}

pub static SETTINGS: &[SettingDef] = &[
    setting!(
        General,
        "Color by slot",
        "tint high mid low rig rows racks",
        preferences.general.color_by_slot
    ),
    setting!(
        General,
        "Separate racks",
        "gap between racks fitting view",
        preferences.general.rack_slots
    ),
    setting!(
        General,
        "Show rack labels",
        "header band slot count",
        preferences.general.rack_labels
    ),
    setting!(
        General,
        "Reopen fits on startup",
        "restore tabs session",
        preferences.general.reopen_fits
    ),
    setting!(
        General,
        "Open fits in a new tab",
        "fittings browser replace current",
        preferences.general.open_fit_in_new_tab
    ),
    setting!(
        General,
        "Tab tooltips",
        "hover fit tab ship name",
        preferences.general.tab_tooltips
    ),
    setting!(
        General,
        "Character implants for new fits",
        "imported fits pod",
        preferences.general.character_implants
    ),
    setting!(
        General,
        "Change charges in every module of a type",
        "ammo all same modules load",
        preferences.general.ammo_change_all
    ),
    setting!(
        General,
        "Use global character",
        "every fit same pilot",
        preferences.general.global_character
    ),
    setting!(
        General,
        "Use global damage pattern",
        "every fit same incoming damage",
        preferences.general.global_damage_pattern
    ),
    setting!(
        General,
        "Compact skills needed tooltip",
        "missing skills list once",
        preferences.general.compact_skills
    ),
    setting!(
        General,
        "Implants and boosters take cargo space",
        "hold volume count",
        preferences.general.implants_in_cargo
    ),
    setting!(
        General,
        "Descriptive names for mutated items",
        "abyssal mutaplasmid expanded",
        preferences.general.expanded_mutant_names
    ),
    setting!(
        General,
        "Ship tooltips",
        "fittings browser hover traits bonuses",
        preferences.general.ship_browser_tooltips
    ),
    setting!(
        General,
        "Omega markers",
        "alpha clone icons",
        preferences.general.omega_icons
    ),
    setting!(
        General,
        "Window buttons in the header",
        "minimize maximize close title bar decorations",
        preferences.general.window_controls
    ),
    setting!(
        Engine,
        "Factor in reload time",
        "dps capacitor ammo reloading",
        factor_reload
    ),
    setting!(
        Engine,
        "Strict skill requirements",
        "lowering unlearns skills levels",
        preferences.engine.strict_skill_levels
    ),
    setting!(
        Stats,
        "Show Resources",
        "stats pane section cpu powergrid",
        preferences.stats.resources
    ),
    setting!(
        Stats,
        "Show Resistances",
        "stats pane section ehp",
        preferences.stats.resistances
    ),
    setting!(
        Stats,
        "Show Shield/Armor Tank",
        "stats pane section recharge repair",
        preferences.stats.recharge
    ),
    setting!(
        Stats,
        "Show Firepower",
        "stats pane section dps volley",
        preferences.stats.firepower
    ),
    setting!(
        Stats,
        "Show Remote Reps",
        "stats pane section outgoing logistics",
        preferences.stats.outgoing
    ),
    setting!(
        Stats,
        "Show Capacitor",
        "stats pane section cap stable",
        preferences.stats.capacitor
    ),
    setting!(
        Stats,
        "Show Targeting & Misc",
        "stats pane section scan speed align",
        preferences.stats.targeting
    ),
    setting!(
        Stats,
        "Show Bombing",
        "stats pane section bombs",
        preferences.stats.bombing
    ),
    setting!(
        Stats,
        "Show Price",
        "stats pane section isk cost",
        preferences.stats.price
    ),
    setting!(
        ContextMenus,
        "Menu: Set as Damage Pattern",
        "context menu charges ammo",
        preferences.context_menus.ammo_pattern
    ),
    setting!(
        ContextMenus,
        "Menu: Change Skills",
        "context menu items levels",
        preferences.context_menus.change_skills
    ),
    setting!(
        ContextMenus,
        "Menu: Variations",
        "context menu meta swap",
        preferences.context_menus.variations
    ),
    setting!(
        ContextMenus,
        "Menu: Project onto Fit",
        "context menu projected",
        preferences.context_menus.project
    ),
    setting!(
        ContextMenus,
        "Menu: Fill With Module",
        "context menu rack",
        preferences.context_menus.module_fill
    ),
    setting!(
        ContextMenus,
        "Menu: Spoolup",
        "context menu triglavian spool",
        preferences.context_menus.spoolup
    ),
    setting!(
        ContextMenus,
        "Menu: Copy and paste in additions",
        "context menu drones cargo implants",
        preferences.context_menus.additions_copy_paste
    ),
    setting!(
        ContextMenus,
        "Menu: Fill Cargo With",
        "context menu hold",
        preferences.context_menus.cargo_fill
    ),
    setting!(
        Network,
        "Enable network",
        "internet offline requests",
        preferences.network.enabled
    ),
    setting!(
        Network,
        "Price updates",
        "fetch market prices isk",
        preferences.network.prices
    ),
    setting!(
        Network,
        "Check for updates automatically",
        "new release version startup",
        preferences.network.check_updates
    ),
    setting!(
        Html,
        "Minimal HTML export",
        "plain links without styling",
        preferences.html.minimal
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_commands_exist() {
        for label in WITH_SHIP.iter().chain(&WITHOUT_SHIP) {
            assert!(
                COMMANDS.iter().any(|c| c.label == *label),
                "no command called {label}"
            );
        }
    }

    #[test]
    fn settings_read_what_they_write() {
        let mut saved = Saved::default();
        for setting in SETTINGS {
            let before = (setting.get)(&saved);
            (setting.set)(&mut saved, !before);
            assert_eq!((setting.get)(&saved), !before, "{}", setting.label);
            (setting.set)(&mut saved, before);
        }
        assert_eq!(
            saved.settings.preferences,
            Saved::default().settings.preferences
        );
    }

    #[test]
    fn names_are_unique() {
        let mut labels: Vec<&str> = COMMANDS.iter().map(|c| c.label).collect();
        labels.extend(SETTINGS.iter().map(|s| s.label));
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count);
    }
}
