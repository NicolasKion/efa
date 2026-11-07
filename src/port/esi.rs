//! ESI's fitting JSON (service/port/esi.py), the shape the character fittings routes use:
//! a name, a description, the ship and items with inventory flags.

use serde::{Deserialize, Serialize};

use super::{Bay, Imported, charge_count, new_fit, place, subsystem_flag};
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
    pub const SUBSYSTEM: u32 = 125;
    pub const FIGHTER_BAY: u32 = 158;
}

/// ESI caps fitting names at 50 characters and descriptions at 500; pyfa cuts them shorter.
const NAME_MAX: usize = 50;
const DESCRIPTION_MAX: usize = 400;

/// A fitting as ESI's character fittings routes list it, and as the JSON format holds it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fitting {
    /// The game's ID for a fitting saved there; absent in files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fitting_id: Option<u64>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub ship_type_id: TypeId,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    #[serde(deserialize_with = "flag_from_any")]
    pub flag: u32,
    pub quantity: u32,
    pub type_id: TypeId,
}

/// An item as the fittings route takes it, with the flag by name.
#[derive(Debug, Serialize)]
struct NamedItem {
    flag: String,
    quantity: u32,
    type_id: TypeId,
}

#[derive(Debug, Serialize)]
struct Upload {
    name: String,
    description: String,
    ship_type_id: TypeId,
    items: Vec<NamedItem>,
}

/// What else goes up with a fit sent to EVE, pyfa's Export to EVE options. Each lands in the
/// cargo hold, as the game has nowhere else for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UploadOptions {
    /// The charges loaded in the modules.
    pub charges: bool,
    pub implants: bool,
    pub boosters: bool,
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self {
            charges: true,
            implants: false,
            boosters: false,
        }
    }
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
        .or_else(|| numbered("SubSystemSlot", flag::SUBSYSTEM))
        .unwrap_or(match name {
            "DroneBay" => flag::DRONE_BAY,
            "FighterBay" => flag::FIGHTER_BAY,
            _ => flag::CARGO,
        })
}

/// The name the fittings route wants for a numbered flag.
fn flag_name(flag: u32) -> String {
    let named = |name: &str, base: u32| format!("{name}{}", flag - base);
    match flag {
        f if (flag::LOW..flag::LOW + 8).contains(&f) => named("LoSlot", flag::LOW),
        f if (flag::MID..flag::MID + 8).contains(&f) => named("MedSlot", flag::MID),
        f if (flag::HIGH..flag::HIGH + 8).contains(&f) => named("HiSlot", flag::HIGH),
        f if (flag::RIG..flag::RIG + 8).contains(&f) => named("RigSlot", flag::RIG),
        f if (flag::SUBSYSTEM..flag::SUBSYSTEM + 8).contains(&f) => {
            named("SubSystemSlot", flag::SUBSYSTEM)
        }
        flag::DRONE_BAY => "DroneBay".to_owned(),
        flag::FIGHTER_BAY => "FighterBay".to_owned(),
        _ => "Cargo".to_owned(),
    }
}

/// The fit's racks, rigs, subsystems, cargo and drones as flagged items.
fn items(fit: &Fit, catalog: &Catalog) -> Vec<Item> {
    let mut items = Vec::new();
    for (slot, base) in [
        (Slot::Low, flag::LOW),
        (Slot::Mid, flag::MID),
        (Slot::High, flag::HIGH),
    ] {
        for (n, m) in fit.rack(slot).iter().flatten().enumerate() {
            items.push(Item {
                flag: base + n as u32,
                quantity: 1,
                type_id: m.type_id,
            });
        }
    }
    for (n, &rig) in fit.rigs.iter().flatten().enumerate() {
        items.push(Item {
            flag: flag::RIG + n as u32,
            quantity: 1,
            type_id: rig,
        });
    }
    for &subsystem in &fit.subsystems {
        items.push(Item {
            flag: subsystem_flag(catalog, subsystem),
            quantity: 1,
            type_id: subsystem,
        });
    }
    for &(type_id, quantity) in &fit.cargo {
        items.push(Item {
            flag: flag::CARGO,
            quantity,
            type_id,
        });
    }
    for d in &fit.drones {
        items.push(Item {
            flag: flag::DRONE_BAY,
            quantity: d.count,
            type_id: d.type_id,
        });
    }
    for f in &fit.fighters {
        items.push(Item {
            flag: flag::FIGHTER_BAY,
            quantity: f.size(catalog),
            type_id: f.type_id,
        });
    }
    items
}

/// The fit as ESI JSON. Like pyfa's copy dialog it leaves loaded charges, implants and boosters
/// out; those only go along when uploading to the game, see [`upload`].
pub fn export(fit: &Fit, catalog: &Catalog) -> Result<String, String> {
    let ship = fit.ship.ok_or("the fit has no ship")?;
    let items = items(fit, catalog);
    if items.is_empty() {
        return Err("cannot export a fit without items".to_owned());
    }
    let esi = Fitting {
        fitting_id: None,
        name: shorten(&fit.name, NAME_MAX),
        description: shorten(&fit.notes, DESCRIPTION_MAX),
        ship_type_id: ship,
        items,
    };
    serde_json::to_string(&esi).map_err(|e| e.to_string())
}

/// The body for saving the fit in the game (pyfa's Export to EVE): flags by name, and what
/// `options` asks for in the cargo hold.
pub fn upload(fit: &Fit, catalog: &Catalog, options: UploadOptions) -> Result<String, String> {
    let ship = fit.ship.ok_or("the fit has no ship")?;
    let mut items = items(fit, catalog);
    let mut extra: Vec<(TypeId, u32)> = Vec::new();
    let mut add = |type_id: TypeId, count: u32| match extra.iter_mut().find(|(t, _)| *t == type_id)
    {
        Some((_, n)) => *n += count,
        None => extra.push((type_id, count)),
    };
    if options.charges {
        for m in fit.modules() {
            if let Some(charge) = m.charge {
                add(charge, charge_count(catalog, m.type_id, charge));
            }
        }
    }
    if options.implants {
        for &implant in &fit.implants {
            add(implant, 1);
        }
    }
    if options.boosters {
        for b in &fit.boosters {
            add(b.type_id, 1);
        }
    }
    items.extend(extra.into_iter().map(|(type_id, quantity)| Item {
        flag: flag::CARGO,
        quantity,
        type_id,
    }));
    if items.is_empty() {
        return Err("cannot export a fit without items".to_owned());
    }
    let body = Upload {
        name: shorten(&fit.name, NAME_MAX),
        description: shorten(&fit.notes, DESCRIPTION_MAX),
        ship_type_id: ship,
        items: items
            .into_iter()
            .map(|i| NamedItem {
                flag: flag_name(i.flag),
                quantity: i.quantity,
                type_id: i.type_id,
            })
            .collect(),
    };
    serde_json::to_string(&body).map_err(|e| e.to_string())
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
    let esi: Fitting = serde_json::from_str(text).map_err(|e| format!("ESI fitting: {e}"))?;
    to_fit(catalog, &esi)
}

/// A fitting from ESI or a file as a efa fit.
pub fn to_fit(catalog: &Catalog, esi: &Fitting) -> Result<Imported, String> {
    if catalog.get(esi.ship_type_id).is_none() {
        return Err(format!("unknown ship #{}", esi.ship_type_id));
    }
    let mut fit = new_fit(catalog, esi.ship_type_id, esi.name.clone());
    fit.notes.clone_from(&esi.description);
    let mut items = esi.items.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_round_trip_through_names() {
        for f in [
            flag::LOW,
            flag::LOW + 7,
            flag::MID + 3,
            flag::HIGH,
            flag::RIG + 2,
            flag::SUBSYSTEM + 3,
            flag::DRONE_BAY,
            flag::FIGHTER_BAY,
            flag::CARGO,
        ] {
            assert_eq!(named_flag(&flag_name(f)), f, "{}", flag_name(f));
        }
        assert_eq!(flag_name(flag::HIGH + 1), "HiSlot1");
        assert_eq!(flag_name(flag::MID), "MedSlot0");
        assert_eq!(flag_name(126), "SubSystemSlot1");
    }

    #[test]
    fn reads_the_fittings_list() {
        let json = r#"[{"fitting_id": 42, "name": "Rat", "description": "d",
            "ship_type_id": 587, "items": [
                {"flag": "HiSlot0", "quantity": 1, "type_id": 3001},
                {"flag": "Cargo", "quantity": 100, "type_id": 21894}
            ]}]"#;
        let fits: Vec<Fitting> = serde_json::from_str(json).expect("parses");
        assert_eq!(fits[0].fitting_id, Some(42));
        assert_eq!(fits[0].items[0].flag, flag::HIGH);
        assert_eq!(fits[0].items[1].flag, flag::CARGO);
    }

    #[test]
    fn long_names_are_cut() {
        let name = "x".repeat(60);
        let cut = shorten(&name, NAME_MAX);
        assert_eq!(cut.chars().count(), NAME_MAX);
        assert!(cut.ends_with("..."));
    }
}
