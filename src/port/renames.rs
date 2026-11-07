//! Items CCP renamed, from pyfa's service/conversions: old fits name them the old way, and the
//! name lookup maps them to today's names before searching. `scripts/renames.py` writes the
//! table from a pyfa checkout.

use std::{collections::HashMap, sync::LazyLock};

const TABLE: &str = include_str!("renames.tsv");

/// Renames chain when an item was renamed twice; more hops than this is a loop.
const MAX_HOPS: usize = 8;

/// Old name to current name, both lowercase, with chains followed to the end.
static RENAMES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let direct: HashMap<String, String> = TABLE
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(old, new)| (old.trim().to_lowercase(), new.trim().to_lowercase()))
        .collect();
    direct
        .iter()
        .map(|(old, new)| {
            let mut current = new;
            for _ in 0..MAX_HOPS {
                match direct.get(current) {
                    Some(next) if next != old => current = next,
                    _ => break,
                }
            }
            (old.clone(), current.clone())
        })
        .collect()
});

/// The current name, in lowercase, of an item known by a lowercase old name.
pub fn current_name(old: &str) -> Option<&'static str> {
    RENAMES.get(old).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_names_map_to_current_ones() {
        assert_eq!(
            current_name("adaptive invulnerability field ii"),
            Some("adaptive invulnerability shield hardener ii")
        );
        assert_eq!(current_name("125mm gatling autocannon ii"), None);
    }
}
