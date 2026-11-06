//! The right side: the fit's stats in collapsible sections, as pyfa groups them.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, relative,
};

use super::{
    Gyfa, Loaded,
    widgets::{
        DAMAGE_COLORS, color, duration, gauge, icon, icon_box, or_dash, section_title, short,
    },
};
use crate::{
    engine::{Cap, Layer, Resource, Slots, Stats},
    icons::Icon,
    theme::Palette,
};

impl Gyfa {
    pub(super) fn stats_panel(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let character = h_flex()
            .gap_2()
            .p_2()
            .border_b_1()
            .border_color(theme.border)
            .child("Character:")
            .child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .bg(theme.secondary)
                    .child("All 5"),
            );
        let Some(s) = &self.stats else {
            return v_flex()
                .size_full()
                .bg(theme.sidebar)
                .child(character)
                .into_any_element();
        };
        let ehp: f64 = [s.shield, s.armor, s.hull]
            .iter()
            .filter_map(|l| l.ehp)
            .sum();
        let sections = [
            (
                "Resources",
                "Resources".to_owned(),
                resources(loaded, s, self.palette, cx),
            ),
            (
                "Resistances",
                format!("Resistances (Effective HP: {})", short(ehp)),
                resistances(loaded, s, cx),
            ),
            (
                "Firepower",
                "Firepower".to_owned(),
                firepower(loaded, s, cx),
            ),
            (
                "Capacitor",
                "Capacitor".to_owned(),
                capacitor(loaded, s, cx),
            ),
            ("Targeting", "Targeting & Misc".to_owned(), targeting(s, cx)),
        ];
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(character)
            .child(
                v_flex()
                    .id("stats-sections")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_2()
                    .children(
                        sections
                            .into_iter()
                            .map(|(key, title, body)| self.section(key, title, body, cx)),
                    ),
            )
            .into_any_element()
    }

    fn section(
        &self,
        key: &'static str,
        title: String,
        body: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let collapsed = self.collapsed_sections.contains(key);
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .id(SharedString::from(format!("section-{key}")))
                    .gap_1()
                    .child(
                        div()
                            .w(px(10.))
                            .text_xs()
                            .child(if collapsed { "▸" } else { "▾" }),
                    )
                    .child(section_title(title))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !this.collapsed_sections.remove(key) {
                            this.collapsed_sections.insert(key);
                        }
                        cx.notify();
                    })),
            )
            .when(!collapsed, |s| s.child(div().pl_3().child(body)))
            .into_any_element()
    }
}

fn labeled_icon(loaded: &Loaded, icon_id: i32, size: f32) -> Div {
    icon_box(&loaded.catalog, &loaded.icons, Icon::Id(icon_id), size)
}

fn warn_text(text: String, warn: bool, cx: &Context<Gyfa>) -> Div {
    div()
        .when(warn, |d| d.text_color(cx.theme().danger))
        .child(text)
}

fn resources(loaded: &Loaded, s: &Stats, palette: Palette, cx: &Context<Gyfa>) -> AnyElement {
    let slot_counts = h_flex()
        .gap_3()
        .children(
            [
                (icon::TURRET, s.turret),
                (icon::LAUNCHER, s.launcher),
                (icon::DRONE_BAY, s.drones_launched),
            ]
            .into_iter()
            .map(|(icon_id, slots)| {
                let total = slots.total.unwrap_or(0);
                h_flex()
                    .gap_1()
                    .child(labeled_icon(loaded, icon_id, 24.))
                    .child(warn_text(
                        format!("{}/{total}", slots.used),
                        slots_over(slots),
                        cx,
                    ))
            }),
        )
        .child(
            h_flex()
                .gap_1()
                .child(labeled_icon(loaded, icon::CALIBRATION, 24.))
                .child(warn_text(
                    format!(
                        "{:.0}/{:.0}",
                        s.calibration.used,
                        s.calibration.output.unwrap_or(0.0)
                    ),
                    resource_over(s.calibration),
                    cx,
                )),
        );
    let bar = |icon_id: i32, r: Resource, unit: &str| {
        let out = r.output.unwrap_or(0.0);
        let fraction = match (out > 0.0, r.used > 0.0) {
            (true, _) => r.used / out,
            (false, true) => 1.0,
            (false, false) => 0.0,
        };
        let fill = if resource_over(r) {
            palette.red
        } else {
            palette.green
        };
        h_flex()
            .gap_1()
            .child(labeled_icon(loaded, icon_id, 24.))
            .child(
                v_flex()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}/{}{unit}", short(r.used), short(out))),
                    )
                    .child(gauge(
                        fraction,
                        fill,
                        format!("{:.1}%", fraction * 100.0),
                        110.,
                        cx,
                    )),
            )
    };
    v_flex()
        .gap_2()
        .child(slot_counts)
        .child(
            h_flex()
                .gap_4()
                .child(
                    v_flex()
                        .gap_1()
                        .child(bar(icon::CPU, s.cpu, " tf"))
                        .child(bar(icon::POWERGRID, s.powergrid, " MW")),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(bar(icon::DRONE_BAY, s.drone_bay, " m³"))
                        .child(bar(icon::DRONE_BANDWIDTH, s.drone_bandwidth, " Mbit/s")),
                ),
        )
        .into_any_element()
}

fn resistances(loaded: &Loaded, s: &Stats, cx: &Context<Gyfa>) -> AnyElement {
    let head = h_flex()
        .gap_1()
        .child(div().w(px(22.)))
        .children(
            [icon::EM, icon::THERMAL, icon::KINETIC, icon::EXPLOSIVE]
                .into_iter()
                .map(|icon_id| {
                    h_flex()
                        .w(px(58.))
                        .justify_center()
                        .child(labeled_icon(loaded, icon_id, 20.))
                }),
        )
        .child(div().text_color(cx.theme().muted_foreground).child("EHP"));
    let row = |icon_id: i32, layer: Layer| {
        h_flex()
            .gap_1()
            .child(labeled_icon(loaded, icon_id, 20.))
            .children(
                layer
                    .resists
                    .iter()
                    .zip(DAMAGE_COLORS)
                    .map(|(r, fill)| gauge(*r, fill, format!("{:.1}%", r * 100.0), 58., cx)),
            )
            .child(div().pl_1().child(or_dash(layer.ehp, short)))
    };
    v_flex()
        .gap_1()
        .child(head)
        .child(row(icon::SHIELD, s.shield))
        .child(row(icon::ARMOR, s.armor))
        .child(row(icon::HULL, s.hull))
        .into_any_element()
}

fn firepower(loaded: &Loaded, s: &Stats, cx: &Context<Gyfa>) -> AnyElement {
    let block = |icon_id: i32, label: String, value: String| {
        h_flex()
            .gap_1()
            .child(labeled_icon(loaded, icon_id, 24.))
            .child(
                v_flex()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(value),
            )
    };
    let parts = [s.dps.em, s.dps.thermal, s.dps.kinetic, s.dps.explosive];
    let total: f64 = parts.iter().sum();
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_4()
                .child(block(
                    icon::TURRET,
                    "Weapon".into(),
                    format!("{:.1} DPS", s.weapon_dps),
                ))
                .child(block(
                    icon::DRONE_BANDWIDTH,
                    "Drone".into(),
                    format!("{:.1} DPS", s.drone_dps),
                ))
                .child(block(
                    icon::DAMAGE,
                    format!("Volley: {}", short(s.volley.total())),
                    format!("DPS: {:.1}", s.dps.total()),
                )),
        )
        .when(total > 0.0, |col| {
            col.child(
                h_flex()
                    .w_full()
                    .h(px(6.))
                    .rounded(px(2.))
                    .overflow_hidden()
                    .children(parts.iter().zip(DAMAGE_COLORS).map(|(part, fill)| {
                        div()
                            .h_full()
                            .w(relative((*part / total) as f32))
                            .bg(color(fill))
                    })),
            )
        })
        .into_any_element()
}

fn capacitor(loaded: &Loaded, s: &Stats, cx: &Context<Gyfa>) -> AnyElement {
    let stability = match s.cap {
        Some(Cap::Stable(level)) => format!("Stable: {:.1}%", level * 100.0),
        Some(Cap::Lasts(secs)) => format!("Lasts {}", duration(secs)),
        None => "–".into(),
    };
    h_flex()
        .gap_6()
        .child(
            h_flex()
                .gap_1()
                .child(labeled_icon(loaded, icon::CAPACITOR, 24.))
                .child(
                    v_flex()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Total: {} GJ", or_dash(s.cap_amount, short))),
                        )
                        .child(stability),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .child(labeled_icon(loaded, icon::RECHARGE, 24.))
                .child(
                    v_flex()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Balance"),
                        )
                        .child(or_dash(s.cap_balance, |v| format!("{v:+.1} GJ/s"))),
                ),
        )
        .into_any_element()
}

fn targeting(s: &Stats, cx: &Context<Gyfa>) -> AnyElement {
    let left = [
        (
            "Targets",
            or_dash(s.locks.map(f64::from), |v| format!("{v:.0}")),
        ),
        (
            "Range",
            or_dash(s.lock_range, |v| format!("{:.1} km", v / 1000.0)),
        ),
        ("Scan res.", or_dash(s.scan_res, |v| format!("{v:.0} mm"))),
        (
            "Sensor str.",
            or_dash(s.sensor_strength, |v| format!("{v:.1}")),
        ),
    ];
    let right = [
        ("Speed", or_dash(s.speed, |v| format!("{v:.1} m/s"))),
        ("Align time", or_dash(s.align_time, |v| format!("{v:.2} s"))),
        ("Signature", or_dash(s.sig_radius, |v| format!("{v:.0} m"))),
        (
            "Warp speed",
            or_dash(s.warp_speed, |v| format!("{v:.2} AU/s")),
        ),
    ];
    let column = |pairs: [(&'static str, String); 4]| {
        v_flex()
            .gap_0p5()
            .children(pairs.into_iter().map(|(name, value)| {
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(76.))
                            .text_color(cx.theme().muted_foreground)
                            .child(name),
                    )
                    .child(value)
            }))
    };
    h_flex()
        .gap_4()
        .child(column(left))
        .child(column(right))
        .into_any_element()
}

fn slots_over(s: Slots) -> bool {
    s.used > s.total.unwrap_or(0)
}

fn resource_over(r: Resource) -> bool {
    r.output.is_some_and(|out| r.used > out + 1e-6)
}
