//! Matching for the universal search: what was typed, and how well a name answers it.

/// What the search looks through. Tab steps along these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    All,
    Actions,
    Items,
    Ships,
    Fits,
    Settings,
}

impl Scope {
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::Actions,
        Self::Items,
        Self::Ships,
        Self::Fits,
        Self::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Actions => "Actions",
            Self::Items => "Items",
            Self::Ships => "Ships",
            Self::Fits => "Fits",
            Self::Settings => "Settings",
        }
    }

    /// Whether results of `kind` show in this scope.
    pub fn shows(self, kind: Self) -> bool {
        self == Self::All || self == kind
    }

    /// The scope next to this one, wrapping around at either end.
    #[must_use]
    pub fn step(self, forward: bool) -> Self {
        let at = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[if forward {
            (at + 1) % len
        } else {
            (at + len - 1) % len
        }]
    }
}

/// The name is exactly what was typed.
const EXACT: u32 = 1000;
/// The name starts with what was typed.
const PREFIX: u32 = 800;
/// Every word starts a word of the name.
const WORD_STARTS: u32 = 600;
/// What was typed is the name's initials, "fw" for Fitting Wizard.
const INITIALS: u32 = 500;
/// Every word is somewhere in the name.
const INSIDE: u32 = 400;
/// Some words are only in the keywords.
const KEYWORDS: u32 = 200;
/// Longer names give up this much at most, so the shorter of two equal matches comes first.
const LENGTH_PENALTY: usize = 99;

/// What was typed: the words to look for and an amount given as "x1000" or "5x".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// The text without the amount, trimmed, as typed.
    pub text: String,
    /// How many to add, when the query says: "antimatter x1000", "5x hobgoblin".
    pub amount: Option<u32>,
    /// The text's words in lower case.
    words: Vec<String>,
}

impl Query {
    pub fn parse(text: &str) -> Self {
        let mut amount = None;
        let mut kept = Vec::new();
        for word in text.split_whitespace() {
            match parse_amount(word) {
                Some(n) if amount.is_none() => amount = Some(n),
                _ => kept.push(word),
            }
        }
        // An amount with nothing to add is just a word.
        if kept.is_empty() {
            amount = None;
            kept = text.split_whitespace().collect();
        }
        Self {
            text: kept.join(" "),
            amount,
            words: kept.iter().map(|w| w.to_lowercase()).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// How well `name` answers the query, higher is better. None when it does not.
    pub fn score(&self, name: &str) -> Option<u32> {
        self.score_with(name, "")
    }

    /// [`Query::score`] with words the name can also be found by, which count for less.
    pub fn score_with(&self, name: &str, keywords: &str) -> Option<u32> {
        if self.is_empty() {
            return None;
        }
        let name = name.to_lowercase();
        let text = self.words.join(" ");
        let rank = if name == text {
            EXACT
        } else if name.starts_with(&text) {
            PREFIX
        } else if self.words.iter().all(|w| starts_a_word(&name, w)) {
            WORD_STARTS
        } else if self.words.len() == 1 && text.len() > 1 && initials(&name).starts_with(&text) {
            INITIALS
        } else if self.words.iter().all(|w| name.contains(w.as_str())) {
            INSIDE
        } else {
            let keywords = keywords.to_lowercase();
            let found = |w: &String| name.contains(w.as_str()) || keywords.contains(w.as_str());
            if !self.words.iter().all(found) {
                return None;
            }
            KEYWORDS
        };
        Some(rank - name.len().min(LENGTH_PENALTY) as u32)
    }
}

/// "x1000", "1000x" and "X5" as amounts. A bare number is not one: "1600 plate" means the plate.
fn parse_amount(word: &str) -> Option<u32> {
    let digits = word
        .strip_prefix(['x', 'X'])
        .or_else(|| word.strip_suffix(['x', 'X']))?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&n| n > 0)
}

/// Whether `word` begins one of the words of `name`.
fn starts_a_word(name: &str, word: &str) -> bool {
    name.match_indices(word).any(|(at, _)| {
        name[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric())
    })
}

/// The first letters of a name's words.
fn initials(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .filter_map(|word| word.chars().next())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(query: &str, name: &str) -> Option<u32> {
        Query::parse(query).score(name)
    }

    #[test]
    fn better_matches_score_higher() {
        let name = "Damage Control II";
        let exact = score("damage control ii", name).unwrap();
        let prefix = score("damage con", name).unwrap();
        let words = score("control dam", name).unwrap();
        let initials = score("dc", name).unwrap();
        let inside = score("amage", name).unwrap();
        assert!(exact > prefix && prefix > words && words > initials && initials > inside);
        assert_eq!(score("shield", name), None);
        assert_eq!(score("damage shield", name), None);
        assert_eq!(score("", name), None);
    }

    #[test]
    fn shorter_names_win_a_tie() {
        assert!(score("vexor", "Vexor Navy Issue") < score("vexor", "Vexor"));
        assert!(score("drone", "Drone Damage Amplifier II") < score("drone", "Drone Link"));
    }

    #[test]
    fn keywords_find_what_the_name_does_not_say() {
        let q = Query::parse("eft export");
        assert_eq!(q.score("Copy Fit to Clipboard"), None);
        let by_keyword = q.score_with("Copy Fit to Clipboard", "export eft").unwrap();
        assert!(
            by_keyword
                < Query::parse("clipboard")
                    .score("Copy Fit to Clipboard")
                    .unwrap()
        );
    }

    #[test]
    fn initials_need_more_than_a_letter() {
        assert!(score("ftw", "Fitting Wizard").is_none());
        assert!(score("fw", "Fitting Wizard").is_some());
        // A single letter is a word start, not initials.
        assert!(score("w", "Fitting Wizard") > score("fw", "Fitting Wizard"));
    }

    #[test]
    fn amounts_are_spelled_with_an_x() {
        let q = Query::parse("antimatter x1000");
        assert_eq!((q.text.as_str(), q.amount), ("antimatter", Some(1000)));
        let q = Query::parse("5x Hobgoblin II");
        assert_eq!((q.text.as_str(), q.amount), ("Hobgoblin II", Some(5)));
        // Sizes in names stay words.
        let q = Query::parse("1600 plate");
        assert_eq!((q.text.as_str(), q.amount), ("1600 plate", None));
        let q = Query::parse("100mn x0");
        assert_eq!((q.text.as_str(), q.amount), ("100mn x0", None));
        // Nothing to add that many of.
        let q = Query::parse("x5");
        assert_eq!((q.text.as_str(), q.amount), ("x5", None));
    }

    #[test]
    fn scopes_step_around() {
        assert_eq!(Scope::All.step(true), Scope::Actions);
        assert_eq!(Scope::All.step(false), Scope::Settings);
        assert_eq!(Scope::Settings.step(true), Scope::All);
        assert!(Scope::All.shows(Scope::Fits) && !Scope::Items.shows(Scope::Fits));
    }
}
