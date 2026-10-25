//! phosphor: a synthwave horizon in CRT amber. A sliced sun with heat shimmer setting on the horizon, a
//! floor of lines rolling toward the viewer, six stripes climbing past the sun and turning off the right
//! edge, and HUD brackets in the corners. Shapes and colours are tweaks (core.toml's [tweaks]).

use crate::config::Theme;
use crate::gfx::{Blend, Gfx, Tex, Uploads};
use crate::scene::*;
use crate::title::{Brackets, bracket_corners};
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, LinearGradient, Operator, RadialGradient};
use std::f64::consts::PI;

const SPEED: f64 = 1.0 / 3.5; // floor lines per second passing toward the viewer

/// The sun: where it stands across the screen, and its radius (both fractions).
fn sun_xy(th: &Theme) -> (f64, f64) {
    let s = th.tw.sun;
    (s[0], s[1])
}

// --- the stripes -----------------------------------------------------------------------------------

/// The height of the band's turn, on its centre line.
fn turn_y(th: &Theme, width: i32, height: i32) -> f64 {
    let tw = &th.tw;
    let climb = (1.0 - tw.turn_y) * if height > width { tw.portrait_climb } else { 1.0 };
    (1.0 - climb) * height as f64
}

type P = (f64, f64);

/// One edge of the band, its line moved down by `shift` pixels: (start below the bottom edge, turn,
/// end past the right edge).
fn edge(th: &Theme, width: i32, height: i32, shift: f64) -> (P, P, P) {
    let a = th.tw.angle.to_radians();
    let n1 = (a.sin(), a.cos());
    let sun = sun_xy(th);
    let (tx, ty) = (sun.0 * width as f64 + (sun.1 + th.tw.turn_gap) * unit(width, height), turn_y(th, width, height));
    let climb = shift * (a.cos() + th.tw.miter * (1.0 - a.cos()));
    let (x, y) = (tx + (climb - n1.1 * shift) / n1.0, ty + shift);
    let bottom = 1.2 * height as f64;
    ((x - (bottom - y) / a.tan(), bottom), (x, y), (width as f64 * 1.1, y))
}

/// A rounded corner of the band: its two tangent points, centre, and their angles about the centre.
fn fillet(th: &Theme, corner: P, radius: f64) -> (P, P, P, f64, f64) {
    let a = th.tw.angle.to_radians();
    let (u_in, u_out) = ((a.cos(), -a.sin()), (1.0, 0.0));
    let t = radius * (a / 2.0).tan();
    let t1 = (corner.0 - u_in.0 * t, corner.1 - u_in.1 * t);
    let t2 = (corner.0 + u_out.0 * t, corner.1 + u_out.1 * t);
    let bis = (a.sin(), a.cos() + 1.0);
    let blen = bis.0.hypot(bis.1);
    let d = radius / (a / 2.0).cos();
    let c = (corner.0 + bis.0 / blen * d, corner.1 + bis.1 / blen * d);
    (t1, t2, c, (t1.1 - c.1).atan2(t1.0 - c.0), (t2.1 - c.1).atan2(t2.0 - c.0))
}

/// Trace stripe i (0 is the top one) as a filled shape.
fn stripe(th: &Theme, c: &Context, width: i32, height: i32, n: usize, i: usize, w: f64, g: f64, radius: f64) {
    let shift = (i as f64 - (n as f64 - 1.0) / 2.0) * (w + g);
    let (s1, c1, e1) = edge(th, width, height, shift - w / 2.0);
    let (s2, c2, e2) = edge(th, width, height, shift + w / 2.0);
    let (t1, _t2, ce, a1, a2) = fillet(th, c1, radius);
    c.move_to(s1.0, s1.1);
    c.line_to(t1.0, t1.1);
    c.arc(ce.0, ce.1, radius, a1, a2);
    c.line_to(e1.0, e1.1);
    c.line_to(e2.0, e2.1);
    let (_t1, t2, ce, a1, a2) = fillet(th, c2, radius);
    c.line_to(t2.0, t2.1);
    c.arc_negative(ce.0, ce.1, radius, a2, a1);
    c.line_to(s2.0, s2.1);
    c.close_path();
}

fn stripes_drawn(th: &Theme, width: i32, height: i32, scale: i32) -> ImageSurface {
    let u = unit(width, height);
    let (w, g) = (th.tw.stripe * u, th.tw.gap * u);
    let colours = th.tw.stripes;
    let n = colours.len();
    let round = th.tw.round;
    let bbox = traced(|c| {
        for i in 0..n {
            stripe(th, c, width, height, n, i, w, g, round * w);
        }
    });
    supersampled(
        width,
        height,
        scale,
        |c| {
            for (i, colour) in colours.iter().enumerate() {
                let (r, gg, b) = colour.rgb();
                c.set_source_rgb(r, gg, b);
                stripe(th, c, width, height, n, i, w, g, round * w);
                let _ = c.fill();
            }
        },
        Some(bbox),
    )
}

// --- the sun ---------------------------------------------------------------------------------------

/// The cuts across the sun's lower part: (top, thickness), thinner toward the horizon.
fn slices(th: &Theme, width: i32, height: i32) -> Vec<(f64, f64)> {
    let (hz, r) = (th.horizon_y(height), sun_xy(th).1 * unit(width, height));
    let (top, n) = (hz - 0.35 * r - 0.05 * r, 7);
    (0..n)
        .map(|k| {
            let k1 = (k + 1) as f64;
            let n1 = (n + 1) as f64;
            (top + (hz - top) * k1 / n1, (0.008 + 0.032 * k1 / n1) * r)
        })
        .collect()
}

/// The sun on transparency: a disc setting on the horizon, sliced if `cut` (the shimmer wants it whole).
fn sun_drawn(th: &Theme, width: i32, height: i32, scale: i32, cut: bool) -> ImageSurface {
    let (hz, r) = (th.horizon_y(height), sun_xy(th).1 * unit(width, height));
    let (cx, cy) = (sun_xy(th).0 * width as f64, hz - 0.35 * r);
    let stops = th.tw.sun_colours.clone();
    let bbox = traced(|c| c.arc(cx, cy, r, 0.0, 2.0 * PI));
    supersampled(
        width,
        height,
        scale,
        |c| {
            c.arc(cx, cy, r, 0.0, 2.0 * PI);
            let _ = c.set_source(gradient(cy - r, hz, &stops));
            let _ = c.fill();
            c.set_operator(Operator::Clear);
            if cut {
                for (y, thick) in slices(th, width, height) {
                    c.rectangle(cx - r - 2.0, y, 2.0 * r + 4.0, thick);
                }
            }
            c.rectangle(0.0, hz, width as f64, height as f64);
            let _ = c.fill();
        },
        Some(bbox),
    )
}

/// The heat's waves: bob and swell (fractions), and two waves of (length, period).
#[derive(Clone, Copy, Default)]
struct Heat {
    bob: f64,
    swell: f64,
    one: (f64, f64),
    two: (f64, f64),
}

impl Heat {
    fn of(th: &Theme) -> Heat {
        let (bob, swell, one, two) = th.tw.shimmer;
        Heat { bob, swell, one: (one[0], one[1]), two: (two[0], two[1]) }
    }
}

/// The sun's cuts at time t, each moved by the heat: (top, thickness), into `out`.
fn cuts_at(rest: &[(f64, f64)], hz: f64, u: f64, heat: Heat, t: f64, out: &mut Vec<(f64, f64)>) {
    let ((l1, p1), (l2, p2)) = (heat.one, heat.two);
    let first = rest[0].0;
    out.clear();
    out.extend(rest.iter().map(|&(y, thick)| {
        let w = ((y - first) / (hz - first)).powf(1.2);
        let wave = |shift: f64| 0.6 * (2.0 * PI * (y / (l1 * u) + t / p1 + shift)).sin() + 0.4 * (2.0 * PI * (y / (l2 * u) + t / p2 + 2.1 * shift)).sin();
        (((y + heat.bob * u * w * wave(0.0)) * 4.0).round() / 4.0, (thick * (1.0 + heat.swell * w * wave(0.25)) * 4.0).round() / 4.0)
    }));
}

/// The pixels a span from a to b covers, as (first, last + 1, coverage), partly covered ones split off
/// (as `util::spans`, without a list).
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

/// The columns the shimmer moves: around the sun, and its bloom.
fn shimmer_x(th: &Theme, width: i32, height: i32) -> (f64, f64) {
    let sun = sun_xy(th);
    let reach = sun.1.max(th.tw.bloom[0]) + 0.04;
    (sun.0 * width as f64 - reach * unit(width, height), sun.0 * width as f64 + reach * unit(width, height))
}

// --- the floor -------------------------------------------------------------------------------------

fn grid(th: &Theme) -> (f64, f64, f64) {
    th.tw.grid.rgb()
}

/// The floor's lines that converge under the sun.
fn rays(th: &Theme, c: &Context, width: i32, height: i32) {
    let (hz, vx) = (th.horizon_y(height), sun_xy(th).0 * width as f64);
    c.set_line_width(1f64.max(unit(width, height) / 800.0));
    let n = th.tw.rays;
    let step = th.tw.ray_step;
    let (r, g, b) = grid(th);
    for k in -n..=n {
        let xb = vx + k as f64 * step * width as f64;
        let gr = LinearGradient::new(0.0, hz, 0.0, height as f64);
        gr.add_color_stop_rgba(0.0, r, g, b, 0.0);
        gr.add_color_stop_rgba(1.0, r, g, b, 0.5);
        let _ = c.set_source(&gr);
        c.move_to(vx, hz);
        c.line_to(xb, height as f64);
        let _ = c.stroke();
    }
}

/// Where a cross line at `y` runs: between the outermost rays, within the screen.
fn floor_span(th: &Theme, width: i32, height: i32, y: f64) -> (f64, f64) {
    let (hz, vx) = (th.horizon_y(height), sun_xy(th).0 * width as f64);
    let reach = th.tw.rays as f64 * th.tw.ray_step * width as f64 * (y - hz) / (height as f64 - hz);
    (0f64.max(vx - reach), (width as f64).min(vx + reach))
}

/// The floor's cross lines at `phase`: (y, alpha) for each one on screen.
fn cross_lines(th: &Theme, width: i32, height: i32, phase: f64) -> Vec<(f64, f64)> {
    let step = th.tw.grid_step;
    let hz = th.horizon_y(height);
    let h = height as f64;
    let mut lines = vec![];
    let mut k: i64 = 0;
    loop {
        let d = 1.0 + (k as f64 - phase) * step;
        k += 1;
        if d <= 0.0 {
            continue;
        }
        let y = hz + (h - hz) / d;
        let gap = (h - hz) * (1.0 / d - 1.0 / (d + step)) / (unit(width, height) / 800.0);
        if gap < 2.5 {
            break;
        }
        if y > h + 1.0 {
            continue;
        }
        let emerge = 1f64.min((gap - 2.5) / 3.5);
        let alpha = ((0.06 + 0.42 / d) * emerge * emerge * (3.0 - 2.0 * emerge) * 64.0).round() / 64.0;
        lines.push(((y * 4.0).round() / 4.0, alpha));
    }
    lines
}

/// Draw the floor's cross lines at `phase`.
fn cross(th: &Theme, c: &Context, width: i32, height: i32, phase: f64) {
    let hz = th.horizon_y(height);
    let _ = c.save();
    c.rectangle(0.0, hz, width as f64, height as f64 - hz);
    c.clip();
    c.set_line_width(1f64.max(unit(width, height) / 800.0));
    let (r, g, b) = grid(th);
    for (y, alpha) in cross_lines(th, width, height, phase) {
        let (x0, x1) = floor_span(th, width, height, y);
        c.set_source_rgba(r, g, b, alpha);
        c.move_to(x0, y);
        c.line_to(x1, y);
        let _ = c.stroke();
    }
    let _ = c.restore();
}

/// The sun's light on the ground: a wide, shallow glow under it, below the horizon.
fn bloom(th: &Theme, c: &Context, width: i32, height: i32) {
    let _ = c.save();
    let hz = th.horizon_y(height);
    let [rx, ry, strength] = th.tw.bloom;
    let colours = th.tw.bloom_colours.map(|c| c.rgb());
    c.rectangle(0.0, hz, width as f64, height as f64 - hz);
    c.clip();
    c.translate(sun_xy(th).0 * width as f64, hz);
    c.scale(rx * unit(width, height), ry * unit(width, height));
    let g = RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
    for k in 0..13 {
        let r = k as f64 / 12.0;
        let (cr, cg, cb) = if r < 0.3 { colours[0] } else { colours[1] };
        g.add_color_stop_rgba(r, cr, cg, cb, strength * (1.0 - r).powf(1.8));
    }
    let _ = c.set_source(&g);
    let _ = c.paint();
    let _ = c.restore();
}

/// The horizon line, brightest under the sun.
fn horizon(th: &Theme, c: &Context, width: i32, height: i32) {
    let hz = th.horizon_y(height);
    let (r, gg, b) = th.tw.horizon.rgb();
    let g = LinearGradient::new(0.0, 0.0, width as f64, 0.0);
    g.add_color_stop_rgba(0.0, r, gg, b, 0.15);
    g.add_color_stop_rgba(sun_xy(th).0, r, gg, b, 0.9);
    g.add_color_stop_rgba(1.0, r, gg, b, 0.5);
    let _ = c.set_source(&g);
    c.rectangle(0.0, hz.round() - 1.0, width as f64, 1.5f64.max(unit(width, height) / 500.0));
    let _ = c.fill();
}

/// HUD corner brackets.
fn brackets(th: &Theme, c: &Context, width: i32, height: i32, edges: Edges) {
    let Brackets { left, right, top, bottom, arm: length } = bracket_corners(width, height, edges);
    let (r, g, b) = th.tw.bracket.rgb();
    c.set_source_rgba(r, g, b, 0.85);
    c.set_line_width(1.5f64.max(unit(width, height) / 500.0));
    for (x, y, dx, dy) in [(left, top, 1.0, 1.0), (right, top, -1.0, 1.0), (left, bottom, 1.0, -1.0), (right, bottom, -1.0, -1.0)] {
        c.move_to(x + dx * length, y);
        c.line_to(x, y);
        c.line_to(x, y + dy * length);
    }
    let _ = c.stroke();
}

// --- the elements ----------------------------------------------------------------------------------

/// What the sun leaves the shimmer: the picture under it, and its halos (each with its alpha).
pub struct SunLeft {
    pub under: ImageSurface,
    pub halos: Vec<(ImageSurface, f64)>,
}

const SUN: Key<SunLeft> = Key::new("sun");

/// The sun, sliced, setting on the horizon, in its own glow. The shimmer moves its cuts.
pub struct Sun {
    backdrop: Option<ImageSurface>,
}

impl Part for Sun {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn backdrop(&self) -> Backdrop {
        Backdrop::Picture
    }
    fn set_backdrop(&mut self, picture: &ImageSurface) {
        self.backdrop = Some(picture.clone());
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let th = &g.th;
        let small = sun_drawn(th, g.w, g.h, g.scale, true);
        let mut halos = vec![];
        for &(blur, alpha) in &th.tw.sun_glow {
            halos.push((resampled(&resampled(&small, 1.0 / blur, Filter::Good), blur, Filter::Bilinear), alpha));
        }
        for (halo, alpha) in &halos {
            let _ = c.set_source_surface(halo, 0.0, 0.0);
            let _ = c.paint_with_alpha(*alpha);
        }
        let _ = c.set_source_surface(&small, 0.0, 0.0);
        let _ = c.paint_with_alpha(th.tw.sun_alpha);
        // the scene gives the sun its backdrop before painting it
        let under = self.backdrop.take().unwrap_or_else(|| surface_rgb(g.w, g.h));
        s.put(&SUN, SunLeft { under, halos });
    }
}

/// The floor's lines converging under the sun: supersampled.
pub struct Rays;

impl Part for Rays {
    fn still(&self) -> bool {
        true
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (w, h) = (g.w, g.h);
        let th = &g.th;
        let bbox = (0.0, th.horizon_y(h).floor() - 1.0, w as f64, h as f64);
        let pic = supersampled(w, h, g.scale, |c| rays(th, c, w, h), Some(bbox));
        let _ = c.set_source_surface(&pic, 0.0, 0.0);
        let _ = c.paint();
    }
}

/// Heat rising through the sun's cuts (live): the sun drawn whole, then the sky through each cut where
/// the heat has moved it, its edges by coverage.
#[derive(Default)]
pub struct Shimmer {
    bbox: Option<(i32, i32, i32, i32)>,
    slices: Vec<(f64, f64)>,
    heat: Heat,
    hz: f64,
    u: f64,
    sky: Option<Tex>,
    solid: Option<Tex>,
    cuts: Vec<(f64, f64)>,
    rects: Vec<(f64, f64, f64, f64, f64)>,
}

impl Part for Shimmer {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        slices(&g.th, g.w, g.h).first().map_or(g.h, |s| s.0 as i32 - 3)
    }
    fn textures(&mut self, g: &Geo, s: &Shared, up: &mut Uploads) {
        let th = &g.th;
        self.slices = slices(th, g.w, g.h);
        self.heat = Heat::of(th);
        (self.hz, self.u) = (th.horizon_y(g.h), unit(g.w, g.h));
        let Some(left) = s.get(&SUN) else {
            self.bbox = None;
            return;
        };
        let sky = surface_rgb(g.w, g.h);
        {
            let c = ctx(&sky);
            let _ = c.set_source_surface(&left.under, 0.0, 0.0);
            let _ = c.paint();
            for (halo, alpha) in &left.halos {
                let _ = c.set_source_surface(halo, 0.0, 0.0);
                let _ = c.paint_with_alpha(*alpha);
            }
        }
        let solid = surface_rgb(g.w, g.h);
        {
            let c = ctx(&solid);
            let _ = c.set_source_surface(&sky, 0.0, 0.0);
            let _ = c.paint();
            let _ = c.set_source_surface(sun_drawn(th, g.w, g.h, g.scale, false), 0.0, 0.0);
            let _ = c.paint_with_alpha(th.tw.sun_alpha);
        }
        let (x0, x1) = shimmer_x(th, g.w, g.h);
        let bbox = (x0 as i32, self.top(g), x1 as i32, th.horizon_y(g.h) as i32);
        self.bbox = Some(bbox);
        // only the sun's box is drawn of either
        let region = (bbox.0.max(0), bbox.1.max(0), (bbox.2 + 1).min(g.w), (bbox.3 + 1).min(g.h));
        (self.sky, self.solid) = (Some(up.add_layer(sky, region)), Some(up.add_layer(solid, region)));
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        cuts_at(&self.slices, self.hz, self.u, self.heat, w.clock, &mut self.cuts);
        let mut h = 0;
        for &(y, t) in &self.cuts {
            hash_f(&mut h, y);
            hash_f(&mut h, t);
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (Some((x0, top, x1, hz)), Some(sky), Some(solid)) = (self.bbox, self.sky, self.solid) else { return };
        let (x0, top, x1, hz) = (x0 as f64, top as f64, x1 as f64, hz as f64);
        gfx.layer(solid, Some(&[(x0, top, x1, hz)]), 1.0, Blend::Opaque);
        self.rects.clear();
        for &(y, thick) in &self.cuts {
            each_span(y, y + thick, |ya, yb, cover| self.rects.push((x0, ya, x1, yb, cover)));
        }
        gfx.rects(Some(sky), &self.rects, (1.0, 1.0, 1.0), Blend::Alpha);
    }
}

/// The floor's cross lines (live), rolling slowly toward the viewer in a seamless loop.
#[derive(Default)]
pub struct Floor {
    steps: i64,
    fps: f64,
    half: f64,
    step: i64,
    colour: (f64, f64, f64),
    /// Each step's lines, made the first time it shows.
    lines: Vec<Option<Vec<(f64, f64, f64, f64, f64)>>>,
}

impl Floor {
    fn lines(&self, g: &Geo, step: i64) -> Vec<(f64, f64, f64, f64, f64)> {
        let th = &g.th;
        let mut quads = vec![];
        for (y, alpha) in cross_lines(th, g.w, g.h, step as f64 / self.steps as f64) {
            if alpha > 0.0 {
                let (x0, x1) = floor_span(th, g.w, g.h, y);
                quads.extend(box_(x0, y - self.half, x1, y + self.half, alpha));
            }
        }
        quads
    }
}

impl Part for Floor {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        g.th.horizon_y(g.h) as i32
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, _up: &mut Uploads) {
        self.fps = g.th.tw.fps;
        self.steps = ((self.fps / SPEED).round() as i64).max(1);
        self.half = 1f64.max(unit(g.w, g.h) / 800.0) / 2.0;
        self.colour = grid(&g.th);
        self.lines = vec![None; self.steps as usize];
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.step = ((w.clock * self.fps).round() as i64).rem_euclid(self.steps);
        self.step as u64
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let step = self.step as usize;
        if self.lines.get(step).is_some_and(|l| l.is_none()) {
            self.lines[step] = Some(self.lines(g, self.step));
        }
        if let Some(Some(lines)) = self.lines.get(step) {
            gfx.rects(None, lines, self.colour, Blend::Alpha);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        cross(&g.th, c, g.w, g.h, 0.0);
    }
}

/// The sun's light on the ground.
pub struct Bloom;

impl Part for Bloom {
    fn still(&self) -> bool {
        true
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        bloom(&g.th, c, g.w, g.h);
    }
}

/// The line the sun sets on.
pub struct Horizon;

impl Part for Horizon {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        horizon(&g.th, c, g.w, g.h);
    }
}

/// The six stripes, climbing and turning off the right edge, with their glow.
pub struct Stripes;

impl Part for Stripes {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let th = &g.th;
        let (width, height) = (g.w as f64, g.h);
        let band = stripes_drawn(th, g.w, g.h, g.scale);
        let sun = sun_xy(th);
        let tx = sun.0 * width + (sun.1 + th.tw.turn_gap) * unit(g.w, height);
        let fade_at = (th.tw.fade[0], th.tw.fade[1]);
        let fade = LinearGradient::new(tx + fade_at.0 * (width - tx), 0.0, width, 0.0);
        fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 1.0);
        fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, fade_at.1);
        {
            let bc = ctx(&band);
            bc.set_operator(Operator::DestIn);
            let _ = bc.set_source(&fade);
            let _ = bc.paint();
        }
        glowing(c, &band, &th.tw.glow);
    }
}

/// The HUD's corner brackets.
pub struct Corners;

impl Part for Corners {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Front)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let (u, w, h) = (unit(g.w, g.h), g.w as f64, g.h as f64);
        let margin = 1.5f64.max(u / 500.0).ceil() + 2.0;
        let (inset, reach) = ((0.03 * u).floor() - margin, (0.075 * u).ceil() + margin);
        let Edges { top: t, right: r, bottom: b, left: l } = g.edges;
        let mut out = vec![];
        for x in [l as f64 + inset, w - r as f64 - reach] {
            for y in [t as f64 + inset, h - b as f64 - reach] {
                out.push((x, y, x + reach - inset, y + reach - inset));
            }
        }
        Some(out)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        brackets(&g.th, c, g.w, g.h, g.edges);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::{Kind, Validate, check};
    /// Phosphor's elements have no settings of their own: their shapes are tweaks.
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NoSettings {}
    impl Validate for NoSettings {}
    let none = check::<NoSettings>;
    vec![
        Kind { name: "sun", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Sun { backdrop: None })]) },
        Kind { name: "rays", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Rays)]) },
        Kind { name: "shimmer", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Shimmer::default())]) },
        Kind { name: "floor", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Floor::default())]) },
        Kind { name: "bloom", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Bloom)]) },
        Kind { name: "horizon", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Horizon)]) },
        Kind { name: "stripes", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Stripes)]) },
        Kind { name: "brackets", defaults: "", varies: &[], check: none, make: |_, _, _g| Ok(vec![Box::new(Corners)]) },
    ]
}
