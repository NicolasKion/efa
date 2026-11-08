//! Local repairs and capacitor boosters, pyfa's `getRepairData`.

use rc::ItemId;
use serde_json::{Map, Value, json};

use super::{Built, modules_in_groups, num};
use crate::engine::read;
use crate::{data::Catalog, dogma::attr, fit::Fit};

/// Local repairers and capacitor boosters, pyfa's `getRepairData`.
pub(super) fn repairs(fit: &Fit, built: &mut Built, catalog: &Catalog) -> Vec<Value> {
    const GROUPS: [&str; 6] = [
        "Shield Booster",
        "Armor Repair Unit",
        "Ancillary Shield Booster",
        "Ancillary Armor Repairer",
        "Hull Repair Unit",
        "Capacitor Booster",
    ];
    let mut repairs = Vec::new();
    for (slot, i, id, group) in modules_in_groups(fit, built, catalog, &GROUPS) {
        let mut stats = Map::new();
        let mut put = |key: &str, value: Value| {
            stats.insert(key.to_owned(), value);
        };
        put("duration", num(built.attr(id, attr::DURATION)));
        put("capacitorNeed", num(built.attr(id, attr::CAPACITOR_NEED)));
        let shots = built
            .sol
            .get_module(&id)
            .ok()
            .and_then(|mut m| m.get_charged_cycle_count())
            .map(rc::Count::into_u32);
        match group.as_str() {
            "Armor Repair Unit" | "Ancillary Armor Repairer" => {
                put("type", json!("Armor Repairer"));
                put(
                    "armorDamageAmount",
                    num(built.attr(id, attr::ARMOR_DAMAGE_AMOUNT)),
                );
                if group == "Ancillary Armor Repairer" {
                    put("numShots", json!(shots.unwrap_or(0)));
                    put("reloadTime", num(built.attr(id, attr::RELOAD_TIME)));
                    put(
                        "chargedArmorDamageMultiplier",
                        num(built.attr(id, attr::CHARGED_ARMOR_DAMAGE_MULTIPLIER)),
                    );
                }
            }
            "Shield Booster" | "Ancillary Shield Booster" => {
                put("type", json!("Shield Booster"));
                put("shieldBonus", num(built.attr(id, attr::SHIELD_BONUS)));
                if group == "Ancillary Shield Booster" {
                    put("numShots", json!(shots.unwrap_or(0)));
                    put("reloadTime", num(built.attr(id, attr::RELOAD_TIME)));
                    // What it needs once its cap boosters run out.
                    if fit
                        .rack(slot)
                        .get(i)
                        .and_then(|m| m.as_ref()?.charge)
                        .is_some()
                    {
                        put("unloadedCapacitorNeed", num(unloaded_cap_need(built, id)));
                    }
                }
            }
            "Capacitor Booster" => {
                put("type", json!("Capacitor Booster"));
                put("numShots", json!(shots.unwrap_or(0)));
                put("reloadTime", num(built.attr(id, attr::RELOAD_TIME)));
            }
            _ => {}
        }
        repairs.push(Value::Object(stats));
    }
    repairs
}

/// What an ancillary shield booster needs once its cap boosters run out.
pub(super) fn unloaded_cap_need(built: &mut Built, id: ItemId) -> Option<f64> {
    let mut tx = built.sol.begin_tx();
    if let Some(charge) = tx.get_module(&id).ok()?.get_charge() {
        charge.remove();
    }
    read::attr(&mut tx, id, attr::CAPACITOR_NEED)
}
