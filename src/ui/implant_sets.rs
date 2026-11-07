//! pyfa's Implant Set Editor (gui/setEditor.py) and the name prompt behind "Save as New Implant
//! Set". Laid out like the cargo preset editor, from the same [`editor_kit`] pieces: the saved
//! sets, the chosen set as its ten slots, and every implant there is, by slot, to drag onto it.

use std::{collections::HashSet, ops::Range, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, WindowBounds,
    WindowOptions, div, prelude::FluentBuilder, px, size, uniform_list,
};

use super::{
    Action, Efa, Loaded,
    editor_kit::{self, INDENT, ListEntry, TREE_ROW, caption, empty_state, icon_button},
    fitting::{DragPreview, DragSource, Dragged},
    item_stats, side,
    stats::format,
    widgets::icon_box,
};
use crate::{
    data::TypeId,
    fit::implant_slot,
    icons::Icon,
    implant_set::{self, ImplantSet, SLOTS, SlotImplants},
    item_data::ItemData,
    prices::Prices,
};

gpui_kit::actions!(
    implant_set_editor,
    [DeletePicked, DuplicateSet, RenameSet, FocusSearch]
);

const CONTEXT: &str = "ImplantSetEditor";

/// The editor window's starting size.
const EDITOR_W: f32 = 1100.;
const EDITOR_H: f32 = 700.;
const LIST_W: f32 = 230.;
/// The set list's width in a narrow window.
const LIST_NARROW_W: f32 = 180.;
/// Below this window width the implant list folds away behind a button.
const NARROW_W: f32 = 960.;
/// Wide enough for the longest hardwiring names.
const TREE_W: f32 = 420.;
const TREE_ICON: f32 = 20.;
const SLOT_ROW_H: f32 = 38.;
const SLOT_ICON: f32 = 26.;
/// What a new set is called until it is renamed.
const NEW_SET: &str = "New Implant Set";

const NAME_MISSING: &str = "You must supply a name for the Implant Set!";
const NAME_TAKEN: &str = "Implant Set name already in use, please choose another.";

/// The editor's shortcuts. A text field with the focus keeps Delete for its text.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("delete", DeletePicked, Some(CONTEXT)),
        KeyBinding::new("ctrl-d", DuplicateSet, Some(CONTEXT)),
        KeyBinding::new("f2", RenameSet, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
    ]);
}

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
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(EDITOR_W), px(EDITOR_H)), cx)),
            app_id: Some("efa".into()),
            ..Default::default()
        };
        // Opened once this update is over: the new window draws right away, and drawing reads
        // this view, which gpui does not allow while it is being updated.
        cx.defer(move |cx| {
            let weak = efa.downgrade();
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("efa: Implant Set Editor");
                cx.new(|cx| ImplantSetEditor::new(&efa, loaded, select, window, cx))
            });
            match opened {
                // The sets' prices, for their totals.
                Ok(_) => Efa::app_update(&weak, cx, |g, cx| {
                    g.with_prices_for(priced, cx, |_, _| {});
                }),
                Err(e) => Efa::app_update(&weak, cx, |g, cx| {
                    g.status = Some(format!("Could not open the implant set editor: {e}").into());
                    cx.notify();
                }),
            }
        });
    }

    /// Ask for a name, save the fit's applied implants under it, then show the set in the
    /// editor.
    pub(super) fn save_implant_set_dialog(
        implants: Vec<TypeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let check = {
            let this = this.clone();
            move |name: &str, cx: &App| name_problem(&this, name, None, cx)
        };
        editor_kit::prompt_name(
            window,
            cx,
            "New Implant Set".into(),
            "Enter a name for your new Implant Set:".into(),
            String::new(),
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
fn name_problem(
    efa: &WeakEntity<Efa>,
    name: &str,
    keep: Option<&str>,
    cx: &App,
) -> Option<&'static str> {
    if name.is_empty() {
        return Some(NAME_MISSING);
    }
    let taken = efa.upgrade().is_some_and(|g| {
        g.read(cx)
            .saved
            .implant_sets
            .iter()
            .any(|s| s.name == name && Some(s.name.as_str()) != keep)
    });
    taken.then_some(NAME_TAKEN)
}

/// Which part of the implant tree is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotFilter {
    All,
    Slot(u8),
    /// Only the pirate and faction sets, to add whole.
    Sets,
}

impl SlotFilter {
    fn shows_slot(self, slot: u8) -> bool {
        match self {
            Self::All => true,
            Self::Slot(s) => s == slot,
            Self::Sets => false,
        }
    }

    fn shows_sets(self) -> bool {
        matches!(self, Self::All | Self::Sets)
    }
}

/// A part of the tree that folds away.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Fold {
    Slot(u8),
    Group(u8, String),
    Sets,
}

/// One line of the implant tree.
#[derive(Debug, Clone)]
enum TreeRow {
    /// A slot, or the sets: a top-level heading.
    Section {
        fold: Fold,
        title: SharedString,
        count: usize,
        open: bool,
    },
    /// A kind of implant within a slot.
    Group {
        fold: Fold,
        title: SharedString,
        count: usize,
        open: bool,
    },
    Item(TypeId),
    /// One grade of a pirate or faction set, added whole.
    Set {
        name: SharedString,
        implants: Rc<[TypeId]>,
    },
}

/// Whether every word of a search is found in one of the texts, ignoring case.
fn matches(words: &[String], texts: &[&str]) -> bool {
    if words.is_empty() {
        return true;
    }
    let texts: Vec<String> = texts.iter().map(|t| t.to_lowercase()).collect();
    words
        .iter()
        .all(|word| texts.iter().any(|text| text.contains(word.as_str())))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// A drag of these implants out of the tree, shown under `label` when there are several.
fn tree_drag(loaded: &Loaded, implants: &[TypeId], label: Option<&str>) -> Option<Dragged> {
    let (&first, rest) = implants.split_first()?;
    let dragged = Dragged::of(loaded, DragSource::Market, Some(first))?.with_more(rest.to_vec());
    Some(match label {
        Some(label) => dragged.labelled(label.to_owned()),
        None => dragged,
    })
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
    by_slot: Rc<[SlotImplants]>,
    /// Every grade of every pirate and faction set, by name.
    pirate_sets: Rc<[(SharedString, Rc<[TypeId]>)]>,
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
        let redraw =
            |_: &mut Self, _: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            };
        let subscriptions = vec![
            cx.observe(efa, |_, _, cx| cx.notify()),
            cx.subscribe(&search, redraw),
            cx.subscribe(&list_search, redraw),
            // Descriptions, for the bonus lines, load in the background.
            item_stats::observe_data(cx),
        ];
        // Start loading them now rather than while drawing.
        item_stats::hover_data(&loaded, cx);
        let catalog = &loaded.catalog;
        let pirate_sets = implant_set::precalculated(catalog)
            .iter()
            .flat_map(|set| {
                set.grades.iter().map(move |grade| {
                    (
                        SharedString::from(implant_set::grade_name(set, grade)),
                        Rc::from(grade.1.as_slice()),
                    )
                })
            })
            .collect();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            efa: efa.downgrade(),
            by_slot: implant_set::by_slot(catalog).into(),
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
        let (efa, this) = (self.efa.clone(), cx.entity().downgrade());
        editor_kit::prompt_name(
            window,
            cx,
            "New Implant Set".into(),
            "Enter a name for your new Implant Set:".into(),
            initial,
            move |name, cx| name_problem(&efa, name, None, cx),
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
        let (efa, this, keep) = (self.efa.clone(), cx.entity().downgrade(), old.clone());
        editor_kit::prompt_name(
            window,
            cx,
            "Rename Implant Set".into(),
            "Enter a new name for the Implant Set:".into(),
            old.clone(),
            move |name, cx| name_problem(&efa, name, Some(&keep), cx),
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
        let this = cx.entity().downgrade();
        editor_kit::confirm_delete(
            window,
            cx,
            format!("Delete the implant set \"{name}\"? This cannot be undone."),
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
        let sets = self.sets(cx);
        let next = at
            .and_then(|i| sets.get(i).or_else(|| sets.last()))
            .map(|s| s.name.clone());
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
        let (count, errors) = (imported.sets.len(), imported.errors);
        let sets = plural(count, "set", "sets");
        self.notice = Some(match (count, errors) {
            (0, _) => "No implant sets found on the clipboard.".into(),
            (_, 0) => format!("Imported {sets} from the clipboard.").into(),
            _ => format!(
                "Imported {sets} from the clipboard; {} could not be read.",
                plural(errors, "line", "lines")
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
        let count = sets.len();
        let text = implant_set::export(&self.loaded.catalog, sets);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice =
            Some(format!("Copied {} to the clipboard.", plural(count, "set", "sets")).into());
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

    fn list_column(&self, sets: &[ImplantSet], cx: &mut Context<Self>) -> AnyElement {
        let width = if self.narrow { LIST_NARROW_W } else { LIST_W };
        let query = self.list_search.read(cx).value().to_string();
        let entries: Vec<ListEntry> = sets
            .iter()
            .map(|s| ListEntry {
                name: s.name.clone(),
                detail: format!("{}/{}", s.implants.len(), SLOTS.end()),
            })
            .collect();
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
        let empty = if sets.is_empty() {
            Some(empty_state(
                AssetIcon::Layers,
                "No sets yet",
                "Click New, import sets from the clipboard, or save a fit's implants from the \
                 Implants tab's menu.",
                cx,
            ))
        } else if rows.is_empty() {
            Some(empty_state(
                AssetIcon::SearchX,
                "No sets match",
                "Clear the search to see them all.",
                cx,
            ))
        } else {
            None
        };
        let theme = cx.theme();
        v_flex()
            .w(px(width))
            .flex_none()
            .h_full()
            .gap_2()
            .p_3()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(caption("Implant sets", cx))
            .child(Input::new(&self.list_search).small().cleanable(true))
            .child(
                v_flex()
                    .id("implant-set-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(empty)
                    .children(rows),
            )
            .child(
                h_flex()
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
                        .disabled(sets.is_empty())
                        .on_click(cx.listener(|e, _, _, cx| e.export(cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        icon_button("set-delete", AssetIcon::Trash, "Delete (Delete)")
                            .disabled(!has)
                            .on_click(cx.listener(|e, _, window, cx| e.confirm_delete(window, cx))),
                    ),
            )
            .into_any_element()
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
            .min_w_0()
            .h_full()
            .gap_2()
            .p_3()
            .drag_over::<Dragged>(|style, dragged, _, cx| {
                if matches!(dragged.source, DragSource::Market) {
                    style.bg(cx.theme().drop_target.opacity(0.25))
                } else {
                    style
                }
            })
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
        let data = item_stats::hover_data(&self.loaded, cx);
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
                plural(unpriced, "implant", "implants")
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
            .drag_over::<Dragged>(move |style, dragged, _, cx| {
                let lands_here = matches!(dragged.source, DragSource::Market)
                    && dragged
                        .types()
                        .any(|t| implant_slot(&fills.catalog, t) == slot);
                if lands_here {
                    style.bg(cx.theme().drop_target)
                } else {
                    style
                }
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
                .child(div().flex_1().text_xs().text_color(muted).child(format!(
                    "Drop a slot {slot} implant here, or click to list them"
                )))
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
                    .child(div().child(catalog.name(id)))
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
            .tooltip(move |window, cx| side::item_tooltip(&tip, id, price, window, cx))
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

    // The implant tree.

    /// The tree's lines for the search and slot filter. A search shows everything it finds,
    /// folded or not.
    fn tree_rows(&self, query: &str) -> Vec<TreeRow> {
        let catalog = &self.loaded.catalog;
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let searching = !words.is_empty();
        let open = |fold: &Fold| searching || !self.folded.contains(fold);
        let name = |t: TypeId| catalog.get(t).map_or("", |t| t.name.as_str());
        let mut rows = Vec::new();
        for slot in self
            .by_slot
            .iter()
            .filter(|s| self.filter.shows_slot(s.slot))
        {
            let groups: Vec<(&str, Vec<TypeId>)> = slot
                .groups
                .iter()
                .map(|g| {
                    let hits = g
                        .implants
                        .iter()
                        .copied()
                        .filter(|&t| matches(&words, &[name(t), &g.name]))
                        .collect();
                    (g.name.as_str(), hits)
                })
                .filter(|(_, hits): &(&str, Vec<TypeId>)| !hits.is_empty())
                .collect();
            if groups.is_empty() {
                continue;
            }
            let fold = Fold::Slot(slot.slot);
            let slot_open = open(&fold);
            rows.push(TreeRow::Section {
                title: format!("Slot {}", slot.slot).into(),
                count: groups.iter().map(|(_, hits)| hits.len()).sum(),
                open: slot_open,
                fold,
            });
            if !slot_open {
                continue;
            }
            for (group, hits) in groups {
                let fold = Fold::Group(slot.slot, group.to_owned());
                let group_open = open(&fold);
                rows.push(TreeRow::Group {
                    title: group.to_owned().into(),
                    count: hits.len(),
                    open: group_open,
                    fold,
                });
                if group_open {
                    rows.extend(hits.into_iter().map(TreeRow::Item));
                }
            }
        }
        if self.filter.shows_sets() {
            let sets: Vec<&(SharedString, Rc<[TypeId]>)> = self
                .pirate_sets
                .iter()
                .filter(|(set, _)| matches(&words, &[set.as_ref(), "set"]))
                .collect();
            if !sets.is_empty() {
                let sets_open = open(&Fold::Sets);
                rows.push(TreeRow::Section {
                    fold: Fold::Sets,
                    title: "Implant sets".into(),
                    count: sets.len(),
                    open: sets_open,
                });
                if sets_open {
                    rows.extend(sets.into_iter().map(|(name, implants)| TreeRow::Set {
                        name: name.clone(),
                        implants: Rc::clone(implants),
                    }));
                }
            }
        }
        rows
    }

    fn tree_column(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).value().to_string();
        let rows = Rc::new(self.tree_rows(&query));
        let found: usize = rows
            .iter()
            .map(|row| match row {
                TreeRow::Section {
                    count,
                    fold: Fold::Slot(_),
                    ..
                } => *count,
                _ => 0,
            })
            .sum();
        let in_set: Rc<HashSet<TypeId>> = Rc::new(
            self.current(cx)
                .map_or_else(HashSet::new, |s| s.implants.iter().copied().collect()),
        );
        let chip = |id: String, label: String, filter: SlotFilter, cx: &mut Context<Self>| {
            Button::new(SharedString::from(id))
                .xsmall()
                .label(label)
                .selected(self.filter == filter)
                .on_click(cx.listener(move |e, _, _, cx| {
                    e.filter = filter;
                    cx.notify();
                }))
        };
        let mut chips = vec![chip("filter-all".into(), "All".into(), SlotFilter::All, cx)];
        for slot in SLOTS {
            chips.push(chip(
                format!("filter-{slot}"),
                slot.to_string(),
                SlotFilter::Slot(slot),
                cx,
            ));
        }
        chips.push(chip(
            "filter-sets".into(),
            "Sets".into(),
            SlotFilter::Sets,
            cx,
        ));
        let body = if rows.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .child(empty_state(
                    AssetIcon::SearchX,
                    "No implants match",
                    "Try fewer or shorter words, or another slot.",
                    cx,
                ))
                .child(
                    Button::new("tree-show-all")
                        .small()
                        .label("Show every implant")
                        .on_click(cx.listener(|e, _, window, cx| {
                            e.filter = SlotFilter::All;
                            e.search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            uniform_list(
                "implant-tree",
                rows.len(),
                cx.processor(move |e, range: Range<usize>, _, cx| {
                    rows[range]
                        .iter()
                        .map(|row| e.tree_row(row, &in_set, cx))
                        .collect()
                }),
            )
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        v_flex()
            .id("implant-tree-column")
            .w(px(TREE_W))
            .flex_none()
            .h_full()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            // An implant of the set dragged back here comes out of the set.
            .drag_over::<Dragged>(|style, dragged, _, cx| {
                if matches!(dragged.source, DragSource::Listed) {
                    style.bg(cx.theme().drop_target)
                } else {
                    style
                }
            })
            .child(
                h_flex()
                    .justify_between()
                    .child(caption("Implants", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(plural(found, "implant", "implants")),
                    ),
            )
            .child(Input::new(&self.search).small().cleanable(true))
            .child(h_flex().flex_wrap().gap_1().children(chips))
            .child(body)
            .child(div().text_xs().text_color(muted).child(
                "Drag an implant or a whole set onto the set, or double-click it. Drag one back \
                 here to remove it.",
            ))
            .into_any_element()
    }

    fn tree_row(
        &self,
        row: &TreeRow,
        in_set: &HashSet<TypeId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let heading = |fold: &Fold, title: &SharedString, count: usize, open: bool| {
            let fold = fold.clone();
            h_flex()
                .id(SharedString::from(format!("tree-{fold:?}")))
                .h(px(TREE_ROW))
                .pr_1()
                .gap_1()
                .rounded(px(4.))
                .hover(|s| s.bg(theme.list_hover))
                .child(
                    UiIcon::new(if open {
                        AssetIcon::ChevronDown
                    } else {
                        AssetIcon::ChevronRight
                    })
                    .size(px(14.))
                    .text_color(muted),
                )
                .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(count.to_string()),
                )
                .on_click(cx.listener(move |e, _, _, cx| e.toggle(fold.clone(), cx)))
        };
        match row {
            TreeRow::Section {
                fold,
                title,
                count,
                open,
            } => heading(fold, title, *count, *open)
                .font_weight(FontWeight::SEMIBOLD)
                .into_any_element(),
            TreeRow::Group {
                fold,
                title,
                count,
                open,
            } => heading(fold, title, *count, *open)
                .pl(px(INDENT))
                .into_any_element(),
            TreeRow::Item(id) => self.tree_item(*id, in_set.contains(id), cx),
            TreeRow::Set { name, implants } => self.tree_set(name, implants, cx),
        }
    }

    fn tree_item(&self, id: TypeId, in_set: bool, cx: &mut Context<Self>) -> AnyElement {
        let loaded = Rc::clone(&self.loaded);
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let price = self.prices(cx).get(id).filter(|&p| p > 0.0);
        let tip = Rc::clone(&loaded);
        let row = h_flex()
            .id(SharedString::from(format!("tree-item-{id}")))
            .h(px(TREE_ROW))
            .pl(px(INDENT * 2.))
            .pr_1()
            .gap_2()
            .rounded(px(4.))
            // Small enough for the longest hardwiring names to fit.
            .text_xs()
            .hover(|s| s.bg(theme.list_hover))
            .child(icon_box(catalog, &loaded.icons, Icon::Type(id), TREE_ICON))
            .child(div().flex_1().min_w_0().truncate().child(catalog.name(id)))
            .when(in_set, |row| {
                row.child(
                    UiIcon::new(AssetIcon::Check)
                        .size(px(12.))
                        .text_color(theme.primary),
                )
            })
            .tooltip(move |window, cx| side::item_tooltip(&tip, id, price, window, cx))
            .on_click(cx.listener(move |e, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    e.add_implants(&[id], cx);
                }
            }));
        match tree_drag(&loaded, &[id], None) {
            Some(dragged) => row
                .on_drag(dragged, |dragged, _, _, cx| {
                    cx.new(|_| DragPreview(dragged.clone()))
                })
                .into_any_element(),
            None => row.into_any_element(),
        }
    }

    fn tree_set(
        &self,
        name: &SharedString,
        implants: &Rc<[TypeId]>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loaded = Rc::clone(&self.loaded);
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let mut lines = vec![name.to_string(), String::new()];
        lines.extend(
            implants
                .iter()
                .map(|&t| format!("Slot {}: {}", implant_slot(catalog, t), catalog.name(t))),
        );
        lines.push(String::new());
        lines.push("Drag onto the set or double-click to add every piece.".to_owned());
        let add = Rc::clone(implants);
        let row = h_flex()
            .id(SharedString::from(format!("tree-set-{name}")))
            .h(px(TREE_ROW))
            .pl(px(INDENT))
            .pr_1()
            .gap_2()
            .rounded(px(4.))
            .text_xs()
            .hover(|s| s.bg(theme.list_hover))
            .children(
                implants
                    .first()
                    .map(|&t| icon_box(catalog, &loaded.icons, Icon::Type(t), TREE_ICON)),
            )
            .child(div().flex_1().min_w_0().truncate().child(name.clone()))
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(plural(implants.len(), "piece", "pieces")),
            )
            .tooltip(move |window, cx| side::text_tooltip(lines.clone(), window, cx))
            .on_click(cx.listener(move |e, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    e.add_implants(&add, cx);
                }
            }));
        match tree_drag(&loaded, implants, Some(name)) {
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
        self.narrow = window.viewport_size().width < px(NARROW_W);
        let sets = self.sets(cx).to_vec();
        // Another window may have removed or renamed the set this one was showing.
        if self
            .selected
            .as_ref()
            .is_none_or(|name| !sets.iter().any(|s| &s.name == name))
        {
            self.selected = sets.first().map(|s| s.name.clone());
            self.slot = None;
        }
        let selected = self
            .selected
            .as_ref()
            .and_then(|name| sets.iter().find(|s| &s.name == name));
        let list = self.list_column(&sets, cx);
        let middle = self.set_column(selected, !sets.is_empty(), cx);
        let tree = (!self.narrow || self.tree_open).then(|| self.tree_column(cx));
        let theme = cx.theme();
        h_flex()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|e, _: &DeletePicked, window, cx| e.delete_picked(window, cx)))
            .on_action(cx.listener(|e, _: &DuplicateSet, _, cx| e.duplicate(cx)))
            .on_action(cx.listener(|e, _: &RenameSet, window, cx| e.rename(window, cx)))
            .on_action(cx.listener(|e, _: &FocusSearch, window, cx| e.focus_search(window, cx)))
            // An implant of the set dragged anywhere but the set comes out of it.
            .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                if matches!(dragged.source, DragSource::Listed) {
                    let implants: Vec<TypeId> = dragged.types().collect();
                    e.remove_implants(&implants, cx);
                }
            }))
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(list)
            .child(middle)
            .children(tree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(query: &str) -> Vec<String> {
        query.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn search_words_may_match_name_or_group() {
        assert!(matches(&words(""), &["Anything"]));
        assert!(matches(
            &words("snake alpha"),
            &["High-grade Snake Alpha", "Snake set"]
        ));
        assert!(matches(
            &words("ARMOR noble"),
            &["'Noble' Repair", "Armor Implants"]
        ));
        assert!(!matches(
            &words("snake beta"),
            &["High-grade Snake Alpha", "Snake set"]
        ));
    }

    #[test]
    fn slot_filter_shows_its_part() {
        assert!(SlotFilter::All.shows_slot(3) && SlotFilter::All.shows_sets());
        assert!(SlotFilter::Slot(3).shows_slot(3) && !SlotFilter::Slot(3).shows_slot(4));
        assert!(!SlotFilter::Slot(3).shows_sets());
        assert!(SlotFilter::Sets.shows_sets() && !SlotFilter::Sets.shows_slot(1));
    }
}
