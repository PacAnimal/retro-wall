//! A battery filled to its charge, the number in the middle, a bolt beside it while on mains.

use super::draw::*;
use super::ring::reading_state;
use super::{Base, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, settings};
use crate::scene::World;
use cairo::Context;
use std::f64::consts::PI;

super::reading_settings!(BatterySettings {}, "bat", "BAT");

impl Validate for BatterySettings {
    fn validate(&self) -> Result<(), String> {
        check_ease(self.tau, self.deadband, self.step)
    }
}

pub const TYPE: WidgetType = WidgetType {
    name: "battery",
    check: check::<BatterySettings>,
    make: |name, t| {
        let s: BatterySettings = settings(t);
        Box::new(Battery { b: Base::new(name), r: Reading::new(s.reading()) })
    },
};

pub struct Battery {
    b: Base,
    r: Reading,
}

impl Widget for Battery {
    super::reading_widget!();
    fn state(&mut self, w: &World) -> u64 {
        let (v, l) = self.r.read(w);
        reading_state(&mut self.b, v, l)
    }
    fn draw(&mut self, th: &Theme, c: &Context, w: i32, h: i32, scale: f64) {
        let (value, mains) = (self.r.value, self.r.lit);
        let (lit, track, ink) = Reading::colours(th);
        let (w, h) = (w as f64, h as f64);
        let (cap, gap) = self.r.room(th, scale);
        let below = cap + gap + (4.0 * scale).round();
        let bh = (((h - below) * 0.5).min((w - 12.0 * scale) / 2.4)).round();
        let (bw, nub) = ((bh * 2.1).round(), 3f64.max((bh * 0.14).round()));
        let x0 = ((w - bw - nub) / 2.0).round();
        let y0 = ((h - below - bh) / 2.0).round();
        let line = 1.5f64.max(bh * 0.04);
        let rounded = |x: f64, y: f64, rw: f64, rh: f64, r: f64| {
            c.new_sub_path();
            c.arc(x + rw - r, y + r, r, -PI / 2.0, 0.0);
            c.arc(x + rw - r, y + rh - r, r, 0.0, PI / 2.0);
            c.arc(x + r, y + rh - r, r, PI / 2.0, PI);
            c.arc(x + r, y + r, r, PI, 1.5 * PI);
            c.close_path();
        };
        c.set_line_width(line);
        rounded(x0 + line / 2.0, y0 + line / 2.0, bw - line, bh - line, bh * 0.14);
        rgba(c, track, 0.9);
        let _ = c.stroke();
        rounded(x0 + bw, y0 + bh * 0.3, nub, bh * 0.4, nub * 0.4);
        let _ = c.fill();
        let pad = (line * 2.0).max(bh * 0.1);
        let inner = (x0 + pad, y0 + pad, bw - 2.0 * pad, bh - 2.0 * pad);
        let full = inner.2 * value / 100.0;
        if full > 0.0 {
            rounded(inner.0, inner.1, full, inner.3, (full / 2.0).min(bh * 0.07));
            rgba(c, lit, 0.95);
            let _ = c.fill();
        }
        let text = format!("{}{}", value.round() as i64, if mains { "\u{26a1}" } else { "" });
        mono(c, true);
        c.set_font_size((inner.3 * 0.72).round());
        if let Ok(ext) = crate::util::text_extents(c, &text) {
            c.move_to(x0 + bw / 2.0 - ext.x_advance() / 2.0, y0 + bh / 2.0 + ext.height() / 2.0);
            crate::util::text_path(c, &text);
        }
        let path = c.copy_path();
        c.new_path();
        for (clip, colour) in [((inner.0, y0, full, bh), track), ((inner.0 + full, y0, w, bh), ink)] {
            c.save().ok();
            c.rectangle(clip.0, clip.1, clip.2, clip.3);
            c.clip();
            if let Ok(path) = &path {
                c.append_path(path);
            }
            rgba(c, colour, 0.95);
            let _ = c.fill();
            c.restore().ok();
        }
        self.r.name_under(c, th, w as i32, y0 + bh, scale);
    }
}
