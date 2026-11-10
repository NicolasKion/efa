//! pyfa's generic command links: fleet boosts at a chosen strength, without a booster fit to
//! carry them. The engine flies them on a hidden booster with real T2 command bursts.

use serde::{Deserialize, Serialize};

use crate::data::{Catalog, TypeId};

/// The ship bonuses offered, in % per level with the ship skill at V, strongest first.
pub const STRENGTHS: [u8; 6] = [5, 4, 3, 2, 1, 0];

/// What a warfare mindlink adds to every burst, as pyfa assumes for every link.
const MINDLINK: f64 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkCategory {
    Shield,
    Armor,
    Skirmish,
    Information,
    Expedition,
    Mining,
}

impl LinkCategory {
    pub const ALL: [Self; 6] = [
        Self::Shield,
        Self::Armor,
        Self::Skirmish,
        Self::Information,
        Self::Expedition,
        Self::Mining,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Shield => "Shield",
            Self::Armor => "Armor",
            Self::Skirmish => "Skirmish",
            Self::Information => "Information",
            Self::Expedition => "Expedition",
            Self::Mining => "Mining",
        }
    }

    /// The T2 burst that carries the category's charges.
    pub fn burst(self) -> TypeId {
        match self {
            Self::Shield => 43_555,
            Self::Armor => 43_552,
            Self::Skirmish => 43_556,
            Self::Information => 43_554,
            Self::Expedition => 89_615,
            Self::Mining => 43_551,
        }
    }

    /// The category's burst charges, one per link.
    pub fn charges(self) -> &'static [TypeId] {
        match self {
            // Shield Harmonizing, Active Shielding, Shield Extension.
            Self::Shield => &[42_695, 42_694, 42_696],
            // Armor Energizing, Rapid Repair, Armor Reinforcement.
            Self::Armor => &[42_832, 42_833, 42_834],
            // Evasive Maneuvers, Interdiction Maneuvers, Rapid Deployment.
            Self::Skirmish => &[42_838, 42_839, 42_840],
            // Sensor Optimization, Electronic Superiority, Electronic Hardening.
            Self::Information => &[42_835, 42_836, 42_837],
            // Expedition Strength, Expedition Reach, Expedition Pinpointing.
            Self::Expedition => &[89_612, 89_613, 89_614],
            // Mining Laser Field Enhancement, Mining Laser Optimization, Mining Equipment
            // Preservation, Mining Laser Efficiency.
            Self::Mining => &[42_829, 42_830, 42_831, 90_733],
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Shield => "shield",
            Self::Armor => "armor",
            Self::Skirmish => "skirmish",
            Self::Information => "information",
            Self::Expedition => "expedition",
            Self::Mining => "mining",
        }
    }
}

/// The links a generic link entry stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkSet {
    All,
    Category(LinkCategory),
    /// One link, by its burst charge.
    Charge(TypeId),
}

impl LinkSet {
    /// Every burst charge of the set, with the category whose burst carries it.
    pub fn charges(self) -> Vec<(LinkCategory, TypeId)> {
        let of = |c: LinkCategory| c.charges().iter().map(move |&t| (c, t));
        match self {
            Self::All => LinkCategory::ALL.into_iter().flat_map(of).collect(),
            Self::Category(c) => of(c).collect(),
            Self::Charge(t) => LinkCategory::ALL
                .into_iter()
                .find(|c| c.charges().contains(&t))
                .map(|c| (c, t))
                .into_iter()
                .collect(),
        }
    }

    pub fn label(self, catalog: &Catalog) -> String {
        match self {
            Self::All => "All Links".to_owned(),
            Self::Category(c) => format!("All {} Links", c.label()),
            Self::Charge(t) => link_name(catalog, t),
        }
    }

    /// How the set is stored: "all", a category's name, or a charge's type ID.
    pub fn key(self) -> String {
        match self {
            Self::All => "all".to_owned(),
            Self::Category(c) => c.key().to_owned(),
            Self::Charge(t) => t.to_string(),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        if key == "all" {
            return Some(Self::All);
        }
        if let Some(c) = LinkCategory::ALL.into_iter().find(|c| c.key() == key) {
            return Some(Self::Category(c));
        }
        let charge: TypeId = key.parse().ok()?;
        LinkCategory::ALL
            .into_iter()
            .any(|c| c.charges().contains(&charge))
            .then_some(Self::Charge(charge))
    }
}

/// A link's name: its charge's, without "Charge".
pub fn link_name(catalog: &Catalog, charge: TypeId) -> String {
    let name = catalog.name(charge);
    name.strip_suffix(" Charge").unwrap_or(&name).to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandLink {
    pub set: LinkSet,
    /// The booster's ship bonus in % per level, its skill at V.
    pub strength: u8,
    pub mindlink: bool,
    pub active: bool,
}

impl CommandLink {
    pub fn new(set: LinkSet, strength: u8, mindlink: bool) -> Self {
        Self {
            set,
            strength: strength.min(STRENGTHS[0]),
            mindlink,
            active: true,
        }
    }

    /// pyfa's name for it: "All Shield Links (5%/lvl + Mindlink)".
    pub fn label(&self, catalog: &Catalog) -> String {
        let mindlink = if self.mindlink { " + Mindlink" } else { "" };
        format!(
            "{} ({}%/lvl{mindlink})",
            self.set.label(catalog),
            self.strength
        )
    }
}

/// What the ship bonus and mindlink multiply a burst's strength by. Skills and the T2 burst
/// come on top, from the booster itself.
pub fn link_factor(strength: u8, mindlink: bool) -> f64 {
    let ship = 1. + f64::from(strength) * 5. / 100.;
    ship * if mindlink { MINDLINK } else { 1. }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_round_trip_through_their_keys() {
        let sets = [
            LinkSet::All,
            LinkSet::Category(LinkCategory::Information),
            LinkSet::Charge(42_838),
        ];
        for set in sets {
            assert_eq!(LinkSet::parse(&set.key()), Some(set));
        }
        assert_eq!(LinkSet::parse("587"), None);
        assert_eq!(LinkSet::All.charges().len(), 19);
    }

    #[test]
    fn the_factor_matches_pyfa() {
        assert!((link_factor(0, false) - 1.).abs() < 1e-12);
        assert!((link_factor(5, true) - 1.5625).abs() < 1e-12);
    }
}
