//! Dragging items: what is dragged, the preview under the pointer, and drops on fitting rows.

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::{
    Context, Div, InteractiveElement, IntoElement, Modifiers, ParentElement, Render, SharedString,
    Stateful, Styled, Window, img, prelude::FluentBuilder, px,
};

use super::widgets::DropHighlight;
use super::{Action, Efa, Loaded, RowRef};
use crate::{
    data::{Kind, Slot, TypeId},
    icons::Icon,
};

/// Where a dragged item came from.
#[derive(Debug, Clone, Copy)]
pub enum DragSource {
    Market,
    Row(RowRef),
    /// A stack in the cargo tab.
    Cargo,
    /// A stack of the drones pane, by its index in the fit.
    Drone(usize),
    /// An entry of an editor's list, such as a cargo preset's item; dropped outside the list,
    /// it comes off.
    Listed,
}

/// An item being dragged: from the market onto a slot, or between slots. Rows hand one out
/// every time they are drawn, so it only names the item; the preview looks up its name and
/// icon once a drag starts.
#[derive(Clone)]
pub struct Dragged {
    pub source: DragSource,
    pub type_id: TypeId,
    /// Further items dragged along, from a selection in the market results.
    pub more: Vec<TypeId>,
    /// Shown instead of the item's name, such as the name of a set of items.
    label: Option<SharedString>,
    loaded: Rc<Loaded>,
}

impl Dragged {
    pub fn of(loaded: &Rc<Loaded>, source: DragSource, type_id: Option<TypeId>) -> Option<Self> {
        Some(Self {
            source,
            type_id: type_id?,
            more: Vec::new(),
            label: None,
            loaded: Rc::clone(loaded),
        })
    }

    /// Drag `more` along with the item.
    pub fn with_more(mut self, more: Vec<TypeId>) -> Self {
        self.more = more;
        self
    }

    /// Show `label` while dragging, such as the name of a set of items.
    pub fn labelled(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// What the preview says: the label, the count when several go, or the item's name.
    fn caption(&self) -> SharedString {
        match &self.label {
            Some(label) => label.clone(),
            None if !self.more.is_empty() => format!("{} items", self.more.len() + 1).into(),
            None => self.loaded.catalog.name(self.type_id).into(),
        }
    }

    /// Every item dragged, the grabbed one first.
    pub fn types(&self) -> impl Iterator<Item = TypeId> + '_ {
        std::iter::once(self.type_id).chain(self.more.iter().copied())
    }
}

/// What follows the pointer while dragging: the item's icon and name.
pub struct DragPreview(pub Dragged);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let loaded = &self.0.loaded;
        h_flex()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.ring)
            .shadow_md()
            .opacity(0.9)
            .when_some(
                loaded
                    .icons
                    .path(&loaded.catalog, Icon::Type(self.0.type_id)),
                |row, path| row.child(img(path).size(px(22.))),
            )
            .child(self.0.caption())
    }
}

/// A fitting row a drag can end on.
#[derive(Debug, Clone, Copy)]
pub(super) enum DropOn {
    Item(RowRef),
    Empty(Slot, usize),
}

impl DropOn {
    fn position(self) -> Option<(Slot, usize)> {
        match self {
            Self::Item(RowRef::Module(slot, i)) | Self::Empty(slot, i) => Some((slot, i)),
            Self::Item(RowRef::Rig(i)) => Some((Slot::Rig, i)),
            Self::Item(RowRef::Subsystem(_)) => None,
        }
    }

    fn row(self) -> Option<RowRef> {
        match self {
            Self::Item(row) => Some(row),
            Self::Empty(Slot::Rig, i) => Some(RowRef::Rig(i)),
            Self::Empty(slot @ (Slot::High | Slot::Mid | Slot::Low | Slot::Service), i) => {
                Some(RowRef::Module(slot, i))
            }
            Self::Empty(Slot::Subsystem, _) => None,
        }
    }
}

/// Highlight a row while something is dragged over it, and handle the drop.
pub(super) fn drop_target(row: Stateful<Div>, on: DropOn, cx: &mut Context<Efa>) -> Stateful<Div> {
    row.drop_highlight::<Dragged>()
        .on_drop(cx.listener(move |g, dragged: &Dragged, window, cx| {
            cx.stop_propagation();
            g.drop_on(on, dragged, window.modifiers(), cx);
        }))
}

impl Efa {
    /// pyfa's drop rules: a module onto its own rack goes to that position, a charge onto a
    /// module loads it, a module dragged within its rack swaps places, anything else is added.
    fn drop_on(
        &mut self,
        on: DropOn,
        dragged: &Dragged,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let Some(t) = loaded.catalog.get(dragged.type_id) else {
            return;
        };
        match dragged.source {
            // fittingView.py swapItems: Ctrl copies onto an empty slot, Ctrl+Alt fills the
            // rack with copies, no modifier swaps; other modifiers do nothing.
            DragSource::Row(from) => {
                let Some(to) = on.row() else { return };
                let (ctrl, alt) = (modifiers.control || modifiers.platform, modifiers.alt);
                match (ctrl, alt, modifiers.shift) {
                    (true, true, false) => {
                        if let RowRef::Module(slot, i) = from {
                            self.apply(Action::FillClone(slot, i), cx);
                        }
                    }
                    (true, false, false) if matches!(on, DropOn::Empty(..)) => {
                        self.apply(Action::Clone(from, to), cx);
                    }
                    (false, false, false) => self.apply(Action::Swap(from, to), cx),
                    _ => {}
                }
            }
            // Drone stacks only move within the drones pane.
            DragSource::Drone(_) | DragSource::Listed => {}
            // Several from the market: the grabbed one goes where it was dropped, the rest
            // wherever they fit, as a double-click would put them.
            DragSource::Market if !dragged.more.is_empty() => {
                let single = Dragged {
                    more: Vec::new(),
                    ..dragged.clone()
                };
                self.drop_on(on, &single, modifiers, cx);
                for &id in &dragged.more {
                    self.add_item(id, cx);
                }
            }
            DragSource::Market => match (t.kind, on.position()) {
                (Kind::Module(slot), Some((target, pos))) if slot == target => {
                    self.apply(Action::Place(slot, pos, t.id), cx);
                }
                (Kind::Charge, Some(_)) => match on {
                    DropOn::Item(row @ RowRef::Module(..)) => {
                        self.apply(Action::SetCharge(vec![row], Some(t.id)), cx);
                    }
                    _ => self.add_item(t.id, cx),
                },
                _ => self.add_item(t.id, cx),
            },
            DragSource::Cargo => {
                if let Some(onto) = on.position() {
                    let transfer = super::additions::drop_transfer(modifiers);
                    self.apply(Action::CargoToModule(t.id, onto, transfer), cx);
                }
            }
        }
    }
}
