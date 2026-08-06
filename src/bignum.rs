//! A minimal unsigned big integer.
//!
//! Every finite posit is `frac × 2^s` for an integer `frac` and (possibly negative) `s`, so it has
//! a terminating decimal expansion. For BPosit64 that expansion can run to a few hundred digits,
//! and the significand alone (58 bits) already exceeds what an `f64` can hold — so the decimal
//! display cannot go through `f64` without lying. Hence this.
//!
//! The rounding inspector then needs the *exact* result of an operation before rounding, and
//! division of two posits is not dyadic (`1/3` has no finite binary expansion), so this also
//! carries enough to support the general rationals in [`crate::rational`]: full multiplication,
//! subtraction, and long division.

/// Little-endian base-2^32 magnitude. Never stores trailing zero limbs, so the representation is
/// canonical and ordering can compare limb counts first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Big {
    limbs: Vec<u32>,
}

impl Ord for Big {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }
}

impl PartialOrd for Big {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// The largest power of 5 that fits in a `u32`, used to multiply in bulk.
const POW5_CHUNK_EXP: u32 = 13;
const POW5_CHUNK: u32 = 1_220_703_125; // 5^13

impl Big {
    pub fn from_u64(v: u64) -> Self {
        let mut b = Big {
            limbs: vec![v as u32, (v >> 32) as u32],
        };
        b.trim();
        b
    }

    pub fn from_u128(v: u128) -> Self {
        let mut b = Big {
            limbs: vec![
                v as u32,
                (v >> 32) as u32,
                (v >> 64) as u32,
                (v >> 96) as u32,
            ],
        };
        b.trim();
        b
    }

    pub fn zero() -> Self {
        Big { limbs: Vec::new() }
    }

    pub fn one() -> Self {
        Big { limbs: vec![1] }
    }

    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    pub fn is_one(&self) -> bool {
        self.limbs == [1]
    }

    /// Position of the highest set bit, plus one. Zero for zero.
    pub fn bit_len(&self) -> u32 {
        match self.limbs.last() {
            None => 0,
            Some(top) => (self.limbs.len() as u32 - 1) * 32 + (32 - top.leading_zeros()),
        }
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    fn set_bit(&mut self, i: u32) {
        let limb = (i / 32) as usize;
        if self.limbs.len() <= limb {
            self.limbs.resize(limb + 1, 0);
        }
        self.limbs[limb] |= 1 << (i % 32);
    }

    /// Number of trailing zero bits. Zero for zero.
    pub fn trailing_zeros(&self) -> u32 {
        for (i, &limb) in self.limbs.iter().enumerate() {
            if limb != 0 {
                return i as u32 * 32 + limb.trailing_zeros();
            }
        }
        0
    }

    /// `self >>= n`
    pub fn shr_bits(&mut self, n: u32) {
        if self.is_zero() || n == 0 {
            return;
        }
        let whole = (n / 32) as usize;
        if whole >= self.limbs.len() {
            self.limbs.clear();
            return;
        }
        self.limbs.drain(0..whole);
        let part = n % 32;
        if part > 0 {
            let mut carry = 0u32;
            for limb in self.limbs.iter_mut().rev() {
                let v = (*limb >> part) | carry;
                carry = limb.wrapping_shl(32 - part);
                *limb = v;
            }
        }
        self.trim();
    }

    /// `self -= other`. The caller must ensure `self >= other`.
    pub fn sub_assign(&mut self, other: &Big) {
        debug_assert!(*self >= *other);
        let mut borrow = 0i64;
        for i in 0..self.limbs.len() {
            let rhs = *other.limbs.get(i).unwrap_or(&0) as i64;
            let mut d = self.limbs[i] as i64 - rhs - borrow;
            if d < 0 {
                d += 1 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            self.limbs[i] = d as u32;
        }
        debug_assert_eq!(borrow, 0);
        self.trim();
    }

    /// Schoolbook multiplication.
    pub fn mul_big(&self, other: &Big) -> Big {
        if self.is_zero() || other.is_zero() {
            return Big::zero();
        }
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let v = a as u64 * b as u64 + out[i + j] as u64 + carry;
                out[i + j] = v as u32;
                carry = v >> 32;
            }
            let mut k = i + other.limbs.len();
            while carry != 0 {
                let v = out[k] as u64 + carry;
                out[k] = v as u32;
                carry = v >> 32;
                k += 1;
            }
        }
        let mut b = Big { limbs: out };
        b.trim();
        b
    }

    /// `self /= other`, returning the remainder.
    ///
    /// Binary long division: one shift-and-compare per bit of the quotient. Slower than Knuth's
    /// algorithm D, but the operands here are a few thousand bits at most and this is short
    /// enough to be obviously correct.
    pub fn divmod_big(&self, other: &Big) -> (Big, Big) {
        assert!(!other.is_zero(), "division by zero");
        if *self < *other {
            return (Big::zero(), self.clone());
        }
        let shift = self.bit_len() - other.bit_len();
        let mut rem = self.clone();
        let mut quot = Big::zero();
        let mut d = other.clone();
        d.shl_bits(shift);
        for i in (0..=shift).rev() {
            if d <= rem {
                rem.sub_assign(&d);
                quot.set_bit(i);
            }
            d.shr_bits(1);
        }
        quot.trim();
        (quot, rem)
    }

    /// `self <<= n`
    pub fn shl_bits(&mut self, n: u32) {
        if self.is_zero() || n == 0 {
            return;
        }
        let whole = (n / 32) as usize;
        let part = n % 32;
        if part > 0 {
            let mut carry = 0u32;
            for limb in self.limbs.iter_mut() {
                let v = ((*limb as u64) << part) | carry as u64;
                *limb = v as u32;
                carry = (v >> 32) as u32;
            }
            if carry != 0 {
                self.limbs.push(carry);
            }
        }
        if whole > 0 {
            self.limbs.splice(0..0, core::iter::repeat_n(0, whole));
        }
    }

    /// `self *= m`
    pub fn mul_u32(&mut self, m: u32) {
        if self.is_zero() {
            return;
        }
        if m == 0 {
            self.limbs.clear();
            return;
        }
        let mut carry = 0u64;
        for limb in self.limbs.iter_mut() {
            let v = (*limb as u64) * (m as u64) + carry;
            *limb = v as u32;
            carry = v >> 32;
        }
        while carry != 0 {
            self.limbs.push(carry as u32);
            carry >>= 32;
        }
    }

    /// `self *= 5^n`
    pub fn mul_pow5(&mut self, mut n: u32) {
        while n >= POW5_CHUNK_EXP {
            self.mul_u32(POW5_CHUNK);
            n -= POW5_CHUNK_EXP;
        }
        if n > 0 {
            self.mul_u32(5u32.pow(n));
        }
    }

    /// `self += v`
    pub fn add_u32(&mut self, v: u32) {
        if v == 0 {
            return;
        }
        let mut carry = v as u64;
        for limb in self.limbs.iter_mut() {
            let s = *limb as u64 + carry;
            *limb = s as u32;
            carry = s >> 32;
            if carry == 0 {
                return;
            }
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
    }

    /// `self += other`
    pub fn add(&mut self, other: &Big) {
        if other.is_zero() {
            return;
        }
        if self.limbs.len() < other.limbs.len() {
            self.limbs.resize(other.limbs.len(), 0);
        }
        let mut carry = 0u64;
        for (i, limb) in self.limbs.iter_mut().enumerate() {
            let rhs = *other.limbs.get(i).unwrap_or(&0) as u64;
            let s = *limb as u64 + rhs + carry;
            *limb = s as u32;
            carry = s >> 32;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
    }

    /// `self /= d`, returning the remainder.
    fn divmod_u32(&mut self, d: u32) -> u32 {
        let mut rem = 0u64;
        for limb in self.limbs.iter_mut().rev() {
            let cur = (rem << 32) | (*limb as u64);
            *limb = (cur / d as u64) as u32;
            rem = cur % d as u64;
        }
        self.trim();
        rem as u32
    }

    /// Decimal digits, most significant first. `"0"` when zero.
    pub fn to_decimal(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let mut work = self.clone();
        let mut groups: Vec<u32> = Vec::new();
        while !work.is_zero() {
            groups.push(work.divmod_u32(1_000_000_000));
        }
        let mut s = String::with_capacity(groups.len() * 9);
        // The most significant group has no leading zeros; every later one is zero-padded to 9.
        s.push_str(&groups[groups.len() - 1].to_string());
        for g in groups.iter().rev().skip(1) {
            s.push_str(&format!("{:09}", g));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values() {
        assert_eq!(Big::from_u64(0).to_decimal(), "0");
        assert_eq!(Big::from_u64(1).to_decimal(), "1");
        assert_eq!(Big::from_u64(1_000_000_000).to_decimal(), "1000000000");
        assert_eq!(Big::from_u64(u64::MAX).to_decimal(), "18446744073709551615");
    }

    #[test]
    fn shifting() {
        let mut b = Big::from_u64(1);
        b.shl_bits(64);
        assert_eq!(b.to_decimal(), "18446744073709551616");
        let mut b = Big::from_u64(3);
        b.shl_bits(100);
        assert_eq!(b.to_decimal(), "3802951800684688204490109616128");
    }

    #[test]
    fn powers_of_five() {
        let mut b = Big::from_u64(1);
        b.mul_pow5(0);
        assert_eq!(b.to_decimal(), "1");
        let mut b = Big::from_u64(1);
        b.mul_pow5(13);
        assert_eq!(b.to_decimal(), "1220703125");
        // 5^40, spanning several chunks
        let mut b = Big::from_u64(1);
        b.mul_pow5(40);
        assert_eq!(b.to_decimal(), "9094947017729282379150390625");
    }

    fn big(s: &str) -> Big {
        let mut b = Big::zero();
        for c in s.chars() {
            b.mul_u32(10);
            b.add_u32(c as u32 - '0' as u32);
        }
        b
    }

    #[test]
    fn bit_lengths() {
        assert_eq!(Big::zero().bit_len(), 0);
        assert_eq!(Big::from_u64(1).bit_len(), 1);
        assert_eq!(Big::from_u64(255).bit_len(), 8);
        assert_eq!(Big::from_u64(256).bit_len(), 9);
        assert_eq!(Big::from_u64(u64::MAX).bit_len(), 64);
        let mut b = Big::from_u64(1);
        b.shl_bits(200);
        assert_eq!(b.bit_len(), 201);
    }

    #[test]
    fn from_u128_roundtrips() {
        assert_eq!(Big::from_u128(0).to_decimal(), "0");
        assert_eq!(
            Big::from_u128(u128::MAX).to_decimal(),
            "340282366920938463463374607431768211455"
        );
    }

    #[test]
    fn shifting_right_is_the_inverse_of_left() {
        for bits in [1u32, 5, 31, 32, 33, 64, 100, 129] {
            let mut b = big("123456789012345678901234567890");
            let original = b.clone();
            b.shl_bits(bits);
            b.shr_bits(bits);
            assert_eq!(b, original, "shift by {bits}");
        }
        // Shifting everything out yields zero rather than garbage.
        let mut b = Big::from_u64(0xff);
        b.shr_bits(500);
        assert!(b.is_zero());
        // Truncation really truncates.
        let mut b = Big::from_u64(0b1011);
        b.shr_bits(2);
        assert_eq!(b.to_decimal(), "2");
    }

    #[test]
    fn subtraction() {
        let mut a = big("1000000000000000000000000000000");
        a.sub_assign(&big("1"));
        assert_eq!(a.to_decimal(), "999999999999999999999999999999");
        // Borrow chains across many limbs.
        let mut a = Big::from_u64(1);
        a.shl_bits(128);
        a.sub_assign(&Big::from_u64(1));
        assert_eq!(a.to_decimal(), "340282366920938463463374607431768211455");
        // Subtracting to exactly zero must normalise.
        let mut a = big("42");
        a.sub_assign(&big("42"));
        assert!(a.is_zero());
    }

    #[test]
    fn multiplication() {
        assert!(big("12345").mul_big(&Big::zero()).is_zero());
        assert_eq!(big("12345").mul_big(&Big::one()).to_decimal(), "12345");
        assert_eq!(
            big("123456789").mul_big(&big("987654321")).to_decimal(),
            "121932631112635269"
        );
        // Cross-check against repeated shifting: x * 2^64 == x << 64.
        let mut shifted = big("98765432109876543210");
        shifted.shl_bits(64);
        let mut two64 = Big::from_u64(1);
        two64.shl_bits(64);
        assert_eq!(big("98765432109876543210").mul_big(&two64), shifted);
    }

    #[test]
    fn division() {
        let (q, r) = big("100").divmod_big(&big("7"));
        assert_eq!(
            (q.to_decimal().as_str(), r.to_decimal().as_str()),
            ("14", "2")
        );
        // Exact division leaves no remainder.
        let (q, r) = big("121932631112635269").divmod_big(&big("123456789"));
        assert_eq!(q.to_decimal(), "987654321");
        assert!(r.is_zero());
        // Divisor larger than dividend.
        let (q, r) = big("5").divmod_big(&big("1000"));
        assert!(q.is_zero());
        assert_eq!(r.to_decimal(), "5");
        // Big / big, checked by reconstructing the dividend.
        let n = big("31415926535897932384626433832795028841971693993751");
        let d = big("2718281828459045235360287471352");
        let (q, r) = n.divmod_big(&d);
        let mut back = q.mul_big(&d);
        back.add(&r);
        assert_eq!(back, n);
        assert!(r < d);
    }

    #[test]
    fn zero_is_absorbing() {
        let mut b = Big::from_u64(0);
        b.shl_bits(500);
        b.mul_pow5(500);
        assert!(b.is_zero());
        assert_eq!(b.to_decimal(), "0");
    }
}
