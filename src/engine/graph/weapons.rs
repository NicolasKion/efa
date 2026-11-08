//! What each weapon deals and how it applies: modules, breacher pods, drones and fighter
//! abilities.

use std::collections::HashSet;

use rc::{
    ItemId, SolOwned,
    stats::{ItemStatsCommon, StatCritOptions, StatDmg, StatItemChargeOptions, StatTimeOptions},
};

use super::{
    CIVILIAN_LAUNCHER, Charge, PlacedDrones, PlacedModule, abilities, ability, falloff, max_range,
    reload_of, spool_at, spool_cycles,
};
use crate::engine::{Damage, read};
use crate::{
    data::Hardpoint,
    dogma::{attr, effect, group},
    graphs::{
        math::{self, Gun},
        snapshot::{Cycle, Dmg, Weapon, WeaponKind},
    },
};

pub(super) fn item_dmg(sol: &mut SolOwned, id: ItemId, time: StatTimeOptions) -> Option<StatDmg> {
    let mut stats = sol.stats();
    let mut item = stats.get_item(&id).ok()?;
    item.get_stat_dmg(
        time,
        StatCritOptions::default(),
        StatItemChargeOptions::Include,
    )
    .ok()
}

pub(super) fn module_weapon(
    sol: &mut SolOwned,
    placed: &PlacedModule,
    hardpoint: Hardpoint,
    group: i32,
    effects: &HashSet<i32>,
    charge: Option<&Charge>,
    ship_radius: f64,
) -> Option<Weapon> {
    let id = placed.id;
    let burst = item_dmg(sol, id, StatTimeOptions::default())?;
    let (dps, volley) = (
        Damage::from(&burst.dps).as_array(),
        Damage::from(&burst.volley).as_array(),
    );
    if dps.iter().sum::<f64>() <= 0.0 {
        return breacher_weapon(sol, id, &burst, charge, ship_radius);
    }
    let kind = if effects.contains(&effect::VORTON) {
        WeaponKind::Vorton {
            range: read::attr_or_zero(sol, id, attr::MAX_RANGE),
            er: read::attr_or_zero(sol, id, attr::AOE_CLOUD_SIZE),
            ev: read::attr_or_zero(sol, id, attr::AOE_VELOCITY),
            drf: read::attr_or_zero(sol, id, attr::AOE_DAMAGE_REDUCTION_FACTOR),
        }
    } else if hardpoint == Hardpoint::Turret {
        WeaponKind::Turret(Gun {
            optimal: max_range(sol, id),
            falloff: falloff(sol, id),
            tracking: read::attr_or_zero(sol, id, attr::TRACKING_SPEED),
            optimal_sig: read::attr_or_zero(sol, id, attr::OPTIMAL_SIG_RADIUS),
        })
    } else if hardpoint == Hardpoint::Launcher || placed.type_id == CIVILIAN_LAUNCHER {
        let charge = charge?;
        let c = charge.id;
        let fof = charge.effects.contains(&effect::FOF_MISSILE);
        let range = math::missile_range(
            read::attr_or_zero(sol, c, attr::MAX_VELOCITY),
            read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY),
            read::attr_or_zero(sol, c, attr::MASS),
            read::attr_or_zero(sol, c, attr::AGILITY),
            ship_radius,
            fof.then(|| read::attr_or_zero(sol, c, attr::MAX_FOF_RANGE)),
        );
        WeaponKind::Launcher {
            er: read::attr_or_zero(sol, c, attr::AOE_CLOUD_SIZE),
            ev: read::attr_or_zero(sol, c, attr::AOE_VELOCITY),
            drf: read::attr_or_zero(sol, c, attr::AOE_DAMAGE_REDUCTION_FACTOR),
            range,
            fof,
        }
    } else if matches!(group, group::SMART_BOMB | group::AREA_DENIAL) {
        WeaponKind::Smartbomb {
            range: max_range(sol, id),
        }
    } else if group == group::BOMB_LAUNCHER {
        let c = charge?.id;
        // A bomb flies its full flight time before it goes off.
        let range = read::attr_or_zero(sol, c, attr::MAX_VELOCITY)
            * read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY)
            / 1000.0;
        WeaponKind::Bomb {
            range,
            blast_radius: read::attr_or_zero(sol, c, attr::EXPLOSION_RANGE),
            er: read::attr_or_zero(sol, c, attr::AOE_CLOUD_SIZE),
        }
    } else if group == group::GUIDED_BOMB_LAUNCHER {
        let c = charge?.id;
        let range = read::attr_or_zero(sol, c, attr::MAX_VELOCITY)
            * read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY)
            / 1000.0;
        WeaponKind::GuidedBomb {
            range,
            er: read::attr_or_zero(sol, c, attr::AOE_CLOUD_SIZE),
        }
    } else if matches!(group, group::SUPER_WEAPON | group::STRUCTURE_DOOMSDAY) {
        WeaponKind::Doomsday {
            range: max_range(sol, id),
            damage_sig: read::attr_or_zero(sol, id, attr::SIGNATURE_RADIUS),
            needs_lock: effect::SUPER_WEAPONS.iter().any(|e| effects.contains(e)),
        }
    } else {
        // Breacher pods and anything else pyfa has no application model for.
        return None;
    };

    let duration = cycle_of(&volley, &dps)?;
    let (shots, reload) = reload_of(sol, id);
    let spool = spool_cycles(sol, id);
    let amounts = if spool > 0 {
        (0..=spool)
            .map(|n| {
                item_dmg(sol, id, spool_at(n))
                    .map_or(volley, |d| Damage::from(&d.volley).as_array())
            })
            .collect()
    } else {
        vec![volley]
    };
    Some(Weapon {
        kind,
        dps,
        volley,
        cycle: Cycle {
            duration,
            shots,
            reload,
            amounts,
            delay: 0.0,
        },
    })
}

/// A breacher pod launcher: its damage is a share of the target's HP, so the weapon carries
/// the cap and the share, and application picks the smaller for the target at hand.
pub(super) fn breacher_weapon(
    sol: &mut SolOwned,
    id: ItemId,
    burst: &StatDmg,
    charge: Option<&Charge>,
    ship_radius: f64,
) -> Option<Weapon> {
    let pair = |e: &rc::stats::StatDmgEntry| {
        [
            e.breacher.absolute_max.into_f64(),
            e.breacher.relative_max.into_f64(),
            0.0,
            0.0,
        ]
    };
    let (dps, volley) = (pair(&burst.dps), pair(&burst.volley));
    if dps[0] <= 0.0 && dps[1] <= 0.0 {
        return None;
    }
    let c = charge?.id;
    let range = math::missile_range(
        read::attr_or_zero(sol, c, attr::MAX_VELOCITY),
        read::attr_or_zero(sol, c, attr::EXPLOSION_DELAY),
        read::attr_or_zero(sol, c, attr::MASS),
        read::attr_or_zero(sol, c, attr::AGILITY),
        ship_radius,
        None,
    );
    let duration = read::first_attr(sol, id, &[attr::RATE_OF_FIRE, attr::DURATION]) / 1000.0;
    let (shots, reload) = reload_of(sol, id);
    Some(Weapon {
        kind: WeaponKind::Breacher { range },
        dps,
        volley,
        cycle: Cycle {
            duration: if duration > 0.0 { duration } else { 1.0 },
            shots,
            reload,
            // pyfa has the pod start eating a second after launch.
            amounts: vec![dps],
            delay: 1.0,
        },
    })
}

/// Every damaging ability of a launched squadron as a weapon of its own, read from refine by
/// switching the squadron's other abilities off in turn.
pub(super) fn fighter_weapons(sol: &mut SolOwned, id: ItemId) -> Vec<Weapon> {
    let states = abilities(sol, id);
    let speed = read::attr_or_zero(sol, id, attr::MAX_VELOCITY);
    let radius = read::attr_or_zero(sol, id, attr::RADIUS);
    let mut out = Vec::new();
    for &(ability_id, on) in &states {
        let attack = ability::ATTACK.contains(&ability_id);
        let missiles = ability::MISSILES.contains(&ability_id);
        let plain = ability_id == ability::BOMB || ability_id == ability::KAMIKAZE;
        if !on || !(attack || missiles || plain) {
            continue;
        }
        for &(other, other_on) in &states {
            set_ability(sol, id, other, other == ability_id && other_on);
        }
        let burst = item_dmg(sol, id, StatTimeOptions::default());
        for &(other, other_on) in &states {
            set_ability(sol, id, other, other_on);
        }
        let Some(burst) = burst else { continue };
        let (dps, volley) = (
            Damage::from(&burst.dps).as_array(),
            Damage::from(&burst.volley).as_array(),
        );
        let Some(duration) = cycle_of(&volley, &dps) else {
            continue;
        };
        let kind = if attack {
            WeaponKind::Fighter {
                optimal: read::attr_or_zero(sol, id, attr::FTR_ATTACK_OPTIMAL),
                falloff: read::attr_or_zero(sol, id, attr::FTR_ATTACK_FALLOFF),
                er: read::attr_or_zero(sol, id, attr::FTR_ATTACK_ER),
                ev: read::attr_or_zero(sol, id, attr::FTR_ATTACK_EV),
                drf: read::reduction_exponent(
                    read::attr_or_zero(sol, id, attr::FTR_ATTACK_DRF),
                    read::attr_or_zero(sol, id, attr::FTR_ATTACK_DRS),
                ),
                speed,
                radius,
                plain: false,
            }
        } else if missiles {
            WeaponKind::Fighter {
                optimal: read::attr_or_zero(sol, id, attr::FTR_MISSILES_RANGE),
                falloff: 0.0,
                er: read::attr_or_zero(sol, id, attr::FTR_MISSILES_ER),
                ev: read::attr_or_zero(sol, id, attr::FTR_MISSILES_EV),
                drf: read::reduction_exponent(
                    read::attr_or_zero(sol, id, attr::FTR_MISSILES_DRF),
                    read::attr_or_zero(sol, id, attr::FTR_MISSILES_DRS),
                ),
                speed,
                radius,
                plain: false,
            }
        } else {
            WeaponKind::Fighter {
                optimal: read::attr_or_zero(sol, id, attr::FTR_KAMIKAZE_RANGE),
                falloff: 0.0,
                er: 0.0,
                ev: 0.0,
                drf: 0.0,
                speed,
                radius,
                plain: true,
            }
        };
        out.push(Weapon {
            kind,
            dps,
            volley,
            cycle: Cycle {
                duration,
                // pyfa's rearm model for fighter charges is left out of the timelines.
                shots: None,
                reload: 0.0,
                amounts: vec![volley],
                delay: 0.0,
            },
        });
    }
    out
}

pub(super) fn set_ability(sol: &mut SolOwned, id: ItemId, ability: i32, on: bool) {
    if let Ok(mut fighter) = sol.get_fighter(&id)
        && let Ok(mut a) = fighter.get_ability(&rc::AbilityId::from_i32(ability))
        && a.get_state() != on
    {
        a.set_state(on);
    }
}

/// Seconds per cycle, from what refine says one volley and one second deal.
pub(super) fn cycle_of(volley: &Dmg, dps: &Dmg) -> Option<f64> {
    let (v, d) = (volley.iter().sum::<f64>(), dps.iter().sum::<f64>());
    (v > 0.0 && d > 0.0).then_some(v / d)
}

pub(super) fn drone_weapon(sol: &mut SolOwned, stack: &PlacedDrones) -> Option<Weapon> {
    let id = stack.id;
    let burst = item_dmg(sol, id, StatTimeOptions::default())?;
    let count = f64::from(stack.active);
    let (dps, volley) = (
        Damage::from(&burst.dps).as_array().map(|v| v * count),
        Damage::from(&burst.volley).as_array().map(|v| v * count),
    );
    let duration = cycle_of(&volley, &dps)?;
    Some(Weapon {
        kind: WeaponKind::Drone {
            gun: Gun {
                optimal: read::first_attr(sol, id, &[attr::MAX_RANGE]),
                falloff: read::first_attr(sol, id, &[attr::FALLOFF, attr::FALLOFF_EFFECTIVENESS]),
                tracking: read::attr_or_zero(sol, id, attr::TRACKING_SPEED),
                optimal_sig: read::attr_or_zero(sol, id, attr::OPTIMAL_SIG_RADIUS),
            },
            speed: read::attr_or_zero(sol, id, attr::MAX_VELOCITY),
            radius: read::attr_or_zero(sol, id, attr::RADIUS),
        },
        dps,
        volley,
        cycle: Cycle {
            duration,
            shots: None,
            reload: 0.0,
            amounts: vec![volley],
            delay: 0.0,
        },
    })
}
