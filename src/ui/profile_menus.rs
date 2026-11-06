//! The damage pattern and target profile menus, built from builtin and user entries the way
//! pyfa nests them (gui/builtinContextMenus/shared/patterns.py, targetProfile/switcher.py and
//! adder.py). The graphs window can reuse [`target_profile_menu`] for "Add Target Profile".

use std::rc::Rc;

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{App, Context, Window};

use crate::{
    damage_pattern::{self, DamagePattern, SELECTED_AMMO},
    fit::DamageProfile,
    profile::Tree,
    target_profile::{self, TargetProfile},
};

/// What happens when a pattern is picked.
pub type PickPattern = Rc<dyn Fn(DamageProfile, &mut Window, &mut App)>;
/// What happens when a target profile is picked; none is "No Profile".
pub type PickProfile = Rc<dyn Fn(Option<TargetProfile>, &mut Window, &mut App)>;

/// pyfa's damage pattern menu: Uniform and Selected Ammo first, the other uncategorized patterns
/// after them, then the categories with their sublevels before their patterns.
pub fn damage_pattern_menu(
    menu: PopupMenu,
    user: &[DamagePattern],
    current: &DamageProfile,
    pick: &PickPattern,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let tree = damage_pattern::tree(user);
    let item = |label: &str, profile: DamageProfile, checked: bool| {
        let pick = Rc::clone(pick);
        PopupMenuItem::new(label.to_owned())
            .checked(checked)
            .on_click(move |_, window, cx| pick(profile.clone(), window, cx))
    };
    let selected_ammo = matches!(current, DamageProfile::SelectedAmmo)
        || matches!(current, DamageProfile::Pattern { name, .. } if name == SELECTED_AMMO);
    let mut menu = menu;
    for (i, (short, pattern)) in tree.entries.iter().enumerate() {
        let profile = if *pattern == damage_pattern::BUILTINS[0] {
            DamageProfile::Uniform
        } else {
            pattern.to_profile()
        };
        let checked = *current == profile
            || matches!((current, &profile), (DamageProfile::Pattern { name: a, .. }, DamageProfile::Pattern { name: b, .. }) if a == b);
        menu = menu.item(item(short, profile, checked));
        // gyfa's Selected Ammo, the fit's own loaded charges, sits right after Uniform.
        if i == 0 {
            menu = menu.item(item(
                SELECTED_AMMO,
                DamageProfile::SelectedAmmo,
                selected_ammo,
            ));
        }
    }
    let current = pattern_name(current);
    tree.children.iter().fold(menu, |menu, (name, level)| {
        pattern_submenu(
            menu,
            name,
            Rc::new(level.clone()),
            current.as_deref(),
            pick,
            window,
            cx,
        )
    })
}

fn pattern_name(profile: &DamageProfile) -> Option<String> {
    match profile {
        DamageProfile::Pattern { name, .. } => Some(name.clone()),
        DamageProfile::Uniform | DamageProfile::SelectedAmmo => None,
    }
}

fn pattern_submenu(
    menu: PopupMenu,
    name: &str,
    level: Rc<Tree<DamagePattern>>,
    current: Option<&str>,
    pick: &PickPattern,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (current, pick) = (current.map(str::to_owned), Rc::clone(pick));
    menu.submenu(name.to_owned(), window, cx, move |menu, window, cx| {
        let menu = level.children.iter().fold(menu, |menu, (name, child)| {
            pattern_submenu(
                menu,
                name,
                Rc::new(child.clone()),
                current.as_deref(),
                &pick,
                window,
                cx,
            )
        });
        level.entries.iter().fold(menu, |menu, (short, pattern)| {
            let checked = current.as_deref() == Some(pattern.name.as_str());
            let (pick, profile) = (Rc::clone(&pick), pattern.to_profile());
            menu.item(
                PopupMenuItem::new(short.clone())
                    .checked(checked)
                    .on_click(move |_, window, cx| pick(profile.clone(), window, cx)),
            )
        })
    })
}

/// The entry a target profile menu starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstProfile {
    /// The firepower view's "No Profile", checked when none is set, then a separator.
    NoProfile,
    /// The graphs' "Ideal Target".
    Ideal,
}

/// pyfa's target profile menu: the first entry, then uncategorized profiles, then categories,
/// everything in natural order. `current` is checked; the graphs' adder passes none.
pub fn target_profile_menu(
    menu: PopupMenu,
    user: &[TargetProfile],
    current: Option<&str>,
    first: FirstProfile,
    pick: &PickProfile,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let tree = target_profile::tree(user);
    let mut menu = match first {
        FirstProfile::NoProfile => {
            let pick = Rc::clone(pick);
            let menu = menu.item(
                PopupMenuItem::new("No Profile")
                    .checked(current.is_none())
                    .on_click(move |_, window, cx| pick(None, window, cx)),
            );
            if tree.is_empty() {
                menu
            } else {
                menu.separator()
            }
        }
        FirstProfile::Ideal => {
            let ideal = TargetProfile::ideal();
            let pick = Rc::clone(pick);
            menu.item(
                PopupMenuItem::new(ideal.name.clone())
                    .on_click(move |_, window, cx| pick(Some(ideal.clone()), window, cx)),
            )
        }
    };
    menu = profile_entries(menu, &tree, current, pick);
    tree.children.iter().fold(menu, |menu, (name, level)| {
        profile_submenu(
            menu,
            name,
            Rc::new(level.clone()),
            current,
            pick,
            window,
            cx,
        )
    })
}

fn profile_entries(
    menu: PopupMenu,
    level: &Tree<TargetProfile>,
    current: Option<&str>,
    pick: &PickProfile,
) -> PopupMenu {
    level.entries.iter().fold(menu, |menu, (short, profile)| {
        let (pick, profile) = (Rc::clone(pick), profile.clone());
        menu.item(
            PopupMenuItem::new(short.clone())
                .checked(current == Some(profile.name.as_str()))
                .on_click(move |_, window, cx| pick(Some(profile.clone()), window, cx)),
        )
    })
}

fn profile_submenu(
    menu: PopupMenu,
    name: &str,
    level: Rc<Tree<TargetProfile>>,
    current: Option<&str>,
    pick: &PickProfile,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (current, pick) = (current.map(str::to_owned), Rc::clone(pick));
    menu.submenu(name.to_owned(), window, cx, move |menu, window, cx| {
        let menu = profile_entries(menu, &level, current.as_deref(), &pick);
        level.children.iter().fold(menu, |menu, (name, child)| {
            profile_submenu(
                menu,
                name,
                Rc::new(child.clone()),
                current.as_deref(),
                &pick,
                window,
                cx,
            )
        })
    })
}
