//! The universal search: one box over the fit window that finds commands, settings, items,
//! ships, fits and characters, and acts on what it found, like an editor's command palette.
//!
//! Enter does the first thing a result offers; Shift+Enter lists everything it offers, which
//! typing narrows like the results. Tab steps through the scopes. An amount typed as "x1000"
//! or "5x" goes with the item: that many into the cargo hold, or the drone bay for a drone.
//!
//! The list is gpui-kit's command palette with its own filtering switched off; [`Efa::find`]
//! answers the query instead.

mod commands;
mod hits;

use std::{cell::Cell, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IndexPath, Sizable, WindowExt,
    command::{Command, CommandGroup, CommandItem, CommandState},
    h_flex,
    kbd::Kbd,
};
use gpui_kit::{
    Action, AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, Keystroke, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{Efa, Loaded, widgets::icon_box};
use crate::{data::TypeId, finder::Scope, icons::Icon};
use hits::Run;

gpui_kit::actions!(efa_finder, [NextScope, PreviousScope, MoreActions]);

/// Key context of the search, around the palette.
const CONTEXT: &str = "EfaFinder";
/// The same with the focus in the query field, which has a Tab of its own to outrank.
const QUERY_CONTEXT: &str = "EfaFinder > Input";

/// How wide the search is, and how tall its list, in a window with room for them.
const WIDTH: f32 = 640.;
const LIST_H: f32 = 400.;
/// How far below the window's top the search opens.
const TOP: f32 = 80.;
/// The window's height the list leaves to the rest of the search and the room around it.
const AROUND: f32 = 240.;
/// The size of a result's icon.
const GLYPH: f32 = 20.;
/// Sections every other list of results starts at; see [`Finder::shifted`].
const SHIFT: usize = 16;

/// Bind the search's keys. Call once at startup, after gpui-kit bound its own.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([CONTEXT, QUERY_CONTEXT].into_iter().flat_map(|context| {
        [
            KeyBinding::new("tab", NextScope, Some(context)),
            KeyBinding::new("shift-tab", PreviousScope, Some(context)),
            KeyBinding::new("shift-enter", MoreActions, Some(context)),
        ]
    }));
}

/// What stands for a result at the start of its row.
#[derive(Debug, Clone)]
enum Glyph {
    Icon(AssetIcon),
    /// An item's own icon.
    Type(TypeId),
}

/// What a result's row ends with.
#[derive(Debug, Clone)]
enum Tag {
    None,
    Text(SharedString),
    /// The keys bound to a menu bar action, if any.
    Shortcut(fn() -> Box<dyn Action>),
    /// Whether a preference is on.
    Switch(bool),
}

/// Something a result offers to do.
#[derive(Debug, Clone)]
struct Act {
    label: SharedString,
    icon: AssetIcon,
    run: Run,
    /// The search stays open afterwards, as for a preference switched.
    stay: bool,
}

impl Act {
    fn new(label: impl Into<SharedString>, icon: AssetIcon, run: Run) -> Self {
        Self {
            label: label.into(),
            icon,
            run,
            stay: false,
        }
    }

    fn stay(mut self) -> Self {
        self.stay = true;
        self
    }
}

/// A row of the search: a result, or one of a result's actions.
#[derive(Debug, Clone)]
struct Entry {
    glyph: Glyph,
    label: SharedString,
    /// Said quieter after the label: where a command is, what ship a fit is on.
    detail: Option<SharedString>,
    tag: Tag,
    /// What it offers, the first being what Enter does. Never empty.
    acts: Vec<Act>,
    /// Listed but not to be picked, like a command the current fit has no use for.
    disabled: bool,
}

impl Entry {
    /// An action as a row of its own, for the list of everything a result offers.
    fn of_act(act: &Act) -> Self {
        Self {
            glyph: Glyph::Icon(act.icon),
            label: act.label.clone(),
            detail: None,
            tag: Tag::None,
            acts: vec![act.clone()],
            disabled: false,
        }
    }
}

#[derive(Debug)]
struct Group {
    title: Option<SharedString>,
    entries: Vec<Entry>,
}

/// The universal search of a fit window.
pub struct Finder {
    command: Entity<CommandState>,
    scope: Scope,
    /// What is typed, which is a filter for the actions while a result's actions show.
    query: String,
    /// The result whose actions are listed in place of results, and the query to go back to.
    focused: Option<(Entry, String)>,
    groups: Rc<Vec<Group>>,
    /// Flips with every new list of results. The palette keeps its highlight on a row that
    /// is still there by section and row, which for new results would leave it wherever it
    /// was; every other list starts [`SHIFT`] sections in, so no row carries over and the
    /// highlight starts at the top.
    shifted: bool,
}

impl Finder {
    pub fn new(window: &mut Window, cx: &mut Context<Efa>) -> Self {
        Self {
            command: cx.new(|cx| CommandState::new(window, cx)),
            scope: Scope::All,
            query: String::new(),
            focused: None,
            groups: Rc::default(),
            shifted: false,
        }
    }

    fn shift(&self) -> usize {
        if self.shifted { SHIFT } else { 0 }
    }

    fn entry(&self, index: IndexPath) -> Option<&Entry> {
        entry_at(&self.groups, self.shift(), index)
    }
}

fn entry_at(groups: &[Group], shift: usize, index: IndexPath) -> Option<&Entry> {
    groups
        .get(index.section.checked_sub(shift)?)?
        .entries
        .get(index.row)
}

impl Efa {
    /// Open the search over this window, empty and looking through everything.
    pub(super) fn open_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finder.scope = Scope::All;
        self.finder.focused = None;
        self.set_finder_query(String::new(), window, cx);

        let this = cx.weak_entity();
        let command = self.finder.command.clone();
        let focus_on_mount = Rc::new(Cell::new(true));
        window.open_dialog(cx, move |dialog, window, _| {
            let (this, command) = (this.clone(), command.clone());
            let focus_on_mount = Rc::clone(&focus_on_mount);
            let width = (window.viewport_size().width - px(32.)).min(px(WIDTH));
            dialog
                .close_button(false)
                .p_0()
                .w(width)
                .margin_top(px(TOP))
                .content(move |content, window, cx| {
                    if focus_on_mount.replace(false) {
                        let command = command.clone();
                        window.defer(cx, move |window, cx| {
                            command.focus_handle(cx).focus(window, cx);
                        });
                    }
                    content.children(view(&this, window, cx))
                })
        });
    }

    /// Put `query` into the field and list what it finds.
    fn set_finder_query(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        self.finder.query.clone_from(&query);
        self.finder
            .command
            .update(cx, |command, cx| command.set_query(query, window, cx));
        self.refind(cx);
    }

    /// List the results again, for a new query, scope or state of the window.
    fn refind(&mut self, cx: &mut Context<Self>) {
        self.finder.groups = Rc::new(self.find());
        self.finder.shifted = !self.finder.shifted;
        cx.notify();
    }

    fn finder_typed(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.finder.query != query {
            query.clone_into(&mut self.finder.query);
            self.refind(cx);
        }
    }

    fn set_finder_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        // A click on a scope keeps the typing going.
        self.finder.command.focus_handle(cx).focus(window, cx);
        if self.finder.focused.is_none() && self.finder.scope != scope {
            self.finder.scope = scope;
            self.refind(cx);
        }
    }

    fn step_finder_scope(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.set_finder_scope(self.finder.scope.step(forward), window, cx);
    }

    /// Shift+Enter: list everything the highlighted result offers.
    fn finder_more(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finder.focused.is_some() {
            return;
        }
        let Some(index) = self.finder.command.read(cx).selected_index() else {
            return;
        };
        let Some(entry) = self.finder.entry(index).cloned() else {
            return;
        };
        if entry.disabled || entry.acts.len() < 2 {
            return;
        }
        let query = std::mem::take(&mut self.finder.query);
        self.finder.focused = Some((entry, query));
        self.set_finder_query(String::new(), window, cx);
    }

    /// Escape: back from a result's actions to the results. False when the results show, and
    /// Escape is the dialog's to close with.
    fn finder_back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((_, query)) = self.finder.focused.take() else {
            return false;
        };
        self.set_finder_query(query, window, cx);
        true
    }

    /// Enter or a click on a row: do the first thing it offers.
    fn finder_confirm(&mut self, index: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(act) = self
            .finder
            .entry(index)
            .filter(|entry| !entry.disabled)
            .and_then(|entry| entry.acts.first())
            .cloned()
        else {
            return;
        };
        if !act.stay {
            window.close_dialog(cx);
        }
        self.finder_run(act.run, window, cx);
        if act.stay {
            // The results show what it did, a switch now on: back to them from the actions.
            match self.finder.focused.take() {
                Some((_, query)) => self.set_finder_query(query, window, cx),
                None => self.refind(cx),
            }
        }
    }
}

/// The search as the dialog shows it. None once the window behind it is gone.
fn view(this: &WeakEntity<Efa>, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let efa = this.upgrade()?;
    let (command, groups, shift, scope, focused, query, loaded, root) = {
        let g = efa.read(cx);
        let finder = &g.finder;
        (
            finder.command.clone(),
            Rc::clone(&finder.groups),
            finder.shift(),
            finder.scope,
            finder
                .focused
                .as_ref()
                .map(|(entry, _)| entry.label.clone()),
            finder.query.clone(),
            g.loaded(),
            g.root_focus.clone(),
        )
    };
    let in_actions = focused.is_some();
    // A fixed height, so the search does not jump as results come and go.
    let height = (window.viewport_size().height - px(AROUND))
        .min(px(LIST_H))
        .max(px(120.));
    let placeholder: SharedString = match (&focused, scope) {
        (Some(_), _) => "Filter the actions".into(),
        (None, Scope::All) => "Search items, ships, fits, actions and settings".into(),
        (None, scope) => format!("Search {}", scope.label().to_lowercase()).into(),
    };

    let mut palette = Command::new(&command)
        .bordered(false)
        .rounded(cx.theme().radius_lg)
        .filterable(false)
        .placeholder(placeholder)
        .min_h(height)
        .max_h(height);
    for _ in 0..shift {
        palette = palette.group(CommandGroup::new());
    }
    for group in groups.iter() {
        let items = group.entries.iter().map(|entry| {
            let (shown, loaded, root) = (entry.clone(), loaded.clone(), root.clone());
            CommandItem::new()
                .label(entry.label.clone())
                .disabled(entry.disabled)
                .child(move |window, cx| row(&shown, loaded.as_deref(), &root, window, cx))
        });
        let mut section = CommandGroup::new().items(items);
        if let Some(title) = &group.title {
            section = section.label(title.clone());
        }
        palette = palette.group(section);
    }

    let palette = palette
        .header({
            let this = this.clone();
            move |_, _, cx| header(scope, focused.clone(), &this, cx)
        })
        .footer({
            let groups = Rc::clone(&groups);
            move |state, _, cx| {
                let entry = state
                    .selected_index()
                    .and_then(|index| entry_at(&groups, shift, index));
                footer(entry, in_actions, cx)
            }
        })
        .empty(move |_, _, cx| {
            let text = if query.trim().is_empty() {
                "Type to search.".to_owned()
            } else {
                format!("Nothing found for \u{201c}{}\u{201d}.", query.trim())
            };
            div()
                .w_full()
                .py_8()
                .text_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text)
        })
        .on_query({
            let this = this.clone();
            move |query, _, cx| {
                let _ = this.update(cx, |g, cx| g.finder_typed(query, cx));
            }
        })
        .on_confirm({
            let this = this.clone();
            move |index, window, cx| {
                let _ = this.update(cx, |g, cx| g.finder_confirm(index, window, cx));
            }
        });

    let on = |f: fn(&mut Efa, &mut Window, &mut Context<Efa>)| {
        let this = this.clone();
        move |window: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |g, cx| f(g, window, cx));
        }
    };
    let (next, previous, more) = (
        on(|g, window, cx| g.step_finder_scope(true, window, cx)),
        on(|g, window, cx| g.step_finder_scope(false, window, cx)),
        on(Efa::finder_more),
    );
    let back = this.clone();
    Some(
        div()
            .key_context(CONTEXT)
            .on_action(move |_: &NextScope, window, cx| next(window, cx))
            .on_action(move |_: &PreviousScope, window, cx| previous(window, cx))
            .on_action(move |_: &MoreActions, window, cx| more(window, cx))
            // The palette lets an Escape with nothing typed through, for the dialog to close
            // on. From a result's actions it goes back to the results first.
            .on_action(move |_: &Cancel, window, cx| {
                let went_back = back
                    .update(cx, |g, cx| g.finder_back(window, cx))
                    .unwrap_or(false);
                if !went_back {
                    cx.propagate();
                }
            })
            .child(palette)
            .into_any_element(),
    )
}

/// A result's row: its icon, name, what there is to say about it, and its tag at the end.
fn row(
    entry: &Entry,
    loaded: Option<&Loaded>,
    root: &FocusHandle,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let glyph = match (&entry.glyph, loaded) {
        (Glyph::Type(id), Some(loaded)) => {
            icon_box(&loaded.catalog, &loaded.icons, Icon::Type(*id), GLYPH)
        }
        (Glyph::Type(_), None) => div().flex_none().size(px(GLYPH)),
        (Glyph::Icon(icon), _) => div()
            .flex_none()
            .size(px(GLYPH))
            .flex()
            .items_center()
            .justify_center()
            .child(UiIcon::new(*icon).small()),
    };
    // Quieter by opacity rather than color, so it reads on the highlighted row too.
    let quiet = |text: SharedString| div().text_xs().opacity(0.6).child(text);
    let tag = match &entry.tag {
        Tag::None => None,
        Tag::Text(text) => Some(quiet(text.clone()).flex_none().into_any_element()),
        Tag::Shortcut(make) => Kbd::binding_for_action_in(make().as_ref(), root, window)
            .map(IntoElement::into_any_element),
        Tag::Switch(on) => Some(
            div()
                .flex_none()
                .px_1p5()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .text_xs()
                .when(*on, |tag| {
                    tag.bg(cx.theme().primary)
                        .text_color(cx.theme().primary_foreground)
                })
                .when(!*on, |tag| tag.opacity(0.6))
                .child(if *on { "On" } else { "Off" })
                .into_any_element(),
        ),
    };
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .items_center()
        .child(glyph)
        .child(
            div()
                .flex_none()
                .max_w_full()
                .truncate()
                .child(entry.label.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .children(entry.detail.clone().map(quiet)),
        )
        .children(tag)
        .into_any_element()
}

/// Above the query: the scopes, or the result whose actions are listed.
fn header(
    scope: Scope,
    focused: Option<SharedString>,
    this: &WeakEntity<Efa>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let bar = h_flex()
        .gap_1()
        .px_2()
        .py_1p5()
        .border_b_1()
        .border_color(theme.border)
        .text_xs();
    if let Some(label) = focused {
        return bar
            .px_3()
            .gap_2()
            .child(UiIcon::new(AssetIcon::CornerDownRight).xsmall())
            .child(div().min_w_0().truncate().child(label))
            .into_any_element();
    }
    bar.children(Scope::ALL.map(|chip| {
        let this = this.clone();
        div()
            .id(chip.label())
            .px_2()
            .py_0p5()
            .rounded(theme.radius)
            .cursor_pointer()
            .when(chip == scope, |c| {
                c.bg(theme.accent).text_color(theme.accent_foreground)
            })
            .when(chip != scope, |c| {
                c.text_color(theme.muted_foreground)
                    .hover(|c| c.bg(theme.accent.opacity(0.5)))
            })
            .child(chip.label())
            .on_click(move |_, window, cx| {
                let _ = this.update(cx, |g, cx| g.set_finder_scope(chip, window, cx));
            })
    }))
    .into_any_element()
}

/// Below the list: what the keys do with the highlighted row.
fn footer(entry: Option<&Entry>, in_actions: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let hint = |keys: &'static str, text: SharedString| {
        h_flex()
            .flex_none()
            .gap_1p5()
            .items_center()
            .children(Keystroke::parse(keys).ok().map(Kbd::new))
            .child(text)
    };
    let act = entry
        .filter(|entry| !entry.disabled)
        .and_then(|entry| entry.acts.first());
    let more = !in_actions && entry.is_some_and(|entry| !entry.disabled && entry.acts.len() > 1);
    h_flex()
        .gap_4()
        .px_3()
        .py_1p5()
        .border_t_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(theme.muted_foreground)
        .when_some(act, |bar, act| bar.child(hint("enter", act.label.clone())))
        .when(more, |bar| {
            bar.child(hint("shift-enter", "More actions".into()))
        })
        .child(div().flex_1())
        .when(!in_actions, |bar| bar.child(hint("tab", "Scope".into())))
        .child(hint(
            "escape",
            if in_actions { "Back" } else { "Close" }.into(),
        ))
        .into_any_element()
}
