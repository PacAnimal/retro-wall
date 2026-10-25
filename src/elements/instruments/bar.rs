//! A bar filling left to right in `segments` blocks (0: smooth), the number above on the right.

use super::draw::*;
use super::ring::reading_state;
use super::{Base, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, settings, within};
use crate::scene::World;
use cairo::Context;

super::reading_settings!(BarSettings { segments: i64 = 12 }, "cpu", "CPU");

impl Validate for BarSettings {
    fn validate(&self) -> Result<(), String> {
        check_ease(self.tau, self.deadband, self.step)?;
        within("segments", self.segments as f64, 0.0, 100.0)
    }
}

pub const TYPE: WidgetType = WidgetType {
    name: "bar",
    check: check::<BarSettings>,
    make: |name, t| {
        let s: BarSettings = settings(t);
        Box::new(Bar { b: Base::new(name), r: Reading::new(s.reading()), segments: s.segments })
    },
};

pub struct Bar {
    b: Base,
    r: Reading,
    segments: i64,
}

impl Widget for Bar {
    super::reading_widget!();
    fn state(&mut self, w: &World) -> u64 {
        let (v, l) = self.r.read(w);
        reading_state(&mut self.b, v, l)
    }
    fn draw(&mut self, th: &Theme, c: &Context, w: i32, h: i32, scale: f64) {
        let value = self.r.value;
        let (lit, track, _) = Reading::colours(th);
        let (w, h) = (w as f64, h as f64);
        let (x0, x1) = ((12.0 * scale).round(), w - (12.0 * scale).round());
        let (y0, y1) = ((h * 0.42).round(), (h * 0.6).round());
        let n = self.segments;
        let gap = if n != 0 { 1f64.max((3.0 * scale).round()) } else { 0.0 };
        let cells = if n != 0 { n } else { 1 };
        let cw = (x1 - x0 - gap * (cells - 1) as f64) / cells as f64;
        let filled = value / 100.0 * cells as f64;
        for i in 0..cells {
            let cx = x0 + i as f64 * (cw + gap);
            c.rectangle(cx, y0, cw, y1 - y0);
            rgba(c, track, 0.3);
            let _ = c.fill();
            let part = if n == 0 {
                0f64.max(1f64.min(filled - i as f64))
            } else if (i as f64) < filled.round() {
                1.0
            } else {
                0.0
            };
            if part != 0.0 {
                c.rectangle(cx, y0, cw * part, y1 - y0);
                rgba(c, lit, 0.95);
                let _ = c.fill();
            }
        }
        Reading::number(c, th, value, x1, y0 - (6.0 * scale).round(), (9f64.max(20.0 * scale)).round());
        self.r.name_under(c, th, w as i32, y1, scale);
    }
}
