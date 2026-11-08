//! pyfa's Application Profile graph: at each distance, the charge that does the most for each
//! group of identical weapons, drawn in segments named after the charge.
//!
//! The candidate charges come from the catalog; their damage and application numbers come from
//! refine with the charge loaded ([`crate::engine::Engine::graph_ammo`]).

use serde::{Deserialize, Serialize};

use super::{
    defs::Param,
    plot::{self, Request, TargetData},
    snapshot::{Dmg, FitSnapshot, Weapon},
};
use crate::data::{Catalog, ItemType, TypeId};

/// Which charges the graph considers, pyfa's "Ammo Meta" choice. Each tier includes the ones
/// before it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AmmoQuality {
    T1,
    #[default]
    Navy,
    All,
}

impl AmmoQuality {
    pub const ALL: [Self; 3] = [Self::T1, Self::Navy, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::Navy => "Navy",
            Self::All => "All",
        }
    }
}

/// How segments show their charge, pyfa's "Style" choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmoStyle {
    /// The fit's own color and the target's dash throughout.
    None,
    /// A dash per charge.
    Pattern,
    /// A color per charge.
    #[default]
    Color,
}

impl AmmoStyle {
    pub const ALL: [Self; 3] = [Self::None, Self::Pattern, Self::Color];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Pattern => "Pattern",
            Self::Color => "Color",
        }
    }
}

/// Empire navy ammo, which the "Navy" tier adds to tech I and II.
const NAVY_PREFIXES: [&str; 4] = [
    "Imperial Navy ",
    "Republic Fleet ",
    "Caldari Navy ",
    "Federation Navy ",
];
/// Capital ammo has no navy line; pyfa counts these pirate lines instead.
const CAPITAL_NAVY_PREFIXES: [&str; 3] = ["Sansha ", "Arch Angel ", "Shadow "];
/// Charge size of capital (XL) ammo.
const CAPITAL_SIZE: f64 = 4.0;

/// Whether a charge belongs to `quality`, pyfa's `filterChargesByQuality`. Charges without a
/// meta group count as tech I.
pub fn in_quality(charge: &ItemType, quality: AmmoQuality) -> bool {
    let meta = charge.meta_group.unwrap_or(1);
    match quality {
        AmmoQuality::All => true,
        AmmoQuality::T1 => meta == 1,
        AmmoQuality::Navy => {
            let capital = charge.charge_size == Some(CAPITAL_SIZE);
            meta == 1
                || meta == 2
                || NAVY_PREFIXES.iter().any(|p| charge.name.starts_with(p))
                || capital
                    && CAPITAL_NAVY_PREFIXES
                        .iter()
                        .any(|p| charge.name.starts_with(p))
        }
    }
}

/// Published charges a module type takes, by name.
pub fn charges_for(catalog: &Catalog, module: TypeId, quality: AmmoQuality) -> Vec<TypeId> {
    let Some(module) = catalog.get(module) else {
        return Vec::new();
    };
    let mut charges: Vec<&ItemType> = catalog
        .types
        .values()
        .filter(|t| t.published && t.fits_in(module))
        .filter(|t| module.capacity <= 0.0 || t.volume <= module.capacity)
        .filter(|t| in_quality(t, quality))
        .collect();
    charges.sort_by(|a, b| a.name.cmp(&b.name));
    charges.into_iter().map(|t| t.id).collect()
}

/// One charge loaded into a whole weapon group.
#[derive(Debug, Clone)]
pub struct AmmoOption {
    pub charge: TypeId,
    pub name: String,
    pub weapons: Vec<Weapon>,
}

/// Identical weapons that switch ammo together.
#[derive(Debug, Clone)]
pub struct WeaponGroup {
    pub module: TypeId,
    pub options: Vec<AmmoOption>,
}

/// A stretch of the line where the first group's best charge stays the same.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub ammo: String,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

/// Points per line; pyfa samples every 100 m or coarser on long ranges.
const SAMPLES: u32 = 200;

/// The best total at each distance over `req.range` (km), split where the best charge of the
/// first group changes.
pub fn segments(
    req: &Request,
    src: &FitSnapshot,
    groups: &[WeaponGroup],
    tgt: &TargetData,
) -> Vec<Segment> {
    if groups.iter().all(|g| g.options.is_empty()) {
        return Vec::new();
    }
    let base = plot::engagement(req, &src.ship, tgt);
    let volley = req.y.param == Param::Volley;
    // What each option's weapons deal before application, per group.
    let values: Vec<Vec<Vec<Dmg>>> = groups
        .iter()
        .map(|g| {
            g.options
                .iter()
                .map(|o| {
                    o.weapons
                        .iter()
                        .map(|w| if volley { w.volley } else { w.dps })
                        .collect()
                })
                .collect()
        })
        .collect();
    let value = |o: &AmmoOption, values: &[Dmg], distance: f64| -> f64 {
        let mut e = base;
        e.distance = Some(distance);
        plot::applied_damage(req, src, tgt, &o.weapons, values, e)
    };
    let (low, high) = (req.range.0.max(0.0), req.range.1.max(0.0));
    let mut out: Vec<Segment> = Vec::new();
    for i in 0..=SAMPLES {
        let km = low + (high - low) * f64::from(i) / f64::from(SAMPLES);
        let meters = km * 1000.0;
        let mut total = 0.0;
        let mut first_best: Option<&str> = None;
        for (n, (group, values)) in groups.iter().zip(&values).enumerate() {
            let best = group
                .options
                .iter()
                .zip(values)
                .map(|(o, values)| (o, value(o, values, meters)))
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((option, v)) = best {
                total += v;
                if n == 0 {
                    first_best = Some(&option.name);
                }
            }
        }
        let ammo = first_best.unwrap_or_default();
        match out.last_mut() {
            Some(last) if last.ammo == ammo => {
                last.xs.push(km);
                last.ys.push(total);
            }
            Some(last) => {
                // The old segment runs up to where the new one starts, so the line stays whole.
                last.xs.push(km);
                last.ys.push(total);
                out.push(Segment {
                    ammo: ammo.to_owned(),
                    xs: vec![km],
                    ys: vec![total],
                });
            }
            None => out.push(Segment {
                ammo: ammo.to_owned(),
                xs: vec![km],
                ys: vec![total],
            }),
        }
        if high <= low {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::graphs::{
        defs::Graph,
        math::Gun,
        plot::Options,
        snapshot::{Cycle, Ship, WeaponKind},
    };

    fn gun(optimal: f64, dps: f64) -> Weapon {
        Weapon {
            kind: WeaponKind::Turret(Gun {
                optimal,
                falloff: 1000.0,
                tracking: 1.0,
                optimal_sig: 40_000.0,
            }),
            dps: [dps, 0.0, 0.0, 0.0],
            volley: [dps, 0.0, 0.0, 0.0],
            cycle: Cycle {
                duration: 1.0,
                shots: None,
                reload: 0.0,
                amounts: vec![[dps, 0.0, 0.0, 0.0]],
                delay: 0.0,
            },
        }
    }

    #[test]
    fn short_range_ammo_wins_close_and_long_range_far() {
        let graph = Graph::ApplicationProfile;
        let req = Request {
            graph,
            x: graph.x_defs(true)[0],
            y: graph.y_defs(true, true)[0],
            range: (0.0, 40.0),
            misc: HashMap::from([(Param::TgtSpeed, Some(0.0))]),
            checkboxes: HashMap::new(),
            options: Options {
                ignore_lock_range: true,
                ..Options::default()
            },
        };
        let src = FitSnapshot {
            ship: Ship {
                lock_range: 100_000.0,
                ..Ship::default()
            },
            ..FitSnapshot::default()
        };
        let option = |name: &str, optimal, dps| AmmoOption {
            charge: 0,
            name: name.to_owned(),
            weapons: vec![gun(optimal, dps)],
        };
        let groups = [WeaponGroup {
            module: 0,
            options: vec![
                option("Close", 5_000.0, 100.0),
                option("Far", 30_000.0, 50.0),
            ],
        }];
        let tgt = TargetData::profile(0.0, f64::INFINITY, 0.0, [0.0; 4], None);
        let segs = segments(&req, &src, &groups, &tgt);
        let names: Vec<&str> = segs.iter().map(|s| s.ammo.as_str()).collect();
        assert_eq!(names, ["Close", "Far"]);
        // The line is continuous across the switch.
        assert_eq!(segs[0].xs.last(), segs[1].xs.first());
    }
}
