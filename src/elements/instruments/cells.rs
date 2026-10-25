//! A reading as an upright neon bar of `cells` cells lit from the bottom, topped by a thin cell that
//! lights in `alarm` from `high` up.

use super::draw::*;
use super::status::status_font;
use super::{Base, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, positive, settings, within};
use crate::scene::World;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Filter};

super::reading_settings!(
    CellsSettings {
        cells: i64 = 5,
        colour: Colour = Colour(0xa5 as f64 / 255.0, 0x26 as f64 / 255.0, 1.0),
        alarm: Colour = Colour(1.0, 0x1e as f64 / 255.0, 0x2d as f64 / 255.0),
        high: f64 = 95.0,
        cap: f64 = 0.1667,
        glow: f64 = 1.0,
        width: f64 = 0.17,
        bright: f64 = 0.45,
        space: f64 = 0.16,
    },
    "cpu",
    "CPU"
);

impl Validate for CellsSettings {
    fn validate(&self) -> Result<(), String> {
        check_ease(self.tau, self.deadband, self.step)?;
        within("cells", self.cells as f64, 1.0, 50.0)?;
        within("high", self.high, 0.0, 1000.0)?;
        within("cap", self.cap, 0.0, 1.0)?;
        within("glow", self.glow, 0.0, 10.0)?;
        positive("width", self.width)?;
        within("width", self.width, 0.0, 1.0)?;
        within("bright", self.bright, 0.0, 1.0)?;
        within("space", self.space, 0.0, 0.5)
    }
}

pub const TYPE: WidgetType = WidgetType { name: "cells", check: check::<CellsSettings>, make: |name, t| Box::new(Cells::new(name, settings(t))) };

pub struct Cells {
    b: Base,
    r: Reading,
    cells: i64,
    colour: Colour,
    alarm: Colour,
    high: f64,
    cap: f64,
    glow: f64,
    width: f64,
    bright: f64,
    space: f64,
    shown: (i64, bool),
}

impl Cells {
    fn new(name: &str, t: CellsSettings) -> Self {
        Cells {
            b: Base::new(name),
            r: Reading::new(t.reading()),
            cells: t.cells,
            colour: t.colour,
            alarm: t.alarm,
            high: t.high,
            cap: t.cap,
            glow: t.glow,
            width: t.width,
            bright: t.bright,
            space: t.space,
            shown: (-1, false),
        }
    }
    /// Glow reach past the bar, in pixels.
    fn reach(scale: f64) -> f64 {
        (16.0 * scale).round()
    }
    fn bar(&self, th: &Theme, w: f64, h: f64, scale: f64) -> (Vec<(f64, f64, f64, f64)>, f64) {
        let r = Self::reach(scale);
        let (w, h) = (w - 2.0 * r, h - 2.0 * r);
        let (cap, gap) = self.r.room(th, scale);
        let bw = (self.width * w).round();
        let top = r;
        let bottom = r + h - (cap + gap + (4.0 * scale).round()) - (self.space * h).round();
        let space = 1f64.max((4.0 * scale).round());
        let n = self.cells as f64;
        let cell = (bottom - top - space * n) / (n + self.cap);
        let mut boxes = vec![(r, top, bw, cell * self.cap)];
        boxes.extend((0..self.cells).map(|i| (r, bottom - (i + 1) as f64 * cell - i as f64 * space, bw, cell)));
        (boxes, bottom)
    }
}

impl Widget for Cells {
    super::reading_widget!();
    /// The layout's card, widened by the glow reach on every side.
    fn set_area(&mut self, a: Option<Area>) {
        let a = match a {
            Some(a) if Some(a) != self.b.area => {
                let r = Self::reach(a.scale);
                Some(Area { x: a.x - r, y: a.y - r, w: a.w + 2.0 * r, h: a.h + 2.0 * r, scale: a.scale })
            }
            other => other,
        };
        self.b.area = a;
    }
    fn state(&mut self, w: &World) -> u64 {
        let (value, _) = self.r.read(w);
        let share = 100.0 / self.cells as f64;
        let lit = (0..self.cells).filter(|&i| value > i as f64 * share).count() as i64;
        self.shown = (lit, value >= self.high);
        self.b.state = (lit as u64) << 1 | self.shown.1 as u64;
        self.b.state
    }
    fn draw(&mut self, th: &Theme, c: &Context, wi: i32, hi: i32, scale: f64) {
        let (w, h) = (wi as f64, hi as f64);
        let (lit, alarm) = self.shown;
        let (boxes, bottom) = self.bar(th, w, h, scale);
        let (purple, red) = (self.colour.rgb(), self.alarm.rgb());
        let mut looks = vec![(red, alarm)];
        looks.extend((0..self.cells).map(|i| (purple, i < lit)));
        let glow = surface(wi, hi);
        {
            let g = ctx(&glow);
            for (&(x, y, bw, ch), &(colour, on)) in boxes.iter().zip(looks.iter()) {
                if on {
                    g.rectangle(x, y, bw, ch);
                    g.set_source_rgba(colour.0, colour.1, colour.2, 1.0);
                    let _ = g.fill();
                }
            }
        }
        for (blur, alpha) in
            [(2f64.max((3.0 * scale).round()), self.glow), (3f64.max((8.0 * scale).round()), self.glow), (4f64.max((15.0 * scale).round()), 0.7 * self.glow)]
        {
            let halo = resampled(&resampled(&glow, 1.0 / blur, Filter::Good), blur, Filter::Bilinear);
            if c.set_source_surface(&halo, 0.0, 0.0).is_ok() {
                let _ = c.paint_with_alpha(alpha.min(1.0));
            }
        }
        for (&(x, y, bw, ch), &(colour, on)) in boxes.iter().zip(looks.iter()) {
            c.rectangle(x + 0.5, y + 0.5, bw - 1.0, ch - 1.0);
            let lighter = |k: f64| (colour.0 + (1.0 - colour.0) * k, colour.1 + (1.0 - colour.1) * k, colour.2 + (1.0 - colour.2) * k);
            if on {
                c.set_source_rgba(colour.0, colour.1, colour.2, 1.0);
                let _ = c.fill_preserve();
                let e = lighter(0.55);
                c.set_source_rgba(e.0, e.1, e.2, 1.0);
                c.set_line_width(1f64.max(1.2 * scale));
                let _ = c.stroke();
                let inset = 1.5f64.max(0.22 * bw.min(ch));
                c.rectangle(x + inset, y + inset, bw - 2.0 * inset, ch - 2.0 * inset);
                let k = lighter(0.4);
                c.set_source_rgba(k.0, k.1, k.2, 0.75);
                let _ = c.fill();
                continue;
            }
            c.set_source_rgba(colour.0 * 0.16, colour.1 * 0.16, colour.2 * 0.16, 0.8);
            let _ = c.fill_preserve();
            c.set_source_rgba(colour.0, colour.1, colour.2, 0.28);
            c.set_line_width(1f64.max(1.2 * scale));
            let _ = c.stroke();
        }
        let (cap, gap) = self.r.room(th, scale);
        if cap != 0.0 && !self.r.label.is_empty() {
            let zoom = th.tw.status_zoom;
            let (x, _, bw, _) = boxes[0];
            let track = status_font(th, c, scale * zoom).0;
            let widths: Vec<f64> = self.r.label.chars().map(|ch| advance(c, ch)).collect();
            let left = x + bw / 2.0 - (widths.iter().sum::<f64>() + track * (widths.len() - 1) as f64) / 2.0;
            let base = th.tw.status_colours[0].0.unwrap_or(th.tw.deck_colour).rgb();
            let colour = (base.0 + (1.0 - base.0) * self.bright, base.1 + (1.0 - base.1) * self.bright, base.2 + (1.0 - base.2) * self.bright);
            let name = surface(wi, hi);
            {
                let n = ctx(&name);
                status_font(th, &n, scale * zoom);
                n.set_source_rgba(colour.0, colour.1, colour.2, 1.0);
                tracked_rounded(&n, &self.r.label, left, (bottom + gap + cap).round(), track);
            }
            let (blur, off) = (2f64.max((2.5 * scale).round()), 1f64.max((1.5 * scale).round()));
            let shade = resampled(&resampled(&name, 1.0 / blur, Filter::Good), blur, Filter::Bilinear);
            c.set_source_rgba(0.0, 0.0, 0.0, 0.95);
            for _ in 0..2 {
                let _ = c.mask_surface(&shade, off, off);
            }
            if c.set_source_surface(&name, 0.0, 0.0).is_ok() {
                let _ = c.paint();
            }
        }
    }
}
