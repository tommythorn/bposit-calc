//! Exact decimal rendering of a posit.
//!
//! A finite posit is `±frac × 2^s`, a dyadic rational, so its decimal expansion always terminates
//! — but it can be long: a BPosit64 near `minpos` needs a few hundred digits. Since `f64` cannot
//! even hold a BPosit64 significand, the expansion is computed with big integers instead.

use crate::bignum::Big;

/// A value in normalised scientific form: `digits[0] . digits[1..] × 10^exp`.
///
/// `digits` never has a trailing zero, so its length is exactly the number of significant digits.
#[derive(Clone, Debug)]
pub struct Sci {
    pub digits: String,
    pub exp: i32,
}

/// Exact scientific form of `frac × 2^s`, for `frac != 0`.
pub fn exact_sci(frac: u64, s: i64) -> Sci {
    let mut n = Big::from_u64(frac);
    let e10 = if s >= 0 {
        n.shl_bits(s as u32);
        0i64
    } else {
        // frac / 2^|s| = frac × 5^|s| / 10^|s|
        n.mul_pow5((-s) as u32);
        s
    };

    sci_from(n.to_decimal(), e10)
}

/// Normalise `digits × 10^e10` into scientific form, dropping trailing zeros.
pub fn sci_from(digits: String, e10: i64) -> Sci {
    let trimmed = digits.trim_end_matches('0');
    if trimmed.is_empty() {
        // The digits were all zeros, so the value is zero whatever the exponent says.
        return Sci {
            digits: "0".to_string(),
            exp: 0,
        };
    }
    let removed = (digits.len() - trimmed.len()) as i64;
    Sci {
        digits: trimmed.to_string(),
        exp: (e10 + removed + trimmed.len() as i64 - 1) as i32,
    }
}

/// Round a digit string to `max_sig` significant digits, half-up.
///
/// Returns the new digits and how much the exponent shifted (1 when a carry propagated off the
/// front, as in 999 → 100).
fn round_digits(digits: &str, max_sig: usize) -> (String, i32) {
    if digits.len() <= max_sig {
        return (digits.to_string(), 0);
    }
    let bytes = digits.as_bytes();
    let round_up = bytes[max_sig] >= b'5';
    let mut kept: Vec<u8> = bytes[..max_sig].to_vec();
    let mut shift = 0i32;
    if round_up {
        let mut i = max_sig;
        loop {
            if i == 0 {
                // Carried off the front: 999… became 1000…
                kept.insert(0, b'1');
                kept.pop();
                shift = 1;
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    // `Sci::digits` never carries trailing zeros, so the carry path must strip them too — leaving
    // them made a rounded 0.0999… print as "0.100000000000" while an equal value printed "0.1".
    let s = String::from_utf8(kept).unwrap();
    let trimmed = s.trim_end_matches('0');
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    (trimmed.to_string(), shift)
}

/// Lay out `digits × 10^exp` in positional notation, without an exponent suffix.
fn positional(digits: &str, exp: i32) -> String {
    let len = digits.len() as i32;
    if exp >= len - 1 {
        // Integer with trailing zeros.
        let mut s = digits.to_string();
        s.push_str(&"0".repeat((exp - len + 1) as usize));
        s
    } else if exp >= 0 {
        let split = (exp + 1) as usize;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
    }
}

/// Render for display, switching to scientific notation outside a comfortable range.
///
/// The returned flag is false when digits had to be dropped, letting the UI mark the value as
/// approximate rather than silently implying the display is the whole number.
pub fn render(neg: bool, sci: &Sci, max_sig: usize) -> (String, bool) {
    let exact = sci.digits.len() <= max_sig;
    let (digits, shift) = round_digits(&sci.digits, max_sig);
    let exp = sci.exp + shift;

    let body = if (-7..21).contains(&exp) {
        positional(&digits, exp)
    } else {
        let mantissa = if digits.len() == 1 {
            digits.clone()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{}e{}", mantissa, exp)
    };

    (format!("{}{}", if neg { "-" } else { "" }, body), exact)
}

/// The complete exact expansion, in positional notation. Can be hundreds of digits long.
pub fn exact_string(neg: bool, sci: &Sci) -> String {
    format!(
        "{}{}",
        if neg { "-" } else { "" },
        positional(&sci.digits, sci.exp)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sci_of(frac: u64, s: i64) -> String {
        let sci = exact_sci(frac, s);
        exact_string(false, &sci)
    }

    #[test]
    fn integers() {
        assert_eq!(sci_of(1, 0), "1");
        assert_eq!(sci_of(3, 0), "3");
        assert_eq!(sci_of(1, 10), "1024");
        assert_eq!(sci_of(5, 3), "40");
    }

    #[test]
    fn negative_powers_are_exact() {
        assert_eq!(sci_of(1, -1), "0.5");
        assert_eq!(sci_of(1, -3), "0.125");
        assert_eq!(sci_of(1, -10), "0.0009765625");
        // 2^-60, the kind of expansion f64 could not print exactly
        assert_eq!(
            sci_of(1, -60),
            "0.000000000000000000867361737988403547205962240695953369140625"
        );
    }

    #[test]
    fn scientific_form_is_normalised() {
        let s = exact_sci(1, 10);
        assert_eq!(s.digits, "1024");
        assert_eq!(s.exp, 3);
        let s = exact_sci(5, 3);
        assert_eq!(s.digits, "4"); // 40 -> 4 x 10^1
        assert_eq!(s.exp, 1);
    }

    #[test]
    fn rounding_carries() {
        // Carrying off the front bumps the exponent; the trailing zero is not significant.
        let (d, shift) = round_digits("999", 2);
        assert_eq!((d.as_str(), shift), ("1", 1));
        let (d, shift) = round_digits("123456", 3);
        assert_eq!((d.as_str(), shift), ("123", 0));
        let (d, shift) = round_digits("125", 2);
        assert_eq!((d.as_str(), shift), ("13", 0));
        // Trailing zeros created by rounding are dropped.
        let (d, shift) = round_digits("10499", 3);
        assert_eq!((d.as_str(), shift), ("105", 0));
    }

    #[test]
    fn render_marks_truncation() {
        let sci = exact_sci(1, -60);
        let (_, exact) = render(false, &sci, 20);
        assert!(!exact, "a 60-digit expansion cannot be exact in 20 digits");
        let sci = exact_sci(1, 10);
        let (s, exact) = render(false, &sci, 20);
        assert!(exact);
        assert_eq!(s, "1024");
    }

    #[test]
    fn render_negative_and_scientific() {
        let sci = exact_sci(1, 100);
        let (s, _) = render(true, &sci, 20);
        assert!(s.starts_with("-1.26765060022822940"), "got {s}");
        assert!(s.ends_with("e30"), "got {s}");
    }
}
