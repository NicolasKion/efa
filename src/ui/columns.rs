//! The fitting view's columns, sized the way pyfa sizes them: each fits its widest cell until the
//! user drags its header edge, after which it keeps the width it was given. Widths are kept with
//! the fits. Double-clicking an edge hands the column back to auto-sizing.

pub mod misc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AppContext, ClickEvent, Context, CursorStyle, Div, DragMoveEvent, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px,
};

use std::{collections::HashMap, rc::Rc};

use super::{
    Efa, Loaded, stats,
    widgets::{self, icon, icon_box, short},
};
use crate::{
    data::{Catalog, Slot},
    engine::{FitItem, ModuleInfo, Stats},
    fit::{Fit, Module, mutation::display_name},
    icons::Icon,
    prices::Prices,
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
    /// pyfa's Miscellanea: one figure per kind of module.
    Misc,
    Price,
    Charge,
    /// Time until an overheated module burns out.
    Heat,
}

impl Col {
    pub const ALL: [Self; 9] = [
        Self::Name,
        Self::Powergrid,
        Self::Cpu,
        Self::Capacitor,
        Self::Range,
        Self::Misc,
        Self::Price,
        Self::Charge,
        Self::Heat,
    ];

    /// Name the width is saved under.
    fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Powergrid => "powergrid",
            Self::Cpu => "cpu",
            Self::Capacitor => "capacitor",
            Self::Range => "range",
            Self::Misc => "misc",
            Self::Price => "price",
            Self::Charge => "charge",
            Self::Heat => "heat",
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
            Self::Name | Self::Misc | Self::Price | Self::Charge | Self::Heat => None,
        }
    }

    fn header_text(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Misc => "Misc",
            Self::Charge => "Charge",
            Self::Price => "Price",
            Self::Heat => "Heat",
            Self::Powergrid | Self::Cpu | Self::Capacitor | Self::Range => "",
        }
    }

    /// The header's tooltip, for headers that do not say it all.
    fn header_tooltip(self) -> Option<&'static str> {
        match self {
            Self::Misc => Some("Misc data"),
            Self::Heat => Some("Estimated time till an overheated module burns out"),
            _ => self.header_icon().map(|(_, tip)| tip),
        }
    }

    /// What a module's cell shows in this column. `misc` is the module's Miscellanea text;
    /// `expanded` names mutated modules after their mutaplasmid and base item.
    pub fn text(
        self,
        catalog: &Catalog,
        prices: &Prices,
        m: &Module,
        info: &ModuleInfo,
        expanded: bool,
        misc: Option<&str>,
    ) -> Option<String> {
        match self {
            Self::Name => {
                let name = display_name(catalog, m.type_id, m.mutation.as_ref(), expanded);
                Some(match (m.spool, info.spool_max) {
                    (Some(cycles), Some(max)) => format!("{name} ({cycles}/{max} cycles)"),
                    _ => name,
                })
            }
            Self::Powergrid => info.powergrid.map(short),
            Self::Cpu => info.cpu.map(short),
            Self::Capacitor => info.cap_per_sec.map(|v| format!("{:+.1}/s", -v)),
            Self::Range => range_text(info.optimal, info.falloff),
            // pyfa prices the module alone, not what it has loaded.
            Self::Price => prices
                .get(m.type_id)
                .filter(|&p| p > 0.0)
                .map(stats::format::currency),
            Self::Charge => m
                .charge
                .map(|c| charge_text(&catalog.name(c), info.charges, info.charged_cycles)),
            Self::Misc => misc.map(str::to_owned),
            Self::Heat => info.burnout.map(burnout_text),
        }
    }
}

/// pyfa's ammo column (builtinViewColumns/ammo.py): the charge's name with how many the module
/// holds, and how many cycles they last when that differs.
fn charge_text(name: &str, charges: Option<u32>, cycles: Option<u32>) -> String {
    match (charges.filter(|&n| n > 0), cycles) {
        (Some(n), Some(c)) if c != 0 && c != n => format!("{name} ({n}, {c} cycles)"),
        (Some(n), _) => format!("{name} ({n})"),
        (None, _) => name.to_owned(),
    }
}

/// Time the way pyfa's heat column writes it: minutes and seconds, hours only when there are
/// any.
fn burnout_text(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = ((total / 3600) % 24, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
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
            widths: [140., 56., 56., 66., 106., 90., 70., 190., 54.],
            name_fixed: false,
        }
    }
}

/// Space between cells and around a row, as the rows lay them out.
const CELL_GAP: f32 = 4.;
const ROW_PADDING: f32 = 8.;

impl Layout {
    /// The narrowest the table can get before it has to scroll: every column at its width, the
    /// name column at the least it shrinks to.
    pub fn min_width(&self) -> f32 {
        let columns: f32 = Col::ALL
            .iter()
            .map(|&col| {
                if col == Col::Name && !self.name_fixed {
                    NAME_MIN
                } else {
                    self.widths[col.index()]
                }
            })
            .sum();
        let gaps = CELL_GAP * (Col::ALL.len() + 2) as f32;
        STATE_W + ICON_W + columns + gaps + ROW_PADDING
    }

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

/// What the fitting view's widths and Miscellanea texts were worked out from.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FittingKey {
    calc: u64,
    current: usize,
    /// The prices, by address: they are replaced, never changed.
    prices: usize,
    expanded: bool,
    raw_hp: bool,
    rem: gpui_kit::Pixels,
}

/// The fitting view's column widths and Miscellanea texts, kept while what they come from
/// stays the same, since measuring text is the slow part of drawing the view.
pub struct FittingCache {
    key: FittingKey,
    /// The widths the user set, which the layout starts from.
    columns: std::collections::BTreeMap<String, f32>,
    layout: Layout,
    misc: HashMap<FitItem, (String, String)>,
}

impl Efa {
    /// The Miscellanea text and tooltip of a module or subsystem.
    pub(super) fn misc_of(
        &self,
        loaded: &Loaded,
        fit: &Fit,
        stats: &Stats,
        item: FitItem,
    ) -> Option<(String, String)> {
        let misc = stats.module_misc.get(&item)?;
        let (type_id, charge) = match item {
            FitItem::Module(slot, i) => {
                let m = fit.rack(slot).get(i)?.as_ref()?;
                (m.type_id, m.charge)
            }
            FitItem::Subsystem(i) => (*fit.subsystems.get(i)?, None),
            _ => return None,
        };
        // Ancillary repairers count in effective HP while the resistances section does.
        let ehp = (!self.stats_view.raw_hp).then(|| {
            let ratio = |layer: crate::engine::Layer| layer.mult.unwrap_or(1.0);
            misc::EhpRatios {
                shield: ratio(stats.shield),
                armor: ratio(stats.armor),
            }
        });
        misc::module_misc(&loaded.catalog, type_id, charge, misc, ehp)
    }

    /// Work out the fitting view's column widths and Miscellanea texts again when something
    /// they come from changed: the fit or its stats, the prices, the column widths set, the
    /// preferences they show and the text size.
    pub(super) fn refresh_fitting_cache(&mut self, loaded: &Loaded, window: &Window) {
        let Some(current) = self.saved.settings.current else {
            return;
        };
        let key = FittingKey {
            calc: self.calc_generation,
            current,
            prices: Rc::as_ptr(&self.prices) as usize,
            expanded: self
                .saved
                .settings
                .preferences
                .general
                .expanded_mutant_names,
            raw_hp: self.stats_view.raw_hp,
            rem: window.rem_size(),
        };
        if let Some(cache) = &self.fitting_cache
            && cache.key == key
            && cache.columns == self.saved.settings.columns
        {
            self.column_layout = cache.layout;
            return;
        }
        let no_stats = Stats::default();
        let fit = &self.saved.fits[current];
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let modules = [Slot::High, Slot::Mid, Slot::Low, Slot::Service]
            .into_iter()
            .flat_map(|slot| (0..fit.rack(slot).len()).map(move |i| FitItem::Module(slot, i)));
        let subsystems = (0..fit.subsystems.len()).map(FitItem::Subsystem);
        let misc: HashMap<FitItem, (String, String)> = modules
            .chain(subsystems)
            .filter_map(|item| Some((item, self.misc_of(loaded, fit, stats, item)?)))
            .collect();
        let layout = self.column_layout(loaded, fit, stats, &misc, window);
        self.column_layout = layout;
        self.fitting_cache = Some(FittingCache {
            key,
            columns: self.saved.settings.columns.clone(),
            layout,
            misc,
        });
    }

    /// A module's or subsystem's Miscellanea text and tooltip, as the fitting view last worked
    /// them out.
    pub(super) fn cached_misc(&self, item: FitItem) -> Option<&(String, String)> {
        self.fitting_cache.as_ref()?.misc.get(&item)
    }

    /// Widths for this frame: what the user set, or the widest header or cell plus padding.
    fn column_layout(
        &self,
        loaded: &Loaded,
        fit: &Fit,
        stats: &Stats,
        misc: &HashMap<FitItem, (String, String)>,
        window: &Window,
    ) -> Layout {
        let expanded = self
            .saved
            .settings
            .preferences
            .general
            .expanded_mutant_names;
        let mut layout = Layout::default();
        let text_of = |item: FitItem| misc.get(&item).map(|(text, _)| text.as_str());
        let modules: Vec<(&Module, ModuleInfo, Option<&str>)> =
            [Slot::High, Slot::Mid, Slot::Low, Slot::Service]
                .into_iter()
                .flat_map(|slot| {
                    fit.rack(slot).iter().enumerate().filter_map(move |(i, m)| {
                        let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                        let misc = text_of(FitItem::Module(slot, i));
                        m.as_ref().map(|m| (m, info, misc))
                    })
                })
                .collect();
        // Subsystems show their slot changes in the misc column.
        let subsystem_misc: Vec<&str> = (0..fit.subsystems.len())
            .filter_map(|i| text_of(FitItem::Subsystem(i)))
            .collect();
        for col in Col::ALL {
            if let Some(&width) = self.saved.settings.columns.get(col.key()) {
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
            let mut widest = modules
                .iter()
                .filter_map(|(m, info, misc)| {
                    col.text(&loaded.catalog, &self.prices, m, info, expanded, *misc)
                })
                .map(|text| text_width(window, &text))
                .fold(header, f32::max);
            if col == Col::Misc {
                widest = subsystem_misc
                    .iter()
                    .map(|text| text_width(window, text))
                    .fold(widest, f32::max);
            }
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
        widgets::table_head(cx)
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
                            g.saved.settings.columns.remove(col.key());
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
                    .when_some(col.header_tooltip(), widgets::TipExt::tip)
                    .child(content)
                    .child(handle)
                    .on_drag_move(cx.listener(
                        move |g, event: &DragMoveEvent<ColumnResize>, _, cx| {
                            if event.drag(cx).0 != col {
                                return;
                            }
                            let width = f32::from(event.event.position.x - event.bounds.left())
                                .max(MIN_WIDTH);
                            g.saved.settings.columns.insert(col.key().to_owned(), width);
                            g.unsaved = true;
                            cx.notify();
                        },
                    ))
            }))
            .into_any_element()
    }
}

/// pyfa's Max Range text: "10km+5km", three significant digits, the falloff alone when there is
/// no optimal.
pub(super) fn range_text(optimal: Option<f64>, falloff: Option<f64>) -> Option<String> {
    let metres = |v: f64| format!("{}m", stats::format::amount(v, 3, 0, 3));
    let falloff = falloff.map(|f| format!("+{}", metres(f)));
    match (optimal, falloff) {
        (Some(o), f) => Some(format!("{}{}", metres(o), f.unwrap_or_default())),
        (None, f) => f,
    }
}

/// pyfa's Max Range tooltip: a missile's odds of flying each whole second, or what the figures
/// are.
pub(super) fn range_tooltip(info: &crate::engine::ModuleInfo) -> Option<String> {
    if let Some(m) = info.missile {
        // pyfa rounds the chance to three decimals before leaving out a sure thing.
        if !(0.0005..0.9995).contains(&m.higher_chance) {
            return Some("Missile flight range".to_owned());
        }
        let line = |share: f64, range: f64| {
            format!(
                "{}% chance to fly {}m",
                stats::format::amount(share * 100.0, 3, 0, 0),
                stats::format::amount(range, 3, 0, 3)
            )
        };
        return Some(format!(
            "Missile flight range\n{}\n{}",
            line(1.0 - m.higher_chance, m.lower),
            line(m.higher_chance, m.higher)
        ));
    }
    (info.optimal.is_some() || info.falloff.is_some()).then(|| "Optimal + Falloff".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{charge_text, range_text};

    #[test]
    fn ranges_read_like_pyfa() {
        assert_eq!(
            range_text(Some(10_000.0), Some(5_000.0)).as_deref(),
            Some("10km+5km")
        );
        assert_eq!(range_text(Some(42_390.0), None).as_deref(), Some("42.4km"));
        assert_eq!(range_text(None, Some(500.0)).as_deref(), Some("+500m"));
        assert_eq!(range_text(None, None), None);
    }

    #[test]
    fn charge_text_follows_pyfa() {
        assert_eq!(
            charge_text("Scourge Light Missile", Some(40), Some(40)),
            "Scourge Light Missile (40)"
        );
        assert_eq!(
            charge_text("Multifrequency S", Some(1), Some(4000)),
            "Multifrequency S (1, 4000 cycles)"
        );
        assert_eq!(
            charge_text("Optimal Range Script", Some(1), None),
            "Optimal Range Script (1)"
        );
        assert_eq!(
            charge_text("Cap Booster 800", Some(1), Some(0)),
            "Cap Booster 800 (1)"
        );
        assert_eq!(charge_text("Nothing", Some(0), Some(3)), "Nothing");
        assert_eq!(charge_text("Unknown", None, None), "Unknown");
    }
}
