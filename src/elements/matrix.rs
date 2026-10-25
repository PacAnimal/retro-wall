//! Matrix theme elements: `void` (the black background), `rain` (the digital rain) and `codetitle`
//! (the hostname in the film's title lettering).
//!
//! Glyphs and title lettering are outlines read from MatrixCode.json and Matrix.json beside the theme.

use crate::config::Theme;
use crate::elements::{Validate, check, each, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::path::PathData;
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::{Colour, MaybeColour};
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, LinearGradient};
use serde::Deserialize;
use std::collections::HashMap;
use std::rc::Rc;

/// The rain's glyphs: katakana, numerals and signs. Keep the order: seeded picks index into it.
fn glyphs() -> Vec<char> {
    let kana = [
        0x30a2, 0x30a6, 0x30a8, 0x30aa, 0x30ab, 0x30ad, 0x30b1, 0x30b3, 0x30b5, 0x30b7, 0x30b9, 0x30bb, 0x30bd, 0x30bf, 0x30c4, 0x30c6, 0x30ca, 0x30cb, 0x30cc,
        0x30cd, 0x30cf, 0x30d2, 0x30db, 0x30de, 0x30df, 0x30e0, 0x30e1, 0x30e2, 0x30e4, 0x30e8, 0x30e9, 0x30ea, 0x30ef, 0x30fc,
    ];
    kana.iter().map(|&c| char::from_u32(c).unwrap()).chain("012345789:\"*+<>z|¦╌꞊".chars()).collect()
}

// --- the letterings ------------------------------------------------------------------------------------

/// A glyph of a lettering: its advance, its outline (traced: cairo's fixed-point points), and its ink box
/// [x_min, y_min, x_max, y_max], in font units, y down, origin on the baseline.
pub struct Glyph {
    pub advance: f64,
    outline: cairo::Path,
    pub ink: [f64; 4],
}

/// A lettering from NAME.json beside the theme. Kerning maps a pair to an adjustment of the first
/// char's advance.
pub struct Lettering {
    pub units_per_em: f64,
    pub glyphs: HashMap<char, Glyph>,
    pub kerning: HashMap<String, f64>,
}

/// A lettering as its file has it: each glyph (advance, SVG path, ink box).
#[derive(Deserialize)]
struct LetteringFile {
    units_per_em: f64,
    glyphs: HashMap<char, (f64, String, [f64; 4])>,
    #[serde(default)]
    kerning: HashMap<String, f64>,
}

impl Lettering {
    /// NAME.json of the theme's, every outline read; or why it cannot be.
    pub fn load(th: &Theme, name: &str) -> Result<Lettering, String> {
        let path = th.path(&format!("{name}.json"));
        let at = |e: String| format!("{}: {e}", path.display());
        let text = crate::files::read_text(&path, crate::files::Kind::Lettering).map_err(|e| at(e.to_string()))?;
        let file: LetteringFile = serde_json::from_str(&text).map_err(|e| at(e.to_string()))?;
        if !(16.0..=16384.0).contains(&file.units_per_em) {
            return Err(at(format!("units_per_em is {}: it is 16 to 16384", file.units_per_em)));
        }
        let mut glyphs = HashMap::new();
        for (ch, (advance, d, ink)) in file.glyphs {
            if !advance.is_finite() || advance.abs() > 1e6 || ink.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
                return Err(at(format!("glyph {ch:?}: its advance and ink box are numbers within a million units")));
            }
            let outline = PathData::parse(&d).map_err(|e| at(format!("glyph {ch:?}: {e}")))?.traced().ok_or_else(|| at("cairo cannot trace".into()))?;
            glyphs.insert(ch, Glyph { advance, outline, ink });
        }
        if file.kerning.values().any(|v| !v.is_finite() || v.abs() > 1e6) {
            return Err(at("kerning: numbers within a million units".into()));
        }
        Ok(Lettering { units_per_em: file.units_per_em, glyphs, kerning: file.kerning })
    }

    /// Fills a glyph's outline (nonzero) on c, in font units at the current origin.
    pub fn letter(&self, c: &Context, ch: char) {
        if let Some(g) = self.glyphs.get(&ch) {
            c.append_path(&g.outline);
            let _ = c.fill();
        }
    }
}

// --- the void --------------------------------------------------------------------------------------

/// The black background behind the rain: `colour`, or the palette's base.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Void {
    colour: MaybeColour,
}

impl Validate for Void {}

impl Part for Void {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (r, gg, b) = self.colour.0.unwrap_or(g.th.palette.base).rgb();
        c.set_source_rgb(r, gg, b);
        let _ = c.paint();
    }
}

// --- the rain --------------------------------------------------------------------------------------

const CHUNK: i64 = 4; // rows a lit piece of a trail spans
const VARIANTS: usize = 3;
// the textures after the grid variants
const HEADS: usize = VARIANTS;
const BEAM: usize = VARIANTS + 1;
const HALO: usize = VARIANTS + 2;

/// A front's quads with its head at a row: (per grid variant, light, head halo).
type Front = (Vec<Vec<f32>>, Vec<f32>, Vec<f32>);

/// The rain's settings.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RainSettings {
    cell: f64,
    shape: f64,
    speed: [f64; 2],
    trail: [f64; 2],
    fronts: f64,
    flicker: f64,
    #[serde(rename = "loop")]
    looped: f64,
    rate: f64,
    seed: i64,
    bloom: [f64; 2],
    palette: Vec<Colour>,
    cursor: Colour,
}

impl Validate for RainSettings {
    fn validate(&self) -> Result<(), String> {
        within("cell", self.cell, 0.005, 0.1)?;
        within("shape", self.shape, 0.75, 3.0)?;
        each("speed", &self.speed, 0.1, 200.0)?;
        each("trail", &self.trail, 0.05, 3.0)?;
        within("fronts", self.fronts, 0.5, 8.0)?;
        within("flicker", self.flicker, 0.0, 60.0)?;
        within("loop", self.looped, 1.0, 600.0)?;
        within("rate", self.rate, 1.0, 120.0)?;
        each("bloom", &self.bloom, 0.0, 2.0)?;
        if self.palette.len() > 64 {
            return Err("palette takes at most 64 colours".into());
        }
        if self.palette.len() < 2 {
            return Err("palette takes two colours or more, unlit to brightest".into());
        }
        Ok(())
    }
}

/// The glyph grid on a monitor: whole columns spanning its width exactly, whole rows covering its height.
#[derive(Clone, Copy, Default)]
struct Grid {
    rows: i64,
    cols: i64,
    ch: f64, // row height
    cw: f64, // column width
    w: f64,
}

impl Grid {
    /// The nearest whole number of columns to the set cell's, the cell scaled to fill the width with them.
    fn of(cell: f64, shape: f64, g: &Geo) -> Grid {
        let ch = (cell * unit(g.w, g.h)).max(8.0);
        let cw = (ch / shape).max(6.0);
        let w = g.w as f64;
        let cols = ((w / cw).round() as i64).max(1);
        let k = w / cols as f64 / cw;
        let ch = ch * k;
        Grid { rows: (g.h as f64 / ch).ceil() as i64, cols, ch, cw: cw * k, w }
    }

    /// Column c's left edge; column `cols` is the right edge of the picture, exactly.
    fn x(&self, c: i64) -> f64 {
        c as f64 * self.w / self.cols as f64
    }
}

/// A frame's quads: (per grid variant, heads, light, halos).
#[derive(Default)]
struct Quads {
    grids: Vec<Vec<f32>>,
    heads: Vec<f32>,
    light: Vec<f32>,
    halos: Vec<f32>,
}

/// The digital rain (live): a fixed grid of glyphs lit by fronts falling down each column, with a
/// flickering glyph at each head.
pub struct Rain {
    font: Lettering,
    texes: Vec<Tex>, // grid variants, heads, beam, halo
    cell: f64,
    shape: f64,
    speed: (f64, f64),
    trail: (f64, f64),
    fronts: f64,
    flicker: f64,
    loop_: f64,
    rate: f64,
    seed: i64,
    bloom: (f64, f64),
    palette: Vec<(f64, f64, f64)>,
    cursor: (f64, f64, f64),
    // made once, by make()
    ready: bool,
    grid: Grid,
    head_uv: (f64, f64), // one glyph's width and height in the heads' strip
    steps: i64,
    fronts_at: Vec<(i64, i64, i64, i64, f64)>,
    made: HashMap<(usize, i64), Rc<Front>>,
    grids: Vec<ImageSurface>,
    heads_surface: Option<ImageSurface>,
    beam_surface: Option<ImageSurface>,
    halo_surface: Option<ImageSurface>,
    heads_count: usize,
    step: i64,
    quads: Quads,
    built: Option<i64>, // the step `quads` holds
}

impl Rain {
    fn new(s: RainSettings, font: Lettering) -> Rain {
        Rain {
            font,
            texes: vec![],
            cell: s.cell,
            shape: s.shape,
            speed: (s.speed[0], s.speed[1]),
            trail: (s.trail[0], s.trail[1]),
            fronts: s.fronts,
            flicker: s.flicker,
            loop_: s.looped,
            rate: s.rate,
            seed: s.seed,
            bloom: (s.bloom[0], s.bloom[1]),
            palette: s.palette.iter().map(|c| c.rgb()).collect(),
            cursor: s.cursor.rgb(),
            ready: false,
            grid: Grid::default(),
            head_uv: (1.0, 1.0),
            steps: 1,
            fronts_at: vec![],
            made: HashMap::new(),
            grids: vec![],
            heads_surface: None,
            beam_surface: None,
            halo_surface: None,
            heads_count: 0,
            step: 0,
            quads: Quads { grids: vec![vec![]; VARIANTS], ..Quads::default() },
            built: None,
        }
    }

    /// The colour at brightness `b` (0 to 1), interpolated along the palette.
    fn shade(&self, b: f64) -> (f64, f64, f64) {
        let stops = &self.palette;
        let x = 0.0f64.max(1.0f64.min(b)) * (stops.len() - 1) as f64;
        let k = (x as usize).min(stops.len() - 2);
        let f = x - k as f64;
        let (a, z) = (stops[k], stops[k + 1]);
        (a.0 + (z.0 - a.0) * f, a.1 + (z.1 - a.1) * f, a.2 + (z.2 - a.2) * f)
    }

    /// Brightness of a glyph `d` rows above the head of a `trail` rows long trail.
    fn lit(d: f64, trail: f64) -> f64 {
        if d >= trail {
            return 0.0;
        }
        let x = d / trail;
        0.8 * (-d / 2.2).exp() + 0.62 * (1.0 - x).powf(1.1) * (1.0 - (-d / 2.2).exp()) + if d < 1.0 { 0.2 } else { 0.0 }
    }

    /// Makes the grid variants, the heads' strip, the light and the halo, once.
    fn make(&mut self, g: &Geo) {
        if self.ready {
            return;
        }
        self.ready = true;
        self.grid = Grid::of(self.cell, self.shape, g);
        let Grid { rows, cols, ch, cw, .. } = self.grid;
        let font = &self.font;
        let glyphs = glyphs();
        self.steps = 1.max((self.loop_ * self.rate).round() as i64);
        self.fronts_at = self.place_fronts(rows, cols);
        self.made.clear();
        self.built = None;
        let size = ch * 0.78;
        let lettered = |font: &Lettering, c: &Context, gl: char, x: f64, y: f64| {
            let Some([x0, y0, x1, y1]) = font.glyphs.get(&gl).map(|g| g.ink) else { return };
            let s = size / font.units_per_em;
            let _ = c.save();
            c.translate(x + cw / 2.0, y + ch / 2.0);
            c.scale(s, s);
            c.translate(-(x0 + x1) / 2.0, -(y0 + y1) / 2.0);
            font.letter(c, gl);
            let _ = c.restore();
        };
        let near = self.bloom.0;
        let mut grids = vec![];
        for k in 0..VARIANTS {
            let plain = surface(g.w, g.h);
            {
                let c = ctx(&plain);
                c.set_source_rgb(1.0, 1.0, 1.0);
                let mut rng = Rng::new(self.seed * 7 + k as i64);
                for cc in 0..cols {
                    for r in 0..rows {
                        let gl = *rng.pick(&glyphs);
                        lettered(font, &c, gl, self.grid.x(cc), r as f64 * ch);
                    }
                }
            }
            let out = surface(plain.width(), plain.height());
            glowing(&ctx(&out), &plain, &[(2.0f64.max((ch / 16.0).round()), near)]);
            grids.push(out);
        }
        let strip = surface((cw * glyphs.len() as f64).ceil() as i32, ch.ceil() as i32);
        {
            let c = ctx(&strip);
            c.set_source_rgb(1.0, 1.0, 1.0);
            for (k, &gl) in glyphs.iter().enumerate() {
                lettered(font, &c, gl, k as f64 * cw, 0.0);
            }
        }
        self.head_uv = (cw / strip.width() as f64, ch / strip.height() as f64);
        let beam = surface(64, 4);
        {
            let gr = LinearGradient::new(0.0, 0.0, 64.0, 0.0);
            for k in 0..17 {
                let x = k as f64 / 16.0;
                gr.add_color_stop_rgba(x, 1.0, 1.0, 1.0, (-((x - 0.5) / 0.2).powf(2.0)).exp());
            }
            let c = ctx(&beam);
            let _ = c.set_source(&gr);
            let _ = c.paint();
        }
        self.heads_count = glyphs.len();
        self.grids = grids;
        self.heads_surface = Some(strip);
        self.beam_surface = Some(beam);
        self.halo_surface = Some(soft_dot(64, &[(0.0, 0.9), (0.2, 0.5), (0.5, 0.15), (1.0, 0.0)]));
    }

    /// Each front, from the seed: (column, trail rows, rows per pass, passes per loop, start phase).
    fn place_fronts(&self, rows: i64, cols: i64) -> Vec<(i64, i64, i64, i64, f64)> {
        let mut rng = Rng::new(self.seed + 1);
        let mut out = vec![];
        for c in 0..cols {
            let n = 1.max((self.fronts + rng.range(-0.5, 0.5)).round() as i64);
            for k in 0..n {
                let trail = 3.max((rng.range(self.trail.0, self.trail.1) * rows as f64).round() as i64);
                let span = rows + trail + rng.int(0, rows);
                let passes = 1.max((rng.range(self.speed.0, self.speed.1) * self.loop_ / span as f64).round() as i64);
                out.push((c, trail, span, passes, rng.f64() + k as f64 / n as f64));
            }
        }
        out
    }

    /// Front i's (column, trail, head row) at a step.
    fn head_at(&self, i: usize, step: i64) -> (i64, i64, i64) {
        let (c, trail, span, passes, start) = self.fronts_at[i];
        let at = (step * passes * span) as f64 / self.steps as f64 + start * span as f64;
        (c, trail, at.rem_euclid(span as f64) as i64)
    }

    /// Front i's quads with its head at `row`, cached.
    fn front(&mut self, g: &Geo, i: usize, c: i64, trail: i64, row: i64) -> Rc<Front> {
        if let Some(f) = self.made.get(&(i, row)) {
            return f.clone();
        }
        let Grid { rows, ch, cw, .. } = self.grid;
        let (wf, hf) = (g.w as f64, g.h as f64);
        let (_near, wide) = self.bloom;
        let (x0, x1) = (self.grid.x(c), self.grid.x(c + 1));
        let (u0, u1) = (x0 / wf, x1 / wf);
        let mut grids: Vec<Vec<f32>> = vec![vec![]; VARIANTS];
        let (mut light, mut halo): (Vec<f32>, Vec<f32>) = (vec![], vec![]);
        let push = |v: &mut Vec<f32>, q: &[f64]| v.extend(q.iter().map(|&x| x as f32));
        let tf = trail as f64;
        let mut r = 0.max(row - trail + 1);
        while r < row.min(rows) {
            let band_end = ((r.div_euclid(CHUNK) + 1) * CHUNK).min(row).min(rows);
            let (b0, b1) = (Self::lit((row - r) as f64, tf), Self::lit((row - band_end) as f64, tf));
            if b0 > 0.004 || b1 > 0.004 {
                let k = (chance(&[self.seed, c, r.div_euclid(CHUNK), row]) * VARIANTS as f64) as usize;
                let (c0, c1) = (self.shade(b0), self.shade(b1));
                let (y0, y1) = (r as f64 * ch, hf.min(band_end as f64 * ch));
                let (a0, a1) = (1.0f64.min(b0 * 2.0), 1.0f64.min(b1 * 2.0));
                push(
                    &mut grids[k],
                    &[
                        u0,
                        y0 / hf,
                        c0.0 * a0,
                        c0.1 * a0,
                        c0.2 * a0,
                        a0,
                        x0,
                        y0,
                        u1,
                        y0 / hf,
                        c0.0 * a0,
                        c0.1 * a0,
                        c0.2 * a0,
                        a0,
                        x1,
                        y0,
                        u1,
                        y1 / hf,
                        c1.0 * a1,
                        c1.1 * a1,
                        c1.2 * a1,
                        a1,
                        x1,
                        y1,
                        u0,
                        y1 / hf,
                        c1.0 * a1,
                        c1.1 * a1,
                        c1.2 * a1,
                        a1,
                        x0,
                        y1,
                    ],
                );
            }
            r = band_end;
        }
        let (bx0, bx1) = ((x0 - cw).max(0.0), wf.min(x1 + cw));
        for (d0, d1) in [(trail, 4), (4, 0), (0, -2)] {
            let (top, bottom) = (row - d0 + 1, row - d1 + 1);
            let (y0, y1) = ((top as f64 * ch).max(0.0), hf.min(bottom as f64 * ch));
            if y1 <= y0 {
                continue;
            }
            let rowf = row as f64;
            let mut g0 = if d0 > 0 { wide * 1.0f64.min(Self::lit(0.0f64.max(rowf - y0 / ch), tf) * 2.0) } else { 0.0 };
            let mut g1 = if d1 > 0 { wide * 1.0f64.min(Self::lit(0.0f64.max(rowf - y1 / ch), tf) * 2.0) } else { 0.0 };
            if d0 == 0 {
                g0 = wide * 1.0f64.min(Self::lit(0.0, tf) * 2.0);
                g1 = 0.0;
            }
            let (c0, c1) = (self.shade(g0 / wide / 2.0), self.shade(g1 / wide / 2.0));
            push(
                &mut light,
                &[
                    0.0,
                    0.0,
                    c0.0 * g0,
                    c0.1 * g0,
                    c0.2 * g0,
                    g0,
                    bx0,
                    y0,
                    1.0,
                    0.0,
                    c0.0 * g0,
                    c0.1 * g0,
                    c0.2 * g0,
                    g0,
                    bx1,
                    y0,
                    1.0,
                    1.0,
                    c1.0 * g1,
                    c1.1 * g1,
                    c1.2 * g1,
                    g1,
                    bx1,
                    y1,
                    0.0,
                    1.0,
                    c1.0 * g1,
                    c1.1 * g1,
                    c1.2 * g1,
                    g1,
                    bx0,
                    y1,
                ],
            );
        }
        if 0 <= row && row < rows {
            let (cx, cy, rr) = ((x0 + x1) / 2.0, (row as f64 + 0.5) * ch, 1.6 * ch);
            push(&mut halo, &image_quad(cx - rr, cy - rr, cx + rr, cy + rr, self.cursor, wide * 1.4, (0.0, 0.0, 1.0, 1.0)));
        }
        let f = Rc::new((grids, light, halo));
        self.made.insert((i, row), f.clone());
        f
    }

    /// The quads at a step, into `self.quads`.
    fn build(&mut self, g: &Geo, step: i64) {
        if self.built == Some(step) {
            return;
        }
        self.built = Some(step);
        let Grid { rows, ch, .. } = self.grid;
        let flick = (step as f64 * self.flicker / self.rate) as i64;
        let n = self.heads_count;
        let (hu, hv) = self.head_uv;
        let mut q = std::mem::take(&mut self.quads);
        q.grids.iter_mut().for_each(Vec::clear);
        q.heads.clear();
        q.light.clear();
        q.halos.clear();
        for i in 0..self.fronts_at.len() {
            let (c, trail, row) = self.head_at(i, step);
            let f = self.front(g, i, c, trail, row);
            for k in 0..VARIANTS {
                q.grids[k].extend_from_slice(&f.0[k]);
            }
            q.light.extend_from_slice(&f.1);
            q.halos.extend_from_slice(&f.2);
            if 0 <= row && row < rows {
                let (x0, x1) = (self.grid.x(c), self.grid.x(c + 1));
                let glyph = (chance(&[self.seed, c, row, flick]) * n as f64) as usize as f64;
                let quad = image_quad(x0, row as f64 * ch, x1, (row + 1) as f64 * ch, self.cursor, 1.0, (glyph * hu, 0.0, (glyph + 1.0) * hu, hv));
                q.heads.extend(quad.iter().map(|&x| x as f32));
            }
        }
        self.quads = q;
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
        self.make(g);
        // handed over: the still picture is drawn by now
        let (Some(heads), Some(beam), Some(halo)) = (self.heads_surface.take(), self.beam_surface.take(), self.halo_surface.take()) else { return };
        self.texes = std::mem::take(&mut self.grids).into_iter().map(|s| up.add(s, Kind::Label)).collect();
        self.texes.push(up.add(heads, Kind::Label));
        self.texes.push(up.add(beam, Kind::Sprite));
        self.texes.push(up.add(halo, Kind::Sprite));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.step = ((w.clock * self.rate) as i64).rem_euclid(self.steps);
        self.step as u64
    }
    fn interval(&self) -> f64 {
        1.0 / self.rate
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if self.texes.len() <= HALO {
            return;
        }
        self.build(g, self.step);
        let q = &self.quads;
        gfx.draw(Some(self.texes[BEAM]), &q.light, Blend::Premultiplied); // the light under the code
        for (k, data) in q.grids.iter().enumerate() {
            if !data.is_empty() {
                gfx.draw(Some(self.texes[k]), data, Blend::Premultiplied);
            }
        }
        gfx.draw(Some(self.texes[HALO]), &q.halos, Blend::Premultiplied);
        gfx.draw(Some(self.texes[HEADS]), &q.heads, Blend::Premultiplied);
    }
    /// Paints the first step with cairo, each trail piece tinted and faded as its quad is.
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        self.make(g);
        self.build(g, 0);
        let (Some(beam), Some(halo), Some(heads)) = (&self.beam_surface, &self.halo_surface, &self.heads_surface) else { return };
        let q = &self.quads;
        paint_vertices(c, beam, &q.light);
        for (k, data) in q.grids.iter().enumerate() {
            for q in data.as_chunks::<32>().0 {
                let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
                let (x0, y0, x1, y1) = (q[6], q[7], q[14], q[23]);
                let _ = c.save();
                c.rectangle(x0, y0, x1 - x0, y1 - y0);
                c.clip();
                let fade = LinearGradient::new(0.0, y0, 0.0, y1);
                let a0 = if q[5] != 0.0 { q[5] } else { 1e-6 };
                let a1 = if q[29] != 0.0 { q[29] } else { 1e-6 };
                fade.add_color_stop_rgba(0.0, q[2] / a0, q[3] / a0, q[4] / a0, a0);
                fade.add_color_stop_rgba(1.0, q[26] / a1, q[27] / a1, q[28] / a1, a1);
                let _ = c.set_source(&fade);
                let _ = c.mask_surface(&self.grids[k], 0.0, 0.0);
                let _ = c.restore();
            }
        }
        paint_vertices(c, halo, &q.halos);
        paint_vertices(c, heads, &q.heads);
    }
}

// --- the title ---------------------------------------------------------------------------------------

/// The hostname in the film's title lettering, green and glowing, at the theme's title position, with
/// the rain dimmed around it (`shade`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeTitleSettings {
    colour: Colour,
    glow: f64,
    shade: f64,
}

impl Validate for CodeTitleSettings {
    fn validate(&self) -> Result<(), String> {
        within("glow", self.glow, 0.0, 1.0)?;
        within("shade", self.shade, 0.0, 1.0)
    }
}

pub struct CodeTitle {
    colour: Colour,
    glow: f64,
    shade: f64,
    font: Lettering,
}

impl Part for CodeTitle {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Title)
    }
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn paint(&mut self, g: &Geo, ctx_: &Context, _s: &mut Shared) {
        let (w, h) = (g.w, g.h);
        let font = &self.font;
        let text: Vec<char> = g.th.title_text(&g.host).iter().flat_map(|c| c.to_uppercase()).filter(|c| font.glyphs.contains_key(c)).collect();
        let p = g.th.title_place(w, h, &g.host, Edges::NONE);
        let (y0, base, x0) = (p.y0, p.base, p.x0);
        let mut pens = vec![];
        let mut pen = 0.0;
        for k in 0..text.len() {
            pens.push(pen);
            let pair: String = text[k..(k + 2).min(text.len())].iter().collect();
            pen += font.glyphs.get(&text[k]).map_or(0.0, |g| g.advance) + font.kerning.get(&pair).copied().unwrap_or(0.0);
        }
        let boxes: Vec<(f64, f64, f64)> = pens
            .iter()
            .zip(&text)
            .filter_map(|(&p, ch)| {
                let b = font.glyphs.get(ch)?.ink;
                Some((p + b[0], b[1], p + b[2]))
            })
            .collect();
        let surface_ = surface(w, h);
        let out = surface(w, h);
        {
            let o = ctx(&out);
            if !boxes.is_empty() {
                let left = boxes.iter().map(|b| b.0).fold(f64::INFINITY, f64::min);
                let right = boxes.iter().map(|b| b.2).fold(f64::NEG_INFINITY, f64::max);
                let top = -boxes.iter().map(|b| b.1).fold(f64::INFINITY, f64::min);
                let tr = g.th.title_right(w, h);
                let s = ((base - y0) * 1.25 / 1.0f64.max(top)).min((tr - x0) / 1.0f64.max(right - left));
                let x1 = tr;
                let (x0_ink, y0_ink) = (x1 - (right - left) * s, base - top * s);
                let (pad, blur) = (0.35 * top * s, 2.0f64.max((0.5 * top * s).round()));
                let dark = surface(w, h);
                {
                    let d = ctx(&dark);
                    d.rectangle(x0_ink - pad, y0_ink - pad, x1 - x0_ink + 2.0 * pad, base - y0_ink + 2.0 * pad);
                    d.set_source_rgb(0.0, 0.0, 0.0);
                    let _ = d.fill();
                }
                let _ = o.set_source_surface(resampled(&resampled(&dark, 1.0 / blur, Filter::Good), blur, Filter::Bilinear), 0.0, 0.0);
                let _ = o.paint_with_alpha(self.shade);
                let c = ctx(&surface_);
                let (r, gg, b) = self.colour.rgb();
                c.set_source_rgb(r, gg, b);
                c.translate(x1 - right * s, base);
                c.scale(s, s);
                for (&p, &ch) in pens.iter().zip(&text) {
                    let _ = c.save();
                    c.translate(p, 0.0);
                    font.letter(&c, ch);
                    let _ = c.restore();
                }
            }
            glowing(&o, &surface_, &[(2.0f64.max((unit(w, h) / 200.0).round()), self.glow)]);
        }
        let _ = ctx_.set_source_surface(&out, 0.0, 0.0);
        let _ = ctx_.paint();
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind { name: "void", defaults: r#"colour = """#, varies: &[], check: check::<Void>, make: |_, t, _g| Ok(vec![Box::new(settings::<Void>(t))]) },
        Kind {
            name: "code-rain",
            defaults: r##"cell = 0.027
shape = 1.35
speed = [8.0, 20.0]
trail = [0.5, 1.3]
fronts = 1.7
flicker = 14.0
loop = 30.0
rate = 20
seed = 1999
bloom = [0.6, 0.22]
palette = ["#000000", "#0a4d1e", "#19e66b", "#d8ffe4"]
cursor = "#b4ffc8""##,
            varies: &["palette"],
            check: check::<RainSettings>,
            make: |_, t, g| Ok(vec![Box::new(Rain::new(settings(t), Lettering::load(&g.th, "MatrixCode")?))]),
        },
        Kind {
            name: "codetitle",
            defaults: r##"colour = "#00ff41"
glow = 0.8
shade = 0.9"##,
            varies: &[],
            check: check::<CodeTitleSettings>,
            make: |_, t, g| {
                let s: CodeTitleSettings = settings(t);
                Ok(vec![Box::new(CodeTitle { colour: s.colour, glow: s.glow, shade: s.shade, font: Lettering::load(&g.th, "Matrix")? })])
            },
        },
    ]
}
