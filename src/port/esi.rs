//! ESI's fitting JSON (service/port/esi.py), the shape the character fittings routes use:
//! a name, a description, the ship and items with inventory flags.

use serde::{Deserialize, Serialize};

use super::{Bay, Imported, new_fit, place, subsystem_flag};
use crate::{
    data::{Catalog, Slot, TypeId},
    fit::Fit,
};

/// Inventory flags ESI uses for where an item sits.
mod flag {
    pub const CARGO: u32 = 5;
    pub const LOW: u32 = 11;
    pub const MID: u32 = 19;
    pub const HIGH: u32 = 27;
    pub const DRONE_BAY: u32 = 87;
    pub const RIG: u32 = 92;
    pub const FIGHTER_BAY: u32 = 158;
}

/// ESI caps fitting names at 50 characters and descriptions at 500; pyfa cuts them shorter.
const NAME_MAX: usize = 50;
const DESCRIPTION_MAX: usize = 400;

#[derive(Debug, Serialize, Deserialize)]
struct EsiFit {
    name: String,
    #[serde(default)]
    description: String,
    ship_type_id: TypeId,
    items: Vec<EsiItem>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EsiItem {
    #[serde(deserialize_with = "flag_from_any")]
    flag: u32,
    quantity: u32,
    type_id: TypeId,
}

/// ESI's newer routes name flags (`HiSlot0`, `Cargo`) where older ones number them.
fn flag_from_any<'de, D: serde::Deserializer<'de>>(de: D) -> Result<u32, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Flag {
        Number(u32),
        Name(String),
    }
    Ok(match Flag::deserialize(de)? {
        Flag::Number(n) => n,
        Flag::Name(name) => named_flag(&name),
    })
}

fn named_flag(name: &str) -> u32 {
    let numbered = |prefix: &str, base: u32| {
        name.strip_prefix(prefix)
            .and_then(|n| n.parse::<u32>().ok())
            .map(|n| base + n)
    };
    numbered("LoSlot", flag::LOW)
        .or_else(|| numbered("MedSlot", flag::MID))
        .or_else(|| numbered("HiSlot", flag::HIGH))
        .or_else(|| numbered("RigSlot", flag::RIG))
        .or_else(|| numbered("SubSystemSlot", 125))
        .unwrap_or(match name {
            "DroneBay" => flag::DRONE_BAY,
            "FighterBay" => flag::FIGHTER_BAY,
            _ => flag::CARGO,
        })
}

/// The fit as ESI JSON. Like pyfa's copy dialog it leaves loaded charges, implants and boosters
/// out; pyfa only adds them when uploading to the game, which needs a login gyfa does not have.
pub fn export(fit: &Fit, catalog: &Catalog) -> Result<String, String> {
    let ship = fit.ship.ok_or("the fit has no ship")?;
    let mut items = Vec::new();
    for (slot, base) in [
        (Slot::Low, flag::LOW),
        (Slot::Mid, flag::MID),
        (Slot::High, flag::HIGH),
    ] {
        for (n, m) in fit.rack(slot).iter().flatten().enumerate() {
            items.push(EsiItem {
                flag: base + n as u32,
                quantity: 1,
                type_id: m.type_id,
            });
        }
    }
    for (n, &rig) in fit.rigs.iter().flatten().enumerate() {
        items.push(EsiItem {
            flag: flag::RIG + n as u32,
            quantity: 1,
            type_id: rig,
        });
    }
    for &subsystem in &fit.subsystems {
        items.push(EsiItem {
            flag: subsystem_flag(catalog, subsystem),
            quantity: 1,
            type_id: subsystem,
        });
    }
    for &(type_id, quantity) in &fit.cargo {
        items.push(EsiItem {
            flag: flag::CARGO,
            quantity,
            type_id,
        });
    }
    for d in &fit.drones {
        items.push(EsiItem {
            flag: flag::DRONE_BAY,
            quantity: d.count,
            type_id: d.type_id,
        });
    }
    if items.is_empty() {
        return Err("cannot export a fit without items".to_owned());
    }
    let esi = EsiFit {
        name: shorten(&fit.name, NAME_MAX),
        description: shorten(&fit.notes, DESCRIPTION_MAX),
        ship_type_id: ship,
        items,
    };
    serde_json::to_string(&esi).map_err(|e| e.to_string())
}

/// Cut text to `max` characters, marking the cut with dots like pyfa.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max - 3).collect();
    format!("{kept}...")
}

pub fn import(catalog: &Catalog, text: &str) -> Result<Imported, String> {
    let esi: EsiFit = serde_json::from_str(text).map_err(|e| format!("ESI fitting: {e}"))?;
    if catalog.get(esi.ship_type_id).is_none() {
        return Err(format!("unknown ship #{}", esi.ship_type_id));
    }
    let mut fit = new_fit(catalog, esi.ship_type_id, esi.name);
    fit.notes = esi.description;
    let mut items = esi.items;
    // Racks fill in flag order, so positions come back as they were.
    items.sort_by_key(|i| i.flag);
    let mut unknown = Vec::new();
    for item in items {
        if catalog.get(item.type_id).is_none() {
            unknown.push(format!("#{}", item.type_id));
            continue;
        }
        let bay = if item.flag == flag::CARGO {
            Bay::Cargo
        } else {
            Bay::Fitted
        };
        place(&mut fit, catalog, item.type_id, item.quantity, bay);
    }
    Ok(Imported {
        fits: vec![fit],
        unknown,
    })
}
