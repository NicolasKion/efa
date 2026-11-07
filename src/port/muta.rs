//! pyfa's text for one mutated item (service/port/muta.py): the base item, the mutaplasmid and
//! the rolled attributes, each on a line of its own. EFT exports carry these blocks after the
//! fit, numbered, and the item lines point at them with " [n]".

use std::collections::HashMap;

use crate::{
    data::{Catalog, TypeId},
    fit::Mutation,
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
