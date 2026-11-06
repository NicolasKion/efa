//! pyfa's resources view (builtinStatsViews/resourcesViewFull.py): hardpoints, active drones or
//! fighter tubes, calibration, then gauges for fitting resources and the bay the open tab is about.

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div};

use super::{AdditionsTab, Pane, Section, format, ids, muted, tip, title, warn};
use crate::{
    engine::{Resource, Slots},
    theme::Rgb,
    ui::{
        Gyfa,
        widgets::{gauge, icon},
    },
};

pub(super) fn section(p: &Pane<'_>, cx: &Context<Gyfa>) -> Section {
    let s = p.stats;
    let fighters = s.fighter_tubes.total.unwrap_or(0) > 0;
    let (minions, minions_tip) = if fighters {
        (
            (ids::FIGHTER_TUBES, s.fighter_tubes),
            "Fighter squadrons active",
        )
    } else {
        ((icon::DRONE_BAY, s.drones_launched), "Drones active")
    };
    let counts = [
        ("turret", icon::TURRET, s.turret, "Turret hardpoints"),
        (
            "launcher",
            icon::LAUNCHER,
            s.launcher,
            "Launcher hardpoints",
        ),
        ("minions", minions.0, minions.1, minions_tip),
    ];
    let calibration = Slots {
        used: s.calibration.used.round() as u32,
        total: s.calibration.output.map(|v| v.round() as u32),
    };
    let top = h_flex().w_full().justify_between().children(
        counts
            .into_iter()
            .chain([("calibration", icon::CALIBRATION, calibration, "Calibration")])
            .map(|(key, icon_id, slots, text)| {
                let total = slots.total.unwrap_or(0);
                h_flex()
                    .gap_1()
                    .child(p.icon_tip(format!("res-{key}"), icon_id, 24., text))
                    .child(warn(
                        format!("{}/{total}", slots.used),
                        slots.used > total,
                        cx,
                    ))
            }),
    );

    let cargo_used: f64 = p
        .fit
        .cargo
        .iter()
        .filter_map(|&(id, count)| {
            p.loaded
                .catalog
                .get(id)
                .map(|t| t.volume * f64::from(count))
        })
        .sum();
    let bay = if p.additions_tab == AdditionsTab::Cargo {
        (
            "cargo",
            ids::CARGO,
            Resource {
                used: cargo_used,
                output: s.cargo_capacity,
            },
            "Cargo bay",
        )
    } else if fighters {
        ("fighter-bay", icon::DRONE_BAY, s.fighter_bay, "Fighter bay")
    } else {
        ("drone-bay", icon::DRONE_BAY, s.drone_bay, "Drone bay")
    };

    let bar = |key: &str, icon_id: i32, r: Resource, text: &str, unit: &str, prec: i32| {
        let out = r.output.unwrap_or(0.0);
        let fraction = match (out > 0.0, r.used > 0.0) {
            (true, _) => r.used / out,
            (false, true) => 1.0,
            (false, false) => 0.0,
        };
        let over = r.output.is_some_and(|out| r.used > out + 1e-6);
        let fill: Rgb = if over { p.palette.red } else { p.palette.green };
        h_flex()
            .w_full()
            .gap_1()
            .child(p.icon_tip(format!("res-{key}"), icon_id, 24., text))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(tip(
                        format!("res-{key}-value"),
                        muted(
                            format!(
                                "{}/{}{unit}",
                                format::amount(r.used, prec, 0, 9),
                                format::amount(out, prec, 0, 9)
                            ),
                            cx,
                        ),
                        format!("{:.1}/{out:.1}", r.used),
                    ))
                    .child(
                        gauge(fraction, fill, format!("{:.2}%", fraction * 100.0), 0., cx)
                            // In a column, filling the room means full width rather than flex.
                            .w_full(),
                    ),
            )
    };
    let body = v_flex()
        .gap_2()
        .child(top)
        .child(div().h_px().w_full().bg(cx.theme().border))
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .child(
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(bar("cpu", icon::CPU, s.cpu, "CPU", " tf", 4))
                        .child(bar(
                            "pg",
                            icon::POWERGRID,
                            s.powergrid,
                            "PowerGrid",
                            " MW",
                            4,
                        )),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(bar(bay.0, bay.1, bay.2, bay.3, " m³", 3))
                        .child(bar(
                            "bandwidth",
                            icon::DRONE_BANDWIDTH,
                            s.drone_bandwidth,
                            "Drone bandwidth",
                            " mbit/s",
                            3,
                        )),
                ),
        )
        .into_any_element();
    Section {
        key: "Resources",
        title: title("Resources"),
        body,
        menu: None,
        action: None,
    }
}
