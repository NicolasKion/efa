//! What the fit puts out: firepower (firepowerViewFull), mining yield in its place
//! (miningyieldViewFull) and remote repairs (outgoingViewFull).

use std::rc::Rc;

use gpui_kit::component::{h_flex, menu::PopupMenuItem, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, WeakEntity, div, prelude::FluentBuilder, px, relative,
};

use super::{Pane, Section, format, ids, muted, tip, title};
use crate::{
    engine::{Damage, FirepowerDetail, MiningRate, SpoolRange},
    target_profile::applied,
    ui::{
        Gyfa,
        actions::Action,
        profile_menus::{FirstProfile, PickProfile, target_profile_menu},
        widgets::{DAMAGE_COLORS, color, icon},
    },
};

/// Significant digits pyfa's firepower and outgoing views print.
const PREC: i32 = 3;

/// pyfa's spool marker after a value that depends on spool-up.
const SPOOL_MARK: &str = "\u{02e2}";

fn has_spool(spool: SpoolRange) -> bool {
    (format::round_to_prec(spool.pre, PREC) - format::round_to_prec(spool.full, PREC)).abs() > 1e-9
}

/// pyfa's firepower tooltip: the spool range if there is one, then each damage type's share.
fn dps_tooltip(normal: Damage, spool: SpoolRange) -> String {
    let spooling = has_spool(spool);
    let mut lines = Vec::new();
    if spooling {
        lines.push(format!(
            "Spool up: {}-{}",
            format::amount(spool.pre, PREC, 0, 0),
            format::amount(spool.full, PREC, 0, 0)
        ));
    }
    let total = normal.total();
    if total != 0.0 {
        if spooling {
            lines.push(String::new());
            lines.push(format!("Current: {}", format::amount(total, PREC, 0, 0)));
        }
        let indent = if spooling { "  " } else { "" };
        for (name, value) in [
            ("Em", normal.em),
            ("Thermal", normal.thermal),
            ("Kinetic", normal.kinetic),
            ("Explosive", normal.explosive),
        ] {
            if value != 0.0 {
                lines.push(format!(
                    "{indent}{name}: {}%",
                    format::amount(value / total * 100.0, 3, 0, 0)
                ));
            }
        }
    }
    lines.join("\n")
}

/// A spool range scaled the way the current value changed from `raw` to `applied`.
fn scaled(range: SpoolRange, applied: Damage, raw: Damage) -> SpoolRange {
    let factor = if raw.total() > 0.0 {
        applied.total() / raw.total()
    } else {
        1.0
    };
    SpoolRange {
        pre: range.pre * factor,
        full: range.full * factor,
    }
}

fn minus(a: Damage, b: Damage) -> Damage {
    Damage {
        em: a.em - b.em,
        thermal: a.thermal - b.thermal,
        kinetic: a.kinetic - b.kinetic,
        explosive: a.explosive - b.explosive,
    }
}

/// An icon with a caption over a value, the block pyfa's firepower and mining views are built of.
/// Blocks share their row equally, so they line up with the columns of the other sections.
fn block(p: &Pane<'_>, key: &str, icon_id: i32, caption: &str, value: AnyElement) -> AnyElement {
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(p.icon(icon_id, 24.))
        .child(v_flex().child(div().child(caption.to_owned())).child(value))
        .id(format!("block-{key}"))
        .into_any_element()
}

/// The small button pyfa puts at the end of the firepower and mining views to swap them.
fn swap_button(
    p: &Pane<'_>,
    key: &'static str,
    icon_id: i32,
    text: &'static str,
    cx: &Context<Gyfa>,
) -> AnyElement {
    tip(key, p.icon(icon_id, 16.), text)
        .flex_none()
        .self_start()
        .cursor_pointer()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.stats_view.mining = !this.stats_view.mining;
            cx.notify();
        }))
        .into_any_element()
}

pub(super) fn firepower(
    p: &Pane<'_>,
    this: &WeakEntity<Gyfa>,
    factor_reload: bool,
    cx: &Context<Gyfa>,
) -> Section {
    let s = p.stats;
    let target = p.fit.target_profile.as_ref();
    // pyfa's "Effective" numbers only take the target's resists off. Spool-up scales every
    // damage type alike, so the spool ranges scale by the same factor as the current value.
    let (raw_weapon, raw_volley, raw_dps) = (minus(s.dps, s.firepower.drone_dps), s.volley, s.dps);
    let (dps, volley) = (applied(raw_dps, target), applied(raw_volley, target));
    let drone = applied(s.firepower.drone_dps, target);
    let weapon = minus(dps, drone);
    let f = FirepowerDetail {
        drone_dps: drone,
        weapon_dps: scaled(s.firepower.weapon_dps, weapon, raw_weapon),
        volley: scaled(s.firepower.volley, volley, raw_volley),
        dps: scaled(s.firepower.dps, dps, raw_dps),
    };
    let no_spool = SpoolRange {
        pre: drone.total(),
        full: drone.total(),
    };
    let value = |key: &str, normal: Damage, spool: SpoolRange, suffix: &str| {
        let mark = if has_spool(spool) { SPOOL_MARK } else { "" };
        tip(
            format!("fp-{key}"),
            format!(
                "{}{mark}{suffix}",
                format::amount(normal.total(), PREC, 0, 0)
            ),
            dps_tooltip(normal, spool),
        )
        .into_any_element()
    };

    let parts = [dps.em, dps.thermal, dps.kinetic, dps.explosive];
    let total: f64 = parts.iter().sum();
    // Labels right-aligned in one column and numbers in the next, so the numbers line up.
    let totals = h_flex()
        .gap_1()
        .child(
            v_flex()
                .items_end()
                .child(muted("Volley:", cx))
                .child(muted("DPS:", cx)),
        )
        .child(
            v_flex()
                .child(value("volley", volley, f.volley, ""))
                .child(value("dps", dps, f.dps, "")),
        );
    let body = v_flex()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .child(block(
                    p,
                    "weapon",
                    icon::TURRET,
                    "Weapon",
                    value("weapon", weapon, f.weapon_dps, " DPS"),
                ))
                .child(block(
                    p,
                    "drone",
                    icon::DRONE_BANDWIDTH,
                    "Drone",
                    value("drone", drone, no_spool, " DPS"),
                ))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(p.icon(icon::DAMAGE, 24.))
                        .child(totals),
                ),
        )
        // The damage type bar is gyfa's own: the mix at a glance, which pyfa leaves to tooltips.
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
        .into_any_element();

    // pyfa marks numbers taken against a target profile "( Effective )" in the header.
    let header = match target {
        Some(target) => h_flex()
            .gap_1()
            .child(title("Firepower"))
            .child(tip(
                "fp-effective",
                muted("( Effective )", cx),
                format!("Against target profile {}", target.name),
            ))
            .into_any_element(),
        None => title("Firepower"),
    };
    let this = this.clone();
    let current = target.map(|t| t.name.clone());
    let user = Rc::clone(&p.target_profiles);
    let pick: PickProfile = {
        let this = this.clone();
        Rc::new(move |profile, _, cx| {
            Gyfa::app_update(&this, cx, |g, cx| {
                g.apply(Action::SetTargetProfile(profile), cx);
            });
        })
    };
    Section {
        action: Some(swap_button(
            p,
            "fp-to-mining",
            ids::MINING,
            "Click to toggle to Mining Yield",
            cx,
        )),
        key: "Firepower",
        title: header,
        body,
        menu: Some(Rc::new(move |menu, window, cx| {
            let this = this.clone();
            let (user, current, pick) = (Rc::clone(&user), current.clone(), Rc::clone(&pick));
            menu.item(
                PopupMenuItem::new("Factor in Reload Time")
                    .checked(factor_reload)
                    .on_click(move |_, _, cx| {
                        Gyfa::app_update(&this, cx, |g, cx| {
                            g.saved.factor_reload = !g.saved.factor_reload;
                            g.changed(cx);
                        });
                    }),
            )
            .submenu("Target Resists", window, cx, move |menu, window, cx| {
                target_profile_menu(
                    menu,
                    &user,
                    current.as_deref(),
                    FirstProfile::NoProfile,
                    &pick,
                    window,
                    cx,
                )
            })
        })),
    }
}

/// pyfa's mining tooltip: yield and drain per second and per hour, and how much is kept.
fn mining_tooltip(rate: MiningRate) -> String {
    let efficiency = if rate.drain > 0.0 {
        format!(
            "{}%",
            format::amount(rate.yield_ / rate.drain * 100.0, 4, 0, 0)
        )
    } else {
        "0%".to_owned()
    };
    let per = |v: f64| {
        (
            format::amount(v, PREC, 0, 0),
            format::amount(v * 3600.0, PREC, 0, 0),
        )
    };
    let ((yps, yph), (dps, dph)) = (per(rate.yield_), per(rate.drain));
    format!(
        "{yps} m³ yield per second ({yph} m³ per hour)\n\
         {dps} m³ drain per second ({dph} m³ per hour)\n\
         {efficiency} efficiency"
    )
}

pub(super) fn mining(p: &Pane<'_>, cx: &Context<Gyfa>) -> Section {
    let m = p.stats.mining;
    let total = MiningRate {
        yield_: m.miner.yield_ + m.drone.yield_,
        drain: m.miner.drain + m.drone.drain,
    };
    let value = |key: &str, rate: MiningRate| {
        tip(
            format!("mining-{key}"),
            format!("{} m³/s", format::amount(rate.yield_, PREC, 0, 0)),
            mining_tooltip(rate),
        )
        .into_any_element()
    };
    let body = h_flex()
        .w_full()
        .child(block(
            p,
            "miner",
            ids::MINING,
            "Miner",
            value("miner", m.miner),
        ))
        .child(block(
            p,
            "mining-drone",
            icon::DRONE_BAY,
            "Drone",
            value("drone", m.drone),
        ))
        .child(block(
            p,
            "mining-total",
            ids::CARGO,
            "Total",
            value("total", total),
        ))
        .into_any_element();
    Section {
        key: "Firepower",
        title: title("Mining Yield"),
        body,
        menu: None,
        action: Some(swap_button(
            p,
            "mining-to-fp",
            icon::TURRET,
            "Click to toggle to Firepower View",
            cx,
        )),
    }
}

pub(super) fn outgoing(p: &Pane<'_>) -> Section {
    let o = p.stats.outgoing;
    let column =
        |key: &str, icon_id: i32, text: &str, value: f64, spool: Option<SpoolRange>, unit: &str| {
            let spool = spool.filter(|&s| has_spool(s));
            let hover = spool.map_or_else(
                || text.to_owned(),
                |s| {
                    format!(
                        "{text}\nSpool up: {}-{}",
                        format::amount(s.pre, PREC, 0, 0),
                        format::amount(s.full, PREC, 0, 0)
                    )
                },
            );
            let mark = if spool.is_some() { SPOOL_MARK } else { "" };
            v_flex()
                .flex_1()
                .items_center()
                .child(p.icon(icon_id, 24.))
                .child(tip(
                    format!("out-{key}"),
                    format!("{}{mark} {unit}", format::amount(value, PREC, 0, 0)),
                    hover,
                ))
        };
    let [shield, armor, hull] = o.reps;
    let [shield_spool, armor_spool, hull_spool] = o.reps_spool;
    let body = h_flex()
        .w_full()
        .justify_between()
        .child(column(
            "cap",
            ids::TRANSFER,
            "Capacitor restored",
            o.cap,
            None,
            "GJ/s",
        ))
        .child(column(
            "shield",
            icon::SHIELD,
            "Shield restored",
            shield,
            Some(shield_spool),
            "HP/s",
        ))
        .child(column(
            "armor",
            icon::ARMOR,
            "Armor restored",
            armor,
            Some(armor_spool),
            "HP/s",
        ))
        .child(column(
            "hull",
            icon::HULL,
            "Hull restored",
            hull,
            Some(hull_spool),
            "HP/s",
        ))
        // pyfa has no neutralizer column; gyfa adds one when the fit neuts, since refine knows it.
        .when(o.neut > 0.0, |row| {
            row.child(column(
                "neut",
                ids::NEUT,
                "Capacitor neutralized",
                o.neut,
                None,
                "GJ/s",
            ))
        })
        .into_any_element();
    Section {
        key: "Outgoing",
        title: title("Remote Reps"),
        body,
        menu: None,
        action: None,
    }
}
