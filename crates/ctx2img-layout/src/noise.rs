//! Deterministic value noise + hashing. No external RNG: everything is a
//! pure function of coordinates and seed, which is what keeps layouts
//! byte-identical across runs and platforms.

pub fn hash64(bytes: &[u8], seed: u64) -> u64 {
    let mut h = 0xcbf29ce484222325u64 ^ seed.wrapping_mul(0x9E3779B97F4A7C15);
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // final avalanche
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h
}

fn lattice(ix: i64, iy: i64, seed: u64) -> f32 {
    let mut h = seed ^ (ix as u64).wrapping_mul(0x9E3779B97F4A7C15);
    h ^= (iy as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    (h & 0xFFFFFF) as f32 / 0xFFFFFF as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Single-octave value noise at (x, y), frequency in lattice cells per unit.
fn value_noise(x: f32, y: f32, freq: f32, seed: u64) -> f32 {
    let fx = x * freq;
    let fy = y * freq;
    let ix = fx.floor() as i64;
    let iy = fy.floor() as i64;
    let tx = smooth(fx - ix as f32);
    let ty = smooth(fy - iy as f32);
    let a = lattice(ix, iy, seed);
    let b = lattice(ix + 1, iy, seed);
    let c = lattice(ix, iy + 1, seed);
    let d = lattice(ix + 1, iy + 1, seed);
    a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty
}

/// Fractal (2-octave) noise in [0,1].
pub fn fbm(x: f32, y: f32, seed: u64) -> f32 {
    0.65 * value_noise(x, y, 5.0, seed) + 0.35 * value_noise(x, y, 11.0, seed ^ 0xABCD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_bounded() {
        for i in 0..100 {
            let x = i as f32 / 17.0;
            let v = fbm(x, x * 0.7, 42);
            assert!((0.0..=1.0).contains(&v));
            assert_eq!(v, fbm(x, x * 0.7, 42));
        }
        assert_ne!(fbm(0.3, 0.3, 1), fbm(0.3, 0.3, 2));
    }
}
