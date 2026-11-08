//! The two kinds of entry the editor edits, damage patterns and target profiles, and how their
//! fields read and show.

use std::rc::Rc;

use gpui_kit::SharedString;

use crate::ui::widgets::{icon, parse_decimal};
use crate::{
    damage_pattern::{self, DamagePattern},
    fit::Fit,
    store::Saved,
    target_profile::{self, TargetProfile},
};

/// One value field of an entry.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub label: &'static str,
    /// pyfa's tooltip for the field.
    pub tip: &'static str,
    /// Game icon shown before the field, if it has one.
    pub icon: Option<i32>,
    pub unit: &'static str,
}

/// What an editor edits: damage patterns or target profiles.
pub trait Kind: 'static {
    type Item: Clone + PartialEq + 'static;
    /// The window's title, after "efa: ".
    const TITLE: &'static str;
    /// What pyfa calls one entry in its prompts.
    const ENTITY: &'static str;
    /// The caption over the list.
    const LIST: &'static str;
    const IMPORT_TIP: &'static str;
    const EXPORT_TIP: &'static str;
    const IMPORTED: &'static str;
    const EXPORTED: &'static str;
    const WIDTH: f32;

    fn fields() -> &'static [Field];
    fn name(item: &Self::Item) -> &str;
    fn renamed(item: &Self::Item, name: &str) -> Self::Item;
    /// What New makes.
    fn blank(name: &str) -> Self::Item;
    fn builtins() -> &'static [Self::Item];
    fn user(saved: &Saved) -> &[Self::Item];
    fn user_mut(saved: &mut Saved) -> &mut Vec<Self::Item>;
    /// Whether the editor lists a user entry at all.
    fn listed(_: &Self::Item) -> bool {
        true
    }
    /// Whether a listed user entry's values can be changed.
    fn editable(_: &Self::Item) -> bool {
        true
    }
    /// The text each field shows for an entry.
    fn texts(item: &Self::Item) -> Vec<String>;
    /// The entry with the fields' texts read in, or why they cannot be.
    fn read(item: &Self::Item, texts: &[SharedString]) -> Result<Self::Item, &'static str>;
    /// What the editor shows after a field, e.g. a damage type's share.
    fn note(_: &Self::Item, _: usize) -> Option<String> {
        None
    }
    /// Read clipboard text into the user's entries and say how it went.
    fn import(text: &str, user: &mut Vec<Self::Item>) -> String;
    fn export(user: &[Self::Item]) -> String;
    /// Make fits that use user entry `old` follow it to `new`, or drop it.
    fn retarget(fits: &mut [Rc<Fit>], old: &str, new: Option<&Self::Item>);
}

const DAMAGE_FIELDS: [Field; 4] = [
    Field {
        label: "EM",
        tip: "EM damage",
        icon: Some(icon::EM),
        unit: "",
    },
    Field {
        label: "Thermal",
        tip: "Thermal damage",
        icon: Some(icon::THERMAL),
        unit: "",
    },
    Field {
        label: "Kinetic",
        tip: "Kinetic damage",
        icon: Some(icon::KINETIC),
        unit: "",
    },
    Field {
        label: "Explosive",
        tip: "Explosive damage",
        icon: Some(icon::EXPLOSIVE),
        unit: "",
    },
];

const PROFILE_FIELDS: [Field; 8] = [
    Field {
        label: "EM",
        tip: "EM resistance",
        icon: Some(icon::EM),
        unit: "%",
    },
    Field {
        label: "Thermal",
        tip: "Thermal resistance",
        icon: Some(icon::THERMAL),
        unit: "%",
    },
    Field {
        label: "Kinetic",
        tip: "Kinetic resistance",
        icon: Some(icon::KINETIC),
        unit: "%",
    },
    Field {
        label: "Explosive",
        tip: "Explosive resistance",
        icon: Some(icon::EXPLOSIVE),
        unit: "%",
    },
    Field {
        label: "Speed",
        tip: "Maximum speed",
        icon: None,
        unit: "m/s",
    },
    Field {
        label: "Signature",
        tip: "Signature radius\nLeave blank for infinitely big value",
        icon: None,
        unit: "m",
    },
    Field {
        label: "Radius",
        tip: "Radius\nThe radius of the sphere that represents a ship/drone in space. Affects range calculations.",
        icon: None,
        unit: "m",
    },
    Field {
        label: "HP",
        tip: "Total HP\nAffects how much damage breacher pods can do. Leave blank for infinitely big value",
        icon: None,
        unit: "hp",
    },
];

/// pyfa's `FloatBox` complaint.
const BAD_NUMBER: &str = "Incorrect formatting (decimals only)";

/// A number as pyfa's float boxes show it: no trailing zeros.
fn plain(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A field's number; blank is none.
fn number(text: &str) -> Result<Option<f64>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    parse_decimal::<f64>(text)
        .filter(|v| v.is_finite())
        .map(Some)
        .ok_or(BAD_NUMBER)
}

/// The Damage Pattern Editor.
pub struct DamagePatterns;

impl Kind for DamagePatterns {
    type Item = DamagePattern;
    const TITLE: &'static str = "Damage Pattern Editor";
    const ENTITY: &'static str = "Damage Profile";
    const LIST: &'static str = "Damage patterns";
    const IMPORT_TIP: &'static str = "Import patterns from clipboard";
    const EXPORT_TIP: &'static str = "Export patterns to clipboard";
    const IMPORTED: &'static str = "Patterns successfully imported from clipboard";
    const EXPORTED: &'static str = "Patterns exported to clipboard";
    const WIDTH: f32 = 640.;

    fn fields() -> &'static [Field] {
        &DAMAGE_FIELDS
    }
    fn name(item: &DamagePattern) -> &str {
        &item.name
    }
    fn renamed(item: &DamagePattern, name: &str) -> DamagePattern {
        DamagePattern::new(name, item.amounts)
    }
    fn blank(name: &str) -> DamagePattern {
        DamagePattern::new(name, [0.0; 4])
    }
    fn builtins() -> &'static [DamagePattern] {
        &damage_pattern::BUILTINS
    }
    fn user(saved: &Saved) -> &[DamagePattern] {
        &saved.damage_patterns
    }
    fn user_mut(saved: &mut Saved) -> &mut Vec<DamagePattern> {
        &mut saved.damage_patterns
    }
    fn listed(item: &DamagePattern) -> bool {
        damage_pattern::listed(item)
    }
    fn editable(item: &DamagePattern) -> bool {
        damage_pattern::editable(item)
    }
    fn texts(item: &DamagePattern) -> Vec<String> {
        // pyfa shows the amounts rounded to whole numbers.
        item.amounts
            .iter()
            .map(|a| (a.round() as i64).to_string())
            .collect()
    }
    fn read(item: &DamagePattern, texts: &[SharedString]) -> Result<DamagePattern, &'static str> {
        let mut amounts = [0.0; 4];
        for (amount, text) in amounts.iter_mut().zip(texts) {
            let value = number(text)?.unwrap_or(0.0);
            if value < 0.0 {
                return Err("Incorrect range (must be 0 or more)");
            }
            *amount = value;
        }
        Ok(DamagePattern::new(item.name.clone(), amounts))
    }
    fn note(item: &DamagePattern, i: usize) -> Option<String> {
        let total: f64 = item.amounts.iter().sum();
        let share = if total > 0.0 {
            item.amounts[i] * 100.0 / total
        } else {
            0.0
        };
        Some(format!("{share:.1}%"))
    }
    fn import(text: &str, user: &mut Vec<DamagePattern>) -> String {
        let imported = damage_pattern::import(text);
        let notice = imported.notice(Self::IMPORTED);
        damage_pattern::merge(user, imported.items);
        notice
    }
    fn export(user: &[DamagePattern]) -> String {
        damage_pattern::export(user)
    }
    fn retarget(fits: &mut [Rc<Fit>], old: &str, new: Option<&DamagePattern>) {
        damage_pattern::retarget(fits, old, new);
    }
}

/// The Target Profile Editor.
pub struct TargetProfiles;

impl Kind for TargetProfiles {
    type Item = TargetProfile;
    const TITLE: &'static str = "Target Profile Editor";
    const ENTITY: &'static str = "Target Profile";
    const LIST: &'static str = "Target profiles";
    const IMPORT_TIP: &'static str = "Import profiles from clipboard";
    const EXPORT_TIP: &'static str = "Export profiles to clipboard";
    const IMPORTED: &'static str = "Profiles successfully imported from clipboard";
    const EXPORTED: &'static str = "Profiles exported to clipboard";
    const WIDTH: f32 = 720.;

    fn fields() -> &'static [Field] {
        &PROFILE_FIELDS
    }
    fn name(item: &TargetProfile) -> &str {
        &item.name
    }
    fn renamed(item: &TargetProfile, name: &str) -> TargetProfile {
        TargetProfile {
            name: name.to_owned(),
            ..item.clone()
        }
    }
    fn blank(name: &str) -> TargetProfile {
        TargetProfile::new(name, [0.0; 4])
    }
    fn builtins() -> &'static [TargetProfile] {
        &target_profile::BUILTINS
    }
    fn user(saved: &Saved) -> &[TargetProfile] {
        &saved.target_profiles
    }
    fn user_mut(saved: &mut Saved) -> &mut Vec<TargetProfile> {
        &mut saved.target_profiles
    }
    fn texts(item: &TargetProfile) -> Vec<String> {
        let infinite = |v: Option<f64>| v.map_or_else(String::new, plain);
        item.resists
            .iter()
            .map(|r| plain(r * 100.0))
            .chain([
                plain(item.max_velocity()),
                infinite(item.signature_radius),
                plain(item.radius()),
                infinite(item.hp),
            ])
            .collect()
    }
    fn read(item: &TargetProfile, texts: &[SharedString]) -> Result<TargetProfile, &'static str> {
        let mut resists = [0.0; 4];
        for (resist, text) in resists.iter_mut().zip(texts) {
            let value = number(text)?.unwrap_or(0.0);
            if !(0.0..=100.0).contains(&value) {
                return Err("Incorrect range (must be 0-100)");
            }
            *resist = value / 100.0;
        }
        let misc = |i: usize| number(texts.get(i).map_or("", |t| t.as_ref()));
        Ok(TargetProfile {
            name: item.name.clone(),
            resists,
            max_velocity: misc(4)?,
            signature_radius: misc(5)?,
            radius: misc(6)?,
            hp: misc(7)?,
        })
    }
    fn import(text: &str, user: &mut Vec<TargetProfile>) -> String {
        let imported = target_profile::import(text);
        let notice = imported.notice(Self::IMPORTED);
        target_profile::merge(user, imported.items);
        notice
    }
    fn export(user: &[TargetProfile]) -> String {
        target_profile::export(user)
    }
    fn retarget(fits: &mut [Rc<Fit>], old: &str, new: Option<&TargetProfile>) {
        target_profile::retarget(fits, old, new);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_fields_round_trip() {
        let profile = TargetProfile {
            max_velocity: Some(1020.0),
            signature_radius: Some(37.0),
            radius: None,
            hp: None,
            ..TargetProfile::new("Enyo", [0.575, 0.724, 0.862, 0.235])
        };
        let texts: Vec<SharedString> = TargetProfiles::texts(&profile)
            .into_iter()
            .map(SharedString::from)
            .collect();
        assert_eq!(texts[0], "57.5");
        assert_eq!(texts[5], "37");
        assert_eq!(texts[7], "", "infinite HP shows blank");
        let back = TargetProfiles::read(&profile, &texts).expect("its own texts read back");
        assert!((back.resists[1] - 0.724).abs() < 1e-9);
        assert_eq!(back.signature_radius, Some(37.0));
        assert_eq!(back.radius, Some(0.0), "pyfa shows a missing radius as 0");
        assert_eq!(back.hp, None);
        let mut bad = texts.clone();
        bad[2] = "101".into();
        assert_eq!(
            TargetProfiles::read(&profile, &bad),
            Err("Incorrect range (must be 0-100)")
        );
        bad[2] = "x".into();
        assert_eq!(TargetProfiles::read(&profile, &bad), Err(BAD_NUMBER));
    }

    #[test]
    fn pattern_fields_show_shares() {
        let pattern = DamagePattern::new("Mine", [3.0, 1.0, 0.0, 0.0]);
        assert_eq!(DamagePatterns::note(&pattern, 0).as_deref(), Some("75.0%"));
        assert_eq!(DamagePatterns::texts(&pattern), ["3", "1", "0", "0"]);
        let texts: Vec<SharedString> = ["2.5", "", "1", "0"].map(SharedString::from).to_vec();
        let read = DamagePatterns::read(&pattern, &texts).expect("numbers read");
        assert_eq!(read.amounts, [2.5, 0.0, 1.0, 0.0]);
    }
}
