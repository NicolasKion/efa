//! The tank sections: resistances with effective HP and the damage pattern (resistancesViewFull),
//! recharge rates (rechargeViewFull) and bombs to kill (bombingViewFull).

use std::{rc::Rc, sync::LazyLock};

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, IntoElement, ParentElement, Styled, WeakEntity, Window,
    div, px,
};

use super::{Pane, Section, format, ids, profiles::Tree, tip, title};
use crate::{
    engine::Layer,
    fit::DamageProfile,
    ui::{
        Gyfa,
        actions::Action,
        widgets::{DAMAGE_COLORS, gauge, icon},
    },
};

const DAMAGE_ICONS: [(i32, &str); 4] = [
    (icon::EM, "Electromagnetic resistance"),
    (icon::THERMAL, "Thermal resistance"),
    (icon::KINETIC, "Kinetic resistance"),
    (icon::EXPLOSIVE, "Explosive resistance"),
];

/// Width of one resist gauge; pyfa's are 48 px at its smaller font.
/// Width of the value column, which fits `WWWWk` in pyfa.
const VALUE: f32 = 64.;

static PATTERNS: LazyLock<Tree> = LazyLock::new(Tree::builtin);

/// The tank layers with their name, icon and pyfa's tooltip.
fn layers(p: &Pane<'_>) -> [(&'static str, i32, Layer, &'static str); 3] {
    let s = p.stats;
    [
        ("Shield", icon::SHIELD, s.shield, "Shield resistance"),
        ("Armor", icon::ARMOR, s.armor, "Armor resistance"),
        ("Hull", icon::HULL, s.hull, "Hull resistance"),
    ]
}

/// What a layer's value column shows: effective or raw HP.
fn shown_hp(layer: Layer, raw: bool) -> f64 {
    if raw {
        layer.hp
    } else {
        layer.ehp.unwrap_or(0.0)
    }
}

/// A row of four damage type gauges after an icon, with a value column at the end.
fn gauge_row(
    p: &Pane<'_>,
    key: &'static str,
    icon_id: i32,
    text: &str,
    fractions: [f64; 4],
    value: AnyElement,
    cx: &App,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap_1()
        .child(p.icon_tip(format!("resist-{key}"), icon_id, 20., text))
        .children(
            fractions
                .into_iter()
                .zip(DAMAGE_COLORS)
                .map(|(f, fill)| gauge(f, fill, format!("{:.1}%", f * 100.0), 0., cx)),
        )
        .child(h_flex().w(px(VALUE)).justify_center().child(value))
        .into_any_element()
}

pub(super) fn resistances(p: &Pane<'_>, this: &WeakEntity<Gyfa>, cx: &Context<Gyfa>) -> Section {
    let raw = p.view.raw_hp;
    let total: f64 = layers(p).iter().map(|&(_, _, l, _)| shown_hp(l, raw)).sum();
    let (label, hover) = if raw {
        ("Raw HP", format!("Raw: {} HP", total as i64))
    } else {
        ("Effective HP", format!("Effective: {} HP", total as i64))
    };
    let header = h_flex()
        .child(title(format!("Resistances ({label}: ")))
        .child(tip(
            "resist-total",
            title(format::amount(total, 3, 0, 9)),
            hover,
        ))
        .child(title(")"))
        .into_any_element();

    let toggle = Button::new("resist-ehp-toggle")
        .ghost()
        .xsmall()
        .label(if raw { "HP" } else { "EHP" })
        .tooltip("Click to toggle between effective HP and raw HP")
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.stats_view.raw_hp = !this.stats_view.raw_hp;
            cx.notify();
        }));
    let head = h_flex()
        .w_full()
        .gap_1()
        .child(div().flex_none().w(px(20.)))
        .children(
            DAMAGE_ICONS
                .iter()
                .enumerate()
                .map(|(i, &(icon_id, text))| {
                    h_flex().flex_1().justify_center().child(p.icon_tip(
                        format!("resist-head-{i}"),
                        icon_id,
                        20.,
                        text,
                    ))
                }),
        )
        .child(h_flex().w(px(VALUE)).justify_center().child(toggle));

    let mut body = v_flex().gap_1().child(head);
    for (name, icon_id, layer, text) in layers(p) {
        let value = shown_hp(layer, raw);
        let factor = layer.ehp.unwrap_or(0.0) / layer.hp;
        let hover = format!(
            "{name}: {}\nResist Multiplier: {:.2}x",
            value as i64,
            if factor.is_finite() { factor } else { 0.0 }
        );
        let value = tip(
            format!("resist-value-{name}"),
            format::amount(value, 3, 0, 9),
            hover,
        )
        .into_any_element();
        body = body.child(gauge_row(p, name, icon_id, text, layer.resists, value, cx));
    }

    // The pattern row shows each type's share; pyfa empties it while raw HP is shown.
    let incoming = p.stats.incoming;
    let sum: f64 = incoming.iter().sum();
    let shares = if raw || sum <= 0.0 {
        [0.0; 4]
    } else {
        incoming.map(|v| v / sum)
    };
    body = body
        .child(div().h_px().w_full().bg(cx.theme().border))
        .child(gauge_row(
            p,
            "pattern",
            icon::DAMAGE,
            "Incoming damage pattern",
            shares,
            div().into_any_element(),
            cx,
        ));

    let (this, current) = (this.clone(), p.fit.damage_profile.clone());
    Section {
        key: "Resistances",
        title: header,
        body: body.into_any_element(),
        menu: Some(Rc::new(move |menu, window, cx| {
            pattern_menu(menu, &this, &current, window, cx)
        })),
    }
}

/// pyfa's damage pattern menu: Uniform and Selected Ammo first, then the builtin categories.
fn pattern_menu(
    menu: PopupMenu,
    this: &WeakEntity<Gyfa>,
    current: &DamageProfile,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let pick = |this: &WeakEntity<Gyfa>, profile: DamageProfile| {
        let this = this.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            Gyfa::app_update(&this, cx, |g, cx| {
                g.apply(Action::SetDamageProfile(profile.clone()), cx);
            });
        }
    };
    let menu = menu
        .item(
            PopupMenuItem::new("Uniform")
                .checked(*current == DamageProfile::Uniform)
                .on_click(pick(this, DamageProfile::Uniform)),
        )
        .item(
            PopupMenuItem::new("Selected Ammo")
                .checked(*current == DamageProfile::SelectedAmmo)
                .on_click(pick(this, DamageProfile::SelectedAmmo)),
        );
    // Uniform already sits at the top as its own entry.
    let root: &'static Tree = &PATTERNS;
    root.children.iter().fold(menu, |menu, (name, level)| {
        pattern_submenu(menu, name, level, this, current, window, cx)
    })
}

fn pattern_submenu(
    menu: PopupMenu,
    name: &'static str,
    level: &'static Tree,
    this: &WeakEntity<Gyfa>,
    current: &DamageProfile,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, current) = (this.clone(), current.clone());
    menu.submenu(name, window, cx, move |menu, window, cx| {
        let menu = level.children.iter().fold(menu, |menu, (name, child)| {
            pattern_submenu(menu, name, child, &this, &current, window, cx)
        });
        level
            .patterns
            .iter()
            .fold(menu, |menu, &(short, raw, amounts)| {
                let checked =
                    matches!(&current, DamageProfile::Pattern { name, .. } if name == raw);
                let this = this.clone();
                menu.item(
                    PopupMenuItem::new(short)
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            let profile = DamageProfile::Pattern {
                                name: raw.to_owned(),
                                amounts,
                            };
                            Gyfa::app_update(&this, cx, |g, cx| {
                                g.apply(Action::SetDamageProfile(profile), cx);
                            });
                        }),
                )
            })
    })
}

/// Rounds the way pyfa compares spool ranges: to one decimal.
fn differs(a: f64, b: f64) -> bool {
    (a * 10.0).round() as i64 != (b * 10.0).round() as i64
}

pub(super) fn recharge(p: &Pane<'_>) -> Section {
    let raw = p.view.raw_hp;
    let unit = if raw { " HP/s" } else { " EHP/s" };
    let s = p.stats;
    // pyfa's effective tank scales each layer's repair by that layer's resist multiplier.
    let mult = |layer: Layer| if raw { 1.0 } else { layer.mult.unwrap_or(1.0) };
    let (shield, armor, hull) = (mult(s.shield), mult(s.armor), mult(s.hull));

    let cell = |content: AnyElement| h_flex().flex_1().justify_center().child(content);
    let head = h_flex()
        .w_full()
        .child(div().flex_none().w(px(28.)))
        .children(
            [
                (icon::RECHARGE, "Passive shield recharge"),
                (icon::SHIELD, "Active shield boost"),
                (icon::ARMOR, "Armor repair amount"),
                (icon::HULL, "Hull repair amount"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (icon_id, text))| {
                cell(p.icon_tip(format!("recharge-head-{i}"), icon_id, 24., text))
            }),
        );

    let rate = |key: String, amount: f64, spool: Option<(f64, f64)>| {
        let spool = spool.filter(|&(pre, full)| differs(pre, full));
        let hover = spool.map_or_else(String::new, |(pre, full)| {
            format!("Spool up: {pre:.1}-{full:.1}")
        });
        let mark = if spool.is_some() { "\u{02e2}" } else { "" };
        tip(key, format!("{amount:.1}{mark}{unit}"), hover).into_any_element()
    };
    let row = |name: &'static str, icon_id: i32, text: &str, tank: crate::engine::Tank| {
        let passive = if name == "sustained" {
            let value = s.tank.passive_shield * shield;
            tip(
                "recharge-passive",
                format!("{}{unit}", format::amount(value, 3, 0, 9)),
                format!("{value:.3}"),
            )
            .into_any_element()
        } else {
            div().into_any_element()
        };
        let spool = (tank.armor_spool.pre * armor, tank.armor_spool.full * armor);
        h_flex()
            .w_full()
            .child(h_flex().flex_none().w(px(28.)).child(p.icon_tip(
                format!("recharge-{name}"),
                icon_id,
                24.,
                text,
            )))
            .child(cell(passive))
            .child(cell(rate(
                format!("{name}-shield"),
                tank.shield * shield,
                None,
            )))
            .child(cell(rate(
                format!("{name}-armor"),
                tank.armor * armor,
                Some(spool),
            )))
            .child(cell(rate(format!("{name}-hull"), tank.hull * hull, None)))
    };
    let body = v_flex()
        .gap_1()
        .child(head)
        .child(row("reinforced", ids::REPAIR, "Reinforced", s.tank))
        .child(row("sustained", icon::CAPACITOR, "Sustained", s.sustained))
        .into_any_element();
    Section {
        key: "Recharge",
        title: title("Recharge rates"),
        body,
        menu: None,
    }
}

/// Bombs, bombers and the damage type each deals, as pyfa's bombing view lists them.
const BOMBS: [(i32, &str); 4] = [
    (27920, "Purifier"),
    (27916, "Nemesis"),
    (27912, "Manticore"),
    (27918, "Hound"),
];

/// Bomb damage bonus per Covert Ops level on stealth bombers.
const COVERT_OPS_BONUS: f64 = 0.05;

pub(super) fn bombing(p: &Pane<'_>) -> Section {
    let s = p.stats;
    // HP over the share each pure damage type gets through, summed over all layers.
    let ehp: [f64; 4] = std::array::from_fn(|i| {
        [s.shield, s.armor, s.hull]
            .iter()
            .map(|l| l.hp / (1.0 - l.resists[i]))
            .sum()
    });
    let ship_sig = s.sig_radius.unwrap_or(0.0);
    let catalog = &p.loaded.catalog;
    let cell = |content: AnyElement| h_flex().flex_1().justify_center().child(content);

    let head = h_flex()
        .w_full()
        .child(cell(p.icon_tip(
            "bomb-skill",
            ids::SKILL,
            24.,
            "Covert Ops level",
        )))
        .children(
            BOMBS
                .iter()
                .zip(DAMAGE_ICONS)
                .map(|(&(id, _), (icon_id, _))| {
                    cell(p.icon_tip(format!("bomb-{id}"), icon_id, 20., &catalog.name(id)))
                }),
        );
    let mut body = v_flex().gap_0p5().child(head);
    for level in 0..=5u8 {
        let mut row = h_flex().w_full().child(cell(
            tip(
                format!("bomb-level-{level}"),
                level.to_string(),
                "Covert Ops level",
            )
            .into_any_element(),
        ));
        for (i, &(id, bomber)) in BOMBS.iter().enumerate() {
            let Some(bomb) = catalog.get(id) else {
                row = row.child(cell(div().child("–").into_any_element()));
                continue;
            };
            let bomb_sig = bomb.signature_radius.unwrap_or(0.0);
            let applied = bomb.damage.iter().sum::<f64>()
                * (1.0 + COVERT_OPS_BONUS * f64::from(level))
                * (ship_sig.min(bomb_sig) / bomb_sig);
            let count = (ehp[i] / applied * 10.0).ceil() / 10.0;
            let hover = if level == 0 {
                format!(
                    "Number of {} to kill a {} with Covert Ops level {level}",
                    bomb.name, p.fit.name
                )
            } else {
                format!(
                    "Number of {} to kill a {} using a {bomber} with Covert Ops level {level}",
                    bomb.name, p.fit.name
                )
            };
            let text = if count.is_finite() {
                format!("{count:.1}")
            } else {
                "–".to_owned()
            };
            row = row.child(cell(
                tip(format!("bomb-{id}-{level}"), text, hover).into_any_element(),
            ));
        }
        body = body.child(row);
    }
    Section {
        key: "Bombing",
        title: title("Bombing"),
        body: body.into_any_element(),
        menu: None,
    }
}
