//! What other ships and the environment do to a fit: pyfa's projected fits, modules, drones and
//! fighters, environmental effects, and the fleet boosters of the Command tab.
//!
//! Other fits are referenced by their [`Fit::uid`], which survives fits being deleted or moved
//! around in the saved list.

use serde::{Deserialize, Serialize};

use super::{Fit, ModuleState, fighters::Fighter};
use crate::data::TypeId;

/// pyfa's limit on how many copies of a fit can be projected.
pub const MAX_FIT_COPIES: u32 = 20;

/// Another saved fit projecting onto this one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectedFit {
    pub fit: u64,
    /// Copies of the fit projecting at once.
    pub amount: u32,
    pub active: bool,
    /// Surface to surface distance in m; None applies everything at full strength.
    #[serde(default)]
    pub range: Option<f64>,
}

/// A module projecting without a fit around it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectedModule {
    pub type_id: TypeId,
    pub state: ModuleState,
    #[serde(default)]
    pub charge: Option<TypeId>,
    #[serde(default)]
    pub spool: Option<u32>,
    #[serde(default)]
    pub range: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectedDrone {
    pub type_id: TypeId,
    pub count: u32,
    /// pyfa's projected drone stacks are all out or all in.
    pub active: bool,
    #[serde(default)]
    pub range: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectedFighter {
    pub fighter: Fighter,
    #[serde(default)]
    pub range: Option<f64>,
}

/// A wormhole, incursion, abyssal weather or other system effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEffect {
    pub type_id: TypeId,
    pub active: bool,
}

/// Everything projected onto a fit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Projected {
    pub fits: Vec<ProjectedFit>,
    pub modules: Vec<ProjectedModule>,
    pub drones: Vec<ProjectedDrone>,
    pub fighters: Vec<ProjectedFighter>,
    pub env: Vec<EnvEffect>,
}

impl Projected {
    pub fn is_empty(&self) -> bool {
        self.fits.is_empty()
            && self.modules.is_empty()
            && self.drones.is_empty()
            && self.fighters.is_empty()
            && self.env.is_empty()
    }

    /// Rows the Projected tab lists.
    pub fn len(&self) -> usize {
        self.fits.len()
            + self.modules.len()
            + self.drones.len()
            + self.fighters.len()
            + self.env.len()
    }

    /// Project a fit, or one more copy of it when it is projected already, as pyfa does.
    pub fn add_fit(&mut self, uid: u64) {
        match self.fits.iter_mut().find(|p| p.fit == uid) {
            Some(p) => p.amount = (p.amount + 1).min(MAX_FIT_COPIES),
            None => self.fits.push(ProjectedFit {
                fit: uid,
                amount: 1,
                active: true,
                range: None,
            }),
        }
    }

    pub fn add_module(&mut self, type_id: TypeId, activatable: bool) {
        self.modules.push(ProjectedModule {
            type_id,
            state: if activatable {
                ModuleState::Active
            } else {
                ModuleState::Online
            },
            charge: None,
            spool: None,
            range: None,
        });
    }

    /// Add drones to the stack of that drone, launching them if the stack is out.
    pub fn add_drones(&mut self, type_id: TypeId, count: u32) {
        match self.drones.iter_mut().find(|d| d.type_id == type_id) {
            Some(stack) => stack.count += count,
            None => self.drones.push(ProjectedDrone {
                type_id,
                count,
                active: true,
                range: None,
            }),
        }
    }

    pub fn add_fighter(&mut self, type_id: TypeId) {
        self.fighters.push(ProjectedFighter {
            fighter: Fighter::new(type_id),
            range: None,
        });
    }

    pub fn add_env(&mut self, type_id: TypeId) {
        self.env.push(EnvEffect {
            type_id,
            active: true,
        });
    }

    /// Drop references to fits that no longer exist.
    pub fn retain_fits(&mut self, exists: impl Fn(u64) -> bool) {
        self.fits.retain(|p| exists(p.fit));
    }
}

/// A fit boosting this one from the same fleet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandFit {
    pub fit: u64,
    pub active: bool,
}

impl Fit {
    /// Add a fleet booster. A fit boosts another only once; returns whether it was added.
    pub fn add_command_fit(&mut self, uid: u64) -> bool {
        if self.command.iter().any(|c| c.fit == uid) {
            return false;
        }
        self.command.push(CommandFit {
            fit: uid,
            active: true,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projecting_a_fit_again_adds_a_copy() {
        let mut projected = Projected::default();
        projected.add_fit(7);
        projected.add_fit(7);
        assert_eq!(projected.fits.len(), 1);
        assert_eq!(projected.fits[0].amount, 2);
        for _ in 0..30 {
            projected.add_fit(7);
        }
        assert_eq!(projected.fits[0].amount, MAX_FIT_COPIES);
    }

    #[test]
    fn projected_drones_join_their_stack() {
        let mut projected = Projected::default();
        projected.add_drones(1, 2);
        projected.add_drones(1, 3);
        projected.add_drones(2, 1);
        let stacks: Vec<(TypeId, u32)> = projected
            .drones
            .iter()
            .map(|d| (d.type_id, d.count))
            .collect();
        assert_eq!(stacks, [(1, 5), (2, 1)]);
    }

    #[test]
    fn a_fit_boosts_once() {
        let mut fit = Fit::default();
        assert!(fit.add_command_fit(3));
        assert!(!fit.add_command_fit(3));
        assert_eq!(fit.command.len(), 1);
    }

    #[test]
    fn saves_without_projections_load() {
        let fit: Fit = serde_json::from_str(
            r#"{"name":"x","ship":null,"high":[],"mid":[],"low":[],"rigs":[],"subsystems":[],
                "drones":[],"implants":[],"boosters":[],"cargo":[]}"#,
        )
        .expect("an old save parses");
        assert!(fit.projected.is_empty());
        assert_eq!(fit.command, []);
        assert_eq!(fit.fighters, []);
        assert_eq!(fit.uid, 0);
    }
}
