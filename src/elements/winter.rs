//! Winter theme elements: a sky that follows the time of day, the sun and moon on their arcs, the
//! snowy land, falling snow blown by gusts, sun rays and drifting haze.
//!
//! The time of day comes from `day::daytime()`; the sun's hours from the theme's `day` tweak.

use crate::config::Theme;
use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Extend, Filter, ImageSurface, LinearGradient, Matrix, Mesh, MeshCorner, Operator, RadialGradient, SurfacePattern};
use serde::Deserialize;
use std::f64::consts::PI;
use toml::Table;

/// At day fraction f: (sun, moon, elevation, light, dusk).
fn at(th: &Theme, f: f64) -> (f64, f64, f64, f64, f64) {
    let [rise, set] = th.tw.day;
    crate::day::at(rise, set, f)
}

/// The time of day, rounded to the minute.
fn minute() -> f64 {
    (crate::day::daytime() * 1440.0).round() / 1440.0
}

fn fps(th: &Theme) -> f64 {
    th.tw.fps
}

/// A quad shaded from its corner colours: top left, top right, bottom right, bottom left.
fn cornered(x0: f64, y0: f64, x1: f64, y1: f64, c: [Rgb; 4]) -> [f64; 32] {
    let mut q = [0.0; 32];
    for (k, (x, y)) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into_iter().enumerate() {
        q[8 * k..8 * k + 8].copy_from_slice(&[0.0, 0.0, c[k].0, c[k].1, c[k].2, 1.0, x, y]);
    }
    q
}

fn f32s(d: impl IntoIterator<Item = f64>) -> Vec<f32> {
    d.into_iter().map(|v| v as f32).collect()
}

fn hash_all(vs: &[f64]) -> u64 {
    let mut h = 0;
    vs.iter().for_each(|&v| hash_f(&mut h, v));
    h
}

// --- the sky ------------------------------------------------------------------------------------------

/// The sky (live): starry blue at night, hazy grey by day, a glow toward the sun at dawn and dusk.
pub struct Sky {
    tex: Option<Tex>,
    stars: i64,
    night: [Colour; 3],
    day: [Colour; 3],
    dusk: [Colour; 3],
    glow: Colour,
    star_at: Vec<(f64, f64, f64, f64)>,
    cache: Option<(f64, Vec<f32>, Vec<f32>)>,
    f: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkySettings {
    stars: i64,
    night: [Colour; 3],
    day: [Colour; 3],
    dusk: [Colour; 3],
    glow: Colour,
}

impl Validate for SkySettings {
    fn validate(&self) -> Result<(), String> {
        within("stars", self.stars as f64, 0.0, 10000.0)
    }
}

const COLUMNS: i32 = 12;
const ROWS: [f64; 3] = [0.0, 0.55, 1.0];

impl Sky {
    fn new(s: SkySettings) -> Self {
        Sky { tex: None, stars: s.stars, night: s.night, day: s.day, dusk: s.dusk, glow: s.glow, star_at: vec![], cache: None, f: 0.0 }
    }

    fn make_stars(&mut self, g: &Geo) -> ImageSurface {
        let mut rng = Rng::new(41);
        let (hz, u) = (g.th.horizon_y(g.h), unit(g.w, g.h));
        let n = ((self.stars * g.w as i64) as f64 / 1920.0).round() as i64;
        self.star_at = (0..n)
            .map(|_| {
                let x = rng.range(0.0, g.w as f64);
                let y = hz * rng.f64().powf(1.6);
                let size = rng.range(0.8, 2.4) * u / 1080.0;
                (x, y, size, rng.range(0.3, 1.0))
            })
            .collect();
        self.cache = None;
        soft_dot(16, &[(0.0, 1.0), (0.25, 0.7), (1.0, 0.0)])
    }

    fn shades(&self, light: f64, dusk: f64) -> Vec<Rgb> {
        (0..3).map(|i| mixed(mixed(self.night[i].rgb(), self.day[i].rgb(), light), self.dusk[i].rgb(), dusk * 0.85)).collect()
    }

    /// The sky's quads and star sprites at time of day f.
    fn placed(&self, g: &Geo, f: f64) -> (Vec<f32>, Vec<f32>) {
        let (w, hz) = (g.w as f64, g.th.horizon_y(g.h) + 2.0);
        let (sun, _moon, _elevation, light, dusk) = at(&g.th, f);
        let shades = self.shades(light, dusk);
        let near = if 0.5 > (sun - 0.5).abs() - 0.2 {
            0.1 + 0.8 * 1.0f64.min(0.0f64.max(sun))
        } else if sun > 1.0 {
            0.0
        } else {
            1.0
        };
        let glow = self.glow.rgb();
        let mut quads = vec![];
        for c in 0..COLUMNS {
            let (x0, x1) = ((g.w * c) as f64 / COLUMNS as f64, (g.w * (c + 1)) as f64 / COLUMNS as f64);
            let shade = |x: f64, row: usize| {
                let towards = (-((x / w - near) / 0.35).powf(2.0)).exp() * dusk;
                mixed(shades[row], glow, towards * [0.0, 0.25, 0.75][row])
            };
            for row in 0..ROWS.len() - 1 {
                let (y0, y1) = (ROWS[row] * hz, ROWS[row + 1] * hz);
                quads.extend(cornered(x0, y0, x1, y1, [shade(x0, row), shade(x1, row), shade(x1, row + 1), shade(x0, row + 1)]));
            }
        }
        let stars: Vec<_> = if 1.0 - light > 0.05 {
            self.star_at.iter().map(|&(x, y, size, a)| (x, y, size, 0.0, (0.85, 0.9, 1.0), a * (1.0 - light).powf(2.0))).collect()
        } else {
            vec![]
        };
        (f32s(quads), sprite_vertices(&stars))
    }
}

impl Part for Sky {
    fn still(&self) -> bool {
        true
    }
    fn paint(&mut self, _g: &Geo, c: &Context, _s: &mut Shared) {
        let (r, gg, b) = self.night[0].rgb();
        c.set_source_rgb(r, gg, b);
        let _ = c.paint();
    }
    fn area(&self, _g: &Geo) -> Option<Vec<Rect>> {
        Some(vec![]) // so never painted in the live picture
    }
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let star = self.make_stars(g);
        self.tex = Some(up.add(star, Kind::Sprite));
    }
    fn state(&mut self, _g: &Geo, _w: &mut World) -> u64 {
        self.f = minute();
        self.f.to_bits()
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.drawn(g, gfx, w, p);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let star = self.make_stars(g);
        let (quads, stars) = self.placed(g, minute());
        for q in quads.as_chunks::<32>().0 {
            let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
            let mesh = Mesh::new();
            mesh.begin_patch();
            for k in 0..4 {
                if k == 0 { mesh.move_to(q[8 * k + 6], q[8 * k + 7]) } else { mesh.line_to(q[8 * k + 6], q[8 * k + 7]) }
            }
            for (k, corner) in [MeshCorner::MeshCorner0, MeshCorner::MeshCorner1, MeshCorner::MeshCorner2, MeshCorner::MeshCorner3].into_iter().enumerate() {
                mesh.set_corner_color_rgb(corner, q[8 * k + 2], q[8 * k + 3], q[8 * k + 4]);
            }
            mesh.end_patch();
            c.rectangle(q[6], q[7], q[14] - q[6], q[23] - q[7]);
            let _ = c.set_source(&mesh);
            let _ = c.fill();
        }
        paint_vertices(c, &star, &stars);
    }
}

impl Sky {
    fn drawn(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if self.cache.as_ref().is_none_or(|c| c.0 != self.f) {
            let (q, s) = self.placed(g, self.f);
            self.cache = Some((self.f, q, s));
        }
        let (_, quads, stars) = self.cache.as_ref().unwrap();
        gfx.draw(None, quads, Blend::Alpha);
        if !stars.is_empty() {
            gfx.draw(self.tex, stars, Blend::Premultiplied);
        }
    }
}

// --- the sun and the moon -----------------------------------------------------------------------------

/// An orb's position at fraction `along` of its arc; it peaks `peak` of the picture height up.
fn orb_where(g: &Geo, peak: f64, along: f64) -> (f64, f64) {
    (g.w as f64 * (0.12 + 0.76 * along), g.th.horizon_y(g.h) - g.h as f64 * peak * (PI * along).sin())
}

/// The orb's peak, for the beams.
const ORB: Key<f64> = Key::new("winter.orb");

/// The orb's pictures, as `pictures` gives them.
const DISC: usize = 0;
const MOON: usize = 1;
const HALO: usize = 2;

/// The sun by day and the moon by night, each on its arc across the sky.
pub struct Orb {
    texes: Vec<Tex>,
    sun: Colour,
    low: Colour,
    moon: Colour,
    size: f64,
    peak: f64,
    f: f64,
    shown: Option<f64>,
    sprites: Vec<(usize, Vec<f32>)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrbSettings {
    sun: Colour,
    low: Colour,
    moon: Colour,
    size: f64,
    peak: f64,
}

impl Validate for OrbSettings {
    fn validate(&self) -> Result<(), String> {
        within("size", self.size, 0.005, 0.5)?;
        within("peak", self.peak, 0.0, 1.0)
    }
}

impl Orb {
    fn new(s: OrbSettings) -> Self {
        Orb { texes: vec![], sun: s.sun, low: s.low, moon: s.moon, size: s.size, peak: s.peak, f: 0.0, shown: None, sprites: vec![] }
    }

    /// The disc, the moon and the halo.
    fn pictures(&self) -> [ImageSurface; 3] {
        let disc = surface(128, 128);
        {
            let c = ctx(&disc);
            c.arc(64.0, 64.0, 60.0, 0.0, 2.0 * PI);
            c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            let _ = c.fill();
        }
        let moon = surface(128, 128); // its seas, faint
        {
            let c = ctx(&moon);
            c.arc(64.0, 64.0, 60.0, 0.0, 2.0 * PI);
            c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            let _ = c.fill_preserve();
            c.clip();
            c.set_operator(Operator::DestOut);
            for (x, y, r, a) in [(48.0, 50.0, 18.0, 0.16), (78.0, 44.0, 13.0, 0.13), (70.0, 76.0, 16.0, 0.12), (44.0, 80.0, 9.0, 0.1)] {
                let g = RadialGradient::new(x, y, 0.0, x, y, r);
                g.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, a);
                g.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
                let _ = c.set_source(&g);
                let _ = c.paint();
            }
        }
        let halo = soft_dot(128, &[(0.0, 0.8), (0.12, 0.45), (0.4, 0.12), (1.0, 0.0)]);
        [disc, moon, halo]
    }

    /// (picture, sprite) pairs at time of day f; both orbs show near the turn of day.
    fn placed(&self, g: &Geo, f: f64) -> Vec<(usize, (f64, f64, f64, f64, Rgb, f64))> {
        let u = unit(g.w, g.h);
        let (sun, moon, elevation, light, dusk) = at(&g.th, f);
        let mut out = vec![];
        for (along, name, colour, strength) in
            [(sun, DISC, mixed(self.low.rgb(), self.sun.rgb(), smooth_ab(0.0, 0.35, elevation)), 0.95), (moon, MOON, self.moon.rgb(), 0.9 * (1.0 - light))]
        {
            if !(-0.08 < along && along < 1.08) || strength < 0.02 {
                continue;
            }
            let (x, y) = orb_where(g, self.peak, along);
            let size = self.size * u;
            out.push((HALO, (x, y, 9.0 * size, 0.0, colour, strength * (0.6 + 0.4 * dusk))));
            out.push((name, (x, y, size, 0.0, colour, strength)));
        }
        out
    }
}

impl Part for Orb {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn start(&mut self, _g: &Geo, _w: &mut World, s: &mut Shared) {
        s.put(&ORB, self.peak);
    }
    fn textures(&mut self, _g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.texes = self.pictures().into_iter().map(|s| up.add(s, Kind::Sprite)).collect();
    }
    fn state(&mut self, _g: &Geo, _w: &mut World) -> u64 {
        self.f = minute();
        self.f.to_bits()
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.drawn(g, gfx, w, p);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        s.put(&ORB, self.peak);
        let pictures = self.pictures();
        for (k, sprite) in self.placed(g, minute()) {
            paint_vertices(c, &pictures[k], &sprite_vertices(&[sprite]));
        }
    }
}

impl Orb {
    fn drawn(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if self.shown != Some(self.f) {
            self.sprites = self.placed(g, self.f).into_iter().map(|(k, sprite)| (k, sprite_vertices(&[sprite]))).collect();
            self.shown = Some(self.f);
        }
        for (k, data) in &self.sprites {
            gfx.draw(Some(self.texes[*k]), data, Blend::Premultiplied);
        }
    }
}

// --- the land -----------------------------------------------------------------------------------------
struct Colours {
    far: Rgb,
    near: Rgb,
    lit: Rgb,
    shade: Rgb,
    rock: Rgb,
    edge: Rgb,
    post: Rgb,
}

/// One light's colours of the land, as set.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColourSettings {
    far: Colour,
    near: Colour,
    lit: Colour,
    shade: Colour,
    rock: Colour,
    edge: Colour,
    post: Colour,
}

impl ColourSettings {
    fn rgb(&self) -> Colours {
        Colours {
            far: self.far.rgb(),
            near: self.near.rgb(),
            lit: self.lit.rgb(),
            shade: self.shade.rgb(),
            rock: self.rock.rgb(),
            edge: self.edge.rgb(),
            post: self.post.rgb(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LandSettings {
    seed: i64,
    rocks: i64,
    posts: i64,
    glow: Colour,
    by_day: ColourSettings,
    by_night: ColourSettings,
}

impl Validate for LandSettings {
    fn validate(&self) -> Result<(), String> {
        within("rocks", self.rocks as f64, 0.0, 1000.0)?;
        within("posts", self.posts as f64, 0.0, 200.0)
    }
}

/// The land's settings with `by_day` and `by_night` each the defaults with the theme's colours over them.
fn land_settings(t: &Table) -> Table {
    let d: Table = LAND_DEFAULTS.parse().unwrap();
    let mut t = t.clone();
    for k in ["by_day", "by_night"] {
        let mut whole = d[k].as_table().unwrap().clone();
        if let Some(given) = t.get(k).and_then(|g| g.as_table()) {
            whole.extend(given.clone());
        }
        t.insert(k.into(), toml::Value::Table(whole));
    }
    t
}

/// The land (live, changes with the light): mountains, snow dunes, rocks and a fence. Painted once for
/// day and once for night and crossfaded by the light, with a warm cast at dawn and dusk.
pub struct Land {
    texes: Vec<Tex>,
    seed: i64,
    rocks: i64,
    posts: i64,
    glow: Colour,
    by_day: Colours,
    by_night: Colours,
    pictures: Option<(ImageSurface, ImageSurface)>,
    shown: (f64, f64),
    area: Vec<Rect>,
}

const LAND_DEFAULTS: &str = r##"seed = 3
rocks = 46
posts = 13
glow = "#ffb07a"
[by_day]
far = "#9aa8b6"
near = "#7f8e9e"
lit = "#e8edf1"
shade = "#a6b4c4"
rock = "#3b4048"
edge = "#f6f8fa"
post = "#2e2a28"
[by_night]
far = "#1b2537"
near = "#162030"
lit = "#46546e"
shade = "#222d42"
rock = "#0a0d13"
edge = "#8a9ac0"
post = "#07080b""##;

impl Land {
    fn new(s: LandSettings) -> Self {
        Land {
            texes: vec![],
            seed: s.seed,
            rocks: s.rocks,
            posts: s.posts,
            glow: s.glow,
            by_day: s.by_day.rgb(),
            by_night: s.by_night.rgb(),
            pictures: None,
            shown: (0.0, 0.0),
            area: vec![],
        }
    }

    fn ground_top(g: &Geo) -> f64 {
        (g.th.horizon_y(g.h) - 0.12 * unit(g.w, g.h)).floor()
    }

    fn area_of(g: &Geo) -> Vec<Rect> {
        vec![(0.0, Self::ground_top(g), g.w as f64, g.h as f64)]
    }

    fn paint_land(&self, g: &Geo, cx: &Context, c: &Colours) {
        let (w, h, u) = (g.w as f64, g.h as f64, unit(g.w, g.h));
        let hz = g.th.horizon_y(g.h);
        let mut rng = Rng::new(self.seed);
        self.mountains(g, cx, c, hz, u);
        // snow dunes, far to near, lit on the crests
        let (mut y, mut band) = (hz, 0.012 * u);
        while y < h + band {
            let waves: Vec<(f64, f64, f64)> = [1.0, 0.5, 0.25]
                .iter()
                .map(|&wt| {
                    let length = rng.range(0.1, 0.7) * w;
                    (length, rng.range(0.0, 2.0 * PI), wt)
                })
                .collect();
            let amp = band * rng.range(0.15, 0.4);
            let crest = |x: f64| y + amp * (waves.iter().map(|&(length, phase, wt)| wt * (x / length * 2.0 * PI + phase).sin()).sum::<f64>()) / 1.75;
            cx.move_to(0.0, h + 4.0);
            for i in 0..97 {
                let x = (g.w * i) as f64 / 96.0;
                cx.line_to(x, crest(x));
            }
            cx.line_to(w, h + 4.0);
            cx.close_path();
            let gr = LinearGradient::new(0.0, y - amp, 0.0, y + band * 1.4);
            let a = mixed(c.lit, c.edge, 0.5);
            gr.add_color_stop_rgb(0.0, a.0, a.1, a.2);
            gr.add_color_stop_rgb(0.3, c.lit.0, c.lit.1, c.lit.2);
            let b = mixed(c.lit, c.shade, rng.range(0.3, 0.55));
            gr.add_color_stop_rgb(1.0, b.0, b.1, b.2);
            let _ = cx.set_source(&gr);
            let _ = cx.fill();
            y += band * rng.range(0.7, 1.3);
            band *= 1.22;
        }
        self.ripples(g, cx, c, &mut rng, hz, u);
        cx.push_group(); // the fence over the rocks, laid down after them
        self.fence(g, cx, c, &mut rng, hz, u);
        let Ok(fence) = cx.pop_group() else { return };
        let mut clusters: Vec<f64> = (0..((self.rocks * g.w as i64) as f64 / 1920.0 / 2.2).round() as i64).map(|_| rng.f64().powf(1.2)).collect();
        clusters.extend((0..3).map(|_| rng.range(0.7, 0.95)));
        clusters.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for d in clusters {
            let x = rng.range(-0.02, 1.02) * w;
            let y = hz + d * (h - hz) + 2.0;
            let size = (0.006 + 0.11 * d.powf(1.6)) * u;
            for _ in 0..*rng.pick(&[1, 1, 2, 3, 4]) {
                let rx = x + rng.range(-1.2, 1.2) * size;
                let ry = y + rng.range(-0.1, 0.25) * size;
                let rs = size * rng.range(0.35, 1.0);
                Self::rock(cx, c, &mut rng, rx, ry, rs);
            }
        }
        let _ = cx.set_source(&fence); // a rock must not cut its wire
        let _ = cx.paint();
    }

    fn mountains(&self, g: &Geo, cx: &Context, c: &Colours, hz: f64, u: f64) {
        let mid = mixed(c.far, c.near, 0.5);
        let light = Light { lit: c.lit, edge: c.edge, shade: c.shade, rock: c.rock };
        let ranges = [Range::new(c.far, 0.1, 32.0, 1.0, 0.55), Range::new(mid, 0.065, 26.0, 0.8, 0.3), Range::new(c.near, 0.028, 36.0, 0.0, 0.05)];
        peaks(cx, &mut Rng::new(self.seed * 31 + 5), g.w, hz, u, &ranges, &light);
    }

    /// Wind ripples in the snow: short faint strokes.
    fn ripples(&self, g: &Geo, cx: &Context, c: &Colours, rng: &mut Rng, hz: f64, u: f64) {
        let (w, h) = (g.w as f64, g.h as f64);
        for _ in 0..((900 * g.w) as f64 / 1920.0).round() as i64 {
            let d = rng.f64().powf(0.8);
            let x = rng.range(0.0, w);
            let y = hz + d * (h - hz);
            let length = (0.004 + 0.05 * d) * u * rng.range(0.4, 1.0);
            cx.move_to(x, y);
            cx.curve_to(x + length / 3.0, y - length * 0.04, x + 2.0 * length / 3.0, y + length * 0.03, x + length, y);
            cx.set_line_width(0.5f64.max(1.6 * d));
            let lit = rng.f64() < 0.5;
            let k = if lit { c.edge } else { c.shade };
            cx.set_source_rgba(k.0, k.1, k.2, rng.range(0.15, 0.4));
            let _ = cx.stroke();
        }
    }

    /// A rock at (x, y), `size` wide.
    fn rock(cx: &Context, c: &Colours, rng: &mut Rng, x: f64, y: f64, size: f64) {
        let (w, h) = (size, size * rng.range(0.3, 0.75));
        let mut pts = vec![];
        for i in 0..7 {
            let t = PI + PI * i as f64 / 6.0 + rng.range(-0.15, 0.15);
            let r = rng.range(0.6, 1.1);
            let dy = t.sin() * h * r;
            pts.push((x + t.cos() * w / 2.0 * r, y + if dy < 0.0 { dy } else { 0.0 }));
        }
        let mut peak = 0;
        for i in 1..pts.len() {
            if pts[i].1 < pts[peak].1 {
                peak = i;
            }
        }
        let outline = || {
            cx.move_to(x - w / 2.0, y);
            for &(px, py) in &pts {
                cx.line_to(px, py);
            }
            cx.line_to(x + w / 2.0, y);
            cx.close_path();
        };
        outline();
        cx.set_source_rgb(c.rock.0, c.rock.1, c.rock.2);
        let _ = cx.fill();
        cx.move_to(x - w / 2.0, y); // its lit side, up to its peak
        for &(px, py) in &pts[..peak + 1] {
            cx.line_to(px, py);
        }
        cx.line_to(pts[peak].0 - w * 0.08, y);
        cx.close_path();
        let k = mixed(c.rock, c.shade, 0.45);
        cx.set_source_rgb(k.0, k.1, k.2);
        let _ = cx.fill();
        cx.save().ok(); // snow on its top
        outline();
        cx.clip();
        cx.move_to(x - w, y - h * 3.0);
        for &(px, py) in &pts {
            cx.line_to(px, py + h * rng.range(0.1, 0.35));
        }
        cx.line_to(x + w, y - h * 3.0);
        cx.close_path();
        cx.set_source_rgb(c.lit.0, c.lit.1, c.lit.2);
        let _ = cx.fill();
        let _ = cx.restore();
        cx.move_to(x - w * 0.8, y + 1.0); // the drift at its foot
        cx.curve_to(x - w * 0.45, y - h * 0.45, x + w * 0.15, y - h * 0.2, x + w * 0.7, y + 1.0);
        cx.close_path();
        let k = mixed(c.lit, c.edge, 0.3);
        cx.set_source_rgb(k.0, k.1, k.2);
        let _ = cx.fill();
    }

    /// Fence posts from the near left to the horizon, with a sagging wire.
    fn fence(&self, g: &Geo, cx: &Context, c: &Colours, rng: &mut Rng, hz: f64, u: f64) {
        let (w, h) = (g.w as f64, g.h as f64);
        let (near, far) = ((-0.16 * w, h * 1.32), (0.47 * w, hz + 0.004 * u));
        let mut standing = vec![];
        for i in 0..self.posts {
            let t = (i as f64 / self.posts as f64).powf(0.55);
            let x = near.0 + (far.0 - near.0) * t;
            let y = near.1 + (far.1 - near.1) * t;
            let tall = (1.0 - t).powf(1.2) * 0.17 * u + 0.006 * u;
            if rng.f64() < 0.18 {
                continue;
            }
            let lean = rng.range(-0.25, 0.25);
            let top = (x + lean * tall, y - tall);
            cx.move_to(x, y);
            cx.line_to(top.0, top.1);
            cx.set_line_width(1.0f64.max(tall * 0.07));
            cx.set_source_rgb(c.post.0, c.post.1, c.post.2);
            let _ = cx.stroke();
            cx.arc(x, y, 1.5f64.max(tall * 0.18), PI, 2.0 * PI); // snow heaped at its foot
            cx.set_source_rgb(c.lit.0, c.lit.1, c.lit.2);
            let _ = cx.fill();
            standing.push((top.0, top.1 + tall * 0.2, tall));
        }
        for p in standing.windows(2) {
            let ((x0, y0, t0), (x1, y1, t1)) = (p[0], p[1]);
            cx.move_to(x0, y0);
            cx.curve_to(x0 + (x1 - x0) / 3.0, y0 + t0 * 0.15, x1 - (x1 - x0) / 3.0, y1 + t1 * 0.15, x1, y1);
            cx.set_line_width(0.6f64.max(t0 * 0.02));
            cx.set_source_rgba(c.post.0, c.post.1, c.post.2, 0.8);
            let _ = cx.stroke();
        }
    }

    /// The day and night pictures, painted once.
    fn both(&mut self, g: &Geo) -> (ImageSurface, ImageSurface) {
        if self.pictures.is_none() {
            let day = supersampled(g.w, g.h, 2, |c| self.paint_land(g, c, &self.by_day), None);
            let night = supersampled(g.w, g.h, 2, |c| self.paint_land(g, c, &self.by_night), None);
            self.pictures = Some((day, night));
        }
        self.pictures.clone().unwrap()
    }

    fn light(th: &Theme, f: f64) -> (f64, f64) {
        let (_, _, _, light, dusk) = at(th, f);
        (((light) * 1000.0).round() / 1000.0, ((dusk) * 1000.0).round() / 1000.0)
    }

    /// The warm dawn and dusk cast over the land, strongest at the horizon.
    fn cast(&self, g: &Geo, dusk: f64) -> [f64; 32] {
        let glow = self.glow.rgb();
        shade_quad(0.0, g.th.horizon_y(g.h), g.w as f64, g.h as f64, glow, glow, (0.3 * dusk, 0.05 * dusk))
    }
}

impl Part for Land {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        Self::ground_top(g) as i32
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let (day, night) = self.both(g);
        self.pictures = None; // handed over
        self.area = Self::area_of(g);
        let region = (0, Self::ground_top(g).floor() as i32, g.w, g.h);
        self.texes = vec![up.add_layer(day, region), up.add_layer(night, region)];
    }
    fn state(&mut self, g: &Geo, _w: &mut World) -> u64 {
        self.shown = Self::light(&g.th, minute());
        hash_all(&[self.shown.0, self.shown.1])
    }
    fn interval(&self) -> f64 {
        1.0
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.drawn(g, gfx, w, p);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (day, night) = self.both(g);
        let (light, dusk) = Self::light(&g.th, minute());
        for (picture, alpha) in [(&night, 1.0), (&day, light)] {
            let _ = c.save();
            c.scale(1.0, 1.0);
            let p = SurfacePattern::create(picture);
            p.set_filter(Filter::Bilinear);
            let _ = c.set_source(&p);
            let _ = c.paint_with_alpha(alpha);
            let _ = c.restore();
        }
        if dusk > 0.01 {
            let q = f32s(self.cast(g, dusk)).iter().map(|&v| v as f64).collect::<Vec<_>>();
            let gr = LinearGradient::new(0.0, q[7], 0.0, q[23]);
            gr.add_color_stop_rgba(0.0, q[2], q[3], q[4], q[5]);
            gr.add_color_stop_rgba(1.0, q[18], q[19], q[20], q[21]);
            c.rectangle(q[6], q[7], q[14] - q[6], q[23] - q[7]);
            let _ = c.set_source(&gr);
            let _ = c.fill();
        }
    }
}

impl Land {
    fn drawn(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (light, dusk) = self.shown;
        if light < 1.0 {
            gfx.layer(self.texes[1], Some(&self.area), 1.0, Blend::Premultiplied);
        }
        if light > 0.0 {
            gfx.layer(self.texes[0], Some(&self.area), light, Blend::Premultiplied);
        }
        if dusk > 0.01 {
            gfx.draw(None, &self.cast(g, dusk).map(|v| v as f32), Blend::Alpha);
        }
    }
}

// --- the wind and the snow ----------------------------------------------------------------------------

/// The wind, in pixels a second at a unit of 1080: a steady breeze, a flutter and occasional gusts.
/// `blown(t)` is the distance carried since time 0, kept up incrementally.
struct Wind {
    seed: i64,
    breeze: f64,
    gusts: f64,
    every: f64,
    flutter: Vec<(f64, f64, f64)>,
    done: f64,
    slot: i64,
}

impl Wind {
    fn new(seed: i64) -> Wind {
        let mut rng = Rng::new(seed);
        let flutter = (0..3).map(|_| (rng.range(3.0, 7.0), rng.range(0.15, 0.6), rng.range(0.0, 2.0 * PI))).collect();
        Wind { seed, breeze: 14.0, gusts: 0.6, every: 13.0, flutter, done: 0.0, slot: 0 }
    }

    /// The gust in slot k: (start, length, strength), or None.
    fn gust(&self, k: i64) -> Option<(f64, f64, f64)> {
        let s = self.seed;
        if chance(&[s, k, 1]) >= self.gusts {
            return None;
        }
        let length = 2.5 + 5.0 * chance(&[s, k, 2]);
        let start = k as f64 * self.every + (self.every - length) * chance(&[s, k, 3]);
        let strength = (90.0 + 260.0 * chance(&[s, k, 4])) * if chance(&[s, k, 5]) < 0.25 { -0.6 } else { 1.0 };
        Some((start, length, strength))
    }

    fn gusting(&self, t: f64) -> f64 {
        match self.gust((t / self.every).floor() as i64) {
            Some(g) if g.0 <= t && t < g.0 + g.1 => g.2 * (PI * (t - g.0) / g.1).sin().powf(2.0),
            _ => 0.0,
        }
    }

    fn blown(&mut self, t: f64) -> f64 {
        let k = (t / self.every).floor() as i64;
        while self.slot > k {
            self.slot -= 1;
            let g = self.gust(self.slot);
            self.done -= g.map_or(0.0, |g| g.1 * g.2 / 2.0);
        }
        while self.slot < k {
            let g = self.gust(self.slot);
            self.done += g.map_or(0.0, |g| g.1 * g.2 / 2.0);
            self.slot += 1;
        }
        let (g, mut now) = (self.gust(k), self.done);
        if let Some(g) = g
            && t > g.0
        {
            let tau = (t - g.0).min(g.1);
            now += g.2 * (tau / 2.0 - g.1 / (4.0 * PI) * (2.0 * PI * tau / g.1).sin());
        }
        let flutter: f64 = self.flutter.iter().map(|&(a, w, p)| a / w * (p.cos() - (w * t + p).cos())).sum();
        self.breeze * t + now + flutter
    }
}

const TILE: i32 = 512;

/// Falling snow (live) in `depths` sheets, far to near. Each sheet is a flake tile drawn as a strip of
/// quads down the picture, offset by wind and fall.
pub struct Snow {
    texes: Vec<Tex>,
    depths: Vec<(f64, f64, f64, f64)>,
    strips: i64,
    by_day: Colour,
    by_night: Colour,
    seed: i64,
    wind: Wind,
    flakes: Vec<Option<(f64, ImageSurface)>>,
    lit_at: Option<f64>,
    kept: Vec<Vec<f32>>,
    shown: (f64, f64),
    carried: Vec<(f64, f64, f64)>,
    edges: Vec<(f64, f64, f64)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnowSettings {
    depths: Vec<[f64; 4]>,
    strips: i64,
    by_day: Colour,
    by_night: Colour,
    seed: i64,
}

impl Validate for SnowSettings {
    fn validate(&self) -> Result<(), String> {
        within("strips", self.strips as f64, 1.0, 1000.0)?;
        if self.depths.len() > 16 {
            return Err(format!("depths has {}: at most 16", self.depths.len()));
        }
        for &[size, fall, wind, alpha] in &self.depths {
            within("a depth's flake size", size, 0.1, 5.0)?;
            within("a depth's fall", fall, 0.0, 1000.0)?;
            within("a depth's wind", wind, -5.0, 5.0)?;
            within("a depth's alpha", alpha, 0.0, 1.0)?;
        }
        Ok(())
    }
}

impl Snow {
    fn new(s: SnowSettings) -> Self {
        let depths: Vec<_> = s.depths.iter().map(|d| (d[0], d[1], d[2], d[3])).collect();
        Snow {
            texes: vec![],
            flakes: vec![None; depths.len()],
            depths,
            strips: s.strips,
            by_day: s.by_day,
            by_night: s.by_night,
            seed: s.seed,
            wind: Wind::new(s.seed),
            lit_at: None,
            kept: vec![],
            shown: (0.0, 0.0),
            carried: vec![],
            edges: vec![],
        }
    }

    fn all_flakes(&mut self, g: &Geo) -> Vec<ImageSurface> {
        self.wind = Wind::new(self.seed);
        let u = unit(g.w, g.h) / 1080.0;
        (0..self.depths.len())
            .map(|k| {
                let size = self.depths[k].0 * u;
                let key = round4(size);
                if self.flakes[k].as_ref().is_none_or(|f| f.0 != key) {
                    self.flakes[k] = Some((key, self.drawn_flakes(k, size)));
                }
                self.flakes[k].as_ref().unwrap().1.clone()
            })
            .collect()
    }

    /// A wrapping tile of soft flakes, scaled by `size`.
    fn drawn_flakes(&self, k: usize, size: f64) -> ImageSurface {
        let t = TILE as f64;
        let mut rng = Rng::new(self.seed * 100 + k as i64);
        let tile = surface(TILE, TILE);
        {
            let c = ctx(&tile);
            for _ in 0..(60.0 / size.powf(1.2)).round() as i64 {
                let x = rng.range(0.0, t);
                let y = rng.range(0.0, t);
                let r = size * rng.range(1.0, 3.2);
                let a = rng.range(0.5, 1.0);
                for dx in [-t, 0.0, t] {
                    for dy in [-t, 0.0, t] {
                        let gr = RadialGradient::new(x + dx, y + dy, 0.0, x + dx, y + dy, r);
                        gr.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, a);
                        gr.add_color_stop_rgba(0.45, 1.0, 1.0, 1.0, a * 0.7);
                        gr.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
                        let _ = c.set_source(&gr);
                        c.arc(x + dx, y + dy, r, 0.0, 2.0 * PI);
                        let _ = c.fill();
                    }
                }
            }
        }
        tile
    }

    /// (y, blown, gusting) at each strip edge at time t, lagged by the time snow takes to fall to y.
    fn carry(&mut self, g: &Geo, t: f64) {
        let u = unit(g.w, g.h) / 1080.0;
        self.carried.clear();
        for j in 0..=self.strips {
            let y = (g.h as i64 * j) as f64 / self.strips as f64;
            let lag = t - y / (420.0 * u);
            self.carried.push((y, self.wind.blown(lag), self.wind.gusting(lag)));
        }
    }

    /// Each sheet's quads at time of day f; rebuilt only when the light changes.
    fn sheets(&mut self, g: &Geo, f: f64) {
        if self.lit_at != Some(f) {
            let w = g.w as f64;
            let light = at(&g.th, f).3;
            let colour = mixed(self.by_night.rgb(), self.by_day.rgb(), light);
            let ys: Vec<f64> = (0..=self.strips).map(|j| (g.h as i64 * j) as f64 / self.strips as f64).collect();
            self.kept = self
                .depths
                .iter()
                .map(|&(_, _, _, alpha)| {
                    f32s(ys.windows(2).flat_map(|p| image_quad(0.0, p[0], w, p[1], colour, alpha * (0.75 + 0.25 * light), (0.0, 0.0, 1.0, 1.0))))
                })
                .collect();
            self.lit_at = Some(f);
        }
    }

    /// Rewrites each sheet's texture coordinates in place for time t, time of day f.
    fn placed(&mut self, g: &Geo, t: f64, f: f64) {
        let tt = TILE as f64;
        let u = unit(g.w, g.h) / 1080.0;
        self.carry(g, t);
        let across = g.w as f64 / tt;
        self.sheets(g, f);
        for (k, &(_size, fall, wind, _alpha)) in self.depths.iter().enumerate() {
            let data = &mut self.kept[k];
            self.edges.clear();
            self.edges.extend(self.carried.iter().map(|&(y, blown, gusting)| {
                let x = wind * u * blown + 7.0 * u * (y / (190.0 * u) + 0.9 * t + k as f64).sin();
                let down = fall * u * t - 0.12 * u * gusting;
                (y / tt, x.rem_euclid(tt) / tt, down.rem_euclid(tt) / tt)
            }));
            let mut i = 0;
            for e in self.edges.windows(2) {
                let ((y0, s0, d0), (y1, mut s1, mut d1)) = (e[0], e[1]);
                if (s1 - s0).abs() > 0.5 {
                    s1 += if s1 < s0 { 1.0 } else { -1.0 };
                }
                if (d1 - d0).abs() > 0.5 {
                    d1 += if d1 < d0 { 1.0 } else { -1.0 };
                }
                let (v0, v1) = (y0 - d0, y1 - d1);
                data[i] = (-s0) as f32;
                data[i + 1] = v0 as f32;
                data[i + 8] = (across - s0) as f32;
                data[i + 9] = v0 as f32;
                data[i + 16] = (across - s1) as f32;
                data[i + 17] = v1 as f32;
                data[i + 24] = (-s1) as f32;
                data[i + 25] = v1 as f32;
                i += 32;
            }
        }
    }
}

fn round4(x: f64) -> f64 {
    format!("{x:.4}").parse().unwrap()
}

impl Part for Snow {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.texes = self.all_flakes(g).into_iter().map(|s| up.add(s, Kind::Tile)).collect();
        self.flakes.iter_mut().for_each(|f| *f = None); // handed over
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        self.shown = ((w.clock * fps(&g.th)).round() / fps(&g.th), minute());
        hash_all(&[self.shown.0, self.shown.1])
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.drawn(g, gfx, w, p);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let tiles = self.all_flakes(g);
        self.placed(g, 0.0, minute());
        let tt = TILE as f64;
        for (k, data) in self.kept.iter().enumerate() {
            let tile = &tiles[k];
            for q in data.as_chunks::<32>().0 {
                let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
                let ((u0, v0, x0, y0), (u3, v3, y1)) = ((q[0], q[1], q[6], q[7]), (q[24], q[25], q[31]));
                let _ = c.save();
                c.rectangle(x0, y0, g.w as f64, y1 - y0);
                c.clip();
                let (du, dv) = ((u3 - u0) / (y1 - y0), (v3 - v0) / (y1 - y0));
                let pattern = SurfacePattern::create(tile);
                pattern.set_extend(Extend::Repeat);
                pattern.set_matrix(Matrix::new(1.0, 0.0, du * tt, dv * tt, (u0 - du * y0) * tt, (v0 - dv * y0) * tt));
                let alpha = q[5];
                if alpha > 0.0 {
                    c.set_source_rgba(q[2] / alpha, q[3] / alpha, q[4] / alpha, alpha);
                    let _ = c.mask(&pattern);
                }
                let _ = c.restore();
            }
        }
    }
}

impl Snow {
    fn drawn(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (t, f) = self.shown;
        self.placed(g, t, f);
        for (&tex, data) in self.texes.iter().zip(&self.kept) {
            gfx.draw(Some(tex), data, Blend::Premultiplied);
        }
    }
}

// --- the beams ----------------------------------------------------------------------------------------

/// Sun rays (live): faint shafts fanning out from the sun, strongest when it is low, none at night.
/// Drawn into one small image laid over the sky.
pub struct Beams {
    tex: Option<Tex>,
    image: Option<ImageSurface>, // the live overlay, redrawn in place
    beams: i64,
    length: f64,
    spread: f64,
    alpha: f64,
    colour: Colour,
    low: Colour,
    seed: i64,
    shrink: f64,
    steps: f64,
    orb: Option<f64>,
    rays: Vec<(f64, f64, f64, f64, f64)>,
    beam: Option<ImageSurface>,
    baked: Option<(f64, f64)>,
    shown: bool,
    state: (f64, f64),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeamsSettings {
    beams: i64,
    length: f64,
    spread: f64,
    alpha: f64,
    colour: Colour,
    low: Colour,
    seed: i64,
    shrink: f64,
    steps: f64,
}

impl Validate for BeamsSettings {
    fn validate(&self) -> Result<(), String> {
        within("beams", self.beams as f64, 0.0, 200.0)?;
        within("length", self.length, 0.01, 5.0)?;
        within("spread", self.spread, 0.1, 120.0)?;
        within("alpha", self.alpha, 0.0, 1.0)?;
        within("shrink", self.shrink, 1.0, 16.0)?;
        within("steps", self.steps, 0.1, 60.0)
    }
}

impl Beams {
    fn new(s: BeamsSettings) -> Self {
        Beams {
            tex: None,
            image: None,
            beams: s.beams,
            length: s.length,
            spread: s.spread,
            alpha: s.alpha,
            colour: s.colour,
            low: s.low,
            seed: s.seed,
            shrink: s.shrink,
            steps: s.steps,
            orb: None,
            rays: vec![],
            beam: None,
            baked: None,
            shown: false,
            state: (0.0, 0.0),
        }
    }

    fn bottom(g: &Geo) -> f64 {
        g.th.horizon_y(g.h).ceil() + 2.0
    }

    fn setup(&mut self, s: &Shared) {
        self.orb = s.get(&ORB).copied();
        let mut rng = Rng::new(self.seed);
        self.rays = (0..self.beams)
            .map(|_| {
                let a = rng.range(0.0, 360.0);
                let b = rng.range(0.6, 1.4);
                let c = rng.range(0.0, 2.0 * PI);
                let d = rng.range(0.02, 0.06);
                (a, b, c, d, rng.range(0.4, 1.0))
            })
            .collect();
        if self.beam.is_none() {
            self.beam = Some(Self::ray());
        }
        self.baked = None;
    }

    /// One ray: soft across, fading along its length.
    fn ray() -> ImageSurface {
        let (w, h) = (32, 128);
        let mut beam = surface(w, h);
        let stride = beam.stride() as usize;
        {
            let mut data = beam.data().unwrap();
            for y in 0..h {
                let v = (y as f64 + 0.5) / h as f64;
                let along = v.powf(0.5) * (1.0 - v).powf(1.6) / 0.32;
                for x in 0..w {
                    let a = 1.0f64.min(along) * (PI / 2.0 * ((x as f64 + 0.5) / w as f64 * 2.0 - 1.0)).cos().powf(4.0);
                    let b = (255.0 * a).round() as u8;
                    let o = y as usize * stride + 4 * x as usize;
                    data[o..o + 4].copy_from_slice(&[b; 4]);
                }
            }
        }
        beam.mark_dirty();
        beam
    }

    /// A surface for the overlay: the sky, shrunk.
    fn canvas(&self, g: &Geo) -> ImageSurface {
        surface((g.w as f64 / self.shrink).ceil() as i32, (Self::bottom(g) / self.shrink).ceil() as i32)
    }

    /// All rays at time t, time of day f, drawn over `image` (cleared first); false without the sun.
    fn overlay(&self, image: &ImageSurface, g: &Geo, t: f64, f: f64) -> bool {
        let (sun, _, elevation, light, dusk) = at(&g.th, f);
        let strength = self.alpha * light.max(0.9 * dusk) * smooth_ab(-0.04, 0.03, elevation);
        let Some(peak) = self.orb else { return false };
        if strength < 0.002 {
            return false;
        }
        let k = self.shrink;
        {
            let c = ctx(image);
            c.set_operator(Operator::Clear);
            let _ = c.paint();
            c.set_operator(Operator::Over);
            c.scale(1.0 / k, 1.0 / k);
            let (x, y) = orb_where(g, peak, sun);
            let reach = self.length * g.w.max(g.h) as f64;
            let colour = mixed(self.low.rgb(), self.colour.rgb(), smooth_ab(0.0, 0.4, elevation));
            let beam = self.beam.as_ref().unwrap();
            let (w, h) = (beam.width() as f64, beam.height() as f64);
            for &(angle, width, phase, rate, own) in &self.rays {
                let a = (angle + 4.0 * (rate * t + phase).sin()).to_radians();
                let alpha = strength * own * (0.55 + 0.45 * (rate * 1.7 * t + phase * 2.0).sin());
                let half = (self.spread * width / 2.0).to_radians();
                let (lx, ly) = (x + reach * (a + -half).cos(), y + reach * (a + -half).sin());
                let (rx, ry) = (x + reach * (a + half).cos(), y + reach * (a + half).sin());
                let _ = c.save();
                c.move_to(x, y);
                c.line_to(rx, ry);
                c.line_to(lx, ly);
                c.close_path();
                c.clip();
                c.transform(Matrix::new((rx - lx) / w, (ry - ly) / w, ((rx + lx) / 2.0 - x) / h, ((ry + ly) / 2.0 - y) / h, x, y));
                c.translate(-w / 2.0, 0.0);
                c.set_source_rgba(colour.0, colour.1, colour.2, alpha);
                let _ = c.mask_surface(beam, 0.0, 0.0);
                let _ = c.restore();
            }
        }
        true
    }
}

impl Part for Beams {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn textures(&mut self, _g: &Geo, s: &Shared, _up: &mut Uploads) {
        self.setup(s);
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.state = ((w.clock * self.steps).round() / self.steps, minute());
        hash_all(&[self.state.0, self.state.1])
    }
    fn interval(&self) -> f64 {
        1.0 / self.steps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.drawn(g, gfx, w, p);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let baked = self.baked;
        self.setup(s);
        self.baked = baked; // drawing at rest leaves the live overlay be
        let image = self.canvas(g);
        if self.overlay(&image, g, 0.0, minute()) {
            let _ = c.save();
            c.scale(g.w as f64 / image.width() as f64, Self::bottom(g) / image.height() as f64);
            let p = SurfacePattern::create(&image);
            p.set_filter(Filter::Bilinear);
            let _ = c.set_source(&p);
            let _ = c.paint();
            let _ = c.restore();
        }
    }
}

impl Beams {
    fn drawn(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if Some(self.state) != self.baked {
            let image = self.image.take().unwrap_or_else(|| self.canvas(g));
            self.shown = self.overlay(&image, g, self.state.0, self.state.1);
            if self.shown {
                match self.tex {
                    Some(t) => gfx.replace(t, &image, Kind::Smooth),
                    None => self.tex = Some(gfx.upload(&image, Kind::Smooth)),
                }
            }
            self.image = Some(image);
            self.baked = Some(self.state);
        }
        if let (true, Some(t)) = (self.shown, self.tex) {
            gfx.image(t, 0.0, 0.0, g.w as f64, Self::bottom(g), 1.0, Blend::Premultiplied);
        }
    }
}

// --- the drift ----------------------------------------------------------------------------------------

const DRIFT_TILE: (i32, i32) = (2048, 256);

/// A faint band drifting across the picture (live): high cloud or low haze. A tile of soft wisps moved
/// at `speed`, in quarter-pixel steps.
pub struct Drift {
    /// FPS, the most it changes.
    fps: f64,
    tex: Option<Tex>,
    band: (f64, f64),
    speed: f64,
    alpha: f64,
    wisps: i64,
    stretch: f64,
    by_day: Colour,
    by_night: Colour,
    seed: i64,
    picture: Option<ImageSurface>,
    shown: (f64, f64),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DriftSettings {
    band: [f64; 2],
    speed: f64,
    alpha: f64,
    wisps: i64,
    stretch: f64,
    by_day: Colour,
    by_night: Colour,
    seed: i64,
}

impl Validate for DriftSettings {
    fn validate(&self) -> Result<(), String> {
        let [top, bottom] = self.band;
        if !(0.0 <= top && top < bottom && bottom <= 1.0) {
            return Err("band is [top, bottom], fractions of the picture's height, top first".into());
        }
        if bottom - top < 0.02 {
            return Err("band is at least 0.02 of the picture's height".into());
        }
        within("speed", self.speed, -100.0, 100.0)?;
        within("alpha", self.alpha, 0.0, 1.0)?;
        within("wisps", self.wisps as f64, 0.0, 200.0)?;
        within("stretch", self.stretch, 0.1, 20.0)
    }
}

impl Drift {
    fn new(s: DriftSettings) -> Self {
        Drift {
            fps: 30.0,
            tex: None,
            band: (s.band[0], s.band[1]),
            speed: s.speed,
            alpha: s.alpha,
            wisps: s.wisps,
            stretch: s.stretch,
            by_day: s.by_day,
            by_night: s.by_night,
            seed: s.seed,
            picture: None,
            shown: (0.0, 0.0),
        }
    }

    fn rect(&self, g: &Geo) -> (f64, f64, f64, f64) {
        (0.0, (self.band.0 * g.h as f64).round(), g.w as f64, (self.band.1 * g.h as f64).round())
    }

    fn wisps_tile(&mut self) -> ImageSurface {
        if self.picture.is_none() {
            self.picture = Some(self.tile());
        }
        self.picture.clone().unwrap()
    }

    /// The wisps' tile, faded at top and bottom.
    fn tile(&self) -> ImageSurface {
        let (w, h) = (DRIFT_TILE.0 as f64, DRIFT_TILE.1 as f64);
        let mut rng = Rng::new(self.seed);
        let tile = surface(DRIFT_TILE.0, DRIFT_TILE.1);
        {
            let c = ctx(&tile);
            for _ in 0..self.wisps {
                let cx = rng.range(0.0, w);
                let cy = h * (0.3 + 0.4 * rng.f64());
                let a = rng.range(0.4, 1.0);
                let reach = rng.range(0.6, 1.6) * h;
                for _ in 0..rng.int(5, 10) {
                    let x = cx + rng.range(-reach, reach);
                    let y = cy + rng.range(-0.1, 0.1) * h;
                    let r = rng.range(0.08, 0.2) * h;
                    let stretch = self.stretch * rng.range(0.7, 1.3);
                    for dx in [-w, 0.0, w] {
                        let _ = c.save();
                        c.translate(x + dx, y);
                        c.scale(stretch, 1.0);
                        let gr = RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, r);
                        gr.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, a);
                        gr.add_color_stop_rgba(0.5, 1.0, 1.0, 1.0, a * 0.45);
                        gr.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
                        let _ = c.set_source(&gr);
                        c.arc(0.0, 0.0, r, 0.0, 2.0 * PI);
                        let _ = c.fill();
                        let _ = c.restore();
                    }
                }
            }
        }
        let fade = LinearGradient::new(0.0, 0.0, 0.0, h);
        for (at, a) in [(0.0, 0.0), (0.3, 1.0), (0.7, 1.0), (1.0, 0.0)] {
            fade.add_color_stop_rgba(at, 0.0, 0.0, 0.0, a);
        }
        let out = surface(DRIFT_TILE.0, DRIFT_TILE.1);
        {
            let c = ctx(&out);
            let _ = c.set_source_surface(&tile, 0.0, 0.0);
            let _ = c.mask(&fade);
        }
        out
    }

    fn placed(&self, g: &Geo, shift: f64, f: f64) -> [f64; 32] {
        let (x0, y0, x1, y1) = self.rect(g);
        let w = DRIFT_TILE.0 as f64;
        let scale = (y1 - y0) / DRIFT_TILE.1 as f64;
        let u0 = -shift / (w * scale);
        let light = at(&g.th, f).3;
        let colour = mixed(self.by_night.rgb(), self.by_day.rgb(), light);
        image_quad(x0, y0, x1, y1, colour, self.alpha * (0.6 + 0.4 * light), (u0, 0.0, u0 + (x1 - x0) / (w * scale), 1.0))
    }
}

impl Part for Drift {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        self.rect(g).1 as i32
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.fps = fps(&g.th);
        self.tex = Some(up.add(self.wisps_tile(), Kind::Tile));
        self.picture = None; // handed over
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        let u = unit(g.w, g.h) / 1080.0;
        self.shown = ((self.speed * u * w.clock * 4.0).round() / 4.0, minute());
        hash_all(&[self.shown.0, self.shown.1])
    }
    fn interval(&self) -> f64 {
        (1.0 / self.fps).max(0.25 / 0.01f64.max(self.speed.abs()))
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.drawn(g, gfx, w, p);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let tile = self.wisps_tile();
        let q: Vec<f64> = f32s(self.placed(g, 0.0, minute())).iter().map(|&v| v as f64).collect();
        let (x0, y0, x1, y1) = self.rect(g);
        let scale = (y1 - y0) / DRIFT_TILE.1 as f64;
        let pattern = SurfacePattern::create(&tile);
        pattern.set_extend(Extend::Repeat);
        pattern.set_matrix(Matrix::new(1.0 / scale, 0.0, 0.0, 1.0 / scale, -x0 / scale + q[0] * DRIFT_TILE.0 as f64, -y0 / scale));
        let _ = c.save();
        c.rectangle(x0, y0, x1 - x0, y1 - y0);
        c.clip();
        let alpha = q[5];
        if alpha > 0.0 {
            c.set_source_rgba(q[2] / alpha, q[3] / alpha, q[4] / alpha, alpha);
            let _ = c.mask(&pattern);
        }
        let _ = c.restore();
    }
}

impl Drift {
    fn drawn(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let q = self.placed(g, self.shown.0, self.shown.1).map(|v| v as f32);
        gfx.draw(self.tex, &q, Blend::Premultiplied);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "drift",
            defaults: r##"band = [0.08, 0.42]
speed = 3.0
alpha = 0.12
wisps = 14
stretch = 3.0
by_day = "#ffffff"
by_night = "#8a98b8"
seed = 9"##,
            varies: &[],
            check: check::<DriftSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Drift::new(settings(t)))]),
        },
        Kind {
            name: "beams",
            defaults: r##"beams = 12
length = 0.6
spread = 12.0
alpha = 0.07
colour = "#fff8ec"
low = "#ffc890"
seed = 5
shrink = 4
steps = 2"##,
            varies: &[],
            check: check::<BeamsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Beams::new(settings(t)))]),
        },
        Kind {
            name: "winter-sky",
            defaults: r##"stars = 160
night = ["#04060d", "#0c1424", "#1d2a40"]
day = ["#6f8296", "#a9b7c4", "#d6dde3"]
dusk = ["#2a3352", "#7a6a86", "#e9a27a"]
glow = "#ffb985""##,
            varies: &[],
            check: check::<SkySettings>,
            make: |_, t, _g| Ok(vec![Box::new(Sky::new(settings(t)))]),
        },
        Kind {
            name: "orb",
            defaults: r##"sun = "#fff4e2"
low = "#ffc79a"
moon = "#e4ecff"
size = 0.05
peak = 0.3"##,
            varies: &[],
            check: check::<OrbSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Orb::new(settings(t)))]),
        },
        Kind {
            name: "land",
            defaults: LAND_DEFAULTS,
            varies: &[],
            check: |t| check::<LandSettings>(&land_settings(t)),
            make: |_, t, _g| Ok(vec![Box::new(Land::new(settings(&land_settings(t))))]),
        },
        Kind {
            name: "snow",
            defaults: r##"depths = [[0.45, 22.0, 0.5, 0.55], [0.75, 38.0, 0.8, 0.75], [1.0, 62.0, 1.25, 0.9]]
strips = 24
by_day = "#ffffff"
by_night = "#9fb0d0"
seed = 7"##,
            varies: &["depths"],
            check: check::<SnowSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Snow::new(settings(t)))]),
        },
    ]
}
