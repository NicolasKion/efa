//! The window's layout: a toolbar with the graph and axis pickers, the plot in a card with the
//! settings column beside it, and the fit and target lists under both, pyfa's graph frame in
//! the preferences window's cards.

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, Sizable,
    button::Button,
    h_flex,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};

use super::{Field, GraphsWindow, canvas, lists, panel, vector};
use crate::{
    graphs::{
        ammo::{AmmoQuality, AmmoStyle},
        defs::{Checkbox as CheckboxKey, Graph, Unit, conditions_met},
        plot::DroneMode,
    },
    icons::Icon,
    ui::widgets::icon_box,
};

/// The settings column's width.
const PANEL_W: f32 = 320.;
/// The fit and target lists' height.
const LISTS_H: f32 = 230.;

impl Render for GraphsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let root = v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm();
        if self.loaded.is_none() {
            return root
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(theme.muted_foreground)
                .child(UiIcon::new(AssetIcon::ChartSpline).size(px(28.)))
                .child("Loading EVE data…")
                .into_any_element();
        }
        let lists = h_flex()
            .flex_none()
            .h(px(LISTS_H))
            .gap_4()
            .child(lists::sources(self, cx))
            .when(self.graph.has_targets(), |row| {
                row.child(lists::targets(self, cx))
            });
        root.child(self.toolbar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .p_4()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .gap_4()
                            .child(self.plot_card(cx))
                            .child(self.settings_panel(cx)),
                    )
                    .child(lists),
            )
            // A vector drag ends wherever the button comes up.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|g, _, _, _| g.drag_vector = None),
            )
            .into_any_element()
    }
}

impl GraphsWindow {
    /// The graph picker and the axis pickers, each under a small label.
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let x_labels: Vec<String> = self
            .x_defs()
            .iter()
            .map(|x| axis_choice(x.label, x.unit))
            .collect();
        let y_labels: Vec<String> = self
            .y_defs()
            .iter()
            .map(|y| axis_choice(y.selector_label(), y.unit))
            .collect();
        let (x, y) = (
            self.x.min(x_labels.len() - 1),
            self.y.min(y_labels.len() - 1),
        );
        let graph = self.graph_selector(cx);
        let y_axis = Self::axis_button(
            "axis-y",
            y_labels,
            y,
            |g, i, w, cx| {
                let x = g.x;
                g.set_axes(x, i, w, cx);
            },
            cx,
        );
        // pyfa offers no X choice for the Application Profile.
        let x_axis = (!self.graph.has_segments()).then(|| {
            Self::axis_button(
                "axis-x",
                x_labels,
                x,
                |g, i, w, cx| {
                    let y = g.y;
                    g.set_axes(i, y, w, cx);
                },
                cx,
            )
        });
        let theme = cx.theme();
        let tile = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(36.))
            .rounded(px(8.))
            .bg(theme.primary.opacity(0.15))
            .child(
                UiIcon::new(AssetIcon::ChartSpline)
                    .size(px(18.))
                    .text_color(theme.primary),
            );
        h_flex()
            .flex_none()
            .items_end()
            .gap_4()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(tile)
            .child(picker("Graph", graph, cx))
            .child(picker("Y axis", y_axis, cx))
            .when_some(x_axis, |bar, x_axis| {
                bar.child(picker("X axis", x_axis, cx))
            })
    }

    fn graph_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let current = self.graph;
        let graphs: Vec<Graph> = Graph::ALL
            .into_iter()
            .chain(self.include_hidden.then_some(Graph::EcmBurstDamps))
            .collect();
        Button::new("graph-select")
            .outline()
            .small()
            .w(px(240.))
            .dropdown_caret(true)
            .label(current.name())
            .dropdown_menu(move |menu, _, _| {
                graphs.iter().fold(menu, |menu, &graph| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(graph.name())
                            .checked(graph == current)
                            .on_click(move |_, window, cx| {
                                let _ = this.update(cx, |g, cx| g.set_graph(graph, window, cx));
                            }),
                    )
                })
            })
            .into_any_element()
    }

    fn axis_button(
        id: &'static str,
        labels: Vec<String>,
        current: usize,
        pick: fn(&mut Self, usize, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity().downgrade();
        let label = labels.get(current).cloned().unwrap_or_default();
        Button::new(id)
            .outline()
            .small()
            .w(px(240.))
            .dropdown_caret(true)
            .label(label)
            .dropdown_menu(move |menu, _, _| {
                labels.iter().enumerate().fold(menu, |menu, (i, text)| {
                    let this = this.clone();
                    menu.item(
                        PopupMenuItem::new(text.clone())
                            .checked(i == current)
                            .on_click(move |_, window, cx| {
                                let _ = this.update(cx, |g, cx| pick(g, i, window, cx));
                            }),
                    )
                })
            })
            .into_any_element()
    }

    /// The plot in a card, under a header naming the axes, with the pinned readout or a hint.
    fn plot_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mark = self.hover_mark.or(self.pinned_mark);
        let plot = canvas::render(&self.lines, &self.frame, mark, &self.plot_bounds, cx);
        let theme = cx.theme();
        let frame = &self.frame;
        let title = canvas::axis_label(&frame.y_label, frame.y_unit);
        let subtitle = format!(
            "against {}",
            canvas::axis_label(&frame.x_label, frame.x_unit)
        );
        let empty = self.lines.is_empty();

        let aside = if let Some(x) = self.pinned_mark {
            let text = match frame.x_unit {
                Some(unit) => format!("Pinned at {} {unit}", canvas::format(x)),
                None => format!("Pinned at {}", canvas::format(x)),
            };
            h_flex()
                .id("graph-pin")
                .gap_1p5()
                .px_2p5()
                .py_1()
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .text_xs()
                .child(
                    UiIcon::new(AssetIcon::Pin)
                        .size(px(12.))
                        .text_color(theme.primary),
                )
                .child(text)
                .child(
                    UiIcon::new(AssetIcon::X)
                        .size(px(12.))
                        .text_color(theme.muted_foreground),
                )
                .hover(|s| s.bg(theme.secondary))
                .tooltip(|window, cx| Tooltip::new("Unpin the readout").build(window, cx))
                .on_click(cx.listener(|g, _: &ClickEvent, _, cx| {
                    g.pinned_mark = None;
                    cx.notify();
                }))
                .into_any_element()
        } else if let Some(status) = self.status.clone().filter(|_| !empty) {
            hint(AssetIcon::TriangleAlert, status, cx)
        } else if empty {
            div().into_any_element()
        } else {
            hint(
                AssetIcon::MousePointerClick,
                "Click the plot to pin a readout".into(),
                cx,
            )
        };

        let header = h_flex()
            .gap_3()
            .px_4()
            .pt_3()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .child(subtitle),
                    ),
            )
            .child(div().flex_none().child(aside));

        // With nothing plotted, say why over the empty axes.
        let notice = self.status.clone().filter(|_| empty).map(|status| {
            v_flex()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    v_flex()
                        .items_center()
                        .gap_2()
                        .px_5()
                        .py_4()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .child(
                            UiIcon::new(AssetIcon::ChartSpline)
                                .size(px(22.))
                                .text_color(theme.muted_foreground),
                        )
                        .child(div().text_color(theme.muted_foreground).child(status)),
                )
        });

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .overflow_hidden()
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(200.))
                    .child(plot)
                    .children(notice),
            )
    }

    /// The column of settings cards: inputs, movement, options, ammo and display.
    fn settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut sections = Vec::new();
        let fields: Vec<AnyElement> = self
            .main_input
            .iter()
            .map(|f| self.field_row(f, true, cx))
            .chain(self.fields.iter().map(|f| self.field_row(f, false, cx)))
            .collect();
        if !fields.is_empty() {
            sections.push(panel::section("Inputs", fields, cx));
        }
        if let Some(movement) = self.movement(cx) {
            sections.push(movement);
        }
        let options = self.option_rows(cx);
        if !options.is_empty() {
            sections.push(panel::section("Options", options, cx));
        }
        if self.graph.has_segments() {
            sections.push(self.ammo_section(cx));
        }
        sections.push(panel::section(
            "Display",
            vec![
                panel::switch(
                    "show-legend",
                    "Show legend",
                    "Name every line in the corner of the plot.",
                    self.settings.show_legend,
                    |g, v, cx| {
                        g.settings.show_legend = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ),
                panel::switch(
                    "show-y0",
                    "Always show Y = 0",
                    "Start the Y axis at zero even when every line sits higher.",
                    self.settings.show_y0,
                    |g, v, cx| {
                        g.settings.show_y0 = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ),
            ],
            cx,
        ));
        v_flex()
            .id("graph-settings")
            .flex_none()
            .w(px(PANEL_W))
            .h_full()
            .gap_4()
            .overflow_y_scroll()
            .children(sections)
    }

    /// An input field: the game's icon, its name and unit, and the field.
    fn field_row(&self, field: &Field, main: bool, cx: &Context<Self>) -> AnyElement {
        let def = field.def;
        let theme = cx.theme();
        let tooltip = if main { def.main_tooltip } else { def.tooltip };
        let description = if main {
            "Range along the X axis, like 0-100."
        } else if def.default.is_none() {
            "Leave empty to leave it unset."
        } else {
            ""
        };
        let icon = self
            .loaded
            .as_ref()
            .filter(|_| def.icon > 0)
            .map(|l| icon_box(&l.catalog, &l.icons, Icon::Id(def.icon), 20.));
        let input = Input::new(&field.state)
            .small()
            .w(px(if main { 120. } else { 104. }))
            .when_some(def.unit.label(), |input, unit| {
                input.suffix(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(unit),
                )
            });
        h_flex()
            .id(SharedString::from(format!(
                "field-{:?}-{:?}",
                def.param, def.unit
            )))
            .gap_2p5()
            .px_3()
            .py_2()
            .children(icon)
            .child(panel::text(def.label, description, cx))
            .child(div().flex_none().child(input))
            .when_some(tooltip, |d, tip| {
                d.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            })
            .into_any_element()
    }

    /// The attacker's and target's speed and heading dials, side by side.
    fn movement(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let src = self.graph.src_vector();
        let tgt = self.graph.tgt_vector();
        if src.is_none() && tgt.is_none() {
            return None;
        }
        let dials = h_flex()
            .justify_around()
            .px_3()
            .py_3()
            .when_some(src, |row, def| {
                row.child(vector::render(
                    &self.atk_vector,
                    vector::Which::Attacker,
                    def.label,
                    cx,
                ))
            })
            .when_some(tgt, |row, def| {
                row.child(vector::render(
                    &self.tgt_vector,
                    vector::Which::Target,
                    def.label,
                    cx,
                ))
            });
        let help = div()
            .px_3()
            .pt_2p5()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("Drag in a circle to set speed and heading. 0° points at the other ship.");
        Some(panel::section(
            "Movement",
            vec![v_flex().child(help).child(dials).into_any_element()],
            cx,
        ))
    }

    /// The graph's own checkboxes and pyfa's graph options menu, as switches.
    fn option_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (x, y) = (self.x_def(), self.y_def());
        let (x_spec, y_spec) = ((x.param, x.unit), (y.param, y.unit));
        let mut rows: Vec<AnyElement> = Vec::new();
        for def in self.graph.checkboxes() {
            if !conditions_met(def.conditions, x_spec, y_spec) {
                continue;
            }
            let key = def.key;
            rows.push(panel::switch(
                SharedString::from(format!("graph-cb-{key:?}")),
                def.label,
                checkbox_description(key),
                self.checkbox(key),
                move |g, v, cx| {
                    g.checkboxes.insert(key, v);
                    g.recompute(cx);
                    cx.notify();
                },
                cx,
            ));
        }
        let s = self.settings;
        match self.graph {
            Graph::DamageStats => {
                rows.push(panel::switch(
                    "opt-projected",
                    "Apply projected items",
                    "Count what is projected onto the fits.",
                    s.apply_projected,
                    |g, v, cx| {
                        g.settings.apply_projected = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ));
                rows.push(panel::switch(
                    "opt-resists",
                    "Ignore target resists",
                    "Plot raw damage, before the target's resistances.",
                    s.ignore_resists,
                    |g, v, cx| {
                        g.settings.ignore_resists = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ));
            }
            Graph::ApplicationProfile => {
                rows.push(panel::switch(
                    "opt-ammo-resists",
                    "Ignore target resists",
                    "Pick the best charge by raw damage.",
                    s.ammo_ignore_resists,
                    |g, v, cx| {
                        g.settings.ammo_ignore_resists = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ));
                rows.push(panel::switch(
                    "opt-ammo-projected",
                    "Apply projected effects",
                    "Count what is projected onto the fits.",
                    s.ammo_apply_projected,
                    |g, v, cx| {
                        g.settings.ammo_apply_projected = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ));
                return rows;
            }
            _ => {}
        }
        if !self.graph.has_options() {
            return rows;
        }
        rows.push(panel::switch(
            "opt-lock-range",
            "Ignore lock range",
            "Keep plotting beyond the attacker's targeting range.",
            s.ignore_lock_range,
            |g, v, cx| {
                g.settings.ignore_lock_range = v;
                g.settings_changed(cx);
            },
            cx,
        ));
        rows.push(panel::switch(
            "opt-dcr",
            "Ignore drone control range",
            "Let drones work beyond the control range.",
            s.ignore_dcr,
            |g, v, cx| {
                g.settings.ignore_dcr = v;
                g.settings_changed(cx);
            },
            cx,
        ));
        if self.graph == Graph::DamageStats {
            let modes: Vec<(DroneMode, &str)> =
                DroneMode::ALL.iter().map(|&m| (m, m.label())).collect();
            rows.push(panel::choice(
                "drone-mode",
                "Drone mode",
                "Which ship the drones keep up with.",
                &modes,
                s.drone_mode,
                |g, v, cx| {
                    g.settings.drone_mode = v;
                    g.settings_changed(cx);
                },
                cx,
            ));
        }
        rows
    }

    /// The Application Profile's style and ammo meta.
    fn ammo_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles: Vec<(AmmoStyle, &str)> =
            AmmoStyle::ALL.iter().map(|&s| (s, s.label())).collect();
        let qualities: Vec<(AmmoQuality, &str)> =
            AmmoQuality::ALL.iter().map(|&q| (q, q.label())).collect();
        panel::section(
            "Ammo",
            vec![
                panel::choice(
                    "ammo-style",
                    "Style",
                    "How the line shows which charge is loaded.",
                    &styles,
                    self.settings.ammo_style,
                    |g, v, cx| {
                        g.settings.ammo_style = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ),
                panel::choice(
                    "ammo-quality",
                    "Ammo meta",
                    "Which charges to compare.",
                    &qualities,
                    self.settings.ammo_quality,
                    |g, v, cx| {
                        g.settings.ammo_quality = v;
                        g.settings_changed(cx);
                    },
                    cx,
                ),
            ],
            cx,
        )
    }
}

/// A picker with its small label above it.
fn picker(label: &str, control: impl IntoElement, cx: &Context<GraphsWindow>) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(panel::caption(label, cx))
        .child(control)
}

/// A muted line with an icon, for the plot card's header.
fn hint(icon: AssetIcon, text: SharedString, cx: &Context<GraphsWindow>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .gap_1p5()
        .text_xs()
        .text_color(muted)
        .child(UiIcon::new(icon).size(px(12.)).text_color(muted))
        .child(text)
        .into_any_element()
}

/// pyfa's axis choice text: "Distance (km)".
fn axis_choice(label: &str, unit: Unit) -> String {
    match unit.label() {
        Some(unit) => format!("{label} ({unit})"),
        None => label.to_owned(),
    }
}

fn checkbox_description(key: CheckboxKey) -> &'static str {
    match key {
        CheckboxKey::AncReload => "Count the reload once ancillary repairers run dry.",
        CheckboxKey::UseCapsim => "Simulate every module cycle instead of the average use.",
        CheckboxKey::ApplyDamps => "Count the fit's sensor dampeners against the target.",
        CheckboxKey::ApplyDrones => "Count the fit's drones too.",
    }
}
