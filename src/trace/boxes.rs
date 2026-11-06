//! A ray and a box with its sides along the axes: exactly, where it meets the box and on which face;
//! quickly, whether it might, to leave most boxes untested.

use super::V3;

/// Where a ray meets a box from outside: how far, and the face's axis and side.
pub fn slab(o: V3, d: V3, lo: V3, hi: V3) -> Option<(f64, usize, f64)> {
    let (mut t0, mut t1, mut axis, mut sign) = (f64::MIN, f64::MAX, 0, 1.0);
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i] < lo[i] || o[i] > hi[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
        let (near, far) = if a < b { (a, b) } else { (b, a) };
        if near > t0 {
            t0 = near;
            axis = i;
            sign = if d[i] > 0.0 { -1.0 } else { 1.0 };
        }
        t1 = t1.min(far);
    }
    if t0 > t1 || t1 < 0.0 || t0 < 0.0 {
        return None;
    }
    Some((t0, axis, sign))
}

/// How far a box is grown for the quick test (`Ray::enters`), in the box's own units: far more than that
/// test's rounding, so it never misses a box `slab` would find. Boxes are made of units of this order
/// or larger.
pub const PAD: f64 = 0.01;

/// A ray made ready for the quick test: its reciprocal, worked out once for every box it is tried on.
/// A division costs many multiplications, more so on a small processor.
#[derive(Clone, Copy)]
pub struct Ray {
    pub o: V3,
    pub d: V3,
    inv: V3,
}

impl Ray {
    pub fn new(o: V3, d: V3) -> Ray {
        // one division for the three: the quick test's padding holds far more than its rounding
        let p = d[0] * d[1] * d[2];
        let inv = if p.abs() > 1e-30 {
            let r = 1.0 / p;
            [d[1] * d[2] * r, d[0] * d[2] * r, d[0] * d[1] * r]
        } else {
            d.map(|v| 1.0 / v)
        };
        Ray { o, d, inv }
    }

    /// How far along it it enters the box, where it passes through it anywhere ahead of its origin;
    /// quickly, so not to the last bit. Give it a box grown by `PAD`: it then enters a little before
    /// the box itself does, and passes through it wherever `slab` finds the box.
    pub fn enters(&self, lo: V3, hi: V3) -> Option<f64> {
        let (mut t0, mut t1) = (f64::MIN, f64::MAX);
        for i in 0..3 {
            if self.d[i].abs() < 1e-12 {
                if self.o[i] < lo[i] || self.o[i] > hi[i] {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo[i] - self.o[i]) * self.inv[i], (hi[i] - self.o[i]) * self.inv[i]);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        (t0 <= t1 && t1 >= 0.0).then_some(t0)
    }
}

/// A box grown by `PAD` on every side.
pub fn grown(lo: V3, hi: V3) -> (V3, V3) {
    (lo.map(|v| v - PAD), hi.map(|v| v + PAD))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn the_quick_test_finds_every_box_the_exact_one_does() {
        let mut rng = Rng::new(1);
        let (mut met, mut tried) = (0, 0);
        while met < 2000 {
            let lo: V3 = std::array::from_fn(|_| rng.range(-8.0, 8.0));
            let hi: V3 = std::array::from_fn(|i| lo[i] + rng.range(0.5, 8.0));
            let o: V3 = std::array::from_fn(|_| rng.range(-200.0, 200.0));
            // towards a point about the box, so many rays graze it
            let aim: V3 = std::array::from_fn(|i| rng.range(lo[i] - 1.0, hi[i] + 1.0));
            let d: V3 = std::array::from_fn(|i| aim[i] - o[i]);
            tried += 1;
            let Some((t, _, _)) = slab(o, d, lo, hi) else { continue };
            met += 1;
            let (glo, ghi) = grown(lo, hi);
            let near = Ray::new(o, d).enters(glo, ghi);
            assert!(near.is_some_and(|n| n <= t), "missed: {o:?} {d:?} {lo:?} {hi:?}, exactly at {t}, quickly {near:?}");
        }
        assert!(tried > met, "some rays miss");
    }
}
