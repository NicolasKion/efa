//! pyfa's Damage Pattern Editor (gui/patternEditor.py) and Target Profile Editor
//! (gui/targetProfileEditor.py), each in a window of its own.
//!
//! Both are the same editor over a different [`Kind`]: a list of entries, New, Rename, Copy and
//! Delete with pyfa's name prompts, value fields that save as they are typed, and clipboard
//! import and export. The entries live in the [`Hub`], so every window sees a change at once;
//! fits that use an entry follow it when it is renamed, edited or deleted, as pyfa's do. efa
//! also lists the builtins, read-only, so one can be copied as a starting point.

mod kinds;

use std::{marker::PhantomData, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, ClipboardItem, Context, Entity, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px,
};

use super::{
    Loaded,
    dialogs::{Confirm, Prompt, confirm, prompt_text},
    editor_kit::{
        CONTEXT, Delete, Duplicate, FocusSearch, Rename, Sidebar, icon_button, name_problem,
        next_after_delete,
    },
    hub::{Hub, HubLoading},
    widgets::{TipExt, caption, empty_line, icon_box, list_row, redraw_on_change},
};
use crate::{icons::Icon, profile, store::Saved};
use kinds::{DamagePatterns, Kind, TargetProfiles};

/// Below this width the list narrows and the fields' notes give way first.
const NARROW_W: f32 = 640.;

/// Which entry the editor shows.
#[derive(Debug, Clone, PartialEq)]
enum Selection {
    User(String),
    /// By index into the builtins.
    Builtin(usize),
}

/// What a name prompt is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Naming {
    New,
    Rename,
    Copy,
}

/// Open the Damage Pattern Editor, or bring it up showing `select` if it is open.
pub fn open_damage_pattern_editor(hub: Entity<Hub>, select: Option<String>, cx: &mut App) {
    open::<DamagePatterns>(hub, select, cx);
}

/// Open the Target Profile Editor, or bring it up showing `select` if it is open. This is what
/// the graphs' "Edit Target Profile" calls. It reads the hub, so call it from a click handler or
/// another place where the hub is not being updated.
pub fn open_target_profile_editor(hub: Entity<Hub>, select: Option<String>, cx: &mut App) {
    open::<TargetProfiles>(hub, select, cx);
}

fn open<K: Kind>(hub: Entity<Hub>, select: Option<String>, cx: &mut App) {
    let shown = select.clone();
    crate::ui::chrome::open_single(
        K::TITLE,
        (K::WIDTH, 520.),
        cx,
        move |e: &mut Editor<K>, window, cx| {
            if let Some(name) = &shown {
                e.select_name(name, window, cx);
            }
        },
        move |window, cx| Editor::<K>::new(hub, select.as_deref(), window, cx),
    )
    .detach();
}

pub struct Editor<K: Kind> {
    hub: Entity<Hub>,
    loaded: Option<Rc<Loaded>>,
    selected: Option<Selection>,
    inputs: Vec<Entity<InputState>>,
    /// Narrows the list by name.
    search: Entity<InputState>,
    notice: Option<SharedString>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _kind: PhantomData<K>,
}

impl<K: Kind> Editor<K> {
    fn new(
        hub: Entity<Hub>,
        select: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inputs: Vec<Entity<InputState>> = K::fields()
            .iter()
            .map(|_| cx.new(|cx| InputState::new(window, cx)))
            .collect();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let mut subscriptions = vec![
            cx.observe_in(&hub, window, |this, hub, window, cx| {
                if this.loaded.is_none()
                    && let HubLoading::Ready(loaded) = &hub.read(cx).loading
                {
                    this.loaded = Some(Rc::clone(loaded));
                }
                // Another window may have removed or renamed the shown entry.
                if matches!(&this.selected, Some(Selection::User(_)))
                    && this.selected_item(cx).is_none()
                {
                    let first = this
                        .user(cx)
                        .first()
                        .map(|item| Selection::User(K::name(item).to_owned()));
                    this.select(first, window, cx);
                }
                cx.notify();
            }),
            redraw_on_change(cx, &search),
        ];
        subscriptions.extend(inputs.iter().map(|input| {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.commit(cx);
                }
            })
        }));
        let loaded = match &hub.read(cx).loading {
            HubLoading::Ready(loaded) => Some(Rc::clone(loaded)),
            HubLoading::Pending | HubLoading::Failed(_) => None,
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            hub,
            loaded,
            selected: None,
            inputs,
            search,
            notice: None,
            focus,
            _subscriptions: subscriptions,
            _kind: PhantomData,
        };
        let first = this
            .user(cx)
            .first()
            .map(|item| Selection::User(K::name(item).to_owned()));
        let selection = select.and_then(|name| this.find(name, cx)).or(first);
        this.select(selection, window, cx);
        this
    }

    /// The listed user entries, sorted by name as pyfa's editor lists them.
    fn user<'a>(&self, cx: &'a App) -> Vec<&'a K::Item> {
        let mut user: Vec<&K::Item> = K::user(&self.hub.read(cx).saved)
            .iter()
            .filter(|item| K::listed(item))
            .collect();
        user.sort_by(|a, b| K::name(a).cmp(K::name(b)));
        user
    }

    fn find(&self, name: &str, cx: &App) -> Option<Selection> {
        if self.user(cx).iter().any(|item| K::name(item) == name) {
            return Some(Selection::User(name.to_owned()));
        }
        K::builtins()
            .iter()
            .position(|item| K::name(item) == name)
            .map(Selection::Builtin)
    }

    fn item(&self, selection: &Selection, cx: &App) -> Option<K::Item> {
        match selection {
            Selection::User(name) => K::user(&self.hub.read(cx).saved)
                .iter()
                .find(|item| K::name(item) == name)
                .cloned(),
            Selection::Builtin(i) => K::builtins().get(*i).cloned(),
        }
    }

    fn selected_item(&self, cx: &App) -> Option<K::Item> {
        self.selected.as_ref().and_then(|s| self.item(s, cx))
    }

    /// Whether the shown entry's values and name can be changed.
    fn can_edit(&self, cx: &App) -> bool {
        matches!(self.selected, Some(Selection::User(_)))
            && self
                .selected_item(cx)
                .is_some_and(|item| K::editable(&item))
    }

    fn select_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.find(name, cx);
        if selection.is_some() {
            self.select(selection, window, cx);
        }
    }

    /// Show an entry: its values in the fields, which only a user entry may change.
    fn select(
        &mut self,
        selection: Option<Selection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = selection;
        let item = self.selected_item(cx);
        let texts = item.as_ref().map_or_default(K::texts);
        let editable = self.can_edit(cx);
        for (i, input) in self.inputs.iter().enumerate() {
            let text = texts.get(i).cloned().unwrap_or_default();
            input.update(cx, |input, cx| {
                input.set_value(text, window, cx);
                input.set_disabled(!editable, cx);
            });
        }
        self.notice = None;
        cx.notify();
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Saved)) {
        self.hub.update(cx, |hub, cx| hub.update_saved(edit, cx));
    }

    /// Save what the fields say, as pyfa does after each edit.
    fn commit(&mut self, cx: &mut Context<Self>) {
        if !self.can_edit(cx) {
            return;
        }
        let (Some(Selection::User(name)), Some(item)) =
            (self.selected.clone(), self.selected_item(cx))
        else {
            return;
        };
        let texts: Vec<SharedString> = self.inputs.iter().map(|i| i.read(cx).value()).collect();
        match K::read(&item, &texts) {
            Ok(changed) => {
                self.notice = None;
                if changed != item {
                    self.edit(cx, |saved| {
                        if let Some(slot) =
                            K::user_mut(saved).iter_mut().find(|i| K::name(i) == name)
                        {
                            *slot = changed.clone();
                        }
                        K::retarget(&mut saved.fits, &name, Some(&changed));
                    });
                }
            }
            Err(reason) => self.notice = Some(reason.into()),
        }
        cx.notify();
    }

    /// Why `name` cannot name an entry, in pyfa's words. `keep` is the entry being renamed,
    /// which may keep its name.
    fn name_problem(&self, name: &str, keep: Option<&str>, cx: &App) -> Option<SharedString> {
        let user = K::user(&self.hub.read(cx).saved);
        name_problem(name, user.iter().map(|item| K::name(item)), keep).map(|p| {
            p.say(
                format!("You must supply a name for your {}!", K::ENTITY),
                format!("{} name already in use, please choose another.", K::ENTITY),
            )
        })
    }

    /// Carry out a name prompt that was answered with a usable name.
    fn named(&mut self, naming: Naming, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.selected_item(cx);
        match (naming, current) {
            (Naming::New, _) => {
                let item = K::blank(&name);
                self.edit(cx, |saved| K::user_mut(saved).push(item));
            }
            (Naming::Copy, Some(item)) => {
                let copy = K::renamed(&item, &name);
                self.edit(cx, |saved| K::user_mut(saved).push(copy));
            }
            (Naming::Rename, Some(item)) => {
                let old = K::name(&item).to_owned();
                let renamed = K::renamed(&item, &name);
                self.edit(cx, |saved| {
                    if let Some(slot) = K::user_mut(saved).iter_mut().find(|i| K::name(i) == old) {
                        *slot = renamed.clone();
                    }
                    K::retarget(&mut saved.fits, &old, Some(&renamed));
                });
            }
            (Naming::Copy | Naming::Rename, None) => return,
        }
        self.select(Some(Selection::User(name)), window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Selection::User(name)) = self.selected.clone() else {
            return;
        };
        let at = self.user(cx).iter().position(|item| K::name(item) == name);
        self.edit(cx, |saved| {
            K::user_mut(saved).retain(|item| K::name(item) != name);
            K::retarget(&mut saved.fits, &name, None);
        });
        let next = next_after_delete(&self.user(cx), at)
            .map(|item| Selection::User(K::name(item).to_owned()));
        self.select(next, window, cx);
    }

    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.notice = Some("Could not import from clipboard".into());
            cx.notify();
            return;
        };
        let mut notice = String::new();
        self.edit(cx, |saved| notice = K::import(&text, K::user_mut(saved)));
        // Values of the shown entry may have been replaced.
        let selection = self.selected.clone().or_else(|| {
            self.user(cx)
                .first()
                .map(|item| Selection::User(K::name(item).to_owned()))
        });
        self.select(selection, window, cx);
        self.notice = Some(notice.into());
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let text = K::export(K::user(&self.hub.read(cx).saved));
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice = Some(K::EXPORTED.into());
        cx.notify();
    }

    /// pyfa's name prompt for New, Rename and Copy, in this window. A renamed entry may keep
    /// its name.
    fn prompt_name(&mut self, naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.selected_item(cx) else {
            if naming == Naming::New {
                Self::ask_name(naming, String::new(), window, cx);
            }
            return;
        };
        let current = K::name(&item).to_owned();
        match naming {
            Naming::New => Self::ask_name(naming, String::new(), window, cx),
            Naming::Rename if self.can_edit(cx) => Self::ask_name(naming, current, window, cx),
            Naming::Rename => {}
            Naming::Copy => Self::ask_name(naming, format!("{current} Copy"), window, cx),
        }
    }

    fn ask_name(naming: Naming, default: String, window: &mut Window, cx: &mut Context<Self>) {
        let (title, message) = match naming {
            Naming::New => (
                format!("New {}", K::ENTITY),
                format!("Enter a name for your new {}:", K::ENTITY),
            ),
            Naming::Rename => (
                format!("Rename {}", K::ENTITY),
                format!("Enter a new name for your {}:", K::ENTITY),
            ),
            Naming::Copy => (
                format!("Copy {}", K::ENTITY),
                format!("Enter a name for your {} copy:", K::ENTITY),
            ),
        };
        let keep = (naming == Naming::Rename).then(|| default.clone());
        let editor = cx.weak_entity();
        let check = {
            let editor = editor.clone();
            move |name: &str, cx: &App| {
                editor
                    .upgrade()?
                    .read(cx)
                    .name_problem(name, keep.as_deref(), cx)
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

    /// pyfa asks before deleting.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Selection::User(name)) = self.selected.clone() else {
            return;
        };
        let editor = cx.weak_entity();
        confirm(
            window,
            cx,
            Confirm::delete(format!(
                "Do you really want to delete the {name} {}?",
                K::ENTITY
            )),
            move |window, cx| {
                let _ = editor.update(cx, |e, cx| e.delete(window, cx));
            },
        );
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    fn entry_row(
        &self,
        id: SharedString,
        label: String,
        selection: Selection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&selection);
        list_row(id, selected, cx)
            .px_2()
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.select(Some(selection.clone()), window, cx);
                window.focus(&this.focus, cx);
            }))
            .into_any_element()
    }

    fn list(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let shown = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
        let user: Vec<String> = self
            .user(cx)
            .iter()
            .map(|item| K::name(item).to_owned())
            .filter(|name| shown(name))
            .collect();
        let builtins: Vec<(usize, String)> = K::builtins()
            .iter()
            .enumerate()
            .map(|(i, item)| (i, profile::full_name(K::name(item))))
            .filter(|(_, name)| shown(name))
            .collect();
        let heading = |text: &str, cx: &App| {
            div()
                .px_2()
                .pt_2()
                .pb_1()
                .child(caption(text, cx))
                .into_any_element()
        };
        let mut rows = Vec::new();
        if !user.is_empty() || query.is_empty() {
            rows.push(heading("Yours", cx));
        }
        if user.is_empty() && query.is_empty() {
            rows.push(empty_line("None yet. Click New, or Copy a builtin.", cx).into_any_element());
        }
        for (i, name) in user.into_iter().enumerate() {
            let row = self.entry_row(
                SharedString::from(format!("user-{i}")),
                name.clone(),
                Selection::User(name),
                cx,
            );
            rows.push(row);
        }
        if !builtins.is_empty() {
            rows.push(heading("Builtin", cx));
        }
        for (i, name) in builtins {
            let row = self.entry_row(
                SharedString::from(format!("builtin-{i}")),
                name,
                Selection::Builtin(i),
                cx,
            );
            rows.push(row);
        }
        let has = self.selected.is_some();
        let user = matches!(self.selected, Some(Selection::User(_)));
        let editable = self.can_edit(cx);
        let footer = h_flex()
            .gap_1()
            .child(
                icon_button("profile-new", AssetIcon::Plus, "New").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.prompt_name(Naming::New, window, cx);
                    },
                )),
            )
            .child(
                icon_button("profile-copy", AssetIcon::Copy, "Copy (Ctrl+D)")
                    .disabled(!has)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.prompt_name(Naming::Copy, window, cx);
                    })),
            )
            .child(
                icon_button("profile-rename", AssetIcon::Pencil, "Rename (F2)")
                    .disabled(!editable)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.prompt_name(Naming::Rename, window, cx);
                    })),
            )
            .child(
                icon_button("profile-import", AssetIcon::ClipboardPaste, K::IMPORT_TIP)
                    .on_click(cx.listener(|this, _, window, cx| this.import(window, cx))),
            )
            .child(
                icon_button("profile-export", AssetIcon::ClipboardCopy, K::EXPORT_TIP)
                    .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
            )
            .child(div().flex_1())
            .child(
                icon_button("profile-delete", AssetIcon::Trash, "Delete (Delete)")
                    .disabled(!user)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_delete(window, cx);
                    })),
            );
        Sidebar {
            caption: K::LIST.into(),
            search: Some(self.search.clone()),
            list_id: "profile-list",
            rows,
            empty: Some(empty_line("Nothing matches.", cx).into_any_element()),
            footer: Some(footer.into_any_element()),
            narrow,
        }
        .render(cx)
    }

    fn fields(&self, item: Option<&K::Item>, cx: &App) -> impl IntoElement {
        let rows = K::fields().iter().enumerate().map(|(i, field)| {
            let icon = match (field.icon, &self.loaded) {
                (Some(id), Some(loaded)) => {
                    icon_box(&loaded.catalog, &loaded.icons, Icon::Id(id), 20.).into_any_element()
                }
                _ => div().w(px(20.)).into_any_element(),
            };
            h_flex()
                .id(SharedString::from(format!("field-{i}")))
                .gap_2()
                .child(icon)
                .child(div().flex_none().w(px(76.)).truncate().child(field.label))
                .child(
                    div()
                        .w(px(110.))
                        .min_w(px(70.))
                        .child(Input::new(&self.inputs[i]).small()),
                )
                .child(div().flex_none().w(px(36.)).child(field.unit))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(item.and_then(|item| K::note(item, i)).unwrap_or_default()),
                )
                .tip(field.tip)
        });
        v_flex().flex_none().gap_2().children(rows)
    }
}

impl<K: Kind> Render for Editor<K> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < px(NARROW_W);
        let item = self.selected_item(cx);
        let heading = match (&self.selected, &item) {
            (Some(Selection::Builtin(_)), Some(item)) => {
                format!("{} (builtin, read-only)", profile::full_name(K::name(item)))
            }
            (_, Some(item)) => K::name(item).to_owned(),
            (_, None) => format!("No {} selected", K::ENTITY),
        };
        let notice = self.notice.clone();
        h_flex()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Delete, window, cx| this.confirm_delete(window, cx)))
            .on_action(cx.listener(|this, _: &Duplicate, window, cx| {
                this.prompt_name(Naming::Copy, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Rename, window, cx| {
                this.prompt_name(Naming::Rename, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.focus_search(window, cx);
            }))
            .child(self.list(narrow, cx))
            .child(
                v_flex()
                    .id("profile-fields")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_3()
                    .gap_3()
                    .child(
                        div()
                            .flex_none()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(heading),
                    )
                    .child(self.fields(item.as_ref(), cx))
                    .when_some(notice, |col, notice| {
                        col.child(div().text_color(cx.theme().muted_foreground).child(notice))
                    }),
            )
    }
}
