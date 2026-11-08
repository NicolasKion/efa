//! pyfa's Implant Set Editor (gui/setEditor.py) and the name prompt behind "Save as New Implant
//! Set". Laid out like the cargo preset editor, from the same [`editor_kit`] pieces: the saved
//! sets, the chosen set as its ten slots, and every implant there is, by slot, to drag onto it.

mod tree;

use std::{collections::HashSet, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::InputState,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Action, Efa, Loaded,
    dialogs::{Confirm, Prompt, confirm, prompt_text},
    drag::{DragPreview, DragSource, Dragged},
    editor_kit::{
        self, CONTEXT, Delete, Duplicate, FOLD_W, FocusSearch, ListEntry, MIDDLE_MIN_W, Rename,
        Sidebar, icon_button, name_problem, next_after_delete,
    },
    item_data, item_stats,
    stats::format,
    tooltips,
    widgets::{DropHighlight, caption, count, empty_state, icon_box, redraw_on_change},
};
use crate::{
    data::TypeId,
    fit::implant_slot,
    icons::Icon,
    implant_set::{self, ImplantSet, SLOTS},
    item_data::ItemData,
    prices::Prices,
};
use tree::{Fold, PirateSet, SearchSlot, SlotFilter};

/// The editor window's starting size.
const EDITOR_W: f32 = 1100.;
const EDITOR_H: f32 = 700.;
const SLOT_ROW_H: f32 = 38.;
const SLOT_ICON: f32 = 26.;
/// What a new set is called until it is renamed.
const NEW_SET: &str = "New Implant Set";

const NAME_MISSING: &str = "You must supply a name for the Implant Set!";
const NAME_TAKEN: &str = "Implant Set name already in use, please choose another.";

impl Efa {
    /// Change the saved sets, keeping them sorted by name, and write them out.
    fn edit_implant_sets(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Vec<ImplantSet>),
    ) {
        edit(&mut self.saved.implant_sets);
        self.saved.implant_sets.sort_by(|a, b| a.name.cmp(&b.name));
        self.save();
        cx.notify();
    }

    /// Open the set editor, showing the named set if there is one.
    pub(super) fn open_implant_set_editor(
        &mut self,
        select: Option<String>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let efa = cx.entity();
        // Worked out here, while this view can still be read directly.
        let select = select.or_else(|| self.saved.implant_sets.first().map(|s| s.name.clone()));
        let priced: HashSet<TypeId> = self
            .saved
            .implant_sets
            .iter()
            .flat_map(|s| s.implants.iter().copied())
            .collect();
        // A window of its own, like pyfa's editors, so it can sit beside the fit.
        let shown = select.clone();
        let opening = crate::ui::chrome::open_single(
            "Implant Set Editor",
            (EDITOR_W, EDITOR_H),
            cx,
            move |e: &mut ImplantSetEditor, _, cx| {
                if shown.is_some() {
                    e.select(shown, cx);
                }
            },
            move |window, cx| ImplantSetEditor::new(&efa, loaded, select, window, cx),
        );
        Self::report_open(opening, cx);
        // The sets' prices, for their totals.
        self.with_prices_for(priced, cx, |_, _| {});
    }

    /// Ask for a name, save the fit's applied implants under it, then show the set in the
    /// editor.
    pub(super) fn save_implant_set_dialog(
        implants: Vec<TypeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.weak_entity();
        let check = {
            let this = this.clone();
            move |name: &str, cx: &App| set_name_problem(&this, name, None, cx)
        };
        prompt_text(
            window,
            cx,
            Prompt::new("New Implant Set").message("Enter a name for your new Implant Set:"),
            check,
            move |name, window, cx| {
                Efa::app_update(&this, cx, |g, cx| {
                    let set = ImplantSet {
                        name: name.clone(),
                        implants: implants.clone(),
                    };
                    g.edit_implant_sets(cx, |sets| sets.push(set));
                    g.open_implant_set_editor(Some(name), window, cx);
                });
            },
        );
    }
}

/// What is wrong with a name for a set, if anything. `keep` is the set being renamed, which
/// may keep its name.
fn set_name_problem(
    efa: &WeakEntity<Efa>,
    name: &str,
    keep: Option<&str>,
    cx: &App,
) -> Option<SharedString> {
    let efa = efa.upgrade();
    let sets = efa
        .as_ref()
        .map_or(&[][..], |g| &g.read(cx).saved.implant_sets);
    name_problem(name, sets.iter().map(|s| s.name.as_str()), keep)
        .map(|p| p.say(NAME_MISSING, NAME_TAKEN))
}

/// The editor's own state; the sets themselves live in [`Efa`]'s saved data.
pub struct ImplantSetEditor {
    efa: WeakEntity<Efa>,
    loaded: Rc<Loaded>,
    /// The set being edited, by name; names are unique.
    selected: Option<String>,
    /// The slot picked in the set, which Delete empties.
    slot: Option<u8>,
    list_search: Entity<InputState>,
    search: Entity<InputState>,
    filter: SlotFilter,
    /// What of the tree is folded away; everything starts open.
    folded: HashSet<Fold>,
    /// Every implant by slot and kind, worked out once.
    by_slot: Vec<SearchSlot>,
    /// Every grade of every pirate and faction set, by name.
    pirate_sets: Vec<PirateSet>,
    notice: Option<SharedString>,
    focus: FocusHandle,
    /// The implant list is open in a narrow window, where it is folded away by default.
    tree_open: bool,
    /// The window is narrow, as of the last frame.
    narrow: bool,
    _subscriptions: Vec<Subscription>,
}

impl ImplantSetEditor {
    fn new(
        efa: &Entity<Efa>,
        loaded: Rc<Loaded>,
        select: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search sets"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search implants"));
        let subscriptions = vec![
            cx.observe(efa, |_, _, cx| cx.notify()),
            redraw_on_change(cx, &search),
            redraw_on_change(cx, &list_search),
            // Descriptions, for the bonus lines, load in the background.
            item_data::observe_data(cx),
        ];
        // Start loading them now rather than while drawing.
        item_data::hover_data(&loaded, cx);
        let catalog = &loaded.catalog;
        let pirate_sets = implant_set::precalculated(catalog)
            .iter()
            .flat_map(|set| {
                set.grades.iter().map(move |grade| {
                    let name = implant_set::grade_name(set, grade);
                    PirateSet {
                        lower: name.to_lowercase(),
                        name: name.into(),
                        implants: Rc::from(grade.1.as_slice()),
                    }
                })
            })
            .collect();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            efa: efa.downgrade(),
            by_slot: SearchSlot::index(catalog, implant_set::by_slot(catalog)),
            pirate_sets,
            loaded,
            selected: select,
            slot: None,
            list_search,
            search,
            filter: SlotFilter::All,
            folded: HashSet::new(),
            notice: None,
            focus,
            tree_open: false,
            narrow: false,
            _subscriptions: subscriptions,
        }
    }

    fn sets<'a>(&self, cx: &'a App) -> &'a [ImplantSet] {
        self.efa
            .upgrade()
            .map_or(&[], |g| g.read(cx).saved.implant_sets.as_slice())
    }

    fn prices(&self, cx: &App) -> Rc<Prices> {
        self.efa
            .upgrade()
            .map_or_else(Rc::default, |g| Rc::clone(&g.read(cx).prices))
    }

    fn current<'a>(&self, cx: &'a App) -> Option<&'a ImplantSet> {
        let name = self.selected.as_deref()?;
        self.sets(cx).iter().find(|s| s.name == name)
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Vec<ImplantSet>)) {
        Efa::app_update(&self.efa, cx, |g, cx| g.edit_implant_sets(cx, edit));
    }

    /// Change the named set.
    fn edit_set(&self, name: &str, cx: &mut Context<Self>, edit: impl FnOnce(&mut ImplantSet)) {
        self.edit(cx, |sets| {
            if let Some(set) = sets.iter_mut().find(|s| s.name == name) {
                edit(set);
            }
        });
    }

    /// Fetch prices for these implants if the ones there are are stale.
    fn want_prices(&self, implants: &[TypeId], cx: &mut Context<Self>) {
        let types: HashSet<TypeId> = implants.iter().copied().collect();
        Efa::app_update(&self.efa, cx, |g, cx| {
            g.with_prices_for(types, cx, |_, _| {});
        });
    }

    fn select(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.selected = name;
        self.slot = None;
        cx.notify();
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let initial = implant_set::free_name(self.sets(cx), NEW_SET);
        let (efa, this) = (self.efa.clone(), cx.weak_entity());
        prompt_text(
            window,
            cx,
            Prompt::new("New Implant Set")
                .message("Enter a name for your new Implant Set:")
                .default(initial),
            move |name, cx| set_name_problem(&efa, name, None, cx),
            move |name, _, cx| {
                let _ = this.update(cx, |e, cx| {
                    let set = ImplantSet::new(name.clone());
                    e.edit(cx, |sets| sets.push(set));
                    e.notice = None;
                    e.select(Some(name), cx);
                });
            },
        );
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        let (efa, this, keep) = (self.efa.clone(), cx.weak_entity(), old.clone());
        prompt_text(
            window,
            cx,
            Prompt::new("Rename Implant Set")
                .message("Enter a new name for the Implant Set:")
                .default(old.clone()),
            move |name, cx| set_name_problem(&efa, name, Some(&keep), cx),
            move |name, _, cx| {
                let old = old.clone();
                let _ = this.update(cx, |e, cx| {
                    let new = name.clone();
                    e.edit_set(&old, cx, |set| set.name = new);
                    e.select(Some(name), cx);
                });
            },
        );
    }

    /// Copy the set as "<name> copy", numbered if that is taken.
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(set) = self.current(cx).cloned() else {
            return;
        };
        let name = implant_set::copy_name(self.sets(cx), &set.name);
        let copy = ImplantSet {
            name: name.clone(),
            implants: set.implants,
        };
        self.edit(cx, |sets| sets.push(copy));
        self.notice = Some(format!("Made {name}.").into());
        self.select(Some(name), cx);
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let this = cx.weak_entity();
        confirm(
            window,
            cx,
            Confirm::delete(format!(
                "Delete the implant set \"{name}\"? This cannot be undone."
            )),
            move |_, cx| {
                let _ = this.update(cx, |e, cx| e.delete(&name, cx));
            },
        );
    }

    /// Delete a set and show the one that took its place in the list.
    fn delete(&mut self, name: &str, cx: &mut Context<Self>) {
        let at = self.sets(cx).iter().position(|s| s.name == name);
        let gone = name.to_owned();
        self.edit(cx, |sets| sets.retain(|s| s.name != gone));
        let next = next_after_delete(self.sets(cx), at).map(|s| s.name.clone());
        self.notice = Some(format!("Deleted {name}.").into());
        self.select(next, cx);
    }

    /// Delete: empty the picked slot, or with none picked, delete the set.
    fn delete_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = self.slot.and_then(|slot| {
            self.current(cx)
                .and_then(|set| set.in_slot(&self.loaded.catalog, slot))
        });
        match picked {
            Some(implant) => self.remove_implants(&[implant], cx),
            None => self.confirm_delete(window, cx),
        }
    }

    /// Put implants into their slots in the chosen set, making a set first if there is none.
    fn add_implants(&mut self, implants: &[TypeId], cx: &mut Context<Self>) {
        let name = if let Some(name) = self.selected.clone() {
            name
        } else {
            let name = implant_set::free_name(self.sets(cx), NEW_SET);
            let set = ImplantSet::new(name.clone());
            self.edit(cx, |sets| sets.push(set));
            self.notice = Some(format!("Made {name} for them.").into());
            self.selected = Some(name.clone());
            name
        };
        self.add_to(&name, implants, cx);
        self.slot = implants
            .last()
            .map(|&t| implant_slot(&self.loaded.catalog, t));
    }

    fn add_to(&mut self, name: &str, implants: &[TypeId], cx: &mut Context<Self>) {
        let loaded = Rc::clone(&self.loaded);
        let list = implants.to_vec();
        self.edit_set(name, cx, |set| {
            set.add_all(&loaded.catalog, &list);
        });
        self.want_prices(implants, cx);
        cx.notify();
    }

    fn remove_implants(&mut self, implants: &[TypeId], cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let list = implants.to_vec();
        self.edit_set(&name, cx, |set| {
            for t in list {
                set.remove(t);
            }
        });
        cx.notify();
    }

    /// Put the set's implants on the current fit, as the Implants tab's menu does.
    fn apply_to_fit(&mut self, cx: &mut Context<Self>) {
        let Some(implants) = self.current(cx).map(|s| s.implants.clone()) else {
            return;
        };
        let mut applied = false;
        Efa::app_update(&self.efa, cx, |g, cx| {
            if g.fit().is_some() {
                g.apply(Action::AddImplants(implants), cx);
                applied = true;
            }
        });
        self.notice = Some(if applied {
            "Put on the current fit.".into()
        } else {
            "Open a fit first.".into()
        });
        cx.notify();
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.notice = Some("There is no text on the clipboard to import.".into());
            cx.notify();
            return;
        };
        let imported = implant_set::import(&self.loaded.catalog, &text);
        let (found, errors) = (imported.sets.len(), imported.errors);
        let sets = count(found, "set", "sets");
        self.notice = Some(match (found, errors) {
            (0, _) => "No implant sets found on the clipboard.".into(),
            (_, 0) => format!("Imported {sets} from the clipboard.").into(),
            _ => format!(
                "Imported {sets} from the clipboard; {} could not be read.",
                count(errors, "line", "lines")
            )
            .into(),
        });
        let first = imported.sets.first().map(|s| s.name.clone());
        let implants: Vec<TypeId> = imported
            .sets
            .iter()
            .flat_map(|s| s.implants.iter().copied())
            .collect();
        let loaded = Rc::clone(&self.loaded);
        self.edit(cx, |sets| {
            implant_set::merge(sets, imported.sets, &loaded.catalog);
        });
        self.want_prices(&implants, cx);
        if first.is_some() {
            self.select(first, cx);
        }
        cx.notify();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let sets = self.sets(cx);
        let total = sets.len();
        let text = implant_set::export(&self.loaded.catalog, sets);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice =
            Some(format!("Copied {} to the clipboard.", count(total, "set", "sets")).into());
        cx.notify();
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    fn toggle(&mut self, fold: Fold, cx: &mut Context<Self>) {
        if !self.folded.remove(&fold) {
            self.folded.insert(fold);
        }
        cx.notify();
    }

    // The set list.

    fn list_column(&self, entries: Vec<ListEntry>, cx: &mut Context<Self>) -> AnyElement {
        let query = self.list_search.read(cx).value().to_string();
        let any = !entries.is_empty();
        let rows = editor_kit::name_list(
            entries,
            &query,
            self.selected.as_deref(),
            cx,
            |e: &mut Self, name, window, cx| {
                e.notice = None;
                e.select(Some(name), cx);
                window.focus(&e.focus, cx);
            },
            // Implants dropped on a set in the list go into that set.
            Some(|e: &mut Self, name, dragged, cx| {
                let implants: Vec<TypeId> = dragged.types().collect();
                e.add_to(&name, &implants, cx);
                e.select(Some(name), cx);
            }),
        );
        let has = self.selected.is_some();
        let empty = if any {
            empty_state(
                AssetIcon::SearchX,
                "No sets match",
                "Clear the search to see them all.",
                cx,
            )
        } else {
            empty_state(
                AssetIcon::Layers,
                "No sets yet",
                "Click New, import sets from the clipboard, or save a fit's implants from the \
                 Implants tab's menu.",
                cx,
            )
        };
        let footer = h_flex()
            .gap_1()
            .child(
                icon_button("set-new", AssetIcon::Plus, "New set")
                    .on_click(cx.listener(|e, _, window, cx| e.create(window, cx))),
            )
            .child(
                icon_button("set-duplicate", AssetIcon::Copy, "Duplicate (Ctrl+D)")
                    .disabled(!has)
                    .on_click(cx.listener(|e, _, _, cx| e.duplicate(cx))),
            )
            .child(
                icon_button("set-rename", AssetIcon::Pencil, "Rename (F2)")
                    .disabled(!has)
                    .on_click(cx.listener(|e, _, window, cx| e.rename(window, cx))),
            )
            .child(
                icon_button(
                    "sets-import",
                    AssetIcon::ClipboardPaste,
                    "Import sets from the clipboard, in pyfa's format",
                )
                .on_click(cx.listener(|e, _, _, cx| e.import(cx))),
            )
            .child(
                icon_button(
                    "sets-export",
                    AssetIcon::ClipboardCopy,
                    "Copy every set to the clipboard, in pyfa's format",
                )
                .disabled(!any)
                .on_click(cx.listener(|e, _, _, cx| e.export(cx))),
            )
            .child(div().flex_1())
            .child(
                icon_button("set-delete", AssetIcon::Trash, "Delete (Delete)")
                    .disabled(!has)
                    .on_click(cx.listener(|e, _, window, cx| e.confirm_delete(window, cx))),
            );
        Sidebar {
            caption: "Implant sets".into(),
            search: Some(self.list_search.clone()),
            list_id: "implant-set-list",
            rows,
            empty: Some(empty.into_any_element()),
            footer: Some(footer.into_any_element()),
            narrow: self.narrow,
        }
        .render(cx)
    }

    // The chosen set.

    fn set_column(
        &self,
        set: Option<&ImplantSet>,
        any_sets: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let column = v_flex()
            .id("implant-set")
            .flex_1()
            .min_w(px(if self.narrow { MIDDLE_MIN_W } else { 0. }))
            .h_full()
            .gap_2()
            .p_3()
            .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Market))
            .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                cx.stop_propagation();
                if matches!(dragged.source, DragSource::Market) {
                    let implants: Vec<TypeId> = dragged.types().collect();
                    e.add_implants(&implants, cx);
                }
            }));
        let Some(set) = set else {
            let (title, hint) = if any_sets {
                (
                    "No set picked",
                    "Pick a set on the left, or make a new one.",
                )
            } else {
                (
                    "No implant sets yet",
                    "Make one with New, or drag implants here from the list on the right and \
                     one is made for them.",
                )
            };
            return column
                .child(v_flex().flex_1().justify_center().child(empty_state(
                    AssetIcon::Layers,
                    title,
                    hint,
                    cx,
                )))
                .into_any_element();
        };
        let prices = self.prices(cx);
        let data = item_data::hover_data(&self.loaded, cx);
        let known: Vec<f64> = set
            .implants
            .iter()
            .filter_map(|&t| prices.get(t).filter(|&p| p > 0.0))
            .collect();
        let unpriced = set.implants.len() - known.len();
        let total: f64 = known.iter().sum();
        let has_fit = self
            .efa
            .upgrade()
            .is_some_and(|g| g.read(cx).fit().is_some());
        let rows: Vec<AnyElement> = SLOTS
            .map(|slot| self.slot_row(set, slot, &prices, data.as_deref(), cx))
            .collect();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let missing = if unpriced > 0 {
            format!(
                " ({} without a price)",
                count(unpriced, "implant", "implants")
            )
        } else {
            String::new()
        };
        let summary = format!(
            "{} of {} slots filled, {} ISK{missing}",
            set.implants.len(),
            SLOTS.end(),
            format::currency(total)
        );
        let toggle = self.narrow.then(|| {
            Button::new("set-tree-toggle")
                .small()
                .ghost()
                .icon(UiIcon::new(AssetIcon::ListTree))
                .when(self.tree_open, |b| b.selected(true))
                .tooltip("Show or hide the implant list")
                .on_click(cx.listener(|e, _, _, cx| {
                    e.tree_open = !e.tree_open;
                    cx.notify();
                }))
        });
        let header = h_flex()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(set.name.clone()),
                    )
                    .child(div().text_xs().text_color(muted).truncate().child(summary)),
            )
            .children(toggle)
            .child(
                Button::new("set-apply")
                    .small()
                    .primary()
                    .icon(UiIcon::new(AssetIcon::Check))
                    .label("Apply")
                    .disabled(!has_fit || set.implants.is_empty())
                    .tooltip("Put these implants on the current fit")
                    .on_click(cx.listener(|e, _, _, cx| e.apply_to_fit(cx))),
            );
        let hint = if set.implants.is_empty() {
            "This set is empty. Drag implants here from the list on the right, or double-click \
             them there. Each one goes into its own slot."
        } else {
            "Drop an implant anywhere here and it goes into its slot. Drag one back to the list, \
             press Delete or click its x to take it out."
        };
        column
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1p5()
                    .child(caption("Slots", cx))
                    .child(
                        v_flex()
                            .id("implant-slots")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .gap_0p5()
                            .children(rows),
                    ),
            )
            .child(div().text_xs().text_color(muted).child(hint))
            .when_some(self.notice.clone(), |col, notice| {
                col.child(div().text_xs().text_color(muted).child(notice))
            })
            .into_any_element()
    }

    fn slot_row(
        &self,
        set: &ImplantSet,
        slot: u8,
        prices: &Prices,
        data: Option<&ItemData>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loaded = Rc::clone(&self.loaded);
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let picked = self.slot == Some(slot);
        let fills = Rc::clone(&loaded);
        let row = h_flex()
            .id(SharedString::from(format!("slot-{slot}")))
            .min_h(px(SLOT_ROW_H))
            .flex_none()
            .px_2()
            .py_1()
            .gap_3()
            .rounded(px(4.))
            .when(picked, |row| row.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            // The row an implant would go into lights up under it.
            .drop_highlight_if::<Dragged>(move |dragged| {
                matches!(dragged.source, DragSource::Market)
                    && dragged
                        .types()
                        .any(|t| implant_slot(&fills.catalog, t) == slot)
            })
            .child(
                div()
                    .w(px(18.))
                    .flex_none()
                    .text_xs()
                    .text_right()
                    .text_color(muted)
                    .child(slot.to_string()),
            );
        let Some(id) = set.in_slot(catalog, slot) else {
            return row
                .child(
                    div()
                        .flex_none()
                        .size(px(SLOT_ICON))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_dashed()
                        .border_color(theme.border)
                        .child(UiIcon::new(AssetIcon::Plus).size(px(14.)).text_color(muted)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .child(format!(
                            "Drop a slot {slot} implant here, or click to list them"
                        )),
                )
                .on_click(cx.listener(move |e, _, window, cx| {
                    e.slot = Some(slot);
                    e.filter = SlotFilter::Slot(slot);
                    window.focus(&e.focus, cx);
                    cx.notify();
                }))
                .into_any_element();
        };
        let price = prices.get(id).filter(|&p| p > 0.0);
        let bonus = data
            .and_then(|data| item_stats::description_text(data, id))
            .and_then(|text| implant_set::main_bonus(&text));
        let tip = Rc::clone(&loaded);
        let row = row
            .child(icon_box(catalog, &loaded.icons, Icon::Type(id), SLOT_ICON))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(catalog.name(id)))
                    .when_some(bonus, |col, bonus| {
                        col.child(div().text_xs().text_color(muted).truncate().child(bonus))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(price.map_or_else(|| "no price".to_owned(), format::currency)),
            )
            .child(
                Button::new(SharedString::from(format!("slot-remove-{slot}")))
                    .ghost()
                    .xsmall()
                    .icon(UiIcon::new(AssetIcon::X))
                    .tooltip("Take it out of the set (Delete)")
                    .on_click(cx.listener(move |e, _, _, cx| {
                        cx.stop_propagation();
                        e.remove_implants(&[id], cx);
                    })),
            )
            .tooltip(move |window, cx| tooltips::item_tooltip(&tip, id, price, window, cx))
            .on_click(cx.listener(move |e, _, window, cx| {
                e.slot = Some(slot);
                window.focus(&e.focus, cx);
                cx.notify();
            }));
        match Dragged::of(&loaded, DragSource::Listed, Some(id)) {
            Some(dragged) => row
                .on_drag(dragged, |dragged, _, _, cx| {
                    cx.new(|_| DragPreview(dragged.clone()))
                })
                .into_any_element(),
            None => row.into_any_element(),
        }
    }
}

impl Render for ImplantSetEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.narrow = window.viewport_size().width < px(FOLD_W);
        // Only the list's lines and the shown set are taken from the sets.
        let (entries, selected) = {
            let sets = self.sets(cx);
            // Another window may have removed or renamed the set this one was showing.
            if self
                .selected
                .as_ref()
                .is_none_or(|name| !sets.iter().any(|s| &s.name == name))
            {
                self.selected = sets.first().map(|s| s.name.clone());
                self.slot = None;
            }
            let entries: Vec<ListEntry> = sets
                .iter()
                .map(|s| ListEntry {
                    name: s.name.clone(),
                    detail: format!("{}/{}", s.implants.len(), SLOTS.end()),
                })
                .collect();
            (entries, self.current(cx).cloned())
        };
        let any_sets = !entries.is_empty();
        let list = self.list_column(entries, cx);
        let middle = self.set_column(selected.as_ref(), any_sets, cx);
        let tree = (!self.narrow || self.tree_open).then(|| self.tree_column(cx));
        // Narrow, an open market takes the list's place while a set is open.
        let list = (!self.narrow || !self.tree_open || selected.is_none()).then_some(list);
        h_flex()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|e, _: &Delete, window, cx| e.delete_picked(window, cx)))
            .on_action(cx.listener(|e, _: &Duplicate, _, cx| e.duplicate(cx)))
            .on_action(cx.listener(|e, _: &Rename, window, cx| e.rename(window, cx)))
            .on_action(cx.listener(|e, _: &FocusSearch, window, cx| e.focus_search(window, cx)))
            // An implant of the set dragged anywhere but the set comes out of it.
            .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                if matches!(dragged.source, DragSource::Listed) {
                    let implants: Vec<TypeId> = dragged.types().collect();
                    e.remove_implants(&implants, cx);
                }
            }))
            .children(list)
            .child(middle)
            .children(tree)
    }
}
