//! The plot itself: axes with nice ticks, the lines, the legend, and the readout at the
//! marked X, pyfa's canvas panel without matplotlib.

use std::{cell::Cell, rc::Rc};

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Bounds, Context, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, PathBuilder, Pixels, StatefulInteractiveElement,
    Styled, canvas, div, point, prelude::FluentBuilder, px, relative,
};

use super::{
    GraphsWindow,
    style::{Lightness, LineColor, LineStyle},
};
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
        .take_while(|&v| v <= range.1 + step * 1e-9)
        .take(100)
        .collect()
}

/// A number the way pyfa's readouts print it: four significant digits, no trailing zeros.
pub fn format(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v.is_finite() {
            "0".to_owned()
        } else {
            v.to_string()
        };
    }
    let digits = (4 - 1 - v.abs().log10().floor() as i32).max(0) as usize;
    let s = format!("{v:.digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

fn axis_label(label: &str, unit: Option<&str>) -> String {
    match unit {
        Some(unit) => format!("{label}, {unit}"),
        None => label.to_owned(),
    }
}

/// Fraction of the way through `range`.
fn frac(v: f64, range: (f64, f64)) -> f32 {
    ((v - range.0) / (range.1 - range.0)) as f32
}

pub fn render(
    lines: &[PlotLine],
    frame: &Frame,
    mark: Option<f64>,
    bounds: &Rc<Cell<Option<Bounds<Pixels>>>>,
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let theme = cx.theme();
    let (grid, axis, muted) = (theme.border, theme.muted_foreground, theme.muted_foreground);
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
                .right(px(4.))
                .top(relative(1.0 - frac(v, frame.y_range)))
                .mt(px(-TICK_LABEL_H / 2.))
                .h(px(TICK_LABEL_H))
                .text_xs()
                .text_color(muted)
                .child(format(v))
        }));
    let x_labels = div()
        .relative()
        .flex_1()
        .h(px(TICK_LABEL_H + 2.))
        .children(x_ticks.iter().map(|&v| {
            div()
                .absolute()
                .left(relative(frac(v, frame.x_range)))
                .ml(px(-TICK_LABEL_W / 2.))
                .w(px(TICK_LABEL_W))
                .flex()
                .justify_center()
                .text_xs()
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
    let store = Rc::clone(bounds);
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
                if let Some(x) = mark {
                    let top = to_px(x, y_range.1);
                    let bottom = to_px(x, y_range.0);
                    stroke(window, &[top, bottom], 1.0, &[px(2.), px(2.)], axis);
                }
            });
            let corners = [
                to_px(x_range.0, y_range.0),
                to_px(x_range.1, y_range.0),
                to_px(x_range.1, y_range.1),
                to_px(x_range.0, y_range.1),
                to_px(x_range.0, y_range.0),
            ];
            stroke(window, &corners, 1.0, &[], axis);
        },
    )
    .absolute()
    .size_full();

    let legend = (frame.legend && !lines.is_empty()).then(|| {
        v_flex()
            .absolute()
            .top(px(6.))
            .right(px(6.))
            .p_1()
            .gap_0p5()
            .rounded(px(3.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background.opacity(0.85))
            .text_xs()
            .children(
                lines
                    .iter()
                    .zip(&colors)
                    .filter(|(line, _)| line.in_legend)
                    .map(|(line, &color)| {
                        h_flex()
                            .gap_1()
                            .child(swatch(color, line.style))
                            .child(line.label.clone())
                    }),
            )
    });

    let readout = mark.map(|x| {
        let fx = frac(x, frame.x_range);
        let x_text = match frame.x_unit {
            Some(unit) => format!("{} {unit}", format(x)),
            None => format(x),
        };
        let mut labels: Vec<AnyElement> = vec![
            div()
                .absolute()
                .top(px(2.))
                .left(relative(fx))
                .ml(px(4.))
                .px_1()
                .rounded(px(2.))
                .bg(theme.background.opacity(0.85))
                .text_xs()
                .child(x_text)
                .into_any_element(),
        ];
        for (line, &color) in lines.iter().zip(&colors) {
            let Some(y) = line.series.at(x) else { continue };
            let fy = frac(y, frame.y_range).clamp(0.0, 1.0);
            labels.push(
                div()
                    .absolute()
                    .left(relative(fx))
                    .top(relative(1.0 - fy))
                    .ml(px(4.))
                    .mt(px(-TICK_LABEL_H))
                    .px_1()
                    .rounded(px(2.))
                    .bg(theme.background.opacity(0.85))
                    .border_1()
                    .border_color(color)
                    .text_xs()
                    .text_color(color)
                    .child(format(y))
                    .into_any_element(),
            );
        }
        labels
    });

    let area = div()
        .id("graph-plot")
        .relative()
        .flex_1()
        .h_full()
        .min_w_0()
        .child(plot)
        .children(legend)
        .children(readout.into_iter().flatten())
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
                this.pinned_mark = None;
                cx.notify();
            }),
        )
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !hovered {
                this.hover_mark = None;
                cx.notify();
            }
        }));

    v_flex()
        .size_full()
        .p_2()
        .child(
            div()
                .ml(px(Y_TICKS_W))
                .text_xs()
                .text_color(muted)
                .child(axis_label(&frame.y_label, frame.y_unit)),
        )
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .mt_1()
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
                .text_color(muted)
                .child(axis_label(&frame.x_label, frame.x_unit)),
        )
        .when(lines.is_empty(), |d| d.opacity(0.7))
        .into_any_element()
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
    .w(px(22.))
    .h(px(10.))
}

/// Where in data units a window position falls, if over the plot.
pub fn data_x(
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
pub fn clamp_mark(x: f64, range: (f64, f64)) -> f64 {
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
}
