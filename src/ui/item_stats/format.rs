//! Attribute values the way pyfa's item stats print them: converted by unit (percentages,
//! milliseconds, item and group IDs), then rounded to three significant digits.

use crate::{
    data::Catalog,
    item_data::{AttrInfo, ItemData},
};

use super::super::stats::format::amount;

/// Rig size names, by the Sizeclass value.
const RIG_SIZES: [&str; 4] = ["Small", "Medium", "Large", "X-Large"];

/// A value or a word, before the unit is added.
enum Shown {
    Number(f64),
    Text(String),
}

/// `value` of `attr` with its unit, as the Attributes tab shows it.
pub fn value(data: &ItemData, catalog: &Catalog, attr: Option<&AttrInfo>, value: f64) -> String {
    let Some(unit) = attr.and_then(|a| data.unit(a)) else {
        return amount(value, 3, 0, 0);
    };
    let display = unit.display_name.as_deref();
    let (shown, suffix) = match unit.name.as_str() {
        "Inverse Absolute Percent" | "Inversed Modifier Percent" => {
            (Shown::Number((1.0 - value) * 100.0), display)
        }
        "Modifier Percent" => (Shown::Text(signed_percent(value)), display),
        "Volume" => (Shown::Number(value), Some("m³")),
        "Sizeclass" => (
            Shown::Text(
                RIG_SIZES
                    .get((value as usize).wrapping_sub(1))
                    .map_or_else(|| raw(value), |s| (*s).to_owned()),
            ),
            None,
        ),
        "Absolute Percent" => (Shown::Number(value * 100.0), display),
        "Milliseconds" => (Shown::Number(value / 1000.0), display),
        "Boolean" => (
            Shown::Text(if value == 0.0 { "No" } else { "Yes" }.to_owned()),
            None,
        ),
        "typeID" => (Shown::Text(type_name(catalog, value)), None),
        "groupID" => (Shown::Text(group_name(catalog, value)), None),
        "attributeID" => (Shown::Text(attr_name(data, value)), None),
        _ => (Shown::Number(value), display),
    };
    let text = match shown {
        Shown::Number(v) => amount(v, 3, 0, 0),
        Shown::Text(t) => t,
    };
    match suffix {
        Some(unit) if !unit.is_empty() => format!("{text} {unit}"),
        _ => text,
    }
}

/// A bonus multiplier as a signed percentage: 1.05 is "+5", 0.875 is "-12.50".
fn signed_percent(value: f64) -> String {
    let percent = (value - 1.0) * 100.0;
    if percent.rem_euclid(1.0) == 0.0 {
        format!("{percent:+.0}")
    } else {
        format!("{percent:+.2}")
    }
}

fn type_name(catalog: &Catalog, value: f64) -> String {
    let id = value as i32;
    catalog
        .get(id)
        .map_or_else(|| id.to_string(), |t| format!("{} ({id})", t.name))
}

fn group_name(catalog: &Catalog, value: f64) -> String {
    let id = value as i32;
    catalog
        .group_names
        .get(&id)
        .map_or_else(|| id.to_string(), |name| format!("{name} ({id})"))
}

fn attr_name(data: &ItemData, value: f64) -> String {
    let id = value as i32;
    // Some attributes point at attribute 0, which is no attribute.
    match data.attr(id).filter(|_| id != 0) {
        Some(attr) => format!("{} ({id})", capitalize(&attr.name)),
        None => id.to_string(),
    }
}

/// Python's `str.capitalize`: the first letter upper case, the rest lower.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// How Python prints a float, which is what pyfa's raw view and CSV export show.
pub fn raw(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    let magnitude = value.abs();
    if value != 0.0 && !(1e-4..1e16).contains(&magnitude) {
        // Python writes the exponent with a sign and at least two digits.
        let text = format!("{value:e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let exponent: i32 = exponent.parse().unwrap_or(0);
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// An ISK amount the way pyfa's compare list prints prices: 1.23M, 4.5B.
pub fn isk(value: f64) -> String {
    let text = amount(value, 3, 3, 9);
    match text.strip_suffix('G') {
        Some(number) => format!("{number}B"),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_print_like_python() {
        for (value, want) in [
            (1500.0, "1500.0"),
            (0.75, "0.75"),
            (2.24e-06, "2.24e-06"),
            (2_147_483_647_000.0, "2147483647000.0"),
            (1e16, "1e+16"),
            (0.0, "0.0"),
            (-7.5, "-7.5"),
        ] {
            assert_eq!(raw(value), want, "value {value}");
        }
    }

    #[test]
    fn modifier_percent_is_signed() {
        // Python sees the float error in (1.05 - 1) * 100 too, so pyfa prints decimals there.
        assert_eq!(signed_percent(1.05), "+5.00");
        assert_eq!(signed_percent(1.5), "+50");
        assert_eq!(signed_percent(0.875), "-12.50");
        assert_eq!(signed_percent(1.0), "+0");
    }

    #[test]
    fn capitalize_like_python() {
        assert_eq!(capitalize("maxVelocity"), "Maxvelocity");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn prices_use_billions() {
        assert_eq!(isk(2_500_000_000.0), "2.5B");
        assert_eq!(isk(1_234_567.0), "1.23M");
    }
}
