//! The city traced: each ray through the glass towers front to back, gathering the light of their
//! lettering and the glow inside them, to the floor's traces (mirroring the towers above them) or the
//! dark beyond. Then a soft bloom over what is bright. What a face gives a pixel is a term of its own
//! (`face`, `lettering`), so a change to its lettering is worked out without tracing the ray again.

use super::atlas::Atlas;
use super::city::{CELL, City, Glass, Panel, Show, X_MAX, Z_MIN, cell_of};
use crate::trace::camera::{overlap, span};
use crate::trace::{Camera, Px, V3, add, dot, mul, norm};
use std::collections::HashMap;

pub type Rgb3 = [f64; 3];

/// The colours of the city.
#[derive(Clone, Copy)]
pub struct Look {
    pub lit: Rgb3,
    pub blue: Rgb3,
    pub smoked: Rgb3,
    pub text: Rgb3,
    pub trace: Rgb3,
    pub sky: Rgb3,
    pub select: Rgb3,
    pub bloom: f64,
    pub exposure: f64,
}

/// What a panel shows besides its lettering, now: a bar over a row (scanning down it), a row in
/// another colour (chosen, or just written), the rows shown so far from its top (written again).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Marks {
    /// The bar's row, a fraction while it moves, and how strong.
    pub bar: Option<(f64, f64)>,
    /// A row lit in a colour, and how strongly (0 to 1).
    pub row: Option<(usize, Rgb3, f64)>,
    /// How many rows show, from the top; the rest are cleared.
    pub shown: Option<f64>,
    /// Other entries than its own (0 for its own), and how many rows they have scrolled up.
    pub seed: u64,
    pub scroll: i64,
}

pub type MarkMap = HashMap<(usize, usize), Marks, Quick>;

/// A hasher for the panels' keys, asked thousands of times a frame: two small numbers mixed, not the
/// default's defence against crafted keys, which these are not.
#[derive(Default, Clone, Copy)]
pub struct Quick;

impl std::hash::BuildHasher for Quick {
    type Hasher = QuickHasher;
    fn build_hasher(&self) -> QuickHasher {
        QuickHasher(0)
    }
}

pub struct QuickHasher(u64);

impl std::hash::Hasher for QuickHasher {
    fn finish(&self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51AFD7ED558CCD);
        h ^ (h >> 33)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x100000001B3);
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (self.0.rotate_left(29) ^ n as u64).wrapping_mul(0x9E3779B97F4A7C15);
    }
}

pub struct Tracer<'a> {
    pub city: &'a City,
    pub atlas: &'a Atlas,
    pub cam: Camera,
    pub look: Look,
    pub marks: &'a MarkMap,
    /// Rays a pixel, along each side.
    pub ss: usize,
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A line `half` wide either side, at `d` from its middle, seen with `fp` to a pixel: its coverage.
fn line(d: f64, half: f64, fp: f64) -> f64 {
    let fp = fp.max(1e-5);
    // thinner than a pixel, it fades rather than flickers
    ((half - d) / fp + 0.5).clamp(0.0, 1.0) * (2.0 * half / fp).min(1.0)
}

/// `x.floor()` for a number never below 0 (or -0), without the call that rounding is on a processor
/// not assumed to have SSE4.1: truncating is flooring there.
fn floor_up(x: f64) -> f64 {
    debug_assert!(x >= 0.0 || x.is_nan());
    (x as i64) as f64 // through i64: one instruction, where u64 takes several
}

/// `x.ceil()` for a number at least 1 and below 2^53, the same way.
fn ceil_up(x: f64) -> f64 {
    let t = (x as i64) as f64;
    if t < x { t + 1.0 } else { t }
}

fn hash(a: u64, b: u64) -> u64 {
    let mut h = a ^ b.wrapping_mul(0x9E3779B97F4A7C15);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 29;
    h
}

fn mix(a: Rgb3, b: Rgb3, t: f64) -> Rgb3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Where the distance hides things: the light that reaches the eye from `t` away.
fn fog(t: f64) -> f64 {
    (-(t - 14.0).max(0.0) / 62.0).exp()
}

/// A ray's way through a tower: in and out, and the faces (0 to 4) it crosses there.
struct Span {
    t_in: f64,
    t_out: f64,
    tower: usize,
    face_in: usize,
    face_out: Option<usize>,
}

impl<'a> Tracer<'a> {
    /// One pixel's colour, linear and unbounded, from a grid of rays across it.
    pub fn pixel(&self, x: usize, y: usize) -> Rgb3 {
        let n = self.ss;
        let mut sum = [0.0; 3];
        let pa = self.cam.pixel() / n as f64;
        for j in 0..n {
            for i in 0..n {
                let (px, py) = (x as f64 + (i as f64 + 0.5) / n as f64, y as f64 + (j as f64 + 0.5) / n as f64);
                let c = self.shade(self.cam.eye, self.cam.ray(px, py), pa, false).0;
                for k in 0..3 {
                    sum[k] += c[k];
                }
            }
        }
        sum.map(|v| v / (n * n) as f64)
    }

    /// One ray through a pixel's middle: its colour, and what it meets first (the edges between
    /// what pixels meet are where the grid of rays is needed).
    pub fn centre(&self, x: usize, y: usize) -> (Rgb3, u64) {
        self.shade(self.cam.eye, self.cam.ray(x as f64 + 0.5, y as f64 + 0.5), self.cam.pixel(), false)
    }

    /// The towers a ray goes through before `limit`, nearest first.
    fn spans(&self, o: V3, d: V3, limit: f64) -> Vec<Span> {
        let c = self.city;
        let mut out: Vec<Span> = vec![];
        let mut seen: Vec<u16> = Vec::with_capacity(32);
        // the floor's cells along the ray, in order (a grid walk in x and z)
        let (ox, oz) = (o[0], o[2]);
        let (dx, dz) = (d[0], d[2]);
        // into the grid, if outside it
        let mut t = 0.0f64;
        let (gx0, gz0, gx1, gz1) = (-X_MAX, Z_MIN, X_MAX, Z_MIN + c.nz as f64 * CELL);
        let enter = |p: f64, dd: f64, lo: f64, hi: f64| -> (f64, f64) {
            if dd.abs() < 1e-12 {
                if p >= lo && p <= hi { (f64::NEG_INFINITY, f64::INFINITY) } else { (f64::INFINITY, f64::NEG_INFINITY) }
            } else {
                let (a, b) = ((lo - p) / dd, (hi - p) / dd);
                (a.min(b), a.max(b))
            }
        };
        let (ax, bx) = enter(ox, dx, gx0, gx1);
        let (az, bz) = enter(oz, dz, gz0, gz1);
        let (t0, t1) = (ax.max(az).max(0.0), bx.min(bz).min(limit));
        if t0 >= t1 {
            return out;
        }
        t = t.max(t0);
        let p = (ox + dx * (t + 1e-7), oz + dz * (t + 1e-7));
        let Some((mut cx, mut cz)) = cell_of(p.0.clamp(gx0, gx1 - 1e-9), p.1.clamp(gz0, gz1 - 1e-9)) else { return out };
        let step_x: i64 = if dx > 0.0 { 1 } else { -1 };
        let step_z: i64 = if dz > 0.0 { 1 } else { -1 };
        let next = |cell: usize, step: i64, lo: f64, pos: f64, dd: f64| -> f64 {
            if dd.abs() < 1e-12 {
                return f64::INFINITY;
            }
            let edge = lo + (cell as f64 + if step > 0 { 1.0 } else { 0.0 }) * CELL;
            (edge - pos) / dd
        };
        let mut tx = next(cx, step_x, gx0, ox, dx);
        let mut tz = next(cz, step_z, gz0, oz, dz);
        let (dtx, dtz) = (if dx.abs() < 1e-12 { f64::INFINITY } else { CELL / dx.abs() }, if dz.abs() < 1e-12 { f64::INFINITY } else { CELL / dz.abs() });
        let mut far_out = 0.0f64;
        loop {
            if t > t1 || (d[1] > 0.0 && o[1] + d[1] * t > c.tallest) {
                break;
            }
            if out.len() >= 7 && t > far_out {
                break;
            }
            for &ti in &c.tower_cells[cz * c.nx + cx] {
                if seen.contains(&ti) {
                    continue;
                }
                seen.push(ti);
                let tw = &c.towers[ti as usize];
                if let Some(s) = slab(o, d, tw.min, tw.max, limit) {
                    far_out = far_out.max(s.1);
                    out.push(Span { t_in: s.0, t_out: s.1, tower: ti as usize, face_in: s.2, face_out: s.3 });
                }
            }
            if tx < tz {
                t = tx;
                tx += dtx;
                let n = cx as i64 + step_x;
                if n < 0 || n >= c.nx as i64 {
                    break;
                }
                cx = n as usize;
            } else {
                t = tz;
                tz += dtz;
                let n = cz as i64 + step_z;
                if n < 0 || n >= c.nz as i64 {
                    break;
                }
                cz = n as usize;
            }
        }
        out.sort_by(|a, b| a.t_in.total_cmp(&b.t_in));
        out
    }

    /// The light coming back along a ray. `pa` is the angle of a pixel (of a ray, supersampled);
    /// `mirrored` for a ray off the floor, which is not mirrored again.
    pub fn shade(&self, o: V3, d: V3, pa: f64, mirrored: bool) -> (Rgb3, u64) {
        let t_floor = if d[1] < -1e-9 { -o[1] / d[1] } else { f64::INFINITY };
        let far = if mirrored { 70.0 } else { 260.0 };
        let limit = t_floor.min(far);
        let mut col = [0.0; 3];
        let mut through = 1.0f64;
        let mut id = 0u64;
        for (n, s) in self.spans(o, d, limit).into_iter().enumerate() {
            if n < 3 {
                id = hash(id, (s.tower * 8 + s.face_in) as u64 + 1);
            }
            let tw = &self.city.towers[s.tower];
            let tint = match tw.glass {
                Glass::Lit => self.look.lit,
                Glass::Blue => self.look.blue,
                Glass::Smoked => self.look.smoked,
            };
            let (haze, absorb) = match tw.glass {
                Glass::Lit => (0.11, 0.05),
                Glass::Blue => (0.08, 0.07),
                Glass::Smoked => (0.03, 0.16),
            };
            let t_out = s.t_out.min(t_floor);
            // in through a face
            let fi = tw.faces[s.face_in];
            let (e, a) = self.face(fi, self.face_uv(fi, add(o, mul(d, s.t_in)), d, s.t_in * pa), tint);
            let w = fog(s.t_in);
            for k in 0..3 {
                col[k] += through * w * e[k];
            }
            through *= 1.0 - a;
            // the glow within, by how far the ray goes inside
            let len = (t_out - s.t_in).max(0.0);
            let g = 1.0 - (-len * haze).exp();
            for k in 0..3 {
                col[k] += through * w * tint[k] * g * 0.55;
            }
            through *= (-len * absorb).exp();
            // out through the far face, the lettering on it seen from behind
            if let Some(f) = s.face_out
                && s.t_out <= t_floor
            {
                let fi = tw.faces[f];
                let (e, a) = self.face(fi, self.face_uv(fi, add(o, mul(d, t_out)), d, t_out * pa), tint);
                let w = fog(t_out);
                for k in 0..3 {
                    col[k] += through * w * e[k] * 0.45;
                }
                through *= 1.0 - a * 0.8;
            }
            if through < 0.015 {
                return (col, id);
            }
        }
        if t_floor < far {
            let p = add(o, mul(d, t_floor));
            let f = self.floor(p, d, t_floor * pa, mirrored);
            let w = fog(t_floor);
            for k in 0..3 {
                col[k] += through * (w * f[k] + (1.0 - w) * self.look.sky[k] * 0.6);
            }
        } else {
            let s = self.sky(d);
            for k in 0..3 {
                col[k] += through * s[k];
            }
            id = hash(id, 7);
        }
        (col, id)
    }

    /// How much of face `face` a pixel's middle ray sees, and where: the weight its light reaches the
    /// eye with (through the glass before it, and the distance), the tower's tint, and where on the face
    /// (`face_uv`). None if the ray does not meet it.
    pub fn probe(&self, x: usize, y: usize, face: usize) -> Option<(f64, Rgb3, [f64; 4])> {
        let (o, d, pa) = (self.cam.eye, self.cam.ray(x as f64 + 0.5, y as f64 + 0.5), self.cam.pixel());
        let t_floor = if d[1] < -1e-9 { -o[1] / d[1] } else { f64::INFINITY };
        let mut through = 1.0f64;
        for s in self.spans(o, d, t_floor.min(260.0)) {
            let tw = &self.city.towers[s.tower];
            let tint = self.tint(tw.glass);
            let absorb = match tw.glass {
                Glass::Lit => 0.05,
                Glass::Blue => 0.07,
                Glass::Smoked => 0.16,
            };
            let t_out = s.t_out.min(t_floor);
            let fi = tw.faces[s.face_in];
            let at = self.face_uv(fi, add(o, mul(d, s.t_in)), d, s.t_in * pa);
            if fi == face {
                return Some((through * fog(s.t_in), tint, at));
            }
            through *= 1.0 - self.face(fi, at, tint).1;
            through *= (-(t_out - s.t_in).max(0.0) * absorb).exp();
            if let Some(f) = s.face_out
                && s.t_out <= t_floor
            {
                let fi = tw.faces[f];
                let at = self.face_uv(fi, add(o, mul(d, t_out)), d, t_out * pa);
                if fi == face {
                    return Some((through * fog(t_out) * 0.45, tint, at));
                }
                through *= 1.0 - self.face(fi, at, tint).1 * 0.8;
            }
            if through < 0.015 {
                return None;
            }
        }
        None
    }

    pub fn tint(&self, g: Glass) -> Rgb3 {
        match g {
            Glass::Lit => self.look.lit,
            Glass::Blue => self.look.blue,
            Glass::Smoked => self.look.smoked,
        }
    }

    /// The dark above and beyond the city: black overhead, a deep blue haze low down ahead.
    fn sky(&self, d: V3) -> Rgb3 {
        let up = smoothstep(-0.05, 0.4, d[1]);
        let ahead = d[2].max(0.0).powi(24);
        let base = mul(self.look.sky, 1.0 - up);
        add(base, mul(self.look.lit, ahead * 0.05))
    }

    /// Where the ray `d` meets face `fi` at `p`, `fp` across a pixel there: (u, v) on the face, and a
    /// pixel's size there across and up.
    fn face_uv(&self, fi: usize, p: V3, d: V3, fp: f64) -> [f64; 4] {
        let f = &self.city.faces[fi];
        let rel = [p[0] - f.origin[0], p[1] - f.origin[1], p[2] - f.origin[2]];
        let cos = dot(d, f.normal).abs().max(0.03);
        [dot(rel, f.u_axis), dot(rel, f.v_axis), fp * (1.0 + (dot(d, f.u_axis) / cos).powi(2)).sqrt(), fp * (1.0 + (dot(d, f.v_axis) / cos).powi(2)).sqrt()]
    }

    /// A face's light at `at` (`face_uv`), and how much of what is behind it it holds back: the
    /// lettering never changes that, so a mark changes only the face's own light.
    pub fn face(&self, fi: usize, at: [f64; 4], tint: Rgb3) -> (Rgb3, f64) {
        let f = &self.city.faces[fi];
        let [u, v, fu, fv] = at;
        let mut light = mul(tint, 0.05);
        // the glass's edges catch the light: its corners and its top, and its foot glows
        let side = u.min(f.width - u);
        let edge = line(side, 0.018, fu).max(line(f.height - v, 0.018, fv));
        let foot = if f.v_axis[1] > 0.5 { (-v / 0.35).exp() * 0.6 + line(v, 0.03, fv) } else { 0.0 };
        light = add(light, mul(mix(tint, [1.0, 1.0, 1.0], 0.5), edge * 0.9 + foot));
        let (text, back_cover) = self.lettering(fi, at, tint, None);
        // a dark backing holds back what is behind, and the glow, so the lettering stands out on it
        (add(light, text), (0.08 + back_cover * 0.62).min(0.95))
    }

    /// The light of a face's lettering at `at`, as it is marked now (all that a mark changes), and how
    /// much of the pixel is on a dark backing. With `only`, that panel's share alone: a pixel on the
    /// border of two panels looks at both, and the light is the sum of their shares.
    pub fn lettering(&self, fi: usize, at: [f64; 4], tint: Rgb3, only: Option<usize>) -> (Rgb3, f64) {
        let f = &self.city.faces[fi];
        let [u, v, fu, fv] = at;
        // filtered across the pixel: several looks along its long side
        let taps = ceil_up(fu.max(fv) / fu.min(fv).max(1e-6)).clamp(1.0, 4.0) as usize;
        let (mut text, mut back_cover, mut bar, mut tinted) = (0.0, 0.0, 0.0, [0.0; 3]);
        for k in 0..taps {
            let off = (k as f64 + 0.5) / taps as f64 - 0.5;
            let (su, sv) = if fu >= fv { (u + off * fu, v) } else { (u, v + off * fv) };
            let fine = fu.min(fv);
            if let Some(pi) = f.panel_at(su, sv).filter(|&pi| only.is_none_or(|o| o == pi)) {
                let panel = &f.panels[pi];
                let marks = self.marks.get(&(fi, pi)).copied().unwrap_or_default();
                let (t, b, colour) = self.panel(panel, &marks, su - panel.u0, panel.v1 - sv, fine, fv);
                text += t * panel.bright;
                bar += b;
                if panel.backed {
                    back_cover += 1.0;
                }
                if let Some(c) = colour {
                    for i in 0..3 {
                        tinted[i] += c[i] * (0.55 + 0.9 * t);
                    }
                }
            }
        }
        let n = taps as f64;
        let (text, back_cover, bar) = (text / n, back_cover / n, bar / n);
        tinted = tinted.map(|v| v / n);
        let text_colour = mix(self.look.text, tint, 0.25);
        let mut light = mul(text_colour, text * 1.6);
        light = add(light, mul(tinted, 1.3));
        light = add(light, mul(mix(tint, [1.0; 3], 0.6), bar * 1.1));
        (light, back_cover)
    }

    /// A panel's lettering at (pu across from its left, pd down from its top): its coverage, a bar's
    /// coverage, and the colour of a lit row. `fine` is a pixel's smaller side there, `fv` its height.
    fn panel(&self, p: &Panel, m: &Marks, pu: f64, pd: f64, fine: f64, fv: f64) -> (f64, f64, Option<Rgb3>) {
        let pw = p.u1 - p.u0;
        match p.show {
            Show::Rows { set, right } => {
                let row = floor_up(pd / p.row);
                let i = row as i64 as usize;
                if i >= p.rows() {
                    return (0.0, 0.0, None);
                }
                if let Some(shown) = m.shown
                    && row >= shown
                {
                    return (0.0, 0.0, None);
                }
                let within = pd / p.row - row;
                let entries = self.atlas.set(set);
                let e = entries[(hash(p.seed ^ m.seed, (i as i64 + m.scroll) as u64) % entries.len() as u64) as usize];
                let text_h = p.row * 0.84;
                let ew = e.w as f64 / e.h as f64 * text_h;
                let x0 = if right { pw - ew } else { 0.0 };
                let s = (pu - x0) / ew;
                let t = (within - 0.08) / 0.84;
                let texels = fine / (text_h / e.h as f64);
                // from far off, the rows run together into a grey of the set's mean: all grey, the letters
                // are not looked at
                let far = smoothstep(0.3 * p.row, 1.1 * p.row, fv);
                let mean = self.atlas.mean[set as usize] * 0.75;
                let cov = if far >= 1.0 {
                    mean
                } else {
                    let cov = if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) { self.atlas.sample(e, s, t, texels) } else { 0.0 };
                    cov + (mean - cov) * far
                };
                let bar = match m.bar {
                    Some((b, k)) if (row - b).abs() < 1.0 => k * (1.0 - (row - b).abs()).max(0.0),
                    _ => 0.0,
                };
                let lit = match m.row {
                    Some((r, c, k)) if r == i && within > 0.04 && within < 0.96 => Some(mul(c, k)),
                    _ => None,
                };
                (cov, bar, lit)
            }
            Show::Tiles { set } => {
                let i = floor_up(pd / p.row);
                let tile_v = pd / p.row - i;
                let s = pu / p.row;
                if s > 1.0 || (i + 1.0) * p.row > p.v1 - p.v0 {
                    return (0.0, 0.0, None);
                }
                let entries = self.atlas.set(set);
                let e = entries[(hash(p.seed ^ m.seed, (i as i64 + m.scroll) as u64) % entries.len() as u64) as usize];
                let texels = fine / (p.row / e.w as f64);
                (self.atlas.sample(e, s, tile_v, texels) * 0.8, 0.0, None)
            }
        }
    }

    /// The floor at `p`: dark glass over a circuit board, its traces glowing, the towers' light
    /// spilling round their feet, and (looking down at it) the city mirrored in it.
    fn floor(&self, p: V3, d: V3, fp: f64, mirrored: bool) -> Rgb3 {
        let c = self.city;
        let (x, z) = (p[0], p[2]);
        let fz = fp / d[1].abs().max(0.02);
        let fine = fp.min(fz);
        let mut col = mul(self.look.sky, 0.25);
        // a faint grid of the board
        let gx = (x - x.round()).abs();
        let gz = (z - z.round()).abs();
        let grid = line(gx, 0.006, fp).max(line(gz, 0.006, fz));
        col = add(col, mul(self.look.trace, grid * 0.05));
        if let Some((cx, cz)) = cell_of(x, z) {
            let cell = cz * c.nx + cx;
            let (mut core, mut glow) = (0.0f64, 0.0f64);
            for &(ti, k) in &c.trace_cells[cell] {
                let t = &c.traces[ti as usize];
                let (a, b) = (t.points[k as usize], t.points[k as usize + 1]);
                let (vx, vz) = (b.0 - a.0, b.1 - a.1);
                let l2 = vx * vx + vz * vz;
                let h = if l2 > 0.0 { (((x - a.0) * vx + (z - a.1) * vz) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let dist = (x - a.0 - vx * h).hypot(z - a.1 - vz * h);
                // the pixel's size along the floor depends on which way the trace runs
                let along_z = vz.abs() / l2.sqrt().max(1e-9);
                let px = fine + (fz - fine) * (1.0 - along_z);
                core = core.max(line(dist, t.half, px));
                glow += (-dist / 0.11).exp() * if t.ends.is_some() { 0.3 } else { 0.22 };
            }
            for &pi in &c.pad_cells[cell] {
                let pad = &c.pads[pi as usize];
                // copper round its drill hole: round where wires join, square at a dead end
                let (dx, dz) = (x - pad.x, z - pad.z);
                let r = if pad.round { dx.hypot(dz) } else { dx.abs().max(dz.abs()) };
                let px = fine.max(fz * 0.5);
                let copper = ((pad.half - r) / px + 0.5).clamp(0.0, 1.0);
                let hole = ((pad.half * 0.4 - dx.hypot(dz)) / px + 0.5).clamp(0.0, 1.0) * (pad.half * 0.8 / px).min(1.0);
                core = core.max(copper * (1.0 - hole));
                glow += (-(r - pad.half).max(0.0) / 0.08).exp() * 0.2;
            }
            col = add(col, mul(self.look.trace, core * 1.1 + glow.min(1.2) * 0.35));
            col = add(col, mul([1.0, 1.0, 1.0], core * 0.04));
            // the towers' glow at their feet, and the dark under them
            for &ti in &c.tower_cells[cell] {
                let t = &c.towers[ti as usize];
                let ox = (t.min[0] - x).max(x - t.max[0]).max(0.0);
                let oz = (t.min[2] - z).max(z - t.max[2]).max(0.0);
                let out = ox.hypot(oz);
                let tint = match t.glass {
                    Glass::Lit => self.look.lit,
                    Glass::Blue => self.look.blue,
                    Glass::Smoked => self.look.smoked,
                };
                col = add(col, mul(tint, 0.35 * (-out / 0.45).exp()));
            }
        }
        if !mirrored {
            // the city mirrored: a few rays fanned up and down, for a mirror a little dulled
            let r = [d[0], -d[1], d[2]];
            let grazing = (1.0 - d[1].abs()).powi(4);
            let strength = 0.16 + 0.3 * grazing;
            let mut m = [0.0; 3];
            let taps = [-1.0, 0.0, 1.0];
            let seed = hash((x * 4096.0) as i64 as u64, (z * 4096.0) as i64 as u64);
            let jitter = (seed % 1000) as f64 / 1000.0 - 0.5;
            for k in taps {
                let rr = norm([r[0], r[1] + (k + jitter * 0.5) * 0.035, r[2]]);
                let s = self.shade(add(p, [0.0, 1e-4, 0.0]), rr, fp * 4.0, true).0;
                for i in 0..3 {
                    m[i] += s[i] / taps.len() as f64;
                }
            }
            col = add(col, mul(m, strength));
        }
        col
    }
}

/// A ray against a box: in and out (the out clipped to `limit`), the face it enters by and the one
/// it leaves by (none past `limit`); None if it misses or the box is behind.
fn slab(o: V3, d: V3, min: V3, max: V3, limit: f64) -> Option<(f64, f64, usize, Option<usize>)> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    let (mut f0, mut f1) = (0usize, 0usize);
    for a in 0..3 {
        if d[a].abs() < 1e-12 {
            if o[a] < min[a] || o[a] > max[a] {
                return None;
            }
            continue;
        }
        let (n, x) = ((min[a] - o[a]) / d[a], (max[a] - o[a]) / d[a]);
        let (near, far) = if n < x { (n, x) } else { (x, n) };
        // the face: -x 0, +x 1, -z 2, +z 3, top 4 (the foot is the floor)
        let (fn_, ff) = match a {
            0 => {
                if d[0] > 0.0 {
                    (0, 1)
                } else {
                    (1, 0)
                }
            }
            2 => {
                if d[2] > 0.0 {
                    (2, 3)
                } else {
                    (3, 2)
                }
            }
            _ => {
                if d[1] > 0.0 {
                    (5, 4)
                } else {
                    (4, 5)
                }
            }
        };
        if near > t0 {
            t0 = near;
            f0 = fn_;
        }
        if far < t1 {
            t1 = far;
            f1 = ff;
        }
    }
    if t0 >= t1 || t1 <= 1e-6 || t0 >= limit || t0 < 0.0 || f0 == 5 {
        return None;
    }
    let out = (t1 <= limit && f1 != 5).then_some(f1);
    Some((t0, t1.min(limit), f0, out))
}

/// Linear light to the screen: a soft shoulder, never clipping hard.
pub fn tone(c: Rgb3, exposure: f64) -> [u8; 4] {
    let f = |v: f64| ((1.0 - (-v * exposure).exp()) * 255.0).round().clamp(0.0, 255.0) as u8;
    [f(c[2]), f(c[1]), f(c[0]), 255]
}

/// A bloom: the picture made small, blurred wide, to be added back large. Quarter size, linear.
pub struct Bloom {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 3]>,
}

pub const BLOOM_SCALE: usize = 4;

impl Bloom {
    pub fn new(raw: &[[f32; 3]], w: usize, h: usize) -> Bloom {
        let (bw, bh) = (w.div_ceil(BLOOM_SCALE), h.div_ceil(BLOOM_SCALE));
        let mut small = vec![[0.0f32; 3]; bw * bh];
        for y in 0..h {
            for x in 0..w {
                let s = &mut small[(y / BLOOM_SCALE) * bw + x / BLOOM_SCALE];
                let p = raw[y * w + x];
                // only what is bright blooms
                let l = (p[0] + p[1] + p[2]) / 3.0;
                let k = ((l - 0.25) / 0.75).clamp(0.0, 1.0);
                for i in 0..3 {
                    s[i] += p[i] * k / (BLOOM_SCALE * BLOOM_SCALE) as f32;
                }
            }
        }
        // two widths of blur, summed
        let narrow = blur(&small, bw, bh, 3);
        let wide = blur(&narrow, bw, bh, 9);
        let px = narrow.iter().zip(&wide).map(|(a, b)| [a[0] * 0.5 + b[0], a[1] * 0.5 + b[1], a[2] * 0.5 + b[2]]).collect();
        Bloom { w: bw, h: bh, px }
    }

    /// The bloom at a pixel of the full picture, filtered.
    pub fn at(&self, x: usize, y: usize) -> [f32; 3] {
        let fx = ((x as f32 + 0.5) / BLOOM_SCALE as f32 - 0.5).clamp(0.0, (self.w - 1) as f32);
        let fy = ((y as f32 + 0.5) / BLOOM_SCALE as f32 - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let p = |x: usize, y: usize| self.px[y * self.w + x];
        std::array::from_fn(|i| {
            let top = p(x0, y0)[i] + (p(x1, y0)[i] - p(x0, y0)[i]) * ax;
            let bot = p(x0, y1)[i] + (p(x1, y1)[i] - p(x0, y1)[i]) * ax;
            top + (bot - top) * ay
        })
    }
}

/// A box blur three times over (near a gaussian), `r` wide either side.
fn blur(src: &[[f32; 3]], w: usize, h: usize, r: usize) -> Vec<[f32; 3]> {
    let mut a = src.to_vec();
    let mut b = vec![[0.0f32; 3]; w * h];
    for _ in 0..3 {
        for (along, across, stride, step) in [(w, h, w, 1usize), (h, w, 1usize, w)] {
            for line in 0..across {
                let base = line * stride;
                let mut sum = [0.0f32; 3];
                let n = (2 * r + 1) as f32;
                let at = |i: i64| -> [f32; 3] { a[base + (i.clamp(0, along as i64 - 1) as usize) * step] };
                for i in -(r as i64)..=(r as i64) {
                    let p = at(i);
                    for k in 0..3 {
                        sum[k] += p[k];
                    }
                }
                for i in 0..along {
                    b[base + i * step] = sum.map(|v| v / n);
                    let (add_, sub) = (at(i as i64 + r as i64 + 1), at(i as i64 - r as i64));
                    for k in 0..3 {
                        sum[k] += add_[k] - sub[k];
                    }
                }
                let _ = along;
            }
            std::mem::swap(&mut a, &mut b);
        }
    }
    a
}

/// What one row of a panel shows under some marks: its bar, its colour, whether it is cleared.
fn row_look(m: &Marks, i: usize) -> (u64, Option<(usize, [u64; 3], u64)>, bool, u64, i64) {
    let bar = match m.bar {
        Some((b, k)) if (i as f64 - b).abs() < 1.0 => (k * (1.0 - (i as f64 - b).abs())).to_bits(),
        _ => 0,
    };
    let lit = m.row.filter(|r| r.0 == i).map(|(r, c, k)| (r, c.map(f64::to_bits), k.to_bits()));
    (bar, lit, m.shown.is_some_and(|s| i as f64 >= s), m.seed, m.scroll)
}

/// The runs of a panel's rows (first, past the last) that look different under marks `a` and `b`.
pub fn rows_changed(p: &Panel, a: &Marks, b: &Marks) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = vec![];
    for i in 0..p.rows() {
        if row_look(a, i) != row_look(b, i) {
            match out.last_mut() {
                Some(run) if run.1 == i => run.1 = i + 1,
                _ => out.push((i, i + 1)),
            }
        }
    }
    out
}

/// The pixels inside an outline of the screen (`hull`) and within `clip`, traced: the rectangle about
/// them and each pixel's linear light, None outside the outline. One ray a pixel, and the full grid
/// of rays where a pixel's neighbours meet something else first: a pixel comes out the same whatever
/// it is traced with.
pub fn trace_region(tr: &Tracer, outline: &[(f64, f64)], clip: Px) -> (Px, Vec<Option<[f32; 3]>>) {
    let screen = (0, 0, tr.cam.w as i32, tr.cam.h as i32);
    let Some(clip) = overlap(clip, screen) else { return ((0, 0, 0, 0), vec![]) };
    let spans: Vec<Option<(i32, i32)>> =
        (clip.1..clip.3).map(|y| span(outline, y).map(|(a, b)| (a.max(clip.0), b.min(clip.2))).filter(|s| s.0 < s.1)).collect();
    let rows: Vec<i32> = (clip.1..clip.3).filter(|&y| spans[(y - clip.1) as usize].is_some()).collect();
    let (Some(&y0), Some(&y1)) = (rows.first(), rows.last()) else { return ((0, 0, 0, 0), vec![]) };
    let x0 = spans.iter().flatten().map(|s| s.0).min().unwrap_or(clip.0);
    let x1 = spans.iter().flatten().map(|s| s.1).max().unwrap_or(clip.0);
    let r = (x0, y0, x1, y1 + 1);
    // the first pass one pixel further all round, for the neighbours
    let e = (r.0.max(1) - 1, r.1.max(1) - 1, (r.2 + 1).min(screen.2), (r.3 + 1).min(screen.3));
    let (ew, eh) = ((e.2 - e.0) as usize, (e.3 - e.1) as usize);
    let span_at = |y: i32| if y >= clip.1 && y < clip.3 { spans[(y - clip.1) as usize] } else { None };
    let wanted = |x: i32, y: i32| (y - 1..=y + 1).any(|yy| span_at(yy).is_some_and(|(a, b)| x >= a - 1 && x < b + 1));
    const B: usize = 21;
    let mut first = vec![0u8; ew * eh * B];
    crate::trace::rows([&mut first[..]], ew * B, |y, [row]| {
        let sy = e.1 + y as i32;
        for x in 0..ew {
            let sx = e.0 + x as i32;
            if !wanted(sx, sy) {
                continue;
            }
            let (c, id) = tr.centre(sx as usize, sy as usize);
            let at = &mut row[x * B..x * B + B];
            for k in 0..3 {
                at[k * 4..k * 4 + 4].copy_from_slice(&(c[k] as f32).to_le_bytes());
            }
            at[12..20].copy_from_slice(&id.to_le_bytes());
            at[20] = 1;
        }
    });
    let id = |x: i32, y: i32| -> Option<u64> {
        if x < e.0 || y < e.1 || x >= e.2 || y >= e.3 {
            return None;
        }
        let i = ((y - e.1) as usize * ew + (x - e.0) as usize) * B;
        (first[i + 20] == 1).then(|| u64::from_le_bytes(first[i + 12..i + 20].try_into().unwrap_or([0; 8])))
    };
    let (w, h) = ((r.2 - r.0) as usize, (r.3 - r.1) as usize);
    let mut out = vec![0u8; w * h * 13];
    crate::trace::rows([&mut out[..]], w * 13, |y, [row]| {
        let sy = r.1 + y as i32;
        let Some((a, b)) = span_at(sy) else { return };
        for sx in a..b {
            let x = (sx - r.0) as usize;
            let me = id(sx, sy);
            let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| id(sx + dx, sy + dy).is_some_and(|o| Some(o) != me));
            let c: [f32; 3] = if edge && tr.ss > 1 {
                tr.pixel(sx as usize, sy as usize).map(|v| v as f32)
            } else {
                let i = ((sy - e.1) as usize * ew + (sx - e.0) as usize) * B;
                std::array::from_fn(|k| f32::from_le_bytes(first[i + k * 4..i + k * 4 + 4].try_into().unwrap_or([0; 4])))
            };
            for k in 0..3 {
                row[x * 13 + k * 4..x * 13 + k * 4 + 4].copy_from_slice(&c[k].to_le_bytes());
            }
            row[x * 13 + 12] = 1;
        }
    });
    let px = out
        .as_chunks::<13>()
        .0
        .iter()
        .map(|p| (p[12] == 1).then(|| std::array::from_fn(|k| f32::from_le_bytes([p[k * 4], p[k * 4 + 1], p[k * 4 + 2], p[k * 4 + 3]]))))
        .collect();
    (r, px)
}

/// A rectangle of the picture traced, all of it.
pub fn trace_rect(tr: &Tracer, r: Px) -> Vec<[f32; 3]> {
    let outline = [(r.0 as f64, r.1 as f64), (r.2 as f64, r.1 as f64), (r.2 as f64, r.3 as f64), (r.0 as f64, r.3 as f64)];
    trace_region(tr, &outline, r).1.into_iter().map(|p| p.unwrap_or([0.0; 3])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_through_a_box_enters_and_leaves_by_the_right_faces() {
        let s = slab([0.0, 1.0, -5.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 3.0, 2.0], 100.0).unwrap();
        assert_eq!((s.2, s.3), (2, Some(3)));
        assert!((s.0 - 5.0).abs() < 1e-9 && (s.1 - 7.0).abs() < 1e-9);
        // down onto the floor inside it: no face out
        let s = slab([0.0, 5.0, 1.0], norm([0.0, -1.0, 0.1]), [-1.0, 0.0, 0.0], [1.0, 3.0, 2.0], 5.0).unwrap();
        assert_eq!((s.2, s.3), (4, None));
        assert!(slab([5.0, 1.0, -5.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [1.0, 3.0, 2.0], 100.0).is_none());
    }
}
