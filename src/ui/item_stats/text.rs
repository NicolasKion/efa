//! The Description and Traits tabs: the game's markup cleaned up the way pyfa does before
//! showing it (gui/builtinItemStatsViews/itemDescription.py, itemTraits.py).

use crate::{data::Catalog, item_data::Traits};

/// Drop every `<tag ...>` and `</tag>` of the named element, keeping what is between them.
fn strip_tag(text: &str, tag: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let name = inner
            .trim_start_matches('/')
            .trim_start()
            .split(|c: char| c.is_whitespace() || c == '=' || c == '/')
            .next()
            .unwrap_or("");
        if !name.eq_ignore_ascii_case(tag) {
            out.push_str(&rest[start..=start + 1 + end]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A description as HTML: line breaks kept, colours and links dropped.
pub fn description_html(description: &str) -> String {
    let text = description.replace("\r\n", "\n").replace('\n', "<br>");
    strip_tag(&strip_tag(&text, "font"), "a")
}

/// The bonuses as pyfa lays them out: skills by name, then role and misc bonuses, each under a
/// bold heading.
pub fn traits_html(catalog: &Catalog, traits: &Traits) -> String {
    fn section(header: &str, bonuses: &[crate::item_data::Bonus]) -> String {
        let mut lines = vec![format!("<b>{header}</b>")];
        for bonus in bonuses {
            let text = bonus.text.replace('\u{00B7}', "\u{2022} ");
            lines.push(match &bonus.number {
                Some(number) => format!("{number} {text}"),
                None => text,
            });
        }
        lines.join("<br />\n")
    }
    let mut skills: Vec<(String, &[crate::item_data::Bonus])> = traits
        .skills
        .iter()
        .map(|(skill, bonuses)| {
            (
                format!("{} bonuses (per skill level):", catalog.name(*skill)),
                bonuses.as_slice(),
            )
        })
        .collect();
    skills.sort_by(|a, b| a.0.cmp(&b.0));
    let mut sections: Vec<String> = skills.iter().map(|(h, b)| section(h, b)).collect();
    if !traits.role.is_empty() {
        sections.push(section("Role Bonus:", &traits.role));
    }
    if !traits.misc.is_empty() {
        sections.push(section("Misc Bonus:", &traits.misc));
    }
    strip_tag(&sections.join("<br />\n<br />\n"), "a")
}

/// What the Copy entry puts on the clipboard: the text without markup.
pub fn plain(html: &str) -> String {
    let text = html
        .replace("<br />\n", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n");
    crate::text::strip_tags(&text)
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_colours_are_dropped() {
        let text = "Fits <a href=showinfo:3302>Small Turrets</a> and <font color=\"0xff\">red</font>\nnext";
        assert_eq!(description_html(text), "Fits Small Turrets and red<br>next");
    }

    #[test]
    fn other_tags_stay() {
        assert_eq!(strip_tag("<b>bold</b> <a>x</a>", "a"), "<b>bold</b> x");
        assert_eq!(strip_tag("a < b", "a"), "a < b");
    }

    #[test]
    fn plain_text_has_line_breaks() {
        assert_eq!(
            plain("<b>Role Bonus:</b><br />\n50% bonus &amp; more"),
            "Role Bonus:\n50% bonus & more"
        );
    }
}
