//! Target profiles for the graphs: pyfa's builtin ones and its ideal target.
//!
//! A profile is an abstract target, a set of resists with optional speed, signature and
//! radius. Other sources of profiles (user-made ones) only need to produce a [`TargetProfile`].

/// An abstract target. Resists are fractions; a missing signature radius means infinitely large,
/// which every weapon hits fully.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetProfile {
    /// pyfa's raw name: `[Category]` prefixes are submenu levels.
    pub name: String,
    pub resists: [f64; 4],
    pub max_velocity: f64,
    pub sig_radius: Option<f64>,
    pub radius: f64,
    /// Total HP, when the profile knows it.
    pub hp: Option<f64>,
}

impl TargetProfile {
    /// pyfa's "Ideal Target": no resists, standing still, infinitely large.
    pub fn ideal() -> Self {
        Self {
            name: "Ideal Target".to_owned(),
            resists: [0.0; 4],
            max_velocity: 0.0,
            sig_radius: None,
            radius: 0.0,
            hp: None,
        }
    }

    /// The name without its category prefixes.
    pub fn short_name(&self) -> &str {
        let name = self.name.rsplit(']').next().unwrap_or(&self.name);
        name.trim_start_matches('|')
    }

    /// The category path, `[NPC][Burner]` as `["NPC", "Burner"]`.
    pub fn categories(&self) -> Vec<&str> {
        let mut rest = self.name.as_str();
        let mut out = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            out.push(&stripped[..end]);
            rest = &stripped[end + 1..];
        }
        out
    }

    /// Every builtin profile, in pyfa's order.
    pub fn builtins() -> impl Iterator<Item = Self> {
        BUILTIN.iter().map(|b| Self {
            name: b.name.to_owned(),
            resists: b.resists,
            max_velocity: b.speed.unwrap_or(0.0),
            sig_radius: b.sig,
            radius: b.radius.unwrap_or(0.0),
            hp: None,
        })
    }
}

/// A builtin profile as pyfa lists it (eos/saveddata/targetProfile.py).
pub struct Builtin {
    pub name: &'static str,
    pub resists: [f64; 4],
    pub speed: Option<f64>,
    pub sig: Option<f64>,
    pub radius: Option<f64>,
}

#[rustfmt::skip]
#[expect(clippy::approx_constant, reason = "resist values from pyfa that happen to look like 1/pi")]
pub const BUILTIN: &[Builtin] = &[
    Builtin { name: "Uniform (25%)", resists: [0.25, 0.25, 0.25, 0.25], speed: None, sig: None, radius: None },
    Builtin { name: "Uniform (50%)", resists: [0.5, 0.5, 0.5, 0.5], speed: None, sig: None, radius: None },
    Builtin { name: "Uniform (75%)", resists: [0.75, 0.75, 0.75, 0.75], speed: None, sig: None, radius: None },
    Builtin { name: "Uniform (90%)", resists: [0.9, 0.9, 0.9, 0.9], speed: None, sig: None, radius: None },
    Builtin { name: "[T1 Resist]Shield", resists: [0.0, 0.2, 0.4, 0.5], speed: None, sig: None, radius: None },
    Builtin { name: "[T1 Resist]Armor", resists: [0.5, 0.45, 0.25, 0.1], speed: None, sig: None, radius: None },
    Builtin { name: "[T1 Resist]Hull", resists: [0.33, 0.33, 0.33, 0.33], speed: None, sig: None, radius: None },
    Builtin { name: "[T1 Resist]Shield (+T2 DCU)", resists: [0.125, 0.3, 0.475, 0.562], speed: None, sig: None, radius: None },
    Builtin { name: "[T1 Resist]Armor (+T2 DCU)", resists: [0.575, 0.532, 0.363, 0.235], speed: None, sig: None, radius: None },
    Builtin { name: "[T1 Resist]Hull (+T2 DCU)", resists: [0.598, 0.598, 0.598, 0.598], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Amarr (Shield)", resists: [0.0, 0.2, 0.7, 0.875], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Amarr (Armor)", resists: [0.5, 0.35, 0.625, 0.8], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Caldari (Shield)", resists: [0.2, 0.84, 0.76, 0.6], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Caldari (Armor)", resists: [0.5, 0.8625, 0.625, 0.1], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Gallente (Shield)", resists: [0.0, 0.6, 0.85, 0.5], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Gallente (Armor)", resists: [0.5, 0.675, 0.8375, 0.1], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Minmatar (Shield)", resists: [0.75, 0.6, 0.4, 0.5], speed: None, sig: None, radius: None },
    Builtin { name: "[T2 Resist]Minmatar (Armor)", resists: [0.9, 0.675, 0.25, 0.1], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Asteroid]Angel Cartel", resists: [0.54, 0.42, 0.37, 0.32], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Asteroid]Blood Raiders", resists: [0.34, 0.39, 0.45, 0.52], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Asteroid]Guristas", resists: [0.55, 0.35, 0.3, 0.48], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Asteroid]Rogue Drones", resists: [0.35, 0.38, 0.44, 0.49], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Asteroid]Sanshas Nation", resists: [0.35, 0.4, 0.47, 0.53], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Asteroid]Serpentis", resists: [0.49, 0.38, 0.29, 0.51], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Deadspace]Angel Cartel", resists: [0.59, 0.48, 0.4, 0.32], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Deadspace]Blood Raiders", resists: [0.31, 0.39, 0.47, 0.56], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Deadspace]Guristas", resists: [0.57, 0.39, 0.31, 0.5], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Deadspace]Rogue Drones", resists: [0.42, 0.42, 0.47, 0.49], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Deadspace]Sanshas Nation", resists: [0.31, 0.39, 0.47, 0.56], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Deadspace]Serpentis", resists: [0.49, 0.38, 0.29, 0.56], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]Amarr Empire", resists: [0.34, 0.38, 0.42, 0.46], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]Caldari State", resists: [0.51, 0.38, 0.3, 0.51], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]CONCORD", resists: [0.47, 0.46, 0.47, 0.47], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]Gallente Federation", resists: [0.51, 0.38, 0.31, 0.52], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]Khanid", resists: [0.51, 0.42, 0.36, 0.4], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]Minmatar Republic", resists: [0.51, 0.46, 0.41, 0.35], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Mission]Mordus Legion", resists: [0.32, 0.48, 0.4, 0.62], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Other]Sleeper", resists: [0.61, 0.61, 0.61, 0.61], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Other]Sansha Incursion", resists: [0.65, 0.63, 0.64, 0.65], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Burner][Team]Enyo", resists: [0.575, 0.724, 0.862, 0.235], speed: Some(1020.0), sig: Some(37.0), radius: Some(39.0) },
    Builtin { name: "[NPC][Burner][Team]Navitas", resists: [0.681, 0.586, 0.522, 0.49], speed: Some(870.0), sig: Some(30.0), radius: Some(35.0) },
    Builtin { name: "[NPC][Burner][Team]Hawk", resists: [0.3, 0.86, 0.79, 0.65], speed: Some(1122.0), sig: Some(48.0), radius: Some(39.0) },
    Builtin { name: "[NPC][Burner][Team]Bantam", resists: [0.344, 0.475, 0.606, 0.672], speed: Some(1016.0), sig: Some(45.0), radius: Some(27.0) },
    Builtin { name: "[NPC][Burner][Team]Jaguar", resists: [0.781, 0.65, 0.475, 0.563], speed: Some(1400.0), sig: Some(42.0), radius: Some(31.0) },
    Builtin { name: "[NPC][Burner][Team]Burst", resists: [0.344, 0.475, 0.606, 0.672], speed: Some(1174.0), sig: Some(39.0), radius: Some(31.0) },
    Builtin { name: "[NPC][Burner][Team]Vengeance", resists: [0.66, 0.558, 0.745, 0.864], speed: Some(1050.0), sig: Some(37.0), radius: Some(40.0) },
    Builtin { name: "[NPC][Burner][Team]Inquisitor", resists: [0.681, 0.586, 0.522, 0.49], speed: Some(920.0), sig: Some(29.0), radius: Some(20.5) },
    Builtin { name: "[NPC][Burner]Cruor", resists: [0.795, 0.734, 0.693, 0.672], speed: Some(900.0), sig: Some(18.0), radius: Some(20.5) },
    Builtin { name: "[NPC][Burner]Dramiel", resists: [0.351, 0.481, 0.611, 0.676], speed: Some(2100.0), sig: Some(11.0), radius: Some(25.0) },
    Builtin { name: "[NPC][Burner]Daredevil", resists: [0.685, 0.59, 0.59, 0.433], speed: Some(1200.0), sig: Some(18.0), radius: Some(25.0) },
    Builtin { name: "[NPC][Burner]Succubus", resists: [0.351, 0.481, 0.611, 0.676], speed: Some(4750.0), sig: Some(30.0), radius: Some(59.0) },
    Builtin { name: "[NPC][Burner]Worm", resists: [0.475, 0.58, 0.685, 0.738], speed: Some(360.0), sig: Some(70.0), radius: Some(39.0) },
    Builtin { name: "[NPC][Burner]Ashimmu", resists: [0.8, 0.76, 0.68, 0.7], speed: Some(500.0), sig: Some(120.0), radius: Some(137.0) },
    Builtin { name: "[NPC][Burner]Sentinel", resists: [0.575, 0.448, 0.522, 0.66], speed: Some(500.0), sig: Some(50.0), radius: Some(39.0) },
    Builtin { name: "[NPC][Burner]Talos", resists: [0.681, 0.586, 0.586, 0.426], speed: Some(150.0), sig: Some(125.0), radius: Some(266.0) },
    Builtin { name: "[NPC][Burner]Dragonfly", resists: [0.35, 0.72, 0.7, 0.55], speed: Some(1200.0), sig: Some(15.0), radius: Some(35.0) },
    Builtin { name: "[NPC][Burner]Mantis", resists: [0.6, 0.52, 0.71, 0.71], speed: Some(900.0), sig: Some(25.0), radius: Some(35.0) },
    Builtin { name: "[NPC][Invasion]Triglavian Entities", resists: [0.422, 0.367, 0.453, 0.411], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Amarr EDENCOM Entities", resists: [0.36, 0.31, 0.441, 0.602], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Caldari EDENCOM Entities", resists: [0.303, 0.61, 0.487, 0.401], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Gallente EDENCOM Entities", resists: [0.383, 0.414, 0.578, 0.513], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Minmatar EDENCOM Entities", resists: [0.62, 0.422, 0.355, 0.399], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Drones", resists: [0.439, 0.522, 0.529, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Overmind", resists: [0.643, 0.593, 0.624, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Seeker", resists: [0.082, 0.082, 0.082, 0.082], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Triglavian", resists: [0.494, 0.41, 0.464, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Drifter", resists: [0.415, 0.415, 0.415, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Sleeper", resists: [0.435, 0.435, 0.435, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]All", resists: [0.508, 0.474, 0.495, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Drones", resists: [0.323, 0.522, 0.529, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Overmind", resists: [0.542, 0.593, 0.624, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Seeker", resists: [0.0, 0.082, 0.082, 0.082], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Triglavian", resists: [0.356, 0.41, 0.464, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Drifter", resists: [0.277, 0.415, 0.415, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Sleeper", resists: [0.329, 0.435, 0.435, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]All", resists: [0.381, 0.474, 0.495, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Drones", resists: [0.255, 0.522, 0.529, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Overmind", resists: [0.48, 0.593, 0.624, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Seeker", resists: [0.0, 0.082, 0.082, 0.0822], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Triglavian", resists: [0.268, 0.41, 0.464, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Drifter", resists: [0.191, 0.415, 0.415, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Sleeper", resists: [0.268, 0.435, 0.435, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]All", resists: [0.308, 0.474, 0.495, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Drones", resists: [0.193, 0.522, 0.529, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Overmind", resists: [0.423, 0.593, 0.624, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Seeker", resists: [0.0, 0.082, 0.082, 0.082], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Triglavian", resists: [0.206, 0.41, 0.464, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Drifter", resists: [0.111, 0.415, 0.415, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Sleeper", resists: [0.215, 0.435, 0.435, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]All", resists: [0.247, 0.474, 0.495, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Drones", resists: [0.461, 0.425, 0.541, 0.443], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Overmind", resists: [0.666, 0.489, 0.634, 0.646], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Seeker", resists: [0.084, 0.0, 0.084, 0.084], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Triglavian", resists: [0.537, 0.269, 0.489, 0.371], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Drifter", resists: [0.43, 0.289, 0.43, 0.43], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Sleeper", resists: [0.512, 0.402, 0.512, 0.512], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]All", resists: [0.537, 0.352, 0.512, 0.495], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Drones", resists: [0.461, 0.36, 0.541, 0.443], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Overmind", resists: [0.666, 0.413, 0.634, 0.646], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Seeker", resists: [0.084, 0.0, 0.084, 0.084], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Triglavian", resists: [0.537, 0.166, 0.489, 0.371], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Drifter", resists: [0.43, 0.201, 0.43, 0.43], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Sleeper", resists: [0.512, 0.337, 0.512, 0.512], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]All", resists: [0.537, 0.269, 0.512, 0.495], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Drones", resists: [0.461, 0.305, 0.541, 0.443], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Overmind", resists: [0.666, 0.345, 0.634, 0.646], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Seeker", resists: [0.084, 0.0, 0.084, 0.084], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Triglavian", resists: [0.537, 0.085, 0.489, 0.371], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Drifter", resists: [0.43, 0.117, 0.43, 0.43], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Sleeper", resists: [0.512, 0.276, 0.512, 0.512], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]All", resists: [0.537, 0.201, 0.512, 0.495], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Drones", resists: [0.439, 0.522, 0.417, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Overmind", resists: [0.643, 0.593, 0.511, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Seeker", resists: [0.082, 0.082, 0.0, 0.082], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Triglavian", resists: [0.494, 0.41, 0.304, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Drifter", resists: [0.415, 0.415, 0.277, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Sleeper", resists: [0.435, 0.435, 0.329, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]All", resists: [0.508, 0.474, 0.359, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Drones", resists: [0.439, 0.522, 0.351, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Overmind", resists: [0.643, 0.593, 0.435, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Seeker", resists: [0.082, 0.082, 0.0, 0.082], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Triglavian", resists: [0.494, 0.41, 0.198, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Drifter", resists: [0.415, 0.415, 0.191, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Sleeper", resists: [0.435, 0.435, 0.268, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]All", resists: [0.508, 0.474, 0.276, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Drones", resists: [0.439, 0.522, 0.293, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Overmind", resists: [0.643, 0.593, 0.362, 0.639], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Seeker", resists: [0.082, 0.082, 0.0, 0.082], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Triglavian", resists: [0.494, 0.41, 0.122, 0.376], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Drifter", resists: [0.415, 0.415, 0.111, 0.415], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Sleeper", resists: [0.435, 0.435, 0.215, 0.435], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]All", resists: [0.508, 0.474, 0.208, 0.488], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Drones", resists: [0.449, 0.54, 0.549, 0.336], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Overmind", resists: [0.619, 0.574, 0.612, 0.522], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Seeker", resists: [0.085, 0.085, 0.085, 0.0], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Triglavian", resists: [0.477, 0.4, 0.461, 0.202], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Drifter", resists: [0.437, 0.437, 0.437, 0.295], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Sleeper", resists: [0.435, 0.435, 0.435, 0.329], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]All", resists: [0.493, 0.468, 0.492, 0.35], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Drones", resists: [0.449, 0.54, 0.549, 0.264], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Overmind", resists: [0.619, 0.574, 0.612, 0.449], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Seeker", resists: [0.085, 0.085, 0.085, 0.0], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Triglavian", resists: [0.477, 0.4, 0.461, 0.081], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Drifter", resists: [0.437, 0.437, 0.437, 0.206], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Sleeper", resists: [0.435, 0.435, 0.435, 0.268], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]All", resists: [0.493, 0.468, 0.492, 0.264], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Drones", resists: [0.449, 0.54, 0.549, 0.197], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Overmind", resists: [0.619, 0.574, 0.612, 0.379], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Seeker", resists: [0.085, 0.085, 0.085, 0.0], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Triglavian", resists: [0.477, 0.4, 0.461, 0.034], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Drifter", resists: [0.437, 0.437, 0.437, 0.121], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Sleeper", resists: [0.435, 0.435, 0.435, 0.215], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]All", resists: [0.493, 0.468, 0.492, 0.196], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Concord", resists: [0.324, 0.318, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Sansha", resists: [0.137, 0.331, 0.332, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Dark Matter All Tiers]Angel", resists: [0.582, 0.508, 0.457, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Concord", resists: [0.121, 0.318, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Sansha", resists: [0.034, 0.331, 0.332, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T0/T1/T2]Angel", resists: [0.456, 0.508, 0.457, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Concord", resists: [0.025, 0.318, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Sansha", resists: [0.018, 0.331, 0.332, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Angel", resists: [0.373, 0.508, 0.457, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Concord", resists: [0.008, 0.318, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Sansha", resists: [0.009, 0.331, 0.332, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Electrical T4/T5/T6]Angel", resists: [0.3, 0.508, 0.457, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Concord", resists: [0.324, 0.107, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Sansha", resists: [0.148, 0.181, 0.329, 0.328], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T0/T1/T2]Angel", resists: [0.587, 0.342, 0.439, 0.39], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Concord", resists: [0.324, 0.016, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Sansha", resists: [0.148, 0.14, 0.329, 0.328], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Angel", resists: [0.587, 0.241, 0.439, 0.39], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Concord", resists: [0.324, 0.004, 0.369, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Sansha", resists: [0.148, 0.106, 0.329, 0.328], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Firestorm T4/T5/T6]Angel", resists: [0.587, 0.172, 0.439, 0.39], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Concord", resists: [0.324, 0.318, 0.18, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Sansha", resists: [0.137, 0.331, 0.166, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T0/T1/T2]Angel", resists: [0.582, 0.508, 0.295, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Concord", resists: [0.324, 0.318, 0.089, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Sansha", resists: [0.137, 0.331, 0.108, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Angel", resists: [0.582, 0.508, 0.203, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Concord", resists: [0.324, 0.318, 0.068, 0.372], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Sansha", resists: [0.137, 0.331, 0.073, 0.322], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Exotic T4/T5/T6]Angel", resists: [0.582, 0.508, 0.14, 0.416], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Concord", resists: [0.324, 0.318, 0.369, 0.203], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Sansha", resists: [0.137, 0.355, 0.352, 0.16], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T0/T1/T2]Angel", resists: [0.59, 0.528, 0.477, 0.286], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Concord", resists: [0.324, 0.318, 0.369, 0.112], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Sansha", resists: [0.137, 0.355, 0.352, 0.05], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Angel", resists: [0.59, 0.528, 0.477, 0.197], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Concord", resists: [0.324, 0.318, 0.369, 0.086], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Sansha", resists: [0.137, 0.355, 0.352, 0.0], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Abyssal][Gamma T4/T5/T6]Angel", resists: [0.59, 0.528, 0.477, 0.126], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Drifter Entities", resists: [0.128, 0.375, 0.383, 0.383], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Rogue Drone Entities", resists: [0.104, 0.147, 0.147, 0.102], speed: None, sig: None, radius: None },
    Builtin { name: "[NPC][Invasion]Sleeper Entities", resists: [0.563, 0.563, 0.563, 0.563], speed: None, sig: None, radius: None },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_keep_pyfa_names_and_burner_numbers() {
        let enyo = TargetProfile::builtins()
            .find(|p| p.name == "[NPC][Burner][Team]Enyo")
            .expect("the Enyo burner is a builtin");
        assert_eq!(enyo.short_name(), "Enyo");
        assert_eq!(enyo.categories(), vec!["NPC", "Burner", "Team"]);
        assert_eq!(
            (enyo.max_velocity, enyo.sig_radius, enyo.radius),
            (1020.0, Some(37.0), 39.0)
        );
        assert_eq!(TargetProfile::ideal().sig_radius, None);
    }
}
