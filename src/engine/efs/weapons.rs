//! The weapons part of the export, pyfa's `getWeaponSystemData`: turrets, launchers and smart bombs
//! grouped by type and charge, then drones and fighters.

use std::collections::BTreeMap;

use rc::ItemId;
use serde_json::{Value, json};

use super::{Built, num, round6};
use crate::engine::{Damage, Stats, read};
use crate::{
    data::{Catalog, Hardpoint, Slot, TypeId},
    dogma::attr,
    fit::Fit,
};

/// Weapons grouped by type and charge, then drones and fighters, pyfa's
/// `getWeaponSystemData`.
pub(super) fn weapons(
    fit: &Fit,
    built: &mut Built,
    catalog: &Catalog,
    stats: &Stats,
) -> Vec<Value> {
    let mut groups: BTreeMap<WeaponKey, (Slot, usize, ItemId, u32)> = BTreeMap::new();
    let mut order = Vec::new();
    for &(slot, i, id) in &built.modules.clone() {
        let Some(Some(module)) = fit.rack(slot).get(i) else {
            continue;
        };
        let dps = built.dmg(id).map_or(0.0, |d| Damage::from(&d.dps).total());
        if dps <= 0.0 {
            continue;
        }
        let key = (module.type_id, module.charge);
        groups
            .entry(key)
            .and_modify(|g| g.3 += 1)
            .or_insert_with(|| {
                order.push(key);
                (slot, i, id, 1)
            });
    }
    let mut weapons = Vec::new();
    for key in order {
        let (slot, i, id, n) = groups[&key];
        let n_f = f64::from(n);
        let (module, charge) = key;
        let Some(t) = catalog.get(module) else {
            continue;
        };
        let group = catalog
            .group_names
            .get(&t.group_id)
            .map_or("", String::as_str);
        let name = match charge {
            Some(c) => format!("{}, {}", t.name, catalog.name(c)),
            None => t.name.clone(),
        };
        let info = stats.modules.get(&(slot, i));
        let optimal = info.and_then(|m| m.optimal);
        let mut kind = "None";
        let (mut tracking, mut optimal_sig) = (0.0, 0.0);
        let (mut velocity, mut delay, mut reduction, mut radius, mut aoe_velocity) =
            (0.0, 0.0, 0.0, 0.0, 0.0);
        let mut field_range = 0.0;
        if t.hardpoint == Hardpoint::Turret {
            tracking = built.attr0(id, attr::TRACKING_SPEED);
            optimal_sig = built.attr0(id, attr::OPTIMAL_SIG_RADIUS);
            kind = "Turret";
        } else if t.hardpoint == Hardpoint::Launcher || t.name.contains("Bomb Launcher") {
            if let Some(c) = built.charge(id) {
                velocity = built.attr0(c, attr::MAX_VELOCITY);
                delay = built.attr0(c, attr::EXPLOSION_DELAY);
                reduction = built.attr0(c, attr::AOE_DAMAGE_REDUCTION_FACTOR);
                radius = built.attr0(c, attr::AOE_CLOUD_SIZE);
                aoe_velocity = built.attr0(c, attr::AOE_VELOCITY);
            }
            kind = "Missile";
        } else if group == "Super Weapon" && optimal.is_some() {
            radius = built.attr0(id, attr::SIGNATURE_RADIUS);
            kind = "Missile";
        } else if t.hardpoint == Hardpoint::None {
            field_range = built.attr0(id, attr::EMP_FIELD_RANGE);
            kind = "SmartBomb";
        }
        let max_range = if group == "Super Weapon" && optimal.is_none() {
            Some(300_000.0)
        } else {
            optimal
        };
        let dmg = built.dmg(id);
        let dps = dmg.as_ref().map_or(0.0, |d| Damage::from(&d.dps).total());
        let volley = dmg
            .as_ref()
            .map_or(0.0, |d| Damage::from(&d.volley).total());
        let spread = dmg.as_ref().map_or_else(
            || json!({"em": 0, "therm": 0, "kin": 0, "exp": 0, "pure": 0}),
            |d| {
                let v = |x: rc::PValue| round6(x.into_f64() * n_f);
                json!({
                    "em": v(d.dps.em), "therm": v(d.dps.thermal), "kin": v(d.dps.kinetic),
                    "exp": v(d.dps.explosive), "pure": 0,
                })
            },
        );
        let (charges, shots) = built
            .sol
            .get_module(&id)
            .ok()
            .map_or((None, None), |mut m| {
                (
                    m.get_charge_count().map(rc::Count::into_u32),
                    m.get_charged_cycle_count().map(rc::Count::into_u32),
                )
            });
        let cycle = built
            .attr(id, attr::RATE_OF_FIRE)
            .filter(|&v| v > 0.0)
            .or_else(|| built.attr(id, attr::DURATION));
        weapons.push(json!({
            "dps": round6(dps * n_f),
            "capUse": round6(info.and_then(|m| m.cap_per_sec).unwrap_or(0.0) * n_f),
            "falloff": num(built.attr(id, attr::FALLOFF)),
            "type": kind,
            "name": name,
            "optimal": num(max_range),
            "numCharges": charges.unwrap_or(0),
            "numShots": shots.unwrap_or(0),
            "reloadTime": round6(built.attr0(id, attr::RELOAD_TIME)),
            "cycleTime": num(cycle),
            "volley": round6(volley * n_f),
            "tracking": round6(tracking),
            "maxVelocity": round6(velocity),
            "explosionDelay": round6(delay),
            "damageReductionFactor": round6(reduction),
            "explosionRadius": round6(radius),
            "explosionVelocity": round6(aoe_velocity),
            "aoeFieldRange": round6(field_range),
            "damageMultiplierBonusMax": num(built.attr(id, attr::DAMAGE_MULTIPLIER_BONUS_MAX)),
            "damageMultiplierBonusPerCycle": num(built.attr(id, attr::DAMAGE_MULTIPLIER_BONUS_PER_CYCLE)),
            "dps_spread": spread,
            "optimalSigRadius": round6(optimal_sig),
        }));
    }
    for (stack, id) in fit.drones.iter().zip(built.drones.clone()) {
        let Some(id) = id.filter(|_| stack.active > 0) else {
            continue;
        };
        let active = f64::from(stack.active);
        let dmg = built.dmg(id);
        let dps = dmg.as_ref().map_or(0.0, |d| Damage::from(&d.dps).total()) * active;
        if dps <= 0.0 {
            continue;
        }
        let volley = dmg
            .as_ref()
            .map_or(0.0, |d| Damage::from(&d.volley).total())
            * active;
        let sig = built.attr0(id, attr::OPTIMAL_SIG_RADIUS);
        // Drones use the old tracking formula; pyfa scales it to match turrets.
        let tracking = if sig > 0.0 {
            built.attr0(id, attr::TRACKING_SPEED) / (sig / 40_000.0)
        } else {
            0.0
        };
        weapons.push(json!({
            "dps": round6(dps),
            "cycleTime": num(built.attr(id, attr::RATE_OF_FIRE)),
            "type": "Drone",
            "optimal": num(built.attr(id, attr::MAX_RANGE)),
            "name": catalog.name(stack.type_id),
            "falloff": num(built.attr(id, attr::FALLOFF)),
            "maxSpeed": round6(built.attr0(id, attr::MAX_VELOCITY)),
            "tracking": round6(tracking),
            "volley": round6(volley),
        }));
    }
    for ((fighter, id), info) in fit
        .fighters
        .iter()
        .zip(built.fighters.clone())
        .zip(&stats.fighters)
    {
        let amount = info.count.map_or(0, |(now, _)| now);
        if amount == 0 {
            continue;
        }
        let dmg = built.dmg(id);
        let dps = dmg.as_ref().map_or(0.0, |d| Damage::from(&d.dps).total());
        if dps <= 0.0 {
            continue;
        }
        let abilities: Vec<Value> = [FIGHTER_ATTACK, FIGHTER_MISSILES]
            .iter()
            .filter_map(|ability| fighter_ability(built, id, ability, amount))
            .collect();
        weapons.push(json!({
            "dps": round6(dps),
            "type": "Fighter",
            "name": catalog.name(fighter.type_id),
            "maxSpeed": round6(built.attr0(id, attr::MAX_VELOCITY)),
            "abilities": abilities,
            "ehp": round6(built.attr0(id, attr::SHIELD_CAPACITY) / FIGHTER_EHP * f64::from(amount)),
            "volley": round6(dmg.as_ref().map_or(0.0, |d| Damage::from(&d.volley).total())),
            "signatureRadius": round6(built.attr0(id, attr::SIGNATURE_RADIUS)),
        }));
    }
    weapons
}

/// A weapon and the charge it has loaded, which pyfa groups identical weapons by.
type WeaponKey = (TypeId, Option<TypeId>);

/// Fighter shield HP to EHP, pyfa's rough average fighter resist.
pub(super) const FIGHTER_EHP: f64 = 0.8875;

/// One of a fighter's damaging abilities: attribute IDs from its base name.
pub(super) struct FighterAbility {
    name: &'static str,
    damage: [i32; 4],
    multiplier: i32,
    reduction_factor: i32,
    reduction_sensitivity: i32,
    explosion_radius: i32,
    explosion_velocity: i32,
    range: i32,
    duration: i32,
}

/// pyfa calls the attack missile `RegularAttack` and the missiles `MissileAttack`.
pub(super) const FIGHTER_ATTACK: FighterAbility = FighterAbility {
    name: "RegularAttack",
    damage: [2227, 2228, 2230, 2229],
    multiplier: 2226,
    reduction_factor: 2231,
    reduction_sensitivity: 2232,
    explosion_radius: 2234,
    explosion_velocity: 2235,
    range: 2236,
    duration: 2233,
};

pub(super) const FIGHTER_MISSILES: FighterAbility = FighterAbility {
    name: "MissileAttack",
    damage: [2131, 2132, 2134, 2133],
    multiplier: 2130,
    reduction_factor: 2127,
    reduction_sensitivity: 2128,
    explosion_radius: 2125,
    explosion_velocity: 2126,
    range: 2149,
    duration: 2182,
};

pub(super) fn fighter_ability(
    built: &mut Built,
    id: ItemId,
    ability: &FighterAbility,
    amount: u32,
) -> Option<Value> {
    // pyfa checks the type has the ability's EM damage at all.
    built.attr(id, ability.damage[0])?;
    let reduction = read::reduction_exponent(
        built.attr0(id, ability.reduction_factor),
        built.attr0(id, ability.reduction_sensitivity),
    );
    let base: f64 = ability.damage.iter().map(|&a| built.attr0(id, a)).sum();
    let damage = base * built.attr0(id, ability.multiplier);
    Some(json!({
        "name": ability.name,
        "volley": round6(damage * f64::from(amount)),
        "explosionRadius": round6(built.attr0(id, ability.explosion_radius)),
        "explosionVelocity": round6(built.attr0(id, ability.explosion_velocity)),
        "optimal": round6(built.attr0(id, ability.range)),
        "damageReductionFactor": round6(reduction),
        "rof": round6(built.attr0(id, ability.duration)),
    }))
}
