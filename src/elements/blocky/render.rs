//! The tracer: a ray from the eye through each pixel, stepped block by block through the grid until it
//! meets something, and that face shaded as a block game shades it: its picture (the level for its
//! distance), the face's own shade by its side, the sky's light smoothed over its corners and
//! darkened where blocks close round it. Leaves and plants are seen through where their pictures are
//! clear, water is a tinted pane over what lies under it, the distance fades into the haze, and the
//! viewer's arm is held out in front. Over the screen's rows in parallel, into two pictures: what is seen,
//! unlit, clear where the sky shows (so the clouds can go between), and the light on it, the sky's
//! and the torches' apart, with the haze; the time of day lights them each frame (`sky`).

use super::mobs::slab;
use super::textures::{ARM, Rgba, Textures, WATER as WATER_FACE};
use super::world::*;

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> V3 {
    mul(a, 1.0 / dot(a, a).sqrt())
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// The eye, where it looks, and its lens.
#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: V3,
    pub forward: V3,
    pub right: V3,
    pub up: V3,
    /// tan of half the vertical field of view, and the screen's width over its height.
    pub tan: f64,
    pub aspect: f64,
    pub w: usize,
    pub h: usize,
}

impl Camera {
    pub fn new(eye: V3, yaw: f64, pitch: f64, fov: f64, w: usize, h: usize) -> Camera {
        let forward = [pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()];
        let right = norm(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        Camera { eye, forward, right, up, tan: (fov.to_radians() / 2.0).tan(), aspect: w as f64 / h as f64, w, h }
    }
    /// The ray through a point of the screen, in pixels from its top left.
    pub fn ray(&self, px: f64, py: f64) -> V3 {
        let x = (2.0 * px / self.w as f64 - 1.0) * self.tan * self.aspect;
        let y = (1.0 - 2.0 * py / self.h as f64) * self.tan;
        norm(add(add(self.forward, mul(self.right, x)), mul(self.up, y)))
    }
    /// The angle one pixel spans.
    pub fn pixel(&self) -> f64 {
        2.0 * self.tan / self.h as f64
    }
    /// Where a point of the world falls on the screen, in pixels from its top left, and how many pixels a
    /// block spans there; None behind the eye.
    pub fn project(&self, p: V3) -> Option<(f64, f64, f64)> {
        let d = add(p, mul(self.eye, -1.0));
        let z = dot(d, self.forward);
        if z < 0.2 {
            return None;
        }
        let (x, y) = (dot(d, self.right) / z, dot(d, self.up) / z);
        let (w, h) = (self.w as f64, self.h as f64);
        Some(((x / (self.tan * self.aspect) + 1.0) * w / 2.0, (1.0 - y / self.tan) * h / 2.0, h / (2.0 * self.tan * z)))
    }
}

/// The light curve: a level of light, 0 to 15, as brightness, 0 to 1.
fn bright(level: f32) -> f32 {
    const DARK: f32 = 0.104; // 0.86 ^ 15
    (0.86f32.powf(15.0 - level) - DARK) / (1.0 - DARK)
}

/// What is seen at a point, unlit, and the light on it: the sky's and the torches', each 0 to 1; and
/// which way it faces, for the sun's light (`facing`).
#[derive(Clone, Copy)]
pub struct Lit {
    pub c: [f32; 3],
    pub sky: f32,
    pub torch: f32,
    pub n: u8,
    /// What it is, for what moves: water, falling water, leaves, a plant; or nothing that moves.
    pub m: u8,
}

/// The materials that move each frame: the water's surface ripples, a waterfall's runs down, leaves and
/// plants sway.
pub const STILL: u8 = 0;
pub const RIPPLE: u8 = 1;
pub const FALL: u8 = 2;
pub const LEAF: u8 = 3;
pub const STEM: u8 = 4;
/// A wandering animal's side, turned any way about the upright: its facing (the sun texture's green) is
/// its normal's angle about the upright, a whole turn in 256, so the sun on it turns smoothly as it does.
pub const SIDE: u8 = 6;
/// A light's own flame or glass: it shines with its own colour while the lamps are lit, whatever light
/// falls on it.
pub const GLOW: u8 = 5;

/// How a surface faces, for the sun: up, down, +x, -x, +z, -z, a plant (lit from every side, a
/// little less), the viewer's arm (never in shadow).
pub const PLANT: u8 = 6;
pub const HELD: u8 = 7;

/// A mob's face shaded by its side as a block's is, and its facing: the axis its normal is nearest.
pub fn mob_shade(n: V3) -> (f32, u8) {
    let k = if n[1] > 0.5 {
        1.0
    } else if n[1] < -0.5 {
        0.5
    } else {
        0.62 + 0.18 * n[2].abs() as f32
    };
    let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap_or(1);
    let mut f = [0; 3];
    f[axis] = n[axis].signum() as i32;
    (k, facing(f))
}

/// The facing of a face with normal `n`.
pub fn facing(n: [i32; 3]) -> u8 {
    match n {
        [0, 1, 0] => 0,
        [0, -1, 0] => 1,
        [1, 0, 0] => 2,
        [-1, 0, 0] => 3,
        [0, 0, 1] => 4,
        _ => 5,
    }
}

/// A facing's normal (a plant's and the arm's: up).
pub fn normal(n: u8) -> V3 {
    [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]]
        .get(n as usize)
        .copied()
        .unwrap_or([0.0, 1.0, 0.0])
}

impl Lit {
    fn mix(a: Lit, b: Lit, t: f32) -> Lit {
        let nearer = if t > 0.5 { b } else { a };
        Lit { c: mix3(a.c, b.c, t), sky: a.sky + (b.sky - a.sky) * t, torch: a.torch + (b.torch - a.torch) * t, n: nearer.n, m: nearer.m }
    }
    /// The light in a cell, as brightness, on something facing `n`.
    fn of(w: &World, x: i32, y: i32, z: i32, c: [f32; 3], n: u8) -> Lit {
        let (s, t) = w.light(x, y, z);
        Lit { c, sky: bright(s as f32), torch: bright(t as f32), n, m: STILL }
    }
}

/// How much a corner is darkened with 0 to 3 of the blocks about it open.
const AO: [f32; 4] = [0.5, 0.66, 0.82, 1.0];

/// Each face's own shade by its side: the top full, the bottom half, the sides between.
fn face_shade(n: [i32; 3]) -> f32 {
    match n {
        [0, 1, 0] => 1.0,
        [0, -1, 0] => 0.5,
        [0, 0, _] => 0.8,
        _ => 0.62,
    }
}

/// The first person's arm: a box held out at the lower right, in the eye's own frame (right, up,
/// forward), its middle, its half size and its axes.
struct Arm {
    centre: V3,
    half: V3,
    axes: [V3; 3],
}

impl Arm {
    fn new() -> Arm {
        // it points forward, up a little and in towards the middle, from the shoulder off the screen's
        // lower right corner; turned about its length so two of its sides show
        let along = norm([-0.15, 1.0, 0.6]);
        let across = norm(cross([0.0, 1.0, 0.0], along));
        let over = cross(along, across);
        let roll = 0.95f64;
        let (s, c) = (roll.sin(), roll.cos());
        let x = add(mul(across, c), mul(over, s));
        let z = add(mul(over, c), mul(across, -s));
        let hand = [0.44, -0.3, 0.85];
        Arm { centre: add(hand, mul(along, -0.375)), half: [0.125, 0.375, 0.125], axes: [x, along, z] }
    }
    /// Where a ray from the eye (in the eye's frame) meets it: the face's normal axis and sign, and the
    /// point in the box's own frame, -1 to 1.
    fn hit(&self, d: V3) -> Option<(usize, f64, V3)> {
        let o = mul(self.centre, -1.0);
        let (lo, ld): (V3, V3) = (std::array::from_fn(|i| dot(o, self.axes[i])), std::array::from_fn(|i| dot(d, self.axes[i])));
        let (mut t0, mut t1, mut axis, mut sign) = (f64::MIN, f64::MAX, 0, 1.0);
        for i in 0..3 {
            if ld[i].abs() < 1e-12 {
                if lo[i].abs() > self.half[i] {
                    return None;
                }
                continue;
            }
            let (a, b) = ((-self.half[i] - lo[i]) / ld[i], (self.half[i] - lo[i]) / ld[i]);
            let (near, far) = if a < b { (a, b) } else { (b, a) };
            if near > t0 {
                t0 = near;
                axis = i;
                sign = if a < b { -1.0 } else { 1.0 };
            }
            t1 = t1.min(far);
        }
        if t0 > t1 || t1 < 0.0 {
            return None;
        }
        let p: V3 = std::array::from_fn(|i| (lo[i] + ld[i] * t0) / self.half[i]);
        Some((axis, sign, p))
    }
}

pub struct Tracer<'a> {
    pub world: &'a World,
    pub tex: &'a Textures,
    pub cam: Camera,
    /// Where the haze starts and where it hides everything, in blocks across the ground.
    fog: (f64, f64),
    /// The sun's (and the moon's) places along their arc whose shadows are baked: `SUNS` of them.
    suns: [V3; SUNS],
    arm: Option<Arm>,
    /// The texels a pixel spans at one block's distance, along a face seen square on.
    footprint: f64,
}

/// How many places along the sun's arc the shadows are baked for, evenly from rising to setting.
pub const SUNS: usize = 8;

/// The three pictures, each rows of B, G, R, A bytes as cairo keeps them: what is seen, unlit and
/// premultiplied, clear where the sky shows; the light on it, the sky's in red, the torches' in green,
/// the haze in blue; and its facing (times 32) in green, its material (times 40) in red, and in blue a
/// bit for each of the sun's places, set where the sun there reaches it. Each with the same coverage.
pub struct Frame {
    pub base: Vec<u8>,
    pub light: Vec<u8>,
    pub sun: Vec<u8>,
}

fn hash3(x: i32, y: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

impl<'a> Tracer<'a> {
    pub fn new(world: &'a World, tex: &'a Textures, cam: Camera, fog: (f64, f64), suns: [V3; SUNS], arm: bool) -> Tracer<'a> {
        Tracer { world, tex, cam, fog, suns, arm: arm.then(Arm::new), footprint: cam.pixel() * 16.0 }
    }

    /// The texel at a point of a face, its picture's level picked for how far off and how slanted it
    /// is, between two levels.
    fn texel(&self, face: usize, u: f32, v: f32, t: f64, slant: f64) -> Rgba {
        let fp = (self.footprint * t / slant.max(0.15)).max(1e-6);
        let lod = (fp.log2() as f32 - 0.25).clamp(0.0, 4.0);
        let l0 = lod.floor() as usize;
        let k = lod - l0 as f32;
        let a = self.tex.all[face].at(u, v, l0);
        if k < 0.01 || l0 >= 4 {
            return a;
        }
        let b = self.tex.all[face].at(u, v, l0 + 1);
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k)
    }

    fn opaque(&self, x: i32, y: i32, z: i32) -> bool {
        block(self.world.get(x, y, z)).shape == Shape::Cube
    }

    /// The light on a face at a point of it, from `a`, the open cell it faces: the light in the open
    /// cells about each corner, averaged, each corner darkened by the blocks that close round it,
    /// eased across the face. The sky's and the torches' light, and the darkening.
    fn smooth_light(&self, a: [i32; 3], fu: f32, fv: f32, ua: usize, va: usize) -> (f32, f32, f32) {
        let mut l = [[(0f32, 0f32); 3]; 3];
        let mut o = [[false; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let mut c = a;
                c[ua] += i as i32 - 1;
                c[va] += j as i32 - 1;
                o[i][j] = self.opaque(c[0], c[1], c[2]);
                let (s, t) = self.world.light(c[0], c[1], c[2]);
                l[i][j] = (s as f32, t as f32);
            }
        }
        let mut corner = [[(0f32, 0f32, 0f32); 2]; 2];
        for (ci, si) in [0usize, 2].into_iter().enumerate() {
            for (cj, sj) in [0usize, 2].into_iter().enumerate() {
                let (s1, s2, c) = (o[si][1], o[1][sj], o[si][sj]);
                let ao = if s1 && s2 { 0 } else { 3 - s1 as usize - s2 as usize - c as usize };
                let (mut sum, mut cnt) = (l[1][1], 1.0);
                for (open, lv) in [(!s1, l[si][1]), (!s2, l[1][sj]), (!(c || s1 && s2), l[si][sj])] {
                    if open {
                        sum = (sum.0 + lv.0, sum.1 + lv.1);
                        cnt += 1.0;
                    }
                }
                corner[ci][cj] = (bright(sum.0 / cnt), bright(sum.1 / cnt), AO[ao]);
            }
        }
        let ease = |f: fn(&(f32, f32, f32)) -> f32| {
            let a = f(&corner[0][0]) + (f(&corner[1][0]) - f(&corner[0][0])) * fu;
            let b = f(&corner[0][1]) + (f(&corner[1][1]) - f(&corner[0][1])) * fu;
            a + (b - a) * fv
        };
        (ease(|c| c.0), ease(|c| c.1), ease(|c| c.2))
    }

    /// A face's picture coordinates at a point of a cell (its fractions), the face's axes in its plane
    /// and which of the block's three pictures it shows.
    fn face_uv(b: Block, cell: [i32; 3], n: [i32; 3], f: [f32; 3]) -> (f32, f32, usize, usize, usize) {
        let (mut u, mut v, ua, va, which) = match n {
            [0, 1, 0] | [0, -1, 0] => (f[0], f[2], 0, 2, if n[1] > 0 { 0 } else { 2 }),
            [1, 0, 0] => (1.0 - f[2], 1.0 - f[1], 2, 1, 1),
            [-1, 0, 0] => (f[2], 1.0 - f[1], 2, 1, 1),
            [0, 0, 1] => (f[0], 1.0 - f[1], 0, 1, 1),
            _ => (1.0 - f[0], 1.0 - f[1], 0, 1, 1),
        };
        if b.turned && which != 1 {
            match hash3(cell[0], cell[1], cell[2]) & 3 {
                1 => (u, v) = (v, 1.0 - u),
                2 => (u, v) = (1.0 - u, 1.0 - v),
                3 => (u, v) = (1.0 - v, u),
                _ => {}
            }
        }
        (u, v, ua, va, which)
    }

    /// A face where a ray met it, at `p`, facing the open cell `open`: its picture, its shade, the
    /// light on it. None where a cutout face is clear.
    fn face(&self, b: Block, cell: [i32; 3], n: [i32; 3], open: [i32; 3], p: V3, d: V3, t: f64) -> Option<Lit> {
        let f = [(p[0] - cell[0] as f64) as f32, (p[1] - cell[1] as f64) as f32, (p[2] - cell[2] as f64) as f32];
        let f = f.map(|v| v.clamp(0.0, 0.9999));
        let (u, v, ua, va, which) = Self::face_uv(b, cell, n, f);
        // a lying log: its ends ringed, its bark's grain along it
        let face = match b.lying {
            Some(axis) if n[axis] != 0 => b.faces[0],
            Some(axis) if ua == axis => b.faces[2],
            Some(_) => b.faces[1],
            None => b.faces[which],
        };
        if matches!(b.shape, Shape::Cutout | Shape::Boxes) && !self.tex.all[face].solid(u, v) {
            return None;
        }
        let slant = (d[0] * n[0] as f64 + d[1] * n[1] as f64 + d[2] * n[2] as f64).abs();
        use super::textures::{BIRCH_LEAVES, CAMPFIRE, LANTERN, OAK_LEAVES, SPRUCE_LEAVES, TORCH};
        // a light's hot pixels, told by their colour in the full-size picture, and shown in it: a
        // smaller picture would mix a far torch's flame with its stick, and it would not glow
        let full = self.tex.all[face].at(u, v, 0);
        let hot = matches!(face, TORCH | LANTERN | CAMPFIRE) && full[0] > 0.75 && full[0] > full[2] + 0.25;
        let c = if hot { full } else { self.texel(face, u, v, t, slant) };
        // the light's corners run along the axes, from low to high
        let (sky, torch, ao) = self.smooth_light(open, f[ua], f[va], ua, va);
        let k = if hot { 1.0 } else { face_shade(n) * ao };
        let m = if hot {
            GLOW
        } else if matches!(face, OAK_LEAVES | BIRCH_LEAVES | SPRUCE_LEAVES) {
            LEAF
        } else {
            STILL
        };
        Some(Lit { c: [c[0] * k, c[1] * k, c[2] * k], sky, torch, n: facing(n), m })
    }

    /// The boxes of a block where a ray meets them in this cell: the nearest with something there.
    fn boxes(&self, b: u8, bl: Block, cell: [i32; 3], o: V3, d: V3) -> Option<(Lit, f64)> {
        let (list, count) = boxes(self.world, b, cell[0], cell[1], cell[2]);
        let lo: V3 = std::array::from_fn(|i| (o[i] - cell[i] as f64) * 16.0);
        let ld = d.map(|v| v * 16.0);
        let mut hits: Vec<(f64, usize, f64, usize)> = list[..count]
            .iter()
            .enumerate()
            .filter_map(|(k, bx)| {
                let (a, z) = ([bx[0] as f64, bx[1] as f64, bx[2] as f64], [bx[3] as f64, bx[4] as f64, bx[5] as f64]);
                slab(lo, ld, a, z).map(|(t, axis, sign)| (t, axis, sign, k))
            })
            .collect();
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (t, axis, sign, k) in hits {
            let mut n = [0; 3];
            n[axis] = sign as i32;
            let p = add(o, mul(d, t));
            // a face on the cell's side faces the next cell; one within it, the block's own
            let edge = list[k][axis + if sign > 0.0 { 3 } else { 0 }];
            let open = if edge == 0 || edge == 16 { [cell[0] + n[0], cell[1] + n[1], cell[2] + n[2]] } else { cell };
            if let Some(l) = self.face(bl, cell, n, open, p, d, t) {
                return Some((l, t));
            }
        }
        None
    }

    /// A crop: four pictures in a square in the cell, the nearest with something there.
    fn crop(&self, bl: Block, cell: [i32; 3], o: V3, d: V3, t0: f64, t1: f64) -> Option<(Lit, f64)> {
        let mut hits: Vec<(f64, usize)> = vec![];
        for axis in [0, 2] {
            if d[axis].abs() < 1e-9 {
                continue;
            }
            for at in [0.25, 0.75] {
                let t = (cell[axis] as f64 + at - o[axis]) / d[axis];
                if t >= t0 - 1e-6 && t <= t1 + 1e-6 {
                    hits.push((t, axis));
                }
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let face = bl.faces[1];
        for (t, axis) in hits {
            let p = add(o, mul(d, t));
            let across = if axis == 0 { 2 } else { 0 };
            let u = ((p[across] - cell[across] as f64) as f32).clamp(0.0, 0.9999);
            let v = (1.0 - (p[1] - cell[1] as f64) as f32).clamp(0.0, 0.9999);
            if !self.tex.all[face].solid(u, v) {
                continue;
            }
            let c = self.texel(face, u, v, t, 0.7);
            let k = 0.8 + 0.2 * (1.0 - v);
            return Some((Lit { m: STEM, ..Lit::of(self.world, cell[0], cell[1], cell[2], [c[0] * k, c[1] * k, c[2] * k], PLANT) }, t));
        }
        None
    }

    /// A plant: where a ray crosses either of its two diagonal pictures in this cell, the first that
    /// has something there.
    fn plant(&self, b: Block, cell: [i32; 3], o: V3, d: V3, t0: f64, t1: f64) -> Option<(Lit, f64)> {
        let (cx, cz) = (cell[0] as f64 + 0.5, cell[2] as f64 + 0.5);
        let mut hits = [(f64::MAX, 0.0f64), (f64::MAX, 0.0)];
        for (k, sign) in [1.0f64, -1.0].into_iter().enumerate() {
            // the plane (x - cx) - sign (z - cz) = 0
            let den = d[0] - sign * d[2];
            if den.abs() < 1e-9 {
                continue;
            }
            let t = -((o[0] - cx) - sign * (o[2] - cz)) / den;
            if t >= t0 - 1e-6 && t <= t1 + 1e-6 {
                hits[k] = (t, sign);
            }
        }
        if hits[0].0 > hits[1].0 {
            hits.swap(0, 1);
        }
        for (t, sign) in hits {
            if t == f64::MAX {
                continue;
            }
            let p = add(o, mul(d, t));
            let u = ((p[0] - cell[0] as f64) as f32).clamp(0.0, 0.9999);
            let u = if sign > 0.0 { u } else { 1.0 - u };
            let v = (1.0 - (p[1] - cell[1] as f64) as f32).clamp(0.0, 0.9999);
            let face = b.faces[1];
            if !self.tex.all[face].solid(u, v) {
                continue;
            }
            let c = self.texel(face, u, v, t, 0.7);
            // lit a little less the lower it is: the ground's shade among the stems
            let k = 0.78 + 0.22 * (1.0 - v);
            return Some((Lit { m: STEM, ..Lit::of(self.world, cell[0], cell[1], cell[2], [c[0] * k, c[1] * k, c[2] * k], PLANT) }, t));
        }
        None
    }

    /// Follow a ray through the grid: what it meets, and how far.
    fn trace(&self, o: V3, d: V3, tmax: f64) -> Option<(Lit, f64)> {
        let w = self.world;
        let inv = d.map(|v| if v.abs() < 1e-12 { 1e12 } else { 1.0 / v });
        let step = d.map(|v| if v >= 0.0 { 1 } else { -1 });
        let mut t = 0.0f64;
        // the water passed through: where it was entered and its surface's colour
        let mut water: Option<(f64, Lit)> = None;
        let mut axis = 3usize;
        'restart: loop {
            let p = add(o, mul(d, t + 1e-7));
            let mut cell = p.map(|v| v.floor() as i32);
            let mut next: V3 = std::array::from_fn(|i| {
                let edge = if step[i] > 0 { cell[i] as f64 + 1.0 } else { cell[i] as f64 };
                (edge - o[i]) * inv[i]
            });
            loop {
                if t > tmax {
                    return water.map(|(_, c)| (c, t));
                }
                let [x, y, z] = cell;
                if !(-R..R).contains(&x) || !(-R..R).contains(&z) || (y >= Y1 && d[1] >= 0.0) {
                    return water.map(|(wt, c)| (c, wt));
                }
                // high over every block in this square of columns: skip to where it leaves the square,
                // or comes down to the blocks
                let ci = (((x + R) / COARSE) * CS + (z + R) / COARSE) as usize;
                let roof = Y0 + w.coarse[ci] as i32;
                if y >= roof && water.is_none() {
                    let (sx, sz) = ((x + R) / COARSE * COARSE - R, (z + R) / COARSE * COARSE - R);
                    let ex = ((if step[0] > 0 { sx + COARSE } else { sx }) as f64 - o[0]) * inv[0];
                    let ez = ((if step[2] > 0 { sz + COARSE } else { sz }) as f64 - o[2]) * inv[2];
                    let mut leave = ex.min(ez);
                    if d[1] < 0.0 {
                        leave = leave.min((roof as f64 - o[1]) * inv[1] - 1e-5);
                    }
                    if leave > t + 1e-4 {
                        t = leave;
                        axis = 3;
                        continue 'restart;
                    }
                }
                let b = w.get(x, y, z);
                let t_out = next[0].min(next[1]).min(next[2]);
                if b != AIR {
                    let bl = block(b);
                    let n = if axis < 3 {
                        let mut n = [0; 3];
                        n[axis] = -step[axis];
                        n
                    } else {
                        [0, 1, 0]
                    };
                    match bl.shape {
                        Shape::Cube | Shape::Cutout => {
                            let ph = add(o, mul(d, t));
                            if let Some(c) = self.face(bl, cell, n, [x + n[0], y + n[1], z + n[2]], ph, d, t) {
                                return Some((self.through(water, c, t), t));
                            }
                        }
                        Shape::Boxes => {
                            // a rail may have a cart on it: whichever comes first, the rail or the cart
                            let mob = if matches!(b, RAIL | RAIL_Z) { w.mobs.hit(cell, o, d, t, t_out) } else { None };
                            let rail = self.boxes(b, bl, cell, o, d);
                            match (rail, mob) {
                                (Some((_, th)), Some(m)) if m.t < th => return Some((self.through(water, self.mob(&m), m.t), m.t)),
                                (Some((c, th)), _) => return Some((self.through(water, c, th), th)),
                                (None, Some(m)) => return Some((self.through(water, self.mob(&m), m.t), m.t)),
                                (None, None) => {}
                            }
                        }
                        Shape::Crop => {
                            if let Some((c, th)) = self.crop(bl, cell, o, d, t, t_out) {
                                return Some((self.through(water, c, th), th));
                            }
                        }
                        Shape::Mob => {
                            if let Some(m) = w.mobs.hit(cell, o, d, t, t_out) {
                                return Some((self.through(water, self.mob(&m), m.t), m.t));
                            }
                        }
                        Shape::Cross => {
                            if let Some((c, th)) = self.plant(bl, cell, o, d, t, t_out) {
                                return Some((self.through(water, c, th), th));
                            }
                        }
                        Shape::Water => {
                            // a duck afloat, a fish beneath: whichever comes first, the surface or it
                            let mob = w.mobs.hit(cell, o, d, t, t_out);
                            if water.is_none() {
                                // the surface sits a little below the block's top under open air
                                let level = y as f64 + if w.get(x, y + 1, z) == WATER { 1.0 } else { 0.875 };
                                let ts = if d[1] < 0.0 { (level - o[1]) * inv[1] } else { f64::MAX };
                                let entered = add(o, mul(d, t));
                                let (hit, at, nn) = if entered[1] <= level + 1e-6 {
                                    (true, t, n)
                                } else if ts <= t_out {
                                    (true, ts, [0, 1, 0])
                                } else {
                                    (false, 0.0, n)
                                };
                                if let Some(m) = &mob
                                    && (!hit || m.t < at)
                                {
                                    return Some((self.mob(m), m.t));
                                }
                                if hit {
                                    water = Some((at, self.surface(cell, nn, add(o, mul(d, at)), d, at)));
                                }
                            }
                            if let Some(m) = &mob
                                && water.is_some()
                            {
                                return Some((self.through(water, self.mob(m), m.t), m.t));
                            }
                        }
                        Shape::Air => {}
                    }
                }
                // step to the next cell
                axis = if next[0] < next[1] {
                    if next[0] < next[2] { 0 } else { 2 }
                } else if next[1] < next[2] {
                    1
                } else {
                    2
                };
                t = next[axis];
                cell[axis] += step[axis];
                next[axis] += inv[axis].abs();
                if cell[1] < Y0 - 1 {
                    return water.map(|(wt, c)| (c, wt));
                }
            }
        }
    }

    /// A mob where a ray met it: its colour, shaded by the side of it facing the ray, lit where it
    /// stands.
    fn mob(&self, m: &super::mobs::MobHit) -> Lit {
        let (k, f) = mob_shade(m.normal);
        Lit::of(self.world, m.cell[0], m.cell[1], m.cell[2], m.colour.map(|v| v * k), f)
    }

    /// How far a ray through a point of the screen goes before it meets something; infinity for the sky.
    pub fn depth(&self, px: f64, py: f64) -> f32 {
        let d = self.cam.ray(px, py);
        self.trace(self.cam.eye, d, self.fog.1 + 8.0).map_or(f32::INFINITY, |(_, t)| t as f32)
    }

    /// The water's surface where a ray meets it: its picture, lit as a top face.
    fn surface(&self, cell: [i32; 3], n: [i32; 3], p: V3, d: V3, t: f64) -> Lit {
        let (u, v) = (((p[0] - cell[0] as f64).rem_euclid(1.0)) as f32, ((p[2] - cell[2] as f64).rem_euclid(1.0)) as f32);
        let c = self.texel(WATER_FACE, u, v, t, d[1].abs());
        let k = face_shade(n);
        let m = if n == [0, 1, 0] { RIPPLE } else { FALL };
        Lit { m, ..Lit::of(self.world, cell[0], cell[1] + 1, cell[2], [c[0] * k, c[1] * k, c[2] * k], facing(n)) }
    }

    /// What lies under the water seen through it: the deeper, the more the water's own colour.
    fn through(&self, water: Option<(f64, Lit)>, l: Lit, t: f64) -> Lit {
        let Some((wt, wl)) = water else { return l };
        let depth = (t - wt).max(0.0);
        let a = (0.5 + 0.5 * (1.0 - (-depth * 0.28).exp())) as f32;
        // what is under it, a little tinted by the water as it goes
        let under = Lit { c: [l.c[0] * 0.75, l.c[1] * 0.88, l.c[2]], ..l };
        Lit::mix(under, wl, a)
    }

    /// What a ray meets and the light on it, with how much the haze hides it and where it is (None for
    /// the arm), or None for the sky.
    fn sample(&self, px: f64, py: f64) -> Option<(Lit, f32, Option<V3>)> {
        let d = self.cam.ray(px, py);
        if let Some(arm) = &self.arm {
            let local = [dot(d, self.cam.right), dot(d, self.cam.up), dot(d, self.cam.forward)];
            if let Some((axis, sign, p)) = arm.hit(local) {
                return Some((self.arm_colour(arm, axis, sign, p), 0.0, None));
            }
        }
        let (l, t) = self.trace(self.cam.eye, d, self.fog.1 + 8.0)?;
        // the haze, by distance across the ground
        let flat = t * (d[0] * d[0] + d[2] * d[2]).sqrt();
        let f = (((flat - self.fog.0) / (self.fog.1 - self.fog.0)).clamp(0.0, 1.0)) as f32;
        if f >= 1.0 {
            return None;
        }
        Some((l, f * f * (3.0 - 2.0 * f), Some(add(self.cam.eye, mul(d, t)))))
    }

    /// Which of the sun's places reach a point facing `n`: a bit each, set where nothing stands between
    /// it and the sun there. The sun below the horizon reaches nothing.
    pub fn sunlit(&self, p: V3, n: u8) -> u8 {
        let lift = mul(normal(n), if n == PLANT { 0.3 } else { 0.02 });
        let from = add(p, lift);
        self.suns.iter().enumerate().fold(0u8, |bits, (k, &s)| {
            let lit = s[1] > 0.01 && self.trace(add(from, mul(s, 0.01)), s, 64.0).is_none();
            bits | ((lit as u8) << k)
        })
    }

    fn arm_colour(&self, arm: &Arm, axis: usize, sign: f64, p: V3) -> Lit {
        // the face's picture coordinates: across it and along the arm
        let (u, v) = match axis {
            1 => ((p[0] * 0.5 + 0.5) as f32, (p[2] * 0.5 + 0.5) as f32),
            0 => ((p[2] * 0.5 + 0.5) as f32, (0.5 - p[1] * 0.5) as f32),
            _ => ((p[0] * 0.5 + 0.5) as f32, (0.5 - p[1] * 0.5) as f32),
        };
        let (u, v) = if axis == 1 { (u * 0.25, v * 0.25) } else { (u * 0.25, v * 0.75) };
        let c = self.tex.all[ARM].at(u, v, 0);
        // lit from above and to the left, as a held thing is
        let n = mul(arm.axes[axis], sign);
        let light = norm([-0.35, 0.8, -0.5]);
        let k = (0.45 + 0.55 * dot(n, light).max(0.0)) as f32;
        let e = self.cam.eye.map(|v| v.floor() as i32);
        Lit::of(self.world, e[0], e[1], e[2], [c[0] * k, c[1] * k, c[2] * k], HELD)
    }

    /// The pictures, each pixel the mean of `n` x `n` rays through it, on every core.
    pub fn render(&self, n: usize) -> Frame {
        let (w, h) = (self.cam.w, self.cam.h);
        let mut base = vec![0u8; w * h * 4];
        let mut light = vec![0u8; w * h * 4];
        let mut sun = vec![0u8; w * h * 4];
        {
            let rows: Vec<(usize, &mut [u8], &mut [u8], &mut [u8])> =
                base.chunks_mut(w * 4).zip(light.chunks_mut(w * 4)).zip(sun.chunks_mut(w * 4)).enumerate().map(|(y, ((b, l), s))| (y, b, l, s)).collect();
            let rows = std::sync::Mutex::new(rows.into_iter());
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(32);
            std::thread::scope(|s| {
                for _ in 0..threads {
                    s.spawn(|| {
                        loop {
                            let next = rows.lock().unwrap_or_else(|e| e.into_inner()).next();
                            let Some((y, brow, lrow, srow)) = next else { break };
                            self.row(y, n, brow, lrow, srow);
                        }
                    });
                }
            });
        }
        Frame { base, light, sun }
    }

    fn row(&self, y: usize, n: usize, brow: &mut [u8], lrow: &mut [u8], srow: &mut [u8]) {
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        for x in 0..self.cam.w {
            // the mean of the rays that meet something, the colour in a near linear light; its coverage
            let (mut sum, mut lit, mut hits) = ([0f32; 3], [0f32; 3], 0usize);
            // the facing and the sun's reach from the ray nearest the pixel's middle
            let mut mid: Option<(u8, u8, Option<V3>)> = None;
            for j in 0..n {
                for i in 0..n {
                    let (px, py) = (x as f64 + (i as f64 + 0.5) / n as f64, y as f64 + (j as f64 + 0.5) / n as f64);
                    if let Some((l, fog, at)) = self.sample(px, py) {
                        if mid.is_none() || (i == n / 2 && j == n / 2) {
                            mid = Some((l.n, l.m, at));
                        }
                        for (s, c) in sum.iter_mut().zip(l.c) {
                            *s += c * c;
                        }
                        lit = [lit[0] + l.sky, lit[1] + l.torch, lit[2] + fog];
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let a = hits as f32 / (n * n) as f32;
            let c = sum.map(|v| (v / hits as f32).sqrt() * a);
            let l = lit.map(|v| v / hits as f32);
            brow[x * 4..x * 4 + 4].copy_from_slice(&[to8(c[2]), to8(c[1]), to8(c[0]), to8(a)]);
            lrow[x * 4..x * 4 + 4].copy_from_slice(&[to8(l[2]), to8(l[1]), to8(l[0]), to8(a)]);
            let (face, mat, at) = mid.unwrap_or((0, STILL, None));
            let bits = at.map_or(255, |p| self.sunlit(p, face));
            srow[x * 4..x * 4 + 4].copy_from_slice(&[bits, face * 32, mat * 40, to8(a)]);
        }
    }
}
