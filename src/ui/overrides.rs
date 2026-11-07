//! pyfa's attribute override editor (gui/propertyEditor.py) as a window of its own: pick a type
//! from the market search or the list of overridden ones, and type a new base value next to any
//! of its attributes. Every change goes to the hub, which saves it and rebuilds the engine's data
//! while overrides are on.

use std::{path::PathBuf, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, WindowBounds,
    WindowOptions, div, prelude::FluentBuilder, px, size,
};

use super::{Efa, Hub, Loaded, item_stats, preferences::section, widgets::icon_box};
use crate::{data::TypeId, icons::Icon, item_data::ItemData, overrides::Overrides};

/// Most search hits the list shows, so a short query stays cheap to draw.
const SEARCH_LIMIT: usize = 200;
const ROW_HEIGHT: f32 = 30.;
const SIDEBAR_W: f32 = 300.;
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
    _subscriptions: Vec<Subscription>,
    field_subscriptions: Vec<Subscription>,
}

impl Efa {
    /// pyfa's Global > Attribute Overrides.
    pub(super) fn open_overrides(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let hub = self.hub.clone();
        let weak = cx.entity().downgrade();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(960.), px(640.)), cx)),
            app_id: Some("efa".into()),
            ..Default::default()
        };
        cx.defer(move |cx| {
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("efa: Attribute Overrides");
                cx.new(|cx| OverridesWindow::new(hub, loaded, window, cx))
            });
            if let Err(e) = opened {
                Efa::app_update(&weak, cx, |g, cx| {
                    g.status = Some(format!("Could not open a window: {e}").into());
                    cx.notify();
                });
            }
        });
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
        item_stats::hover_data(&loaded, cx);
        let subscriptions = vec![
            cx.observe(&hub, |_, _, cx| cx.notify()),
            item_stats::observe_data(cx),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            hub,
            loaded,
            search,
            selected: None,
            fields: Vec::new(),
            notice: None,
            _subscriptions: subscriptions,
            field_subscriptions: Vec::new(),
        }
    }

    fn overrides<'a>(&self, cx: &'a gpui_kit::App) -> &'a Overrides {
        &self.hub.read(cx).saved.overrides
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
        let overrides = self.overrides(cx).clone();
        for (attr, name, title, base) in fields {
            let value = overrides
                .get(type_id, attr)
                .map_or_else(String::new, number);
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
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => {
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
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let yes = this.clone();
            dialog.title("Confirm Delete").w(px(380.)).child(
                v_flex()
                    .gap_3()
                    .child("Are you sure you want to delete all overrides?")
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("overrides-clear-no")
                                    .label("No")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                Button::new("overrides-clear-yes")
                                    .danger()
                                    .label("Delete All")
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        let _ = yes.update(cx, |this, cx| {
                                            this.edit(cx, |o| o.values.clear());
                                            this.fields.clear();
                                            this.field_subscriptions.clear();
                                            cx.notify();
                                        });
                                    }),
                            ),
                    ),
            )
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
        let dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
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
        let theme = cx.theme();
        let catalog = &self.loaded.catalog;
        let selected = self.selected == Some(type_id);
        h_flex()
            .id(SharedString::from(format!("override-type-{type_id}")))
            .gap_2()
            .h(px(ROW_HEIGHT))
            .px_2()
            .rounded(px(6.))
            .when(selected, |d| d.bg(theme.secondary))
            .when(!selected, |d| {
                d.hover(|s| s.bg(theme.secondary.opacity(0.5)))
            })
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
                        .text_color(theme.primary)
                        .child(count.to_string()),
                )
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.select(type_id, cx);
            }))
            .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let overrides = self.overrides(cx).clone();
        let query = self.search.read(cx).value().trim().to_owned();
        let searching = query.chars().filter(|c| !matches!(c, '*' | '?')).count() >= 3;
        let (heading, types): (&str, Vec<TypeId>) = if searching {
            ("RESULTS", self.loaded.catalog.search(&query, SEARCH_LIMIT))
        } else {
            let mut types: Vec<TypeId> = overrides.values.keys().copied().collect();
            types.sort_by_key(|&t| self.loaded.catalog.name(t));
            ("OVERRIDDEN ITEMS", types)
        };
        let rows: Vec<AnyElement> = types
            .iter()
            .map(|&t| {
                let count = overrides.of(t).map_or(0, std::collections::BTreeMap::len);
                self.type_row(t, count, cx)
            })
            .collect();
        let theme = cx.theme();
        let empty = if searching {
            "Nothing matches."
        } else {
            "No overrides yet. Search for an item to override its attributes."
        };
        let switch = Switch::new("overrides-enabled")
            .checked(overrides.enabled)
            .on_click(cx.listener(|this, value: &bool, _, cx| {
                let value = *value;
                this.edit(cx, |o| o.enabled = value);
            }));
        v_flex()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_none()
            .overflow_hidden()
            .px_3()
            .py_4()
            .gap_3()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .px_1()
                    .child(
                        // The text wraps beside the switch instead of pushing it out.
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().font_weight(FontWeight::MEDIUM).child("Use overrides"))
                            .child(
                                div().text_xs().text_color(theme.muted_foreground).child(
                                    "Overridden values replace the base values in every fit.",
                                ),
                            ),
                    )
                    .child(div().flex_none().child(switch)),
            )
            .child(Input::new(&self.search).small().cleanable(true))
            .child(
                div()
                    .px_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(heading),
            )
            .child(
                v_flex()
                    .id("override-types")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .when(rows.is_empty(), |d| {
                        d.child(
                            div()
                                .px_1()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(empty),
                        )
                    })
                    .children(rows),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("overrides-import")
                            .small()
                            .ghost()
                            .icon(UiIcon::new(AssetIcon::FileUp))
                            .label("Import")
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| Self::import(cx))),
                    )
                    .child(
                        Button::new("overrides-export")
                            .small()
                            .ghost()
                            .icon(UiIcon::new(AssetIcon::FileDown))
                            .label("Export")
                            .disabled(overrides.values.is_empty())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.export(cx))),
                    )
                    .child(
                        Button::new("overrides-clear")
                            .small()
                            .ghost()
                            .icon(UiIcon::new(AssetIcon::Trash))
                            .label("Clear All")
                            .disabled(overrides.values.is_empty())
                            .on_click(cx.listener(|_, _: &ClickEvent, window, cx| {
                                Self::confirm_clear(window, cx);
                            })),
                    ),
            )
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
        h_flex()
            .gap_4()
            .px_4()
            .py_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
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
            )
            .into_any_element()
    }

    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (muted_color, danger) = (cx.theme().muted_foreground, cx.theme().danger);
        let muted = |text: &'static str| {
            div()
                .p_8()
                .text_color(muted_color)
                .child(text)
                .into_any_element()
        };
        let Some(type_id) = self.selected else {
            return muted("Pick an item on the left to see its attributes.");
        };
        let data = match item_stats::data(cx) {
            Some(Ok(data)) => data,
            Some(Err(e)) => {
                return div().p_8().text_color(danger).child(e).into_any_element();
            }
            None => return muted("Loading attributes…"),
        };
        self.ensure_fields(&data, window, cx);
        let overrides = self.overrides(cx).clone();
        let count = overrides
            .of(type_id)
            .map_or(0, std::collections::BTreeMap::len);
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let summary = match (count, overrides.enabled) {
            (0, _) => "No attributes overridden.".to_owned(),
            (n, true) => format!("{n} overridden, in effect."),
            (n, false) => format!("{n} overridden, not in effect while overrides are off."),
        };
        let header = h_flex()
            .gap_3()
            .px_8()
            .pt_6()
            .pb_4()
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
                Button::new("overrides-reset-type")
                    .small()
                    .icon(UiIcon::new(AssetIcon::ListRestart))
                    .label("Remove Overrides for Item")
                    .disabled(count == 0)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.reset_type(window, cx);
                    })),
            );
        let rows: Vec<AnyElement> = self
            .fields
            .iter()
            .map(|f| Self::field_row(f, overrides.get(type_id, f.attr), cx))
            .collect();
        let content = if rows.is_empty() {
            muted("This item has no attributes.")
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
                        .px_8()
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
                    .px_8()
                    .pb_8()
                    .child(content),
            )
            .into_any_element()
    }
}

impl Render for OverridesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.body(window, cx);
        let theme = cx.theme();
        h_flex()
            .size_full()
            .items_start()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(self.sidebar(cx))
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
