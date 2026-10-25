//! Element kinds shared by all themes: sky, rain, finish (scanlines and vignette), hostname (solid or
//! neon) and its ruler.

use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Patch, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::title::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Extend, ImageSurface, LineCap, LinearGradient, Matrix, RadialGradient, SurfacePattern};
use serde::Deserialize;

/// The ruler under the hostname. The caption leaves it; the ticks read it.
const RULER: Key<Ruler> = Key::new("ruler");

/// A kind with no settings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoSettings {}

impl Validate for NoSettings {}

// --- rain ------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RainSettings {
    drops: f64,
    angle: f64,
    colour: Colour,
    alpha: [f64; 2],
    length: [f64; 2],
    #[serde(rename = "loop")]
    loop_: f64,
    splash: f64,
    sway: f64,
    tiled: bool,
}

impl Validate for RainSettings {
    fn validate(&self) -> Result<(), String> {
        within("drops", self.drops, 0.0, 100000.0)?;
        within("angle", self.angle, -80.0, 80.0)?;
        for a in self.alpha {
            within("alpha", a, 0.0, 1.0)?;
        }
        for l in self.length {
            within("length", l, 0.001, 1.0)?;
        }
        within("loop", self.loop_, 0.5, 600.0)?;
        within("splash", self.splash, 0.0, 1.0)?;
        within("sway", self.sway, 0.0, 3600.0)
    }
}

/// Slanting rain (live): `drops` streaks, far ones short and faint, near ones long and bright, each
/// ending in a splash. A seamless loop of `loop` seconds, vertices built once. `sway` swings the slant
/// from `angle` to its mirror, as a shear on upright drops. `tiled`: three pre-drawn depth tiles
/// scrolling at their own speeds, three quads a frame, no splashes.
pub struct Rain {
    tiled: bool,
    drops: f64,
    angle: f64,
    colour: (f64, f64, f64),
    alpha: (f64, f64),
    length: (f64, f64),
    loop_: f64,
    splash: f64,
    sway: f64,
    speeds: Vec<f64>,
    t: f64,
    // untiled
    steps: i64,
    step: i64,
    drop: Vec<(f64, f64, f64, f64, f64, f64)>,
    slope: f64,
    tilt: f64,
    cache: std::collections::HashMap<i64, (Vec<f32>, Vec<f32>)>,
    sheared: (Vec<f32>, Vec<f32>),
    // textures: streak and ring (untiled), or the depth tiles (tiled)
    sprites: Option<(Tex, Tex)>,
    depths: Vec<Tex>,
}

const TILE: i32 = 512;
const DEPTHS: [(f64, f64); 3] = [(0.0, 0.33), (0.33, 0.66), (0.66, 1.0)];
const FPS: f64 = 30.0;

impl Rain {
    fn new(st: RainSettings) -> Self {
        Rain {
            tiled: st.tiled,
            drops: st.drops,
            angle: st.angle,
            colour: st.colour.rgb(),
            alpha: (st.alpha[0], st.alpha[1]),
            length: (st.length[0], st.length[1]),
            loop_: st.loop_,
            splash: st.splash,
            sway: st.sway,
            speeds: vec![],
            t: 0.0,
            steps: 1,
            step: 0,
            drop: vec![],
            slope: 0.0,
            tilt: 0.0,
            cache: Default::default(),
            sheared: (vec![], vec![]),
            sprites: None,
            depths: vec![],
        }
    }
    fn slant(&self, t: f64) -> f64 {
        if self.sway != 0.0 { (self.angle * (std::f64::consts::PI * t / self.sway).cos()).to_radians().tan() } else { 0.0 }
    }
    /// Tiled mode: each depth's tile and fall speed.
    fn tiles(&mut self, g: &Geo) -> Vec<ImageSurface> {
        let (w, h, u) = (g.w as f64, g.h as f64, unit(g.w, g.h));
        let n = self.drops * w * h / (1920.0 * 1080.0);
        let t = TILE as f64;
        let mut rng = Rng::new(23);
        let mut out = vec![];
        self.speeds.clear();
        for (lo, hi) in DEPTHS.iter() {
            let share = hi.powf(2.0 / 3.0) - lo.powf(2.0 / 3.0);
            let count = (n * share * t * t / (w * h)).round() as i64;
            let mut streaks = vec![];
            for _ in 0..count {
                let depth = rng.range(*lo, *hi);
                let x = rng.range(0.0, t);
                let y = rng.range(0.0, t);
                streaks.push((x, y, (self.length.0 + (self.length.1 - self.length.0) * depth) * u, self.alpha.0 + (self.alpha.1 - self.alpha.0) * depth));
            }
            let tile_s = tile(
                TILE,
                TILE,
                |c: &Context| {
                    c.set_line_cap(LineCap::Round);
                    for &(x, y, length, alpha) in &streaks {
                        let gr = LinearGradient::new(0.0, y, 0.0, y + length);
                        gr.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
                        gr.add_color_stop_rgba(0.8, 1.0, 1.0, 1.0, 0.8 * alpha);
                        gr.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, alpha);
                        let _ = c.set_source(&gr);
                        c.set_line_width(2.2 / 64.0 * length);
                        c.move_to(x, y);
                        c.line_to(x, y + length);
                        let _ = c.stroke();
                    }
                },
                Some(self.length.1 * u + 4.0),
            );
            let middle = (lo + hi) / 2.0;
            let falls = if middle < 0.5 { 3.0 } else { 5.0 };
            self.speeds.push(falls * h / self.loop_);
            out.push(tile_s);
        }
        out
    }

    /// Untiled mode: streak and splash ring sprites, and the drops.
    fn untiled(&mut self, g: &Geo) -> (ImageSurface, ImageSurface) {
        let (w, h, u) = (g.w as f64, g.h as f64, unit(g.w, g.h));
        let size = 64;
        let streak = surface(size, size);
        {
            let c = ctx(&streak);
            let gr = LinearGradient::new(0.0, 4.0, 0.0, size as f64 - 4.0);
            gr.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
            gr.add_color_stop_rgba(0.8, 1.0, 1.0, 1.0, 0.8);
            gr.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 1.0);
            let _ = c.set_source(&gr);
            c.set_line_width(2.2);
            c.set_line_cap(LineCap::Round);
            c.move_to(size as f64 / 2.0, 4.0);
            c.line_to(size as f64 / 2.0, size as f64 - 4.0);
            let _ = c.stroke();
        }
        let ring = surface(size, size);
        {
            let c = ctx(&ring);
            let half = size as f64 / 2.0;
            c.translate(half, half);
            c.scale(half - 4.0, (half - 4.0) * 0.3);
            c.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
            c.set_line_width(0.16);
            c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            let _ = c.stroke();
        }
        let mut rng = Rng::new(23);
        let hz = g.th.horizon_y(g.h);
        let mut n = (self.drops * w * h / (1920.0 * 1080.0)).round();
        self.steps = ((self.loop_ * FPS).round() as i64).max(1);
        self.cache.clear();
        self.drop.clear();
        let mut slope = self.angle.to_radians().tan();
        let (lo, hi);
        if self.sway != 0.0 {
            let reach = slope.abs() * h;
            n = (n * (w + 2.0 * reach) / w).round();
            (lo, hi, slope) = (-reach, w + reach, 0.0);
        } else {
            (lo, hi) = (0f64.min(-slope * h), w.max(w - slope * h));
        }
        for _ in 0..n as i64 {
            let depth = rng.f64().powf(1.5);
            let length = (self.length.0 + (self.length.1 - self.length.0) * depth) * u;
            let land = hz + (h - hz) * (0.05 + 0.95 * depth) * rng.range(0.6, 1.0);
            let falls = if depth < 0.5 { *rng.pick(&[2.0, 3.0, 3.0, 4.0]) } else { *rng.pick(&[4.0, 5.0, 6.0]) };
            let x = rng.range(lo, hi);
            let phase = rng.f64();
            self.drop.push((x, phase, falls, length, land, depth));
        }
        self.slope = slope;
        self.tilt = if self.sway != 0.0 { 0.0 } else { self.angle };
        (streak, ring)
    }

    /// Vertices of the drops and splashes at one loop step.
    fn placed(&self, g: &Geo, step: i64) -> (Vec<f32>, Vec<f32>) {
        let f = step as f64 / self.steps as f64;
        let (mut streaks, mut splashes) = (vec![], vec![]);
        for &(x0, phase, falls, length, land, depth) in &self.drop {
            let k = (phase + falls * f).rem_euclid(1.0);
            let span = land + length;
            let y = -length + k * (span + 0.18 * span);
            let alpha = self.alpha.0 + (self.alpha.1 - self.alpha.0) * depth;
            if y <= land {
                let (cx, cy) = (x0 + (y - length / 2.0) * self.slope, y - length / 2.0);
                streaks.push((cx, cy, length, -self.tilt, self.colour, alpha));
            } else if self.splash != 0.0 {
                let s = (y - land) / (0.18 * span);
                let size = (0.006 + 0.012 * depth) * unit(g.w, g.h) * (0.5 + s);
                splashes.push((x0 + land * self.slope, land, size, 0.0, self.colour, alpha * self.splash * 2.0 * (1.0 - s)));
            }
        }
        (sprite_vertices(&streaks), sprite_vertices(&splashes))
    }

    /// Shear: x += k * y.
    fn shear(data: &mut [f32], k: f64) {
        if k == 0.0 {
            return;
        }
        for v in data.as_chunks_mut::<8>().0 {
            v[6] = (v[6] as f64 + v[7] as f64 * k) as f32;
        }
    }
}

impl Part for Rain {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        if self.tiled {
            self.depths = self.tiles(g).into_iter().map(|t| up.add(t, Kind::Tile)).collect();
        } else {
            let (streak, ring) = self.untiled(g);
            self.sprites = Some((up.add(streak, Kind::Sprite), up.add(ring, Kind::Sprite)));
        }
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        if self.tiled {
            self.t = (w.clock * FPS).round() / FPS;
            return self.t.to_bits();
        }
        self.step = ((w.clock * FPS).round() as i64).rem_euclid(self.steps);
        self.step as u64
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, w: &World, _p: f64) {
        let (r, gg, b) = self.colour;
        if !self.tiled {
            if !self.cache.contains_key(&self.step) {
                let p = self.placed(g, self.step);
                self.cache.insert(self.step, p);
            }
            let k = self.slant(w.clock);
            let (streaks, splashes) = &self.cache[&self.step];
            for (out, data) in [(&mut self.sheared.0, streaks), (&mut self.sheared.1, splashes)] {
                out.clear();
                out.extend_from_slice(data);
                Self::shear(out, k);
            }
            if let Some((streak, ring)) = self.sprites {
                gfx.draw(Some(streak), &self.sheared.0, Blend::Premultiplied);
                gfx.draw(Some(ring), &self.sheared.1, Blend::Premultiplied);
            }
            return;
        }
        let (w, h, size) = (g.w as f64, g.h as f64, TILE as f64);
        let k = self.slant(self.t);
        let reach = k.abs() * h + 2.0;
        let (x0, x1) = (-reach, w + reach);
        for (&speed, &tex) in self.speeds.iter().zip(self.depths.iter()) {
            let v = (self.t * speed / size).rem_euclid(1.0);
            let (u0, u1, v0, v1) = (x0 / size, x1 / size, -v, h / size - v);
            let q =
                [u0, v0, r, gg, b, 1.0, x0, 0.0, u1, v0, r, gg, b, 1.0, x1, 0.0, u1, v1, r, gg, b, 1.0, x1 + h * k, h, u0, v1, r, gg, b, 1.0, x0 + h * k, h];
            gfx.draw(Some(tex), &q.map(|x| x as f32), Blend::Premultiplied);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        if self.tiled {
            let k = self.slant(0.0);
            for s in &self.tiles(g) {
                let p = SurfacePattern::create(s);
                p.set_extend(Extend::Repeat);
                p.set_matrix(Matrix::new(1.0, 0.0, -k, 1.0, 0.0, 0.0));
                c.set_source_rgb(self.colour.0, self.colour.1, self.colour.2);
                let _ = c.mask(&p);
            }
            return;
        }
        let (streak, ring) = self.untiled(g);
        let (mut streaks, mut splashes) = self.placed(g, 0);
        let k = self.slant(0.0);
        Self::shear(&mut streaks, k);
        Self::shear(&mut splashes, k);
        paint_vertices(c, &streak, &streaks);
        paint_vertices(c, &ring, &splashes);
    }
}

// --- the finish ------------------------------------------------------------------------------------

/// Faint CRT scanlines and a vignette over everything, on their own layer.
pub struct Finish;

impl Part for Finish {
    fn still(&self) -> bool {
        true
    }
    fn alone(&self) -> bool {
        true
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let th = &g.th;
        let (w, h) = (g.w as f64, g.h as f64);
        if th.tw.scanlines {
            let (step, alpha) = th.tw.scanline;
            c.set_source_rgba(0.0, 0.0, 0.0, alpha);
            let mut y = 0;
            while y < g.h {
                c.rectangle(0.0, y as f64, w, 1.0);
                y += (step as i32).max(1);
            }
            let _ = c.fill();
        }
        let (cx, cy, big) = (w / 2.0, h / 2.0, w.max(h));
        let v = RadialGradient::new(cx, cy, 0.35 * big, cx, cy, 0.75 * big);
        v.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
        v.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, th.tw.vignette);
        let _ = c.set_source(&v);
        let _ = c.paint();
    }
}

// --- the hostname ----------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TitleSettings {
    /// "solid" (extruded, gradient face) or "tube" (neon).
    style: String,
    /// Tubes only: a letter now and then stutters.
    flicker: bool,
}

impl Validate for TitleSettings {
    fn validate(&self) -> Result<(), String> {
        match self.style.as_str() {
            "solid" | "tube" => Ok(()),
            s => Err(format!("style is {s:?}: it is \"solid\" or \"tube\"")),
        }
    }
}

/// The hostname as neon tubes. With flicker, a letter now and then stutters like a failing tube.
/// Driven by the scene clock from a fixed seed.
pub struct Tubes {
    flicker: bool,
    atlas: Option<TubeAtlas>,
    rng: Rng,
    events: Vec<(f64, Vec<(f64, bool)>, i64)>,
    dark: i64,
    tex: Option<Tex>,
    pieces: Vec<Patch>,
}

impl Tubes {
    fn new(flicker: bool) -> Self {
        Tubes { flicker, atlas: None, rng: Rng::new(0x7ab3), events: vec![], dark: -1, tex: None, pieces: vec![] }
    }
    fn atlas(&mut self, g: &Geo) -> &TubeAtlas {
        if self.atlas.is_none() {
            self.atlas = Some(g.th.tube_title(g.w, g.h, g.scale, &g.host, g.edges));
        }
        self.atlas.get_or_insert_with(|| TubeAtlas { atlas: surface(1, 1), placed: vec![] })
    }
    fn dark_at(&mut self, c: f64) -> i64 {
        let n = self.atlas.as_ref().map_or(0, |a| a.placed.len()) as i64;
        while self.events.last().is_none_or(|e| e.0 < c + 2.0) {
            let mut at = self.events.last().and_then(|e| e.1.last()).map_or(0.0, |s| s.0);
            at += self.rng.range(4.0, 14.0);
            let mut steps = vec![];
            let mut t = at;
            for _ in 0..self.rng.int(2, 5) {
                t += self.rng.range(0.03, 0.09);
                steps.push((t, true));
                t += self.rng.range(0.04, 0.16);
                steps.push((t, false));
            }
            if self.rng.f64() < 0.25 {
                t += self.rng.range(0.6, 1.8);
                steps.push((t, true));
                t += self.rng.range(0.05, 0.1);
                steps.push((t, false));
            }
            let letter = self.rng.int_below(0, n.max(1));
            self.events.push((at, steps, letter));
            if self.events.len() > 8 {
                self.events.remove(0);
            }
        }
        for (at, steps, letter) in &self.events {
            if *at <= c && steps.last().is_some_and(|s| c < s.0) {
                let dark = steps.iter().find(|(until, _)| c < *until).is_some_and(|s| s.1);
                return if dark { *letter } else { -1 };
            }
        }
        -1
    }
}

impl Part for Tubes {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Title)
    }
    fn live(&self) -> bool {
        self.flicker
    }
    fn still(&self) -> bool {
        !self.flicker
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let a = self.atlas(g);
        for &Cell { ax, x, y, w, h } in &a.placed {
            c.save().ok();
            c.rectangle(x as f64, y as f64, w as f64, h as f64);
            c.clip();
            if c.set_source_surface(&a.atlas, (x - ax) as f64, y as f64).is_ok() {
                let _ = c.paint();
            }
            c.restore().ok();
        }
    }
    fn top(&self, g: &Geo) -> i32 {
        self.atlas.as_ref().map_or(g.h - 1, |a| a.placed.iter().map(|p| p.y).min().unwrap_or(g.h - 1))
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.atlas(g);
        if let Some(a) = self.atlas.as_mut() {
            // handed over: only the cells' places are kept
            let picture = std::mem::replace(&mut a.atlas, surface(1, 1));
            self.tex = Some(up.add(picture, Kind::Label));
        }
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.dark = self.dark_at(w.clock);
        self.dark as u64
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (Some(a), Some(tex)) = (self.atlas.as_ref(), self.tex) else { return };
        let half = (gfx.size(tex).1 / 2) as f64;
        self.pieces.clear();
        self.pieces.extend(a.placed.iter().enumerate().map(|(i, &Cell { ax, x, y, w, h })| {
            let sy = if i as i64 == self.dark { half } else { 0.0 };
            Patch { sx0: ax as f64, sy0: sy, sx1: (ax + w) as f64, sy1: sy + h as f64, x0: x as f64, y0: y as f64, x1: (x + w) as f64, y1: (y + h) as f64 }
        }));
        gfx.patches(tex, &self.pieces, 1.0);
    }
}

/// The neon hostname's caption: the ruled line (none without RULE) and the ruler position.
pub struct Caption;

impl Part for Caption {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Title)
    }
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let p = g.th.title_place(g.w, g.h, &g.host, g.edges);
        let y = (p.base + 0.03 * unit(g.w, g.h)).round();
        Some(vec![(p.x0.floor() - 1.0, y - 3.0, g.w as f64, y + 4.0)])
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let p = g.th.title_place(g.w, g.h, &g.host, g.edges);
        s.put(&RULER, g.th.caption(c, g.w, g.h, p.x0, p.base));
    }
}

/// The solid hostname with its glow and the ruled line under it.
pub struct Solid;

impl Part for Solid {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Title)
    }
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let th = &g.th;
        let (lettering, left, base) = th.title(g.w, g.h, g.scale, &g.host, g.edges);
        let u = th.title_place(g.w, g.h, &g.host, g.edges).u;
        glowing(c, &drop_shadow(lettering, &th.tw.title_shadow, u / 10.0, None), &th.tw.title_glow);
        s.put(&RULER, th.caption(c, g.w, g.h, left, base));
    }
}

// --- the sky and the ruler -------------------------------------------------------------------------

/// Fills the picture with BASE.
pub struct Sky;

impl Part for Sky {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (r, gg, b) = g.th.tw.base.rgb();
        c.set_source_rgb(r, gg, b);
        let _ = c.paint();
    }
}

/// The ruler's ticks (live), drifting left. Drawn over the finish, dimmed to match the vignette.
#[derive(Default)]
pub struct Ticks {
    ruler: Option<Ruler>,
    offset: Option<f64>,
    marks: Vec<(f64, f64, f64, f64, f64)>,
}

/// The pixels a span from a to b covers, as `util::spans` gives them, without allocating.
fn each_span(a: f64, b: f64, mut f: impl FnMut(f64, f64, f64)) {
    let (fa, fb) = (a.floor(), b.floor());
    if fa == fb {
        if b > a {
            f(fa, fa + 1.0, b - a);
        }
        return;
    }
    if a > fa {
        f(fa, fa + 1.0, fa + 1.0 - a);
    }
    let start = if a > fa { fa + 1.0 } else { fa };
    if fb > start {
        f(start, fb, 1.0);
    }
    if b > fb {
        f(fb, fb + 1.0, b - fb);
    }
}

impl Ticks {
    /// The ticks at an offset into `self.marks`, and their colour.
    fn marks(&mut self, g: &Geo, ruler: Ruler, offset: f64) -> (f64, f64, f64) {
        let th = &g.th;
        let (w, h) = (g.w as f64, g.h as f64);
        let Ruler { left, stop, y } = ruler;
        let step = 0.012 * w;
        let right = stop + 0.05 * w;
        let (cx, cy, big) = (w / 2.0, h / 2.0, w.max(h));
        let reach = ((left + stop) / 2.0 - cx).hypot(y - cy);
        let dark = th.tw.vignette * 1f64.min(0f64.max((reach - 0.35 * big) / (0.4 * big)));
        let marks = &mut self.marks;
        marks.clear();
        let mut k: i64 = -5;
        loop {
            let x = right - k as f64 * step - offset;
            k += 1;
            if x > stop {
                continue;
            }
            if x < left {
                break;
            }
            let tall = if (k - 1) % 5 == 0 { 4.0 } else { 2.0 };
            each_span(y - tall, y, |ya, yb, cy| each_span(x, x + 1.0, |xa, xb, cx| marks.push((xa, ya, xb, yb, 0.55 * cx * cy))));
        }
        let cap = th.tw.caption.rgb();
        (cap.0 * (1.0 - dark), cap.1 * (1.0 - dark), cap.2 * (1.0 - dark))
    }
}

impl Part for Ticks {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Title)
    }
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, _g: &Geo, _w: &mut World, s: &mut Shared) {
        self.ruler = s.get(&RULER).copied();
    }
    fn top(&self, g: &Geo) -> i32 {
        self.ruler.map_or(g.h, |r| ruler_area(r).1)
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        self.offset = self.ruler.map(|_| g.th.tick_offset(g.w, w.clock));
        self.offset.map_or(u64::MAX, |o| o.to_bits())
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if let (Some(ruler), Some(offset)) = (self.ruler, self.offset) {
            let rgb = self.marks(g, ruler, offset);
            gfx.rects(None, &self.marks, rgb, Blend::Alpha);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        if let Some(&r) = s.get(&RULER) {
            g.th.ticks(c, g.w, g.h, r, 0.0);
        }
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "rain",
            defaults: r##"drops = 520
angle = 11.0
colour = "#c3cdff"
alpha = [0.12, 0.5]
length = [0.012, 0.05]
loop = 4.0
splash = 0.4
sway = 0.0
tiled = false"##,
            varies: &[],
            check: check::<RainSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Rain::new(settings(t)))]),
        },
        Kind { name: "finish", defaults: "", varies: &[], check: check::<NoSettings>, make: |_, _, _g| Ok(vec![Box::new(Finish)]) },
        Kind {
            name: "title",
            defaults: r#"style = "solid"
flicker = false"#,
            varies: &[],
            check: check::<TitleSettings>,
            make: |_, t, _g| {
                let st: TitleSettings = settings(t);
                Ok(match st.style.as_str() {
                    "solid" => vec![Box::new(Solid)],
                    _ => vec![Box::new(Caption), Box::new(Tubes::new(st.flicker))],
                })
            },
        },
        Kind { name: "sky", defaults: "", varies: &[], check: check::<NoSettings>, make: |_, _, _g| Ok(vec![Box::new(Sky)]) },
        Kind { name: "ruler", defaults: "", varies: &[], check: check::<NoSettings>, make: |_, _, _g| Ok(vec![Box::new(Ticks::default())]) },
    ]
}
