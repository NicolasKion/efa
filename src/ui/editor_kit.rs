//! Pieces the list editors share: captions, empty states, the name prompt and delete
//! confirmation, the list of named entries and the market item tree to add from.

use std::{collections::HashSet, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Div, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Loaded,
    fitting::{DragPreview, DragSource, Dragged},
    widgets::icon_box,
};
use crate::{
    data::{Catalog, ItemType, TypeId},
    icons::Icon,
};

/// Most search hits the item tree lists, so a one-letter query stays cheap to draw.
const SEARCH_LIMIT: usize = 200;
pub const ROW_HEIGHT: f32 = 28.;
/// Height of a line of an item tree.
pub const TREE_ROW: f32 = 24.;
/// How far each level of an item tree is indented.
pub const INDENT: f32 = 14.;

/// A small uppercase caption over a column or section.
pub fn caption(text: &str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}

/// What an empty list shows: an icon, a line on what is missing and one on what to do.
pub fn empty_state(icon: AssetIcon, title: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_8()
        .child(
            UiIcon::new(icon)
                .size(px(28.))
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_center()
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_xs()
                .text_center()
                .text_color(theme.muted_foreground)
                .child(hint.to_owned()),
        )
}

/// A small ghost button showing only an icon, with a tooltip.
pub fn icon_button(id: impl Into<SharedString>, icon: AssetIcon, tip: &'static str) -> Button {
    Button::new(id.into())
        .ghost()
        .small()
        .icon(UiIcon::new(icon))
        .tooltip(tip)
}

/// Ask for a name in a dialog. `check` returns what is wrong with a name, which keeps the
/// dialog open; otherwise `done` gets the trimmed name. Enter submits like the OK button.
pub fn prompt_name(
    window: &mut Window,
    cx: &mut App,
    title: String,
    message: String,
    default: String,
    check: impl Fn(&str, &App) -> Option<&'static str> + 'static,
    done: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(default));
    let error = cx.new(|_| None::<SharedString>);
    let (check, done) = (Rc::new(check), Rc::new(done));
    // Typing replaces the name, once the dialog is up and cannot take the focus back.
    let field = input.clone();
    window.defer(cx, move |window, cx| {
        field.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    });
    window.open_dialog(cx, move |dialog, _, cx| {
        let problem = error.read(cx).clone();
        let submit: super::widgets::Submit = {
            let (input, error, check, done) = (
                input.clone(),
                error.clone(),
                Rc::clone(&check),
                Rc::clone(&done),
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
                done(name, window, cx);
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
            .title(title.clone())
            .w(px(380.))
            .child(
                v_flex()
                    .gap_3()
                    .child(message.clone())
                    .child(Input::new(&input))
                    .when_some(problem, |col, problem| {
                        col.child(div().text_sm().text_color(cx.theme().danger).child(problem))
                    })
                    .child(h_flex().justify_end().child(
                        Button::new("name-ok").primary().label("OK").on_click({
                            let submit = Rc::clone(&submit);
                            move |e: &ClickEvent, w: &mut Window, cx: &mut App| submit(e, w, cx)
                        }),
                    )),
            )
    });
}

/// Ask before deleting; `yes` runs once the user agrees.
pub fn confirm_delete(
    window: &mut Window,
    cx: &mut App,
    message: String,
    yes: impl Fn(&mut Window, &mut App) + 'static,
) {
    let yes = Rc::new(yes);
    window.open_dialog(cx, move |dialog, _, _| {
        let yes = Rc::clone(&yes);
        dialog.title("Confirm Delete").w(px(380.)).child(
            v_flex().gap_3().child(message.clone()).child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("delete-no")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("delete-yes").danger().label("Delete").on_click(
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            yes(window, cx);
                        },
                    )),
            ),
        )
    });
}

/// A line of a list of named entries, with a muted detail such as its size.
pub struct ListEntry {
    pub name: String,
    pub detail: String,
}

/// What dropping market items on an entry of a [`name_list`] does, given the entry's name.
pub type DropOnEntry<V> = fn(&mut V, String, &Dragged, &mut Context<V>);

/// The entries matching `query`, as clickable rows; the selected one highlighted. With
/// `on_drop`, items from the market can be dropped on an entry.
pub fn name_list<V: 'static>(
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
            let row = h_flex()
                .id(SharedString::from(format!("entry-{i}")))
                .gap_2()
                .h(px(ROW_HEIGHT))
                .flex_none()
                .px_2()
                .rounded(px(4.))
                .when(is_selected, |row| {
                    row.bg(theme.list_active).font_weight(FontWeight::MEDIUM)
                })
                .when(!is_selected, |row| row.hover(|s| s.bg(theme.list_hover)))
                .child(div().flex_1().min_w_0().truncate().child(entry.name))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(entry.detail),
                )
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    on_select(view, name.clone(), window, cx);
                }));
            match on_drop {
                Some(on_drop) => row
                    .drag_over::<Dragged>(|style, dragged, _, cx| {
                        if matches!(dragged.source, DragSource::Market) {
                            style.bg(cx.theme().drop_target)
                        } else {
                            style
                        }
                    })
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
pub struct ItemTree {
    pub search: Entity<InputState>,
    accepts: fn(&ItemType) -> bool,
    /// Market groups holding something accepted, themselves or below.
    useful: HashSet<i32>,
    roots: Vec<i32>,
    expanded: HashSet<i32>,
}

impl ItemTree {
    /// A tree with its top groups open.
    pub fn new(
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

    pub fn toggle(&mut self, group: i32) {
        if !self.expanded.remove(&group) {
            self.expanded.insert(group);
        }
    }

    /// The search box and the tree or the search hits under it. `tree` finds this tree in the
    /// view, `add` takes a double-clicked item.
    pub fn render<V: 'static>(
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
            let hits = catalog
                .search(&query, SEARCH_LIMIT)
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
        let theme = cx.theme();
        out.push(
            h_flex()
                .id(SharedString::from(format!("tree-group-{id}")))
                .gap_1()
                .h(px(TREE_ROW))
                .flex_none()
                .pl(px(INDENT * depth as f32))
                .pr_1()
                .rounded(px(4.))
                .hover(|s| s.bg(theme.list_hover))
                .child(
                    UiIcon::new(if open {
                        AssetIcon::ChevronDown
                    } else {
                        AssetIcon::ChevronRight
                    })
                    .size(px(14.))
                    .text_color(theme.muted_foreground),
                )
                .children(
                    group
                        .icon_id
                        .map(|icon| icon_box(catalog, &loaded.icons, Icon::Id(icon), 18.)),
                )
                .child(div().min_w_0().truncate().child(group.name.clone()))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    tree(view).toggle(id);
                    cx.notify();
                }))
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
    let theme = cx.theme();
    let row = h_flex()
        .id(SharedString::from(format!("tree-item-{type_id}")))
        .gap_2()
        .h(px(TREE_ROW))
        .flex_none()
        .pl(px(INDENT * depth as f32 + 4.))
        .pr_1()
        .rounded(px(4.))
        .hover(|s| s.bg(theme.list_hover))
        .child(icon_box(catalog, &loaded.icons, Icon::Type(type_id), 20.))
        .child(div().min_w_0().truncate().child(catalog.name(type_id)))
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
