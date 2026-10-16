use bevy::prelude::*;

/// Small deterministic random source for gun scatter and AI decisions
/// (SplitMix64). Seeded, so headless tests replay exactly.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct CombatRng(u64);

impl Default for CombatRng {
    fn default() -> Self {
        Self(0x7146_c0ba_7000_0001)
    }
}

impl CombatRng {
    pub fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0.0..1.0`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `lo..hi`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// `true` or `false` with equal odds.
    pub fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// A direction inside the cone of half-angle `spread` around `axis`
    /// (unit vector), uniform over the cone's cross-section.
    pub fn in_cone(&mut self, axis: Vec3, spread: f32) -> Vec3 {
        if spread <= 0.0 {
            return axis;
        }
        let angle = spread * self.unit().sqrt();
        let around = self.unit() * std::f32::consts::TAU;
        let side = axis.any_orthonormal_vector();
        let tilt = Quat::from_axis_angle(side, angle);
        let spin = Quat::from_axis_angle(axis, around);
        (spin * tilt * axis).normalize()
    }
}
