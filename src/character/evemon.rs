//! `EVEMon` files: character sheets to import characters from, and skill plans to export what a fit
//! needs (pyfa's importCharacter and exportSkillsNeeded).

use std::{collections::BTreeMap, io::Write, path::Path};

use crate::data::{Catalog, Kind, TypeId};

use super::{Character, MAX_LEVEL, SEC_STATUS_RANGE};

/// Root elements of the character sheets `EVEMon` saves.
const SHEET_ROOTS: [&str; 2] = ["SerializableCCPCharacter", "SerializableUriCharacter"];

/// Read an `EVEMon` character sheet into a new character named `"<name> (EVEMon)"`. Skills gyfa
/// does not know or with impossible levels are left out, as pyfa does.
pub fn import_sheet(xml: &str, catalog: &Catalog) -> Result<Character, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    if !SHEET_ROOTS.contains(&root.tag_name().name()) {
        return Err("not an EVEMon character sheet".to_owned());
    }
    let text = |tag: &str| {
        root.descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(str::trim)
    };
    let name = text("name").ok_or("the sheet has no name")?;
    let sec_status = text("securityStatus")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
        .clamp(SEC_STATUS_RANGE.0, SEC_STATUS_RANGE.1);
    let skills: BTreeMap<TypeId, u8> = root
        .descendants()
        .filter(|n| n.has_tag_name("skill"))
        .filter_map(|n| {
            let skill: TypeId = n.attribute("typeID")?.parse().ok()?;
            let level: u8 = n.attribute("level")?.parse().ok()?;
            let known = catalog.get(skill).is_some_and(|t| t.kind == Kind::Skill);
            (known && level <= MAX_LEVEL).then_some((skill, level))
        })
        .collect();
    Ok(Character {
        name: format!("{name} (EVEMon)"),
        skills,
        sec_status,
        ..Character::default()
    })
}

/// Formats "Export Skills Needed" writes, picked by the file's extension as pyfa's save dialog
/// picks them by filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanFormat {
    /// `EVEMon`'s gzipped plan, `.emp`.
    Emp,
    /// `EVEMon`'s plan as plain XML.
    Xml,
    Text,
}

impl PlanFormat {
    /// The format for a path; `EVEMon`'s own `.emp` unless the extension says otherwise.
    pub fn for_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("xml") => Self::Xml,
            Some("txt") => Self::Text,
            _ => Self::Emp,
        }
    }
}

/// The bytes of a plan file. `xml` and `text` are the plan in both forms.
pub fn plan_file(format: PlanFormat, xml: &str, text: &str) -> Result<Vec<u8>, String> {
    match format {
        PlanFormat::Text => Ok(text.as_bytes().to_vec()),
        PlanFormat::Xml => Ok(xml.as_bytes().to_vec()),
        PlanFormat::Emp => {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
            gz.finish().map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_files_that_are_not_sheets() {
        let catalog = Catalog::default();
        assert!(import_sheet("<plan/>", &catalog).is_err());
        assert!(import_sheet("not xml", &catalog).is_err());
    }

    #[test]
    fn reads_name_and_security_status() {
        let catalog = Catalog::default();
        let sheet = "<SerializableCCPCharacter><name>Pilot &amp; Co</name>\
            <securityStatus>-2.5</securityStatus><skills>\
            <skill typeID=\"3300\" level=\"5\" name=\"Gunnery\"/></skills>\
            </SerializableCCPCharacter>";
        let c = import_sheet(sheet, &catalog).expect("a sheet");
        assert_eq!(c.name, "Pilot & Co (EVEMon)");
        assert!((c.sec_status + 2.5).abs() < f64::EPSILON);
        // The empty catalog knows no skills.
        assert!(c.skills.is_empty());
    }

    #[test]
    fn plan_format_follows_the_extension() {
        assert_eq!(PlanFormat::for_path(Path::new("a.XML")), PlanFormat::Xml);
        assert_eq!(PlanFormat::for_path(Path::new("a.txt")), PlanFormat::Text);
        assert_eq!(PlanFormat::for_path(Path::new("a")), PlanFormat::Emp);
    }
}
