//! Which types count as published: the SDE's flag with pyfa's corrections from its data update
//! (`db_update.py`). Everything that lists items goes by the result, so an item hidden here is
//! hidden everywhere.

/// `boosterLastInjectionDatetime`: the last day a booster can be used, in days since 1970.
pub const LAST_INJECTION: i32 = 2422;

/// The destructible effect beacons group, which pyfa publishes for its projection menus.
const GROUP_DESTRUCTIBLE_BEACON: i32 = 4033;

/// Single types pyfa publishes: the Metenox service and two Trig buff carriers.
const PUBLISHED_TYPES: [i32; 3] = [82941, 87164, 87177];

/// Drifter weapons the SDE publishes by mistake.
const DRIFTER_WEAPONS: [&str; 4] = ["Lux Kontos", "Lux Xiphos", "Lux Ballistra", "Lux Kopis"];

/// Whether a type counts as published. `last_injection` is the booster expiry, when it has one,
/// and `today` the day the data is read, both in days since 1970.
pub fn published(
    id: i32,
    name: &str,
    group_id: i32,
    flag: bool,
    last_injection: Option<f64>,
    today: f64,
) -> bool {
    // Event boosters stay in the data long after they stop working in game; pyfa drops them
    // after its other fixes, so this wins over them.
    if last_injection.is_some_and(|last| last > 0.0 && last < today) {
        return false;
    }
    if (name.starts_with("Civilian") && !name.contains("Shuttle"))
        || name == "Capsule"
        || group_id == GROUP_DESTRUCTIBLE_BEACON
        || PUBLISHED_TYPES.contains(&id)
        || is_air_booster(name)
    {
        return true;
    }
    if name.starts_with("Limited Synth ")
        || name.starts_with("Expired ")
        || is_grand_prix_booster(name)
        || is_mining_blitz_dose(name)
        || name.contains("_PLACEHOLDER")
        || DRIFTER_WEAPONS.contains(&name)
    {
        return false;
    }
    flag
}

/// The current day in days since 1970, the unit booster expiry dates use.
pub fn today() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() / 86_400.0)
}

/// `AIR .+Booster.*`
fn is_air_booster(name: &str) -> bool {
    name.strip_prefix("AIR ")
        .and_then(|rest| rest.find("Booster"))
        .is_some_and(|at| at > 0)
}

/// `Grand Prix .*Booster`
fn is_grand_prix_booster(name: &str) -> bool {
    name.strip_prefix("Grand Prix ")
        .is_some_and(|rest| rest.contains("Booster"))
}

/// `Mining Blitz .+ Booster Dose .+`
fn is_mining_blitz_dose(name: &str) -> bool {
    name.strip_prefix("Mining Blitz ")
        .and_then(|rest| rest.find(" Booster Dose ").map(|at| (at, rest)))
        .is_some_and(|(at, rest)| at > 0 && rest.len() > at + " Booster Dose ".len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: f64 = 20_765.0;

    fn check(name: &str, flag: bool, last: Option<f64>) -> bool {
        published(1, name, 1, flag, last, TODAY)
    }

    #[test]
    fn expired_boosters_are_hidden() {
        assert!(!check(
            "Serenity YC122.9 Season Booster - Max Velocity",
            true,
            Some(18_500.0)
        ));
        assert!(check("Republic Defense Booster I", true, Some(20_767.5)));
        assert!(check("Standard Blue Pill Booster", true, None));
    }

    #[test]
    fn pyfas_name_rules() {
        assert!(check("AIR Armor Repair Booster I", false, None));
        assert!(!check("Limited Synth Blue Pill Booster", true, None));
        assert!(!check("Expired Cerebral Accelerator", true, None));
        assert!(!check("Grand Prix Speed Booster", true, None));
        assert!(!check("Mining Blitz Yield Booster Dose I", true, None));
        assert!(!check("Lux Kontos", true, None));
        assert!(check("Civilian Gatling Railgun", false, None));
        assert!(!check("Civilian Shuttle", false, None));
        assert!(check("Capsule", false, None));
        assert!(!check("AIR Armor Repair Booster I", false, Some(18_000.0)));
    }
}
