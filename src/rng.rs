//! The scenes' random numbers: xoshiro256++, seeded through SplitMix64. Each element has its own
//! generator seeded from its settings, so a scene is the same on every start and every machine.

pub struct Rng {
    s: [u64; 4],
    spare: Option<f64>,
}

fn splitmix(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: i64) -> Rng {
        let mut x = seed as u64;
        Rng { s: [splitmix(&mut x), splitmix(&mut x), splitmix(&mut x), splitmix(&mut x)], spare: None }
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let out = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        out
    }

    /// A float in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A float in [a, b).
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f64()
    }

    /// An integer in [0, n), without bias.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let zone = u64::MAX - u64::MAX % n;
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % n;
            }
        }
    }

    /// An integer in [start, stop).
    pub fn int_below(&mut self, start: i64, stop: i64) -> i64 {
        start + self.below((stop - start).max(0) as u64) as i64
    }

    /// An integer in [a, b].
    pub fn int(&mut self, a: i64, b: i64) -> i64 {
        self.int_below(a, b + 1)
    }

    pub fn pick<'a, T>(&mut self, seq: &'a [T]) -> &'a T {
        &seq[self.below(seq.len() as u64) as usize]
    }

    /// Index chosen with probability proportional to its weight.
    pub fn weighted(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut r = self.f64() * total;
        for (i, w) in weights.iter().enumerate() {
            if r < *w {
                return i;
            }
            r -= w;
        }
        weights.len() - 1
    }

    pub fn shuffle<T>(&mut self, x: &mut [T]) {
        for i in (1..x.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            x.swap(i, j);
        }
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n + 8);
        while out.len() < n {
            out.extend_from_slice(&self.next_u64().to_le_bytes());
        }
        out.truncate(n);
        out
    }

    /// Normal distribution by Box-Muller; the second value is kept for the next call.
    pub fn normal(&mut self, mu: f64, sigma: f64) -> f64 {
        if let Some(z) = self.spare.take() {
            return mu + z * sigma;
        }
        let (u, v) = (1.0 - self.f64(), self.f64());
        let r = (-2.0 * u.ln()).sqrt();
        let a = 2.0 * std::f64::consts::PI * v;
        self.spare = Some(r * a.sin());
        mu + r * a.cos() * sigma
    }
}
