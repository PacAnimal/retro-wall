//! Network traffic as mountains: download as a light range, upload as a solid one over it, on a log scale
//! whose top follows the data, scrolling left, with a scale beside them.
//! Placed on the horizon, between the lower brackets, along the bottom edge, or in the bottom right corner
//! (optionally in a card with frosted glass).

use crate::config::Theme;
use crate::elements::instruments::{card_scale, label_size, mountain_scale_x, status_area, status_bottom};
use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::title::{Brackets, bracket_corners};
use crate::tweaks::{Colour, MaybeColour};
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, LineJoin, Operator, SurfacePattern};
use serde::Deserialize;
use std::collections::HashMap;

/// A scale label for 10^d bit/s: 100k, 1M, 10M ... 100G.
pub fn decade_name(d: i64) -> String {
    let prefix = match 3 * d.div_euclid(3) {
        3 => "k",
        6 => "M",
        9 => "G",
        _ => "?",
    };
    format!("{}{}", 10i64.pow(d.rem_euclid(3) as u32), prefix)
}

/// The x span where the horizon mountains fade out: (start, end).
pub fn mountains_span(th: &Theme, w: i32, h: i32) -> (f64, f64) {
    let u = unit(w, h);
    let sun = th.tw.sun;
    let sun_right = sun[0] * w as f64 + sun[1] * u;
    let bend = sun_right + th.tw.turn_gap * u;
    (bend, bend + th.tw.mountain_fade[0] * th.tw.phi * (bend - sun_right))
}

/// A label's (width, ascent, height).
pub fn label_metrics(text: &str, size: f64) -> (i32, i32, i32) {
    let c = probe();
    select_mono(&c, false);
    c.set_font_size(size);
    let (Ok(ext), Ok(fe)) = (text_extents(&c, text), c.font_extents()) else { return (1, 1, 1) };
    (ext.x_advance().ceil() as i32 + 1, fe.ascent().round() as i32, (fe.ascent() + fe.descent()).ceil() as i32)
}

type Rect5 = (f64, f64, f64, f64, f64);

struct Canvas {
    key: (u64, u64, u64),
    surface: ImageSurface,
    upto: Option<i64>,
}

/// The two ranges and their scale.
struct Ranges {
    width: i32,
    height: i32,
    bottom: Option<f64>,
    hz: f64,
    left: f64,
    right: f64,
    fade: (f64, f64),
    fade_in: f64,
    scale_x: f64,
    dx: f64,
    full: f64,
    ceiling: f64,
    ridge: f64,
    last: Option<f64>,
    base: Option<i64>,
    epoch: u64,
    top: f64,
    fill: [((f64, f64, f64), f64); 2],
    ridges: [((f64, f64, f64), f64); 2],
    caption: (f64, f64, f64),
    shadow: Shadow,
    bold: bool,
    strength: f64,
    labels: HashMap<String, (i32, i32, i32)>, // width, ascent, height
    canvases: [Option<Canvas>; 4],            // by quarter-pixel phase
    floor: f64,
    /// MOUNTAINS' height, of the unit.
    tall: f64,
    /// The instruments' card scale: the scale's shadow follows it.
    card_scale: f64,
    top_tier: i64,
    heights: Vec<(f64, [f64; 2])>, // a paint's samples, reused
}

enum Across {
    None,
    Edge,
    Corner(f64),
}

impl Ranges {
    fn new(g: &Geo, w: &World, rise: f64, bottom: Option<f64>, span: f64, fade: f64, across: Across, lift: f64, pad: f64) -> Self {
        let th = &g.th;
        let (width, height, edges) = (g.w, g.h, g.edges);
        let u = unit(width, height);
        let (hz, left, right, fade_r, fade_in, scale_x);
        if bottom.is_some() {
            let Brackets { left: bl, right: br, bottom: line, arm: length, .. } = bracket_corners(width, height, edges);
            let gap = 0.03 * u;
            let (h0, l0, r0) = match across {
                // along the whole bottom edge, nothing faded, its zero line a little above it
                Across::Edge => ((height - edges.bottom) as f64 - 3f64.max((0.006 * u).round()), 0.0, width as f64),
                // in the corner, lifted by what stands under it
                Across::Corner(across) => ((line - lift).round(), (br - across * width as f64).round(), (br - pad).round()),
                Across::None => {
                    let (a, b) = (bl + length + gap, br - length - gap);
                    let inset = (b - a) * (1.0 - span) / 2.0; // narrower, still centred
                    (line.round(), (a + inset).round(), (b - inset).round())
                }
            };
            hz = h0;
            left = l0;
            right = r0;
            if matches!(across, Across::Edge) {
                fade_r = (0.0, 0.0);
                fade_in = 0.0;
                scale_x = (width as f64 - edges.right as f64 - 0.03 * u).round();
            } else {
                fade_r = (left, left + (fade * u).round());
                fade_in = (0.03 * u).round();
                scale_x = right;
            }
        } else {
            hz = th.horizon_y(height) - (rise * u).round();
            fade_r = mountains_span(th, width, height);
            left = fade_r.0;
            right = width as f64;
            fade_in = 0.0;
            scale_x = mountain_scale_x(width, height);
        }
        let ridge = 1f64.max(1.4 * u / 800.0);
        let full = match bottom {
            Some(b) => b * height as f64,
            None => th.tw.mountains[0] * u,
        };
        let (d, up) = (th.tw.download, th.tw.upload);
        let floor = th.tw.mountain_floor as f64;
        let top_tier = th.tw.mountain_tiers.last().copied().unwrap_or(th.tw.mountain_floor + 1);
        let size = label_size(width, height) as f64;
        let mut labels = HashMap::new();
        for name in std::iter::once("bit/s".to_string()).chain((floor as i64 + 1..=top_tier).map(decade_name)) {
            labels.insert(name.clone(), label_metrics(&name, size));
        }
        Ranges {
            width,
            height,
            bottom,
            hz,
            left,
            right,
            fade: fade_r,
            fade_in,
            scale_x,
            dx: (right - left) / w.traffic.columns as f64,
            full,
            ceiling: (hz - full - 2.0 * ridge).floor(),
            ridge,
            last: None,
            base: None,
            epoch: 0,
            top: w.traffic.goal,
            fill: [(d.0.rgb(), d.1), (up.0.rgb(), up.1)],
            ridges: [(d.2.rgb(), d.3), (up.2.rgb(), up.3)],
            caption: th.tw.caption.rgb(),
            shadow: Shadow { dx: 0.0, dy: 0.0, blur: 0.0, strength: 0.0, colour: Colour(0.0, 0.0, 0.0) },
            bold: false,
            strength: 0.6,
            labels,
            canvases: [None, None, None, None],
            floor,
            tall: th.tw.mountains[0],
            card_scale: card_scale(th, width, height),
            top_tier,
            heights: vec![],
        }
    }
    /// Pixels kept below the baseline: half the ridge and one, so a flat ridge shows whole.
    fn below(&self) -> i32 {
        (self.ridge / 2.0).ceil() as i32 + 1
    }
    fn pad(&self) -> f64 {
        self.ridge + 2.0
    }
    fn c(&self, n: i64) -> f64 {
        (n - self.base.unwrap()) as f64 * self.dx
    }
    fn canvas_width(&self, columns: i64) -> i32 {
        ((columns + 2) as f64 * self.dx + 768.0 + 2.0 * self.pad()).ceil() as i32
    }
    /// Height in pixels for a rate in bit/s at scale `top`.
    fn rise(&self, rate: f64, top: f64) -> f64 {
        if self.bottom.is_some() {
            return 1f64.min(0f64.max((rate.max(1.0).log10() - self.floor) / (top - self.floor))) * self.full;
        }
        let hh = 1f64.min(0f64.max((rate.max(1.0).log10() - self.floor) / (top - self.floor)));
        hh * self.tall * unit(self.width, self.height)
    }

    /// (version, shift, top): data version, scroll offset in quarter pixels, and scale top as log10 bit/s
    /// rounded to 0.001.
    fn at(&mut self, w: &mut World) -> (u64, f64, f64) {
        let t = w.clock_real; // traffic is sampled in real time, like the live loop does
        let demo = w.demo;
        w.traffic.sample(t, demo);
        let n = w.traffic.newest;
        if let Some(n) = n
            && (self.base.is_none() || self.c(n) + 2.0 * self.dx + self.pad() > self.canvas_width(w.traffic.columns) as f64)
        {
            self.base = Some(n - w.traffic.columns - 2);
            self.epoch += 1;
        }
        let dt = self.last.map_or(0.0, |l| (t - l).min(0.1));
        self.last = Some(t);
        let goal = w.traffic.goal;
        self.top += (goal - self.top) * (1.0 - (-dt / 0.5).exp());
        if (goal - self.top).abs() < 0.001 {
            self.top = goal;
        }
        let x0 = match self.base {
            None => 0.0,
            Some(b) => self.right + self.dx - (t / crate::sources::SAMPLE - b as f64) * self.dx,
        };
        (w.traffic.version, (x0 * 4.0).round() / 4.0, ((self.top) * 1000.0).round() / 1000.0)
    }

    /// Draw samples n0 ..= n1: download fill, upload fill, upload ridge, download ridge.
    fn paint(&mut self, c: &Context, w: &World, n0: i64, n1: i64, top: f64, strength: f64) {
        let mut hs = std::mem::take(&mut self.heights);
        hs.clear();
        hs.extend((n0..=n1).filter_map(|n| w.traffic.rate(n).map(|r| (self.c(n), [self.rise(r.0, top), self.rise(r.1, top)]))));
        if hs.len() >= 2 {
            self.draw_ranges(c, &hs, strength);
        }
        self.heights = hs;
    }

    fn draw_ranges(&self, c: &Context, hs: &[(f64, [f64; 2])], strength: f64) {
        let hz = self.hz;
        let trace = |k: usize| {
            c.move_to(hs[0].0, hz - hs[0].1[k]);
            for (x, h) in &hs[1..] {
                c.line_to(*x, hz - h[k]);
            }
        };
        for k in 0..2 {
            trace(k);
            c.line_to(hs[hs.len() - 1].0, hz);
            c.line_to(hs[0].0, hz);
            c.close_path();
            let ((r, g, b), a) = self.fill[k];
            c.set_source_rgba(r, g, b, a * strength);
            let _ = c.fill();
        }
        c.set_line_width(self.ridge);
        c.set_line_join(LineJoin::Round);
        for k in [1, 0] {
            trace(k);
            let ((r, g, b), a) = self.ridges[k];
            c.set_source_rgba(r, g, b, a * strength);
            let _ = c.stroke();
        }
    }

    /// Update the canvas for a quarter-pixel phase. Returns (dirty: 0 none, 1 all, 2 columns, x0, x1).
    fn canvas(&mut self, w: &World, phase: i32, top: f64, strength: f64) -> (i32, i32, i32) {
        let key = (self.epoch, top.to_bits(), strength.to_bits());
        let cw = self.canvas_width(w.traffic.columns);
        // room below the baseline for the ridge's full width: zero traffic is a whole line
        let h = self.hz.ceil() as i32 - self.ceiling as i32 + 1 + self.below();
        // the history is shared: another monitor may sample it before this one has a base
        let newest = w.traffic.newest.filter(|_| self.base.is_some());
        let p = phase as usize;
        let fresh = self.canvases[p].as_ref().is_none_or(|c| c.key != key);
        if fresh {
            let s = surface(cw, h);
            {
                let c = ctx(&s);
                c.translate(phase as f64 / 4.0, -self.ceiling);
                if let Some(n) = newest {
                    self.paint(&c, w, self.base.unwrap(), n, top, strength);
                }
            }
            s.flush();
            self.canvases[p] = Some(Canvas { key, surface: s, upto: newest });
            return (1, 0, 0);
        }
        let upto = self.canvases[p].as_ref().unwrap().upto;
        let (Some(n), Some(upto)) = (newest, upto) else { return (0, 0, 0) };
        if upto == n {
            return (0, 0, 0);
        }
        let a = self.c(upto) + phase as f64 / 4.0 - self.pad();
        let b = self.c(n) + phase as f64 / 4.0 + self.pad();
        let (x0, x1) = (0.max(a.floor() as i32), cw.min(b.ceil() as i32));
        let back = ((self.pad() + self.ridge + 1.0) / self.dx).ceil() as i64 + 1;
        let s = self.canvases[p].as_ref().unwrap().surface.clone();
        {
            let c = ctx(&s);
            c.rectangle(x0 as f64, 0.0, (x1 - x0) as f64, h as f64);
            c.clip();
            c.set_operator(Operator::Clear);
            let _ = c.paint();
            c.set_operator(Operator::Over);
            c.translate(phase as f64 / 4.0, -self.ceiling);
            self.paint(&c, w, self.base.unwrap().max(upto - back), n, top, strength);
        }
        s.flush();
        self.canvases[p].as_mut().unwrap().upto = newest;
        (2, x0, x1)
    }

    /// The scale at `top` as (rects, labels): the upright, a labelled tick per decade (fading in as it
    /// rises into view), and "bit/s" above.
    fn marks(&self, top: f64) -> (Vec<Rect5>, Vec<(String, f64, f64, f64)>) {
        let (x, hz) = (self.scale_x, self.hz.round());
        let size = label_size(self.width, self.height) as f64;
        let mut rects = vec![(x, hz - self.full.round(), x + 1.0, hz, 1.0)];
        let (tick, gap) = ((size * 0.8).round(), (size * 0.4).round());
        let lw = |n: &str| self.labels[n].0 as f64;
        let la = |n: &str| self.labels[n].1;
        let mut labels = vec![("bit/s".to_string(), x - tick - gap - lw("bit/s"), hz - self.full.round() - (size * 1.9).round(), 1.0)];
        for d in self.floor as i64 + 1..=self.top_tier {
            let a = 1f64.min(0f64.max((top + 0.15 - d as f64) / 0.3));
            if a == 0.0 {
                continue;
            }
            let y = (hz - self.rise(10f64.powi(d as i32), top)).round();
            rects.push((x - tick + 1.0, y, x + 1.0, y + 1.0, a));
            let name = decade_name(d);
            labels.push((name.clone(), x - tick - gap - lw(&name), y - la(&name).div_euclid(2) as f64 - 1.0, a));
        }
        (rects, labels)
    }

    /// The scale at `top` as one image: (surface, left, top).
    fn scale_surface(&self, top: f64) -> (ImageSurface, f64, f64) {
        let (rects, labels) = self.marks(top);
        let size = label_size(self.width, self.height) as f64;
        let mut boxes: Vec<Rect> = rects.iter().map(|r| (r.0, r.1, r.2, r.3)).collect();
        boxes.extend(labels.iter().map(|(n, x, y, _)| (*x, *y, x + self.labels[n].0 as f64, y + self.labels[n].2 as f64)));
        let pad = if self.shadow.strength > 0.0 { 6.0 } else { 0.0 };
        let bx = boxes.iter().map(|b| b.0).fold(f64::INFINITY, f64::min) - pad;
        let by = boxes.iter().map(|b| b.1).fold(f64::INFINITY, f64::min) - pad;
        let w = boxes.iter().map(|b| b.2).fold(f64::NEG_INFINITY, f64::max) - bx + pad;
        let h = boxes.iter().map(|b| b.3).fold(f64::NEG_INFINITY, f64::max) - by + pad;
        let strength = self.strength;
        let out = surface(w as i32, h as i32);
        let mut fades: Vec<f64> = rects.iter().map(|r| r.4).chain(labels.iter().map(|l| l.3)).collect();
        fades.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        fades.dedup();
        let (r, gg, b) = self.caption;
        {
            let o = ctx(&out);
            for fade in fades {
                let layer = surface(w as i32, h as i32);
                {
                    let c = ctx(&layer);
                    c.translate(-bx, -by);
                    for rc in rects.iter().filter(|rc| rc.4 == fade) {
                        c.set_source_rgba(r, gg, b, strength);
                        c.rectangle(rc.0, rc.1, rc.2 - rc.0, rc.3 - rc.1);
                        let _ = c.fill();
                    }
                    select_mono(&c, self.bold);
                    c.set_font_size(size);
                    for (name, x, y, _) in labels.iter().filter(|l| l.3 == fade) {
                        c.set_source_rgba(r, gg, b, strength);
                        c.move_to(*x, y + self.labels[name].1 as f64);
                        let _ = show_text(&c, name);
                    }
                }
                layer.flush();
                let shaded = drop_shadow(layer, &self.shadow, self.card_scale, None);
                let _ = o.set_source_surface(&shaded, 0.0, 0.0);
                let _ = o.paint_with_alpha(fade);
            }
        }
        out.flush();
        (out, bx, by)
    }
}

/// Where the mountains stand.
#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Place {
    /// On the horizon, from the sun to the right edge.
    Horizon,
    /// Between the lower brackets.
    Bottom,
    /// Along the whole bottom edge.
    Edge,
    /// In the bottom right corner.
    Corner,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MountainsSettings {
    rise: f64,
    place: Place,
    size: f64,
    span: f64,
    fade: f64,
    across: f64,
    card: bool,
    frost: i64,
    scale: MaybeColour,
    bold: bool,
    colours: [MaybeColour; 4],
}

impl Validate for MountainsSettings {
    fn validate(&self) -> Result<(), String> {
        within("rise", self.rise, -1.0, 1.0)?;
        within("size", self.size, 0.001, 1.0)?;
        within("span", self.span, 0.001, 1.0)?;
        within("fade", self.fade, 0.0, 1.0)?;
        within("across", self.across, 0.0, 1.0)?;
        within("frost", self.frost as f64, 0.0, 100.0)
    }
}

/// Network traffic as mountains, with their scale.
pub struct Mountains {
    rise: f64,
    place: Place,
    size: f64,
    span: f64,
    fade: f64,
    across: f64,
    card: bool,
    frost: f64,
    scale: Option<Colour>,
    bold: bool,
    colours: [Option<Colour>; 4],
    // made at start
    ranges: Option<Ranges>,
    pad: f64,
    boxes: Vec<[f64; 32]>,
    card_verts: Vec<f32>,
    card_box: (i32, i32, i32, i32),
    /// What lies under it, and that picture's place: the card's box of it once started.
    under: Option<(ImageSurface, (i32, i32))>,
    frosted_at: Option<i64>,
    shown: (u64, f64, f64, i64),
    scale_baked: Option<(f64, (f64, f64, f64, f64))>,
    /// MOUNTAIN_FADE's alpha at the start of the fade.
    fade_alpha: f64,
    backdrop_tex: Option<Tex>,
    frost_tex: Option<Tex>,
    range_tex: Option<[Tex; 4]>,
    scale_tex: Option<Tex>,
}

impl Mountains {
    fn new(t: MountainsSettings) -> Self {
        Mountains {
            rise: t.rise,
            place: t.place,
            size: t.size,
            span: t.span,
            fade: t.fade,
            across: t.across,
            card: t.card,
            frost: t.frost as f64,
            scale: t.scale.0,
            bold: t.bold,
            colours: t.colours.map(|c| c.0),
            ranges: None,
            pad: 0.0,
            boxes: vec![],
            card_verts: vec![],
            card_box: (0, 0, 0, 0),
            under: None,
            frosted_at: None,
            shown: (0, 0.0, 0.0, 0),
            scale_baked: None,
            fade_alpha: 0.0,
            backdrop_tex: None,
            frost_tex: None,
            range_tex: None,
            scale_tex: None,
        }
    }

    /// Whether it lies on frosted glass.
    fn frosted_glass(&self) -> bool {
        self.card && self.frost != 0.0
    }

    /// What lies under the card, blurred.
    fn frosted(&self) -> ImageSurface {
        let (x0, y0, x1, y1) = self.card_box;
        let (w, h) = (x1 - x0, y1 - y0);
        let picture = surface(w, h);
        if let Some((under, (ux, uy))) = &self.under {
            let c = ctx(&picture);
            if c.set_source_surface(under, (ux - x0) as f64, (uy - y0) as f64).is_ok() {
                let _ = c.paint();
            }
        }
        let small = resampled(&picture, 1.0 / self.frost, Filter::Good);
        let out = surface(w, h);
        {
            let c = ctx(&out);
            c.scale(w as f64 / small.width() as f64, h as f64 / small.height() as f64);
            let p = SurfacePattern::create(&small);
            p.set_filter(Filter::Bilinear);
            let _ = c.set_source(&p);
            let _ = c.paint();
        }
        out
    }

    /// Key that changes once a minute, so the frost is remade; 0 in a snapshot.
    fn frost_key(&self, w: &World) -> i64 {
        if w.frozen.is_some() || self.frost == 0.0 {
            return 0;
        }
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64 / 60).unwrap_or(0)
    }
}

fn fill_quad(x0: f64, y0: f64, x1: f64, y1: f64, (r, g, b): (f64, f64, f64), a: f64) -> [f64; 32] {
    [0.0, 0.0, r, g, b, a, x0, y0, 0.0, 0.0, r, g, b, a, x1, y0, 0.0, 0.0, r, g, b, a, x1, y1, 0.0, 0.0, r, g, b, a, x0, y1]
}

impl Part for Mountains {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Graph)
    }
    fn live(&self) -> bool {
        true
    }
    fn fades(&self) -> bool {
        true
    }
    fn backdrop(&self) -> Backdrop {
        Backdrop::Texture
    }
    fn set_backdrop_tex(&mut self, tex: Tex) {
        self.backdrop_tex = Some(tex);
    }
    fn under(&self) -> bool {
        self.frosted_glass()
    }
    fn set_under(&mut self, picture: &ImageSurface) {
        self.under = Some((picture.clone(), (0, 0)));
    }
    fn top(&self, g: &Geo) -> i32 {
        let Some(r) = self.ranges.as_ref() else { return g.h };
        (r.ceiling - if self.card { self.pad + (label_size(g.w, g.h) as f64 * 2.0).round() } else { 0.0 }) as i32
    }
    fn start(&mut self, g: &Geo, w: &mut World, s: &mut Shared) {
        let th = &g.th;
        self.fade_alpha = th.tw.mountain_fade[1];
        let (width, height) = (g.w, g.h);
        let u = unit(width, height);
        let mut lift = 0.0;
        self.pad = if self.card { (0.012 * u).round() } else { 0.0 };
        let status_shown = g.header.contains(Piece::Widgets) && th.layout.iter().any(|gr| gr.widgets.iter().any(|n| n == "status"));
        let line = bracket_corners(width, height, g.edges).bottom;
        if self.place == Place::Corner && status_shown {
            lift = status_bottom(th, width, height, w.zoom, line) - line;
        }
        let mut across = self.across;
        if self.card && status_shown {
            let right = bracket_corners(width, height, g.edges).right - self.pad;
            let status = status_area(th, width, height, w.zoom, Some(GraphPlace { left: 0.0, right, base: line, corner: true })).x;
            across = across.max((right + self.pad - status) / width as f64);
        }
        let mode = match self.place {
            Place::Edge => Across::Edge,
            Place::Corner => Across::Corner(across),
            _ => Across::None,
        };
        let mut r = Ranges::new(g, w, self.rise, (self.place != Place::Horizon).then_some(self.size), self.span, self.fade, mode, lift + self.pad, self.pad);
        if self.card {
            let Brackets { right, bottom: line, .. } = bracket_corners(width, height, g.edges);
            let top = r.hz - r.full - (label_size(width, height) as f64 * 1.9).round();
            let status = if status_shown {
                status_area(th, width, height, w.zoom, Some(GraphPlace { left: r.left, right: r.right, base: r.hz, corner: true })).x
            } else {
                r.left
            };
            let bx = ((r.left.min(status) - self.pad).floor(), (top - self.pad).floor(), right.round(), line.round());
            let (fill, fa) = (th.tw.deck_fill.0.rgb(), th.tw.deck_fill.1);
            let edge = th.tw.deck_colour.rgb();
            let da = th.tw.deck_alpha;
            let (x0, y0, x1, y1) = bx;
            let mut quads = vec![fill_quad(x0, y0, x1, y1, fill, fa)];
            for q in [(x0, y0, x1, y0 + 1.0), (x0, y1 - 1.0, x1, y1), (x0, y0 + 1.0, x0 + 1.0, y1 - 1.0), (x1 - 1.0, y0 + 1.0, x1, y1 - 1.0)] {
                quads.push(fill_quad(q.0, q.1, q.2, q.3, edge, da));
            }
            self.boxes = quads;
            self.card_verts = Vec::with_capacity(self.boxes.len() * 32);
            self.card_box = (x0 as i32, y0 as i32, x1 as i32, y1 as i32);
            self.frosted_at = None;
            // only the card's box of what lies under it is frosted
            if let Some((under, at)) = self.under.take() {
                let (bx0, by0, bx1, by1) = self.card_box;
                let (ux0, uy0) = (bx0.max(at.0), by0.max(at.1));
                let (ux1, uy1) = (bx1.min(at.0 + under.width()), by1.min(at.1 + under.height()));
                if ux1 > ux0 && uy1 > uy0 {
                    let part = surface(ux1 - ux0, uy1 - uy0);
                    let c = ctx(&part);
                    if c.set_source_surface(&under, (at.0 - ux0) as f64, (at.1 - uy0) as f64).is_ok() {
                        let _ = c.paint();
                    }
                    drop(c);
                    self.under = Some((part, (ux0, uy0)));
                }
            }
        }
        self.scale_baked = None;
        if let Some(scale) = self.scale {
            r.caption = scale.rgb();
            r.shadow = th.tw.status_shadow;
        }
        r.bold = self.bold;
        r.strength = if self.scale.is_none() { 0.6 } else { 1.0 };
        let own = self.colours.map(|c| c.map(|c| c.rgb()));
        r.fill = [(own[0].unwrap_or(r.fill[0].0), r.fill[0].1), (own[2].unwrap_or(r.fill[1].0), r.fill[1].1)];
        r.ridges = [(own[1].unwrap_or(r.ridges[0].0), r.ridges[0].1), (own[3].unwrap_or(r.ridges[1].0), r.ridges[1].1)];
        if self.place == Place::Bottom || self.place == Place::Corner {
            s.graph = Some(GraphPlace { left: r.left, right: r.right, base: r.hz, corner: self.place == Place::Corner });
        }
        self.ranges = Some(r);
    }
    /// Placeholders, replaced as they are drawn.
    fn textures(&mut self, _g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.range_tex = Some(std::array::from_fn(|_| up.add(surface(1, 1), Kind::Label)));
        self.scale_tex = Some(up.add(surface(1, 1), Kind::Label));
        self.frost_tex = self.frosted_glass().then(|| up.add(surface(1, 1), Kind::Smooth));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let Some(ranges) = self.ranges.as_mut() else { return 0 };
        let (version, shift, top) = ranges.at(w);
        let key = if self.frost != 0.0 { self.frost_key(w) } else { 0 };
        self.shown = (version, shift, top, key);
        let mut h = version;
        hash_f(&mut h, shift);
        hash_f(&mut h, top);
        hash_f(&mut h, key as f64);
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        let (_, shift, scale, key) = self.shown;
        if let Some(tex) = self.frost_tex {
            // frosted glass
            let (x0, y0, x1, y1) = self.card_box;
            if self.frosted_at != Some(key) {
                gfx.replace(tex, &self.frosted(), Kind::Smooth);
                self.frosted_at = Some(key);
            }
            gfx.image(tex, x0 as f64, y0 as f64, (x1 - x0) as f64, (y1 - y0) as f64, p, Blend::Premultiplied);
        }
        if self.card {
            // the card, fading with the instruments
            self.card_verts.clear();
            for q in &self.boxes {
                for (k, v) in q.iter().enumerate() {
                    self.card_verts.push(if k % 8 == 5 { (v * p) as f32 } else { *v as f32 });
                }
            }
            gfx.draw(None, &self.card_verts, Blend::Alpha);
        }
        let whole = shift.floor();
        let phase = (((shift - whole) * 4.0).round() as i32).rem_euclid(4);
        let (Some(range_tex), Some(scale_tex), Some(r)) = (self.range_tex, self.scale_tex, self.ranges.as_mut()) else { return };
        let tex = range_tex[phase as usize];
        let (dirty, x0, x1) = r.canvas(w, phase, scale, p);
        let Some(s) = r.canvases[phase as usize].as_ref().map(|c| &c.surface) else { return };
        if dirty == 1 {
            gfx.replace(tex, s, Kind::Label);
        } else if dirty == 2 {
            gfx.replace_columns(tex, s, x0, x1);
        }
        let (sw, sh) = (s.width() as f64, s.height() as f64);
        if w.traffic.newest.is_some() {
            let (_, y, pw, h) = gfx.rect;
            let (top, hz) = (r.ceiling, r.hz.round());
            let (start, end) = r.fade;
            let left = start.floor();
            if r.bottom.is_some() {
                // between the brackets: fade in and out at both ends
                let out = r.right - r.fade_in;
                let below = r.below();
                gfx.scissor(Some((left as i32, y + h - hz as i32 - below, (r.right - left) as i32, (hz - top) as i32 + below)));
                gfx.spans(tex, whole, r.ceiling, sw, sh, &[(left, end, 0.0, 1.0), (end, out, 1.0, 1.0), (out, r.right, 1.0, 0.0)], Blend::Premultiplied);
                gfx.scissor(None);
            } else {
                let below = r.below();
                gfx.scissor(Some((left as i32, y + h - hz as i32 - below, pw - left as i32, (hz - top) as i32 + below)));
                gfx.image(tex, whole, r.ceiling, sw, sh, 1.0, Blend::Premultiplied);
                // fade out by painting the backdrop back over them
                if let Some(backdrop) = self.backdrop_tex {
                    gfx.fade(backdrop, left + 1.0, top, end, hz, self.fade_alpha, 0.0, Blend::Alpha);
                }
                gfx.scissor(None);
            }
        }
        let baked = match self.scale_baked {
            Some((top, at)) if top == scale => at,
            _ => {
                let (s, bx, by) = r.scale_surface(scale);
                gfx.replace(scale_tex, &s, Kind::Label);
                let at = (bx, by, s.width() as f64, s.height() as f64);
                self.scale_baked = Some((scale, at));
                at
            }
        };
        let (bx, by, sw, sh) = baked;
        gfx.image(scale_tex, bx, by, sw, sh, p, Blend::Premultiplied);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "mountains",
        defaults: r#"rise = 0.0
place = "horizon"
size = 0.1
span = 1.0
fade = 0.12
across = 0.2
card = false
frost = 0
scale = ""
bold = false
colours = ["", "", "", ""]"#,
        varies: &[],
        check: check::<MountainsSettings>,
        make: |_, t, _g| Ok(vec![Box::new(Mountains::new(settings(t)))]),
    }]
}
