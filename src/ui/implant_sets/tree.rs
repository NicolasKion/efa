//! The implant tree: every implant by slot and the pirate and faction sets, searched and folded, to
//! drag onto the chosen set.

use std::{collections::HashSet, ops::Range, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, Selectable, Sizable, button::Button, h_flex, input::Input, v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
    px, uniform_list,
};

use super::ImplantSetEditor;
use crate::ui::{
    Loaded,
    drag::{DragPreview, DragSource, Dragged},
    tooltips,
    widgets::{DropHighlight, caption, count, empty_state, icon_box, tree_row},
};
use crate::{
    data::TypeId,
    fit::implant_slot,
    icons::Icon,
    implant_set::{SLOTS, SlotImplants},
};

/// Wide enough for the longest hardwiring names.
const TREE_W: f32 = 420.;
const TREE_ICON: f32 = 18.;

/// Which part of the implant tree is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotFilter {
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
pub(super) enum Fold {
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

/// Whether every word of a search is found in one of the texts. Both are lowercase already.
fn matches(words: &[String], texts: &[&str]) -> bool {
    words
        .iter()
        .all(|word| texts.iter().any(|text| text.contains(word.as_str())))
}

/// A kind of implant with its names in lowercase, worked out once for the search.
struct SearchGroup {
    name: String,
    lower: String,
    implants: Vec<(TypeId, String)>,
}

/// The implants of a slot, ready to search.
pub(super) struct SearchSlot {
    slot: u8,
    groups: Vec<SearchGroup>,
}

impl SearchSlot {
    pub(super) fn index(catalog: &crate::data::Catalog, slots: Vec<SlotImplants>) -> Vec<Self> {
        let lower = |t: TypeId| catalog.get(t).map_or_default(|t| t.name.to_lowercase());
        slots
            .into_iter()
            .map(|s| Self {
                slot: s.slot,
                groups: s
                    .groups
                    .into_iter()
                    .map(|g| SearchGroup {
                        lower: g.name.to_lowercase(),
                        implants: g.implants.iter().map(|&t| (t, lower(t))).collect(),
                        name: g.name,
                    })
                    .collect(),
            })
            .collect()
    }
}

/// One grade of a pirate or faction set, with its name in lowercase for the search.
pub(super) struct PirateSet {
    pub(super) name: SharedString,
    pub(super) lower: String,
    pub(super) implants: Rc<[TypeId]>,
}

/// A drag of these implants out of the tree, shown under `label` when there are several.
fn tree_drag(loaded: &Rc<Loaded>, implants: &[TypeId], label: Option<&str>) -> Option<Dragged> {
    let (&first, rest) = implants.split_first()?;
    let dragged = Dragged::of(loaded, DragSource::Market, Some(first))?.with_more(rest.to_vec());
    Some(match label {
        Some(label) => dragged.labelled(label.to_owned()),
        None => dragged,
    })
}

impl ImplantSetEditor {
    /// The tree's lines for the search and slot filter. A search shows everything it finds,
    /// folded or not.
    fn tree_rows(&self, query: &str) -> Vec<TreeRow> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let searching = !words.is_empty();
        let open = |fold: &Fold| searching || !self.folded.contains(fold);
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
                        .filter(|(_, name)| matches(&words, &[name, &g.lower]))
                        .map(|&(t, _)| t)
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
            let sets: Vec<&PirateSet> = self
                .pirate_sets
                .iter()
                .filter(|set| matches(&words, &[&set.lower, "set"]))
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
                    rows.extend(sets.into_iter().map(|set| TreeRow::Set {
                        name: set.name.clone(),
                        implants: Rc::clone(&set.implants),
                    }));
                }
            }
        }
        rows
    }

    pub(super) fn tree_column(&self, cx: &mut Context<Self>) -> AnyElement {
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
        // Narrow, it shares the room with the set instead of taking a fixed width.
        v_flex()
            .id("implant-tree-column")
            .when(self.narrow, |d| d.flex_1().min_w_0().max_w(px(TREE_W)))
            .when(!self.narrow, |d| d.w(px(TREE_W)).flex_none())
            .h_full()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            // An implant of the set dragged back here comes out of the set.
            .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Listed))
            .child(
                h_flex()
                    .justify_between()
                    .child(caption("Implants", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(count(found, "implant", "implants")),
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
        let muted = cx.theme().muted_foreground;
        let heading = |fold: &Fold,
                       depth: usize,
                       title: &SharedString,
                       count: usize,
                       open: bool,
                       cx: &mut Context<Self>| {
            let fold = fold.clone();
            tree_row(
                SharedString::from(format!("tree-{fold:?}")),
                depth,
                Some(open),
                None,
                title.clone(),
                false,
                cx,
            )
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
            } => heading(fold, 0, title, *count, *open, cx)
                .font_weight(FontWeight::SEMIBOLD)
                .into_any_element(),
            TreeRow::Group {
                fold,
                title,
                count,
                open,
            } => heading(fold, 1, title, *count, *open, cx).into_any_element(),
            TreeRow::Item(id) => self.tree_item(*id, in_set.contains(id), cx),
            TreeRow::Set { name, implants } => self.tree_set(name, implants, cx),
        }
    }

    fn tree_item(&self, id: TypeId, in_set: bool, cx: &mut Context<Self>) -> AnyElement {
        let loaded = Rc::clone(&self.loaded);
        let catalog = &loaded.catalog;
        let price = self.prices(cx).get(id).filter(|&p| p > 0.0);
        let tip = Rc::clone(&loaded);
        let icon = icon_box(catalog, &loaded.icons, Icon::Type(id), TREE_ICON).into_any_element();
        let primary = cx.theme().primary;
        let row = tree_row(
            SharedString::from(format!("tree-item-{id}")),
            2,
            None,
            Some(icon),
            catalog.name(id),
            false,
            cx,
        )
        // Small enough for the longest hardwiring names to fit.
        .text_xs()
        .when(in_set, |row| {
            row.child(
                UiIcon::new(AssetIcon::Check)
                    .size(px(12.))
                    .text_color(primary),
            )
        })
        .tooltip(move |window, cx| tooltips::item_tooltip(&tip, id, price, window, cx))
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
        let mut lines = vec![name.to_string(), String::new()];
        lines.extend(
            implants
                .iter()
                .map(|&t| format!("Slot {}: {}", implant_slot(catalog, t), catalog.name(t))),
        );
        lines.push(String::new());
        lines.push("Drag onto the set or double-click to add every piece.".to_owned());
        let add = Rc::clone(implants);
        let icon = implants.first().map(|&t| {
            icon_box(catalog, &loaded.icons, Icon::Type(t), TREE_ICON).into_any_element()
        });
        let muted = cx.theme().muted_foreground;
        let row = tree_row(
            SharedString::from(format!("tree-set-{name}")),
            1,
            None,
            icon,
            name.clone(),
            false,
            cx,
        )
        .text_xs()
        .child(
            div()
                .flex_none()
                .text_color(muted)
                .child(count(implants.len(), "piece", "pieces")),
        )
        .tooltip(move |window, cx| tooltips::text_tooltip(lines.clone(), window, cx))
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
            &["high-grade snake alpha", "snake set"]
        ));
        assert!(matches(
            &words("ARMOR noble"),
            &["'noble' repair", "armor implants"]
        ));
        assert!(!matches(
            &words("snake beta"),
            &["high-grade snake alpha", "snake set"]
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
