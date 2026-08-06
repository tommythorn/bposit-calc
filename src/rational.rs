//! Exact rational arithmetic, for showing what an operation produced *before* it was rounded.
//!
//! Sums, differences and products of posits are dyadic and could be handled with the plain
//! big integers in [`crate::bignum`], but quotients are not — `1/3` has no finite binary
//! expansion — so the inspector needs a real numerator and denominator.
//!
//! Fractions are not reduced to lowest terms; only common powers of two are stripped. A full GCD
//! would cost more than it saves here, because each displayed value is the result of one or two
//! operations rather than a long chain.

use crate::bignum::Big;
use crate::decimal;
use core::cmp::Ordering;

/// A signed exact rational, `±num / den`, with `den` never zero.
#[derive(Clone, Debug)]
pub struct Rational {
    pub neg: bool,
    num: Big,
    den: Big,
}

impl Rational {
    fn new(neg: bool, num: Big, den: Big) -> Rational {
        debug_assert!(!den.is_zero());
        if num.is_zero() {
            return Rational::zero();
        }
        let mut r = Rational { neg, num, den };
        r.strip_common_twos();
        r
    }

    pub fn zero() -> Rational {
        Rational {
            neg: false,
            num: Big::zero(),
            den: Big::one(),
        }
    }

    /// Keeps dyadic values (the common case) from carrying a needlessly large denominator.
    fn strip_common_twos(&mut self) {
        let t = self.num.trailing_zeros().min(self.den.trailing_zeros());
        if t > 0 {
            self.num.shr_bits(t);
            self.den.shr_bits(t);
        }
    }

    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    /// If the expansion terminates, the power of ten that clears the denominator.
    ///
    /// A fraction terminates in decimal exactly when its denominator is `2^a·5^b`, and then
    /// `num × 10^max(a,b)` is divisible by it — which is how the exact digits are obtained.
    fn terminating_scale(&self) -> Option<u32> {
        let a = self.den.trailing_zeros();
        let mut d = self.den.clone();
        d.shr_bits(a);
        let five = Big::from_u64(5);
        let mut b = 0u32;
        loop {
            let (q, r) = d.divmod_big(&five);
            if !r.is_zero() {
                break;
            }
            d = q;
            b += 1;
        }
        d.is_one().then(|| a.max(b))
    }

    /// Whether the value has a finite decimal expansion at all.
    pub fn is_terminating(&self) -> bool {
        self.terminating_scale().is_some()
    }

    /// `±frac × 2^(exp - HIDDEN_BIT)`, the form a decoded posit comes in.
    pub fn from_significand(neg: bool, frac: u64, exp: i64) -> Rational {
        let e = exp - crate::bits::HIDDEN_BIT as i64;
        let mut num = Big::from_u64(frac);
        let mut den = Big::one();
        if e > 0 {
            num.shl_bits(e as u32);
        } else if e < 0 {
            den.shl_bits((-e) as u32);
        }
        Rational::new(neg, num, den)
    }

    /// A small integer, for the constants operations need (1 for reciprocal, 2 for doubling).
    pub fn from_int(v: i64) -> Rational {
        Rational::from_significand(v < 0, v.unsigned_abs(), crate::bits::HIDDEN_BIT as i64)
    }

    /// `±mantissa × 10^exp10`, the form a typed literal comes in.
    pub fn from_decimal_parts(neg: bool, mantissa: Big, exp10: i32) -> Rational {
        let mut num = mantissa;
        let mut den = Big::one();
        if exp10 > 0 {
            mul_pow10(&mut num, exp10 as u32);
        } else if exp10 < 0 {
            mul_pow10(&mut den, (-exp10) as u32);
        }
        Rational::new(neg, num, den)
    }

    pub fn negated(&self) -> Rational {
        if self.is_zero() {
            return Rational::zero();
        }
        Rational::new(!self.neg, self.num.clone(), self.den.clone())
    }

    pub fn add(&self, other: &Rational) -> Rational {
        let n1 = self.num.mul_big(&other.den);
        let n2 = other.num.mul_big(&self.den);
        let den = self.den.mul_big(&other.den);
        if self.neg == other.neg {
            let mut n = n1;
            n.add(&n2);
            Rational::new(self.neg, n, den)
        } else {
            // Opposite signs: the larger magnitude wins and sets the sign.
            match n1.cmp(&n2) {
                Ordering::Greater => {
                    let mut n = n1;
                    n.sub_assign(&n2);
                    Rational::new(self.neg, n, den)
                }
                Ordering::Less => {
                    let mut n = n2;
                    n.sub_assign(&n1);
                    Rational::new(other.neg, n, den)
                }
                Ordering::Equal => Rational::zero(),
            }
        }
    }

    pub fn sub(&self, other: &Rational) -> Rational {
        self.add(&other.negated())
    }

    pub fn mul(&self, other: &Rational) -> Rational {
        Rational::new(
            self.neg != other.neg,
            self.num.mul_big(&other.num),
            self.den.mul_big(&other.den),
        )
    }

    /// `None` when dividing by zero — the caller decides whether that means NaR.
    pub fn div(&self, other: &Rational) -> Option<Rational> {
        if other.is_zero() {
            return None;
        }
        Some(Rational::new(
            self.neg != other.neg,
            self.num.mul_big(&other.den),
            self.den.mul_big(&other.num),
        ))
    }

    pub fn abs(&self) -> Rational {
        Rational {
            neg: false,
            num: self.num.clone(),
            den: self.den.clone(),
        }
    }

    /// Signed comparison.
    pub fn cmp(&self, other: &Rational) -> Ordering {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => {
                return if other.neg {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (false, true) => {
                return if self.neg {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            _ => {}
        }
        match (self.neg, other.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.cmp_abs(other),
            (true, true) => other.cmp_abs(self),
        }
    }

    /// Compare magnitudes: `a/b` versus `c/d` is `a·d` versus `c·b`.
    pub fn cmp_abs(&self, other: &Rational) -> Ordering {
        self.num
            .mul_big(&other.den)
            .cmp(&other.num.mul_big(&self.den))
    }

    /// Render to at most `sig` significant digits.
    ///
    /// The flag is true only when the returned text is the whole value — false both when the
    /// expansion does not terminate and when it terminates but was too long to show.
    pub fn to_decimal(&self, sig: usize) -> (String, bool) {
        if self.is_zero() {
            return ("0".to_string(), true);
        }

        if let Some(k) = self.terminating_scale() {
            let mut scaled = self.num.clone();
            mul_pow10(&mut scaled, k);
            let (q, rem) = scaled.divmod_big(&self.den);
            debug_assert!(
                rem.is_zero(),
                "terminating_scale must clear the denominator"
            );
            let sci = decimal::sci_from(q.to_decimal(), -(k as i64));
            return decimal::render(self.neg, &sci, sig);
        }

        // Non-terminating: produce `sig` digits and never claim exactness. The scale is estimated
        // from the bit lengths, which is accurate to within a digit, then widened if the quotient
        // came out shorter than the rounding needs.
        let e2 = self.num.bit_len() as i64 - self.den.bit_len() as i64;
        let e10 = (e2 as f64 * core::f64::consts::LOG10_2) as i64;
        let mut k = (sig as i64 + 10 - e10).max(0) as u32;
        for _ in 0..4 {
            let mut scaled = self.num.clone();
            mul_pow10(&mut scaled, k);
            let (q, _) = scaled.divmod_big(&self.den);
            let digits = q.to_decimal();
            if digits.len() < sig + 2 {
                k += 64;
                continue;
            }
            let sci = decimal::sci_from(digits, -(k as i64));
            return (decimal::render(self.neg, &sci, sig).0, false);
        }
        unreachable!("a non-zero rational always yields digits once the scale is wide enough")
    }
}

/// `b *= 10^n`
fn mul_pow10(b: &mut Big, n: u32) {
    if n == 0 || b.is_zero() {
        return;
    }
    b.mul_pow5(n);
    b.shl_bits(n);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: i64) -> Rational {
        Rational::from_int(v)
    }

    fn dec(s: &str, sig: usize) -> String {
        // Parse a plain decimal literal for test convenience.
        let (neg, body) = match s.strip_prefix('-') {
            Some(b) => (true, b),
            None => (false, s),
        };
        let mut mantissa = Big::zero();
        let mut exp10 = 0i32;
        let mut after_point = false;
        for c in body.chars() {
            if c == '.' {
                after_point = true;
                continue;
            }
            mantissa.mul_u32(10);
            mantissa.add_u32(c as u32 - '0' as u32);
            if after_point {
                exp10 -= 1;
            }
        }
        Rational::from_decimal_parts(neg, mantissa, exp10)
            .to_decimal(sig)
            .0
    }

    #[test]
    fn integers_and_signs() {
        assert_eq!(int(0).to_decimal(20), ("0".into(), true));
        assert_eq!(int(7).to_decimal(20), ("7".into(), true));
        assert_eq!(int(-7).to_decimal(20), ("-7".into(), true));
    }

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(int(60).add(&int(2)).to_decimal(20), ("62".into(), true));
        assert_eq!(int(64).sub(&int(2)).to_decimal(20), ("62".into(), true));
        assert_eq!(int(6).mul(&int(7)).to_decimal(20), ("42".into(), true));
        assert_eq!(
            int(1).div(&int(2)).unwrap().to_decimal(20),
            ("0.5".into(), true)
        );
        // Opposite signs cancelling exactly.
        assert!(int(5).sub(&int(5)).is_zero());
        assert_eq!(int(2).sub(&int(5)).to_decimal(20), ("-3".into(), true));
        assert_eq!(int(-2).add(&int(5)).to_decimal(20), ("3".into(), true));
        assert_eq!(int(-2).mul(&int(-5)).to_decimal(20), ("10".into(), true));
        assert_eq!(int(-2).mul(&int(5)).to_decimal(20), ("-10".into(), true));
    }

    #[test]
    fn division_by_zero_is_rejected() {
        assert!(int(1).div(&int(0)).is_none());
    }

    /// The whole reason this module exists: 1/3 has no finite expansion, so it must be reported
    /// as truncated rather than claimed exact.
    #[test]
    fn non_terminating_quotients_are_flagged() {
        let third = int(1).div(&int(3)).unwrap();
        let (text, exact) = third.to_decimal(20);
        assert!(!exact, "1/3 cannot be exact");
        assert!(text.starts_with("0.33333333333333333333"), "got {text}");
        assert!(!third.is_terminating());

        // A quotient that does terminate must say so.
        let quarter = int(1).div(&int(4)).unwrap();
        assert_eq!(quarter.to_decimal(20), ("0.25".into(), true));
        assert!(quarter.is_terminating());
        // 1/5 terminates in decimal even though it is not dyadic.
        assert!(int(1).div(&int(5)).unwrap().is_terminating());
    }

    #[test]
    fn decimal_literals_round_trip() {
        assert_eq!(dec("0.1", 20), "0.1");
        assert_eq!(dec("-0.001", 20), "-0.001");
        assert_eq!(dec("123.456", 20), "123.456");
        assert_eq!(dec("1000", 20), "1000");
    }

    #[test]
    fn ordering() {
        assert_eq!(int(1).cmp(&int(2)), Ordering::Less);
        assert_eq!(int(2).cmp(&int(1)), Ordering::Greater);
        assert_eq!(int(2).cmp(&int(2)), Ordering::Equal);
        assert_eq!(int(-1).cmp(&int(1)), Ordering::Less);
        assert_eq!(int(-2).cmp(&int(-1)), Ordering::Less);
        assert_eq!(int(0).cmp(&int(-1)), Ordering::Greater);
        assert_eq!(int(0).cmp(&int(1)), Ordering::Less);
        // Fractions with different denominators.
        let a = int(1).div(&int(3)).unwrap();
        let b = int(1).div(&int(4)).unwrap();
        assert_eq!(a.cmp(&b), Ordering::Greater);
    }

    /// Very small and very large magnitudes must still produce the requested digits.
    #[test]
    fn extreme_magnitudes() {
        let mut tiny = Big::one();
        tiny.shl_bits(1);
        let big_den = {
            let mut d = Big::one();
            d.shl_bits(400);
            d
        };
        let r = Rational::new(false, tiny, big_den); // 2^-399
        let (text, exact) = r.to_decimal(10);
        // Rounded to 10 significant digits; the trailing zero of 7.745183830 is not shown.
        assert_eq!(text, "7.74518383e-121", "got {text}");
        // Dyadic, so it terminates -- but 2^-399 is 5^399/10^399, whose numerator has 279
        // significant digits, so 10 of them are necessarily a rounding.
        assert!(r.is_terminating());
        assert!(!exact, "10 digits cannot hold a 279-digit expansion");
        assert!(!r.to_decimal(278).1, "nor can 278");
        assert!(r.to_decimal(279).1, "279 digits is exactly enough");

        let mut huge = Big::one();
        huge.shl_bits(400);
        let r = Rational::new(false, huge, Big::one());
        let (text, _) = r.to_decimal(10);
        assert!(text.starts_with("2.582249878e120"), "got {text}");
    }
}
