//! What damage patterns and target profiles share: pyfa's raw names with `[Category]` prefixes,
//! its natural sort, and the category tree its menus are built from.

use std::cmp::Ordering;

/// Categories and short name of a raw name, like pyfa's `__parseRawName`.
///
/// Each `[Category]` prefix is a menu level, and a `|` right before a `[` stops the parsing so
/// names like `[T2] Aurora` keep their brackets.
pub fn parse(raw: &str) -> (Vec<&str>, &str) {
    let mut categories = Vec::new();
    let mut rest = raw.trim();
    loop {
        let (Some(start), Some(end)) = (rest.find('['), rest.find(']')) else {
            return (categories, rest);
        };
        if let Some(splitter) = rest.find('|')
            && splitter + 1 == start
        {
            return (categories, &rest[1..]);
        }
        categories.push(&rest[start + 1..end]);
        rest = rest[end + 1..].trim();
    }
}

/// The name with its categories but without the `|` marker, pyfa's `fullName`, which it sorts by.
pub fn full_name(raw: &str) -> String {
    let (categories, short) = parse(raw);
    let mut name = String::new();
    for category in categories {
        name.push('[');
        name.push_str(category);
        name.push(']');
    }
    name.push_str(short);
    name
}

/// pyfa's `smartSort`: text compared as text, runs of digits as numbers, so "T10" follows "T9".
pub fn smart_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (chunks(a), chunks(b));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (x, y) {
                    (Chunk::Number(x), Chunk::Number(y)) => cmp_digits(x, y),
                    (Chunk::Text(x), Chunk::Text(y)) => x.cmp(y),
                    // pyfa's lists always alternate the same way, so these never meet there.
                    (Chunk::Number(_), Chunk::Text(_)) => Ordering::Less,
                    (Chunk::Text(_), Chunk::Number(_)) => Ordering::Greater,
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

enum Chunk<'a> {
    Text(&'a str),
    Number(&'a str),
}

/// Splits like `re.split('([0-9]+)', s)`: text, digits, text, ..., starting and ending with
/// text that may be empty.
fn chunks(s: &str) -> impl Iterator<Item = Chunk<'_>> {
    let mut rest = s;
    let mut digits = false;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let end = rest
            .find(|c: char| c.is_ascii_digit() != digits)
            .unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(end);
        let chunk = if digits {
            Chunk::Number(chunk)
        } else {
            Chunk::Text(chunk)
        };
        if tail.is_empty() && !digits {
            done = true;
        }
        rest = tail;
        digits = !digits;
        Some(chunk)
    })
}

/// Compares two digit runs by value, however long they are.
fn cmp_digits(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// One menu level: named sublevels and the entries directly in it, both in insertion order.
#[derive(Debug, Clone)]
pub struct Tree<T> {
    pub children: Vec<(String, Tree<T>)>,
    pub entries: Vec<(String, T)>,
}

impl<T> Default for Tree<T> {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            entries: Vec::new(),
        }
    }
}

impl<T> Tree<T> {
    /// File `item` under its raw name's categories. A later entry with the same short name in
    /// the same level replaces the earlier one, as pyfa's dictionaries do.
    pub fn insert(&mut self, raw: &str, item: T) {
        let (categories, short) = parse(raw);
        let mut level = self;
        for category in categories {
            let i = if let Some(i) = level.children.iter().position(|(n, _)| n == category) {
                i
            } else {
                level.children.push((category.to_owned(), Tree::default()));
                level.children.len() - 1
            };
            level = &mut level.children[i].1;
        }
        match level.entries.iter_mut().find(|(n, _)| n == short) {
            Some(entry) => entry.1 = item,
            None => level.entries.push((short.to_owned(), item)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty() && self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pyfa_names() {
        assert_eq!(parse("Uniform"), (vec![], "Uniform"));
        assert_eq!(parse("[Generic]EM"), (vec!["Generic"], "EM"));
        assert_eq!(
            parse("[Frequency Crystals]|[T2] Aurora"),
            (vec!["Frequency Crystals"], "[T2] Aurora")
        );
        assert_eq!(
            parse("[NPC][Burner][Team]Enyo"),
            (vec!["NPC", "Burner", "Team"], "Enyo")
        );
        assert_eq!(
            full_name("[Frequency Crystals]|[T2] Aurora"),
            "[Frequency Crystals][T2] Aurora"
        );
    }

    #[test]
    fn sorts_numbers_by_value() {
        let mut names = vec!["T10", "T9", "T1", "A", "T09x"];
        names.sort_by(|a, b| smart_cmp(a, b));
        assert_eq!(names, ["A", "T1", "T9", "T09x", "T10"]);
        assert_eq!(smart_cmp("abc", "abc"), Ordering::Equal);
        assert_eq!(
            smart_cmp("B", "a"),
            Ordering::Less,
            "case matters, as in Python"
        );
    }

    #[test]
    fn later_entries_replace_earlier_ones() {
        let mut tree = Tree::default();
        tree.insert("[A]x", 1);
        tree.insert("[A]y", 2);
        tree.insert("[A]x", 3);
        tree.insert("z", 4);
        assert_eq!(tree.entries, [("z".to_owned(), 4)]);
        assert_eq!(
            tree.children[0].1.entries,
            [("x".to_owned(), 3), ("y".to_owned(), 2)]
        );
    }
}
