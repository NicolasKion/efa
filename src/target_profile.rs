//! Target profiles: what the fit shoots at. pyfa's builtin list (eos/saveddata/targetProfile.py),
//! the user's own, pyfa's clipboard format, and the damage that gets through a profile.

mod builtin;

use std::fmt::Write;

use serde::{Deserialize, Serialize};

pub use builtin::BUILTINS;

use crate::{
    engine::Damage,
    fit::Fit,
    profile::{self, Imported, Kind, Tree, lines},
};

/// Resists, and what the graphs use for application: speed, signature and radius.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetProfile {
    /// pyfa's raw name, with `[Category]` prefixes.
    pub name: String,
    /// EM, thermal, kinetic, explosive, as fractions resisted.
    pub resists: [f64; 4],
    /// m/s; none is standing still.
    pub max_velocity: Option<f64>,
    /// m; none is infinitely big, so everything applies in full.
    pub signature_radius: Option<f64>,
    /// m; none is a point.
    pub radius: Option<f64>,
    /// Total HP, which caps breacher pod damage; none is infinite.
    pub hp: Option<f64>,
}

impl TargetProfile {
    pub fn new(name: impl Into<String>, resists: [f64; 4]) -> Self {
        Self {
            name: name.into(),
            resists,
            ..Self::default()
        }
    }

    /// pyfa's "Ideal Target": no resists, standing still, infinitely big.
    pub fn ideal() -> Self {
        Self {
            max_velocity: Some(0.0),
            radius: Some(0.0),
            ..Self::new("Ideal Target", [0.0; 4])
        }
    }

    pub fn max_velocity(&self) -> f64 {
        self.max_velocity.unwrap_or(0.0)
    }

    pub fn signature_radius(&self) -> f64 {
        self.signature_radius.unwrap_or(f64::INFINITY)
    }

    pub fn radius(&self) -> f64 {
        self.radius.unwrap_or(0.0)
    }

    pub fn hp(&self) -> f64 {
        self.hp.unwrap_or(f64::INFINITY)
    }

    /// Damage that gets through the resists. This is all pyfa's firepower view does with a
    /// target profile ("Effective" DPS); speed and signature only matter to the graphs.
    pub fn apply(&self, damage: Damage) -> Damage {
        apply_resists(damage, self.resists)
    }

    /// Breacher pod damage against this target: the smaller of its flat cap and its share of
    /// the target's HP, as pyfa's `DmgTypes.pure`.
    pub fn breacher(&self, absolute: f64, relative: f64) -> f64 {
        absolute.min(relative * self.hp())
    }
}

/// Damage after resists, each type cut by its resist.
pub fn apply_resists(damage: Damage, resists: [f64; 4]) -> Damage {
    let [em, thermal, kinetic, explosive] = resists.map(|r| 1.0 - r);
    Damage {
        em: damage.em * em,
        thermal: damage.thermal * thermal,
        kinetic: damage.kinetic * kinetic,
        explosive: damage.explosive * explosive,
        breacher: damage.breacher,
    }
}

impl Kind for TargetProfile {
    const PLURAL: &'static str = "profiles";
}

/// Breacher pod damage against a fit's target profile, or its flat cap without one, pyfa's
/// `DmgTypes.pure`.
pub fn applied_breacher(damage: Damage, profile: Option<&TargetProfile>) -> f64 {
    let b = damage.breacher;
    profile.map_or(b.absolute, |p| p.breacher(b.absolute, b.relative))
}

/// Damage against a fit's target profile, or raw without one.
pub fn applied(damage: Damage, profile: Option<&TargetProfile>) -> Damage {
    profile.map_or(damage, |p| p.apply(damage))
}

/// Builtin and user profiles as pyfa's target profile menus nest them: all in natural order of
/// their full names, a user profile replacing a builtin of the same name.
pub fn tree(user: &[TargetProfile]) -> Tree<TargetProfile> {
    let mut all: Vec<&TargetProfile> = BUILTINS.iter().chain(user).collect();
    all.sort_by(|a, b| {
        profile::smart_cmp(&profile::full_name(&a.name), &profile::full_name(&b.name))
    });
    let mut tree = Tree::default();
    for p in all {
        tree.insert(&p.name, p.clone());
    }
    tree
}

/// Read pyfa's `TargetProfile = name,em%,thermal%,kinetic%,explosive%[,speed,sig,radius]` lines
/// (and the older `TargetResists`). A resist outside 0 to 100 makes the line an error.
pub fn import(text: &str) -> Imported<TargetProfile> {
    let mut imported = Imported::default();
    for (key, data) in lines(text) {
        if key != "TargetProfile" && key != "TargetResists" {
            continue;
        }
        imported.lines += 1;
        if let Some(profile) = parse_line(data) {
            imported.items.push(profile);
        }
    }
    imported
}

/// pyfa's `float(v) if v else 0`.
fn number(value: &str) -> Option<f64> {
    if value.is_empty() {
        Some(0.0)
    } else {
        value.parse().ok()
    }
}

fn parse_line(data: &str) -> Option<TargetProfile> {
    let fields: Vec<&str> = data.split(',').map(str::trim).collect();
    let name = fields.first()?;
    let resists: Vec<f64> = fields
        .iter()
        .skip(1)
        .take(4)
        .map(|v| {
            let v = number(v)?;
            let v = if v.is_infinite() { 0.0 } else { v };
            (0.0..=100.0).contains(&v).then_some(v / 100.0)
        })
        .collect::<Option<_>>()?;
    let mut profile = TargetProfile::new(*name, <[f64; 4]>::try_from(resists).ok()?);
    let misc = fields.get(5..8).unwrap_or_default();
    if misc.len() == 3 {
        let [speed, sig, radius] = [misc[0], misc[1], misc[2]].map(number);
        let finite = |v: f64| if v.is_infinite() { 0.0 } else { v };
        profile.max_velocity = Some(finite(speed?));
        // An infinite signature is how pyfa writes "none".
        profile.signature_radius = Some(sig?).filter(|v| !v.is_infinite());
        profile.radius = Some(finite(radius?));
    }
    Some(profile)
}

/// Take imported profiles in: ones with a known name are replaced but keep the name, the rest
/// are added.
pub fn merge(user: &mut Vec<TargetProfile>, imported: Vec<TargetProfile>) {
    for profile in imported {
        match user.iter_mut().find(|p| p.name == profile.name) {
            Some(existing) => *existing = profile,
            None => user.push(profile),
        }
    }
}

/// The user's profiles in pyfa's clipboard format, sorted by full name.
pub fn export(user: &[TargetProfile]) -> String {
    let mut profiles: Vec<&TargetProfile> = user.iter().collect();
    profiles.sort_by_key(|p| profile::full_name(&p.name));
    let mut out = String::from(
        "# Exported from pyfa\n#\n\
         # Values are in following format:\n\
         # TargetProfile = [name],[EM %],[Thermal %],[Kinetic %],[Explosive %],[Max velocity m/s],[Signature radius m],[Radius m]\n\n",
    );
    for p in profiles {
        let [em, thermal, kinetic, explosive] = p.resists.map(|r| r * 100.0);
        let _ = writeln!(
            out,
            "TargetProfile = {},{em:.1},{thermal:.1},{kinetic:.1},{explosive:.1},{:.1},{:.1},{:.1}",
            p.name,
            p.max_velocity(),
            p.signature_radius(),
            p.radius()
        );
    }
    out.trim().to_owned()
}

/// Point fits that use the user profile `old` at what it became, or at none once it is deleted.
pub fn retarget(fits: &mut [Fit], old: &str, new: Option<&TargetProfile>) {
    for fit in fits {
        if fit.target_profile.as_ref().is_some_and(|p| p.name == old) {
            fit.target_profile = new.cloned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_match_pyfa() {
        assert_eq!(BUILTINS.len(), 195);
        let enyo = BUILTINS
            .iter()
            .find(|p| p.name == "[NPC][Burner][Team]Enyo")
            .expect("Enyo is builtin");
        assert_eq!(enyo.resists, [0.575, 0.724, 0.862, 0.235]);
        assert_eq!(
            (enyo.max_velocity, enyo.signature_radius, enyo.radius),
            (Some(1020.0), Some(37.0), Some(39.0))
        );
        assert_eq!(BUILTINS[0].signature_radius(), f64::INFINITY);
    }

    #[test]
    fn applied_dps_against_a_profile() {
        let dps = Damage {
            em: 100.0,
            thermal: 100.0,
            kinetic: 0.0,
            explosive: 50.0,
            ..Damage::default()
        };
        // pyfa's T1 shield: 0/20/40/50.
        let shield = TargetProfile::new("Shield", [0.0, 0.2, 0.4, 0.5]);
        let applied = shield.apply(dps);
        assert!((applied.em - 100.0).abs() < 1e-9);
        assert!((applied.thermal - 80.0).abs() < 1e-9);
        assert!((applied.explosive - 25.0).abs() < 1e-9);
        assert!((applied.total() - 205.0).abs() < 1e-9);
        assert!((super::applied(dps, None).total() - 250.0).abs() < 1e-9);
        // Breacher pods take the smaller of their cap and their share of the HP.
        let small = TargetProfile {
            hp: Some(10_000.0),
            ..TargetProfile::default()
        };
        assert!((small.breacher(150.0, 0.01) - 100.0).abs() < 1e-9);
        assert!((TargetProfile::ideal().breacher(150.0, 0.01) - 150.0).abs() < 1e-9);
    }

    #[test]
    fn import_and_export_use_pyfa_format() {
        let text = "TargetProfile = A,10,20,30,40,1000,inf,50\n\
                    TargetResists = B,0,0,0,0\n\
                    TargetProfile = C,10,20,30,140\n\
                    TargetProfile = D,,,,";
        let imported = import(text);
        assert_eq!(imported.lines, 4);
        let names: Vec<&str> = imported.items.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "D"]);
        assert_eq!(
            imported.notice("ok"),
            "4 profiles imported from clipboard; 1 had errors"
        );
        assert_eq!(import("").notice("ok"), "No profiles found for import");
        let a = &imported.items[0];
        assert!((a.resists[3] - 0.4).abs() < 1e-9);
        assert_eq!(
            (a.max_velocity, a.signature_radius, a.radius),
            (Some(1000.0), None, Some(50.0))
        );
        let out = export(&imported.items);
        assert!(out.contains("TargetProfile = A,10.0,20.0,30.0,40.0,1000.0,inf,50.0"));
        assert!(out.ends_with("TargetProfile = D,0.0,0.0,0.0,0.0,0.0,inf,0.0"));
        let back = import(&out);
        assert_eq!(back.items.len(), 3);
        assert_eq!(back.items[0].signature_radius, None);
    }

    #[test]
    fn menu_nests_and_sorts_like_pyfa() {
        let user = [TargetProfile::new("[NPC][Other]Zed", [0.0; 4])];
        let tree = tree(&user);
        let top: Vec<&str> = tree.entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            top[..4],
            [
                "Uniform (25%)",
                "Uniform (50%)",
                "Uniform (75%)",
                "Uniform (90%)"
            ]
        );
        let categories: Vec<&str> = tree.children.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(categories, ["NPC", "T1 Resist", "T2 Resist"]);
        let other = &tree.children[0]
            .1
            .children
            .iter()
            .find(|(n, _)| n == "Other")
            .expect("Other is a category")
            .1;
        assert_eq!(other.entries.last().map(|(n, _)| n.as_str()), Some("Zed"));
    }
}
