//! Damage patterns: pyfa's builtin list (eos/saveddata/damagePattern.py), the user's own, and
//! pyfa's clipboard format for them.

use std::{fmt::Write, sync::LazyLock};

use serde::{Deserialize, Serialize};

use crate::{
    fit::{DamageProfile, Fit},
    profile::{self, Imported, Kind, Tree, lines},
};

/// A named EM, thermal, kinetic, explosive mix; only the ratios matter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DamagePattern {
    /// pyfa's raw name, with `[Category]` prefixes.
    pub name: String,
    pub amounts: [f64; 4],
}

impl DamagePattern {
    pub fn new(name: impl Into<String>, amounts: [f64; 4]) -> Self {
        Self {
            name: name.into(),
            amounts,
        }
    }

    /// The pattern as a fit stores it.
    pub fn to_profile(&self) -> DamageProfile {
        DamageProfile::Pattern {
            name: self.name.clone(),
            amounts: self.amounts,
        }
    }
}

/// The name "Set {X} as Damage Pattern" gives the pattern it makes, as pyfa does.
pub const SELECTED_AMMO: &str = "Selected Ammo";

/// Uniform damage, pyfa's default pattern.
pub const UNIFORM: [f64; 4] = [25.0; 4];

/// Names pyfa keeps out of the editor and the export: they are set from the fit, not edited.
const NOT_EDITABLE: [&str; 2] = ["Uniform", SELECTED_AMMO];

/// Whether the editor lists this user pattern.
pub fn listed(pattern: &DamagePattern) -> bool {
    pattern.name != SELECTED_AMMO
}

/// Whether the editor lets this user pattern change; pyfa locks one named Uniform.
pub fn editable(pattern: &DamagePattern) -> bool {
    !NOT_EDITABLE.contains(&pattern.name.as_str())
}

/// Raw name and EM, thermal, kinetic, explosive amounts, in pyfa's order.
const BUILTIN: &[(&str, [f64; 4])] = &[
    ("Uniform", UNIFORM),
    ("[Generic]EM", [1.0, 0.0, 0.0, 0.0]),
    ("[Generic]Thermal", [0.0, 1.0, 0.0, 0.0]),
    ("[Generic]Kinetic", [0.0, 0.0, 1.0, 0.0]),
    ("[Generic]Explosive", [0.0, 0.0, 0.0, 1.0]),
    ("[Frequency Crystals]|[T2] Aurora", [5.0, 3.0, 0.0, 0.0]),
    ("[Frequency Crystals]|[T2] Scorch", [9.0, 2.0, 0.0, 0.0]),
    ("[Frequency Crystals]Radio", [5.0, 0.0, 0.0, 0.0]),
    ("[Frequency Crystals]Microwave", [4.0, 2.0, 0.0, 0.0]),
    ("[Frequency Crystals]Infrared", [5.0, 2.0, 0.0, 0.0]),
    ("[Frequency Crystals]Standard", [5.0, 3.0, 0.0, 0.0]),
    ("[Frequency Crystals]Ultraviolet", [6.0, 3.0, 0.0, 0.0]),
    ("[Frequency Crystals]Xray", [6.0, 4.0, 0.0, 0.0]),
    ("[Frequency Crystals]Gamma", [7.0, 4.0, 0.0, 0.0]),
    ("[Frequency Crystals]Multifrequency", [7.0, 5.0, 0.0, 0.0]),
    ("[Frequency Crystals]|[T2] Gleam", [7.0, 7.0, 0.0, 0.0]),
    (
        "[Frequency Crystals]|[T2] Conflagration",
        [7.7, 7.7, 0.0, 0.0],
    ),
    ("[Exotic Plasma]|[T2] Mystic", [0.0, 66319.0, 0.0, 33681.0]),
    ("[Exotic Plasma]Meson", [0.0, 60519.0, 0.0, 39481.0]),
    ("[Exotic Plasma]Baryon", [0.0, 59737.0, 0.0, 40263.0]),
    ("[Exotic Plasma]Tetryon", [0.0, 69208.0, 0.0, 30792.0]),
    ("[Exotic Plasma]|[T2] Occult", [0.0, 55863.0, 0.0, 44137.0]),
    (
        "[Condenser Packs]|[T2] StrikeSnipe",
        [51817.0, 0.0, 48183.0, 0.0],
    ),
    ("[Condenser Packs]MesmerFlux", [76476.0, 0.0, 23524.0, 0.0]),
    ("[Condenser Packs]SlamBolt", [23376.0, 0.0, 76624.0, 0.0]),
    ("[Condenser Packs]BlastShot", [19820.0, 0.0, 80180.0, 0.0]),
    ("[Condenser Packs]GalvaSurge", [80206.0, 0.0, 19794.0, 0.0]),
    (
        "[Condenser Packs]|[T2] ElectroPunch",
        [50547.0, 0.0, 49453.0, 0.0],
    ),
    ("[Hybrid Charges]|[T2] Spike", [0.0, 4.0, 4.0, 0.0]),
    ("[Hybrid Charges]|[T2] Null", [0.0, 6.0, 5.0, 0.0]),
    ("[Hybrid Charges]Iron", [0.0, 2.0, 3.0, 0.0]),
    ("[Hybrid Charges]Tungsten", [0.0, 2.0, 4.0, 0.0]),
    ("[Hybrid Charges]Iridium", [0.0, 3.0, 4.0, 0.0]),
    ("[Hybrid Charges]Lead", [0.0, 3.0, 5.0, 0.0]),
    ("[Hybrid Charges]Thorium", [0.0, 4.0, 5.0, 0.0]),
    ("[Hybrid Charges]Uranium", [0.0, 4.0, 6.0, 0.0]),
    ("[Hybrid Charges]Plutonium", [0.0, 5.0, 6.0, 0.0]),
    ("[Hybrid Charges]Antimatter", [0.0, 5.0, 7.0, 0.0]),
    ("[Hybrid Charges]|[T2] Javelin", [0.0, 8.0, 6.0, 0.0]),
    ("[Hybrid Charges]|[T2] Void", [0.0, 7.7, 7.7, 0.0]),
    ("[Projectile Ammo]|[T2] Tremor", [0.0, 0.0, 3.0, 5.0]),
    ("[Projectile Ammo]|[T2] Barrage", [0.0, 0.0, 5.0, 6.0]),
    ("[Projectile Ammo]Carbonized Lead", [0.0, 0.0, 4.0, 1.0]),
    ("[Projectile Ammo]Nuclear", [0.0, 0.0, 1.0, 4.0]),
    ("[Projectile Ammo]Proton", [3.0, 0.0, 2.0, 0.0]),
    ("[Projectile Ammo]Depleted Uranium", [0.0, 3.0, 2.0, 3.0]),
    ("[Projectile Ammo]Titanium Sabot", [0.0, 0.0, 6.0, 2.0]),
    ("[Projectile Ammo]EMP", [9.0, 0.0, 1.0, 2.0]),
    ("[Projectile Ammo]Phased Plasma", [0.0, 10.0, 2.0, 0.0]),
    ("[Projectile Ammo]Fusion", [0.0, 0.0, 2.0, 10.0]),
    ("[Projectile Ammo]|[T2] Quake", [0.0, 0.0, 5.0, 9.0]),
    ("[Projectile Ammo]|[T2] Hail", [0.0, 0.0, 3.3, 12.1]),
    ("[Missiles]Mjolnir", [1.0, 0.0, 0.0, 0.0]),
    ("[Missiles]Inferno", [0.0, 1.0, 0.0, 0.0]),
    ("[Missiles]Scourge", [0.0, 0.0, 1.0, 0.0]),
    ("[Missiles]Nova", [0.0, 0.0, 0.0, 1.0]),
    ("[Bombs]Electron Bomb", [6400.0, 0.0, 0.0, 0.0]),
    ("[Bombs]Scorch Bomb", [0.0, 6400.0, 0.0, 0.0]),
    ("[Bombs]Concussion Bomb", [0.0, 0.0, 6400.0, 0.0]),
    ("[Bombs]Shrapnel Bomb", [0.0, 0.0, 0.0, 6400.0]),
    ("[NPC][Abyssal]All", [126.0, 427.0, 218.0, 230.0]),
    ("[NPC][Abyssal]Angel", [450.0, 72.0, 80.0, 398.0]),
    ("[NPC][Abyssal]Concord", [53.0, 559.0, 94.0, 295.0]),
    ("[NPC][Abyssal]Drifter", [250.0, 250.0, 250.0, 250.0]),
    ("[NPC][Abyssal]Drones", [250.0, 250.0, 250.0, 250.0]),
    ("[NPC][Abyssal]Overmind", [0.0, 410.0, 590.0, 0.0]),
    ("[NPC][Abyssal]Sansha", [569.0, 431.0, 0.0, 0.0]),
    ("[NPC][Abyssal]Seeker", [402.0, 402.0, 98.0, 98.0]),
    ("[NPC][Abyssal]Sleeper", [313.0, 313.0, 187.0, 187.0]),
    ("[NPC][Abyssal]Triglavian", [0.0, 615.0, 0.0, 385.0]),
    (
        "[NPC][Asteroid]Angel Cartel",
        [1838.0, 562.0, 2215.0, 3838.0],
    ),
    ("[NPC][Asteroid]Blood Raiders", [5067.0, 4214.0, 0.0, 0.0]),
    ("[NPC][Asteroid]Guristas", [0.0, 1828.0, 7413.0, 0.0]),
    ("[NPC][Asteroid]Rogue Drone", [394.0, 666.0, 1090.0, 1687.0]),
    ("[NPC][Asteroid]Sanshas Nation", [5586.0, 4112.0, 0.0, 0.0]),
    ("[NPC][Asteroid]Serpentis", [0.0, 5373.0, 4813.0, 0.0]),
    ("[NPC][Burner][Team]Enyo", [0.0, 147.0, 147.0, 0.0]),
    ("[NPC][Burner][Team]Hawk", [0.0, 0.0, 247.0, 0.0]),
    ("[NPC][Burner][Team]Jaguar", [36.0, 0.0, 50.0, 182.0]),
    ("[NPC][Burner][Team]Vengeance", [232.0, 0.0, 0.0, 0.0]),
    ("[NPC][Burner]Cruor", [90.0, 90.0, 0.0, 0.0]),
    ("[NPC][Burner]Dramiel", [55.0, 0.0, 20.0, 96.0]),
    ("[NPC][Burner]Daredevil", [0.0, 110.0, 154.0, 0.0]),
    ("[NPC][Burner]Succubus", [135.0, 30.0, 0.0, 0.0]),
    ("[NPC][Burner]Worm", [0.0, 0.0, 228.0, 0.0]),
    ("[NPC][Burner]Ashimmu", [260.0, 100.0, 0.0, 0.0]),
    ("[NPC][Burner]Talos", [0.0, 413.0, 413.0, 0.0]),
    ("[NPC][Burner]Sentinel", [0.0, 0.0, 75.0, 90.0]),
    (
        "[NPC][Deadspace]Angel Cartel",
        [369.0, 533.0, 1395.0, 3302.0],
    ),
    (
        "[NPC][Deadspace]Blood Raiders",
        [6040.0, 5052.0, 10.0, 15.0],
    ),
    ("[NPC][Deadspace]Guristas", [0.0, 1531.0, 9680.0, 0.0]),
    (
        "[NPC][Deadspace]Rogue Drone",
        [276.0, 1071.0, 1069.0, 871.0],
    ),
    ("[NPC][Deadspace]Sanshas Nation", [3009.0, 2237.0, 0.0, 0.0]),
    ("[NPC][Deadspace]Serpentis", [0.0, 3110.0, 1929.0, 0.0]),
    (
        "[NPC][Invasion][Triglavian Entities]Dread",
        [0.0, 417.0, 0.0, 583.0],
    ),
    (
        "[NPC][Invasion][Triglavian Entities]Normal Subcaps",
        [0.0, 610.0, 0.0, 390.0],
    ),
    (
        "[NPC][Invasion][Triglavian Entities]Subcaps w/missiles 0% spool up",
        [367.0, 155.0, 367.0, 112.0],
    ),
    (
        "[NPC][Invasion][Triglavian Entities]Subcaps w/missiles 50% spool up",
        [291.0, 243.0, 291.0, 175.0],
    ),
    (
        "[NPC][Invasion][Triglavian Entities]Subcaps w/missiles 100% spool up",
        [241.0, 301.0, 241.0, 217.0],
    ),
    (
        "[NPC][Invasion][Amarr EDENCOM Entities]Dread/Subcaps",
        [583.0, 417.0, 0.0, 0.0],
    ),
    (
        "[NPC][Invasion][Caldari EDENCOM Entities]Dread",
        [1000.0, 0.0, 0.0, 0.0],
    ),
    (
        "[NPC][Invasion][Caldari EDENCOM Entities]Subcaps",
        [511.0, 21.0, 29.0, 440.0],
    ),
    (
        "[NPC][Invasion][Gallente EDENCOM Entities]Dread/Subcaps",
        [0.0, 417.0, 583.0, 0.0],
    ),
    (
        "[NPC][Invasion][Minmatar EDENCOM Entities]Dread",
        [0.0, 0.0, 583.0, 417.0],
    ),
    (
        "[NPC][Invasion][Minmatar EDENCOM Entities]Subcaps",
        [302.0, 136.0, 328.0, 234.0],
    ),
    (
        "[NPC][Invasion]Drifter Entities",
        [250.0, 250.0, 250.0, 250.0],
    ),
    (
        "[NPC][Invasion]Sleeper Entities",
        [265.0, 265.0, 235.0, 235.0],
    ),
    (
        "[NPC][Invasion]Rogue Drone Entities",
        [250.0, 250.0, 250.0, 250.0],
    ),
    ("[NPC][Mission]Amarr Empire", [4464.0, 3546.0, 97.0, 0.0]),
    ("[NPC][Mission]Caldari State", [0.0, 2139.0, 4867.0, 0.0]),
    ("[NPC][Mission]CONCORD", [336.0, 134.0, 212.0, 412.0]),
    (
        "[NPC][Mission]Gallente Federation",
        [9.0, 3712.0, 2758.0, 0.0],
    ),
    ("[NPC][Mission]Khanid", [612.0, 483.0, 43.0, 6.0]),
    (
        "[NPC][Mission]Minmatar Republic",
        [1024.0, 388.0, 1655.0, 4285.0],
    ),
    ("[NPC][Mission]Mordus Legion", [25.0, 262.0, 625.0, 0.0]),
    ("[NPC][Mission]Thukker", [0.0, 52.0, 10.0, 79.0]),
    ("[NPC]Sansha Incursion", [1682.0, 1347.0, 3678.0, 3678.0]),
    ("[NPC]Sleepers", [1472.0, 1472.0, 1384.0, 1384.0]),
];

/// The builtin patterns, in pyfa's order.
pub static BUILTINS: LazyLock<Vec<DamagePattern>> = LazyLock::new(|| {
    BUILTIN
        .iter()
        .map(|&(name, amounts)| DamagePattern::new(name, amounts))
        .collect()
});

/// Every pattern in pyfa's damage pattern menu order: builtins as listed, then the user's in
/// natural order, with Uniform and Selected Ammo pulled to the front. A user pattern with a
/// builtin's name takes its place in the tree.
pub fn tree(user: &[DamagePattern]) -> Tree<DamagePattern> {
    let mut user: Vec<&DamagePattern> = user.iter().collect();
    user.sort_by(|a, b| {
        profile::smart_cmp(&profile::full_name(&a.name), &profile::full_name(&b.name))
    });
    let mut all: Vec<&DamagePattern> = BUILTINS.iter().chain(user).collect();
    // Stable, so everything else keeps its order.
    all.sort_by_key(|p| !NOT_EDITABLE.contains(&profile::full_name(&p.name).as_str()));
    let mut tree = Tree::default();
    for pattern in all {
        tree.insert(&pattern.name, pattern.clone());
    }
    tree
}

impl Kind for DamagePattern {
    const PLURAL: &'static str = "patterns";
}

/// Read pyfa's `DamageProfile = name,em,thermal,kinetic,explosive` lines. Amounts are whole
/// numbers, as pyfa's `int()` wants them.
pub fn import(text: &str) -> Imported<DamagePattern> {
    let mut imported = Imported::default();
    for (key, data) in lines(text) {
        if key != "DamageProfile" {
            continue;
        }
        imported.lines += 1;
        let mut fields = data.split(',');
        let name = fields.next().unwrap_or_default().trim();
        let amounts: Vec<f64> = fields
            .take(4)
            .filter_map(|v| v.trim().parse::<i64>().ok())
            .map(|v| v as f64)
            .collect();
        if let Ok(amounts) = <[f64; 4]>::try_from(amounts) {
            imported.items.push(DamagePattern::new(name, amounts));
        }
    }
    imported
}

/// Take imported patterns in: ones with a known name replace its amounts, the rest are added.
pub fn merge(user: &mut Vec<DamagePattern>, imported: Vec<DamagePattern>) {
    for pattern in imported {
        match user.iter_mut().find(|p| p.name == pattern.name) {
            Some(existing) => existing.amounts = pattern.amounts,
            None => user.push(pattern),
        }
    }
}

/// The user's patterns in pyfa's clipboard format, sorted by full name. Amounts are cut to whole
/// numbers like pyfa's `%d`.
pub fn export(user: &[DamagePattern]) -> String {
    let mut patterns: Vec<&DamagePattern> = user
        .iter()
        .filter(|p| !NOT_EDITABLE.contains(&p.name.as_str()))
        .collect();
    patterns.sort_by_key(|p| profile::full_name(&p.name));
    let mut out = String::from(
        "# Exported from pyfa\n#\n\
         # Values are in following format:\n\
         # DamageProfile = [name],[EM amount],[Thermal amount],[Kinetic amount],[Explosive amount]\n\n",
    );
    for p in patterns {
        let [em, thermal, kinetic, explosive] = p.amounts.map(|a| a.trunc() as i64);
        let _ = writeln!(
            out,
            "DamageProfile = {},{em},{thermal},{kinetic},{explosive}",
            p.name
        );
    }
    out.trim().to_owned()
}

/// Point fits that use the user pattern `old` at what it became: its new name and amounts, or
/// uniform damage once it is deleted. pyfa's fits hold the pattern itself, so they follow it.
pub fn retarget(fits: &mut [Fit], old: &str, new: Option<&DamagePattern>) {
    for fit in fits {
        if matches!(&fit.damage_profile, DamageProfile::Pattern { name, .. } if name == old) {
            fit.damage_profile = new.map_or(DamageProfile::Uniform, DamagePattern::to_profile);
        }
    }
}

/// The share each damage type has of a mix, for EHP: pyfa's `effectivify` divides HP by the
/// resonances weighted by these.
pub fn shares(amounts: [f64; 4]) -> [f64; 4] {
    let total: f64 = amounts.iter().sum();
    let total = if total == 0.0 { 1.0 } else { total };
    amounts.map(|a| a / total)
}

/// Effective HP of a layer under a pattern, pyfa's `effectivify`.
pub fn effective_hp(hp: f64, resists: [f64; 4], amounts: [f64; 4]) -> f64 {
    let divider: f64 = shares(amounts)
        .iter()
        .zip(resists)
        .map(|(share, resist)| share * (1.0 - resist))
        .sum();
    if divider == 0.0 { hp } else { hp / divider }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_puts_uniform_first_and_user_patterns_in_their_categories() {
        let user = [
            DamagePattern::new("Mine", [1.0, 0.0, 0.0, 0.0]),
            DamagePattern::new("[Generic]EM", [5.0, 0.0, 0.0, 0.0]),
            DamagePattern::new("[Mine]b10", [0.0; 4]),
            DamagePattern::new("[Mine]b9", [0.0; 4]),
        ];
        let tree = tree(&user);
        let top: Vec<&str> = tree.entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(top, ["Uniform", "Mine"]);
        let generic = &tree.children[0].1;
        let em = generic
            .entries
            .iter()
            .find(|(n, _)| n == "EM")
            .expect("EM is listed");
        assert_eq!(
            em.1.amounts,
            [5.0, 0.0, 0.0, 0.0],
            "the user's replaces the builtin"
        );
        let mine = &tree.children.last().expect("a user category").1;
        let names: Vec<&str> = mine.entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["b9", "b10"]);
    }

    #[test]
    fn import_reads_pyfa_exports_and_counts_errors() {
        let text = "# Exported from pyfa\n\nDamageProfile = A,1,2,3,4\n\
                    DamageProfile = B,1,2.5,3,4 # pyfa wants whole numbers\n\
                    TargetProfile = C,1,2,3,4\nDamageProfile=D,0,0,0,9";
        let imported = import(text);
        assert_eq!(
            imported.items,
            [
                DamagePattern::new("A", [1.0, 2.0, 3.0, 4.0]),
                DamagePattern::new("D", [0.0, 0.0, 0.0, 9.0]),
            ]
        );
        assert_eq!(imported.lines, 3);
        assert_eq!(
            imported.notice("ok"),
            "3 patterns imported from clipboard; 1 had errors"
        );
        assert_eq!(import("").notice("ok"), "No patterns found for import");
    }

    #[test]
    fn export_round_trips() {
        let user = vec![
            DamagePattern::new("[Cat]b", [1.9, 0.0, 3.0, 0.0]),
            DamagePattern::new("a", [25.0; 4]),
            DamagePattern::new(SELECTED_AMMO, [1.0; 4]),
        ];
        let text = export(&user);
        assert!(text.starts_with("# Exported from pyfa"));
        assert!(text.ends_with("DamageProfile = a,25,25,25,25"));
        let back = import(&text);
        assert_eq!(back.items.len(), 2, "Selected Ammo stays out");
        assert_eq!(
            back.items[0],
            DamagePattern::new("[Cat]b", [1.0, 0.0, 3.0, 0.0])
        );
        let mut merged = vec![DamagePattern::new("a", [0.0; 4])];
        merge(&mut merged, back.items);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].amounts, [25.0; 4]);
    }

    #[test]
    fn ehp_under_a_pattern_matches_pyfa() {
        // 1000 HP with 0/20/40/50 resists: uniform damage gets through 72.5% of the time.
        let resists = [0.0, 0.2, 0.4, 0.5];
        let uniform = effective_hp(1000.0, resists, [25.0; 4]);
        assert!((uniform - 1000.0 / 0.725).abs() < 1e-9);
        // Pure EM goes straight through.
        assert!((effective_hp(1000.0, resists, [1.0, 0.0, 0.0, 0.0]) - 1000.0).abs() < 1e-9);
        // Kinetic 3 : explosive 1 is 0.75 * 0.6 + 0.25 * 0.5 = 0.575.
        let mix = effective_hp(1000.0, resists, [0.0, 0.0, 3.0, 1.0]);
        assert!((mix - 1000.0 / 0.575).abs() < 1e-9);
        // An empty pattern counts like pyfa's `or 1` guards: no damage, so raw HP.
        assert!((effective_hp(1000.0, resists, [0.0; 4]) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn deleting_a_used_pattern_falls_back_to_uniform() {
        let mut fits = vec![Fit::new("a"), Fit::new("b")];
        fits[0].damage_profile = DamagePattern::new("Mine", [1.0; 4]).to_profile();
        let renamed = DamagePattern::new("Ours", [2.0; 4]);
        retarget(&mut fits, "Mine", Some(&renamed));
        assert_eq!(fits[0].damage_profile, renamed.to_profile());
        assert_eq!(fits[1].damage_profile, DamageProfile::Uniform);
        retarget(&mut fits, "Ours", None);
        assert_eq!(fits[0].damage_profile, DamageProfile::Uniform);
    }
}
