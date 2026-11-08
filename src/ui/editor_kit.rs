//! Pieces the list editors share: their keys, the sidebar listing the named entries, name
//! checks, and the market item tree to add from.

use std::{collections::HashSet, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, Sizable,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder, px,
};

use super::{
    Loaded,
    drag::{DragPreview, DragSource, Dragged},
    widgets::{DropHighlight, caption, empty_state, icon_box, list_row, tree_row},
};
use crate::{
    data::{Catalog, ItemType, TypeId},
    icons::Icon,
};

gpui_kit::actions!(list_editor, [Delete, Duplicate, Rename, FocusSearch]);

/// The key context every list editor's window sets, for the shortcuts below.
pub(super) const CONTEXT: &str = "ListEditor";

/// The list editors' shortcuts. A text field with the focus keeps Delete for its text.
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("ctrl-d", Duplicate, Some(CONTEXT)),
        KeyBinding::new("f2", Rename, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
    ]);
}

/// Most search hits the item tree lists, so a one-letter query stays cheap to draw.
const SEARCH_LIMIT: usize = 200;
/// The list's width, and its width in a narrow window.
const LIST_W: f32 = 230.;
const LIST_NARROW_W: f32 = 180.;
/// Below this window width an editor folds its market away behind a button.
pub(super) const FOLD_W: f32 = 960.;
/// What the middle column keeps beside an open market in a narrow window.
pub(super) const MIDDLE_MIN_W: f32 = 260.;

/// A small ghost button showing only an icon, with a tooltip.
pub(super) fn icon_button(
    id: impl Into<SharedString>,
    icon: AssetIcon,
    tip: &'static str,
) -> Button {
    Button::new(id.into())
        .ghost()
        .small()
        .icon(UiIcon::new(icon))
        .tooltip(tip)
}

/// What is wrong with a name for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameProblem {
    Missing,
    Taken,
}

impl NameProblem {
    /// The editor's own words for the problem.
    pub(super) fn say(
        self,
        missing: impl Into<SharedString>,
        taken: impl Into<SharedString>,
    ) -> SharedString {
        match self {
            Self::Missing => missing.into(),
            Self::Taken => taken.into(),
        }
    }
}

/// What is wrong with `name` given the names in use, if anything. `keep` is the entry being
/// renamed, which may keep its name.
pub(super) fn name_problem<'a>(
    name: &str,
    taken: impl IntoIterator<Item = &'a str>,
    keep: Option<&str>,
) -> Option<NameProblem> {
    if name.is_empty() {
        Some(NameProblem::Missing)
    } else if Some(name) != keep && taken.into_iter().any(|t| t == name) {
        Some(NameProblem::Taken)
    } else {
        None
    }
}

/// The entry shown after deleting the one at `at`: the one that took its place, or the last.
pub(super) fn next_after_delete<T>(items: &[T], at: Option<usize>) -> Option<&T> {
    at.and_then(|i| items.get(i).or_else(|| items.last()))
}

/// The column of named entries down a list editor's left side: a caption, a search box, the
/// rows, what shows when there are none, and the editor's buttons under them.
pub(super) struct Sidebar {
    pub(super) caption: SharedString,
    pub(super) search: Option<Entity<InputState>>,
    pub(super) list_id: &'static str,
    pub(super) rows: Vec<AnyElement>,
    /// Shown when `rows` is empty.
    pub(super) empty: Option<AnyElement>,
    pub(super) footer: Option<AnyElement>,
    /// The window is narrow, and the list takes less of it.
    pub(super) narrow: bool,
}

impl Sidebar {
    pub(super) fn render(self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let empty = if self.rows.is_empty() {
            self.empty
        } else {
            None
        };
        v_flex()
            .w(px(if self.narrow { LIST_NARROW_W } else { LIST_W }))
            .flex_none()
            .h_full()
            .gap_2()
            .p_2()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(caption(&self.caption, cx))
            .children(
                self.search
                    .map(|search| Input::new(&search).small().cleanable(true)),
            )
            .child(
                v_flex()
                    .id(self.list_id)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(empty)
                    .children(self.rows),
            )
            .children(self.footer)
            .into_any_element()
    }
}

/// A line of a list of named entries, with a muted detail such as its size.
pub(super) struct ListEntry {
    pub(super) name: String,
    pub(super) detail: String,
}

/// What dropping market items on an entry of a [`name_list`] does, given the entry's name.
pub(super) type DropOnEntry<V> = fn(&mut V, String, &Dragged, &mut Context<V>);

/// The entries matching `query`, as clickable rows; the selected one highlighted. With
/// `on_drop`, items from the market can be dropped on an entry.
pub(super) fn name_list<V: 'static>(
    entries: Vec<ListEntry>,
    query: &str,
    selected: Option<&str>,
    cx: &mut Context<V>,
    on_select: fn(&mut V, String, &mut Window, &mut Context<V>),
    on_drop: Option<DropOnEntry<V>>,
) -> Vec<AnyElement> {
    let query = query.trim().to_lowercase();
    let theme = cx.theme().clone();
    entries
        .into_iter()
        .filter(|e| query.is_empty() || e.name.to_lowercase().contains(&query))
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = selected == Some(entry.name.as_str());
            let name = entry.name.clone();
            let drop_name = entry.name.clone();
            let row = list_row(SharedString::from(format!("entry-{i}")), is_selected, cx)
                .gap_2()
                .px_2()
                .when(is_selected, |row| row.font_weight(FontWeight::MEDIUM))
                .child(div().flex_1().min_w_0().truncate().child(entry.name))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(entry.detail),
                )
                // Picking a preset or set happens on press, like a tab.
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(move |view, _: &gpui_kit::MouseDownEvent, window, cx| {
                        on_select(view, name.clone(), window, cx);
                    }),
                );
            match on_drop {
                Some(on_drop) => row
                    .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Market))
                    .on_drop(cx.listener(move |view, dragged: &Dragged, _, cx| {
                        if matches!(dragged.source, DragSource::Market) {
                            cx.stop_propagation();
                            on_drop(view, drop_name.clone(), dragged, cx);
                        }
                    }))
                    .into_any_element(),
                None => row.into_any_element(),
            }
        })
        .collect()
}

/// The market as a tree to add items from, narrowed to what `accepts`, with a search box that
/// lists hits instead. Items can be dragged out or double-clicked.
pub(super) struct ItemTree {
    pub(super) search: Entity<InputState>,
    accepts: fn(&ItemType) -> bool,
    /// Market groups holding something accepted, themselves or below.
    useful: HashSet<i32>,
    roots: Vec<i32>,
    expanded: HashSet<i32>,
}

impl ItemTree {
    /// A tree with its top groups open.
    pub(super) fn new(
        catalog: &Catalog,
        accepts: fn(&ItemType) -> bool,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let mut useful = HashSet::new();
        let roots: Vec<i32> = catalog
            .market_roots
            .iter()
            .copied()
            .filter(|&root| mark_useful(catalog, accepts, root, &mut useful))
            .collect();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        Self {
            search,
            accepts,
            useful,
            expanded: roots.iter().copied().collect(),
            roots,
        }
    }

    pub(super) fn toggle(&mut self, group: i32) {
        if !self.expanded.remove(&group) {
            self.expanded.insert(group);
        }
    }

    /// The search box and the tree or the search hits under it. `tree` finds this tree in the
    /// view, `add` takes a double-clicked item.
    pub(super) fn render<V: 'static>(
        &self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<V>,
        tree: fn(&mut V) -> &mut Self,
        add: fn(&mut V, TypeId, &mut Window, &mut Context<V>),
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let query = self.search.read(cx).value().trim().to_owned();
        let mut rows: Vec<AnyElement> = Vec::new();
        if query.is_empty() {
            for &root in &self.roots {
                self.group_rows(loaded, root, 0, cx, tree, add, &mut rows);
            }
        } else {
            let hits = crate::market::search(catalog, &query, SEARCH_LIMIT)
                .into_iter()
                .filter(|&t| catalog.get(t).is_some_and(self.accepts));
            for t in hits {
                rows.push(item_row(loaded, t, 0, cx, add));
            }
            if rows.is_empty() {
                rows.push(
                    empty_state(
                        AssetIcon::SearchX,
                        "No items match",
                        "Try fewer or shorter words.",
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        v_flex()
            .size_full()
            .gap_2()
            .child(Input::new(&self.search).small().cleanable(true))
            .child(
                v_flex()
                    .id("item-tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the recursion carries the view's callbacks along"
    )]
    fn group_rows<V: 'static>(
        &self,
        loaded: &Rc<Loaded>,
        id: i32,
        depth: usize,
        cx: &mut Context<V>,
        tree: fn(&mut V) -> &mut Self,
        add: fn(&mut V, TypeId, &mut Window, &mut Context<V>),
        out: &mut Vec<AnyElement>,
    ) {
        let catalog = &loaded.catalog;
        let Some(group) = catalog.market_groups.get(&id) else {
            return;
        };
        let open = self.expanded.contains(&id);
        let icon = group
            .icon_id
            .map(|icon| icon_box(catalog, &loaded.icons, Icon::Id(icon), 18.).into_any_element());
        out.push(
            tree_row(
                SharedString::from(format!("tree-group-{id}")),
                depth,
                Some(open),
                icon,
                group.name.clone(),
                false,
                cx,
            )
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(move |view, _: &gpui_kit::MouseDownEvent, _, cx| {
                    tree(view).toggle(id);
                    cx.notify();
                }),
            )
            .into_any_element(),
        );
        if !open {
            return;
        }
        for &child in &group.children {
            if self.useful.contains(&child) {
                self.group_rows(loaded, child, depth + 1, cx, tree, add, out);
            }
        }
        for &t in &group.types {
            if catalog.get(t).is_some_and(self.accepts) {
                out.push(item_row(loaded, t, depth + 1, cx, add));
            }
        }
    }
}

/// Whether a group holds something accepted, noting each group that does.
fn mark_useful(
    catalog: &Catalog,
    accepts: fn(&ItemType) -> bool,
    id: i32,
    useful: &mut HashSet<i32>,
) -> bool {
    let Some(group) = catalog.market_groups.get(&id) else {
        return false;
    };
    let mut any = group
        .types
        .iter()
        .any(|&t| catalog.get(t).is_some_and(accepts));
    for &child in &group.children {
        any |= mark_useful(catalog, accepts, child, useful);
    }
    if any {
        useful.insert(id);
    }
    any
}

fn item_row<V: 'static>(
    loaded: &Rc<Loaded>,
    type_id: TypeId,
    depth: usize,
    cx: &mut Context<V>,
    add: fn(&mut V, TypeId, &mut Window, &mut Context<V>),
) -> AnyElement {
    let catalog = &loaded.catalog;
    let icon = icon_box(catalog, &loaded.icons, Icon::Type(type_id), 20.).into_any_element();
    let row = tree_row(
        SharedString::from(format!("tree-item-{type_id}")),
        depth,
        None,
        Some(icon),
        catalog.name(type_id),
        false,
        cx,
    )
    .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
        if event.click_count() >= 2 {
            add(view, type_id, window, cx);
        }
    }));
    match Dragged::of(loaded, DragSource::Market, Some(type_id)) {
        Some(dragged) => row
            .on_drag(dragged, |dragged, _, _, cx| {
                cx.new(|_| DragPreview(dragged.clone()))
            })
            .into_any_element(),
        None => row.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_entry_may_keep_its_name() {
        let taken = ["Fuel", "Scripts"];
        assert_eq!(name_problem("", taken, None), Some(NameProblem::Missing));
        assert_eq!(name_problem("Fuel", taken, None), Some(NameProblem::Taken));
        assert_eq!(name_problem("Fuel", taken, Some("Fuel")), None);
        assert_eq!(name_problem("Ammo", taken, None), None);
    }

    #[test]
    fn deleting_shows_the_next_entry_or_the_last() {
        let left = ["a", "c"];
        assert_eq!(next_after_delete(&left, Some(1)), Some(&"c"));
        assert_eq!(next_after_delete(&left, Some(2)), Some(&"c"));
        assert_eq!(next_after_delete::<&str>(&[], Some(0)), None);
        assert_eq!(next_after_delete(&left, None), None);
    }
}
