//! Target profiles as the graphs see them: a view over [`crate::target_profile`], with the
//! speed, signature and radius filled in the way application wants them.
//!
//! A profile is an abstract target, a set of resists with optional speed, signature and
//! radius.

use crate::target_profile::BUILTINS;

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
    /// One of the user's own, which the profile editor can change.
    pub user: bool,
}

impl From<&crate::target_profile::TargetProfile> for TargetProfile {
    fn from(p: &crate::target_profile::TargetProfile) -> Self {
        let builtin = crate::target_profile::TargetProfile::ideal().name == p.name
            || BUILTINS.iter().any(|b| b.name == p.name);
        Self {
            name: p.name.clone(),
            resists: p.resists,
            max_velocity: p.max_velocity(),
            sig_radius: p.signature_radius,
            radius: p.radius(),
            hp: p.hp,
            user: !builtin,
        }
    }
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
            user: false,
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
        BUILTINS.iter().map(Self::from)
    }
}

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
