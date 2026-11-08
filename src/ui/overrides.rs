//! pyfa's attribute override editor (gui/propertyEditor.py) as a window of its own: pick a type
//! from the market search or the list of overridden ones, and type a new base value next to any
//! of its attributes. Every change goes to the hub, which saves it and rebuilds the engine's data
//! while overrides are on.

use std::rc::Rc;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, ParentElement, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder, px,
};

use super::{
    Efa, Hub, Loaded,
    dialogs::{Confirm, confirm},
    editor_kit::{CONTEXT, FocusSearch, Sidebar},
    item_data,
    preferences::section,
    widgets::{empty_line, icon_box, list_row, parse_decimal, redraw_on_change},
};
use crate::{data::TypeId, icons::Icon, item_data::ItemData, overrides::Overrides};

/// Most search hits the list shows, so a short query stays cheap to draw.
const SEARCH_LIMIT: usize = 200;
/// Below this width the item list narrows and the attributes sit closer to the edges.
const NARROW_W: f32 = 760.;
/// What an attribute's name keeps beside its field before the field drops under it.
const NAME_MIN_W: f32 = 160.;
const FIELD_W: f32 = 150.;

/// An attribute of the selected type and the field its override is typed into.
struct Field {
    attr: i32,
    name: String,
    title: String,
    base: f64,
    input: Entity<InputState>,
}

pub struct OverridesWindow {
    hub: Entity<Hub>,
    loaded: Rc<Loaded>,
    search: Entity<InputState>,
    selected: Option<TypeId>,
    /// The selected type's attributes, by name, once the item data is in.
    fields: Vec<Field>,
    /// A message under the header, and whether it is an error.
    notice: Option<(SharedString, bool)>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    field_subscriptions: Vec<Subscription>,
}

impl Efa {
    /// pyfa's Global > Attribute Overrides.
    pub(super) fn open_overrides(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let hub = self.hub.clone();
        let opening = crate::ui::chrome::open_single(
            "Attribute Overrides",
            (960., 640.),
            cx,
            |_: &mut OverridesWindow, _, _| {},
            move |window, cx| OverridesWindow::new(hub, loaded, window, cx),
        );
        Self::report_open(opening, cx);
    }

    /// pyfa's Global > Turn Overrides On/Off.
    pub(super) fn toggle_overrides(&mut self, cx: &mut Context<Self>) {
        self.hub.update(cx, |hub, cx| {
            hub.edit_overrides(|o| o.enabled = !o.enabled, cx);
        });
    }
}

impl OverridesWindow {
    fn new(
        hub: Entity<Hub>,
        loaded: Rc<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search items"));
        // Starts loading the attribute names if no stats window did yet.
        item_data::hover_data(&loaded, cx);
        let subscriptions = vec![
            cx.observe(&hub, |_, _, cx| cx.notify()),
            item_data::observe_data(cx),
            redraw_on_change(cx, &search),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            hub,
            loaded,
            search,
            selected: None,
            fields: Vec::new(),
            notice: None,
            focus,
            _subscriptions: subscriptions,
            field_subscriptions: Vec::new(),
        }
    }

    fn overrides<'a>(&self, cx: &'a gpui_kit::App) -> &'a Overrides {
        &self.hub.read(cx).saved.settings.overrides
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Overrides)) {
        self.hub.update(cx, |hub, cx| hub.edit_overrides(edit, cx));
    }

    fn select(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        if self.selected != Some(type_id) {
            self.selected = Some(type_id);
            self.fields.clear();
            self.field_subscriptions.clear();
            self.notice = None;
        }
        cx.notify();
    }

    /// One field per attribute of the selected type, made once its data is in.
    fn ensure_fields(&mut self, data: &ItemData, window: &mut Window, cx: &mut Context<Self>) {
        let Some(type_id) = self.selected else { return };
        if !self.fields.is_empty() {
            return;
        }
        let Some(info) = data.type_info(type_id) else {
            return;
        };
        let mut fields: Vec<(i32, String, String, f64)> = info
            .attrs
            .iter()
            .map(|&(attr, base)| {
                let a = data.attr(attr);
                let name = a.map_or_else(|| attr.to_string(), |a| a.name.clone());
                let title = a
                    .and_then(|a| a.display_name.clone())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| name.clone());
                (attr, name, title, base)
            })
            .collect();
        // pyfa sorts them by name.
        fields.sort_by_key(|a| a.1.to_lowercase());
        let values: Vec<String> = {
            let overrides = self.overrides(cx);
            fields
                .iter()
                .map(|f| overrides.get(type_id, f.0).map_or_else(String::new, number))
                .collect()
        };
        for ((attr, name, title, base), value) in fields.into_iter().zip(values) {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(number(base))
                    .default_value(value)
            });
            self.field_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let text = input.read(cx).value().to_string();
                        this.commit(type_id, attr, base, &text, input, window, cx);
                    }
                },
            ));
            self.fields.push(Field {
                attr,
                name,
                title,
                base,
                input,
            });
        }
    }

    /// Take what was typed for an attribute: a number overrides it, nothing or its own base
    /// value drops the override.
    #[expect(
        clippy::too_many_arguments,
        reason = "a subscription callback's context, passed on"
    )]
    fn commit(
        &mut self,
        type_id: TypeId,
        attr: i32,
        base: f64,
        text: &str,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.trim();
        let current = self.overrides(cx).get(type_id, attr);
        if text.is_empty() {
            self.edit(cx, |o| {
                o.remove(type_id, attr);
            });
            return;
        }
        match parse_decimal::<f64>(text) {
            Some(value) if value.is_finite() => {
                self.notice = None;
                // Typed back exactly as shown, so an exact match is what counts.
                if value.to_bits() == base.to_bits() {
                    self.edit(cx, |o| {
                        o.remove(type_id, attr);
                    });
                    input.update(cx, |i, cx| i.set_value("", window, cx));
                } else if current != Some(value) {
                    self.edit(cx, |o| o.set(type_id, attr, value));
                }
            }
            _ => {
                self.notice = Some((format!("\"{text}\" is not a number.").into(), true));
                let back = current.map_or_else(String::new, number);
                input.update(cx, |i, cx| i.set_value(back, window, cx));
                cx.notify();
            }
        }
    }

    fn reset(&mut self, attr: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(type_id) = self.selected else { return };
        self.edit(cx, |o| {
            o.remove(type_id, attr);
        });
        if let Some(field) = self.fields.iter().find(|f| f.attr == attr) {
            field.input.update(cx, |i, cx| i.set_value("", window, cx));
        }
    }

    /// pyfa's "Remove Overrides for Item".
    fn reset_type(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(type_id) = self.selected else { return };
        self.edit(cx, |o| o.clear_type(type_id));
        for field in &self.fields {
            field.input.update(cx, |i, cx| i.set_value("", window, cx));
        }
    }

    fn confirm_clear(window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        let question = Confirm {
            yes: "Delete All".into(),
            ..Confirm::delete("Are you sure you want to delete all overrides?")
        };
        confirm(window, cx, question, move |_, cx| {
            let _ = this.update(cx, |this, cx| {
                this.edit(cx, |o| o.values.clear());
                this.fields.clear();
                this.field_subscriptions.clear();
                cx.notify();
            });
        });
    }

    /// Read a pyfa override file.
    fn import(cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let notice = match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let mut overrides = this.overrides(cx).clone();
                        match overrides.import_csv(&text) {
                            Ok(count) => {
                                this.edit(cx, |o| *o = overrides);
                                (format!("Imported {count} overrides."), false)
                            }
                            Err(e) => (format!("Not a pyfa override file, {e}"), true),
                        }
                    }
                    Err(e) => (format!("{}: {e}", path.display()), true),
                };
                this.notice = Some((notice.0.into(), notice.1));
                this.fields.clear();
                this.field_subscriptions.clear();
                cx.notify();
            });
        })
        .detach();
    }

    /// Write the overrides as a pyfa override file.
    fn export(&self, cx: &mut Context<Self>) {
        let csv = self.overrides(cx).to_csv();
        let dir = crate::paths::home_dir();
        let chosen = cx.prompt_for_new_path(&dir, Some("pyfa_overrides.csv"));
        cx.background_spawn(async move {
            if let Ok(Ok(Some(path))) = chosen.await
                && let Err(e) = std::fs::write(&path, csv)
            {
                eprintln!("{}: {e}", path.display());
            }
        })
        .detach();
    }

    /// A type in the list on the left.
    fn type_row(&self, type_id: TypeId, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let primary = cx.theme().primary;
        let catalog = &self.loaded.catalog;
        let selected = self.selected == Some(type_id);
        list_row(
            SharedString::from(format!("override-type-{type_id}")),
            selected,
            cx,
        )
        .gap_2()
        .px_2()
        .child(icon_box(
            catalog,
            &self.loaded.icons,
            Icon::Type(type_id),
            20.,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(catalog.name(type_id)),
        )
        .when(count > 0, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(primary)
                    .child(count.to_string()),
            )
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.select(type_id, cx);
            window.focus(&this.focus, cx);
        }))
        .into_any_element()
    }

    fn sidebar(&self, narrow: bool, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).value().trim().to_owned();
        let searching = query.chars().filter(|c| !matches!(c, '*' | '?')).count() >= 3;
        let catalog = &self.loaded.catalog;
        // What the rows need, read before they are built.
        let (enabled, any, listed): (bool, bool, Vec<(TypeId, usize)>) = {
            let overrides = self.overrides(cx);
            let types = if searching {
                crate::market::search(catalog, &query, SEARCH_LIMIT)
            } else {
                let mut types: Vec<TypeId> = overrides.values.keys().copied().collect();
                types.sort_by_key(|&t| catalog.name(t));
                types
            };
            let listed = types
                .into_iter()
                .map(|t| (t, overrides.of(t).map_or(0, std::collections::BTreeMap::len)))
                .collect();
            (overrides.enabled, !overrides.values.is_empty(), listed)
        };
        let rows: Vec<AnyElement> = listed
            .into_iter()
            .map(|(t, count)| self.type_row(t, count, cx))
            .collect();
        let empty = if searching {
            "Nothing matches."
        } else {
            "No overrides yet. Search for an item to override its attributes."
        };
        let switch = Switch::new("overrides-enabled")
            .checked(enabled)
            .on_click(cx.listener(|this, value: &bool, _, cx| {
                let value = *value;
                this.edit(cx, |o| o.enabled = value);
            }));
        let muted = cx.theme().muted_foreground;
        let button = |id: &'static str, icon: AssetIcon, label: &'static str| {
            Button::new(id)
                .small()
                .ghost()
                .icon(UiIcon::new(icon))
                .when(narrow, |b| b.tooltip(label))
                .when(!narrow, |b| b.label(label))
        };
        let footer = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        // The text wraps beside the switch instead of pushing it out.
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().font_weight(FontWeight::MEDIUM).child("Use overrides"))
                            .child(div().text_xs().text_color(muted).child(
                                "Overridden values replace the base values in every fit.",
                            )),
                    )
                    .child(div().flex_none().child(switch)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        button("overrides-import", AssetIcon::FileUp, "Import")
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| Self::import(cx))),
                    )
                    .child(
                        button("overrides-export", AssetIcon::FileDown, "Export")
                            .disabled(!any)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.export(cx))),
                    )
                    .child(
                        button("overrides-clear", AssetIcon::Trash, "Clear All")
                            .disabled(!any)
                            .on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                                Self::confirm_clear(window, cx);
                            })),
                    ),
            );
        Sidebar {
            caption: if searching {
                "Results"
            } else {
                "Overridden items"
            }
            .into(),
            search: Some(self.search.clone()),
            list_id: "override-types",
            rows,
            empty: Some(empty_line(empty, cx).text_xs().into_any_element()),
            footer: Some(footer.into_any_element()),
            narrow,
        }
        .render(cx)
    }

    /// An attribute: its name and base value, the field for its override, and a reset.
    fn field_row(field: &Field, value: Option<f64>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let attr = field.attr;
        let description = if field.title == field.name {
            format!("Base value {}", number(field.base))
        } else {
            format!("{}, base value {}", field.name, number(field.base))
        };
        // The field drops under the attribute's name when the two don't fit on a line.
        h_flex()
            .flex_wrap()
            .gap_x_4()
            .gap_y_1()
            .px_4()
            .py_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(NAME_MIN_W))
                    .gap_0p5()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .when(value.is_some(), |d| d.text_color(theme.primary))
                            .child(field.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(description),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_4()
                    .child(
                        div()
                            .flex_none()
                            .w(px(FIELD_W))
                            .child(Input::new(&field.input).small()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("override-reset-{attr}")))
                            .xsmall()
                            .ghost()
                            .icon(UiIcon::new(AssetIcon::RotateCcw))
                            .tooltip("Back to the base value")
                            .disabled(value.is_none())
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.reset(attr, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn body(&mut self, narrow: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let muted = |text: &'static str, cx: &Context<Self>| empty_line(text, cx).into_any_element();
        let Some(type_id) = self.selected else {
            return muted("Pick an item on the left to see its attributes.", cx);
        };
        let data = match item_data::data(cx) {
            Some(Ok(data)) => data,
            Some(Err(e)) => {
                return empty_line(e, cx)
                    .text_color(cx.theme().danger)
                    .into_any_element();
            }
            None => return muted("Loading attributes…", cx),
        };
        self.ensure_fields(&data, window, cx);
        // What the rows need, read before they are built.
        let (count, enabled, values): (usize, bool, Vec<Option<f64>>) = {
            let overrides = self.overrides(cx);
            (
                overrides
                    .of(type_id)
                    .map_or(0, std::collections::BTreeMap::len),
                overrides.enabled,
                self.fields
                    .iter()
                    .map(|f| overrides.get(type_id, f.attr))
                    .collect(),
            )
        };
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let summary = match (count, enabled) {
            (0, _) => "No attributes overridden.".to_owned(),
            (n, true) => format!("{n} overridden, in effect."),
            (n, false) => format!("{n} overridden, not in effect while overrides are off."),
        };
        let header = h_flex()
            .gap_3()
            .when(narrow, |d| d.px_4().pt_4().pb_3())
            .when(!narrow, |d| d.px_8().pt_6().pb_4())
            .child(icon_box(
                catalog,
                &self.loaded.icons,
                Icon::Type(type_id),
                36.,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(catalog.name(type_id)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(summary),
                    ),
            )
            .child(
                // Narrow, the button keeps only its icon.
                Button::new("overrides-reset-type")
                    .small()
                    .icon(UiIcon::new(AssetIcon::ListRestart))
                    .when(!narrow, |b| b.label("Remove Overrides for Item"))
                    .when(narrow, |b| b.tooltip("Remove Overrides for Item"))
                    .disabled(count == 0)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.reset_type(window, cx);
                    })),
            );
        let rows: Vec<AnyElement> = self
            .fields
            .iter()
            .zip(values)
            .map(|(f, value)| Self::field_row(f, value, cx))
            .collect();
        let content = if rows.is_empty() {
            muted("This item has no attributes.", cx)
        } else {
            section("Attributes", rows, cx)
        };
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(header)
            .when_some(self.notice.clone(), |d, (notice, error)| {
                let theme = cx.theme();
                d.child(
                    div()
                        .when(narrow, gpui_kit::Styled::px_4)
                        .when(!narrow, gpui_kit::Styled::px_8)
                        .pb_2()
                        .text_sm()
                        .text_color(if error {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(notice),
                )
            })
            .child(
                v_flex()
                    .id("override-attributes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(narrow, |d| d.px_4().pb_4())
                    .when(!narrow, |d| d.px_8().pb_8())
                    .child(content),
            )
            .into_any_element()
    }
}

impl Render for OverridesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < px(NARROW_W);
        let body = self.body(narrow, window, cx);
        h_flex()
            .size_full()
            .items_start()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.search.update(cx, |input, cx| {
                    input.focus(window, cx);
                    input.select_all(window, cx);
                });
            }))
            .child(self.sidebar(narrow, cx))
            // The attribute list takes the rest of the width and can shrink, so nothing in it
            // pushes past the window.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(body),
            )
    }
}

/// A value as typed: no trailing zeros, and every digit it has.
fn number(value: f64) -> String {
    format!("{value}")
}
