//! The Implants tab: the character's implants, plugged in by drag, menu or implant set.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu},
    v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};

use super::{CharacterEditor, ROW_HEIGHT};
use crate::ui::{
    drag::{DragSource, Dragged},
    widgets::{DropHighlight, empty_line, entry, icon_box, icon_label},
};
use crate::{
    data::{ItemType, Kind, TypeId},
    icons::Icon,
};

impl CharacterEditor {
    fn add_implant(&mut self, implant: TypeId, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, loaded| c.add_implant(id, &loaded.catalog, implant));
    }

    fn remove_implant(&mut self, implant: TypeId, cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, _| c.remove_implant(id, implant));
    }

    fn apply_implant_set(&mut self, implants: &[TypeId], cx: &mut Context<Self>) {
        let id = self.selected;
        self.edit(cx, |c, loaded| {
            for &implant in implants {
                c.add_implant(id, &loaded.catalog, implant);
            }
        });
    }

    fn implant_row(
        &self,
        id: SharedString,
        implant: TypeId,
        trailing: Option<AnyElement>,
        cx: &Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let catalog = &self.loaded.catalog;
        let slot = catalog.get(implant).and_then(|t| t.implant_slot);
        h_flex()
            .id(id)
            .gap_2()
            .h(px(ROW_HEIGHT + 2.))
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
                Icon::Type(implant),
                20.,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(catalog.name(implant)),
            )
            .children(trailing)
    }

    pub(super) fn implants_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.selected.is_builtin() {
            return empty_line(
                "All 0 and All 5 have no implants. Pick or make another character.",
                cx,
            )
            .into_any_element();
        }
        let loaded = Rc::clone(&self.loaded);
        let available = self.implant_tree.render(
            &loaded,
            cx,
            |e: &mut Self| &mut e.implant_tree,
            |e: &mut Self, t, _, cx| e.add_implant(t, cx),
        );
        let plugged: Vec<AnyElement> = self
            .characters
            .implants(self.selected)
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                let remove = Button::new(SharedString::from(format!("implant-remove-{i}")))
                    .ghost()
                    .xsmall()
                    .label("×")
                    .tooltip("Remove implant")
                    .on_click(cx.listener(move |e, _, _, cx| e.remove_implant(t, cx)));
                self.implant_row(
                    SharedString::from(format!("plugged-{i}")),
                    t,
                    Some(remove.into_any_element()),
                    cx,
                )
                .on_click(cx.listener(move |e, event: &ClickEvent, _, cx| {
                    if event.click_count() >= 2 {
                        e.remove_implant(t, cx);
                    }
                }))
                .into_any_element()
            })
            .collect();
        let this = cx.weak_entity();
        let hub = self.hub.downgrade();
        let theme = cx.theme();
        h_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_1()
                    .child(div().flex_1().min_h_0().child(available))
                    .child(
                        div().text_xs().text_color(theme.muted_foreground).child(
                            "Double-click an implant or drag it to the right to plug it in.",
                        ),
                    ),
            )
            .child(
                v_flex()
                    .id("editor-plugged")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(theme.border)
                    .when(plugged.is_empty(), |pane| {
                        pane.child(empty_line("No implants plugged in.", cx))
                    })
                    .children(plugged)
                    .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Market))
                    .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                        if matches!(dragged.source, DragSource::Market) {
                            for t in dragged.types() {
                                e.add_implant(t, cx);
                            }
                        }
                    }))
                    // pyfa's implantEditor menu, with the sets as they are when it opens.
                    .context_menu(move |menu, window, cx| {
                        let Some(hub) = hub.upgrade() else {
                            return menu;
                        };
                        let sets: Vec<(String, Vec<TypeId>)> = hub
                            .read(cx)
                            .saved
                            .implant_sets
                            .iter()
                            .map(|s| (s.name.clone(), s.implants.clone()))
                            .collect();
                        apply_set_menu(menu, &this, sets, window, cx)
                    }),
            )
            .into_any_element()
    }
}

fn apply_set_menu(
    menu: PopupMenu,
    this: &gpui_kit::WeakEntity<CharacterEditor>,
    sets: Vec<(String, Vec<TypeId>)>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if sets.is_empty() {
        return menu;
    }
    let this = this.clone();
    menu.submenu_with_element(
        "Apply Implant Set",
        icon_label(IconName::Plus, "Apply Implant Set"),
        window,
        cx,
        move |menu, _, _| {
            sets.iter().fold(menu, |menu, (name, implants)| {
                let (this, implants) = (this.clone(), implants.clone());
                menu.item(
                    entry(IconName::Plus, name.clone()).on_click(move |_, _, cx| {
                        let _ = this.update(cx, |e, cx| e.apply_implant_set(&implants, cx));
                    }),
                )
            })
        },
    )
}

/// What the implants tab offers to plug in.
pub(super) fn is_implant(t: &ItemType) -> bool {
    t.kind == Kind::Implant
}
