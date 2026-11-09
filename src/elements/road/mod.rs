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

mod art;
mod course;
mod shaders;

use art::*;
use course::*;
use shaders::*;

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
    /// How it goes up, once made the pixel art the low frame shows (`Road::art`).
    kind: Option<Kind>,
    tex: Option<Tex>,
    /// For a tree or a pole: its pixel art made at heights a step apart, smallest first, each with an
    /// outline a pixel wide (`Road::art`). Drawn from the one at or just over the height it is seen
    /// at, it is never shrunk by much, so its outline stays whole as it comes near.
    ladder: Vec<Rung>,
    size: (f64, f64),
    foot: f64,
    /// For a picture of lines (a pole): where its main upright and its main crossing line start, as
    /// parts of its width and height (`line_edges`). Each rung is laid so they start on a pixel's edge.
    edges: Option<(f64, f64)>,
}

/// One of a sprite's sizes of pixel art: its height in the low frame's pixels, its picture and texture.
struct Rung {
    h: f64,
    /// How far its picture was moved, in its pixels, to lay its lines on the pixels' edges (`edges`):
    /// drawn as far back, so it stands where it is.
    shift: (f64, f64),
    surface: Option<ImageSurface>,
    tex: Option<Tex>,
}

/// How much bigger each of a ladder's rungs is than the one before: a tree's, and a picture of lines'
/// (a pole's). Between two rungs the lower is drawn whole, grown to the height, and the upper over it
/// (`Road::emit`). A picture of lines is drawn from its nearest rung alone (its edges are covered in
/// part, `lined`): its rungs are close enough that one differs from the next by a fraction of a pixel.
const RUNG: f64 = 1.1;
const RUNG_LINES: f64 = 1.03;

impl Sprite {
    /// The rung to draw it from at a height of `h` low-frame pixels: the smallest at least that tall,
    /// or the tallest; None without a ladder.
    /// A rung's width for its height `h`, and how far its picture is moved to lay its lines on the
    /// pixels' edges (`edges`; none without). An even width, so a picture symmetric about its middle
    /// stays so on the pixels.
    fn rung_size(&self, h: f64) -> (f64, (f64, f64)) {
        let w = (h * self.size.0 / self.size.1 / 2.0).round().max(1.0) * 2.0;
        let shift = self.edges.map_or((0.0, 0.0), |(x, y)| ((x * w).round() - x * w, (y * h).round() - y * h));
        (w, shift)
    }

    /// The rung for a rectangle on the screen `pix` pixels to the low frame's, and the rectangle moved
    /// back by as much as the rung's picture was moved (`Rung::shift`), `mirrored` or not.
    fn laid(&self, r: (f64, f64, f64, f64), mirrored: bool, pix: f64) -> (Option<&Rung>, (f64, f64, f64, f64)) {
        let rung = self.rung((r.3 - r.1).abs() / pix);
        let Some(g) = rung.filter(|g| g.shift != (0.0, 0.0)) else { return (rung, r) };
        let dx = (r.2 - r.0) / self.rung_size(g.h).0 * g.shift.0 * if mirrored { -1.0 } else { 1.0 };
        let dy = (r.3 - r.1) / g.h * g.shift.1;
        (rung, (r.0 - dx, r.1 - dy, r.2 - dx, r.3 - dy))
    }

    /// The rungs either side of height `h` and how far it is from the lower to the upper, so it grows
    /// from one into the other rather than stepping. None without a ladder.
    fn between(&self, h: f64) -> Option<(&Rung, &Rung, f64)> {
        let hi = self.ladder.iter().position(|r| r.h >= h)?;
        let (a, b) = (&self.ladder[hi.saturating_sub(1)], &self.ladder[hi]);
        let t = if b.h > a.h { ((h - a.h) / (b.h - a.h)).clamp(0.0, 1.0) } else { 1.0 };
        Some((a, b, t))
    }

    fn rung(&self, h: f64) -> Option<&Rung> {
        self.ladder.iter().find(|r| r.h >= h).or(self.ladder.last())
    }

    /// The SVG at `path`, drawn `tall` pixels tall (or as wide, for one wider than tall).
    fn of(path: &std::path::Path, tall: f64, foot: f64) -> Sprite {
        let size = svg_size(path).unwrap_or((100.0, 100.0));
        let k = (tall / size.1.max(size.0)).max(1e-3);
        let (w, h) = ((size.0 * k).ceil().max(1.0) as i32, (size.1 * k).ceil().max(1.0) as i32);
        Sprite { surface: Some(svg_layer(path, w, h, k, 0.0, 0.0, None)), kind: None, tex: None, ladder: vec![], size, foot, edges: None }
    }
    fn picture(surface: ImageSurface, size: (f64, f64)) -> Sprite {
        Sprite { surface: Some(surface), kind: None, tex: None, ladder: vec![], size, foot: 0.5, edges: None }
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
/// The free pixels a far pole needs between it and the one before it on its side, to be seen.
const POLE_CLEAR: f64 = 3.0;
/// How far a lit lamp lights its own pole, at night: its tint that far towards the lamp's colour.
const POLE_LIT_BY: f64 = 0.6;

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

pub struct Road {
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
    /// The CRT as it shows the fine frame, and the pass that makes the fine frame its rows (`RESOLVE`).
    crt_fine: Option<Option<Program>>,
    resolve: Option<Option<Program>>,
    /// The fine frame the things that pass by are drawn into, FINE times the low frame each way, and
    /// its rows as the CRT's beams take them; None without them (the low frame alone, then).
    fine: Option<Option<(Tex, Tex)>>,
    /// The low frame the scene is drawn into, which the CRT shows; None without one (no grid, or a
    /// GL without render targets: then the shaders keep to the grid themselves, `grid`).
    frame: Option<Option<Tex>>,
    grid: f64,
    /// The road's table, as a picture a row high, and its texture.
    table: ImageSurface,
    table_tex: Option<Tex>,
    /// Whether the table goes to the road's shader as uniforms (`TABLE_UNIFORMS`), once asked.
    table_uniforms: Option<bool>,
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

impl Road {
    fn new(g: &Geo, t: &Table) -> Road {
        let s: Settings = settings(t);
        let s_seed = s.seed;
        let [rise, set] = g.th.tw.day;
        let (w, h) = (g.w as f64, g.h as f64);
        let k = (w / CANVAS.0).max(h / CANVAS.1);
        let fit = (k, (w - CANVAS.0 * k) / 2.0, h - CANVAS.1 * k);
        let pix = if s.pixels > 0.0 { (h / s.pixels).round().max(1.0) } else { 1.0 };
        let levels = s.colours.round();
        Road {
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
            crt_fine: None,
            resolve: None,
            fine: None,
            frame: None,
            grid: pix,
            table: surface(ROWS as i32, 1),
            table_tex: None,
            table_uniforms: None,
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

    /// The sprites as the low frame shows them, once: on the grid, the car, the trees and the poles are
    /// pixel art, made at the size they are seen in the low frame (the car exactly, the trees and poles
    /// near), each texel a sharp pixel there. The still picture is drawn with these, as the live one is.
    fn art(&mut self, g: &Geo) {
        if self.sprites.iter().all(|sp| sp.kind.is_some()) {
            return;
        }
        let art = self.pix > 1.0;
        let frame_h = self.s.pixels.max(1.0);
        let per = self.fit.0 / self.pix;
        let k = CAR_WIDTH / self.s.car_span;
        // each sprite made pixel art: its height there, its levels of light, whether it is leaves
        let plans: Vec<Option<(f64, f64, bool)>> = (0..self.sprites.len())
            .map(|i| {
                let sp = &self.sprites[i];
                let tall = if (CARS..CARS + VIEWS).contains(&i) {
                    Some((sp.size.1 * k * per, 10.0, false))
                } else if i == POLE || i == POLE_LIT {
                    Some((POLE_TALL * scale(16.0) * per, 7.0, false))
                } else if self.trees.contains(&i) {
                    Some((1.1 * scale(16.0) * per, 6.0, true))
                } else {
                    None
                };
                // within what a picture can be, whatever the settings
                tall.filter(|&(h, _, _)| art && sp.surface.is_some() && (1.0..=ART_MAX).contains(&h) && h * sp.size.0 / sp.size.1 <= ART_MAX)
            })
            .collect();
        // a tree's or a pole's rungs: from a few pixels to taller than the frame, in steps of RUNG (a tree's) or RUNG_LINES (a pole's)
        let rungs = |i: usize| -> Vec<f64> {
            let sp = &self.sprites[i];
            let top = (frame_h * 1.5).min(ART_MAX / (sp.size.0 / sp.size.1).max(1.0));
            let step = if plans[i].is_some_and(|p| p.2) { RUNG } else { RUNG_LINES };
            let mut heights = vec![];
            let mut r = 3.0;
            while r < top {
                heights.push(r.round());
                r = (r * step).max(r + 1.0);
            }
            heights.dedup();
            heights
        };
        let laddered: Vec<(usize, Vec<f64>)> =
            plans.iter().enumerate().filter(|&(i, p)| p.is_some() && !(CARS..CARS + VIEWS).contains(&i)).map(|(i, _)| (i, rungs(i))).collect();
        // a picture of lines, not leaves (a pole): where its lines start, to lay each rung's on the pixels
        for (i, _) in &laddered {
            let leaves = plans[*i].is_some_and(|p| p.2);
            let sp = &mut self.sprites[*i];
            sp.edges = if leaves { None } else { sp.surface.as_ref().map(line_edges) };
        }
        // the rungs are the same each start: kept in the theme's cache
        let make = || {
            let mut all = vec![];
            for (i, heights) in &laddered {
                let (sp, (_, levels, leaves)) = (&self.sprites[*i], plans[*i].unwrap_or_default());
                let Some(s) = sp.surface.as_ref() else { continue };
                // the leaves' clusters as the pixel art at its near height has them
                let near = plans[*i].map_or(1.0, |p| p.0);
                let clusters = (((near * sp.size.0 / sp.size.1 / 2.0).round() * 2.0 / 2.0).floor(), (near.round() / 2.0).floor());
                // each made from a picture at least twice its height, halved down from the SVG's as far
                // as that allows: as fine, with a fraction of the pixels to filter
                let mut chain = vec![s.clone()];
                while let Some(last) = chain.last().filter(|l| l.height() >= 12) {
                    chain.push(halved(last));
                }
                for &h in heights {
                    let from = chain.iter().rev().find(|c| c.height() as f64 >= 2.0 * h).unwrap_or(s);
                    let (w, shift) = sp.rung_size(h);
                    all.push(pixel_art_with(
                        from,
                        w as i32,
                        h as i32,
                        levels,
                        leaves.then_some(clusters),
                        false,
                        sp.edges.map(|_| (shift, near)),
                        cairo::Filter::Good,
                    ));
                }
            }
            all
        };
        let files: Vec<&String> = self.s.trees.iter().chain([&self.s.pole, &self.s.pole_lit]).collect();
        let tag = format!(
            "{} {} {} {:?} {:?}",
            self.pix,
            self.fit.0,
            frame_h,
            laddered,
            files.iter().map(|f| crate::svg::fnv(&std::fs::read(Self::path(g, f)).unwrap_or_default())).collect::<Vec<_>>()
        );
        let count: usize = laddered.iter().map(|l| l.1.len()).sum();
        let kept =
            g.th.dirs
                .first()
                .and_then(|dir| unpack_surfaces(&crate::svg::kept_bytes(&dir.join("theme.toml"), g.w, g.h, "racing-art", &tag, || pack_surfaces(&make()))));
        let pictures = kept.filter(|p| p.len() == count).unwrap_or_else(make);
        let mut pictures = pictures.into_iter();
        for (i, heights) in &laddered {
            let sp = &self.sprites[*i];
            let ladder = heights.iter().map(|&h| Rung { h, shift: sp.rung_size(h).1, surface: pictures.next(), tex: None }).collect();
            self.sprites[*i].ladder = ladder;
            self.sprites[*i].surface = None;
        }
        for (i, plan) in plans.iter().enumerate() {
            let sp = &mut self.sprites[i];
            let car = (CARS..CARS + VIEWS).contains(&i);
            sp.kind = Some(match (plan, &sp.surface) {
                (Some((h, levels, leaves)), Some(s)) if car => {
                    let w = (h * sp.size.0 / sp.size.1 / 2.0).round() * 2.0;
                    sp.surface = Some(pixel_art(s, w as i32, h.round() as i32, *levels, *leaves, true));
                    // the car is drawn at its own size on the frame's pixels: sharp
                    Kind::Pixel
                }
                _ => Kind::Sprite,
            });
        }
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
        // widths from the centre, its picture `tall` half widths tall, `own` how far its own lamp lights
        // it, `show` how strongly it is seen there; hidden below a crest before it
        let stand = |items: &mut Vec<(f64, Vec<Placed>)>,
                     sprite: usize,
                     at: f64,
                     side: f64,
                     out: f64,
                     tall: f64,
                     flip: bool,
                     own: f64,
                     show: f64,
                     extra: &dyn Fn(f64, f64, f64, f64, f64) -> Vec<Placed>| {
            let z = at - travel;
            if z < NEAR || show <= 0.0 {
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
            let a = show;
            let clip_screen = self.at(0.0, clip).1;
            let mut p = self.place(sprite, (x0, foot_y - h, x0 + wd, foot_y), mixed(mixed(tint, lamp, own), beam, lit(z, side * out + x)), a);
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
        // a pole fades in out of the haze as a tree does, and only once it stands clear of the pole
        // before it on its side by POLE_CLEAR pixels: else a flat road's far poles are a lump along it
        let per = self.fit.0 / self.pix;
        let foot = |i: i64| {
            let z = i as f64 * spacing - travel;
            let side = if i.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
            self.ahead.at(z).map(|(_, x, _)| road_x(z, side * 1.13 + x))
        };
        let pole_shown = |i: i64| -> f64 {
            let z = i as f64 * spacing - travel;
            let clear = match (foot(i), foot(i - 2)) {
                (Some(a), Some(b)) if (i - 2) as f64 * spacing - travel >= NEAR => (a - b).abs() * per - 1.0,
                _ => f64::INFINITY,
            };
            smoothstep(FAR, FAR * 0.7, z) * smoothstep(POLE_CLEAR, POLE_CLEAR + 1.0, clear)
        };
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
            // a lit lamp lights its own pole: else, past the headlights' reach, a dark pole is lost
            // against the night sky, and its lamp floats with nothing under it. Seen as far as the road
            // is, the lamps coming out of the distance with it: from where the trees stop, a lamp on
            // the road already in view came on out of nothing
            stand(&mut items, sprite, i as f64 * spacing, side, 1.13, POLE_TALL, side > 0.0, POLE_LIT_BY * level * dark, pole_shown(i), &glows);
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
                    // far ones fade in out of the haze, rather than appear
                    let show = smoothstep(FAR, FAR * 0.7, at - travel);
                    stand(&mut items, sprite, at, side, min + 2.2 * out * out, tall, false, 0.0, show, &none);
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
        lamp_pools(self.seed, self.s.pole_spacing, l.day, self.dark(l), travel, t)
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
                crate::errln!("road: a shader does not compile: {e}");
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
        let code = |row: &Row| match *row {
            Row::Sky => (0, 0),
            Row::Beyond => (u16::MAX, 0),
            Row::Road { z, x } => {
                (((z / 128.0 * 65533.0).round() as u16).clamp(1, 65534), (((x + 256.0) / 512.0 * 65535.0).round().clamp(0.0, 65535.0)) as u16)
            }
        };
        if let Some(mut d) = view(&mut self.table) {
            for (i, (zc, xc)) in self.ahead.rows.iter().map(code).enumerate() {
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
    let mut cell = (i64::MIN, None);
    while z < Z_END {
        let next = (z * 1.012).min(Z_END);
        if next > zc {
            // the curvature summed twice from the car on: straight on at the car
            let from = z.max(zc);
            let dz = next - from;
            let c = course.curvature(s + (from + dz / 2.0 - zc));
            x += slope_x * dz + c * dz * dz / 2.0;
            slope_x += c * dz;
        }
        z = next;
        // the hills along the walk, each worked out once: the steps are many to a hill
        let at = s + z - zc;
        let k = (at / HILL_CELL).floor() as i64;
        if cell.0 != k {
            cell = (k, hill(seed, k));
        }
        let dh = height_only(at, cell.1) - h_car - tilt * (z - zc);
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

/// The street lamps' light on the road at `travel` (each one's distance ahead, its side, its brightness):
/// the lamps `spacing` apart, `dark` how dark it is, `day` the daylight, `t` the scene's clock.
fn lamp_pools(seed: i64, spacing: f64, day: f64, dark: f64, travel: f64, t: f64) -> [[f32; 4]; POOLS] {
    // the lamps over a stretch of road that holds at most POOLS of them, each one's light fading in
    // over the far end of it: cut off there, a pool flashed on where the road ahead rises into view
    let (near, far) = (NEAR - 4.0, (NEAR - 4.0 + (POOLS - 1) as f64 * spacing).min(FAR));
    let first = ((travel + near) / spacing).floor() as i64;
    let lit: Vec<[f32; 4]> = (first..first + POOLS as i64)
        .filter_map(|i| {
            let z = i as f64 * spacing - travel;
            let k = lamp_level(seed, i, day, t) * dark * (1.0 - smoothstep(far - 2.0 * spacing, far, z));
            let side = if i.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
            // under the lamp's head, out over the road from the pole at its edge
            (k > 0.01 && z <= far).then_some([z as f32, (side * (1.13 - POLE_TALL * 65.0 / 300.0)) as f32, k as f32, 0.0])
        })
        .collect();
    std::array::from_fn(|n| lit.get(n).copied().unwrap_or([0.0; 4]))
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

impl Part for Road {
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
        self.art(g);
        for sp in self.sprites.iter_mut() {
            // a rung is shrunk by little or not at all: filtered, without mipmaps; all of them on a page
            // or two, so the trees and poles go to the screen in a draw or two
            for r in sp.ladder.iter_mut() {
                r.tex = r.surface.take().map(|s| up.add_packed(s));
            }
            let Some(s) = sp.surface.take() else { continue };
            sp.tex = Some(up.add(s, sp.kind.unwrap_or(Kind::Sprite)));
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
        let frame =
            *self.frame.get_or_insert_with(|| if pix > 1.0 { gfx.render_target((w / pix).ceil() as i32, (h / pix).ceil() as i32, false) } else { None });
        let fine = *self.fine.get_or_insert_with(|| {
            let (fw, fh) = gfx.size(frame?);
            Some((gfx.render_target(fw * FINE, fh * FINE, true)?, gfx.render_target(fw * FINE, fh, true)?))
        });
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
        // the table as uniforms where the driver has room for them, else as a texture
        let as_uniforms = *self.table_uniforms.get_or_insert_with(|| gfx.uniform_room() >= TABLE_ROOM);
        let mut tablef = vec![];
        if as_uniforms {
            let _ = self.table.with_data(|d| {
                for p in d[..ROWS * 4].as_chunks::<4>().0 {
                    tablef.push((p[2] as u32 * 256 + p[1] as u32) as f32);
                    tablef.push((p[0] as u32 * 256 + p[3] as u32) as f32);
                }
            });
        } else if let Some(tex) = self.table_tex {
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
                .replace("VIEW", &format!("{VIEW:.4}"))
                .replace("TABLE_DECL", if as_uniforms { TABLE_UNIFORMS.0 } else { TABLE_TEXTURE.0 })
                .replace("TABLE_READ", if as_uniforms { TABLE_UNIFORMS.1 } else { TABLE_TEXTURE.1 });
            let mut p = Self::compile(gfx, &src, &ROAD_UNIFORMS)?;
            p.at.push(gfx.uniform(p.prog, if as_uniforms { "table" } else { "tex0" }));
            p.at.push(gfx.uniform(p.prog, "lampc"));
            p.at.extend((0..POOLS).map(|k| gfx.uniform(p.prog, &format!("pools[{k}]"))));
            Some(p)
        });
        if let (Some(p), Some(tex)) = (road_prog, self.table_tex) {
            let mut all: Vec<_> = p.at.iter().zip(&u).zip(ROAD_SIZES).map(|((&at, v), n)| (at, n, *v)).collect();
            let n = ROAD_UNIFORMS.len();
            all.push((p.at[n + 1], 3, [lampc.0 as f32, lampc.1 as f32, lampc.2 as f32, 0.0]));
            all.extend(p.at[n + 2..].iter().zip(pools).map(|(&at, v)| (at, 4, v)));
            let at = p.at[ROAD_UNIFORMS.len()];
            if as_uniforms {
                gfx.uniform_array(at, tablef);
                gfx.custom(p.prog, &all, &[], 0.0, top, w, h, Blend::Premultiplied);
            } else {
                gfx.custom(p.prog, &all, &[(at, tex)], 0.0, top, w, h, Blend::Premultiplied);
            }
        }
        // what passes by goes into the fine frame, over the low one made FINE times as fine, so it
        // moves smoothly: on a tube's rows a thing between them lights both, and along a row the beam
        // is a smooth signal, not pixels; on the low frame it went from pixel to pixel
        if let (Some(f), Some((fine_tex, _))) = (frame, fine) {
            gfx.target(Some(fine_tex));
            let q: Vec<f32> = image_quad(0.0, 0.0, w, h, (1.0, 1.0, 1.0), 1.0, (0.0, 1.0, 1.0, 0.0)).iter().map(|&v| v as f32).collect();
            gfx.draw(Some(f), &q, Blend::Opaque);
        }
        let mut front = vec![];
        self.front(&l, travel, self.t, &mut front);
        self.emit(gfx, &front);
        if let (Some(_), Some((fine_tex, rows))) = (frame, fine) {
            let (fw, fh) = gfx.size(fine_tex);
            gfx.target(Some(rows));
            let resolve = self.resolve.get_or_insert_with(|| {
                let src = RESOLVE.replace("REACH", &(FINE * 3 / 2).to_string()).replace("FINE", &format!("{FINE}.0"));
                let mut p = Self::compile(gfx, &src, &CRT_UNIFORMS[..1])?;
                p.at.push(gfx.uniform(p.prog, "tex0"));
                Some(p)
            });
            if let Some(p) = resolve {
                gfx.custom(p.prog, &[(p.at[0], 4, [fw as f32, fh as f32, HARD_PIX as f32, 0.0])], &[(p.at[1], fine_tex)], 0.0, 0.0, w, h, Blend::Opaque);
            }
            gfx.target(None);
            let crt = self.crt_fine.get_or_insert_with(|| {
                let mut p = Self::compile(gfx, &format!("{CRT_COMMON}{CRT_FINE}"), &CRT_UNIFORMS[..3])?;
                p.at.push(gfx.uniform(p.prog, "tex0"));
                Some(p)
            });
            if let Some(p) = crt {
                let (rw, rh) = gfx.size(rows);
                let values = [[rw as f32, rh as f32, -8.0, 0.0], [0.65, 1.35, 1.7, 0.0], [0.0, 0.0, w as f32, h as f32]];
                let all: Vec<_> = p.at.iter().zip(values).map(|(&at, v)| (at, 4, v)).collect();
                gfx.custom(p.prog, &all, &[(p.at[3], rows)], 0.0, 0.0, w, h, Blend::Opaque);
            }
        } else if let Some(f) = frame {
            gfx.target(None);
            let crt = self.crt.get_or_insert_with(|| {
                let mut p = Self::compile(gfx, &format!("{CRT_COMMON}{CRT}"), &CRT_UNIFORMS[..3])?;
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
        self.art(g);
        self.advance();
        let (l, travel, _) = self.moment();
        let sky = self.sky_look(&l);
        let paint = |c: &Context| {
            self.paint_sky(g, c, &l, &sky);
            let mut back = vec![];
            self.back(&l, &sky, self.t, &mut back);
            self.paint_placed(c, &back);
            self.paint_road(g, c, &l, &sky, travel);
            let mut front = vec![];
            self.front(&l, travel, self.t, &mut front);
            self.paint_placed(c, &front);
        };
        if self.pix <= 1.0 {
            paint(c);
            return;
        }
        // into the low frame, as the live picture is, then onto the screen as the CRT shows it
        // a grid coarser than the screen is one beam over all of it
        let pix = (self.pix as usize).clamp(1, g.h.max(1) as usize);
        let low = surface((g.w as usize).div_ceil(pix) as i32, (g.h as usize).div_ceil(pix) as i32);
        {
            let lc = ctx(&low);
            lc.scale(1.0 / pix as f64, 1.0 / pix as f64);
            paint(&lc);
        }
        let mut screen = surface(g.w, g.h);
        beams(&low, &mut screen, pix);
        if c.set_source_surface(&screen, 0.0, 0.0).is_ok() {
            let _ = c.paint();
        }
    }
}

impl Road {
    /// Sprites as quads: tinted from top to foot, cut off at the crest in front of them; the car on the
    /// low frame's pixels.
    fn emit(&mut self, gfx: &mut Gfx, placed: &[Placed]) {
        let snap = |v: f64, g: f64| (v / g).round() * g;
        let grid = if self.frame.flatten().is_some() { self.pix } else { 1.0 };
        for s in placed {
            let sp = &self.sprites[s.sprite];
            // a laddered picture between two of its rungs: the lower whole, the upper over it as far as
            // the height is from one to the other, so it grows from one into the next rather than
            // stepping; each read where its picture was laid (`Rung::shift`). Else the one picture
            let h = (s.rect.3 - s.rect.1).abs() / self.pix;
            let layers: Vec<(Tex, (f64, f64), f64)> = match sp.between(h) {
                // a picture of lines, covered in part where its edges fall: its rungs a few hundredths
                // apart differ by a fraction of a pixel, so the nearest serves; drawn over one another,
                // two rungs' part-covered edges would add up and darken as it grew
                Some((a, b, t)) if sp.edges.is_some() => {
                    let r = if t < 0.5 { a } else { b };
                    r.tex.map(|tex| (tex, (r.shift.0 / sp.rung_size(r.h).0, r.shift.1 / r.h), 1.0)).into_iter().collect()
                }
                Some((a, b, t)) => [(a, 1.0), (b, t)]
                    .into_iter()
                    .filter(|&(_, k)| k > 0.0)
                    .filter_map(|(r, k)| Some((r.tex?, (r.shift.0 / sp.rung_size(r.h).0, r.shift.1 / r.h), k)))
                    .collect(),
                None => sp.rung(h).map_or(sp.tex, |r| r.tex).map(|t| (t, (0.0, 0.0), 1.0)).into_iter().collect(),
            };
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
            let blend = match s.mode {
                Mode::Solid => Blend::Premultiplied,
                Mode::Light(b) => b,
            };
            for (tex, (du, dv), k) in layers {
                let mut q = image_quad(x0, ya, x1, yb, top.0, top.1 * k, (u0 + du, v0 + dv, u1 + du, v1 + dv));
                for corner in [2usize, 3] {
                    q[corner * 8 + 2..corner * 8 + 6].copy_from_slice(&[foot.0.0 * foot.1 * k, foot.0.1 * foot.1 * k, foot.0.2 * foot.1 * k, foot.1 * k]);
                }
                let data: Vec<f32> = q.iter().map(|&v| v as f32).collect();
                gfx.draw(Some(tex), &data, blend);
            }
        }
    }

    /// Sprites drawn with cairo, for the still picture, as `emit` has the GPU draw them: each texel its
    /// colour times the tint, from the tint at its top to the one at its foot; the car on the low
    /// frame's pixels, sharp.
    fn paint_placed(&self, c: &Context, placed: &[Placed]) {
        let snap = |v: f64, g: f64| (v / g).round() * g;
        for p in placed {
            let sp = &self.sprites[p.sprite];
            let (rung, rect) = sp.laid(p.rect, p.st.0 > p.st.2, self.pix);
            let Some(s) = rung.map_or(sp.surface.as_ref(), |r| r.surface.as_ref()) else { continue };
            let car = (CARS..CARS + VIEWS).contains(&p.sprite);
            let g = if car { self.pix.max(1.0) } else { 1.0 };
            let (x0, y0) = (snap(rect.0, g), snap(rect.1, g));
            let (x1, y1) = (x0 + snap(rect.2 - rect.0, g), y0 + snap(rect.3 - rect.1, g));
            let (u0, v0, u1, v1) = p.st;
            // too small to see, or too large to draw: cairo's matrix would not invert
            if !((0.5..1e6).contains(&(x1 - x0).abs()) && (0.5..1e6).contains(&(y1 - y0).abs())) || u1 == u0 || v1 == v0 {
                continue;
            }
            let picture = tinted(s, p.top, p.foot, v0, v1);
            let _ = c.save();
            if p.clip.is_finite() {
                c.rectangle(-1e5, -1e5, 2e5, p.clip + 1e5);
                c.clip();
            }
            c.rectangle(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
            c.clip();
            c.set_operator(match p.mode {
                Mode::Light(Blend::Add) => Operator::Add,
                Mode::Light(_) => Operator::Screen,
                Mode::Solid => Operator::Over,
            });
            let (w, h) = (s.width() as f64, s.height() as f64);
            let (sx, sy) = ((x1 - x0) / ((u1 - u0) * w), (y1 - y0) / ((v1 - v0) * h));
            c.translate(x0 - u0 * w * sx, y0 - v0 * h * sy);
            c.scale(sx, sy);
            let source = cairo::SurfacePattern::create(&picture);
            if car {
                source.set_filter(cairo::Filter::Nearest);
            }
            if c.set_source(&source).is_ok() {
                let _ = c.paint();
            }
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
        name: "road",
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
        make: |_, t, g| Ok(vec![Box::new(Road::new(g, t))]),
    }]
}
#[cfg(test)]
mod tests;
