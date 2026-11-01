//! Element kinds for a painted scene: the painting covering the screen, fading patches of it (lights),
//! a flickering lamp, a screen showing broadcasts, and a window whose glass is cut out by a mask, with
//! rain behind it and drops on it. Sizes, positions and masks come from the theme's file.

use crate::config::Theme;
use crate::elements::{Validate, check, each, settings, within};
use crate::gfx::{self, Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, Matrix, Operator, SurfacePattern};
use serde::Deserialize;
use std::f64::consts::PI;
use std::sync::Arc;
use toml::Table;

/// The theme's canvas: the painting's size, and the first painting's scale onto it (masks and patches
/// are placed in the first painting's pixels).
pub fn canvas(th: &Theme) -> ((f64, f64), (f64, f64), (f64, f64)) {
    let tw = &th.tw;
    let p = |a: [f64; 2]| (a[0], a[1]);
    (p(tw.canvas), p(tw.canvas_source), p(tw.canvas_scale))
}

/// How the painting covers a screen of width x height, cropped about crop_centre: (scale, x, y).
pub fn covering(th: &Theme, width: i32, height: i32) -> (f64, f64, f64) {
    th.covering(width, height, canvas(th).0)
}

fn picture(th: &Theme, rel: &str) -> SurfacePattern {
    let p = SurfacePattern::create(load_png(th.path(rel)));
    p.set_filter(Filter::Good);
    p
}

/// A theme picture standing on the painting at (x, y), w by h, scaled by `k` onto it, drawn on the
/// screen aligned to whole pixels: (surface, screen x, screen y).
fn on_screen(g: &Geo, rel: &str, x: f64, y: f64, w: f64, h: f64, k: (f64, f64)) -> (ImageSurface, i32, i32) {
    let th = &g.th;
    let (s, ox, oy) = covering(th, g.w, g.h);
    let (sx, sy) = (ox + x * s, oy + y * s);
    let (x0, y0) = (sx.floor(), sy.floor());
    let out = surface((w * s).ceil() as i32 + 2, (h * s).ceil() as i32 + 2);
    {
        let c = ctx(&out);
        c.translate(sx - x0, sy - y0);
        c.scale(s * k.0, s * k.1);
        let _ = c.set_source(picture(th, rel));
        let _ = c.paint();
    }
    (out, x0 as i32, y0 as i32)
}

/// A setting naming a picture: it must name one.
fn named(name: &str, v: &str) -> Result<(), String> {
    if v.is_empty() { Err(format!("{name}: it names a picture of the theme's")) } else { Ok(()) }
}

/// A setting that is a range [lo, hi]: lo at most hi, and lo over `floor` (or at least it, with `or_equal`).
fn range(name: &str, v: [f64; 2], floor: f64, or_equal: bool) -> Result<(), String> {
    if v[0] > v[1] {
        return Err(format!("{name} is {v:?}: it is [low, high]"));
    }
    if v[0] < floor || (!or_equal && v[0] == floor) {
        return Err(format!("{name} is {v:?}: it must be {} {floor}", if or_equal { "at least" } else { "over" }));
    }
    Ok(())
}

/// Say something about a theme's file once per run.
fn say_once(what: &std::path::Path, message: &str) {
    static SAID: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap();
    if !said.iter().any(|p| p == what) {
        crate::errln!("{}: {message}", what.display());
        said.push(what.to_path_buf());
    }
}

// --- the painting ---------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Steady {
    image: String,
    at: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaintingSettings {
    image: String,
    steady: Vec<Steady>,
}

impl Validate for PaintingSettings {
    fn validate(&self) -> Result<(), String> {
        if self.image.is_empty() && self.steady.is_empty() {
            return Err("image: it names the painting, or steady its patches alone".into());
        }
        self.steady.iter().enumerate().try_for_each(|(i, p)| {
            named(&format!("steady[{i}].image"), &p.image)?;
            each(&format!("steady[{i}].at"), &p.at, -16384.0, 16384.0)
        })
    }
}

/// The painting, covering the screen, with its steady patches laid on it for good. With no image, the
/// patches alone, over what is under them (a pipe in front of the machine's moving parts).
pub struct Painting {
    image: String,
    steady: Vec<(String, f64, f64)>,
}

impl Painting {
    fn new(t: &Table) -> Self {
        let s: PaintingSettings = settings(t);
        Painting { image: s.image, steady: s.steady.into_iter().map(|p| (p.image, p.at[0], p.at[1])).collect() }
    }
}

impl Part for Painting {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let th = &g.th;
        let (s, ox, oy) = covering(th, g.w, g.h);
        let (_, _, sc) = canvas(th);
        let out = surface(g.w, g.h);
        {
            let p = ctx(&out);
            p.translate(ox, oy);
            p.scale(s, s);
            if !self.image.is_empty() {
                let _ = p.set_source(picture(th, &self.image));
                let _ = p.paint();
            }
            p.scale(sc.0, sc.1);
            for (img, x, y) in &self.steady {
                let patch = picture(th, img);
                patch.set_matrix(Matrix::new(1.0, 0.0, 0.0, 1.0, -x, -y));
                let _ = p.set_source(&patch);
                let _ = p.paint();
            }
        }
        let _ = c.set_source_surface(&out, 0.0, 0.0);
        let _ = c.paint();
    }
}

/// The window's mask: how much glass shows at a point of the painting, 0 to 1.
pub struct Glass {
    w: i32,
    h: i32,
    stride: i32,
    data: Vec<u8>,
    pillar: (f64, f64),
    scale: (f64, f64),
}

impl Glass {
    pub fn new(th: &Theme, mask: &str, pillar: (f64, f64)) -> Glass {
        let mut s = load_png(th.path(mask));
        let (w, h, stride) = (s.width(), s.height(), s.stride());
        Glass { w, h, stride, data: pixels(&mut s), pillar, scale: canvas(th).2 }
    }
    pub fn at(&self, x: f64, y: f64) -> f64 {
        let (x, y) = ((x / self.scale.0) as i64, (y / self.scale.1) as i64);
        if !(0 <= x && x < self.w as i64 && 0 <= y && y < self.h as i64) || (self.pillar.0 <= x as f64 && (x as f64) < self.pillar.1) {
            return 0.0;
        }
        self.data[(y * self.stride as i64 + x * 4 + 3) as usize] as f64 / 255.0
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameSettings {
    image: String,
    mask: String,
    pillar: [f64; 2],
    /// The glass-drops element whose beads sit on the glass, or "".
    beads: String,
}

impl Validate for FrameSettings {
    fn validate(&self) -> Result<(), String> {
        named("image", &self.image)?;
        named("mask", &self.mask)?;
        range("pillar", self.pillar, 0.0, true)?;
        each("pillar", &self.pillar, 0.0, 16384.0)
    }
}

/// The painting over the rain: the glass cut out by its mask, the pillar restored, and the beads that
/// sit on the glass for good.
pub struct GlassFrame {
    image: String,
    mask: String,
    pillar: (f64, f64),
    beads: Option<DropsSettings>,
}

impl GlassFrame {
    fn new(th: &Theme, t: &Table) -> Self {
        let s: FrameSettings = settings(t);
        let beads = (!s.beads.is_empty()).then(|| th.scene.iter().find(|e| e.name == s.beads).map(|e| (e.kind.clone(), e.settings.clone()))).flatten();
        let beads = match beads {
            Some((kind, t)) if kind == "glass-drops" => Some(settings::<DropsSettings>(&t)),
            None if s.beads.is_empty() => None,
            _ => {
                crate::errln!("glass-frame: beads = {:?} is not a glass-drops element of the scene: no beads", s.beads);
                None
            }
        };
        GlassFrame { image: s.image, mask: s.mask, pillar: (s.pillar[0], s.pillar[1]), beads }
    }
}

impl Part for GlassFrame {
    fn still(&self) -> bool {
        true
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let th = &g.th;
        let (s, ox, oy) = covering(th, g.w, g.h);
        let (_, src, sc) = canvas(th);
        let out = surface(g.w, g.h);
        {
            let p = ctx(&out);
            p.translate(ox, oy);
            p.scale(s, s);
            let _ = p.set_source(picture(th, &self.image));
            let _ = p.paint();
            p.scale(sc.0, sc.1);
            p.set_operator(Operator::DestOut);
            let _ = p.set_source(picture(th, &self.mask));
            let _ = p.paint();
            p.set_operator(Operator::Over);
            p.rectangle(self.pillar.0, 0.0, self.pillar.1 - self.pillar.0, src.1);
            p.clip();
            p.scale(1.0 / sc.0, 1.0 / sc.1);
            let _ = p.set_source(picture(th, &self.image));
            let _ = p.paint();
        }
        if let Some(t) = &self.beads {
            let beads = sitting(t, g);
            let p = ctx(&out);
            let _ = p.set_source_surface(&beads, 0.0, 0.0);
            let _ = p.paint();
        }
        let _ = c.set_source_surface(&out, 0.0, 0.0);
        let _ = c.paint();
    }
}

// --- lights ----------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    /// The theme's label for it.
    #[allow(dead_code)]
    name: String,
    image: String,
    /// Where it stands on the first painting: x, y, width, height.
    rect: [f64; 4],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LightsSettings {
    city: [f64; 2],
    dim: [f64; 2],
    fade: [f64; 2],
    hold: [f64; 2],
    seed: i64,
    steps: f64,
    patches: Vec<Patch>,
}

impl Validate for LightsSettings {
    fn validate(&self) -> Result<(), String> {
        range("city", self.city, 0.0, false)?;
        each("city", &self.city, 1.0, 3600.0)?;
        range("dim", self.dim, 0.0, true)?;
        within("dim", self.dim[1], 0.0, 1.0)?;
        range("fade", self.fade, 0.0, false)?;
        each("fade", &self.fade, 0.05, 60.0)?;
        range("hold", self.hold, 0.0, true)?;
        each("hold", &self.hold, 0.0, 600.0)?;
        within("steps", self.steps, 1.0, 256.0)?;
        if self.patches.len() > 256 {
            return Err(format!("patches has {}: at most 256", self.patches.len()));
        }
        for (i, p) in self.patches.iter().enumerate() {
            named(&format!("patches[{i}].image"), &p.image)?;
            let [x, y, w, h] = p.rect;
            let fits = (0.0..=16384.0).contains(&x) && (0.0..=16384.0).contains(&y) && w > 0.0 && w <= 4096.0 && h > 0.0 && h <= 4096.0;
            if !fits {
                return Err(format!("patches[{i}].rect is {:?}: it is [x, y, width, height] on the first painting, width and height over 0", p.rect));
            }
        }
        Ok(())
    }
}

/// Patches of the painting laid over it, each fading to `dim` of its light once every `city` seconds
/// or so, over `fade` seconds each way, held dim for `hold`; on `steps` steps of brightness.
pub struct Lights {
    tex: Option<Tex>,
    patches: Vec<(String, (f64, f64, f64, f64))>,
    schedule: Vec<(f64, f64, f64, f64, f64)>,
    steps: f64,
    placed: Vec<(i32, i32, i32, i32)>,
    spans: Vec<(f64, f64, f64)>,
    shown: Vec<f64>,
    verts: Vec<f32>,
}

impl Lights {
    fn new(t: &Table) -> Self {
        let s: LightsSettings = settings(t);
        let patches: Vec<_> = s.patches.into_iter().map(|p| (p.image, (p.rect[0], p.rect[1], p.rect[2], p.rect[3]))).collect();
        let mut rng = Rng::new(s.seed);
        let (city, dim, fade, hold) = (s.city, s.dim, s.fade, s.hold);
        let schedule = patches
            .iter()
            .map(|_| (rng.range(city[0], city[1]), rng.range(0.0, 60.0), rng.range(dim[0], dim[1]), rng.range(fade[0], fade[1]), rng.range(hold[0], hold[1])))
            .collect();
        let n = patches.len();
        Lights { tex: None, patches, schedule, steps: s.steps, placed: vec![], spans: vec![], shown: vec![0.0; n], verts: Vec::with_capacity(n * 32) }
    }
    fn brightness(&self, i: usize, t: f64) -> f64 {
        let (period, moment, level, fade, hold) = self.schedule[i];
        let u = (t + moment).rem_euclid(period);
        let dark = if u < fade {
            smooth(u / fade)
        } else if u < fade + hold {
            1.0
        } else {
            1.0 - smooth((u - fade - hold) / fade)
        };
        1.0 - (1.0 - level) * dark
    }
}

impl Part for Lights {
    fn live(&self) -> bool {
        true
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let th = &g.th;
        if self.patches.is_empty() {
            return;
        }
        let (_, _, sc) = canvas(th);
        let made: Vec<_> = self.patches.iter().map(|(img, (x, y, w, h))| on_screen(g, img, x * sc.0, y * sc.1, w * sc.0, h * sc.1, sc)).collect();
        let width: i32 = made.iter().map(|m| m.0.width()).sum();
        let height = made.iter().map(|m| m.0.height()).max().unwrap_or(1);
        let atlas = surface(width, height);
        {
            let c = ctx(&atlas);
            let mut at = 0;
            for (s, x, y) in &made {
                let _ = c.set_source_surface(s, at as f64, 0.0);
                let _ = c.paint();
                self.spans.push((at as f64 / width as f64, (at + s.width()) as f64 / width as f64, s.height() as f64 / height as f64));
                self.placed.push((*x, *y, s.width(), s.height()));
                at += s.width();
            }
        }
        self.tex = Some(up.add(atlas, Kind::Label));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let mut h = 0;
        for i in 0..self.patches.len() {
            let v = (self.brightness(i, w.clock) * self.steps).round() / self.steps;
            self.shown[i] = v;
            hash_f(&mut h, v);
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some(tex) = self.tex else { return };
        self.verts.clear();
        for (i, &k) in self.shown.iter().enumerate() {
            if k > 0.0 {
                let (x, y, w, h) = self.placed[i];
                let (u0, u1, v1) = self.spans[i];
                quad(&mut self.verts, x as f64, y as f64, (x + w) as f64, (y + h) as f64, (1.0, 1.0, 1.0), k, (u0, 0.0, u1, v1));
            }
        }
        gfx.draw(Some(tex), &self.verts, Blend::Premultiplied);
    }
}

/// A texture stretched over a rectangle and tinted (premultiplied).
pub fn quad(d: &mut Vec<f32>, x0: f64, y0: f64, x1: f64, y1: f64, c: (f64, f64, f64), a: f64, uv: (f64, f64, f64, f64)) {
    let (r, g, b) = (c.0 * a, c.1 * a, c.2 * a);
    let (u0, v0, u1, v1) = uv;
    for (u, v, x, y) in [(u0, v0, x0, y0), (u1, v0, x1, y0), (u1, v1, x1, y1), (u0, v1, x0, y1)] {
        d.extend_from_slice(&[u as f32, v as f32, r as f32, g as f32, b as f32, a as f32, x as f32, y as f32]);
    }
}

/// The room's light flickers over an hour, repeating: [(when, depth, second depth or 0)].
fn flickers(seed: i64, quiet: (f64, f64), dip: (f64, f64)) -> Vec<(f64, f64, f64)> {
    let mut rng = Rng::new(seed);
    let mut out = vec![];
    let mut t = rng.range(2.0, 6.0);
    while t < 3600.0 {
        let first = rng.range(dip.0, dip.1);
        let second = if rng.f64() < 0.6 { rng.range(dip.0, dip.1) } else { 0.0 };
        out.push((t, first, second));
        t += rng.range(quiet.0, quiet.1);
    }
    out
}

fn flicker(events: &[(f64, f64, f64)], t: f64) -> f64 {
    let t = t.rem_euclid(3600.0);
    for &(when, first, second) in events {
        if when > t {
            break;
        }
        if t < when + 0.6 {
            let u = t - when;
            let mut level: f64 = 1.0;
            for (at, depth) in [(0.0, first), (0.3, if second != 0.0 { 1.0 - (1.0 - second) * 0.6 } else { 0.0 })] {
                if depth != 0.0 && at <= u && u < at + 0.18 {
                    let v = u - at;
                    level = level.min(1.0 - (1.0 - depth) * (if v < 0.06 { v / 0.06 } else { 1.0 - (v - 0.06) / 0.12 }));
                }
            }
            return level;
        }
    }
    1.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LampSettings {
    image: String,
    bright: f64,
    quiet: [f64; 2],
    dip: [f64; 2],
    seed: i64,
    steps: f64,
}

impl Validate for LampSettings {
    fn validate(&self) -> Result<(), String> {
        named("image", &self.image)?;
        within("bright", self.bright, 0.0, 1.0)?;
        range("quiet", self.quiet, 0.0, false)?;
        each("quiet", &self.quiet, 0.5, 3600.0)?;
        range("dip", self.dip, 0.0, true)?;
        within("dip", self.dip[1], 0.0, 1.0)?;
        within("steps", self.steps, 1.0, 256.0)
    }
}

/// A light laid over the whole painting at `bright`, dipping to `dip` of it in a blink or two every
/// `quiet` seconds or so.
pub struct Lamp {
    tex: Option<Tex>,
    image: String,
    bright: f64,
    events: Vec<(f64, f64, f64)>,
    steps: f64,
    placed: (i32, i32, i32, i32),
    k: f64,
}

impl Lamp {
    fn new(t: &Table) -> Self {
        let s: LampSettings = settings(t);
        Lamp {
            tex: None,
            image: s.image,
            bright: s.bright,
            events: flickers(s.seed, (s.quiet[0], s.quiet[1]), (s.dip[0], s.dip[1])),
            steps: s.steps,
            placed: (0, 0, 0, 0),
            k: 0.0,
        }
    }
}

impl Part for Lamp {
    fn live(&self) -> bool {
        true
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let th = &g.th;
        let ((cw, ch), _, _) = canvas(th);
        let (s, x, y) = on_screen(g, &self.image, 0.0, 0.0, cw, ch, (1.0, 1.0));
        self.placed = (x, y, s.width(), s.height());
        self.tex = Some(up.add(s, Kind::Label));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.k = (self.bright * flicker(&self.events, w.clock) * self.steps).round() / self.steps;
        self.k.to_bits()
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if let (Some(tex), true) = (self.tex, self.k > 0.0) {
            let (x, y, w, h) = self.placed;
            gfx.image(tex, x as f64, y as f64, w as f64, h as f64, self.k, Blend::Premultiplied);
        }
    }
}

// --- the screen ------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenSettings {
    channels: String,
    at: [f64; 2],
    show: [f64; 2],
    fade: f64,
    on: bool,
    seed: i64,
    #[serde(rename = "static")]
    static_: f64,
    tears: [f64; 2],
    tear: [f64; 2],
    steps: f64,
}

impl Validate for ScreenSettings {
    fn validate(&self) -> Result<(), String> {
        if self.channels.is_empty() {
            return Err("channels: it names the theme's directory of broadcasts (PNGs)".into());
        }
        each("at", &self.at, -16384.0, 16384.0)?;
        range("show", self.show, 0.0, false)?;
        each("show", &self.show, 0.5, 7200.0)?;
        within("fade", self.fade, 0.0, 60.0)?;
        within("static", self.static_, 0.0, 1.0)?;
        range("tears", self.tears, 0.0, false)?;
        each("tears", &self.tears, 0.5, 7200.0)?;
        range("tear", self.tear, 0.0, true)?;
        each("tear", &self.tear, 0.0, 60.0)?;
        within("steps", self.steps, 1.0, 256.0)
    }
}

/// A screen in the painting: one broadcast after another, each on for `show` seconds or so, every one
/// shown before any comes round again, the next faded in over `fade`; a faint flicker of static over
/// it, and every `tears` seconds or so a tear: a band of the picture pushed sideways for `tear` seconds.
/// With no broadcasts (no `channels` directory, or no PNGs in it) it draws nothing.
pub struct Screen {
    channels: Vec<String>,
    at: (f64, f64),
    fade: f64,
    on: bool,
    seed: i64,
    static_: f64,
    steps: f64,
    programme: Vec<(f64, usize)>,
    tears: Vec<(f64, f64, f64, f64, f64)>,
    placed: Vec<(i32, i32, i32, i32)>,
    /// The channels' textures, then the static's.
    texs: Vec<Tex>,
    shown: Option<(usize, usize, f64, usize, Option<(f64, f64, f64)>)>,
    scale: f64,
}

const GRAIN: usize = 4;

impl Screen {
    fn new(th: &Theme, t: &Table) -> Self {
        let s: ScreenSettings = settings(t);
        let dir = th.path(&s.channels);
        let mut channels: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .filter_map(|e| {
                    let n = e.ok()?.file_name().into_string().ok()?;
                    n.ends_with(".png").then_some(n)
                })
                .collect(),
            Err(e) => {
                say_once(&dir, &format!("{e}: the screen shows nothing"));
                vec![]
            }
        };
        if channels.is_empty() {
            say_once(&dir, "no broadcasts (PNGs) in it: the screen shows nothing");
        }
        channels.sort();
        let channels: Vec<String> = channels.into_iter().map(|n| format!("{}/{}", s.channels, n)).collect();
        let seed = s.seed;
        let (show, tears_every, tear) = (s.show, s.tears, s.tear);
        let mut rng = Rng::new(seed);
        let (mut programme, mut tt, mut order) = (vec![], 0.0, Vec::<usize>::new());
        while tt < 7200.0 && !channels.is_empty() {
            if order.is_empty() {
                order = (0..channels.len()).collect();
                rng.shuffle(&mut order);
            }
            programme.push((tt, order.pop().unwrap()));
            tt += rng.range(show[0], show[1]);
        }
        let mut rng = Rng::new(seed + 1);
        let (mut tears, mut tt) = (vec![], 0.0);
        loop {
            tt += rng.range(tears_every[0], tears_every[1]);
            if tt >= 7200.0 {
                break;
            }
            let until = tt + rng.range(tear[0], tear[1]);
            let top = rng.range(0.1, 0.85);
            let height = rng.range(0.015, 0.06);
            let sign = *rng.pick(&[-1.0, 1.0]);
            tears.push((tt, until, top, height, sign * rng.range(2.0, 6.0)));
        }
        Screen {
            channels,
            at: (s.at[0], s.at[1]),
            fade: s.fade,
            on: s.on,
            seed,
            static_: s.static_,
            steps: s.steps,
            programme,
            tears,
            placed: vec![],
            texs: vec![],
            shown: None,
            scale: 1.0,
        }
    }

    fn tuned(&self, t: f64) -> (usize, usize, f64) {
        let t = t.rem_euclid(7200.0);
        let p = &self.programme;
        for (i, &(start, channel)) in p.iter().enumerate() {
            let end = if i + 1 < p.len() { p[i + 1].0 } else { 7200.0 };
            if start <= t && t < end {
                let coming = if i + 1 < p.len() { p[i + 1].1 } else { p[0].1 };
                let f = if self.fade != 0.0 { smooth((t - (end - self.fade)) / self.fade) } else { 0.0 };
                return (channel, coming, f);
            }
        }
        (p[0].1, p[0].1, 0.0)
    }

    fn tearing(&self, t: f64) -> Option<(f64, f64, f64)> {
        let t = t.rem_euclid(7200.0);
        let n = self.tears.partition_point(|e| e.0 <= t);
        if n == 0 {
            return None;
        }
        let e = self.tears[n - 1];
        (t < e.1).then_some((e.2, e.3, e.4))
    }

    fn grain(&self, screen: &ImageSurface, k: usize) -> ImageSurface {
        let (sw, sh) = (screen.width(), screen.height());
        let (w, h) = ((sw + 1) / 2, (sh + 1) / 2);
        let table: Vec<u8> = (0..256).map(|v| (255.0 * (v as f64 / 255.0).powi(3)).round() as u8).collect();
        let specks: Vec<u8> = Rng::new(self.seed * 100 + k as i64).bytes((w * h) as usize).into_iter().map(|b| table[b as usize]).collect();
        let mut small = surface(w, h);
        {
            let stride = small.stride() as usize;
            let mut data = small.data().unwrap();
            for row in 0..h as usize {
                for x in 0..w as usize {
                    let b = specks[row * w as usize + x];
                    data[row * stride + 4 * x..row * stride + 4 * x + 4].copy_from_slice(&[b, b, b, b]);
                }
            }
        }
        small.mark_dirty();
        let out = surface(sw, sh);
        {
            let c = ctx(&out);
            c.scale(2.0, 2.0);
            let _ = c.set_source_surface(&small, 0.0, 0.0);
            c.source().set_filter(Filter::Nearest);
            let _ = c.paint();
            c.identity_matrix();
            c.set_operator(Operator::DestIn);
            let _ = c.set_source_surface(screen, 0.0, 0.0);
            let _ = c.paint();
        }
        out
    }

    fn put(&self, gfx: &mut Gfx, i: usize, strength: f64) {
        if strength > 0.0 {
            let (x, y, w, h) = self.placed[i];
            gfx.image(self.texs[i], x as f64, y as f64, w as f64, h as f64, strength, Blend::Premultiplied);
        }
    }
}

impl Part for Screen {
    fn live(&self) -> bool {
        true
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let th = &g.th;
        if self.channels.is_empty() {
            return;
        }
        let first = load_png(th.path(&self.channels[0]));
        let (w, h) = (first.width() as f64, first.height() as f64);
        let mut screen0 = None;
        for (k, name) in self.channels.iter().enumerate() {
            let (s, x, y) = on_screen(g, name, self.at.0, self.at.1, w, h, (1.0, 1.0));
            self.placed.push((x, y, s.width(), s.height()));
            if k == 0 {
                screen0 = Some(s.clone());
            }
            self.texs.push(up.add(s, Kind::Label));
        }
        let Some(screen0) = screen0 else { return };
        let (x, y, _, _) = self.placed[0];
        for k in 0..GRAIN {
            let s = self.grain(&screen0, k);
            self.placed.push((x, y, s.width(), s.height()));
            self.texs.push(up.add(s, Kind::Label));
        }
        self.scale = covering(th, g.w, g.h).0;
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        if !self.on || self.programme.is_empty() {
            self.shown = None;
            return 0;
        }
        let (channel, coming, f) = self.tuned(w.clock);
        let grain = if self.static_ != 0.0 && w.frozen.is_none() { ((w.clock * 12.0) as i64 % GRAIN as i64) as usize } else { 0 };
        let tear = self.tearing(w.clock);
        let f = (f * self.steps).round() / self.steps;
        self.shown = Some((channel, coming, f, grain, tear));
        let mut h = 1;
        for v in [channel as f64, coming as f64, f, grain as f64, tear.map_or(-1.0, |t| t.0 + t.1 * 7.0 + t.2 * 13.0)] {
            hash_f(&mut h, v);
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some((channel, coming, f, grain, tear)) = self.shown else { return };
        self.put(gfx, channel, 1.0);
        self.put(gfx, coming, f);
        if let Some((top, height, shift)) = tear {
            let (x, y, w, h) = self.placed[channel];
            let (x, y, w, h) = (x as f64, y as f64, w as f64, h as f64);
            let y0 = (top * h).round();
            let y1 = ((top + height) * h).round() + 1.0;
            let dx = (shift * self.scale).round();
            gfx.patches(self.texs[channel], &[gfx::Patch { sx0: 0.0, sy0: y0, sx1: w, sy1: y1, x0: x + dx, y0: y + y0, x1: x + dx + w, y1: y + y1 }], 1.0);
        }
        let n = self.channels.len();
        self.put(gfx, n + grain, self.static_ * if tear.is_some() { 2.5 } else { 1.0 });
    }
}

// --- the drops on the glass ------------------------------------------------------------------------

const CELL: i32 = 64;
const PAD: i32 = 8;
const LEFT: f64 = 150.0;

/// A drop of water on dark glass, white on transparency, `size` square, `long` times as tall as wide.
fn bead(size: i32, long: f64) -> ImageSurface {
    let out = surface(size, size);
    {
        let c = ctx(&out);
        c.translate(size as f64 / 2.0, size as f64 / 2.0);
        let r = size as f64 * 0.36 / long;
        c.scale(1.0, long);
        let body = cairo::RadialGradient::new(0.0, -0.15 * r, 0.0, 0.0, 0.0, r);
        body.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.06);
        body.add_color_stop_rgba(0.7, 1.0, 1.0, 1.0, 0.14);
        body.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.45);
        c.arc(0.0, 0.0, r, 0.0, 2.0 * PI);
        let _ = c.set_source(&body);
        let _ = c.fill();
        c.set_line_width(r * 0.18);
        c.arc(0.0, 0.0, r * 0.86, 25f64.to_radians(), 155f64.to_radians());
        c.set_source_rgba(1.0, 1.0, 1.0, 0.7);
        let _ = c.stroke();
        c.arc(0.0, 0.0, r * 0.95, 200f64.to_radians(), 340f64.to_radians());
        c.set_source_rgba(0.0, 0.0, 0.0, 0.5);
        let _ = c.stroke();
        c.arc(-0.34 * r, -0.38 * r, r * 0.18, 0.0, 2.0 * PI);
        c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        let _ = c.fill();
    }
    out
}

/// A wet trail, white on transparency: a soft band down its middle, strongest at its foot.
fn trickle(size: i32) -> ImageSurface {
    let out = surface(size, size);
    {
        let c = ctx(&out);
        let across = cairo::LinearGradient::new(0.0, 0.0, size as f64, 0.0);
        for (at, a) in [(0.0, 0.0), (0.3, 0.8), (0.5, 1.0), (0.7, 0.8), (1.0, 0.0)] {
            across.add_color_stop_rgba(at, 1.0, 1.0, 1.0, a);
        }
        let along = cairo::LinearGradient::new(0.0, 0.0, 0.0, size as f64);
        for (at, a) in [(0.0, 0.0), (0.85, 0.5), (1.0, 0.5)] {
            along.add_color_stop_rgba(at, 0.0, 0.0, 0.0, a);
        }
        let _ = c.set_source(&across);
        let _ = c.mask(&along);
    }
    out
}

#[derive(Clone, Copy)]
struct Own {
    size: f64,
    speed: f64,
    rate: f64,
    phase: f64,
    trail: f64,
    wander: f64,
    wave: f64,
    sit: f64,
    offset: f64,
}

#[derive(Clone, Copy)]
struct Plan {
    cycle: i64,
    start: f64,
    formed: f64,
    lets_go: f64,
    // its course: x0 + wander * sin(2 pi (y - y0) / wave + turn), None off the glass
    course: Option<(f64, f64)>, // (x0, turn)
    y0: f64,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct DropsSettings {
    mask: String,
    window: [f64; 2],
    pillar: [f64; 2],
    beads: i64,
    bead: [f64; 2],
    drops: i64,
    sit: [f64; 2],
    size: [f64; 2],
    speed: [f64; 2],
    stick: f64,
    wander: [f64; 2],
    trail: [f64; 2],
    opacity: f64,
    tint: Colour,
    seed: i64,
    rate: f64,
}

impl Validate for DropsSettings {
    fn validate(&self) -> Result<(), String> {
        named("mask", &self.mask)?;
        range("window", self.window, f64::NEG_INFINITY, true)?;
        each("window", &self.window, -16384.0, 16384.0)?;
        range("pillar", self.pillar, 0.0, true)?;
        each("pillar", &self.pillar, 0.0, 16384.0)?;
        within("beads", self.beads as f64, 0.0, 20000.0)?;
        within("drops", self.drops as f64, 0.0, 5000.0)?;
        range("bead", self.bead, 0.0, false)?;
        each("bead", &self.bead, 0.5, 200.0)?;
        range("sit", self.sit, 0.0, true)?;
        each("sit", &self.sit, 0.0, 3600.0)?;
        range("size", self.size, 0.0, false)?;
        each("size", &self.size, 1.0, 500.0)?;
        range("speed", self.speed, 0.0, false)?;
        each("speed", &self.speed, 1.0, 5000.0)?;
        within("stick", self.stick, 0.0, 1.0)?;
        range("wander", self.wander, 0.0, true)?;
        each("wander", &self.wander, 0.0, 100.0)?;
        range("trail", self.trail, 0.0, true)?;
        each("trail", &self.trail, 0.0, 5000.0)?;
        within("opacity", self.opacity, 0.0, 1.0)?;
        within("rate", self.rate, 1.0, 120.0)
    }
}

/// The beads of a glass-drops element, on a picture of the screen: drawn once, laid down with the frame.
fn sitting(t: &DropsSettings, g: &Geo) -> ImageSurface {
    let th = &g.th;
    let (s, ox, oy) = covering(th, g.w, g.h);
    let glass = Glass::new(th, &t.mask, (t.pillar[0], t.pillar[1]));
    let canvas_h = canvas(th).0.1;
    let tint = t.tint.rgb();
    let mut rng = Rng::new(t.seed * 3 + 1);
    let out = surface(g.w, g.h);
    let looks = [bead(CELL, 1.0), bead(CELL, 1.08), bead(CELL, 1.18)];
    let bsz = t.bead;
    {
        let c = ctx(&out);
        c.set_source_rgb(tint.0, tint.1, tint.2);
        for _ in 0..t.beads {
            let x = rng.range(t.window[0], t.window[1]);
            let y = rng.range(0.0, canvas_h);
            if glass.at(x, y) <= 0.0 {
                continue;
            }
            let size = (bsz[0] + (bsz[1] - bsz[0]) * rng.f64().powf(2.5)) * s;
            let k = size / CELL as f64;
            let _ = c.save();
            c.translate(ox + x * s - size / 2.0, oy + y * s - size / 2.0);
            c.scale(k, k);
            let look = rng.pick(&looks);
            let _ = c.mask_surface(look, 0.0, 0.0);
            let _ = c.restore();
        }
    }
    out
}

/// Rain on the glass: `beads` beads that sit on it for good (baked into the frame), and `drops` more
/// that sit for `sit` seconds or so, then run down, stopping and starting, leaving a wet trail.
pub struct Drops {
    th: Arc<Theme>,
    tex: Option<Tex>,
    mask: String,
    pillar: (f64, f64),
    seed: i64,
    window: (f64, f64),
    opacity: f64,
    stick: f64,
    tint: (f64, f64, f64),
    rate: f64,
    own: Vec<Own>,
    plans: Vec<Option<Plan>>,
    glass: Option<Glass>,
    uv: [(f64, f64, f64, f64); 3],
    clock: f64,
    canvas_h: f64,
    /// A frame's drops in drawing order, and their quads.
    order: Vec<(usize, Plan)>,
    verts: Vec<f32>,
}

impl Drops {
    fn new(th: &Arc<Theme>, t: &Table) -> Self {
        let s: DropsSettings = settings(t);
        let mut rng = Rng::new(s.seed);
        let (size, speed, trail, wander, sit) = (s.size, s.speed, s.trail, s.wander, s.sit);
        let own = (0..s.drops)
            .map(|_| Own {
                size: rng.range(size[0], size[1]),
                speed: rng.range(speed[0], speed[1]),
                rate: rng.range(0.5, 1.4),
                phase: rng.range(0.0, 2.0 * PI),
                trail: rng.range(trail[0], trail[1]),
                wander: rng.range(wander[0], wander[1]),
                wave: rng.range(140.0, 320.0),
                sit: rng.range(sit[0], sit[1]),
                offset: rng.range(0.0, 600.0),
            })
            .collect::<Vec<_>>();
        let n = own.len();
        Drops {
            th: th.clone(),
            tex: None,
            mask: s.mask,
            pillar: (s.pillar[0], s.pillar[1]),
            seed: s.seed,
            window: (s.window[0], s.window[1]),
            opacity: s.opacity,
            stick: s.stick,
            tint: s.tint.rgb(),
            rate: s.rate,
            plans: vec![None; n],
            own,
            glass: None,
            uv: [(0.0, 0.0, 0.0, 0.0); 3],
            clock: 0.0,
            canvas_h: canvas(th).0.1,
            order: Vec::with_capacity(n),
            verts: Vec::with_capacity(4096),
        }
    }

    fn glass(&mut self) -> &Glass {
        if self.glass.is_none() {
            self.glass = Some(Glass::new(&self.th, &self.mask, self.pillar));
        }
        self.glass.as_ref().unwrap()
    }

    fn plan(&mut self, i: usize, t: f64) -> Plan {
        let o = self.own[i];
        let life = o.sit + (self.canvas_h + 400.0) / o.speed;
        let cycle = ((t + o.offset) / life).floor() as i64;
        if let Some(p) = self.plans[i]
            && p.cycle == cycle
        {
            return p;
        }
        let start = cycle as f64 * life - o.offset;
        let mut rng = Rng::new(self.seed * 7919 + i as i64 * 131 + cycle);
        let x0 = rng.range(self.window.0, self.window.1);
        let y0 = rng.range(0.0, self.canvas_h * 0.9);
        let turn = rng.range(0.0, 2.0 * PI);
        let course = if self.glass().at(x0, y0) <= 0.0 { None } else { Some((x0, turn)) };
        let p = Plan { cycle, start, formed: start + 2.5, lets_go: start + o.sit, course, y0 };
        self.plans[i] = Some(p);
        p
    }

    /// Drop i at time t: (head y, trail, alpha), or None.
    fn drop(&self, i: usize, t: f64, p: &Plan) -> Option<(f64, f64, f64)> {
        let o = &self.own[i];
        p.course?;
        let run = t - p.lets_go;
        let moved = if run > 0.0 { o.speed * (run - self.stick * ((o.rate * run + o.phase).sin() - o.phase.sin()) / o.rate) } else { 0.0 };
        let alpha = self.opacity * smooth((t - p.start) / 2.5);
        if alpha <= 0.01 || p.y0 + moved >= self.canvas_h + o.size {
            return None;
        }
        Some((p.y0 + moved, o.trail.min(moved.max(0.0)), alpha))
    }

    fn x_at(&self, i: usize, p: &Plan, y: f64) -> f64 {
        let (x0, turn) = p.course.unwrap();
        let o = &self.own[i];
        x0 + o.wander * (2.0 * PI * (y - p.y0) / o.wave + turn).sin()
    }

    /// What drop i shows on the screen, as quads over the atlas; `cover` is the painting's place.
    fn items(&self, cover: (f64, f64, f64), i: usize, p: &Plan, (y, trail, alpha): (f64, f64, f64), d: &mut Vec<f32>) {
        let (s, ox, oy) = cover;
        let size = self.own[i].size;
        let mut put = |look: usize, cx: f64, cy: f64, w: f64, h: f64, a: f64| {
            let (w, h) = (w / 2.0, h / 2.0);
            quad(d, cx - w, cy - h, cx + w, cy + h, self.tint, a, self.uv[look]);
        };
        if trail > size {
            let (top, foot) = (y - trail, y - size * 0.3);
            put(2, ox + self.x_at(i, p, (top + foot) / 2.0) * s, oy + (top + foot) / 2.0 * s, size * 0.32 * s, (foot - top) * s, alpha * 0.8);
            let (mut k, mut yk) = (0usize, p.y0 + LEFT);
            while yk < y - size {
                if yk > top {
                    let fade = (yk - top) / trail;
                    let r = size * (0.22 + 0.12 * ((i + k) % 3) as f64);
                    put(1, ox + self.x_at(i, p, yk) * s, oy + yk * s, r * s, r * s, alpha * 0.85 * fade);
                }
                k += 1;
                yk += LEFT;
            }
        }
        put(0, ox + self.x_at(i, p, y) * s, oy + y * s, size * s, size * s, alpha);
    }
}

impl Part for Drops {
    fn live(&self) -> bool {
        true
    }
    fn textures(&mut self, _g: &Geo, _s: &Shared, up: &mut Uploads) {
        let looks = [bead(CELL, 1.22), bead(CELL, 1.0), trickle(CELL)];
        let atlas = surface(3 * (CELL + 2 * PAD), CELL + 2 * PAD);
        {
            let c = ctx(&atlas);
            let (aw, ah) = (atlas.width() as f64, atlas.height() as f64);
            for (k, look) in looks.iter().enumerate() {
                let x = k as i32 * (CELL + 2 * PAD) + PAD;
                let _ = c.set_source_surface(look, x as f64, PAD as f64);
                let _ = c.paint();
                self.uv[k] = (x as f64 / aw, PAD as f64 / ah, (x + CELL) as f64 / aw, (PAD + CELL) as f64 / ah);
            }
        }
        self.tex = Some(up.add(atlas, Kind::Sprite));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.clock = (w.clock * self.rate).round() / self.rate;
        self.clock.to_bits()
    }
    fn interval(&self) -> f64 {
        1.0 / self.rate
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let th = &g.th;
        let Some(tex) = self.tex else { return };
        // still drops first, then those forming or running
        let t = self.clock;
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        for still in [true, false] {
            for i in 0..self.own.len() {
                let p = self.plan(i, t);
                if p.course.is_some() && (t >= p.formed && t < p.lets_go) == still {
                    order.push((i, p));
                }
            }
        }
        let cover = covering(th, g.w, g.h);
        let mut d = std::mem::take(&mut self.verts);
        d.clear();
        for (i, p) in &order {
            if let Some(dr) = self.drop(*i, t, p) {
                self.items(cover, *i, p, dr, &mut d);
            }
        }
        gfx.draw(Some(tex), &d, Blend::Premultiplied);
        self.verts = d;
        self.order = order;
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "painting",
            defaults: r#"image = ""
steady = []"#,
            varies: &[],
            check: check::<PaintingSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Painting::new(t))]),
        },
        Kind {
            name: "lights",
            defaults: r#"city = [23.0, 48.0]
dim = [0.12, 0.5]
fade = [0.3, 0.8]
hold = [0.6, 3.5]
seed = 5
steps = 48
patches = []"#,
            varies: &[],
            check: check::<LightsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Lights::new(t))]),
        },
        Kind {
            name: "lamp",
            defaults: r#"image = ""
bright = 1.0
quiet = [12.0, 25.0]
dip = [0.6, 0.82]
seed = 11
steps = 48"#,
            varies: &[],
            check: check::<LampSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Lamp::new(t))]),
        },
        Kind {
            name: "screen",
            defaults: r#"channels = "tv"
at = [0.0, 0.0]
show = [15.0, 35.0]
fade = 0.4
on = true
seed = 3
static = 0.05
tears = [6.0, 20.0]
tear = [0.08, 0.22]
steps = 24"#,
            varies: &[],
            check: check::<ScreenSettings>,
            make: |_, t, g| Ok(vec![Box::new(Screen::new(&g.th, t))]),
        },
        Kind {
            name: "glass-drops",
            defaults: r##"mask = ""
window = [0.0, 1600.0]
pillar = [0.0, 0.0]
beads = 3000
bead = [4.0, 17.0]
drops = 260
sit = [4.0, 150.0]
size = [12.0, 24.0]
speed = [45.0, 120.0]
stick = 0.6
wander = [1.0, 3.5]
trail = [200.0, 600.0]
opacity = 0.9
tint = "#ddd6ff"
seed = 9
rate = 30"##,
            varies: &[],
            check: check::<DropsSettings>,
            make: |_, t, g| Ok(vec![Box::new(Drops::new(&g.th, t))]),
        },
        Kind {
            name: "glass-frame",
            defaults: r#"image = ""
mask = ""
pillar = [0.0, 0.0]
beads = """#,
            varies: &[],
            check: check::<FrameSettings>,
            make: |_, t, g| Ok(vec![Box::new(GlassFrame::new(&g.th, t))]),
        },
    ]
}
