//! EVE's XML fitting files (service/port/xml.py), which the game exports and pyfa backs up all
//! fits to. Only the handful of elements fittings use is read, so a small tag scanner does.

use std::fmt::Write;

use super::{Bay, Imported, charge_count, escape_xml, new_fit, place, subsystem_flag};
use crate::{
    data::{Catalog, Kind, Slot},
    fit::Fit,
};

/// pyfa cuts descriptions to this many characters.
const DESCRIPTION_MAX: usize = 400;

/// Fits as one XML document, in pyfa's layout.
pub fn export(fits: &[&Fit], catalog: &Catalog) -> String {
    let mut out = String::from("<?xml version=\"1.0\" ?>\n");
    let _ = writeln!(out, "<fittings count=\"{}\">", fits.len());
    for fit in fits {
        let Some(ship) = fit.ship else { continue };
        let _ = writeln!(out, "\t<fitting name=\"{}\">", escape_xml(&fit.name));
        let notes = description(&fit.notes);
        let _ = writeln!(out, "\t\t<description value=\"{}\"/>", escape_xml(&notes));
        let _ = writeln!(
            out,
            "\t\t<shipType value=\"{}\"/>",
            escape_xml(&catalog.name(ship))
        );
        let mut charges: Vec<(String, u32)> = Vec::new();
        let mut add_charge =
            |name: String, count: u32| match charges.iter_mut().find(|(n, _)| *n == name) {
                Some((_, n)) => *n += count,
                None => charges.push((name, count)),
            };
        let mut hardware = |type_id: i32, slot: String| {
            let _ = writeln!(
                out,
                "\t\t<hardware slot=\"{slot}\" type=\"{}\"/>",
                escape_xml(&catalog.name(type_id))
            );
        };
        for (slot, label) in [(Slot::Low, "low"), (Slot::Mid, "med"), (Slot::High, "hi")] {
            for (n, m) in fit.rack(slot).iter().flatten().enumerate() {
                hardware(m.type_id, format!("{label} slot {n}"));
                if let Some(charge) = m.charge {
                    add_charge(
                        catalog.name(charge),
                        charge_count(catalog, m.type_id, charge),
                    );
                }
            }
        }
        for (n, &rig) in fit.rigs.iter().flatten().enumerate() {
            hardware(rig, format!("rig slot {n}"));
        }
        for &subsystem in &fit.subsystems {
            let n = subsystem_flag(catalog, subsystem) - 125;
            hardware(subsystem, format!("subsystem slot {n}"));
        }
        for d in &fit.drones {
            let _ = writeln!(
                out,
                "\t\t<hardware qty=\"{}\" slot=\"drone bay\" type=\"{}\"/>",
                d.count,
                escape_xml(&catalog.name(d.type_id))
            );
        }
        for &(type_id, count) in &fit.cargo {
            add_charge(catalog.name(type_id), count);
        }
        for (name, count) in charges {
            let _ = writeln!(
                out,
                "\t\t<hardware qty=\"{count}\" slot=\"cargo\" type=\"{}\"/>",
                escape_xml(&name)
            );
        }
        out.push_str("\t</fitting>\n");
    }
    out.push_str("</fittings>\n");
    out
}

/// Notes as the game's description: cut like pyfa does, line breaks as <br>.
fn description(notes: &str) -> String {
    let notes = if notes.chars().count() > DESCRIPTION_MAX {
        let kept: String = notes.chars().take(DESCRIPTION_MAX - 3).collect();
        format!("{kept}...")
    } else {
        notes.to_owned()
    };
    notes
        .split(['\r', '\n'])
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("<br>")
}

/// Every fitting in an XML document.
pub fn import(catalog: &Catalog, text: &str) -> Result<Imported, String> {
    let mut imported = Imported::default();
    let mut current: Option<Fit> = None;
    let mut name = String::new();
    let mut notes = String::new();
    // The ship comes after the description and before any hardware, but do not count on it.
    let mut pending: Vec<Tag> = Vec::new();
    for tag in tags(text) {
        match tag.name.as_str() {
            "fitting" => {
                name = tag.attr("name").unwrap_or_default();
                notes.clear();
                current = None;
                pending.clear();
            }
            "/fitting" => {
                if let Some(fit) = current.take() {
                    imported.fits.push(fit);
                }
            }
            "description" => notes = from_description(&tag.attr("value").unwrap_or_default()),
            "shipType" => {
                let ship_name = tag.attr("value").unwrap_or_default();
                match catalog.find(&ship_name).filter(|t| t.kind == Kind::Ship) {
                    Some(ship) => {
                        let mut fit = new_fit(catalog, ship.id, std::mem::take(&mut name));
                        fit.notes = std::mem::take(&mut notes);
                        for tag in pending.drain(..) {
                            hardware(&mut fit, catalog, &tag, &mut imported.unknown);
                        }
                        current = Some(fit);
                    }
                    None => imported.unknown.push(ship_name),
                }
            }
            "hardware" => match current.as_mut() {
                Some(fit) => hardware(fit, catalog, &tag, &mut imported.unknown),
                None => pending.push(tag),
            },
            _ => {}
        }
    }
    if imported.fits.is_empty() {
        return Err("no fittings in the XML".to_owned());
    }
    Ok(imported)
}

fn hardware(fit: &mut Fit, catalog: &Catalog, tag: &Tag, unknown: &mut Vec<String>) {
    let type_name = tag
        .attr("base_type")
        .or_else(|| tag.attr("type"))
        .unwrap_or_default();
    let Some(t) = catalog.find(&type_name) else {
        unknown.push(type_name);
        return;
    };
    let count = tag
        .attr("qty")
        .and_then(|q| q.trim().parse().ok())
        .unwrap_or(1);
    let slot = tag.attr("slot").unwrap_or_default().to_lowercase();
    let bay = if slot == "cargo" {
        Bay::Cargo
    } else {
        Bay::Fitted
    };
    place(fit, catalog, t.id, count, bay);
}

/// The game writes line breaks in descriptions as <br>, escaped inside the attribute.
fn from_description(value: &str) -> String {
    let mut text = value.to_owned();
    for br in ["<br>", "<BR>", "<br/>", "<br />"] {
        text = text.replace(br, "\n");
    }
    // Any other markup the game added goes.
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// A start, empty or end tag with its attributes; end tags are named "/name".
#[derive(Debug, Clone)]
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, key: &str) -> Option<String> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }
}

/// Element tags in document order, skipping the declaration, comments and text.
fn tags(text: &str) -> Vec<Tag> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        if let Some(comment) = rest.strip_prefix("!--") {
            rest = comment.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        let Some(end) = tag_end(rest) else { break };
        let body = &rest[..end];
        rest = &rest[end + 1..];
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        let body = body.trim_end_matches('/').trim();
        let (name, attrs) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        out.push(Tag {
            name: name.to_owned(),
            attrs: attributes(attrs),
        });
    }
    out
}

/// Where a tag closes, skipping '>' inside quoted attribute values.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn attributes(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_owned();
        let value_part = rest[eq + 1..].trim_start();
        let Some(quote) = value_part
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\''))
        else {
            break;
        };
        let value_part = &value_part[1..];
        let Some(close) = value_part.find(quote) else {
            break;
        };
        out.push((key, unescape(&value_part[..close])));
        rest = value_part[close + 1..].trim_start();
    }
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(semi) = rest.find(';') else { break };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[semi + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tags_attributes_and_entities() {
        let doc = r#"<?xml version="1.0" ?>
<!-- a comment with <tags> -->
<fittings count="1">
  <fitting name="Mine &amp; yours">
    <description value="one&lt;br&gt;two"/>
    <hardware qty="5" slot='drone bay' type="Hobgoblin II"/>
  </fitting>
</fittings>"#;
        let tags = tags(doc);
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "fittings",
                "fitting",
                "description",
                "hardware",
                "/fitting",
                "/fittings"
            ]
        );
        assert_eq!(tags[1].attr("name").as_deref(), Some("Mine & yours"));
        assert_eq!(
            from_description(&tags[2].attr("value").unwrap_or_default()),
            "one\ntwo"
        );
        assert_eq!(tags[3].attr("slot").as_deref(), Some("drone bay"));
    }

    #[test]
    fn descriptions_keep_lines() {
        assert_eq!(description("a\nb\r\nc"), "a<br>b<br>c");
        assert_eq!(from_description(&description("a\nb")), "a\nb");
    }
}
