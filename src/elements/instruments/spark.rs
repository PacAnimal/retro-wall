//! A history graph of the last `span` seconds, one point every `every` seconds, newest on the right.

use super::draw::*;
use super::ring::reading_state;
use super::{Base, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, positive, settings, within};
use crate::scene::{Geo, Shared, World, hash_f};
use cairo::Context;

super::reading_settings!(SparkSettings { span: f64 = 60.0, every: f64 = 1.0 }, "cpu", "CPU");

impl Validate for SparkSettings {
    fn validate(&self) -> Result<(), String> {
        check_ease(self.tau, self.deadband, self.step)?;
        within("every", self.every, 0.05, 3600.0)?;
        positive("span", self.span)?;
        within("span / every", self.span / self.every, 1.0, 10_000.0)
    }
}

pub const TYPE: WidgetType = WidgetType {
    name: "spark",
    check: check::<SparkSettings>,
    make: |name, t| {
        let s: SparkSettings = settings(t);
        Box::new(Spark { b: Base::new(name), r: Reading::new(s.reading()), span: s.span, every: s.every, points: vec![], at: None, shown: vec![] })
    },
};

pub struct Spark {
    b: Base,
    r: Reading,
    span: f64,
    every: f64,
    points: Vec<f64>,
    at: Option<i64>,
    shown: Vec<f64>,
}

impl Widget for Spark {
    super::reading_widget!();
    fn start(&mut self, _g: &Geo, w: &World, _s: &Shared) {
        self.points.clear();
        self.at = None;
        if w.frozen.is_some() || w.demo {
            // made-up history
            self.points = (0..(self.span / self.every) as i64)
                .map(|i| {
                    let i = i as f64;
                    0f64.max(100f64.min(45.0 + 30.0 * (i / 5.0).sin() + 12.0 * (i / 1.7).sin()))
                })
                .collect();
        }
    }
    fn state(&mut self, w: &World) -> u64 {
        let (value, _) = w.reading(&self.r.source);
        let slot = (w.clock_real / self.every) as i64;
        if self.at != Some(slot) && w.frozen.is_none() {
            self.at = Some(slot);
            self.points.push(value);
            let keep = (self.span / self.every) as usize;
            if self.points.len() > keep {
                self.points.drain(..self.points.len() - keep);
            }
        }
        self.shown.clear();
        self.shown.extend(self.points.iter().map(|&p| p.round()));
        let (v, l) = self.r.read(w);
        let mut h = reading_state(&mut self.b, v, l);
        for &p in &self.shown {
            hash_f(&mut h, p);
        }
        self.b.state = h;
        h
    }
    fn draw(&mut self, th: &Theme, c: &Context, w: i32, h: i32, scale: f64) {
        let value = self.r.value;
        let (lit, track, _) = Reading::colours(th);
        let (w, h) = (w as f64, h as f64);
        let (x0, x1) = ((10.0 * scale).round(), w - (10.0 * scale).round());
        let (cap, gap) = self.r.room(th, scale);
        let (y0, y1) = ((h * 0.3).round(), h - (24.0 * scale).round().max(cap + gap + (6.0 * scale).round()));
        if th.tw.card_frame {
            c.rectangle(x0, y0, x1 - x0, y1 - y0);
            rgba(c, track, 0.12);
            let _ = c.fill();
        } else {
            c.rectangle(x0, y1, x1 - x0, 1f64.max((1.5 * scale).round()));
            rgba(c, track, 0.6);
            let _ = c.fill();
        }
        let n = 2.max((self.span / self.every) as i64);
        let points = &self.shown;
        if !points.is_empty() {
            let step = (x1 - x0) / (n - 1) as f64;
            let start = x1 - step * (points.len() - 1) as f64;
            let pts: Vec<(f64, f64)> = points.iter().enumerate().map(|(i, &p)| (start + i as f64 * step, y1 - (y1 - y0) * p / 100.0)).collect();
            c.move_to(pts[0].0, y1);
            for &(x, y) in &pts {
                c.line_to(x, y);
            }
            c.line_to(pts[pts.len() - 1].0, y1);
            c.close_path();
            rgba(c, lit, 0.3);
            let _ = c.fill();
            for (i, &(x, y)) in pts.iter().enumerate() {
                if i > 0 { c.line_to(x, y) } else { c.move_to(x, y) }
            }
            c.set_line_width(1f64.max(1.6 * scale));
            rgba(c, lit, 0.95);
            let _ = c.stroke();
        }
        Reading::number(c, th, value, x1, y0 - (5.0 * scale).round(), (9f64.max(18.0 * scale)).round());
        self.r.name_under(c, th, w as i32, y1, scale);
    }
}
