//! Deterministic random numbers for the city generator.

/// SplitMix64: tiny, fast, and identical on every platform.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    /// Independent stream per `(seed, tag, a, b)`, so changing one block
    /// does not reshuffle the rest of the city.
    pub(crate) fn stream(seed: u64, tag: u64, a: i64, b: i64) -> Self {
        let mut r = Rng(seed ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let x = r.next_u64() ^ (a as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        let mut r = Rng(x);
        let y = r.next_u64() ^ (b as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
        Rng(y)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub(crate) fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub(crate) fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    pub(crate) fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// Uniform in `0..n`.
    pub(crate) fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n)) as u32
    }

    pub(crate) fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u32) as usize]
    }
}
