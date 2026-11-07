//! pyfa's vector picker: a circle to drag a speed and heading on, for attacker and target.

use std::{cell::Cell, rc::Rc};

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::{
    AnyElement, Bounds, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, PathBuilder, Pixels, Point, SharedString,
    Styled, canvas, div, point, px,
};

use super::GraphsWindow;

const SIZE: f32 = 90.;

/// A speed as a fraction of the maximum and a heading in degrees, 0 pointing at the target.
#[derive(Debug, Clone)]
pub struct Vector {
    pub length: f64,
    pub angle: f64,
    /// When the speed is the X axis, only the heading can be picked.
    pub direction_only: bool,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Vector {
    pub fn new(length: f64, angle: f64) -> Self {
        Self {
            length,
            angle,
            direction_only: false,
            bounds: Rc::default(),
        }
    }

    /// Point the vector at a window position; true when it changed.
    pub fn set_from(&mut self, position: Point<Pixels>) -> bool {
        let Some(b) = self.bounds.get() else {
            return false;
        };
        let center = b.center();
        let radius = f64::from(b.size.width / 2.);
        let dx = f64::from(position.x - center.x);
        let dy = f64::from(center.y - position.y);
        let distance = dx.hypot(dy);
        if radius <= 0.0 || distance < 1.0 {
            return false;
        }
        let angle = dy.atan2(dx).to_degrees().round();
        let length = (distance / radius).min(1.0);
        // Snap near the rim to full speed, as nobody can hit 100% exactly by hand.
        let length = if length > 0.95 {
            1.0
        } else {
            (length * 100.0).round() / 100.0
        };
        let moved = |a: f64, b: f64| (a - b).abs() > f64::EPSILON;
        let changed =
            moved(angle, self.angle) || (!self.direction_only && moved(length, self.length));
        self.angle = angle;
        if !self.direction_only {
            self.length = length;
        }
        changed
    }
}

/// Which of the two pickers an event is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Attacker,
    Target,
}

pub fn render(
    vector: &Vector,
    which: Which,
    label: &'static str,
    cx: &mut Context<GraphsWindow>,
) -> AnyElement {
    let theme = cx.theme();
    let (ring, cross, arrow) = (theme.border, theme.border.opacity(0.5), theme.primary);
    let (length, angle) = (
        if vector.direction_only {
            1.0
        } else {
            vector.length
        },
        vector.angle,
    );
    let store = Rc::clone(&vector.bounds);
    let dial = canvas(
        move |b, _, _| store.set(Some(b)),
        move |b, (), window, _| {
            let center = b.center();
            let radius = b.size.width / 2. - px(2.);
            let mut circle = PathBuilder::stroke(px(1.));
            let steps = 48;
            for i in 0..=steps {
                let t = f64::from(i) / f64::from(steps) * std::f64::consts::TAU;
                let p = point(
                    center.x + radius * t.cos() as f32,
                    center.y - radius * t.sin() as f32,
                );
                if i == 0 {
                    circle.move_to(p);
                } else {
                    circle.line_to(p);
                }
            }
            if let Ok(path) = circle.build() {
                window.paint_path(path, ring);
            }
            // Faint crosshairs, so headings are easy to judge.
            for (from, to) in [
                (
                    point(center.x - radius, center.y),
                    point(center.x + radius, center.y),
                ),
                (
                    point(center.x, center.y - radius),
                    point(center.x, center.y + radius),
                ),
            ] {
                let mut line = PathBuilder::stroke(px(1.));
                line.move_to(from);
                line.line_to(to);
                if let Ok(path) = line.build() {
                    window.paint_path(path, cross);
                }
            }
            let rad = angle.to_radians();
            let tip = point(
                center.x + radius * (length * rad.cos()) as f32,
                center.y - radius * (length * rad.sin()) as f32,
            );
            let mut line = PathBuilder::stroke(px(2.));
            line.move_to(center);
            line.line_to(tip);
            if let Ok(path) = line.build() {
                window.paint_path(path, arrow);
            }
            let dot = Bounds::centered_at(tip, gpui_kit::size(px(7.), px(7.)));
            window.paint_quad(gpui_kit::fill(dot, arrow).corner_radii(px(3.5)));
        },
    )
    .size(px(SIZE));

    let readout: SharedString = if vector.direction_only {
        format!("{angle:.0}°").into()
    } else {
        format!("{:.0}% at {angle:.0}°", length * 100.0).into()
    };
    v_flex()
        .items_center()
        .gap_1p5()
        .text_xs()
        .child(div().font_weight(FontWeight::MEDIUM).child(label))
        .child(
            div()
                .id(SharedString::from(format!("vector-{which:?}")))
                .cursor_crosshair()
                .child(dial)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.drag_vector = Some(which);
                        this.set_vector(which, event.position, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    if event.dragging() && this.drag_vector == Some(which) {
                        this.set_vector(which, event.position, cx);
                    }
                })),
        )
        .child(div().text_color(theme.muted_foreground).child(readout))
        .into_any_element()
}
