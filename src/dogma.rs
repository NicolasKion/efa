//! The game's dogma IDs efa reads by number: attributes, effects and item groups, one name each.
//! Names follow the game's own where they read well.

/// Dogma attribute IDs.
pub mod attr {
    pub const MASS: i32 = 4;
    pub const CAPACITOR_NEED: i32 = 6;
    pub const HP: i32 = 9;
    pub const POWER_OUTPUT: i32 = 11;
    pub const LOW_SLOTS: i32 = 12;
    pub const MED_SLOTS: i32 = 13;
    pub const HI_SLOTS: i32 = 14;
    pub const SPEED_FACTOR: i32 = 20;
    pub const POWER: i32 = 30;
    pub const MAX_VELOCITY: i32 = 37;
    pub const CAPACITY: i32 = 38;
    pub const CPU_OUTPUT: i32 = 48;
    pub const CPU: i32 = 50;
    /// The game calls it `speed`: the cycle time of turrets and launchers.
    pub const RATE_OF_FIRE: i32 = 51;
    /// Optimal range.
    pub const MAX_RANGE: i32 = 54;
    pub const RECHARGE_RATE: i32 = 55;
    pub const DAMAGE_MULTIPLIER: i32 = 64;
    pub const CAPACITOR_BONUS: i32 = 67;
    pub const SHIELD_BONUS: i32 = 68;
    pub const AGILITY: i32 = 70;
    pub const DURATION: i32 = 73;
    pub const ARMOR_DAMAGE_AMOUNT: i32 = 84;
    pub const SHIELD_TRANSFER_RANGE: i32 = 87;
    /// Capacitor moved per cycle by nosferatus and remote capacitor transmitters.
    pub const POWER_TRANSFER_AMOUNT: i32 = 90;
    pub const POWER_TRANSFER_RANGE: i32 = 91;
    pub const NEUT_AMOUNT: i32 = 97;
    pub const EMP_FIELD_RANGE: i32 = 99;
    pub const LAUNCHER_SLOTS: i32 = 101;
    pub const TURRET_SLOTS: i32 = 102;
    pub const WARP_SCRAMBLE_RANGE: i32 = 103;
    pub const WARP_SCRAMBLE_STATUS: i32 = 104;
    pub const WARP_SCRAMBLE_STRENGTH: i32 = 105;
    pub const EXPLOSION_RANGE: i32 = 107;
    pub const SHIP_SCAN_RANGE: i32 = 125;
    pub const CARGO_SCAN_RANGE: i32 = 126;
    pub const ECM_BURST_RANGE: i32 = 142;
    pub const FALLOFF: i32 = 158;
    pub const TRACKING_SPEED: i32 = 160;
    pub const RADIUS: i32 = 162;
    pub const SURVEY_SCAN_RANGE: i32 = 197;
    pub const SPEED_MULTIPLIER: i32 = 204;
    pub const MISSILE_DAMAGE_MULTIPLIER_BONUS: i32 = 213;
    pub const SCAN_GRAVIMETRIC_BONUS: i32 = 238;
    pub const SCAN_LADAR_BONUS: i32 = 239;
    pub const SCAN_MAGNETOMETRIC_BONUS: i32 = 240;
    pub const SCAN_RADAR_BONUS: i32 = 241;
    pub const SHIELD_CAPACITY: i32 = 263;
    pub const ARMOR_EM_RESONANCE: i32 = 267;
    pub const ARMOR_EXPLOSIVE_RESONANCE: i32 = 268;
    pub const ARMOR_KINETIC_RESONANCE: i32 = 269;
    pub const ARMOR_THERMAL_RESONANCE: i32 = 270;
    pub const EXPLOSION_DELAY: i32 = 281;
    pub const MAX_TARGET_RANGE_BONUS: i32 = 309;
    pub const BOOSTER_DURATION: i32 = 330;
    pub const FALLOFF_BONUS: i32 = 349;
    pub const MAX_RANGE_BONUS: i32 = 351;
    pub const SHIELD_RECHARGE_RATE: i32 = 479;
    pub const CAPACITOR_CAPACITY: i32 = 482;
    pub const SHIP_SCAN_FALLOFF: i32 = 510;
    pub const MISSILE_VELOCITY_BONUS: i32 = 547;
    pub const SIGNATURE_RADIUS: i32 = 552;
    pub const SIGNATURE_RADIUS_BONUS: i32 = 554;
    pub const CLOAKING_TARGETING_DELAY: i32 = 560;
    pub const SCAN_RESOLUTION: i32 = 564;
    pub const SCAN_RESOLUTION_BONUS: i32 = 566;
    pub const EXPLOSION_DELAY_BONUS: i32 = 596;
    pub const OPTIMAL_SIG_RADIUS: i32 = 620;
    pub const AOE_VELOCITY: i32 = 653;
    pub const AOE_CLOUD_SIZE: i32 = 654;
    pub const REACTIVATION_DELAY: i32 = 669;
    pub const CONSUMPTION_TYPE: i32 = 713;
    pub const CONSUMPTION_QUANTITY: i32 = 714;
    pub const TRACKING_SPEED_BONUS: i32 = 767;
    pub const COMMAND_BONUS: i32 = 833;
    pub const AOE_VELOCITY_BONUS: i32 = 847;
    pub const AOE_CLOUD_SIZE_BONUS: i32 = 848;
    pub const ACCESS_DIFFICULTY_BONUS: i32 = 902;
    pub const ECM_RANGE_OPTIMAL: i32 = 936;
    pub const NEUT_DURATION: i32 = 942;
    pub const SCAN_GRAVIMETRIC_PERCENT: i32 = 1027;
    pub const SCAN_LADAR_PERCENT: i32 = 1028;
    pub const SCAN_MAGNETOMETRIC_PERCENT: i32 = 1029;
    pub const SCAN_RADAR_PERCENT: i32 = 1030;
    pub const RIG_SLOTS: i32 = 1137;
    pub const UPGRADE_COST: i32 = 1153;
    pub const HEAT_ABSORBTION_RATE_MODIFIER: i32 = 1180;
    pub const HEAT_DAMAGE: i32 = 1211;
    pub const HEAT_GENERATION_MULTIPLIER: i32 = 1224;
    pub const DRONE_DAMAGE_BONUS: i32 = 1255;
    pub const HEAT_ATTENUATION_HI: i32 = 1259;
    pub const HEAT_ATTENUATION_MED: i32 = 1261;
    pub const HEAT_ATTENUATION_LOW: i32 = 1262;
    pub const DRONE_BANDWIDTH: i32 = 1271;
    pub const COMMAND_BONUS_HIDDEN: i32 = 1310;
    pub const ACTIVATION_BLOCKED_STRENGTH: i32 = 1350;
    pub const AOE_DAMAGE_REDUCTION_FACTOR: i32 = 1353;
    pub const BASE_SCAN_RANGE: i32 = 1370;
    pub const BASE_SENSOR_STRENGTH: i32 = 1371;
    pub const HI_SLOT_MODIFIER: i32 = 1374;
    pub const MED_SLOT_MODIFIER: i32 = 1375;
    pub const LOW_SLOT_MODIFIER: i32 = 1376;
    pub const RIG_SIZE: i32 = 1547;
    pub const RELOAD_TIME: i32 = 1795;
    pub const CHARGED_ARMOR_DAMAGE_MULTIPLIER: i32 = 1886;
    pub const VIRUS_COHERENCE: i32 = 1909;
    pub const VIRUS_STRENGTH: i32 = 1910;
    pub const NOS_OVERRIDE: i32 = 1945;
    pub const FALLOFF_EFFECTIVENESS: i32 = 2044;
    pub const ENERGY_WARFARE_RESISTANCE: i32 = 2045;
    pub const FTR_MISSILES_ER: i32 = 2125;
    pub const FTR_MISSILES_EV: i32 = 2126;
    pub const FTR_MISSILES_DRF: i32 = 2127;
    pub const FTR_MISSILES_DRS: i32 = 2128;
    pub const FTR_MISSILES_RANGE: i32 = 2149;
    pub const FTR_WEB_PENALTY: i32 = 2184;
    pub const FTR_WEB_OPTIMAL: i32 = 2186;
    pub const FTR_WEB_FALLOFF: i32 = 2187;
    pub const FTR_ATTACK_DRF: i32 = 2231;
    pub const FTR_ATTACK_DRS: i32 = 2232;
    pub const FTR_ATTACK_ER: i32 = 2234;
    pub const FTR_ATTACK_EV: i32 = 2235;
    pub const FTR_ATTACK_OPTIMAL: i32 = 2236;
    pub const FTR_ATTACK_FALLOFF: i32 = 2237;
    pub const DOOMSDAY_AOE_RANGE: i32 = 2279;
    pub const FTR_KAMIKAZE_RANGE: i32 = 2330;
    pub const NEUT_SIG_RESOLUTION: i32 = 2451;
    pub const MAX_FOF_RANGE: i32 = 2700;
    pub const DAMAGE_MULTIPLIER_BONUS_PER_CYCLE: i32 = 2733;
    pub const DAMAGE_MULTIPLIER_BONUS_MAX: i32 = 2734;
    pub const DOT_DURATION: i32 = 5735;
    pub const DOT_MAX_DAMAGE_PER_TICK: i32 = 5736;
    pub const DOT_MAX_HP_PERCENTAGE_PER_TICK: i32 = 5737;
    pub const BREACHER_RESIST: i32 = 6255;

    /// EM, thermal, kinetic, explosive damage of a charge or drone.
    pub const DAMAGE: [i32; 4] = [114, 118, 117, 116];
    /// EM, thermal, kinetic, explosive resonances of each layer.
    pub const HULL_RESONANCE: [i32; 4] = [113, 110, 109, 111];
    pub const ARMOR_RESONANCE: [i32; 4] = [
        ARMOR_EM_RESONANCE,
        ARMOR_THERMAL_RESONANCE,
        ARMOR_KINETIC_RESONANCE,
        ARMOR_EXPLOSIVE_RESONANCE,
    ];
    pub const SHIELD_RESONANCE: [i32; 4] = [271, 274, 273, 272];
    /// Jam strength per sensor type, the bonuses ECM gives.
    pub const ECM_STRENGTH: [i32; 4] = [
        SCAN_GRAVIMETRIC_BONUS,
        SCAN_LADAR_BONUS,
        SCAN_MAGNETOMETRIC_BONUS,
        SCAN_RADAR_BONUS,
    ];
    /// A fighter's jam strength per sensor type.
    pub const FTR_ECM_STRENGTH: [i32; 4] = [2246, 2247, 2248, 2249];
    /// warfareBuff1ID to warfareBuff4ID.
    pub const WARFARE_BUFF_IDS: [i32; 4] = [2468, 2470, 2472, 2536];
    /// warfareBuff1Value to warfareBuff4Value.
    pub const WARFARE_BUFF_VALUES: [i32; 4] = [2469, 2471, 2473, 2537];
}

/// Dogma effect IDs.
pub mod effect {
    pub const FOF_MISSILE: i32 = 104;
    pub const DISRUPT_SPHERE: i32 = 3380;
    pub const NOSFERATU: i32 = 6197;
    pub const DOOMSDAY_WEB: i32 = 6476;
    pub const DOOMSDAY_NEUT: i32 = 6477;
    pub const DOOMSDAY_PAINT: i32 = 6478;
    pub const DOOMSDAY_TRACK: i32 = 6479;
    pub const DOOMSDAY_DAMP: i32 = 6481;
    pub const DOOMSDAY_ECM: i32 = 6513;
    /// Drone and fighter ewar: the game's entity effects.
    pub const ENTITY_WEBIFIER: i32 = 6690;
    pub const ENTITY_NEUTRALIZER: i32 = 6691;
    pub const ENTITY_TARGET_PAINTER: i32 = 6692;
    pub const ENTITY_SENSOR_DAMPENER: i32 = 6693;
    pub const ENTITY_WEAPON_DISRUPTOR: i32 = 6694;
    pub const ENTITY_ECM: i32 = 6695;
    pub const FOCUSED_SCRIPT: i32 = 6848;
    pub const VORTON: i32 = 8037;

    /// Single-target doomsdays and lances.
    pub const SUPER_WEAPONS: [i32; 5] = [4489, 4490, 4491, 4492, 6447];
    pub const NEUTRALIZER: [i32; 2] = [6187, 6216];
    pub const WEBIFIER: [i32; 2] = [6426, 6682];
    pub const ECM: [i32; 2] = [6470, 6685];
    pub const SENSOR_DAMPENER: [i32; 2] = [6422, 6684];
    pub const TRACKING_DISRUPTOR: [i32; 2] = [6424, 6686];
    pub const GUIDANCE_DISRUPTOR: [i32; 2] = [6423, 6686];
    pub const TARGET_PAINTER: [i32; 2] = [6425, 6683];
    pub const SCRAM: [i32; 2] = [5934, 6222];
    /// Microwarpdrives and micro jump drives, which a scram shuts down.
    pub const SCRAMMABLE: [i32; 3] = [6730, 4921, 6208];
}

/// Item group IDs.
pub mod group {
    pub const SHIELD_BOOSTER: i32 = 40;
    pub const REMOTE_SHIELD_BOOSTER: i32 = 41;
    pub const ARMOR_REPAIRER: i32 = 62;
    pub const SMART_BOMB: i32 = 72;
    pub const CAPACITOR_BOOSTER: i32 = 76;
    pub const REMOTE_ARMOR_REPAIRER: i32 = 325;
    pub const REMOTE_HULL_REPAIRER: i32 = 585;
    pub const SUPER_WEAPON: i32 = 588;
    pub const BOMB_LAUNCHER: i32 = 862;
    pub const ANCILLARY_SHIELD_BOOSTER: i32 = 1156;
    pub const ANCILLARY_ARMOR_REPAIRER: i32 = 1199;
    pub const GUIDED_BOMB_LAUNCHER: i32 = 1328;
    pub const AREA_DENIAL: i32 = 1330;
    pub const STRUCTURE_DOOMSDAY: i32 = 1333;
    pub const ANCILLARY_REMOTE_SHIELD_BOOSTER: i32 = 1697;
    pub const ANCILLARY_REMOTE_ARMOR_REPAIRER: i32 = 1698;
    pub const MUTADAPTIVE_REMOTE_ARMOR_REPAIRER: i32 = 2018;

    /// Modules that change speed but cannot run in warp: off for the speed a warp starts at.
    pub const NOT_IN_WARP: [i32; 8] = [46, 2008, 330, 515, SUPER_WEAPON, 658, 815, 590];
    /// Battlecruisers: medium hulls that carry large shield extenders and boosters.
    pub const BATTLECRUISERS: [i32; 3] = [419, 540, 1201];
}
