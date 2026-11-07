//! pyfa's market search (service/market.py `SearchWorkerThread`) with its jargon
//! (service/jargon/): every word of the query is a pattern the item name has to contain, and
//! community abbreviations like "hml" or "lse" stand for the names they abbreviate.
//!
//! The defaults are pyfa's own file. A `user_jargon.yaml` next to efa's saved fits adds to them
//! and wins where both name the same abbreviation, as pyfa's user file does.

use std::{collections::HashMap, fmt, path::PathBuf, sync::OnceLock};

use fancy_regex::{Regex, RegexBuilder};
use serde::{Deserialize, Deserializer, de};

const DEFAULTS: &str = include_str!("jargon.yaml");
const USER_FILE: &str = "user_jargon.yaml";
/// What the joined pattern must be at least, pyfa's `minItemSearchLength`.
const MIN_LENGTH: usize = 3;
/// The same for Chinese, Japanese and Korean, where one character is a word.
const MIN_LENGTH_CJK: usize = 1;
/// Prefix that makes the rest of the query raw regular expressions.
const REGEX_PREFIX: &str = "re:";

/// Abbreviations and what each stands for, as regular expressions.
#[derive(Debug, Default, Clone)]
pub struct Jargon {
    terms: HashMap<String, Vec<String>>,
}

impl Jargon {
    /// Read a jargon file. Abbreviations are not case sensitive, and blank ones are ignored.
    pub fn from_yaml(text: &str) -> Result<Self, String> {
        let raw: Option<HashMap<Key, Option<Vec<String>>>> =
            serde_saphyr::from_str(text).map_err(|e| e.to_string())?;
        let terms = raw
            .unwrap_or_default()
            .into_iter()
            .filter(|(key, _)| !key.0.is_empty())
            .map(|(key, expansions)| (key.0.to_lowercase(), expansions.unwrap_or_default()))
            .collect();
        Ok(Self { terms })
    }

    /// pyfa's jargon file.
    pub fn defaults() -> Self {
        Self::from_yaml(DEFAULTS).expect("the bundled jargon file parses")
    }

    /// The defaults with the user's file on top, read once.
    pub fn global() -> &'static Self {
        static JARGON: OnceLock<Jargon> = OnceLock::new();
        JARGON.get_or_init(|| {
            let mut jargon = Self::defaults();
            if let Some(path) = user_file()
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                match Self::from_yaml(&text) {
                    Ok(user) => jargon.extend(user),
                    Err(e) => eprintln!("{}: {e}", path.display()),
                }
            }
            jargon
        })
    }

    /// Take `other`'s abbreviations, replacing ones both have.
    pub fn extend(&mut self, other: Self) {
        self.terms.extend(other.terms);
    }

    pub fn get(&self, term: &str) -> Option<&[String]> {
        self.terms
            .get(&term.to_lowercase())
            .map(Vec::as_slice)
            .filter(|e| !e.is_empty())
    }
}

/// A YAML key read as text, since pyfa's file has numbers among its keys.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Key(String);

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl de::Visitor<'_> for Visitor {
            type Value = Key;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an abbreviation")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key, E> {
                Ok(Key(v.to_owned()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Key, E> {
                Ok(Key(v.to_string()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Key, E> {
                Ok(Key(String::new()))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn user_file() -> Option<PathBuf> {
    crate::paths::data_file(USER_FILE)
}

/// A parsed market search: each word a pattern the name has to contain somewhere.
#[derive(Debug)]
pub struct Query {
    patterns: Vec<Regex>,
}

impl Query {
    /// The query as pyfa runs it, or None when it is too short to search for. A query starting
    /// with `re:` is taken as regular expressions; otherwise `*` and `?` are the only wildcards.
    pub fn parse(text: &str, jargon: &Jargon) -> Option<Self> {
        let trimmed = text.trim();
        let tokens = match strip_prefix_ignore_case(trimmed, REGEX_PREFIX) {
            Some(rest) => regex_tokens(rest).unwrap_or_else(|| plain_tokens(rest)),
            None => plain_tokens(trimmed),
        };
        let expanded: Vec<String> = tokens
            .into_iter()
            .map(|(raw, pattern)| match jargon.get(&raw) {
                Some(expansions) => format!("({})", expansions.join("|")),
                None => pattern,
            })
            .collect();
        let joined = expanded.join(" ");
        let min = if joined.chars().any(is_cjk) {
            MIN_LENGTH_CJK
        } else {
            MIN_LENGTH
        };
        if joined.chars().count() < min {
            return None;
        }
        let patterns = expanded
            .iter()
            // A pattern that does not compile is searched for as plain text.
            .map(|p| compile(p).or_else(|| compile(&fancy_regex::escape(p))))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { patterns })
    }

    /// Whether every pattern is found in the name.
    pub fn matches(&self, name: &str) -> bool {
        self.patterns
            .iter()
            .all(|p| p.is_match(name).unwrap_or(false))
    }
}

fn compile(pattern: &str) -> Option<Regex> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// Words of a normal query, each with its pattern: the text escaped, `*` and `?` standing for
/// any run of word characters and at most one.
fn plain_tokens(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .map(|word| {
            let mut pattern = String::new();
            for part in word.split_inclusive(['*', '?']) {
                let (literal, wildcard) = match part.chars().last() {
                    Some('*') => (&part[..part.len() - 1], r"\w*"),
                    Some('?') => (&part[..part.len() - 1], r"\w?"),
                    _ => (part, ""),
                };
                pattern.push_str(&fancy_regex::escape(literal));
                pattern.push_str(wildcard);
            }
            (word.to_owned(), pattern)
        })
        .collect()
}

/// Words of a `re:` query, split on whitespace outside brackets and parentheses. None when the
/// brackets do not pair up, which pyfa then searches as a normal query.
fn regex_tokens(text: &str) -> Option<Vec<(String, String)>> {
    let (mut round, mut square) = (0_i32, 0_i32);
    let mut escaped = false;
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if escaped {
            escaped = false;
            current.push(c);
            continue;
        }
        match c {
            '\\' => {
                escaped = true;
                current.push(c);
            }
            '[' => {
                square += 1;
                current.push(c);
            }
            ']' => {
                square -= 1;
                current.push(c);
            }
            '(' if square == 0 => {
                round += 1;
                current.push(c);
            }
            ')' if square == 0 => {
                round -= 1;
                current.push(c);
            }
            c if c.is_whitespace() && round == 0 && square == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
        if !(0..=1).contains(&square) || round < 0 {
            return None;
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Some(tokens.into_iter().map(|t| (t.clone(), t)).collect())
}

/// Han, kana and hangul, where pyfa searches from a single character.
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}'
        | '\u{F900}'..='\u{FAFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> Query {
        Query::parse(text, &Jargon::defaults()).expect("long enough")
    }

    #[test]
    fn defaults_load_with_numeric_keys() {
        let jargon = Jargon::defaults();
        assert!(jargon.get("hml").is_some());
        assert!(jargon.get("LSE").is_some());
        assert!(jargon.get("2").is_some());
        assert!(jargon.get("no such abbreviation").is_none());
    }

    #[test]
    fn abbreviations_expand() {
        let q = query("lse 2");
        assert!(q.matches("Large Shield Extender II"));
        assert!(!q.matches("Large Shield Extender I"));
        assert!(!q.matches("Medium Shield Extender II"));

        let q = query("hml");
        assert!(q.matches("Heavy Missile Launcher II"));
        assert!(!q.matches("Rapid Heavy Missile Launcher II"));

        let q = query("dc");
        assert!(q.matches("Damage Control II"));
        assert!(!q.matches("Assault Damage Control II"));
    }

    #[test]
    fn plain_words_match_anywhere_in_any_order() {
        let q = query("extender large");
        assert!(q.matches("Large Shield Extender II"));
        assert!(!q.matches("Large Armor Plate"));
    }

    #[test]
    fn wildcards_and_special_characters() {
        let q = query("cap* booster");
        assert!(q.matches("Capital Capacitor Booster I"));
        let q = query("(test)");
        assert!(q.matches("Thing (Test)"));
        assert!(!q.matches("Thing Test"));
    }

    #[test]
    fn regex_queries() {
        let q = query("re:^dam(age)? control");
        assert!(q.matches("Damage Control II"));
        assert!(!q.matches("Assault Damage Control II"));
        // Unbalanced brackets fall back to a plain search.
        assert!(query("re:abc)").matches("xabc)"));
    }

    #[test]
    fn short_queries_do_not_search() {
        assert!(Query::parse("zq", &Jargon::defaults()).is_none());
        assert!(Query::parse("  ", &Jargon::defaults()).is_none());
        // An abbreviation can be short, its expansion is what counts.
        assert!(Query::parse("dc", &Jargon::defaults()).is_some());
        assert!(Query::parse("盾", &Jargon::defaults()).is_some());
    }

    #[test]
    fn user_jargon_wins() {
        let mut jargon = Jargon::defaults();
        jargon.extend(Jargon::from_yaml("hml:\n  - 'hammer'\n").expect("parses"));
        let q = Query::parse("hml", &jargon).expect("long enough");
        assert!(q.matches("Hammer"));
        assert!(!q.matches("Heavy Missile Launcher II"));
    }
}
