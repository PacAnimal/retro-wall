//! A model made of boxes, as a block game's animals are: each part a box in the model's own units,
//! some turned on a hinge (a leg about its hip, a head about its neck, a door, a wing); the whole stood
//! at a point of the world and turned about the upright.
//!
//! Posed once a frame (`Pose::new`): the hinges' turns, the eye in each part's frame and a box about
//! each part as it is turned are worked out then, not for every pixel. A ray (`Pose::hit`) is tried on
//! the box about the whole first, then on each part's box quickly, and only the parts it may meet
//! nearer than the nearest yet are tested exactly, so a pixel costs a few multiplications where the
//! model is not.

use super::boxes::{Ray, grown, slab};
use super::camera::{Camera, Px, V3, bounds, centres, hull, span};

/// A turn about one of the model's axes (0 x, 1 y, 2 z) through `pivot`, in radians.
#[derive(Clone, Copy)]
pub struct Hinge {
    pub axis: usize,
    pub pivot: V3,
    pub angle: f64,
}

/// Where a ray meets a posed model: how far along it (in the world's units), the part, the face's axis
/// and side in the part's frame, the point met in the part's frame (the model's units, before its
/// hinge's turn), and the face's normal in the world.
pub struct RigHit {
    pub t: f64,
    pub part: usize,
    pub axis: usize,
    pub sign: f64,
    pub local: V3,
    pub normal: V3,
}

struct Part {
    lo: V3,
    hi: V3,
    /// Its hinge's axis, cosine and sine; None where it is not turned.
    turn: Option<(usize, f64, f64)>,
    /// The eye in its frame, and the box about it as it is turned, in the model's frame, grown.
    eye: V3,
    blo: V3,
    bhi: V3,
}

/// A model as it stands, seen from one eye.
pub struct Pose {
    at: V3,
    /// Its own axes (across, up, forward) in the world, and the world's unit in its own.
    axes: [V3; 3],
    scale: f64,
    parts: Vec<Part>,
    /// The eye in its frame, and the box about all of its parts there, grown.
    eye: V3,
    lo: V3,
    hi: V3,
}

/// `v` turned about `axis` by the angle of cosine `c` and sine `s`: backwards (`k` = 1, into a hinged
/// part's frame) or forwards (`k` = -1, out of it).
fn turn(v: V3, axis: usize, c: f64, s: f64, k: f64) -> V3 {
    let (a, b) = [(1, 2), (2, 0), (0, 1)][axis];
    let mut out = v;
    out[a] = v[a] * c + v[b] * s * k;
    out[b] = -v[a] * s * k + v[b] * c;
    out
}

/// A direction in the world in a frame of `axes`, and back. Turned only about the upright (an axis
/// of 0s and a 1), each is what multiplying by the cosine and sine alone makes, to the bit.
fn into(axes: [V3; 3], v: V3) -> V3 {
    axes.map(|a| a[0] * v[0] + a[1] * v[1] + a[2] * v[2])
}
fn out_of(axes: [V3; 3], v: V3) -> V3 {
    std::array::from_fn(|i| axes[0][i] * v[0] + axes[1][i] * v[1] + axes[2][i] * v[2])
}

/// `v` turned about `axis` through `pivot`.
fn turn_about(v: V3, pivot: V3, axis: usize, c: f64, s: f64, k: f64) -> V3 {
    let r = turn([v[0] - pivot[0], v[1] - pivot[1], v[2] - pivot[2]], axis, c, s, k);
    [r[0] + pivot[0], r[1] + pivot[1], r[2] + pivot[2]]
}

impl Pose {
    /// A model of `boxes` (low and high corners, in its units, `scale` of them to the world's) with
    /// each box's hinge, standing at `at` turned by `yaw` about the upright, seen from `eye`.
    pub fn new(boxes: impl IntoIterator<Item = (V3, V3, Option<Hinge>)>, at: V3, yaw: f64, scale: f64, eye: V3) -> Pose {
        let (c, s) = (yaw.cos(), yaw.sin());
        Pose::oriented(boxes, at, [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]], scale, eye)
    }

    /// The same, its own axes (across, up and forward, each a unit and square to the others) the
    /// world's `axes`: on a wall, upside down, head over heels.
    pub fn oriented(boxes: impl IntoIterator<Item = (V3, V3, Option<Hinge>)>, at: V3, axes: [V3; 3], scale: f64, eye: V3) -> Pose {
        let e = into(axes, [eye[0] - at[0], eye[1] - at[1], eye[2] - at[2]]).map(|v| v * scale);
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        let parts = boxes
            .into_iter()
            .map(|(plo, phi, hinge)| {
                let turned = hinge.filter(|h| h.angle != 0.0).map(|h| (h.axis, h.angle.cos(), h.angle.sin()));
                let pivot = hinge.map_or([0.0; 3], |h| h.pivot);
                let (mut blo, mut bhi) = ([f64::MAX; 3], [f64::MIN; 3]);
                for k in 0..8 {
                    let corner = [0, 1, 2].map(|i| if k >> i & 1 == 0 { plo[i] } else { phi[i] });
                    let out = turned.map_or(corner, |(axis, c, s)| turn_about(corner, pivot, axis, c, s, -1.0));
                    for i in 0..3 {
                        blo[i] = blo[i].min(out[i]);
                        bhi[i] = bhi[i].max(out[i]);
                    }
                }
                for i in 0..3 {
                    lo[i] = lo[i].min(blo[i]);
                    hi[i] = hi[i].max(bhi[i]);
                }
                let peye = turned.map_or(e, |(axis, c, s)| turn_about(e, pivot, axis, c, s, 1.0));
                let (blo, bhi) = grown(blo, bhi);
                Part { lo: plo, hi: phi, turn: turned, eye: peye, blo, bhi }
            })
            .collect();
        let (lo, hi) = grown(lo, hi);
        Pose { at, axes, scale, parts, eye: e, lo, hi }
    }

    /// The corners of a box in its frame (`lo`, `hi`), in the world.
    fn corners(&self, lo: V3, hi: V3) -> [V3; 8] {
        std::array::from_fn(|n| {
            let v = [0, 1, 2].map(|i| if n >> i & 1 == 0 { lo[i] } else { hi[i] } / self.scale);
            // added to where it stands term by term, as turned about the upright it was, to the bit
            let a = &self.axes;
            std::array::from_fn(|i| self.at[i] + a[0][i] * v[0] + a[1][i] * v[1] + a[2][i] * v[2])
        })
    }

    /// The pixels a box in its frame can be seen in, all of it in front of the eye; None otherwise.
    fn box_pixels(&self, cam: &Camera, lo: V3, hi: V3) -> Option<Px> {
        match bounds(cam, self.corners(lo, hi)) {
            (Some(r), true) => Some(centres(r)),
            _ => None,
        }
    }

    /// Whether a point of the world is within the box about all of its parts.
    pub fn contains(&self, p: V3) -> bool {
        let l = into(self.axes, [p[0] - self.at[0], p[1] - self.at[1], p[2] - self.at[2]]).map(|v| v * self.scale);
        (0..3).all(|i| self.lo[i] <= l[i] && l[i] <= self.hi[i])
    }

    /// The pixels it can be seen in, all of it in front of the eye; None otherwise.
    pub fn pixels(&self, cam: &Camera) -> Option<Px> {
        self.box_pixels(cam, self.lo, self.hi)
    }

    /// How far its nearest point can be from the eye, in the world's units: nowhere along a ray does it
    /// meet the model nearer than this.
    pub fn near(&self) -> f64 {
        let off: f64 = (0..3).map(|i| (self.lo[i] - self.eye[i]).max(self.eye[i] - self.hi[i]).max(0.0).powi(2)).sum();
        // the box is grown, so this is short of the parts by far more than its rounding
        off.sqrt() / self.scale
    }

    /// Trace it into the pixels of `clip`: `each` is given each pixel it is seen in, the ray there and
    /// what the ray meets. Only the pixels some part can be seen in are traced, and in each only those
    /// parts, so a pixel beside a leg costs nothing and one on it a part or two. `scene` is how far off
    /// what is already in a pixel is: where that is nearer than the model can be (`near`), behind a
    /// house, say, the pixel is not traced.
    pub fn trace(&self, cam: &Camera, clip: Px, scene: impl Fn(i32, i32) -> f32, mut each: impl FnMut(i32, i32, V3, RigHit)) {
        let near = self.near() as f32;
        assert!(self.parts.len() <= 64, "a model of more than 64 parts");
        // each part's pixels; a part partly behind the eye, anywhere
        // each part's outline on the screen, all of it in front of the eye; else anywhere
        let outlines: Vec<Option<Vec<(f64, f64)>>> = self
            .parts
            .iter()
            .map(|p| {
                let pts: Option<Vec<(f64, f64)>> = self.corners(p.blo, p.bhi).iter().map(|&c| cam.project(c).map(|(x, y, _)| (x, y))).collect();
                pts.map(hull)
            })
            .collect();
        let columns: Vec<V3> = (clip.0..clip.2).map(|x| cam.column(x as f64 + 0.5)).collect();
        let mut spans: Vec<(i32, i32, u64)> = Vec::with_capacity(self.parts.len());
        for y in clip.1..clip.3 {
            let row = cam.row(y as f64 + 0.5);
            spans.clear();
            for (i, o) in outlines.iter().enumerate() {
                let s = match o {
                    Some(o) => span(o, y),
                    None => Some((clip.0, clip.2)),
                };
                if let Some((x0, x1)) = s {
                    spans.push((x0, x1, 1 << i));
                }
            }
            if spans.is_empty() {
                continue;
            }
            for x in clip.0..clip.2 {
                let parts = spans.iter().filter(|s| s.0 <= x && x < s.1).fold(0u64, |m, s| m | s.2);
                if parts == 0 || scene(x, y) <= near {
                    continue;
                }
                let d = Camera::through(columns[(x - clip.0) as usize], row);
                if let Some(h) = self.hit_among(d, parts) {
                    each(x, y, d, h);
                }
            }
        }
    }

    /// Where the ray from the eye along `d` meets it: the nearest of its parts. A hinged part's frame is
    /// only turned, so the distances along the ray are the same in each frame, and the quick tests are
    /// all in the model's.
    pub fn hit(&self, d: V3) -> Option<RigHit> {
        self.hit_among(d, u64::MAX)
    }

    /// As `hit`, of the parts in `parts` (a bit each); the others the ray is known to miss.
    fn hit_among(&self, d: V3, parts: u64) -> Option<RigHit> {
        let ld = into(self.axes, d).map(|v| v * self.scale);
        let ray = Ray::new(self.eye, ld);
        // from `trace`, the parts' rectangles have already left out what misses the whole
        if parts == u64::MAX {
            ray.enters(self.lo, self.hi)?;
        }
        // the nearest part met: its index, how far, the face, and the ray in its frame
        let mut best: Option<(usize, f64, usize, f64, V3)> = None;
        for (pi, p) in self.parts.iter().enumerate().filter(|&(i, _)| parts >> i & 1 == 1) {
            match ray.enters(p.blo, p.bhi) {
                Some(near) if best.is_none_or(|b| near <= b.1) => {}
                _ => continue,
            }
            let rd = p.turn.map_or(ld, |(axis, c, s)| turn(ld, axis, c, s, 1.0));
            let Some((t, axis, sign)) = slab(p.eye, rd, p.lo, p.hi) else { continue };
            if best.is_some_and(|b| b.1 <= t) {
                continue;
            }
            best = Some((pi, t, axis, sign, rd));
        }
        let (pi, t, axis, sign, rd) = best?;
        let p = &self.parts[pi];
        let local: V3 = std::array::from_fn(|k| p.eye[k] + rd[k] * t);
        let mut n = [0.0; 3];
        n[axis] = sign;
        // the normal turned out of its hinge, then out into the world
        let n = p.turn.map_or(n, |(axis, c, s)| turn(n, axis, c, s, -1.0));
        let normal = out_of(self.axes, n);
        Some(RigHit { t, part: pi, axis, sign, local, normal })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::trace::camera::norm;

    /// A model of a few boxes, some hinged about each axis, and its pose.
    fn model(rng: &mut Rng) -> Vec<(V3, V3, Option<Hinge>)> {
        (0..rng.int(1, 9))
            .map(|_| {
                let lo: V3 = std::array::from_fn(|_| rng.range(-8.0, 6.0));
                let hi: V3 = std::array::from_fn(|i| lo[i] + rng.range(1.0, 8.0));
                let hinge =
                    (rng.f64() < 0.6).then(|| Hinge { axis: rng.below(3) as usize, pivot: [0.0, hi[1], (lo[2] + hi[2]) * 0.5], angle: rng.range(-1.0, 1.0) });
                (lo, hi, hinge)
            })
            .collect()
    }

    /// The nearest part a ray meets, by testing every part exactly in its own frame.
    fn every_part(boxes: &[(V3, V3, Option<Hinge>)], at: V3, yaw: f64, scale: f64, o: V3, d: V3) -> Option<(usize, f64)> {
        let (c, s) = (yaw.cos(), yaw.sin());
        let back = |v: V3| [v[0] * c - v[2] * s, v[1], v[0] * s + v[2] * c].map(|x| x * scale);
        let (lo, ld) = (back([o[0] - at[0], o[1] - at[1], o[2] - at[2]]), back(d));
        let mut best: Option<(usize, f64)> = None;
        for (i, &(plo, phi, h)) in boxes.iter().enumerate() {
            let (ro, rd) = match h.filter(|h| h.angle != 0.0) {
                Some(h) => {
                    let (hc, hs) = (h.angle.cos(), h.angle.sin());
                    (turn_about(lo, h.pivot, h.axis, hc, hs, 1.0), turn(ld, h.axis, hc, hs, 1.0))
                }
                None => (lo, ld),
            };
            if let Some((t, _, _)) = super::slab(ro, rd, plo, phi)
                && best.is_none_or(|b| t < b.1)
            {
                best = Some((i, t));
            }
        }
        best
    }

    #[test]
    fn a_pose_meets_what_tracing_every_part_meets() {
        let mut rng = Rng::new(7);
        let mut met = 0;
        for _ in 0..300 {
            let boxes = model(&mut rng);
            let at = [rng.range(-20.0, 20.0), rng.range(-2.0, 2.0), rng.range(10.0, 40.0)];
            let (yaw, eye) = (rng.range(-3.0, 3.0), [0.0, 4.0, 0.0]);
            let pose = Pose::new(boxes.iter().copied(), at, yaw, 16.0, eye);
            for _ in 0..200 {
                let aim = [at[0] + rng.range(-1.0, 1.0), at[1] + rng.range(-0.5, 1.5), at[2] + rng.range(-1.0, 1.0)];
                let d = norm([aim[0] - eye[0], aim[1] - eye[1], aim[2] - eye[2]]);
                let want = every_part(&boxes, at, yaw, 16.0, eye, d);
                let got = pose.hit(d).map(|h| (h.part, h.t));
                assert_eq!(got, want);
                met += want.is_some() as usize;
            }
        }
        assert!(met > 5000, "many rays meet the model: {met}");
    }

    #[test]
    fn a_pose_turned_any_way_meets_what_the_upright_one_does_in_its_frame() {
        let mut rng = Rng::new(11);
        for _ in 0..200 {
            let boxes = model(&mut rng);
            let at = [rng.range(-20.0, 20.0), rng.range(-2.0, 2.0), rng.range(10.0, 40.0)];
            // turned upright, the same to the bit as turned by a yaw
            let yaw = rng.range(-3.0, 3.0);
            let (c, s) = (yaw.cos(), yaw.sin());
            let eye = [0.0, 4.0, 0.0];
            let (a, b) = (
                Pose::new(boxes.iter().copied(), at, yaw, 16.0, eye),
                Pose::oriented(boxes.iter().copied(), at, [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]], 16.0, eye),
            );
            // any way: its axes a random turn
            let up = norm([rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)]);
            let f = norm(crate::trace::cross(up, norm([rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)])));
            let axes = [crate::trace::cross(up, f), up, f];
            let turned = Pose::oriented(boxes.iter().copied(), at, axes, 16.0, eye);
            let local = |v: V3| into(axes, v);
            let upright = Pose::new(boxes.iter().copied(), [0.0; 3], 0.0, 16.0, local([eye[0] - at[0], eye[1] - at[1], eye[2] - at[2]]));
            for _ in 0..100 {
                let aim = [at[0] + rng.range(-1.0, 1.0), at[1] + rng.range(-1.0, 1.0), at[2] + rng.range(-1.0, 1.0)];
                let d = norm([aim[0] - eye[0], aim[1] - eye[1], aim[2] - eye[2]]);
                let (ha, hb) = (a.hit(d), b.hit(d));
                assert_eq!(
                    ha.as_ref().map(|h| (h.part, h.t.to_bits(), h.normal.map(f64::to_bits))),
                    hb.as_ref().map(|h| (h.part, h.t.to_bits(), h.normal.map(f64::to_bits)))
                );
                let (ht, hu) = (turned.hit(d), upright.hit(local(d)));
                assert_eq!(ht.as_ref().map(|h| h.part), hu.as_ref().map(|h| h.part));
                if let (Some(ht), Some(hu)) = (ht, hu) {
                    assert!((ht.t - hu.t).abs() < 1e-9);
                    let n = local(ht.normal);
                    assert!((0..3).all(|i| (n[i] - hu.normal[i]).abs() < 1e-9), "its normal turned with it");
                }
            }
            assert_eq!(a.pixels(&Camera::new(eye, 0.0, 0.0, 60.0, 320, 180)), b.pixels(&Camera::new(eye, 0.0, 0.0, 60.0, 320, 180)));
        }
    }

    #[test]
    fn tracing_a_region_meets_what_each_pixel_does_where_nothing_nearer_hides_it() {
        let mut rng = Rng::new(3);
        let cam = Camera::new([0.0, 4.0, 0.0], 0.0, -0.1, 60.0, 320, 180);
        for _ in 0..40 {
            let boxes = model(&mut rng);
            let at = [rng.range(-4.0, 4.0), rng.range(1.0, 4.0), rng.range(8.0, 16.0)];
            let pose = Pose::new(boxes.iter().copied(), at, rng.range(-3.0, 3.0), 16.0, cam.eye);
            // a wall part way through the screen, at a distance the model may be either side of
            let wall = rng.range(6.0, 18.0) as f32;
            let scene = |x: i32, _y: i32| if x < 160 { wall } else { f32::INFINITY };
            let mut traced = std::collections::HashMap::new();
            pose.trace(&cam, (0, 0, 320, 180), scene, |x, y, _, h| {
                traced.insert((x, y), (h.part, h.t));
            });
            for y in 0..180 {
                for x in 0..320 {
                    let h = pose.hit(cam.ray(x as f64 + 0.5, y as f64 + 0.5)).map(|h| (h.part, h.t));
                    match traced.get(&(x, y)) {
                        Some(&got) => assert_eq!(Some(got), h, "at {x}, {y}"),
                        // left out only where it is not there, or behind the scene
                        None => assert!(h.is_none_or(|(_, t)| t as f32 >= scene(x, y)), "lost at {x}, {y}: {h:?}"),
                    }
                }
            }
        }
    }
}
