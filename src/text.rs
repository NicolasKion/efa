//! Small text helpers the game's data needs: skill levels in Roman numerals and its markup
//! taken out.

/// Skill levels I to V.
pub const ROMAN: [&str; 5] = ["I", "II", "III", "IV", "V"];

/// A skill level in Roman numerals; 0 and anything past V stay digits.
pub fn roman(level: u8) -> String {
    match level {
        1..=5 => ROMAN[usize::from(level) - 1].to_owned(),
        _ => level.to_string(),
    }
}

/// The text with every `<...>` tag taken out and everything else kept.
pub fn strip_tags(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_tags() {
        assert_eq!(roman(0), "0");
        assert_eq!(roman(4), "IV");
        assert_eq!(
            strip_tags("<b>25%</b> bonus to <a href=x>Shield</a> > 0"),
            "25% bonus to Shield > 0"
        );
    }
}
