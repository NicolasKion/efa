//! pyfa's builtin damage patterns (eos/saveddata/damagePattern.py), in its order and naming.
//!
//! Names keep pyfa's raw form: each `[Category]` prefix is a submenu level, and a `|` stops the
//! category parsing so names like `[T2] Aurora` keep their brackets.

/// Raw name and EM, thermal, kinetic, explosive amounts.
pub const BUILTIN: &[(&str, [f64; 4])] = &[
    ("Uniform", [25.0, 25.0, 25.0, 25.0]),
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

/// Categories and short name of a raw pattern name, like pyfa's `__parseRawName`.
pub fn parse(raw: &str) -> (Vec<&str>, &str) {
    let mut categories = Vec::new();
    let mut rest = raw.trim();
    loop {
        let (Some(start), Some(end)) = (rest.find('['), rest.find(']')) else {
            return (categories, rest);
        };
        if let Some(splitter) = rest.find('|')
            && splitter + 1 == start
        {
            return (categories, &rest[1..]);
        }
        categories.push(&rest[start + 1..end]);
        rest = rest[end + 1..].trim();
    }
}

/// A pattern as a menu shows it: short label, the raw name stored on the fit, the amounts.
pub type Entry = (&'static str, &'static str, [f64; 4]);

/// One menu level: named sublevels, then the patterns directly in it, both in pyfa's order.
#[derive(Debug, Default)]
pub struct Tree {
    pub children: Vec<(&'static str, Tree)>,
    pub patterns: Vec<Entry>,
}

impl Tree {
    /// The builtin patterns nested as pyfa's damage pattern menu shows them.
    pub fn builtin() -> Self {
        let mut root = Tree::default();
        for &(raw, amounts) in BUILTIN {
            let (categories, short) = parse(raw);
            let mut level = &mut root;
            for category in categories {
                let i = if let Some(i) = level.children.iter().position(|(n, _)| *n == category) {
                    i
                } else {
                    level.children.push((category, Tree::default()));
                    level.children.len() - 1
                };
                level = &mut level.children[i].1;
            }
            level.patterns.push((short, raw, amounts));
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pyfa_names() {
        assert_eq!(parse("Uniform"), (vec![], "Uniform"));
        assert_eq!(parse("[Generic]EM"), (vec!["Generic"], "EM"));
        assert_eq!(
            parse("[Frequency Crystals]|[T2] Aurora"),
            (vec!["Frequency Crystals"], "[T2] Aurora")
        );
        assert_eq!(
            parse("[NPC][Burner][Team]Enyo"),
            (vec!["NPC", "Burner", "Team"], "Enyo")
        );
    }

    #[test]
    fn nests_like_the_pyfa_menu() {
        let tree = Tree::builtin();
        assert_eq!(tree.patterns.len(), 1, "only Uniform sits at the top");
        let names: Vec<&str> = tree.children.iter().map(|(n, _)| *n).collect();
        assert_eq!(names.first(), Some(&"Generic"));
        assert_eq!(names.last(), Some(&"NPC"));
    }
}
