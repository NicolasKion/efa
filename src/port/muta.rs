//! pyfa's text for one mutated item (service/port/muta.py): the base item, the mutaplasmid and
//! the rolled attributes, each on a line of its own. EFT exports carry these blocks after the
//! fit, numbered, and the item lines point at them with " [n]".

use std::collections::HashMap;

use serde::Deserialize;

use crate::{
    data::{Catalog, TypeId},
    fit::Mutation,
    net::{COMPATIBILITY_DATE, ESI_BASE, Http},
};

/// pyfa's `renderMutant`: base item, mutaplasmid and attributes, the first line after
/// `first_prefix` and the others after `prefix`.
pub fn render(catalog: &Catalog, mutation: &Mutation, first_prefix: &str, prefix: &str) -> String {
    format!(
        "{first_prefix}{}\n{prefix}{}\n{prefix}{}",
        catalog.name(mutation.base),
        catalog.name(mutation.mutaplasmid),
        mutation.render_attrs(catalog)
    )
}

/// A mutated item read from text: the base item, and the mutaplasmid with its rolls if those
/// were there and known.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub base: TypeId,
    pub mutaplasmid: Option<TypeId>,
    /// Rolled values by attribute ID.
    pub attrs: Vec<(i32, f64)>,
}

impl Parsed {
    /// The abyssal type and the mutation, when the mutaplasmid takes the base item.
    pub fn mutation(&self, catalog: &Catalog) -> Option<(TypeId, Mutation)> {
        Mutation::with_values(
            catalog,
            self.base,
            self.mutaplasmid?,
            self.attrs.iter().copied(),
        )
    }
}

/// pyfa's `parseMutant`: up to three lines, base item, mutaplasmid, attributes. None when the
/// first line is no item.
pub fn parse(catalog: &Catalog, lines: &[&str]) -> Option<Parsed> {
    let base = catalog.find(lines.first()?.trim())?.id;
    let mutaplasmid = lines
        .get(1)
        .and_then(|name| catalog.find(name.trim()))
        .map(|t| t.id)
        .filter(|&id| catalog.mutaplasmids.get(id).is_some());
    let attrs = match (mutaplasmid, lines.get(2)) {
        (Some(_), Some(line)) => parse_attrs(catalog, line),
        _ => Vec::new(),
    };
    Some(Parsed {
        base,
        mutaplasmid,
        attrs,
    })
}

/// pyfa's `parseMutantAttrs`: "name value" pairs split by commas; anything else is skipped.
pub fn parse_attrs(catalog: &Catalog, line: &str) -> Vec<(i32, f64)> {
    line.split(',')
        .filter_map(|pair| {
            let mut words = pair.split_whitespace();
            let (name, value, None) = (words.next()?, words.next()?, words.next()) else {
                return None;
            };
            let value: f64 = value.parse().ok()?;
            Some((catalog.mutaplasmids.attr_by_name(name)?, value))
        })
        .collect()
}

/// Text that is one mutated item and nothing else, which pyfa's import fits into the current
/// fit. It must name a mutaplasmid, or a plain module name would pass for one.
pub fn parse_single(catalog: &Catalog, text: &str) -> Option<Parsed> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if !(2..=3).contains(&lines.len()) {
        return None;
    }
    parse(catalog, &lines).filter(|p| p.mutaplasmid.is_some())
}

/// The item link the game client copies, `<url=showinfo:TYPE//ITEM>Name</url>`, as its type and
/// item IDs (pyfa's `parseDynamicItemString`).
pub fn parse_item_link(text: &str) -> Option<(TypeId, i64)> {
    const OPEN: &str = "<url=showinfo:";
    let rest = &text[text.find(OPEN)? + OPEN.len()..];
    let (type_id, rest) = rest.split_once("//")?;
    let (item_id, rest) = rest.split_once('>')?;
    let closed = rest.find("</url>").is_some_and(|end| end > 0);
    closed.then_some(())?;
    Some((type_id.parse().ok()?, item_id.parse().ok()?))
}

#[derive(Deserialize)]
struct DynamicItem {
    source_type_id: TypeId,
    mutator_type_id: TypeId,
    dogma_attributes: Vec<DynamicAttr>,
}

#[derive(Deserialize)]
struct DynamicAttr {
    attribute_id: i32,
    value: f64,
}

/// What a mutated item in the game is, from ESI's public dynamic item route (pyfa's
/// `fetchDynamicItem`): the base item, the mutaplasmid and every attribute's value.
pub fn fetch_item(http: &dyn Http, type_id: TypeId, item_id: i64) -> Result<Parsed, String> {
    let url = format!("{ESI_BASE}/dogma/dynamic/items/{type_id}/{item_id}");
    let reply = http.get(&url, &[("X-Compatibility-Date", COMPATIBILITY_DATE)])?;
    if reply.status == 404 || reply.status == 400 {
        return Err("the game does not know it as a mutated item".to_owned());
    }
    let reply = reply.ok(&url)?;
    let item: DynamicItem =
        serde_json::from_slice(&reply.body).map_err(|e| format!("{url}: {e}"))?;
    Ok(Parsed {
        base: item.source_type_id,
        mutaplasmid: Some(item.mutator_type_id),
        attrs: item
            .dogma_attributes
            .into_iter()
            .map(|a| (a.attribute_id, a.value))
            .collect(),
    })
}

/// pyfa's `importGetMutationData`: take the "[n] Base" blocks, with the lines that follow each up
/// to a blank one, out of `lines`, and read them.
pub fn take_blocks(catalog: &Catalog, lines: &mut Vec<String>) -> HashMap<u32, Parsed> {
    let mut blocks: Vec<(u32, Vec<String>)> = Vec::new();
    let mut current: Option<usize> = None;
    let mut kept = Vec::with_capacity(lines.len());
    for line in lines.drain(..) {
        let trimmed = line.trim();
        if let Some((reference, tail)) = block_header(trimmed) {
            blocks.push((reference, vec![tail.to_owned()]));
            current = Some(blocks.len() - 1);
        } else if trimmed.is_empty() {
            current = None;
            kept.push(line);
        } else if let Some(at) = current {
            blocks[at].1.push(trimmed.to_owned());
        } else {
            kept.push(line);
        }
    }
    *lines = kept;
    blocks
        .into_iter()
        .filter_map(|(reference, block)| {
            let block: Vec<&str> = block.iter().map(String::as_str).collect();
            Some((reference, parse(catalog, &block)?))
        })
        .collect()
}

/// "[3] Stasis Webifier II" -> (3, "Stasis Webifier II").
fn block_header(line: &str) -> Option<(u32, &str)> {
    let (reference, tail) = line.strip_prefix('[')?.split_once(']')?;
    Some((reference.parse().ok()?, tail.trim()))
}

/// An item line's " [n]" pointer to its mutation block, split off.
pub fn split_reference(line: &str) -> (&str, Option<u32>) {
    let trimmed = line.trim_end();
    if let Some(open) = trimmed.strip_suffix(']').and_then(|l| l.rfind('['))
        && let Ok(reference) = trimmed[open + 1..trimmed.len() - 1].parse()
    {
        return (trimmed[..open].trim_end(), Some(reference));
    }
    (line, None)
}

/// The mutation blocks of an export, numbered from one in the order items point at them.
#[derive(Debug, Default)]
pub struct Blocks {
    blocks: Vec<String>,
}

impl Blocks {
    /// Note a mutated item and give the " [n]" its line ends with.
    pub fn add(&mut self, catalog: &Catalog, mutation: &Mutation) -> String {
        let reference = self.blocks.len() + 1;
        self.blocks
            .push(render(catalog, mutation, &format!("[{reference}] "), "  "));
        format!(" [{reference}]")
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// The blocks, one after another, as pyfa's `formatMutants` writes them.
    pub fn render(&self) -> String {
        self.blocks.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::Reply;

    #[test]
    fn item_links_read_like_pyfa() {
        let link = "<url=showinfo:47820//1043563011234>Unstable Large Shield Extender</url>";
        assert_eq!(parse_item_link(link), Some((47_820, 1_043_563_011_234)));
        assert_eq!(
            parse_item_link(&format!("Look: {link} nice")),
            Some((47_820, 1_043_563_011_234))
        );
        assert_eq!(parse_item_link("<url=showinfo:47820//1>"), None, "unclosed");
        assert_eq!(
            parse_item_link("<url=showinfo:587>Rifter</url>"),
            None,
            "no item ID"
        );
        assert_eq!(parse_item_link("Large Shield Extender II"), None);
    }

    struct Esi(u16, &'static str);

    impl Http for Esi {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, String> {
            assert_eq!(url, "https://esi.evetech.net/dogma/dynamic/items/47820/42");
            assert!(headers.iter().any(|&(k, _)| k == "X-Compatibility-Date"));
            Ok(Reply {
                status: self.0,
                body: self.1.as_bytes().to_vec(),
                ..Reply::default()
            })
        }
    }

    #[test]
    fn dynamic_items_come_from_esi() {
        let body = r#"{"created_by": 1, "dogma_attributes": [{"attribute_id": 72, "value": 1650.5}],
            "dogma_effects": [], "mutator_type_id": 47819, "source_type_id": 3841}"#;
        let parsed = fetch_item(&Esi(200, body), 47_820, 42).expect("fetched");
        assert_eq!(
            parsed,
            Parsed {
                base: 3841,
                mutaplasmid: Some(47_819),
                attrs: vec![(72, 1650.5)],
            }
        );
        assert!(fetch_item(&Esi(404, "{}"), 47_820, 42).is_err());
    }

    #[test]
    fn references_split_off() {
        assert_eq!(
            split_reference("Stasis Webifier II [2]"),
            ("Stasis Webifier II", Some(2))
        );
        assert_eq!(
            split_reference("Hobgoblin II x5 [1]"),
            ("Hobgoblin II x5", Some(1))
        );
        assert_eq!(
            split_reference("[Empty High slot]"),
            ("[Empty High slot]", None)
        );
        assert_eq!(
            split_reference("Stasis Webifier II"),
            ("Stasis Webifier II", None)
        );
    }

    #[test]
    fn block_headers_need_a_number() {
        assert_eq!(
            block_header("[1] Stasis Webifier II"),
            Some((1, "Stasis Webifier II"))
        );
        assert_eq!(block_header("[Empty Med slot]"), None);
        assert_eq!(block_header("[Rifter, Fit]"), None);
    }
}
