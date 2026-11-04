//! The racing theme's element: a car driving on an endless country road, seen from behind, as an
//! old DOS racer on a tube showed it: drawn into a low frame, a few hundred pixels high, and shown
//! through a CRT, its rows beams and its pixels phosphors. The car, the trees and the poles are pixel
//! art. The road comes towards the viewer, bends left and right and climbs and drops over hills ahead;
//! the car wanders a little in its lane (it keeps to the right), turns in the bends, and bumps a little
//! on the road's surface. Trees and light poles pass by. The sky follows the local time of day: the sun
//! and the moon on their arcs, dawn and dusk glowing about the sun, stars at night, when the poles, the
//! tail lights and the headlights are lit.
//!
//! The pictures are SVGs and PNGs beside the theme: the car as fifteen views, from three heights and
//! five angles. The view is a camera that adds up (the projection's constants): a viewer's height over
//! the road, right behind the car, over the middle of the right-hand lane, seeing the car from the
//! height its middle view is drawn from. It follows the car exactly: the road runs straight and level
//! at the car, and bends and hills lie ahead on the road, are seen coming, and are driven through. The
//! car turns only in the road's bends, and shows more of its roof climbing out of a dip and less going
//! over a crest, as the viewer behind it on the road would see it.
//!
//! Each frame the road is walked from the car outwards: for each row of the screen, the depth seen
//! there (nearest first, so a crest hides what lies behind it) and the road's offset across. The sky and
//! the road are drawn by shaders, the road reading that table; everything else is sprites.
//!
//! Everything is a function of the scene's clock and the time of day, so a frame at a given moment is
//! always the same picture.

use crate::elements::{Validate, check, positive, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::svg::{svg_layer, svg_size};
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, ImageSurface, LinearGradient, Operator, RadialGradient};
use gl::types::{GLint, GLuint};
use serde::Deserialize;
use std::f64::consts::PI;
use std::path::PathBuf;
use toml::Table;

// --- the projection -------------------------------------------------------------------------------

// A camera that adds up: it is a viewer's height (VIEWER, in metres) over the road, right behind the
// car, looking along the road; lengths are in those heights. The car is Z_CAR of them ahead of it, so
// it is seen from atan(1 / Z_CAR) = 6 degrees above, the height its middle view is drawn from; the
// lens (FOCAL) keeps the road as wide on the screen as it was with a larger car; the horizon is low on
// the screen.

const CANVAS: (f64, f64) = (1440.0, 900.0);
/// The horizon's row on the canvas.
const HORIZON: f64 = 591.0;
/// The lens: canvas pixels for a thing one viewer's height across, one viewer's height away.
const FOCAL: f64 = 1888.0;
/// The viewer's height over the road, in metres: just over the car's roof.
const VIEWER: f64 = 1.5;
/// The car's rear wheels' depth.
const Z_CAR: f64 = 9.51;
/// The car's width, a real one's 1.97 metres.
const CAR_W: f64 = 1.97 / VIEWER;
/// The road's half width: one lane, the car under two thirds of it.
const ROAD_HALF: f64 = CAR_W / 0.65;
/// The curb's outer edge, in half widths.
const CURB: f64 = 1.055;
/// The nearest depth anything is placed at, under the canvas's foot, and the farthest.
const NEAR: f64 = 4.0;
const FAR: f64 = 110.0;
/// The farthest the road is walked.
const Z_END: f64 = 400.0;

/// Where the road vanishes, across the canvas: right above the car.
const VANISH: f64 = 720.0;
/// Where the viewer is across the road, in half widths from its centre: over the right-hand lane.
const VIEW: f64 = 0.5;

/// Canvas pixels to the road's half width, at depth z: sprites' sizes are in these.
fn scale(z: f64) -> f64 {
    FOCAL * ROAD_HALF / z
}

/// Where a point `x` road half widths from the straight line ahead (right positive) is across the
/// canvas, at depth z.
fn road_x(z: f64, x: f64) -> f64 {
    VANISH + (x - VIEW) * scale(z)
}

/// The row on the canvas of a point at depth z standing `dh` above the level of the road at the car (in
/// the viewer's heights).
fn row_of(z: f64, dh: f64) -> f64 {
    HORIZON + FOCAL * (1.0 - dh) / z
}

/// How near a depth is, as the level road's rows go: 1 at the canvas's foot, 0 at the horizon. For
/// what fades with distance.
fn nearness(z: f64) -> f64 {
    (FOCAL / z / (CANVAS.1 - HORIZON)).clamp(0.0, 1.5)
}

/// The car's width on the canvas.
const CAR_WIDTH: f64 = FOCAL * CAR_W / Z_CAR;
/// Its length, a real one's 4.36 metres: its nose is this far ahead of its rear wheels' depth.
const NOSE: f64 = 4.36 / VIEWER;
/// Its headlamps' distance either side of its middle, in road half widths: four tenths of its width.
const LAMPS: f64 = 0.4 * CAR_W / ROAD_HALF;

// --- the hills --------------------------------------------------------------------------------------

/// Each stretch of road this long holds one hill or dip, or none.
const HILL_CELL: f64 = 220.0;

/// The hill or dip in stretch k: where it starts, its length, and its height (in the viewer's heights
/// over the road; a dip below 0): high and long, so the road is seen climbing and dropping far ahead.
/// None for a level stretch.
fn hill(seed: i64, k: i64) -> Option<(f64, f64, f64)> {
    if k < 1 || hash(seed, k, 81) > 0.75 {
        return None;
    }
    let len = 130.0 + 70.0 * hash(seed, k, 82);
    let at = k as f64 * HILL_CELL + (HILL_CELL - len) * hash(seed, k, 83);
    let high = 2.2 + 2.6 * hash(seed, k, 84);
    Some((at, len, if hash(seed, k, 85) < 0.65 { high } else { -0.75 * high }))
}

/// The road's height at distance s, and its slope: smooth hills and dips, each a raised cosine.
fn height(seed: i64, s: f64) -> (f64, f64) {
    let k = (s / HILL_CELL).floor() as i64;
    match hill(seed, k) {
        Some((at, len, high)) if (at..at + len).contains(&s) => {
            let u = (s - at) / len;
            (high * (1.0 - (2.0 * PI * u).cos()) / 2.0, high * PI / len * (2.0 * PI * u).sin())
        }
        _ => (0.0, 0.0),
    }
}

// --- the road ahead, row by row ------------------------------------------------------------------

/// How many rows the road's table has, over the canvas from ROWS_TOP to its foot: finer than the
/// coarsest grid of pixels.
const ROWS: usize = 512;
/// The highest the road can be seen: a hill ahead rises over the horizon.
const ROWS_TOP: f64 = HORIZON - 260.0;

fn row_height() -> f64 {
    (CANVAS.1 - ROWS_TOP) / ROWS as f64
}

/// What a row of the screen shows: sky (nothing), the land beyond the road's end, or the road at a
/// depth, with its offset across.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Sky,
    Beyond,
    Road { z: f64, x: f64 },
}

/// The road walked from the car to the horizon: each step's depth, row, offset across and the row
/// above which nearer road hides it (a crest), and what each row of the screen shows.
struct Ahead {
    steps: Vec<(f64, f64, f64, f64)>,
    rows: Vec<Row>,
    /// The car's slope against the viewer's, and its turn: what picks its view.
    pitch: f64,
    yaw: f64,
    /// The backdrop's rise on the canvas: the car climbing tilts the view up, and the horizon down.
    rise: f64,
}

impl Ahead {
    fn level() -> Ahead {
        Ahead { steps: vec![], rows: vec![Row::Sky; ROWS], pitch: 0.0, yaw: 0.0, rise: 0.0 }
    }

    /// Where a thing at depth z stands: its row, its offset across, and the row below which the road
    /// in front of it hides it. None beyond the road walked.
    fn at(&self, z: f64) -> Option<(f64, f64, f64)> {
        let i = self.steps.partition_point(|s| s.0 < z);
        if i == 0 || i >= self.steps.len() {
            return None;
        }
        let (a, b) = (self.steps[i - 1], self.steps[i]);
        let t = (z - a.0) / (b.0 - a.0);
        Some((a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t, a.3))
    }
}
/// A number in [0, 1) for a key: the same on every start and every machine.
fn hash(seed: i64, k: i64, salt: u64) -> f64 {
    let mut x = (seed as u64) ^ (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ salt.wrapping_mul(0xd1b5_4a32_d192_ed03);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// A picture `period` canvas pixels wide, repeating, moved `offset` across, over the canvas's width:
/// its pieces as (x0, x1, u0, u1), each within one repeat of the picture.
fn wrapped(period: f64, offset: f64) -> Vec<(f64, f64, f64, f64)> {
    let mut out = vec![];
    let mut x = 0.0;
    while x < CANVAS.0 {
        // rem_euclid of a tiny negative gives the period itself: that is the start of a repeat
        let at = Some((x - offset).rem_euclid(period)).filter(|&a| a < period).unwrap_or(0.0);
        let end = CANVAS.0.min(x + period - at).max(x + 1e-6);
        out.push((x, end, at / period, (at + end - x) / period));
        x = end;
    }
    out
}

/// Smooth value noise in [-1, 1] over x.
fn noise(seed: i64, salt: u64, x: f64) -> f64 {
    let (i, f) = (x.floor() as i64, x - x.floor());
    let (a, b) = (hash(seed, i, salt), hash(seed, i + 1, salt));
    2.0 * (a + (b - a) * smooth(f)) - 1.0
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    smooth(((x - a) / (b - a)).clamp(0.0, 1.0))
}

// --- the drive: the road's bends and the car's place, as functions of time ------------------------

/// The length of each piece of road, in units of depth, and of the ease from one piece's curvature
/// into the next, at the piece's start.
const PIECE: f64 = 60.0;
const EASE: f64 = 25.0;
/// How far the backdrop turns across, in canvas pixels, per unit of the road's turn (its curvature
/// summed along it): as far as the angle it turns, through the lens. A right-hand bend carries the sun,
/// the hills and the clouds to the left, while the car is in it.
const TURN: f64 = FOCAL * ROAD_HALF;
/// How far the backdrop may be turned either way: the sun and the moon stay on the canvas. A bend that
/// would carry it further is not taken; the bends keep it within this, the next going back as often as
/// on, so two right-hand bends can follow a left-hand one.
const TURNED: f64 = 380.0;

/// The curvature `tau` into a piece easing from `from` to `to`, and its sum over that length.
fn piece_at(from: f64, to: f64, tau: f64) -> (f64, f64) {
    let e = EASE;
    if tau >= e {
        (to, from * e + (to - from) * e / 2.0 + to * (tau - e))
    } else {
        // the smoothstep's sum: u^3 - u^4 / 2
        let u = tau / e;
        (from + (to - from) * smooth(u), from * tau + (to - from) * e * (u * u * u - u * u * u * u / 2.0))
    }
}

/// The road, piece by piece along its length: each piece's curvature (right positive), and the
/// backdrop's turn at its end. A bend lies on the road: it is seen far ahead, comes nearer as the car
/// drives on, and turns the backdrop only while the car is in it. A piece's curvature is drawn from its
/// seed and from where the backdrop stands: about half the pieces straight, the rest gently left or
/// right, back towards the middle more often the further it is turned, and never so far as to carry it
/// past TURNED. A piece often runs on as the one before it, so bends and straights are of many lengths.
/// Each piece follows from the one before it, so the road is made once, as far as it has been seen,
/// and kept.
struct Course {
    seed: i64,
    pieces: Vec<(f64, f64)>,
}

impl Course {
    fn new(seed: i64) -> Course {
        Course { seed, pieces: vec![(0.0, 0.0)] }
    }

    /// Piece i: its curvature, and the backdrop's turn at its end (in canvas pixels, right positive).
    fn piece(&mut self, i: usize) -> (f64, f64) {
        while self.pieces.len() <= i {
            let n = self.pieces.len() as i64;
            let (from, turned) = *self.pieces.last().unwrap_or(&(0.0, 0.0));
            let h = |salt| hash(self.seed, n, salt);
            let mut to = if h(11) < 0.35 {
                from
            } else if h(12) < 0.45 {
                0.0
            } else {
                // right with an even chance in the middle, more likely back when turned: the backdrop
                // carried left (by right-hand bends) asks for a left-hand bend
                let right = 0.5 + 0.45 * (turned / TURNED).clamp(-1.0, 1.0);
                let size = 0.0005 + 0.0007 * h(13);
                if h(14) < right { size } else { -size }
            };
            // the turn this piece would leave, and the most the next can add while it eases out of it
            let end = |to: f64| turned - TURN * (piece_at(from, to, PIECE).1 + to * EASE / 2.0);
            if end(to).abs() > TURNED {
                to = 0.0;
            }
            let after = turned - TURN * piece_at(from, to, PIECE).1;
            self.pieces.push((to, after));
        }
        self.pieces[i]
    }

    /// At distance s along the road: the curvature, and the backdrop's turn.
    fn at(&mut self, s: f64) -> (f64, f64) {
        let s = s.max(0.0);
        let i = (s / PIECE).floor() as usize;
        let (from, turned) = if i == 0 { (0.0, 0.0) } else { self.piece(i - 1) };
        let (to, _) = self.piece(i);
        let (c, sum) = piece_at(from, to, s - i as f64 * PIECE);
        (c, turned - TURN * sum)
    }
}

/// The car's place across the road at time t, in half widths from the centre line: the middle of the
/// right-hand lane, wandering slowly a little to either side, never near its edges.
fn lane_at(seed: i64, t: f64) -> f64 {
    VIEW + 0.06 * noise(seed, 31, t / 4.7) + 0.025 * noise(seed, 32, t / 1.9)
}

/// The car's bump at road distance s, in canvas pixels, up: a little, as an old racer's car jiggles.
fn bump_at(seed: i64, s: f64) -> f64 {
    let fine = noise(seed, 41, s * 0.68);
    let coarse = noise(seed, 42, s * 0.18);
    // now and then a pothole: a jolt that dies away within a few tenths of a second
    let (i, f) = ((s * 0.24).floor() as i64, s * 0.24 - (s * 0.24).floor());
    let jolt = if hash(seed, i, 43) < 0.18 { (-f * 10.0).exp() * (f * 34.0).sin() } else { 0.0 };
    0.9 * fine + 0.7 * coarse.abs() + 1.4 * jolt
}

// --- settings --------------------------------------------------------------------------------------

/// The car's views: five heights, low to high, each with seven angles: its nose turned left 6, 4 and 2
/// degrees, straight, and right 2, 4 and 6.
const HEIGHTS: usize = 5;
const ANGLES: usize = 7;
const VIEWS: usize = HEIGHTS * ANGLES;
/// The heights' step, in radians: 2.25 degrees, the middle one (seen on the level) 6.
const HEIGHT_STEP: f64 = 2.25 * PI / 180.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    seed: i64,
    speed: f64,
    cars: Vec<String>,
    car_feet: [f64; HEIGHTS],
    car_middle: f64,
    car_span: f64,
    car_lights: Vec<[f64; 8]>,
    car_pipes: Vec<[f64; 6]>,
    sun: String,
    moon: String,
    stars: String,
    hills: String,
    clouds: Vec<String>,
    trees: Vec<String>,
    pole: String,
    pole_lit: String,
    trees_density: f64,
    pole_spacing: f64,
    pixels: f64,
    colours: f64,
    sky_day: [Colour; 3],
    sky_dawn: [Colour; 3],
    sky_dusk: [Colour; 3],
    sky_night: [Colour; 3],
    glow_dawn: Colour,
    glow_dusk: Colour,
    hills_day: [Colour; 2],
    hills_dusk: [Colour; 2],
    hills_night: [Colour; 2],
    ground_day: [Colour; 2],
    ground_night: [Colour; 2],
    road: [Colour; 4],
    light_day: Colour,
    light_dusk: Colour,
    light_night: Colour,
    headlights: Colour,
    lamps: Colour,
}

impl Validate for Settings {
    fn validate(&self) -> Result<(), String> {
        within("speed", self.speed, 0.0, 20.0)?;
        within("trees_density", self.trees_density, 0.0, 1.0)?;
        positive("pole_spacing", self.pole_spacing)?;
        within("pole_spacing", self.pole_spacing, 0.3, 40.0)?;
        within("pixels", self.pixels, 0.0, 4320.0)?;
        within("colours", self.colours, 2.0, 256.0)?;
        within("car_span", self.car_span, 1.0, 10000.0)?;
        within("car_middle", self.car_middle, 0.0, 100000.0)?;
        for (i, &f) in self.car_feet.iter().enumerate() {
            within(&format!("car_feet[{i}]"), f, 0.0, 100000.0)?;
        }
        if self.cars.len() != VIEWS {
            return Err(format!("cars has {}: it is the car's {VIEWS} views, five heights of seven angles", self.cars.len()));
        }
        if self.car_pipes.len() != VIEWS {
            return Err(format!("car_pipes has {}: the tail pipes' middles in each of the car's {VIEWS} views", self.car_pipes.len()));
        }
        if self.car_lights.len() != VIEWS {
            return Err(format!("car_lights has {}: the tail lights' places in each of the car's {VIEWS} views", self.car_lights.len()));
        }
        for (name, files) in [("clouds", &self.clouds), ("trees", &self.trees)] {
            if files.len() > 16 {
                return Err(format!("{name} has {}: up to 16", files.len()));
            }
        }
        Ok(())
    }
}

// --- the pictures ---------------------------------------------------------------------------------

/// A sprite: its picture until it is uploaded, its texture after, its size in its own units, and its
/// foot's place across it (a pole stands at its left).
struct Sprite {
    surface: Option<ImageSurface>,
    tex: Option<Tex>,
    size: (f64, f64),
    foot: f64,
}

impl Sprite {
    /// The SVG at `path`, drawn `tall` pixels tall (or as wide, for one wider than tall).
    fn of(path: &std::path::Path, tall: f64, foot: f64) -> Sprite {
        let size = svg_size(path).unwrap_or((100.0, 100.0));
        let k = (tall / size.1.max(size.0)).max(1e-3);
        let (w, h) = ((size.0 * k).ceil().max(1.0) as i32, (size.1 * k).ceil().max(1.0) as i32);
        Sprite { surface: Some(svg_layer(path, w, h, k, 0.0, 0.0, None)), tex: None, size, foot }
    }
    fn picture(surface: ImageSurface, size: (f64, f64)) -> Sprite {
        Sprite { surface: Some(surface), tex: None, size, foot: 0.5 }
    }
    /// A PNG, at its own size in pixels.
    fn png(path: &std::path::Path) -> Sprite {
        let s = load_png(path);
        let size = (s.width() as f64, s.height() as f64);
        Sprite::picture(s, size)
    }
}

/// The sprites, by index.
const SUN: usize = 0;
const MOON: usize = 1;
const STARS: usize = 2;
const HILLS: usize = 3;
const POLE: usize = 4;
const POLE_LIT: usize = 5;
const GLOW: usize = 6;
const CARS: usize = 7;
// then the car's views, the clouds and the trees

/// How high the sun climbs at noon, in canvas pixels above its rising, and where its centre is as it
/// rises and sets: on the ridges, so it is half hidden behind them then, and sinks behind them after.
const SUN_PEAK: f64 = 200.0;
const SUN_HORIZON: f64 = HORIZON - 88.0;

/// A pole's height, in half widths of the road where it stands.
const POLE_TALL: f64 = 1.6;

/// The hills' rows on the canvas: their highest peak to their foot, under the ground.
const HILLS_TOP: f64 = HORIZON - 110.0;
const HILLS_FOOT: f64 = HORIZON + 72.0;
/// The rows of its picture the hills' SVG is drawn from: its own canvas's, whose horizon was 440.
const HILLS_SVG_TOP: f64 = 330.0;

/// How a sprite is drawn: over what is under it, or added to it as light (`Add`), or lightening it
/// (`Screen`).
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Solid,
    Light(Blend),
}

/// A sprite placed for a frame: which, its rectangle on the screen and the piece of its picture
/// there, its tints at the top and the foot with their strengths, how it is drawn, and the row below
/// which it is hidden (by a crest in front of it).
#[derive(Clone, Copy)]
struct Placed {
    sprite: usize,
    rect: (f64, f64, f64, f64),
    st: (f64, f64, f64, f64),
    top: (Rgb, f64),
    foot: (Rgb, f64),
    mode: Mode,
    clip: f64,
}

/// The light of the moment, from the time of day.
#[derive(Clone, Copy, Debug, Default)]
struct Light {
    /// The sun's and moon's places along their arcs (0 rising, 1 setting), its height (1 at noon, -1
    /// at midnight), the daylight (0 to 1), dusk (1 with the sun on the horizon).
    sun: f64,
    moon: f64,
    elevation: f64,
    day: f64,
    dusk: f64,
    /// Morning (1) or evening (0), for dawn's colours or dusk's.
    morning: f64,
}

/// The sky's colours for a moment: top, middle and horizon, the glow about the sun and its strength.
struct SkyLook {
    stops: [Rgb; 3],
    glow: Rgb,
    strength: f64,
    band: f64,
}

pub struct Racing {
    s: Settings,
    seed: i64,
    rise: f64,
    set: f64,
    sprites: Vec<Sprite>,
    clouds: std::ops::Range<usize>,
    trees: std::ops::Range<usize>,
    /// The canvas on the screen: scale, then offsets.
    fit: (f64, f64, f64),
    /// The size of the grid's pixels, in the screen's (1: no grid), and the colours each channel has.
    pix: f64,
    levels: f64,
    sky: Option<Option<Program>>,
    road: Option<Option<Program>>,
    crt: Option<Option<Program>>,
    /// The low frame the scene is drawn into, which the CRT shows; None without one (no grid, or a
    /// GL without render targets: then the shaders keep to the grid themselves, `grid`).
    frame: Option<Option<Tex>>,
    grid: f64,
    /// The road's table, as a picture a row high, and its texture.
    table: ImageSurface,
    table_tex: Option<Tex>,
    t: f64,
    f: f64,
    fps: f64,
    /// The road ahead now, and the backdrop's turn.
    ahead: Ahead,
    turned: f64,
    course: Course,
}

/// A shader of this element, and its uniforms' places by name.
struct Program {
    prog: GLuint,
    at: Vec<GLint>,
}

impl Racing {
    fn new(g: &Geo, t: &Table) -> Racing {
        let s: Settings = settings(t);
        let s_seed = s.seed;
        let [rise, set] = g.th.tw.day;
        let (w, h) = (g.w as f64, g.h as f64);
        let k = (w / CANVAS.0).max(h / CANVAS.1);
        let fit = (k, (w - CANVAS.0 * k) / 2.0, h - CANVAS.1 * k);
        let pix = if s.pixels > 0.0 { (h / s.pixels).round().max(1.0) } else { 1.0 };
        let levels = s.colours.round();
        Racing {
            seed: s.seed,
            s,
            rise,
            set,
            sprites: vec![],
            clouds: 0..0,
            trees: 0..0,
            fit,
            pix,
            levels,
            sky: None,
            road: None,
            crt: None,
            frame: None,
            grid: pix,
            table: surface(ROWS as i32, 1),
            table_tex: None,
            t: 0.0,
            f: crate::day::daytime(),
            fps: g.th.tw.fps,
            ahead: Ahead::level(),
            turned: 0.0,
            course: Course::new(s_seed),
        }
    }

    fn path(g: &Geo, name: &str) -> PathBuf {
        g.th.path(name)
    }

    /// The pictures, drawn once for this screen's size.
    fn pictures(&mut self, g: &Geo) {
        if !self.sprites.is_empty() {
            return;
        }
        let k = self.fit.0;
        let tall = (g.h as f64).min(900.0);
        let s = &self.s;
        let mut v = vec![Sprite::of(&Self::path(g, &s.sun), 300.0 * k, 0.5), Sprite::of(&Self::path(g, &s.moon), 170.0 * k, 0.5)];
        // the stars, over the sky only
        let (sw, sh) = ((CANVAS.0 * k).ceil() as i32, (HORIZON * k).ceil() as i32);
        v.push(Sprite::picture(svg_layer(&Self::path(g, &s.stars), sw, sh, k, 0.0, 0.0, None), (CANVAS.0, HORIZON)));
        v.push(Sprite::picture(Self::hills(&Self::path(g, &s.hills), k), (2.0 * CANVAS.0, HILLS_FOOT - HILLS_TOP)));
        // the poles: drawn at their foot, which is at the left of their picture
        v.push(Sprite::of(&Self::path(g, &s.pole), tall, 30.0 / 110.0));
        v.push(Sprite::of(&Self::path(g, &s.pole_lit), tall, 30.0 / 160.0));
        v.push(Sprite::picture(soft_dot(128, &SOFT_DOT), (1.0, 1.0)));
        v.extend(s.cars.iter().map(|c| Sprite::png(&Self::path(g, c))));
        let start = v.len();
        v.extend(s.clouds.iter().map(|c| Sprite::of(&Self::path(g, c), 330.0 * k, 0.5)));
        self.clouds = start..v.len();
        let start = v.len();
        v.extend(s.trees.iter().map(|c| Sprite::of(&Self::path(g, c), tall * 0.8, 0.5)));
        self.trees = start..v.len();
        self.sprites = v;
    }

    /// The hills' silhouette, white: the SVG's, then its mirror image, so it repeats without a seam as
    /// the backdrop turns.
    fn hills(path: &std::path::Path, k: f64) -> ImageSurface {
        let (w, h) = ((2.0 * CANVAS.0 * k).ceil() as i32, ((HILLS_FOOT - HILLS_TOP) * k).ceil() as i32);
        let s = surface(w, h);
        {
            let c = ctx(&s);
            for (x, flip) in [(0.0, false), (2.0 * CANVAS.0, true)] {
                let _ = c.save();
                c.scale(k, k);
                c.translate(x, -HILLS_SVG_TOP);
                if flip {
                    c.scale(-1.0, 1.0);
                }
                crate::svg::draw_svg(&c, path, None);
                let _ = c.restore();
            }
        }
        s
    }

    /// A point of the canvas moved onto the low frame's pixels (as it is without one).
    fn on_grid(&self, x: f64, y: f64) -> (f64, f64) {
        if self.frame.flatten().is_none() && self.pix <= 1.0 {
            return (x, y);
        }
        let (sx, sy) = self.at(x, y);
        let g = self.pix;
        (((sx / g).round() * g - self.fit.1) / self.fit.0, ((sy / g).round() * g - self.fit.2) / self.fit.0)
    }

    /// Canvas to screen.
    fn at(&self, x: f64, y: f64) -> (f64, f64) {
        (self.fit.1 + x * self.fit.0, self.fit.2 + y * self.fit.0)
    }

    /// A sprite over a rectangle of the canvas, tinted, at a strength.
    fn place(&self, sprite: usize, r: (f64, f64, f64, f64), tint: Rgb, a: f64) -> Placed {
        let (x0, y0) = self.at(r.0, r.1);
        let (x1, y1) = self.at(r.2, r.3);
        Placed { sprite, rect: (x0, y0, x1, y1), st: (0.0, 0.0, 1.0, 1.0), top: (tint, a), foot: (tint, a), mode: Mode::Solid, clip: f64::INFINITY }
    }

    fn light(&self) -> Light {
        let (sun, moon, elevation, day, dusk) = crate::day::at(self.rise, self.set, self.f);
        let h = self.f.rem_euclid(1.0) * 24.0;
        let noon = (self.rise + self.set) / 2.0;
        Light { sun, moon, elevation, day, dusk, morning: smoothstep(noon + 1.0, noon - 1.0, h) * smoothstep(self.rise - 7.0, self.rise - 5.0, h) }
    }

    /// The sky's colours: night to day by the daylight, dawn's or dusk's colours about the horizon as
    /// the sun nears it.
    fn sky_look(&self, l: &Light) -> SkyLook {
        let s = &self.s;
        let pick = |set: &[Colour; 3], i: usize| set[i].rgb();
        let twilight: [Rgb; 3] = std::array::from_fn(|i| mixed(pick(&s.sky_dusk, i), pick(&s.sky_dawn, i), l.morning));
        let glow = mixed(s.glow_dusk.rgb(), s.glow_dawn.rgb(), l.morning);
        let stops: [Rgb; 3] = std::array::from_fn(|i| {
            let base = mixed(pick(&s.sky_night, i), pick(&s.sky_day, i), l.day);
            // the top of the sky takes less of dusk than the horizon
            mixed(base, twilight[i], l.dusk * [0.55, 0.85, 0.95][i])
        });
        SkyLook { stops, glow, strength: 0.25 * l.day + 0.9 * l.dusk, band: 0.55 * l.dusk }
    }

    /// The scene's light on what stands in it: daylight by day, warm at dusk, dark blue at night.
    fn tint(&self, l: &Light) -> Rgb {
        let night = mixed(self.s.light_night.rgb(), self.s.light_day.rgb(), l.day);
        mixed(night, self.s.light_dusk.rgb(), l.dusk * l.day.max(0.35) * 0.9)
    }

    /// How dark it is: 0 by day, 1 at night: the lamps and lights come on.
    fn dark(&self, l: &Light) -> f64 {
        smoothstep(0.55, 0.12, l.day)
    }

    /// Where the sun (or the moon) stands, at its place along its arc: low at each end, behind the
    /// hills before it rises and after it sets; turned and risen with the backdrop.
    fn orb(&self, along: f64, peak: f64) -> (f64, f64) {
        (CANVAS.0 * (0.14 + 0.44 * along) + self.turned * 0.9, SUN_HORIZON - peak * (PI * along).sin().max(-1.0) + self.ahead.rise)
    }

    fn sun_size(l: &Light) -> f64 {
        // larger, and redder, low in the sky; small and white high in it
        105.0 + 135.0 * smoothstep(0.5, 0.0, l.elevation.abs())
    }

    /// The sprites behind the road: stars, the sun's halo, the sun, the moon, clouds and hills; all
    /// turned and risen with the backdrop.
    fn back(&self, l: &Light, sky: &SkyLook, t: f64, out: &mut Vec<Placed>) {
        let up = self.ahead.rise;
        let stars = smoothstep(0.3, 0.02, l.day) * 0.95;
        if stars > 0.01 {
            for (x0, x1, u0, u1) in wrapped(CANVAS.0, self.turned * 0.9) {
                let mut p = self.place(STARS, (x0, up, x1, HORIZON + up), (1.0, 1.0, 1.0), stars);
                p.st = (u0, 0.0, u1, 1.0);
                out.push(p);
            }
        }
        // the sun, its halo first
        if (-0.08..=1.08).contains(&l.sun) {
            let (x, y) = self.orb(l.sun, SUN_PEAK);
            let size = Self::sun_size(l);
            let low = smoothstep(0.5, 0.0, l.elevation);
            let halo = size * (2.4 + 2.2 * l.dusk);
            let glow = mixed((1.0, 0.95, 0.8), sky.glow, 0.4 + 0.6 * low);
            let mut p = self.place(GLOW, (x - halo, y - halo, x + halo, y + halo), glow, 0.22 + 0.28 * l.dusk);
            p.mode = Mode::Light(Blend::Screen);
            out.push(p);
            let tint = mixed((1.0, 1.0, 1.0), (1.0, 0.72, 0.62), low);
            out.push(self.place(SUN, (x - size / 2.0, y - size / 2.0, x + size / 2.0, y + size / 2.0), tint, 1.0));
            // high in the sky, a white glare over it: the sunset's colours are only the low sun's
            let high = smoothstep(0.15, 0.6, l.elevation);
            if high > 0.01 {
                for r in [size * 1.5, size * 0.9] {
                    let mut p = self.place(GLOW, (x - r, y - r, x + r, y + r), (1.0, 0.99, 0.94), high);
                    p.mode = Mode::Light(Blend::Screen);
                    out.push(p);
                }
            }
        }
        // the moon, by night
        if (-0.08..=1.08).contains(&l.moon) && l.day < 0.6 {
            let (x, y) = self.orb(l.moon, SUN_PEAK - 20.0);
            let size = 130.0;
            let a = smoothstep(0.45, 0.1, l.day);
            let mut p = self.place(GLOW, (x - size * 1.6, y - size * 1.6, x + size * 1.6, y + size * 1.6), (0.75, 0.8, 1.0), 0.16 * a);
            p.mode = Mode::Light(Blend::Screen);
            out.push(p);
            out.push(self.place(MOON, (x - size / 2.0, y - size / 2.0, x + size / 2.0, y + size / 2.0), (1.0, 1.0, 1.0), a));
        }
        // long thin streaks of cloud low over the hills, in front of a low sun: pale by day, glowing at
        // dawn and dusk, grey at night
        let n = self.clouds.len();
        if n > 0 {
            let lit = mixed(mixed((0.08, 0.09, 0.15), (1.0, 0.98, 0.95), l.day), mixed(sky.glow, (1.0, 0.86, 0.7), 0.35), l.dusk);
            let shade = mixed(mixed((0.06, 0.07, 0.12), (0.86, 0.88, 0.92), l.day), mixed(sky.stops[1], sky.glow, 0.45), l.dusk);
            for i in 0..4 {
                let sprite = self.clouds.start + (hash(self.seed, i, 71) * n as f64) as usize % n;
                let (w0, h0) = self.sprites[sprite].size;
                let (w, h) = (w0 * (1.6 + 1.2 * hash(self.seed, i, 72)), h0 * (0.36 + 0.18 * hash(self.seed, i, 73)));
                let span = CANVAS.0 + 2.0 * 700.0;
                let x = (hash(self.seed, i, 74) * span - t * 1.5 + self.turned * 0.95).rem_euclid(span) - 700.0;
                let y = HORIZON - 178.0 + 70.0 * hash(self.seed, i, 75) + up;
                // only about dawn and dusk: by day a thin streak is a bright line on the low frame
                let a = 0.82 * smoothstep(0.15, 0.6, l.dusk);
                if a < 0.02 {
                    continue;
                }
                let mut p = self.place(sprite, (x, y, x + w, y + h), shade, a);
                p.foot = (lit, a);
                out.push(p);
            }
        }
        // clouds drifting: grey beneath by day, lit from below at dawn and dusk, dark at night
        if n > 0 {
            let top = mixed(mixed((0.1, 0.11, 0.18), (0.95, 0.95, 0.94), l.day), mixed(sky.stops[1], (1.0, 0.85, 0.85), 0.5), l.dusk * 0.7);
            let shaded = mixed(top, (0.6, 0.66, 0.76), 0.55 * l.day * (1.0 - l.dusk));
            let under = mixed(shaded, sky.glow, l.dusk * 0.85);
            for i in 0..5 {
                let sprite = self.clouds.start + (hash(self.seed, i, 51) * n as f64) as usize % n;
                let (w0, h0) = self.sprites[sprite].size;
                let size = 0.7 + 0.5 * hash(self.seed, i, 52);
                let (w, h) = (w0 * size, h0 * size);
                let span = CANVAS.0 + 2.0 * 360.0;
                let speed = 4.0 + 5.0 * hash(self.seed, i, 53);
                let x = (hash(self.seed, i, 54) * span - t * speed + self.turned * 1.1).rem_euclid(span) - 360.0;
                let y = 30.0 + (HORIZON - 260.0) * hash(self.seed, i, 55) + up;
                let mut p = self.place(sprite, (x, y, x + w, y + h), top, 1.0);
                p.foot = (under, 1.0);
                out.push(p);
            }
        }
        // the hills: a far ridge, pale with haze, and the near one, darker
        let s = &self.s;
        let pair = |set: &[Colour; 2]| (set[0].rgb(), set[1].rgb());
        let (day, dusk, night) = (pair(&s.hills_day), pair(&s.hills_dusk), pair(&s.hills_night));
        let mix2 = |a: (Rgb, Rgb), b: (Rgb, Rgb), t: f64| (mixed(a.0, b.0, t), mixed(a.1, b.1, t));
        let near = mix2(mix2(night, day, l.day), dusk, l.dusk * 0.85);
        let haze = sky.stops[2];
        // the far ridge is the near one half a turn of its picture on, so their peaks do not line up
        for (lift, squash, hazed, depth, on) in [(22.0, 0.9, 0.5, 0.92, CANVAS.0 * 0.83), (0.0, 1.0, 0.0, 1.0, 0.0)] {
            let y1 = HILLS_FOOT + up;
            let y0 = y1 - (HILLS_FOOT - HILLS_TOP) * squash - lift;
            let (top, foot) = (mixed(near.0, haze, hazed), mixed(near.1, haze, hazed * 0.8));
            // drawn down to just under the horizon only: below it is land, and a crest that drops the land
            // would show the ridges' pale foot as a band
            let horizon = HORIZON + up + 3.0;
            let cut = ((horizon - y0) / (y1 - y0)).clamp(0.0, 1.0);
            let foot = mixed(top, foot, cut);
            for (x0, x1, u0, u1) in wrapped(2.0 * CANVAS.0, self.turned * depth + on) {
                let mut p = self.place(HILLS, (x0, y0, x1, horizon), top, 1.0);
                p.st = (u0, 0.0, u1, cut);
                p.foot = (foot, 1.0);
                out.push(p);
            }
        }
    }

    /// The scenery and the car, far to near.
    fn front(&self, l: &Light, travel: f64, t: f64, out: &mut Vec<Placed>) {
        let tint = self.tint(l);
        let dark = self.dark(l);
        let lamp = self.s.lamps.rgb();
        let mut items: Vec<(f64, Vec<Placed>)> = vec![];
        let lane = lane_at(self.seed, t);
        let beam = self.s.headlights.rgb();
        // the headlights' light on a thing at depth z, `across` half widths from the road's centre line:
        // in the beam ahead, which widens with distance and fades far off, and spilling round the car as
        // it passes; only in the dark
        // the headlights' two cones, as the road's shader draws them: from the nose only
        let lit = |z: f64, across: f64| -> f64 {
            let ahead = z - Z_CAR - NOSE;
            if dark <= 0.01 || ahead <= 0.0 {
                return 0.0;
            }
            let half_w = (0.12 + 0.268 * ahead) / ROAD_HALF;
            let cone = [-1.0, 1.0].iter().map(|side| 1.0 - smoothstep(0.5 * half_w, half_w, (across - lane - side * LAMPS).abs())).sum::<f64>().min(1.0);
            dark * 0.5 * cone * smoothstep(0.0, 1.5, ahead) * (1.0 - smoothstep(25.0, 60.0, ahead))
        };
        // a thing standing at road distance `at`, `side` of the road (1 right, -1 left), `out` half
        // widths from the centre, its picture `tall` half widths tall; hidden below a crest before it
        let stand = |items: &mut Vec<(f64, Vec<Placed>)>,
                     sprite: usize,
                     at: f64,
                     side: f64,
                     out: f64,
                     tall: f64,
                     flip: bool,
                     extra: &dyn Fn(f64, f64, f64, f64, f64) -> Vec<Placed>| {
            let z = at - travel;
            if !(NEAR..=FAR).contains(&z) {
                return;
            }
            let Some((foot_y, x, clip)) = self.ahead.at(z) else { return };
            let w = scale(z);
            let foot_x = road_x(z, side * out + x);
            let sp = &self.sprites[sprite];
            let h = tall * w;
            let wd = h * sp.size.0 / sp.size.1;
            let foot = if flip { 1.0 - sp.foot } else { sp.foot };
            let x0 = foot_x - wd * foot;
            if x0 > CANVAS.0 + 50.0 || x0 + wd < -50.0 || foot_y - h > CANVAS.1 + 50.0 || foot_y - h >= clip {
                return;
            }
            // far things fade in out of the haze, rather than appear
            let a = smoothstep(FAR, FAR * 0.7, z);
            let clip_screen = self.at(0.0, clip).1;
            let mut p = self.place(sprite, (x0, foot_y - h, x0 + wd, foot_y), mixed(tint, beam, lit(z, side * out + x)), a);
            if flip {
                p.st = (1.0, 0.0, 0.0, 1.0);
            }
            p.clip = clip_screen;
            let mut v = vec![p];
            // its light spreads on the ground round its foot, in front of it too: cut off only where a
            // crest hides the thing itself, never at its own foot
            let hidden = foot_y > clip - 1.0;
            v.extend(extra(foot_x, foot_y, w, a, if hidden { clip_screen } else { f64::INFINITY }));
            items.push((z, v));
        };
        let none = |_: f64, _: f64, _: f64, _: f64, _: f64| vec![];
        // the light poles, by turns at the right and the left of the road, their arms over it; lit at
        // night, with a pool of light under each lamp and a halo about it; each lamp as it is
        // (`lamp_level`)
        let spacing = self.s.pole_spacing;
        let first = ((travel + NEAR) / spacing).floor() as i64;
        for i in first..=((travel + FAR) / spacing).ceil() as i64 {
            let level = lamp_level(self.seed, i, l.day, t);
            let sprite = if level > 0.3 { POLE_LIT } else { POLE };
            let side = if i.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
            // about the lamp a little light by day too
            let halo_k = level * (0.3 + 0.7 * dark);
            let glows = |fx: f64, fy: f64, w: f64, a: f64, clip: f64| -> Vec<Placed> {
                if halo_k <= 0.01 {
                    return vec![];
                }
                let pole_h = POLE_TALL * w;
                // the lamp's head, over the road near the pole's top: (95, 24) of its 300 high picture,
                // whose foot is at 30; at the right of the road mirrored, the arm reaching left
                let (hx, hy) = (fx - side * pole_h * (95.0 - 30.0) / 300.0, fy - pole_h * 276.0 / 300.0);
                let r = pole_h * 0.16;
                // its light on the ground is the road's shader's (`pools`); here the glow about the lamp
                let mut v = vec![self.place(GLOW, (hx - r, hy - r, hx + r, hy + r), lamp, 0.5 * halo_k * a)];
                for p in &mut v {
                    p.mode = Mode::Light(Blend::Add);
                    p.clip = clip;
                }
                v
            };
            stand(&mut items, sprite, i as f64 * spacing, side, 1.13, POLE_TALL, side > 0.0, &glows);
        }
        // trees, by cells along the road, each of either side holding one or none
        let cell = 1.7;
        let first = ((travel + NEAR) / cell).floor() as i64;
        let nt = self.trees.len();
        for i in first..=((travel + FAR) / cell).ceil() as i64 {
            for (side, salt) in [(-1.0, 0u64), (1.0, 100)] {
                let r = hash(self.seed, i, 61 + salt);
                let at = (i as f64 + 0.15 + 0.7 * hash(self.seed, i, 62 + salt)) * cell;
                let pick = hash(self.seed, i, 63 + salt);
                let out = hash(self.seed, i, 64 + salt);
                if nt > 0 && r < self.s.trees_density {
                    let sprite = self.trees.start + (pick * nt as f64) as usize % nt;
                    let tall = 0.83 + 0.5 * hash(self.seed, i, 65 + salt);
                    // keep clear of the poles, at either side
                    let min = 1.55;
                    stand(&mut items, sprite, at, side, min + 2.2 * out * out, tall, hash(self.seed, i, 66 + salt) < 0.5, &none);
                }
            }
        }
        items.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.extend(items.into_iter().flat_map(|(_, v)| v));
        // the car, in the view its turn and pitch pick, at its foot on the road, in its lane, bumping
        let view = view_of(self.ahead.pitch, self.ahead.yaw);
        let sp = &self.sprites[CARS + view];
        let k = CAR_WIDTH / self.s.car_span;
        let foot = self.s.car_feet[view / ANGLES];
        let foot_y = row_of(Z_CAR, 0.0) - bump_at(self.seed, travel);
        // the road runs straight on at the car: no offset there
        let cx = road_x(Z_CAR, lane);
        let (x0, y0) = (cx - self.s.car_middle * k, foot_y - foot * k);
        // on the low frame the car keeps to its pixels: placed there first, so its lights, its fire and
        // its shadow, placed from it, move with it exactly
        let (x0, y0) = self.on_grid(x0, y0);
        // its shadow on the road, under it from wheel to wheel: the gaps under its bumper show it, not
        // the road
        let (sx0, sx1) = (x0 + (self.s.car_middle - 0.47 * self.s.car_span) * k, x0 + (self.s.car_middle + 0.47 * self.s.car_span) * k);
        let sy = y0 + foot * k;
        out.push(self.place(GLOW, (sx0, sy - 0.11 * CAR_WIDTH, sx1, sy + 0.03 * CAR_WIDTH), (0.0, 0.0, 0.0), 0.85));
        out.push(self.place(CARS + view, (x0, y0, x0 + sp.size.0 * k, y0 + sp.size.1 * k), mixed(tint, (1.0, 1.0, 1.0), 0.25), 1.0));
        // fire from its tail pipes, briefly, as it changes gear going up and down the hills: between the
        // inner tail lights, under them on the bumper; flickering, shooting back
        if let Some(age) = shift_age(self.seed, travel, self.s.speed) {
            // from the middle of each pipe, where it is in this view
            let pipes = self.s.car_pipes[view];
            // the change of gear this fire is from: each pipe kicks in a little after it, by its own
            // moment, and burns for its own while
            let key = ((travel - self.s.speed * age) * 4.0).round() as i64;
            for n in 0..3 {
                let delay = 0.07 * hash(self.seed, key, 200 + n as u64);
                let burn = FIRE * (0.75 + 0.5 * hash(self.seed, key, 210 + n as u64));
                let local = age - delay;
                if !(0.0..burn).contains(&local) {
                    continue;
                }
                let fade = 1.0 - local / burn;
                // a burst that swells, then shrinks as it dies
                let swell = (local / 0.05).min(1.0) * (0.55 + 0.45 * fade);
                let (px, fy) = (x0 + pipes[2 * n] * k, y0 + pipes[2 * n + 1] * k);
                let flicker = 0.7 + 0.3 * noise(self.seed, 47 + n as u64, travel * 9.0);
                for (dy, r, colour, a) in [
                    (1.5, 62.0, (1.0, 0.2, 0.02), 0.6),
                    (1.0, 42.0, (1.0, 0.42, 0.05), 0.85),
                    (0.5, 25.0, (1.0, 0.72, 0.22), 1.0),
                    (0.0, 12.0, (1.0, 0.97, 0.82), 1.0),
                ] {
                    let r = r * swell * flicker;
                    let y = fy + dy * swell;
                    let mut p = self.place(GLOW, (px - r * 0.8, y - r * 0.6, px + r * 0.8, y + r * 0.6), colour, a * (0.4 + 0.6 * fade));
                    p.mode = Mode::Light(Blend::Add);
                    out.push(p);
                }
            }
        }
        // its tail lights, glowing at night, where they are in its view: the outer two amber, the inner
        // two red
        if dark > 0.01 {
            let at = self.s.car_lights[view];
            for (i, (r, colour)) in
                [(26.0, (1.0, 0.55, 0.25)), (34.0, (1.0, 0.15, 0.12)), (34.0, (1.0, 0.15, 0.12)), (26.0, (1.0, 0.55, 0.25))].into_iter().enumerate()
            {
                let (lx, ly) = (x0 + at[2 * i] * k, y0 + at[2 * i + 1] * k);
                let mut p = self.place(GLOW, (lx - r, ly - r, lx + r, ly + r), colour, 0.55 * dark);
                p.mode = Mode::Light(Blend::Add);
                out.push(p);
            }
        }
    }

    /// The street lamps' light on the ground, for the road's shader: the nearest lit lamps', each as
    /// (depth, place across the road under its head in half widths, strength), the rest none.
    fn pools(&self, l: &Light, travel: f64, t: f64) -> [[f32; 4]; POOLS] {
        let dark = self.dark(l);
        let spacing = self.s.pole_spacing;
        let first = ((travel + NEAR - 4.0) / spacing).floor() as i64;
        let mut lit: Vec<[f32; 4]> = (first..=((travel + FAR) / spacing).ceil() as i64)
            .filter_map(|i| {
                let k = lamp_level(self.seed, i, l.day, t) * dark;
                let side = if i.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
                // under the lamp's head, out over the road from the pole at its edge
                (k > 0.01).then_some([(i as f64 * spacing - travel) as f32, (side * (1.13 - POLE_TALL * 65.0 / 300.0)) as f32, k as f32, 0.0])
            })
            .collect();
        lit.truncate(POOLS);
        std::array::from_fn(|n| lit.get(n).copied().unwrap_or([0.0; 4]))
    }

    /// The road shader's uniforms for a moment.
    fn road_uniforms(&self, l: &Light, sky: &SkyLook, travel: f64, lane: f64) -> Vec<[f32; 4]> {
        let s = &self.s;
        let ground: [Rgb; 2] = std::array::from_fn(|i| mixed(s.ground_night[i].rgb(), s.ground_day[i].rgb(), l.day));
        let road: [Rgb; 4] = std::array::from_fn(|i| s.road[i].rgb());
        let tint = self.tint(l);
        let v3 = |c: Rgb| [c.0 as f32, c.1 as f32, c.2 as f32, 0.0];
        let dark = self.dark(l);
        // the far land fades towards the sky's colour at the horizon, but stays land: a sliver of it
        // seen over a crest is far fields, not a pale line
        let lit = |c: Rgb| (c.0 * tint.0, c.1 * tint.1, c.2 * tint.2);
        let haze = mixed(lit(mixed(ground[0], ground[1], 0.5)), sky.stops[2], 0.2);
        vec![
            [self.fit.0 as f32, self.fit.1 as f32, self.fit.2 as f32, self.grid as f32],
            [travel as f32, lane as f32, dark as f32, (0.35 + 0.35 * l.dusk) as f32],
            v3(ground[0]),
            v3(ground[1]),
            v3(road[0]),
            v3(road[1]),
            v3(road[2]),
            v3(road[3]),
            v3(haze),
            v3(tint),
            v3(s.headlights.rgb()),
            [self.orb(l.sun.clamp(-0.08, 1.08), SUN_PEAK).0 as f32, (0.75 * l.dusk * smoothstep(-0.25, 0.05, l.elevation)) as f32, self.levels as f32, 0.0],
            v3(sky.glow),
            [ROWS_TOP as f32, row_height() as f32, ROWS as f32, (HORIZON + self.ahead.rise) as f32],
        ]
    }

    fn sky_uniforms(&self, l: &Light, sky: &SkyLook) -> Vec<[f32; 4]> {
        let (x, y) = self.orb(l.sun.clamp(-0.08, 1.08), SUN_PEAK);
        let v3 = |c: Rgb| [c.0 as f32, c.1 as f32, c.2 as f32, 0.0];
        vec![
            [self.fit.0 as f32, self.fit.1 as f32, self.fit.2 as f32, (HORIZON + self.ahead.rise) as f32],
            v3(sky.stops[0]),
            v3(sky.stops[1]),
            v3(sky.stops[2]),
            [x as f32, y as f32, (260.0 + 160.0 * l.dusk) as f32, sky.strength as f32],
            v3(sky.glow),
            [sky.band as f32, (300.0 + 300.0 * l.dusk) as f32, self.grid as f32, self.levels as f32],
        ]
    }

    fn compile(gfx: &Gfx, src: &str, names: &[&str]) -> Option<Program> {
        match gfx.compile(&format!("{DOS}{src}")) {
            Ok(prog) => Some(Program { prog, at: names.iter().map(|n| gfx.uniform(prog, n)).collect() }),
            Err(e) => {
                crate::errln!("racing: a shader does not compile: {e}");
                None
            }
        }
    }

    /// The road at the moment, the car being as far along it as it has driven: walked from the car to
    /// the horizon, the depth each row of the screen shows (nearest first: a crest hides what is
    /// behind it), the road's offset across there, and what picks the car's view; and the backdrop's
    /// turn.
    fn advance(&mut self) {
        let s = self.t * self.s.speed;
        self.turned = self.course.at(s).1;
        self.ahead = walk(&mut self.course, self.seed, s);
        // the table, for the road's shader: per row, its depth (0: sky; all ones: beyond) and offset,
        // each in sixteen bits
        let codes: Vec<(u16, u16)> = self
            .ahead
            .rows
            .iter()
            .map(|row| match *row {
                Row::Sky => (0, 0),
                Row::Beyond => (u16::MAX, 0),
                Row::Road { z, x } => {
                    (((z / 128.0 * 65533.0).round() as u16).clamp(1, 65534), (((x + 256.0) / 512.0 * 65535.0).round().clamp(0.0, 65535.0)) as u16)
                }
            })
            .collect();
        if let Some(mut d) = view(&mut self.table) {
            for (i, (zc, xc)) in codes.into_iter().enumerate() {
                // in memory a pixel is B, G, R, A: the depth in R and G, the offset in B and A
                d[i * 4..i * 4 + 4].copy_from_slice(&[(xc >> 8) as u8, (zc & 0xff) as u8, (zc >> 8) as u8, (xc & 0xff) as u8]);
            }
        }
        self.table.mark_dirty();
    }

    /// The moment's state: the time of day, the distance driven, the car's lane.
    fn moment(&self) -> (Light, f64, f64) {
        (self.light(), self.t * self.s.speed, lane_at(self.seed, self.t))
    }
}

/// The road walked from the car, at distance `s` along it, to the horizon: the depth each row of the
/// screen shows (nearest first: a crest hides what is behind it), the road's offset across there, and
/// what picks the car's view: the road's turn (only the road's: drifting in its lane does not turn the
/// car) and its pitch.
fn walk(course: &mut Course, seed: i64, s: f64) -> Ahead {
    let zc = Z_CAR;
    let (h_car, slope) = height(seed, s);
    // the car's pitch against the viewer's, who follows it as a chase camera does, easing into a change
    // of slope over half again its distance behind: starting up a hill the car is nose up against the
    // viewer's line, who sees less of its roof; over a crest nose down, and more
    let pitch_lag = slope - height(seed, s - 1.5 * Z_CAR).1;
    // its turn: the road's curvature where it is, a little: most bends 2 degrees or none, the sharpest
    // 4
    let yaw = course.at(s).0 / 0.0012 * (3.6 * PI / 180.0);
    // the view tilts with the road at the car, wholly, so the road under the car is always level on the
    // screen, as the car is; the horizon, the sky and the far land move by just as much
    let tilt = slope;
    let rise = FOCAL * tilt;
    // the car's pitch on the screen: against the camera still on the slope behind it
    let pitch = pitch_lag;
    let rh = row_height();
    let mut rows = vec![Row::Sky; ROWS];
    let mut steps = vec![];
    let (mut ymin, mut prev) = (f64::INFINITY, None::<(f64, f64, f64)>);
    let (mut z, mut slope_x, mut x) = (NEAR, 0.0, 0.0);
    while z < Z_END {
        let next = (z * 1.012).min(Z_END);
        if next > zc {
            // the curvature summed twice from the car on: straight on at the car
            let from = z.max(zc);
            let dz = next - from;
            let c = course.at(s + (from + dz / 2.0 - zc)).0;
            x += slope_x * dz + c * dz * dz / 2.0;
            slope_x += c * dz;
        }
        z = next;
        let dh = height(seed, s + z - zc).0 - h_car - tilt * (z - zc);
        let y = row_of(z, dh);
        if y < ymin {
            // every row any of whose height the road reaches: a row half in it is road, not a gap of sky
            let first = (((y - ROWS_TOP) / rh).floor().max(0.0) as usize).min(ROWS);
            let last = if ymin.is_finite() { (((ymin - ROWS_TOP) / rh).floor().max(0.0) as usize).min(ROWS) } else { ROWS };
            for (r, row) in rows.iter_mut().enumerate().take(last).skip(first) {
                let yr = ROWS_TOP + (r as f64 + 0.5) * rh;
                *row = match prev {
                    Some((zp, yp, xp)) if (yp - y).abs() > 1e-9 => {
                        let t = ((yr - yp) / (y - yp)).clamp(0.0, 1.0);
                        Row::Road { z: zp + (z - zp) * t, x: xp + (x - xp) * t }
                    }
                    _ => Row::Road { z, x },
                };
            }
        }
        steps.push((z, y, x, ymin));
        ymin = ymin.min(y);
        prev = Some((z, y, x));
    }
    // above the road's end and under the horizon: the land beyond it
    let beyond = HORIZON + rise;
    for (r, row) in rows.iter_mut().enumerate() {
        let yr = ROWS_TOP + (r as f64 + 0.5) * rh;
        if *row == Row::Sky && yr + rh / 2.0 > beyond && yr - rh / 2.0 < ymin {
            *row = Row::Beyond;
        }
    }
    Ahead { steps, rows, pitch, yaw, rise }
}

/// How a street lamp is, by its place along the road: about one in twenty broken, one in twenty
/// flickering, one in twenty dim, one in twenty left on all day; the rest come on as dusk begins and go
/// off once the sun is up, each at its own moment, as lamps with their own sensors do.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lamp {
    Broken,
    Flickering,
    Dim,
    AlwaysOn,
    Normal,
}

fn lamp_kind(seed: i64, i: i64) -> Lamp {
    match hash(seed, i, 91) {
        r if r < 0.05 => Lamp::Broken,
        r if r < 0.10 => Lamp::Flickering,
        r if r < 0.15 => Lamp::Dim,
        r if r < 0.20 => Lamp::AlwaysOn,
        _ => Lamp::Normal,
    }
}

/// Lamp i's light now, 0 to 1, at daylight `day` and time t.
fn lamp_level(seed: i64, i: i64, day: f64, t: f64) -> f64 {
    // its sensor's moment: on below this much daylight, between dusk's start and sunrise's end
    let on = day < 0.62 + 0.2 * hash(seed, i, 92);
    match lamp_kind(seed, i) {
        Lamp::Broken => 0.0,
        Lamp::AlwaysOn => 1.0,
        Lamp::Normal if on => 1.0,
        Lamp::Dim if on => 0.4,
        // mostly on, dropping out now and then for a few frames, a buzz in its brightness
        Lamp::Flickering if on => {
            if noise(seed, 94 + (i.rem_euclid(97) as u64), t * 9.0) < -0.55 {
                0.08
            } else {
                0.85 + 0.15 * noise(seed, 95, t * 23.0)
            }
        }
        _ => 0.0,
    }
}

/// How long fire shows from the tail pipes after a change of gear, in seconds.
const FIRE: f64 = 0.2;

/// The gear for a slope: up a hill down a gear or two, down it up: in steps of five degrees.
fn gear(seed: i64, s: f64) -> i64 {
    (height(seed, s).1 / 0.09).round() as i64
}

/// Seconds since the car last changed gear, at road distance s and `speed`, while any pipe may still be
/// burning.
fn shift_age(seed: i64, s: f64, speed: f64) -> Option<f64> {
    let now = gear(seed, s);
    // as long as the longest a pipe can burn, after its latest start
    let reach = FIRE * 1.25 + 0.07;
    (1..=12).map(|k| k as f64 * reach / 12.0).find(|&t| gear(seed, s - speed * t) != now)
}

/// The car's view: its height (0 low, 4 high) from its pitch against the camera behind it, to the
/// nearest of the heights' steps. Nose up against the camera (at the foot of a hill, starting up it, the
/// camera still on the level behind) its roof leans towards the camera: seen from higher, more of the
/// roof. Nose down (over a crest, starting down, the camera still climbing behind) its roof leans away:
/// seen from lower. On a steady slope, and on the level, the middle one. Its angle (0, turned 6 degrees
/// left, to 6, turned 6 right) the view nearest its turn. As an index into its views.
fn view_of(pitch: f64, yaw: f64) -> usize {
    let height = (2.0 + (pitch / HEIGHT_STEP).round()).clamp(0.0, (HEIGHTS - 1) as f64) as usize;
    // the views are 2 degrees apart: each taken from half way to it
    let turn = (yaw.abs() / (2.0 * PI / 180.0)).round().min(3.0) as i64;
    height * ANGLES + (3 + turn * yaw.signum() as i64) as usize
}

const SKY_UNIFORMS: [&str; 8] = ["view", "top", "mid", "low", "sun", "glow", "band", "area"];
const SKY_SIZES: [usize; 8] = [4, 3, 3, 3, 4, 3, 4, 4];
/// How many street lamps' light the road's shader takes: as many as are near enough to show.
const POOLS: usize = 8;
const ROAD_UNIFORMS: [&str; 15] =
    ["view", "motion", "grass", "grass2", "asphalt", "curb", "cream", "dash", "haze", "tint", "beam", "sheen", "glow", "rows", "area"];
const ROAD_SIZES: [usize; 15] = [4, 4, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 3, 4, 4];
const CRT_UNIFORMS: [&str; 4] = ["src", "mask", "area", "tex0"];

/// What every shader of this element shares: the grid of pixels and the colours. `cell` is the grid's
/// pixel a screen position is in; `centre` that pixel's middle; `bayer` its threshold in a 4 x 4
/// ordered dither, 0 to 1; `quant` a colour brought to `levels` a channel, dithered by it.
const DOS: &str = "float b2(vec2 v) { v = mod(v, 2.0); return mod(2.0 * v.x + 3.0 * v.y, 4.0); }
float bayer(vec2 cell) { return (4.0 * b2(cell) + b2(floor(cell / 2.0)) + 0.5) / 16.0; }
vec2 cell(vec2 p, float pix) { return floor(p / max(pix, 1.0)); }
vec2 centre(vec2 p, float pix) { return pix > 1.0 ? (cell(p, pix) + 0.5) * pix : p; }
vec3 quant(vec3 c, float levels, float b) { float l = levels - 1.0; return clamp(floor(c * l + b) / l, 0.0, 1.0); }
";

/// The sky: three colours from the top to the horizon, the glow about the sun, a warm band along the
/// horizon at dawn and dusk, brightest under the sun. On the grid, as an old game's sky: its gradient
/// and its glow in steps, each step's edge dithered.
const SKY: &str = "uniform vec4 view; uniform vec3 top; uniform vec3 mid; uniform vec3 low;
uniform vec4 sun; uniform vec3 glow; uniform vec4 band;
void main() {
  vec2 raw = area.xy + uv * area.zw;
  vec2 px = centre(raw, band.z);
  vec2 c = (px - view.yz) / view.x;
  float b = bayer(cell(raw, band.z));
  bool grid = band.z > 1.0;
  float h = clamp(c.y / view.w, 0.0, 1.0);
  if (grid) h = floor(h * 18.0 + b) / 18.0;
  vec3 col = mix(top, mid, smoothstep(0.0, 0.62, h));
  col = mix(col, low, smoothstep(0.5, 1.0, h));
  vec2 d = c - sun.xy;
  d.y *= 1.35;
  float r = length(d) / sun.z;
  float g = sun.w * (0.55 * exp(-r * r * 1.6) + 0.45 * exp(-r * 1.4));
  if (grid) g = floor(g * 9.0 + b) / 9.0;
  col = 1.0 - (1.0 - col) * (1.0 - glow * clamp(g, 0.0, 1.0));
  float az = exp(-pow((c.x - sun.x) / band.y, 2.0));
  if (grid) az = floor(az * 6.0 + b) / 6.0;
  col = mix(col, glow, band.x * smoothstep(0.5, 1.0, h) * smoothstep(0.5, 1.0, h) * (0.3 + 0.7 * az));
  gl_FragColor = vec4(grid ? quant(col, band.w, b) : col, 1.0);
}";

/// The ground and the road, row by row from the table (`tex0`): the land in bands, the road's curbs,
/// centre line and surface, all moving towards the viewer, beyond the road's end the land in the haze,
/// and the sky left alone. The far bands are filtered over a pixel of the grid, so they dither rather
/// than shimmer; the edges are sharp, as on the grid they would be. The scene's light over them, the
/// headlights' beams on the road at night, the haze at the horizon, the glare of a low sun.
const ROAD_SHADER: &str = "uniform vec4 view; uniform vec4 motion; uniform vec3 grass; uniform vec3 grass2;
uniform vec3 asphalt; uniform vec3 curb; uniform vec3 cream; uniform vec3 dash; uniform vec3 haze;
uniform vec3 tint; uniform vec3 beam; uniform vec4 sheen; uniform vec3 glow; uniform vec4 rows;
uniform vec3 lampc; uniform vec4 pools[POOLS];
float rise(float x, float duty) { return floor(x) * duty + min(fract(x), duty); }
float pulse(float p, float duty, float fw) {
  fw = max(fw, 1e-4);
  return clamp((rise(p + 0.5 * fw, duty) - rise(p - 0.5 * fw, duty)) / fw, 0.0, 1.0);
}
float inside(float d, float edge, float fw) { return fw <= 0.0 ? step(d, edge) : 1.0 - smoothstep(edge - 0.5 * fw, edge + 0.5 * fw, d); }
// the road or land at a screen position, and whether there is any (0: sky); `ps` the screen pixels a
// fragment covers
vec4 shade(vec2 p, float ps) {
  vec2 c = (p - view.yz) / view.x;
  float r = floor((c.y - rows.x) / rows.y);
  if (r < 0.0 || r >= rows.z) return vec4(0.0);
  vec4 e = floor(texture2D(tex0, vec2((r + 0.5) / rows.z, 0.5)) * 255.0 + 0.5);
  float zc = e.r * 256.0 + e.g;
  // sky only above the horizon: under it, whatever the table says, it is the land beyond the road
  if (zc < 0.5) { if (c.y < rows.w) return vec4(0.0); zc = 65535.0; }
  vec3 col;
  if (zc > 65534.5) {
    col = mix(mix(grass, grass2, 0.5) * tint, haze, motion.w);
  } else {
    float z = zc / 65533.0 * 128.0;
    float off = (e.b * 256.0 + e.a) / 65535.0 * 512.0 - 256.0;
    float q = clamp(FOCAL / z / (900.0 - HORIZON), 0.0, 1.5);
    float d = (c.x - VANISH) * z / (FOCAL * ROAD_HALF) + VIEW - off;
    float s = z + motion.x;
    float ad = abs(d);
    // how far the road runs, and across it, in what a fragment covers: the bands' and edges' filter
    float fz = z * z * ps / view.x / FOCAL;
    float fw = z * ps / (view.x * FOCAL * ROAD_HALF);
    col = mix(grass, grass2, pulse(s / 6.0, 0.42, fz / 6.0));
    col = mix(col, mix(cream, curb, pulse(s / 1.9, 0.5, fz / 1.9)), inside(ad, 1.055, fw));
    vec3 road = mix(asphalt, asphalt * 1.06, pulse(s / 6.0, 0.42, fz / 6.0));
    col = mix(col, road, inside(ad, 1.0, fw));
    col = mix(col, dash, inside(ad, 0.008, fw) * pulse(s / 3.4, 0.45, fz / 3.4));
    col *= tint;
    // the headlights: two cones from the car's nose, one from each lamp, each spreading thirty degrees,
    // fifteen to a side, fading far off; nothing beside or behind the nose
    float ahead = z - Z_CAR - NOSE;
    float qq = max(q, 0.0);
    float lit = 0.0;
    if (ahead > 0.0) {
      float half_w = (0.12 + 0.268 * ahead) / ROAD_HALF;
      for (int k = 0; k < 2; k++) {
        float lamp = motion.y + (float(k) * 2.0 - 1.0) * LAMPS;
        lit += 1.0 - smoothstep(0.5 * half_w, half_w, abs(d - lamp));
      }
      lit = min(lit, 1.0) * smoothstep(0.0, 1.5, ahead) * (1.0 - smoothstep(25.0, 60.0, ahead)) * motion.z;
    }
    col += beam * lit * 0.55 / (1.0 + 0.05 * max(ahead, 0.0)) * (0.35 + 0.65 * inside(ad, 1.055, fw));
    // the street lamps' light: about the ground under each lamp's head, by its true distance there,
    // longer along the road than across it, so that from this low it looks round; the road takes it,
    // the grass much less
    float pl = 0.0;
    for (int k = 0; k < POOLS; k++) {
      vec4 P = pools[k];
      float dz = (z - P.x) / 1.8;
      float dx = (d - P.y) * ROAD_HALF;
      pl += P.z * exp(-(dx * dx + dz * dz) / 5.8);
    }
    col += lampc * pl * 0.34 * mix(0.3, 1.0, inside(ad, 1.055, fw));
    col = mix(col, haze, motion.w * exp(-qq * 14.0));
    float under = exp(-pow((c.x - sheen.x) / (120.0 + 900.0 * qq), 2.0));
    col = 1.0 - (1.0 - col) * (1.0 - glow * sheen.y * under * exp(-qq * 5.0) * (0.35 + 0.65 * inside(ad, 1.0, fw)));
  }
  return vec4(col, 1.0);
}
void main() {
  vec2 raw = area.xy + uv * area.zw;
  // a fragment's height on the screen: on the low frame a whole pixel of it; its edge with the sky
  // taken in four rows, so the horizon and the crests glide rather than step a pixel at a time
  float ps = max(fwidth(raw.y), 1.0);
  vec3 sum = vec3(0.0);
  float n = 0.0;
  for (int k = 0; k < 4; k++) {
    vec4 v = shade(vec2(raw.x, raw.y + ps * ((float(k) + 0.5) / 4.0 - 0.5)), ps);
    sum += v.rgb * v.a;
    n += v.a;
  }
  if (n < 0.5) discard;
  float a = n / 4.0;
  gl_FragColor = vec4(sum / 4.0, a);
}";

/// The CRT: the low frame (`tex0`, `src.xy` pixels) shown as a tube would show it, after crt-lottes
/// (Timothy Lottes, public domain): each row a beam, bright at its middle and dark between rows
/// (Gaussian, `src.z`), each pixel spread a little into its neighbours along the row (`src.w`), the
/// rows above and below lending their light, then the phosphors' mask, a stretched VGA shadow mask
/// (`mask.x` dark, `mask.y` light), and the light the mask and the rows took made back (`mask.z`). In
/// a near linear light (squared), back by the square root.
const CRT: &str = "uniform vec4 src; uniform vec4 mask;
float gaus(float p, float s) { return exp2(s * p * p); }
vec3 fetch(vec2 pos, vec2 off) {
  vec2 p = (floor(pos * src.xy + off) + 0.5) / src.xy;
  vec3 c = texture2D(tex0, vec2(p.x, 1.0 - p.y)).rgb;
  return c * c;
}
vec2 dist(vec2 pos) { pos *= src.xy; return -((pos - floor(pos)) - 0.5); }
vec3 horz3(vec2 pos, float off) {
  vec3 b = fetch(pos, vec2(-1.0, off));
  vec3 c = fetch(pos, vec2(0.0, off));
  vec3 d = fetch(pos, vec2(1.0, off));
  float dst = dist(pos).x;
  float wb = gaus(dst - 1.0, src.w);
  float wc = gaus(dst, src.w);
  float wd = gaus(dst + 1.0, src.w);
  return (b * wb + c * wc + d * wd) / (wb + wc + wd);
}
float scan(vec2 pos, float off) { return gaus(dist(pos).y + off, src.z); }
vec3 tri(vec2 pos) {
  return horz3(pos, -1.0) * scan(pos, -1.0) + horz3(pos, 0.0) * scan(pos, 0.0) + horz3(pos, 1.0) * scan(pos, 1.0);
}
vec3 shadow(vec2 p) {
  vec3 m = vec3(mask.x);
  p.x += p.y * 3.0;
  float f = fract(p.x / 6.0);
  if (f < 0.333) m.r = mask.y; else if (f < 0.666) m.g = mask.y; else m.b = mask.y;
  return m;
}
void main() {
  vec3 c = tri(uv) * shadow(floor(gl_FragCoord.xy)) * mask.z;
  gl_FragColor = vec4(sqrt(max(c, 0.0)), 1.0);
}";

impl Part for Racing {
    fn still(&self) -> bool {
        true
    }
    fn paint(&mut self, _g: &Geo, c: &Context, _s: &mut Shared) {
        let (r, gg, b) = self.s.sky_night[0].rgb();
        c.set_source_rgb(r, gg, b);
        let _ = c.paint();
    }
    fn area(&self, _g: &Geo) -> Option<Vec<Rect>> {
        Some(vec![]) // painted only under the live picture, never shown in it
    }
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.pictures(g);
        // on the grid, the car, the trees and the poles are pixel art: made at the size they are seen in
        // the low frame (the car exactly, the trees and poles near), each texel a sharp pixel there
        let art = self.pix > 1.0;
        let per = self.fit.0 / self.pix;
        let k = CAR_WIDTH / self.s.car_span;
        for (i, sp) in self.sprites.iter_mut().enumerate() {
            let Some(mut s) = sp.surface.take() else { continue };
            let tall = if (CARS..CARS + VIEWS).contains(&i) {
                Some((sp.size.1 * k * per, 10.0, false))
            } else if i == POLE || i == POLE_LIT {
                Some((POLE_TALL * scale(16.0) * per, 7.0, false))
            } else if self.trees.contains(&i) {
                Some((1.1 * scale(16.0) * per, 6.0, true))
            } else {
                None
            };
            let kind = match tall {
                Some((h, levels, leaves)) if art => {
                    // an even width, so a picture symmetric about its middle stays so on the pixels
                    let w = (h * sp.size.0 / sp.size.1 / 2.0).round() * 2.0;
                    s = pixel_art(&s, w as i32, h.round() as i32, levels, leaves, (CARS..CARS + VIEWS).contains(&i));
                    // the car is drawn at its own size on the frame's pixels: sharp; the trees and poles
                    // at every size and between pixels: filtered, so they glide
                    if (CARS..CARS + VIEWS).contains(&i) { Kind::Pixel } else { Kind::Sprite }
                }
                _ => Kind::Sprite,
            };
            sp.tex = Some(up.add(s, kind));
        }
        self.table_tex = Some(up.add(surface(ROWS as i32, 1), Kind::Label));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.t = w.clock;
        self.f = crate::day::daytime();
        self.advance();
        let mut h = self.t.to_bits();
        hash_f(&mut h, self.f);
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.fps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (l, travel, lane) = self.moment();
        let sky = self.sky_look(&l);
        let (w, h) = (g.w as f64, g.h as f64);
        let pix = self.pix;
        let frame = *self.frame.get_or_insert_with(|| if pix > 1.0 { gfx.render_target((w / pix).ceil() as i32, (h / pix).ceil() as i32) } else { None });
        self.grid = if frame.is_some() { 1.0 } else { pix };
        gfx.target(frame);
        let mut u = self.sky_uniforms(&l, &sky);
        u.push([0.0, 0.0, w as f32, h as f32]);
        let sky_prog = self.sky.get_or_insert_with(|| Self::compile(gfx, SKY, &SKY_UNIFORMS));
        if let Some(p) = sky_prog {
            let all: Vec<_> = p.at.iter().zip(&u).zip(SKY_SIZES).map(|((&at, v), n)| (at, n, *v)).collect();
            gfx.custom(p.prog, &all, &[], 0.0, 0.0, w, h, Blend::Opaque);
        }
        let mut back = vec![];
        self.back(&l, &sky, self.t, &mut back);
        self.emit(gfx, &back);
        if let Some(tex) = self.table_tex {
            gfx.replace(tex, &self.table, Kind::Label);
        }
        let top = self.at(0.0, ROWS_TOP).1.floor().max(0.0);
        let mut u = self.road_uniforms(&l, &sky, travel, lane);
        u.push([0.0, top as f32, w as f32, (h - top) as f32]);
        let pools = self.pools(&l, travel, self.t);
        let lampc = self.s.lamps.rgb();
        let road_prog = self.road.get_or_insert_with(|| {
            let src = ROAD_SHADER
                .replace("VANISH", &format!("{VANISH:.1}"))
                .replace("ROAD_HALF", &format!("{ROAD_HALF:.5}"))
                .replace("FOCAL", &format!("{FOCAL:.1}"))
                .replace("HORIZON", &format!("{HORIZON:.1}"))
                .replace("Z_CAR", &format!("{Z_CAR:.3}"))
                .replace("NOSE", &format!("{NOSE:.3}"))
                .replace("LAMPS", &format!("{LAMPS:.4}"))
                .replace("POOLS", &POOLS.to_string())
                .replace("VIEW", &format!("{VIEW:.4}"));
            let mut p = Self::compile(gfx, &src, &ROAD_UNIFORMS)?;
            p.at.push(gfx.uniform(p.prog, "tex0"));
            p.at.push(gfx.uniform(p.prog, "lampc"));
            p.at.extend((0..POOLS).map(|k| gfx.uniform(p.prog, &format!("pools[{k}]"))));
            Some(p)
        });
        if let (Some(p), Some(tex)) = (road_prog, self.table_tex) {
            let mut all: Vec<_> = p.at.iter().zip(&u).zip(ROAD_SIZES).map(|((&at, v), n)| (at, n, *v)).collect();
            let n = ROAD_UNIFORMS.len();
            all.push((p.at[n + 1], 3, [lampc.0 as f32, lampc.1 as f32, lampc.2 as f32, 0.0]));
            all.extend(p.at[n + 2..].iter().zip(pools).map(|(&at, v)| (at, 4, v)));
            let sampler = p.at[ROAD_UNIFORMS.len()];
            gfx.custom(p.prog, &all, &[(sampler, tex)], 0.0, top, w, h, Blend::Premultiplied);
        }
        let mut front = vec![];
        self.front(&l, travel, self.t, &mut front);
        self.emit(gfx, &front);
        if let Some(f) = frame {
            gfx.target(None);
            let crt = self.crt.get_or_insert_with(|| {
                let mut p = Self::compile(gfx, CRT, &CRT_UNIFORMS[..3])?;
                p.at.push(gfx.uniform(p.prog, "tex0"));
                Some(p)
            });
            if let Some(p) = crt {
                let (fw, fh) = gfx.size(f);
                // the beam's sharpness (crt-lottes's hardScan) and the spread along a row (hardPix); the
                // mask's dark and light, and the light given back
                let values = [[fw as f32, fh as f32, -8.0, -3.0], [0.65, 1.35, 1.7, 0.0], [0.0, 0.0, w as f32, h as f32]];
                let all: Vec<_> = p.at.iter().zip(values).map(|(&at, v)| (at, 4, v)).collect();
                gfx.custom(p.prog, &all, &[(p.at[3], f)], 0.0, 0.0, w, h, Blend::Opaque);
            }
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.pictures(g);
        self.advance();
        let (l, travel, _) = self.moment();
        let sky = self.sky_look(&l);
        self.paint_sky(g, c, &l, &sky);
        let mut back = vec![];
        self.back(&l, &sky, self.t, &mut back);
        self.paint_placed(c, &back);
        self.paint_road(g, c, &l, &sky, travel);
        let mut front = vec![];
        self.front(&l, travel, self.t, &mut front);
        self.paint_placed(c, &front);
        if self.pix > 1.0
            && let Ok(mut target) = ImageSurface::try_from(c.target())
        {
            c.target().flush();
            pixelate(&mut target, self.pix as usize, self.levels);
            target.mark_dirty();
        }
    }
}

/// A picture as the CRT shows the low frame, near enough for a still: each pixel of the grid the colour
/// at its middle, its rows a beam, bright at their middle and dark between, as bright overall.
fn pixelate(s: &mut ImageSurface, pix: usize, _levels: f64) {
    let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
    let Some(mut d) = view(s) else { return };
    let beam: Vec<f64> = (0..pix).map(|y| (-8.0 * ((y as f64 + 0.5) / pix as f64 - 0.5).powi(2)).exp2()).collect();
    let mean = beam.iter().sum::<f64>() / pix as f64;
    for cy in 0..h.div_ceil(pix) {
        for cx in 0..w.div_ceil(pix) {
            let (mx, my) = ((cx * pix + pix / 2).min(w - 1), (cy * pix + pix / 2).min(h - 1));
            let at = my * stride + mx * 4;
            let px = [d[at], d[at + 1], d[at + 2], d[at + 3]];
            for y in cy * pix..((cy + 1) * pix).min(h) {
                let k = beam[y - cy * pix] / mean;
                let row = [0, 1, 2].map(|i| (px[i] as f64 * k).round().min(255.0) as u8);
                for x in cx * pix..((cx + 1) * pix).min(w) {
                    d[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&[row[0], row[1], row[2], px[3]]);
                }
            }
        }
    }
}

/// A picture made pixel art, `w` x `h`: drawn down to that size, its edges hard; lit from the top left
/// (its edges there a step lighter, at the bottom right a step darker), its light in `levels` steps,
/// a dark outline round it; leaves in clusters of lighter and darker pixels.
fn pixel_art(src: &ImageSurface, w: i32, h: i32, levels: f64, leaves: bool, car: bool) -> ImageSurface {
    let (w, h) = (w.max(1), h.max(1));
    let mut small = surface(w, h);
    {
        let c = ctx(&small);
        c.scale(w as f64 / src.width().max(1) as f64, h as f64 / src.height().max(1) as f64);
        if c.set_source_surface(src, 0.0, 0.0).is_ok() {
            c.source().set_filter(cairo::Filter::Best);
            let _ = c.paint();
        }
    }
    let (wu, hu, stride) = (w as usize, h as usize, small.stride() as usize);
    let Some(mut d) = view(&mut small) else { return small };
    // straight colours, 0 to 1, and whether each pixel is there
    let px: Vec<[f64; 4]> = (0..wu * hu)
        .map(|i| {
            let at = (i / wu) * stride + (i % wu) * 4;
            let a = d[at + 3] as f64 / 255.0;
            let un = |v: u8| if a > 0.0 { (v as f64 / 255.0 / a).min(1.0) } else { 0.0 };
            [un(d[at + 2]), un(d[at + 1]), un(d[at]), a]
        })
        .collect();
    let solid = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < wu && (y as usize) < hu && px[y as usize * wu + x as usize][3] >= 0.5;
    for y in 0..hu as i64 {
        for x in 0..wu as i64 {
            let i = y as usize * wu + x as usize;
            let out = if solid(x, y) {
                let mut k = 1.0;
                if leaves {
                    k *= 0.8 + 0.4 * hash(0, (x / 2) * 7919 + (y / 2) * 104_729, 91);
                }
                // lit from straight above, shaded below: the same on both sides
                if !solid(x, y - 1) {
                    k *= 1.22;
                } else if !solid(x, y + 1) {
                    k *= 0.75;
                }
                let [r, g, b, _] = px[i];
                let l = 0.299 * r + 0.587 * g + 0.114 * b;
                let lq = ((l * k * levels).round() / levels).clamp(0.0, 1.0);
                let f = if l > 1e-6 { lq / l } else { 0.0 };
                [(r * f).min(1.0), (g * f).min(1.0), (b * f).min(1.0), 1.0]
            } else if let Some(n) = [(0, -1), (-1, 0), (1, 0), (0, 1)]
                .iter()
                .filter(|(dx, dy)| solid(x + dx, y + dy))
                .map(|(dx, dy)| px[(y + dy) as usize * wu + (x + dx) as usize])
                .reduce(|a, b| if a[0] + a[1] + a[2] < b[0] + b[1] + b[2] { a } else { b })
            {
                // the outline: the darkest neighbour's colour, deep
                [n[0] * 0.3, n[1] * 0.3, n[2] * 0.3, 1.0]
            } else {
                [0.0; 4]
            };
            let at = y as usize * stride + x as usize * 4;
            let a = out[3];
            d[at..at + 4].copy_from_slice(&[
                (out[2] * a * 255.0).round() as u8,
                (out[1] * a * 255.0).round() as u8,
                (out[0] * a * 255.0).round() as u8,
                (a * 255.0).round() as u8,
            ]);
        }
    }
    if car {
        underside(&mut d, wu, hu, stride);
    }
    drop(d);
    small
}

/// A car's underside made clean: between its tyres, from its bumper's lower edge down, a flat dark band
/// with a straight foot, its tail pipes' bright rims kept; below it nothing, so its shadow shows. The
/// parts that hang under a car, drawn this small, are ragged teeth that look to scrape the road.
fn underside(d: &mut [u8], w: usize, h: usize, stride: usize) {
    let at = |x: usize, y: usize| y * stride + x * 4;
    let solid = |d: &[u8], x: usize, y: usize| d[at(x, y) + 3] > 127;
    // the tyres' foot: the lowest row with anything in it
    let Some(foot) = (0..h).rev().find(|&y| (0..w).any(|x| solid(d, x, y))) else { return };
    // the tyres: the outermost runs two rows above their foot; between them, the underside
    let row = foot.saturating_sub(2);
    let runs: Vec<(usize, usize)> = {
        let mut v = vec![];
        let mut x = 0;
        while x < w {
            if solid(d, x, row) {
                let s = x;
                while x < w && solid(d, x, row) {
                    x += 1;
                }
                v.push((s, x));
            }
            x += 1;
        }
        v
    };
    if runs.len() < 2 {
        return;
    }
    let (left, right) = (runs[0].1, runs[runs.len() - 1].0);
    if right <= left + 2 {
        return;
    }
    // the bumper's lower edge: in the inner half between the tyres, the lowest row that is still red
    let red = |d: &[u8], x: usize, y: usize| {
        let i = at(x, y);
        let (b, g, r) = (d[i] as f64, d[i + 1] as f64, d[i + 2] as f64);
        solid(d, x, y) && r > 60.0 && r > 1.7 * g && r > 1.4 * b
    };
    let inner = left + (right - left) / 4..right - (right - left) / 4;
    let mut edges: Vec<usize> = inner.clone().filter_map(|x| (0..foot).rev().find(|&y| red(d, x, y))).collect();
    if edges.is_empty() {
        return;
    }
    edges.sort();
    let edge = edges[edges.len() / 2];
    let band_foot = foot.saturating_sub(1);
    for y in edge + 1..=foot {
        for x in left..right {
            let i = at(x, y);
            let (b, g, r) = (d[i] as f64, d[i + 1] as f64, d[i + 2] as f64);
            let chrome = solid(d, x, y) && (r + g + b) / 3.0 > 120.0 && (r - b).abs() < 40.0;
            let px = if y > band_foot {
                [0, 0, 0, 0]
            } else if chrome {
                [d[i], d[i + 1], d[i + 2], 255]
            } else {
                [26, 22, 22, 255]
            };
            d[i..i + 4].copy_from_slice(&px);
        }
    }
}

impl Racing {
    /// Sprites as quads: tinted from top to foot, cut off at the crest in front of them; the car on the
    /// low frame's pixels.
    fn emit(&mut self, gfx: &mut Gfx, placed: &[Placed]) {
        let snap = |v: f64, g: f64| (v / g).round() * g;
        let grid = if self.frame.flatten().is_some() { self.pix } else { 1.0 };
        for s in placed {
            let Some(tex) = self.sprites[s.sprite].tex else { continue };
            // the car keeps to the low frame's pixels, as an old racer's did; everything else moves
            // smoothly between them, filtered into the frame, so it glides rather than steps
            let g = if (CARS..CARS + VIEWS).contains(&s.sprite) { grid } else { 1.0 };
            let (x0, y0) = (snap(s.rect.0, g), snap(s.rect.1, g));
            let (x1, y1) = (x0 + snap(s.rect.2 - s.rect.0, g), y0 + snap(s.rect.3 - s.rect.1, g));
            let (u0, mut v0, u1, mut v1) = s.st;
            let (mut top, mut foot, mut ya, mut yb) = (s.top, s.foot, y0, y1);
            if s.clip < yb {
                if s.clip <= ya {
                    continue;
                }
                let t = (s.clip - ya) / (yb - ya);
                v1 = v0 + (v1 - v0) * t;
                foot = (mixed(top.0, foot.0, t), top.1 + (foot.1 - top.1) * t);
                yb = s.clip;
            }
            if yb < ya {
                std::mem::swap(&mut ya, &mut yb);
                std::mem::swap(&mut v0, &mut v1);
                std::mem::swap(&mut top, &mut foot);
            }
            let mut q = image_quad(x0, ya, x1, yb, top.0, top.1, (u0, v0, u1, v1));
            for corner in [2usize, 3] {
                q[corner * 8 + 2..corner * 8 + 6].copy_from_slice(&[foot.0.0 * foot.1, foot.0.1 * foot.1, foot.0.2 * foot.1, foot.1]);
            }
            let data: Vec<f32> = q.iter().map(|&v| v as f32).collect();
            gfx.draw(
                Some(tex),
                &data,
                match s.mode {
                    Mode::Solid => Blend::Premultiplied,
                    Mode::Light(b) => b,
                },
            );
        }
    }

    /// Sprites drawn with cairo, for the still picture.
    fn paint_placed(&self, c: &Context, placed: &[Placed]) {
        for p in placed {
            let Some(s) = self.sprites[p.sprite].surface.as_ref() else { continue };
            let _ = c.save();
            if p.clip.is_finite() {
                c.rectangle(-1e5, -1e5, 2e5, p.clip + 1e5);
                c.clip();
            }
            c.set_operator(match p.mode {
                Mode::Light(Blend::Add) => Operator::Add,
                Mode::Light(_) => Operator::Screen,
                Mode::Solid => Operator::Over,
            });
            let (x0, y0, x1, y1) = p.rect;
            // too small to see, or too large to draw: cairo's matrix would not invert
            if !((0.5..1e6).contains(&(x1 - x0).abs()) && (0.5..1e6).contains(&(y1 - y0).abs())) {
                let _ = c.restore();
                continue;
            }
            let mut q = image_quad(x0, y0, x1, y1, p.top.0, p.top.1, p.st);
            let (fc, fa) = p.foot;
            for corner in [2usize, 3] {
                q[corner * 8 + 2..corner * 8 + 6].copy_from_slice(&[fc.0 * fa, fc.1 * fa, fc.2 * fa, fa]);
            }
            let data: Vec<f32> = q.iter().map(|&v| v as f32).collect();
            paint_vertices(c, s, &data);
            let _ = c.restore();
        }
    }

    /// The sky, for the still picture: its gradient and the glow about the sun.
    fn paint_sky(&self, g: &Geo, c: &Context, l: &Light, sky: &SkyLook) {
        let (_, y0) = self.at(0.0, self.ahead.rise);
        let (_, y1) = self.at(0.0, HORIZON + self.ahead.rise);
        let grad = LinearGradient::new(0.0, y0, 0.0, y1);
        for (at, col) in
            [(0.0, sky.stops[0]), (0.45, mixed(sky.stops[0], sky.stops[1], 0.8)), (0.75, mixed(sky.stops[1], sky.stops[2], 0.5)), (1.0, sky.stops[2])]
        {
            grad.add_color_stop_rgb(at, col.0, col.1, col.2);
        }
        let _ = c.set_source(&grad);
        c.rectangle(0.0, 0.0, g.w as f64, g.h as f64);
        let _ = c.fill();
        let (sx, sy) = self.orb(l.sun.clamp(-0.08, 1.08), SUN_PEAK);
        let (sx, sy) = self.at(sx, sy);
        let r = (260.0 + 160.0 * l.dusk) * self.fit.0 * 2.2;
        let glow = RadialGradient::new(sx, sy, 0.0, sx, sy, r);
        let a = sky.strength.min(1.0);
        glow.add_color_stop_rgba(0.0, sky.glow.0, sky.glow.1, sky.glow.2, a);
        glow.add_color_stop_rgba(0.35, sky.glow.0, sky.glow.1, sky.glow.2, a * 0.35);
        glow.add_color_stop_rgba(1.0, sky.glow.0, sky.glow.1, sky.glow.2, 0.0);
        let _ = c.save();
        c.set_operator(Operator::Screen);
        let _ = c.set_source(&glow);
        c.rectangle(0.0, 0.0, g.w as f64, y1 + 2.0);
        let _ = c.fill();
        let _ = c.restore();
    }

    /// The ground and the road, for the still picture: the table's rows, as the shader colours them.
    fn paint_road(&self, g: &Geo, c: &Context, l: &Light, sky: &SkyLook, travel: f64) {
        let u = self.road_uniforms(l, sky, travel, VIEW);
        let rgb = |i: usize| (u[i][0] as f64, u[i][1] as f64, u[i][2] as f64);
        let (grass, grass2, asphalt, curb, cream, dash, haze, tint) = (rgb(2), rgb(3), rgb(4), rgb(5), rgb(6), rgb(7), rgb(8), rgb(9));
        let shade = |col: Rgb, q: f64| {
            let lit = (col.0 * tint.0, col.1 * tint.1, col.2 * tint.2);
            mixed(lit, haze, (0.35 + 0.35 * l.dusk) * (-q.max(0.0) * 14.0).exp())
        };
        let band = |s: f64, period: f64, duty: f64| (s / period).rem_euclid(1.0) < duty;
        let rh = row_height();
        let fill = |col: Rgb| {
            c.set_source_rgb(col.0, col.1, col.2);
            let _ = c.fill();
        };
        for (r, row) in self.ahead.rows.iter().enumerate() {
            let (ya, yb) = (ROWS_TOP + r as f64 * rh, ROWS_TOP + (r + 1) as f64 * rh);
            let (_, sa) = self.at(0.0, ya);
            let (_, sb) = self.at(0.0, yb);
            match *row {
                Row::Sky => {}
                Row::Beyond => {
                    c.rectangle(0.0, sa, g.w as f64, sb - sa + 0.6);
                    let lit = mixed(grass, grass2, 0.5);
                    fill(mixed((lit.0 * tint.0, lit.1 * tint.1, lit.2 * tint.2), haze, 0.35 + 0.35 * l.dusk));
                }
                Row::Road { z, x } => {
                    let q = nearness(z);
                    let s = z + travel;
                    c.rectangle(0.0, sa, g.w as f64, sb - sa + 0.6);
                    fill(shade(if band(s, 6.0, 0.42) { grass2 } else { grass }, q));
                    let strip = |from: f64, to: f64, col: Rgb| {
                        let (xa, xb) = (self.at(road_x(z, from + x), 0.0).0, self.at(road_x(z, to + x), 0.0).0);
                        c.rectangle(xa, sa, xb - xa, sb - sa + 0.6);
                        fill(col);
                    };
                    let curb_col = shade(if band(s, 1.9, 0.5) { cream } else { curb }, q);
                    strip(-CURB, -1.0, curb_col);
                    strip(1.0, CURB, curb_col);
                    strip(-1.0, 1.0, shade(if band(s, 6.0, 0.42) { mixed(asphalt, (1.0, 1.0, 1.0), 0.03) } else { asphalt }, q));
                    if band(s, 3.4, 0.45) {
                        strip(-0.008, 0.008, shade(dash, q));
                    }
                }
            }
        }
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "racing",
        defaults: r##"seed = 7
speed = 8.4
cars = ["car-0-l3.png", "car-0-l2.png", "car-0-l1.png", "car-0-c.png", "car-0-r1.png", "car-0-r2.png", "car-0-r3.png",
        "car-1-l3.png", "car-1-l2.png", "car-1-l1.png", "car-1-c.png", "car-1-r1.png", "car-1-r2.png", "car-1-r3.png",
        "car-2-l3.png", "car-2-l2.png", "car-2-l1.png", "car-2-c.png", "car-2-r1.png", "car-2-r2.png", "car-2-r3.png",
        "car-3-l3.png", "car-3-l2.png", "car-3-l1.png", "car-3-c.png", "car-3-r1.png", "car-3-r2.png", "car-3-r3.png",
        "car-4-l3.png", "car-4-l2.png", "car-4-l1.png", "car-4-c.png", "car-4-r1.png", "car-4-r2.png", "car-4-r3.png"]
car_feet = [180.0, 188.0, 196.0, 204.0, 212.0]
car_middle = 190.0
car_span = 316.0
car_lights = [
    [109.2, 78.2, 138.3, 78.1, 313.0, 77.7, 342.4, 77.7],
    [96.9, 78.2, 126.1, 78.1, 301.5, 77.8, 331.1, 77.8],
    [84.9, 78.1, 114.0, 78.0, 289.8, 77.9, 319.7, 77.9],
    [72.5, 78.0, 102.0, 78.0, 278.0, 78.0, 307.5, 78.0],
    [60.3, 77.9, 90.2, 77.9, 266.0, 78.0, 295.1, 78.1],
    [48.9, 77.8, 78.5, 77.8, 253.9, 78.1, 283.1, 78.2],
    [37.6, 77.7, 67.0, 77.7, 241.7, 78.1, 270.8, 78.2],
    [109.2, 92.2, 138.3, 92.0, 313.0, 90.8, 342.4, 90.5],
    [96.9, 92.0, 126.1, 91.9, 301.5, 91.0, 331.1, 90.9],
    [84.9, 91.8, 114.0, 91.7, 289.8, 91.3, 319.7, 91.2],
    [72.5, 91.5, 102.0, 91.5, 278.0, 91.5, 307.5, 91.5],
    [60.3, 91.2, 90.2, 91.3, 266.0, 91.7, 295.1, 91.8],
    [48.9, 90.9, 78.5, 91.0, 253.9, 91.9, 283.1, 92.0],
    [37.6, 90.5, 67.0, 90.8, 241.7, 92.0, 270.8, 92.2],
    [109.2, 106.1, 138.3, 105.9, 312.9, 103.8, 342.3, 103.4],
    [97.0, 105.8, 126.1, 105.6, 301.5, 104.3, 331.1, 104.0],
    [84.9, 105.4, 114.0, 105.4, 289.8, 104.7, 319.7, 104.5],
    [72.5, 105.0, 102.0, 105.0, 278.0, 105.0, 307.5, 105.0],
    [60.3, 104.5, 90.2, 104.7, 266.0, 105.4, 295.1, 105.4],
    [48.9, 104.0, 78.5, 104.3, 253.9, 105.6, 283.0, 105.8],
    [37.7, 103.4, 67.1, 103.8, 241.7, 105.9, 270.8, 106.1],
    [109.2, 120.0, 138.4, 119.7, 312.9, 116.8, 342.3, 116.2],
    [97.0, 119.6, 126.1, 119.4, 301.4, 117.4, 331.1, 117.0],
    [84.9, 119.0, 114.0, 119.0, 289.8, 118.0, 319.6, 117.8],
    [72.6, 118.4, 102.1, 118.5, 277.9, 118.5, 307.4, 118.4],
    [60.4, 117.8, 90.2, 118.0, 266.0, 119.0, 295.1, 119.0],
    [48.9, 117.0, 78.6, 117.4, 253.9, 119.4, 283.0, 119.6],
    [37.7, 116.2, 67.1, 116.8, 241.6, 119.7, 270.8, 120.0],
    [109.3, 133.9, 138.4, 133.4, 312.8, 129.7, 342.2, 129.0],
    [97.1, 133.3, 126.2, 133.0, 301.4, 130.5, 331.0, 130.0],
    [85.0, 132.6, 114.1, 132.5, 289.7, 131.3, 319.5, 131.0],
    [72.7, 131.8, 102.1, 131.9, 277.9, 131.9, 307.4, 131.8],
    [60.5, 131.0, 90.3, 131.3, 265.9, 132.5, 295.0, 132.6],
    [49.0, 130.0, 78.6, 130.5, 253.8, 133.0, 282.9, 133.3],
    [37.8, 129.0, 67.2, 129.7, 241.6, 133.4, 270.7, 133.9]
]
car_pipes = [
    [209.2, 142.4, 223.5, 142.4, 237.8, 142.3],
    [198.0, 142.5, 212.4, 142.4, 226.7, 142.4],
    [186.8, 142.5, 201.2, 142.4, 215.6, 142.4],
    [175.6, 142.4, 190.0, 142.4, 204.4, 142.4],
    [164.4, 142.4, 178.8, 142.4, 193.2, 142.5],
    [153.3, 142.4, 167.6, 142.4, 182.0, 142.5],
    [142.2, 142.3, 156.5, 142.4, 170.8, 142.4],
    [209.2, 154.9, 223.5, 154.7, 237.8, 154.6],
    [198.0, 154.9, 212.4, 154.8, 226.7, 154.7],
    [186.8, 154.9, 201.2, 154.9, 215.6, 154.8],
    [175.6, 154.9, 190.0, 154.9, 204.4, 154.9],
    [164.4, 154.8, 178.8, 154.9, 193.2, 154.9],
    [153.3, 154.7, 167.6, 154.8, 182.0, 154.9],
    [142.2, 154.6, 156.5, 154.7, 170.8, 154.9],
    [209.2, 167.1, 223.4, 166.9, 237.7, 166.7],
    [198.0, 167.2, 212.3, 167.1, 226.6, 166.9],
    [186.8, 167.2, 201.2, 167.1, 215.5, 167.1],
    [175.6, 167.2, 190.0, 167.2, 204.4, 167.2],
    [164.5, 167.1, 178.8, 167.1, 193.2, 167.2],
    [153.4, 166.9, 167.7, 167.1, 182.0, 167.2],
    [142.3, 166.7, 156.6, 166.9, 170.8, 167.1],
    [209.1, 179.2, 223.4, 179.0, 237.6, 178.7],
    [198.0, 179.3, 212.3, 179.1, 226.6, 178.9],
    [186.8, 179.3, 201.2, 179.2, 215.5, 179.1],
    [175.6, 179.3, 190.0, 179.3, 204.4, 179.3],
    [164.5, 179.1, 178.8, 179.2, 193.2, 179.3],
    [153.4, 178.9, 167.7, 179.1, 182.0, 179.3],
    [142.4, 178.7, 156.6, 179.0, 170.9, 179.2],
    [209.1, 191.1, 223.3, 190.8, 237.6, 190.5],
    [198.0, 191.2, 212.3, 191.0, 226.5, 190.8],
    [186.8, 191.3, 201.1, 191.1, 215.5, 191.0],
    [175.7, 191.2, 190.0, 191.2, 204.3, 191.2],
    [164.5, 191.0, 178.9, 191.1, 193.2, 191.3],
    [153.5, 190.8, 167.7, 191.0, 182.0, 191.2],
    [142.4, 190.5, 156.7, 190.8, 170.9, 191.1]
]
sun = "sun.svg"
moon = "moon.svg"
stars = "stars.svg"
hills = "hills.svg"
clouds = ["cloud-wide.svg", "cloud-small.svg"]
trees = ["tree-pine.svg", "tree-round.svg", "tree-poplar.svg"]
pole = "light-pole.svg"
pole_lit = "light-pole-lit.svg"
trees_density = 0.55
pole_spacing = 18.0
pixels = 240.0
colours = 32.0
sky_day = ["#3b6a96", "#7299b8", "#b4c8cf"]
sky_dawn = ["#2e3a72", "#c47a9c", "#ffd0a0"]
sky_dusk = ["#2a1c52", "#b8467a", "#ff9a5c"]
sky_night = ["#02030a", "#060a18", "#10162c"]
glow_dawn = "#ffc89a"
glow_dusk = "#ff8a4c"
hills_day = ["#637588", "#7a8c98"]
hills_dusk = ["#4a3a6e", "#7a5a86"]
hills_night = ["#0a0e1c", "#0e1424"]
ground_day = ["#4a6339", "#566f40"]
ground_night = ["#0e1a20", "#112026"]
road = ["#535366", "#dc6262", "#fff0cd", "#f6e3bd"]
light_day = "#e6e2d6"
light_dusk = "#ffb48a"
light_night = "#141a30"
headlights = "#fff0c8"
lamps = "#ffd88a""##,
        varies: &["clouds", "trees"],
        check: check::<Settings>,
        make: |_, t, g| Ok(vec![Box::new(Racing::new(g, t))]),
    }]
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_car_keeps_to_the_middle_of_its_lane() {
        for i in 0..40000 {
            let lane = lane_at(7, i as f64 * 0.1);
            // its wheels, a third of the lane out from its middle, stay well within the lane (0 to 1)
            assert!((0.38..=0.62).contains(&lane), "lane {lane}");
        }
        let spread = (0..40000).map(|i| (lane_at(7, i as f64 * 0.1) - VIEW).abs()).fold(0.0, f64::max);
        assert!(spread > 0.03, "it wanders, by up to {spread}");
    }

    /// A stretch of road the course and the hills leave straight and level for `before` ahead of the
    /// car, then bending: where the bend begins.
    fn bend_after_straight(c: &mut Course, before: f64) -> f64 {
        (2..4000)
            .map(|i| i as f64 * PIECE)
            .find(|&at| {
                c.piece((at / PIECE) as usize).0 != 0.0
                    && c.piece((at / PIECE) as usize - 1).0 == 0.0
                    && c.piece((at / PIECE) as usize - 2).0 == 0.0
                    && (0..200).all(|k| height(7, at - before - 10.0 + k as f64).0 == 0.0)
            })
            .expect("a bend after a level straight")
    }

    #[test]
    fn a_bend_is_seen_ahead_before_the_car_reaches_it() {
        let mut c = Course::new(7);
        let start = bend_after_straight(&mut c, 20.0);
        let zc = Z_CAR;
        // the car 20 units before the bend begins: straight up to it, bending beyond it
        let a = walk(&mut c, 7, start - 20.0);
        let (_, near, _) = a.at(zc + 15.0).unwrap();
        let (_, far, _) = a.at(100.0).unwrap();
        assert!(near.abs() < 1e-9, "straight before the bend: {near}");
        assert!(far.abs() > 0.5, "the bend seen in the distance: {far}");
        assert_eq!(c.at(start - 20.0).0, 0.0, "the car not yet in it");
        // at the car the road runs straight on, whatever the bend
        for s in [start - 20.0, start + 5.0, start + 15.0] {
            let a = walk(&mut c, 7, s);
            assert!(a.at(zc).unwrap().1.abs() < 1e-4, "within a tenth of a pixel");
        }
    }

    #[test]
    fn a_crest_hides_the_road_behind_it_and_tilts_the_car() {
        let mut c = Course::new(7);
        let (at, len, _) = (1..1000).filter_map(|k| hill(7, k)).find(|h| h.2 > 3.0).expect("a high hill");
        // just before the top, climbing: the far side hidden behind the crest
        let a = walk(&mut c, 7, at + len * 0.35);
        assert!(a.steps.iter().any(|s| s.1 >= s.3), "some road hidden behind the crest");
        assert!(a.rows.iter().any(|r| matches!(r, Row::Road { .. })));
        // over the top, the car nose down ahead of the viewer still climbing behind it
        let over = walk(&mut c, 7, at + len * 0.5);
        assert!(over.pitch < -HEIGHT_STEP / 2.0, "pitch {}", over.pitch);
        assert!(view_of(over.pitch, 0.0) / ANGLES < 2, "over the top, nose down: less of its roof");
        // starting up it, nose up ahead of the viewer still on the level
        let up = walk(&mut c, 7, at + len * 0.1);
        assert!(view_of(up.pitch, 0.0) / ANGLES > 2, "starting up, nose up: more of its roof, pitch {}", up.pitch);
    }

    #[test]
    fn hills_are_smooth_and_the_level_road_is_level() {
        for s in (0..200000).map(|i| i as f64 * 0.05) {
            let (h, slope) = height(7, s);
            let numeric = (height(7, s + 1e-4).0 - height(7, s - 1e-4).0) / 2e-4;
            assert!((slope - numeric).abs() < 1e-3, "slope {slope} against {numeric} at {s}");
            assert!(h.abs() <= 4.9);
        }
        let a = walk(&mut Course::new(7), 7, 0.0);
        assert_eq!((a.pitch, a.rise), (0.0, 0.0));
        // level: the rows under the horizon road, those over it sky
        for (r, row) in a.rows.iter().enumerate() {
            let y = ROWS_TOP + (r as f64 + 0.5) * row_height();
            if y > row_of(Z_END, 0.0) + 2.0 {
                assert!(matches!(row, Row::Road { .. }), "row {r} at {y}: {row:?}");
            } else if y < HORIZON - 2.0 {
                assert_eq!(*row, Row::Sky, "row {r} at {y}");
            }
        }
    }

    #[test]
    fn the_car_turns_into_bends_and_drifts() {
        let d = PI / 180.0;
        assert_eq!(view_of(0.0, 0.0), 17, "level and straight: the middle height, straight on");
        assert_eq!(view_of(0.0, 0.8 * d), 17, "a bend too gentle to turn the car");
        assert_eq!(view_of(0.0, 2.2 * d), 18);
        assert_eq!(view_of(0.0, 4.1 * d), 19);
        assert_eq!(view_of(0.0, 5.5 * d), 20);
        assert_eq!(view_of(0.0, 9.0 * d), 20, "no further than the furthest");
        assert_eq!(view_of(0.0, -4.1 * d), 15);
        assert_eq!(view_of(HEIGHT_STEP, 0.0), 24, "nose up a step: the next height up");
        assert_eq!(view_of(-9.0, 0.0), 3, "no lower than the lowest");
        let mut c = Course::new(7);
        let start = bend_after_straight(&mut c, 0.0);
        let right = c.piece((start / PIECE) as usize).0 > 0.0;
        let a = walk(&mut c, 7, start + EASE + 5.0);
        assert_eq!(a.yaw.signum(), if right { 1.0 } else { -1.0 }, "turned into the bend: yaw {}", a.yaw);
        assert!(a.yaw.abs() <= 3.6 * PI / 180.0 + 1e-9, "a little only: {}", a.yaw);
    }

    #[test]
    fn the_road_bends_both_ways_within_its_limits() {
        let mut c = Course::new(7);
        let (mut left, mut right, mut straight, mut most, mut step) = (0, 0, 0, 0.0f64, 0.0f64);
        let per_frame = 8.4 / 30.0;
        let mut last = c.at(0.0).1;
        let mut rights_after_left = false;
        for i in 1..(30 * 3600) {
            let now = c.at(i as f64 * per_frame).1;
            step = step.max((now - last).abs());
            most = most.max(now.abs());
            last = now;
        }
        for i in 1..c.pieces.len() {
            let b = c.pieces[i].0;
            if b < 0.0 {
                left += 1
            } else if b > 0.0 {
                right += 1
            } else {
                straight += 1
            }
            if i >= 3 && c.pieces[i - 2].0 < 0.0 && c.pieces[i - 1].0 > 0.0 && b > 0.0 && c.pieces[i - 1].0 != b {
                rights_after_left = true;
            }
        }
        assert!(left > 20 && right > 20 && straight > 20, "{left} left, {right} right, {straight} straight");
        assert!(most <= TURNED, "the backdrop turned {most}");
        assert!(step < 2.5, "a frame turns the backdrop at most {step}");
        assert!(rights_after_left, "two different right-hand bends after a left-hand one");
    }

    #[test]
    fn wrapped_pictures_cover_the_canvas_seamlessly() {
        for offset in [0.0, 100.0, -2000.0, 5000.5, -1e-17, 1440.0 * 3.0 - 1e-13] {
            let pieces = wrapped(1440.0, offset);
            assert_eq!(pieces.first().unwrap().0, 0.0);
            assert_eq!(pieces.last().unwrap().1, CANVAS.0);
            for w in pieces.windows(2) {
                assert_eq!(w[0].1, w[1].0);
            }
            for (x0, x1, u0, u1) in pieces {
                assert!((0.0..=1.0).contains(&u0) && (0.0..=1.0 + 1e-9).contains(&u1) && ((u1 - u0) * 1440.0 - (x1 - x0)).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn fire_shows_only_briefly_at_changes_of_gear_on_hills() {
        let mut shows = 0;
        let (mut on, mut longest, mut run) = (false, 0, 0);
        for i in 0..(3600 * 30) {
            let s = i as f64 * 8.4 / 30.0;
            let fire = shift_age(7, s, 8.4).is_some();
            if fire {
                shows += 1;
                run += 1;
                longest = longest.max(run);
                assert!(height(7, s).1 != 0.0 || height(7, s - 8.4 * (FIRE * 1.25 + 0.07)).1 != 0.0, "only on hills, at {s}");
            } else {
                run = 0;
            }
            on |= fire;
        }
        assert!(on, "now and then");
        // two changes close together may run into one burst, never a lasting flame
        assert!(longest <= (2.5 * (FIRE * 1.25 + 0.07) * 30.0) as i32, "a flash, not a flame: {longest} frames");
        assert!(shows < 3600 * 30 / 10, "rare: {shows} frames in an hour");
    }

    #[test]
    fn street_lamps_are_each_as_they_are() {
        let kinds: Vec<Lamp> = (0..20000).map(|i| lamp_kind(7, i)).collect();
        let share = |k: Lamp| kinds.iter().filter(|&&x| x == k).count() as f64 / kinds.len() as f64;
        for k in [Lamp::Broken, Lamp::Flickering, Lamp::Dim, Lamp::AlwaysOn] {
            assert!((share(k) - 0.05).abs() < 0.01, "{k:?}: {}", share(k));
        }
        for i in 0..2000 {
            let (night, noon) = (lamp_level(7, i, 0.0, 1.0), lamp_level(7, i, 1.0, 1.0));
            match lamp_kind(7, i) {
                Lamp::Broken => assert_eq!((night, noon), (0.0, 0.0)),
                Lamp::AlwaysOn => assert_eq!((night, noon), (1.0, 1.0)),
                Lamp::Dim => assert_eq!((night, noon), (0.4, 0.0)),
                Lamp::Normal => assert_eq!((night, noon), (1.0, 0.0)),
                Lamp::Flickering => assert_eq!(noon, 0.0),
            }
        }
        // a flickering lamp is mostly on, and off now and then
        let i = (0..).find(|&i| lamp_kind(7, i) == Lamp::Flickering).unwrap();
        let lit = (0..3000).filter(|&k| lamp_level(7, i, 0.0, k as f64 / 30.0) > 0.5).count();
        assert!((2000..2990).contains(&lit), "{lit} of 3000 frames lit");
    }

    #[test]
    fn the_car_bumps_only_a_little() {
        let most = (0..100000).map(|i| bump_at(7, i as f64 * 0.01).abs()).fold(0.0, f64::max);
        assert!(most < 3.5 && most > 0.5, "bumps up to {most} canvas pixels");
    }

    #[test]
    fn the_camera_adds_up() {
        // the car seen from 6 degrees above, the height its middle view is drawn from
        assert!(((1.0 / Z_CAR).atan().to_degrees() - 6.0).abs() < 0.1);
        // its rear wheels near the canvas's foot, under two thirds of its lane wide
        let foot = row_of(Z_CAR, 0.0);
        assert!((760.0..860.0).contains(&foot), "foot at {foot}");
        assert!((CAR_WIDTH / scale(Z_CAR) - 0.65).abs() < 1e-9);
        // the horizon well below the middle of the screen
        assert!(HORIZON / CANVAS.1 > 0.6);
        // the viewer's line runs straight up to the vanishing point
        for z in [3.0, 10.0, 100.0] {
            assert_eq!(road_x(z, VIEW), VANISH);
        }
    }

    #[test]
    fn pixel_art_is_pixels() {
        let big = surface(200, 300);
        {
            let c = ctx(&big);
            c.set_source_rgb(0.2, 0.6, 0.3);
            c.arc(100.0, 150.0, 80.0, 0.0, 6.3);
            let _ = c.fill();
        }
        let mut art = pixel_art(&big, 40, 60, 6.0, true, false);
        assert_eq!((art.width(), art.height()), (40, 60));
        let d = pixels(&mut art);
        // every pixel wholly there or not: no soft edges
        assert!(d.chunks(4).all(|p| p[3] == 0 || p[3] == 255));
        // and many shades within: lit edges, leaves, an outline
        let mut shades: Vec<[u8; 3]> = d.chunks(4).filter(|p| p[3] == 255).map(|p| [p[0], p[1], p[2]]).collect();
        shades.sort();
        shades.dedup();
        assert!(shades.len() >= 5, "{} shades", shades.len());
    }
}
