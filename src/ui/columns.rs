//! The fitting view's columns, sized the way pyfa sizes them: each fits its widest cell until the
//! user drags its header edge, after which it keeps the width it was given. Widths are kept with
//! the fits. Double-clicking an edge hands the column back to auto-sizing.

use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::{
    AppContext, ClickEvent, Context, CursorStyle, Div, DragMoveEvent, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px,
};

use super::{
    Gyfa, Loaded,
    widgets::{icon, icon_box, km, short},
};
use crate::{
    data::{Catalog, Slot},
    engine::{ModuleInfo, Stats},
    fit::{Fit, Module},
    icons::Icon,
};

/// Room pyfa adds to a column's widest text when sizing it.
const AUTO_PADDING: f32 = 24.;
/// Narrowest the auto-sized name column shrinks to in a narrow pane.
const NAME_MIN: f32 = 80.;
/// Narrowest a dragged column may get, so its edge can still be grabbed.
const MIN_WIDTH: f32 = 28.;
/// Width of the grab area on a header's right edge.
const HANDLE_W: f32 = 6.;
/// The state mark and item icon at the start of every row.
pub const STATE_W: f32 = 22.;
pub const ICON_W: f32 = 26.;
const HEADER_ICON: f32 = 18.;
const CHARGE_ICON: f32 = 20.;
/// Text in rows is `text_sm`, 0.875 of the root size.
const ROW_TEXT_SCALE: f32 = 0.875;

/// The resizable columns, after the fixed state and icon columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Name,
    Powergrid,
    Cpu,
    Capacitor,
    Range,
    Charge,
}

impl Col {
    pub const ALL: [Self; 6] = [
        Self::Name,
        Self::Powergrid,
        Self::Cpu,
        Self::Capacitor,
        Self::Range,
        Self::Charge,
    ];

    /// Name the width is saved under.
    fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Powergrid => "powergrid",
            Self::Cpu => "cpu",
            Self::Capacitor => "capacitor",
            Self::Range => "range",
            Self::Charge => "charge",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&c| c == self).unwrap_or(0)
    }

    /// The header's icon, for columns pyfa labels with one.
    fn header_icon(self) -> Option<(i32, &'static str)> {
        match self {
            Self::Powergrid => Some((icon::POWERGRID, "Powergrid")),
            Self::Cpu => Some((icon::CPU, "CPU")),
            Self::Capacitor => Some((icon::CAPACITOR, "Capacitor use")),
            Self::Range => Some((icon::RANGE, "Optimal + falloff")),
            Self::Name | Self::Charge => None,
        }
    }

    fn header_text(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Charge => "Charge",
            Self::Powergrid | Self::Cpu | Self::Capacitor | Self::Range => "",
        }
    }

    /// What a module's cell shows in this column.
    pub fn text(self, catalog: &Catalog, m: &Module, info: &ModuleInfo) -> Option<String> {
        match self {
            Self::Name => Some(match (m.spool, info.spool_max) {
                (Some(cycles), Some(max)) => {
                    format!("{} ({cycles}/{max} cycles)", catalog.name(m.type_id))
                }
                _ => catalog.name(m.type_id),
            }),
            Self::Powergrid => info.powergrid.map(short),
            Self::Cpu => info.cpu.map(short),
            Self::Capacitor => info.cap_per_sec.map(|v| format!("{:+.1}/s", -v)),
            Self::Range => match (info.optimal, info.falloff) {
                (Some(o), Some(f)) => Some(format!("{}+{}", km(o), km(f))),
                (Some(o), None) => Some(km(o)),
                _ => None,
            },
            Self::Charge => m.charge.map(|c| catalog.name(c)),
        }
    }
}

/// How wide each column is this frame.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    widths: [f32; Col::ALL.len()],
    /// Whether the name column was sized by hand; otherwise it also takes the leftover room.
    name_fixed: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            widths: [140., 56., 56., 66., 106., 190.],
            name_fixed: false,
        }
    }
}

impl Layout {
    /// An empty cell of the column's width, to put the cell's content in.
    pub fn cell(&self, col: Col) -> Div {
        let width = px(self.widths[col.index()]);
        if col == Col::Name && !self.name_fixed {
            // Takes the leftover room, and is the first to give it up when the pane is narrow.
            div()
                .flex_1()
                .flex_basis(width)
                .min_w(px(NAME_MIN))
                .overflow_hidden()
        } else {
            div().flex_none().w(width).overflow_hidden()
        }
    }
}

/// Being dragged: the edge of a column's header.
#[derive(Debug, Clone, Copy)]
struct ColumnResize(Col);

/// Resizing a column needs a drag preview; there is nothing to show.
struct NoPreview;

impl Render for NoPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub(super) fn text_width(window: &Window, text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let size = window.rem_size() * ROW_TEXT_SCALE;
    let run = window.text_style().to_run(text.len());
    let line =
        window
            .text_system()
            .shape_line(SharedString::from(text.to_owned()), size, &[run], None);
    f32::from(line.width)
}

impl Gyfa {
    /// Widths for this frame: what the user set, or the widest header or cell plus padding.
    pub(super) fn column_layout(
        &self,
        loaded: &Loaded,
        fit: &Fit,
        stats: &Stats,
        window: &Window,
    ) -> Layout {
        let mut layout = Layout::default();
        let modules: Vec<(&Module, ModuleInfo)> = [Slot::High, Slot::Mid, Slot::Low]
            .into_iter()
            .flat_map(|slot| {
                fit.rack(slot).iter().enumerate().filter_map(move |(i, m)| {
                    let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                    m.as_ref().map(|m| (m, info))
                })
            })
            .collect();
        for col in Col::ALL {
            if let Some(&width) = self.saved.columns.get(col.key()) {
                layout.widths[col.index()] = width;
                if col == Col::Name {
                    layout.name_fixed = true;
                }
                continue;
            }
            let header = match col.header_icon() {
                Some(_) => HEADER_ICON,
                None => text_width(window, col.header_text()),
            };
            let widest = modules
                .iter()
                .filter_map(|(m, info)| col.text(&loaded.catalog, m, info))
                .map(|text| text_width(window, &text))
                .fold(header, f32::max);
            let extra = if col == Col::Charge {
                CHARGE_ICON + 4.
            } else {
                0.
            };
            layout.widths[col.index()] = widest + extra + AUTO_PADDING;
        }
        // Other rows show bare names in the name column; it has to fit those too.
        if !layout.name_fixed {
            let names = fit
                .rigs
                .iter()
                .flatten()
                .chain(&fit.subsystems)
                .chain(&fit.mode)
                .map(|&t| text_width(window, &loaded.catalog.name(t)) + AUTO_PADDING);
            let name = &mut layout.widths[Col::Name.index()];
            *name = names.fold(*name, f32::max);
        }
        layout
    }

    /// The table header: a label or icon per column, with a grab handle on each right edge.
    pub(super) fn column_header(
        loaded: &Loaded,
        layout: Layout,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        h_flex()
            .h(px(24.))
            .px_1()
            .gap_1()
            .bg(theme.table_head)
            .text_color(theme.table_head_foreground)
            .child(div().w(px(STATE_W + ICON_W)).flex_none())
            .children(Col::ALL.into_iter().map(|col| {
                let content = match col.header_icon() {
                    Some((icon_id, _)) => icon_box(
                        &loaded.catalog,
                        &loaded.icons,
                        Icon::Id(icon_id),
                        HEADER_ICON,
                    )
                    .into_any_element(),
                    None => div().child(col.header_text()).into_any_element(),
                };
                let handle = div()
                    .id(SharedString::from(format!("resize-{}", col.key())))
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .w(px(HANDLE_W))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|s| s.bg(cx.theme().border))
                    .on_drag(ColumnResize(col), |_, _, _, cx| cx.new(|_| NoPreview))
                    .on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if event.click_count() >= 2 {
                            g.saved.columns.remove(col.key());
                            g.save();
                            cx.notify();
                        }
                    }));
                layout
                    .cell(col)
                    .id(SharedString::from(format!("column-{}", col.key())))
                    .relative()
                    .h_full()
                    .flex()
                    .items_center()
                    .when_some(col.header_icon(), |cell, (_, tip)| {
                        cell.tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx)
                        })
                    })
                    .child(content)
                    .child(handle)
                    .on_drag_move(cx.listener(
                        move |g, event: &DragMoveEvent<ColumnResize>, _, cx| {
                            if event.drag(cx).0 != col {
                                return;
                            }
                            let width = f32::from(event.event.position.x - event.bounds.left())
                                .max(MIN_WIDTH);
                            g.saved.columns.insert(col.key().to_owned(), width);
                            g.unsaved = true;
                            cx.notify();
                        },
                    ))
            }))
            .into_any_element()
    }
}
