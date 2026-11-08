//! The plot itself: axes with nice ticks, the lines, the legend, and the readout at the
//! marked X, pyfa's canvas panel without matplotlib.

use std::{cell::Cell, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Icon as UiIcon, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Bounds, Context, Div, EntityId, FontWeight, Hsla, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, PathBuilder, Pixels,
    Point, Render, SharedString, StatefulInteractiveElement, Styled, Window, canvas, div, point,
    prelude::FluentBuilder, px, relative,
};

use super::style::{Lightness, LineColor, LineStyle};
use crate::graphs::plot::Series;

/// One plotted line and how to draw it.
#[derive(Debug, Clone)]
pub struct PlotLine {
    pub label: String,
    pub color: LineColor,
    pub lightness: Lightness,
    /// A color of its own instead of the fit's: the Application Profile's per-charge colors.
    pub hue: Option<f32>,
    /// Segments of one line share a legend entry.
    pub in_legend: bool,
    pub style: LineStyle,
    pub series: Series,
}

/// What the plot shows besides the lines.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub x_label: String,
    pub y_label: String,
    pub x_unit: Option<&'static str>,
    pub y_unit: Option<&'static str>,
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub legend: bool,
}

const LINE_WIDTH: f32 = 1.6;
const Y_TICKS_W: f32 = 64.;
const TICK_LABEL_W: f32 = 80.;
const TICK_LABEL_H: f32 = 14.;

/// Room left around the data, as fractions of its span (pyfa's `_getLimits`).
pub fn limits(values: impl Iterator<Item = f64>, extra: (f64, f64), round: bool) -> (f64, f64) {
    let (mut min, mut max) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    if !min.is_finite() || !max.is_finite() {
        (min, max) = (0.0, 0.0);
    }
    // A span that is only rounding noise counts as a flat line; otherwise its ticks would be
    // a millionth apart and their labels a dozen digits long.
    let magnitude = min.abs().max(max.abs());
    if max - min <= (magnitude * 1e-6).max(1e-9) {
        let mid = f64::midpoint(min, max);
        let mid = if mid.abs() < 1e-9 { 0.0 } else { mid };
        (min, max) = (mid, mid);
    }
    let span = max - min;
    min -= span * extra.0;
    max += span * extra.1;
    if max <= min {
        min -= min.abs() * 0.05;
        max += max.abs() * 0.05;
    }
    if max <= min {
        min -= 5.0;
        max += 5.0;
    }
    if round && max > 0.0 {
        max = nice(max, true);
    }
    (min, max)
}

/// The nearest 1, 2, 2.5 or 5 times a power of ten, at or above `v` when `up`.
fn nice(v: f64, up: bool) -> f64 {
    let magnitude = 10_f64.powf(v.abs().log10().floor());
    let steps = [1.0, 2.0, 2.5, 5.0, 10.0];
    let scaled = v / magnitude;
    let pick = if up {
        steps.iter().copied().find(|&s| s >= scaled - 1e-9)
    } else {
        steps
            .iter()
            .copied()
            .min_by(|a, b| (a - scaled).abs().total_cmp(&(b - scaled).abs()))
    };
    pick.unwrap_or(10.0) * magnitude
}

/// Tick positions over `range`, about `count` of them at round values.
pub fn ticks(range: (f64, f64), count: usize) -> Vec<f64> {
    let span = range.1 - range.0;
    if span <= 0.0 || !span.is_finite() {
        return vec![range.0];
    }
    let step = nice(span / count.max(1) as f64, false);
    let first = (range.0 / step).ceil() * step;
    (0..)
        .map(|i| first + f64::from(i) * step)
        // Adding steps up leaves noise like 5.6e-17 where zero belongs.
        .map(|v| if v.abs() < step * 1e-6 { 0.0 } else { v })
        .take_while(|&v| v <= range.1 + step * 1e-9)
        .take(100)
        .collect()
}

/// A number the way pyfa's readouts print it: four significant digits, no trailing zeros.
pub fn format(v: f64) -> String {
    if !v.is_finite() {
        return if v > 0.0 {
            "∞".to_owned()
        } else {
            v.to_string()
        };
    }
    let magnitude = v.abs();
    if magnitude < 1e-9 {
        return "0".to_owned();
    }
    // Big numbers get k, M and G; tiny ones scientific notation, so a label stays short.
    for (scale, suffix) in [(1e9, "G"), (1e6, "M"), (1e5, "k")] {
        if magnitude >= scale {
            let scale = if suffix == "k" { 1e3 } else { scale };
            return format!("{}{suffix}", significant(v / scale, 4));
        }
    }
    if magnitude < 1e-3 {
        let text = format!("{v:.2e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}e{exponent}");
    }
    significant(v, 4)
}

/// `v` to `digits` significant digits, without trailing zeros.
fn significant(v: f64, digits: i32) -> String {
    let places = (digits - 1 - v.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{v:.places$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

pub fn axis_label(label: &str, unit: Option<&str>) -> String {
    match unit {
        Some(unit) => format!("{label}, {unit}"),
        None => label.to_owned(),
    }
}

/// Fraction of the way through `range`.
fn frac(v: f64, range: (f64, f64)) -> f32 {
    ((v - range.0) / (range.1 - range.0)) as f32
}

/// The plot with its readouts, a view of its own so following the pointer redraws only it.
pub struct Plot {
    pub lines: Vec<PlotLine>,
    pub frame: Frame,
    /// Why nothing is plotted, or what the plot is missing.
    pub status: Option<SharedString>,
    /// Where the pointer is over the plot.
    hover: Option<f64>,
    /// Where the plot was last clicked; stays when the pointer leaves.
    pub pinned: Option<f64>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The graph window, whose header shows the pinned readout.
    owner: EntityId,
}

impl Plot {
    pub fn new(owner: EntityId) -> Self {
        Self {
            lines: Vec::new(),
            frame: Frame::default(),
            status: None,
            hover: None,
            pinned: None,
            bounds: Rc::default(),
            owner,
        }
    }

    /// Follow the pointer, and pin the readout where it is when `pin`.
    fn hover_at(&mut self, position: Point<Pixels>, pin: bool, cx: &mut Context<Self>) {
        let Some(x) = data_x(self.bounds.get(), position, self.frame.x_range) else {
            return;
        };
        let x = clamp_mark(x, self.frame.x_range);
        if self.hover != Some(x) {
            self.hover = Some(x);
            cx.notify();
        }
        if pin && self.pinned != Some(x) {
            self.pinned = Some(x);
            cx.notify();
            App::notify(cx, self.owner);
        }
    }
}

impl Render for Plot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let plot = self.render_plot(cx);
        let theme = cx.theme();
        // With nothing plotted, say why over the empty axes.
        let notice = self
            .status
            .clone()
            .filter(|_| self.lines.is_empty())
            .map(|status| {
                v_flex()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
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
        div().relative().size_full().child(plot).children(notice)
    }
}

impl Plot {
    fn render_plot(&self, cx: &mut Context<Self>) -> AnyElement {
        let (lines, frame, pin, hover) = (&self.lines, &self.frame, self.pinned, self.hover);
        let theme = cx.theme();
        // Soft gridlines, firmer axes, so the lines stand out.
        let (grid, axis, muted) = (
            theme.border.opacity(0.45),
            theme.border,
            theme.muted_foreground,
        );
        let mark_color = theme.muted_foreground.opacity(0.8);
        let pin_color = theme.primary.opacity(0.8);
        // The hover readout is left out where it would sit on top of the pinned one.
        let hover = hover.filter(|&h| {
            pin.is_none_or(|p| (frac(h, frame.x_range) - frac(p, frame.x_range)).abs() > 0.01)
        });
        // Colors come from the theme at draw time, so a theme switch recolors the lines.
        let colors: Vec<Hsla> = lines
            .iter()
            .map(|l| match l.hue {
                Some(hue) => l.lightness.apply(gpui_kit::hsla(hue, 0.75, 0.6, 1.0)),
                None => l.lightness.apply(l.color.color(theme)),
            })
            .collect();
        let x_ticks = ticks(frame.x_range, 8);
        let y_ticks = ticks(frame.y_range, 6);

        let y_labels = div()
            .relative()
            .flex_none()
            .w(px(Y_TICKS_W))
            .h_full()
            .children(y_ticks.iter().map(|&v| {
                div()
                    .absolute()
                    .right(px(8.))
                    .top(relative(1.0 - frac(v, frame.y_range)))
                    .mt(px(-TICK_LABEL_H / 2.))
                    .h(px(TICK_LABEL_H))
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(format(v))
            }));
        let x_labels =
            div()
                .relative()
                .flex_1()
                .h(px(TICK_LABEL_H + 6.))
                .children(x_ticks.iter().map(|&v| {
                    div()
                        .absolute()
                        .top(px(4.))
                        .left(relative(frac(v, frame.x_range)))
                        .ml(px(-TICK_LABEL_W / 2.))
                        .w(px(TICK_LABEL_W))
                        .flex()
                        .justify_center()
                        .text_size(px(11.))
                        .text_color(muted)
                        .child(format(v))
                }));

        let paint_lines: Vec<(Series, Hsla, LineStyle)> = lines
            .iter()
            .zip(&colors)
            .map(|(l, &c)| (l.series.clone(), c, l.style))
            .collect();
        let (x_range, y_range) = (frame.x_range, frame.y_range);
        let (verticals, horizontals) = (x_ticks.clone(), y_ticks.clone());
        let store = Rc::clone(&self.bounds);
        let plot = canvas(
            move |b, _, _| store.set(Some(b)),
            move |b, (), window, _| {
                let to_px = |x: f64, y: f64| {
                    point(
                        b.origin.x + b.size.width * frac(x, x_range),
                        b.origin.y + b.size.height * (1.0 - frac(y, y_range)),
                    )
                };
                let stroke = |window: &mut gpui_kit::Window,
                              points: &[gpui_kit::Point<Pixels>],
                              width: f32,
                              dashes: &[Pixels],
                              color: Hsla| {
                    if points.len() < 2 {
                        return;
                    }
                    let mut path = PathBuilder::stroke(px(width));
                    if !dashes.is_empty() {
                        path = path.dash_array(dashes);
                    }
                    path.move_to(points[0]);
                    for &p in &points[1..] {
                        path.line_to(p);
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                };
                for &x in &verticals {
                    let top = to_px(x, y_range.1);
                    let bottom = to_px(x, y_range.0);
                    stroke(window, &[top, bottom], 1.0, &[], grid);
                }
                for &y in &horizontals {
                    let left = to_px(x_range.0, y);
                    let right = to_px(x_range.1, y);
                    stroke(window, &[left, right], 1.0, &[], grid);
                }
                window.with_content_mask(Some(gpui_kit::ContentMask { bounds: b }), |window| {
                    for (series, color, style) in &paint_lines {
                        let points: Vec<_> = series
                            .xs
                            .iter()
                            .zip(&series.ys)
                            .map(|(&x, &y)| to_px(x, y))
                            .collect();
                        if points.len() == 1 {
                            // A single point gets a dot, or it would not show at all.
                            let p = points[0];
                            let dot = Bounds::centered_at(p, gpui_kit::size(px(4.), px(4.)));
                            window.paint_quad(gpui_kit::fill(dot, *color));
                            continue;
                        }
                        stroke(
                            window,
                            &points,
                            LINE_WIDTH,
                            &style.dashes(LINE_WIDTH),
                            *color,
                        );
                    }
                    if let Some(x) = pin {
                        let top = to_px(x, y_range.1);
                        let bottom = to_px(x, y_range.0);
                        stroke(window, &[top, bottom], 1.0, &[], pin_color);
                    }
                    if let Some(x) = hover {
                        let top = to_px(x, y_range.1);
                        let bottom = to_px(x, y_range.0);
                        stroke(window, &[top, bottom], 1.0, &[px(3.), px(3.)], mark_color);
                    }
                });
                // The axes on the left and at the bottom; the gridlines close the rest.
                let corners = [
                    to_px(x_range.0, y_range.1),
                    to_px(x_range.0, y_range.0),
                    to_px(x_range.1, y_range.0),
                ];
                stroke(window, &corners, 1.0, &[], axis);
            },
        )
        .absolute()
        .size_full();

        let legend = (frame.legend && !lines.is_empty()).then(|| {
            v_flex()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .max_w(relative(0.6))
                .px_2p5()
                .py_2()
                .gap_1()
                .rounded(px(6.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover.opacity(0.92))
                .shadow_sm()
                .text_xs()
                .text_color(theme.popover_foreground)
                .children(
                    lines
                        .iter()
                        .zip(&colors)
                        .filter(|(line, _)| line.in_legend)
                        .map(|(line, &color)| {
                            h_flex()
                                .gap_2()
                                .child(swatch(color, line.style))
                                .child(div().min_w_0().truncate().child(line.label.clone()))
                        }),
                )
        });

        // The pinned readout stays while the mouse moves; the hover readout is drawn over it.
        let readout = |x: f64, pinned: bool| {
            let fx = frac(x, frame.x_range);
            let x_text = match frame.x_unit {
                Some(unit) => format!("{} {unit}", format(x)),
                None => format(x),
            };
            // Labels sit right of the cursor, or left of it past the middle so they never run
            // into the plot's edge and wrap.
            let beside = |label: Div| {
                let label = label.whitespace_nowrap();
                if fx > 0.5 {
                    label.right(relative(1.0 - fx)).mr(px(6.))
                } else {
                    label.left(relative(fx)).ml(px(6.))
                }
            };
            let mut labels: Vec<AnyElement> = vec![
                beside(div().absolute().top(px(4.)))
                    .px_1p5()
                    .rounded(px(4.))
                    .bg(if pinned {
                        theme.primary
                    } else {
                        theme.foreground
                    })
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if pinned {
                        theme.primary_foreground
                    } else {
                        theme.background
                    })
                    .child(x_text)
                    .into_any_element(),
            ];
            for (line, &color) in lines.iter().zip(&colors) {
                let Some(y) = line.series.at(x) else { continue };
                let fy = frac(y, frame.y_range).clamp(0.0, 1.0);
                labels.push(
                    beside(h_flex().absolute())
                        .top(relative(1.0 - fy))
                        .mt(px(-TICK_LABEL_H))
                        .gap_1()
                        .px_1p5()
                        .rounded(px(4.))
                        .bg(theme.popover.opacity(0.92))
                        .border_1()
                        .border_color(color)
                        .text_size(px(11.))
                        .text_color(theme.popover_foreground)
                        .child(div().flex_none().size(px(6.)).rounded_full().bg(color))
                        .child(format(y))
                        .into_any_element(),
                );
            }
            labels
        };
        let readouts: Vec<AnyElement> = pin
            .map(|x| readout(x, true))
            .into_iter()
            .chain(hover.map(|x| readout(x, false)))
            .flatten()
            .collect();

        let area = div()
            .id("graph-plot")
            .relative()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(plot)
            .children(legend)
            .children(readouts)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.hover_at(event.position, false, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.hover_at(event.position, true, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    if this.pinned.take().is_some() {
                        cx.notify();
                        App::notify(cx, this.owner);
                    }
                }),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered && this.hover.take().is_some() {
                    cx.notify();
                }
            }));

        v_flex()
            .absolute()
            .inset_0()
            .pl_1()
            .pr_5()
            .pt_3()
            .pb_3()
            .child(
                div()
                    .ml(px(Y_TICKS_W))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child(axis_label(&frame.y_label, frame.y_unit)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .mt_2()
                    .child(y_labels)
                    .child(area),
            )
            .child(
                h_flex()
                    .child(div().flex_none().w(px(Y_TICKS_W)))
                    .child(x_labels),
            )
            .child(
                div()
                    .ml(px(Y_TICKS_W))
                    .flex()
                    .justify_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child(axis_label(&frame.x_label, frame.x_unit)),
            )
            .when(lines.is_empty(), |d| d.opacity(0.6))
            .into_any_element()
    }
}

/// A hue for the `index`th charge, spread by the golden angle so neighbours differ.
pub fn ammo_hue(index: usize) -> f32 {
    const GOLDEN: f32 = 0.381_966;
    (index as f32 * GOLDEN).fract()
}

/// A short sample of a line, for the legend and the lists.
pub fn swatch(color: Hsla, style: LineStyle) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, (), window, _| {
            let y = b.origin.y + b.size.height / 2.;
            let mut path = PathBuilder::stroke(px(LINE_WIDTH));
            let dashes = style.dashes(LINE_WIDTH);
            if !dashes.is_empty() {
                path = path.dash_array(&dashes);
            }
            path.move_to(point(b.origin.x, y));
            path.line_to(point(b.origin.x + b.size.width, y));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .flex_none()
    .w(px(20.))
    .h(px(10.))
}

/// Where in data units a window position falls, if over the plot.
fn data_x(
    bounds: Option<Bounds<Pixels>>,
    position: gpui_kit::Point<Pixels>,
    x_range: (f64, f64),
) -> Option<f64> {
    let b = bounds?;
    if !b.contains(&position) || b.size.width <= px(0.) {
        return None;
    }
    let f = f64::from((position.x - b.origin.x) / b.size.width);
    Some(x_range.0 + f * (x_range.1 - x_range.0))
}

/// Keeps the readout's X inside the plot.
fn clamp_mark(x: f64, range: (f64, f64)) -> f64 {
    x.clamp(range.0.min(range.1), range.0.max(range.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_land_on_round_numbers() {
        assert_eq!(
            ticks((0.0, 100.0), 5),
            vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]
        );
        assert_eq!(ticks((0.0, 1.0), 4), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        let t = ticks((-3.0, 47.0), 8);
        assert_eq!(t.first(), Some(&0.0));
    }

    #[test]
    fn limits_pad_and_round_up() {
        let (lo, hi) = limits([0.0, 437.0].into_iter(), (0.05, 0.03), true);
        assert!(lo < 0.0);
        assert_eq!(hi, 500.0);
        assert_eq!(limits(std::iter::empty(), (0.0, 0.0), false), (-5.0, 5.0));
    }

    #[test]
    fn readout_numbers_keep_four_digits() {
        assert_eq!(format(1234.567), "1235");
        assert_eq!(format(12.3456), "12.35");
        assert_eq!(format(0.5), "0.5");
        assert_eq!(format(0.0), "0");
    }

    #[test]
    fn labels_stay_short() {
        assert_eq!(format(0.000_000_000_012), "0");
        assert_eq!(format(0.000_123_4), "1.23e-4");
        assert_eq!(format(1_234_567.0), "1.235M");
        assert_eq!(format(250_000.0), "250k");
        assert_eq!(format(0.1 + 0.2), "0.3");
        // A line that is flat but for noise gets a sane range and round ticks.
        let (lo, hi) = limits([0.0, 1e-13].into_iter(), (0.05, 0.03), true);
        assert!(hi - lo >= 1.0, "{lo}..{hi}");
        assert!(ticks((lo, hi), 5).iter().all(|t| format(*t).len() <= 6));
    }
}
