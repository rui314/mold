//! This file implements HyperLogLog algorithm, which estimates
//! the number of unique items in a given multiset.
//!
//! For more info, read
//! https://engineering.fb.com/2018/12/13/data-infrastructure/hyperloglog

/// Estimates the number of distinct values in a stream of hashes.
///
/// Each hash selects a register by its low bits; the register keeps the
/// longest run of leading zeros seen, plus one. The estimate is a scaled
/// harmonic mean of the registers.
#[derive(Clone)]
pub struct HyperLogLog {
    registers: [u8; Self::NUM_REGISTERS],
}

impl HyperLogLog {
    const NUM_REGISTERS: usize = 2048;
    const ALPHA: f64 = 0.79402;

    #[inline]
    pub fn insert(&mut self, hash: u64) {
        let register = &mut self.registers[hash as usize & (Self::NUM_REGISTERS - 1)];
        *register = (*register).max(hash.leading_zeros() as u8 + 1);
    }

    /// Combines two estimators, as if every value had been inserted into
    /// one of them.
    #[inline]
    pub fn merged(mut self, other: &Self) -> Self {
        for (a, b) in self.registers.iter_mut().zip(&other.registers) {
            *a = (*a).max(*b);
        }
        self
    }

    #[inline]
    pub fn cardinality(&self) -> u64 {
        let z: f64 = self.registers.iter().map(|&r| 2f64.powi(-i32::from(r))).sum();
        (Self::ALPHA * (Self::NUM_REGISTERS * Self::NUM_REGISTERS) as f64 / z) as u64
    }
}

impl Default for HyperLogLog {
    fn default() -> Self {
        Self { registers: [0; Self::NUM_REGISTERS] }
    }
}

impl Extend<u64> for HyperLogLog {
    fn extend<I: IntoIterator<Item = u64>>(&mut self, hashes: I) {
        for hash in hashes {
            self.insert(hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_within_a_few_percent() {
        let mut hll = HyperLogLog::default();
        hll.extend((0..100_000u64).map(|i| xxhash_rust::xxh3::xxh3_64(&i.to_le_bytes())));
        let n = hll.cardinality();
        // The scale constant follows mold, whose estimates run about 10% high.
        assert!((95_000..125_000).contains(&n), "estimate {n}");
    }
}
