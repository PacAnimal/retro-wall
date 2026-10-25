//! A line of text scrolling right to left at `speed` pixels a second. The text is drawn into a strip
//! when it changes and shifted by whole pixels each frame. Shows nothing when stale or empty.

use super::draw::*;
use super::{Base, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Patch};
use crate::scene::World;
use crate::util::*;
use cairo::ImageSurface;
use std::sync::Arc;

super::settings!(TickerSettings { source: String = String::new(), label: String = String::new(), speed: f64 = 40.0 });

impl Validate for TickerSettings {
    fn validate(&self) -> Result<(), String> {
        within("speed", self.speed, -10_000.0, 10_000.0)
    }
}

pub const TYPE: WidgetType = WidgetType { name: "ticker", check: check::<TickerSettings>, make: |name, t| Box::new(Ticker::new(name, settings(t))) };

/// The strip of the text now shown: the text, and the strip's height and width.
struct Strip {
    text: Arc<str>,
    h: i32,
    w: i32,
}

pub struct Ticker {
    b: Base,
    source: String,
    label: String,
    speed: f64,
    text: Option<Arc<str>>,
    offset: i64,
    made: Option<Strip>,
    pieces: Vec<Patch>,
}

impl Ticker {
    fn new(name: &str, t: TickerSettings) -> Self {
        Ticker { b: Base::new(name), source: t.source, label: t.label, speed: t.speed, text: None, offset: 0, made: None, pieces: vec![] }
    }
    /// The text's window in the card: inset by the label margin, above the label.
    fn window(a: Area) -> (f64, f64, f64, f64) {
        let inset = (12.0 * a.scale).round();
        (a.x + inset, a.y + inset, a.w - 2.0 * inset, a.h - 2.0 * inset - (7f64.max(13.0 * a.scale)).round())
    }
    fn strip(th: &Theme, a: Area, text: &str) -> ImageSurface {
        let size = 9f64.max((Self::window(a).3 * 0.32).round());
        let p = probe();
        mono(&p, false);
        p.set_font_size(size);
        let line = format!("{text}     \u{25c6}     ");
        // no wider than a texture can be
        let width = Self::window(a).2.max(text_extents(&p, &line).map_or(0.0, |e| e.x_advance().ceil())).min(16384.0);
        let s = surface(width as i32, (1.6 * size).round() as i32);
        {
            let c = ctx(&s);
            mono(&c, false);
            c.set_font_size(size);
            rgba(&c, th.tw.needle_colour.rgb(), 0.95);
            c.move_to(0.0, (1.15 * size).round());
            let _ = show_text(&c, &line);
        }
        s.flush();
        s
    }
}

impl Widget for Ticker {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn size(&self) -> (i32, i32) {
        (2, 1)
    }
    fn label(&self) -> Option<&str> {
        Some(self.label.as_str()).filter(|l| !l.is_empty())
    }
    fn has_value(&self) -> bool {
        self.text.is_some()
    }
    fn state(&mut self, w: &World) -> u64 {
        self.b.stale = w.stale(&self.source);
        self.text = w.text(&self.source).filter(|t| !t.is_empty() && !self.b.stale);
        let width = self.made.as_ref().map_or(1, |m| m.w.max(1)) as i64;
        self.offset = ((w.clock * self.speed).round() as i64).rem_euclid(width);
        self.b.state = match &self.text {
            Some(t) => hash_str(t) ^ (self.offset as u64).wrapping_mul(0x9e3779b97f4a7c15),
            None => self.b.stale as u64,
        };
        self.b.state
    }
    fn over(&mut self, th: &Theme, gfx: &mut Gfx, p: f64) {
        let Some(a) = self.b.area else { return };
        let Some(text) = self.text.clone() else {
            if self.b.error.is_some() {
                if self.b.baked != Some(self.b.state) {
                    let s = surface(a.w as i32, a.h as i32);
                    card_error(&ctx(&s), th, a.scale);
                    s.flush();
                    self.b.put(gfx, &drop_shadow(s, &th.tw.card_shadow, a.scale, None));
                    self.b.baked = Some(self.b.state);
                    self.made = None; // the strip is gone from the texture
                }
                if let Some(t) = self.b.tex {
                    gfx.image(t, a.x, a.y, a.w, a.h, p, Blend::Premultiplied);
                }
            }
            return;
        };
        let (x, y, w, h) = Self::window(a);
        if self.made.as_ref().is_none_or(|m| m.text != text) {
            let s = Self::strip(th, a, &text);
            self.b.put(gfx, &s);
            self.made = Some(Strip { text, h: s.height(), w: s.width() });
        }
        let (Some(made), Some(tex)) = (self.made.as_ref(), self.b.tex) else { return };
        let (sh, sw) = (made.h as f64, made.w as f64);
        let top = y + ((h - sh) / 2.0).round();
        self.pieces.clear();
        let mut at = 0.0;
        while at < w {
            let sx = (self.offset as f64 + at).rem_euclid(sw);
            let n = (sw - sx).min(w - at);
            self.pieces.push(Patch { sx0: sx, sy0: 0.0, sx1: sx + n, sy1: sh, x0: x + at, y0: top, x1: x + at + n, y1: top + sh });
            at += n;
        }
        gfx.patches(tex, &self.pieces, p);
    }
}
