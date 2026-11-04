//! forest: an isometric wood, laid out anew each start (or by `seed`) from pictures beside the theme.
//! The ground, its paths and a river are seamless textures seen from above, turned to the isometric
//! angle and blended by noise, with leaves, moss and flowers flat on it; that is the still painting.
//! What stands on it (trees, undergrowth, rocks, clutter, the crossings) is laid out here and drawn by
//! the woods (live, `woods.rs`), which move in the wind with critters among them. The river's water flows
//! (live): a shader moves the picture under it, within a mask of where the water shows.
//!
//! Isometric: the ground's two axes run down-right and up-right on the screen at 2:1, so a river or a
//! path follows one of them, wandering to its sides along the other.

use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Extend, Filter, ImageSurface, Matrix, RadialGradient, SurfacePattern};
use gl::types::{GLint, GLuint};
use serde::Deserialize;
use std::f64::consts::{FRAC_PI_4, TAU};
use toml::Table;

/// The water's mask, which the river's live part takes from the painting.
const WATER: Key<Water> = Key::new("forest-water");
/// What stands on the ground, which the woods take from the painting.
pub const STAND: Key<Stand> = Key::new("forest-stand");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    ground: String,
    path: String,
    bed: String,
    trees: Vec<String>,
    undergrowth: Vec<String>,
    stones: String,
    /// Placed now and then, in the open: logs, stumps, boulders.
    scattered: Vec<String>,
    /// In small groups, in the open and in the woods.
    rocks: Vec<String>,
    /// Small things on the forest floor, thickest in the woods: mushrooms, burrows, branches.
    clutter: Vec<String>,
    /// Set only in clearings, out from under the trees, by a path where there is one: a campfire.
    camps: Vec<String>,
    /// Flat on the ground, faded into it at their edges: leaves, moss, flowers, puddles.
    decals: Vec<String>,
    /// Flat on the paths: footprints.
    tracks: Vec<String>,
    /// Along the river's banks: reeds, rushes.
    banks: Vec<String>,
    /// Where a path crosses the river, each crossing a different one: their pictures run up and right.
    bridges: Vec<Bridge>,
    /// The critters. The duck, the rabbit and the bird are sheets: square frames of the critter as the
    /// forest is seen, one row per pose with a frame for every heading on the ground (from right,
    /// clockwise). The duck swims (one row), its waterline's middle 62% down each frame; the rabbit
    /// sits, hops and nibbles, its feet 72% down; the bird flies wings up, level and down, its middle in
    /// the middle. The fish is one picture, leaping right. Each left empty leaves that critter out.
    pub duck: String,
    pub rabbit: String,
    pub bird: String,
    pub fish: String,
    /// How far the wind bends the trees, as a share of their height.
    pub wind: f64,
    /// How much of the ground the clouds' shadows cover, 0 to 1.
    pub clouds: f64,
    /// 0: a new forest each start (one the same every run while the time is held: --demo, --snapshot).
    seed: i64,
    /// A texture's side, in UNITs (a 1080-row screen's shorter side).
    tile: f64,
    /// Trees' heights, in UNITs.
    tree: [f64; 2],
    /// Trees per square UNIT, where the wood is thickest.
    density: f64,
    river: f64,
    path_width: f64,
    water: Colour,
    mud: Colour,
    pub fps: f64,
    /// Every picture's brightness and saturation, 1 as painted: below, darker and duller.
    pub brightness: f64,
    pub saturation: f64,
}

/// A crossing's picture, and the two ends of its middle line on the ground, in the picture's pixels: left
/// then right, where the path runs on and off it. It is set across the river to reach dry ground both
/// ways, its line on the path's.
#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    pub image: String,
    pub ends: [[f64; 2]; 2],
}

impl Validate for Settings {
    fn validate(&self) -> Result<(), String> {
        for b in &self.bridges {
            let [[x0, y0], [x1, y1]] = b.ends;
            if !(x1 > x0 && y1 < y0) {
                return Err(format!("bridges: {}: its ends run up and right, left end first", b.image));
            }
        }
        if self.trees.is_empty() {
            return Err("trees: it names at least one picture".into());
        }
        within("tile", self.tile, 0.05, 4.0)?;
        within("tree", self.tree[0], 0.02, 1.0)?;
        within("tree", self.tree[1], self.tree[0], 1.0)?;
        within("density", self.density, 0.0, 400.0)?;
        within("river", self.river, 0.0, 0.5)?;
        within("path_width", self.path_width, 0.0, 0.2)?;
        within("wind", self.wind, 0.0, 0.1)?;
        within("clouds", self.clouds, 0.0, 1.0)?;
        within("brightness", self.brightness, 0.3, 1.5)?;
        within("saturation", self.saturation, 0.0, 2.0)?;
        within("fps", self.fps, 1.0, 60.0)
    }
}

/// The forest's unit of size, in millimetres on the screen: everything is as large on every monitor,
/// whatever its pixels. A larger screen shows more of the forest; a denser one (retina), the same.
/// It is the height of a 14-inch laptop's panel.
pub const UNIT_MM: f64 = 174.0;

/// The unit in pixels on a 1080-row 14-inch panel: what the tests lay a forest out at.
pub const UNIT: f64 = 1080.0;

/// The unit on this monitor, in its pixels.
pub fn unit_on(g: &Geo) -> f64 {
    UNIT_MM * g.px_per_mm
}

/// How far a river or a path swings to either side in its long meander, in UNITs.
const MEANDER: f64 = 0.18;

/// How much more screen there is than a 1920 x 1080 one, in units `u`, so that what is counted per
/// screen keeps its thickness: 1 or more.
pub fn more(w: f64, h: f64, u: f64) -> f64 {
    (w * h / (u * u * 16.0 / 9.0)).max(1.0)
}

// --- the ground's axes -------------------------------------------------------------------------------

/// The isometric axes on the screen: down-right and up-right, at 2:1.
pub const DOWN: (f64, f64) = (0.894_427_190_999_915_9, 0.447_213_595_499_957_9);
pub const UP: (f64, f64) = (0.894_427_190_999_915_9, -0.447_213_595_499_957_9);

/// A river or a path: its middle line, and how wide it is.
pub struct Line {
    pub pts: Vec<(f64, f64)>,
    pub width: f64,
    /// Along DOWN (else UP).
    pub down: bool,
}

impl Line {
    /// Across the screen along one axis through a point in `within` (fractions of the screen: x0, x1,
    /// y0, y1), wandering to the sides by `amp`, `width` wide, give or take a tenth. Its bends are of
    /// a size in units (`u`), not in screens: one long, wide meander and smaller wiggles on it, so that over a
    /// larger screen it bends more often, never one long straight line.
    #[allow(clippy::too_many_arguments)]
    fn wander(rng: &mut Rng, down: bool, within: [f64; 4], w: f64, h: f64, amp: f64, width: f64, u: f64) -> Line {
        let (x, y) = (w * rng.range(within[0], within[1]), h * rng.range(within[2], within[3]));
        let width = width * rng.range(0.9, 1.1);
        let (dir, side) = if down { (DOWN, UP) } else { (UP, DOWN) };
        let reach = (w * w + h * h).sqrt() * 0.75;
        // (frequency, phase, amplitude): the wiggles, `amp` across all told, then the meander
        let mut waves: Vec<(f64, f64, f64)> =
            (0..3).map(|i| (TAU / (rng.range(0.7, 1.3) * u * (1.2 - 0.3 * i as f64)), rng.range(0.0, TAU), amp / (1.0 + 2.0 * i as f64) / 1.83)).collect();
        waves.push((TAU / (u * rng.range(1.8, 2.6)), rng.range(0.0, TAU), u * MEANDER));
        let n = ((2.0 * reach / (u * 0.02)).ceil() as usize).max(160);
        let pts = (0..=n)
            .map(|i| {
                let t = -reach + 2.0 * reach * i as f64 / n as f64;
                let off = waves.iter().map(|(f, p, a)| a * (t * f + p).sin()).sum::<f64>();
                (x + dir.0 * t + side.0 * off, y + dir.1 * t + side.1 * off)
            })
            .collect();
        Line { pts, width, down }
    }

    /// How far (x, y) is from its middle line.
    pub fn distance(&self, x: f64, y: f64) -> f64 {
        self.pts
            .windows(2)
            .map(|s| {
                let ((ax, ay), (bx, by)) = (s[0], s[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
            })
            .fold(f64::MAX, f64::min)
    }

    /// Run straight along `dir` (near its axis) through `at` for `reach` either way, easing back into
    /// its wander over as far again: where a bridge, which cannot bend, carries it.
    fn straighten(&mut self, at: (f64, f64), dir: (f64, f64), reach: f64) {
        for p in &mut self.pts {
            let t = (p.0 - at.0) * dir.0 + (p.1 - at.1) * dir.1;
            let k = 1.0 - smooth((t.abs() - reach) / reach);
            *p = (p.0 + (at.0 + dir.0 * t - p.0) * k, p.1 + (at.1 + dir.1 * t - p.1) * k);
        }
    }

    /// Where it crosses `other`, near enough.
    fn crossing(&self, other: &Line) -> Option<(f64, f64)> {
        self.pts.iter().map(|&(x, y)| (other.distance(x, y), x, y)).min_by(|a, b| a.0.total_cmp(&b.0)).filter(|c| c.0 < other.width).map(|c| (c.1, c.2))
    }
}

/// Smooth random values over the screen: a lattice `cell` pixels apart, eased between.
struct Lattice {
    cell: f64,
    cols: usize,
    v: Vec<f64>,
}

impl Lattice {
    fn new(rng: &mut Rng, w: f64, h: f64, cell: f64) -> Lattice {
        let cols = (w / cell).ceil() as usize + 2;
        let rows = (h / cell).ceil() as usize + 2;
        Lattice { cell, cols, v: (0..cols * rows).map(|_| rng.f64()).collect() }
    }
    fn at(&self, x: f64, y: f64) -> f64 {
        let rows = self.v.len() / self.cols;
        let (fx, fy) = ((x / self.cell).clamp(0.0, self.cols as f64 - 1.001), (y / self.cell).clamp(0.0, rows as f64 - 1.001));
        let (ix, iy) = (fx as usize, fy as usize);
        let (tx, ty) = (smooth(fx - ix as f64), smooth(fy - iy as f64));
        let v = |i: usize, j: usize| self.v[j.min(rows - 1) * self.cols + i.min(self.cols - 1)];
        (v(ix, iy) * (1.0 - tx) + v(ix + 1, iy) * tx) * (1.0 - ty) + (v(ix, iy + 1) * (1.0 - tx) + v(ix + 1, iy + 1) * tx) * ty
    }
}

/// Noise for frayed edges: a coarse wander and a fine one.
struct Fray(Lattice, Lattice);

impl Fray {
    fn new(rng: &mut Rng, w: f64, h: f64, u: f64) -> Fray {
        Fray(Lattice::new(rng, w, h, u * 0.09), Lattice::new(rng, w, h, u * 0.022))
    }
    /// -0.5 to 0.5.
    fn at(&self, x: f64, y: f64) -> f64 {
        self.0.at(x, y) * 0.6 + self.1.at(x, y) * 0.4 - 0.5
    }
}

/// The scale the lines' masks are worked out at, before they are scaled up smoothly.
const FIELD: f64 = 0.25;

/// How far each pixel of a picture at FIELD of the size is from the nearest of the lines, in their
/// half-widths (`widen` times their width), moved in and out by `rag` of that with the fray: its width,
/// height and the distances.
fn field(lines: &[&Line], w: i32, h: i32, widen: f64, reach: f64, rag: f64, fray: &Fray) -> (usize, usize, Vec<f64>) {
    let k = FIELD;
    let (sw, sh) = (((w as f64 * k).ceil() as usize).max(1), ((h as f64 * k).ceil() as usize).max(1));
    let mut near = vec![f64::MAX; sw * sh];
    for l in lines {
        let half = l.width * widen * 0.5 * k;
        let reach = half * (reach + rag);
        for seg in l.pts.windows(2) {
            let ((ax, ay), (bx, by)) = ((seg[0].0 * k, seg[0].1 * k), (seg[1].0 * k, seg[1].1 * k));
            let (dx, dy) = (bx - ax, by - ay);
            let len = dx * dx + dy * dy;
            let (x0, x1) = ((ax.min(bx) - reach).floor().max(0.0) as usize, ((ax.max(bx) + reach).ceil().max(0.0) as usize).min(sw));
            let (y0, y1) = ((ay.min(by) - reach).floor().max(0.0) as usize, ((ay.max(by) + reach).ceil().max(0.0) as usize).min(sh));
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let t = if len > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
                    let d = ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt() / half;
                    let n = &mut near[y * sw + x];
                    *n = n.min(d);
                }
            }
        }
    }
    for y in 0..sh {
        for x in 0..sw {
            near[y * sw + x] += rag * fray.at(x as f64 / k, y as f64 / k);
        }
    }
    (sw, sh, near)
}

/// A picture at FIELD of the size, scaled up smoothly to the screen.
fn scaled_up(small: &ImageSurface) -> SurfacePattern {
    let p = SurfacePattern::create(small);
    p.set_filter(Filter::Good);
    p.set_matrix(Matrix::new(FIELD, 0.0, 0.0, FIELD, 0.0, 0.0));
    p
}

/// A mask of the lines, `widen` times their width: solid in the middle, fading over `edge` of the
/// half-width to each side, its edge moved in and out by `rag` of the half-width with the fray.
fn frayed(lines: &[&Line], w: i32, h: i32, widen: f64, edge: f64, rag: f64, fray: &Fray) -> SurfacePattern {
    let (sw, sh, near) = field(lines, w, h, widen, 1.0 + edge, rag, fray);
    let mut small = surface(sw as i32, sh as i32);
    let stride = small.stride() as usize;
    if let Some(mut px) = view(&mut small) {
        for y in 0..sh {
            for x in 0..sw {
                let a = 1.0 - smooth((near[y * sw + x] - (1.0 - edge)) / (2.0 * edge).max(1e-6));
                px[y * stride + x * 4 + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
    }
    scaled_up(&small)
}

/// The water over the whole picture: where it shows (alpha) and how deep it is there (red, times the
/// alpha), from nothing at its edge to all in its middle.
fn water_mask(river: &Line, w: i32, h: i32, fray: &Fray) -> ImageSurface {
    const EDGE: f64 = 0.45;
    let (sw, sh, near) = field(&[river], w, h, WATER_WIDEN, 1.0 + EDGE, 0.4, fray);
    let mut small = surface(sw as i32, sh as i32);
    let stride = small.stride() as usize;
    if let Some(mut px) = view(&mut small) {
        for y in 0..sh {
            for x in 0..sw {
                let d = near[y * sw + x];
                let a = (1.0 - smooth((d - (1.0 - EDGE)) / (2.0 * EDGE))).clamp(0.0, 1.0);
                let deep = smooth((1.0 - d) / 0.9).clamp(0.0, 1.0);
                let at = y * stride + x * 4;
                px[at + 3] = (a * 255.0) as u8;
                px[at + 2] = (a * deep * 255.0) as u8;
            }
        }
    }
    let full = surface(w, h);
    let c = ctx(&full);
    let _ = c.set_source(scaled_up(&small));
    let _ = c.paint();
    drop(c);
    full
}

/// The water mask's depth as an alpha mask of its own, at most `DEEP`.
fn depth_alpha(water: &mut ImageSurface) -> ImageSurface {
    const DEEP: f64 = 0.55;
    let (w, h, stride) = (water.width(), water.height(), water.stride() as usize);
    let mut out = surface(w, h);
    let os = out.stride() as usize;
    if let (Some(src), Some(mut dst)) = (view(water).map(|d| d.to_vec()), view(&mut out)) {
        for y in 0..h as usize {
            for x in 0..w as usize {
                let d = src[y * stride + x * 4 + 2] as f64 / 255.0;
                dst[y * os + x * 4 + 3] = (d * d * DEEP * 255.0) as u8;
            }
        }
    }
    out
}

/// How much wider than the river's line its water is.
const WATER_WIDEN: f64 = 1.0;

/// Paint `c`'s source through `mask` at `alpha`.
fn mask_with(c: &Context, mask: &SurfacePattern, alpha: f64) {
    c.push_group();
    let _ = c.mask(mask);
    let _ = c.pop_group_to_source();
    let _ = c.paint_with_alpha(alpha);
}

/// A texture seen from above, laid on the ground: turned 45 degrees, half as tall, `side` pixels a tile.
fn ground_pattern(tex: &ImageSurface, side: f64, ox: f64, oy: f64) -> SurfacePattern {
    let p = SurfacePattern::create(tex);
    p.set_extend(Extend::Repeat);
    p.set_filter(Filter::Good);
    let k = side / tex.width().max(1) as f64;
    let mut m = Matrix::identity();
    m.translate(ox, oy);
    m.scale(k, k * 0.5);
    m.rotate(FRAC_PI_4);
    p.set_matrix(m.try_invert().unwrap_or(Matrix::identity()));
    p
}

/// Smooth noise over the screen, 0 to 1 as alpha: random values `cell` pixels apart, scaled up.
fn noise(rng: &mut Rng, w: i32, h: i32, cell: f64, lo: f64, hi: f64) -> SurfacePattern {
    let (nw, nh) = ((w as f64 / cell).ceil() as i32 + 2, (h as f64 / cell).ceil() as i32 + 2);
    let mut s = surface(nw, nh);
    let stride = s.stride() as usize;
    if let Some(mut px) = view(&mut s) {
        for y in 0..nh as usize {
            for x in 0..nw as usize {
                px[y * stride + x * 4 + 3] = (smooth((rng.f64() - lo) / (hi - lo)) * 255.0) as u8;
            }
        }
    }
    let p = SurfacePattern::create(&s);
    p.set_filter(Filter::Good);
    p.set_matrix(Matrix::new(1.0 / cell, 0.0, 0.0, 1.0 / cell, 0.5, 0.5));
    p
}

// --- the things standing on the ground ---------------------------------------------------------------

/// A thing's picture standing on the ground, its name (its file's), its size, and where its foot is on
/// it (the middle of its lowest rows).
pub struct Sprite {
    pub pic: ImageSurface,
    pub name: String,
    pub size: (f64, f64),
    pub foot: (f64, f64),
}

impl Sprite {
    pub fn load(path: std::path::PathBuf) -> Sprite {
        let name = path.file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let mut pic = load_png(&path);
        let (w, h, stride) = (pic.width() as usize, pic.height() as usize, pic.stride() as usize);
        let mut foot = (w as f64 / 2.0, h as f64);
        if let Some(px) = view(&mut pic) {
            let opaque = |y: usize| (0..w).any(|x| px[y * stride + x * 4 + 3] > 128);
            let bottom = (0..h).rev().find(|&y| opaque(y)).map_or(h, |y| y + 1);
            let (mut sum, mut n) = (0.0, 0.0);
            for y in bottom.saturating_sub((bottom / 25).max(1))..bottom {
                for x in 0..w {
                    let a = px[y * stride + x * 4 + 3] as f64;
                    sum += a * x as f64;
                    n += a;
                }
            }
            foot = (if n > 0.0 { sum / n } else { foot.0 }, bottom as f64);
        }
        Sprite { pic, name, size: (w as f64, h as f64), foot }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Sort {
    Tree,
    Low,
    Stone,
    Scattered,
    Rock,
    Clutter,
    /// Flat on the ground: drawn first, faded at its edges, no shadow.
    Decal,
    /// Footprints on a path: flat as a decal, only the prints of the picture, faint.
    Track,
    Bank,
    Bridge,
}

/// One thing placed: which sprite, its foot on the screen, its height there, mirrored or not.
pub struct Placed {
    pub sort: Sort,
    pub pic: usize,
    pub x: f64,
    pub y: f64,
    pub height: f64,
    pub mirror: bool,
}

/// What stands on the forest's ground, for the woods to draw, and what the critters need to know of it.
pub struct Stand {
    pub sprites: Vec<Sprite>,
    /// Back to front.
    pub placed: Vec<Placed>,
    pub river: Option<Line>,
    /// Downstream, on the screen.
    pub flow: (f64, f64),
    /// Where the paths cross the river.
    pub crossings: Vec<(f64, f64)>,
    /// Flowers, for the butterflies; burrows and dens, for the rabbits.
    pub flowers: Vec<(f64, f64)>,
    pub burrows: Vec<(f64, f64)>,
    pub seed: i64,
}

/// The forest's sprites, and which are which.
struct Pictures {
    sprites: Vec<Sprite>,
    trees: std::ops::Range<usize>,
    low: std::ops::Range<usize>,
    stones: usize,
    scattered: std::ops::Range<usize>,
    rocks: std::ops::Range<usize>,
    clutter: std::ops::Range<usize>,
    camps: std::ops::Range<usize>,
    decals: std::ops::Range<usize>,
    tracks: std::ops::Range<usize>,
    banks: std::ops::Range<usize>,
    bridges: std::ops::Range<usize>,
    /// Each bridge's ends (Bridge::ends), in the order of `bridges`.
    ends: Vec<[[f64; 2]; 2]>,
    /// The trees by species, from their file names (oak1.png and oak-ancient.png are oaks).
    species: Vec<Vec<usize>>,
}

impl Pictures {
    fn load(g: &Geo, st: &Settings) -> Pictures {
        let mut sprites = vec![];
        let mut add = |names: &[String]| {
            let at = sprites.len();
            sprites.extend(names.iter().map(|n| Sprite::load(g.th.path(n))));
            at..sprites.len()
        };
        let trees = add(&st.trees);
        let low = add(&st.undergrowth);
        let stones = add(std::slice::from_ref(&st.stones)).start;
        let scattered = add(&st.scattered);
        let rocks = add(&st.rocks);
        let clutter = add(&st.clutter);
        let camps = add(&st.camps);
        let decals = add(&st.decals);
        let tracks = add(&st.tracks);
        let banks = add(&st.banks);
        let bridges = add(&st.bridges.iter().map(|b| b.image.clone()).collect::<Vec<_>>());
        let ends = st.bridges.iter().map(|b| b.ends).collect();
        let mut species: Vec<(String, Vec<usize>)> = vec![];
        for (i, n) in st.trees.iter().enumerate() {
            let stem = std::path::Path::new(n).file_stem().map_or(String::new(), |s| s.to_string_lossy().into_owned());
            let name: String = stem.split('-').next().unwrap_or_default().trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
            match species.iter_mut().find(|s| s.0 == name) {
                Some(s) => s.1.push(trees.start + i),
                None => species.push((name, vec![trees.start + i])),
            }
        }
        let species = species.into_iter().map(|s| s.1).collect();
        let mut sprites = sprites;
        for s in &mut sprites {
            graded(&mut s.pic, st.brightness, st.saturation);
        }
        for s in &mut sprites[tracks.clone()] {
            prints_only(&mut s.pic);
        }
        Pictures { sprites, trees, low, stones, scattered, rocks, clutter, camps, decals, tracks, banks, bridges, ends, species }
    }
}

/// A picture graded: its colours times `brightness`, drawn towards their grey by 1 - `saturation`.
pub fn graded(pic: &mut ImageSurface, brightness: f64, saturation: f64) {
    if brightness == 1.0 && saturation == 1.0 {
        return;
    }
    let Some(mut px) = view(pic) else { return };
    for p in px.as_chunks_mut::<4>().0 {
        // premultiplied BGRA: grading the premultiplied colours grades the colours
        let (b, g, r) = (p[0] as f64, p[1] as f64, p[2] as f64);
        let grey = 0.11 * b + 0.59 * g + 0.3 * r;
        let a = p[3] as f64;
        for (c, v) in [b, g, r].into_iter().enumerate() {
            p[c] = ((grey + (v - grey) * saturation) * brightness).clamp(0.0, a).round() as u8;
        }
    }
}

/// Keep only the prints of a footprints picture, which comes on a patch of its own ground: the pixels
/// well darker than the patch's middle brightness, and brown rather than green (not the grass).
fn prints_only(pic: &mut ImageSurface) {
    let stride = pic.stride() as usize;
    let (w, h) = (pic.width() as usize, pic.height() as usize);
    let Some(mut px) = view(pic) else { return };
    // premultiplied BGRA: each opaque pixel's brightness, and the middle one
    let lum = |p: &[u8]| (0.11 * p[0] as f64 + 0.59 * p[1] as f64 + 0.3 * p[2] as f64) / p[3].max(1) as f64;
    let mut all: Vec<f64> = (0..h).flat_map(|y| (0..w).map(move |x| y * stride + x * 4)).filter(|&i| px[i + 3] > 128).map(|i| lum(&px[i..i + 4])).collect();
    if all.is_empty() {
        return;
    }
    let mid = all.len() / 2;
    let m = *all.select_nth_unstable_by(mid, f64::total_cmp).1;
    for y in 0..h {
        for x in 0..w {
            let i = y * stride + x * 4;
            let (g, r) = (px[i + 1] as f64, px[i + 2] as f64);
            let k = smooth((m * 0.6 - lum(&px[i..i + 4])) / (m * 0.12)) * smooth((r - g * 0.85) / (g * 0.1 + 1.0));
            for c in 0..4 {
                px[i + c] = (px[i + c] as f64 * k).round() as u8;
            }
        }
    }
}

/// The copies of each picture placed so far, where and how tall, so that copies stay apart.
struct Copies(Vec<Vec<(f64, f64, f64)>>);

impl Copies {
    /// Whether a copy of `pic`, `h` tall at (x, y), would be near another: within `near` on the ground,
    /// or within three times that at nearly the same size.
    fn crowd(&self, pic: usize, x: f64, y: f64, h: f64, near: f64) -> bool {
        self.0[pic].iter().any(|&(cx, cy, ch)| {
            let d = ((cx - x).powi(2) + ((cy - y) * 2.0).powi(2)).sqrt();
            d < near || (d < near * 3.0 && (ch / h - 1.0).abs() < 0.15)
        })
    }

    /// One of `choices`, `tall` (a range) at (x, y), kept apart from its copies: the first of a few tries
    /// that is, or none.
    fn choose(&mut self, rng: &mut Rng, choices: &[usize], x: f64, y: f64, tall: (f64, f64), near: f64) -> Option<(usize, f64)> {
        if choices.is_empty() {
            return None;
        }
        for _ in 0..8 {
            let (pic, h) = (choices[rng.below(choices.len() as u64) as usize], rng.range(tall.0, tall.1));
            if !self.crowd(pic, x, y, h, near) {
                self.0[pic].push((x, y, h));
                return Some((pic, h));
            }
        }
        None
    }
}

// --- the painting ------------------------------------------------------------------------------------

/// The forest (still): every piece of it in one layer, and the water's mask left for the river.
struct Forest {
    st: Settings,
}

/// The water's mask (alpha, and its depth in red) over the picture, the rectangle it covers, and which
/// way it flows.
pub struct Water {
    mask: ImageSurface,
    area: Rect,
    flow: (f64, f64),
}

impl Forest {
    fn new(t: &Table) -> Forest {
        Forest { st: settings(t) }
    }
}

impl Part for Forest {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let (w, h, u) = (g.w as f64, g.h as f64, unit_on(g));
        let st = &self.st;
        let mut rng = Rng::new(crate::rng::run_seed(st.seed));
        let side = st.tile * u;
        let (mut ground, mut path, mut bed) = (load_png(g.th.path(&st.ground)), load_png(g.th.path(&st.path)), load_png(g.th.path(&st.bed)));
        for t in [&mut ground, &mut path, &mut bed] {
            graded(t, st.brightness, st.saturation);
        }
        let pics = Pictures::load(g, st);

        // the layout: a river along one axis, paths across it and one along it
        let down = rng.f64() < 0.5;
        let river = (st.river > 0.0).then(|| Line::wander(&mut rng, down, [0.35, 0.65, 0.35, 0.65], w, h, u * 0.07, st.river * u, u));
        let pw = st.path_width * u;
        let mut paths = vec![];
        let mut crossings: Vec<Crossing> = vec![];
        if pw > 0.0 {
            // across the river, each crossing it well away from the others, on a crossing of its own
            let mut unused = pics.bridges.clone().collect::<Vec<usize>>();
            rng.shuffle(&mut unused);
            for _ in 0..rng.int(1, 2) {
                let mut p = Line::wander(&mut rng, !down, [0.2, 0.8, 0.25, 0.75], w, h, u * 0.06, pw, u);
                if let Some(r) = &river
                    && let Some(at) = p.crossing(r)
                {
                    if crossings.iter().any(|c| ((c.at.0 - at.0).powi(2) + (c.at.1 - at.1).powi(2)).sqrt() < u * 0.9) {
                        continue;
                    }
                    // straight on and off the crossing, along its own line
                    let dir = match unused.pop() {
                        Some(pic) => {
                            let c = Crossing::new(&pics, pic, at, p.down, r);
                            let dir = c.dir;
                            crossings.push(c);
                            dir
                        }
                        None => {
                            if p.down {
                                DOWN
                            } else {
                                UP
                            }
                        }
                    };
                    p.straighten(at, dir, r.width * 1.6);
                }
                paths.push(p);
            }
            // one along the river, kept well off it
            for _ in 0..12 {
                let along = Line::wander(&mut rng, down, [0.1, 0.9, 0.1, 0.9], w, h, u * 0.05, pw * 0.9, u);
                if river.as_ref().is_none_or(|r| along.pts.iter().all(|&(x, y)| r.distance(x, y) > u * 0.28)) {
                    paths.push(along);
                    break;
                }
            }
        }

        // the ground: the forest floor, darker and drier in patches
        let _ = c.set_source(ground_pattern(&ground, side, rng.range(0.0, side), rng.range(0.0, side)));
        let _ = c.paint();
        let dry = noise(&mut rng, g.w, g.h, u * 0.22, 0.55, 0.95);
        let _ = c.set_source(ground_pattern(&path, side * 0.8, rng.range(0.0, side), 0.0));
        c.push_group();
        let _ = c.mask(&dry);
        let _ = c.pop_group_to_source();
        let _ = c.paint_with_alpha(0.55);
        let shade = noise(&mut rng, g.w, g.h, u * 0.3, 0.3, 1.0);
        c.set_source_rgba(0.09, 0.12, 0.04, 0.45);
        let _ = c.mask(&shade);
        let sun = noise(&mut rng, g.w, g.h, u * 0.18, 0.6, 1.0);
        c.set_source_rgba(0.95, 0.8, 0.45, 0.16);
        let _ = c.mask(&sun);

        // the paths: worn earth fraying into the grass, the ground a little trodden either side
        let fray = Fray::new(&mut rng, w, h, u);
        let refs: Vec<&Line> = paths.iter().collect();
        if !refs.is_empty() {
            c.set_source_rgba(0.2, 0.15, 0.06, 1.0);
            mask_with(c, &frayed(&refs, g.w, g.h, 1.7, 0.6, 0.7, &fray), 0.14);
            let _ = c.set_source(ground_pattern(&path, side * 0.7, 0.0, 0.0));
            mask_with(c, &frayed(&refs, g.w, g.h, 1.0, 0.4, 0.8, &fray), 0.9);
        }

        // the river: the ground damp along it, a bank of dark wet earth, a stony margin reaching
        // unevenly into it, a wet line where the water starts, and the water: the bed clear through it in
        // the shallows, darker where it is deep
        let mut water = surface(g.w, g.h);
        if let Some(r) = &river {
            c.set_source_rgba(0.05, 0.06, 0.02, 1.0);
            mask_with(c, &frayed(&[r], g.w, g.h, 2.2, 0.7, 0.9, &fray), 0.3);
            let (mr, mg, mb) = st.mud.rgb();
            c.set_source_rgba(mr, mg, mb, 1.0);
            mask_with(c, &frayed(&[r], g.w, g.h, 1.55, 0.45, 0.8, &fray), 0.7);
            let _ = c.set_source(ground_pattern(&bed, side * 0.6, 0.0, 0.0));
            mask_with(c, &frayed(&[r], g.w, g.h, 1.3, 0.3, 0.9, &fray), 1.0);
            c.set_source_rgba(mr * 0.5, mg * 0.5, mb * 0.5, 1.0);
            mask_with(c, &frayed(&[r], g.w, g.h, 1.1, 0.35, 0.5, &fray), 0.45);
            water = water_mask(r, g.w, g.h, &fray);
            // the water's colour, deeper where it is deep: the mask's own depth, as alpha
            let (wr, wg, wb) = st.water.rgb();
            let deep = depth_alpha(&mut water);
            c.set_source_rgba(wr, wg, wb, 1.0);
            mask_with(c, &frayed(&[r], g.w, g.h, WATER_WIDEN, 0.45, 0.4, &fray), 0.35);
            c.set_source_rgba(wr * 0.55, wg * 0.6, wb * 0.65, 1.0);
            let _ = c.mask_surface(&deep, 0.0, 0.0);
        }

        // what stands on it, placed back to front: the flat things painted here, the rest left for the
        // woods
        let placed = place(&mut rng, st, &pics, river.as_ref(), &paths, &crossings, w, h, u);
        let (flat, standing): (Vec<Placed>, Vec<Placed>) = placed.into_iter().partition(|p| matches!(p.sort, Sort::Decal | Sort::Track));
        for p in &flat {
            draw(c, &pics.sprites[p.pic], p);
        }
        let named = |p: &Placed, names: &[&str]| names.iter().any(|n| pics.sprites[p.pic].name.contains(n));
        let flowers = flat.iter().chain(&standing).filter(|p| named(p, &["flower", "clover", "foxglove"])).map(|p| (p.x, p.y)).collect();
        let burrows = standing.iter().filter(|p| named(p, &["burrow", "den"])).map(|p| (p.x, p.y)).collect();
        let crossings = crossings.iter().map(|c| c.middle).collect();
        let flow = if down { DOWN } else { UP };
        let flow = if rng.f64() < 0.5 { flow } else { (-flow.0, -flow.1) };
        if river.is_some() {
            let area = wet_area(&mut water, g.w, g.h);
            s.put(&WATER, Water { mask: water, area, flow });
        }
        s.put(&STAND, Stand { sprites: pics.sprites, placed: standing, river, flow, crossings, flowers, burrows, seed: crate::rng::run_seed(st.seed) });
    }
}

/// The rectangle the water covers, a little wider.
fn wet_area(mask: &mut ImageSurface, w: i32, h: i32) -> Rect {
    let stride = mask.stride() as usize;
    let Some(px) = view(mask) else { return (0.0, 0.0, 0.0, 0.0) };
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if px[y as usize * stride + x as usize * 4 + 3] > 2 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    ((x0 - 2).max(0) as f64, (y0 - 2).max(0) as f64, (x1 + 2).min(w) as f64, (y1 + 2).min(h) as f64)
}

/// Where a path crosses the river: the crossing's picture, mirrored to run down-right on a path along
/// DOWN, its line on the screen (`dir`, left to right), and its ends there, on dry ground either side.
struct Crossing {
    pic: usize,
    mirror: bool,
    at: (f64, f64),
    dir: (f64, f64),
    middle: (f64, f64),
    span: f64,
}

/// How far from the river's middle line the ground is dry enough for a crossing's end, in its widths:
/// past the stony margin (`frayed` at 1.55 of the width, whose half this is), with a little to spare.
const DRY: f64 = 0.85;

impl Crossing {
    /// Crossing `pic` over the river `r` where a path meets it at `at`.
    fn new(pics: &Pictures, pic: usize, at: (f64, f64), mirror: bool, r: &Line) -> Crossing {
        let [[x0, y0], [x1, y1]] = pics.ends[pic - pics.bridges.start];
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt().max(1.0);
        // up-right as drawn; mirrored, it runs down-right
        let dir = if mirror { ((x1 - x0) / len, (y0 - y1) / len) } else { ((x1 - x0) / len, (y1 - y0) / len) };
        // the river's edge both ways along that line, then the middle between
        let out = |sign: f64| {
            let step = (r.width * 0.02).max(0.5);
            let mut t = 0.0;
            while t < r.width * 6.0 && r.distance(at.0 + dir.0 * t * sign, at.1 + dir.1 * t * sign) < r.width * DRY {
                t += step;
            }
            t * sign
        };
        let (back, on) = (out(-1.0), out(1.0));
        let mid = (back + on) / 2.0;
        let middle = (at.0 + dir.0 * mid, at.1 + dir.1 * mid);
        Crossing { pic, mirror, at, dir, middle, span: on - back }
    }

    /// Its picture placed: scaled so its ends lie `span` apart, its ends' middle on the crossing's.
    fn placed(&self, pics: &Pictures) -> Placed {
        let s = &pics.sprites[self.pic];
        let [[x0, y0], [x1, y1]] = pics.ends[self.pic - pics.bridges.start];
        let k = self.span / ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt().max(1.0);
        // the ends' middle in the picture, mirrored with it, and the foot that `spot` measures from
        let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (mx, foot) = if self.mirror { (s.size.0 - mx, s.size.0 - s.foot.0) } else { (mx, s.foot.0) };
        let (x, y) = (self.middle.0 + (foot - mx) * k, self.middle.1 + (s.foot.1 - my) * k);
        Placed { sort: Sort::Bridge, pic: self.pic, x, y, height: s.foot.1 * k, mirror: self.mirror }
    }
}

/// Things placed on the ground, back to front: trees in stands with clearings between, undergrowth
/// round them, stones along the river's banks, a log now and then, and a bridge where a path crosses.
#[allow(clippy::too_many_arguments)]
fn place(rng: &mut Rng, st: &Settings, pics: &Pictures, river: Option<&Line>, paths: &[Line], crossings: &[Crossing], w: f64, h: f64, u: f64) -> Vec<Placed> {
    let mut out = vec![];
    let mut copies = Copies(vec![vec![]; pics.sprites.len()]);
    let all = |r: &std::ops::Range<usize>| r.clone().collect::<Vec<usize>>();
    let clear = |x: f64, y: f64, room: f64| {
        river.is_none_or(|r| r.distance(x, y) > r.width * 0.75 + room) && paths.iter().all(|p| p.distance(x, y) > p.width * 0.6 + room)
    };
    // the stands: a coarse grid of which species grows where, and how thick
    let (gx, gy) = ((w / (u * 0.35)).ceil() as usize + 2, (h / (u * 0.35)).ceil() as usize + 2);
    let thick: Vec<f64> = (0..gx * gy).map(|_| rng.f64()).collect();
    // a species as often as it has pictures: the many-pictured ones make the woods, the others stand out
    let kind: Vec<usize> = (0..gx * gy)
        .map(|_| {
            let tree = pics.trees.start + rng.below(pics.trees.len().max(1) as u64) as usize;
            pics.species.iter().position(|s| s.contains(&tree)).unwrap_or(0)
        })
        .collect();
    let at = |grid: &[f64], x: f64, y: f64| {
        let (fx, fy) = ((x / (u * 0.35)).clamp(0.0, gx as f64 - 1.001), (y / (u * 0.35)).clamp(0.0, gy as f64 - 1.001));
        let (ix, iy) = (fx as usize, fy as usize);
        let (tx, ty) = (smooth(fx - ix as f64), smooth(fy - iy as f64));
        let v = |i: usize, j: usize| grid[j.min(gy - 1) * gx + i.min(gx - 1)];
        (v(ix, iy) * (1.0 - tx) + v(ix + 1, iy) * tx) * (1.0 - ty) + (v(ix, iy + 1) * (1.0 - tx) + v(ix + 1, iy + 1) * tx) * ty
    };
    // trees: mostly the stand's species, now and then any; another species where its copies crowd
    let trees = all(&pics.trees);
    let cell = u / st.density.max(0.01).sqrt();
    let mut y = -u * 0.05;
    while y < h + st.tree[1] * u * 0.8 {
        let mut x = -u * 0.15;
        while x < w + u * 0.15 {
            let (px, py) = (x + rng.range(0.0, cell), y + rng.range(0.0, cell * 0.5));
            // thick woods where the stand is thickest, a tree now and then in the open between, and
            // clearings only where it is thinnest
            let k = at(&thick, px, py);
            let p = 0.12 * smooth((k - 0.06) / 0.1) + 0.88 * smooth((k - 0.28) / 0.4);
            if rng.f64() < p && clear(px, py, u * 0.03) {
                let fx = (px / (u * 0.35)).clamp(0.0, gx as f64 - 1.0) as usize;
                let fy = (py / (u * 0.35)).clamp(0.0, gy as f64 - 1.0) as usize;
                let stand = pics.species.get(kind[fy * gx + fx]).filter(|_| rng.f64() < 0.75).unwrap_or(&trees);
                let tall = (u * st.tree[0], u * st.tree[1]);
                if let Some((pic, height)) = copies.choose(rng, stand, px, py, tall, u * 0.45).or_else(|| copies.choose(rng, &trees, px, py, tall, u * 0.45)) {
                    out.push(Placed { sort: Sort::Tree, pic, x: px, y: py, height, mirror: false });
                }
            }
            x += cell;
        }
        y += cell * 0.5;
    }
    // the trees' feet, ground distance (depth doubled back) from them in shares of their heights; what
    // stands keeps off the trunks, and some things out from under the crowns
    let trunks: Vec<(f64, f64, f64)> = out.iter().map(|p| (p.x, p.y, p.height)).collect();
    let off_trunks = |x: f64, y: f64, share: f64| trunks.iter().all(|&(tx, ty, th)| (tx - x).hypot((ty - y) * 2.0) > th * share);
    // undergrowth, thickest at the stands' edges
    let low = all(&pics.low);
    let cell = cell * 0.8;
    let mut y = -u * 0.02;
    while y < h + u * 0.05 {
        let mut x = -u * 0.05;
        while x < w + u * 0.05 {
            let (px, py) = (x + rng.range(0.0, cell), y + rng.range(0.0, cell * 0.5));
            let t = at(&thick, px, py);
            let p = 0.04 + 0.75 * smooth((t - 0.2) / 0.25) * (1.0 - 0.6 * smooth((t - 0.6) / 0.3));
            if rng.f64() < p
                && clear(px, py, u * 0.01)
                && let Some((pic, height)) = copies.choose(rng, &low, px, py, (u * 0.035, u * 0.065), u * 0.12)
            {
                out.push(Placed { sort: Sort::Low, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
            }
            x += cell;
        }
        y += cell * 0.5;
    }
    // stones along the banks, a few in the water
    if let Some(r) = river {
        let side = if r.down { UP } else { DOWN };
        for (i, &(x, y)) in r.pts.iter().enumerate() {
            if i % 2 == 1 || rng.f64() > 0.55 {
                continue;
            }
            let k = *rng.pick(&[-1.0, 1.0]) * r.width * rng.range(0.35, 0.7);
            let (px, py) = (x + side.0 * k, y + side.1 * k);
            if paths.iter().all(|p| p.distance(px, py) > p.width * 1.5)
                && let Some((pic, height)) = copies.choose(rng, &[pics.stones], px, py, (u * 0.012, u * 0.026), u * 0.04)
            {
                out.push(Placed { sort: Sort::Stone, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
            }
        }
    }
    // logs, stumps and boulders, now and then
    let area = w * h / (u * u);
    let scattered = all(&pics.scattered);
    for _ in 0..rng.int((area * 2.0) as i64, (area * 4.0) as i64) {
        let (px, py) = (rng.range(0.03, 0.97) * w, rng.range(0.05, 0.98) * h);
        if clear(px, py, u * 0.04)
            && off_trunks(px, py, 0.25)
            && let Some((pic, height)) = copies.choose(rng, &scattered, px, py, (u * 0.05, u * 0.08), u * 0.3)
        {
            out.push(Placed { sort: Sort::Scattered, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
        }
    }
    let anywhere = |rng: &mut Rng| (rng.range(-0.02, 1.02) * w, rng.range(0.0, 1.04) * h);
    // rocks, in groups of one to four
    let rocks = all(&pics.rocks);
    for _ in 0..rng.int((area * 2.0) as i64, (area * 4.0) as i64) {
        let (cx, cy) = anywhere(rng);
        for _ in 0..rng.int(1, 4) {
            let (px, py) = (cx + rng.range(-0.06, 0.06) * u, cy + rng.range(-0.03, 0.03) * u);
            if clear(px, py, u * 0.03)
                && off_trunks(px, py, 0.18)
                && let Some((pic, height)) = copies.choose(rng, &rocks, px, py, (u * 0.025, u * 0.07), u * 0.15)
            {
                out.push(Placed { sort: Sort::Rock, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
            }
        }
    }
    // clutter on the forest floor, mostly where the woods are
    let clutter = all(&pics.clutter);
    for _ in 0..(area * 30.0) as i64 {
        let (px, py) = anywhere(rng);
        if rng.f64() < 0.15 + 0.85 * smooth((at(&thick, px, py) - 0.25) / 0.3)
            && clear(px, py, u * 0.015)
            && off_trunks(px, py, 0.15)
            && let Some((pic, height)) = copies.choose(rng, &clutter, px, py, (u * 0.022, u * 0.04), u * 0.2)
        {
            out.push(Placed { sort: Sort::Clutter, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
        }
    }
    // a camp or two, in the open by a path
    let camps = all(&pics.camps);
    let mut wanted = if camps.is_empty() { 0 } else { rng.int(1, 2) };
    for _ in 0..400 {
        if wanted == 0 {
            break;
        }
        let (px, py) = (rng.range(0.05, 0.95) * w, rng.range(0.08, 0.95) * h);
        let by_path = paths.is_empty() || paths.iter().any(|p| p.distance(px, py) < p.width * 0.6 + u * 0.08);
        if by_path
            && clear(px, py, u * 0.03)
            && off_trunks(px, py, 0.6)
            // nor behind one, where its trunk and crown would hide it
            && trunks.iter().all(|&(tx, ty, th)| !(ty > py && ty - py < th && (tx - px).abs() < th * 0.4))
            && let Some((pic, height)) = copies.choose(rng, &camps, px, py, (u * 0.035, u * 0.045), u * 0.5)
        {
            out.push(Placed { sort: Sort::Clutter, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
            wanted -= 1;
        }
    }
    // flat things on the ground, anywhere off the water
    let decals = all(&pics.decals);
    for _ in 0..(area * 22.0) as i64 {
        let (px, py) = anywhere(rng);
        if river.is_none_or(|r| r.distance(px, py) > r.width * 0.9)
            && let Some((pic, height)) = copies.choose(rng, &decals, px, py, (u * 0.03, u * 0.05), u * 0.15)
        {
            // only bare things lie on the paths: twigs, a puddle, earth; flowers, clover, moss and the
            // bright litter of leaves (which reads as a flowerbed there) keep off them
            let grows = !["twigs", "puddle", "earth"].iter().any(|n| pics.sprites[pic].name.contains(n));
            if grows && paths.iter().any(|p| p.distance(px, py) < p.width * 0.5 + height * 1.2) {
                continue;
            }
            out.push(Placed { sort: Sort::Decal, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
        }
    }
    // footprints along the paths, now and then: about one patch in every 1.2 units of path
    let tracks = all(&pics.tracks);
    for p in paths {
        let step = p.pts.windows(2).map(|s| (s[1].0 - s[0].0).hypot(s[1].1 - s[0].1)).sum::<f64>() / (p.pts.len().max(2) - 1) as f64;
        for &(x, y) in &p.pts {
            if rng.f64() > step / (u * 1.2) || river.is_some_and(|r| r.distance(x, y) < r.width * 1.2) {
                continue;
            }
            let k = rng.range(-0.25, 0.25) * p.width;
            let side = if p.down { UP } else { DOWN };
            let (px, py) = (x + side.0 * k, y + side.1 * k);
            // a picture's trail runs up-right; mirrored, down-right, as the path does
            if let Some((pic, height)) = copies.choose(rng, &tracks, px, py, (u * 0.06, u * 0.08), u * 0.12) {
                out.push(Placed { sort: Sort::Track, pic, x: px, y: py, height, mirror: p.down });
            }
        }
    }
    // reeds and rushes along the banks, reaching into the water
    if let Some(r) = river {
        let banks = all(&pics.banks);
        let side = if r.down { UP } else { DOWN };
        for (i, &(x, y)) in r.pts.iter().enumerate() {
            if i % 2 == 0 || rng.f64() > 0.5 {
                continue;
            }
            let k = *rng.pick(&[-1.0, 1.0]) * r.width * rng.range(0.52, 0.8);
            let (px, py) = (x + side.0 * k, y + side.1 * k);
            if paths.iter().all(|p| p.distance(px, py) > p.width * 1.5)
                && let Some((pic, height)) = copies.choose(rng, &banks, px, py, (u * 0.035, u * 0.06), u * 0.1)
            {
                out.push(Placed { sort: Sort::Bank, pic, x: px, y: py, height, mirror: rng.f64() < 0.5 });
            }
        }
    }
    // the crossings, laid out with the paths
    out.extend(crossings.iter().map(|c| c.placed(pics)));
    // nothing stands on a crossing, or in front of it where its picture would hide it
    let crossings: Vec<(f64, f64, f64, f64)> = out
        .iter()
        .filter(|p| p.sort == Sort::Bridge)
        .map(|p| {
            let s = &pics.sprites[p.pic];
            let (k, x, y) = spot(s, p);
            (x, y, x + s.size.0 * k, y + s.size.1 * k)
        })
        .collect();
    out.retain(|p| {
        let s = &pics.sprites[p.pic];
        let (k, x, y) = spot(s, p);
        let (x1, y1) = (x + s.size.0 * k, y + s.size.1 * k);
        p.sort == Sort::Bridge || crossings.iter().all(|c| x >= c.2 || x1 <= c.0 || y >= c.3 || y1 <= c.1 || p.y < c.1 + (c.3 - c.1) * 0.25)
    });
    // flat things first, then the bridge, then back to front
    let layer = |p: &Placed| match p.sort {
        Sort::Decal | Sort::Track => 0,
        Sort::Bridge => 1,
        _ => 2,
    };
    out.sort_by(|a, b| layer(a).cmp(&layer(b)).then(a.y.total_cmp(&b.y)));
    out
}

/// Its picture's scale and its canvas's top-left corner on the screen.
pub fn spot(s: &Sprite, p: &Placed) -> (f64, f64, f64) {
    let k = p.height / s.foot.1.max(1.0);
    let foot = if p.mirror { s.size.0 - s.foot.0 } else { s.foot.0 };
    (k, p.x - foot * k, p.y - s.foot.1 * k)
}

pub fn draw(c: &Context, s: &Sprite, p: &Placed) {
    let (k, x, y) = spot(s, p);
    c.save().ok();
    c.translate(x, y);
    c.scale(k, k);
    if p.mirror {
        c.translate(s.size.0, 0.0);
        c.scale(-1.0, 1.0);
    }
    let pat = SurfacePattern::create(&s.pic);
    pat.set_filter(Filter::Good);
    if matches!(p.sort, Sort::Decal | Sort::Track) {
        // faded into the ground from the middle out, so its own patch of ground leaves no edge; prints
        // faint, pressed into the path
        let a = if p.sort == Sort::Track { 0.85 } else { 1.0 };
        let (cx, cy) = (s.size.0 / 2.0, s.size.1 / 2.0);
        let fade = RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, a);
        fade.add_color_stop_rgba(0.55, 0.0, 0.0, 0.0, a);
        fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        fade.set_matrix(Matrix::new(1.0 / cx, 0.0, 0.0, 1.0 / cy, -1.0, -1.0));
        let _ = c.set_source(&pat);
        let _ = c.mask(&fade);
        c.restore().ok();
        return;
    }
    let _ = c.set_source(&pat);
    let _ = c.paint();
    c.restore().ok();
}

/// Its shadow on the ground, an ellipse: its middle, its radii and how dark it is at the middle. The
/// sun is up and to the right, so it falls down and to the left.
pub fn shadow_at(s: &Sprite, p: &Placed) -> Option<(f64, f64, f64, f64, f64)> {
    if matches!(p.sort, Sort::Bridge | Sort::Decal | Sort::Track) {
        return None;
    }
    let (k, _, _) = spot(s, p);
    let width = s.size.0 * k;
    let (reach, dark) = if p.sort == Sort::Tree { (0.55, 0.42) } else { (0.3, 0.3) };
    let (cx, cy) = (p.x - width * reach * 0.45, p.y + width * reach * 0.12);
    Some((cx, cy, width * (0.35 + reach * 0.4), width * (0.16 + reach * 0.12), dark))
}

pub fn shadow(c: &Context, s: &Sprite, p: &Placed) {
    let Some((cx, cy, rx, ry, dark)) = shadow_at(s, p) else { return };
    c.save().ok();
    c.translate(cx, cy);
    c.scale(1.0, ry / rx);
    let g = RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, rx);
    g.add_color_stop_rgba(0.0, 0.03, 0.04, 0.0, dark);
    g.add_color_stop_rgba(0.6, 0.03, 0.04, 0.0, dark * 0.6);
    g.add_color_stop_rgba(1.0, 0.03, 0.04, 0.0, 0.0);
    let _ = c.set_source(&g);
    c.arc(0.0, 0.0, rx, 0.0, TAU);
    let _ = c.fill();
    c.restore().ok();
}

// --- the flowing water -------------------------------------------------------------------------------

/// The water's motion over the picture at rest: a surface of ripples streaming down the current,
/// faster in the deep middle, that bends the bed seen through it and catches the sun, and foam along the
/// banks.
const FLOW: &str = r#"
uniform vec2 flow; // downstream, a unit vector on the screen
float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
// the surface's height at q (along the current, across it), streaks drawn out along the current
float surface_at(vec2 q, float t) {
    return noise(vec2(q.x * 9.0 - t * 1.1, q.y * 30.0)) * 0.55
         + noise(vec2(q.x * 17.0 - t * 1.7, q.y * 52.0) + 5.2) * 0.3
         + noise(vec2(q.x * 31.0 - t * 2.6, q.y * 90.0) + 9.1) * 0.15;
}
void main() {
    vec2 px = area.xy + uv * area.zw;
    vec2 at = px / resolution;
    vec4 mk = texture2D(tex1, at);
    float m = mk.a;
    if (m < 0.004) { gl_FragColor = vec4(0.0); return; }
    float deep = clamp(mk.r / m, 0.0, 1.0);
    float u = min(resolution.x, resolution.y);
    vec2 q = vec2(dot(px, flow), dot(px, vec2(-flow.y, flow.x))) / u;
    float t = time * mix(0.45, 1.0, deep);
    float e = 0.0015;
    float h0 = surface_at(q, t);
    vec2 slope = vec2(surface_at(q + vec2(e, 0.0), t) - h0, surface_at(q + vec2(0.0, e), t) - h0) / e;
    // the bed, bent by the surface
    vec2 bend = (flow * slope.x + vec2(-flow.y, flow.x) * slope.y) * u * 0.00006 * (0.3 + deep);
    vec3 c = texture2D(tex0, (px + bend) / resolution).rgb;
    c *= 1.0 + (h0 - 0.5) * 0.1;
    // the sun, up and to the right, on the slopes facing it
    vec3 n = normalize(vec3(-slope * 0.02, 1.0));
    float sun = pow(max(dot(n, normalize(vec3(0.45, -0.35, 0.82))), 0.0), 60.0);
    c += vec3(1.0, 0.96, 0.85) * sun * 0.28 * (0.25 + deep);
    // foam along the banks, carried with the current
    float shore = 1.0 - smoothstep(0.0, 0.3, deep);
    float froth = noise(vec2(q.x * 22.0 - t * 1.3, q.y * 70.0)) * 0.6 + noise(vec2(q.x * 40.0 - t * 2.0, q.y * 120.0) + 3.3) * 0.4;
    c = mix(c, vec3(0.82, 0.86, 0.8), shore * smoothstep(0.55, 0.85, froth) * 0.45);
    gl_FragColor = vec4(c * m, m);
}
"#;

/// The river's water (live), over the painting within its mask.
struct River {
    fps: f64,
    t: f64,
    backdrop: Option<Tex>,
    mask: Option<Tex>,
    flow: (f64, f64),
    area: (f64, f64, f64, f64),
    program: Option<Option<(GLuint, [GLint; 6])>>,
}

impl Part for River {
    fn live(&self) -> bool {
        true
    }
    fn backdrop(&self) -> Backdrop {
        Backdrop::Texture
    }
    fn set_backdrop_tex(&mut self, tex: Tex) {
        self.backdrop = Some(tex);
    }
    fn top(&self, _g: &Geo) -> i32 {
        self.area.1 as i32
    }
    fn textures(&mut self, _g: &Geo, s: &Shared, up: &mut Uploads) {
        let Some(water) = s.get(&WATER) else { return };
        self.flow = water.flow;
        self.area = water.area;
        self.mask = Some(up.add(water.mask.clone(), Kind::Layer));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.t = (w.clock * self.fps).round() / self.fps;
        self.t.to_bits()
    }
    fn interval(&self) -> f64 {
        1.0 / self.fps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (Some(backdrop), Some(mask)) = (self.backdrop, self.mask) else { return };
        let (x0, y0, x1, y1) = self.area;
        if x1 <= x0 {
            return;
        }
        let program = self.program.get_or_insert_with(|| match gfx.compile(FLOW) {
            Ok(p) => Some((p, ["time", "resolution", "area", "flow", "tex0", "tex1"].map(|n| gfx.uniform(p, n)))),
            Err(e) => {
                crate::errln!("the river's shader: {e}: the water stands still");
                None
            }
        });
        let Some((prog, loc)) = program else { return };
        let uniforms = [
            (loc[0], 1, [self.t as f32, 0.0, 0.0, 0.0]),
            (loc[1], 2, [g.w as f32, g.h as f32, 0.0, 0.0]),
            (loc[2], 4, [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32]),
            (loc[3], 2, [self.flow.0 as f32, self.flow.1 as f32, 0.0, 0.0]),
        ];
        gfx.custom(*prog, &uniforms, &[(loc[4], backdrop), (loc[5], mask)], x0, y0, x1, y1, Blend::Premultiplied);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "forest",
        defaults: r##"ground = ""
path = ""
bed = ""
trees = []
undergrowth = []
stones = ""
scattered = []
rocks = []
clutter = []
camps = []
decals = []
tracks = []
banks = []
bridges = []
duck = ""
rabbit = ""
bird = ""
fish = ""
wind = 0.012
clouds = 0.35
seed = 0
tile = 0.45
tree = [0.17, 0.27]
density = 55.0
river = 0.075
path_width = 0.035
water = "#3c5650"
mud = "#3a3020"
fps = 30.0
brightness = 1.0
saturation = 1.0"##,
        varies: &["trees", "undergrowth", "scattered", "rocks", "clutter", "camps", "decals", "tracks", "banks", "bridges"],
        check: check::<Settings>,
        make: |_, t, _g| {
            let st: Settings = settings(t);
            let fps = st.fps;
            let mut parts: Vec<Box<dyn Part>> = vec![Box::new(Forest::new(t))];
            if st.river > 0.0 {
                parts.push(Box::new(River { fps, t: 0.0, backdrop: None, mask: None, flow: DOWN, area: (0.0, 0.0, 0.0, 0.0), program: None }));
            }
            parts.push(Box::new(super::woods::Woods::new(&st)));
            Ok(parts)
        },
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Edges, Header};

    /// The bundled forest's settings, pictures and a screen to place them on.
    fn bundled() -> (Settings, Pictures, Geo) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/forest");
        let l = crate::config::resolve(&path.display().to_string(), &crate::config::Config::default(), crate::elements::kind_info, true)
            .unwrap_or_else(|e| panic!("{}", e.0));
        let e = l.plain.scene.iter().find(|e| e.kind == "forest").expect("a forest");
        let st: Settings = settings(&e.settings);
        let g = Geo::new(1920, 1080, "retro", Edges::NONE, Header::NONE, None, l.plain.clone());
        let pics = Pictures::load(&g, &st);
        (st, pics, g)
    }

    #[test]
    fn a_path_runs_straight_over_its_bridge() {
        let (w, h) = (1920.0, 1080.0);
        let mut rng = Rng::new(3);
        let mut p = Line::wander(&mut rng, false, [0.2, 0.8, 0.25, 0.75], w, h, 64.0, 38.0, UNIT);
        let before = p.pts.clone();
        let (at, reach) = (p.pts[80], 120.0);
        p.straighten(at, UP, reach);
        // off the axis through `at`, across it
        let across = |q: (f64, f64)| (q.1 - at.1) * UP.0 - (q.0 - at.0) * UP.1;
        let along = |q: (f64, f64)| (q.0 - at.0) * UP.0 + (q.1 - at.1) * UP.1;
        for (q, was) in p.pts.iter().zip(&before) {
            if along(*q).abs() <= reach {
                assert!(across(*q).abs() < 1e-6, "{q:?} is off the bridge's line");
            } else if along(*was).abs() > reach * 2.0 + 1.0 {
                assert_eq!(q, was, "far from the bridge, it wanders as it did");
            }
        }
        assert!(before.iter().any(|q| along(*q).abs() <= reach && across(*q).abs() > 1.0), "the test path was straight already");
    }

    #[test]
    fn nothing_stands_on_a_trunk_and_camps_are_in_clearings_by_a_path() {
        let (st, pics, g) = bundled();
        let (w, h, u) = (g.w as f64, g.h as f64, UNIT);
        let ground = |a: &Placed, x: f64, y: f64| (a.x - x).hypot((a.y - y) * 2.0);
        let mut camps = 0;
        for seed in 1..8 {
            let mut rng = Rng::new(seed);
            let river = Line::wander(&mut rng, true, [0.35, 0.65, 0.35, 0.65], w, h, u * 0.07, st.river * u, u);
            let paths = [Line::wander(&mut rng, false, [0.2, 0.8, 0.25, 0.75], w, h, u * 0.06, st.path_width * u, u)];
            let placed = place(&mut rng, &st, &pics, Some(&river), &paths, &[], w, h, u);
            let trees: Vec<&Placed> = placed.iter().filter(|p| p.sort == Sort::Tree).collect();
            for p in placed.iter().filter(|p| matches!(p.sort, Sort::Scattered | Sort::Rock | Sort::Clutter)) {
                assert!(trees.iter().all(|t| ground(t, p.x, p.y) > t.height * 0.15), "seed {seed}: {} on a trunk", pics.sprites[p.pic].name);
            }
            for d in placed.iter().filter(|p| p.sort == Sort::Decal) {
                let name = &pics.sprites[d.pic].name;
                if !["twigs", "puddle", "earth"].iter().any(|n| name.contains(n)) {
                    assert!(paths.iter().all(|p| p.distance(d.x, d.y) >= p.width * 0.5 + d.height * 1.2), "seed {seed}: {name} on a path");
                }
            }
            for c in placed.iter().filter(|p| pics.camps.contains(&p.pic)) {
                camps += 1;
                assert!(trees.iter().all(|t| ground(t, c.x, c.y) > t.height * 0.6), "seed {seed}: a camp under a tree");
                assert!(paths.iter().any(|p| p.distance(c.x, c.y) < p.width * 0.6 + u * 0.08), "seed {seed}: a camp off the paths");
            }
        }
        assert!(camps > 0, "no camp in any forest");
    }

    /// Every bundled crossing, wherever a path meets the river either way: its picture's ends on dry
    /// ground both sides, and on the path, which runs straight along its line.
    #[test]
    fn a_crossing_reaches_dry_ground_both_ways_on_its_path() {
        let (st, pics, g) = bundled();
        let (w, h, u) = (g.w as f64, g.h as f64, UNIT);
        let mut seen = 0;
        for seed in 1..16 {
            for down in [false, true] {
                for pic in pics.bridges.clone() {
                    let mut rng = Rng::new(seed);
                    let river = Line::wander(&mut rng, !down, [0.35, 0.65, 0.35, 0.65], w, h, u * 0.07, st.river * u, u);
                    let mut path = Line::wander(&mut rng, down, [0.2, 0.8, 0.25, 0.75], w, h, u * 0.06, st.path_width * u, u);
                    let Some(at) = path.crossing(&river) else { continue };
                    let c = Crossing::new(&pics, pic, at, down, &river);
                    path.straighten(at, c.dir, river.width * 1.6);
                    let (s, p) = (&pics.sprites[pic], c.placed(&pics));
                    let (k, x, y) = spot(s, &p);
                    let name = &s.name;
                    for [ex, ey] in pics.ends[pic - pics.bridges.start] {
                        let e = (x + if down { s.size.0 - ex } else { ex } * k, y + ey * k);
                        assert!(river.distance(e.0, e.1) > river.width * (DRY - 0.05), "seed {seed}: {name}'s end {e:?} is in the river");
                        assert!(path.distance(e.0, e.1) < 1.0, "seed {seed}: {name}'s end {e:?} is off the path");
                    }
                    seen += 1;
                }
            }
        }
        assert!(seen > 20, "only {seen} crossings tried");
    }

    /// A larger screen shows more of the forest, not a bigger one: the trees as tall, and more of them. A
    /// retina screen of the same size shows the same forest, with twice the pixels per unit.
    #[test]
    fn a_larger_screen_shows_more_forest_at_the_same_size() {
        let (st, pics, _) = bundled();
        let trees = |w: f64, h: f64, u: f64| {
            let (mut n, mut tall) = (0.0, 0.0);
            for seed in 1..6 {
                let mut rng = Rng::new(seed);
                let river = Line::wander(&mut rng, true, [0.35, 0.65, 0.35, 0.65], w, h, u * 0.07, st.river * u, u);
                let paths = [Line::wander(&mut rng, false, [0.2, 0.8, 0.25, 0.75], w, h, u * 0.06, st.path_width * u, u)];
                for t in place(&mut rng, &st, &pics, Some(&river), &paths, &[], w, h, u).iter().filter(|p| p.sort == Sort::Tree) {
                    n += 1.0;
                    tall += t.height;
                }
            }
            (n, tall / n)
        };
        let (n, tall) = trees(1920.0, 1080.0, UNIT);
        let (n4, tall4) = trees(3840.0, 2160.0, UNIT);
        assert!((tall4 / tall - 1.0).abs() < 0.1, "trees {tall:.0} px tall on 1920x1080, {tall4:.0} on 3840x2160");
        assert!((3.0..5.0).contains(&(n4 / n)), "{n} trees on 1920x1080, {n4} on four times the screen");
        let (nr, tallr) = trees(3840.0, 2160.0, UNIT * 2.0);
        assert!((tallr / tall / 2.0 - 1.0).abs() < 0.1, "trees {tallr:.0} px tall on a retina 3840x2160, {tall:.0} on 1920x1080");
        assert!((0.75..1.33).contains(&(nr / n)), "{nr} trees on a retina 3840x2160, {n} on 1920x1080");
    }

    /// Over a large screen a river or a path bends to and fro: across its axis it swings well to both
    /// sides, and turns back at least twice, wherever it runs.
    #[test]
    fn rivers_and_paths_keep_bending_on_a_large_screen() {
        let (w, h) = (3840.0, 2160.0);
        for seed in 1..20 {
            for down in [false, true] {
                let l = Line::wander(&mut Rng::new(seed), down, [0.2, 0.8, 0.25, 0.75], w, h, UNIT * 0.06, 40.0, UNIT);
                let dir = if down { DOWN } else { UP };
                let across: Vec<f64> = l.pts.iter().filter(|p| (0.0..w).contains(&p.0) && (0.0..h).contains(&p.1)).map(|p| p.1 * dir.0 - p.0 * dir.1).collect();
                let swing = across.iter().cloned().fold(f64::MIN, f64::max) - across.iter().cloned().fold(f64::MAX, f64::min);
                let turns = across.windows(3).filter(|v| (v[1] - v[0]) * (v[2] - v[1]) < 0.0).count();
                assert!(swing > UNIT * MEANDER, "seed {seed}: it swings only {swing:.0} px across 3840x2160");
                assert!(turns >= 2, "seed {seed}: it turns back only {turns} times across 3840x2160");
            }
        }
    }
}
