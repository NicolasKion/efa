//! pyfa's Implant Set Editor and the name prompt behind "Save as New Implant Set".

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, WeakEntity, Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder, px,
    size,
};

use super::{Gyfa, Loaded, widgets::icon_box};
use crate::{
    data::{Kind, TypeId},
    fit::implant_slot,
    icons::Icon,
    implant_set::{self, ImplantSet},
};

/// Most search hits the editor lists, so a one-letter query stays cheap to draw.
const SEARCH_LIMIT: usize = 200;
const ROW_HEIGHT: f32 = 26.;
const PANE_HEIGHT: f32 = 320.;
/// The editor window's starting size.
const EDITOR_W: f32 = 1000.;
const EDITOR_H: f32 = 520.;

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
        // Worked out here: the editor is built while this view is being updated, so it cannot
        // read the view itself yet.
        let select = select.or_else(|| self.saved.implant_sets.first().map(|s| s.name.clone()));
        // A window of its own, like pyfa's editors, so it can sit beside the fit.
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(EDITOR_W), px(EDITOR_H)), cx)),
            app_id: Some("gyfa".into()),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            window.set_window_title("gyfa: Implant Set Editor");
            cx.new(|cx| ImplantSetEditor::new(&gyfa, loaded, select, window, cx))
        });
        if let Err(e) = opened {
            self.status = Some(format!("Could not open the implant set editor: {e}").into());
            cx.notify();
        }
    }

    /// Ask for a name, save the fit's applied implants under it, then show the set in the
    /// editor.
    pub(super) fn save_implant_set_dialog(
        implants: Vec<TypeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Set name"));
        let error = cx.new(|_| None::<SharedString>);
        let this = cx.entity().downgrade();
        let implants = Rc::new(implants);
        window.open_dialog(cx, move |dialog, _, cx| {
            let (input, error, this, implants) = (
                input.clone(),
                error.clone(),
                this.clone(),
                Rc::clone(&implants),
            );
            let message = error.read(cx).clone();
            dialog.title("New Implant Set").w(px(360.)).child(
                v_flex()
                    .gap_3()
                    .child("Enter a name for your new Implant Set:")
                    .child(Input::new(&input))
                    .when_some(message, |col, message| {
                        col.child(div().text_sm().text_color(cx.theme().danger).child(message))
                    })
                    .child(
                        h_flex().justify_end().child(
                            Button::new("implant-set-name-ok")
                                .primary()
                                .label("OK")
                                .on_click(move |_, window, cx| {
                                    let name = input.read(cx).value().trim().to_owned();
                                    if name.is_empty() {
                                        window.close_dialog(cx);
                                        return;
                                    }
                                    let taken = this.upgrade().is_some_and(|g| {
                                        g.read(cx).saved.implant_sets.iter().any(|s| s.name == name)
                                    });
                                    if taken {
                                        error.update(cx, |e, cx| {
                                            *e = Some(NAME_TAKEN.into());
                                            cx.notify();
                                        });
                                        window.refresh();
                                        return;
                                    }
                                    window.close_dialog(cx);
                                    Gyfa::app_update(&this, cx, |g, cx| {
                                        let set = ImplantSet {
                                            name: name.clone(),
                                            implants: implants.to_vec(),
                                        };
                                        g.edit_implant_sets(cx, |sets| sets.push(set));
                                        g.open_implant_set_editor(Some(name), window, cx);
                                    });
                                }),
                        ),
                    ),
            )
        });
    }
}

const NAME_MISSING: &str = "You must supply a name for the Implant Set!";
const NAME_TAKEN: &str = "Implant Set name already in use, please choose another.";

/// The editor's own state; the sets themselves live in [`Gyfa`]'s saved data.
pub struct ImplantSetEditor {
    gyfa: WeakEntity<Gyfa>,
    loaded: Rc<Loaded>,
    /// The set being edited, by name; names are unique.
    selected: Option<String>,
    name: Entity<InputState>,
    search: Entity<InputState>,
    notice: Option<SharedString>,
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
        let selected = select;
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Set name")
                .default_value(selected.clone().unwrap_or_default())
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search implants"));
        let subscriptions = vec![
            cx.observe(gyfa, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            gyfa: gyfa.downgrade(),
            loaded,
            selected,
            name,
            search,
            notice: None,
            _subscriptions: subscriptions,
        }
    }

    fn sets<'a>(&self, cx: &'a App) -> &'a [ImplantSet] {
        self.gyfa
            .upgrade()
            .map_or(&[], |g| g.read(cx).saved.implant_sets.as_slice())
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Vec<ImplantSet>)) {
        Gyfa::app_update(&self.gyfa, cx, |g, cx| g.edit_implant_sets(cx, edit));
    }

    fn select(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let text = name.clone().unwrap_or_default();
        self.name
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.selected = name;
        cx.notify();
    }

    /// The name typed in, if it can name a new set.
    fn new_name(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let name = self.name.read(cx).value().trim().to_owned();
        let problem = if name.is_empty() {
            Some(NAME_MISSING)
        } else if self.sets(cx).iter().any(|s| s.name == name) {
            Some(NAME_TAKEN)
        } else {
            None
        };
        self.notice = problem.map(SharedString::from);
        cx.notify();
        problem.is_none().then_some(name)
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.new_name(cx) else {
            return;
        };
        let set = ImplantSet::new(name.clone());
        self.edit(cx, |sets| sets.push(set));
        self.select(Some(name), window, cx);
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        let Some(name) = self.new_name(cx) else {
            return;
        };
        let new = name.clone();
        self.edit(cx, |sets| {
            if let Some(set) = sets.iter_mut().find(|s| s.name == old) {
                set.name = new;
            }
        });
        self.select(Some(name), window, cx);
    }

    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        let Some(name) = self.new_name(cx) else {
            return;
        };
        let new = name.clone();
        self.edit(cx, |sets| {
            if let Some(set) = sets.iter().find(|s| s.name == old) {
                let copy = ImplantSet {
                    name: new,
                    implants: set.implants.clone(),
                };
                sets.push(copy);
            }
        });
        self.select(Some(name), window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        self.edit(cx, |sets| sets.retain(|s| s.name != old));
        let next = self.sets(cx).first().map(|s| s.name.clone());
        self.notice = None;
        self.select(next, window, cx);
    }

    fn add_implant(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        let Some(selected) = self.selected.clone() else {
            self.notice = Some("Create a set first".into());
            cx.notify();
            return;
        };
        let loaded = Rc::clone(&self.loaded);
        self.edit(cx, |sets| {
            if let Some(set) = sets.iter_mut().find(|s| s.name == selected) {
                set.add(&loaded.catalog, type_id);
            }
        });
    }

    fn remove_implant(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        let Some(selected) = self.selected.clone() else {
            return;
        };
        self.edit(cx, |sets| {
            if let Some(set) = sets.iter_mut().find(|s| s.name == selected) {
                set.implants.retain(|&t| t != type_id);
            }
        });
    }

    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.notice = Some("Could not import from clipboard".into());
            cx.notify();
            return;
        };
        let imported = implant_set::import(&self.loaded.catalog, &text);
        let (count, errors) = (imported.sets.len(), imported.errors);
        self.notice = Some(match (count, errors) {
            (0, _) => "No patterns found for import".into(),
            (_, 0) => "Patterns successfully imported from clipboard".into(),
            _ => format!("{count} sets imported from clipboard; {errors} errors").into(),
        });
        let loaded = Rc::clone(&self.loaded);
        self.edit(cx, |sets| {
            implant_set::merge(sets, imported.sets, &loaded.catalog);
        });
        if self.selected.is_none() {
            let first = self.sets(cx).first().map(|s| s.name.clone());
            self.select(first, window, cx);
        }
        cx.notify();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let text = implant_set::export(&self.loaded.catalog, self.sets(cx));
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice = Some("Sets exported to clipboard".into());
        cx.notify();
    }

    fn implant_row(
        &self,
        id: SharedString,
        type_id: TypeId,
        trailing: Option<AnyElement>,
        cx: &Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let catalog = &self.loaded.catalog;
        let slot = catalog.get(type_id).and_then(|t| t.implant_slot);
        h_flex()
            .id(id)
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .hover(|s| s.bg(cx.theme().table_hover))
            .child(
                div()
                    .w(px(18.))
                    .text_color(cx.theme().muted_foreground)
                    .child(slot.map_or_default(|s| s.to_string())),
            )
            .child(icon_box(
                catalog,
                &self.loaded.icons,
                Icon::Type(type_id),
                20.,
            ))
            .child(div().flex_1().truncate().child(catalog.name(type_id)))
            .children(trailing)
    }

    fn sets_pane(&self, sets: &[ImplantSet], cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .id("implant-sets")
            .w(px(200.))
            .h(px(PANE_HEIGHT))
            .overflow_y_scroll()
            .border_1()
            .border_color(theme.border)
            .rounded(px(4.))
            .when(sets.is_empty(), |pane| {
                pane.child(
                    div()
                        .p_2()
                        .text_color(theme.muted_foreground)
                        .child("No implant sets yet. Type a name below and click New."),
                )
            })
            .children(sets.iter().enumerate().map(|(i, set)| {
                let selected = self.selected.as_deref() == Some(set.name.as_str());
                let name = set.name.clone();
                div()
                    .id(SharedString::from(format!("implant-set-{i}")))
                    .px_2()
                    .py_1()
                    .truncate()
                    .when(selected, |row| row.bg(theme.table_active))
                    .hover(|s| s.bg(theme.table_hover))
                    .child(set.name.clone())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.notice = None;
                        this.select(Some(name.clone()), window, cx);
                    }))
            }))
    }

    fn available_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().to_string();
        let catalog = &self.loaded.catalog;
        let hits: Vec<TypeId> = catalog
            .search(&query, SEARCH_LIMIT)
            .into_iter()
            .filter(|&t| catalog.get(t).is_some_and(|t| t.kind == Kind::Implant))
            .collect();
        let theme = cx.theme();
        let hint = if query.trim().is_empty() {
            Some("Search for implants, then click one to add it to the set.")
        } else if hits.is_empty() {
            Some("No implants match.")
        } else {
            None
        };
        let rows: Vec<AnyElement> = hits
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                self.implant_row(SharedString::from(format!("available-{i}")), t, None, cx)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.add_implant(t, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .flex_1()
            .gap_1()
            .child(Input::new(&self.search).small())
            .child(
                v_flex()
                    .id("available-implants")
                    .h(px(PANE_HEIGHT - 32.))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(px(4.))
                    .when_some(hint, |pane, hint| {
                        pane.child(div().p_2().text_color(theme.muted_foreground).child(hint))
                    })
                    .children(rows),
            )
    }

    fn set_pane(&self, set: Option<&ImplantSet>, cx: &mut Context<Self>) -> impl IntoElement {
        let mut implants: Vec<TypeId> = set.map_or_default(|s| s.implants.clone());
        implants.sort_by_key(|&t| implant_slot(&self.loaded.catalog, t));
        let theme = cx.theme();
        let rows: Vec<AnyElement> = implants
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let remove = Button::new(SharedString::from(format!("set-remove-{i}")))
                    .ghost()
                    .xsmall()
                    .label("×")
                    .tooltip("Remove from set")
                    .on_click(cx.listener(move |this, _, _, cx| this.remove_implant(t, cx)));
                self.implant_row(
                    SharedString::from(format!("set-implant-{i}")),
                    t,
                    Some(remove.into_any_element()),
                    cx,
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    if event.click_count() >= 2 {
                        this.remove_implant(t, cx);
                    }
                }))
                .into_any_element()
            })
            .collect();
        let hint = match set {
            None => Some("Pick or create a set."),
            Some(set) if set.implants.is_empty() => Some("This set has no implants yet."),
            Some(_) => None,
        };
        v_flex()
            .id("set-implants")
            .flex_1()
            .h(px(PANE_HEIGHT))
            .overflow_y_scroll()
            .border_1()
            .border_color(theme.border)
            .rounded(px(4.))
            .when_some(hint, |pane, hint| {
                pane.child(div().p_2().text_color(theme.muted_foreground).child(hint))
            })
            .children(rows)
    }
}

impl Render for ImplantSetEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sets = self.sets(cx).to_vec();
        // Another window may have removed the set this one was showing.
        if self
            .selected
            .as_ref()
            .is_some_and(|name| !sets.iter().any(|s| &s.name == name))
        {
            self.selected = None;
        }
        let selected = self
            .selected
            .as_ref()
            .and_then(|name| sets.iter().find(|s| &s.name == name));
        let has_selection = selected.is_some();
        let notice = self.notice.clone();
        v_flex()
            .size_full()
            .p_3()
            .gap_3()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(self.sets_pane(&sets, cx))
                    .child(self.available_pane(cx))
                    .child(self.set_pane(selected, cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(240.)).child(Input::new(&self.name).small()))
                    .child(
                        Button::new("set-new")
                            .small()
                            .label("New")
                            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                    )
                    .child(
                        Button::new("set-rename")
                            .small()
                            .label("Rename")
                            .disabled(!has_selection)
                            .on_click(cx.listener(|this, _, window, cx| this.rename(window, cx))),
                    )
                    .child(
                        Button::new("set-copy")
                            .small()
                            .label("Copy")
                            .disabled(!has_selection)
                            .on_click(cx.listener(|this, _, window, cx| this.copy(window, cx))),
                    )
                    .child(
                        Button::new("set-delete")
                            .small()
                            .danger()
                            .label("Delete")
                            .disabled(!has_selection)
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("sets-import")
                            .small()
                            .label("Import")
                            .tooltip("Import implant sets from clipboard")
                            .on_click(cx.listener(|this, _, window, cx| this.import(window, cx))),
                    )
                    .child(
                        Button::new("sets-export")
                            .small()
                            .label("Export")
                            .tooltip("Export implant sets to clipboard")
                            .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                    )
                    .when_some(notice, |row, notice| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(notice),
                        )
                    }),
            )
    }
}
