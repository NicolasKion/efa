//! pyfa's mutaplasmid menu entries (gui/builtinContextMenus/itemMutations.py,
//! moduleMutatedExport.py), shared by the fitting view and the drones pane.

use std::rc::Rc;

use gpui_kit::component::{
    IconName,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{App, ClipboardItem, Context, WeakEntity, Window};

use super::{
    Efa, Loaded,
    actions::{Action, MutaTarget},
    widgets::{entry, item_entry, item_label, on_efa},
};
use crate::{
    data::{TypeId, muta::short_name},
    fit::Mutation,
    port::muta,
};

/// Tallest the mutaplasmid list grows before it scrolls.
const MENU_MAX_H: f32 = 480.;

/// "Apply Mutaplasmid" with the mutaplasmids `type_id` takes, or "Revert to" its base item when
/// it is mutated. Nothing when neither applies.
#[expect(
    clippy::too_many_arguments,
    reason = "the entries need the row's whole context"
)]
pub(super) fn mutation_entries(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    type_id: TypeId,
    mutation: Option<&Mutation>,
    target: &MutaTarget,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    if let Some(mutation) = mutation {
        let target = target.clone();
        let label = format!("Revert to {}", catalog.name(mutation.base));
        return menu.item(entry(IconName::Undo2, label).on_click(on_efa(
            this,
            move |g, _, _, cx| {
                g.apply(Action::Revert(target.clone()), cx);
            },
        )));
    }
    let mutaplasmids: Vec<(TypeId, String)> = catalog
        .mutaplasmids
        .for_base(type_id)
        .iter()
        .map(|&m| (m, short_name(&catalog.name(m))))
        .collect();
    let Some(&(first, _)) = mutaplasmids.first() else {
        return menu;
    };
    let (this, loaded, target) = (this.clone(), Rc::clone(loaded), target.clone());
    menu.submenu_with_element(
        "Apply Mutaplasmid",
        item_label(&loaded, Some(first), "Apply Mutaplasmid"),
        window,
        cx,
        move |menu, window, _| {
            mutaplasmids.iter().fold(
                menu.scrollable(true)
                    .max_h(crate::ui::widgets::menu_max_h(window, MENU_MAX_H)),
                |menu, (id, name)| {
                    let (target, id) = (target.clone(), *id);
                    menu.item(item_entry(&loaded, id, name.clone()).on_click(on_efa(
                        &this,
                        move |g, _, _, cx| {
                            g.apply(Action::Mutate(target.clone(), id), cx);
                        },
                    )))
                },
            )
        },
    )
}

/// pyfa's "Copy Module to Clipboard": the mutated module as muta text.
pub(super) fn copy_entry(loaded: &Rc<Loaded>, mutation: &Mutation) -> PopupMenuItem {
    let text = muta::render(&loaded.catalog, mutation, "", "  ");
    entry(IconName::Copy, "Copy Module to Clipboard").on_click(move |_, _, cx: &mut App| {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    })
}
