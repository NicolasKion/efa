//! How much the hull's bonuses add to weapon damage, pyfa's `getWeaponBonusMultipliers`.

use rc::{AddMode, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack};

use crate::engine::{Engine, Pilot, read};
use crate::{
    data::{Catalog, Hardpoint, Kind, TypeId},
    dogma::attr,
    fit::Fit,
};

/// The capsule, the hull without bonuses the weapon multipliers are measured against.
const CAPSULE: TypeId = 670;

/// pyfa's Tech II meta group, which the weapon test set is taken from.
const TECH_II: i32 = 2;

/// A weapon of pyfa's test set.
#[derive(Debug, Clone, Copy)]
enum TestWeapon {
    Turret(TypeId),
    /// A launcher with one of the charges it takes.
    Launcher(TypeId, TypeId),
    Drone(TypeId),
}

/// pyfa's `getTestSet`: Tech II turrets, one per charge group and size; Tech II launchers with
/// one charge of each family they take, for the damage-type bonuses; Tech II drones.
fn test_set(catalog: &Catalog) -> Vec<TestWeapon> {
    let mut t2: Vec<&crate::data::ItemType> = catalog
        .types
        .values()
        .filter(|t| t.published && t.meta_group == Some(TECH_II))
        .collect();
    t2.sort_by_key(|t| t.id);
    let mut set = Vec::new();
    let mut turret_kinds = Vec::new();
    for t in &t2 {
        match (t.kind, t.hardpoint) {
            (Kind::Module(_), Hardpoint::Turret) if !t.mining && !t.charge_groups.is_empty() => {
                let kind = (t.charge_groups[0], t.charge_size.map(f64::to_bits));
                if !turret_kinds.contains(&kind) {
                    turret_kinds.push(kind);
                    set.push(TestWeapon::Turret(t.id));
                }
            }
            (Kind::Module(_), Hardpoint::Launcher) => {
                let Some(&group) = t.charge_groups.first() else {
                    continue;
                };
                let mut charges: Vec<&crate::data::ItemType> = catalog
                    .types
                    .values()
                    .filter(|c| c.group_id == group && c.published && c.kind == Kind::Charge)
                    .collect();
                charges.sort_by_key(|c| c.id);
                let mut families = Vec::new();
                for c in charges {
                    let family = c.variation_parent.unwrap_or(c.id);
                    if !families.contains(&family) {
                        families.push(family);
                        set.push(TestWeapon::Launcher(t.id, c.id));
                    }
                }
            }
            (Kind::Drone, _) => set.push(TestWeapon::Drone(t.id)),
            _ => {}
        }
    }
    set
}

impl TestWeapon {
    fn kind(self) -> usize {
        match self {
            Self::Turret(_) => 0,
            Self::Launcher(..) => 1,
            Self::Drone(_) => 2,
        }
    }
}

impl Engine {
    /// How much more damage the hull's bonuses give turrets, launchers and drones, pyfa's
    /// `getWeaponBonusMultipliers`: Tech II weapons on this hull against the same on a capsule,
    /// the biggest gain of each kind.
    pub(super) fn weapon_multipliers(
        &self,
        fit: &Fit,
        pilot: &Pilot,
        catalog: &Catalog,
    ) -> [f64; 3] {
        let test = test_set(catalog);
        let bare = Fit {
            ship: Some(CAPSULE),
            ..Fit::default()
        };
        let pre = self.measure(&bare, pilot, &test);
        let post = self.measure(fit, pilot, &test);
        let mut best = [0.0_f64; 3];
        for ((kind, before), (_, after)) in pre.iter().zip(&post) {
            if *before > 0.0 && after.is_finite() {
                best[*kind] = best[*kind].max(after / before);
            }
        }
        best.map(|ratio| {
            if ratio > 0.0 {
                (ratio * 1e6).round() / 1e6
            } else {
                1.0
            }
        })
    }

    /// Damage per second factor of each test weapon on the fit's hull, mode and subsystems:
    /// (kind, factor) with kinds 0 turret, 1 launcher, 2 drone.
    fn measure(&self, fit: &Fit, pilot: &Pilot, test: &[TestWeapon]) -> Vec<(usize, f64)> {
        let mut sol = self.sol();
        let mut f = sol.add_fit();
        pilot.board(&mut f);
        if let Some(ship) = fit.ship {
            f.set_ship(ItemTypeId::from_i32(ship), None, None);
        }
        if let Some(mode) = fit.mode {
            f.set_stance(ItemTypeId::from_i32(mode));
        }
        for &sub in &fit.subsystems {
            f.add_subsystem(ItemTypeId::from_i32(sub));
        }
        let mut ids = Vec::with_capacity(test.len());
        for weapon in test {
            let id = match *weapon {
                TestWeapon::Turret(t) | TestWeapon::Launcher(t, _) => {
                    let Ok(mut m) = f.add_module(
                        ModRack::High,
                        AddMode::Append,
                        ItemTypeId::from_i32(t),
                        rc::ModuleState::Active,
                    ) else {
                        ids.push(None);
                        continue;
                    };
                    if let TestWeapon::Launcher(_, charge) = *weapon {
                        m.set_charge_type_id(ItemTypeId::from_i32(charge));
                    }
                    m.get_item_id()
                }
                TestWeapon::Drone(t) => f
                    .add_drone(ItemTypeId::from_i32(t), MinionState::Engaging, None, None)
                    .get_item_id(),
            };
            ids.push(Some(id));
        }
        let mut charges = Vec::new();
        for (weapon, id) in test.iter().zip(&ids) {
            if let (TestWeapon::Launcher(..), Some(id)) = (weapon, id) {
                charges.push(
                    sol.get_module(id)
                        .ok()
                        .and_then(|mut m| m.get_charge().map(|c| c.get_item_id())),
                );
            } else {
                charges.push(None);
            }
        }
        let mut read = |id: ItemId, attr: i32| read::attr_or_zero(&mut sol, id, attr);
        test.iter()
            .zip(ids)
            .zip(charges)
            .map(|((weapon, id), charge)| {
                let Some(id) = id else {
                    return (weapon.kind(), 0.0);
                };
                let sum = |read: &mut dyn FnMut(ItemId, i32) -> f64, id: ItemId| {
                    attr::DAMAGE.iter().map(|&a| read(id, a)).sum::<f64>()
                };
                let factor = match weapon {
                    TestWeapon::Turret(_) => {
                        let rof = read(id, attr::RATE_OF_FIRE);
                        if rof > 0.0 {
                            read(id, attr::DAMAGE_MULTIPLIER) / rof
                        } else {
                            0.0
                        }
                    }
                    TestWeapon::Launcher(..) => {
                        let rof = read(id, attr::RATE_OF_FIRE);
                        match charge {
                            Some(c) if rof > 0.0 => sum(&mut read, c) / rof,
                            _ => 0.0,
                        }
                    }
                    TestWeapon::Drone(_) => sum(&mut read, id) * read(id, attr::DAMAGE_MULTIPLIER),
                };
                (weapon.kind(), factor)
            })
            .collect()
    }
}
