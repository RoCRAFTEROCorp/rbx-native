//! Seeded gradient noise for the Generate tool. Deterministic for a seed on
//! every platform: only integer hashing and f32 arithmetic, no tables built
//! from a random source.

fn hash(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// A value in 0..1 fixed by the seed and lattice point.
pub(crate) fn unit(seed: u32, x: i32, y: i32, z: i32) -> f32 {
    (hash(seed, x, y, z) >> 8) as f32 / (1u32 << 24) as f32
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn gradient2(seed: u32, x: i32, y: i32, dx: f32, dy: f32) -> f32 {
    let angle = unit(seed, x, y, 0) * std::f32::consts::TAU;
    angle.cos() * dx + angle.sin() * dy
}

/// Perlin-style gradient noise, roughly -1..1.
pub(crate) fn noise2(seed: u32, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (u, v) = (fade(fx), fade(fy));
    let a = lerp(
        gradient2(seed, x0, y0, fx, fy),
        gradient2(seed, x0 + 1, y0, fx - 1.0, fy),
        u,
    );
    let b = lerp(
        gradient2(seed, x0, y0 + 1, fx, fy - 1.0),
        gradient2(seed, x0 + 1, y0 + 1, fx - 1.0, fy - 1.0),
        u,
    );
    lerp(a, b, v) * std::f32::consts::SQRT_2
}

fn gradient3(seed: u32, cell: [i32; 3], d: [f32; 3]) -> f32 {
    // Twelve cube-edge directions, as in improved Perlin noise.
    const G: [[f32; 3]; 12] = [
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
        [1.0, 0.0, -1.0],
        [-1.0, 0.0, -1.0],
        [0.0, 1.0, 1.0],
        [0.0, -1.0, 1.0],
        [0.0, 1.0, -1.0],
        [0.0, -1.0, -1.0],
    ];
    let g = G[(hash(seed, cell[0], cell[1], cell[2]) % 12) as usize];
    g[0] * d[0] + g[1] * d[1] + g[2] * d[2]
}

pub(crate) fn noise3(seed: u32, p: [f32; 3]) -> f32 {
    let base = p.map(|v| v.floor() as i32);
    let f: [f32; 3] = std::array::from_fn(|a| p[a] - base[a] as f32);
    let w = f.map(fade);
    let corner = |dx: i32, dy: i32, dz: i32| {
        gradient3(
            seed,
            [base[0] + dx, base[1] + dy, base[2] + dz],
            [f[0] - dx as f32, f[1] - dy as f32, f[2] - dz as f32],
        )
    };
    let x00 = lerp(corner(0, 0, 0), corner(1, 0, 0), w[0]);
    let x10 = lerp(corner(0, 1, 0), corner(1, 1, 0), w[0]);
    let x01 = lerp(corner(0, 0, 1), corner(1, 0, 1), w[0]);
    let x11 = lerp(corner(0, 1, 1), corner(1, 1, 1), w[0]);
    lerp(lerp(x00, x10, w[1]), lerp(x01, x11, w[1]), w[2])
}

/// Fractal sum of `octaves` layers of [`noise2`], each twice the frequency
/// and half the amplitude of the last, normalized back to about -1..1.
pub(crate) fn fbm2(seed: u32, x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut frequency, mut total) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += amplitude * noise2(seed.wrapping_add(octave * 7919), x * frequency, y * frequency);
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    sum / total
}

/// Sharp-crested noise for mountain ranges: 1 on a ridge, falling to 0.
pub(crate) fn ridged2(seed: u32, x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut frequency, mut total) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        let n = 1.0 - noise2(seed.wrapping_add(octave * 104_729), x * frequency, y * frequency).abs();
        sum += amplitude * n * n;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    sum / total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_bounded_and_zero_on_the_lattice() {
        assert_eq!(noise2(7, 3.3, -1.7), noise2(7, 3.3, -1.7));
        assert_ne!(noise2(7, 3.3, -1.7), noise2(8, 3.3, -1.7));
        assert_eq!(noise2(1, 4.0, 5.0), 0.0);
        assert_eq!(noise3(1, [4.0, 5.0, 6.0]), 0.0);
        for i in 0..2000 {
            let x = i as f32 * 0.137 - 50.0;
            let y = i as f32 * 0.071 + 3.0;
            assert!(noise2(3, x, y).abs() <= 1.01);
            assert!(noise3(3, [x, y, x * 0.5]).abs() <= 1.01);
            assert!((0.0..=1.0).contains(&ridged2(3, x, y, 4)));
            assert!(fbm2(3, x, y, 5).abs() <= 1.01);
        }
    }

    #[test]
    fn noise_varies() {
        let values: Vec<f32> = (0..50).map(|i| noise2(9, i as f32 * 0.31, 0.5)).collect();
        let spread = values.iter().cloned().fold(f32::MIN, f32::max)
            - values.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread > 0.5);
    }
}
