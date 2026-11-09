//! A painted pirate island: SVG layers on one canvas, drawn at screen size. Clouds drift behind the
//! island, the sea sways with light moving on it, and the volcano's plume billows in four sections, each
//! about its own pivot. The island, rocks and ships stand still in front.
//!
//! Every layer, and every piece cleaned from one, is drawn once per screen size and cached on disk
//! (`svg::kept`): the island's SVG is slow to draw.

use crate::config::Theme;
use crate::elements::{Validate, check, each, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Patch, Tex, Uploads};
use crate::scene::*;
use crate::svg::{fnv, kept, kept_value, svg_head, svg_layer, svg_size};
use crate::util::*;
use cairo::{Context, Filter, Format, ImageSurface, LinearGradient, Matrix, Operator, SurfacePattern};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::PathBuf;
use std::sync::Arc;

type Box4 = (i32, i32, i32, i32);
type DrawnKey = (PathBuf, Vec<String>, i32, i32, (u64, u64));

thread_local! {
    /// Each layer at each screen size, drawn or read from the cache once while a scene is built.
    static DRAWN: RefCell<HashMap<DrawnKey, ImageSurface>> = RefCell::new(HashMap::new());
    static SIZES: RefCell<HashMap<PathBuf, Option<(f64, f64)>>> = RefCell::new(HashMap::new());
}

/// A scene is built: its layers are not asked for again (the disk cache has them for the next).
pub fn built() {
    DRAWN.with(|d| d.borrow_mut().clear());
}

pub fn crop(s: &ImageSurface, b: Box4) -> ImageSurface {
    let (x0, y0, x1, y1) = b;
    let out = surface(x1 - x0, y1 - y0);
    {
        let c = ctx(&out);
        let _ = c.set_source_surface(s, -x0 as f64, -y0 as f64);
        let _ = c.paint();
    }
    out
}

/// A mask (w by h, 0 or 1 or any byte) as an A8 surface, each value times `k`.
fn a8(mask: &[u8], w: i32, h: i32, k: u8) -> ImageSurface {
    let mut m = surface_of(Format::A8, w, h);
    let stride = m.stride() as usize;
    {
        let mut d = m.data().unwrap();
        for y in 0..h as usize {
            for x in 0..w as usize {
                d[y * stride + x] = mask[y * w as usize + x] * k;
            }
        }
    }
    m
}

/// The 4-connected pieces of a mask (w by h), each a list of pixel indices.
fn pieces(mask: &[u8], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    for start in 0..w * h {
        if mask[start] == 0 || seen[start] {
            continue;
        }
        let (mut stack, mut members) = (vec![start], vec![start]);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let x = i % w;
            let near = [if x > 0 { Some(i - 1) } else { None }, if x < w - 1 { Some(i + 1) } else { None }, i.checked_sub(w), Some(i + w)];
            for j in near.into_iter().flatten() {
                if j < w * h && mask[j] != 0 && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                    members.push(j);
                }
            }
        }
        out.push(members);
    }
    out
}

/// A cloud layer cleaned of what is not cloud.
pub fn cleaned(s: &ImageSurface, speck: f64) -> ImageSurface {
    let (w, h) = (s.width(), s.height());
    let k = 2.0;
    let mut small = resampled(s, 1.0 / k, Filter::Good);
    let (sw, sh, stride) = (small.width() as usize, small.height() as usize, small.stride() as usize);
    let Some(data) = view(&mut small) else { return s.clone() };
    let mut keep = vec![0u8; sw * sh];
    for y in 0..sh {
        for x in 0..sw {
            let i = y * stride + 4 * x;
            let a = data[i + 3] as i32;
            if a < 24 {
                continue;
            }
            let (b, g, r) = (data[i] as i32 * 255 / a, data[i + 1] as i32 * 255 / a, data[i + 2] as i32 * 255 / a);
            let orange = r > b + 40 && r > g + 25;
            let mean = (r + g + b) as f64 / 3.0;
            let grey = mean < 160.0 && r.max(g).max(b) - r.min(g).min(b) < 45;
            if !(orange || grey || mean < 90.0) {
                keep[y * sw + x] = 1;
            }
        }
    }
    let limit = speck * sw as f64 * sh as f64;
    for piece in pieces(&keep, sw, sh) {
        if (piece.len() as f64) < limit {
            for i in piece {
                keep[i] = 0;
            }
        }
    }
    let mask = a8(&keep, sw as i32, sh as i32, 255);
    let out = surface(w, h);
    {
        let c = ctx(&out);
        let _ = c.set_source_surface(s, 0.0, 0.0);
        c.scale(k, k);
        let p = SurfacePattern::create(&mask);
        p.set_filter(Filter::Bilinear);
        let _ = c.mask(&p);
    }
    out
}

/// Min or max filter of a mask (w by h) over the square r pixels around each pixel.
fn spread(mask: &[u8], w: usize, h: usize, r: usize, max: bool) -> Vec<u8> {
    let pick = |a: u8, b: u8| if max { a.max(b) } else { a.min(b) };
    let mut rows = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            rows[y * w + x] = (a..=b).map(|i| mask[y * w + i]).reduce(pick).unwrap();
        }
    }
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            out[y * w + x] = (a..=b).map(|j| rows[j * w + x]).reduce(pick).unwrap();
        }
    }
    out
}

/// Smoke without the strands left hanging off it.
pub fn unstranded(s: ImageSurface, r: usize, faint: u8, speck: f64) -> ImageSurface {
    let k = 2.0;
    let mut small = resampled(&s, 1.0 / k, Filter::Good);
    let (sw, sh, stride) = (small.width() as usize, small.height() as usize, small.stride() as usize);
    let Some(data) = view(&mut small) else { return s };
    let solid: Vec<u8> = (0..sh).flat_map(|y| (0..sw).map(move |x| (y, x))).map(|(y, x)| (data[y * stride + 4 * x + 3] >= faint) as u8).collect();
    let mut body = spread(&spread(&solid, sw, sh, r, false), sw, sh, r, true);
    for piece in pieces(&body, sw, sh) {
        if (piece.len() as f64) < speck * sw as f64 * sh as f64 {
            for i in piece {
                body[i] = 0;
            }
        }
    }
    let near = spread(&body, sw, sh, r, true);
    let thin: Vec<u8> = solid.iter().zip(&body).map(|(&s, &b)| (s != 0 && b == 0) as u8).collect();
    let mut gone = vec![0u8; sw * sh];
    for piece in pieces(&thin, sw, sh) {
        if piece.iter().any(|&i| near[i] == 0) {
            for i in piece {
                gone[i] = 1;
            }
        }
    }
    if !gone.iter().any(|&g| g != 0) {
        return s;
    }
    let gone = spread(&gone, sw, sh, 1, true);
    let mask = a8(&gone, sw as i32, sh as i32, 255);
    {
        let c = ctx(&s);
        c.set_operator(Operator::DestOut);
        c.scale(k, k);
        let p = SurfacePattern::create(&mask);
        p.set_filter(Filter::Bilinear);
        c.set_source_rgba(0.0, 0.0, 0.0, 1.0);
        let _ = c.mask(&p);
    }
    s
}

/// Smoke cleaned of the sky the tracing caught in it.
pub fn unskied(mut s: ImageSurface) -> ImageSurface {
    let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
    {
        let mut data = s.data().unwrap();
        for y in 0..h {
            for x in 0..w {
                let i = y * stride + 4 * x;
                let a = data[i + 3] as i32;
                if a != 0 {
                    let (b, r) = (data[i] as i32 * 255 / a, data[i + 2] as i32 * 255 / a);
                    let blue = b - r;
                    if blue > 40 {
                        let keep = (1.0 - (blue - 40) as f64 / 30.0).max(0.0);
                        for c in &mut data[i..i + 4] {
                            *c = (*c as f64 * keep).round() as u8;
                        }
                    }
                }
            }
        }
    }
    s
}

/// Boxes [x0, y0, x1, y1] on the canvas, as the tuples the cache's tags are written from.
fn boxes(b: &[[f64; 4]]) -> Vec<(f64, f64, f64, f64)> {
    b.iter().map(|r| (r[0], r[1], r[2], r[3])).collect()
}

/// A setting naming one of the theme's SVGs: it must name one.
fn named(name: &str, v: &str) -> Result<(), String> {
    if v.is_empty() { Err(format!("{name}: it names an SVG of the theme's")) } else { Ok(()) }
}

/// One layer: `file`, an SVG in the theme's directory on the shared canvas, drawn at screen size and
/// cropped about the theme's crop_centre, so all layers line up.
#[derive(Clone)]
struct Picture {
    file: String,
    th: Arc<Theme>,
}

impl Picture {
    fn path(&self, name: Option<&str>) -> PathBuf {
        self.th.path(name.unwrap_or(&self.file))
    }

    /// Where the canvas stands on the screen: (scale, x, y). Without the SVG's size (said), the screen's.
    fn place(&self, g: &Geo) -> (f64, f64, f64) {
        let p = self.path(None);
        let size = SIZES.with(|s| *s.borrow_mut().entry(p.clone()).or_insert_with(|| svg_size(&p)));
        self.th.covering(g.w, g.h, size.unwrap_or((g.w as f64, g.h as f64)))
    }

    /// A canvas point in screen coordinates.
    fn at(&self, g: &Geo, x: f64, y: f64) -> (f64, f64) {
        let (scale, ox, oy) = self.place(g);
        (ox + x * scale, oy + y * scale)
    }

    /// A layer at this screen's size, drawn once and cached.
    fn layer(&self, g: &Geo, name: Option<&str>, ids: &[&str]) -> ImageSurface {
        let name = name.unwrap_or(&self.file).to_string();
        let crop = self.th.tw.crop_centre;
        let key = (self.path(Some(&name)), ids.iter().map(|s| s.to_string()).collect(), g.w, g.h, (crop[0].to_bits(), crop[1].to_bits()));
        DRAWN.with(|d| {
            d.borrow_mut()
                .entry(key)
                .or_insert_with(|| {
                    let (scale, x, y) = self.place(g);
                    let mut sorted: Vec<&str> = ids.to_vec();
                    sorted.sort();
                    let kind = if ids.is_empty() { "layer".to_string() } else { format!("layer{:04x}", fnv(format!("{sorted:?}").as_bytes()) & 0xffff) };
                    let tag = format!("{sorted:?} {:?} {:?} {:?}", scale, x, y);
                    let path = self.path(Some(&name));
                    kept(&path, g.w, g.h, &kind, &tag, || svg_layer(&path, g.w, g.h, scale, x, y, if ids.is_empty() { None } else { Some(ids) }))
                })
                .clone()
        })
    }

    fn drawn(&self, g: &Geo, name: Option<&str>, ids: &[&str]) -> ImageSurface {
        self.layer(g, name, ids)
    }

    fn source(&self, g: &Geo, name: Option<&str>) -> ImageSurface {
        self.layer(g, name, &[])
    }

    /// Where a layer has anything drawn (`ink_box`), cached.
    fn ink(&self, g: &Geo, name: Option<&str>, pad: i32) -> Option<Box4> {
        let path = self.path(name);
        let v = kept_value(&path, g.w, g.h, "box", &format!("{pad} {:?}", self.place(g)), || match ink_box(&self.source(g, name), pad) {
            Some(b) => format!("{} {} {} {}", b.0, b.1, b.2, b.3),
            None => "none".into(),
        });
        let n: Vec<i32> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
    }

    /// A piece made from a layer, cached.
    fn piece(&self, g: &Geo, name: Option<&str>, kind: &str, tag: &str, make: impl FnOnce() -> ImageSurface) -> ImageSurface {
        kept(&self.path(name), g.w, g.h, kind, tag, make)
    }

    /// Cut and fade the theme's `erase` and `fades` boxes ([x0, y0, x1, y1] on the canvas) out of a
    /// piece standing at (left, top) on the screen.
    fn touch_up(&self, g: &Geo, piece: &ImageSurface, left: f64, top: f64, erase: &[(f64, f64, f64, f64)], fades: &[(f64, f64, f64, f64)]) {
        let (scale, ox, oy) = self.place(g);
        let at = |x: f64, y: f64| (ox + x * scale - left, oy + y * scale - top);
        let c = ctx(piece);
        c.set_operator(Operator::DestOut);
        let soft = 6.0 * scale;
        for &(x0, y0, x1, y1) in erase {
            if x1 > x0 && y1 > y0 {
                let ((a, b), (cc, d)) = (at(x0, y0), at(x1, y1));
                for k in 0..4 {
                    let gg = soft * (3 - k) as f64 / 3.0;
                    c.rectangle(a - gg, b - gg, cc - a + 2.0 * gg, d - b + 2.0 * gg);
                    c.set_source_rgba(0.0, 0.0, 0.0, if k < 3 { 0.25 } else { 1.0 });
                    let _ = c.fill();
                }
            }
        }
        for &(x0, y0, x1, y1) in fades {
            if x0 != x1 && y1 > y0 {
                let ((a, b), (cc, d)) = (at(x0, y0), at(x1, y1));
                let fade = LinearGradient::new(a, 0.0, cc, 0.0);
                fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 1.0);
                fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
                c.rectangle(a.min(cc) - if a < cc { soft } else { 0.0 }, b, (cc - a).abs() + soft, d - b);
                let _ = c.set_source(&fade);
                let _ = c.fill();
            }
        }
    }
}

// --- a still layer ---------------------------------------------------------------------------------

/// A still layer: the sky and the open sea behind, or the island in front.
struct Layer {
    pic: Picture,
}

impl Part for Layer {
    fn still(&self) -> bool {
        true
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        self.pic.ink(g, None, 4).map(|(a, b, c, d)| vec![(a as f64, b as f64, c as f64, d as f64)])
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let _ = c.set_source_surface(self.pic.drawn(g, None, &[]), 0.0, 0.0);
        let _ = c.paint();
    }
}

// --- the clouds ------------------------------------------------------------------------------------

/// Clouds drifting (live) behind the island: their layer moving slowly across the sky, faded out at
/// the sides and wrapping round, in whole pixels.
struct Clouds {
    pic: Picture,
    tex: Option<Tex>,
    speed: f64,
    erase: Vec<(f64, f64, f64, f64)>,
    fades: Vec<(f64, f64, f64, f64)>,
    /// Where its piece stands, once made; the piece until it is uploaded.
    made: Option<Box4>,
    piece: Option<ImageSurface>,
    shown: i64,
}

impl Clouds {
    fn make(&mut self, g: &Geo) -> (Box4, ImageSurface) {
        if let (Some(b), Some(p)) = (self.made, &self.piece) {
            return (b, p.clone());
        }
        let b = self.pic.ink(g, None, 4).unwrap_or((0, 0, 1, 1));
        let bx = (0, b.1, g.w, b.3);
        let x1 = bx.2;
        let tag = format!("{bx:?} {:?} {:?} {:?}", self.erase, self.fades, self.pic.place(g));
        let piece = self.pic.piece(g, None, "clouds", &tag, || {
            let piece = cleaned(&crop(&self.pic.source(g, None), bx), 0.0015);
            self.pic.touch_up(g, &piece, 0.0, bx.1 as f64, &self.erase, &self.fades);
            let c = ctx(&piece);
            c.set_operator(Operator::DestOut);
            for (edge, inward) in [(0.0, 1.0), (x1 as f64, -1.0)] {
                let fade = LinearGradient::new(edge, 0.0, edge + inward * 0.1 * g.w as f64, 0.0);
                fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 1.0);
                fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
                let _ = c.set_source(&fade);
                let _ = c.paint();
            }
            drop(c);
            piece
        });
        (self.made, self.piece) = (Some(bx), Some(piece.clone()));
        (bx, piece)
    }

    fn offset(&self, g: &Geo, t: f64) -> i64 {
        ((self.speed * unit(g.w, g.h) / 1080.0 * t).round() as i64).rem_euclid(g.w as i64)
    }
}

impl Part for Clouds {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, _g: &Geo) -> i32 {
        self.made.map_or(0, |m| m.1)
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        let (_, piece) = self.make(g);
        self.piece = None;
        self.tex = Some(up.add(piece, Kind::Label));
    }
    fn state(&mut self, g: &Geo, w: &mut World) -> u64 {
        self.shown = self.offset(g, w.clock);
        self.shown as u64
    }
    fn interval(&self) -> f64 {
        0.1
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (Some(tex), Some((x0, y0, x1, y1))) = (self.tex, self.made) else { return };
        let x = self.shown as f64;
        let (w, h) = ((x1 - x0) as f64, (y1 - y0) as f64);
        gfx.image(tex, x, y0 as f64, w, h, 1.0, Blend::Premultiplied);
        gfx.image(tex, x - g.w as f64, y0 as f64, w, h, 1.0, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (b, piece) = self.make(g);
        let _ = c.set_source_surface(&piece, 0.0, b.1 as f64);
        let _ = c.paint();
    }
}

// --- the sea ---------------------------------------------------------------------------------------

const MARGIN: i32 = 24;

type Band = Patch;

struct WavesMade {
    steps: i64,
    top: i32,
    /// Until they are uploaded.
    pictures: Option<WavePictures>,
}

struct WavePictures {
    sea: ImageSurface,
    light: ImageSurface,
    open: ImageSurface,
}

/// The sea (live): the painted sea swaying sideways band by band on two slow waves, more where nearer;
/// over it the wave highlights swaying the other way, with bands of light running across; in the open
/// sea a copy swaying `open` times as far.
struct Waves {
    pic: Picture,
    /// The sea's, the highlights' and the open sea's textures.
    texs: Option<[Tex; 3]>,
    light: String,
    land: String,
    sway: f64,
    open: f64,
    glow: f64,
    looped: f64,
    rate: i64,
    made: Option<WavesMade>,
    cache: HashMap<i64, (Vec<Band>, Vec<Band>, Vec<f32>)>,
    shown: i64,
}

impl Waves {
    fn make(&mut self, g: &Geo) {
        if self.made.is_some() {
            return;
        }
        let steps = 1.max((self.looped * self.rate as f64).round() as i64);
        let sea = self.pic.drawn(g, None, &[]);
        let light = self.pic.drawn(g, Some(&self.light), &["wave-highlights"]);
        let top = (self.pic.at(g, 0.0, 344.0).1.floor() as i32).min(g.h);
        let (m, w, h) = (MARGIN, g.w, g.h - top);
        let mut made = vec![];
        for (layer, pad) in [(&sea, true), (&light, false)] {
            let s = surface(w + 2 * m, h);
            {
                let c = ctx(&s);
                let p = SurfacePattern::create(layer);
                if pad {
                    p.set_extend(cairo::Extend::Pad);
                }
                p.set_matrix(Matrix::new(1.0, 0.0, 0.0, 1.0, -m as f64, top as f64));
                let _ = c.set_source(&p);
                let _ = c.paint();
            }
            made.push(s);
        }
        let light = made.pop().unwrap();
        let sea = made.pop().unwrap();
        let open = self.openness(g, &sea, top);
        self.made = Some(WavesMade { steps, top, pictures: Some(WavePictures { sea, light, open }) });
    }

    /// The sea, faded by distance from the islands (their layer, blurred wide) and toward the lower
    /// left: none at the shores, full far out.
    fn openness(&self, g: &Geo, sea: &ImageSurface, top: i32) -> ImageSurface {
        let land = self.pic.drawn(g, Some(&self.land), &[]);
        let k = 16.0;
        let small = resampled(&land, 1.0 / k, Filter::Good);
        let near = resampled(&resampled(&small, 1.0 / 4.0, Filter::Good), 4.0 * k, Filter::Bilinear);
        let (sw, sh) = (sea.width(), sea.height());
        let weight = surface(sw, sh);
        {
            let c = ctx(&weight);
            let lean = LinearGradient::new(sw as f64, 0.0, 0.0, sh as f64);
            lean.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.35);
            lean.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
            let _ = c.set_source(&lean);
            let _ = c.paint();
            c.set_operator(Operator::DestOut);
            let _ = c.set_source_surface(&near, MARGIN as f64, -top as f64);
            let _ = c.paint_with_alpha(1.0);
            let _ = c.set_source_surface(&near, MARGIN as f64, -top as f64);
            let _ = c.paint_with_alpha(1.0);
        }
        let out = surface(sw, sh);
        {
            let c = ctx(&out);
            let _ = c.set_source_surface(sea, 0.0, 0.0);
            let _ = c.mask_surface(&weight, 0.0, 0.0);
        }
        out
    }

    fn shift(&self, g: &Geo, r: i32, depth: i32, f: f64, sign: f64) -> i32 {
        let k = unit(g.w, g.h) / 1080.0;
        let near = (r as f64 / depth as f64).powf(0.8);
        let s = (sign
            * self.sway
            * k
            * (0.2 + 0.8 * near)
            * (0.65 * (2.0 * PI * (r as f64 / 31.0 - f)).sin() + 0.35 * (2.0 * PI * (r as f64 / 13.0 + 2.0 * f)).sin()))
        .round() as i32;
        (-MARGIN).max(MARGIN.min(s))
    }

    /// The sea's bands at a step; `far` scales the sway.
    fn bands(&self, g: &Geo, step: i64, far: f64, phase: f64) -> Vec<Band> {
        let made = self.made.as_ref().unwrap();
        let (f, w, m, top) = (step as f64 / made.steps as f64, g.w, MARGIN, made.top);
        let depth = g.h - top;
        let mut out = vec![];
        let mut r = 0;
        while r < depth {
            let end = depth.min(r + if (r as f64) < depth as f64 / 4.0 { 2 } else { 3 });
            let x = m + self.shift(g, r, depth, f + phase, far);
            out.push(Patch {
                sx0: x as f64,
                sy0: r as f64,
                sx1: (x + w) as f64,
                sy1: end as f64,
                x0: 0.0,
                y0: (top + r) as f64,
                x1: w as f64,
                y1: (top + end) as f64,
            });
            r = end;
        }
        out
    }

    /// The highlights' bands at a step, swaying against the sea with bands of light running across, as
    /// one vertex array.
    fn glinting(&self, g: &Geo, step: i64) -> Vec<f32> {
        let made = self.made.as_ref().unwrap();
        let (f, w, m, top) = (step as f64 / made.steps as f64, g.w, MARGIN, made.top);
        let (depth, tw) = (g.h - top, (g.w + 2 * m) as f64);
        let mut out = vec![];
        let mut r = 0;
        while r < depth {
            let end = depth.min(r + 3);
            let x = (m + self.shift(g, r, depth, f + 0.37, -1.4)) as f64;
            let a = self.glow * (0.35 + 0.65 * (0.5 + 0.5 * (2.0 * PI * (r as f64 / 47.0 - 2.0 * f)).sin()).powi(2));
            let d = depth as f64;
            out.extend(
                image_quad(
                    0.0,
                    (top + r) as f64,
                    w as f64,
                    (top + end) as f64,
                    (1.0, 1.0, 1.0),
                    a,
                    (x / tw, r as f64 / d, (x + w as f64) / tw, end as f64 / d),
                )
                .iter()
                .map(|&v| v as f32),
            );
            r = end;
        }
        out
    }
}

impl Part for Waves {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        self.made.as_ref().map_or(g.h, |m| m.top)
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.make(g);
        let Some(m) = self.made.as_mut().and_then(|m| m.pictures.take()) else { return };
        self.texs = Some([m.sea, m.light, m.open].map(|s| up.add(s, Kind::Label)));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let steps = self.made.as_ref().unwrap().steps;
        self.shown = ((w.clock * self.rate as f64) as i64).rem_euclid(steps);
        self.shown as u64
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some([sea, light_tex, open]) = self.texs else { return };
        let step = self.shown;
        if !self.cache.contains_key(&step) {
            let v = (self.bands(g, step, 1.0, 0.0), self.bands(g, step, self.open, 0.21), self.glinting(g, step));
            self.cache.insert(step, v);
        }
        let (bands, far, light) = &self.cache[&step];
        gfx.patches(sea, bands, 1.0);
        gfx.patches(open, far, 1.0);
        gfx.draw(Some(light_tex), light, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.make(g);
        let Some(made) = self.made.as_ref() else { return };
        let Some(m) = made.pictures.as_ref() else { return };
        for (bands, src) in [(self.bands(g, 0, 1.0, 0.0), &m.sea), (self.bands(g, 0, self.open, 0.21), &m.open)] {
            for Patch { sx0, sy0, x0, y0, x1, y1, .. } in bands {
                let _ = c.save();
                c.rectangle(x0, y0, x1 - x0, y1 - y0);
                c.clip();
                let _ = c.set_source_surface(src, x0 - sx0, y0 - sy0);
                let _ = c.paint();
                let _ = c.restore();
            }
        }
        let (data, tw) = (self.glinting(g, 0), (g.w + 2 * MARGIN) as f64);
        for q in data.as_chunks::<32>().0 {
            let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
            let _ = c.save();
            c.rectangle(q[6], q[7], q[14] - q[6], q[23] - q[7]);
            c.clip();
            let _ = c.set_source_surface(&m.light, -q[0] * tw, made.top as f64);
            let _ = c.paint_with_alpha(q[5]);
            let _ = c.restore();
        }
    }
}

// --- the smoke -------------------------------------------------------------------------------------

/// The volcano's plume (live): its sections (`files`, top down to the crater), each swaying, swelling
/// and breathing about its own pivot on its own phase, more the higher it is.
struct Smoke {
    pic: Picture,
    files: Vec<String>,
    turn: f64,
    swell: f64,
    drift: f64,
    looped: f64,
    rate: i64,
    erase: Vec<(f64, f64, f64, f64)>,
    steps: i64,
    /// Each section: its box, pivot and reach.
    pieces: Vec<(Box4, (f64, f64), f64)>,
    surfaces: Vec<ImageSurface>,
    texs: Vec<Tex>,
    made: bool,
    /// Each step's quads, a list per section.
    cache: HashMap<i64, Vec<Vec<f32>>>,
    shown: i64,
}

impl Smoke {
    fn pivot(&self, g: &Geo, name: &str) -> Option<(f64, f64)> {
        let head = svg_head(&self.pic.path(Some(name)), 65536);
        // finds data-pivot-x="N" data-pivot-y="N"
        let number = |s: &str| -> Option<(f64, usize)> {
            let n = s.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
            if n == 0 || !s[n..].starts_with('"') {
                return None;
            }
            Some((s[..n].parse::<f64>().ok().filter(|v| *v <= 1e6)?, n + 1))
        };
        let mut from = 0;
        while let Some(p) = head[from..].find("data-pivot-x=\"") {
            let s = from + p + 14;
            from = from + p + 1;
            let Some((x, n)) = number(&head[s..]) else { continue };
            let rest = &head[s + n..];
            let ws = rest.len() - rest.trim_start().len();
            if ws == 0 {
                continue;
            }
            let Some(after) = rest[ws..].strip_prefix("data-pivot-y=\"") else { continue };
            let Some((y, _)) = number(after) else { continue };
            return Some(self.pic.at(g, x, y));
        }
        None
    }

    fn make(&mut self, g: &Geo) {
        if self.made {
            return;
        }
        self.made = true;
        self.steps = 1.max((self.looped * self.rate as f64).round() as i64);
        let n = self.files.len();
        for (k, name) in self.files.clone().iter().enumerate() {
            let Some(b) = self.pic.ink(g, Some(name), 12) else { continue };
            let reach = 1.0 - k as f64 / 1.max(n as i64 - 1) as f64 * 0.7;
            let pivot = self.pivot(g, name).unwrap_or(((b.0 + b.2) as f64 / 2.0, b.3 as f64));
            self.pieces.push((b, pivot, reach));
            let tag = format!("{b:?} {:?} {:?}", self.erase, self.pic.place(g));
            let piece = self.pic.piece(g, Some(name), "smoke", &tag, || {
                let piece = unstranded(unskied(crop(&self.pic.source(g, Some(name)), b)), 2, 64, 0.004);
                self.pic.touch_up(g, &piece, b.0 as f64, b.1 as f64, &self.erase, &[]);
                piece
            });
            self.surfaces.push(piece);
        }
    }

    /// A section's four corners and alpha at a step: swayed, swollen and drifted about its pivot; with
    /// `foot` (the vent's), its foot stays fixed in the crater.
    fn corners(&self, g: &Geo, step: i64, b: Box4, pivot: (f64, f64), reach: f64, phase: f64, foot: bool) -> ([(f64, f64); 4], f64) {
        let (f, u) = (step as f64 / self.steps as f64, unit(g.w, g.h));
        let angle = (self.turn * reach * (2.0 * PI * (f + phase)).sin()).to_radians();
        let grow = 1.0 + self.swell * reach * (2.0 * PI * (f + phase * 1.7 + 0.25)).sin();
        let dx = -self.drift * u * reach * (0.5 + 0.5 * (2.0 * PI * (f + phase)).sin());
        let dy = -self.drift * u * reach * 0.6 * (0.5 + 0.5 * (2.0 * PI * (f + phase + 0.3)).sin());
        let (c, s) = (angle.cos() * grow, angle.sin() * grow);
        let (px, py) = pivot;
        let (x0, y0, x1, y1) = (b.0 as f64, b.1 as f64, b.2 as f64, b.3 as f64);
        let mut pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| (px + c * (x - px) - s * (y - py) + dx, py + s * (x - px) + c * (y - py) + dy));
        if y0 <= 0.0 {
            let lift = pts[0].1.max(pts[1].1);
            if lift > 0.0 {
                pts = pts.map(|(x, y)| (x, y - lift));
            }
        }
        if foot {
            pts[2] = (x1, y1);
            pts[3] = (x0, y1);
        }
        (pts, 1.0 - 0.08 * reach * (0.5 + 0.5 * (2.0 * PI * (f + phase + 0.5)).sin()))
    }

    fn quads(&self, g: &Geo, step: i64) -> Vec<Vec<f32>> {
        let n = self.pieces.len();
        self.pieces
            .iter()
            .enumerate()
            .map(|(k, (b, pivot, reach))| {
                let (pts, a) = self.corners(g, step, *b, *pivot, *reach, k as f64 * 0.23, k == n - 1);
                let mut d = vec![];
                for ((u, v), (x, y)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].into_iter().zip(pts) {
                    d.extend_from_slice(&[u as f32, v as f32, a as f32, a as f32, a as f32, a as f32, x as f32, y as f32]);
                }
                d
            })
            .collect()
    }
}

impl Part for Smoke {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        0.max(self.pieces.iter().map(|p| p.0.1).min().unwrap_or(g.h) - 20)
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.make(g);
        self.texs = std::mem::take(&mut self.surfaces).into_iter().map(|s| up.add(s, Kind::Sprite)).collect();
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.shown = ((w.clock * self.rate as f64) as i64).rem_euclid(self.steps);
        self.shown as u64
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let step = self.shown;
        if !self.cache.contains_key(&step) {
            let q = self.quads(g, step);
            self.cache.insert(step, q);
        }
        for (&tex, data) in self.texs.iter().zip(&self.cache[&step]) {
            gfx.draw(Some(tex), data, Blend::Premultiplied);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.make(g);
        let n = self.pieces.len();
        for (k, ((b, pivot, reach), s)) in self.pieces.iter().zip(&self.surfaces).enumerate() {
            let ([(ax, ay), (bx, by), _, (dx, dy)], a) = self.corners(g, 0, *b, *pivot, *reach, k as f64 * 0.23, k == n - 1);
            let (w, h) = (s.width() as f64, s.height() as f64);
            let _ = c.save();
            c.transform(Matrix::new((bx - ax) / w, (by - ay) / w, (dx - ax) / h, (dy - ay) / h, ax, ay));
            let _ = c.set_source_surface(s, 0.0, 0.0);
            let _ = c.paint_with_alpha(a);
            let _ = c.restore();
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerSettings {
    file: String,
}

impl Validate for LayerSettings {
    fn validate(&self) -> Result<(), String> {
        named("file", &self.file)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloudsSettings {
    file: String,
    speed: f64,
    erase: Vec<[f64; 4]>,
    fades: Vec<[f64; 4]>,
}

/// Boxes on the canvas: [x0, y0, x1, y1] and the like.
fn canvas_boxes(name: &str, boxes: &[[f64; 4]]) -> Result<(), String> {
    if boxes.len() > 256 {
        return Err(format!("{name} has {}: at most 256", boxes.len()));
    }
    boxes.iter().try_for_each(|b| each(name, b, -1e5, 1e5))
}

impl Validate for CloudsSettings {
    fn validate(&self) -> Result<(), String> {
        named("file", &self.file)?;
        within("speed", self.speed, -1000.0, 1000.0)?;
        canvas_boxes("erase", &self.erase)?;
        canvas_boxes("fades", &self.fades)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WavesSettings {
    file: String,
    light: String,
    land: String,
    sway: f64,
    open: f64,
    glow: f64,
    #[serde(rename = "loop")]
    looped: f64,
    rate: i64,
}

impl Validate for WavesSettings {
    fn validate(&self) -> Result<(), String> {
        named("file", &self.file)?;
        named("light", &self.light)?;
        named("land", &self.land)?;
        within("sway", self.sway, -100.0, 100.0)?;
        within("open", self.open, -100.0, 100.0)?;
        within("glow", self.glow, 0.0, 1.0)?;
        within("loop", self.looped, 1.0, 60.0)?;
        within("rate", self.rate as f64, 1.0, 30.0)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SmokeSettings {
    file: String,
    files: Vec<String>,
    turn: f64,
    swell: f64,
    drift: f64,
    #[serde(rename = "loop")]
    looped: f64,
    rate: i64,
    erase: Vec<[f64; 4]>,
}

impl Validate for SmokeSettings {
    fn validate(&self) -> Result<(), String> {
        named("file", &self.file)?;
        self.files.iter().enumerate().try_for_each(|(i, f)| named(&format!("files[{i}]"), f))?;
        if self.files.len() > 16 {
            return Err(format!("files has {}: at most 16", self.files.len()));
        }
        within("turn", self.turn, -30.0, 30.0)?;
        within("swell", self.swell, 0.0, 0.5)?;
        within("drift", self.drift, 0.0, 0.1)?;
        within("loop", self.looped, 1.0, 60.0)?;
        within("rate", self.rate as f64, 1.0, 30.0)?;
        canvas_boxes("erase", &self.erase)
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "layer",
            defaults: r#"file = "sky.svgz""#,
            varies: &[],
            check: check::<LayerSettings>,
            make: |_, t, g| {
                let s: LayerSettings = settings(t);
                Ok(vec![Box::new(Layer { pic: Picture { file: s.file, th: g.th.clone() } })])
            },
        },
        Kind {
            name: "clouds",
            defaults: r#"file = "clouds.svgz"
speed = 4.0
erase = [[0.0, 0.0, 0.0, 0.0]]
fades = [[0.0, 0.0, 0.0, 0.0]]"#,
            varies: &["erase", "fades"],
            check: check::<CloudsSettings>,
            make: |_, t, g| {
                let s: CloudsSettings = settings(t);
                Ok(vec![Box::new(Clouds {
                    pic: Picture { file: s.file, th: g.th.clone() },
                    tex: None,
                    speed: s.speed,
                    erase: boxes(&s.erase),
                    fades: boxes(&s.fades),
                    made: None,
                    piece: None,
                    shown: 0,
                })])
            },
        },
        Kind {
            name: "waves",
            defaults: r#"file = "sea.svgz"
light = "waves.svgz"
land = "island.svgz"
sway = 5.0
open = 4.0
glow = 0.9
loop = 12.0
rate = 15"#,
            varies: &[],
            check: check::<WavesSettings>,
            make: |_, t, g| {
                let s: WavesSettings = settings(t);
                Ok(vec![Box::new(Waves {
                    pic: Picture { file: s.file, th: g.th.clone() },
                    texs: None,
                    light: s.light,
                    land: s.land,
                    sway: s.sway,
                    open: s.open,
                    glow: s.glow,
                    looped: s.looped,
                    rate: s.rate,
                    made: None,
                    cache: HashMap::new(),
                    shown: 0,
                })])
            },
        },
        Kind {
            name: "smoke",
            defaults: r#"file = "smoke-vent.svgz"
files = ["smoke-drift.svgz", "smoke-billow.svgz", "smoke-rise.svgz", "smoke-vent.svgz"]
turn = 3.5
swell = 0.08
drift = 0.016
loop = 10.0
rate = 15
erase = [[0.0, 0.0, 0.0, 0.0]]"#,
            varies: &["files", "erase"],
            check: check::<SmokeSettings>,
            make: |_, t, g| {
                let s: SmokeSettings = settings(t);
                Ok(vec![Box::new(Smoke {
                    pic: Picture { file: s.file, th: g.th.clone() },
                    files: s.files,
                    turn: s.turn,
                    swell: s.swell,
                    drift: s.drift,
                    looped: s.looped,
                    rate: s.rate,
                    erase: boxes(&s.erase),
                    steps: 1,
                    pieces: vec![],
                    surfaces: vec![],
                    texs: vec![],
                    made: false,
                    cache: HashMap::new(),
                    shown: 0,
                })])
            },
        },
    ]
}
