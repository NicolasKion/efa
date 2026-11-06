//! What the graphs need to know about a fit, read out of refine once per fit change.
//!
//! The engine fills this in ([`crate::engine::Engine::graph_snapshot`]); the graphs then sample
//! it hundreds of times without touching refine again.

use crate::graphs::math::{Gun, MissileRange};

/// EM, thermal, kinetic, explosive.
pub type Dmg = [f64; 4];

pub fn dmg_total(d: &Dmg) -> f64 {
    d.iter().sum()
}

/// Shield, armor, hull.
pub type Layers = [f64; 3];

#[derive(Debug, Clone, Default)]
pub struct FitSnapshot {
    pub ship: Ship,
    pub weapons: Vec<Weapon>,
    pub ewar: Vec<Ewar>,
    pub reps: Vec<RemoteRep>,
    pub cap_users: Vec<CapUser>,
}

/// Ship numbers after every bonus, in base units (m, m/s, s, kg, GJ, HP).
#[derive(Debug, Clone, Default)]
pub struct Ship {
    pub radius: f64,
    pub max_velocity: f64,
    pub sig_radius: f64,
    pub mass: f64,
    pub agility: f64,
    pub align_time: f64,
    /// AU per second.
    pub warp_speed: f64,
    /// In AU.
    pub max_warp_distance: f64,
    /// Top speed with the modules that cannot run in warp switched off, where warps start from.
    pub subwarp_speed: f64,
    pub scan_res: f64,
    pub lock_range: f64,
    pub drone_control_range: f64,
    pub cap_capacity: f64,
    /// Seconds for a full recharge.
    pub cap_recharge: f64,
    pub shield_capacity: f64,
    pub shield_recharge: f64,
    /// Shield EHP per HP under the fit's own damage profile, for "effective" shield graphs.
    pub shield_ehp_mult: f64,
    /// Fractions resisted per layer: EM, thermal, kinetic, explosive.
    pub resists: [Dmg; 3],
    pub hp: Layers,
    /// EHP per layer against uniform damage, for choosing which layer a target tanks with.
    pub ehp_uniform: Layers,
    /// Repaired per second with every repairer cycling, per layer.
    pub tank: Layers,
    /// Peak passive shield regeneration per second.
    pub shield_regen: f64,
    pub dps: f64,
    pub volley: f64,
    /// Remote repairs per second: shield, armor, hull.
    pub rr: Layers,
    /// Seconds the capacitor lasts, None when stable.
    pub cap_lasts: Option<f64>,
    /// Stable capacitor level as a fraction, when stable.
    pub cap_stable: Option<f64>,
}

/// One cycling item that does damage: a module, or one drone stack.
#[derive(Debug, Clone)]
pub struct Weapon {
    pub kind: WeaponKind,
    /// Damage per second as the stats pane shows it (with the module's spool setting).
    pub dps: Dmg,
    pub volley: Dmg,
    pub cycle: Cycle<Dmg>,
}

/// How a weapon's damage lands, with the numbers its application math needs.
#[derive(Debug, Clone, PartialEq)]
pub enum WeaponKind {
    Turret(Gun),
    /// Vorton projectors: missile-like application over a hard range.
    Vorton {
        range: f64,
        er: f64,
        ev: f64,
        drf: f64,
    },
    Launcher {
        er: f64,
        ev: f64,
        drf: f64,
        range: Option<MissileRange>,
        /// Friend-or-foe missiles fly past lock range.
        fof: bool,
    },
    Smartbomb {
        range: f64,
    },
    Bomb {
        range: f64,
        blast_radius: f64,
        er: f64,
    },
    GuidedBomb {
        range: f64,
        er: f64,
    },
    Doomsday {
        range: f64,
        damage_sig: f64,
        /// Single-target doomsdays need a lock.
        needs_lock: bool,
    },
    Drone {
        gun: Gun,
        speed: f64,
        radius: f64,
    },
}

/// When a cycling item fires and what each shot carries, for graphs over time.
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle<T> {
    /// Seconds per cycle.
    pub duration: f64,
    /// Cycles before a reload; None never reloads.
    pub shots: Option<u32>,
    /// Seconds a reload takes.
    pub reload: f64,
    /// What each cycle delivers, by consecutive cycle; the last repeats (spool-up).
    pub amounts: Vec<T>,
    /// Seconds into a cycle the amount lands: 0 for damage and shield reps, the full cycle for
    /// armor and hull reps.
    pub delay: f64,
}

/// One ewar effect a fit projects.
#[derive(Debug, Clone, PartialEq)]
pub struct Ewar {
    pub kind: EwarKind,
    /// Strength in the effect's own unit: GJ/s for neuts, percent for the rest, ECM points.
    pub strength: f64,
    /// Guidance disruptors change two things; this is the flight time part.
    pub strength2: f64,
    pub optimal: f64,
    pub falloff: f64,
    pub needs_lock: bool,
    /// Drones only work inside drone control range.
    pub needs_dcr: bool,
    /// Drones carry their effect to the target: speed and radius, for "apply projected".
    pub mobile: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EwarKind {
    Neut,
    Web,
    Ecm,
    Damp,
    TrackingDisruptor,
    GuidanceDisruptor,
    TargetPainter,
}

/// A remote repairer, or a stack of repair drones.
#[derive(Debug, Clone)]
pub struct RemoteRep {
    pub cycle: Cycle<Layers>,
    /// Ancillary repairers can be told not to reload; this is the cycle they get then.
    pub no_reload: Option<Cycle<Layers>>,
    pub optimal: f64,
    pub falloff: f64,
    pub drone: bool,
}

/// A module that takes from or gives to the capacitor, for the capacitor simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct CapUser {
    /// Milliseconds between activations, reactivation delay included.
    pub cycle_ms: f64,
    /// Negative for cap boosters.
    pub need: f64,
    /// Activations before a reload, 0 for none.
    pub clip: u32,
    pub reload_ms: f64,
    /// Turrets fire together rather than staggered.
    pub no_stagger: bool,
    pub injector: bool,
}

impl<T: Clone + Default> Cycle<T> {
    /// What the cycle starting after `nonstop` uninterrupted cycles delivers.
    pub fn amount(&self, nonstop: usize) -> T {
        self.amounts
            .get(nonstop)
            .or_else(|| self.amounts.last())
            .cloned()
            .unwrap_or_default()
    }

    /// Every cycle until `until` seconds: (start, end, amount). A reload breaks spool-up.
    pub fn cycles(&self, until: f64) -> Vec<(f64, f64, T)> {
        let mut out = Vec::new();
        if self.duration <= 0.0 {
            return out;
        }
        let (mut time, mut nonstop, mut fired) = (0.0, 0, 0);
        loop {
            out.push((time, time + self.duration, self.amount(nonstop)));
            fired += 1;
            nonstop += 1;
            time += self.duration;
            if self
                .shots
                .is_some_and(|shots| shots > 0 && fired % shots == 0)
            {
                time += self.reload;
                nonstop = 0;
            }
            if time > until {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_reload_and_restart_spool() {
        let cycle = Cycle {
            duration: 2.0,
            shots: Some(2),
            reload: 10.0,
            amounts: vec![1.0, 2.0, 3.0],
            delay: 0.0,
        };
        let starts: Vec<(f64, f64)> = cycle.cycles(15.0).iter().map(|c| (c.0, c.2)).collect();
        assert_eq!(starts, vec![(0.0, 1.0), (2.0, 2.0), (14.0, 1.0)]);
    }
}
