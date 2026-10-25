//! What the widgets share: card sizes, the card lettering, sprites, easing, and the reading widgets'
//! common part.

use crate::config::Theme;
use crate::scene::World;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::Context;

use super::status::status_font;

pub fn card_scale(th: &Theme, w: i32, h: i32) -> f64 {
    th.tw.deck * unit(w, h) / 300.0
}

/// A card's width, height, and the gap between cards.
pub fn card_size(th: &Theme, w: i32, h: i32) -> (i32, i32, i32) {
    let s = card_scale(th, w, h);
    ((300.0 * s).round() as i32, (th.tw.card_h * s).round() as i32, (th.tw.instrument_gap * s).round() as i32)
}

pub fn label_size(w: i32, h: i32) -> i32 {
    (6f64.max(0.011 * unit(w, h))).round() as i32
}

pub fn mono(c: &Context, bold: bool) {
    select_mono(c, bold);
}

pub fn rgba(c: &Context, (r, g, b): (f64, f64, f64), a: f64) {
    c.set_source_rgba(r, g, b, a);
}

/// A theme (colour, alpha) pair, such as deck_fill, as (rgb, alpha).
pub fn fill_of((c, a): (Colour, f64)) -> ((f64, f64, f64), f64) {
    (c.rgb(), a)
}

/// A character's advance in the current font, rounded; 0 if it cannot be measured.
pub fn advance(c: &Context, ch: char) -> f64 {
    let mut b = [0u8; 4];
    text_extents(c, ch.encode_utf8(&mut b)).map_or(0.0, |x| x.x_advance().round())
}

/// Text one character at a time from (x, base), each advance rounded and `track` apart. The pen after.
pub fn tracked(c: &Context, text: &str, mut x: f64, base: f64, track: f64) -> f64 {
    let mut b = [0u8; 4];
    for ch in text.chars() {
        let s = ch.encode_utf8(&mut b);
        c.move_to(x, base);
        let _ = show_text(c, s);
        x += advance(c, ch) + track;
    }
    x
}

/// A widget's name in its card's lower left corner, tracked like the caption.
pub fn card_label(c: &Context, th: &Theme, name: &str, a: Area) {
    mono(c, false);
    c.set_font_size((7f64.max(13.0 * a.scale)).round());
    let track = (1f64.max(3.0 * a.scale)).round();
    rgba(c, th.tw.deck_colour.rgb(), th.tw.deck_alpha);
    tracked(c, name, a.x + (9.0 * a.scale).round(), a.y + a.h - (9.0 * a.scale).round(), track);
}

/// Text in the card lettering, tracked like the labels, baseline at `base`. The pen after.
pub fn card_text(c: &Context, th: &Theme, text: &str, x: f64, base: f64, scale: f64, bright: bool) -> f64 {
    mono(c, false);
    c.set_font_size((7f64.max(13.0 * scale)).round());
    let track = (1f64.max(3.0 * scale)).round();
    rgba(c, if bright { th.tw.needle_colour.rgb() } else { th.tw.deck_colour.rgb() }, th.tw.deck_alpha);
    tracked(c, text, x, base, track)
}

/// ERR in a card's top left: a widget that failed.
pub fn card_error(c: &Context, th: &Theme, scale: f64) {
    card_text(c, th, "ERR", (9.0 * scale).round(), (9.0 * scale).round() + (7f64.max(13.0 * scale)).round(), scale, true);
}

/// Where the layout puts a widget, in pixels, and the card scale there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

// --- sprites ---------------------------------------------------------------------------------------

/// A white sprite placed: centred on (cx, cy), `size` across, turned `angle` degrees, tinted.
#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub cx: f64,
    pub cy: f64,
    pub size: f64,
    pub angle: f64,
    pub rgb: (f64, f64, f64),
    pub alpha: f64,
}

/// The sprite pictures the widgets share, drawn in this order: spools, reels, needles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteKind {
    Spool,
    Reel,
    Needle,
}

/// Placed sprites by kind.
pub type Sprites = [Vec<Sprite>; 3];

/// Sprites as vertices, appended to `d`: each a white image `size` across, centred, rotated, tinted.
pub fn sprite_vertices(placed: &[Sprite], d: &mut Vec<f32>) {
    for &Sprite { cx, cy, size, angle, rgb: (r, g, b), alpha } in placed {
        let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
        let h = size / 2.0;
        let (r, g, b) = (r * alpha, g * alpha, b * alpha);
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let (px, py) = ((2.0 * u - 1.0) * h, (2.0 * v - 1.0) * h);
            d.extend_from_slice(&[
                u as f32,
                v as f32,
                r as f32,
                g as f32,
                b as f32,
                alpha as f32,
                (cx + px * c - py * s) as f32,
                (cy + px * s + py * c) as f32,
            ]);
        }
    }
}

// --- state -----------------------------------------------------------------------------------------

pub fn hash_bytes(h: u64, s: &[u8]) -> u64 {
    s.iter().fold(h, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

pub fn hash_str(s: &str) -> u64 {
    hash_bytes(0xcbf29ce484222325, s.as_bytes())
}

/// Eases a reading: exponential approach with time constant `tau`, changes under `deadband` ignored,
/// shown in whole `step`s.
pub struct Ease {
    eased: Option<f64>,
    target: f64,
    last: Option<f64>,
    tau: f64,
    deadband: f64,
    step: f64,
}

impl Ease {
    pub fn new(tau: f64, deadband: f64, step: f64) -> Self {
        Ease { eased: None, target: 0.0, last: None, tau, deadband, step }
    }
    pub fn at(&mut self, t: f64, value: f64, frozen: bool) -> f64 {
        let eased = match self.eased {
            Some(e) if !frozen => e,
            _ => {
                self.target = value;
                value
            }
        };
        if (value - self.target).abs() >= self.deadband {
            self.target = value;
        }
        let dt = self.last.map_or(0.0, |l| (t - l).min(0.2));
        self.last = Some(t);
        let e = eased + (self.target - eased) * (1.0 - (-dt / self.tau).exp());
        self.eased = Some(e);
        (e / self.step).round() * self.step
    }
}

/// The settings every reading widget has: its source and label, and how its reading eases.
pub struct ReadingSettings {
    pub source: String,
    pub label: String,
    pub tau: f64,
    pub deadband: f64,
    pub step: f64,
}

/// The easing every reading widget shares, checked.
pub fn check_ease(tau: f64, deadband: f64, step: f64) -> Result<(), String> {
    crate::elements::positive("tau", tau)?;
    crate::elements::within("tau", tau, 0.0, 3600.0)?;
    crate::elements::within("deadband", deadband, 0.0, 100.0)?;
    crate::elements::positive("step", step)?;
    crate::elements::within("step", step, 0.0, 100.0)
}

/// What every reading widget shares: source, label, easing.
pub struct Reading {
    pub source: String,
    pub label: String,
    ease: Ease,
    pub value: f64,
    pub lit: bool,
}

impl Reading {
    pub fn new(s: ReadingSettings) -> Self {
        Reading { source: s.source, label: s.label, ease: Ease::new(s.tau, s.deadband, s.step), value: 0.0, lit: false }
    }
    /// (eased value, indicator), at the real clock.
    pub fn read(&mut self, w: &World) -> (f64, bool) {
        let (value, lit) = w.reading(&self.source);
        self.value = self.ease.at(w.clock_real, value, w.frozen.is_some());
        self.lit = lit;
        (self.value, lit)
    }
    pub fn label(&self) -> Option<&str> {
        Some(self.label.as_str()).filter(|l| !l.is_empty())
    }
    /// Colours: (lit, unlit track, text).
    pub fn colours(th: &Theme) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64)) {
        (th.tw.needle_colour.rgb(), th.tw.deck_colour.rgb(), th.tw.needle_colour.rgb())
    }
    /// Room for the name under it: (capital height, gap above); zero with a frame.
    pub fn room(&self, th: &Theme, scale: f64) -> (f64, f64) {
        if th.tw.card_frame || self.label.is_empty() {
            return (0.0, 0.0);
        }
        let cap = status_font(th, &probe(), scale * th.tw.status_zoom).1;
        (cap, (0.8 * cap).round())
    }
    /// Without a frame, draw the name centred under the graphic, whose foot is at `bottom`.
    pub fn name_under(&self, c: &Context, th: &Theme, w: i32, bottom: f64, scale: f64) {
        let (cap, gap) = self.room(th, scale);
        if cap == 0.0 {
            return;
        }
        let track = status_font(th, c, scale * th.tw.status_zoom).0;
        let widths: Vec<f64> = self.label.chars().map(|ch| advance(c, ch)).collect();
        let x = w as f64 / 2.0 - (widths.iter().sum::<f64>() + track * (widths.len() as f64 - 1.0)) / 2.0;
        rgba(c, th.tw.status_colours[0].0.unwrap_or(th.tw.deck_colour).rgb(), 0.95);
        tracked_rounded(c, &self.label, x, (bottom + gap + cap).round(), track);
        c.new_path();
    }
    /// The reading as a percentage, right-aligned to x.
    pub fn number(c: &Context, th: &Theme, value: f64, x: f64, base: f64, size: f64) {
        let text = format!("{}%", value.round() as i64);
        mono(c, true);
        c.set_font_size(size);
        rgba(c, Self::colours(th).2, 0.95);
        c.move_to(x - text_extents(c, &text).map_or(0.0, |e| e.x_advance()), base);
        let _ = show_text(c, &text);
    }
}

/// As `tracked`, each character placed at the pen rounded.
pub fn tracked_rounded(c: &Context, text: &str, mut x: f64, base: f64, track: f64) {
    let mut b = [0u8; 4];
    for ch in text.chars() {
        c.move_to(x.round(), base);
        let _ = show_text(c, ch.encode_utf8(&mut b));
        x += advance(c, ch) + track;
    }
}
