//! data-city: a street of a computer's insides as a city. Towers of tinted glass stand along it, each
//! face covered in its files: lists of directories, columns of figures, small print, diagrams, lit
//! from within. The floor is a circuit board under dark glass, mirroring the towers. Files are at
//! work: a bar scans a list, a file is chosen, a pulse carries it along the floor or a beam across
//! the street to another tower, whose list is written again with the file in it. Pulses run errands
//! along the traces. The disk's work sets how many files move, the processor's the errands.
//!
//! The street is traced once on the CPU, every core, and kept in the theme's cache. What a file changes
//! on a face is worked out again only where it shows, with no ray traced (`retrace`); the pulses are
//! drawn over it on the GPU.

mod atlas;
mod city;
mod ops;
mod render;

use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::trace::camera::{hull, overlap, pixels, span};
use crate::trace::{Camera, Px, V3};
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, ImageSurface, Operator};
use render::{Bloom, Look, MarkMap, Rgb3, Tracer, tone, trace_rect};
use serde::Deserialize;
use std::sync::Arc;
use toml::Table;

const KIND: &str = "data-city";
/// What the cache's pictures were traced by: a change to how the city looks changes it.
const VERSION: &str = "25";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    seed: i64,
    /// Where the eye is: across the street, up, and along it (the city starts at 0).
    eye: [f64; 3],
    pitch: f64,
    yaw: f64,
    fov: f64,
    samples: f64,
    lit: Colour,
    blue: Colour,
    smoked: Colour,
    text: Colour,
    trace: Colour,
    sky: Colour,
    select: Colour,
    fresh: Colour,
    pulse: Colour,
    bloom: f64,
    exposure: f64,
    files: String,
    traffic: String,
    fps: f64,
}

impl Validate for Settings {
    fn validate(&self) -> Result<(), String> {
        within("eye[0]", self.eye[0], -2.5, 2.5)?;
        within("eye[1]", self.eye[1], 0.3, 40.0)?;
        within("pitch", self.pitch, -60.0, 30.0)?;
        within("yaw", self.yaw, -40.0, 40.0)?;
        within("fov", self.fov, 20.0, 110.0)?;
        within("samples", self.samples, 1.0, 4.0)?;
        within("bloom", self.bloom, 0.0, 3.0)?;
        within("exposure", self.exposure, 0.2, 5.0)?;
        within("fps", self.fps, 1.0, 60.0)?;
        within("eye[2]", self.eye[2], -25.0, 60.0)
    }
}

/// A pixel's view of a panel (`DataCity::seen`), small: a long run keeps a million of them.
struct Seen {
    x: u16,
    y: u16,
    /// How much of the face's light reaches the eye here, and where on the face it looks (`face_uv`).
    w: f32,
    at: [f32; 4],
    /// This panel's share of the lettering's light here, as the pixel shows it now: what its next mark
    /// replaces.
    last: [f32; 3],
}

/// The pixels that see a panel, in order of how high on the face each looks (`at[1]`), and the most
/// one pixel spans up the face: a run of rows is found among them without looking at the rest. The
/// glass's tint is its tower's, the same for all of them.
#[derive(Default)]
struct Panel {
    px: Vec<Seen>,
    fv: f64,
    tint: [f32; 3],
}

fn rgb3(c: Colour) -> Rgb3 {
    let (r, g, b) = c.rgb();
    [r, g, b]
}

pub struct DataCity {
    s: Settings,
    look: Look,
    seed: i64,
    city: Option<Arc<city::City>>,
    atlas: Option<Arc<atlas::Atlas>>,
    cam: Option<Camera>,
    /// The picture as shown (B, G, R, A), and the bloom it was made with.
    shown: Vec<u8>,
    bloom: Option<Bloom>,
    tex: Option<Tex>,
    streak: Option<Tex>,
    dot: Option<Tex>,
    work: Option<ops::Work>,
    /// The marks the picture shows.
    marks: MarkMap,
    /// What the pixels see of each panel marked so far.
    seen: std::collections::HashMap<(usize, usize), Panel, render::Quick>,
    /// The regions of the picture changed, to go up.
    uploads: Vec<(Px, Vec<u8>)>,
    /// The light of each pixel a mark has changed, linear (NaN: never changed, as `shown` has it).
    lin: Vec<[f32; 3]>,
    /// How many panels' pixels may be found in one frame: the first moments mark dozens at once.
    per_frame: usize,
    /// The faces' moment last worked out (they change fifteen times a second).
    moment: f64,
    /// The work's seed when it is not this run's (a measurement's).
    work_seed: Option<u64>,
    strips: Vec<f32>,
    dots: Vec<f32>,
}

impl DataCity {
    fn new(t: &Table) -> DataCity {
        let s: Settings = settings(t);
        let look = Look {
            lit: rgb3(s.lit),
            blue: rgb3(s.blue),
            smoked: rgb3(s.smoked),
            text: rgb3(s.text),
            trace: rgb3(s.trace),
            sky: rgb3(s.sky),
            select: rgb3(s.select),
            bloom: s.bloom,
            exposure: s.exposure,
        };
        let seed = if s.seed != 0 { s.seed } else { 23 };
        DataCity {
            s,
            look,
            seed,
            city: None,
            atlas: None,
            cam: None,
            shown: vec![],
            bloom: None,
            tex: None,
            streak: None,
            dot: None,
            work: None,
            marks: MarkMap::default(),
            seen: Default::default(),
            uploads: vec![],
            lin: vec![],
            per_frame: 3,
            moment: f64::NAN,
            work_seed: None,
            strips: vec![],
            dots: vec![],
        }
    }

    fn ss(&self) -> usize {
        self.s.samples.round().clamp(1.0, 4.0) as usize
    }

    /// The city laid out and traced, once, for this screen.
    fn render(&mut self, g: &Geo) {
        if self.cam.is_some() {
            return;
        }
        let (w, h) = (g.w.max(1) as usize, g.h.max(1) as usize);
        let s = &self.s;
        let cam = Camera::new(s.eye, s.yaw.to_radians(), s.pitch.to_radians(), s.fov, w, h);
        let atlas = Arc::new(atlas::Atlas::new());
        let city = Arc::new(city::City::new(self.seed, &atlas));
        let l = &self.look;
        let tag = format!(
            "{VERSION} {} {:?} {} {} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {} {}",
            self.seed, s.eye, s.pitch, s.yaw, s.fov, s.samples, l.lit, l.blue, l.smoked, l.text, l.trace, l.sky, l.bloom, l.exposure
        );
        let theme = g.th.dirs.first().map(|d| d.join("theme.toml"));
        let (bw, bh) = (w.div_ceil(render::BLOOM_SCALE), h.div_ceil(render::BLOOM_SCALE));
        let kept = theme.as_ref().and_then(|t| crate::svg::kept_read(t, g.w, g.h, KIND, &tag)).filter(|b| b.len() == w * h * 4 + bw * bh * 12);
        let bytes = kept.unwrap_or_else(|| {
            let marks = MarkMap::default();
            let tr = Tracer { city: &city, atlas: &atlas, cam, look: self.look, marks: &marks, ss: self.ss() };
            let raw = trace_rect(&tr, (0, 0, w as i32, h as i32));
            let bloom = Bloom::new(&raw, w, h);
            let mut out = Vec::with_capacity(w * h * 4 + bw * bh * 12);
            for (i, p) in raw.iter().enumerate() {
                let b = bloom.at(i % w, i / w);
                out.extend(tone(std::array::from_fn(|k| (p[k] + b[k] * self.look.bloom as f32) as f64), self.look.exposure));
            }
            for p in &bloom.px {
                for v in p {
                    out.extend(v.to_le_bytes());
                }
            }
            if let Some(t) = &theme {
                crate::svg::keep(t, g.w, g.h, KIND, &tag, &out);
            }
            out
        });
        let (pic, rest) = bytes.split_at(w * h * 4);
        let px = rest
            .as_chunks::<12>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|k| f32::from_le_bytes([p[k * 4], p[k * 4 + 1], p[k * 4 + 2], p[k * 4 + 3]])))
            .collect();
        self.bloom = Some(Bloom { w: bw, h: bh, px });
        self.shown = pic.to_vec();
        let seen = |p: V3, row: f64| cam.project(p).is_some_and(|(x, y, ppu)| x > 0.0 && x < w as f64 && y > 0.0 && y < h as f64 && ppu * row > 4.0);
        let front = |p: V3| cam.project(p).is_some();
        let seed = self.work_seed.unwrap_or_else(|| crate::rng::run_seed(self.s.seed) as u64);
        // each start its own names on the lists; none while the time of day is held (a snapshot's, the
        // same each time)
        self.work = Some(ops::Work::new(&city, seed, seen, front, !crate::day::held() && self.work_seed.is_none()));
        self.cam = Some(cam);
        self.city = Some(city);
        self.atlas = Some(atlas);
    }

    /// The outlines on the screen of a panel's rows `from` to `to` (past the last), and the rectangle
    /// about each. The floor's mirror of a face is left as it was: dulled and small, a mark does not show
    /// in it.
    fn rows_outline(&self, face: usize, panel: usize, from: usize, to: usize) -> Option<(Vec<(f64, f64)>, Px)> {
        let (city, cam) = (self.city.as_ref()?, self.cam.as_ref()?);
        let f = &city.faces[face];
        let p = &f.panels[panel];
        let (top, bottom) = (p.v1 - from as f64 * p.row, p.v1 - to as f64 * p.row);
        let mut pts = vec![];
        for (u, v) in [(p.u0, bottom), (p.u1, bottom), (p.u0, top), (p.u1, top)] {
            let (x, y, _) = cam.project(f.point(u, v))?;
            pts.push((x, y));
        }
        let outline = hull(pts);
        // the hull's own rectangle, grown a pixel: what a ray near its edge can meet
        let rect = outline.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)));
        let grown: Vec<(f64, f64)> = outline
            .iter()
            .map(|&(x, y)| {
                let (cx, cy) = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
                let (dx, dy) = (x - cx, y - cy);
                let l = dx.hypot(dy).max(1e-9);
                (x + dx / l * 1.5, y + dy / l * 1.5)
            })
            .collect();
        Some((grown, pixels(rect)))
    }

    /// What each pixel of a panel sees of it, found once, when it is first marked: the weight and place
    /// its view of the panel has, and the panel's lettering's light there unmarked. A pixel's light is
    /// the sum of what each face along its ray gives, each by its weight, so a mark changes only that
    /// panel's term; several panels can show in one pixel (a face, and another through its glass).
    fn seen(&mut self, key: (usize, usize)) {
        if self.seen.contains_key(&key) {
            return;
        }
        let rows = self.city.as_ref().map_or(0, |c| c.faces[key.0].panels[key.1].rows());
        let Some((outline, clip)) = self.rows_outline(key.0, key.1, 0, rows) else {
            self.seen.insert(key, Panel::default());
            return;
        };
        let (Some(city), Some(atlas), Some(cam)) = (&self.city, &self.atlas, self.cam) else { return };
        let none = MarkMap::default();
        let tr = Tracer { city, atlas, cam, look: self.look, marks: &none, ss: 1 };
        let Some(clip) = overlap(clip, (0, 0, cam.w as i32, cam.h as i32)) else {
            self.seen.insert(key, Panel::default());
            return;
        };
        let rw = (clip.2 - clip.0) as usize;
        const B: usize = 33;
        let mut buf = vec![0u8; rw * (clip.3 - clip.1) as usize * B];
        crate::trace::rows([&mut buf[..]], rw * B, |y, [row]| {
            let sy = clip.1 + y as i32;
            let Some((a, b)) = span(&outline, sy) else { return };
            for sx in a.max(clip.0)..b.min(clip.2) {
                let Some((w, tint, at)) = tr.probe(sx as usize, sy as usize, key.0) else { continue };
                let o = &mut row[(sx - clip.0) as usize * B..(sx - clip.0) as usize * B + B];
                for (k, v) in [w, tint[0], tint[1], tint[2], at[0], at[1], at[2], at[3]].into_iter().enumerate() {
                    o[k * 4..k * 4 + 4].copy_from_slice(&(v as f32).to_le_bytes());
                }
                o[32] = 1;
            }
        });
        let mut out = vec![];
        let mut tint = [0.0f32; 3];
        for (i, p) in buf.as_chunks::<B>().0.iter().enumerate() {
            if p[32] != 1 {
                continue;
            }
            let f: [f32; 8] = std::array::from_fn(|k| f32::from_le_bytes([p[k * 4], p[k * 4 + 1], p[k * 4 + 2], p[k * 4 + 3]]));
            let (x, y) = (clip.0 as usize + i % rw, clip.1 as usize + i / rw);
            let at = [f[4], f[5], f[6], f[7]];
            tint = [f[1], f[2], f[3]];
            let last = tr.lettering(key.0, at.map(|a| a as f64), tint.map(|t| t as f64), Some(key.1)).0.map(|v| v as f32);
            out.push(Seen { x: x as u16, y: y as u16, w: f[0], at, last });
        }
        out.sort_by(|a, b| a.at[1].total_cmp(&b.at[1]));
        out.shrink_to_fit();
        let fv = out.iter().map(|p| p.at[3] as f64).fold(0.0, f64::max);
        self.seen.insert(key, Panel { px: out, fv, tint });
    }

    /// The rows whose marks changed, their light worked out again and queued to go up. A panel marked for
    /// the first time is found first (`seen`), a few a frame; one still waiting keeps its marks as they
    /// were until it has been found.
    fn retrace(&mut self, mut marks: MarkMap) {
        let Some(city) = self.city.clone() else { return };
        let none = render::Marks::default();
        let mut changed: Vec<(usize, usize)> = marks.iter().filter(|(k, m)| self.marks.get(k).unwrap_or(&none) != *m).map(|(k, _)| *k).collect();
        changed.extend(self.marks.keys().filter(|k| !marks.contains_key(k)).copied());
        let mut budget = self.per_frame;
        for &k in &changed {
            if self.seen.contains_key(&k) {
                continue;
            }
            if budget > 0 {
                budget -= 1;
                self.seen(k);
            } else {
                match self.marks.get(&k) {
                    Some(&old) => marks.insert(k, old),
                    None => marks.remove(&k),
                };
            }
        }
        let mut runs = vec![];
        for k in changed {
            let (a, b) = (self.marks.get(&k).unwrap_or(&none), marks.get(&k).unwrap_or(&none));
            if a != b {
                runs.extend(render::rows_changed(&city.faces[k.0].panels[k.1], a, b).into_iter().map(|(from, to)| (k, from, to)));
            }
        }
        self.marks = marks;
        let (Some(atlas), Some(cam)) = (&self.atlas, self.cam) else { return };
        if !runs.is_empty() && self.lin.is_empty() {
            self.lin = vec![[f32::NAN; 3]; cam.w * cam.h];
        }
        let untone = |b: u8| (-(1.0 - (b as f64 / 255.0).min(0.999)).ln() / self.look.exposure) as f32;
        let now = Tracer { city: &city, atlas, cam, look: self.look, marks: &self.marks, ss: 1 };
        for (k, from, to) in runs {
            let p = &city.faces[k.0].panels[k.1];
            let (lo, hi) = (p.v1 - to as f64 * p.row, p.v1 - from as f64 * p.row);
            let mut rect = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
            let Some(seen) = self.seen.get_mut(&k) else { continue };
            // the pixels that can look at these rows: no further from them than one pixel spans
            let first = seen.px.partition_point(|p| (p.at[1] as f64) < lo - seen.fv);
            let last = seen.px.partition_point(|p| (p.at[1] as f64) <= hi + seen.fv);
            let tint = seen.tint.map(|t| t as f64);
            for px in &mut seen.px[first..last] {
                let [u, v, fu, fv] = px.at.map(|a| a as f64);
                if v < lo - fv || v > hi + fv {
                    continue;
                }
                // this panel's term replaced: what the pixel showed, less what the panel gave it, plus
                // what it gives now
                let a = now.lettering(k.0, [u, v, fu, fv], tint, Some(k.1)).0.map(|v| v as f32);
                let i = px.y as usize * cam.w + px.x as usize;
                let lin = &mut self.lin[i];
                if lin[0].is_nan() {
                    *lin = [untone(self.shown[i * 4 + 2]), untone(self.shown[i * 4 + 1]), untone(self.shown[i * 4])];
                }
                for c in 0..3 {
                    lin[c] += px.w * (a[c] - px.last[c]);
                }
                px.last = a;
                self.shown[i * 4..i * 4 + 4].copy_from_slice(&tone(lin.map(|v| v as f64), self.look.exposure));
                let (x, y) = (px.x as i32, px.y as i32);
                rect = (rect.0.min(x), rect.1.min(y), rect.2.max(x + 1), rect.3.max(y + 1));
            }
            if rect.0 < rect.2 {
                let rw = (rect.2 - rect.0) as usize;
                let mut bytes = Vec::with_capacity(rw * (rect.3 - rect.1) as usize * 4);
                for y in rect.1..rect.3 {
                    let at = (y as usize * cam.w + rect.0 as usize) * 4;
                    bytes.extend_from_slice(&self.shown[at..at + rw * 4]);
                }
                self.uploads.push((rect, bytes));
            }
        }
    }

    /// The pulses as quads: their trails (on `streak`) and their heads (on `dot`).
    fn place(&mut self, pulses: &[ops::Pulse], t: f64, load: f64) {
        self.strips.clear();
        self.dots.clear();
        let Some(cam) = &self.cam else { return };
        let pulse = rgb3(self.s.pulse);
        let trace = self.look.trace;
        let mut strip = |a: (f64, f64), b: (f64, f64), wa: f64, wb: f64, ca: (Rgb3, f64), cb: (Rgb3, f64)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy);
            if l < 1e-6 {
                return;
            }
            let (nx, ny) = (-dy / l, dx / l);
            let corners = [
                (a.0 + nx * wa, a.1 + ny * wa, 0.0, ca),
                (b.0 + nx * wb, b.1 + ny * wb, 0.0, cb),
                (b.0 - nx * wb, b.1 - ny * wb, 1.0, cb),
                (a.0 - nx * wa, a.1 - ny * wa, 1.0, ca),
            ];
            for (x, y, v, (c, al)) in corners {
                self.strips.extend([0.5, v, c[0] * al, c[1] * al, c[2] * al, al, x, y].map(|f| f as f32));
            }
        };
        let mut heads = vec![];
        for p in pulses {
            match *p {
                ops::Pulse::Floor { ref path, head, strength, file, kind } => {
                    if strength <= 0.0 {
                        continue;
                    }
                    let len = city::length(path);
                    // signals of four kinds: their trails' lengths and colours
                    let tail = if file { 4.5 } else { [2.6, 1.2, 4.0, 0.7][kind as usize % 4] };
                    let n = 14;
                    let mut prev: Option<((f64, f64), f64, f64)> = None;
                    for i in 0..=n {
                        let s = head - tail * (1.0 - i as f64 / n as f64);
                        if s < 0.0 || s > len {
                            prev = None;
                            continue;
                        }
                        let ((x, z), _) = city::at(path, s);
                        let Some((sx, sy, ppu)) = cam.project([x, 0.004, z]) else {
                            prev = None;
                            continue;
                        };
                        let k = (i as f64 / n as f64).powf(1.6) * strength;
                        let w = (0.07 * ppu).max(1.5);
                        if let Some((pp, pw, pk)) = prev {
                            let col =
                                if file { pulse } else { [mixed3(trace, pulse, 0.5), self.look.select, self.look.lit, [1.0, 1.0, 1.0]][kind as usize % 4] };
                            strip(pp, (sx, sy), pw, w, (col, pk), (col, k));
                            // a wide faint glow about the trail
                            strip(pp, (sx, sy), pw * 4.0, w * 4.0, (trace, pk * 0.45), (trace, k * 0.45));
                        }
                        prev = Some(((sx, sy), w, k));
                    }
                    if (0.0..=len).contains(&head) {
                        let ((x, z), _) = city::at(path, head);
                        if let Some((sx, sy, ppu)) = cam.project([x, 0.004, z]) {
                            heads.push((sx, sy, (if file { 0.75 } else { 0.5 }) * ppu, strength, file));
                        }
                    }
                }
                ops::Pulse::Air { from, to, head, strength, tail, line } => {
                    let at = |k: f64| [from[0] + (to[0] - from[0]) * k, from[1] + (to[1] - from[1]) * k, from[2] + (to[2] - from[2]) * k];
                    // along the bright part finely, the rest coarsely
                    let n = if line { 16 } else { 40 };
                    let mut prev: Option<((f64, f64), f64, f64)> = None;
                    for i in 0..=n {
                        let k = if line { i as f64 / n as f64 } else { (head - tail * (1.0 - i as f64 / n as f64)).clamp(0.0, 1.0) };
                        let Some((sx, sy, ppu)) = cam.project(at(k)) else {
                            prev = None;
                            continue;
                        };
                        // the whole beam faint, bright toward its head
                        let behind = head - k;
                        let bright = if behind >= 0.0 { (1.0 - behind / tail).max(0.0).powi(2) } else { 0.0 };
                        let a = strength * (if line { 0.12 } else { 0.0 } + 0.88 * bright) * if behind < 0.0 { 0.0 } else { 1.0 };
                        let w = (0.05 * ppu).max(1.4);
                        if let Some((pp, pw, pa)) = prev {
                            strip(pp, (sx, sy), pw, w, (pulse, pa), (pulse, a));
                            strip(pp, (sx, sy), pw * 4.0, w * 4.0, (trace, pa * 0.3), (trace, a * 0.3));
                        }
                        prev = Some(((sx, sy), w, a));
                    }
                    if let Some((sx, sy, ppu)) = cam.project(at(head.min(1.0))) {
                        heads.push((sx, sy, 0.6 * ppu, strength, true));
                    }
                }
            }
        }
        let mut placed = vec![];
        // the far end of the street glows, breathing, brighter with the processor's work
        if let Some((x, y, _)) = cam.project([0.3, 3.0, 230.0]) {
            let breath = 0.5 + 0.5 * (t * 0.7).sin() * (t * 0.23 + 1.0).cos();
            let k = 0.6 + 0.4 * load.clamp(0.0, 1.0);
            let h = cam.h as f64;
            placed.push((x, y, 0.9 * h, 0.0, (self.look.blue[0], self.look.blue[1], self.look.blue[2]), (0.16 + 0.10 * breath) * k));
            placed.push((x, y, 0.3 * h, 0.0, (self.look.lit[0], self.look.lit[1], self.look.lit[2]), (0.22 + 0.25 * breath) * k));
        }
        for (x, y, size, k, file) in heads {
            let size = size.max(6.0);
            placed.push((x, y, size * 3.0, 0.0, (trace[0], trace[1], trace[2]), 0.35 * k));
            placed.push((x, y, size, 0.0, if file { (1.0, 1.0, 1.0) } else { (pulse[0], pulse[1], pulse[2]) }, k));
        }
        self.dots = sprite_vertices(&placed);
    }
}

fn mixed3(a: Rgb3, b: Rgb3, t: f64) -> Rgb3 {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

impl Part for DataCity {
    fn still(&self) -> bool {
        true
    }
    fn area(&self, _g: &Geo) -> Option<Vec<Rect>> {
        Some(vec![]) // painted only under the live picture, never shown in it
    }
    fn paint(&mut self, g: &Geo, _c: &Context, _s: &mut Shared) {
        self.render(g);
    }
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.render(g);
        let _ = c.set_source_surface(picture(&self.shown, g.w, g.h), 0.0, 0.0);
        let _ = c.paint();
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.render(g);
        self.tex = Some(up.add(picture(&self.shown, g.w, g.h), Kind::Label));
        self.streak = Some(up.add(streak(), Kind::Smooth));
        self.dot = Some(up.add(soft_dot(64, &SOFT_DOT), Kind::Sprite));
        self.uploads.clear();
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        self.render(g);
        let t = (w.clock * self.s.fps).floor() / self.s.fps;
        let files = w.reading(&self.s.files).0 / 100.0;
        let traffic = w.reading(&self.s.traffic).0 / 100.0;
        let Some(city) = self.city.clone() else { return 0 };
        // the faces change at most fifteen times a second: worked out again only when their moment does
        let tm = (t * 15.0).floor() / 15.0;
        let (marks, pulses) = {
            let Some(work) = &mut self.work else { return 0 };
            work.advance(&city, t, files, traffic);
            let marks = (tm != self.moment).then(|| work.marks(&city, tm, self.look.select, rgb3(self.s.fresh)));
            (marks, work.pulses(&city, t))
        };
        if let Some(marks) = marks {
            self.moment = tm;
            self.retrace(marks);
        }
        self.place(&pulses, t, traffic);
        let mut h = 0x9e37u64;
        hash_verts(&mut h, &self.strips);
        hash_verts(&mut h, &self.dots);
        for (r, _) in &self.uploads {
            hash_f(&mut h, (r.0 as f64 + r.1 as f64 * 65536.0) + t);
        }
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.s.fps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some(tex) = self.tex else { return };
        for (r, bytes) in std::mem::take(&mut self.uploads) {
            gfx.replace_bytes(tex, &bytes, r.0, r.1, r.2 - r.0, r.3 - r.1);
        }
        gfx.image(tex, 0.0, 0.0, g.w as f64, g.h as f64, 1.0, Blend::Opaque);
        gfx.draw(self.streak, &self.strips, Blend::Add);
        gfx.draw(self.dot, &self.dots, Blend::Add);
    }
}

/// A trail's picture: bright along its middle, fading to either side.
fn streak() -> ImageSurface {
    let s = surface(4, 64);
    {
        let c = ctx(&s);
        let g = cairo::LinearGradient::new(0.0, 0.0, 0.0, 64.0);
        for (at, a) in [(0.0, 0.0), (0.3, 0.12), (0.45, 0.7), (0.5, 1.0), (0.55, 0.7), (0.7, 0.12), (1.0, 0.0)] {
            g.add_color_stop_rgba(at, 1.0, 1.0, 1.0, a);
        }
        let _ = c.set_source(&g);
        c.set_operator(Operator::Source);
        let _ = c.paint();
    }
    s
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: KIND,
        defaults: r##"seed = 0
eye = [0.3, 4.0, -4.5]
pitch = -9.0
yaw = 0.0
fov = 64.0
samples = 2.0
lit = "#12d4f2"
blue = "#2a6cff"
smoked = "#0c2a3c"
text = "#e8fdff"
trace = "#2c46ff"
sky = "#040a1c"
select = "#c35cff"
fresh = "#ffffff"
pulse = "#c8f2ff"
bloom = 0.7
exposure = 1.2
files = "disk"
traffic = "cpu"
fps = 30.0"##,
        varies: &[],
        check: check::<Settings>,
        make: |_, t, _g| Ok(vec![Box::new(DataCity::new(t))]),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street(w: i32, h: i32) -> (Geo, DataCity) {
        crate::font::register();
        let theme = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/gibson");
        let l = crate::config::resolve(theme.to_str().unwrap(), &crate::config::Config::default(), crate::elements::kind_info, false)
            .unwrap_or_else(|e| panic!("{}", e.0));
        let g = Geo::new(w, h, "retro", Edges::NONE, Header::NONE, None, l.plain.clone());
        let defaults: Table = toml::from_str(kinds()[0].defaults).unwrap();
        let mut city = DataCity::new(&defaults);
        city.work_seed = Some(1);
        city.render(&g);
        (g, city)
    }

    fn world() -> World {
        World {
            clock: 0.0,
            clock_real: 0.0,
            frozen: Some(crate::live::Frozen { at: 13.62, mains: true }),
            demo: true,
            zoom: 1.0,
            traffic: crate::sources::Traffic::new(0.1, vec![]),
            network: Default::default(),
            readings: Default::default(),
            title_light: 1.0,
        }
    }

    /// The picture changed in place, panel by panel and mark by mark, for a while, is the picture traced
    /// whole with the same marks: no panel's old lettering is left where another panel shows too.
    #[test]
    fn a_picture_changed_in_place_is_the_picture_traced_whole() {
        let (g, mut city) = street(320, 180);
        city.per_frame = usize::MAX;
        let mut w = world();
        for f in 0..1500 {
            w.clock = f as f64 / 30.0;
            city.state(&g, &mut w);
            city.uploads.clear();
        }
        assert!(city.marks.len() > 20, "the street was at work: {} panels marked", city.marks.len());
        let (Some(c), Some(a), Some(cam), Some(bloom)) = (&city.city, &city.atlas, city.cam, &city.bloom) else { panic!("no street") };
        let tr = Tracer { city: c, atlas: a, cam, look: city.look, marks: &city.marks, ss: city.ss() };
        let raw = trace_rect(&tr, (0, 0, 320, 180));
        let mut off = 0;
        for (i, p) in raw.iter().enumerate() {
            let b = bloom.at(i % 320, i / 320);
            let want = tone(std::array::from_fn(|k| (p[k] + b[k] * city.look.bloom as f32) as f64), city.look.exposure);
            if (0..3).any(|k| want[k].abs_diff(city.shown[i * 4 + k]) > 8) {
                off += 1;
            }
        }
        // a pixel's own edges are traced with more rays than a change is worked out with: a few differ a little
        assert!(off * 200 < 320 * 180, "{off} pixels of {} differ from the picture traced whole", 320 * 180);
    }

    /// Each start writes the lists in sight with names of its own, a seed's the same each time.
    #[test]
    fn each_start_writes_its_own_names_on_the_lists() {
        let (_g, city) = street(320, 180);
        let (Some(c), Some(cam)) = (&city.city, city.cam) else { panic!("no street") };
        let names = |seed: u64| {
            let seen = |p: V3, row: f64| cam.project(p).is_some_and(|(x, y, ppu)| x > 0.0 && x < 320.0 && y > 0.0 && y < 180.0 && ppu * row > 4.0);
            let front = |p: V3| cam.project(p).is_some();
            let mut work = ops::Work::new(c, seed, seen, front, true);
            work.advance(c, 30.0, 0.0, 0.0);
            let mut m: Vec<_> = work.marks(c, 30.0, [0.0; 3], [0.0; 3]).into_iter().map(|(k, m)| (k, m.seed)).collect();
            m.sort_unstable();
            m
        };
        let (a, b) = (names(11), names(12));
        assert!(a.len() >= 3, "the lists were written: {}", a.len());
        assert_ne!(a, b, "two starts, two sets of names");
        assert_eq!(a, names(11), "a seed's names are the same each time");
    }

    /// How long a minute of the street at work takes on this machine, once settled: `state` thirty
    /// times a second, as the wall asks for it, without drawing. A measurement, run by hand:
    /// `cargo test --release --lib data_city::tests::a_minute_of_work -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[allow(clippy::print_stderr)] // a measurement: its number is what it is for
    fn a_minute_of_work() {
        let (g, mut city) = street(1920, 1080);
        let mut w = world();
        // a minute to settle (the panels' first marks are recorded then), and the next one timed
        let mut started = std::time::Instant::now();
        for f in 0..3600 {
            if f == 1800 {
                started = std::time::Instant::now();
            }
            w.clock = f as f64 / 30.0;
            city.state(&g, &mut w);
            city.uploads.clear();
        }
        eprintln!("a minute of work: {:.2} s", started.elapsed().as_secs_f64());
    }
}
