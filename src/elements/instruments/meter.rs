//! A VU meter for a reading from 0 to 100: a needle over an arc scale. For the battery, a bolt in the
//! lower right while on mains.

use super::draw::*;
use super::{Base, SpriteTex, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, settings};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::{Geo, Shared, World, hash_f};
use crate::util::*;
use cairo::{Context, ImageSurface, LineCap};
use std::f64::consts::PI;

super::settings!(MeterSettings { source: String = "cpu".into(), label: String = "CPU".into() });
impl Validate for MeterSettings {}

pub const TYPE: WidgetType = WidgetType { name: "meter", check: check::<MeterSettings>, make: |name, t| Box::new(Meter::new(name, settings(t))) };

/// The needle's angle, degrees clockwise from upright, for a value from 0 to 100.
fn needle_angle(sweep: f64, value: f64) -> f64 {
    -sweep + 2.0 * sweep * 100f64.min(0f64.max(value)) / 100.0
}

fn pivot_of(th: &Theme, a: Area) -> (f64, f64) {
    let p = th.tw.pivot;
    (a.x + p[0] * a.scale, a.y + p[1] * a.scale)
}

fn polar(sweep: f64, (px, py): (f64, f64), value: f64, r: f64) -> (f64, f64) {
    let a = needle_angle(sweep, value).to_radians();
    (px + r * a.sin(), py - r * a.cos())
}

/// A VU meter's face: the scale's arc, heavy from OVERLOAD to 100, ticked every 10, half-ticked between.
fn meter_face(th: &Theme, c: &Context, area: Area) {
    let tw = &th.tw;
    let scale = area.scale;
    let (px, py) = pivot_of(th, area);
    let r = tw.scale_r * scale;
    let (overload, alpha, sweep) = (tw.overload, tw.deck_alpha, tw.sweep);
    let (deck, needle) = (tw.deck_colour.rgb(), tw.needle_colour.rgb());
    let arc = |v0: f64, v1: f64, r: f64| {
        c.new_path();
        c.arc(px, py, r, (needle_angle(sweep, v0) - 90.0).to_radians(), (needle_angle(sweep, v1) - 90.0).to_radians());
    };
    rgba(c, deck, alpha);
    c.set_line_width(1f64.max(2.5 * scale));
    arc(0.0, overload, r);
    let _ = c.stroke();
    rgba(c, needle, alpha);
    c.set_line_width(1.5f64.max(6.0 * scale));
    arc(overload, 100.0, r + 2.0 * scale);
    let _ = c.stroke();
    for v in (0..=100).step_by(5) {
        let major = v % 10 == 0;
        let long = if [0, 50, 100].contains(&v) {
            14.0
        } else if major {
            9.0
        } else {
            5.0
        };
        rgba(c, if v as f64 > overload { needle } else { deck }, alpha);
        c.set_line_width(1f64.max((if major { 2.0 } else { 1.2 }) * scale));
        let (x0, y0) = polar(sweep, (px, py), v as f64, r);
        c.move_to(x0, y0);
        let (x1, y1) = polar(sweep, (px, py), v as f64, r + long * scale);
        c.line_to(x1, y1);
        let _ = c.stroke();
    }
}

/// A VU meter's numbers: 0, 50 and 100.
fn meter_text(th: &Theme, c: &Context, area: Area) {
    let tw = &th.tw;
    let Area { x, w, scale, .. } = area;
    let (px, py) = pivot_of(th, area);
    let r = tw.scale_r * scale;
    mono(c, false);
    c.set_font_size((6f64.max(11.0 * scale)).round());
    for v in [0, 50, 100] {
        let text = v.to_string();
        let Ok(ext) = text_extents(c, &text) else { continue };
        let (cx, cy) = if v == 50 {
            polar(tw.sweep, (px, py), v as f64, r + 22.0 * scale)
        } else {
            let (tx, ty) = polar(tw.sweep, (px, py), v as f64, r + 14.0 * scale);
            let margin = 6.0 * scale;
            ((tx.max(x + margin + ext.x_advance() / 2.0)).min(x + w - margin - ext.x_advance() / 2.0), ty - 3.0 * scale - ext.height() / 2.0)
        };
        rgba(c, if v as f64 > tw.overload { tw.needle_colour.rgb() } else { tw.deck_colour.rgb() }, tw.deck_alpha);
        c.move_to((cx - ext.x_advance() / 2.0).round(), (cy + ext.height() / 2.0).round());
        let _ = show_text(c, &text);
    }
}

/// Draw a needle from its pivot, `length` long, turned `angle` degrees clockwise from upright.
fn needle(c: &Context, px: f64, py: f64, length: f64, angle: f64, width: f64) {
    let a = angle.to_radians();
    c.set_line_width(width);
    c.set_line_cap(LineCap::Round);
    c.move_to(px, py);
    c.line_to(px + length * a.sin(), py - length * a.cos());
    let _ = c.stroke();
}

/// A needle `length` pixels long, upright from the centre, white on transparency, with a clear margin.
fn needle_sprite(th: &Theme, length: f64, width: f64) -> ImageSurface {
    let full = (2.0 * length * th.tw.reel_pad).ceil() as i32;
    let s = surface(full, full);
    {
        let c = ctx(&s);
        c.set_source_rgb(1.0, 1.0, 1.0);
        needle(&c, full as f64 / 2.0, full as f64 / 2.0, length, 0.0, width);
    }
    s
}

/// A lightning bolt, white on transparency, `size` pixels tall.
fn bolt_surface(size: i32) -> ImageSurface {
    let w = (size as f64 * 0.6).ceil() as i32;
    let s = surface(w, size);
    {
        let c = ctx(&s);
        c.scale(w as f64 / 6.0, size as f64 / 10.0);
        for (k, (px, py)) in [(3.6, 0.0), (0.6, 5.6), (2.8, 5.6), (1.8, 10.0), (5.4, 3.8), (3.2, 3.8), (4.4, 0.0)].into_iter().enumerate() {
            if k > 0 { c.line_to(px, py) } else { c.move_to(px, py) }
        }
        c.close_path();
        c.set_source_rgb(1.0, 1.0, 1.0);
        let _ = c.fill();
    }
    s
}

fn bolt_size(th: &Theme, scale: f64) -> i32 {
    9.max((th.tw.bolt * scale).round() as i32)
}

/// A needle: exponential approach to its reading with time constant TAU, shown in half degrees at up
/// to NEEDLE_FPS.
struct Needle {
    sweep: f64,
    fps: f64,
    angle: f64,
    shown: f64,
    last: Option<f64>,
    shown_at: f64,
    value: f64,
}

impl Needle {
    const TAU: f64 = 0.3;
    const STEP: f64 = 0.5;
    const DEADBAND: f64 = 1.0;
    fn new(th: &Theme) -> Self {
        let sweep = th.tw.sweep;
        let a = needle_angle(sweep, 0.0);
        Needle { sweep, fps: th.tw.needle_fps, angle: a, shown: a, last: None, shown_at: -1.0, value: 0.0 }
    }
    fn at(&mut self, t: f64, value: f64) -> f64 {
        let dt = self.last.map_or(0.0, |l| (t - l).min(0.1));
        self.last = Some(t);
        if (value - self.value).abs() >= Self::DEADBAND {
            self.value = value;
        }
        self.angle += (needle_angle(self.sweep, self.value) - self.angle) * (1.0 - (-dt / Self::TAU).exp());
        if t - self.shown_at >= 1.0 / self.fps - 0.004 {
            self.shown = (self.angle / Self::STEP).round() * Self::STEP;
            self.shown_at = t;
        }
        self.shown
    }
}

pub struct Meter {
    b: Base,
    source: String,
    label: String,
    needle: Option<Needle>,
    /// The battery's bolt: its top left and picture.
    bolt: Option<((f64, f64), Tex)>,
    angle: f64,
    lit: bool,
}

impl Meter {
    fn new(name: &str, t: MeterSettings) -> Self {
        Meter { b: Base::new(name), source: t.source, label: t.label, needle: None, bolt: None, angle: 0.0, lit: false }
    }
    fn length(th: &Theme, a: Area) -> f64 {
        (th.tw.scale_r + 4.0) * a.scale
    }
}

impl Widget for Meter {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn stack(&self) -> i32 {
        2
    }
    fn label(&self) -> Option<&str> {
        Some(self.label.as_str()).filter(|l| !l.is_empty())
    }
    fn lines(&self, th: &Theme, c: &Context) {
        if let Some(a) = self.b.area {
            meter_face(th, c, a);
        }
    }
    fn text(&self, th: &Theme, c: &Context) {
        if let Some(a) = self.b.area {
            meter_text(th, c, a);
        }
    }
    /// The needle's cap: a half-round under the pivot, and the pivot screw.
    fn cover(&self, th: &Theme, c: &Context) {
        let Some(area) = self.b.area else { return };
        let tw = &th.tw;
        let scale = area.scale;
        let (px, py) = pivot_of(th, area);
        let r = tw.cap_r * scale;
        let cy = py + tw.cap_drop * scale;
        c.new_path();
        c.arc(px, cy, r, PI, 2.0 * PI);
        c.close_path();
        let (sr, sg, sb) = tw.spool_fill.0.rgb();
        c.set_source_rgb(sr, sg, sb);
        let _ = c.fill_preserve();
        rgba(c, tw.deck_colour.rgb(), tw.deck_alpha);
        c.set_line_width(1f64.max(2.0 * scale));
        let _ = c.stroke();
        c.arc(px, py, 1.5f64.max(4.0 * scale), 0.0, 2.0 * PI);
        c.set_line_width(1f64.max(1.5 * scale));
        let _ = c.stroke();
    }
    fn rest(&self, th: &Theme, c: &Context) {
        let Some(a) = self.b.area else { return };
        rgba(c, th.tw.needle_colour.rgb(), 0.9);
        let (px, py) = pivot_of(th, a);
        needle(c, px, py, Self::length(th, a), needle_angle(th.tw.sweep, 0.0), 1f64.max(1.6 * a.scale));
    }
    fn textures(&mut self, th: &Theme, up: &mut Uploads, sprites: &mut SpriteTex) {
        let Some(a) = self.b.area else { return };
        let needle = &mut sprites[SpriteKind::Needle as usize];
        if needle.is_none() {
            *needle = Some(up.add(needle_sprite(th, 4.0 * Self::length(th, a), 4.0 * 1f64.max(1.6 * a.scale)), Kind::Sprite));
        }
        if self.source.split(':').next() == Some("bat") {
            // the one source with an indicator
            let s = bolt_surface(bolt_size(th, a.scale));
            let at = (a.x + a.w - (9.0 * a.scale).round() - s.width() as f64, a.y + a.h - (9.0 * a.scale).round() - bolt_size(th, a.scale) as f64);
            self.bolt = Some((at, up.add(s, Kind::Label)));
        }
    }
    fn start(&mut self, g: &Geo, _w: &World, _s: &Shared) {
        self.needle = Some(Needle::new(&g.th));
    }
    fn state(&mut self, w: &World) -> u64 {
        let (value, indicator) = w.reading(&self.source);
        if let Some(n) = self.needle.as_mut() {
            self.angle = if w.frozen.is_some() { (needle_angle(n.sweep, value) / Needle::STEP).round() * Needle::STEP } else { n.at(w.clock_real, value) };
        }
        self.lit = indicator;
        let mut h = self.lit as u64;
        hash_f(&mut h, self.angle);
        self.b.state = h;
        h
    }
    fn sprites(&self, th: &Theme, p: f64, out: &mut Sprites) {
        let Some(a) = self.b.area else { return };
        let length = 2.0 * Self::length(th, a) * th.tw.reel_pad;
        let (px, py) = pivot_of(th, a);
        out[SpriteKind::Needle as usize].push(Sprite { cx: px, cy: py, size: length, angle: self.angle, rgb: th.tw.needle_colour.rgb(), alpha: 0.9 * p });
    }
    fn over(&mut self, th: &Theme, gfx: &mut Gfx, p: f64) {
        let Some(((x, y), tex)) = self.bolt.filter(|_| self.lit) else { return };
        let (w, h) = gfx.size(tex);
        let (r, g, b) = th.tw.needle_colour.rgb();
        let a = th.tw.deck_alpha * p;
        let mut d = [0f32; 32];
        for (k, (u, v)) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].into_iter().enumerate() {
            d[k * 8..k * 8 + 8].copy_from_slice(&[
                u as f32,
                v as f32,
                (r * a) as f32,
                (g * a) as f32,
                (b * a) as f32,
                a as f32,
                (x + u * w as f64) as f32,
                (y + v * h as f64) as f32,
            ]);
        }
        gfx.draw(Some(tex), &d, Blend::Premultiplied);
    }
}
