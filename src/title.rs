//! The hostname lettering: glyph strokes, optical spacing, placement, the ruler and caption, and the
//! neon-tube atlas. Each is a method of the theme whose tweaks shape it; its measurements are kept in
//! the theme (`Measures`).

use crate::config::Theme;
use crate::scene::Edges;
use crate::tweaks::TitleCorner;
use crate::util::*;
use cairo::{Context, Filter, Format, ImageSurface, LineCap, LineJoin, Matrix};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

type Pts = Vec<(f64, f64)>;
type InkEdges = Vec<Option<(f64, f64)>>;

/// A theme's lettering measurements, each made once when first needed: a glyph's ink edges by row, a
/// pair's kerning, a name's ink reach.
#[derive(Default)]
pub struct Measures {
    edges: Mutex<HashMap<char, InkEdges>>,
    kerns: Mutex<HashMap<(char, char), f64>>,
    ink: Mutex<HashMap<String, (f64, f64)>>,
}

/// A measurements map; one left poisoned by a panic elsewhere is used as it is.
fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Placed {
    pub text: Vec<char>,
    pub u: f64,
    pub cap: f64,
    pub xs: Vec<f64>,
    pub y0: f64,
    pub base: f64,
    pub x0: f64,
}

/// The ruler under the hostname: its left end, where its ticks end, and its row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruler {
    pub left: f64,
    pub stop: f64,
    pub y: f64,
}

/// The corner brackets, inset from the work area.
#[derive(Clone, Copy, Debug)]
pub struct Brackets {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub arm: f64,
}

/// One letter of the neon atlas: its column in the atlas, and its box on the screen.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub ax: i32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub struct TubeAtlas {
    pub atlas: ImageSurface,
    pub placed: Vec<Cell>,
}

/// Corner brackets, inset from the work area.
pub fn bracket_corners(width: i32, height: i32, edges: Edges) -> Brackets {
    let u = unit(width, height);
    let (inset, arm) = (0.03 * u, 0.045 * u);
    let Edges { top, right, bottom, left } = edges;
    Brackets {
        left: left as f64 + inset,
        right: width as f64 - right as f64 - inset,
        top: top as f64 + inset,
        bottom: height as f64 - bottom as f64 - inset,
        arm,
    }
}

/// Repaint rectangle of the moving ticks: (x, y, width, height).
pub fn ruler_area(r: Ruler) -> (i32, i32, i32, i32) {
    (r.left as i32 - 2, r.y as i32 - 5, (r.stop - r.left) as i32 + 4, 6)
}

impl Theme {
    /// A glyph's strokes as paths: open ends that touch no other stroke lengthened by half a stroke,
    /// closed strokes without their repeated point. None for a glyph the lettering has not.
    fn glyph_paths(&self, ch: char) -> Vec<(Pts, bool)> {
        let gw = self.tw.glyph_width;
        let Some(strokes) = self.glyph(ch) else { return vec![] };
        let segments: Vec<((f64, f64), (f64, f64))> = strokes.iter().flat_map(|st| st.windows(2).map(|w| (w[0], w[1]))).collect();
        let on_stroke = |p: (f64, f64), own: ((f64, f64), (f64, f64))| {
            for &(a, b) in &segments {
                if (a, b) == own {
                    continue;
                }
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                if (a.0 + t * dx - p.0).hypot(a.1 + t * dy - p.1) < 1e-6 {
                    return true;
                }
            }
            false
        };
        let extended = |end: (f64, f64), toward: (f64, f64)| {
            let d = (end.0 - toward.0).hypot(end.1 - toward.1);
            (end.0 + (end.0 - toward.0) / d * gw / 2.0, end.1 + (end.1 - toward.1) / d * gw / 2.0)
        };
        let mut paths = vec![];
        for st in strokes.iter().filter(|st| st.len() >= 2) {
            if st.len() > 2 && st[0] == st[st.len() - 1] {
                paths.push((st[..st.len() - 1].to_vec(), true));
                continue;
            }
            let mut pts = st.clone();
            let n = st.len();
            if !on_stroke(st[0], (st[0], st[1])) {
                pts[0] = extended(st[0], st[1]);
            }
            if !on_stroke(st[n - 1], (st[n - 2], st[n - 1])) {
                pts[n - 1] = extended(st[n - 1], st[n - 2]);
            }
            paths.push((pts, false));
        }
        paths
    }

    pub fn stroke_glyph(&self, c: &Context, ch: char, x: f64, y: f64) {
        for (pts, closed) in self.glyph_paths(ch) {
            for (k, (gx, gy)) in pts.iter().enumerate() {
                if k == 0 { c.move_to(x + gx, y + gy) } else { c.line_to(x + gx, y + gy) }
            }
            if closed {
                c.close_path();
            }
        }
        c.set_line_width(self.tw.glyph_width);
        c.set_line_cap(LineCap::Butt);
        c.set_line_join(LineJoin::Miter);
        c.set_miter_limit(self.tw.glyph_miter);
        let _ = c.stroke();
    }

    fn glyph_edges(&self, ch: char) -> InkEdges {
        if let Some(e) = held(&self.measures.edges).get(&ch) {
            return e.clone();
        }
        let rows = 64;
        let k = 40.0;
        let (w, h) = (8 * 40, 9 * 40);
        let mut edges = vec![None; rows];
        if let Ok(mut s) = ImageSurface::create(Format::A8, w, h) {
            if let Ok(c) = Context::new(&s) {
                c.translate(2.0 * k, 1.5 * k);
                c.scale(k, k);
                self.stroke_glyph(&c, ch, 0.0, 0.0);
            }
            let stride = s.stride() as usize;
            let data = pixels(&mut s);
            for (i, e) in edges.iter_mut().enumerate() {
                let y = ((1.5 + (i as f64 + 0.5) * 6.0 / rows as f64) * k).round() as usize;
                let Some(row) = data.get(y * stride..y * stride + w as usize) else { continue };
                let first = row.iter().position(|&v| v > 127);
                let last = row.iter().rposition(|&v| v > 127);
                if let (Some(a), Some(b)) = (first, last) {
                    *e = Some((a as f64 / k - 2.0, (b + 1) as f64 / k - 2.0));
                }
            }
        }
        held(&self.measures.edges).insert(ch, edges.clone());
        edges
    }

    fn pair_gaps(&self, a: char, b: char, advance: f64) -> Vec<Option<f64>> {
        self.glyph_edges(a)
            .iter()
            .zip(self.glyph_edges(b).iter())
            .map(|(ea, eb)| match (ea, eb) {
                (Some(ea), Some(eb)) => Some(advance + eb.0 - ea.1),
                _ => None,
            })
            .collect()
    }

    fn flat_gap(&self) -> f64 {
        self.pair_gaps('h', 'h', 4.0 + self.tw.track).into_iter().flatten().fold(f64::INFINITY, f64::min)
    }

    fn sidebearings(&self, ch: char) -> (f64, f64) {
        let depth = self.tw.kern_depth;
        let rows: Vec<(f64, f64)> = self.glyph_edges(ch).into_iter().flatten().collect();
        let left = rows.iter().map(|e| e.0).fold(f64::INFINITY, f64::min);
        let right = rows.iter().map(|e| e.1).fold(f64::NEG_INFINITY, f64::max);
        let half = self.flat_gap() / 2.0;
        let n = rows.len() as f64;
        let l: f64 = rows.iter().map(|e| (e.0 - left).min(depth)).sum();
        let r: f64 = rows.iter().map(|e| (right - e.1).min(depth)).sum();
        (half - l / n, half - r / n)
    }

    pub fn kern(&self, a: char, b: char) -> f64 {
        if let Some(&d) = held(&self.measures.kerns).get(&(a, b)) {
            return d;
        }
        let tw = &self.tw;
        let (kmin, kshort, krun) = (tw.kern_min, tw.kern_short, tw.kern_run);
        let plain = 4.0 + tw.track;
        let flat = self.flat_gap();
        let (left, right) = (self.glyph_edges(a), self.glyph_edges(b));
        let ink_a: Vec<(f64, f64)> = left.iter().flatten().cloned().collect();
        let ink_b: Vec<(f64, f64)> = right.iter().flatten().cloned().collect();
        let short = (ink_a.len().min(ink_b.len()) as f64) < left.len() as f64 / 2.0;
        let look = 1.max(((if short { flat } else { kmin * flat }) / 6.0 * left.len() as f64).round() as i64) as usize;
        let least = (if short { kshort } else { kmin }) * flat;
        let reach = ink_a.iter().map(|e| e.1).fold(f64::NEG_INFINITY, f64::max) - plain - ink_b.iter().map(|e| e.0).fold(f64::INFINITY, f64::min);
        let mut d = if short { -99.0 } else { self.sidebearings(a).1 + self.sidebearings(b).0 + reach };
        let window = |i: usize| &right[i.saturating_sub(look).min(right.len())..(i + look + 1).min(right.len())];
        for (i, ea) in left.iter().enumerate() {
            for eb in window(i) {
                if let (Some(ea), Some(eb)) = (ea, eb) {
                    d = d.max(least - (plain + eb.0 - ea.1));
                }
            }
        }
        if short {
            d = d.max(reach - kmin * flat);
            let any = left.iter().enumerate().any(|(i, ea)| window(i).iter().any(|eb| ea.is_some() && eb.is_some()));
            if !any {
                d = d.max(reach + flat);
            }
        }
        let mut same: Vec<f64> = left
            .iter()
            .zip(right.iter())
            .filter_map(|(ea, eb)| match (ea, eb) {
                (Some(ea), Some(eb)) => Some(plain + eb.0 - ea.1),
                _ => None,
            })
            .collect();
        same.sort_by(f64::total_cmp);
        let allowed = (krun * left.len() as f64) as usize;
        if !short && same.len() > allowed {
            d = d.max(flat - 0.01 - same[allowed]);
        }
        held(&self.measures.kerns).insert((a, b), d);
        d
    }

    pub fn title_layout(&self, text: &[char]) -> (Vec<f64>, f64) {
        let track = self.tw.track;
        let mut xs = vec![];
        let mut x = 0.0;
        for (i, &ch) in text.iter().enumerate() {
            if i > 0 {
                x += 4.0 + track + self.kern(text[i - 1], ch);
            }
            xs.push(x);
        }
        let span = if text.is_empty() { 0.0 } else { x + 4.0 };
        (xs, span)
    }

    pub fn margin(&self, width: i32, height: i32) -> f64 {
        let corner = self.tw.corner;
        if corner != 0.0 { corner * unit(width, height) } else { self.tw.margin * width as f64 }
    }

    pub fn title_right(&self, width: i32, height: i32) -> f64 {
        if self.tw.corner == 0.0 { width as f64 * (1.0 - self.tw.margin) } else { width as f64 - self.margin(width, height) }
    }

    /// The host name in the letters the lettering has.
    pub fn title_text(&self, host: &str) -> Vec<char> {
        host.to_lowercase().chars().filter(|&c| self.glyph(c).is_some()).collect()
    }

    pub fn title_unit(&self, width: i32, height: i32, text: &[char]) -> f64 {
        let tw = &self.tw;
        let u = tw.title * unit(width, height) / 6.0;
        let room = if tw.corner == 0.0 { width as f64 * (1.0 - 2.0 * tw.margin) } else { width as f64 - 2.0 * self.margin(width, height) };
        u.min(room / (self.title_layout(text).1 + tw.slant * 6.0).max(1.0))
    }

    /// How far the ink (strokes and extrusion, slanted) reaches past the slanted letter box, in letter
    /// units: (above the cap, right of the top right corner). Measured from a render, kept per name.
    pub fn title_ink(&self, text: &[char]) -> (f64, f64) {
        let key: String = text.iter().collect();
        if let Some(&v) = held(&self.measures.ink).get(&key) {
            return v;
        }
        let slant = self.tw.slant;
        let (xs, span) = self.title_layout(text);
        let (k, pad) = (16.0, 3.0);
        let size = (((span + 6.0 * slant + 2.0 * pad) * k).round() as i32, ((6.0 + 2.0 * pad) * k).round() as i32);
        let mut v = (0.0, 0.0);
        if let Ok(mut image) = ImageSurface::create(Format::A8, size.0, size.1) {
            if let Ok(c) = Context::new(&image) {
                c.scale(k, k);
                c.translate(pad, pad);
                c.transform(Matrix::new(1.0, 0.0, -slant, 1.0, slant * 6.0, 0.0));
                let depth = self.tw.extrude[0].0;
                for d in [0.0, depth * 0.1] {
                    for (&ch, &x) in text.iter().zip(xs.iter()) {
                        self.stroke_glyph(&c, ch, x + d, d);
                    }
                }
            }
            let (w, h) = (image.width() as usize, image.height() as usize);
            let stride = image.stride() as usize;
            let data = pixels(&mut image);
            let rows: Vec<usize> = (0..h).filter(|&y| data[y * stride..y * stride + w].iter().any(|&v| v > 127)).collect();
            let cols: Vec<usize> = (0..w).filter(|&x| !rows.is_empty() && rows.iter().any(|&y| data[y * stride + x] > 127)).collect();
            if let (Some(&last), Some(&top)) = (cols.last(), rows.first()) {
                v = (pad - top as f64 / k, (last + 1) as f64 / k - pad - span - 6.0 * slant);
            }
        }
        held(&self.measures.ink).insert(key, v);
        v
    }

    /// Placement of the hostname, in pixels (letter x positions in letter units). Default: ink's top
    /// right corner at CORNER. TITLE_CORNER bottom-left: first letter's foot inset equally from left and
    /// bottom.
    pub fn title_place(&self, width: i32, height: i32, host: &str, edges: Edges) -> Placed {
        let tw = &self.tw;
        let text = self.title_text(host);
        let u = self.title_unit(width, height, &text);
        let cap = 6.0 * u;
        let (xs, span) = self.title_layout(&text);
        if tw.title_corner == TitleCorner::BottomLeft {
            let Edges { bottom, left, .. } = edges;
            let base = height as f64 - bottom as f64 - self.margin(width, height);
            let x0 = left as f64 + self.margin(width, height) - xs.first().copied().unwrap_or(0.0) * u;
            return Placed { text, u, cap, xs, y0: base - cap, base, x0 };
        }
        if tw.corner != 0.0 {
            let (above, past) = self.title_ink(&text);
            let y0 = tw.corner * unit(width, height) + above * u;
            let x0 = self.title_right(width, height) - (span + past) * u - tw.slant * cap;
            return Placed { text, u, cap, xs, y0, base: y0 + cap, x0 };
        }
        let y0 = tw.title_top * unit(width, height);
        let x0 = self.title_right(width, height) - span * u - tw.slant * cap;
        Placed { text, u, cap, xs, y0, base: y0 + cap, x0 }
    }

    /// Caption rows under the hostname: (ruler y, text baseline, font size).
    pub fn caption_rows(&self, width: i32, height: i32, host: &str) -> (f64, f64, f64) {
        let base = self.title_place(width, height, host, Edges::NONE).base;
        let ruler = base + 0.03 * unit(width, height);
        (ruler, ruler + 0.03 * unit(width, height), 0.012 * width.min(height) as f64)
    }

    /// Rectangle of the hostname, glow and ruler; widgets keep clear of it.
    pub fn title_box(&self, width: i32, height: i32, host: &str, edges: Edges) -> (f64, f64, f64, f64) {
        let p = self.title_place(width, height, host, edges);
        let reach = 3.0 * self.tw.title_glow.iter().map(|g| g.0).fold(f64::NEG_INFINITY, f64::max);
        if self.tw.title_corner == TitleCorner::BottomLeft {
            let right = p.x0 + (self.title_layout(&p.text).1 + self.tw.slant * 6.0) * p.u;
            return ((p.x0 - reach).floor(), (p.y0 - reach).floor(), right.ceil() + reach, height as f64);
        }
        (
            (p.x0 - reach).floor(),
            0f64.max((p.y0 - reach).floor()),
            (self.title_right(width, height)).round() + reach,
            self.caption_rows(width, height, host).0.ceil() + 4.0,
        )
    }

    /// The solid hostname on transparency: chamfered, slanted, extruded, gradient face, sliced lower
    /// half. Drawn at `scale` times size and downsampled. Returns (surface, left, baseline).
    pub fn title(&self, width: i32, height: i32, scale: i32, host: &str, edges: Edges) -> (ImageSurface, f64, f64) {
        let p = self.title_place(width, height, host, edges);
        (self.title_drawn(width, height, scale, &p), p.x0, p.base)
    }

    fn title_drawn(&self, width: i32, height: i32, scale: i32, p: &Placed) -> ImageSurface {
        let tw = &self.tw;
        let (text, u, cap, xs, y0, base, x0) = (&p.text, p.u, p.cap, &p.xs, p.y0, p.base, p.x0);
        let (e0, e1) = (tw.extrude[0], tw.extrude[1]);
        let steps = e0.0 as i64;
        let (f, n) = (e0.1.rgb(), e1.1.rgb());
        let depth = e0.0 * 0.1 * u;
        let slant = tw.slant;
        let gw = tw.glyph_width;
        let face = cairo::LinearGradient::new(0.0, y0, 0.0, base);
        for &(at, colour) in &tw.title_colours {
            let (r, g, b) = colour.rgb();
            face.add_color_stop_rgb(at, r, g, b);
        }
        let slices = &tw.slices;
        let draw = |c: &Context| {
            let Ok(target) = ImageSurface::try_from(c.target()) else { return };
            let pixels = c.matrix();
            let upright = surface(target.width(), target.height());
            {
                let uc = ctx(&upright);
                uc.set_matrix(pixels);
                uc.translate(x0, y0);
                uc.scale(u, u);
                let letter = |dx: f64, dy: f64| {
                    for (&ch, &x) in text.iter().zip(xs.iter()) {
                        self.stroke_glyph(&uc, ch, x + dx, dy);
                    }
                };
                for step in (1..=steps).rev() {
                    let t = step as f64 / steps as f64;
                    uc.set_source_rgb(n.0 + (f.0 - n.0) * t, n.1 + (f.1 - n.1) * t, n.2 + (f.2 - n.2) * t);
                    letter(step as f64 * 0.1, step as f64 * 0.1);
                }
                uc.set_matrix(pixels);
                let _ = uc.set_source(&face);
                uc.translate(x0, y0);
                uc.scale(u, u);
                letter(0.0, 0.0);
            }
            c.save().ok();
            c.transform(Matrix::new(1.0, 0.0, -slant, 1.0, slant * base, 0.0));
            let at = cairo::SurfacePattern::create(&upright);
            at.set_matrix(pixels);
            let _ = c.set_source(&at);
            let _ = c.paint();
            c.restore().ok();
            c.set_operator(cairo::Operator::Clear);
            for &[top, thick] in slices {
                c.rectangle(0.0, y0 + top * cap, width as f64, thick * cap);
            }
            let _ = c.fill();
        };
        let bbox = if let (Some(first), Some(last)) = (xs.first(), xs.last()) {
            let pad = u + gw * u;
            let (ux0, uy0) = (x0 + (first - 1.0) * u - pad, y0 - pad);
            let (ux1, uy1) = (x0 + (last + 5.0) * u + depth + pad, base + depth + pad);
            let leaned: Vec<f64> = [ux0, ux1].iter().flat_map(|&x| [uy0, uy1].map(|y| x - slant * (y - base))).collect();
            (leaned.iter().cloned().fold(f64::INFINITY, f64::min), uy0, leaned.iter().cloned().fold(f64::NEG_INFINITY, f64::max), uy1)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        supersampled(width, height, scale, draw, Some(bbox))
    }

    /// The ruled line under the hostname, bright at the right end (none without RULE). Returns the
    /// ruler.
    pub fn caption(&self, c: &Context, width: i32, height: i32, left: f64, base: f64) -> Ruler {
        let tw = &self.tw;
        let right = self.title_right(width, height);
        let y = base + 0.03 * unit(width, height);
        let end = tw.ruler_end;
        let w = width as f64;
        let ruler = Ruler { left, stop: right - end * w, y: y.round() };
        if !tw.rule {
            return ruler;
        }
        let cap = tw.caption.rgb();
        c.set_source_rgba(cap.0, cap.1, cap.2, 0.55);
        c.rectangle(left, y.round(), right - left, 1.0);
        let _ = c.fill();
        if end != 0.0 {
            let hz = tw.horizon.rgb();
            c.set_source_rgb(hz.0, hz.1, hz.2);
            c.rectangle(right - end * w, y.round() - 1.0, end * w, 3.0);
            let _ = c.fill();
        }
        ruler
    }

    /// The ruler's ticks at time t, every fifth long, dimmed to match the vignette.
    pub fn ticks(&self, c: &Context, width: i32, height: i32, ruler: Ruler, t: f64) {
        let Ruler { left, stop, y } = ruler;
        let w = width as f64;
        let step = 0.012 * w;
        let offset = self.tick_offset(width, t);
        let right = stop + self.tw.ruler_end * w;
        let (cx, cy, big) = (w / 2.0, height as f64 / 2.0, width.max(height) as f64);
        let r = ((left + stop) / 2.0 - cx).hypot(y - cy);
        let dark = self.tw.vignette * 1f64.min(0f64.max((r - 0.35 * big) / (0.4 * big)));
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
            c.rectangle(x, y - tall, 1.0, tall);
        }
        let cap = self.tw.caption.rgb();
        c.set_source_rgba(cap.0 * (1.0 - dark), cap.1 * (1.0 - dark), cap.2 * (1.0 - dark), 0.55);
        let _ = c.fill();
    }

    /// Tick drift at time t, snapped to quarter pixels.
    pub fn tick_offset(&self, width: i32, t: f64) -> f64 {
        let step = 0.012 * width as f64;
        ((t / self.tw.tick_seconds * step).rem_euclid(5.0 * step) * 4.0).round() / 4.0
    }

    /// The hostname as neon tubes, one letter per cell: lit in the atlas's top half, dark in the bottom.
    pub fn tube_title(&self, width: i32, height: i32, scale: i32, host: &str, edges: Edges) -> TubeAtlas {
        let tw = &self.tw;
        let p = self.title_place(width, height, host, edges);
        let (u, cap, y0, base, x0) = (p.u, p.cap, p.y0, p.base, p.x0);
        let slant = tw.slant;
        let glow = &tw.tube_glow;
        let reach = 3.0 * glow.iter().map(|g| g.0).fold(f64::NEG_INFINITY, f64::max) + 2.0;
        let stops = &tw.title_colours;
        let core = stops.first().map_or(self.palette.title_top, |s| s.1).rgb();
        let tube = stops.get(1).map_or(self.palette.title_light, |s| s.1).rgb();
        let [share_tube, share_core] = tw.tube;
        let gw = tw.glyph_width;
        let unlit = tw.tube_unlit;
        let shadow = tw.title_shadow;
        let mut pieces = vec![];
        for (&ch, &gx) in p.text.iter().zip(p.xs.iter()) {
            let lx0 = x0 + (gx - 1.0) * u;
            let lx1 = x0 + (gx + 5.0) * u + slant * cap;
            let bx = (lx0 - reach).floor() as i32;
            let by = (y0 - u - reach).floor() as i32;
            let w = (lx1 + reach).ceil() as i32 - bx;
            let h = (base + u + reach).ceil() as i32 - by;
            let mut lit = vec![];
            for strength in [1.0, unlit] {
                let big = surface(w * scale, h * scale);
                {
                    let c = ctx(&big);
                    let matrix = Matrix::new(scale as f64, 0.0, 0.0, scale as f64, (-bx * scale) as f64, (-by * scale) as f64);
                    c.set_matrix(matrix);
                    c.transform(Matrix::new(1.0, 0.0, -slant, 1.0, slant * base, 0.0));
                    for stroke in self.glyph(ch).into_iter().flatten() {
                        for (k, &(px, py)) in stroke.iter().enumerate() {
                            let (px_, py_) = (x0 + (gx + px) * u, y0 + py * u);
                            if k > 0 { c.line_to(px_, py_) } else { c.move_to(px_, py_) }
                        }
                        if stroke.len() > 2 && stroke[0] == stroke[stroke.len() - 1] {
                            c.close_path();
                        }
                    }
                    let path = c.copy_path();
                    c.set_matrix(matrix);
                    c.set_line_cap(LineCap::Round);
                    c.set_line_join(LineJoin::Round);
                    let dull = (tube.0 * strength, tube.1 * strength, tube.2 * strength);
                    let looks = if strength == 1.0 { [tube, core] } else { [dull, dull] };
                    for (share, colour) in [share_tube, share_core].into_iter().zip(looks) {
                        c.new_path();
                        if let Ok(path) = &path {
                            c.append_path(path);
                        }
                        c.set_line_width(share * gw * u);
                        c.set_source_rgba(colour.0, colour.1, colour.2, 1.0);
                        let _ = c.stroke();
                    }
                }
                let mut small = resampled(&big, 1.0 / scale as f64, Filter::Good);
                if strength == 1.0 {
                    let g = surface(w, h);
                    glowing(&ctx(&g), &small, glow);
                    small = g;
                }
                lit.push(small);
            }
            let caster = lit[0].clone();
            let lit: Vec<ImageSurface> = lit.into_iter().map(|s| drop_shadow(s, &shadow, u / 10.0, Some(&caster))).collect();
            pieces.push((lit, bx, by, w, h));
        }
        let aw = pieces.iter().map(|p| p.3).sum::<i32>().max(1);
        let ah = 2 * pieces.iter().map(|p| p.4).max().unwrap_or(1);
        let atlas = surface(aw, ah);
        let mut placed = vec![];
        {
            let c = ctx(&atlas);
            let mut ax = 0;
            for (states, x, y, w, h) in &pieces {
                for (row, s) in states.iter().enumerate() {
                    let _ = c.set_source_surface(s, ax as f64, (row as i32 * (atlas.height() / 2)) as f64);
                    let _ = c.paint();
                }
                placed.push(Cell { ax, x: *x, y: *y, w: *w, h: *h });
                ax += w;
            }
        }
        atlas.flush();
        TubeAtlas { atlas, placed }
    }

    /// Top of the instruments: the caption's cap line, or just below the hostname without RULE.
    pub fn instruments_top(&self, width: i32, height: i32, host: &str) -> f64 {
        if !self.tw.rule {
            return self.title_place(width, height, host, Edges::NONE).base + 0.022 * unit(width, height);
        }
        let (_, baseline, size) = self.caption_rows(width, height, host);
        let c = probe();
        select_mono(&c, false);
        c.set_font_size(size);
        let cap = text_extents(&c, "X").map_or(0.0, |x| -x.y_bearing());
        baseline - cap
    }
}
