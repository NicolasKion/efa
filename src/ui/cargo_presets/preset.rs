//! The chosen preset's column: its items with their quantities and volumes, and what it does to the
//! hold.

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::{CargoPresetEditor, item_count};
use crate::ui::{
    additions::amount,
    drag::{DragPreview, DragSource, Dragged},
    editor_kit::{FOLD_W, MIDDLE_MIN_W},
    widgets::{DropHighlight, TipExt, caption, empty_state, icon_box},
};
use crate::{
    cargo_preset::{self, CargoPolicy, CargoPreset},
    data::TypeId,
    icons::Icon,
};

/// Below this window width the rows drop their volume column; the header still has the total.
const COMPACT_W: f32 = 640.;
const ITEM_ROW_H: f32 = 30.;
const QUANTITY_W: f32 = 84.;
const VOLUME_W: f32 = 110.;

impl CargoPresetEditor {
    /// The quantity field of an item, made on first use and kept in step with the preset while
    /// it is not being typed in.
    fn quantity_input(
        &mut self,
        type_id: TypeId,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some((input, _)) = self.quantities.get(&type_id) {
            let input = input.clone();
            let typing = input.read(cx).focus_handle(cx).is_focused(window);
            if !typing && input.read(cx).value() != count.to_string().as_str() {
                input.update(cx, |i, cx| i.set_value(count.to_string(), window, cx));
            }
            return input;
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(count.to_string()));
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |e: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    e.commit_quantity(type_id, window, cx);
                }
            },
        );
        self.quantities
            .insert(type_id, (input.clone(), subscription));
        input
    }

    fn item_row(
        &mut self,
        n: usize,
        type_id: TypeId,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = self.quantity_input(type_id, count, window, cx);
        let loaded = Rc::clone(&self.loaded);
        let catalog = &loaded.catalog;
        let unit = catalog.get(type_id).map_or(0.0, |t| t.volume);
        let picked = self.picked == Some(type_id);
        let compact = self.compact(window);
        let theme = cx.theme();
        let row = h_flex()
            .id(SharedString::from(format!("preset-item-{type_id}")))
            .gap_3()
            .h(px(ITEM_ROW_H))
            .flex_none()
            .px_2()
            .when(n % 2 == 1 && !picked, |r| r.bg(theme.table_even))
            .when(picked, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            // Dropped on its own row, a market item adds to the count like anywhere else.
            .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Market))
            .child(icon_box(catalog, &loaded.icons, Icon::Type(type_id), 24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(catalog.name(type_id)),
            )
            .child(
                // A press in the field is the field's: it must not start dragging the row or
                // end in the row's click, which takes the focus back.
                div()
                    .id(SharedString::from(format!("preset-quantity-{type_id}")))
                    .w(px(QUANTITY_W))
                    .flex_none()
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(Input::new(&input).small()),
            )
            .when(!compact, |row| {
                row.child(
                    div()
                        .w(px(VOLUME_W))
                        .flex_none()
                        .text_right()
                        .text_color(theme.muted_foreground)
                        .child(format!("{} m³", amount(unit * f64::from(count)))),
                )
            })
            .child(
                Button::new(SharedString::from(format!("preset-remove-{type_id}")))
                    .ghost()
                    .xsmall()
                    .icon(UiIcon::new(AssetIcon::X))
                    .tooltip("Remove from preset")
                    .on_click(cx.listener(move |e, _, _, cx| e.remove_item(type_id, cx))),
            )
            .on_click(cx.listener(move |e, _: &ClickEvent, window, cx| {
                e.picked = Some(type_id);
                window.focus(&e.focus, cx);
                cx.notify();
            }));
        match Dragged::of(&loaded, DragSource::Listed, Some(type_id)) {
            Some(dragged) => row
                .on_drag(dragged, |dragged, _, _, cx| {
                    cx.new(|_| DragPreview(dragged.clone()))
                })
                .into_any_element(),
            None => row.into_any_element(),
        }
    }

    /// Whether the preset's rows drop their volume column: in a small window, or beside an
    /// open market in a narrow one.
    fn compact(&self, window: &Window) -> bool {
        let width = window.viewport_size().width;
        width < px(COMPACT_W) || (width < px(FOLD_W) && self.market_open)
    }

    pub(super) fn preset_column(
        &mut self,
        preset: Option<&CargoPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(preset) = preset else {
            return v_flex()
                .flex_1()
                .min_w_0()
                .justify_center()
                .child(empty_state(
                    AssetIcon::Package,
                    "No preset selected",
                    "Pick a preset on the left, or click New to make one.",
                    cx,
                ))
                .into_any_element();
        };
        let catalog = &self.loaded.catalog;
        let mut items = preset.items.clone();
        items.sort_by_cached_key(|&(t, _)| catalog.name(t));
        let volume = cargo_preset::volume(catalog, &preset.items);
        let units: u32 = items.iter().fold(0, |sum, &(_, n)| sum.saturating_add(n));
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(n, &(t, count))| self.item_row(n, t, count, window, cx))
            .collect();
        let has_fit = self
            .efa
            .upgrade()
            .is_some_and(|g| g.read(cx).fit().is_some());
        let narrow = window.viewport_size().width < px(FOLD_W);
        let compact = self.compact(window);
        let theme = cx.theme();
        let policy = preset.policy;
        // Everything but applying sits behind the gear: the column is for the items.
        let settings = Button::new("preset-settings")
            .ghost()
            .small()
            .icon(UiIcon::new(AssetIcon::Settings2))
            .tooltip("When applied, and adding the fit's cargo")
            .dropdown_menu({
                let this = cx.weak_entity();
                move |menu, _, _| {
                    let mut menu = menu.label("When applied");
                    for value in CargoPolicy::ALL {
                        let this = this.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!(
                                "{}: {}",
                                value.label(),
                                value.explanation()
                            ))
                            .checked(value == policy)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |e, cx| {
                                    e.edit_current(cx, |p| p.policy = value);
                                });
                            }),
                        );
                    }
                    let this = this.clone();
                    menu.separator().item(
                        PopupMenuItem::new("Add from fit's cargo")
                            .disabled(!has_fit)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, CargoPresetEditor::add_fit_cargo);
                            }),
                    )
                }
            });
        let market = narrow.then(|| {
            Button::new("preset-market")
                .small()
                .ghost()
                .icon(UiIcon::new(AssetIcon::Store))
                .when(self.market_open, |b| b.selected(true))
                .tooltip("Show or hide the market")
                .on_click(cx.listener(|e, _, _, cx| {
                    e.market_open = !e.market_open;
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
                            .child(preset.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(
                                div()
                                    .id("preset-policy")
                                    .px_1p5()
                                    .rounded(px(4.))
                                    .bg(theme.secondary)
                                    .text_color(theme.foreground)
                                    .child(policy.label())
                                    .tip(policy.explanation()),
                            )
                            .child(div().truncate().child(format!(
                                "{}, {units} units, {} m³",
                                item_count(items.len()),
                                amount(volume)
                            ))),
                    ),
            )
            .children(market)
            .child(settings)
            .child(
                Button::new("preset-apply")
                    .small()
                    .primary()
                    .icon(UiIcon::new(AssetIcon::PackageCheck))
                    .label("Apply")
                    .disabled(!has_fit || preset.items.is_empty())
                    .tooltip("Put this preset into the current fit's cargo")
                    .on_click(cx.listener(|e, _, _, cx| e.apply_to_fit(cx))),
            );
        let list = if rows.is_empty() {
            vec![
                empty_state(
                    AssetIcon::PackagePlus,
                    "This preset is empty",
                    "Drag items here from the market on the right, double-click them there, \
                     or add the current fit's cargo.",
                    cx,
                )
                .into_any_element(),
            ]
        } else {
            rows
        };
        let notice = self.notice.clone();
        v_flex()
            .id("preset-column")
            .flex_1()
            .min_w(px(if narrow { MIDDLE_MIN_W } else { 0. }))
            .h_full()
            .gap_2()
            .p_3()
            .drop_highlight_if::<Dragged>(|d| matches!(d.source, DragSource::Market))
            .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                if matches!(dragged.source, DragSource::Market) {
                    for t in dragged.types() {
                        e.add_item(t, 1, cx);
                    }
                }
            }))
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_3()
                            .px_2()
                            .child(div().flex_1().child(caption("Items", cx)))
                            .child(div().w(px(QUANTITY_W)).child(caption("Quantity", cx)))
                            .when(!compact, |row| {
                                row.child(
                                    div()
                                        .w(px(VOLUME_W))
                                        .flex()
                                        .justify_end()
                                        .child(caption("Volume", cx)),
                                )
                            })
                            .child(div().w(px(22.))),
                    )
                    .child(
                        v_flex()
                            .id("preset-items")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(list),
                    ),
            )
            .when_some(notice, |col, notice| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(notice),
                )
            })
            .into_any_element()
    }
}
