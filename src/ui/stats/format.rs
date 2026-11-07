//! pyfa's number formatting (gui/utils/numberFormatter.py), so the pane prints the same digits.

/// Suffixes for powers of a thousand above one, by exponent.
const POS_SUFFIXES: [(i32, &str); 3] = [(9, "G"), (6, "M"), (3, "k")];
/// Suffixes for powers of a thousand below one, by exponent.
const NEG_SUFFIXES: [(i32, &str); 2] = [(-6, "μ"), (-3, "m")];

/// `val` with `prec` significant digits and a k, M, G (or m, μ) suffix, allowed between the
/// powers `lowest` and `highest`. Mirrors pyfa's `formatAmount`.
pub fn amount(val: f64, prec: i32, lowest: i32, highest: i32) -> String {
    format(val, prec, lowest, highest, false)
}

/// ISK the way pyfa's price column writes it, `formatAmount(val, 3, 3, 9, currency=True)`:
/// three digits, B rather than G for billions, no unit.
pub fn currency(val: f64) -> String {
    let text = amount(val, 3, 3, 9);
    match text.strip_suffix('G') {
        Some(mantissa) => format!("{mantissa}B"),
        None => text,
    }
}

/// Like [`amount`] with a plus sign on positive numbers.
pub fn signed(val: f64, prec: i32, lowest: i32, highest: i32) -> String {
    format(val, prec, lowest, highest, true)
}

fn format(val: f64, prec: i32, lowest: i32, highest: i32, force_sign: bool) -> String {
    if val.is_infinite() && val > 0.0 {
        return "∞".to_owned();
    }
    // A sum of nothing can come out as -0.0, which would print as "-0".
    let val = if val == 0.0 { 0.0 } else { val };
    let (mut mantissa, mut suffix) = (val, "");
    let magnitude = val.abs();
    if magnitude > 1.0 && highest >= 3 {
        for (i, &(key, name)) in POS_SUFFIXES.iter().enumerate() {
            if magnitude >= 10f64.powi(key) && key <= highest {
                (mantissa, suffix) = (val / 10f64.powi(key), name);
                // Avoid 1000k when rounding pushes the mantissa into the next order.
                if i > 0 {
                    let (bigger, bigger_name) = POS_SUFFIXES[i - 1];
                    if bigger <= highest {
                        let step = 10f64.powi(bigger - key);
                        if round_to_prec(mantissa, prec) >= step {
                            (mantissa, suffix) = (mantissa / step, bigger_name);
                        }
                    }
                }
                break;
            }
        }
    } else if magnitude < 1.0 && val != 0.0 && lowest <= -3 {
        for (i, &(key, name)) in NEG_SUFFIXES.iter().enumerate() {
            let next = NEG_SUFFIXES.get(i + 1).map_or(0, |&(k, _)| k);
            if magnitude < 10f64.powi(next) && key >= lowest {
                (mantissa, suffix) = (val / 10f64.powi(key), name);
                if next <= highest {
                    let step = 10f64.powi(next - key);
                    if round_to_prec(mantissa, prec) >= step {
                        let next_name = NEG_SUFFIXES.get(i + 1).map_or("", |&(_, n)| n);
                        (mantissa, suffix) = (mantissa / step, next_name);
                    }
                }
                break;
            }
        }
    }
    let mantissa = round_to_prec(mantissa, prec);
    let sign = if force_sign && mantissa > 0.0 {
        "+"
    } else {
        ""
    };
    format!("{sign}{}{suffix}", python_float(mantissa))
}

/// Round to `prec` significant digits, but never into the integer part.
pub fn round_to_prec(val: f64, prec: i32) -> f64 {
    if val.fract() == 0.0 {
        return val;
    }
    let places = (prec - val.abs().log10().floor() as i32 - 1).max(0) as usize;
    // Formatting rounds the exact binary value, as Python's round() does; scaling first would
    // turn 12.345 (stored a hair above) into an exact tie and round it down.
    format!("{val:.places$}").parse().unwrap_or(val)
}

/// How Python prints a float that may be whole: no trailing zeros, no ".0".
fn python_float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v:.0}");
    }
    let text = format!("{v:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `val` with thousands separators and `decimals` places, like Python's `{:,.2f}`.
pub fn grouped(val: f64, decimals: usize) -> String {
    let text = format!("{:.decimals$}", val.abs());
    let (int, frac) = text
        .split_once('.')
        .map_or((text.as_str(), None), |(i, f)| (i, Some(f)));
    let mut out = String::with_capacity(text.len() + int.len() / 3 + 1);
    if val < 0.0 && text.bytes().any(|b| b.is_ascii_digit() && b != b'0') {
        out.push('-');
    }
    for (i, digit) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    if let Some(frac) = frac {
        out.push('.');
        out.push_str(frac);
    }
    out
}

/// "a", "a and b", "a, b and c", like pyfa's formatList.
pub fn format_list(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The first letter in upper case, the rest left alone.
pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Seconds the way pyfa's capacitor view writes them: 1m40s, or 45s.
pub fn minutes_seconds(secs: f64) -> String {
    let secs = secs.max(0.0) as u64;
    if secs > 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_matches_pyfa_price_column() {
        assert_eq!(currency(950.0), "950");
        assert_eq!(currency(12_345.0), "12.3k");
        assert_eq!(currency(999_999.0), "1M");
        assert_eq!(currency(2_500_000_000.0), "2.5B");
    }

    #[test]
    fn negative_zero_prints_as_zero() {
        assert_eq!(amount(-0.0, 3, 3, 9), amount(0.0, 3, 3, 9));
        assert!(!amount(-0.0, 3, 3, 9).starts_with('-'));
    }

    #[test]
    fn matches_pyfa_format_amount() {
        for (val, prec, lowest, highest, want) in [
            (0.0, 3, 0, 0, "0"),
            (887.836, 3, 0, 0, "888"),
            (47.0222, 3, 0, 0, "47"),
            (7.2111, 3, 0, 0, "7.21"),
            (14_196.12, 3, 0, 9, "14.2k"),
            (999_999.0, 3, 0, 9, "1M"),
            (2_250_000.0, 3, 0, 9, "2.25M"),
            (370.5, 4, 0, 9, "370.5"),
            (1078.1, 4, 0, 9, "1.078k"),
            (65.625, 3, 0, 0, "65.6"),
            (480.0, 4, 0, 9, "480"),
            (12_345.0, 4, 0, 9, "12.35k"),
        ] {
            assert_eq!(amount(val, prec, lowest, highest), want, "value {val}");
        }
        assert_eq!(signed(4.25, 3, 0, 0), "+4.25");
        assert_eq!(signed(-15.05, 3, 0, 0), "-15.1");
        assert_eq!(amount(f64::INFINITY, 3, 0, 0), "∞");
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(1_234_567.0, 0), "1,234,567");
        assert_eq!(grouped(480.5, 2), "480.50");
        assert_eq!(grouped(-12_000.125, 1), "-12,000.1");
    }

    #[test]
    fn capacitor_times() {
        assert_eq!(minutes_seconds(99.4), "1m39s");
        assert_eq!(minutes_seconds(45.0), "45s");
    }
}
