//! The Cargo Presets editor, a window of its own: presets on the left, the selected one in the
//! middle and the market to add from on the right. Also the name prompt behind "Save Cargo as
//! Preset". A efa extra.

mod preset;

use std::{collections::HashMap, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Disableable, h_flex, input::InputState, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Action, AdditionsTab, Efa, Loaded,
    dialogs::{Confirm, Prompt, confirm, prompt_text},
    drag::{DragSource, Dragged},
    editor_kit::{
        self, CONTEXT, Delete, Duplicate, FOLD_W, FocusSearch, ItemTree, ListEntry, Rename,
        Sidebar, icon_button, name_problem, next_after_delete,
    },
    widgets::{DropHighlight, caption, count, empty_state, redraw_on_change},
};
use crate::{
    cargo_preset::{CargoPolicy, CargoPreset},
    data::{ItemType, Kind, TypeId},
};

/// The editor window's starting size.
const EDITOR_W: f32 = 1100.;
const EDITOR_H: f32 = 700.;
const TREE_W: f32 = 340.;

const NAME_MISSING: &str = "Enter a name for the preset.";
const NAME_TAKEN: &str = "A preset with this name already exists.";

/// What the item tree offers: anything a hold can carry.
fn carried(t: &ItemType) -> bool {
    !matches!(t.kind, Kind::Ship | Kind::Skill | Kind::Mode | Kind::Other)
}

/// What is wrong with a name, given the presets; `keep` is the one being renamed.
fn preset_name_problem(
    name: &str,
    presets: &[CargoPreset],
    keep: Option<&str>,
) -> Option<SharedString> {
    name_problem(name, presets.iter().map(|p| p.name.as_str()), keep)
        .map(|p| p.say(NAME_MISSING, NAME_TAKEN))
}

/// "3 items" for a list's detail.
fn item_count(n: usize) -> String {
    count(n, "item", "items")
}

impl Efa {
    /// Change the saved presets, keeping them sorted by name, and write them out.
    fn edit_cargo_presets(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Vec<CargoPreset>),
    ) {
        edit(&mut self.saved.cargo_presets);
        self.saved
            .cargo_presets
            .sort_by_key(|p| p.name.to_lowercase());
        self.save();
        cx.notify();
    }

    /// Put a saved preset into the current fit's hold, as one undo step, and show the Cargo tab.
    pub(super) fn apply_cargo_preset(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(preset) = self
            .saved
            .cargo_presets
            .iter()
            .find(|p| p.name == name)
            .cloned()
        else {
            return;
        };
        if self.fit().is_none() {
            self.set_status("Open a fit first");
            cx.notify();
            return;
        }
        self.show_additions_tab(AdditionsTab::Cargo);
        self.apply(Action::ApplyCargoPreset(preset), cx);
    }

    /// Open the preset editor, showing the named preset if there is one.
    pub(super) fn open_cargo_preset_editor(
        &mut self,
        select: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let efa = cx.entity();
        let shown = select.clone();
        let select = select.or_else(|| self.saved.cargo_presets.first().map(|p| p.name.clone()));
        let opening = crate::ui::chrome::open_single(
            "Cargo Presets",
            (EDITOR_W, EDITOR_H),
            cx,
            move |e: &mut CargoPresetEditor, window, cx| {
                if let Some(name) = shown {
                    e.select(Some(name), window, cx);
                }
            },
            move |window, cx| CargoPresetEditor::new(&efa, loaded, select, window, cx),
        );
        Self::report_open(opening, cx);
    }

    /// Ask for a name and save the current fit's cargo under it.
    pub(super) fn save_cargo_preset_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fit) = self.fit() else { return };
        let mut items: Vec<(TypeId, u32)> = Vec::new();
        for &(t, n) in &fit.cargo {
            match items.iter_mut().find(|(i, _)| *i == t) {
                Some((_, count)) => *count = count.saturating_add(n),
                None => items.push((t, n)),
            }
        }
        let this = cx.weak_entity();
        let check = {
            let this = this.clone();
            move |name: &str, cx: &App| {
                let saved = this.upgrade()?;
                preset_name_problem(name, &saved.read(cx).saved.cargo_presets, None)
            }
        };
        prompt_text(
            window,
            cx,
            Prompt::new("Save Cargo as Preset").message("Enter a name for the new cargo preset:"),
            check,
            move |name, _, cx| {
                let preset = CargoPreset {
                    name: name.clone(),
                    items: items.clone(),
                    policy: CargoPolicy::default(),
                };
                Efa::app_update(&this, cx, |g, cx| {
                    g.edit_cargo_presets(cx, |presets| presets.push(preset));
                    g.set_status(format!("Saved the cargo as preset {name}"));
                });
            },
        );
    }
}

/// The editor's own state; the presets themselves live in [`Efa`]'s saved data.
pub struct CargoPresetEditor {
    efa: WeakEntity<Efa>,
    loaded: Rc<Loaded>,
    /// The preset being edited, by name; names are unique.
    selected: Option<String>,
    /// The item row last clicked in the selected preset, which Delete removes.
    picked: Option<TypeId>,
    list_search: Entity<InputState>,
    tree: ItemTree,
    /// Quantity fields of the selected preset's items.
    quantities: HashMap<TypeId, (Entity<InputState>, Subscription)>,
    focus: FocusHandle,
    notice: Option<SharedString>,
    /// The market column is open in a narrow window, where it is folded away by default.
    market_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl CargoPresetEditor {
    fn new(
        efa: &Entity<Efa>,
        loaded: Rc<Loaded>,
        selected: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search presets"));
        let tree = ItemTree::new(&loaded.catalog, carried, "Search items", window, cx);
        let subscriptions = vec![
            cx.observe(efa, |_, _, cx| cx.notify()),
            redraw_on_change(cx, &list_search),
            redraw_on_change(cx, &tree.search),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            efa: efa.downgrade(),
            loaded,
            selected,
            picked: None,
            list_search,
            tree,
            quantities: HashMap::new(),
            focus,
            notice: None,
            market_open: false,
            _subscriptions: subscriptions,
        }
    }

    fn presets<'a>(&self, cx: &'a App) -> &'a [CargoPreset] {
        self.efa
            .upgrade()
            .map_or(&[], |g| g.read(cx).saved.cargo_presets.as_slice())
    }

    fn current(&self, cx: &App) -> Option<CargoPreset> {
        let name = self.selected.as_ref()?;
        self.presets(cx).iter().find(|p| &p.name == name).cloned()
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Vec<CargoPreset>)) {
        Efa::app_update(&self.efa, cx, |g, cx| g.edit_cargo_presets(cx, edit));
    }

    /// Change the selected preset.
    fn edit_current(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut CargoPreset)) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        self.edit(cx, |presets| {
            if let Some(p) = presets.iter_mut().find(|p| p.name == name) {
                edit(p);
            }
        });
    }

    fn select(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != name {
            self.quantities.clear();
        }
        self.picked = None;
        self.selected = name;
        self.notice = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_name(Naming::New, window, cx);
    }

    /// Ask for a name for a new, copied or renamed preset.
    fn ask_name(&mut self, naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.selected.clone();
        if naming != Naming::New && current.is_none() {
            return;
        }
        let current = current.unwrap_or_default();
        let (title, message, default) = match naming {
            Naming::New => (
                "New Cargo Preset",
                "Enter a name for the new preset:",
                String::new(),
            ),
            Naming::Duplicate => (
                "Duplicate Cargo Preset",
                "Enter a name for the copy:",
                format!("{current} copy"),
            ),
            Naming::Rename => ("Rename Cargo Preset", "Enter a new name:", current.clone()),
        };
        let editor = cx.weak_entity();
        let check = {
            let editor = editor.clone();
            let keep = (naming == Naming::Rename).then(|| current.clone());
            move |name: &str, cx: &App| {
                let editor = editor.upgrade()?;
                preset_name_problem(name, editor.read(cx).presets(cx), keep.as_deref())
            }
        };
        prompt_text(
            window,
            cx,
            Prompt::new(title).message(message).default(default),
            check,
            move |name, window, cx| {
                let _ = editor.update(cx, |e, cx| e.named(naming, name, window, cx));
            },
        );
    }

    fn named(&mut self, naming: Naming, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.selected.clone().unwrap_or_default();
        let new = name.clone();
        self.edit(cx, |presets| match naming {
            Naming::New => presets.push(CargoPreset::new(new)),
            Naming::Duplicate => {
                if let Some(p) = presets.iter().find(|p| p.name == current) {
                    let copy = CargoPreset {
                        name: new,
                        ..p.clone()
                    };
                    presets.push(copy);
                }
            }
            Naming::Rename => {
                if let Some(p) = presets.iter_mut().find(|p| p.name == current) {
                    p.name = new;
                }
            }
        });
        if naming == Naming::Rename {
            // Keep the quantity fields: the items did not change.
            self.selected = Some(name);
            cx.notify();
        } else {
            self.select(Some(name), window, cx);
        }
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let editor = cx.weak_entity();
        confirm(
            window,
            cx,
            Confirm::delete(format!("Delete the cargo preset {name}?")),
            move |window, cx| {
                let _ = editor.update(cx, |e, cx| e.delete(window, cx));
            },
        );
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        let at = self.presets(cx).iter().position(|p| p.name == old);
        self.edit(cx, |presets| presets.retain(|p| p.name != old));
        let next = next_after_delete(self.presets(cx), at).map(|p| p.name.clone());
        self.select(next, window, cx);
    }

    /// Delete: take the picked item out of the preset, or with none picked, delete the preset.
    fn delete_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.picked {
            Some(t) => self.remove_item(t, cx),
            None => self.confirm_delete(window, cx),
        }
    }

    fn add_item(&mut self, type_id: TypeId, count: u32, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            self.notice = Some("Pick or create a preset first.".into());
            cx.notify();
            return;
        }
        self.edit_current(cx, |p| p.add(type_id, count));
    }

    fn remove_item(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        self.quantities.remove(&type_id);
        if self.picked == Some(type_id) {
            self.picked = None;
        }
        self.edit_current(cx, |p| p.remove(type_id));
    }

    /// Take the typed quantity, or put the stored one back if it does not read as a number.
    fn commit_quantity(&mut self, type_id: TypeId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((input, _)) = self.quantities.get(&type_id) else {
            return;
        };
        let input = input.clone();
        let typed = input.read(cx).value().trim().parse::<u32>();
        let stored = self.current(cx).and_then(|p| {
            p.items
                .iter()
                .find(|&&(t, _)| t == type_id)
                .map(|&(_, n)| n)
        });
        match (typed, stored) {
            (Ok(n), Some(old)) if n != old => {
                if n == 0 {
                    self.quantities.remove(&type_id);
                }
                self.edit_current(cx, |p| p.set(type_id, n));
            }
            (_, Some(old)) => {
                input.update(cx, |i, cx| i.set_value(old.to_string(), window, cx));
            }
            (_, None) => {}
        }
    }

    /// Add every stack of the current fit's cargo to the preset.
    fn add_fit_cargo(&mut self, cx: &mut Context<Self>) {
        let cargo = self
            .efa
            .upgrade()
            .and_then(|g| g.read(cx).fit().map(|f| f.cargo.clone()));
        let Some(cargo) = cargo else {
            self.notice = Some("Open a fit to take its cargo.".into());
            cx.notify();
            return;
        };
        if cargo.is_empty() {
            self.notice = Some("The current fit's cargo is empty.".into());
            cx.notify();
            return;
        }
        let units: u32 = cargo.iter().fold(0, |sum, &(_, n)| sum.saturating_add(n));
        self.edit_current(cx, |p| {
            for &(t, n) in &cargo {
                p.add(t, n);
            }
        });
        self.notice = Some(format!("Added {units} units from the fit's cargo.").into());
        cx.notify();
    }

    fn apply_to_fit(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        Efa::app_update(&self.efa, cx, |g, cx| g.apply_cargo_preset(&name, cx));
        self.notice = Some("Applied to the current fit; see its status line.".into());
        cx.notify();
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tree
            .search
            .update(cx, |input, cx| input.focus(window, cx));
    }

    fn list_column(
        &self,
        entries: Vec<ListEntry>,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.list_search.read(cx).value().to_string();
        let any = !entries.is_empty();
        let rows = editor_kit::name_list(
            entries,
            &query,
            self.selected.as_deref(),
            cx,
            |e: &mut Self, name, window, cx| e.select(Some(name), window, cx),
            None,
        );
        let has = self.selected.is_some();
        let empty = if any {
            empty_state(
                AssetIcon::SearchX,
                "No presets match",
                "Clear the search to see them all.",
                cx,
            )
        } else {
            empty_state(
                AssetIcon::Package,
                "No presets yet",
                "Click New, or save a fit's cargo from the Cargo tab's menu.",
                cx,
            )
        };
        let footer = h_flex()
            .gap_1()
            .child(
                icon_button("preset-new", AssetIcon::Plus, "New preset")
                    .on_click(cx.listener(|e, _, window, cx| e.create(window, cx))),
            )
            .child(
                icon_button("preset-duplicate", AssetIcon::Copy, "Duplicate (Ctrl+D)")
                    .disabled(!has)
                    .on_click(cx.listener(|e, _, window, cx| {
                        e.ask_name(Naming::Duplicate, window, cx);
                    })),
            )
            .child(
                icon_button("preset-rename", AssetIcon::Pencil, "Rename (F2)")
                    .disabled(!has)
                    .on_click(cx.listener(|e, _, window, cx| {
                        e.ask_name(Naming::Rename, window, cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                icon_button("preset-delete", AssetIcon::Trash, "Delete (Delete)")
                    .disabled(!has)
                    .on_click(cx.listener(|e, _, window, cx| e.confirm_delete(window, cx))),
            );
        Sidebar {
            caption: "Cargo presets".into(),
            search: Some(self.list_search.clone()),
            list_id: "preset-list",
            rows,
            empty: Some(empty.into_any_element()),
            footer: Some(footer.into_any_element()),
            narrow,
        }
        .render(cx)
    }
}

/// Which name prompt is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Naming {
    New,
    Duplicate,
    Rename,
}

impl Render for CargoPresetEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only the list's lines and the shown preset are taken from the presets.
        let (entries, preset) = {
            let presets = self.presets(cx);
            let entries: Vec<ListEntry> = presets
                .iter()
                .map(|p| ListEntry {
                    name: p.name.clone(),
                    detail: item_count(p.items.len()),
                })
                .collect();
            (entries, self.current(cx))
        };
        // Another window may have removed the preset this one was showing.
        if self.selected.is_some() && preset.is_none() {
            self.selected = None;
            self.quantities.clear();
        }
        if let Some(preset) = &preset {
            self.quantities
                .retain(|t, _| preset.items.iter().any(|&(i, _)| i == *t));
        }
        let loaded = Rc::clone(&self.loaded);
        let narrow = window.viewport_size().width < px(FOLD_W);
        let list = self.list_column(entries, narrow, cx);
        let middle = self.preset_column(preset.as_ref(), window, cx);
        let tree = self.tree.render(
            &loaded,
            cx,
            |e: &mut Self| &mut e.tree,
            |e: &mut Self, t, _, cx| e.add_item(t, 1, cx),
        );
        let show_market = !narrow || self.market_open;
        // Narrow, an open market takes the list's place while a preset is open.
        let show_list = !narrow || !self.market_open || preset.is_none();
        let theme = cx.theme();
        h_flex()
            .size_full()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(|e, _: &Delete, window, cx| e.delete_picked(window, cx)))
            .on_action(cx.listener(|e, _: &Duplicate, window, cx| {
                e.ask_name(Naming::Duplicate, window, cx);
            }))
            .on_action(cx.listener(|e, _: &Rename, window, cx| {
                e.ask_name(Naming::Rename, window, cx);
            }))
            .on_action(cx.listener(|e, _: &FocusSearch, window, cx| e.focus_search(window, cx)))
            .when(show_list, |row| row.child(list))
            .child(middle)
            .when(show_market, |row| {
                row.child(
                    v_flex()
                        .when(narrow, |d| d.flex_1().min_w_0().max_w(px(TREE_W)))
                        .when(!narrow, |d| d.w(px(TREE_W)).flex_none())
                        .h_full()
                        .gap_2()
                        .p_2()
                        .border_l_1()
                        .border_color(theme.border)
                        .bg(theme.sidebar)
                        // A preset's item dragged back here comes off the preset.
                        .id("preset-market")
                        .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Listed))
                        .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                            if matches!(dragged.source, DragSource::Listed) {
                                e.remove_item(dragged.type_id, cx);
                            }
                        }))
                        .child(caption("Market", cx))
                        .child(div().flex_1().min_h_0().child(tree))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(
                            "Drag an item onto the preset, or double-click it. Drag one back \
                             here to remove it.",
                        )),
                )
            })
    }
}

/// The Cargo tab's preset entries: apply one, or save the cargo as a new one.
pub(super) fn menu_entries(
    menu: gpui_kit::component::menu::PopupMenu,
    this: &WeakEntity<Efa>,
    window: &mut Window,
    cx: &mut Context<gpui_kit::component::menu::PopupMenu>,
) -> gpui_kit::component::menu::PopupMenu {
    use super::widgets::{entry, icon_label, on_efa};
    use gpui_kit::component::IconName;
    let Some(efa) = this.upgrade() else {
        return menu;
    };
    let g = efa.read(cx);
    let names: Vec<String> = g
        .saved
        .cargo_presets
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let has_cargo = g.fit().is_some_and(|f| !f.cargo.is_empty());
    let mut menu = menu;
    if !names.is_empty() {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Apply Preset",
            icon_label(IconName::Inbox, "Apply Preset"),
            window,
            cx,
            move |menu, _, _| {
                names.iter().fold(menu, |menu, name| {
                    let name = name.clone();
                    menu.item(entry(IconName::Inbox, name.clone()).on_click(on_efa(
                        &this,
                        move |g, _, _, cx| {
                            g.apply_cargo_preset(&name, cx);
                        },
                    )))
                })
            },
        );
    }
    if has_cargo {
        menu = menu.item(
            entry(IconName::Plus, "Save Cargo as Preset…").on_click(on_efa(
                this,
                |g, _, window, cx| {
                    g.save_cargo_preset_dialog(window, cx);
                },
            )),
        );
    }
    menu.item(
        entry(IconName::Settings2, "Edit Cargo Presets…").on_click(on_efa(this, |g, _, _, cx| {
            g.open_cargo_preset_editor(None, cx);
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_must_be_new_and_not_blank() {
        let presets = ["Filaments", "Scripts"].map(CargoPreset::new);
        let problem = |name| preset_name_problem(name, &presets, None);
        assert_eq!(problem(""), Some(NAME_MISSING.into()));
        assert_eq!(problem("Scripts"), Some(NAME_TAKEN.into()));
        assert_eq!(problem("Fuel"), None);
        assert_eq!(
            preset_name_problem("Scripts", &presets, Some("Scripts")),
            None
        );
    }
}
