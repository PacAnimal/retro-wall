//! Night city elements: sky, moon, skylines with windows that light and go dark, the waterfront,
//! waves, window reflections and aircraft-warning beacons. Seeded, so identical on every monitor and
//! run; live elements follow the scene clock.

use crate::config::Theme;
use crate::elements::{Validate, check, each, positive, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, LinearGradient, Operator, RadialGradient};
use serde::Deserialize;
use std::cell::{OnceCell, RefCell};
use std::collections::{BinaryHeap, HashMap};
use std::f64::consts::PI;
use std::rc::Rc;

const FPS: f64 = 30.0;
const WHITE: Rgb = (1.0, 1.0, 1.0);
const BLACK: Rgb = (0.0, 0.0, 0.0);

/// The street's water line, for the reflections.
const WATERLINE: Key<f64> = Key::new("waterline");
/// The quay's foot along the water, for the waves.
const SHORE: Key<Vec<(f64, f64)>> = Key::new("shore");

fn sun(th: &Theme) -> (f64, f64) {
    let [x, r] = th.tw.sun;
    (x, r)
}

/// Both ends of a range over 0.
fn positive_pair(name: &str, v: [f64; 2]) -> Result<(), String> {
    positive(name, v[0])?;
    positive(name, v[1])
}

fn mix(h: &mut u64, v: u64) {
    *h = (*h ^ v).wrapping_mul(0x100000001b3);
}

/// Hashes an optional frame number, keeping `None` distinct from any value.
fn opt(h: &mut u64, v: Option<i64>) {
    match v {
        Some(v) => {
            mix(h, 1);
            mix(h, v as u64);
        }
        None => mix(h, 0),
    }
}

// --- the sky ---------------------------------------------------------------------------------------

/// The sky over a city at night: near black overhead, glowing toward the horizon, a few faint stars.
pub struct NightSky {
    zenith: Rgb,
    middle: Rgb,
    horizon: Rgb,
    glow: f64,
    stars: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NightSkySettings {
    zenith: Colour,
    middle: Colour,
    horizon: Colour,
    glow: f64,
    stars: f64,
}

impl Validate for NightSkySettings {
    fn validate(&self) -> Result<(), String> {
        within("glow", self.glow, 0.0, 1.0)?;
        within("stars", self.stars, 0.0, 100000.0)
    }
}

impl NightSky {
    fn new(st: NightSkySettings) -> Self {
        NightSky { zenith: st.zenith.rgb(), middle: st.middle.rgb(), horizon: st.horizon.rgb(), glow: st.glow, stars: st.stars }
    }
}

impl Part for NightSky {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (w, h, hz) = (g.w as f64, g.h as f64, g.th.horizon_y(g.h));
        let gr = LinearGradient::new(0.0, 0.0, 0.0, hz);
        gr.add_color_stop_rgb(0.0, self.zenith.0, self.zenith.1, self.zenith.2);
        gr.add_color_stop_rgb(1.0 - self.glow, self.middle.0, self.middle.1, self.middle.2);
        gr.add_color_stop_rgb(1.0, self.horizon.0, self.horizon.1, self.horizon.2);
        let _ = c.set_source(&gr);
        let _ = c.paint();
        let mut rng = Rng::new(7);
        let u = unit(g.w, g.h);
        let n = (self.stars * w * h / (1920.0 * 1080.0)).round() as i64;
        let star = mixed(WHITE, self.middle, 0.2);
        for _ in 0..n {
            let x = rng.range(0.0, w);
            let y = hz * rng.range(0.0, 0.55).powf(1.4);
            let a = rng.range(0.15, 0.55) * (1.0 - y / (0.6 * hz));
            c.set_source_rgba(star.0, star.1, star.2, a);
            let r = (u / 1400.0).max(0.6) * *rng.pick(&[1.0, 1.0, 1.0, 1.6]);
            c.arc(x, y, r, 0.0, 2.0 * PI);
            let _ = c.fill();
        }
    }
}

// --- the moon --------------------------------------------------------------------------------------

/// A big low moon in a wide halo, with seas and wisps of cloud.
pub struct Moon {
    colour: Rgb,
    shade: Rgb,
    halo: f64,
    rise: f64,
    clouds: Rgb,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoonSettings {
    colour: Colour,
    shade: Colour,
    halo: f64,
    rise: f64,
    clouds: Colour,
}

impl Validate for MoonSettings {
    fn validate(&self) -> Result<(), String> {
        within("halo", self.halo, 0.0, 1.0)?;
        within("rise", self.rise, -4.0, 4.0)
    }
}

impl Moon {
    fn new(st: MoonSettings) -> Self {
        Moon { colour: st.colour.rgb(), shade: st.shade.rgb(), halo: st.halo, rise: st.rise, clouds: st.clouds.rgb() }
    }
}

impl Part for Moon {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, ctx_: &Context, _s: &mut Shared) {
        let (w, h, u) = (g.w, g.h, unit(g.w, g.h));
        let (sx, sr) = sun(&g.th);
        let (cx, r) = (sx * w as f64, sr * u);
        let cy = g.th.horizon_y(h) - self.rise * r;
        let halo = RadialGradient::new(cx, cy, r * 0.9, cx, cy, r * 3.2);
        let col = self.colour;
        for k in 0..9 {
            let t = k as f64 / 8.0;
            halo.add_color_stop_rgba(t, col.0, col.1, col.2, self.halo * (1.0 - t).powf(2.2));
        }
        let _ = ctx_.set_source(&halo);
        let _ = ctx_.paint();
        let (shade, colour) = (self.shade, self.colour);
        let disc = |c: &Context| {
            let face = RadialGradient::new(cx - 0.35 * r, cy - 0.4 * r, 0.1 * r, cx, cy, r);
            face.add_color_stop_rgb(0.0, colour.0, colour.1, colour.2);
            let m = mixed(colour, shade, 0.35);
            face.add_color_stop_rgb(0.75, m.0, m.1, m.2);
            let m = mixed(colour, shade, 0.8);
            face.add_color_stop_rgb(1.0, m.0, m.1, m.2);
            c.arc(cx, cy, r, 0.0, 2.0 * PI);
            let _ = c.set_source(&face);
            let _ = c.fill_preserve();
            let _ = c.save();
            c.clip();
            let mut rng = Rng::new(3);
            for _ in 0..9 {
                let mx = cx + rng.range(-0.6, 0.6) * r;
                let my = cy + rng.range(-0.6, 0.5) * r;
                let _ = c.save();
                c.translate(mx, my);
                let sx = rng.range(0.12, 0.3) * r;
                let sy = rng.range(0.08, 0.22) * r;
                c.scale(sx, sy);
                c.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI);
                let _ = c.restore();
                c.set_source_rgba(shade.0, shade.1, shade.2, rng.range(0.10, 0.2));
                let _ = c.fill();
            }
            let _ = c.restore();
        };
        let bbox = traced(|c| c.arc(cx, cy, r, 0.0, 2.0 * PI));
        let _ = ctx_.set_source_surface(supersampled(w, h, g.scale, disc, Some(bbox)), 0.0, 0.0);
        let _ = ctx_.paint();
        let k = 8;
        let wisps = surface((w / k).max(1), (h / k).max(1));
        {
            let wc = ctx(&wisps);
            wc.scale(1.0 / k as f64, 1.0 / k as f64);
            let mut rng = Rng::new(9);
            for i in 0..4 {
                let y = cy - (0.75 - 0.3 * i as f64) * r + rng.range(-0.08, 0.08) * r;
                let x0 = cx - rng.range(1.4, 2.6) * r;
                let x1 = cx + rng.range(1.2, 2.4) * r;
                let thick = rng.range(0.05, 0.11) * r;
                wc.move_to(x0, y);
                wc.curve_to(cx - r, y - thick, cx + r, y + thick, x1, y - 0.5 * thick);
                wc.curve_to(cx + r, y + 2.2 * thick, cx - r, y + 1.2 * thick, x0, y + 0.6 * thick);
                wc.close_path();
                let cl = self.clouds;
                wc.set_source_rgba(cl.0, cl.1, cl.2, rng.range(0.3, 0.5));
                let _ = wc.fill();
            }
        }
        let _ = ctx_.set_source_surface(resampled(&wisps, k as f64, Filter::Bilinear), 0.0, 0.0);
        let _ = ctx_.paint();
    }
}

// --- the skylines ----------------------------------------------------------------------------------

type Tier = (i64, i64, i64);

#[derive(Clone)]
pub struct Building {
    tiers: Vec<Tier>,
    crown: &'static str,
    shade: f64,
}

/// A window of a skyline: (x0, y0, x1, y1, brightness, floor, building).
type Pane = (i64, i64, i64, i64, f64, i64, usize);

/// What a skyline passes to the street, windows and beacons.
#[derive(Clone)]
pub struct SkylineData {
    surface: ImageSurface,
    windows: Vec<Pane>,
    buildings: Vec<Building>,
    haze: f64,
    tips: Vec<(f64, f64)>,
}

/// A row of buildings on the horizon, across the screen.
pub struct Skyline {
    name: String,
    seed: i64,
    heights: (f64, f64),
    breadth: (f64, f64),
    downtown: f64,
    colour: Rgb,
    haze: f64,
    fog: Rgb,
    rim: Rgb,
    window: f64,
    glass: Rgb,
    dark: f64,
    planned: OnceCell<(Vec<Building>, i64)>,
}

const FACADES: [&str; 6] = ["punched", "punched", "punched", "curtain", "ribbon", "paired"];

fn facade_k(f: &str) -> (f64, f64, f64, f64) {
    // gaps: between bays, between floors, window height inset, window width inset
    match f {
        "punched" => (0.9, 1.3, 0.8, 0.9),
        "curtain" => (0.25, 0.6, 0.15, 0.25),
        "ribbon" => (0.25, 1.6, 0.9, 0.25),
        _ => (0.5, 1.2, 0.7, 0.5),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkylineSettings {
    seed: i64,
    heights: [f64; 2],
    breadth: [f64; 2],
    downtown: f64,
    colour: Colour,
    haze: f64,
    fog: Colour,
    rim: Colour,
    window: f64,
    glass: Colour,
    dark: f64,
}

impl Validate for SkylineSettings {
    fn validate(&self) -> Result<(), String> {
        for h in self.heights {
            within("heights", h, 0.0, 1.0)?;
        }
        positive_pair("breadth", self.breadth)?;
        each("breadth", &self.breadth, 0.002, 1.0)?;
        within("downtown", self.downtown, -10.0, 10.0)?;
        within("window", self.window, 0.0001, 0.1)?;
        within("haze", self.haze, 0.0, 1.0)?;
        within("dark", self.dark, 0.0, 1.0)
    }
}

impl Skyline {
    fn new(name: &str, st: SkylineSettings) -> Self {
        Skyline {
            name: name.to_string(),
            seed: st.seed,
            heights: (st.heights[0], st.heights[1]),
            breadth: (st.breadth[0], st.breadth[1]),
            downtown: st.downtown,
            colour: st.colour.rgb(),
            haze: st.haze,
            fog: st.fog.rgb(),
            rim: st.rim.rgb(),
            window: st.window,
            glass: st.glass.rgb(),
            dark: st.dark,
            planned: OnceCell::new(),
        }
    }

    fn plan(&self, g: &Geo) -> &(Vec<Building>, i64) {
        self.planned.get_or_init(|| {
            let (ww, u) = (g.w as f64, unit(g.w, g.h));
            let mut rng = Rng::new(self.seed);
            let hz = g.th.horizon_y(g.h);
            let mut buildings = vec![];
            let mut x = -rng.range(0.0, self.breadth.1 * u);
            while x < ww {
                let w = rng.range(self.breadth.0, self.breadth.1) * u;
                let near = (-((x + w / 2.0) / ww - self.downtown).powf(2.0) / 0.045).exp();
                let mut h = (self.heights.0 + (self.heights.1 - self.heights.0) * (0.25 + 0.75 * near) * rng.f64().powf(0.8)) * u;
                if rng.f64() < 0.08 {
                    h *= 1.35;
                }
                let (x0, x1, top) = (x.round() as i64, (x + w).round() as i64, (hz - h).round() as i64);
                let mut tiers = vec![(x0, x1, top)];
                let n = *rng.pick(&[0, 0, 1, 1, 2]);
                for _ in 0..n {
                    let (a, b, t) = *tiers.last().unwrap();
                    let inset = rng.range(0.1, 0.22) * (b - a) as f64;
                    if (b - a) as f64 - 2.0 * inset < 0.012 * u {
                        break;
                    }
                    let na = (a as f64 + inset).round() as i64;
                    let nb = (b as f64 - inset).round() as i64;
                    let nt = (t as f64 - rng.range(0.04, 0.12) * h).round() as i64;
                    tiers.push((na, nb, nt));
                }
                let last = *tiers.last().unwrap();
                let slender = ((last.1 - last.0) as f64) < 0.6 * (hz - last.2 as f64);
                let crown = if slender {
                    *rng.pick(&["flat", "flat", "mast", "mast", "slant", "tank", "box", "spire", "spire"])
                } else {
                    *rng.pick(&["flat", "flat", "mast", "mast", "slant", "tank", "box"])
                };
                let shade = rng.f64();
                buildings.push(Building { tiers, crown, shade });
                x += w * rng.range(0.82, 1.0);
            }
            let top = buildings.iter().flat_map(|b| b.tiers.iter().map(|t| t.2)).min().unwrap_or(hz as i64) - (0.06 * u).round() as i64;
            (buildings, top)
        })
    }

    /// A building's windows, its glass drawn unlit: [(x0, y0, x1, y1, brightness, floor)].
    fn glazing(&self, g: &Geo, c: &Context, tiers: &[Tier], rng: &mut Rng, later: &[Tier], face: Rgb) -> Vec<(i64, i64, i64, i64, f64, i64)> {
        let (u, hz) = (unit(g.w, g.h), g.th.horizon_y(g.h));
        let size = ((self.window * u).round() as i64).max(1);
        let facade = *rng.pick(&FACADES);
        let (kb, kf, kh, kw) = facade_k(facade);
        let s = size as f64;
        let bay = size + ((s * kb).round() as i64).max(1);
        let floor = size + ((s * kf).round() as i64).max(1);
        let height = floor - ((s * kh).round() as i64).max(1);
        let width = bay - ((s * kw).round() as i64).max(1);
        let lobby = ((1.6 * floor as f64).round() as i64).max(2);
        let parapet = ((0.6 * floor as f64).round() as i64).max(1);
        let (a0, b0, _) = tiers[0];
        let centre = (a0 + b0) as f64 / 2.0;
        let glass = mixed(mixed(self.glass, self.fog, self.haze), face, 0.0);
        let dark = rng.f64() < self.dark;
        let bright = if rng.f64() < 0.35 { rng.range(0.76, 0.84) } else { 1.0 };
        let (mut out, mut unlit) = (vec![], vec![]);
        let (mut k, mut y1) = (0i64, (hz - lobby as f64).round() as i64);
        loop {
            let y0 = y1 - height;
            let here: Vec<&Tier> = tiers.iter().filter(|t| t.2 + parapet <= y0).collect();
            if here.is_empty() {
                break;
            }
            let mut best = here[0];
            for t in &here[1..] {
                if t.1 - t.0 < best.1 - best.0 {
                    best = t;
                }
            }
            let (a, b, _) = *best;
            let margin = ((0.5 * bay as f64).round() as i64).max(1);
            let first = (((a + margin) as f64 - centre) / bay as f64 - 0.5).ceil() as i64;
            let last = (((b - margin) as f64 - centre) / bay as f64 - 0.5).floor() as i64;
            for cc in first..=last {
                if facade == "paired" && cc.rem_euclid(3) == 2 {
                    continue;
                }
                let x0 = (centre + (cc as f64 + 0.5) * bay as f64 - width as f64 / 2.0).round() as i64;
                let x1 = x0 + width;
                if later.iter().any(|&(ta, tb, tt)| ta < x1 && x0 < tb && tt < y1) {
                    continue;
                }
                unlit.push((x0, y0, x1, y1));
                if !dark {
                    out.push((x0, y0, x1, y1, bright * (0.96 + 0.08 * rng.f64()), k));
                }
            }
            k += 1;
            y1 -= floor;
        }
        c.set_source_rgb(glass.0, glass.1, glass.2);
        for &(x0, y0, x1, y1) in &unlit {
            c.rectangle(x0 as f64, y0 as f64, (x1 - x0) as f64, (y1 - y0) as f64);
        }
        let _ = c.fill();
        out
    }
}

impl Part for Skyline {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::City)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let top = self.plan(g).1;
        Some(vec![(0.0, (top - 4).max(0) as f64, g.w as f64, g.th.horizon_y(g.h).ceil() + 2.0)])
    }
    fn paint(&mut self, g: &Geo, layer: &Context, s: &mut Shared) {
        let (ww, u) = (g.w as f64, unit(g.w, g.h));
        let hz = g.th.horizon_y(g.h);
        let (buildings, self_top) = self.plan(g).clone();
        let own = surface(g.w, g.h);
        let c = ctx(&own);
        let body = mixed(self.colour, self.fog, self.haze);
        let rim = self.rim;
        let mut rng = Rng::new(self.seed * 31 + 5);
        let (mut windows, mut tips): (Vec<Pane>, Vec<(f64, f64)>) = (vec![], vec![]);
        let sun_x = sun(&g.th).0 * ww;
        for (n, bld) in buildings.iter().enumerate() {
            let k = 0.85 + 0.3 * bld.shade;
            let face = (body.0 * k, body.1 * k, body.2 * k);
            c.set_source_rgb(face.0, face.1, face.2);
            for &(a, b, t) in &bld.tiers {
                c.rectangle(a as f64, t as f64, (b - a) as f64, hz - t as f64 + 1.0);
            }
            let _ = c.fill();
            let (a, b, t) = *bld.tiers.last().unwrap();
            let (af, bf, tf) = (a as f64, b as f64, t as f64);
            let (mid, span) = ((a + b) as f64 / 2.0, (b - a) as f64);
            c.set_line_width((u / 900.0).max(1.0));
            match bld.crown {
                "mast" => {
                    let tip = tf - rng.range(0.03, 0.07) * u;
                    c.move_to(mid, tf);
                    c.line_to(mid, tip);
                    let _ = c.stroke();
                    tips.push((mid, tip));
                }
                "spire" => {
                    c.move_to(af + 0.3 * span, tf);
                    c.line_to(mid, tf - rng.range(0.5, 0.9) * span);
                    c.line_to(bf - 0.3 * span, tf);
                    let _ = c.fill();
                    let tip = tf - rng.range(1.0, 1.6) * span;
                    c.move_to(mid, tf - 0.5 * span);
                    c.line_to(mid, tip);
                    let _ = c.stroke();
                    tips.push((mid, tip));
                }
                "slant" => {
                    c.move_to(af, tf);
                    c.line_to(bf, tf - rng.range(0.25, 0.5) * span);
                    c.line_to(bf, tf);
                    let _ = c.fill();
                }
                "tank" => {
                    let tw = 0.25 * span;
                    let tx = rng.range(af + 2.0, bf - tw - 2.0);
                    c.rectangle(tx, tf - 0.6 * tw, tw, 0.45 * tw);
                    c.rectangle(tx + 0.15 * tw, tf - 0.15 * tw, 0.1 * tw, 0.15 * tw);
                    c.rectangle(tx + 0.75 * tw, tf - 0.15 * tw, 0.1 * tw, 0.15 * tw);
                    let _ = c.fill();
                }
                "box" => {
                    c.rectangle(af + 0.15 * span, tf - 0.08 * span, 0.35 * span, 0.08 * span);
                    let _ = c.fill();
                }
                _ => {}
            }
            // pilasters
            let step = ((rng.range(0.004, 0.009) * u).round() as i64).max(3);
            c.set_source_rgba(rim.0, rim.1, rim.2, 0.035 + 0.03 * (1.0 - self.haze));
            for &(ta, tb, tt) in &bld.tiers {
                let mut px = ta + step;
                while px < tb - 1 {
                    c.rectangle(px as f64, (tt + 2) as f64, 1.0, hz - tt as f64 - 2.0);
                    px += step;
                }
            }
            let _ = c.fill();
            // the edge the moon is on
            let side = if (a + b) as f64 / 2.0 > sun_x { a } else { b - 1 };
            c.set_source_rgba(rim.0, rim.1, rim.2, 0.10 + 0.12 * (1.0 - self.haze));
            for &(ta, tb, tt) in &bld.tiers {
                c.rectangle(if side == a { ta } else { tb - 1 } as f64, tt as f64, 1.0, hz - tt as f64);
                c.rectangle(ta as f64, tt as f64, (tb - ta) as f64, 1.0);
            }
            let _ = c.fill();
            let later: Vec<Tier> = buildings[n + 1..(n + 4).min(buildings.len())].iter().flat_map(|b| b.tiers.iter().cloned()).collect();
            for w in self.glazing(g, &c, &bld.tiers, &mut rng, &later, face) {
                windows.push((w.0, w.1, w.2, w.3, w.4, w.5, n));
            }
        }
        let fog = LinearGradient::new(0.0, self_top as f64, 0.0, hz);
        let f = self.fog;
        fog.add_color_stop_rgba(0.0, f.0, f.1, f.2, 0.0);
        fog.add_color_stop_rgba(0.55, f.0, f.1, f.2, 0.05 + 0.25 * self.haze);
        fog.add_color_stop_rgba(1.0, f.0, f.1, f.2, 0.45 + 0.3 * self.haze);
        c.set_operator(Operator::Atop);
        let _ = c.set_source(&fog);
        let _ = c.paint();
        c.set_operator(Operator::Over);
        drop(c);
        s.put_named(&self.name, SkylineData { surface: own.clone(), windows, buildings, haze: self.haze, tips });
        let _ = layer.set_source_surface(&own, 0.0, 0.0);
        let _ = layer.paint();
    }
}

// --- the lights ------------------------------------------------------------------------------------

/// Windows still fading, in draw order (lower index underneath where reflections overlap).
#[derive(Default)]
pub struct Fading(std::collections::BTreeSet<usize>);

impl Fading {
    fn new() -> Self {
        Fading::default()
    }
    fn add(&mut self, k: usize) {
        self.0.insert(k);
    }
}

#[derive(Clone, Copy, PartialEq)]
struct When(f64, usize);
impl Eq for When {}
impl PartialOrd for When {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for When {
    // Reversed so BinaryHeap pops the earliest (when, window) first.
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// Which of a skyline's windows are lit at clock c.
pub struct Lights {
    period: Vec<f64>,
    phase: Vec<f64>,
    level: Vec<f64>,
    dims: Vec<bool>,
    storeys: Vec<Vec<usize>>,
    floor: Vec<usize>,
    houses: Vec<Vec<usize>>,
    house: Vec<usize>,
    wanted: Vec<bool>,
    seed: i64,
    c: Option<f64>,
    version: u64,
    lit: Vec<bool>,
    own: Vec<(bool, bool)>,
    changed: Vec<f64>,
    dimmed: f64,
    heap: BinaryHeap<When>,
    log: Vec<(u64, usize)>,
    since_full: u64,
}

const DIMMING: f64 = 0.35;
const DIM: f64 = 1.0;
const FILL: f64 = 0.75;
const MOST: f64 = 0.8;

/// A window of the windows' list: (x0, y0, x1, y1, colour, (row, building), floor, shown, mirrored).
#[derive(Clone)]
pub struct Win {
    x0: i64,
    y0: i64,
    y1: i64,
    colour: Rgb,
    building: (usize, usize),
    storey: i64,
    mirrored: Option<(i64, i64)>,
}

impl Lights {
    fn new(windows: &[Win], lit: f64, period: (f64, f64), seed: i64) -> Lights {
        let n = windows.len();
        let mut rng = Rng::new(seed);
        let periods: Vec<f64> = (0..n).map(|_| rng.range(period.0, period.1)).collect();
        let phase: Vec<f64> = (0..n).map(|_| rng.f64()).collect();
        let mut levels: HashMap<(usize, usize), f64> = HashMap::new();
        let mut dimming: HashMap<(usize, usize), bool> = HashMap::new();
        let mut floors: HashMap<(usize, usize, i64), f64> = HashMap::new();
        let (mut level, mut dims) = (vec![], vec![]);
        let mut storeys: Vec<((usize, usize, i64), Vec<usize>)> = vec![];
        let mut storey_at: HashMap<(usize, usize, i64), usize> = HashMap::new();
        let mut houses: Vec<Vec<usize>> = vec![];
        let mut house_at: HashMap<(usize, usize), usize> = HashMap::new();
        for (i, w) in windows.iter().enumerate() {
            let building = w.building;
            let floor = (building.0, building.1, w.storey);
            let si = *storey_at.entry(floor).or_insert_with(|| {
                storeys.push((floor, vec![]));
                storeys.len() - 1
            });
            storeys[si].1.push(i);
            let hi = *house_at.entry(building).or_insert_with(|| {
                houses.push(vec![]);
                houses.len() - 1
            });
            houses[hi].push(i);
            if let std::collections::hash_map::Entry::Vacant(e) = levels.entry(building) {
                e.insert(lit * rng.range(0.2, 1.8));
                dimming.insert(building, rng.f64() < DIMMING);
            }
            floors.entry(floor).or_insert_with(|| *rng.pick(&[0.15, 0.5, 1.0, 1.0, 1.4, 2.0]));
            dims.push(dimming[&building]);
            #[allow(clippy::manual_clamp)] // max then min: a NaN is 0, where clamp would keep it
            level.push((levels[&building] * floors[&floor]).max(0.0).min(0.95));
        }
        let storeys: Vec<Vec<usize>> = storeys
            .into_iter()
            .map(|(_, mut ws)| {
                ws.sort_by_key(|&i| windows[i].x0);
                ws
            })
            .collect();
        let mut floor = vec![0; n];
        for (k, ws) in storeys.iter().enumerate() {
            for &i in ws {
                floor[i] = k;
            }
        }
        let houses: Vec<Vec<usize>> = houses
            .into_iter()
            .map(|mut ws| {
                ws.sort_by(|&a, &b| chance(&[seed + 2, a as i64]).total_cmp(&chance(&[seed + 2, b as i64])));
                ws
            })
            .collect();
        let mut house = vec![0; n];
        for (b, ws) in houses.iter().enumerate() {
            for &i in ws {
                house[i] = b;
            }
        }
        Lights {
            period: periods,
            phase,
            level,
            dims,
            storeys,
            floor,
            houses,
            house,
            wanted: vec![false; n],
            seed,
            c: None,
            version: 0,
            lit: vec![false; n],
            own: vec![(false, false); n],
            changed: vec![f64::NEG_INFINITY; n],
            dimmed: f64::NEG_INFINITY,
            heap: BinaryHeap::new(),
            log: vec![],
            since_full: 0,
        }
    }

    fn on(&self, i: usize, c: f64) -> (bool, bool) {
        let count = (c / self.period[i] + self.phase[i]).floor() as i64;
        (chance(&[self.seed, i as i64, count]) < self.level[i], chance(&[self.seed + 1, i as i64, count]) < FILL)
    }

    /// Storey k's windows wanted lit: their own lights, and dark ones filled in between lit ones.
    /// Whether any changed.
    fn fill(&mut self, k: usize) -> bool {
        let ws = &self.storeys[k];
        let dark = ws.iter().filter(|&&i| !self.own[i].0).count();
        let mut moved = false;
        for (j, &i) in ws.iter().enumerate() {
            let (mut lit, fill) = self.own[i];
            if !lit && fill {
                lit = (ws.len() >= 3 && dark == 1) || (0 < j && j < ws.len() - 1 && self.own[ws[j - 1]].0 && self.own[ws[j + 1]].0);
            }
            moved |= lit != self.wanted[i];
            self.wanted[i] = lit;
        }
        moved
    }

    /// Building b's windows lit: those wanted, up to MOST of them. At a clock `when`, changes are logged.
    fn cap(&mut self, b: usize, when: Option<f64>) {
        let ws = &self.houses[b];
        let mut room = (MOST * ws.len() as f64).floor() as i64;
        for &i in ws {
            let lit = self.wanted[i] && room > 0;
            room -= lit as i64;
            if let Some(when) = when.filter(|_| lit != self.lit[i]) {
                self.version += 1;
                self.log.push((self.version, i));
                if self.dims[i] {
                    self.changed[i] = when;
                    self.dimmed = self.dimmed.max(when);
                }
            }
            self.lit[i] = lit;
        }
    }

    fn brightness(&self, i: usize, c: f64) -> f64 {
        if !self.dims[i] {
            return if self.lit[i] { 1.0 } else { 0.0 };
        }
        let p = ((c - self.changed[i]) / DIM).min(1.0);
        if self.lit[i] { p } else { 1.0 - p }
    }

    fn dimming(&self, c: f64) -> bool {
        c - self.dimmed < DIM
    }

    fn at(&mut self, c: f64) -> u64 {
        let n = self.lit.len();
        if self.c.is_none() || c < self.c.unwrap() {
            for i in 0..n {
                self.own[i] = self.on(i, c);
            }
            for k in 0..self.storeys.len() {
                self.fill(k);
            }
            for b in 0..self.houses.len() {
                self.cap(b, None);
            }
            self.changed.fill(f64::NEG_INFINITY);
            self.dimmed = f64::NEG_INFINITY;
            self.heap.clear();
            for i in 0..n {
                let (p, f) = (self.period[i], self.phase[i]);
                self.heap.push(When((((c / p + f).floor() + 1.0) - f) * p, i));
            }
            self.version += 1;
            self.log.clear();
            self.since_full = self.version;
        } else {
            while let Some(&When(when, i)) = self.heap.peek() {
                if when > c {
                    break;
                }
                self.heap.pop();
                let own = self.on(i, when + 1e-6);
                if own != self.own[i] {
                    self.own[i] = own;
                    if self.fill(self.floor[i]) {
                        self.cap(self.house[i], Some(when));
                    }
                }
                self.heap.push(When(when + self.period[i], i));
            }
            if self.log.len() > 4096 {
                let cut = self.log.len() - 2048;
                self.since_full = self.log[cut].0 - 1;
                self.log.drain(..cut);
            }
        }
        self.c = Some(c);
        self.version
    }

    /// The changes (version, window) after `version`, or None when they are no longer all logged.
    fn since(&self, version: Option<u64>) -> Option<&[(u64, usize)]> {
        let v = version?;
        if v < self.since_full {
            return None;
        }
        let from = self.log.partition_point(|e| e.0 <= v);
        Some(&self.log[from..])
    }
}

/// Vertex array of only the lit windows' rectangles, updated as lights change.
pub struct Lit {
    built: Option<u64>,
    premultiplied: bool,
    quads: Vec<Option<[f32; 32]>>,
    data: Vec<f32>,
    at: HashMap<usize, usize>,
    order: Vec<usize>,
    fading: Fading,
}

impl Lit {
    fn new(quads: &[Option<[f64; 32]>], premultiplied: bool) -> Lit {
        let quads = quads.iter().map(|q| q.map(|q| q.map(|v| v as f32))).collect();
        Lit { built: None, premultiplied, quads, data: vec![], at: HashMap::new(), order: vec![], fading: Fading::new() }
    }

    fn put(&mut self, i: usize, level: f64) {
        if level > 0.0 && !self.at.contains_key(&i) {
            self.at.insert(i, self.order.len());
            self.order.push(i);
            self.data.extend_from_slice(&self.quads[i].unwrap());
        } else if level <= 0.0 && self.at.contains_key(&i) {
            let place = self.at.remove(&i).unwrap();
            let last = self.order.pop().unwrap();
            let n = self.data.len();
            if last != i {
                self.data.copy_within(n - 32.., place * 32);
                self.order[place] = last;
                self.at.insert(last, place);
            }
            self.data.truncate(n - 32);
            return;
        }
        if level > 0.0 {
            let q = self.quads[i].unwrap();
            let k = self.at[&i] * 32;
            for corner in (0..32).step_by(8) {
                if self.premultiplied {
                    for j in 2..6 {
                        self.data[k + corner + j] = (q[corner + j] as f64 * level) as f32;
                    }
                } else {
                    self.data[k + corner + 5] = (q[corner + 5] as f64 * level) as f32;
                }
            }
        }
    }

    /// Window i's light changed.
    fn changed(&mut self, lights: &Lights, i: usize) {
        if self.quads.get(i).is_some_and(|q| q.is_some()) {
            if lights.dims[i] {
                self.fading.add(i);
            } else {
                self.put(i, if lights.lit[i] { 1.0 } else { 0.0 });
            }
        }
    }

    fn current(&mut self, lights: &Lights, c: f64) -> &[f32] {
        match lights.since(self.built) {
            None => (0..self.quads.len()).for_each(|i| self.changed(lights, i)),
            Some(log) => log.iter().for_each(|&(_, i)| self.changed(lights, i)),
        }
        let mut fading = std::mem::take(&mut self.fading);
        fading.0.retain(|&i| {
            let level = lights.brightness(i, c);
            self.put(i, level);
            level != 0.0 && level != 1.0
        });
        self.fading = fading;
        self.built = Some(lights.version);
        &self.data
    }
}

/// A Windows element's windows, built once and shared with the reflections.
pub struct Bundle {
    windows: Vec<Win>,
    quads: Vec<Option<[f64; 32]>>,
    lights: RefCell<Lights>,
}

/// For each column, the top of the highest building of `skylines` standing in it (or `floor`).
fn heights(skylines: &[&SkylineData], width: i64, floor: i64) -> Vec<i64> {
    let mut top = vec![floor; width as usize];
    for data in skylines {
        for b in &data.buildings {
            for &(a, bb, t) in &b.tiers {
                for x in a.max(0)..bb.min(width) {
                    if t < top[x as usize] {
                        top[x as usize] = t;
                    }
                }
            }
        }
    }
    top
}

/// The widest run of columns x0..x1 at height y not hidden by nearer buildings.
fn shown(x0: i64, x1: i64, y: i64, top: &[i64]) -> Option<(i64, i64)> {
    let len = top.len() as i64;
    let (mut best, mut start): (Option<(i64, i64)>, Option<i64>) = (None, None);
    for x in x0.max(0)..len.min(x1) + 1 {
        if x < len.min(x1) && top[x as usize] > y {
            if start.is_none() {
                start = Some(x);
            }
        } else if let Some(s) = start {
            if best.is_none_or(|b| x - s > b.1 - b.0) {
                best = Some((s, x));
            }
            start = None;
        }
    }
    best
}

/// Windows, rectangles and lights of the Windows element `name`, built on first use, then shared.
fn bundle(name: &str, g: &Geo, s: &mut Shared) -> Option<Rc<Bundle>> {
    if let Some(b) = s.get_named::<Rc<Bundle>>(name) {
        return Some(b.clone());
    }
    let e = g.th.scene.iter().find(|e| e.name == name && e.kind == "windows")?;
    let st: WindowsSettings = settings(&e.settings);
    let colours: Vec<Rgb> = st.colours.iter().map(|c| c.rgb()).collect();
    let alpha0 = st.alpha;
    let layers: Vec<&SkylineData> = st.skylines.iter().filter_map(|n| s.get_named::<SkylineData>(n)).collect();
    let (w, floor) = (g.w as i64, g.th.horizon_y(g.h).ceil() as i64 + 1);
    let mut rng = Rng::new(layers.iter().map(|d| d.windows.len() as i64).sum());
    let weights: Vec<f64> = (0..colours.len()).map(|k| 0.5f64.powi(k as i32)).collect();
    let (mut windows, mut quads) = (vec![], vec![]);
    let mut lamps: HashMap<(usize, usize, i64), Rgb> = HashMap::new();
    for (k, data) in layers.iter().enumerate() {
        let top = heights(&layers[k + 1..], w, floor);
        let alpha = alpha0 * (1.0 - 0.45 * data.haze);
        for &(x0, y0, x1, y1, b, storey, building) in &data.windows {
            let sk = (k, building, storey);
            lamps.entry(sk).or_insert_with(|| colours[rng.weighted(&weights)]);
            let colour = if rng.f64() < 0.88 { lamps[&sk] } else { colours[rng.weighted(&weights)] };
            let whole = shown(x0, x1, y1 - 1, &top) == Some((x0, x1));
            let mirrored = shown(x0, x1, y0, &top) == Some((x0, x1));
            windows.push(Win { x0, y0, y1, colour, building: (k, building), storey, mirrored: mirrored.then_some((x0, x1)) });
            quads.push(whole.then(|| fill_quad(x0 as f64, y0 as f64, x1 as f64, y1 as f64, colour, alpha * b)));
        }
    }
    let seed: i64 = name.chars().map(|c| c as i64).sum();
    let lights = Lights::new(&windows, st.lit, (st.period[0], st.period[1]), seed);
    let b = Rc::new(Bundle { windows, quads, lights: RefCell::new(lights) });
    s.put_named(name, b.clone());
    Some(b)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsSettings {
    skylines: Vec<String>,
    lit: f64,
    period: [f64; 2],
    colours: Vec<Colour>,
    alpha: f64,
}

impl Validate for WindowsSettings {
    fn validate(&self) -> Result<(), String> {
        within("lit", self.lit, 0.0, 1.0)?;
        positive_pair("period", self.period)?;
        each("period", &self.period, 1.0, 86400.0)?;
        if self.colours.is_empty() {
            return Err("colours is empty: it needs at least one".into());
        }
        within("alpha", self.alpha, 0.0, 1.0)
    }
}

/// The skylines' windows lighting up and going dark (live), in one draw call.
pub struct Windows {
    name: String,
    bundle: Option<Rc<Bundle>>,
    glass: Option<Lit>,
}

impl Part for Windows {
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.bundle = bundle(&self.name, g, s);
    }
    fn top(&self, g: &Geo) -> i32 {
        self.bundle.as_ref().and_then(|b| b.windows.iter().map(|w| w.y0).min()).map_or(g.h, |y| y as i32)
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let b = self.bundle.as_ref().unwrap();
        let mut l = b.lights.borrow_mut();
        let c = w.clock;
        let mut h = 0xcbf2_9ce4_8422_2325;
        mix(&mut h, l.at(c));
        opt(&mut h, l.dimming(c).then(|| (c * FPS).round() as i64));
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, w: &World, _p: f64) {
        let b = self.bundle.clone().unwrap();
        let glass = self.glass.get_or_insert_with(|| Lit::new(&b.quads, false));
        let data = glass.current(&b.lights.borrow(), w.clock);
        gfx.draw(None, data, Blend::Alpha);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let Some(b) = bundle(&self.name, g, s) else { return };
        let mut l = b.lights.borrow_mut();
        l.at(0.0);
        for (q, &on) in b.quads.iter().zip(l.lit.iter()) {
            if let (true, Some(q)) = (on, q) {
                paint_fills(c, q);
            }
        }
        l.c = None;
    }
}

// --- the street ------------------------------------------------------------------------------------

/// The waterfront: a quay, lamp posts, and water mirroring the skylines.
pub struct Street {
    colour: Rgb,
    sheen: Rgb,
    mirror: Vec<String>,
    reflect: f64,
    moon: Rgb,
    glimmer: f64,
    quay: f64,
    stone: Rgb,
    lamps: Rgb,
    spacing: f64,
    sand: Rgb,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreetSettings {
    colour: Colour,
    sheen: Colour,
    mirror: Vec<String>,
    reflect: f64,
    moon: Colour,
    glimmer: f64,
    quay: f64,
    stone: Colour,
    lamps: Colour,
    spacing: f64,
    sand: Colour,
}

impl Validate for StreetSettings {
    fn validate(&self) -> Result<(), String> {
        within("reflect", self.reflect, 0.0, 1.0)?;
        within("glimmer", self.glimmer, 0.0, 1.0)?;
        within("quay", self.quay, 0.01, 0.5)?;
        within("spacing", self.spacing, 0.01, 10.0)
    }
}

impl Street {
    fn new(st: StreetSettings) -> Self {
        Street {
            colour: st.colour.rgb(),
            sheen: st.sheen.rgb(),
            mirror: st.mirror,
            reflect: st.reflect,
            moon: st.moon.rgb(),
            glimmer: st.glimmer,
            quay: st.quay,
            stone: st.stone.rgb(),
            lamps: st.lamps.rgb(),
            spacing: st.spacing,
            sand: st.sand.rgb(),
        }
    }

    fn waterline(&self, g: &Geo, rng: &mut Rng, wl: f64) -> Vec<(f64, f64)> {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let k = (u / 1080.0).max(1.0);
        let phases: Vec<f64> = (0..3).map(|_| rng.range(0.0, 2.0 * PI)).collect();
        let (mut points, mut x) = (vec![], 0.0);
        while x <= w + 4.0 {
            let mut y =
                wl + k * (1.1 * (x / (0.07 * u) + phases[0]).sin() + 0.6 * (x / (0.017 * u) + phases[1]).sin() + 0.35 * (x / (0.006 * u) + phases[2]).sin());
            if rng.f64() < 0.06 {
                y -= k * rng.range(0.8, 1.8);
            }
            points.push((x, y));
            x += (u / 400.0).max(2.0);
        }
        points
    }

    fn lamps_at(&self, g: &Geo) -> Vec<f64> {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let mut rng = Rng::new(29);
        let (mut xs, mut x) = (vec![], 0.0);
        loop {
            x += self.spacing * u * rng.range(0.8, 1.2);
            if x > w {
                return xs;
            }
            xs.push(x.round());
        }
    }

    fn shoreline(&self, g: &Geo, c: &Context, rng: &mut Rng, face: f64, foot: &[(f64, f64)]) {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let k = (u / 1080.0).max(1.0);
        let lamps = self.lamps_at(g);
        let step_x = foot[1].0 - foot[0].0;
        let at = |x: f64| foot[((x / step_x) as i64).max(0).min(foot.len() as i64 - 1) as usize].1;
        let warmth = |x: f64| {
            let mut best: Option<f64> = None;
            for &lx in &lamps {
                let v = (-((x - lx) / (0.028 * u)).powf(2.0)).exp();
                if best.is_none_or(|b| v > b) {
                    best = Some(v);
                }
            }
            best.unwrap_or(0.0)
        };
        let mut x = rng.range(0.0, 0.05) * u;
        while x < w {
            let kind = rng.weighted(&[0.5, 0.2, 0.3]);
            let length = rng.range(0.02, if kind == 2 { 0.07 } else { 0.2 }) * u;
            if kind == 0 {
                self.rocks(c, rng, x, (w + 8.0).min(x + length), &at, &warmth, k);
            } else if kind == 1 {
                self.beach(c, rng, x, (w + 8.0).min(x + length), &at, &warmth, k, face);
            }
            x += length;
        }
    }

    fn rocks(&self, c: &Context, rng: &mut Rng, a: f64, b: f64, at: &dyn Fn(f64) -> f64, warmth: &dyn Fn(f64) -> f64, k: f64) {
        let mut cx = a;
        while cx < b {
            let taper = (PI * (cx - a) / (b - a)).sin().powf(0.5);
            let w = rng.range(4.0, 10.0) * k * (0.4 + 0.6 * taper);
            let mut piles = vec![(cx, at(cx) + rng.range(0.0, 1.0) * k, w)];
            if taper > 0.4 && rng.f64() < 0.45 {
                let px = cx + rng.range(-0.3, 0.3) * w;
                let base = at(cx) - 0.45 * w;
                let pw = w * rng.range(0.5, 0.75);
                piles.push((px, base, pw));
            }
            for (px, base, pw) in piles {
                let h = pw * rng.range(0.45, 0.8);
                let n = rng.int(6, 8);
                let mut pts = vec![];
                for i in 0..n {
                    let t = 2.0 * PI * i as f64 / n as f64 + rng.range(-0.25, 0.25);
                    let r = rng.range(0.75, 1.0);
                    pts.push((px + t.cos() * pw / 2.0 * r, base.min(base - h / 2.0 + t.sin() * h / 2.0 * r)));
                }
                let lit = warmth(px);
                let body = mixed(mixed(self.stone, BLACK, rng.range(0.25, 0.6)), self.lamps, 0.12 * lit);
                let rim = mixed(mixed(self.stone, self.sheen, 0.5), self.lamps, 0.35 * lit);
                for (dy, colour) in [(0.0, rim), ((0.8 * k).max(1.0), body)] {
                    for (i, &(qx, qy)) in pts.iter().enumerate() {
                        let y = base.min(qy + dy);
                        if i > 0 { c.line_to(qx, y) } else { c.move_to(qx, y) }
                    }
                    c.close_path();
                    c.set_source_rgb(colour.0, colour.1, colour.2);
                    let _ = c.fill();
                }
            }
            cx += w * rng.range(0.55, 0.85);
        }
    }

    fn beach(&self, c: &Context, rng: &mut Rng, a: f64, b: f64, at: &dyn Fn(f64) -> f64, warmth: &dyn Fn(f64) -> f64, k: f64, face: f64) {
        let tall = face * rng.range(0.3, 0.55);
        let phase = rng.range(0.0, 2.0 * PI);
        let xs: Vec<f64> = (0..41).map(|i| a + (b - a) * i as f64 / 40.0).collect();
        let crest = |x: f64| {
            let f = (x - a) / (b - a);
            at(x) - tall * (PI * f).sin().powf(0.7) * (1.0 + 0.12 * (9.0 * f + phase).sin())
        };
        for (i, &x) in xs.iter().enumerate() {
            if i > 0 { c.line_to(x, crest(x)) } else { c.move_to(x, crest(x)) }
        }
        for &x in xs.iter().rev() {
            c.line_to(x, at(x) + 1.0);
        }
        c.close_path();
        let top = xs.iter().map(|&x| crest(x)).fold(f64::INFINITY, f64::min);
        let bottom = xs.iter().map(|&x| at(x)).fold(f64::NEG_INFINITY, f64::max) + 1.0;
        let gr = LinearGradient::new(0.0, top, 0.0, bottom);
        let s0 = mixed(self.sand, self.lamps, 0.3 * warmth((a + b) / 2.0));
        gr.add_color_stop_rgb(0.0, s0.0, s0.1, s0.2);
        let s1 = mixed(self.sand, BLACK, 0.35);
        gr.add_color_stop_rgb(0.65, s1.0, s1.1, s1.2);
        let s2 = mixed(self.sand, BLACK, 0.6);
        gr.add_color_stop_rgb(1.0, s2.0, s2.1, s2.2);
        let _ = c.save();
        c.clip_preserve();
        let _ = c.set_source(&gr);
        let _ = c.fill();
        let n = ((b - a) * tall / (6.0 * k * k)).round() as i64;
        for _ in 0..n {
            let x = rng.range(a, b);
            let y = rng.range(crest(x), at(x));
            let light = rng.f64() < 0.5;
            c.rectangle(x.round(), y.round(), 1.0, 1.0);
            let col = if light { mixed(mixed(self.sand, WHITE, 0.1), self.lamps, 0.25 * warmth(x)) } else { mixed(self.sand, BLACK, 0.5) };
            c.set_source_rgba(col.0, col.1, col.2, rng.range(0.2, 0.45));
            let _ = c.fill();
        }
        let _ = c.restore();
        for (i, &x) in xs.iter().enumerate() {
            if i > 0 { c.line_to(x, at(x) - 0.5) } else { c.move_to(x, at(x) - 0.5) }
        }
        c.set_line_width(1.0);
        let e = mixed(self.sheen, self.lamps, 0.5 * warmth((a + b) / 2.0));
        c.set_source_rgba(e.0, e.1, e.2, 0.6);
        let _ = c.stroke();
    }

    fn lamplight(&self, g: &Geo, c: &Context, rng: &mut Rng, wl: f64) {
        let (h, u) = (g.h as f64, unit(g.w, g.h));
        let lamp = self.lamps;
        for x in self.lamps_at(g) {
            let length = (0.09 + 0.05 * rng.f64()) * u;
            let w = 0.005 * u;
            let column = LinearGradient::new(0.0, wl, 0.0, wl + length);
            column.add_color_stop_rgba(0.0, lamp.0, lamp.1, lamp.2, 0.24);
            column.add_color_stop_rgba(1.0, lamp.0, lamp.1, lamp.2, 0.0);
            let soft = LinearGradient::new(x - w, 0.0, x + w, 0.0);
            soft.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            soft.add_color_stop_rgba(0.5, 0.0, 0.0, 0.0, 1.0);
            soft.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            let _ = c.save();
            c.rectangle(x - w, wl, 2.0 * w, length);
            c.clip();
            let _ = c.set_source(&column);
            let _ = c.mask(&soft);
            let _ = c.restore();
            let mut y = wl + 1.0;
            while y < h.min(wl + length) {
                let d = (y - wl) / length;
                let mut wide = (0.002 + 0.007 * d) * u * rng.range(0.35, 1.3);
                if rng.f64() < 0.12 {
                    wide *= 2.2;
                }
                let cx = x + rng.normal(0.0, 0.6 + 2.2 * d);
                let tall = if rng.f64() < 0.75 { 1.0 } else { 2.0 };
                let gr = LinearGradient::new(cx - wide / 2.0, 0.0, cx + wide / 2.0, 0.0);
                gr.add_color_stop_rgba(0.0, lamp.0, lamp.1, lamp.2, 0.0);
                let m = mixed(self.lamps, WHITE, 0.25 * (1.0 - d));
                gr.add_color_stop_rgba(0.5, m.0, m.1, m.2, 0.8 * (1.0 - d).powf(1.4) * rng.range(0.55, 1.0));
                gr.add_color_stop_rgba(1.0, lamp.0, lamp.1, lamp.2, 0.0);
                let _ = c.set_source(&gr);
                c.rectangle(cx - wide / 2.0, y.round(), wide, tall);
                let _ = c.fill();
                y += tall + (u / 540.0).max(1.0) * *rng.pick(&[1.0, 1.0, 1.5, 2.0, 3.0]) * (1.0 + 1.5 * d);
            }
        }
    }

    fn quayside(&self, g: &Geo, c: &Context, s: &mut Shared, rng: &mut Rng, hz: f64, wl: f64) {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let top = hz.floor();
        let coping = (u / 360.0).round().max(2.0);
        let foot = self.waterline(g, rng, wl);
        s.put(&SHORE, foot.clone());
        let face = LinearGradient::new(0.0, top, 0.0, wl);
        let st = self.stone;
        face.add_color_stop_rgb(0.0, st.0, st.1, st.2);
        let m = mixed(self.stone, BLACK, 0.3);
        face.add_color_stop_rgb(1.0, m.0, m.1, m.2);
        c.move_to(0.0, top);
        c.line_to(w, top);
        for &(x, y) in foot.iter().rev() {
            c.line_to(x, y);
        }
        c.close_path();
        let _ = c.save();
        c.clip_preserve();
        let _ = c.set_source(&face);
        let _ = c.fill();
        let m = mixed(self.stone, BLACK, 0.45);
        c.set_source_rgba(m.0, m.1, m.2, 1.0);
        let mut x = rng.range(0.0, 0.03) * u;
        while x < w {
            c.rectangle(x.round(), top + coping, 1.0, wl + 4.0 - top - coping);
            x += 0.03 * u * rng.range(0.7, 1.3);
        }
        let _ = c.fill();
        let _ = c.restore();
        c.rectangle(0.0, top, w, coping);
        let m = mixed(self.stone, self.sheen, 0.45);
        c.set_source_rgb(m.0, m.1, m.2);
        let _ = c.fill();
        let l = self.lamps;
        for x in self.lamps_at(g) {
            let spot = LinearGradient::new(x - 0.03 * u, 0.0, x + 0.03 * u, 0.0);
            spot.add_color_stop_rgba(0.0, l.0, l.1, l.2, 0.0);
            spot.add_color_stop_rgba(0.5, l.0, l.1, l.2, 0.45);
            spot.add_color_stop_rgba(1.0, l.0, l.1, l.2, 0.0);
            c.rectangle(x - 0.03 * u, top, 0.06 * u, coping);
            let _ = c.set_source(&spot);
            let _ = c.fill();
            let _ = c.save();
            c.translate(x, top + coping);
            c.scale(0.03 * u, (wl - top - coping) * 0.9);
            let wash = RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
            wash.add_color_stop_rgba(0.0, l.0, l.1, l.2, 0.2);
            wash.add_color_stop_rgba(1.0, l.0, l.1, l.2, 0.0);
            c.rectangle(-1.0, 0.0, 2.0, 1.0);
            let _ = c.set_source(&wash);
            let _ = c.fill();
            let _ = c.restore();
        }
        c.rectangle(0.0, top, w, 1.0);
        let m = mixed(self.sheen, WHITE, 0.45);
        c.set_source_rgba(m.0, m.1, m.2, 0.8);
        let _ = c.fill();
        for (k, &(x, y)) in foot.iter().enumerate() {
            if k > 0 { c.line_to(x, y) } else { c.move_to(x, y) }
        }
        c.set_line_width(1.0);
        let m = mixed(self.sheen, WHITE, 0.2);
        c.set_source_rgba(m.0, m.1, m.2, 0.7);
        let _ = c.stroke();
        self.shoreline(g, c, rng, wl - top, &foot);
        let (height, r) = (0.024 * u, (0.0022 * u).max(1.0));
        let thick = (u / 540.0).round().max(2.0);
        for x in self.lamps_at(g) {
            let gr = LinearGradient::new(0.0, top - height, 0.0, top);
            let m = mixed(self.stone, self.lamps, 0.55);
            gr.add_color_stop_rgb(0.0, m.0, m.1, m.2);
            let m = mixed(self.stone, self.lamps, 0.25);
            gr.add_color_stop_rgb(0.35, m.0, m.1, m.2);
            let m = mixed(self.stone, self.sheen, 0.35);
            gr.add_color_stop_rgb(1.0, m.0, m.1, m.2);
            c.rectangle(x - thick / 2.0, top - height + r, thick, height - r);
            let _ = c.set_source(&gr);
            let _ = c.fill();
            c.rectangle(x - 2.2 * r, top - height - 1.6 * r, 4.4 * r, (0.7 * r).max(1.0));
            let m = mixed(self.stone, self.lamps, 0.4);
            c.set_source_rgb(m.0, m.1, m.2);
            let _ = c.fill();
            let glow = RadialGradient::new(x, top - height, 0.0, x, top - height, 9.0 * r);
            glow.add_color_stop_rgba(0.0, l.0, l.1, l.2, 0.55);
            glow.add_color_stop_rgba(0.3, l.0, l.1, l.2, 0.18);
            glow.add_color_stop_rgba(1.0, l.0, l.1, l.2, 0.0);
            c.rectangle(x - 9.0 * r - 1.0, top - height - 9.0 * r - 1.0, 18.0 * r + 2.0, 18.0 * r + 2.0);
            let _ = c.set_source(&glow);
            let _ = c.fill();
            c.arc(x, top - height, r, 0.0, 2.0 * PI);
            let m = mixed(self.lamps, WHITE, 0.5);
            c.set_source_rgb(m.0, m.1, m.2);
            let _ = c.fill();
        }
    }
}

impl Part for Street {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::City)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let u = unit(g.w, g.h);
        Some(vec![(0.0, (g.th.horizon_y(g.h) - 0.04 * u).floor(), g.w as f64, g.h as f64)])
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let (w, h, u) = (g.w as f64, g.h as f64, unit(g.w, g.h));
        let hz = g.th.horizon_y(g.h);
        let wl = (hz + self.quay * u).round();
        s.put(&WATERLINE, wl);
        let gr = LinearGradient::new(0.0, wl, 0.0, h);
        let sh = self.sheen;
        gr.add_color_stop_rgb(0.0, sh.0, sh.1, sh.2);
        let m = mixed(self.sheen, self.colour, 0.7);
        gr.add_color_stop_rgb(0.18, m.0, m.1, m.2);
        gr.add_color_stop_rgb(1.0, self.colour.0, self.colour.1, self.colour.2);
        c.rectangle(0.0, wl - 4.0, w, h - wl + 4.0);
        let _ = c.set_source(&gr);
        let _ = c.fill();
        let mirror = surface(g.w, g.h);
        {
            let mc = ctx(&mirror);
            for name in &self.mirror {
                if let Some(data) = s.get_named::<SkylineData>(name) {
                    let _ = mc.save();
                    mc.translate(0.0, 2.0 * wl);
                    mc.scale(1.0, -1.0);
                    let _ = mc.set_source_surface(&data.surface, 0.0, 0.0);
                    let _ = mc.paint();
                    let _ = mc.restore();
                }
            }
            mc.rectangle(0.0, wl, w, wl - hz);
            mc.set_source_rgb(self.stone.0, self.stone.1, self.stone.2);
            let _ = mc.fill();
        }
        let smear = resampled(&resampled(&mirror, 1.0 / 6.0, Filter::Good), 6.0, Filter::Bilinear);
        let rippled = surface(g.w, g.h);
        let mut rng = Rng::new(11);
        {
            let rc = ctx(&rippled);
            let band = (u / 360.0).round().max(2.0);
            let mut y = wl;
            while y < h {
                let shift = ((y * 0.21).sin() * 1.5 + rng.range(-1.5, 1.5) * (y - wl) / (h - wl)).round();
                rc.rectangle(0.0, y, w, band);
                let _ = rc.set_source_surface(&smear, shift, 0.0);
                let _ = rc.fill();
                y += band;
            }
        }
        let fade = LinearGradient::new(0.0, wl, 0.0, h);
        fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, self.reflect);
        fade.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, self.reflect * 0.35);
        fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        let _ = c.save();
        c.rectangle(0.0, wl, w, h - wl);
        c.clip();
        let _ = c.set_source_surface(&rippled, 0.0, 0.0);
        let _ = c.mask(&fade);
        self.lamplight(g, c, &mut rng, wl);
        let (sx, sr) = sun(&g.th);
        let (mx, r) = (sx * w, sr * u);
        let mut y = wl + 3.0;
        let mo = self.moon;
        while y < h {
            let d = (y - wl) / (h - wl);
            for _ in 0..rng.int(1, 3) {
                let ww = r * (0.08 + 0.35 * d) * rng.range(0.3, 1.0);
                let x = mx + rng.normal(0.0, 0.22 + 0.4 * d) * r;
                let a = self.glimmer * (1.0 - d).powf(2.0) * rng.range(0.2, 1.0) * (-((x - mx) / (0.6 * r)).powf(2.0)).exp();
                let gr = LinearGradient::new(x - ww / 2.0, 0.0, x + ww / 2.0, 0.0);
                gr.add_color_stop_rgba(0.0, mo.0, mo.1, mo.2, 0.0);
                gr.add_color_stop_rgba(0.5, mo.0, mo.1, mo.2, a);
                gr.add_color_stop_rgba(1.0, mo.0, mo.1, mo.2, 0.0);
                let _ = c.set_source(&gr);
                c.rectangle(x - ww / 2.0, y.round(), ww, (u / 900.0).round().max(1.0));
                let _ = c.fill();
            }
            y += (u / 260.0 * (1.0 + 3.0 * d)).max(2.0) * rng.range(0.6, 1.8);
        }
        let _ = c.restore();
        self.quayside(g, c, s, &mut rng, hz, wl);
    }
}

// --- the waves at the quay -------------------------------------------------------------------------

/// Waves at the quay (live): swell crests along the edge and foam flecks. A seamless loop of `loop`
/// seconds at `rate` steps a second.
pub struct Shore {
    loop_: f64,
    rate: f64,
    alpha: f64,
    foam: f64,
    colour: Rgb,
    steps: i64,
    flecks: Vec<(f64, i64, f64, f64)>,
    shore: Vec<(f64, f64)>,
    width: f64,
    step: Option<i64>,
    cache: HashMap<i64, Vec<f32>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShoreSettings {
    #[serde(rename = "loop")]
    loop_: f64,
    rate: f64,
    alpha: f64,
    foam: f64,
    colour: Colour,
}

impl Validate for ShoreSettings {
    fn validate(&self) -> Result<(), String> {
        within("loop", self.loop_, 0.5, 120.0)?;
        within("rate", self.rate, 1.0, 60.0)?;
        within("loop x rate", self.loop_ * self.rate, 1.0, 3600.0)?;
        within("alpha", self.alpha, 0.0, 1.0)?;
        within("foam", self.foam, 0.0, 1000.0)
    }
}

impl Shore {
    fn new(st: ShoreSettings) -> Self {
        Shore {
            loop_: st.loop_,
            rate: st.rate,
            alpha: st.alpha,
            foam: st.foam,
            colour: st.colour.rgb(),
            steps: 1,
            flecks: vec![],
            shore: vec![],
            width: 0.0,
            step: None,
            cache: HashMap::new(),
        }
    }

    fn make(&mut self, g: &Geo, s: &Shared) {
        self.width = g.w as f64;
        self.shore = s.get(&SHORE).cloned().unwrap_or_default();
        self.steps = ((self.loop_ * self.rate).round() as i64).max(1);
        let mut rng = Rng::new(31);
        self.flecks.clear();
        for _ in 0..(self.foam * self.width / 1920.0).round() as i64 {
            let drift = rng.range(2.0, 6.0);
            let x = rng.range(drift, self.width - drift - 3.0);
            let size = *rng.pick(&[1, 2]);
            let phase = rng.f64();
            self.flecks.push((x, size, phase, drift));
        }
    }

    fn swell(&self, x: f64, f: f64) -> f64 {
        (2.0 * PI * (x / (0.11 * self.width) - 2.0 * f)).sin()
    }

    fn crests(&self, step: i64) -> Vec<(f64, f64, f64, f64)> {
        let f = step as f64 / self.steps as f64;
        let mut runs: Vec<(f64, f64, f64, f64)> = vec![];
        for p in self.shore.windows(2) {
            let ((x0, y0), (x1, _)) = (p[0], p[1]);
            let swell = self.swell(x0, f);
            if swell <= 0.3 {
                continue;
            }
            let y = (y0 + 1.0 - swell).round();
            let alpha = round2(self.alpha * ((swell - 0.3) / 0.7).powf(2.0));
            if alpha <= 0.0 {
                continue;
            }
            let (x0, x1) = (x0.round(), x1.round());
            match runs.last_mut() {
                Some(r) if r.1 == x0 && r.2 == y && r.3 == alpha => r.1 = x1,
                _ => runs.push((x0, x1, y, alpha)),
            }
        }
        runs
    }

    fn foamed(&self, step: i64) -> Vec<(f64, f64, i64, f64)> {
        if self.shore.is_empty() {
            return vec![];
        }
        let f = step as f64 / self.steps as f64;
        let mut out = vec![];
        for &(where_, size, phase, drift) in &self.flecks {
            let x = (where_ + drift * (2.0 * PI * (f + phase)).sin()).round();
            let i = ((x / (self.shore[1].0 - self.shore[0].0)) as i64).min(self.shore.len() as i64 - 1);
            let y = self.shore[i as usize].1;
            let glow = (2.0 * PI * (f * 2.0 + phase)).sin().max(0.0);
            let y = (y + 1.0 - self.swell(x, f)).round();
            if glow > 0.05 {
                out.push((x, y, size, round2((1.5 * self.alpha).min(1.0) * glow)));
            }
        }
        out
    }

    fn placed(&self, step: i64) -> Vec<f32> {
        let mut d = vec![];
        for (x0, x1, y, a) in self.crests(step) {
            d.extend(fill_quad(x0, y, x1, y + 1.0, self.colour, a).map(|v| v as f32));
        }
        for (x, y, size, a) in self.foamed(step) {
            d.extend(fill_quad(x, y, x + 1.0 + size as f64, y + 1.0, self.colour, a).map(|v| v as f32));
        }
        d
    }
}

impl Part for Shore {
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.make(g, s);
    }
    fn top(&self, g: &Geo) -> i32 {
        let m = self.shore.iter().map(|p| p.1).fold(None, |a: Option<f64>, y| Some(a.map_or(y, |a| a.min(y))));
        (m.unwrap_or(g.h as f64 + 3.0).floor() - 3.0) as i32
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.step = (!self.shore.is_empty()).then(|| ((w.clock * self.rate) as i64).rem_euclid(self.steps));
        let mut h = 0xcbf2_9ce4_8422_2325;
        opt(&mut h, self.step);
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if let Some(step) = self.step {
            if !self.cache.contains_key(&step) {
                let d = self.placed(step);
                self.cache.insert(step, d);
            }
            gfx.draw(None, &self.cache[&step], Blend::Alpha);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        self.make(g, s);
        let d: Vec<f64> = self.placed(0).iter().map(|&v| v as f64).collect();
        paint_fills(c, &d);
    }
}

// --- the reflections -------------------------------------------------------------------------------

/// Lit windows mirrored on the water (live): a soft streak under each, `stretch` times as tall.
pub struct Reflections {
    windows: String,
    stretch: f64,
    alpha: f64,
    smear: Option<ImageSurface>,
    tex: Option<Tex>,
    sources: Option<Vec<(Vec<Option<[f64; 32]>>, Rc<Bundle>)>>,
    streaks: Option<Vec<Lit>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReflectionsSettings {
    windows: String,
    stretch: f64,
    alpha: f64,
}

impl Validate for ReflectionsSettings {
    fn validate(&self) -> Result<(), String> {
        within("stretch", self.stretch, 0.0, 100.0)?;
        within("alpha", self.alpha, 0.0, 1.0)
    }
}

impl Reflections {
    fn new(st: ReflectionsSettings) -> Self {
        Reflections { windows: st.windows, stretch: st.stretch, alpha: st.alpha, smear: None, tex: None, sources: None, streaks: None }
    }

    fn smear(&mut self) -> ImageSurface {
        if self.smear.is_none() {
            let (w, h) = (32, 128);
            let s = surface(w, h);
            {
                let c = ctx(&s);
                let mut rng = Rng::new(5);
                for y in 0..h {
                    let t = y as f64 / h as f64;
                    if rng.f64() < 0.18 * t {
                        continue;
                    }
                    let across = LinearGradient::new(0.0, 0.0, w as f64, 0.0);
                    for k in 0..9 {
                        let x = k as f64 / 8.0;
                        across.add_color_stop_rgba(x, 1.0, 1.0, 1.0, (-((x - 0.5) / 0.22).powf(2.0)).exp() * (1.0 - t).powf(1.5));
                    }
                    c.rectangle(0.0, y as f64, w as f64, 1.0);
                    let _ = c.set_source(&across);
                    let _ = c.fill();
                }
            }
            self.smear = Some(s);
        }
        self.smear.clone().unwrap()
    }

    fn sources(&mut self, g: &Geo, s: &mut Shared) {
        if self.sources.is_some() {
            return;
        }
        let u = unit(g.w, g.h);
        let h = g.h as f64;
        let hz = s.get(&WATERLINE).copied().unwrap_or_else(|| g.th.horizon_y(g.h));
        let Some(b) = bundle(&self.windows, g, s) else {
            self.sources = Some(vec![]);
            return;
        };
        let mut quads = vec![];
        for w in &b.windows {
            let Some(span) = w.mirrored else {
                quads.push(None);
                continue;
            };
            let m0 = hz + (hz - w.y1 as f64);
            let length = (w.y1 - w.y0) as f64 * self.stretch + 0.012 * u;
            let ww = (2.4 * (span.1 - span.0) as f64).max(3.0);
            let cx = (span.0 + span.1) as f64 / 2.0 + (m0 * 0.13).sin() * 1.5;
            let fade = (1.0 - (m0 - hz) / (h - hz)).max(0.0);
            quads.push(Some(image_quad(cx - ww / 2.0, m0, cx + ww / 2.0, m0 + length, w.colour, self.alpha * fade, (0.0, 0.0, 1.0, 1.0))));
        }
        self.sources = Some(vec![(quads, b)]);
    }
}

impl Part for Reflections {
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.sources(g, s);
    }
    fn textures(&mut self, _g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.tex = Some(up.add(self.smear(), Kind::Sprite));
    }
    fn top(&self, g: &Geo) -> i32 {
        g.th.horizon_y(g.h).floor() as i32
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let c = w.clock;
        let mut h = 0xcbf2_9ce4_8422_2325;
        for (_, b) in self.sources.as_ref().unwrap() {
            let l = b.lights.borrow();
            mix(&mut h, l.version);
            opt(&mut h, l.dimming(c).then(|| (c * FPS).round() as i64));
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, w: &World, _p: f64) {
        let sources = self.sources.as_ref().unwrap();
        let streaks = self.streaks.get_or_insert_with(|| sources.iter().map(|(q, _)| Lit::new(q, true)).collect());
        let tex = self.tex;
        for (lit, (_, b)) in streaks.iter_mut().zip(sources.iter()) {
            let data = lit.current(&b.lights.borrow(), w.clock);
            gfx.draw(tex, data, Blend::Premultiplied);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        self.sources(g, s);
        let smear = self.smear();
        for (quads, b) in self.sources.as_ref().unwrap() {
            let mut l = b.lights.borrow_mut();
            l.at(0.0);
            let mut data: Vec<f32> = vec![];
            for (q, &on) in quads.iter().zip(l.lit.iter()) {
                if let (true, Some(q)) = (on, q) {
                    data.extend(q.map(|v| v as f32));
                }
            }
            paint_vertices(c, &smear, &data);
            l.c = None;
        }
    }
}

// --- the beacons -----------------------------------------------------------------------------------

/// Aircraft-warning lights (live): slow red blinks on the tallest spires.
pub struct Beacons {
    skylines: Vec<String>,
    count: i64,
    colour: Rgb,
    period: f64,
    lit: f64,
    beacons: Vec<(f64, f64, f64)>,
    size: f64,
    on: Vec<bool>,
    // each beacon's sprite quad, and the lit ones' this frame
    quads: Vec<Vec<f32>>,
    data: Vec<f32>,
    tex: Option<Tex>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeaconsSettings {
    skylines: Vec<String>,
    count: i64,
    colour: Colour,
    period: f64,
    lit: f64,
}

impl Validate for BeaconsSettings {
    fn validate(&self) -> Result<(), String> {
        within("count", self.count as f64, 0.0, 1000.0)?;
        within("period", self.period, 0.05, 3600.0)?;
        within("lit", self.lit, 0.0, 1.0)
    }
}

impl Beacons {
    fn new(st: BeaconsSettings) -> Self {
        Beacons {
            skylines: st.skylines,
            count: st.count,
            colour: st.colour.rgb(),
            period: st.period,
            lit: st.lit,
            beacons: vec![],
            size: 0.0,
            on: vec![],
            quads: vec![],
            data: vec![],
            tex: None,
        }
    }

    fn lights(&self, s: &Shared) -> Vec<(f64, f64, f64)> {
        let mut tips: Vec<(f64, f64)> = vec![];
        for name in &self.skylines {
            if let Some(d) = s.get_named::<SkylineData>(name) {
                tips.extend(d.tips.iter().cloned());
            }
        }
        tips.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut rng = Rng::new(41);
        tips.into_iter().take(self.count.max(0) as usize).map(|(x, y)| (x, y, rng.f64())).collect()
    }

    fn make(&mut self, g: &Geo, s: &Shared) -> ImageSurface {
        let dot = soft_dot(32, &SOFT_DOT);
        self.beacons = self.lights(s);
        self.size = (0.012 * unit(g.w, g.h)).max(6.0);
        self.quads = self.beacons.iter().map(|&(x, y, _)| sprite_vertices(&[(x, y, self.size, 0.0, self.colour, 1.0)])).collect();
        dot
    }

    fn on_at(&self, c: f64) -> Vec<bool> {
        self.beacons.iter().map(|&(_, _, phase)| self.lit_at(phase, c)).collect()
    }

    fn lit_at(&self, phase: f64, c: f64) -> bool {
        (c / self.period + phase).rem_euclid(1.0) < self.lit
    }

    fn placed(&self, on: &[bool]) -> Vec<f32> {
        let p: Vec<_> = self.beacons.iter().zip(on).filter(|(_, l)| **l).map(|(&(x, y, _), _)| (x, y, self.size, 0.0, self.colour, 1.0)).collect();
        sprite_vertices(&p)
    }
}

impl Part for Beacons {
    fn live(&self) -> bool {
        true
    }
    fn textures(&mut self, g: &Geo, s: &Shared, up: &mut Uploads) {
        self.tex = Some(up.add(self.make(g, s), Kind::Sprite));
    }
    fn top(&self, g: &Geo) -> i32 {
        let m = self.beacons.iter().map(|b| b.1).fold(g.h as f64, f64::min);
        (m - 0.02 * unit(g.w, g.h)).floor() as i32
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.on.resize(self.beacons.len(), false);
        for i in 0..self.beacons.len() {
            self.on[i] = self.lit_at(self.beacons[i].2, w.clock);
        }
        let mut h = 0xcbf2_9ce4_8422_2325;
        for &l in &self.on {
            mix(&mut h, l as u64);
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        self.data.clear();
        for (q, _) in self.quads.iter().zip(&self.on).filter(|(_, on)| **on) {
            self.data.extend_from_slice(q);
        }
        gfx.draw(self.tex, &self.data, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let dot = self.make(g, s);
        let d = self.placed(&self.on_at(0.0));
        paint_vertices(c, &dot, &d);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "night-sky",
            defaults: r##"zenith = "#04030a"
middle = "#120a26"
horizon = "#4a1a5e"
glow = 0.55
stars = 60"##,
            varies: &[],
            check: check::<NightSkySettings>,
            make: |_, t, _g| Ok(vec![Box::new(NightSky::new(settings(t)))]),
        },
        Kind {
            name: "moon",
            defaults: r##"colour = "#f3ecff"
shade = "#b9a6e6"
halo = 0.30
rise = 0.62
clouds = "#2a1a4a""##,
            varies: &[],
            check: check::<MoonSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Moon::new(settings(t)))]),
        },
        Kind {
            name: "skyline",
            defaults: r##"seed = 1
heights = [0.08, 0.32]
breadth = [0.025, 0.07]
downtown = 0.62
colour = "#0d0a19"
haze = 0.0
fog = "#3a1650"
rim = "#7a5cc8"
window = 0.0042
glass = "#2a2340"
dark = 0.12"##,
            varies: &[],
            check: check::<SkylineSettings>,
            make: |n, t, _g| Ok(vec![Box::new(Skyline::new(n, settings(t)))]),
        },
        Kind {
            name: "windows",
            defaults: r##"skylines = ["skyline"]
lit = 0.35
period = [120.0, 900.0]
colours = ["#ffd38a", "#ffe9c4", "#bfe6ff", "#ff7ad9", "#6ff3ff"]
alpha = 0.9"##,
            varies: &["skylines", "colours"],
            check: check::<WindowsSettings>,
            make: |n, _, _g| Ok(vec![Box::new(Windows { name: n.into(), bundle: None, glass: None })]),
        },
        Kind {
            name: "street",
            defaults: r##"colour = "#06050c"
sheen = "#3a1650"
mirror = ["skyline-far", "skyline-near"]
reflect = 0.55
moon = "#e6dcff"
glimmer = 0.75
quay = 0.016
stone = "#262038"
lamps = "#ffc978"
spacing = 0.07
sand = "#4a3c48""##,
            varies: &["mirror"],
            check: check::<StreetSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Street::new(settings(t)))]),
        },
        Kind {
            name: "shore",
            defaults: r##"loop = 8.0
rate = 15
alpha = 0.2
foam = 10
colour = "#d8c8ff""##,
            varies: &[],
            check: check::<ShoreSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Shore::new(settings(t)))]),
        },
        Kind {
            name: "reflections",
            defaults: r##"windows = "windows"
stretch = 5.0
alpha = 0.42"##,
            varies: &[],
            check: check::<ReflectionsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Reflections::new(settings(t)))]),
        },
        Kind {
            name: "beacons",
            defaults: r##"skylines = ["skyline-far", "skyline-mid", "skyline-near"]
count = 7
colour = "#ff2a2a"
period = 2.4
lit = 0.25"##,
            varies: &["skylines"],
            check: check::<BeaconsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Beacons::new(settings(t)))]),
        },
    ]
}
