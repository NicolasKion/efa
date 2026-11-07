//! The window's layout: a toolbar with the graph picker, the plot, the settings strip under it,
//! and the fit and target lists under that, pyfa's graph frame. A small window wraps the
//! settings, stacks the lists and scrolls what does not fit under the plot.

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

/// The fit and target lists' height, and theirs in a short window.
const LISTS_H: f32 = 240.;
const LISTS_SHORT_H: f32 = 150.;
/// Below this height the lists and the plot's least height get smaller.
const SHORT_H: f32 = 680.;
/// Below this width the fit and target lists stack instead of sharing a row.
const STACK_LISTS_W: f32 = 760.;
/// The plot's least height; below it the area under the toolbar scrolls.
const PLOT_MIN_H: f32 = 240.;
const PLOT_SHORT_MIN_H: f32 = 170.;
/// A settings group's least width; groups wrap onto more lines when they don't fit.
const SECTION_MIN_W: f32 = 220.;
/// The axis pickers' widest.
const AXIS_W: f32 = 200.;
/// The graph picker's width.
const GRAPH_W: f32 = 200.;
/// The plot header's least width beside the graph picker, before it drops under it.
const HEADER_MIN_W: f32 = 260.;

impl Render for GraphsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let viewport = window.viewport_size();
        let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));
        let short = height < SHORT_H;
        let lists_h = if short { LISTS_SHORT_H } else { LISTS_H };
        let has_targets = self.graph.has_targets();
        let stack = has_targets && width < STACK_LISTS_W;
        // Each list's width, for how many value columns fit beside the names.
        let list_w = if has_targets && !stack {
            (width - 24.) / 2.
        } else {
            width - 16.
        };
        let sources = lists::sources(self, list_w, cx);
        let targets = has_targets.then(|| lists::targets(self, list_w, cx));
        let lists = if stack {
            v_flex()
                .flex_none()
                .gap_2()
                .p_2()
                .child(h_flex().h(px(lists_h)).child(sources))
                .children(targets.map(|t| h_flex().h(px(lists_h)).child(t)))
        } else {
            h_flex()
                .flex_none()
                .h(px(lists_h))
                .gap_2()
                .p_2()
                .child(sources)
                .children(targets)
        };
        let plot_min = if short { PLOT_SHORT_MIN_H } else { PLOT_MIN_H };
        // pyfa's order, top to bottom: the pickers, the plot, its settings, then the lists.
        // When the window is too short for all of it, everything under the toolbar scrolls.
        let body = v_flex()
            .id("graph-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(self.plot_card(plot_min, cx))
            .child(self.settings_panel(cx))
            .child(lists);
        root.child(self.toolbar(cx))
            .child(body)
            // A vector drag ends wherever the button comes up.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|g, _, _, _| g.drag_vector = None),
            )
            .into_any_element()
    }
}

impl GraphsWindow {
    /// The axis pickers, under the plot as in pyfa.
    fn axes(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let mut rows = vec![panel::fill_row(
            "Y axis",
            "What the plot shows.",
            y_axis,
            AXIS_W,
        )];
        if let Some(x_axis) = x_axis {
            rows.push(panel::fill_row(
                "X axis",
                "What it is plotted against.",
                x_axis,
                AXIS_W,
            ));
        }
        panel::section("Axes", rows, cx)
    }

    /// The graph picker, what the plot shows, and the readout or a hint, as pyfa heads its
    /// window with the graph choice.
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let graph = self.graph_selector(cx);
        let header = self.plot_header(cx);
        let theme = cx.theme();
        // The header drops under the picker when they don't fit side by side.
        h_flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_x_4()
            .gap_y_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(picker("Graph", graph, cx))
            .child(header)
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
            .w(px(GRAPH_W))
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
            .w_full()
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
    /// What the plot shows, and the pinned readout or a hint on the right.
    fn plot_header(&self, cx: &mut Context<Self>) -> AnyElement {
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

        h_flex()
            .flex_1()
            .min_w(px(HEADER_MIN_W))
            .gap_2()
            .child(
                div()
                    .min_w_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .truncate()
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .truncate()
                    .child(subtitle),
            )
            .child(div().min_w_0().overflow_hidden().child(aside))
            .into_any_element()
    }

    fn plot_card(&self, min_h: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let mark = self.hover_mark.or(self.pinned_mark);
        let plot = canvas::render(&self.lines, &self.frame, mark, &self.plot_bounds, cx);
        let theme = cx.theme();
        let empty = self.lines.is_empty();
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

        v_flex().flex_1().min_h(px(min_h)).overflow_hidden().child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(plot)
                .children(notice),
        )
    }

    /// The column of settings cards: inputs, movement, options, ammo and display.
    fn settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut sections = vec![self.axes(cx)];
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
        // Side by side under the plot, as pyfa lays out its panel, wrapping onto more lines in
        // a narrow window. Every group is ruled off on its left and top; the strip pulls them a
        // pixel up and left, so the rules on the outer edges fall outside and are clipped.
        let theme = cx.theme();
        v_flex()
            .id("graph-settings")
            .flex_none()
            .overflow_hidden()
            .border_t_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .flex_wrap()
                    .items_stretch()
                    .ml(px(-1.))
                    .mt(px(-1.))
                    .children(sections.into_iter().map(|s| {
                        // Each group takes an equal share of its line's width.
                        div()
                            .flex_1()
                            .min_w(px(SECTION_MIN_W))
                            .px_3()
                            .py_2()
                            .border_l_1()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(s)
                    })),
            )
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
            .map(|l| icon_box(&l.catalog, &l.icons, Icon::Id(def.icon), 16.));
        let input = Input::new(&field.state)
            .small()
            .w(px(if main { 110. } else { 90. }))
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
            .h(px(panel::ROW_H))
            .gap_1p5()
            .children(icon)
            .child(div().flex_1().min_w_0().truncate().child(def.label))
            .child(div().flex_none().child(input))
            .when_some(
                tooltip.or((!description.is_empty()).then_some(description)),
                |d, tip| d.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx)),
            )
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
            .id("graph-dials")
            .gap_4()
            .tooltip(|window, cx| {
                Tooltip::new(
                    "Drag in a circle to set speed and heading. 0° points at the other ship.",
                )
                .build(window, cx)
            })
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
        Some(panel::section(
            "Movement",
            vec![dials.into_any_element()],
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
    h_flex()
        .gap_1p5()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_owned()),
        )
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
        .child(div().min_w_0().truncate().child(text))
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
