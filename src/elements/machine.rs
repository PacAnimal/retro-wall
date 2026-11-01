//! Element kinds for a machine in a painting: rotors that turn, a tank whose liquid fills a window cut by
//! a mask, gauges, pilot lamps that breathe, and vents that steam. Each places the theme's pictures on its
//! canvas, in the pixels of the first painting (`canvas_source`), as the painted kinds do, and each can
//! follow a reading: a source's name, or "" for its own idle motion. A source that is stale, or not
//! there, gives way to the idle motion, eased.
//!
//! Every picture is scaled for the monitor once and uploaded once; a frame is quads in the one vertex
//! stream, turned in the vertex positions. The tank's liquid is drawn through the renderer's mask.

use crate::elements::{Validate, check, each, positive, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Mask, Tex, Uploads};
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, Matrix, Operator, SurfacePattern};
use serde::Deserialize;
use std::f64::consts::PI;
use toml::Table;

/// The most items a list setting holds.
const MOST: usize = 64;
/// The clock a part is drawn at in the still picture.
const AT_REST: f64 = 0.0;

// --- shared -----------------------------------------------------------------------------------------

/// Where the theme's canvas stands on the screen: a point (x, y) of the first painting is at
/// (ox + x kx, oy + y ky).
#[derive(Clone, Copy)]
struct Place {
    ox: f64,
    oy: f64,
    kx: f64,
    ky: f64,
}

impl Place {
    fn new(g: &Geo) -> Place {
        let (s, ox, oy) = super::painted::covering(&g.th, g.w, g.h);
        let (_, _, sc) = super::painted::canvas(&g.th);
        Place { ox, oy, kx: s * sc.0, ky: s * sc.1 }
    }
    fn at(&self, p: [f64; 2]) -> (f64, f64) {
        (self.ox + p[0] * self.kx, self.oy + p[1] * self.ky)
    }
    fn size(&self, s: [f64; 2]) -> (f64, f64) {
        (s[0] * self.kx, s[1] * self.ky)
    }
}

/// A theme picture scaled to w x h pixels (at least one each way).
fn scaled(g: &Geo, rel: &str, w: f64, h: f64) -> ImageSurface {
    let src = load_png(g.th.path(rel));
    let (w, h) = ((w.ceil() as i32).clamp(1, 8192), (h.ceil() as i32).clamp(1, 8192));
    let out = surface(w, h);
    {
        let c = ctx(&out);
        c.scale(w as f64 / src.width() as f64, h as f64 / src.height() as f64);
        let p = SurfacePattern::create(&src);
        p.set_filter(Filter::Good);
        let _ = c.set_source(&p);
        let _ = c.paint();
    }
    out
}

/// The alpha of a picture laid on white: a mask that also tints.
fn whitened(s: &ImageSurface) -> ImageSurface {
    let out = surface(s.width(), s.height());
    {
        let c = ctx(&out);
        c.set_source_rgb(1.0, 1.0, 1.0);
        let _ = c.mask_surface(s, 0.0, 0.0);
    }
    out
}

/// Pictures packed into one texture, each with clear pixels round it; their texture pieces.
struct Atlas {
    surface: ImageSurface,
    uv: Vec<(f64, f64, f64, f64)>,
}

const PAD: i32 = 4;

impl Atlas {
    fn new(pics: &[ImageSurface]) -> Atlas {
        // shelves, at most 4096 wide
        let mut at = vec![];
        let (mut x, mut y, mut shelf, mut width) = (0, 0, 0, 1);
        for p in pics {
            let (w, h) = (p.width() + 2 * PAD, p.height() + 2 * PAD);
            if x > 0 && x + w > 4096 {
                (x, y, shelf) = (0, y + shelf, 0);
            }
            at.push((x + PAD, y + PAD));
            x += w;
            shelf = shelf.max(h);
            width = width.max(x);
        }
        let height = (y + shelf).max(1);
        let surface = surface(width, height);
        let uv = {
            let c = ctx(&surface);
            pics.iter()
                .zip(&at)
                .map(|(p, &(px, py))| {
                    let _ = c.set_source_surface(p, px as f64, py as f64);
                    let _ = c.paint();
                    let (w, h) = (width as f64, height as f64);
                    (px as f64 / w, py as f64 / h, (px + p.width()) as f64 / w, (py + p.height()) as f64 / h)
                })
                .collect()
        };
        Atlas { surface, uv }
    }

    /// Its picture, handed over: only the pieces' places are kept.
    fn take(&mut self) -> ImageSurface {
        std::mem::replace(&mut self.surface, surface(1, 1))
    }
}

/// A quad: its texture piece `uv` stretched over w x h, its point `pivot` (0 to 1 across the piece)
/// at (cx, cy), turned `angle` radians clockwise; tinted `rgb` at `alpha`, premultiplied.
fn turned(d: &mut Vec<f32>, cx: f64, cy: f64, w: f64, h: f64, pivot: (f64, f64), angle: f64, rgb: (f64, f64, f64), alpha: f64, uv: (f64, f64, f64, f64)) {
    let (s, c) = angle.sin_cos();
    let (u0, v0, u1, v1) = uv;
    let (r, g, b) = (rgb.0 * alpha, rgb.1 * alpha, rgb.2 * alpha);
    for (fu, fv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let (lx, ly) = ((fu - pivot.0) * w, (fv - pivot.1) * h);
        let (x, y) = (cx + lx * c - ly * s, cy + lx * s + ly * c);
        let (u, v) = (u0 + fu * (u1 - u0), v0 + fv * (v1 - v0));
        d.extend_from_slice(&[u as f32, v as f32, r as f32, g as f32, b as f32, alpha as f32, x as f32, y as f32]);
    }
}

/// Paint quads with cairo as Gfx draws them premultiplied: each its texture piece, mapped by its
/// first, second and fourth corners, clipped to its four. A quad tinted white shows the texture's own
/// colours; a tinted one, the tint through the texture's alpha. Used to draw a part at rest.
fn paint_quads(c: &Context, s: &ImageSurface, data: &[f32]) {
    let (w, h) = (s.width() as f64, s.height() as f64);
    let pattern = SurfacePattern::create(s);
    pattern.set_filter(Filter::Good);
    for q in data.as_chunks::<32>().0 {
        let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
        let alpha = q[5];
        if alpha <= 0.0 {
            continue;
        }
        let p = |i: usize| (q[8 * i + 6], q[8 * i + 7]);
        let ((x0, y0), (x1, y1), (x2, y2), (x3, y3)) = (p(0), p(1), p(2), p(3));
        // texture pixels of the first corner, and per unit along the edges to the second and fourth
        let (a, b) = ((q[0] * w, q[1] * h), ((q[8] - q[0]) * w, (q[9] - q[1]) * h));
        let d = ((q[24] - q[0]) * w, (q[25] - q[1]) * h);
        // the affine map taking texture pixels to the screen: it sends a to p0, a+b to p1, a+d to p3
        let det = b.0 * d.1 - b.1 * d.0;
        if det.abs() < 1e-12 {
            continue;
        }
        let ((ex, ey), (fx, fy)) = ((x1 - x0, y1 - y0), (x3 - x0, y3 - y0));
        // M [b d] = [e f]  =>  M = [e f] [b d]^-1
        let (ib, id) = ((d.1 / det, -b.1 / det), (-d.0 / det, b.0 / det));
        let (xx, xy) = (ex * ib.0 + fx * ib.1, ex * id.0 + fx * id.1);
        let (yx, yy) = (ey * ib.0 + fy * ib.1, ey * id.0 + fy * id.1);
        let m = Matrix::new(xx, yx, xy, yy, x0 - xx * a.0 - xy * a.1, y0 - yx * a.0 - yy * a.1);
        let _ = c.save();
        c.move_to(x0, y0);
        c.line_to(x1, y1);
        c.line_to(x2, y2);
        c.line_to(x3, y3);
        c.close_path();
        c.clip();
        c.transform(m);
        let white = (q[2] - alpha).abs() < 1e-4 && (q[3] - alpha).abs() < 1e-4 && (q[4] - alpha).abs() < 1e-4;
        if white {
            let _ = c.set_source(&pattern);
            let _ = c.paint_with_alpha(alpha);
        } else {
            c.set_source_rgba(q[2] / alpha, q[3] / alpha, q[4] / alpha, alpha);
            let _ = c.mask(&pattern);
        }
        let _ = c.restore();
    }
}

/// A hash of what a vertex array draws.
fn hash_verts(h: &mut u64, d: &[f32]) {
    for v in d {
        *h = (*h ^ v.to_bits() as u64).wrapping_mul(0x100000001b3);
    }
}

/// A reading that drives a part: the value it shows, eased from its own idle motion toward the reading
/// (0 to 1) while there is one, and back while the source is stale. A source may name alternatives,
/// "bat|swap": the first the machine has.
#[derive(Default)]
struct Drive {
    spec: String,
    source: Option<String>,
    shown: f64,
    weight: f64,
    at: Option<f64>,
}

impl Drive {
    fn new(spec: &str) -> Drive {
        Drive { spec: spec.trim().to_string(), ..Default::default() }
    }

    /// The value for now: `idle` blended toward the reading, each eased over `ease` seconds.
    fn value(&mut self, w: &World, ease: f64, idle: f64) -> f64 {
        if self.spec.is_empty() {
            return idle;
        }
        let demo = w.demo;
        let source = self.source.get_or_insert_with(|| crate::sources::first_present(&self.spec, demo).to_string());
        let got = (!w.stale(source)).then(|| (w.reading(source).0 / 100.0).clamp(0.0, 1.0));
        if w.frozen.is_some() {
            return got.unwrap_or(idle);
        }
        let k = match self.at {
            Some(t0) if ease > 0.0 => 1.0 - (-(w.clock_real - t0).max(0.0) / ease).exp(),
            Some(_) => 1.0,
            None => {
                self.shown = got.unwrap_or(idle);
                0.0
            }
        };
        self.at = Some(w.clock_real);
        match got {
            Some(v) => {
                self.shown += (v - self.shown) * k;
                self.weight += (1.0 - self.weight) * k;
            }
            None => self.weight -= self.weight * k,
        }
        idle + self.weight * (self.shown - idle)
    }
}

/// A setting naming a picture of the theme's.
fn named(name: &str, v: &str) -> Result<(), String> {
    if v.is_empty() { Err(format!("{name}: it names a picture of the theme's")) } else { Ok(()) }
}

fn most(name: &str, n: usize) -> Result<(), String> {
    if n > MOST { Err(format!("{name} has {n}: at most {MOST}")) } else { Ok(()) }
}

fn place_ok(name: &str, at: [f64; 2], size: [f64; 2]) -> Result<(), String> {
    each(&format!("{name}.at"), &at, -16384.0, 16384.0)?;
    each(&format!("{name}.size"), &size, 0.0, 16384.0)?;
    size.iter().try_for_each(|&s| positive(&format!("{name}.size"), s))
}

/// The topmost screen row of rectangles (centre, size) on the canvas, each turned any way about its
/// centre (half its diagonal up).
fn top_of(g: &Geo, boxes: impl Iterator<Item = ([f64; 2], [f64; 2], bool)>) -> i32 {
    let pl = Place::new(g);
    let top = boxes
        .map(|(at, size, turns)| {
            let ((_, cy), (w, h)) = (pl.at(at), pl.size(size));
            cy - if turns { w.hypot(h) / 2.0 } else { h / 2.0 }
        })
        .fold(g.h as f64, f64::min);
    (top.floor() as i32).clamp(0, g.h)
}

/// The clock held to whole frames at `fps`.
fn frame(t: f64, fps: f64) -> f64 {
    (t * fps).floor() / fps
}

// --- rotors -----------------------------------------------------------------------------------------

fn half() -> [f64; 2] {
    [0.5, 0.5]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rotor {
    image: String,
    /// Where its pivot stands.
    at: [f64; 2],
    size: [f64; 2],
    /// Degrees a second, clockwise, at rest.
    speed: f64,
    #[serde(default)]
    phase: f64,
    /// Its pivot, 0 to 1 across the picture.
    #[serde(default = "half")]
    pivot: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RotorsSettings {
    source: String,
    load: f64,
    ease: f64,
    fps: f64,
    rotors: Vec<Rotor>,
}

impl Validate for RotorsSettings {
    fn validate(&self) -> Result<(), String> {
        within("load", self.load, 0.0, 50.0)?;
        within("ease", self.ease, 0.0, 600.0)?;
        within("fps", self.fps, 1.0, 60.0)?;
        most("rotors", self.rotors.len())?;
        self.rotors.iter().enumerate().try_for_each(|(i, r)| {
            let n = format!("rotors[{i}]");
            named(&format!("{n}.image"), &r.image)?;
            place_ok(&n, r.at, r.size)?;
            within(&format!("{n}.speed"), r.speed, -3600.0, 3600.0)?;
            within(&format!("{n}.phase"), r.phase, -360000.0, 360000.0)?;
            each(&format!("{n}.pivot"), &r.pivot, 0.0, 1.0)
        })
    }
}

/// Sprites turning about their pivots, each at its own speed, all of them faster with a reading: at
/// `speed` with none (or at 0), `load` times that at 100, eased. The angle adds up the speed, so a
/// change of speed never jumps.
pub struct Rotors {
    s: RotorsSettings,
    drive: Drive,
    images: Vec<String>,
    atlas: Option<Atlas>,
    tex: Option<Tex>,
    angles: Vec<f64>,
    last: Option<f64>,
    verts: Vec<f32>,
}

impl Rotors {
    fn new(t: &Table) -> Self {
        let s: RotorsSettings = settings(t);
        let mut images: Vec<String> = vec![];
        for r in &s.rotors {
            if !images.contains(&r.image) {
                images.push(r.image.clone());
            }
        }
        let n = s.rotors.len();
        Rotors { drive: Drive::new(&s.source), s, images, atlas: None, tex: None, angles: vec![0.0; n], last: None, verts: Vec::with_capacity(n * 32) }
    }

    fn atlas(&mut self, g: &Geo) -> &Atlas {
        let pl = Place::new(g);
        let s = &self.s;
        self.atlas.get_or_insert_with(|| {
            let pics: Vec<ImageSurface> = self
                .images
                .iter()
                .map(|img| {
                    let (w, h) =
                        s.rotors.iter().filter(|r| &r.image == img).map(|r| pl.size(r.size)).fold((1.0f64, 1.0f64), |a, b| (a.0.max(b.0), a.1.max(b.1)));
                    scaled(g, img, w, h)
                })
                .collect();
            Atlas::new(&pics)
        })
    }

    fn factor(&self, v: f64) -> f64 {
        1.0 + (self.s.load - 1.0) * v
    }

    fn turn(&mut self, t: f64, v: f64, closed: bool) {
        let k = self.factor(v);
        let dt = self.last.map(|l| (t - l).max(0.0));
        for (a, r) in self.angles.iter_mut().zip(&self.s.rotors) {
            *a = match dt {
                Some(dt) if !closed => (*a + dt * r.speed * k).rem_euclid(360.0),
                _ => (r.phase + t * r.speed * k).rem_euclid(360.0),
            };
        }
        self.last = Some(t);
    }

    fn quads(&mut self, g: &Geo) {
        let pl = Place::new(g);
        let uvs = self.atlas(g).uv.clone();
        self.verts.clear();
        for (r, a) in self.s.rotors.iter().zip(&self.angles) {
            let i = self.images.iter().position(|x| x == &r.image).unwrap_or(0);
            let ((cx, cy), (w, h)) = (pl.at(r.at), pl.size(r.size));
            turned(&mut self.verts, cx, cy, w, h, (r.pivot[0], r.pivot[1]), a.to_radians(), (1.0, 1.0, 1.0), 1.0, uvs[i]);
        }
    }
}

impl Part for Rotors {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        // a pivot off centre: the farthest corner from it
        top_of(
            g,
            self.s
                .rotors
                .iter()
                .map(|r| (r.at, [r.size[0] * 2.0 * r.pivot[0].max(1.0 - r.pivot[0]), r.size[1] * 2.0 * r.pivot[1].max(1.0 - r.pivot[1])], true)),
        )
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.atlas(g);
        if let Some(a) = &mut self.atlas {
            self.tex = Some(up.add(a.take(), Kind::Sprite));
        }
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        let t = frame(w.clock, self.s.fps);
        let v = self.drive.value(w, self.s.ease, 0.0);
        self.turn(t, v, w.frozen.is_some());
        self.quads(g);
        let mut h = 0;
        hash_verts(&mut h, &self.verts);
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.s.fps
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        gfx.draw(self.tex, &self.verts, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.last = None;
        self.turn(AT_REST, 0.0, true);
        self.last = None;
        self.quads(g);
        if let Some(a) = &self.atlas {
            paint_quads(c, &a.surface, &self.verts);
        }
    }
}

// --- the tank ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TankSettings {
    mask: String,
    liquid: String,
    frame: String,
    glass: String,
    at: [f64; 2],
    size: [f64; 2],
    source: String,
    ease: f64,
    mean: f64,
    amplitude: f64,
    period: f64,
    phase: f64,
    cavity: Colour,
    shade: f64,
    overscan: f64,
    drift: f64,
    ripple: f64,
    meniscus: Colour,
    meniscus_alpha: f64,
    bubbles: i64,
    bubble: Colour,
    bubble_alpha: f64,
    fps: f64,
}

impl Validate for TankSettings {
    fn validate(&self) -> Result<(), String> {
        named("mask", &self.mask)?;
        named("liquid", &self.liquid)?;
        place_ok("tank", self.at, self.size)?;
        within("ease", self.ease, 0.0, 600.0)?;
        within("mean", self.mean, 0.0, 1.0)?;
        within("amplitude", self.amplitude, 0.0, 1.0)?;
        within("period", self.period, 1.0, 86400.0)?;
        within("phase", self.phase, -1000.0, 1000.0)?;
        within("shade", self.shade, 0.0, 1.0)?;
        within("overscan", self.overscan, 0.0, 0.5)?;
        within("drift", self.drift, 0.0, 0.5)?;
        if self.drift > self.overscan {
            return Err(format!("drift is {}: at most overscan ({}), or the liquid's edge would show", num(self.drift), num(self.overscan)));
        }
        within("ripple", self.ripple, 0.0, 50.0)?;
        within("meniscus_alpha", self.meniscus_alpha, 0.0, 1.0)?;
        within("bubbles", self.bubbles as f64, 0.0, MOST as f64)?;
        within("bubble_alpha", self.bubble_alpha, 0.0, 1.0)?;
        within("fps", self.fps, 1.0, 60.0)
    }
}

/// Where the mask lets anything through, as fractions of its picture: left, top, right, bottom.
fn aperture(mask: &mut ImageSurface) -> (f64, f64, f64, f64) {
    let (w, h, stride) = (mask.width() as usize, mask.height() as usize, mask.stride() as usize);
    let px = pixels(mask);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px.get(y * stride + 4 * x + 3).is_some_and(|&a| a > 0) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (x0 as f64 / w as f64, y0 as f64 / h as f64, x1 as f64 / w as f64, y1 as f64 / h as f64)
}

/// A bubble's ring, white on transparency.
fn ring(size: i32) -> ImageSurface {
    let out = surface(size, size);
    {
        let c = ctx(&out);
        let s = size as f64;
        c.set_line_width(s * 0.16);
        c.arc(s / 2.0, s / 2.0, s * 0.34, 0.0, 2.0 * PI);
        c.set_source_rgb(1.0, 1.0, 1.0);
        let _ = c.stroke();
    }
    out
}

/// The tank's pictures for a monitor: its atlas (the window's mask, white; glass; frame; a bubble's
/// ring; a white patch), the liquid, and where the window lets anything through.
struct TankPics {
    atlas: Atlas,
    liquid: ImageSurface,
    aperture: (f64, f64, f64, f64),
}

const MASK: usize = 0;
const GLASS: usize = 1;
const FRAME: usize = 2;
const RING: usize = 3;
const WHITE: usize = 4;

/// A liquid filling a window from its foot: exactly empty at 0, and full at 1, of the window the mask's
/// alpha cuts (its rows with anything in them). Under it the window's cavity; over it the glass, then
/// the frame. The liquid's picture drifts within `overscan` of its edges by at most `drift` of its size;
/// its surface ripples by up to `ripple` canvas pixels, still at empty and full, under a meniscus line;
/// bubbles rise within it. Its level follows a reading, or with none `mean` + `amplitude` sin(2 pi t /
/// `period` + `phase`).
pub struct Tank {
    s: TankSettings,
    drive: Drive,
    pics: Option<TankPics>,
    texs: Option<(Tex, Tex)>,
    level: f64,
    t: f64,
    /// The cavity, glass and frame; the liquid; the meniscus and bubbles (both through the mask).
    under: Vec<f32>,
    over: Vec<f32>,
    liquid: Vec<f32>,
    marks: Vec<f32>,
}

impl Tank {
    fn new(t: &Table) -> Self {
        let s: TankSettings = settings(t);
        Tank { drive: Drive::new(&s.source), s, pics: None, texs: None, level: 0.0, t: 0.0, under: vec![], over: vec![], liquid: vec![], marks: vec![] }
    }

    fn pics(&mut self, g: &Geo) -> &TankPics {
        let s = &self.s;
        self.pics.get_or_insert_with(|| {
            let pl = Place::new(g);
            let (w, h) = pl.size(s.size);
            let mut mask_full = load_png(g.th.path(&s.mask));
            let aperture = aperture(&mut mask_full);
            let mask = whitened(&scaled(g, &s.mask, w, h));
            let pic = |rel: &str| if rel.is_empty() { surface(1, 1) } else { scaled(g, rel, w, h) };
            let white = surface(8, 8);
            {
                let c = ctx(&white);
                c.set_source_rgb(1.0, 1.0, 1.0);
                let _ = c.paint();
            }
            let atlas = Atlas::new(&[mask, pic(&s.glass), pic(&s.frame), ring(48), white]);
            let k = 1.0 + 2.0 * s.overscan;
            let liquid = scaled(g, &s.liquid, w * k, h * k);
            if s.shade > 0.0 {
                let c = ctx(&liquid);
                c.set_operator(Operator::Atop);
                let (r, gg, b) = s.cavity.rgb();
                c.set_source_rgba(r, gg, b, s.shade);
                let _ = c.paint();
            }
            TankPics { atlas, liquid, aperture }
        })
    }

    /// The level and the tank's clock for time t.
    fn settle(&mut self, w: Option<&World>, t: f64) {
        let s = &self.s;
        let idle = (s.mean + s.amplitude * (t * 2.0 * PI / s.period + s.phase).sin()).clamp(0.0, 1.0);
        self.level = match w {
            Some(w) => self.drive.value(w, s.ease, idle).clamp(0.0, 1.0),
            None => idle,
        };
        self.t = t + s.phase;
    }

    fn quads(&mut self, g: &Geo, soft: bool) {
        let pl = Place::new(g);
        let (level, t) = (self.level, self.t);
        let s = &self.s;
        let Some(p) = self.pics.as_ref() else { return };
        let ((cx, cy), (w, h)) = (pl.at(s.at), pl.size(s.size));
        let (x0, y0) = (cx - w / 2.0, cy - h / 2.0);
        let uv = &p.atlas.uv;
        self.under.clear();
        self.over.clear();
        self.liquid.clear();
        self.marks.clear();
        turned(&mut self.under, cx, cy, w, h, (0.5, 0.5), 0.0, s.cavity.rgb(), 1.0, uv[MASK]);
        for (i, show) in [(GLASS, !s.glass.is_empty()), (FRAME, !s.frame.is_empty())] {
            if show {
                turned(&mut self.over, cx, cy, w, h, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), 1.0, uv[i]);
            }
        }
        if level <= 0.0 {
            return;
        }
        let (al, at, ar, ab) = p.aperture;
        let (left, right, top, bottom) = (x0 + al * w, x0 + ar * w, y0 + at * h, y0 + ab * h);
        if right <= left || bottom <= top {
            return;
        }
        let amp = if level >= 1.0 { 0.0 } else { (s.size[0] * 0.008).min(s.ripple) * pl.ky };
        let surface_y = bottom - level * (bottom - top);
        let wave = |x: f64| {
            let u = (x - x0) / w * 2.0 * PI;
            surface_y + (u * 2.0 + t * 1.1).sin() * amp + (u * 3.0 - t * 0.7).sin() * amp * 0.3
        };
        // the liquid's picture, overscanned and drifting
        let k = 1.0 + 2.0 * s.overscan;
        let (lw, lh) = (w * k, h * k);
        let (lx, ly) = (x0 - w * s.overscan + (t * 0.15).sin() * w * s.drift, y0 - h * s.overscan + (t * 0.19).cos() * h * s.drift);
        let luv = |x: f64, y: f64| (((x - lx) / lw) as f32, ((y - ly) / lh) as f32);
        let columns = ((right - left) / 4.0).ceil().clamp(8.0, 160.0) as usize;
        let step = (right - left) / columns as f64;
        let line = (h * 0.003).max(1.0);
        let (mr, mg, mb) = s.meniscus.rgb();
        let ma = s.meniscus_alpha;
        let (wu0, wv0, wu1, wv1) = uv[WHITE];
        let white = (wu0 + (wu1 - wu0) * 0.25, wv0 + (wv1 - wv0) * 0.25, wu0 + (wu1 - wu0) * 0.75, wv0 + (wv1 - wv0) * 0.75);
        // On the GPU the surface's edge is a band a pixel deep whose coverage falls from 1 to 0 across
        // it (the surface is all but level, so its distance is its height); cairo antialiases its own
        // edges, so at rest the liquid is cut sharp. Full, there is no edge: the window is filled.
        let feather = if soft && level < 1.0 { 0.5 } else { 0.0 };
        let (mr, mg, mb) = (mr * ma, mg * ma, mb * ma);
        for j in 0..columns {
            let (xa, xb) = (left + j as f64 * step, left + (j + 1) as f64 * step);
            let (ya, yb) = (wave(xa), wave(xb));
            let (ta, tb) = ((ya + feather).min(bottom), (yb + feather).min(bottom));
            for (x, y) in [(xa, ta), (xb, tb), (xb, bottom), (xa, bottom)] {
                let (u, v) = luv(x, y);
                self.liquid.extend_from_slice(&[u, v, 1.0, 1.0, 1.0, 1.0, x as f32, y as f32]);
            }
            if feather > 0.0 {
                for (x, y, k) in [(xa, ya - feather, 0.0f32), (xb, yb - feather, 0.0), (xb, tb, 1.0), (xa, ta, 1.0)] {
                    let (u, v) = luv(x, y);
                    self.liquid.extend_from_slice(&[u, v, k, k, k, k, x as f32, y as f32]);
                }
            }
            if level < 1.0 && ma > 0.0 {
                // the meniscus: on the GPU bright at its middle, clear a pixel past each side
                let (half, edge) = if soft { (line / 2.0 + 0.5, 0.0) } else { (line / 2.0, 1.0) };
                let c = |k: f64| [(mr * k) as f32, (mg * k) as f32, (mb * k) as f32, (ma * k) as f32];
                let rows = [(ya - half, yb - half, edge), (ya, yb, 1.0), (ya + half, yb + half, edge)];
                let bands: &[(usize, usize)] = if soft { &[(0, 1), (1, 2)] } else { &[(0, 2)] };
                for &(top, foot) in bands {
                    let (t, f) = (rows[top], rows[foot]);
                    let corners = [
                        (xa, t.0, white.0, white.1, t.2),
                        (xb, t.1, white.2, white.1, t.2),
                        (xb, f.1, white.2, white.3, f.2),
                        (xa, f.0, white.0, white.3, f.2),
                    ];
                    for (x, y, u, v, k) in corners {
                        let [r, gg, b, a] = c(k);
                        self.marks.extend_from_slice(&[u as f32, v as f32, r, gg, b, a, x as f32, y as f32]);
                    }
                }
            }
        }
        // bubbles rising, each cut where the liquid ends
        let (br, bg, bb) = s.bubble.rgb();
        let (ru0, rv0, ru1, rv1) = uv[RING];
        for i in 0..s.bubbles as usize {
            let fi = i as f64;
            let rise = (t * (0.09 + fi * 0.002) + fi * 0.618_033_988_75).rem_euclid(1.0);
            let y = bottom - rise * (bottom - top);
            let x = x0 + w * (0.27 + 0.46 * (fi * 0.381_966).rem_euclid(1.0)) + (t * 0.6 + fi).sin() * w * 0.012;
            let r = w * (0.003 + (i % 3) as f64 * 0.0015) / 0.68 + 1.0; // the ring's quad, round its stroke
            let (top_y, foot) = (y - r, y + r);
            let cut = wave(x).max(top_y);
            if cut >= foot || x < left || x > right {
                continue;
            }
            let v_cut = rv0 + (rv1 - rv0) * (cut - top_y) / (foot - top_y);
            let a = s.bubble_alpha;
            let (cr, cg, cb) = ((br * a) as f32, (bg * a) as f32, (bb * a) as f32);
            for (px, py, u, v) in [(x - r, cut, ru0, v_cut), (x + r, cut, ru1, v_cut), (x + r, foot, ru1, rv1), (x - r, foot, ru0, rv1)] {
                self.marks.extend_from_slice(&[u as f32, v as f32, cr, cg, cb, a as f32, px as f32, py as f32]);
            }
        }
    }

    /// The mask over the tank's rectangle on the screen.
    fn mask(&self, g: &Geo, tex: Tex) -> Option<Mask> {
        let pl = Place::new(g);
        let p = self.pics.as_ref()?;
        let ((cx, cy), (w, h)) = (pl.at(self.s.at), pl.size(self.s.size));
        Some(Mask::over(tex, (cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0), p.atlas.uv[MASK]))
    }
}

impl Part for Tank {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        top_of(g, std::iter::once((self.s.at, self.s.size, false)))
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.pics(g);
        if let Some(p) = &mut self.pics {
            let liquid = std::mem::replace(&mut p.liquid, surface(1, 1));
            self.texs = Some((up.add(p.atlas.take(), Kind::Sprite), up.add(liquid, Kind::Smooth)));
        }
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        self.pics(g);
        let t = frame(w.clock, self.s.fps);
        self.settle(Some(&*w), t);
        self.quads(g, true);
        let mut h = 0;
        hash_verts(&mut h, &self.liquid);
        hash_verts(&mut h, &self.marks);
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.s.fps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some((atlas, liquid)) = self.texs else { return };
        gfx.draw(Some(atlas), &self.under, Blend::Premultiplied);
        if let Some(m) = self.mask(g, atlas) {
            gfx.draw_masked(Some(liquid), m, &self.liquid, Blend::Premultiplied);
            gfx.draw_masked(Some(atlas), m, &self.marks, Blend::Premultiplied);
        }
        gfx.draw(Some(atlas), &self.over, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.pics(g);
        self.settle(None, AT_REST);
        self.quads(g, false);
        let Some(p) = &self.pics else { return };
        paint_quads(c, &p.atlas.surface, &self.under);
        // the liquid and its marks, cut by the mask
        let pl = Place::new(g);
        let ((cx, cy), (w, h)) = (pl.at(self.s.at), pl.size(self.s.size));
        let (x0, y0) = ((cx - w / 2.0).floor(), (cy - h / 2.0).floor());
        let inside = surface((w.ceil() as i32 + 2).clamp(1, 16384), (h.ceil() as i32 + 2).clamp(1, 16384));
        {
            let q = ctx(&inside);
            q.translate(-x0, -y0);
            paint_quads(&q, &p.liquid, &self.liquid);
            paint_quads(&q, &p.atlas.surface, &self.marks);
            q.set_operator(Operator::DestIn);
            let mut m = vec![];
            turned(&mut m, cx, cy, w, h, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), 1.0, p.atlas.uv[MASK]);
            paint_quads(&q, &p.atlas.surface, &m);
        }
        let _ = c.set_source_surface(&inside, x0, y0);
        let _ = c.paint();
        paint_quads(c, &p.atlas.surface, &self.over);
    }
}

// --- gauges -----------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gauge {
    at: [f64; 2],
    size: f64,
    #[serde(default)]
    phase: f64,
    /// Its reading; the element's without one.
    source: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GaugesSettings {
    dial: String,
    needle: String,
    needle_size: f64,
    sweep: [f64; 2],
    source: String,
    ease: f64,
    mean: f64,
    swing: f64,
    rate: f64,
    wobble: f64,
    fps: f64,
    gauges: Vec<Gauge>,
}

impl Validate for GaugesSettings {
    fn validate(&self) -> Result<(), String> {
        named("dial", &self.dial)?;
        named("needle", &self.needle)?;
        within("needle_size", self.needle_size, 0.05, 4.0)?;
        each("sweep", &self.sweep, -360.0, 360.0)?;
        within("ease", self.ease, 0.0, 600.0)?;
        within("mean", self.mean, 0.0, 1.0)?;
        within("swing", self.swing, 0.0, 1.0)?;
        within("rate", self.rate, 0.0, 100.0)?;
        within("wobble", self.wobble, 0.0, 0.2)?;
        within("fps", self.fps, 1.0, 60.0)?;
        most("gauges", self.gauges.len())?;
        self.gauges.iter().enumerate().try_for_each(|(i, g)| {
            let n = format!("gauges[{i}]");
            place_ok(&n, g.at, [g.size, g.size])?;
            within(&format!("{n}.phase"), g.phase, -1000.0, 1000.0)
        })
    }
}

/// Dials whose needles turn about their hubs: from `sweep[0]` degrees (clockwise from straight up) at
/// 0 to `sweep[1]` at 1. Each follows its reading, eased, with a small `wobble`; with none, `mean` +
/// `swing` sin(`rate` t + phase). The dial stays still.
pub struct Gauges {
    s: GaugesSettings,
    drives: Vec<Drive>,
    atlas: Option<Atlas>,
    tex: Option<Tex>,
    values: Vec<f64>,
    verts: Vec<f32>,
}

impl Gauges {
    fn new(t: &Table) -> Self {
        let s: GaugesSettings = settings(t);
        let drives = s.gauges.iter().map(|g| Drive::new(g.source.as_deref().unwrap_or(&s.source))).collect();
        let n = s.gauges.len();
        Gauges { s, drives, atlas: None, tex: None, values: vec![0.0; n], verts: vec![] }
    }

    fn atlas(&mut self, g: &Geo) -> &Atlas {
        let pl = Place::new(g);
        let s = &self.s;
        self.atlas.get_or_insert_with(|| {
            let big = s.gauges.iter().map(|x| x.size).fold(1.0, f64::max);
            let (w, h) = pl.size([big, big]);
            Atlas::new(&[scaled(g, &s.dial, w, h), scaled(g, &s.needle, w * s.needle_size, h * s.needle_size)])
        })
    }

    fn settle(&mut self, w: Option<&World>, t: f64) {
        let s = &self.s;
        for (i, (gauge, drive)) in s.gauges.iter().zip(self.drives.iter_mut()).enumerate() {
            let idle = s.mean + s.swing * (t * s.rate + gauge.phase).sin();
            let v = match w {
                Some(w) => drive.value(w, s.ease, idle),
                None => idle,
            };
            self.values[i] = (v + s.wobble * (t * 5.0 + gauge.phase).sin()).clamp(0.0, 1.0);
        }
    }

    fn quads(&mut self, g: &Geo) {
        let pl = Place::new(g);
        let uv = self.atlas(g).uv.clone();
        let s = &self.s;
        self.verts.clear();
        for (gauge, v) in s.gauges.iter().zip(&self.values) {
            let ((cx, cy), (w, h)) = (pl.at(gauge.at), pl.size([gauge.size, gauge.size]));
            turned(&mut self.verts, cx, cy, w, h, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), 1.0, uv[0]);
            let angle = s.sweep[0] + (s.sweep[1] - s.sweep[0]) * v;
            turned(&mut self.verts, cx, cy, w * s.needle_size, h * s.needle_size, (0.5, 0.5), angle.to_radians(), (1.0, 1.0, 1.0), 1.0, uv[1]);
        }
    }
}

impl Part for Gauges {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        let k = self.s.needle_size.max(1.0);
        top_of(g, self.s.gauges.iter().map(|x| (x.at, [x.size * k, x.size * k], true)))
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.atlas(g);
        if let Some(a) = &mut self.atlas {
            self.tex = Some(up.add(a.take(), Kind::Sprite));
        }
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        let t = frame(w.clock, self.s.fps);
        self.settle(Some(&*w), t);
        self.quads(g);
        let mut h = 0;
        hash_verts(&mut h, &self.verts);
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.s.fps
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        gfx.draw(self.tex, &self.verts, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.settle(None, AT_REST);
        self.quads(g);
        if let Some(a) = &self.atlas {
            paint_quads(c, &a.surface, &self.verts);
        }
    }
}

// --- pilot lamps ------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PilotLamp {
    at: [f64; 2],
    size: f64,
    /// Seconds a breath.
    period: f64,
    #[serde(default)]
    phase: f64,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum HaloBlend {
    Screen,
    Add,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LampsSettings {
    off: String,
    lens: String,
    halo: Colour,
    halo_size: f64,
    halo_strength: f64,
    halo_blend: HaloBlend,
    low: f64,
    curve: f64,
    on: bool,
    source: String,
    flicker: f64,
    ease: f64,
    fps: f64,
    lamps: Vec<PilotLamp>,
}

impl Validate for LampsSettings {
    fn validate(&self) -> Result<(), String> {
        named("off", &self.off)?;
        named("lens", &self.lens)?;
        within("halo_size", self.halo_size, 0.0, 20.0)?;
        within("halo_strength", self.halo_strength, 0.0, 1.0)?;
        within("low", self.low, 0.0, 1.0)?;
        within("curve", self.curve, 0.05, 10.0)?;
        within("flicker", self.flicker, 0.0, 1.0)?;
        within("ease", self.ease, 0.0, 600.0)?;
        within("fps", self.fps, 1.0, 60.0)?;
        most("lamps", self.lamps.len())?;
        self.lamps.iter().enumerate().try_for_each(|(i, l)| {
            let n = format!("lamps[{i}]");
            place_ok(&n, l.at, [l.size, l.size])?;
            within(&format!("{n}.period"), l.period, 0.5, 3600.0)?;
            within(&format!("{n}.phase"), l.phase, -1000.0, 1000.0)
        })
    }
}

/// Lamps: each its dark housing, its lit lens laid over it at its brightness, and a soft halo round
/// it, screened or added. Each breathes in its own slow time, from `low` to full (shaped by `curve`);
/// a reading makes them flicker by up to `flicker` of their light. `on = false`: dark, no halo.
pub struct PilotLamps {
    s: LampsSettings,
    drive: Drive,
    atlas: Option<Atlas>,
    tex: Option<Tex>,
    bright: Vec<f64>,
    lit: Vec<f32>,
    halos: Vec<f32>,
}

impl PilotLamps {
    fn new(t: &Table) -> Self {
        let s: LampsSettings = settings(t);
        let n = s.lamps.len();
        PilotLamps { drive: Drive::new(&s.source), s, atlas: None, tex: None, bright: vec![0.0; n], lit: vec![], halos: vec![] }
    }

    fn atlas(&mut self, g: &Geo) -> &Atlas {
        let pl = Place::new(g);
        let s = &self.s;
        self.atlas.get_or_insert_with(|| {
            let big = s.lamps.iter().map(|x| x.size).fold(1.0, f64::max);
            let (w, h) = pl.size([big, big]);
            let halo = soft_dot((w.max(h) * 2.0 * s.halo_size).clamp(8.0, 512.0) as i32, &[(0.0, 0.30), (0.25, 0.14), (1.0, 0.0)]);
            Atlas::new(&[scaled(g, &s.off, w, h), scaled(g, &s.lens, w, h), halo])
        })
    }

    fn settle(&mut self, w: Option<&World>, t: f64) {
        let s = &self.s;
        let d = match w {
            Some(w) => self.drive.value(w, s.ease, 0.0),
            None => 0.0,
        };
        for (i, l) in s.lamps.iter().enumerate() {
            let breath = s.low + (1.0 - s.low) * (0.5 + 0.5 * (t * 2.0 * PI / l.period + l.phase).sin()).powf(s.curve);
            // a flicker of a few beats a second, its own for each lamp
            let fi = i as f64 * 1.7;
            let n = ((t * 11.0 + fi).sin() * 0.5 + (t * 17.3 + fi * 2.3).sin() * 0.3 + (t * 5.1 + fi * 0.7).sin() * 0.2).clamp(-1.0, 1.0);
            self.bright[i] = if s.on { (breath + s.flicker * d * n).clamp(0.0, 1.0) } else { 0.0 };
        }
    }

    fn quads(&mut self, g: &Geo) {
        let pl = Place::new(g);
        let uv = self.atlas(g).uv.clone();
        let s = &self.s;
        self.lit.clear();
        self.halos.clear();
        let colour = s.halo.rgb();
        for (l, &b) in s.lamps.iter().zip(&self.bright) {
            let ((cx, cy), (w, h)) = (pl.at(l.at), pl.size([l.size, l.size]));
            turned(&mut self.lit, cx, cy, w, h, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), 1.0, uv[0]);
            if b > 0.0 {
                turned(&mut self.lit, cx, cy, w, h, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), b, uv[1]);
                let k = b * s.halo_strength;
                if k > 0.0 && s.halo_size > 0.0 {
                    turned(&mut self.halos, cx, cy, w * 2.0 * s.halo_size, h * 2.0 * s.halo_size, (0.5, 0.5), 0.0, colour, k, uv[2]);
                }
            }
        }
    }

    fn blend(&self) -> Blend {
        match self.s.halo_blend {
            HaloBlend::Screen => Blend::Screen,
            HaloBlend::Add => Blend::Add,
        }
    }
}

impl Part for PilotLamps {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        let k = (2.0 * self.s.halo_size).max(1.0);
        top_of(g, self.s.lamps.iter().map(|l| (l.at, [l.size * k, l.size * k], false)))
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.atlas(g);
        if let Some(a) = &mut self.atlas {
            self.tex = Some(up.add(a.take(), Kind::Sprite));
        }
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        let t = frame(w.clock, self.s.fps);
        self.settle(Some(&*w), t);
        // on 64 steps of brightness: a breath changes the picture a few times a second, not every frame
        for b in &mut self.bright {
            *b = (*b * 64.0).round() / 64.0;
        }
        self.quads(g);
        let mut h = 0;
        hash_verts(&mut h, &self.lit);
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.s.fps
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        gfx.draw(self.tex, &self.lit, Blend::Premultiplied);
        gfx.draw(self.tex, &self.halos, self.blend());
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.settle(None, AT_REST);
        self.quads(g);
        let Some(a) = &self.atlas else { return };
        paint_quads(c, &a.surface, &self.lit);
        let _ = c.save();
        c.set_operator(if self.s.halo_blend == HaloBlend::Screen { Operator::Screen } else { Operator::Add });
        paint_quads(c, &a.surface, &self.halos);
        let _ = c.restore();
    }
}

// --- steam ------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vent {
    at: [f64; 2],
    size: [f64; 2],
    /// The steam's box: width, height.
    plume: [f64; 2],
    intensity: f64,
    #[serde(default)]
    phase: f64,
    /// Its reading; the element's without one.
    source: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SteamSettings {
    vent: String,
    puffs: Vec<String>,
    particles: i64,
    period: f64,
    opacity: f64,
    rest: f64,
    source: String,
    ease: f64,
    fps: f64,
    vents: Vec<Vent>,
}

impl Validate for SteamSettings {
    fn validate(&self) -> Result<(), String> {
        if self.puffs.is_empty() || self.puffs.len() > 8 {
            return Err(format!("puffs has {}: it names 1 to 8 pictures of the theme's", self.puffs.len()));
        }
        self.puffs.iter().enumerate().try_for_each(|(i, p)| named(&format!("puffs[{i}]"), p))?;
        within("particles", self.particles as f64, 1.0, 32.0)?;
        within("period", self.period, 0.1, 600.0)?;
        within("opacity", self.opacity, 0.0, 1.0)?;
        within("rest", self.rest, 0.0, 1.0)?;
        within("ease", self.ease, 0.0, 600.0)?;
        within("fps", self.fps, 1.0, 60.0)?;
        most("vents", self.vents.len())?;
        self.vents.iter().enumerate().try_for_each(|(i, v)| {
            let n = format!("vents[{i}]");
            place_ok(&n, v.at, v.size)?;
            place_ok(&n, v.at, v.plume)?;
            within(&format!("{n}.intensity"), v.intensity, 0.0, 4.0)?;
            within(&format!("{n}.phase"), v.phase, -1000.0, 1000.0)
        })
    }
}

/// Vents (their nozzles, or none with `vent = ""`), and the steam each gives: `particles` puffs rising
/// from its mouth, growing, each living `period` seconds, its opacity sin(pi age)^2 x `opacity` x
/// strength, so that a puff comes and goes clear. The strength is the vent's `intensity`; with a
/// reading, from `rest` of it at 0 to all of it at 100, eased.
pub struct Steam {
    s: SteamSettings,
    drives: Vec<Drive>,
    atlas: Option<Atlas>,
    tex: Option<Tex>,
    strength: Vec<f64>,
    t: f64,
    verts: Vec<f32>,
}

impl Steam {
    fn new(t: &Table) -> Self {
        let s: SteamSettings = settings(t);
        let drives = s.vents.iter().map(|v| Drive::new(v.source.as_deref().unwrap_or(&s.source))).collect();
        let n = s.vents.len();
        Steam { s, drives, atlas: None, tex: None, strength: vec![0.0; n], t: 0.0, verts: vec![] }
    }

    fn atlas(&mut self, g: &Geo) -> &Atlas {
        let pl = Place::new(g);
        let s = &self.s;
        self.atlas.get_or_insert_with(|| {
            let big = |f: fn(&Vent) -> [f64; 2]| s.vents.iter().map(f).fold([1.0f64, 1.0f64], |a, b| [a[0].max(b[0]), a[1].max(b[1])]);
            let (vw, vh) = pl.size(big(|v| v.size));
            let (pw, ph) = pl.size(big(|v| v.plume));
            let mut pics = vec![if s.vent.is_empty() { surface(1, 1) } else { scaled(g, &s.vent, vw, vh) }];
            pics.extend(s.puffs.iter().map(|p| scaled(g, p, pw * 0.75, ph * 0.72)));
            Atlas::new(&pics)
        })
    }

    fn settle(&mut self, w: Option<&World>, t: f64) {
        let s = &self.s;
        for (i, (v, drive)) in s.vents.iter().zip(self.drives.iter_mut()).enumerate() {
            let k = match w {
                Some(w) => drive.value(w, s.ease, 1.0),
                None => 1.0,
            };
            self.strength[i] = v.intensity * (s.rest + (1.0 - s.rest) * k);
        }
        self.t = t;
    }

    fn quads(&mut self, g: &Geo) {
        let pl = Place::new(g);
        let uv = self.atlas(g).uv.clone();
        let s = &self.s;
        self.verts.clear();
        let n = s.particles as usize;
        for (v, &strength) in s.vents.iter().zip(&self.strength) {
            let ((cx, cy), (w, h)) = (pl.at(v.at), pl.size(v.size));
            if !s.vent.is_empty() {
                turned(&mut self.verts, cx, cy, w, h, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), 1.0, uv[0]);
            }
            let (pw, ph) = pl.size(v.plume);
            let (ox, oy) = (cx - pw * 0.53, cy - h * 0.45 - ph * 0.98);
            for i in 0..n {
                let age = ((self.t + v.phase) / s.period + i as f64 / n as f64).rem_euclid(1.0);
                let alpha = (PI * age).sin().powi(2) * s.opacity * strength;
                if alpha <= 0.0 {
                    continue;
                }
                let (sw, sh) = (pw * (0.25 + age * 0.5), ph * (0.3 + age * 0.42));
                let x = ox + pw * 0.53 + (age * 2.0 + i as f64).sin() * pw * 0.07 - age * pw * 0.09;
                let y = oy + ph * 0.98 - age * ph * 0.42;
                let puff = 1 + i % s.puffs.len();
                turned(&mut self.verts, x, y, sw, sh, (0.55, 0.96), -0.12 * age, (1.0, 1.0, 1.0), alpha.min(1.0), uv[puff]);
            }
        }
    }
}

impl Part for Steam {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        // a puff's top at its highest: its foot risen 0.42 of the plume above the vent's mouth, and
        // its own height (0.72 of the plume) above that
        top_of(
            g,
            self.s
                .vents
                .iter()
                .flat_map(|v| [(v.at, v.size, false), ([v.at[0], v.at[1] - v.size[1] * 0.45 - v.plume[1] * 0.56], [v.plume[0] * 2.0, v.plume[1] * 2.0], true)]),
        )
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.atlas(g);
        if let Some(a) = &mut self.atlas {
            self.tex = Some(up.add(a.take(), Kind::Sprite));
        }
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        let t = frame(w.clock, self.s.fps);
        self.settle(Some(&*w), t);
        self.quads(g);
        let mut h = 0;
        hash_verts(&mut h, &self.verts);
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.s.fps
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        gfx.draw(self.tex, &self.verts, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.settle(None, AT_REST);
        self.quads(g);
        if let Some(a) = &self.atlas {
            paint_quads(c, &a.surface, &self.verts);
        }
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "rotors",
            defaults: r#"source = ""
load = 4.0
ease = 3.0
fps = 30.0
rotors = []"#,
            varies: &[],
            check: check::<RotorsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Rotors::new(t))]),
        },
        Kind {
            name: "tank",
            defaults: r##"mask = ""
liquid = ""
frame = ""
glass = ""
at = [0.0, 0.0]
size = [100.0, 100.0]
source = ""
ease = 4.0
mean = 0.5
amplitude = 0.3
period = 23.0
phase = 0.0
cavity = "#071014"
shade = 0.2
overscan = 0.07
drift = 0.025
ripple = 3.0
meniscus = "#92ffe4"
meniscus_alpha = 0.7
bubbles = 14
bubble = "#c8fff4"
bubble_alpha = 0.42
fps = 30.0"##,
            varies: &[],
            check: check::<TankSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Tank::new(t))]),
        },
        Kind {
            name: "gauges",
            defaults: r#"dial = ""
needle = ""
needle_size = 0.85
sweep = [-130.0, 130.0]
source = ""
ease = 1.5
mean = 0.5
swing = 0.34
rate = 0.27
wobble = 0.015
fps = 30.0
gauges = []"#,
            varies: &[],
            check: check::<GaugesSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Gauges::new(t))]),
        },
        Kind {
            name: "pilot-lamps",
            defaults: r##"off = ""
lens = ""
halo = "#ff992d"
halo_size = 1.5
halo_strength = 0.65
halo_blend = "screen"
low = 0.12
curve = 0.4
on = true
source = ""
flicker = 0.35
ease = 0.3
fps = 30.0
lamps = []"##,
            varies: &[],
            check: check::<LampsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(PilotLamps::new(t))]),
        },
        Kind {
            name: "steam",
            defaults: r#"vent = ""
puffs = []
particles = 4
period = 2.0
opacity = 0.17
rest = 0.15
source = ""
ease = 1.5
fps = 30.0
vents = []"#,
            varies: &[],
            check: check::<SteamSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Steam::new(t))]),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turned_quad_turns_about_its_pivot() {
        let mut d = vec![];
        turned(&mut d, 10.0, 20.0, 4.0, 2.0, (0.5, 0.5), PI / 2.0, (1.0, 1.0, 1.0), 1.0, (0.0, 0.0, 1.0, 1.0));
        // its top left corner, (-2, -1) from the pivot, turned a quarter clockwise: (1, -2)
        assert!((d[6] - 11.0).abs() < 1e-5 && (d[7] - 18.0).abs() < 1e-5, "{:?}", &d[6..8]);
        let mut e = vec![];
        turned(&mut e, 0.0, 0.0, 10.0, 10.0, (0.5, 0.5), 0.0, (1.0, 1.0, 1.0), 1.0, (0.0, 0.0, 1.0, 1.0));
        assert_eq!((e[6], e[7], e[22], e[23]), (-5.0, -5.0, 5.0, 5.0));
    }

    #[test]
    fn the_aperture_is_where_the_mask_lets_anything_through() {
        let mut m = surface(10, 20);
        {
            let c = ctx(&m);
            c.rectangle(2.0, 5.0, 6.0, 10.0);
            c.set_source_rgba(0.0, 0.0, 0.0, 1.0);
            let _ = c.fill();
        }
        assert_eq!(aperture(&mut m), (0.2, 0.25, 0.8, 0.75));
        assert_eq!(aperture(&mut surface(4, 4)), (0.0, 0.0, 0.0, 0.0));
    }
}
