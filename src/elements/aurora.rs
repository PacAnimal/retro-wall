//! Aurora theme elements: a starry arctic sky, aurora curtains, snowy ranges, a lake, and water that
//! mirrors the curtains and the ranges.
//!
//! Each curtain layer is drawn once into a seamlessly wrapping strip; each frame draws it as slices
//! that drift and sway. The water reuses the same slices for its reflection.

use crate::config::Theme;
use crate::elements::{Validate, check, each, parse, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Extend, Filter, ImageSurface, LinearGradient, Matrix, Operator, SurfacePattern};
use serde::Deserialize;
use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

/// A smooth wander in about -1..1 with period 1: a weighted sum of sines of whole frequencies.
struct Periodic {
    waves: Vec<(f64, f64, f64)>,
    total: f64,
}

impl Periodic {
    fn new(rng: &mut Rng, terms: &[(i64, f64)]) -> Periodic {
        let waves: Vec<_> = terms.iter().map(|&(f, w)| (f as f64, w, rng.range(0.0, 2.0 * PI))).collect();
        let total = waves.iter().fold(0.0, |s, w| s + w.1);
        Periodic { waves, total }
    }
    fn at(&self, t: f64) -> f64 {
        self.waves.iter().fold(0.0, |s, &(f, w, p)| s + w * (2.0 * PI * f * t + p).sin()) / self.total
    }
}

/// Blurs a strip by `blur` (shrink, then stretch back), wrapping at its ends so there is no seam.
fn wrapped(s: &ImageSurface, blur: i32) -> ImageSurface {
    let (w, h) = (s.width(), s.height());
    let pad = 4 * blur;
    let wide = surface(w + 2 * pad, h);
    {
        let c = ctx(&wide);
        for x in [pad - w, pad, pad + w] {
            let _ = c.set_source_surface(s, x as f64, 0.0);
            let _ = c.paint();
        }
    }
    let soft = resampled(&resampled(&wide, 1.0 / blur as f64, Filter::Good), blur as f64, Filter::Bilinear);
    let out = surface(w, h);
    {
        let c = ctx(&out);
        c.set_operator(Operator::Source);
        let _ = c.set_source_surface(&soft, -pad as f64, 0.0);
        let _ = c.paint();
    }
    out
}

// --- the sky ---------------------------------------------------------------------------------------

/// A clear night sky: dark overhead, lighter toward the horizon, with many small stars.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcticSky {
    zenith: Colour,
    horizon: Colour,
    stars: i64,
}

impl Validate for ArcticSky {
    fn validate(&self) -> Result<(), String> {
        within("stars", self.stars as f64, 0.0, 5000.0)
    }
}

impl Part for ArcticSky {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (w, h, hz) = (g.w, g.h, g.th.horizon_y(g.h));
        let gr = LinearGradient::new(0.0, 0.0, 0.0, hz);
        let (z, o) = (self.zenith.rgb(), self.horizon.rgb());
        gr.add_color_stop_rgb(0.0, z.0, z.1, z.2);
        gr.add_color_stop_rgb(1.0, o.0, o.1, o.2);
        let _ = c.set_source(&gr);
        let _ = c.paint();
        let mut rng = Rng::new(17);
        let u = unit(w, h);
        let (white, blue) = (rgb("#ffffff"), rgb("#9fc8ff"));
        for _ in 0..((self.stars * w as i64 * h as i64) as f64 / (1920.0 * 1080.0)).round() as i64 {
            let x = rng.range(0.0, w as f64);
            let y = rng.range(0.0, hz).powf(1.1) / hz.powf(0.1);
            let bright = rng.f64().powf(3.0);
            let col = mixed(white, blue, rng.f64() * bright);
            c.set_source_rgba(col.0, col.1, col.2, 0.25 + 0.7 * bright);
            c.arc(x, y, (u / 2200.0).max(0.5) * (1.0 + 1.5 * bright), 0.0, 2.0 * PI);
            let _ = c.fill();
        }
    }
}

// --- the curtains ----------------------------------------------------------------------------------

/// The curtain strips and their slices, shared by the aurora (draws them) and the water (mirrors them).
pub struct Sky {
    colours: Vec<Rgb>,
    band: (f64, f64),
    speeds: Vec<f64>,
    alpha: Vec<f64>,
    glow: f64,
    wrap: i64,
    seed: i64,
    slices: usize,
    rise: f64,
    shimmer: f64,
    rate: f64,
    // set by size()
    geo: Option<(i32, i32)>,
    top: i32,
    h: i32,
    tw: i32,
    layers: Vec<Curtain>,
    texes: Vec<Tex>, // the strips' textures
    strips: Option<Vec<ImageSurface>>,
    // slices at time `at`, rewritten in place
    at: Option<f64>,
    data: Vec<Vec<f32>>,
    edge: Vec<(f64, f64, f64, f64)>,
}

/// The aurora's settings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuroraSettings {
    colours: [Colour; 3],
    band: [f64; 2],
    speeds: Vec<f64>,
    alpha: Vec<f64>,
    glow: f64,
    wrap: i64,
    seed: i64,
    slices: i64,
    rise: f64,
    shimmer: f64,
    rate: f64,
}

impl Validate for AuroraSettings {
    fn validate(&self) -> Result<(), String> {
        let [top, bottom] = self.band;
        if !(0.0 <= top && top < bottom && bottom <= 1.0) {
            return Err("band is [top, bottom], fractions of the picture's height, top first".into());
        }
        if bottom - top < 0.02 {
            return Err("band is at least 0.02 of the picture's height".into());
        }
        within("rise", self.rise, 0.0, 0.5)?;
        within("shimmer", self.shimmer, 0.0, 1.0)?;
        within("glow", self.glow, 0.0, 1.0)?;
        each("speeds", &self.speeds, -1000.0, 1000.0)?;
        each("alpha", &self.alpha, 0.0, 1.0)?;
        if self.speeds.is_empty() || self.speeds.len() != self.alpha.len() || self.speeds.len() > 16 {
            return Err("speeds and alpha are one each a layer, 1 to 16 layers".into());
        }
        within("wrap", self.wrap as f64, 1.0, 8.0)?;
        within("slices", self.slices as f64, 1.0, 1024.0)?;
        within("rate", self.rate, 1.0, 120.0)
    }
}

/// A layer's fixed slice data: each edge's x and its x over each wave's length, and the wave phases.
struct Curtain {
    speed: f64,
    alpha: f64,
    phase: [f64; 4],
    edges: Vec<(f64, [f64; 4])>,
}

impl Sky {
    fn new(s: AuroraSettings) -> Sky {
        Sky {
            colours: s.colours.iter().map(|c| c.rgb()).collect(),
            band: (s.band[0], s.band[1]),
            speeds: s.speeds,
            alpha: s.alpha,
            glow: s.glow,
            wrap: s.wrap,
            seed: s.seed,
            slices: s.slices as usize,
            rise: s.rise,
            shimmer: s.shimmer,
            rate: s.rate,
            geo: None,
            top: 0,
            h: 0,
            tw: 0,
            layers: vec![],
            texes: vec![],
            strips: None,
            at: None,
            data: vec![],
            edge: vec![],
        }
    }

    /// Sets the sizes and fixed slice data for g's size.
    fn size(&mut self, g: &Geo) {
        if self.geo == Some((g.w, g.h)) {
            return;
        }
        let (w, h) = (g.w as f64, g.h as i64);
        let top = (self.band.0 * h as f64).round() as i32;
        let bottom = (self.band.1 * h as f64).round() as i32;
        self.top = top;
        self.h = bottom - top;
        self.tw = (self.wrap * g.w as i64).min(4096) as i32; // older GPUs take no wider texture than 4096 or 8192
        let n = self.speeds.len().min(self.alpha.len()); // as many layers as both give
        let lengths = [0.6 * w, 0.21 * w, 0.33 * w, 0.09 * w];
        self.layers = (0..n)
            .map(|k| {
                let phase = self.seed as f64 * 1.7 + k as f64 * 2.3;
                let edges = (0..=self.slices)
                    .map(|j| {
                        let x = (g.w as i64 * j as i64) as f64 / self.slices as f64;
                        (x, [x / lengths[0], x / lengths[1], x / lengths[2], x / lengths[3]])
                    })
                    .collect();
                Curtain { speed: self.speeds[k] * h as f64 / 1080.0, alpha: self.alpha[k], phase: [phase, 2.0 * phase, 3.0 * phase, phase], edges }
            })
            .collect();
        self.data = vec![vec![0.0; 32 * self.slices]; n];
        self.edge = vec![(0.0, 0.0, 0.0, 0.0); self.slices + 1];
        self.at = None;
        self.geo = Some((g.w, g.h));
    }

    /// Curtain k's strip: curtains drawn per column at half width, stretched, blurred, plus their glow.
    fn strip(&self, k: usize) -> ImageSurface {
        let (h, w) = (self.h, self.tw);
        let alpha = self.alpha[k];
        let mut rng = Rng::new(self.seed * 13 + k as i64);
        let edge = Periodic::new(&mut rng, &[2, 3, 5, 7, 11, 17].map(|f| (f, 1.0 / f as f64)));
        let tall = Periodic::new(&mut rng, &[3, 4, 9, 14].map(|f| (f, 1.0 / f as f64)));
        let folds = Periodic::new(&mut rng, &[4, 6, 9, 13].map(|f| (f, 1.0 / (f as f64).powf(0.5))));
        let glow = Periodic::new(&mut rng, &[19, 27, 41].map(|f| (f, 1.0 / (f as f64).powf(0.6))));
        let rays = Periodic::new(&mut rng, &[151, 197, 263, 331, 409].map(|f| (f, 1.0)));
        let cols = w / 2; // drawn at half width, then stretched
        let small = surface(cols, h);
        {
            let c = ctx(&small);
            let hf = h as f64;
            for x in 0..cols {
                let t = x as f64 / cols as f64;
                let lower = hf * (0.66 + 0.24 * edge.at(t));
                let height = hf * (0.5 + 0.35 * tall.at(t));
                let drapery = (0.25 + folds.at(t)).max(0.0).powf(0.8); // 0 in the gaps between
                let strength = 1.4 * drapery * (0.7 + 0.3 * glow.at(t)) * (0.6 + 0.4 * rays.at(t));
                if strength <= 0.02 {
                    continue;
                }
                let tail = 0.09 * hf; // a fading glow under the lower edge
                let g = LinearGradient::new(0.0, lower - height, 0.0, lower + tail);
                let end = height / (height + tail);
                let stops = [
                    (0.0, 2, 0.0),
                    (0.35 * end, 2, 0.25),
                    (0.75 * end, 1, 0.6),
                    (0.96 * end, 0, 1.0),
                    (end, 0, 0.9),
                    (end + 0.25 * (1.0 - end), 0, 0.25),
                    (1.0, 0, 0.0),
                ];
                for (at, ci, a) in stops {
                    let col: Rgb = self.colours[ci];
                    g.add_color_stop_rgba(at, col.0, col.1, col.2, a * alpha * strength.min(1.0));
                }
                c.rectangle(x as f64, lower - height, 1.0, height + tail);
                let _ = c.set_source(&g);
                let _ = c.fill();
            }
        }
        let strip = surface(w, h);
        {
            let c = ctx(&strip);
            c.scale(2.0, 1.0);
            let p = SurfacePattern::create(&small);
            p.set_filter(Filter::Bilinear);
            p.set_extend(Extend::Repeat); // seamless across the wrap
            let _ = c.set_source(&p);
            let _ = c.paint();
        }
        let (soft, halo) = (wrapped(&strip, 2), wrapped(&strip, 12));
        {
            let c = ctx(&soft); // the light it casts on the air
            let _ = c.set_source_surface(&halo, 0.0, 0.0);
            let _ = c.paint_with_alpha(self.glow);
        }
        soft
    }

    /// Each layer's strip, drawn once and kept until handed to the GPU.
    fn strips(&mut self, g: &Geo) -> &Vec<ImageSurface> {
        self.size(g);
        if self.strips.is_none() {
            self.strips = Some((0..self.layers.len()).map(|k| self.strip(k)).collect());
        }
        self.strips.as_ref().unwrap()
    }

    fn top(&self, g: &Geo) -> i32 {
        let h = g.h as f64;
        (self.band.0 * h - self.rise * (self.band.1 - self.band.0) * h).round() as i32 - 2
    }

    fn state(&self, clock: f64) -> f64 {
        (clock * self.rate).round() / self.rate
    }

    /// Every layer's slices at time t, into `data`; skipped if t is unchanged.
    fn placed(&mut self, t: f64) {
        if self.at == Some(t) {
            return;
        }
        self.at = Some(t);
        let (tw, h) = (self.tw as f64, self.h as f64);
        let bottom = (self.top + self.h) as f64;
        let (rise, shimmer) = (self.rise, self.shimmer);
        let (t23, t11, t7, t31) = (t / 23.0, t / 11.0, t / 7.0, t / 3.1);
        let two_pi = 2.0 * PI;
        let edge = &mut self.edge;
        for (layer, data) in self.layers.iter().zip(self.data.iter_mut()) {
            let p = layer.phase;
            for (e, &(x, f)) in edge.iter_mut().zip(&layer.edges) {
                let tall = 1.0 + rise * (0.65 * (two_pi * (f[0] - t23) + p[0]).sin() + 0.35 * (two_pi * (f[1] + t11) + p[1]).sin());
                let bright = 1.0 - shimmer * (0.5 + 0.3 * (two_pi * (f[2] - t7) + p[2]).sin() + 0.2 * (two_pi * (f[3] + t31) + p[3]).sin());
                *e = (x, (t * layer.speed + x) / tw, bottom - h * tall, layer.alpha * bright);
            }
            for (q, w) in data.as_chunks_mut::<32>().0.iter_mut().zip(edge.windows(2)) {
                let ((x0, u0, y0, a0), (x1, u1, y1, a1)) = (w[0], w[1]);
                let (x0, u0, y0, a0, x1, u1, y1, a1, b) =
                    (x0 as f32, u0 as f32, y0 as f32, a0 as f32, x1 as f32, u1 as f32, y1 as f32, a1 as f32, bottom as f32);
                q.copy_from_slice(&[
                    u0, 0.0, a0, a0, a0, a0, x0, y0, u1, 0.0, a1, a1, a1, a1, x1, y1, u1, 1.0, a1, a1, a1, a1, x1, b, u0, 1.0, a0, a0, a0, a0, x0, b,
                ]);
            }
        }
    }

    /// Paints the curtains at time 0 with cairo, each slice cut flat at its mean top.
    fn rest(&mut self, g: &Geo, c: &Context) {
        self.strips(g);
        self.at = None;
        self.placed(0.0);
        let (tw, h) = (self.tw as f64, self.h as f64);
        for (s, data) in self.strips.as_ref().unwrap().iter().zip(&self.data) {
            for q in data.as_chunks::<32>().0 {
                let (x0, x1, bottom) = (q[6] as f64, q[14] as f64, q[23] as f64);
                let y = (q[7] as f64 + q[15] as f64) / 2.0;
                let _ = c.save();
                c.rectangle(x0, y, x1 - x0, bottom - y);
                c.clip();
                let p = SurfacePattern::create(s);
                p.set_extend(Extend::Repeat);
                let scale = h / (bottom - y); // picture to strip: stretched to the slice
                p.set_matrix(Matrix::new(1.0, 0.0, 0.0, scale, q[0] as f64 * tw - x0, -y * scale));
                let _ = c.set_source(&p);
                let _ = c.paint_with_alpha((q[5] as f64 + q[13] as f64) / 2.0);
                let _ = c.restore();
            }
        }
    }
}

/// Aurora curtains (live): layers of strips, each drifting at its own speed, rising, falling and
/// brightening with slow waves. Updated `rate` times a second.
pub struct Aurora {
    sky: Rc<RefCell<Sky>>,
    at: f64,
}

const SKY: Key<Rc<RefCell<Sky>>> = Key::new("aurora:sky");

impl Aurora {
    /// Publishes the sky for the water; the first aurora in the scene wins.
    fn share(&self, s: &mut Shared) {
        if s.get(&SKY).is_none() {
            s.put(&SKY, self.sky.clone());
        }
    }
}

impl Part for Aurora {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        self.sky.borrow().top(g)
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.sky.borrow_mut().size(g);
        self.share(s);
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let mut sky = self.sky.borrow_mut();
        sky.strips(g);
        let strips = sky.strips.take().unwrap_or_default(); // on the GPU now: only their sizes are read after
        sky.texes = strips.into_iter().map(|s| up.add(s, Kind::Tile)).collect();
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.at = self.sky.borrow().state(w.clock);
        self.at.to_bits()
    }
    fn interval(&self) -> f64 {
        1.0 / self.sky.borrow().rate
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let mut sky = self.sky.borrow_mut();
        sky.placed(self.at);
        for (&tex, data) in sky.texes.iter().zip(&sky.data) {
            gfx.draw(Some(tex), data, Blend::Premultiplied);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        self.share(s);
        self.sky.borrow_mut().rest(g, c);
    }
}

// --- the land --------------------------------------------------------------------------------------

/// Snowy mountains along the horizon in three ranges, the farthest highest and palest. Shared as
/// `RANGES` for the water to mirror.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ranges {
    seed: i64,
    far: Colour,
    near: Colour,
    snow: Colour,
    haze: Colour,
    shade: Colour,
    heights: [f64; 2],
}

impl Validate for Ranges {
    fn validate(&self) -> Result<(), String> {
        each("heights", &self.heights, 0.0, 0.5)
    }
}

/// The ranges' picture, for the water.
const RANGES: Key<ImageSurface> = Key::new("ranges");

impl Ranges {
    fn drawn(&self, g: &Geo) -> ImageSurface {
        let (w, h, u) = (g.w, g.h, unit(g.w, g.h));
        let hz = g.th.horizon_y(h);
        let [far, near] = self.heights;
        let (far_c, near_c) = (self.far.rgb(), self.near.rgb());
        let massif = |colour, rise, count, snowy, haze| Range { colour, rise, count, snowy, haze, rough: 0.11, shoulders: true, spread: (2.2, 4.2) };
        let ranges =
            [massif(far_c, far, 34, 1.0, 0.45), massif(mixed(far_c, near_c, 0.5), (far + near) / 2.0, 30, 0.8, 0.2), massif(near_c, near, 40, 0.4, 0.05)];
        let light = Light { lit: self.haze.rgb(), edge: self.snow.rgb(), shade: self.shade.rgb(), rock: near_c };
        let (x0, y0, x1, y1) = self.area(g).unwrap()[0];
        let out = supersampled(
            w,
            h,
            g.scale,
            |c| {
                peaks(c, &mut Rng::new(self.seed), w, hz, u, &ranges, &light);
            },
            Some((x0, y0 - 0.01 * u, x1, y1)),
        ); // a peak's jag past its height
        out
    }
}

impl Part for Ranges {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Land)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let (u, hz) = (unit(g.w, g.h), g.th.horizon_y(g.h));
        Some(vec![(0.0, (hz - self.heights[0].max(self.heights[1]) * u).floor() - 2.0, g.w as f64, hz.ceil() + 1.0)])
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let own = self.drawn(g);
        let _ = c.set_source_surface(&own, 0.0, 0.0);
        let _ = c.paint();
        s.put(&RANGES, own);
    }
}

/// The lake below the horizon, lighter toward the far shore. `Water` draws the reflections.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lake {
    ice: Colour,
    far: Colour,
}

impl Validate for Lake {}

impl Part for Lake {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Land)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        Some(vec![(0.0, g.th.horizon_y(g.h).floor(), g.w as f64, g.h as f64)])
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (w, h) = (g.w as f64, g.h as f64);
        let hz = g.th.horizon_y(g.h);
        let gr = LinearGradient::new(0.0, hz, 0.0, h);
        let (ice, far) = (self.ice.rgb(), self.far.rgb());
        let deep = mixed(ice, (0.0, 0.0, 0.0), 0.4);
        gr.add_color_stop_rgb(0.0, far.0, far.1, far.2);
        gr.add_color_stop_rgb(0.5, ice.0, ice.1, ice.2);
        gr.add_color_stop_rgb(1.0, deep.0, deep.1, deep.2);
        c.rectangle(0.0, hz, w, h - hz);
        let _ = c.set_source(&gr);
        let _ = c.fill();
    }
}

// --- the water -------------------------------------------------------------------------------------

/// Reflections in the water (live): the aurora's slices flipped at the horizon (strength `aurora`), and
/// over them the ranges, flipped and cut into bands that sway with waves (`reflect`, `ripple`).
/// A seamless loop of `loop` seconds at `rate` steps a second; each step's vertices are cached.
pub struct Water {
    tex: Option<Tex>,
    reflect: f64,
    ripple: f64,
    aurora: f64,
    looped: f64,
    rate: f64,
    heights: Vec<f64>, // the scene's ranges' heights
    steps: i64,
    size: (f64, f64),
    sky: Option<Rc<RefCell<Sky>>>,
    cache: Vec<Option<Vec<f32>>>,
    turned: Vec<Vec<f32>>,
    turned_at: Option<f64>,
    shown: (i64, Option<f64>),
}

const MARGIN: i32 = 24; // room for the widest sway, either side

/// The water's settings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WaterSettings {
    reflect: f64,
    ripple: f64,
    aurora: f64,
    #[serde(rename = "loop")]
    looped: f64,
    rate: f64,
}

impl Validate for WaterSettings {
    fn validate(&self) -> Result<(), String> {
        within("reflect", self.reflect, 0.0, 1.0)?;
        within("ripple", self.ripple, 0.0, 24.0)?;
        within("aurora", self.aurora, 0.0, 1.0)?;
        within("loop", self.looped, 1.0, 120.0)?;
        within("rate", self.rate, 1.0, 60.0)
    }
}

impl Water {
    fn new(th: &Theme, s: WaterSettings) -> Water {
        let heights =
            th.scene.iter().find(|e| e.kind == "ranges").and_then(|e| parse::<Ranges>(&e.settings).ok()).map_or_else(|| vec![0.17], |r| r.heights.to_vec());
        Water {
            tex: None,
            reflect: s.reflect,
            ripple: s.ripple,
            aurora: s.aurora,
            looped: s.looped,
            rate: s.rate,
            heights,
            steps: 1,
            size: (1.0, 1.0),
            sky: None,
            cache: vec![],
            turned: vec![],
            turned_at: None,
            shown: (0, None),
        }
    }

    fn depth(&self, g: &Geo) -> i32 {
        let most = self.heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (1.3 * most * unit(g.w, g.h)).ceil() as i32 + 2 // the ridges' jags too
    }

    fn sky(&self) -> Option<&Rc<RefCell<Sky>>> {
        self.sky.as_ref().filter(|_| self.aurora > 0.0)
    }

    /// The ranges flipped under the horizon, edge columns extended into a margin either side.
    fn make(&mut self, g: &Geo, s: &Shared) -> ImageSurface {
        self.steps = 1.max((self.looped * self.rate).round() as i64);
        self.cache = vec![None; self.steps as usize];
        let (hz, depth, m, w) = ((g.th.horizon_y(g.h)).round(), self.depth(g), MARGIN, g.w);
        let mirrored = surface(w + 2 * m, depth);
        if let Some(ranges) = s.get(&RANGES) {
            // row r is the ranges' row hz - r
            let p = SurfacePattern::create(ranges);
            p.set_extend(Extend::Pad); // the edge columns on into the margins
            p.set_matrix(Matrix::new(1.0, 0.0, 0.0, -1.0, -m as f64, hz));
            let fade = LinearGradient::new(0.0, 0.0, 0.0, depth as f64); // out toward the viewer: no edge where it ends
            fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 1.0);
            fade.add_color_stop_rgba(0.55, 0.0, 0.0, 0.0, 0.75);
            fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            let c = ctx(&mirrored);
            let _ = c.set_source(&p);
            let _ = c.mask(&fade);
        }
        self.size = ((w + 2 * m) as f64, depth as f64);
        mirrored
    }

    /// Rows 0..depth in bands at loop fraction f: (row, band height, sideways shift) each. The shift is
    /// a wave moving toward the viewer, longer and wider nearer: `ripple` pixels at the front, at 1080p.
    fn bands(&self, g: &Geo, depth: i32, f: f64) -> Vec<(i32, i32, i32)> {
        let (k, m) = (unit(g.w, g.h) / 1080.0, MARGIN);
        let (mut r, mut out, mut along) = (0, vec![], 0.0f64);
        while r < depth {
            let near = r as f64 / depth as f64;
            let band = if near < 0.35 { 1 } else { 2 };
            let wave = 2.0 * PI * along;
            let shift =
                (self.ripple * k * (0.2 + 0.8 * near) * (0.65 * (wave - 2.0 * PI * 2.0 * f).sin() + 0.35 * (1.7 * wave + 2.0 * PI * 3.0 * f + 1.3).sin()))
                    .round() as i32;
            out.push((r, band, shift.clamp(-m, m)));
            along += band as f64 / ((3.0 + 26.0 * near) * k);
            r += band;
        }
        out
    }

    /// The ranges' reflection bands at a loop step: (sx0, sy0, sx1, sy1, x0, y0, x1, y1) each.
    fn placed(&self, g: &Geo, step: i64) -> Vec<(f64, f64, f64, f64, f64, f64, f64, f64)> {
        let f = step as f64 / self.steps as f64;
        let (hz, m, w) = ((g.th.horizon_y(g.h)).round(), MARGIN as f64, g.w as f64);
        self.bands(g, self.depth(g), f)
            .into_iter()
            .map(|(r, b, s)| {
                let (r, b, s) = (r as f64, b as f64, s as f64);
                (m + s, r, m + s + w, r + b, 0.0, hz + r, w, hz + r + b)
            })
            .collect()
    }

    /// A step's quad vertices.
    fn vertices(&self, g: &Geo, step: i64) -> Vec<f32> {
        let (tw, th) = self.size;
        let k = self.reflect as f32;
        let placed = self.placed(g, step);
        let mut d = Vec::with_capacity(placed.len() * 32);
        for (sx0, sy0, sx1, sy1, x0, y0, x1, y1) in placed {
            let (u0, v0, u1, v1) = (sx0 / tw, sy0 / th, sx1 / tw, sy1 / th);
            for (u, v, px, py) in [(u0, v0, x0, y0), (u1, v0, x1, y0), (u1, v1, x1, y1), (u0, v1, x0, y1)] {
                d.extend_from_slice(&[u as f32, v as f32, k, k, k, k, px as f32, py as f32]);
            }
        }
        d
    }
}

impl Part for Water {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        g.th.horizon_y(g.h).floor() as i32
    }
    fn start(&mut self, _g: &Geo, _w: &mut World, s: &mut Shared) {
        self.sky = s.get(&SKY).cloned();
    }
    fn textures(&mut self, g: &Geo, s: &Shared, up: &mut Uploads) {
        self.tex = Some(up.add(self.make(g, s), Kind::Label));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let step = ((w.clock * self.rate) as i64).rem_euclid(self.steps);
        let at = self.sky().map(|s| s.borrow().state(w.clock));
        self.shown = (step, at);
        let mut h = step as u64;
        hash_f(&mut h, at.unwrap_or(f64::NAN));
        h
    }
    fn interval(&self) -> f64 {
        match self.sky() {
            Some(s) => (1.0 / self.rate).min(1.0 / s.borrow().rate),
            None => 1.0 / self.rate,
        }
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (step, at) = self.shown;
        if self.cache[step as usize].is_none() {
            self.cache[step as usize] = Some(self.vertices(g, step));
        }
        if let (Some(at), Some(sky)) = (at, self.sky().cloned()) {
            let (x, y, _, h) = gfx.rect;
            let (x0, y0, x1, y1) = (0.0, self.top(g) as f64, g.w as f64, g.h as f64);
            gfx.scissor(Some(((x0 - x as f64).round() as i32, ((y + h) as f64 - y1).round() as i32, (x1 - x0).round() as i32, (y1 - y0).round() as i32)));
            let mut sky = sky.borrow_mut();
            if self.turned_at != Some(at) {
                // the curtain slices at `at`, flipped at the horizon, scaled by `aurora`
                sky.placed(at);
                let (hz, k) = (g.th.horizon_y(g.h), self.aurora);
                self.turned.resize(sky.data.len(), vec![]);
                for (out, data) in self.turned.iter_mut().zip(&sky.data) {
                    out.clear();
                    out.extend_from_slice(data);
                    for v in out.as_chunks_mut::<8>().0 {
                        for c in &mut v[2..6] {
                            *c = (*c as f64 * k) as f32;
                        }
                        v[7] = (2.0 * hz - v[7] as f64) as f32;
                    }
                }
                self.turned_at = Some(at);
            }
            for (&tex, data) in sky.texes.iter().zip(&self.turned) {
                gfx.draw(Some(tex), data, Blend::Premultiplied);
            }
            gfx.scissor(None);
        }
        gfx.draw(self.tex, self.cache[step as usize].as_ref().unwrap(), Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let mirrored = self.make(g, s);
        let sky = s.get(&SKY).cloned().filter(|_| self.aurora > 0.0);
        let hz = g.th.horizon_y(g.h);
        let top = self.top(g) as f64;
        let _ = c.save();
        c.rectangle(0.0, top, g.w as f64, g.h as f64 - top);
        c.clip();
        if let Some(sky) = sky {
            // the curtains at time 0, flipped
            c.push_group();
            c.translate(0.0, 2.0 * hz);
            c.scale(1.0, -1.0);
            sky.borrow_mut().rest(g, c);
            let _ = c.pop_group_to_source();
            let _ = c.paint_with_alpha(self.aurora);
        }
        for (sx0, sy0, _sx1, _sy1, x0, y0, x1, y1) in self.placed(g, 0) {
            let _ = c.save();
            c.rectangle(x0, y0, x1 - x0, y1 - y0);
            c.clip();
            let _ = c.set_source_surface(&mirrored, x0 - sx0, y0 - sy0);
            let _ = c.paint_with_alpha(self.reflect);
            let _ = c.restore();
        }
        let _ = c.restore();
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "arctic-sky",
            defaults: r##"zenith = "#020610"
horizon = "#0d2236"
stars = 420"##,
            varies: &[],
            check: check::<ArcticSky>,
            make: |_, t, _g| Ok(vec![Box::new(settings::<ArcticSky>(t))]),
        },
        Kind {
            name: "aurora",
            defaults: r##"colours = ["#7dffb8", "#2fd8c0", "#8a5cff"]
band = [0.2, 0.7]
speeds = [3.0, -1.8]
alpha = [1.0, 0.6]
glow = 0.6
wrap = 2
seed = 4
slices = 32
rise = 0.16
shimmer = 0.25
rate = 20"##,
            varies: &[],
            check: check::<AuroraSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Aurora { sky: Rc::new(RefCell::new(Sky::new(settings(t)))), at: 0.0 })]),
        },
        Kind {
            name: "ranges",
            defaults: r##"seed = 2
far = "#22384e"
near = "#0c1824"
snow = "#b8d8e8"
haze = "#2c4a62"
shade = "#050d16"
heights = [0.17, 0.08]"##,
            varies: &[],
            check: check::<Ranges>,
            make: |_, t, _g| Ok(vec![Box::new(settings::<Ranges>(t))]),
        },
        Kind {
            name: "lake",
            defaults: r##"ice = "#0a1a28"
far = "#1a3346""##,
            varies: &[],
            check: check::<Lake>,
            make: |_, t, _g| Ok(vec![Box::new(settings::<Lake>(t))]),
        },
        Kind {
            name: "water",
            defaults: r#"reflect = 0.45
ripple = 9.0
aurora = 0.5
loop = 8.0
rate = 15"#,
            varies: &[],
            check: check::<WaterSettings>,
            make: |_, t, g| Ok(vec![Box::new(Water::new(&g.th, settings(t)))]),
        },
    ]
}
