//! pyfa's builtin target profiles (eos/saveddata/targetProfile.py), in its order.

use std::sync::LazyLock;

use super::TargetProfile;

/// Raw name, resists as fractions, then speed, signature radius and radius where pyfa has them.
type Builtin = (&'static str, [f64; 4], [Option<f64>; 3]);

#[expect(
    clippy::approx_constant,
    reason = "pyfa's resist values, one of which happens to be close to 1/pi"
)]
const BUILTIN: &[Builtin] = &[
    (
        "Uniform (25%)",
        [0.25, 0.25, 0.25, 0.25],
        [None, None, None],
    ),
    ("Uniform (50%)", [0.5, 0.5, 0.5, 0.5], [None, None, None]),
    (
        "Uniform (75%)",
        [0.75, 0.75, 0.75, 0.75],
        [None, None, None],
    ),
    ("Uniform (90%)", [0.9, 0.9, 0.9, 0.9], [None, None, None]),
    (
        "[T1 Resist]Shield",
        [0.0, 0.2, 0.4, 0.5],
        [None, None, None],
    ),
    (
        "[T1 Resist]Armor",
        [0.5, 0.45, 0.25, 0.1],
        [None, None, None],
    ),
    (
        "[T1 Resist]Hull",
        [0.33, 0.33, 0.33, 0.33],
        [None, None, None],
    ),
    (
        "[T1 Resist]Shield (+T2 DCU)",
        [0.125, 0.3, 0.475, 0.562],
        [None, None, None],
    ),
    (
        "[T1 Resist]Armor (+T2 DCU)",
        [0.575, 0.532, 0.363, 0.235],
        [None, None, None],
    ),
    (
        "[T1 Resist]Hull (+T2 DCU)",
        [0.598, 0.598, 0.598, 0.598],
        [None, None, None],
    ),
    (
        "[T2 Resist]Amarr (Shield)",
        [0.0, 0.2, 0.7, 0.875],
        [None, None, None],
    ),
    (
        "[T2 Resist]Amarr (Armor)",
        [0.5, 0.35, 0.625, 0.8],
        [None, None, None],
    ),
    (
        "[T2 Resist]Caldari (Shield)",
        [0.2, 0.84, 0.76, 0.6],
        [None, None, None],
    ),
    (
        "[T2 Resist]Caldari (Armor)",
        [0.5, 0.8625, 0.625, 0.1],
        [None, None, None],
    ),
    (
        "[T2 Resist]Gallente (Shield)",
        [0.0, 0.6, 0.85, 0.5],
        [None, None, None],
    ),
    (
        "[T2 Resist]Gallente (Armor)",
        [0.5, 0.675, 0.8375, 0.1],
        [None, None, None],
    ),
    (
        "[T2 Resist]Minmatar (Shield)",
        [0.75, 0.6, 0.4, 0.5],
        [None, None, None],
    ),
    (
        "[T2 Resist]Minmatar (Armor)",
        [0.9, 0.675, 0.25, 0.1],
        [None, None, None],
    ),
    (
        "[NPC][Asteroid]Angel Cartel",
        [0.54, 0.42, 0.37, 0.32],
        [None, None, None],
    ),
    (
        "[NPC][Asteroid]Blood Raiders",
        [0.34, 0.39, 0.45, 0.52],
        [None, None, None],
    ),
    (
        "[NPC][Asteroid]Guristas",
        [0.55, 0.35, 0.3, 0.48],
        [None, None, None],
    ),
    (
        "[NPC][Asteroid]Rogue Drones",
        [0.35, 0.38, 0.44, 0.49],
        [None, None, None],
    ),
    (
        "[NPC][Asteroid]Sanshas Nation",
        [0.35, 0.4, 0.47, 0.53],
        [None, None, None],
    ),
    (
        "[NPC][Asteroid]Serpentis",
        [0.49, 0.38, 0.29, 0.51],
        [None, None, None],
    ),
    (
        "[NPC][Deadspace]Angel Cartel",
        [0.59, 0.48, 0.4, 0.32],
        [None, None, None],
    ),
    (
        "[NPC][Deadspace]Blood Raiders",
        [0.31, 0.39, 0.47, 0.56],
        [None, None, None],
    ),
    (
        "[NPC][Deadspace]Guristas",
        [0.57, 0.39, 0.31, 0.5],
        [None, None, None],
    ),
    (
        "[NPC][Deadspace]Rogue Drones",
        [0.42, 0.42, 0.47, 0.49],
        [None, None, None],
    ),
    (
        "[NPC][Deadspace]Sanshas Nation",
        [0.31, 0.39, 0.47, 0.56],
        [None, None, None],
    ),
    (
        "[NPC][Deadspace]Serpentis",
        [0.49, 0.38, 0.29, 0.56],
        [None, None, None],
    ),
    (
        "[NPC][Mission]Amarr Empire",
        [0.34, 0.38, 0.42, 0.46],
        [None, None, None],
    ),
    (
        "[NPC][Mission]Caldari State",
        [0.51, 0.38, 0.3, 0.51],
        [None, None, None],
    ),
    (
        "[NPC][Mission]CONCORD",
        [0.47, 0.46, 0.47, 0.47],
        [None, None, None],
    ),
    (
        "[NPC][Mission]Gallente Federation",
        [0.51, 0.38, 0.31, 0.52],
        [None, None, None],
    ),
    (
        "[NPC][Mission]Khanid",
        [0.51, 0.42, 0.36, 0.4],
        [None, None, None],
    ),
    (
        "[NPC][Mission]Minmatar Republic",
        [0.51, 0.46, 0.41, 0.35],
        [None, None, None],
    ),
    (
        "[NPC][Mission]Mordus Legion",
        [0.32, 0.48, 0.4, 0.62],
        [None, None, None],
    ),
    (
        "[NPC][Other]Sleeper",
        [0.61, 0.61, 0.61, 0.61],
        [None, None, None],
    ),
    (
        "[NPC][Other]Sansha Incursion",
        [0.65, 0.63, 0.64, 0.65],
        [None, None, None],
    ),
    (
        "[NPC][Burner][Team]Enyo",
        [0.575, 0.724, 0.862, 0.235],
        [Some(1020.0), Some(37.0), Some(39.0)],
    ),
    (
        "[NPC][Burner][Team]Navitas",
        [0.681, 0.586, 0.522, 0.49],
        [Some(870.0), Some(30.0), Some(35.0)],
    ),
    (
        "[NPC][Burner][Team]Hawk",
        [0.3, 0.86, 0.79, 0.65],
        [Some(1122.0), Some(48.0), Some(39.0)],
    ),
    (
        "[NPC][Burner][Team]Bantam",
        [0.344, 0.475, 0.606, 0.672],
        [Some(1016.0), Some(45.0), Some(27.0)],
    ),
    (
        "[NPC][Burner][Team]Jaguar",
        [0.781, 0.65, 0.475, 0.563],
        [Some(1400.0), Some(42.0), Some(31.0)],
    ),
    (
        "[NPC][Burner][Team]Burst",
        [0.344, 0.475, 0.606, 0.672],
        [Some(1174.0), Some(39.0), Some(31.0)],
    ),
    (
        "[NPC][Burner][Team]Vengeance",
        [0.66, 0.558, 0.745, 0.864],
        [Some(1050.0), Some(37.0), Some(40.0)],
    ),
    (
        "[NPC][Burner][Team]Inquisitor",
        [0.681, 0.586, 0.522, 0.49],
        [Some(920.0), Some(29.0), Some(20.5)],
    ),
    (
        "[NPC][Burner]Cruor",
        [0.795, 0.734, 0.693, 0.672],
        [Some(900.0), Some(18.0), Some(20.5)],
    ),
    (
        "[NPC][Burner]Dramiel",
        [0.351, 0.481, 0.611, 0.676],
        [Some(2100.0), Some(11.0), Some(25.0)],
    ),
    (
        "[NPC][Burner]Daredevil",
        [0.685, 0.59, 0.59, 0.433],
        [Some(1200.0), Some(18.0), Some(25.0)],
    ),
    (
        "[NPC][Burner]Succubus",
        [0.351, 0.481, 0.611, 0.676],
        [Some(4750.0), Some(30.0), Some(59.0)],
    ),
    (
        "[NPC][Burner]Worm",
        [0.475, 0.58, 0.685, 0.738],
        [Some(360.0), Some(70.0), Some(39.0)],
    ),
    (
        "[NPC][Burner]Ashimmu",
        [0.8, 0.76, 0.68, 0.7],
        [Some(500.0), Some(120.0), Some(137.0)],
    ),
    (
        "[NPC][Burner]Sentinel",
        [0.575, 0.448, 0.522, 0.66],
        [Some(500.0), Some(50.0), Some(39.0)],
    ),
    (
        "[NPC][Burner]Talos",
        [0.681, 0.586, 0.586, 0.426],
        [Some(150.0), Some(125.0), Some(266.0)],
    ),
    (
        "[NPC][Burner]Dragonfly",
        [0.35, 0.72, 0.7, 0.55],
        [Some(1200.0), Some(15.0), Some(35.0)],
    ),
    (
        "[NPC][Burner]Mantis",
        [0.6, 0.52, 0.71, 0.71],
        [Some(900.0), Some(25.0), Some(35.0)],
    ),
    (
        "[NPC][Invasion]Triglavian Entities",
        [0.422, 0.367, 0.453, 0.411],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Amarr EDENCOM Entities",
        [0.36, 0.31, 0.441, 0.602],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Caldari EDENCOM Entities",
        [0.303, 0.61, 0.487, 0.401],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Gallente EDENCOM Entities",
        [0.383, 0.414, 0.578, 0.513],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Minmatar EDENCOM Entities",
        [0.62, 0.422, 0.355, 0.399],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Drones",
        [0.439, 0.522, 0.529, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Overmind",
        [0.643, 0.593, 0.624, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Seeker",
        [0.082, 0.082, 0.082, 0.082],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Triglavian",
        [0.494, 0.41, 0.464, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Drifter",
        [0.415, 0.415, 0.415, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Sleeper",
        [0.435, 0.435, 0.435, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]All",
        [0.508, 0.474, 0.495, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Drones",
        [0.323, 0.522, 0.529, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Overmind",
        [0.542, 0.593, 0.624, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Seeker",
        [0.0, 0.082, 0.082, 0.082],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Triglavian",
        [0.356, 0.41, 0.464, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Drifter",
        [0.277, 0.415, 0.415, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Sleeper",
        [0.329, 0.435, 0.435, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]All",
        [0.381, 0.474, 0.495, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Drones",
        [0.255, 0.522, 0.529, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Overmind",
        [0.48, 0.593, 0.624, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Seeker",
        [0.0, 0.082, 0.082, 0.0822],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Triglavian",
        [0.268, 0.41, 0.464, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Drifter",
        [0.191, 0.415, 0.415, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Sleeper",
        [0.268, 0.435, 0.435, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]All",
        [0.308, 0.474, 0.495, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Drones",
        [0.193, 0.522, 0.529, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Overmind",
        [0.423, 0.593, 0.624, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Seeker",
        [0.0, 0.082, 0.082, 0.082],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Triglavian",
        [0.206, 0.41, 0.464, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Drifter",
        [0.111, 0.415, 0.415, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Sleeper",
        [0.215, 0.435, 0.435, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]All",
        [0.247, 0.474, 0.495, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Drones",
        [0.461, 0.425, 0.541, 0.443],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Overmind",
        [0.666, 0.489, 0.634, 0.646],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Seeker",
        [0.084, 0.0, 0.084, 0.084],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Triglavian",
        [0.537, 0.269, 0.489, 0.371],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Drifter",
        [0.43, 0.289, 0.43, 0.43],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Sleeper",
        [0.512, 0.402, 0.512, 0.512],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]All",
        [0.537, 0.352, 0.512, 0.495],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Drones",
        [0.461, 0.36, 0.541, 0.443],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Overmind",
        [0.666, 0.413, 0.634, 0.646],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Seeker",
        [0.084, 0.0, 0.084, 0.084],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Triglavian",
        [0.537, 0.166, 0.489, 0.371],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Drifter",
        [0.43, 0.201, 0.43, 0.43],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Sleeper",
        [0.512, 0.337, 0.512, 0.512],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]All",
        [0.537, 0.269, 0.512, 0.495],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Drones",
        [0.461, 0.305, 0.541, 0.443],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Overmind",
        [0.666, 0.345, 0.634, 0.646],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Seeker",
        [0.084, 0.0, 0.084, 0.084],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Triglavian",
        [0.537, 0.085, 0.489, 0.371],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Drifter",
        [0.43, 0.117, 0.43, 0.43],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Sleeper",
        [0.512, 0.276, 0.512, 0.512],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]All",
        [0.537, 0.201, 0.512, 0.495],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Drones",
        [0.439, 0.522, 0.417, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Overmind",
        [0.643, 0.593, 0.511, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Seeker",
        [0.082, 0.082, 0.0, 0.082],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Triglavian",
        [0.494, 0.41, 0.304, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Drifter",
        [0.415, 0.415, 0.277, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Sleeper",
        [0.435, 0.435, 0.329, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]All",
        [0.508, 0.474, 0.359, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Drones",
        [0.439, 0.522, 0.351, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Overmind",
        [0.643, 0.593, 0.435, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Seeker",
        [0.082, 0.082, 0.0, 0.082],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Triglavian",
        [0.494, 0.41, 0.198, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Drifter",
        [0.415, 0.415, 0.191, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Sleeper",
        [0.435, 0.435, 0.268, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]All",
        [0.508, 0.474, 0.276, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Drones",
        [0.439, 0.522, 0.293, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Overmind",
        [0.643, 0.593, 0.362, 0.639],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Seeker",
        [0.082, 0.082, 0.0, 0.082],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Triglavian",
        [0.494, 0.41, 0.122, 0.376],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Drifter",
        [0.415, 0.415, 0.111, 0.415],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Sleeper",
        [0.435, 0.435, 0.215, 0.435],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]All",
        [0.508, 0.474, 0.208, 0.488],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Drones",
        [0.449, 0.54, 0.549, 0.336],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Overmind",
        [0.619, 0.574, 0.612, 0.522],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Seeker",
        [0.085, 0.085, 0.085, 0.0],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Triglavian",
        [0.477, 0.4, 0.461, 0.202],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Drifter",
        [0.437, 0.437, 0.437, 0.295],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Sleeper",
        [0.435, 0.435, 0.435, 0.329],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]All",
        [0.493, 0.468, 0.492, 0.35],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Drones",
        [0.449, 0.54, 0.549, 0.264],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Overmind",
        [0.619, 0.574, 0.612, 0.449],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Seeker",
        [0.085, 0.085, 0.085, 0.0],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Triglavian",
        [0.477, 0.4, 0.461, 0.081],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Drifter",
        [0.437, 0.437, 0.437, 0.206],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Sleeper",
        [0.435, 0.435, 0.435, 0.268],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]All",
        [0.493, 0.468, 0.492, 0.264],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Drones",
        [0.449, 0.54, 0.549, 0.197],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Overmind",
        [0.619, 0.574, 0.612, 0.379],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Seeker",
        [0.085, 0.085, 0.085, 0.0],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Triglavian",
        [0.477, 0.4, 0.461, 0.034],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Drifter",
        [0.437, 0.437, 0.437, 0.121],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Sleeper",
        [0.435, 0.435, 0.435, 0.215],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]All",
        [0.493, 0.468, 0.492, 0.196],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Concord",
        [0.324, 0.318, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Sansha",
        [0.137, 0.331, 0.332, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Dark Matter All Tiers]Angel",
        [0.582, 0.508, 0.457, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Concord",
        [0.121, 0.318, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Sansha",
        [0.034, 0.331, 0.332, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T0/T1/T2]Angel",
        [0.456, 0.508, 0.457, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Concord",
        [0.025, 0.318, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Sansha",
        [0.018, 0.331, 0.332, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T3 (Some T5 Rooms)]Angel",
        [0.373, 0.508, 0.457, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Concord",
        [0.008, 0.318, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Sansha",
        [0.009, 0.331, 0.332, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Electrical T4/T5/T6]Angel",
        [0.3, 0.508, 0.457, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Concord",
        [0.324, 0.107, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Sansha",
        [0.148, 0.181, 0.329, 0.328],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T0/T1/T2]Angel",
        [0.587, 0.342, 0.439, 0.39],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Concord",
        [0.324, 0.016, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Sansha",
        [0.148, 0.14, 0.329, 0.328],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T3 (Some T5 Rooms)]Angel",
        [0.587, 0.241, 0.439, 0.39],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Concord",
        [0.324, 0.004, 0.369, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Sansha",
        [0.148, 0.106, 0.329, 0.328],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Firestorm T4/T5/T6]Angel",
        [0.587, 0.172, 0.439, 0.39],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Concord",
        [0.324, 0.318, 0.18, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Sansha",
        [0.137, 0.331, 0.166, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T0/T1/T2]Angel",
        [0.582, 0.508, 0.295, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Concord",
        [0.324, 0.318, 0.089, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Sansha",
        [0.137, 0.331, 0.108, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T3 (Some T5 Rooms)]Angel",
        [0.582, 0.508, 0.203, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Concord",
        [0.324, 0.318, 0.068, 0.372],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Sansha",
        [0.137, 0.331, 0.073, 0.322],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Exotic T4/T5/T6]Angel",
        [0.582, 0.508, 0.14, 0.416],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Concord",
        [0.324, 0.318, 0.369, 0.203],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Sansha",
        [0.137, 0.355, 0.352, 0.16],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T0/T1/T2]Angel",
        [0.59, 0.528, 0.477, 0.286],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Concord",
        [0.324, 0.318, 0.369, 0.112],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Sansha",
        [0.137, 0.355, 0.352, 0.05],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T3 (Some T5 Rooms)]Angel",
        [0.59, 0.528, 0.477, 0.197],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Concord",
        [0.324, 0.318, 0.369, 0.086],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Sansha",
        [0.137, 0.355, 0.352, 0.0],
        [None, None, None],
    ),
    (
        "[NPC][Abyssal][Gamma T4/T5/T6]Angel",
        [0.59, 0.528, 0.477, 0.126],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Drifter Entities",
        [0.128, 0.375, 0.383, 0.383],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Rogue Drone Entities",
        [0.104, 0.147, 0.147, 0.102],
        [None, None, None],
    ),
    (
        "[NPC][Invasion]Sleeper Entities",
        [0.563, 0.563, 0.563, 0.563],
        [None, None, None],
    ),
];

/// The builtin profiles, in pyfa's order.
pub static BUILTINS: LazyLock<Vec<TargetProfile>> = LazyLock::new(|| {
    BUILTIN
        .iter()
        .map(
            |&(name, resists, [max_velocity, signature_radius, radius])| TargetProfile {
                max_velocity,
                signature_radius,
                radius,
                ..TargetProfile::new(name, resists)
            },
        )
        .collect()
});
