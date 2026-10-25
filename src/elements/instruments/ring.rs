//! A ring gauge: an arc filling clockwise over a faint track, the number inside.

use super::draw::*;
use super::{Base, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, positive, settings, within};
use crate::scene::{World, hash_f};
use cairo::{Context, LineCap};

super::reading_settings!(RingSettings { sweep: f64 = 300.0, thick: f64 = 0.12 }, "cpu", "CPU");

impl Validate for RingSettings {
    fn validate(&self) -> Result<(), String> {
        check_ease(self.tau, self.deadband, self.step)?;
        within("sweep", self.sweep, 1.0, 360.0)?;
        positive("thick", self.thick)?;
        within("thick", self.thick, 0.0, 0.5)
    }
}

pub const TYPE: WidgetType = WidgetType {
    name: "ring",
    check: check::<RingSettings>,
    make: |name, t| {
        let s: RingSettings = settings(t);
        Box::new(Ring { b: Base::new(name), r: Reading::new(s.reading()), sweep: s.sweep, thick: s.thick })
    },
};

pub struct Ring {
    b: Base,
    r: Reading,
    sweep: f64,
    thick: f64,
}

/// A reading widget's state: its value and indicator.
pub fn reading_state(b: &mut Base, value: f64, lit: bool) -> u64 {
    let mut h = lit as u64;
    hash_f(&mut h, value);
    b.state = h;
    h
}

impl Widget for Ring {
    super::reading_widget!();
    fn state(&mut self, w: &World) -> u64 {
        let (v, l) = self.r.read(w);
        reading_state(&mut self.b, v, l)
    }
    fn draw(&mut self, th: &Theme, c: &Context, w: i32, h: i32, scale: f64) {
        let value = self.r.value;
        let (lit, track, ink) = Reading::colours(th);
        let (w, h) = (w as f64, h as f64);
        let (cap, gap) = self.r.room(th, scale);
        let below = cap + gap + (4.0 * scale).round();
        let thick = w.min(h - below) * self.thick;
        let r = w.min(h - below) * 0.44 - thick / 2.0;
        let half = ((360.0 - self.sweep) / 2.0).to_radians();
        let foot = r * half.cos() + thick / 2.0;
        let (cx, cy) = (w / 2.0, (h - below - foot - r - thick / 2.0) / 2.0 + r + thick / 2.0);
        let start = (90.0 + (360.0 - self.sweep) / 2.0).to_radians();
        let end = start + self.sweep.to_radians();
        c.set_line_width(thick);
        c.set_line_cap(LineCap::Butt);
        c.arc(cx, cy, r, start, end);
        rgba(c, track, 0.35);
        let _ = c.stroke();
        if value > 0.0 {
            c.arc(cx, cy, r, start, start + (end - start) * value / 100.0);
            rgba(c, lit, 0.95);
            let _ = c.stroke();
        }
        let size = (r * 0.62).round();
        mono(c, true);
        c.set_font_size(size);
        let text = format!("{}", value.round() as i64);
        if let Ok(ext) = crate::util::text_extents(c, &text) {
            rgba(c, ink, 0.95);
            c.move_to(cx - ext.x_advance() / 2.0, cy + ext.height() / 2.0);
            let _ = crate::util::show_text(c, &text);
        }
        c.new_path();
        self.r.name_under(c, th, w as i32, cy + foot, scale);
    }
}
