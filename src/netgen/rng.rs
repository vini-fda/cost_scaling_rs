//! Portable LCG used by NETGEN: `i = 7^5 · i mod (2^31 − 1)`.
//!
//! Faithful port of `random.c` from the DIMACS NETGEN sources. The advance step
//! always runs — even when the requested interval is degenerate — so call counts
//! match the C reference exactly. This is intentional and required for
//! bit-identical output parity.

const MULTIPLIER: i64 = 16_807;
const MODULUS: i64 = 2_147_483_647;

/// Park–Miller "minimal standard" generator with Schrage-style 16/15 bit split.
pub(crate) struct Rng {
    state: i64,
}

impl Rng {
    /// Create a new generator from a seed. The seed is expected to be positive;
    /// the public NETGEN entry point rejects non-positive seeds before reaching
    /// here.
    pub(crate) fn new(seed: i64) -> Self {
        Self { state: seed }
    }

    /// Generate a random integer in the inclusive interval `[a, b]`.
    ///
    /// When `b <= a`, the state is still advanced (matching C), and `b` is
    /// returned without using the modulus step.
    pub(crate) fn random(&mut self, a: i64, b: i64) -> i64 {
        let mut hi = MULTIPLIER * (self.state >> 16);
        let mut lo = MULTIPLIER * (self.state & 0xFFFF);
        hi += lo >> 16;
        lo &= 0xFFFF;
        lo += hi >> 15;
        hi &= 0x7FFF;
        lo -= MODULUS;
        self.state = (hi << 16) + lo;
        if self.state < 0 {
            self.state += MODULUS;
        }

        if b <= a {
            return b;
        }
        a + self.state % (b - a + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First 16 outputs of `random(1, 1_000_000)` with seed `13502460`,
    /// captured from a freshly built C `netgen` reference binary.
    /// See `tests/netgen_parity.rs` for the full traces.
    #[test]
    fn first_outputs_match_c_reference() {
        let mut rng = Rng::new(13_502_460);
        let observed: Vec<i64> = (0..16).map(|_| rng.random(1, 1_000_000)).collect();
        // Reference trace captured from the C reference: see
        // `tests/netgen_parity.rs::rng_trace_matches_c`.
        let expected = [
            62_286, 397_840, 652_671, 474_146, 20_605, 661_032, 478_195, 866_960, 719_297, 302_650,
            470_414, 483_395, 753_149, 594_545, 273_660, 183_396,
        ];
        assert_eq!(observed, expected);
    }

    #[test]
    fn returns_b_without_modulus_when_b_le_a() {
        let mut rng_a = Rng::new(13_502_460);
        let mut rng_b = Rng::new(13_502_460);
        // `random(5, 5)` should advance state once but return 5.
        assert_eq!(rng_a.random(5, 5), 5);
        // Same state advance: a fresh call with a normal interval matches a
        // single-advance baseline.
        let _ = rng_b.random(0, 0);
        assert_eq!(rng_a.random(1, 1_000_000), rng_b.random(1, 1_000_000));
    }

    #[test]
    fn state_advance_is_deterministic_for_seed() {
        let mut rng = Rng::new(1);
        // Park-Miller seeded with 1 has well-known first output (16807) for the
        // "raw state" variant. NETGEN's `random` doesn't expose state directly,
        // but `random(0, MODULUS - 2)` returns `state % (MODULUS - 1)`.
        // For seed 1, after one advance state == 16807, so result == 16807.
        assert_eq!(rng.random(0, MODULUS - 2), 16_807);
    }
}
