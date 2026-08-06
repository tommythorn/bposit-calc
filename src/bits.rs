//! Decoding a bounded posit into its constituent bit fields.
//!
//! This mirrors `_bitfields` in the reference implementation
//! (<https://github.com/jamesquinlan/BPosits.jl>) and is checked exhaustively against
//! `fast-posit`'s arithmetic in `tests/`.
//!
//! The layout of a b-posit, reading left to right, is
//!
//! ```text
//!   sign   regime run    [terminator]   exponent   fraction
//!    1        1..=cap       0 or 1          es       the rest
//! ```
//!
//! where `cap = k_max + 1` bounds the regime run. The one rule that makes a *bounded* posit
//! differ from an ordinary posit: **when the run reaches `cap`, the terminating bit is
//! suppressed** — there is nothing left to terminate, so that bit belongs to the exponent
//! instead. This is what puts a floor under the fraction width.

/// Where the hidden (implicit leading) bit of [`Fields::frac`] sits.
pub const HIDDEN_BIT: u32 = 60;

/// The two encodings that stand outside the numeric grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    Zero,
    /// "Not a Real" — the posit analogue of NaN, and the only non-numeric value.
    NaR,
}

/// A fully decoded posit bit pattern.
#[derive(Clone, Debug)]
pub struct Fields {
    pub special: Option<Special>,
    pub neg: bool,
    /// Width of each field, in bits, in left-to-right order.
    pub regime_len: u32,
    /// 1 when a terminating bit is present, 0 when it was suppressed by the cap.
    pub term_len: u32,
    pub exp_len: u32,
    pub frac_len: u32,
    /// True when the regime run hit `cap` and the terminator was therefore suppressed.
    pub capped: bool,
    /// Regime value `k`.
    pub k: i64,
    /// Value of the exponent field.
    pub e: u64,
    /// Total binary exponent, `k·2^es + e`.
    pub total_exp: i64,
    /// Significand with the hidden bit at [`HIDDEN_BIT`], so the value is
    /// `±frac × 2^(total_exp − HIDDEN_BIT)`.
    pub frac: u64,
    /// The magnitude bit pattern the fields were read from. For a negative posit this is the
    /// two's complement of the stored pattern, which is why the field widths line up with it
    /// rather than with the raw bits.
    pub magnitude: u64,
}

/// Decode `bits` (only the low `n` are read) under the format `(n, es, k_max)`.
///
/// Passing `k_max = n - 1` disables the cap, yielding ordinary (unbounded) posit semantics —
/// which is exactly how the "unbounded shadow" comparison is produced.
pub fn decode(bits: u64, n: u32, es: u32, kmax: u32) -> Fields {
    debug_assert!((3..=64).contains(&n));
    let mask = mask_n(n);
    let u = bits & mask;

    let mut f = Fields {
        special: None,
        neg: false,
        regime_len: 0,
        term_len: 0,
        exp_len: 0,
        frac_len: 0,
        capped: false,
        k: 0,
        e: 0,
        total_exp: 0,
        frac: 0,
        magnitude: u,
    };

    if u == 0 {
        f.special = Some(Special::Zero);
        return f;
    }
    if u == 1u64 << (n - 1) {
        f.special = Some(Special::NaR);
        f.neg = true;
        return f;
    }

    f.neg = (u >> (n - 1)) & 1 == 1;
    // Fields are defined on the magnitude, so take the two's complement first.
    let magnitude = if f.neg { u.wrapping_neg() & mask } else { u };
    f.magnitude = magnitude;

    // Drop the sign bit and left-align the remaining n-1 bits in a u64.
    let w = magnitude << (64 - n + 1);
    let regime_is_ones = w >> 63 == 1;
    let cap = kmax + 1;
    let raw_run = if regime_is_ones {
        w.leading_ones()
    } else {
        w.leading_zeros()
    };
    // The run can never be longer than the n-1 bits that follow the sign.
    let run = raw_run.min(cap).min(n - 1);
    f.capped = raw_run >= cap;
    f.regime_len = run;
    f.k = if regime_is_ones {
        run as i64 - 1
    } else {
        -(run as i64)
    };

    // The terminator exists only if the run stopped on its own, and only if a bit is left for it.
    f.term_len = if f.capped || run >= n - 1 { 0 } else { 1 };
    let consumed = run + f.term_len;

    // Whatever bits remain go to the exponent first, then the fraction. Near the top of the
    // range a pattern can run out of bits entirely; the missing trailing bits read as zero,
    // which the shifts below produce naturally.
    let avail = (n - 1).saturating_sub(consumed);
    f.exp_len = es.min(avail);
    f.frac_len = avail - f.exp_len;

    let rest = shl_checked(w, consumed);
    f.e = if es == 0 {
        0
    } else {
        shr_checked(rest, 64 - es)
    };
    f.frac = (1u64 << HIDDEN_BIT) | (shl_checked(rest, es) >> (64 - HIDDEN_BIT));
    f.total_exp = f.k * (1i64 << es) + f.e as i64;

    f
}

/// `x << n`, yielding 0 rather than invoking UB when `n >= 64`.
fn shl_checked(x: u64, n: u32) -> u64 {
    x.checked_shl(n).unwrap_or(0)
}

/// `x >> n`, yielding 0 rather than invoking UB when `n >= 64`.
fn shr_checked(x: u64, n: u32) -> u64 {
    x.checked_shr(n).unwrap_or(0)
}

/// Mask of the low `n` bits.
pub fn mask_n(n: u32) -> u64 {
    if n >= 64 {
        u64::MAX
    } else {
        (1u64 << n) - 1
    }
}

/// The raw stored bit pattern as a string of `n` '0'/'1' characters, most significant first.
pub fn bitstring(bits: u64, n: u32) -> String {
    (0..n)
        .rev()
        .map(|i| if (bits >> i) & 1 == 1 { '1' } else { '0' })
        .collect()
}

#[cfg(test)]
mod tests {
    // Bit literals below are grouped by posit field (sign | regime | terminator | exponent |
    // fraction), which is the whole point of reading them, so the groups are not equal sized.
    #![allow(clippy::unusual_byte_groupings)]

    use super::*;

    /// Reconstruct the value as an f64. Only used where the significand is small enough to fit.
    fn value_f64(bits: u64, n: u32, es: u32, kmax: u32) -> f64 {
        let f = decode(bits, n, es, kmax);
        match f.special {
            Some(Special::Zero) => 0.0,
            Some(Special::NaR) => f64::NAN,
            None => {
                let sig = f.frac as f64 / (1u64 << HIDDEN_BIT) as f64;
                let v = sig * (f.total_exp as f64).exp2();
                if f.neg {
                    -v
                } else {
                    v
                }
            }
        }
    }

    #[test]
    fn specials() {
        let f = decode(0, 8, 2, 1);
        assert_eq!(f.special, Some(Special::Zero));
        let f = decode(0b1000_0000, 8, 2, 1);
        assert_eq!(f.special, Some(Special::NaR));
    }

    #[test]
    fn bposit8_one() {
        // 1.0 is k=0, e=0, frac=1.0. With cap=2 the run is a single 1 plus a terminator.
        let f = decode(0b0_1_0_00_000, 8, 2, 1);
        assert!(!f.neg);
        assert_eq!(f.k, 0);
        assert_eq!(f.total_exp, 0);
        assert_eq!(f.frac, 1u64 << HIDDEN_BIT);
        assert_eq!(f.regime_len, 1);
        assert_eq!(f.term_len, 1);
        assert_eq!(f.exp_len, 2);
        assert_eq!(f.frac_len, 3);
        assert!(!f.capped);
        assert_eq!(value_f64(0b0_1_0_00_000, 8, 2, 1), 1.0);
    }

    #[test]
    fn bposit8_max_is_capped() {
        // 0b0_11_11_111: the run of ones hits cap=2, so no terminator is stored.
        let f = decode(0b0111_1111, 8, 2, 1);
        assert!(f.capped);
        assert_eq!(f.regime_len, 2);
        assert_eq!(f.term_len, 0);
        assert_eq!(f.k, 1);
        assert_eq!(f.exp_len, 2);
        assert_eq!(f.frac_len, 3);
        assert_eq!(f.e, 3);
        assert_eq!(f.total_exp, 7);
        assert_eq!(value_f64(0b0111_1111, 8, 2, 1), 240.0);
    }

    /// Every BPosit8 pattern has exactly 3 fraction bits: with cap=2 the regime plus terminator
    /// always consumes exactly 2 bits. Uniform relative precision, which is the point of the format.
    #[test]
    fn bposit8_precision_is_uniform() {
        for bits in 0u64..256 {
            let f = decode(bits, 8, 2, 1);
            if f.special.is_some() {
                continue;
            }
            assert_eq!(f.frac_len, 3, "bits={bits:08b}");
            assert_eq!(f.exp_len, 2, "bits={bits:08b}");
            assert_eq!(f.regime_len + f.term_len, 2, "bits={bits:08b}");
        }
    }

    /// The fraction width never drops below the format's floor `p_min = n - 1 - cap - es`.
    #[test]
    fn fraction_floor_holds() {
        for &(n, es, kmax, p_min) in &[(8u32, 2u32, 1u32, 3u32), (16, 4, 7, 3)] {
            for bits in 0..(1u64 << n) {
                let f = decode(bits, n, es, kmax);
                if f.special.is_some() {
                    continue;
                }
                assert!(
                    f.frac_len >= p_min,
                    "n={n} bits={bits:b} frac_len={} < p_min={p_min}",
                    f.frac_len
                );
            }
        }
    }

    /// Negation must mirror the fields exactly: a posit and its negation share a magnitude.
    #[test]
    fn negation_mirrors_fields() {
        for bits in 1u64..256 {
            if bits == 0b1000_0000 {
                continue; // NaR
            }
            let f = decode(bits, 8, 2, 1);
            let neg_bits = bits.wrapping_neg() & mask_n(8);
            let g = decode(neg_bits, 8, 2, 1);
            assert_eq!(f.neg, !g.neg, "bits={bits:08b}");
            assert_eq!(f.total_exp, g.total_exp, "bits={bits:08b}");
            assert_eq!(f.frac, g.frac, "bits={bits:08b}");
            assert_eq!(f.regime_len, g.regime_len, "bits={bits:08b}");
            assert_eq!(f.frac_len, g.frac_len, "bits={bits:08b}");
        }
    }

    /// With the cap disabled (`kmax = n-1`) we must reproduce ordinary posit behaviour.
    #[test]
    fn uncapped_matches_ordinary_posit() {
        // maxpos of an unbounded p<8,2> is 2^24: regime run of 7 ones, k=6, es=2.
        let f = decode(0b0111_1111, 8, 2, 7);
        assert_eq!(f.k, 6);
        assert_eq!(f.total_exp, 24);
        assert!(!f.capped);
        assert_eq!(value_f64(0b0111_1111, 8, 2, 7), 16777216.0);
    }
}
