//! pyfa's Implant Set Editor (gui/setEditor.py) and the name prompt behind "Save as New Implant
//! Set". The editor is a window of three columns: the saved sets, the chosen set as its ten
//! slots, and every implant there is, by slot, to drag onto it.

use std::{collections::HashSet, ops::Range, path::PathBuf, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Div, Entity, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, WindowBounds,
    WindowOptions, div, img, prelude::FluentBuilder, px, size, uniform_list,
};

use super::{Gyfa, Loaded, item_stats, side, stats::format, widgets::icon_box};
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
    [RemoveImplant, DuplicateSet, RenameSet, FocusSearch]
);

const CONTEXT: &str = "ImplantSetEditor";

/// The editor window's starting size.
const EDITOR_W: f32 = 1100.;
const EDITOR_H: f32 = 700.;
const SETS_W: f32 = 240.;
const TREE_W: f32 = 430.;
/// Height of every row of the implant tree, as its list needs.
const TREE_ROW_H: f32 = 26.;
const TREE_ICON: f32 = 20.;
const SLOT_ROW_H: f32 = 46.;
const SLOT_ICON: f32 = 32.;
/// What a new set is called until it is renamed.
const NEW_SET: &str = "New Implant Set";

const NAME_MISSING: &str = "You must supply a name for the Implant Set!";
const NAME_TAKEN: &str = "Implant Set name already in use, please choose another.";

/// The editor's shortcuts. Typing in a text field keeps Delete for the text.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("delete", RemoveImplant, Some(CONTEXT)),
        KeyBinding::new("ctrl-d", DuplicateSet, Some(CONTEXT)),
        KeyBinding::new("f2", RenameSet, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
    ]);
}

impl Gyfa {
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
        let gyfa = cx.entity();
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
            app_id: Some("gyfa".into()),
            ..Default::default()
        };
        // Opened once this update is over: the new window draws right away, and drawing reads
        // this view, which gpui does not allow while it is being updated.
        cx.defer(move |cx| {
            let weak = gyfa.downgrade();
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("gyfa: Implant Set Editor");
                cx.new(|cx| ImplantSetEditor::new(&gyfa, loaded, select, window, cx))
            });
            match opened {
                Ok(_) => {
                    Gyfa::app_update(&weak, cx, |g, cx| {
                        g.with_prices_for(priced, cx, |_, _| {});
                    });
                }
                Err(e) => Gyfa::app_update(&weak, cx, |g, cx| {
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
        name_dialog(
            "New Implant Set",
            "Enter a name for your new Implant Set:",
            String::new(),
            check,
            move |name, window, cx| {
                Gyfa::app_update(&this, cx, |g, cx| {
                    let set = ImplantSet {
                        name: name.clone(),
                        implants: implants.clone(),
                    };
                    g.edit_implant_sets(cx, |sets| sets.push(set));
                    g.open_implant_set_editor(Some(name), window, cx);
                });
            },
            window,
            cx,
        );
    }
}

/// What is wrong with a name for a set, if anything. `keep` is the set being renamed, which
/// may keep its name.
fn name_problem(
    gyfa: &WeakEntity<Gyfa>,
    name: &str,
    keep: Option<&str>,
    cx: &App,
) -> Option<&'static str> {
    if name.is_empty() {
        return Some(NAME_MISSING);
    }
    let taken = gyfa.upgrade().is_some_and(|g| {
        g.read(cx)
            .saved
            .implant_sets
            .iter()
            .any(|s| s.name == name && Some(s.name.as_str()) != keep)
    });
    taken.then_some(NAME_TAKEN)
}

/// Ask for a set's name. OK and Enter both check it with `check`, which keeps the dialog open
/// with the problem shown, or close it and hand the name to `on_ok`.
fn name_dialog(
    title: &'static str,
    prompt: &'static str,
    initial: String,
    check: impl Fn(&str, &App) -> Option<&'static str> + 'static,
    on_ok: impl Fn(String, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Set name")
            .default_value(initial)
    });
    let error = cx.new(|_| None::<SharedString>);
    let (check, on_ok) = (Rc::new(check), Rc::new(on_ok));
    let field = input.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let message = error.read(cx).clone();
        let submit: super::widgets::Submit = {
            let (input, error, check, on_ok) = (
                input.clone(),
                error.clone(),
                Rc::clone(&check),
                Rc::clone(&on_ok),
            );
            Rc::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let name = input.read(cx).value().trim().to_owned();
                if let Some(problem) = check(&name, cx) {
                    error.update(cx, |e, cx| {
                        *e = Some(problem.into());
                        cx.notify();
                    });
                    window.refresh();
                    return;
                }
                window.close_dialog(cx);
                on_ok(name, window, cx);
            })
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
                            Button::new("implant-set-name-ok")
                                .primary()
                                .label("OK")
                                .on_click({
                                    let submit = Rc::clone(&submit);
                                    move |e: &ClickEvent, w: &mut Window, cx: &mut App| {
                                        submit(e, w, cx);
                                    }
                                }),
                        ),
                    ),
            )
    });
    // Once the dialog is up, so it does not take the focus back.
    window.defer(cx, move |window, cx| {
        field.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    });
}

/// Implants being dragged: from the tree onto the set, or out of the set.
#[derive(Debug, Clone)]
struct ImplantDrag {
    implants: Vec<TypeId>,
    /// The slots they go in, for highlighting the rows they would fill.
    slots: Vec<u8>,
    /// Dragged out of the set, so dropping it elsewhere takes it out.
    from_set: bool,
    label: SharedString,
    icon: Option<PathBuf>,
}

impl ImplantDrag {
    fn new(loaded: &Loaded, implants: Vec<TypeId>, label: Option<String>, from_set: bool) -> Self {
        let catalog = &loaded.catalog;
        let first = implants.first().copied();
        Self {
            slots: implants.iter().map(|&t| implant_slot(catalog, t)).collect(),
            from_set,
            label: label
                .or_else(|| first.map(|t| catalog.name(t)))
                .unwrap_or_default()
                .into(),
            icon: first.and_then(|t| loaded.icons.path(catalog, Icon::Type(t))),
            implants,
        }
    }
}

/// What follows the pointer while dragging: the icon and name.
struct DragView(ImplantDrag);

impl Render for DragView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .text_sm()
            .border_1()
            .border_color(theme.ring)
            .shadow_md()
            .opacity(0.9)
            .when_some(self.0.icon.clone(), |row, path| {
                row.child(img(path).size(px(22.)))
            })
            .child(self.0.label.clone())
    }
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

/// A small uppercase caption over a part of the window.
fn caption(text: &str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The editor's own state; the sets themselves live in [`Gyfa`]'s saved data.
pub struct ImplantSetEditor {
    gyfa: WeakEntity<Gyfa>,
    loaded: Rc<Loaded>,
    /// The set being edited, by name; names are unique.
    selected: Option<String>,
    /// The slot picked in the set, which Delete empties.
    slot: Option<u8>,
    set_search: Entity<InputState>,
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
    _subscriptions: Vec<Subscription>,
}

impl ImplantSetEditor {
    fn new(
        gyfa: &Entity<Gyfa>,
        loaded: Rc<Loaded>,
        select: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let set_search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter sets"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search implants"));
        let redraw =
            |_: &mut Self, _: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            };
        let subscriptions = vec![
            cx.observe(gyfa, |_, _, cx| cx.notify()),
            cx.subscribe(&search, redraw),
            cx.subscribe(&set_search, redraw),
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
            gyfa: gyfa.downgrade(),
            by_slot: implant_set::by_slot(catalog).into(),
            pirate_sets,
            loaded,
            selected: select,
            slot: None,
            set_search,
            search,
            filter: SlotFilter::All,
            folded: HashSet::new(),
            notice: None,
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn sets<'a>(&self, cx: &'a App) -> &'a [ImplantSet] {
        self.gyfa
            .upgrade()
            .map_or(&[], |g| g.read(cx).saved.implant_sets.as_slice())
    }

    fn prices(&self, cx: &App) -> Rc<Prices> {
        self.gyfa
            .upgrade()
            .map_or_else(Rc::default, |g| Rc::clone(&g.read(cx).prices))
    }

    fn current<'a>(&self, cx: &'a App) -> Option<&'a ImplantSet> {
        let name = self.selected.as_deref()?;
        self.sets(cx).iter().find(|s| s.name == name)
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Vec<ImplantSet>)) {
        Gyfa::app_update(&self.gyfa, cx, |g, cx| g.edit_implant_sets(cx, edit));
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
        Gyfa::app_update(&self.gyfa, cx, |g, cx| {
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
        let (gyfa, this) = (self.gyfa.clone(), cx.entity().downgrade());
        name_dialog(
            "New Implant Set",
            "Enter a name for your new Implant Set:",
            initial,
            move |name, cx| name_problem(&gyfa, name, None, cx),
            move |name, _, cx| {
                let _ = this.update(cx, |e, cx| {
                    let set = ImplantSet::new(name.clone());
                    e.edit(cx, |sets| sets.push(set));
                    e.notice = None;
                    e.select(Some(name), cx);
                });
            },
            window,
            cx,
        );
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        let (gyfa, this, keep) = (self.gyfa.clone(), cx.entity().downgrade(), old.clone());
        name_dialog(
            "Rename Implant Set",
            "Enter a new name for the Implant Set:",
            old.clone(),
            move |name, cx| name_problem(&gyfa, name, Some(&keep), cx),
            move |name, _, cx| {
                let old = old.clone();
                let _ = this.update(cx, |e, cx| {
                    let new = name.clone();
                    e.edit_set(&old, cx, |set| set.name = new);
                    e.select(Some(name), cx);
                });
            },
            window,
            cx,
        );
    }

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
        self.notice = Some(format!("Made {name}").into());
        self.select(Some(name), cx);
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let question = format!("Delete the implant set \"{name}\"? This cannot be undone.");
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, name) = (this.clone(), name.clone());
            dialog.title("Delete Implant Set").w(px(380.)).child(
                v_flex().gap_3().child(question.clone()).child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("delete-set-no")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("delete-set-yes")
                                .danger()
                                .label("Delete")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let name = name.clone();
                                    let _ = this.update(cx, |e, cx| e.delete(&name, cx));
                                }),
                        ),
                ),
            )
        });
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
        self.notice = Some(format!("Deleted {name}").into());
        self.select(next, cx);
    }

    /// Put implants into their slots in the chosen set, making a set first if there is none.
    fn add_implants(&mut self, implants: &[TypeId], cx: &mut Context<Self>) {
        let name = if let Some(name) = self.selected.clone() {
            name
        } else {
            let name = implant_set::free_name(self.sets(cx), NEW_SET);
            let set = ImplantSet::new(name.clone());
            self.edit(cx, |sets| sets.push(set));
            self.notice = Some(format!("Made {name} for them").into());
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

    /// Delete: empty the picked slot.
    fn remove_picked(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self.slot else { return };
        let implant = self
            .current(cx)
            .and_then(|set| set.in_slot(&self.loaded.catalog, slot));
        if let Some(implant) = implant {
            self.remove_implants(&[implant], cx);
        }
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.notice = Some("There is no text on the clipboard to import".into());
            cx.notify();
            return;
        };
        let imported = implant_set::import(&self.loaded.catalog, &text);
        let (count, errors) = (imported.sets.len(), imported.errors);
        let sets = plural(count, "set", "sets");
        self.notice = Some(match (count, errors) {
            (0, _) => "No implant sets found on the clipboard".into(),
            (_, 0) => format!("Imported {sets} from the clipboard").into(),
            _ => format!(
                "Imported {sets} from the clipboard; {} could not be read",
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
            Some(format!("Copied {} to the clipboard", plural(count, "set", "sets")).into());
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

    fn sets_pane(&self, sets: &[ImplantSet], cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.set_search.read(cx).value().to_lowercase();
        let shown: Vec<&ImplantSet> = sets
            .iter()
            .filter(|s| s.name.to_lowercase().contains(query.trim()))
            .collect();
        let has_selection = self.selected.is_some();
        let empty = if sets.is_empty() {
            Some("No sets yet. Click New to make one.")
        } else if shown.is_empty() {
            Some("No sets match the filter.")
        } else {
            None
        };
        let rows: Vec<AnyElement> = shown
            .iter()
            .enumerate()
            .map(|(i, set)| self.set_row(i, set, cx))
            .collect();
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        v_flex()
            .w(px(SETS_W))
            .flex_none()
            .h_full()
            .p_3()
            .gap_3()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(border)
            .child(
                h_flex()
                    .justify_between()
                    .child(caption("Implant sets", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(sets.len().to_string()),
                    ),
            )
            .child(
                Input::new(&self.set_search).small().cleanable(true).prefix(
                    UiIcon::new(AssetIcon::Search)
                        .size(px(14.))
                        .text_color(muted),
                ),
            )
            .child(
                v_flex()
                    .id("implant-set-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .when_some(empty, |list, text| {
                        list.child(div().p_2().text_xs().text_color(muted).child(text))
                    })
                    .children(rows),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div().flex_1().child(
                                    Button::new("set-new")
                                        .small()
                                        .w_full()
                                        .icon(AssetIcon::Plus)
                                        .label("New")
                                        .tooltip("Make a new set")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.create(window, cx);
                                        })),
                                ),
                            )
                            .child(
                                div().flex_1().child(
                                    Button::new("set-duplicate")
                                        .small()
                                        .w_full()
                                        .icon(AssetIcon::Copy)
                                        .label("Duplicate")
                                        .tooltip("Copy the set under a new name (Ctrl+D)")
                                        .disabled(!has_selection)
                                        .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div().flex_1().child(
                                    Button::new("set-rename")
                                        .small()
                                        .w_full()
                                        .icon(AssetIcon::Pencil)
                                        .label("Rename")
                                        .tooltip("Rename the set (F2)")
                                        .disabled(!has_selection)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.rename(window, cx);
                                        })),
                                ),
                            )
                            .child(
                                div().flex_1().child(
                                    Button::new("set-delete")
                                        .small()
                                        .w_full()
                                        .icon(AssetIcon::Trash)
                                        .label("Delete")
                                        .tooltip("Delete the set")
                                        .disabled(!has_selection)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm_delete(window, cx);
                                        })),
                                ),
                            ),
                    ),
            )
            .child(div().h_px().bg(border))
            .child(caption("Clipboard", cx))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        div().flex_1().child(
                            Button::new("sets-import")
                                .small()
                                .w_full()
                                .icon(AssetIcon::ClipboardPaste)
                                .label("Import")
                                .tooltip("Import implant sets from the clipboard, in pyfa's format")
                                .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                        ),
                    )
                    .child(
                        div().flex_1().child(
                            Button::new("sets-export")
                                .small()
                                .w_full()
                                .icon(AssetIcon::ClipboardCopy)
                                .label("Export")
                                .tooltip(
                                    "Copy every implant set to the clipboard, in pyfa's format",
                                )
                                .disabled(sets.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                        ),
                    ),
            )
            .when_some(self.notice.clone(), |pane, notice| {
                pane.child(div().text_xs().text_color(muted).child(notice))
            })
    }

    fn set_row(&self, i: usize, set: &ImplantSet, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let selected = self.selected.as_deref() == Some(set.name.as_str());
        let name = set.name.clone();
        let drop_name = set.name.clone();
        h_flex()
            .id(SharedString::from(format!("implant-set-{i}")))
            .h(px(30.))
            .px_2()
            .gap_2()
            .rounded(px(6.))
            .when(selected, |row| {
                row.bg(theme.secondary).font_weight(FontWeight::MEDIUM)
            })
            .when(!selected, |row| row.hover(|s| s.bg(theme.list_hover)))
            .child(div().flex_1().min_w_0().truncate().child(set.name.clone()))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{}/{}", set.implants.len(), SLOTS.end())),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.notice = None;
                this.select(Some(name.clone()), cx);
                window.focus(&this.focus, cx);
                if event.click_count() >= 2 {
                    this.rename(window, cx);
                }
            }))
            // Dropping implants on a set in the list adds them to that set.
            .drag_over::<ImplantDrag>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .on_drop(cx.listener(move |this, drag: &ImplantDrag, _, cx| {
                cx.stop_propagation();
                this.add_to(&drop_name, &drag.implants, cx);
                this.select(Some(drop_name.clone()), cx);
            }))
            .into_any_element()
    }

    // The chosen set.

    fn set_pane(
        &self,
        set: Option<&ImplantSet>,
        any_sets: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pane = v_flex()
            .id("implant-set")
            .flex_1()
            .min_w_0()
            .h_full()
            .px_5()
            .py_4()
            .gap_3()
            .drag_over::<ImplantDrag>(|style, drag, _, cx| {
                if drag.from_set {
                    style
                } else {
                    style.bg(cx.theme().drop_target.opacity(0.25))
                }
            })
            .on_drop(cx.listener(|this, drag: &ImplantDrag, _, cx| {
                cx.stop_propagation();
                if !drag.from_set {
                    this.add_implants(&drag.implants, cx);
                }
            }));
        match set {
            Some(set) => pane.children(self.set_body(set, cx)),
            None => pane.child(Self::no_set(any_sets, cx)),
        }
    }

    fn no_set(any_sets: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (title, text) = if any_sets {
            (
                "No set picked",
                "Pick a set on the left, or make a new one.",
            )
        } else {
            (
                "No implant sets yet",
                "Make a set with New, or import sets from the clipboard. Then drag implants onto \
                 it from the list on the right, or double-click them.",
            )
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                UiIcon::new(AssetIcon::Layers)
                    .size(px(32.))
                    .text_color(theme.muted_foreground),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .max_w(px(320.))
                    .text_xs()
                    .text_center()
                    .text_color(theme.muted_foreground)
                    .child(text),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("empty-new")
                            .small()
                            .primary()
                            .icon(AssetIcon::Plus)
                            .label("New set")
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    )
                    .when(!any_sets, |row| {
                        row.child(
                            Button::new("empty-import")
                                .small()
                                .icon(AssetIcon::ClipboardPaste)
                                .label("Import from clipboard")
                                .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                        )
                    }),
            )
            .into_any_element()
    }

    fn set_body(&self, set: &ImplantSet, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let prices = self.prices(cx);
        let data = item_stats::hover_data(&self.loaded, cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let known: Vec<f64> = set
            .implants
            .iter()
            .filter_map(|&t| prices.get(t).filter(|&p| p > 0.0))
            .collect();
        let unpriced = set.implants.len() - known.len();
        let total: f64 = known.iter().sum();
        let header = h_flex()
            .items_start()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(caption("Implant set", cx))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(set.name.clone()),
                    )
                    .child(div().text_xs().text_color(muted).child(format!(
                        "{} of {} slots filled",
                        set.implants.len(),
                        SLOTS.end()
                    ))),
            )
            .child(
                v_flex()
                    .flex_none()
                    .items_end()
                    .gap_0p5()
                    .child(caption("Total price", cx))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} ISK", format::currency(total))),
                    )
                    .when(unpriced > 0, |col| {
                        col.child(div().text_xs().text_color(muted).child(format!(
                            "{} without a price",
                            plural(unpriced, "implant", "implants")
                        )))
                    }),
            );
        let hint = div().text_xs().text_color(muted).child(if set.implants.is_empty() {
            "This set is empty. Drag implants here from the list on the right, or double-click \
             them. Each one goes into its own slot."
        } else {
            "Drop an implant anywhere here to put it in its slot. Drag one out, press Delete or \
             click the x to take it out."
        });
        let rows: Vec<AnyElement> = SLOTS
            .map(|slot| self.slot_row(set, slot, &prices, data.as_deref(), cx))
            .collect();
        vec![
            header.into_any_element(),
            hint.into_any_element(),
            v_flex()
                .id("implant-slots")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap_0p5()
                .children(rows)
                .into_any_element(),
        ]
    }

    fn slot_row(
        &self,
        set: &ImplantSet,
        slot: u8,
        prices: &Prices,
        data: Option<&ItemData>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let picked = self.slot == Some(slot);
        let row = h_flex()
            .id(SharedString::from(format!("slot-{slot}")))
            .min_h(px(SLOT_ROW_H))
            .px_2()
            .py_1()
            .gap_3()
            .rounded(px(6.))
            .when(picked, |row| row.bg(theme.list_active))
            .when(!picked, |row| row.hover(|s| s.bg(theme.list_hover)))
            .drag_over::<ImplantDrag>(move |style, drag, _, cx| {
                if !drag.from_set && drag.slots.contains(&slot) {
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
                        .rounded(px(6.))
                        .border_1()
                        .border_dashed()
                        .border_color(theme.border)
                        .child(UiIcon::new(AssetIcon::Plus).size(px(14.)).text_color(muted)),
                )
                .child(div().flex_1().text_xs().text_color(muted).child(format!(
                    "Drop a slot {slot} implant here, or click to list them"
                )))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.slot = Some(slot);
                    this.filter = SlotFilter::Slot(slot);
                    window.focus(&this.focus, cx);
                    cx.notify();
                }))
                .into_any_element();
        };
        let price = prices.get(id).filter(|&p| p > 0.0);
        let bonus = data
            .and_then(|data| item_stats::description_text(data, id))
            .and_then(|text| implant_set::main_bonus(&text));
        let loaded = Rc::clone(&self.loaded);
        let drag = ImplantDrag::new(&self.loaded, vec![id], None, true);
        row.child(icon_box(
            catalog,
            &self.loaded.icons,
            Icon::Type(id),
            SLOT_ICON,
        ))
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
                .icon(AssetIcon::X)
                .tooltip("Take it out of the set (Delete)")
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.remove_implants(&[id], cx);
                })),
        )
        .tooltip(move |window, cx| side::item_tooltip(&loaded, id, price, window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.slot = Some(slot);
            window.focus(&this.focus, cx);
            cx.notify();
        }))
        .on_drag(drag, |drag, _, _, cx| cx.new(|_| DragView(drag.clone())))
        .into_any_element()
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

    fn tree_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let in_set: Rc<HashSet<TypeId>> = Rc::new(
            self.current(cx)
                .map_or_else(HashSet::new, |s| s.implants.iter().copied().collect()),
        );
        let chip =
            |id: String, label: String, on: bool, filter: SlotFilter, cx: &mut Context<Self>| {
                Button::new(SharedString::from(id))
                    .xsmall()
                    .label(label)
                    .selected(on)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter = filter;
                        cx.notify();
                    }))
            };
        let mut chips = vec![chip(
            "filter-all".into(),
            "All".into(),
            self.filter == SlotFilter::All,
            SlotFilter::All,
            cx,
        )];
        for slot in SLOTS {
            chips.push(chip(
                format!("filter-{slot}"),
                slot.to_string(),
                self.filter == SlotFilter::Slot(slot),
                SlotFilter::Slot(slot),
                cx,
            ));
        }
        chips.push(chip(
            "filter-sets".into(),
            "Sets".into(),
            self.filter == SlotFilter::Sets,
            SlotFilter::Sets,
            cx,
        ));
        let body = if rows.is_empty() {
            let text = if query.trim().is_empty() {
                "Nothing here.".to_owned()
            } else {
                format!("No implants match \"{}\".", query.trim())
            };
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(div().text_xs().text_color(muted).child(text))
                .child(
                    Button::new("tree-show-all")
                        .small()
                        .label("Show every implant")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.filter = SlotFilter::All;
                            this.search
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            uniform_list(
                "implant-tree",
                rows.len(),
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    rows[range]
                        .iter()
                        .map(|row| this.tree_row(row, &in_set, cx))
                        .collect()
                }),
            )
            .flex_1()
            .min_h_0()
            .into_any_element()
        };
        let theme = cx.theme();
        v_flex()
            .id("implant-tree-pane")
            .w(px(TREE_W))
            .flex_none()
            .h_full()
            .p_3()
            .gap_2()
            .bg(theme.sidebar)
            .border_l_1()
            .border_color(theme.border)
            // Dropping an implant from the set here takes it out.
            .drag_over::<ImplantDrag>(|style, drag, _, cx| {
                if drag.from_set {
                    style.bg(cx.theme().danger.opacity(0.08))
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
            .child(
                Input::new(&self.search).small().cleanable(true).prefix(
                    UiIcon::new(AssetIcon::Search)
                        .size(px(14.))
                        .text_color(muted),
                ),
            )
            .child(h_flex().flex_wrap().gap_1().children(chips))
            .child(body)
            .child(div().text_xs().text_color(muted).child(
                "Drag onto the set or double-click to add. Drag a whole set to add every piece.",
            ))
    }

    fn tree_row(
        &self,
        row: &TreeRow,
        in_set: &HashSet<TypeId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let heading = |id: String, fold: &Fold, title: &SharedString, count: usize, open: bool| {
            let fold = fold.clone();
            h_flex()
                .id(SharedString::from(id))
                .h(px(TREE_ROW_H))
                .px_1()
                .gap_1p5()
                .rounded(px(4.))
                .cursor_pointer()
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
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(fold.clone(), cx)))
        };
        match row {
            TreeRow::Section {
                fold,
                title,
                count,
                open,
            } => heading(format!("tree-{fold:?}"), fold, title, *count, *open)
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .into_any_element(),
            TreeRow::Group {
                fold,
                title,
                count,
                open,
            } => heading(format!("tree-{fold:?}"), fold, title, *count, *open)
                .pl(px(14.))
                .text_xs()
                .text_color(muted)
                .into_any_element(),
            TreeRow::Item(id) => self.tree_item(*id, in_set.contains(id), cx),
            TreeRow::Set { name, implants } => self.tree_set(name, implants, cx),
        }
    }

    fn tree_item(&self, id: TypeId, in_set: bool, cx: &mut Context<Self>) -> AnyElement {
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let price = self.prices(cx).get(id).filter(|&p| p > 0.0);
        let loaded = Rc::clone(&self.loaded);
        let drag = ImplantDrag::new(&self.loaded, vec![id], None, false);
        h_flex()
            .id(SharedString::from(format!("tree-item-{id}")))
            .h(px(TREE_ROW_H))
            .pl(px(28.))
            .pr_1()
            .gap_2()
            .rounded(px(4.))
            .text_xs()
            .hover(|s| s.bg(theme.list_hover))
            .child(icon_box(
                catalog,
                &self.loaded.icons,
                Icon::Type(id),
                TREE_ICON,
            ))
            .child(div().flex_1().min_w_0().truncate().child(catalog.name(id)))
            .when(in_set, |row| {
                row.child(
                    UiIcon::new(AssetIcon::Check)
                        .size(px(12.))
                        .text_color(theme.primary),
                )
            })
            .tooltip(move |window, cx| side::item_tooltip(&loaded, id, price, window, cx))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.add_implants(&[id], cx);
                }
            }))
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| DragView(drag.clone())))
            .into_any_element()
    }

    fn tree_set(
        &self,
        name: &SharedString,
        implants: &Rc<[TypeId]>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let first = implants.first().copied();
        let mut lines = vec![name.to_string(), String::new()];
        lines.extend(
            implants
                .iter()
                .map(|&t| format!("Slot {}: {}", implant_slot(catalog, t), catalog.name(t))),
        );
        lines.push(String::new());
        lines.push("Drag onto the set or double-click to add every piece.".to_owned());
        let drag = ImplantDrag::new(
            &self.loaded,
            implants.to_vec(),
            Some(name.to_string()),
            false,
        );
        let add = Rc::clone(implants);
        h_flex()
            .id(SharedString::from(format!("tree-set-{name}")))
            .h(px(TREE_ROW_H))
            .pl(px(28.))
            .pr_1()
            .gap_2()
            .rounded(px(4.))
            .text_xs()
            .hover(|s| s.bg(theme.list_hover))
            .children(
                first.map(|t| icon_box(catalog, &self.loaded.icons, Icon::Type(t), TREE_ICON)),
            )
            .child(div().flex_1().min_w_0().truncate().child(name.clone()))
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(plural(implants.len(), "piece", "pieces")),
            )
            .tooltip(move |window, cx| side::text_tooltip(lines.clone(), window, cx))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if event.click_count() >= 2 {
                    this.add_implants(&add, cx);
                }
            }))
            .on_drag(drag, |drag, _, _, cx| cx.new(|_| DragView(drag.clone())))
            .into_any_element()
    }
}

impl Render for ImplantSetEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let theme = cx.theme();
        h_flex()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &RemoveImplant, _, cx| this.remove_picked(cx)))
            .on_action(cx.listener(|this, _: &DuplicateSet, _, cx| this.duplicate(cx)))
            .on_action(cx.listener(|this, _: &RenameSet, window, cx| this.rename(window, cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.focus_search(window, cx);
            }))
            // An implant dragged out of the set and dropped anywhere else is taken out.
            .on_drop(cx.listener(|this, drag: &ImplantDrag, _, cx| {
                if drag.from_set {
                    this.remove_implants(&drag.implants, cx);
                }
            }))
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(self.sets_pane(&sets, cx))
            .child(self.set_pane(selected, !sets.is_empty(), cx))
            .child(self.tree_pane(cx))
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
