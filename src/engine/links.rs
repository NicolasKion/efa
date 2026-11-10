//! Generic command links as refine sees them. refine only boosts through real bursts on a fit in
//! the fleet, so links fly on a hidden capsule with T2 bursts and All 5's skills, loaded with
//! copies of the burst charges whose strength carries the ship bonus and mindlink picked.
//!
//! The copies are written into the adapted data on load, one per charge and strength, under type
//! IDs no EVE item uses.

use rc::{
    ItemTypeId, ModRack, SolOwned,
    ad::{AAttrId, AData, AItemAttr, AItemId, AValue},
};

use super::{
    Pilot,
    build::{add_fit_module, at},
    overrides::with_attrs,
    surroundings::CAPSULE,
};
use crate::{
    data::TypeId,
    dogma::attr,
    fit::{
        Module, ModuleState,
        links::{CommandLink, STRENGTHS, link_factor},
    },
};

/// Where the copies start; each strength and mindlink pair takes a block of IDs.
const COPY_BASE: i32 = 2_000_000_000;
const COPY_BLOCK: i32 = 100_000;

/// The type the hidden booster loads for `charge` at this strength: the charge itself when
/// nothing scales it.
pub(super) fn scaled_charge(charge: TypeId, strength: u8, mindlink: bool) -> TypeId {
    let variant = i32::from(strength.min(STRENGTHS[0])) + if mindlink { 6 } else { 0 };
    if variant == 0 || charge >= COPY_BLOCK {
        return charge;
    }
    COPY_BASE + variant * COPY_BLOCK + charge
}

/// Add the scaled copies of every burst charge to `data`.
pub(super) fn add_scaled_charges(data: &mut AData) -> Result<(), postcard::Error> {
    let multiplier = |a: &AItemAttr| match a.id {
        AAttrId::Eve(id) => attr::WARFARE_BUFF_MULTIPLIERS.contains(&id.into_i32()),
        AAttrId::Custom(_) => false,
    };
    let mut copies = Vec::new();
    for item in data.items.iter() {
        let charge = item.id.into_i32();
        if charge >= COPY_BLOCK || !item.attrs.iter().any(multiplier) {
            continue;
        }
        for mindlink in [false, true] {
            for strength in STRENGTHS {
                let id = scaled_charge(charge, strength, mindlink);
                if id == charge {
                    continue;
                }
                let factor = link_factor(strength, mindlink);
                let attrs = item
                    .attrs
                    .iter()
                    .map(|a| AItemAttr {
                        id: a.id,
                        value: if multiplier(a) {
                            AValue::from_f64(a.value.into_f64() * factor)
                        } else {
                            a.value
                        },
                    })
                    .collect();
                let mut copy = with_attrs(item, attrs)?;
                copy.id = AItemId::from_i32(id);
                copies.push(copy);
            }
        }
    }
    for copy in copies {
        data.items.insert(copy);
    }
    Ok(())
}

/// Put the active links on a hidden booster in `sol`, returning its fit to join the fleet.
pub(super) fn add_link_booster(
    sol: &mut SolOwned,
    links: &[CommandLink],
    pilot: &Pilot,
) -> Option<rc::FitId> {
    let bursts: Vec<Module> = links
        .iter()
        .filter(|l| l.active)
        .flat_map(|l| {
            l.set
                .charges()
                .into_iter()
                .map(|(category, charge)| Module {
                    type_id: category.burst(),
                    state: ModuleState::Active,
                    charge: Some(scaled_charge(charge, l.strength, l.mindlink)),
                    spool: None,
                    mutation: None,
                    rah: None,
                })
        })
        .collect();
    if bursts.is_empty() {
        return None;
    }
    let mut f = sol.add_fit();
    pilot.board(&mut f);
    f.set_ship(ItemTypeId::from_i32(CAPSULE), None, None);
    for (i, burst) in bursts.iter().enumerate() {
        let _ = add_fit_module(&mut f, ModRack::High, at(i), burst);
    }
    Some(f.get_fit_id())
}
