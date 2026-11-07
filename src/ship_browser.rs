//! The fittings browser's logic, after pyfa's gui/shipBrowser.py: ship groups, ships, their fits,
//! search and recent fits, with a history to go back through, a race filter and the option to
//! hide what has no fits.

use std::{
    collections::{HashMap, HashSet},
    hash::BuildHasher,
};

use serde::{Deserialize, Serialize};

use crate::{
    data::{Catalog, TypeId},
    fit::Fit,
};

/// Fits pyfa lists as recent.
pub const RECENT_LIMIT: usize = 50;
/// Fits a search turns up at most, as pyfa's query limits them.
pub const SEARCH_LIMIT: usize = 100;

/// Who builds a ship, as pyfa tells races apart for its race filter. In pyfa's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Race {
    Amarr,
    Caldari,
    Gallente,
    Minmatar,
    Sisters,
    Ore,
    Concord,
    Serpentis,
    Angel,
    Blood,
    Sansha,
    Guristas,
    Mordu,
    Deathless,
    Jove,
    Triglavian,
    Upwell,
}

impl Race {
    /// The corporation whose logo stands for the race: its faction's.
    pub fn logo_corp(self) -> Option<i32> {
        Some(match self {
            Self::Amarr => 1_000_084,
            Self::Caldari => 1_000_035,
            Self::Gallente => 1_000_120,
            Self::Minmatar => 1_000_051,
            Self::Sisters => 1_000_130,
            Self::Ore => 1_000_129,
            Self::Concord => 1_000_137,
            Self::Serpentis => 1_000_135,
            Self::Angel => 1_000_138,
            Self::Blood => 1_000_134,
            Self::Sansha => 1_000_162,
            Self::Guristas => 1_000_127,
            Self::Mordu => 1_000_128,
            Self::Deathless => 1_000_441,
            Self::Jove => 1_000_149,
            Self::Triglavian => 1_000_298,
            Self::Upwell => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Amarr => "Amarr",
            Self::Caldari => "Caldari",
            Self::Gallente => "Gallente",
            Self::Minmatar => "Minmatar",
            Self::Sisters => "Sisters of EVE",
            Self::Ore => "ORE",
            Self::Concord => "CONCORD",
            Self::Serpentis => "Serpentis",
            Self::Angel => "Angel Cartel",
            Self::Blood => "Blood Raiders",
            Self::Sansha => "Sansha's Nation",
            Self::Guristas => "Guristas",
            Self::Mordu => "Mordu's Legion",
            Self::Deathless => "Deathless",
            Self::Jove => "Jove",
            Self::Triglavian => "Triglavian",
            Self::Upwell => "Upwell",
        }
    }

    /// A ship's race the way pyfa's `Item.race` finds it: by the faction that builds it, else by
    /// the races of the skills it needs (two races make a pirate faction), else by its own race.
    pub fn resolve(faction: Option<i32>, skill_races: &[i32], race: Option<i32>) -> Option<Self> {
        if let Some(race) = faction.and_then(Self::of_faction) {
            return Some(race);
        }
        let sum: i32 = skill_races.iter().filter(|&&r| r != 0).sum();
        if sum == 10 {
            // Minmatar and Gallente skills: Angel when the Minmatar one comes first.
            let ordered: Vec<i32> = skill_races.iter().copied().filter(|&r| r != 0).collect();
            return Some(if ordered == [2, 8] {
                Self::Angel
            } else {
                Self::Serpentis
            });
        }
        Self::of_race_id(sum).or_else(|| race.and_then(Self::of_race_id))
    }

    fn of_faction(faction: i32) -> Option<Self> {
        Some(match faction {
            500_001 => Self::Caldari,
            500_002 => Self::Minmatar,
            500_003 => Self::Amarr,
            500_004 => Self::Gallente,
            500_005 => Self::Jove,
            500_010 => Self::Guristas,
            500_011 => Self::Angel,
            500_012 => Self::Blood,
            500_014 => Self::Ore,
            500_016 => Self::Sisters,
            500_018 => Self::Mordu,
            500_019 => Self::Sansha,
            500_020 => Self::Serpentis,
            500_026 => Self::Triglavian,
            500_027 => Self::Upwell,
            500_029 => Self::Deathless,
            _ => return None,
        })
    }

    /// pyfa's race ID map, which also reads sums of two races as the pirate faction flying both.
    fn of_race_id(race: i32) -> Option<Self> {
        Some(match race {
            1 => Self::Caldari,
            2 => Self::Minmatar,
            4 => Self::Amarr,
            5 | 32 => Self::Sansha,
            6 => Self::Blood,
            8 => Self::Gallente,
            9 => Self::Guristas,
            12 => Self::Sisters,
            15 => Self::Concord,
            16 => Self::Jove,
            128 => Self::Ore,
            135 => Self::Triglavian,
            168 => Self::Upwell,
            _ => return None,
        })
    }
}

/// What the fittings browser shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Ship groups: frigates, cruisers and so on.
    Groups,
    /// The ships of one group.
    Ships(i32),
    /// The fits of one ship.
    Fits(TypeId),
    /// Ships and fits whose name matches.
    Search(String),
    /// Fits changed last.
    Recent,
}

/// Where the fittings browser is and how it got there.
#[derive(Debug, Clone)]
pub struct Browser {
    pub stage: Stage,
    history: Vec<Stage>,
    /// Leave out groups and ships without fits.
    pub hide_empty: bool,
    /// Races picked in the race filter. None picked shows every race.
    pub races: HashSet<Race>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            stage: Stage::Groups,
            history: Vec::new(),
            hide_empty: false,
            races: HashSet::new(),
        }
    }
}

impl Browser {
    /// Show another stage, remembering this one for Back. The ship groups are the start, so
    /// going there forgets the way back; a new search replaces the one shown.
    pub fn go(&mut self, stage: Stage) {
        if stage == self.stage {
            return;
        }
        match (&self.stage, &stage) {
            (_, Stage::Groups) => self.history.clear(),
            (Stage::Search(_), Stage::Search(_)) => {}
            _ => self.history.push(self.stage.clone()),
        }
        self.stage = stage;
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn back(&mut self) {
        if let Some(stage) = self.history.pop() {
            self.stage = stage;
        }
    }

    pub fn toggle_race(&mut self, race: Race) {
        if !self.races.remove(&race) {
            self.races.insert(race);
        }
    }

    /// Whether a ship of this race shows, given the races among the ships listed: picking none
    /// of those shows them all, as in pyfa.
    pub fn shows_race(&self, race: Option<Race>, present: &HashSet<Race>) -> bool {
        let any_picked = present.iter().any(|r| self.races.contains(r));
        match race {
            _ if !any_picked => true,
            Some(race) => self.races.contains(&race),
            None => true,
        }
    }
}

/// How many fits each ship has.
pub fn fit_counts(fits: &[Fit]) -> HashMap<TypeId, usize> {
    let mut counts = HashMap::new();
    for ship in fits.iter().filter_map(|f| f.ship) {
        *counts.entry(ship).or_default() += 1;
    }
    counts
}

/// Fits in a group, counted over its ships.
pub fn group_fit_count<S: BuildHasher>(
    catalog: &Catalog,
    group: i32,
    counts: &HashMap<TypeId, usize, S>,
) -> usize {
    catalog
        .ships_in(group)
        .iter()
        .map(|s| counts.get(s).copied().unwrap_or(0))
        .sum()
}

/// A ship's fits by name.
pub fn fits_of(fits: &[Fit], ship: TypeId) -> Vec<usize> {
    let mut found: Vec<usize> = (0..fits.len())
        .filter(|&i| fits[i].ship == Some(ship))
        .collect();
    found.sort_by(|&a, &b| fits[a].name.cmp(&fits[b].name));
    found
}

/// Fits whose name holds the query, not minding case, by name.
pub fn search_fits(fits: &[Fit], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<usize> = (0..fits.len())
        .filter(|&i| fits[i].ship.is_some() && fits[i].name.to_lowercase().contains(&query))
        .collect();
    found.sort_by(|&a, &b| fits[a].name.cmp(&fits[b].name));
    found.truncate(SEARCH_LIMIT);
    found
}

/// Ships whose name holds the query, not minding case, by name.
pub fn search_ships(catalog: &Catalog, query: &str) -> Vec<TypeId> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<TypeId> = catalog
        .ship_groups
        .iter()
        .flat_map(|(_, ships)| ships.iter().copied())
        .filter(|&s| catalog.name(s).to_lowercase().contains(&query))
        .collect();
    found.sort_by_cached_key(|&s| catalog.name(s));
    found
}

/// The fits changed last, newest first. Fits from before efa dated them come last, newest
/// added first.
pub fn recent_fits(fits: &[Fit]) -> Vec<usize> {
    let mut found: Vec<usize> = (0..fits.len())
        .filter(|&i| fits[i].ship.is_some())
        .collect();
    found.sort_by(|&a, &b| fits[b].modified.cmp(&fits[a].modified).then(b.cmp(&a)));
    found.truncate(RECENT_LIMIT);
    found
}

/// When a fit changed, as pyfa's fit rows print it ("%m/%d/%Y %H:%M"), in EVE time. None for
/// fits that were never dated.
pub fn fit_date(secs: u64) -> Option<String> {
    if secs == 0 {
        return None;
    }
    let days = (secs / 86_400).cast_signed();
    let minutes = secs % 86_400 / 60;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{month:02}/{day:02}/{year} {:02}:{:02}",
        minutes / 60,
        minutes % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_print_like_pyfa() {
        assert_eq!(fit_date(0), None);
        assert_eq!(fit_date(1), Some("01/01/1970 00:00".to_owned()));
        assert_eq!(fit_date(951_827_696), Some("02/29/2000 12:34".to_owned()));
        assert_eq!(fit_date(1_794_063_780), Some("11/07/2026 15:03".to_owned()));
    }

    fn fit(name: &str, ship: TypeId, modified: u64) -> Fit {
        Fit {
            ship: Some(ship),
            modified,
            ..Fit::new(name)
        }
    }

    #[test]
    fn races_follow_pyfa() {
        assert_eq!(Race::resolve(Some(500_001), &[], None), Some(Race::Caldari));
        // No faction: two skills of different races make a pirate ship.
        assert_eq!(Race::resolve(None, &[1, 8], None), Some(Race::Guristas));
        assert_eq!(Race::resolve(None, &[2, 8], None), Some(Race::Angel));
        assert_eq!(Race::resolve(None, &[8, 2], None), Some(Race::Serpentis));
        assert_eq!(Race::resolve(None, &[], Some(4)), Some(Race::Amarr));
        assert_eq!(Race::resolve(None, &[], None), None);
    }

    #[test]
    fn back_retraces_the_way_and_groups_start_over() {
        let mut browser = Browser::default();
        assert!(!browser.can_go_back());
        browser.go(Stage::Ships(25));
        browser.go(Stage::Fits(587));
        browser.back();
        assert_eq!(browser.stage, Stage::Ships(25));
        browser.back();
        assert_eq!(browser.stage, Stage::Groups);
        assert!(!browser.can_go_back());

        browser.go(Stage::Search("ri".into()));
        browser.go(Stage::Search("rif".into()));
        browser.back();
        assert_eq!(browser.stage, Stage::Groups);

        browser.go(Stage::Ships(25));
        browser.go(Stage::Groups);
        assert!(!browser.can_go_back());
    }

    #[test]
    fn race_filter_shows_everything_until_a_listed_race_is_picked() {
        let mut browser = Browser::default();
        let present: HashSet<Race> = [Race::Amarr, Race::Caldari].into();
        assert!(browser.shows_race(Some(Race::Amarr), &present));
        browser.toggle_race(Race::Gallente);
        assert!(browser.shows_race(Some(Race::Amarr), &present));
        browser.toggle_race(Race::Caldari);
        assert!(!browser.shows_race(Some(Race::Amarr), &present));
        assert!(browser.shows_race(Some(Race::Caldari), &present));
        assert!(browser.shows_race(None, &present));
    }

    #[test]
    fn fit_lists() {
        let fits = vec![
            fit("Brawler", 1, 30),
            fit("armor kite", 2, 0),
            fit("Kiter", 1, 50),
            Fit::new("no ship"),
            fit("old", 2, 0),
        ];
        assert_eq!(fit_counts(&fits).get(&1), Some(&2));
        assert_eq!(fits_of(&fits, 1), vec![0, 2]);
        assert_eq!(search_fits(&fits, "KIT"), vec![2, 1]);
        assert_eq!(search_fits(&fits, " "), Vec::<usize>::new());
        assert_eq!(recent_fits(&fits), vec![2, 0, 4, 1]);
    }
}
