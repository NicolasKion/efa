//! The projected pane's rows in pyfa's order, and the changes a state click, a range or a removal
//! makes to what is projected.

use gpui_kit::Modifiers;

use crate::ui::additions::drones::format_amount;
use crate::{
    data::{Catalog, TypeId},
    fit::{Fit, ModuleState, projected::Projected},
    store::Saved,
};

/// pyfa's drone order for projected drones, by market group.
const DRONE_ORDER: [&str; 12] = [
    "Light Scout Drones",
    "Light Hybrid Drones",
    "Medium Scout Drones",
    "Medium Hybrid Drones",
    "Heavy Attack Drones",
    "Heavy Hybrid Drones",
    "Sentry Drones",
    "Combat Utility Drones",
    "Electronic Warfare Drones",
    "Logistic Drones",
    "Mining Drones",
    "Salvage Drones",
];

/// A row of the pane and the entry it shows, by index into its list on the fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    Fit { entry: usize, fit: usize },
    Module(usize),
    Env(usize),
    Drone(usize),
    Fighter(usize),
}

/// What a state click proposes for everything it touches, as pyfa's
/// `GuiChangeProjectedItemStatesCommand` works it out from the clicked row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Proposal {
    Active,
    Inactive,
    Overheat,
}

/// Which of the clicked row's peers an action takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reach {
    /// The selection, or the clicked row when it is not selected.
    Selection,
    /// Every module or squadron of the clicked one's type.
    Similar,
}

impl Reach {
    pub(super) fn from_modifiers(modifiers: Modifiers) -> Self {
        if modifiers.alt || modifiers.control {
            Self::Similar
        } else {
            Self::Selection
        }
    }
}

/// The pane's rows in pyfa's order: fits by name, then modules and environmental effects by
/// name, drones by class, fighters by name. Fits that were deleted are left out.
pub(super) fn rows(catalog: &Catalog, saved: &Saved, fit: &Fit) -> Vec<Row> {
    let projected = &fit.projected;
    let mut fits: Vec<Row> = projected
        .fits
        .iter()
        .enumerate()
        .filter_map(|(entry, p)| {
            Some(Row::Fit {
                entry,
                fit: saved.fit_index(p.fit)?,
            })
        })
        .collect();
    fits.sort_by_cached_key(|r| match *r {
        Row::Fit { fit, .. } => saved.fits[fit].name.clone(),
        _ => String::new(),
    });
    let mut modules: Vec<(String, Row)> = projected
        .modules
        .iter()
        .enumerate()
        .map(|(i, m)| (catalog.name(m.type_id), Row::Module(i)))
        .chain(
            projected
                .env
                .iter()
                .enumerate()
                .map(|(i, e)| (env_name(catalog, e.type_id), Row::Env(i))),
        )
        .collect();
    modules.sort_by(|a, b| a.0.cmp(&b.0));
    let mut drones: Vec<usize> = (0..projected.drones.len()).collect();
    drones.sort_by_cached_key(|&i| {
        let t = projected.drones[i].type_id;
        let group = catalog
            .get(t)
            .and_then(|t| t.market_group)
            .and_then(|g| catalog.market_groups.get(&g))
            .map(|g| g.name.as_str());
        let rank = group
            .and_then(|g| DRONE_ORDER.iter().position(|&o| o == g))
            .unwrap_or(DRONE_ORDER.len());
        (rank, catalog.name(t))
    });
    let mut fighters: Vec<usize> = (0..projected.fighters.len()).collect();
    fighters.sort_by_cached_key(|&i| catalog.name(projected.fighters[i].fighter.type_id));
    fits.into_iter()
        .chain(modules.into_iter().map(|(_, r)| r))
        .chain(drones.into_iter().map(Row::Drone))
        .chain(fighters.into_iter().map(Row::Fighter))
        .collect()
}

pub(super) fn env_name(catalog: &Catalog, type_id: TypeId) -> String {
    catalog
        .projection
        .env_name(type_id)
        .map_or_else(|| catalog.name(type_id), str::to_owned)
}

/// pyfa's projection range text: "10 km", "500 m", nothing without a range.
pub(super) fn range_text(range: Option<f64>) -> String {
    match range {
        None => String::new(),
        Some(m) if m >= 1000.0 => format!("{} km", format_amount(m / 1000.0, 0, false)),
        Some(m) => format!("{} m", format_amount(m, 0, false)),
    }
}

/// What the range prompt takes: kilometres, or nothing for no range. Like pyfa, anything but
/// digits and the point is ignored.
pub(super) struct RangeInput(pub(super) Option<f64>);

impl std::str::FromStr for RangeInput {
    type Err = std::num::ParseFloatError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let clean: String = text
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if clean.is_empty() {
            return Ok(Self(None));
        }
        Ok(Self(Some(clean.parse::<f64>()? * 1000.0)))
    }
}

impl Projected {
    pub(super) fn range(&self, row: Row) -> Option<f64> {
        match row {
            Row::Fit { entry, .. } => self.fits.get(entry)?.range,
            Row::Module(i) => self.modules.get(i)?.range,
            Row::Drone(i) => self.drones.get(i)?.range,
            Row::Fighter(i) => self.fighters.get(i)?.range,
            Row::Env(_) => None,
        }
    }

    pub(super) fn set_range(&mut self, row: Row, range: Option<f64>) {
        let slot = match row {
            Row::Fit { entry, .. } => self.fits.get_mut(entry).map(|p| &mut p.range),
            Row::Module(i) => self.modules.get_mut(i).map(|m| &mut m.range),
            Row::Drone(i) => self.drones.get_mut(i).map(|d| &mut d.range),
            Row::Fighter(i) => self.fighters.get_mut(i).map(|f| &mut f.range),
            Row::Env(_) => None,
        };
        if let Some(slot) = slot {
            *slot = range;
        }
    }

    pub(super) fn is_active(&self, row: Row) -> bool {
        match row {
            Row::Fit { entry, .. } => self.fits.get(entry).is_some_and(|p| p.active),
            Row::Module(i) => self
                .modules
                .get(i)
                .is_some_and(|m| m.state != ModuleState::Offline),
            Row::Env(i) => self.env.get(i).is_some_and(|e| e.active),
            Row::Drone(i) => self.drones.get(i).is_some_and(|d| d.active),
            Row::Fighter(i) => self.fighters.get(i).is_some_and(|f| f.fighter.active),
        }
    }

    /// The left-click proposal for a clicked row: off when it is on, on otherwise. A module only
    /// counts as on while active, as pyfa's projected state map has it.
    pub(super) fn proposal(&self, row: Row) -> Proposal {
        let on = match row {
            Row::Module(i) => self
                .modules
                .get(i)
                .is_some_and(|m| m.state == ModuleState::Active),
            _ => self.is_active(row),
        };
        if on {
            Proposal::Inactive
        } else {
            Proposal::Active
        }
    }

    pub(super) fn apply_proposal(&mut self, catalog: &Catalog, rows: &[Row], proposal: Proposal) {
        let on = proposal != Proposal::Inactive;
        for &row in rows {
            match row {
                Row::Fit { entry, .. } => {
                    if let Some(p) = self.fits.get_mut(entry) {
                        p.active = on;
                    }
                }
                Row::Module(i) => {
                    let Some(m) = self.modules.get_mut(i) else {
                        continue;
                    };
                    let t = catalog.get(m.type_id);
                    let activatable = t.is_some_and(|t| t.activatable);
                    let overloadable = t.is_some_and(|t| t.overloadable);
                    m.state = match proposal {
                        Proposal::Inactive => ModuleState::Offline,
                        Proposal::Overheat if overloadable => ModuleState::Overload,
                        _ if activatable => ModuleState::Active,
                        _ => ModuleState::Online,
                    };
                }
                Row::Env(i) => {
                    if let Some(e) = self.env.get_mut(i) {
                        e.active = on;
                    }
                }
                Row::Drone(i) => {
                    if let Some(d) = self.drones.get_mut(i) {
                        d.active = on;
                    }
                }
                Row::Fighter(i) => {
                    if let Some(f) = self.fighters.get_mut(i) {
                        f.fighter.active = on;
                    }
                }
            }
        }
    }

    /// Take rows away: `whole` drops them, otherwise a fit loses one copy and a drone stack one
    /// drone, the way pyfa's double-click does.
    pub(super) fn remove(&mut self, rows: &[Row], whole: bool) {
        let mut fits = Vec::new();
        let mut modules = Vec::new();
        let mut env = Vec::new();
        let mut drones = Vec::new();
        let mut fighters = Vec::new();
        for &row in rows {
            match row {
                Row::Fit { entry, .. } => fits.push(entry),
                Row::Module(i) => modules.push(i),
                Row::Env(i) => env.push(i),
                Row::Drone(i) => drones.push(i),
                Row::Fighter(i) => fighters.push(i),
            }
        }
        if !whole {
            for &i in &fits {
                if let Some(p) = self.fits.get_mut(i) {
                    p.amount = p.amount.saturating_sub(1);
                }
            }
            for &i in &drones {
                if let Some(d) = self.drones.get_mut(i) {
                    d.count = d.count.saturating_sub(1);
                }
            }
        }
        let drop = |len: usize, wanted: &[usize], emptied: &dyn Fn(usize) -> bool| {
            (0..len)
                .map(|i| (whole && wanted.contains(&i)) || emptied(i))
                .collect::<Vec<bool>>()
        };
        let gone_fits = drop(self.fits.len(), &fits, &|i| self.fits[i].amount == 0);
        let gone_drones = drop(self.drones.len(), &drones, &|i| self.drones[i].count == 0);
        let mut n = 0;
        self.fits.retain(|_| {
            n += 1;
            !gone_fits[n - 1]
        });
        let mut n = 0;
        self.drones.retain(|_| {
            n += 1;
            !gone_drones[n - 1]
        });
        retain_indices(&mut self.modules, &modules);
        retain_indices(&mut self.env, &env);
        retain_indices(&mut self.fighters, &fighters);
    }
}

/// Drop the entries at `indices`.
fn retain_indices<T>(list: &mut Vec<T>, indices: &[usize]) {
    let mut n = 0;
    list.retain(|_| {
        n += 1;
        !indices.contains(&(n - 1))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::projected::{EnvEffect, ProjectedDrone, ProjectedFit};

    #[test]
    fn ranges_read_in_km() {
        assert_eq!(range_text(None), "");
        assert_eq!(range_text(Some(500.0)), "500 m");
        assert_eq!(range_text(Some(12_500.0)), "12.5 km");
        let parsed: RangeInput = "12.5 km".parse().expect("parses");
        assert_eq!(parsed.0, Some(12_500.0));
        let empty: RangeInput = "".parse().expect("parses");
        assert_eq!(empty.0, None);
    }

    #[test]
    fn double_click_takes_one_copy_or_one_drone() {
        let mut projected = Projected {
            fits: vec![ProjectedFit {
                fit: 1,
                amount: 2,
                active: true,
                range: None,
            }],
            drones: vec![ProjectedDrone {
                type_id: 5,
                count: 1,
                active: true,
                range: None,
            }],
            env: vec![EnvEffect {
                type_id: 9,
                active: true,
            }],
            ..Projected::default()
        };
        projected.remove(&[Row::Fit { entry: 0, fit: 0 }, Row::Drone(0)], false);
        assert_eq!(projected.fits[0].amount, 1);
        assert!(projected.drones.is_empty(), "the last drone took its stack");
        projected.remove(&[Row::Fit { entry: 0, fit: 0 }, Row::Env(0)], true);
        assert!(projected.is_empty());
    }
}
