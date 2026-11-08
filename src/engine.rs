//! The bridge to refine. Each recalculation builds a fresh solar system from the [`Fit`], which
//! keeps refine's state from ever drifting from what the UI shows.

mod build;
mod column;
mod efs;
mod graph;
mod inspect;
mod items;
mod overrides;
mod pane;
mod pilot;
mod probe;
mod read;
mod source;
mod stats;
mod surroundings;
mod validate;

use std::{path::Path, sync::RwLock};

use rc::{ItemId, SolOwned, Src};

use crate::{data::TypeId, dogma::attr, fit::Fit};

use build::{BuildOptions, Built, Placed};
use items::{booster_info, drone_info};
use stats::{fit_stats, resource, slots};

pub use column::{ModuleMisc, RackHeat, ShipHeat};
pub use efs::EfsContext;
pub use inspect::{AttrValue, FitItem, Inspection, ModOp, Modifier, Source, SourceKind};
pub use items::{BoosterInfo, DroneInfo, SideEffectInfo};
pub use overrides::Overridden;
pub use pane::{
    CalcOptions, CapDetail, FirepowerDetail, Mining, MiningRate, Misc, Outgoing, SpoolRange, Tank,
};
pub use pilot::{MAX_LEVEL, Pilot, SkillTarget};
pub use probe::{TankBench, TankProbe};
pub use source::{EngineSource, PackedEngine};
pub use stats::{Breacher, Cap, Damage, Layer, ModuleInfo, Resource, Slots, Stats};
pub use surroundings::{AbilityInfo, FighterClasses, FighterInfo, surrounding_fits};
pub use validate::{Budget, ItemRef, MissingSkill, Problem, Validation};

/// Type ID of the character item. EVE uses this one for everyone.
const CHARACTER: i32 = 1373;

pub struct Engine {
    /// refine's data, with the attribute overrides in [`Engine::overrides`] applied.
    src: RwLock<Src>,
    overrides: RwLock<crate::overrides::Values>,
    /// Where the data came from, for rebuilding it with other overrides.
    source: EngineSource,
    /// The built-in All 5 character, which [`Engine::compute`] flies fits with.
    all5: Pilot,
}

impl Engine {
    /// Load refine's data from a Phobos-layout dump. The adapted data is cached in `cache_dir`, so
    /// only the first start after a data update pays for converting it.
    pub fn load(data_dir: &Path, cache_dir: &Path, skills: Vec<TypeId>) -> Result<Self, String> {
        let source = EngineSource::Dump {
            data_dir: data_dir.to_owned(),
            cache_dir: cache_dir.to_owned(),
        };
        Self::from_source(source, skills)
    }

    /// Load refine's data from wherever `source` says, the data pack or a dump.
    pub fn from_source(source: EngineSource, skills: Vec<TypeId>) -> Result<Self, String> {
        let src = source::load_src(&source, &crate::overrides::Values::new())?;
        let all5 = Pilot {
            skills: skills.into_iter().map(|s| (s, MAX_LEVEL)).collect(),
            ..Pilot::default()
        };
        Ok(Self {
            src: RwLock::new(src),
            overrides: RwLock::default(),
            source,
            all5,
        })
    }

    /// Where the engine's data came from, for [`Overridden::load`].
    pub fn source(&self) -> &EngineSource {
        &self.source
    }

    /// A solar system on the engine's data.
    fn sol(&self) -> SolOwned {
        SolOwned::new(&self.src.read().expect("engine data lock"))
    }

    /// The attribute overrides the engine calculates with.
    pub fn overrides(&self) -> crate::overrides::Values {
        self.overrides.read().expect("overrides lock").clone()
    }

    /// The attributes of a type whose base value the engine takes from an override.
    pub fn overridden_attrs(&self, type_id: TypeId) -> Vec<i32> {
        self.overrides
            .read()
            .expect("overrides lock")
            .get(&type_id)
            .map_or_default(|attrs| attrs.keys().copied().collect())
    }

    /// Calculate with data [`Overridden::load`] built from now on.
    pub fn set_overrides(&self, data: Overridden) {
        *self.src.write().expect("engine data lock") = data.src;
        *self.overrides.write().expect("overrides lock") = data.values;
    }

    /// Calculate with these attribute overrides from now on, building the data right here.
    pub fn apply_overrides(&self, values: &crate::overrides::Values) -> Result<(), String> {
        if *self.overrides.read().expect("overrides lock") == *values {
            return Ok(());
        }
        self.set_overrides(Overridden::load(&self.source, values)?);
        Ok(())
    }

    /// The built-in All 5 character.
    pub fn all5(&self) -> &Pilot {
        &self.all5
    }

    pub fn compute(&self, fit: &Fit) -> Stats {
        self.compute_with(fit, CalcOptions::default())
    }

    /// Stats with the All 5 character; [`Engine::compute_for`] takes the fit's own.
    pub fn compute_with(&self, fit: &Fit, options: CalcOptions) -> Stats {
        self.compute_for(fit, &self.all5, options)
    }

    /// The fit's stats flown by `pilot`, alone in space.
    pub fn compute_for(&self, fit: &Fit, pilot: &Pilot, options: CalcOptions) -> Stats {
        self.compute_among(fit, &[], pilot, options)
    }

    /// The fit's stats flown by `pilot`, with what surrounds it: `fits` are the saved fits its
    /// projected and command fits are looked up in.
    pub fn compute_among(
        &self,
        fit: &Fit,
        fits: &[Fit],
        pilot: &Pilot,
        options: CalcOptions,
    ) -> Stats {
        let build = BuildOptions {
            surroundings: Some(fits),
            default_spool: options.default_spool,
        };
        self.build(fit, pilot, build)
            .map_or_default(|mut built| stats_of(&mut built, fit, options))
    }
}

/// Every number the stats pane and the fitting view show, read from a fit built into a solar
/// system.
fn stats_of(built: &mut Built, fit: &Fit, options: CalcOptions) -> Stats {
    let mut stats = Stats {
        problems: built.fit.problems.clone(),
        incoming: built.incoming,
        ..Stats::default()
    };
    let Placed {
        fit_id,
        ship: ship_id,
        ref modules,
        ref subsystems,
        ref boosters,
        ref fighters,
        ref items,
        ..
    } = built.fit;
    let sol = &mut built.sol;
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return stats;
    };
    let incoming = pane::dps_profile(stats.incoming);
    fit_stats(&mut f, &mut stats, incoming, options);
    stats.validation = validate::validate(&mut f, items);
    stats.firepower = pane::firepower_detail(&mut f, options.time());

    stats.cargo_capacity = read::attr(sol, ship_id, attr::CAPACITY);

    stats.boosters = boosters.iter().map(|&id| booster_info(sol, id)).collect();
    stats.drones = built
        .fit
        .first_drones()
        .map(|id| id.map_or_default(|id| drone_info(sol, id)))
        .collect();
    stats.fighters = fighters
        .iter()
        .map(|&id| surroundings::fighter_info(sol, id))
        .collect();
    stats.projected_fighters = built
        .projections
        .fighters
        .iter()
        .map(|&id| surroundings::fighter_info(sol, id))
        .collect();
    stats.projected_spool = built
        .projections
        .modules
        .iter()
        .map(|id| {
            let id = id.as_ref()?;
            let mut module = sol.get_module(id).ok()?;
            module
                .get_spool_cycle_count()
                .map(|info| info.max.into_u32())
        })
        .collect();
    stats.fighter_classes = surroundings::fighter_classes(sol, fit_id);

    for &(slot, i, id) in modules {
        let (spool_max, charges, charged_cycles) =
            sol.get_module(&id).ok().map_or_default(|mut m| {
                (
                    m.get_spool_cycle_count().map(|info| info.max.into_u32()),
                    m.get_charge_count().map(rc::Count::into_u32),
                    m.get_charged_cycle_count().map(rc::Count::into_u32),
                )
            });
        if sol.get_item(&id).is_err() {
            continue;
        }
        let info = ModuleInfo {
            cpu: read::positive_attr(sol, id, attr::CPU),
            powergrid: read::positive_attr(sol, id, attr::POWER),
            cap_per_sec: read::cap_per_sec(sol, id),
            optimal: read::positive_attr(sol, id, attr::MAX_RANGE),
            falloff: read::positive_attr(sol, id, attr::FALLOFF),
            spool_max,
            burnout: None,
            burnout_from_neighbours: false,
            charges,
            charged_cycles,
        };
        stats.modules.insert((slot, i), info);
        stats
            .module_misc
            .insert(FitItem::Module(slot, i), column::module_misc(sol, id));
    }
    for (i, &id) in subsystems.iter().enumerate() {
        stats
            .module_misc
            .insert(FitItem::Subsystem(i), column::module_misc(sol, id));
    }
    column::burnout(sol, fit, ship_id, modules, &mut stats);

    let cap_users: Vec<(ItemId, f64)> = modules
        .iter()
        .filter(|&&(slot, i, _)| {
            fit.rack(slot)
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|m| m.state.is_running())
        })
        .filter_map(|&(slot, i, id)| {
            let cap = stats.modules.get(&(slot, i))?.cap_per_sec?;
            Some((id, cap))
        })
        .collect();
    pane::pane_stats(sol, fit_id, ship_id, &cap_users, options, &mut stats);
    stats
}
