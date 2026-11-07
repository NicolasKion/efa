//! Characters as pyfa keeps them: the read-only All 0 and All 5, and the user's own with their
//! skill levels, implants and security status. Written to their own JSON file next to the fits.

pub mod evemon;
pub mod requirements;

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    data::{Catalog, Kind, TypeId},
    engine::Pilot,
    fit::plug_implant,
};

pub use crate::engine::MAX_LEVEL;
/// Security status runs from -10 to 5.
pub const SEC_STATUS_RANGE: (f64, f64) = (-10.0, 5.0);

const ALL0_NAME: &str = "All 0";
const ALL5_NAME: &str = "All 5";

/// Which character flies a fit. Fits saved before characters existed fly All 5.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum CharacterId {
    All0,
    #[default]
    All5,
    User(u32),
}

impl CharacterId {
    /// All 0 and All 5 cannot be edited, renamed or deleted.
    pub fn is_builtin(self) -> bool {
        !matches!(self, Self::User(_))
    }
}

/// How a level change is kept, pyfa's `persist` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Saved right away, as the character editor's level menu does.
    Saved,
    /// Pending until the character is saved or reverted, as imports and fit menus do.
    Pending,
}

/// A user's character.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Character {
    pub id: u32,
    pub name: String,
    /// Saved levels; a skill missing here is not learned.
    pub skills: BTreeMap<TypeId, u8>,
    /// Plugged-in implants, in slot order.
    pub implants: Vec<TypeId>,
    pub sec_status: f64,
    /// The EVE character "Get Skills" reads from, pyfa's linked SSO character.
    pub esi_character: Option<u64>,
    /// The Alpha clone grade capping the skills, pyfa's `alphaCloneID`; None for Omega.
    pub alpha_clone: Option<i32>,
    /// Level changes not saved yet, pyfa's dirty skills; None unlearns. Never written out, so
    /// they are gone after a restart, as in pyfa.
    #[serde(skip)]
    pub pending: BTreeMap<TypeId, Option<u8>>,
}

impl Character {
    /// The level the character flies with: pending if changed, otherwise saved.
    pub fn level(&self, skill: TypeId) -> Option<u8> {
        match self.pending.get(&skill) {
            Some(&level) => level,
            None => self.skills.get(&skill).copied(),
        }
    }

    fn write(&mut self, skill: TypeId, level: Option<u8>, keep: Keep) {
        let saved = self.skills.get(&skill).copied();
        match keep {
            Keep::Saved => {
                self.pending.remove(&skill);
                match level {
                    Some(level) => self.skills.insert(skill, level),
                    None => self.skills.remove(&skill),
                };
            }
            Keep::Pending if saved == level => {
                self.pending.remove(&skill);
            }
            Keep::Pending => {
                self.pending.insert(skill, level);
            }
        }
    }

    /// Levels with pending changes applied.
    fn effective_skills(&self) -> BTreeMap<TypeId, u8> {
        let mut skills = self.skills.clone();
        for (&skill, &level) in &self.pending {
            match level {
                Some(level) => skills.insert(skill, level),
                None => skills.remove(&skill),
            };
        }
        skills
    }
}

/// Every user character, kept in `characters.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Characters {
    pub characters: Vec<Character>,
    /// The highest ID ever handed out. IDs are not reused, so fits of a deleted character fall
    /// back to All 5 instead of flying whoever is made next.
    pub last_id: u32,
}

impl Characters {
    /// What was saved last time, or none if there is nothing readable.
    pub fn load() -> Self {
        path().map_or_else(Self::default, |path| crate::persist::load(&path))
    }

    /// Write the characters out, keeping the previous file as a backup.
    pub fn save(&self) -> Result<(), String> {
        crate::persist::save(&path().ok_or("no home directory")?, self)
    }

    pub fn get(&self, id: CharacterId) -> Option<&Character> {
        let CharacterId::User(id) = id else {
            return None;
        };
        self.characters.iter().find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: CharacterId) -> Option<&mut Character> {
        let CharacterId::User(id) = id else {
            return None;
        };
        self.characters.iter_mut().find(|c| c.id == id)
    }

    /// The character, or All 5 when it was deleted, which is what pyfa falls back to.
    pub fn resolve(&self, id: CharacterId) -> CharacterId {
        match id {
            CharacterId::User(_) if self.get(id).is_none() => CharacterId::All5,
            id => id,
        }
    }

    pub fn name(&self, id: CharacterId) -> String {
        match self.resolve(id) {
            CharacterId::All0 => ALL0_NAME.to_owned(),
            CharacterId::All5 => ALL5_NAME.to_owned(),
            id @ CharacterId::User(_) => self.get(id).map_or_else(String::new, |c| c.name.clone()),
        }
    }

    /// pyfa's order: the built-ins first, then the user's by name.
    pub fn ordered(&self) -> Vec<CharacterId> {
        let mut users: Vec<&Character> = self.characters.iter().collect();
        users.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        [CharacterId::All0, CharacterId::All5]
            .into_iter()
            .chain(users.into_iter().map(|c| CharacterId::User(c.id)))
            .collect()
    }

    /// The level a character has a skill at; None when not learned.
    pub fn level(&self, id: CharacterId, skill: TypeId) -> Option<u8> {
        match self.resolve(id) {
            CharacterId::All0 => Some(0),
            CharacterId::All5 => Some(MAX_LEVEL),
            id @ CharacterId::User(_) => self.get(id).and_then(|c| c.level(skill)),
        }
    }

    /// The level for comparing against requirements, where not learned counts as 0.
    pub fn level_or_zero(&self, id: CharacterId, skill: TypeId) -> u8 {
        self.level(id, skill).unwrap_or(0)
    }

    /// The character has level changes that are not saved.
    pub fn is_dirty(&self, id: CharacterId) -> bool {
        self.get(id).is_some_and(|c| !c.pending.is_empty())
    }

    pub fn is_skill_dirty(&self, id: CharacterId, skill: TypeId) -> bool {
        self.get(id).is_some_and(|c| c.pending.contains_key(&skill))
    }

    pub fn implants(&self, id: CharacterId) -> &[TypeId] {
        self.get(id).map_or(&[], |c| c.implants.as_slice())
    }

    pub fn sec_status(&self, id: CharacterId) -> f64 {
        self.get(id).map_or(0.0, |c| c.sec_status)
    }

    /// The Alpha clone grade the character flies as, None for Omega and the built-ins.
    pub fn alpha_clone(&self, id: CharacterId) -> Option<i32> {
        self.get(id).and_then(|c| c.alpha_clone)
    }

    /// Fly as an Alpha clone of that grade, or Omega with None. The trained levels stay; the
    /// clone only caps them, as in pyfa.
    pub fn set_alpha_clone(&mut self, id: CharacterId, clone: Option<i32>) {
        if let Some(c) = self.get_mut(id) {
            c.alpha_clone = clone;
        }
    }

    /// The level a skill is flown at: [`Characters::level`] capped by the Alpha clone, the
    /// level pyfa shows and checks requirements against. None when not learned.
    pub fn flown_level(&self, id: CharacterId, skill: TypeId, catalog: &Catalog) -> Option<u8> {
        let level = self.level(id, skill)?;
        let cap = self
            .alpha_clone(id)
            .and_then(|clone| catalog.alpha_clone(clone))
            .map(|clone| clone.caps.get(&skill).copied().unwrap_or(0));
        Some(cap.map_or(level, |cap| level.min(cap)))
    }

    /// [`Characters::flown_level`] where not learned counts as 0, for requirements.
    pub fn flown_level_or_zero(&self, id: CharacterId, skill: TypeId, catalog: &Catalog) -> u8 {
        self.flown_level(id, skill, catalog).unwrap_or(0)
    }

    /// The character as the engine flies it, with every skill the catalog knows for the
    /// built-ins.
    pub fn pilot(&self, id: CharacterId, catalog: &Catalog) -> Pilot {
        let skills = &catalog.skills;
        match self.resolve(id) {
            CharacterId::All0 => Pilot::all(skills, 0),
            CharacterId::All5 => Pilot::all(skills, MAX_LEVEL),
            id @ CharacterId::User(_) => {
                let Some(c) = self.get(id) else {
                    return Pilot::all(skills, MAX_LEVEL);
                };
                Pilot {
                    skills: c.effective_skills().into_iter().collect(),
                    implants: c.implants.clone(),
                    sec_status: c.sec_status,
                    alpha_caps: c
                        .alpha_clone
                        .and_then(|clone| catalog.alpha_clone(clone))
                        .map(|clone| clone.caps.clone()),
                }
            }
        }
    }

    /// Why a name cannot name a character, in pyfa's words, or None if it can.
    pub fn name_problem(&self, name: &str) -> Option<&'static str> {
        let name = name.trim();
        if name.is_empty() {
            Some("You must supply a name for the Character!")
        } else if name == ALL0_NAME
            || name == ALL5_NAME
            || self.characters.iter().any(|c| c.name == name)
        {
            Some("Character name already in use, please choose another.")
        } else {
            None
        }
    }

    fn next_id(&self) -> u32 {
        let highest = self.characters.iter().map(|c| c.id).max().unwrap_or(0);
        highest.max(self.last_id) + 1
    }

    /// Add a character under a fresh ID, e.g. one read from an `EVEMon` sheet.
    pub fn add(&mut self, mut character: Character) -> CharacterId {
        character.id = self.next_id();
        self.last_id = character.id;
        let id = CharacterId::User(character.id);
        self.characters.push(character);
        id
    }

    /// A new character with no skills learned, as pyfa makes them.
    pub fn create(&mut self, name: &str) -> CharacterId {
        self.add(Character {
            name: name.trim().to_owned(),
            ..Character::default()
        })
    }

    pub fn rename(&mut self, id: CharacterId, name: &str) {
        if let Some(c) = self.get_mut(id) {
            name.trim().clone_into(&mut c.name);
        }
    }

    /// A copy under a new name. Copying a built-in gives an editable character with its levels.
    pub fn copy(&mut self, id: CharacterId, name: &str, catalog: &Catalog) -> CharacterId {
        let mut copy = match self.resolve(id) {
            CharacterId::All0 => builtin(catalog, 0),
            CharacterId::All5 => builtin(catalog, MAX_LEVEL),
            id @ CharacterId::User(_) => self.get(id).cloned().unwrap_or_default(),
        };
        name.trim().clone_into(&mut copy.name);
        self.add(copy)
    }

    pub fn delete(&mut self, id: CharacterId) {
        if let CharacterId::User(id) = id {
            self.characters.retain(|c| c.id != id);
        }
    }

    /// [`Characters::change_level_with`] with strict skill levels, pyfa's default.
    pub fn change_level(
        &mut self,
        id: CharacterId,
        skill: TypeId,
        level: Option<u8>,
        keep: Keep,
        catalog: &Catalog,
    ) {
        self.change_level_with(id, skill, level, keep, true, catalog);
    }

    /// Set a skill's level the way pyfa's `changeLevel` does: prerequisites are trained up to
    /// what it needs, and with `strict` skill levels skills that need it higher are unlearned.
    /// None, or a level past V, unlearns it. Built-in characters do not change.
    pub fn change_level_with(
        &mut self,
        id: CharacterId,
        skill: TypeId,
        level: Option<u8>,
        keep: Keep,
        strict: bool,
        catalog: &Catalog,
    ) {
        let Some(c) = self.get_mut(id) else { return };
        let set = Setter {
            keep,
            strict,
            catalog,
        };
        match level.filter(|&l| l <= MAX_LEVEL) {
            None => set.level(c, skill, None),
            // Like pyfa, not learned counts as 0 here, so Level 0 leaves such a skill alone.
            Some(level) if c.level(skill).unwrap_or(0) != level => {
                set.prerequisites(c, skill);
                set.level(c, skill, Some(level));
            }
            Some(_) => {}
        }
    }

    /// Raise a skill to `level` unless it is higher already, for Grant Missing Skills.
    pub fn raise_level(&mut self, id: CharacterId, skill: TypeId, level: u8, catalog: &Catalog) {
        if self.level_or_zero(id, skill) <= level {
            self.change_level(id, skill, Some(level), Keep::Pending, catalog);
        }
    }

    /// Make pending changes the saved levels.
    pub fn save_levels(&mut self, id: CharacterId) {
        if let Some(c) = self.get_mut(id) {
            let skills = c.effective_skills();
            c.skills = skills;
            c.pending.clear();
        }
    }

    pub fn revert_levels(&mut self, id: CharacterId) {
        if let Some(c) = self.get_mut(id) {
            c.pending.clear();
        }
    }

    /// Save the levels as they are now as a new character and revert the old one, pyfa's
    /// Save Character As.
    pub fn save_as(&mut self, id: CharacterId, name: &str, catalog: &Catalog) -> CharacterId {
        let new = self.copy(id, name, catalog);
        self.save_levels(new);
        self.revert_levels(id);
        new
    }

    pub fn set_sec_status(&mut self, id: CharacterId, sec_status: f64) {
        if let Some(c) = self.get_mut(id) {
            c.sec_status = sec_status.clamp(SEC_STATUS_RANGE.0, SEC_STATUS_RANGE.1);
        }
    }

    /// The EVE character linked for "Get Skills", if any.
    pub fn esi_character(&self, id: CharacterId) -> Option<u64> {
        self.get(id).and_then(|c| c.esi_character)
    }

    /// Link a character to an EVE character, or unlink it with None.
    pub fn link_esi(&mut self, id: CharacterId, esi_character: Option<u64>) {
        if let Some(c) = self.get_mut(id) {
            c.esi_character = esi_character;
        }
    }

    /// Take what ESI says about the character, the way pyfa's Get Skills does: the trained
    /// levels replace every saved and pending one, and implants and security status are taken
    /// when they came along.
    pub fn apply_esi(
        &mut self,
        id: CharacterId,
        skills: &[(TypeId, u8)],
        implants: Option<&[TypeId]>,
        sec_status: Option<f64>,
        catalog: &Catalog,
    ) {
        let Some(c) = self.get_mut(id) else { return };
        c.pending.clear();
        c.skills = skills
            .iter()
            .map(|&(skill, level)| (skill, level.min(MAX_LEVEL)))
            .collect();
        if let Some(implants) = implants {
            c.implants.clear();
            for &implant in implants {
                plug_implant(&mut c.implants, catalog, implant);
            }
        }
        if let Some(sec) = sec_status {
            c.sec_status = sec.clamp(SEC_STATUS_RANGE.0, SEC_STATUS_RANGE.1);
        }
    }

    /// Plug in an implant, replacing the one in its slot. Returns whether anything changed.
    pub fn add_implant(&mut self, id: CharacterId, catalog: &Catalog, implant: TypeId) -> bool {
        self.get_mut(id)
            .is_some_and(|c| plug_implant(&mut c.implants, catalog, implant))
    }

    pub fn remove_implant(&mut self, id: CharacterId, implant: TypeId) {
        if let Some(c) = self.get_mut(id) {
            c.implants.retain(|&t| t != implant);
        }
    }

    /// Every skill as "Name level", by name, pyfa's "Export skills to clipboard". Skills not
    /// learned export as 0.
    pub fn export_skills(&self, id: CharacterId, catalog: &Catalog) -> String {
        let mut lines: Vec<String> = catalog
            .skills
            .iter()
            .map(|&s| format!("{} {}", catalog.name(s), self.level_or_zero(id, s)))
            .collect();
        lines.sort();
        lines.into_iter().map(|l| l + "\n").collect()
    }

    /// Set levels from "Name level" lines, levels in digits or Roman numerals, as pending
    /// changes. Returns how many lines it could not read.
    pub fn import_skills(
        &mut self,
        id: CharacterId,
        text: &str,
        catalog: &Catalog,
    ) -> Result<usize, String> {
        if id.is_builtin() {
            return Err("Built-in characters cannot be changed".to_owned());
        }
        let mut errors = 0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let parsed = line
                .rsplit_once(char::is_whitespace)
                .and_then(|(name, level)| {
                    let skill = catalog
                        .find(name.trim())
                        .filter(|t| t.kind == Kind::Skill)?;
                    Some((skill.id, parse_level(level)?))
                });
            match parsed {
                Some((skill, level)) => {
                    self.change_level(id, skill, Some(level), Keep::Pending, catalog);
                }
                None => errors += 1,
            }
        }
        Ok(errors)
    }
}

/// A built-in's levels as a user character, for copying it.
fn builtin(catalog: &Catalog, level: u8) -> Character {
    Character {
        skills: catalog.skills.iter().map(|&s| (s, level)).collect(),
        ..Character::default()
    }
}

/// How a level change goes about it.
struct Setter<'a> {
    keep: Keep,
    /// pyfa's "Enforce strict skill level requirements".
    strict: bool,
    catalog: &'a Catalog,
}

impl Setter<'_> {
    /// pyfa's Skill.setLevel: with strict skill levels, a skill that needs this one higher than
    /// its new level is unlearned, and so on down.
    fn level(&self, c: &mut Character, skill: TypeId, level: Option<u8>) {
        c.write(skill, level, self.keep);
        if !self.strict {
            return;
        }
        for (dependent, needed) in dependents(self.catalog, skill) {
            if level.is_none_or(|l| l < needed) {
                self.level(c, dependent, None);
            }
        }
    }

    /// pyfa's _trainSkillReqs: raise each prerequisite to what the skill needs, and theirs in
    /// turn.
    fn prerequisites(&self, c: &mut Character, skill: TypeId) {
        let Some(required) = self.catalog.required_skills.get(&skill) else {
            return;
        };
        for &(prerequisite, needed) in required {
            if c.level(prerequisite).unwrap_or(0) < needed {
                self.level(c, prerequisite, Some(needed));
                self.prerequisites(c, prerequisite);
            }
        }
    }
}

/// Skills that need `skill`, and the level they need it at.
fn dependents(catalog: &Catalog, skill: TypeId) -> Vec<(TypeId, u8)> {
    catalog
        .skills
        .iter()
        .filter_map(|&s| {
            let needed = catalog
                .required_skills
                .get(&s)?
                .iter()
                .find(|&&(r, _)| r == skill)?
                .1;
            Some((s, needed))
        })
        .collect()
}

/// A skill level in digits or Roman numerals, 0 to V.
pub fn parse_level(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Ok(level) = text.parse::<u8>() {
        return (level <= MAX_LEVEL).then_some(level);
    }
    ROMAN
        .iter()
        .position(|r| r.eq_ignore_ascii_case(text))
        .map(|i| i as u8 + 1)
}

/// Levels I to V.
pub const ROMAN: [&str; 5] = ["I", "II", "III", "IV", "V"];

/// A level in Roman numerals, as `EVEMon` writes them; 0 stays a digit.
pub fn roman(level: u8) -> String {
    match level {
        1..=MAX_LEVEL => ROMAN[usize::from(level) - 1].to_owned(),
        _ => level.to_string(),
    }
}

/// Where the characters are kept.
pub fn path() -> Option<PathBuf> {
    crate::paths::data_file("characters.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_read_in_digits_and_roman() {
        assert_eq!(parse_level("4"), Some(4));
        assert_eq!(parse_level("IV"), Some(4));
        assert_eq!(parse_level("v"), Some(5));
        assert_eq!(parse_level("6"), None);
        assert_eq!(parse_level("VI"), None);
        assert_eq!(roman(3), "III");
        assert_eq!(roman(0), "0");
    }

    #[test]
    fn pending_changes_save_and_revert() {
        let mut c = Character::default();
        c.write(1, Some(3), Keep::Saved);
        c.write(1, Some(5), Keep::Pending);
        assert_eq!(c.level(1), Some(5));
        assert_eq!(c.skills.get(&1), Some(&3));
        // Back to the saved level is no change at all.
        c.write(1, Some(3), Keep::Pending);
        assert!(c.pending.is_empty());
        c.write(2, None, Keep::Pending);
        assert!(c.pending.is_empty(), "unlearning a skill never learned");
        c.write(1, None, Keep::Pending);
        assert_eq!(c.level(1), None);
        assert_eq!(c.effective_skills().len(), 0);
    }

    #[test]
    fn deleted_characters_fall_back_to_all5() {
        let mut characters = Characters::default();
        let id = characters.create("Pilot");
        assert_eq!(characters.resolve(id), id);
        assert_eq!(characters.name(id), "Pilot");
        characters.delete(id);
        assert_eq!(characters.resolve(id), CharacterId::All5);
        assert_eq!(characters.level(id, 3300), Some(MAX_LEVEL));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut characters = Characters::default();
        let first = characters.create("First");
        characters.delete(first);
        let second = characters.create("Second");
        assert_ne!(first, second);
    }

    #[test]
    fn names_must_be_new() {
        let mut characters = Characters::default();
        characters.create("Pilot");
        assert!(characters.name_problem("").is_some());
        assert!(characters.name_problem("All 5").is_some());
        assert!(characters.name_problem(" Pilot ").is_some());
        assert!(characters.name_problem("Other").is_none());
    }

    #[test]
    fn builtins_list_first_and_cannot_change() {
        let mut characters = Characters::default();
        let b = characters.create("B");
        let a = characters.create("A");
        assert_eq!(
            characters.ordered(),
            [CharacterId::All0, CharacterId::All5, a, b]
        );
        characters.rename(CharacterId::All5, "Mine");
        assert_eq!(characters.name(CharacterId::All5), "All 5");
    }

    #[test]
    fn fits_without_a_character_fly_all5() {
        #[derive(Deserialize)]
        struct Old {
            #[serde(default)]
            character: CharacterId,
        }
        let old: Old = serde_json::from_str("{}").expect("parses");
        assert_eq!(old.character, CharacterId::All5);
        let json = serde_json::to_string(&CharacterId::User(7)).expect("serializes");
        let back: CharacterId = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, CharacterId::User(7));
    }
}
