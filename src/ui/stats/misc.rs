//! Capacitor (capacitorViewFull) and the targeting and misc text columns
//! (targetingMiscViewMinimal).

use std::fmt::Write;

use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{App, Context, IntoElement, ParentElement, Styled, div, px};

use super::{Pane, Section, format, muted, tip, title};
use crate::{
    engine::Cap,
    ui::{Gyfa, widgets::icon},
};

pub(super) fn capacitor(p: &Pane<'_>) -> Section {
    let s = p.stats;
    let d = s.cap_detail;
    let amount = s.cap_amount.unwrap_or(0.0);
    let capacity_hover = match d.recharge_time {
        Some(secs) => format!(
            "{amount:.1}\nRecharge time: {}",
            format::amount(secs, 3, 0, 9)
        ),
        None => format!("{amount:.1}"),
    };
    let (state, time) = match s.cap {
        Some(Cap::Stable(level)) => ("Stable: ", format!("{:.1}%", level * 100.0)),
        Some(Cap::Lasts(secs)) => ("Lasts ", format::minutes_seconds(secs)),
        None => ("Lasts ", "0s".to_owned()),
    };
    let left = h_flex()
        .gap_1()
        .child(p.icon_tip("cap-stability", icon::CAPACITOR, 24., "Capacitor stability"))
        .child(
            v_flex()
                .child(
                    h_flex()
                        .child("Total: ")
                        .child(tip(
                            "cap-total",
                            format::amount(amount, 3, 0, 9),
                            capacity_hover,
                        ))
                        .child(" GJ"),
                )
                .child(h_flex().child(state).child(time)),
        );

    // refine's neutralizer resistance is the share resisted; pyfa's attribute is what gets through.
    let through = 1.0 - d.neut_resist;
    let delta = d.recharge - d.used;
    let partial = |through: f64| {
        let r = (through * 1e4).round();
        r > 0.0 && r < 1e4
    };
    let delta_hover = if d.recharge != 0.0 || d.used != 0.0 {
        let mut text = format!(
            "Capacitor delta:\n  +{} GJ/s\n  -{} GJ/s",
            format::amount(d.recharge, 3, 0, 3),
            format::amount(d.used, 3, 0, 3)
        );
        if (delta * 1000.0).round() > 0.0 && partial(through) {
            let _ = write!(
                text,
                "\n\nEffective excessive gain:\n  +{} GJ/s",
                format::amount(delta / through, 3, 0, 3)
            );
        }
        text
    } else {
        format!("{delta:.1}")
    };
    let mut resist_hover = String::from("Neutralizer resistance");
    if amount > 0.0 && partial(through) {
        let _ = write!(
            resist_hover,
            "\nEffective capacity: {} GJ",
            format::amount(amount / through, 3, 0, 9)
        );
    }
    let right = h_flex()
        .gap_1()
        .child(p.icon_tip("cap-extra", icon::RECHARGE, 24., "Extra stats"))
        .child(
            v_flex()
                .items_center()
                .child(tip(
                    "cap-delta",
                    format!("{} GJ/s", format::signed(delta, 3, 0, 0)),
                    delta_hover,
                ))
                .child(tip(
                    "cap-resist",
                    format!("{}%", format::amount(d.neut_resist * 100.0, 3, 0, 0)),
                    resist_hover,
                )),
        );
    let body = h_flex()
        .w_full()
        .child(left.flex_1())
        .child(right.flex_1())
        .into_any_element();
    Section {
        key: "Capacitor",
        title: title("Capacitor"),
        body,
        menu: None,
        action: None,
    }
}

/// Signature radii pyfa lists lock times for.
const LOCK_RADII: [(&str, i32); 8] = [
    ("Pod", 25),
    ("Interceptor", 33),
    ("Frigate", 38),
    ("Destroyer", 83),
    ("Cruiser", 130),
    ("Battlecruiser", 265),
    ("Battleship", 420),
    ("Carrier", 3000),
];

/// The game's lock time formula, capped at 30 minutes like pyfa's (eos/calc.py).
fn lock_time(scan_res: f64, sig: f64) -> f64 {
    (40_000.0 / scan_res / sig.asinh().powi(2)).min(30.0 * 60.0)
}

fn lock_times(scan_res: Option<f64>) -> String {
    let Some(res) = scan_res.filter(|&r| r > 0.0) else {
        return String::new();
    };
    let mut text = String::from("Lock Times");
    for (size, radius) in LOCK_RADII {
        let left = format!("{:.1}s", lock_time(res, f64::from(radius)));
        let _ = write!(text, "\n{left:>7}   {size} [{radius}]");
    }
    text
}

/// A label, a value with its tooltip, the rows pyfa's minimal view is made of.
type Line = (&'static str, String, String);

pub(super) fn targeting(p: &Pane<'_>, cx: &Context<Gyfa>) -> Section {
    let s = p.stats;
    let m = &s.misc;
    let or_zero = |v: Option<f64>| v.unwrap_or(0.0);
    let plain = |v: f64, unit: &str, prec: i32, highest: i32| {
        let text = format::amount(v, prec, 0, highest);
        (
            if unit.is_empty() {
                text
            } else {
                format!("{text} {unit}")
            },
            format!("{v:.1}"),
        )
    };

    let jam = (m.jam_chance * 1000.0).round() / 10.0;
    let sensor_kind = m.sensor_kind.unwrap_or("");
    let sensor = if jam > 0.0 {
        (
            format!(
                "{} ({}%)",
                format::amount(or_zero(s.sensor_strength), 3, 0, 0),
                format::amount(jam, 3, 0, 0)
            ),
            format!(
                "Type: {sensor_kind}\n{}% chance to be jammed",
                format::amount(jam, 3, 0, 0)
            ),
        )
    } else {
        (
            format::amount(or_zero(s.sensor_strength), 3, 0, 0),
            format!("Type: {sensor_kind}"),
        )
    };
    let targets = s.locks.map_or(0.0, f64::from);
    let targeting: [Line; 5] = [
        (
            "Targets",
            format::amount(targets, 3, 0, 0),
            format!("{targets:.1}"),
        ),
        {
            let (value, hover) = plain(or_zero(s.lock_range) / 1000.0, "km", 3, 0);
            ("Range", value, hover)
        },
        (
            "Scan res.",
            plain(or_zero(s.scan_res), "mm", 3, 0).0,
            lock_times(s.scan_res),
        ),
        ("Sensor str.", sensor.0, sensor.1),
        {
            let (value, hover) = plain(or_zero(m.drone_control_range) / 1000.0, "km", 3, 0);
            ("Drone range", value, hover)
        },
    ];

    let align = or_zero(s.align_time);
    let align_hover = format!(
        "Align: {align:.3}s\nMass: {}kg\nAgility: {:.3}x",
        format::grouped(or_zero(m.mass), 0),
        or_zero(m.agility)
    );
    let warp_hover = format!(
        "Max Warp Distance: {:.1} AU\nWarp Core Strength: {:.1}",
        or_zero(m.max_warp_range),
        m.warp_core_strength
    );
    let capacity = or_zero(s.cargo_capacity);
    // Every special hold counts towards the extra figure, the maintenance bay too, as in pyfa.
    let extra: f64 = m.holds.iter().map(|&(_, v)| v).sum();
    let cargo_text = if extra > 0.0 {
        format!(
            "{}+{} m³",
            format::amount(capacity, 4, 0, 9),
            format::amount(extra, 4, 0, 9)
        )
    } else {
        format!("{} m³", format::amount(capacity, 4, 0, 9))
    };
    let used: f64 = p.fit.hold_volume(&p.loaded.catalog, p.implants_in_cargo);
    let mut cargo_hover = format!(
        "Cargohold: {}m³ / {}m³",
        format::grouped(used, 2),
        format::grouped(capacity, 2)
    );
    for &(name, v) in &m.holds {
        let _ = write!(cargo_hover, "\n{name}: {}m³", format::grouped(v, 2));
    }
    let misc: [Line; 5] = [
        {
            let (value, hover) = plain(or_zero(s.speed), "m/s", 3, 0);
            ("Speed", value, hover)
        },
        ("Align time", plain(align, "s", 3, 0).0, align_hover),
        (
            "Signature",
            plain(or_zero(s.sig_radius), "m", 3, 9).0,
            format!("Probe Size: {:.3}", or_zero(m.probe_size)),
        ),
        (
            "Warp Speed",
            plain(or_zero(s.warp_speed), "AU/s", 3, 0).0,
            warp_hover,
        ),
        ("Cargo", cargo_text, cargo_hover),
    ];

    let column = |lines: [Line; 5], cx: &App| {
        v_flex()
            .flex_1()
            .gap_0p5()
            .children(lines.into_iter().map(|(name, value, hover)| {
                h_flex()
                    .gap_1()
                    .child(muted(format!("{name}:"), cx).w(px(76.)))
                    .child(tip(format!("misc-{name}"), value, hover))
            }))
    };
    let body = h_flex()
        .w_full()
        .gap_3()
        .child(column(targeting, cx))
        .child(div().w_px().h(px(90.)).bg(cx.theme().border))
        .child(column(misc, cx))
        .into_any_element();
    Section {
        key: "Targeting",
        title: title("Targeting & Misc"),
        body,
        menu: None,
        action: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_time_matches_the_game() {
        // 40000 / 600 / asinh(38)^2
        let t = lock_time(600.0, 38.0);
        assert!((t - 3.554).abs() < 1e-3, "lock time {t}");
        assert_eq!(lock_time(1.0, 25.0), 1800.0, "capped at 30 minutes");
    }
}
