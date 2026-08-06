//! The four standard bounded-posit formats, their arithmetic, and exact conversion into them.
//!
//! Parameters follow the reference definition in
//! [BPosits.jl](https://github.com/jamesquinlan/BPosits.jl): `es = min(4, n / 4)` and
//! `k_max = 1, 7, 13, 19` for `n = 8, 16, 32, 64`.
//!
//! `fast-posit` parameterises the cap as `RS`, the maximum number of *regime bits*, whereas the
//! reference uses `k_max`, the maximum regime *value*. The two are related by `RS = k_max + 1`,
//! which `tests/conformance.rs` checks exhaustively.

use crate::bignum::Big;
use crate::bits::{decode, mask_n, Fields, Special, HIDDEN_BIT};
use core::cmp::Ordering;
use fast_posit::{Posit, RoundFrom};

type P8 = Posit<8, 2, i8, 2>;
type P16 = Posit<16, 4, i16, 8>;
type P32 = Posit<32, 4, i32, 14>;
type P64 = Posit<64, 4, i64, 20>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    B8,
    B16,
    B32,
    B64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    Neg,
    Recip,
    Double,
    Half,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::B8, Format::B16, Format::B32, Format::B64];

    pub fn name(self) -> &'static str {
        match self {
            Format::B8 => "BPosit8",
            Format::B16 => "BPosit16",
            Format::B32 => "BPosit32",
            Format::B64 => "BPosit64",
        }
    }

    pub fn n(self) -> u32 {
        match self {
            Format::B8 => 8,
            Format::B16 => 16,
            Format::B32 => 32,
            Format::B64 => 64,
        }
    }

    /// `es = min(4, n / 4)`.
    pub fn es(self) -> u32 {
        match self {
            Format::B8 => 2,
            _ => 4,
        }
    }

    pub fn kmax(self) -> u32 {
        match self {
            Format::B8 => 1,
            Format::B16 => 7,
            Format::B32 => 13,
            Format::B64 => 19,
        }
    }

    /// Maximum regime run length, `k_max + 1` — the same quantity `fast-posit` calls `RS`.
    pub fn cap(self) -> u32 {
        self.kmax() + 1
    }

    /// The guaranteed fraction-bit floor, `n - 1 - cap - es`. Yields 3, 3, 13, 39.
    pub fn p_min(self) -> u32 {
        self.n() - 1 - self.cap() - self.es()
    }

    pub fn from_index(i: u32) -> Format {
        Format::ALL[(i as usize).min(3)]
    }

    pub fn index(self) -> u32 {
        match self {
            Format::B8 => 0,
            Format::B16 => 1,
            Format::B32 => 2,
            Format::B64 => 3,
        }
    }

    pub fn mask(self) -> u64 {
        mask_n(self.n())
    }

    /// Decode under this format's cap.
    pub fn decode(self, bits: u64) -> Fields {
        decode(bits, self.n(), self.es(), self.kmax())
    }

    /// Decode the same bit pattern as if the regime were *not* capped — i.e. as an ordinary posit
    /// with the same `n` and `es`. This is the "unbounded shadow" the UI shows alongside.
    pub fn decode_unbounded(self, bits: u64) -> Fields {
        decode(bits, self.n(), self.es(), self.n() - 1)
    }

    pub fn nar_bits(self) -> u64 {
        1u64 << (self.n() - 1)
    }

    pub fn max_bits(self) -> u64 {
        (1u64 << (self.n() - 1)) - 1
    }

    pub fn min_positive_bits(self) -> u64 {
        1
    }
}

/// Apply a binary operation using `fast-posit`, in and out as raw bit patterns.
pub fn bin_op(fmt: Format, op: BinOp, a: u64, b: u64) -> u64 {
    macro_rules! run {
        ($T:ty, $I:ty) => {{
            let x = <$T>::from_bits(a as $I);
            let y = <$T>::from_bits(b as $I);
            let r = match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => x / y,
            };
            r.to_bits() as u64 & fmt.mask()
        }};
    }
    match fmt {
        Format::B8 => run!(P8, i8),
        Format::B16 => run!(P16, i16),
        Format::B32 => run!(P32, i32),
        Format::B64 => run!(P64, i64),
    }
}

/// Apply a unary operation. `Double`/`Half` go through real posit multiplication and division so
/// that any rounding they incur is authentic rather than a bit twiddle.
pub fn un_op(fmt: Format, op: UnOp, a: u64) -> u64 {
    macro_rules! run {
        ($T:ty, $I:ty) => {{
            let x = <$T>::from_bits(a as $I);
            let two = <$T>::ONE + <$T>::ONE;
            let r = match op {
                UnOp::Neg => -x,
                UnOp::Recip => <$T>::ONE / x,
                UnOp::Double => x * two,
                UnOp::Half => x / two,
            };
            r.to_bits() as u64 & fmt.mask()
        }};
    }
    match fmt {
        Format::B8 => run!(P8, i8),
        Format::B16 => run!(P16, i16),
        Format::B32 => run!(P32, i32),
        Format::B64 => run!(P64, i64),
    }
}

/// Convert to `f64`. Exact for BPosit8/16/32, but *not* for BPosit64, whose 58 significand bits
/// overflow an `f64`'s 53. Used as a test oracle, never for the displayed decimal.
#[cfg_attr(not(test), allow(dead_code))]
pub fn to_f64(fmt: Format, bits: u64) -> f64 {
    macro_rules! run {
        ($T:ty, $I:ty) => {
            f64::round_from(<$T>::from_bits(bits as $I))
        };
    }
    match fmt {
        Format::B8 => run!(P8, i8),
        Format::B16 => run!(P16, i16),
        Format::B32 => run!(P32, i32),
        Format::B64 => run!(P64, i64),
    }
}

/// Round an `f64` into the format, using `fast-posit`'s own conversion. Used as the test oracle
/// for [`from_decimal`].
#[cfg_attr(not(test), allow(dead_code))]
pub fn from_f64(fmt: Format, v: f64) -> u64 {
    macro_rules! run {
        ($T:ty) => {
            <$T>::round_from(v).to_bits() as u64 & fmt.mask()
        };
    }
    match fmt {
        Format::B8 => run!(P8),
        Format::B16 => run!(P16),
        Format::B32 => run!(P32),
        Format::B64 => run!(P64),
    }
}

// ---------------------------------------------------------------------------------------------
// Exact conversion into a format
// ---------------------------------------------------------------------------------------------

/// A finite posit magnitude, `frac × 2^(exp - HIDDEN_BIT)` with `frac` in `[2^60, 2^61)`.
#[derive(Clone, Copy, Debug)]
pub struct Mag {
    pub exp: i64,
    pub frac: u64,
}

impl Mag {
    fn of(f: &Fields) -> Mag {
        Mag {
            exp: f.total_exp,
            frac: f.frac,
        }
    }
}

/// Compare two magnitudes exactly.
fn cmp_mag(a: Mag, b: Mag) -> Ordering {
    a.exp.cmp(&b.exp).then_with(|| a.frac.cmp(&b.frac))
}

/// A target value to be rounded into a format: an exact magnitude, compared against candidates.
///
/// Two sources exist — another posit (exact, cheap) and a decimal literal (exact, via big
/// integers) — so the search below is written against this trait rather than a concrete type.
pub trait Target {
    /// Compare the target magnitude against a posit magnitude.
    fn cmp_against(&self, m: Mag) -> Ordering;
    /// Compare `2 × target` against `lo + hi`, deciding which neighbour is nearer.
    /// Returns `Less` when the target is nearer `lo`.
    fn cmp_midpoint(&self, lo: Mag, hi: Mag) -> Ordering;
}

/// Target given as another posit's magnitude.
impl Target for Mag {
    fn cmp_against(&self, m: Mag) -> Ordering {
        cmp_mag(*self, m)
    }

    fn cmp_midpoint(&self, lo: Mag, hi: Mag) -> Ordering {
        // Adjacent posits in a bounded format differ by at most one binade, and the target lies
        // between them, so aligning all three on the smallest exponent stays well inside u128.
        let base = lo.exp.min(hi.exp).min(self.exp);
        (scale_to(*self, base) * 2).cmp(&(scale_to(lo, base) + scale_to(hi, base)))
    }
}

/// `m` expressed as an integer at scale `2^(base - HIDDEN_BIT)`.
///
/// The shift is bounded so that a violated invariant degrades into a wrong comparison rather than
/// undefined behaviour; in practice the exponent gap here is 0 or 1.
fn scale_to(m: Mag, base: i64) -> u128 {
    debug_assert!(m.exp >= base && m.exp - base <= 1);
    (m.frac as u128) << ((m.exp - base).clamp(0, 64) as u32)
}

/// Target given as an exact decimal literal `mantissa × 10^exp10`.
pub struct DecimalTarget {
    mantissa: Big,
    exp10: i32,
}

impl DecimalTarget {
    /// Compare `M × 10^q × 2^extra2` against `value × 2^s`, exactly.
    ///
    /// Both sides are multiplied by `5^max(-q,0) · 2^(-min(q,s))`, which clears every negative
    /// power without changing the ordering, leaving
    /// `M · 5^max(q,0) · 2^(max(q-s,0) + extra2)` versus `value · 5^max(-q,0) · 2^max(s-q,0)`.
    fn cmp_scaled(&self, value: Big, s: i64, extra2: u32) -> Ordering {
        let q = self.exp10 as i64;

        let mut lhs = self.mantissa.clone();
        lhs.mul_pow5(q.max(0) as u32);
        lhs.shl_bits((q - s).max(0) as u32 + extra2);

        let mut rhs = value;
        rhs.mul_pow5((-q).max(0) as u32);
        rhs.shl_bits((s - q).max(0) as u32);

        lhs.cmp(&rhs)
    }
}

impl Target for DecimalTarget {
    fn cmp_against(&self, m: Mag) -> Ordering {
        self.cmp_scaled(Big::from_u64(m.frac), m.exp - HIDDEN_BIT as i64, 0)
    }

    fn cmp_midpoint(&self, lo: Mag, hi: Mag) -> Ordering {
        // Compare 2·target against lo + hi. Putting lo and hi on a common exponent first makes
        // their sum a single exact integer at that scale.
        let base = lo.exp.min(hi.exp);
        let sum = scale_to(lo, base) + scale_to(hi, base);
        self.cmp_scaled(big_from_u128(sum), base - HIDDEN_BIT as i64, 1)
    }
}

fn big_from_u128(v: u128) -> Big {
    let mut hi = Big::from_u64((v >> 64) as u64);
    hi.shl_bits(64);
    hi.add(&Big::from_u64(v as u64));
    hi
}

/// Round a positive target magnitude into `fmt`, returning the bit pattern.
///
/// Posits saturate rather than overflow: anything above `maxpos` becomes `maxpos`, and any
/// nonzero value below `minpos` becomes `minpos` — a nonzero input never rounds to zero, and
/// nothing ever rounds to NaR.
fn round_positive<T: Target>(fmt: Format, t: &T) -> u64 {
    let max = fmt.max_bits();
    let min = fmt.min_positive_bits();

    let mag_at = |bits: u64| Mag::of(&fmt.decode(bits));

    if t.cmp_against(mag_at(max)) != Ordering::Less {
        return max;
    }
    if t.cmp_against(mag_at(min)) != Ordering::Greater {
        return min;
    }

    // Bit patterns of a fixed format are monotonically ordered by value, so binary search for the
    // largest pattern whose value does not exceed the target.
    let (mut lo, mut hi) = (min, max);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if t.cmp_against(mag_at(mid)) == Ordering::Less {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    match t.cmp_midpoint(mag_at(lo), mag_at(hi)) {
        Ordering::Less => lo,
        Ordering::Greater => hi,
        // Ties go to the even pattern, matching the posit standard.
        Ordering::Equal => {
            if lo & 1 == 0 {
                lo
            } else {
                hi
            }
        }
    }
}

/// Re-encode a posit from one format into another, exactly and with correct rounding.
pub fn convert(from: Format, to: Format, bits: u64) -> u64 {
    if from == to {
        return bits;
    }
    let f = from.decode(bits);
    match f.special {
        Some(Special::Zero) => 0,
        Some(Special::NaR) => to.nar_bits(),
        None => {
            let out = round_positive(to, &Mag::of(&f));
            if f.neg {
                out.wrapping_neg() & to.mask()
            } else {
                out
            }
        }
    }
}

/// Parse a decimal literal and round it into `fmt`, exactly.
///
/// This deliberately does not route through `f64`: BPosit64 carries up to 58 significand bits, so
/// an `f64` intermediate cannot even represent most of the format, let alone round to it
/// correctly.
pub fn from_decimal(fmt: Format, s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (neg, rest) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };

    // Split off an exponent suffix.
    let (num, exp_part) = match rest.find(['e', 'E']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let mut exp10: i32 = if exp_part.is_empty() {
        0
    } else {
        exp_part.parse().ok()?
    };

    // Accumulate the digits, tracking how far the point moved.
    let mut mantissa = Big::from_u64(0);
    let mut seen_digit = false;
    let mut seen_point = false;
    for c in num.chars() {
        match c {
            '.' => {
                if seen_point {
                    return None;
                }
                seen_point = true;
            }
            '0'..='9' => {
                seen_digit = true;
                mantissa.mul_u32(10);
                mantissa.add_u32(c as u32 - '0' as u32);
                if seen_point {
                    exp10 -= 1;
                }
            }
            '_' => {}
            _ => return None,
        }
    }
    if !seen_digit {
        return None;
    }

    if mantissa.is_zero() {
        return Some(0);
    }

    let target = DecimalTarget { mantissa, exp10 };
    let out = round_positive(fmt, &target);
    Some(if neg {
        out.wrapping_neg() & fmt.mask()
    } else {
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::HIDDEN_BIT;
    use crate::decimal;

    /// A deterministic xorshift, so failures reproduce.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// The value of a bit pattern, reconstructed from *our* decoder rather than `fast-posit`.
    fn our_f64(fmt: Format, bits: u64) -> f64 {
        let f = fmt.decode(bits);
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

    fn same(a: f64, b: f64) -> bool {
        (a.is_nan() && b.is_nan()) || a == b
    }

    /// `fast-posit` parameterises the cap as RS (max regime *bits*); the reference definition uses
    /// k_max (max regime *value*). If `RS = k_max + 1` were wrong, everything else would be too.
    #[test]
    fn rs_matches_kmax_plus_one() {
        assert_eq!(P8::RS, Format::B8.cap());
        assert_eq!(P16::RS, Format::B16.cap());
        assert_eq!(P32::RS, Format::B32.cap());
        assert_eq!(P64::RS, Format::B64.cap());
        assert_eq!(P8::ES, Format::B8.es());
        assert_eq!(P16::ES, Format::B16.es());
        assert_eq!(P32::ES, Format::B32.es());
        assert_eq!(P64::ES, Format::B64.es());
    }

    /// The published fraction floors: 3, 3, 13, 39.
    #[test]
    fn fraction_floors_match_reference() {
        assert_eq!(Format::B8.p_min(), 3);
        assert_eq!(Format::B16.p_min(), 3);
        assert_eq!(Format::B32.p_min(), 13);
        assert_eq!(Format::B64.p_min(), 39);
    }

    /// Our field decoder must agree with `fast-posit`'s arithmetic over every 8- and 16-bit pattern.
    #[test]
    fn decode_agrees_exhaustively() {
        for fmt in [Format::B8, Format::B16] {
            for bits in 0..(1u64 << fmt.n()) {
                assert!(
                    same(our_f64(fmt, bits), to_f64(fmt, bits)),
                    "{:?} bits={bits:b}: ours={} fast-posit={}",
                    fmt,
                    our_f64(fmt, bits),
                    to_f64(fmt, bits)
                );
            }
        }
    }

    /// BPosit32 still fits an f64 exactly (at most 26 significand bits), so it can be sampled.
    #[test]
    fn decode_agrees_on_bposit32() {
        let mut rng = Rng(0x1234_5678_9abc_def0);
        for _ in 0..200_000 {
            let bits = rng.next() & Format::B32.mask();
            assert!(
                same(our_f64(Format::B32, bits), to_f64(Format::B32, bits)),
                "bits={bits:032b}"
            );
        }
    }

    /// Bit patterns read as signed integers are ordered by value. The binary search in
    /// `round_positive` depends on this.
    #[test]
    fn patterns_are_monotonic_in_value() {
        for fmt in [Format::B8, Format::B16] {
            let n = fmt.n();
            let mut prev = f64::NEG_INFINITY;
            // Walk the signed range, skipping NaR (the most negative pattern).
            for i in (-(1i64 << (n - 1)) + 1)..(1i64 << (n - 1)) {
                let bits = (i as u64) & fmt.mask();
                let v = to_f64(fmt, bits);
                assert!(v > prev, "{:?} not monotonic at {i}: {prev} then {v}", fmt);
                prev = v;
            }
        }
    }

    /// Exhaustive check that exact decimal rounding matches `fast-posit`'s own f64 conversion.
    /// Every 16-bit value's exact decimal is fed back through the parser.
    #[test]
    fn decimal_roundtrips_exhaustively_16bit() {
        let fmt = Format::B16;
        for bits in 0..(1u64 << 16) {
            let d = fmt.decode(bits);
            if d.special.is_some() {
                continue;
            }
            let sci = decimal::exact_sci(d.frac, d.total_exp - HIDDEN_BIT as i64);
            let s = decimal::exact_string(d.neg, &sci);
            assert_eq!(
                from_decimal(fmt, &s),
                Some(bits),
                "bits={bits:016b} decimal={s}"
            );
        }
    }

    /// The same round-trip for BPosit64, where the expansion runs to hundreds of digits and no
    /// f64 path could reproduce it.
    #[test]
    fn decimal_roundtrips_on_bposit64() {
        let fmt = Format::B64;
        let mut rng = Rng(0xfeed_face_dead_beef);
        for _ in 0..300 {
            let bits = rng.next();
            let d = fmt.decode(bits);
            if d.special.is_some() {
                continue;
            }
            let sci = decimal::exact_sci(d.frac, d.total_exp - HIDDEN_BIT as i64);
            let s = decimal::exact_string(d.neg, &sci);
            assert_eq!(from_decimal(fmt, &s), Some(bits), "bits={bits:016x}");
        }
    }

    /// Exact decimal of an f64, so `from_decimal` can be checked against `fast-posit`'s rounding.
    fn f64_exact_decimal(v: f64) -> String {
        let b = v.to_bits();
        let neg = b >> 63 == 1;
        let biased = ((b >> 52) & 0x7ff) as i64;
        let mant = b & ((1u64 << 52) - 1);
        let (m, e) = if biased == 0 {
            (mant, -1074i64)
        } else {
            (mant | (1u64 << 52), biased - 1075)
        };
        if m == 0 {
            return "0".into();
        }
        decimal::exact_string(neg, &decimal::exact_sci(m, e))
    }

    /// Rounding a decimal literal must land exactly where `fast-posit` lands, for every format
    /// where an f64 can express the input faithfully.
    #[test]
    fn decimal_rounding_matches_fast_posit() {
        let mut rng = Rng(0x0bad_c0de_1234_5678);
        for fmt in [Format::B8, Format::B16, Format::B32] {
            // A spread of ordinary values, plus randoms across the dynamic range.
            let mut cases: Vec<f64> = vec![
                0.0,
                1.0,
                -1.0,
                0.5,
                2.0,
                3.0,
                -3.0,
                0.1,
                -0.1,
                1e10,
                1e-10,
                123.456,
                1.0 / 3.0,
                2.5,
                1.5,
                0.125,
                1e30,
                1e-30,
                6.02214076e23,
            ];
            for _ in 0..3000 {
                let r = rng.next();
                // Random f64s biased towards the representable range.
                let exp = ((r >> 52) % 120) as i32 - 60;
                let frac = 1.0 + (r & 0xfffff) as f64 / 0x100000 as f64;
                let sign = if r & (1 << 63) != 0 { -1.0 } else { 1.0 };
                cases.push(sign * frac * (exp as f64).exp2());
            }
            for v in cases {
                let s = f64_exact_decimal(v);
                let ours = from_decimal(fmt, &s).expect("parse");
                let theirs = from_f64(fmt, v);
                assert_eq!(
                    ours, theirs,
                    "{:?} v={v} decimal={s}: ours={ours:b} fast-posit={theirs:b}",
                    fmt
                );
            }
        }
    }

    /// Nothing nonzero may round to zero or to NaR — posits saturate instead.
    #[test]
    fn rounding_never_produces_zero_or_nar() {
        for fmt in Format::ALL {
            for s in [
                "1e-300", "-1e-300", "1e300", "-1e300", "1e-99999", "1e99999",
            ] {
                let bits = from_decimal(fmt, s).expect("parse");
                assert_ne!(bits, 0, "{:?} {s} rounded to zero", fmt);
                assert_ne!(bits, fmt.nar_bits(), "{:?} {s} rounded to NaR", fmt);
            }
            assert_eq!(from_decimal(fmt, "0"), Some(0));
            assert_eq!(from_decimal(fmt, "-0.000"), Some(0));
        }
    }

    /// Every narrower format's grid sits inside BPosit64's, so widening then narrowing is lossless.
    #[test]
    fn widening_roundtrip_is_lossless() {
        for fmt in [Format::B8, Format::B16] {
            for bits in 0..(1u64 << fmt.n()) {
                let wide = convert(fmt, Format::B64, bits);
                let back = convert(Format::B64, fmt, wide);
                assert_eq!(back, bits, "{:?} bits={bits:b}", fmt);
            }
        }
        let mut rng = Rng(0xabcd_ef01_2345_6789);
        for _ in 0..50_000 {
            let bits = rng.next() & Format::B32.mask();
            let wide = convert(Format::B32, Format::B64, bits);
            assert_eq!(
                convert(Format::B64, Format::B32, wide),
                bits,
                "bits={bits:032b}"
            );
        }
    }

    /// Converting must preserve the value, not the bit pattern.
    #[test]
    fn conversion_preserves_value() {
        let one8 = from_decimal(Format::B8, "1").unwrap();
        let one64 = from_decimal(Format::B64, "1").unwrap();
        assert_eq!(convert(Format::B8, Format::B64, one8), one64);
        let half8 = from_decimal(Format::B8, "0.5").unwrap();
        let half32 = from_decimal(Format::B32, "0.5").unwrap();
        assert_eq!(convert(Format::B8, Format::B32, half8), half32);
    }

    /// Posit arithmetic saturates rather than overflowing, and 1/0 is NaR.
    #[test]
    fn saturation_and_nar() {
        for fmt in Format::ALL {
            let max = fmt.max_bits();
            assert_eq!(bin_op(fmt, BinOp::Add, max, max), max, "{:?}", fmt);
            assert_eq!(bin_op(fmt, BinOp::Mul, max, max), max, "{:?}", fmt);
            let one = from_decimal(fmt, "1").unwrap();
            assert_eq!(bin_op(fmt, BinOp::Div, one, 0), fmt.nar_bits(), "{:?}", fmt);
            assert_eq!(un_op(fmt, UnOp::Recip, 0), fmt.nar_bits(), "{:?}", fmt);
        }
    }

    /// Negation is exactly two's complement of the bit pattern, for every non-NaR value.
    #[test]
    fn negation_is_twos_complement() {
        for fmt in [Format::B8, Format::B16] {
            for bits in 0..(1u64 << fmt.n()) {
                if bits == fmt.nar_bits() {
                    continue;
                }
                assert_eq!(
                    un_op(fmt, UnOp::Neg, bits),
                    bits.wrapping_neg() & fmt.mask(),
                    "{:?} bits={bits:b}",
                    fmt
                );
            }
        }
    }

    /// In BPosit8 every pattern carries exactly 3 fraction bits, so doubling is exact right up to
    /// the point it saturates. This is the uniform-precision property the cap buys.
    #[test]
    fn doubling_is_exact_until_saturation() {
        let fmt = Format::B8;
        let mut v = from_decimal(fmt, "1").unwrap();
        let max = fmt.max_bits();
        let mut expect = 1.0f64;
        for _ in 0..7 {
            v = un_op(fmt, UnOp::Double, v);
            expect *= 2.0;
            assert_eq!(to_f64(fmt, v), expect);
        }
        // 2^7 = 128 is representable; the next doubling saturates at maxpos = 240.
        v = un_op(fmt, UnOp::Double, v);
        assert_eq!(v, max);
        assert_eq!(to_f64(fmt, v), 240.0);
    }
}
