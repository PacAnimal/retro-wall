//! The woods (live), over the forest's still ground: what stands on it, drawn back to front each frame
//! from a few packed textures and bending in the wind, and life among it: the clouds' shadows drifting
//! over, ducks on the river, a fish leaping now and then, crows flying over with their shadows,
//! butterflies round the flowers, rabbits at their burrows, leaves falling and dust in the light. At
//! rest (the still picture) it is what stands on the ground, with its shadows, and nothing else.
//!
//! Everything moves as a function of the scene's clock, so a frame is the same however it was come to.

use super::forest::{DOWN, Line, STAND, Settings, Sort, Sprite, Stand, UP, draw, shadow, shadow_at, spot};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, RadialGradient};
use std::f64::consts::{PI, TAU};

/// A packed texture's side.
const PAGE: i32 = 2048;
/// Pixels round each picture in a page, so its neighbours do not bleed into it as it is drawn smaller.
const PAD: i32 = 3;

/// Pictures packed into pages, row by row.
#[derive(Default)]
struct Atlas {
    pages: Vec<ImageSurface>,
    /// The page being filled: where the next picture goes, and the tallest in this row.
    at: (i32, i32, i32),
}

/// Where a picture is in the atlas: its page and its corners there (u0, v0, u1, v1).
#[derive(Clone, Copy, Default)]
struct Spot {
    page: usize,
    uv: (f64, f64, f64, f64),
}

impl Atlas {
    fn put(&mut self, pic: &ImageSurface) -> Spot {
        let mut pic = pic.clone();
        if pic.width() + 2 * PAD > PAGE || pic.height() + 2 * PAD > PAGE {
            let k = (PAGE - 2 * PAD) as f64 / pic.width().max(pic.height()) as f64;
            pic = resampled(&pic, k, Filter::Good);
        }
        let (w, h) = (pic.width() + 2 * PAD, pic.height() + 2 * PAD);
        let (mut x, mut y, mut row) = self.at;
        if x + w > PAGE {
            (x, y, row) = (0, y + row, 0);
        }
        if self.pages.is_empty() || y + h > PAGE {
            self.pages.push(surface(PAGE, PAGE));
            (x, y, row) = (0, 0, 0);
        }
        let page = self.pages.len() - 1;
        {
            let c = ctx(&self.pages[page]);
            let _ = c.set_source_surface(&pic, (x + PAD) as f64, (y + PAD) as f64);
            let _ = c.paint();
        }
        self.at = (x + w, y, row.max(h));
        let p = PAGE as f64;
        Spot { page, uv: ((x + PAD) as f64 / p, (y + PAD) as f64 / p, (x + w - PAD) as f64 / p, (y + h - PAD) as f64 / p) }
    }
}

/// A thing standing on the ground, as the woods draw it: its picture, its canvas on the screen (x, y, w,
/// h), its foot's row on the screen, mirrored or not, and how far it bends in the wind (a share of its
/// height at the wind's full strength).
struct Thing {
    pic: Spot,
    canvas: (f64, f64, f64, f64),
    foot: f64,
    mirror: bool,
    bend: f64,
    /// Its shadow: middle, radii, darkness.
    shadow: Option<(f64, f64, f64, f64, f64)>,
}

/// A critter seen in each of its poses at every heading on the ground, from right and clockwise (see
/// `Settings::duck`): each square frame `side` across on the screen, where it is `anchor` of the way down.
struct Turning {
    poses: Vec<Vec<Spot>>,
    side: f64,
    anchor: f64,
}

impl Turning {
    /// The critter at (x, y) on the screen in `pose`, heading the way of `dir` there.
    fn at(&self, (x, y): (f64, f64), dir: (f64, f64), pose: usize) -> Critter {
        let frames = &self.poses[pose.min(self.poses.len() - 1)];
        // back to the ground: the screen halves its depth
        let a = (dir.1 * 2.0).atan2(dir.0).rem_euclid(TAU);
        let pic = frames[(a / TAU * frames.len() as f64).round() as usize % frames.len()];
        Critter { pic, rect: (x - self.side / 2.0, y - self.side * self.anchor, self.side, self.side), mirror: false, angle: 0.0, tint: PLAIN }
    }
}

/// The pictures of the critters and the little things, in the atlas.
#[derive(Default)]
struct Pictures {
    duck: Option<Turning>,
    rabbit: Option<Turning>,
    bird: Option<Turning>,
    fish: Option<(Spot, (f64, f64))>,
    shadow: Spot,
    ring: Spot,
    leaf: Spot,
    mote: Spot,
    butterflies: Vec<Spot>,
}

pub struct Woods {
    fps: f64,
    wind: f64,
    clouds: f64,
    /// The duck's, the rabbit's and the bird's sheets.
    critters: [String; 3],
    fish: String,
    t: f64,
    u: f64,
    pages: Vec<Tex>,
    cloud: Option<Tex>,
    things: Vec<Thing>,
    pics: Pictures,
    river: Option<Line>,
    flow: (f64, f64),
    crossings: Vec<(f64, f64)>,
    /// The crossings' pictures on the screen (x0, y0, x1, y1): ducks and fish keep out of them.
    bridges: Vec<(f64, f64, f64, f64)>,
    flowers: Vec<(f64, f64)>,
    burrows: Vec<(f64, f64)>,
    /// Trees' crowns, where leaves fall from: middle and size.
    crowns: Vec<(f64, f64, f64)>,
    seed: i64,
    /// Every picture's brightness and saturation (Settings).
    grade: (f64, f64),
}

impl Woods {
    pub fn new(st: &Settings) -> Woods {
        Woods {
            fps: st.fps,
            wind: st.wind,
            clouds: st.clouds,
            critters: [st.duck.clone(), st.rabbit.clone(), st.bird.clone()],
            fish: st.fish.clone(),
            grade: (st.brightness, st.saturation),
            t: 0.0,
            u: 1.0,
            pages: vec![],
            cloud: None,
            things: vec![],
            pics: Pictures::default(),
            river: None,
            flow: DOWN,
            crossings: vec![],
            bridges: vec![],
            flowers: vec![],
            burrows: vec![],
            crowns: vec![],
            seed: 1,
        }
    }

    /// How far the wind leans things over at (x, y) now, -1 to 1: gusts rolling across the screen.
    fn gust(&self, x: f64, y: f64) -> f64 {
        let (u, t) = (self.u, self.t);
        let swell = 0.55 + 0.45 * (t * 0.17 + x / u * 0.6).sin();
        swell * (0.65 * (t * 1.15 - (x * 0.9 + y * 0.45) / u * 3.2).sin() + 0.35 * (t * 2.4 - x / u * 7.0 + y / u * 2.0).sin())
    }

    /// A point on the river, `s` along its middle (in pixels from its start), moved `side` across it.
    fn on_river(&self, s: f64, side: f64) -> Option<((f64, f64), (f64, f64))> {
        let r = self.river.as_ref()?;
        let mut left = s.max(0.0);
        for seg in r.pts.windows(2) {
            let ((ax, ay), (bx, by)) = (seg[0], seg[1]);
            let len = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
            if left <= len || len == 0.0 {
                let k = if len > 0.0 { left / len } else { 0.0 };
                let dir = if len > 0.0 { ((bx - ax) / len, (by - ay) / len) } else { (1.0, 0.0) };
                let across = if r.down { UP } else { DOWN };
                return Some(((ax + (bx - ax) * k + across.0 * side, ay + (by - ay) * k + across.1 * side), dir));
            }
            left -= len;
        }
        None
    }

    fn river_length(&self) -> f64 {
        self.river.as_ref().map_or(0.0, |r| r.pts.windows(2).map(|s| ((s[1].0 - s[0].0).powi(2) + (s[1].1 - s[0].1).powi(2)).sqrt()).sum())
    }

    /// A place on the river on the screen and away from its crossings: `s` along it, or none.
    /// Whether a critter's picture (x0, y0, x1, y1) stays off every crossing's.
    fn off_bridges(&self, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
        let m = self.u * 0.01;
        self.bridges.iter().all(|&(a, b, c, d)| x1 + m <= a || x0 - m >= c || y1 + m <= b || y0 - m >= d)
    }

    fn open_water(&self, rng: &mut Rng, w: f64, h: f64) -> Option<f64> {
        let len = self.river_length();
        (0..24).map(|_| rng.range(0.0, len)).find(|&s| {
            self.on_river(s, 0.0).is_some_and(|((x, y), _)| {
                x > w * 0.05
                    && x < w * 0.95
                    && y > h * 0.05
                    && y < h * 0.95
                    && self.crossings.iter().all(|c| ((c.0 - x).powi(2) + ((c.1 - y) * 2.0).powi(2)).sqrt() > self.u * 0.15)
            })
        })
    }
}

/// Whether the stretch from `lo` to `hi` along the river is clear of every one `taken`.
fn apart(taken: &[(f64, f64)], lo: f64, hi: f64) -> bool {
    taken.iter().all(|&(a, b)| hi <= a || lo >= b)
}

/// A picture at rest from the atlas: loaded, scaled to `k` of its size when that is smaller, put in.
fn packed(atlas: &mut Atlas, pic: &ImageSurface, k: f64) -> Spot {
    if k < 0.98 { atlas.put(&resampled(pic, k, Filter::Good)) } else { atlas.put(pic) }
}

/// A critter's picture, scaled for its height on the screen: its spot and its size as drawn, at
/// `height` tall.
fn critter(g: &Geo, atlas: &mut Atlas, name: &str, height: f64, grade: (f64, f64)) -> Option<(Spot, (f64, f64))> {
    if name.is_empty() {
        return None;
    }
    let mut s = Sprite::load(g.th.path(name));
    super::forest::graded(&mut s.pic, grade.0, grade.1);
    let k = height / s.size.1.max(1.0);
    Some((packed(atlas, &s.pic, k), (s.size.0 * k, height)))
}

/// Where on a racetrack `s` along it is: two straight runs `run` long, `turn` either side of the middle,
/// joined by half circles; as (along, across) from the middle.
fn racetrack(run: f64, turn: f64, s: f64) -> (f64, f64) {
    let bend = PI * turn;
    let s = s.rem_euclid(2.0 * (run + bend));
    let half = run / 2.0;
    if s < run {
        (s - half, -turn)
    } else if s < run + bend {
        let a = (s - run) / turn;
        (half + turn * a.sin(), -turn * a.cos())
    } else if s < 2.0 * run + bend {
        (half - (s - run - bend), turn)
    } else {
        let a = (s - 2.0 * run - bend) / turn;
        (-half - turn * a.sin(), turn * a.cos())
    }
}

/// A turning critter's sheet of `poses` rows, its frames `side` across on the screen.
fn turning(g: &Geo, atlas: &mut Atlas, name: &str, poses: i32, side: f64, anchor: f64, grade: (f64, f64)) -> Option<Turning> {
    if name.is_empty() {
        return None;
    }
    let mut sheet = load_png(g.th.path(name));
    super::forest::graded(&mut sheet, grade.0, grade.1);
    let f = sheet.height() / poses;
    let n = (sheet.width() / f.max(1)) as usize;
    if f < 1 || n < 1 {
        return None;
    }
    let mut frame = |row: i32, i: usize| {
        let one = surface(f, f);
        let c = ctx(&one);
        let _ = c.set_source_surface(&sheet, -(i as f64) * f as f64, -(row * f) as f64);
        let _ = c.paint();
        drop(c);
        packed(atlas, &one, side / f as f64)
    };
    let poses = (0..poses).map(|row| (0..n).map(|i| frame(row, i)).collect()).collect();
    Some(Turning { poses, side, anchor })
}

/// A little picture drawn by `paint` on a canvas `w` by `h`.
fn drawn(atlas: &mut Atlas, w: i32, h: i32, paint: impl Fn(&Context, f64, f64)) -> Spot {
    let s = surface(w, h);
    paint(&ctx(&s), w as f64, h as f64);
    atlas.put(&s)
}

/// A soft round blot, darkest (or brightest) in the middle.
fn blot(c: &Context, w: f64, h: f64, rgb: (f64, f64, f64)) {
    let g = RadialGradient::new(w / 2.0, h / 2.0, 0.0, w / 2.0, h / 2.0, w / 2.0);
    g.add_color_stop_rgba(0.0, rgb.0, rgb.1, rgb.2, 1.0);
    g.add_color_stop_rgba(0.55, rgb.0, rgb.1, rgb.2, 0.6);
    g.add_color_stop_rgba(1.0, rgb.0, rgb.1, rgb.2, 0.0);
    let _ = c.set_source(&g);
    let _ = c.paint();
}

/// Smooth random values over a square that tiles, 0 to 1: three octaves.
fn clouds_tile(rng: &mut Rng, side: usize, cover: f64) -> ImageSurface {
    let octaves: Vec<(usize, Vec<f64>)> = [6usize, 12, 24].iter().map(|&n| (n, (0..n * n).map(|_| rng.f64()).collect())).collect();
    let mut s = surface(side as i32, side as i32);
    let stride = s.stride() as usize;
    if let Some(mut px) = view(&mut s) {
        for y in 0..side {
            for x in 0..side {
                let mut v = 0.0;
                for (i, (n, grid)) in octaves.iter().enumerate() {
                    let (fx, fy) = (x as f64 * *n as f64 / side as f64, y as f64 * *n as f64 / side as f64);
                    let (ix, iy) = (fx as usize, fy as usize);
                    let (tx, ty) = (smooth(fx - ix as f64), smooth(fy - iy as f64));
                    let at = |i: usize, j: usize| grid[(j % n) * n + i % n];
                    let a = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
                    let b = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
                    v += (a * (1.0 - ty) + b * ty) * [0.6, 0.28, 0.12][i];
                }
                let a = smooth((v - (0.62 - cover * 0.3)) / 0.18);
                px[y * stride + x * 4 + 3] = (a * 255.0) as u8;
            }
        }
    }
    s
}

/// Quads gathered for the renderer, a draw for each run of one texture and blend.
struct Batch {
    tex: Option<Tex>,
    blend: Blend,
    verts: Vec<f32>,
}

impl Batch {
    fn new() -> Batch {
        Batch { tex: None, blend: Blend::Premultiplied, verts: vec![] }
    }
    fn flush(&mut self, gfx: &mut Gfx) {
        if !self.verts.is_empty() {
            gfx.draw(self.tex, &self.verts, self.blend);
            self.verts.clear();
        }
    }
    /// A quad of a picture: its corners top-left, top-right, bottom-right, bottom-left; tinted
    /// (premultiplied: the colour times the alpha).
    fn quad(&mut self, gfx: &mut Gfx, tex: Tex, blend: Blend, corners: [(f64, f64); 4], uv: (f64, f64, f64, f64), tint: [f64; 4]) {
        if self.tex != Some(tex) || self.blend != blend {
            self.flush(gfx);
            (self.tex, self.blend) = (Some(tex), blend);
        }
        let (u0, v0, u1, v1) = uv;
        let c = tint.map(|v| v as f32);
        for ((x, y), (u, v)) in corners.into_iter().zip([(u0, v0), (u1, v0), (u1, v1), (u0, v1)]) {
            self.verts.extend_from_slice(&[u as f32, v as f32, c[0], c[1], c[2], c[3], x as f32, y as f32]);
        }
    }
    /// A picture upright in the rectangle (x, y, w, h), mirrored or not, turned `angle` about its middle.
    fn image(&mut self, gfx: &mut Gfx, pages: &[Tex], s: Spot, rect: (f64, f64, f64, f64), mirror: bool, angle: f64, tint: [f64; 4], blend: Blend) {
        let (x, y, w, h) = rect;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (sin, cos) = angle.sin_cos();
        let turn = |dx: f64, dy: f64| (cx + dx * cos - dy * sin, cy + dx * sin + dy * cos);
        let corners = [turn(-w / 2.0, -h / 2.0), turn(w / 2.0, -h / 2.0), turn(w / 2.0, h / 2.0), turn(-w / 2.0, h / 2.0)];
        let (u0, v0, u1, v1) = s.uv;
        let uv = if mirror { (u1, v0, u0, v1) } else { (u0, v0, u1, v1) };
        self.quad(gfx, pages[s.page], blend, corners, uv, tint);
    }
}

/// Plain: the picture as it is.
const PLAIN: [f64; 4] = [1.0, 1.0, 1.0, 1.0];

/// Faded: the picture times `a`.
fn faded(a: f64) -> [f64; 4] {
    [a, a, a, a]
}

/// A shadow of a picture's shape, `a` dark.
fn shade(a: f64) -> [f64; 4] {
    [0.0, 0.0, 0.0, a]
}

/// A number from 0 to 1 that is the same for the same `keys`.
fn hashed(keys: &[i64]) -> f64 {
    let mut r = Rng::new(keys.iter().fold(0x9e37_79b9_i64, |h, &k| h.wrapping_mul(1_000_003).wrapping_add(k)));
    r.f64()
}

impl Part for Woods {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn rest(&mut self, _g: &Geo, c: &Context, s: &mut Shared) {
        let Some(stand) = s.get(&STAND) else { return };
        for p in &stand.placed {
            shadow(c, &stand.sprites[p.pic], p);
        }
        for p in &stand.placed {
            draw(c, &stand.sprites[p.pic], p);
        }
    }
    fn textures(&mut self, g: &Geo, s: &Shared, up: &mut Uploads) {
        let Some(stand) = s.get::<Stand>(&STAND) else { return };
        let u = super::forest::unit_on(g);
        self.u = u;
        let mut atlas = Atlas::default();
        // each picture once, at the largest it is drawn
        let mut largest = vec![0.0f64; stand.sprites.len()];
        for p in &stand.placed {
            let (k, _, _) = spot(&stand.sprites[p.pic], p);
            largest[p.pic] = largest[p.pic].max(k);
        }
        let spots: Vec<Option<Spot>> = stand.sprites.iter().zip(&largest).map(|(s, &k)| (k > 0.0).then(|| packed(&mut atlas, &s.pic, k))).collect();
        self.things = stand
            .placed
            .iter()
            .map(|p| {
                let s = &stand.sprites[p.pic];
                let (k, x, y) = spot(s, p);
                let bend = match p.sort {
                    Sort::Tree => 1.0,
                    Sort::Low => 2.6,
                    Sort::Bank => 3.6,
                    _ => 0.0,
                };
                Thing {
                    pic: spots[p.pic].unwrap_or_default(),
                    canvas: (x, y, s.size.0 * k, s.size.1 * k),
                    foot: p.y,
                    mirror: p.mirror,
                    bend,
                    shadow: shadow_at(s, p),
                }
            })
            .collect();
        self.crowns = stand.placed.iter().filter(|p| p.sort == Sort::Tree).map(|p| (p.x, p.y - p.height * 0.62, p.height)).collect();
        // the critters and the little things
        let [duck, rabbit, bird] = &self.critters;
        self.pics.duck = turning(g, &mut atlas, duck, 1, u * 0.055, 0.62, self.grade);
        self.pics.rabbit = turning(g, &mut atlas, rabbit, 3, u * 0.042, 0.72, self.grade);
        self.pics.bird = turning(g, &mut atlas, bird, 3, u * 0.06, 0.5, self.grade);
        // a fish a fifth of the river's width long, whatever the river
        self.pics.fish = critter(g, &mut atlas, &self.fish, stand.river.as_ref().map_or(u * 0.075, |r| r.width) * 0.22, self.grade);
        self.pics.shadow = drawn(&mut atlas, 64, 64, |c, w, h| blot(c, w, h, (0.0, 0.0, 0.0)));
        self.pics.mote = drawn(&mut atlas, 32, 32, |c, w, h| blot(c, w, h, (1.0, 0.95, 0.78)));
        self.pics.ring = drawn(&mut atlas, 128, 64, |c, w, h| {
            c.scale(w / 2.0, h / 2.0);
            c.arc(1.0, 1.0, 0.85, 0.0, TAU);
            c.set_line_width(0.08);
            c.set_source_rgba(0.85, 0.92, 0.9, 0.8);
            let _ = c.stroke();
        });
        self.pics.leaf = drawn(&mut atlas, 32, 32, |c, w, h| {
            c.translate(w / 2.0, h / 2.0);
            c.rotate(-0.6);
            c.move_to(-w * 0.42, 0.0);
            c.curve_to(-w * 0.15, -h * 0.32, w * 0.25, -h * 0.25, w * 0.42, 0.0);
            c.curve_to(w * 0.25, h * 0.25, -w * 0.15, h * 0.32, -w * 0.42, 0.0);
            c.set_source_rgb(0.62, 0.62, 0.2);
            let _ = c.fill_preserve();
            c.set_source_rgba(0.35, 0.33, 0.1, 0.8);
            c.set_line_width(1.0);
            let _ = c.stroke();
        });
        self.pics.butterflies = [(0.96, 0.94, 0.86), (0.98, 0.85, 0.3), (0.9, 0.55, 0.2), (0.55, 0.7, 0.95)]
            .into_iter()
            .map(|(r, gg, b)| {
                drawn(&mut atlas, 48, 32, move |c, w, h| {
                    for side in [-1.0, 1.0] {
                        c.save().ok();
                        c.translate(w / 2.0, h / 2.0);
                        c.scale(side, 1.0);
                        c.move_to(0.0, 0.0);
                        c.curve_to(w * 0.2, -h * 0.55, w * 0.48, -h * 0.4, w * 0.42, -h * 0.05);
                        c.curve_to(w * 0.4, h * 0.2, w * 0.2, h * 0.45, 0.0, h * 0.1);
                        c.close_path();
                        c.set_source_rgb(r, gg, b);
                        let _ = c.fill();
                        c.restore().ok();
                    }
                    c.set_source_rgb(0.15, 0.12, 0.08);
                    c.rectangle(w / 2.0 - 1.0, h * 0.2, 2.0, h * 0.6);
                    let _ = c.fill();
                })
            })
            .collect();
        self.pages = atlas.pages.into_iter().map(|p| up.add(p, Kind::Sprite)).collect();
        let mut rng = Rng::new(stand.seed ^ 0x5eed);
        if self.clouds > 0.0 {
            self.cloud = Some(up.add(clouds_tile(&mut rng, 256, self.clouds), Kind::Tile));
        }
        self.river = stand.river.as_ref().map(|r| Line { pts: r.pts.clone(), width: r.width, down: r.down });
        self.flow = stand.flow;
        self.crossings = stand.crossings.clone();
        self.bridges = stand
            .placed
            .iter()
            .filter(|p| p.sort == Sort::Bridge)
            .map(|p| {
                let sp = &stand.sprites[p.pic];
                let (k, x, y) = spot(sp, p);
                (x, y, x + sp.size.0 * k, y + sp.size.1 * k)
            })
            .collect();
        self.flowers = stand.flowers.clone();
        self.burrows = stand.burrows.clone();
        self.seed = stand.seed;
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.t = (w.clock * self.fps).round() / self.fps;
        self.t.to_bits()
    }
    fn interval(&self) -> f64 {
        1.0 / self.fps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if self.pages.is_empty() {
            return;
        }
        let (w, h, u, t) = (g.w as f64, g.h as f64, self.u, self.t);
        let pages = self.pages.clone();
        let mut b = Batch::new();
        // on the water: rings where the fish leaps and in the ducks' wakes
        let mut ground: Vec<(f64, Vec<Critter>)> = vec![];
        self.water_life(&mut b, gfx, &pages, &mut ground, w, h);
        // shadows, then what stands, back to front, critters among it
        for th in &self.things {
            if let Some((cx, cy, rx, ry, dark)) = th.shadow {
                b.image(gfx, &pages, self.pics.shadow, (cx - rx, cy - ry, rx * 2.0, ry * 2.0), false, 0.0, shade(dark), Blend::Premultiplied);
            }
        }
        self.land_life(&mut ground);
        ground.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut extra = ground.into_iter().peekable();
        for th in &self.things {
            while let Some((_, parts)) = extra.next_if(|e| e.0 <= th.foot) {
                for p in parts {
                    p.put(&mut b, gfx, &pages);
                }
            }
            self.thing(&mut b, gfx, &pages, th);
        }
        for (_, parts) in extra {
            for p in parts {
                p.put(&mut b, gfx, &pages);
            }
        }
        // over it: butterflies, falling leaves, the crows' shadows, the clouds', the crows, the light
        self.butterflies(&mut b, gfx, &pages);
        self.leaves(&mut b, gfx, &pages, w, h);
        let flock = self.flock(w, h);
        for c in &flock {
            let (x, y, cw, ch) = c.rect;
            Critter { rect: (x - u * 0.12, y + u * 0.22, cw, ch), tint: shade(0.22), ..*c }.put(&mut b, gfx, &pages);
        }
        b.flush(gfx);
        if let Some(cloud) = self.cloud {
            // the clouds' shadows, drawn out along the ground and drifting with the wind
            let scale = u * 1.6;
            let (du, dv) = (t * 0.012, t * 0.004);
            let uv = (du, dv, w / (scale * 2.0) + du, h / scale + dv);
            b.quad(gfx, cloud, Blend::Premultiplied, [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], uv, shade(0.3));
            b.flush(gfx);
        }
        for c in &flock {
            c.put(&mut b, gfx, &pages);
        }
        b.flush(gfx);
        self.motes(&mut b, gfx, &pages, w, h);
        b.flush(gfx);
    }
}

/// A critter's picture where it is now, to draw among the things in the order of where it stands.
struct Critter {
    pic: Spot,
    rect: (f64, f64, f64, f64),
    mirror: bool,
    angle: f64,
    tint: [f64; 4],
}

impl Critter {
    fn put(&self, b: &mut Batch, gfx: &mut Gfx, pages: &[Tex]) {
        b.image(gfx, pages, self.pic, self.rect, self.mirror, self.angle, self.tint, Blend::Premultiplied);
    }
}

impl Woods {
    /// A thing, bent by the wind from its foot up: in strips, each leaning further than the one below.
    fn thing(&self, b: &mut Batch, gfx: &mut Gfx, pages: &[Tex], th: &Thing) {
        let (x, y, w, h) = th.canvas;
        let (u0, v0, u1, v1) = th.pic.uv;
        let (ul, ur) = if th.mirror { (u1, u0) } else { (u0, u1) };
        let tex = pages[th.pic.page];
        if th.bend == 0.0 || self.wind == 0.0 {
            b.quad(gfx, tex, Blend::Premultiplied, [(x, y), (x + w, y), (x + w, y + h), (x, y + h)], (ul, v0, ur, v1), PLAIN);
            return;
        }
        let tall = (th.foot - y).max(1.0);
        let lean = self.gust(x + w / 2.0, th.foot) * self.wind * th.bend * tall;
        let at = |yy: f64| lean * ((th.foot - yy) / tall).clamp(0.0, 1.0).powf(1.6);
        let rows = [y, y + tall / 3.0, y + tall * 2.0 / 3.0, th.foot, y + h];
        for r in rows.windows(2) {
            let (ya, yb) = (r[0], r[1].max(r[0]));
            if yb <= ya {
                continue;
            }
            let (va, vb) = (v0 + (v1 - v0) * (ya - y) / h, v0 + (v1 - v0) * (yb - y) / h);
            let (la, lb) = (at(ya), at(yb));
            b.quad(gfx, tex, Blend::Premultiplied, [(x + la, ya), (x + w + la, ya), (x + w + lb, yb), (x + lb, yb)], (ul, va, ur, vb), PLAIN);
        }
    }

    /// Ducks paddling to and fro in open water, a ring of their wake spreading
    /// behind them; and a fish leaping now and then, rings where it leaves the water and where it falls
    /// back. Rings are drawn at once; the ducks and the fish go among the things.
    fn water_life(&self, b: &mut Batch, gfx: &mut Gfx, pages: &[Tex], ground: &mut Vec<(f64, Vec<Critter>)>, w: f64, h: f64) {
        let (Some(r), u, t) = (&self.river, self.u, self.t) else { return };
        let mut rng = Rng::new(self.seed ^ 0xd0c);
        let ring = |b: &mut Batch, gfx: &mut Gfx, (x, y): (f64, f64), age: f64, size: f64| {
            if (0.0..1.0).contains(&age) {
                let (rw, rh) = (size * (0.3 + age), size * (0.3 + age) * 0.5);
                b.image(gfx, pages, self.pics.ring, (x - rw, y - rh, rw * 2.0, rh * 2.0), false, 0.0, faded(0.5 * (1.0 - age)), Blend::Premultiplied);
            }
        };
        if let Some(duck) = &self.pics.duck {
            let count = (self.river_length() / (u * 0.9)).round().clamp(1.0, (3.0 * super::forest::more(w, h, u)).round()) as usize;
            // each round its own stretch of the river, so that no two swim through each other
            let mut taken: Vec<(f64, f64)> = vec![];
            for i in 0..count {
                let found = (0..8).find_map(|_| {
                    let home = self.open_water(&mut rng, w, h)?;
                    let (run, turn) = (u * rng.range(0.05, 0.1), r.width * rng.range(0.14, 0.2));
                    let reach = run / 2.0 + turn + u * 0.02;
                    // its whole loop, its picture at every point of it, off the crossings
                    let side = duck.side;
                    let lap = 2.0 * run + TAU * turn;
                    let clear = (0..32).all(|k| {
                        let (along, across) = racetrack(run, turn, lap * k as f64 / 32.0);
                        self.on_river(home + along, across)
                            .is_some_and(|((x, y), _)| self.off_bridges((x - side / 2.0, y - side * 0.62, x + side / 2.0, y + side * 0.38)))
                    });
                    (clear && apart(&taken, home - reach, home + reach)).then_some((home, run, turn, reach))
                });
                let Some((home, run, turn, reach)) = found else { continue };
                taken.push((home - reach, home + reach));
                let (speed, start) = (u * rng.range(0.005, 0.008) * if rng.f64() < 0.5 { -1.0 } else { 1.0 }, rng.range(0.0, 1.0));
                // round and round a racetrack at an even pace, so it always turns in a steady arc
                let at = |t: f64| {
                    let (along, side) = racetrack(run, turn, speed * t + start * (2.0 * run + TAU * turn));
                    self.on_river(home + along, side).map(|(p, _)| p)
                };
                let (Some((x, y)), Some(next)) = (at(t), at(t + 0.25)) else { continue };
                // the wake: rings left where it was
                for k in 0..3 {
                    let age = (t * 0.9 + k as f64 / 3.0) % 1.0;
                    if let Some(p) = at(t - age * 1.1) {
                        ring(b, gfx, p, age, u * 0.016);
                    }
                }
                let bob = (t * 2.2 + i as f64).sin() * u * 0.0012;
                ground.push((y, vec![duck.at((x, y + bob), (next.0 - x, next.1 - y), 0)]));
            }
        }
        if let Some((pic, (fw, fh))) = self.pics.fish {
            const EVERY: f64 = 9.0;
            let cycle = (t / EVERY).floor();
            let into = t - cycle * EVERY;
            let mut fr = Rng::new(self.seed ^ (cycle as i64).wrapping_mul(7919));
            let (jump, along) = (0.75, fh * 1.2);
            // a leap whose arc and both rings keep off the crossings, of a few tried
            let leap = (0..8).find_map(|_| {
                let s = self.open_water(&mut fr, w, h)?;
                let ((x, y), dir) = self.on_river(s, r.width * fr.range(-0.2, 0.2))?;
                let end = (x + dir.0 * along, y + dir.1 * along);
                let clear = (0..=8).all(|i| {
                    let k = i as f64 / 8.0;
                    let (px, py) = (x + (end.0 - x) * k, y + (end.1 - y) * k - (k * PI).sin() * fh);
                    self.off_bridges((px - fw.max(fh), py - fh, px + fw.max(fh), py + fh))
                });
                clear.then_some(((x, y), dir, end))
            });
            if let Some(((x, y), dir, end)) = leap {
                ring(b, gfx, (x, y), into / 1.4, fh);
                ring(b, gfx, end, (into - jump) / 1.4, fh);
                if into < jump {
                    let k = into / jump;
                    let (px, py) = (x + (end.0 - x) * k, y + (end.1 - y) * k - (k * PI).sin() * fh);
                    let angle = (0.5 - k) * 1.6 * if dir.0 < 0.0 { -1.0 } else { 1.0 };
                    ground.push((y, vec![Critter { pic, rect: (px - fw / 2.0, py - fh / 2.0, fw, fh), mirror: dir.0 < 0.0, angle, tint: PLAIN }]));
                }
            }
        }
    }

    /// Rabbits: at a burrow (or in the open, where there is none) out a while, hopping about and
    /// nibbling, then back in; to go among the things.
    fn land_life(&self, ground: &mut Vec<(f64, Vec<Critter>)>) {
        let Some(rabbit) = &self.pics.rabbit else { return };
        let (u, t) = (self.u, self.t);
        let homes: Vec<(f64, f64)> =
            if self.burrows.is_empty() { self.flowers.iter().take(2).copied().collect() } else { self.burrows.iter().take(3).copied().collect() };
        for (i, &(hx, hy)) in homes.iter().enumerate() {
            const EVERY: f64 = 26.0;
            let tt = t + i as f64 * 9.7;
            let cycle = (tt / EVERY).floor() as i64;
            let into = tt - cycle as f64 * EVERY;
            // out from 4 s to 20 s of each cycle: to three spots near home and back, a hop between each
            if !(4.0..20.0).contains(&into) {
                continue;
            }
            let stops: Vec<(f64, f64)> = (0..5)
                .map(|k| {
                    if k == 0 || k == 4 {
                        (hx, hy)
                    } else {
                        let a = hashed(&[self.seed, i as i64, cycle, k]) * TAU;
                        let d = u * (0.03 + 0.04 * hashed(&[self.seed, i as i64, cycle, k, 1]));
                        (hx + a.cos() * d, hy + a.sin() * d * 0.5)
                    }
                })
                .collect();
            let leg = 16.0 / 4.0;
            let k = ((into - 4.0) / leg).floor() as usize;
            let within = into - 4.0 - k as f64 * leg;
            let (from, to) = (stops[k.min(3)], stops[(k + 1).min(4)]);
            const HOP: f64 = 0.9;
            // poses: sitting, hopping, nibbling
            let (x, y, lift, pose) = if within < HOP {
                // hopping: three small hops over the leg, stretched out in the air
                let f = within / HOP;
                let hop = (f * 3.0).fract();
                (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f, (hop * PI).sin() * u * 0.008, if (0.15..0.85).contains(&hop) { 1 } else { 0 })
            } else {
                let nibbling = ((within * 1.7).sin() > 0.2) as usize * 2;
                (to.0, to.1, 0.0, nibbling)
            };
            ground.push((y, vec![rabbit.at((x, y - lift), (to.0 - from.0, to.1 - from.1), pose)]));
        }
    }

    /// Butterflies fluttering round the flowers (or the open ground), their wings beating.
    fn butterflies(&self, b: &mut Batch, gfx: &mut Gfx, pages: &[Tex]) {
        if self.pics.butterflies.is_empty() {
            return;
        }
        let (u, t) = (self.u, self.t);
        for (i, &(fx, fy)) in self.flowers.iter().take(7).enumerate() {
            let ph = hashed(&[self.seed, i as i64, 3]) * TAU;
            let (x, y) = (fx + u * 0.035 * (t * 0.55 + ph).sin() + u * 0.012 * (t * 1.9 + ph).sin(), fy - u * 0.02 + u * 0.012 * (t * 0.8 + ph * 2.0).sin());
            let beat = 0.25 + 0.75 * (t * 15.0 + ph).sin().abs();
            // larger than life, to be seen at all, but small beside the flowers: 8 x 5 at 1080 rows
            let (bw, bh) = (u * 0.0075 * beat, u * 0.005);
            let pic = self.pics.butterflies[i % self.pics.butterflies.len()];
            b.image(gfx, pages, pic, (x - bw / 2.0, y - bh / 2.0, bw, bh), false, 0.0, PLAIN, Blend::Premultiplied);
        }
    }

    /// Leaves falling from the trees' crowns, swaying and turning, one at a time from each of a few.
    fn leaves(&self, b: &mut Batch, gfx: &mut Gfx, pages: &[Tex], w: f64, h: f64) {
        if self.crowns.is_empty() {
            return;
        }
        let (u, t) = (self.u, self.t);
        for i in 0..(12.0 * super::forest::more(w, h, u)).round() as i64 {
            let every = 6.0 + (i % 4) as f64;
            let tt = t + i as f64 * 1.37;
            let cycle = (tt / every).floor() as i64;
            let f = (tt - cycle as f64 * every) / every;
            let (cx, cy, size) = self.crowns[(hashed(&[self.seed, i, cycle]) * self.crowns.len() as f64) as usize % self.crowns.len()];
            let start = (cx + (hashed(&[self.seed, i, cycle, 1]) - 0.5) * size * 0.5, cy);
            let fall = size * 0.55 * f;
            let (x, y) = (start.0 + (f * 9.0).sin() * u * 0.01 + f * u * 0.03, start.1 + fall);
            let turn = (f * 14.0).cos();
            let a = (f * 6.0).min(1.0) * ((1.0 - f) * 4.0).min(1.0);
            let s = u * 0.008;
            b.image(
                gfx,
                pages,
                self.pics.leaf,
                (x - s * turn.abs() / 2.0, y - s / 2.0, s * turn.abs().max(0.15), s),
                turn < 0.0,
                f * 3.0,
                faded(a),
                Blend::Premultiplied,
            );
        }
    }

    /// The crows flying over now and then, a few together.
    fn flock(&self, w: f64, h: f64) -> Vec<Critter> {
        let Some(bird) = &self.pics.bird else { return vec![] };
        let (u, t) = (self.u, self.t);
        const EVERY: f64 = 120.0;
        const ACROSS: f64 = 14.0;
        let cycle = (t / EVERY).floor() as i64;
        let into = t - cycle as f64 * EVERY;
        if into > ACROSS {
            return vec![];
        }
        let r = |k: i64| hashed(&[self.seed, cycle, k, 5]);
        // from somewhere beyond one side of the screen to beyond the other, slightly downhill or up
        let rightward = r(0) < 0.5;
        let (y0, y1) = (h * (0.1 + 0.8 * r(1)), h * (0.1 + 0.8 * r(2)));
        let (x0, x1) = if rightward { (-u * 0.2, w + u * 0.2) } else { (w + u * 0.2, -u * 0.2) };
        let f = into / ACROSS;
        let (cx, cy) = (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
        let n = 3 + (r(3) * 4.0) as i64;
        (0..n)
            .map(|k| {
                // a loose V behind the leader
                let row = ((k + 1) / 2) as f64;
                let side = if k % 2 == 0 { 1.0 } else { -1.0 } * row;
                let back = row * u * 0.05 * if rightward { -1.0 } else { 1.0 };
                let (x, y) = (cx + back + (t * 1.3 + k as f64).sin() * u * 0.004, cy + side * u * 0.035 + (t * 1.1 + k as f64 * 2.0).sin() * u * 0.004);
                // poses: wings up, level, down
                let beat = (t * 7.0 + k as f64 * 1.9).sin();
                // a glide now and then, a second at most, each bird at its own time
                let gliding = ((t * 0.8 + k as f64 * 2.1).sin()) > 0.92;
                let pose = if gliding || beat.abs() < 0.35 {
                    1
                } else if beat > 0.0 {
                    0
                } else {
                    2
                };
                bird.at((x, y), (x1 - x0, y1 - y0), pose)
            })
            .collect()
    }

    /// Dust drifting in the light, glinting.
    fn motes(&self, b: &mut Batch, gfx: &mut Gfx, pages: &[Tex], w: f64, h: f64) {
        let (u, t) = (self.u, self.t);
        for i in 0..(36.0 * super::forest::more(w, h, u)).round() as i64 {
            let (ax, ay) = (hashed(&[self.seed, i, 7]) * w, hashed(&[self.seed, i, 8]) * h);
            let ph = hashed(&[self.seed, i, 9]) * TAU;
            let (x, y) = (ax + u * 0.03 * (t * 0.13 + ph).sin(), ay + u * 0.02 * (t * 0.17 + ph * 1.3).sin() - (t * u * 0.002) % (u * 0.05));
            let a = 0.35 * (0.5 + 0.5 * (t * 1.3 + ph).sin()).powi(3);
            let s = u * 0.006;
            b.image(gfx, pages, self.pics.mote, (x - s / 2.0, y - s / 2.0, s, s), false, 0.0, faded(a), Blend::Add);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ducks_keep_to_stretches_of_their_own() {
        let taken = [(10.0, 20.0), (40.0, 55.0)];
        assert!(super::apart(&taken, 20.0, 40.0));
        assert!(!super::apart(&taken, 15.0, 30.0));
        assert!(!super::apart(&taken, 0.0, 100.0));
        assert!(super::apart(&[], 0.0, 1.0));
    }

    use super::*;

    #[test]
    fn a_racetrack_turns_evenly_and_never_jumps() {
        let (run, turn) = (80.0, 6.0);
        let lap = 2.0 * (run + PI * turn);
        let step = 0.05;
        let heading = |s: f64| {
            let (a, b) = (racetrack(run, turn, s), racetrack(run, turn, s + step));
            (b.1 - a.1).atan2(b.0 - a.0)
        };
        // round to where it began
        let (a, b) = (racetrack(run, turn, 0.0), racetrack(run, turn, lap));
        assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
        let mut s = -lap;
        while s < lap {
            let (p, q) = (racetrack(run, turn, s), racetrack(run, turn, s + step));
            // an even pace: each step as long as the last
            assert!(((q.0 - p.0).hypot(q.1 - p.1) - step).abs() < 1e-3, "jumps at {s}");
            // never sharper than the bend: no turning on the spot
            let d = (heading(s + step) - heading(s) + PI).rem_euclid(TAU) - PI;
            assert!(d.abs() <= step / turn + 1e-6, "turns {d} at {s}");
            s += step;
        }
    }
}
