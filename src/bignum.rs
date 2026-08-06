//! A minimal unsigned big integer — just enough to print a dyadic rational exactly.
//!
//! Every finite posit is `frac × 2^s` for an integer `frac` and (possibly negative) `s`, so it has
//! a terminating decimal expansion. For BPosit64 that expansion can run to a few hundred digits,
//! and the significand alone (58 bits) already exceeds what an `f64` can hold — so the decimal
//! display cannot go through `f64` without lying. Hence this.

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

    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
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

    #[test]
    fn zero_is_absorbing() {
        let mut b = Big::from_u64(0);
        b.shl_bits(500);
        b.mul_pow5(500);
        assert!(b.is_zero());
        assert_eq!(b.to_decimal(), "0");
    }
}
