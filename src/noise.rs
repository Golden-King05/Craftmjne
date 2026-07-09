//! Seeded simplex noise (2D/3D) + fBm helpers + integer hash functions.
//! Pure math — used by worldgen tasks and texture painters.

/// Deterministic PRNG (mulberry32). Returns values in [0, 1).
pub fn mulberry32(seed: u32) -> impl FnMut() -> f32 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b79f5);
        let mut t = (a ^ (a >> 15)).wrapping_mul(a | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f32 / 4294967296.0
    }
}

pub fn hash_str(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// Deterministic uniform [0,1) hashes for feature placement (trees, ores).
pub fn hash2(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(374761393)
        ^ (z as u32).wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    h as f32 / 4294967296.0
}

pub fn hash3(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(374761393)
        ^ (y as u32).wrapping_mul(2246822519)
        ^ (z as u32).wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    h as f32 / 4294967296.0
}

const GRAD3: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, -1.0], [0.0, -1.0, -1.0],
];

pub struct SimplexNoise {
    perm: [u8; 512],
    perm12: [u8; 512],
}

impl SimplexNoise {
    pub fn new(seed: u32) -> Self {
        let mut rand = mulberry32(seed);
        let mut p: [u8; 256] = std::array::from_fn(|i| i as u8);
        for i in (1..256).rev() {
            let j = (rand() * (i as f32 + 1.0)) as usize;
            p.swap(i, j);
        }
        let mut perm = [0u8; 512];
        let mut perm12 = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
            perm12[i] = perm[i] % 12;
        }
        Self { perm, perm12 }
    }

    pub fn noise2(&self, xin: f64, yin: f64) -> f64 {
        const F2: f64 = 0.3660254037844386; // (sqrt(3)-1)/2
        const G2: f64 = 0.21132486540518713; // (3-sqrt(3))/6
        let s = (xin + yin) * F2;
        let i = (xin + s).floor() as i64;
        let j = (yin + s).floor() as i64;
        let t = (i + j) as f64 * G2;
        let x0 = xin - (i as f64 - t);
        let y0 = yin - (j as f64 - t);
        let (i1, j1) = if x0 > y0 { (1i64, 0i64) } else { (0, 1) };
        let x1 = x0 - i1 as f64 + G2;
        let y1 = y0 - j1 as f64 + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;
        let ii = (i & 255) as usize;
        let jj = (j & 255) as usize;

        let mut n = 0.0;
        let mut corner = |x: f64, y: f64, gi: usize| {
            let t = 0.5 - x * x - y * y;
            if t >= 0.0 {
                let g = GRAD3[gi];
                let t2 = t * t;
                n += t2 * t2 * (g[0] * x + g[1] * y);
            }
        };
        corner(x0, y0, self.perm12[ii + self.perm[jj] as usize] as usize);
        corner(x1, y1, self.perm12[ii + i1 as usize + self.perm[jj + j1 as usize] as usize] as usize);
        corner(x2, y2, self.perm12[ii + 1 + self.perm[jj + 1] as usize] as usize);
        70.0 * n
    }

    pub fn noise3(&self, xin: f64, yin: f64, zin: f64) -> f64 {
        const F3: f64 = 1.0 / 3.0;
        const G3: f64 = 1.0 / 6.0;
        let s = (xin + yin + zin) * F3;
        let i = (xin + s).floor() as i64;
        let j = (yin + s).floor() as i64;
        let k = (zin + s).floor() as i64;
        let t = (i + j + k) as f64 * G3;
        let x0 = xin - (i as f64 - t);
        let y0 = yin - (j as f64 - t);
        let z0 = zin - (k as f64 - t);

        let (i1, j1, k1, i2, j2, k2);
        if x0 >= y0 {
            if y0 >= z0 {
                (i1, j1, k1, i2, j2, k2) = (1, 0, 0, 1, 1, 0);
            } else if x0 >= z0 {
                (i1, j1, k1, i2, j2, k2) = (1, 0, 0, 1, 0, 1);
            } else {
                (i1, j1, k1, i2, j2, k2) = (0, 0, 1, 1, 0, 1);
            }
        } else if y0 < z0 {
            (i1, j1, k1, i2, j2, k2) = (0, 0, 1, 0, 1, 1);
        } else if x0 < z0 {
            (i1, j1, k1, i2, j2, k2) = (0, 1, 0, 0, 1, 1);
        } else {
            (i1, j1, k1, i2, j2, k2) = (0, 1, 0, 1, 1, 0);
        }

        let x1 = x0 - i1 as f64 + G3;
        let y1 = y0 - j1 as f64 + G3;
        let z1 = z0 - k1 as f64 + G3;
        let x2 = x0 - i2 as f64 + 2.0 * G3;
        let y2 = y0 - j2 as f64 + 2.0 * G3;
        let z2 = z0 - k2 as f64 + 2.0 * G3;
        let x3 = x0 - 1.0 + 3.0 * G3;
        let y3 = y0 - 1.0 + 3.0 * G3;
        let z3 = z0 - 1.0 + 3.0 * G3;
        let ii = (i & 255) as usize;
        let jj = (j & 255) as usize;
        let kk = (k & 255) as usize;

        let mut n = 0.0;
        let mut corner = |x: f64, y: f64, z: f64, gi: usize| {
            let t = 0.6 - x * x - y * y - z * z;
            if t >= 0.0 {
                let g = GRAD3[gi];
                let t2 = t * t;
                n += t2 * t2 * (g[0] * x + g[1] * y + g[2] * z);
            }
        };
        let p = &self.perm;
        let p12 = &self.perm12;
        corner(x0, y0, z0, p12[ii + p[jj + p[kk] as usize] as usize] as usize);
        corner(x1, y1, z1, p12[ii + i1 + p[jj + j1 + p[kk + k1] as usize] as usize] as usize);
        corner(x2, y2, z2, p12[ii + i2 + p[jj + j2 + p[kk + k2] as usize] as usize] as usize);
        corner(x3, y3, z3, p12[ii + 1 + p[jj + 1 + p[kk + 1] as usize] as usize] as usize);
        32.0 * n
    }

    /// Fractional Brownian motion, output roughly in [-1, 1].
    pub fn fbm2(&self, mut x: f64, mut y: f64, octaves: u32) -> f64 {
        let mut sum = 0.0;
        let mut amp = 1.0;
        let mut norm = 0.0;
        for _ in 0..octaves {
            sum += amp * self.noise2(x, y);
            norm += amp;
            amp *= 0.5;
            x *= 2.0;
            y *= 2.0;
        }
        sum / norm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a = SimplexNoise::new(42);
        let b = SimplexNoise::new(42);
        for i in 0..500 {
            let x = i as f64 * 0.13 - 30.0;
            let y = i as f64 * 0.07 + 11.0;
            assert_eq!(a.noise2(x, y), b.noise2(x, y));
            assert!(a.noise2(x, y).abs() <= 1.0);
            assert!(a.noise3(x, y, x * 0.5).abs() <= 1.0);
        }
    }

    #[test]
    fn hashes_are_uniformish() {
        let mut sum = 0.0;
        for i in 0..1000 {
            let v = hash2(i, i * 7 - 300, 99);
            assert!((0.0..1.0).contains(&v));
            sum += v;
        }
        assert!((sum / 1000.0 - 0.5).abs() < 0.05);
    }
}
