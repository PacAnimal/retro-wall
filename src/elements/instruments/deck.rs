//! The tape deck: reels play while the disks are busy, and now and then spool fast.

use super::draw::*;
use super::{Base, SpriteTex, Widget, WidgetType};
use crate::config::Theme;
use crate::elements::{Validate, check, settings};
use crate::gfx::{Kind, Uploads};
use crate::rng::Rng;
use crate::scene::{Geo, Shared, World};
use crate::util::*;
use cairo::{Context, FillRule, ImageSurface, LineJoin};
use std::f64::consts::PI;

super::settings!(DeckSettings { label: String = "DISK".into() });
impl Validate for DeckSettings {}

pub const TYPE: WidgetType = WidgetType { name: "deck", check: check::<DeckSettings>, make: |name, t| Box::new(Deck::new(name, settings(t))) };

/// Place a reel's 32 x 32 drawing on `c`, centred on (cx, cy), `size` across, turned `angle` degrees.
fn placed(c: &Context, cx: f64, cy: f64, size: f64, angle: f64) {
    c.translate(cx, cy);
    c.rotate(angle.to_radians());
    c.scale(size / 30.72, size / 30.72);
    c.translate(-16.0, -16.0);
}

/// Fill one reel's spool inside its rim, leaving the windows; placed as reel().
fn spool(th: &Theme, c: &Context, cx: f64, cy: f64, size: f64, angle: f64) {
    c.save().ok();
    placed(c, cx, cy, size, angle);
    c.arc(16.0, 16.0, 15.0, 0.0, 2.0 * PI);
    for window in &th.tw.reel.windows {
        c.new_sub_path();
        window.trace(c);
    }
    c.set_fill_rule(FillRule::EvenOdd);
    let _ = c.fill();
    c.restore().ok();
}

/// Fill one reel, centred on (cx, cy), `size` across, turned `angle` degrees clockwise.
fn reel(th: &Theme, c: &Context, cx: f64, cy: f64, size: f64, angle: f64) {
    c.save().ok();
    placed(c, cx, cy, size, angle);
    th.tw.reel.path.trace(c);
    c.set_fill_rule(FillRule::EvenOdd);
    let _ = c.fill();
    c.restore().ok();
}

/// One reel `size` pixels across, white on transparency, with a margin: its outline or its spool.
fn reel_sprite(th: &Theme, size: f64, spool_part: bool) -> ImageSurface {
    let pad = th.tw.reel_pad;
    let full = (size * pad).ceil() as i32;
    let s = surface(full, full);
    {
        let c = ctx(&s);
        c.set_source_rgb(1.0, 1.0, 1.0);
        let f = full as f64;
        if spool_part { spool(th, &c, f / 2.0, f / 2.0, f / pad, 0.0) } else { reel(th, &c, f / 2.0, f / 2.0, f / pad, 0.0) }
    }
    s
}

#[derive(PartialEq)]
enum TapeMode {
    Still,
    Play,
    Forward,
    Back,
}

/// The tape's motion, driven by disk activity. Idle: still. Active: plays for at least 1-1.5 s; after
/// 4-10 s of play it spools fast for 1-3 s (forward 2 times in 3), then plays again. Speed changes ease.
struct Tape {
    angle: f64,
    speed: f64,
    target: f64,
    last: Option<f64>,
    mode: TapeMode,
    until: f64,
    spool_at: f64,
    ios: Option<u64>,
    busy: f64,
    sampled: f64,
    shown: f64,
    shown_at: f64,
    rng: Rng,
}

impl Tape {
    const PLAY: f64 = 180.0;
    const SPOOL: f64 = 1080.0;
    const EASE: f64 = 0.06;
    const GRACE: f64 = 0.2;
    const SAMPLE: f64 = 0.05;
    const STEP: f64 = 6.0;

    fn new() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0);
        Tape {
            angle: 0.0,
            speed: 0.0,
            target: 0.0,
            last: None,
            mode: TapeMode::Still,
            until: 0.0,
            spool_at: 0.0,
            ios: None,
            busy: -1.0,
            sampled: -1.0,
            shown: 0.0,
            shown_at: -1.0,
            rng: Rng::new(seed & 0x7fff_ffff_ffff),
        }
    }
    /// Whether the disks have completed any reads or writes in the last GRACE seconds.
    fn active(&mut self, t: f64, w: &World) -> bool {
        if t - self.sampled < Self::SAMPLE {
            return t - self.busy < Self::GRACE;
        }
        self.sampled = t;
        let ios = w.disk_ios();
        if self.ios.is_some_and(|i| i != ios) {
            self.busy = t;
        }
        self.ios = Some(ios);
        t - self.busy < Self::GRACE
    }
    fn play(&mut self, t: f64) {
        self.mode = TapeMode::Play;
        self.target = Self::PLAY;
        self.until = t + self.rng.range(1.0, 1.5);
        self.spool_at = t + self.rng.range(4.0, 10.0);
    }
    /// The reels' angle at time t, in half degrees, shown at up to `fps` (`reel_fps` while spooling).
    fn at(&mut self, t: f64, w: &World, fps: f64, reel_fps: f64) -> f64 {
        let dt = self.last.map_or(0.0, |l| (t - l).min(0.1));
        self.last = Some(t);
        let active = self.active(t, w);
        match self.mode {
            TapeMode::Still => {
                if active {
                    self.play(t);
                }
            }
            TapeMode::Play => {
                if t >= self.spool_at {
                    self.mode = if self.rng.f64() < 0.667 { TapeMode::Forward } else { TapeMode::Back };
                    self.target = if self.mode == TapeMode::Forward { Self::SPOOL } else { -Self::SPOOL };
                    self.until = t + self.rng.range(1.0, 3.0);
                } else if t >= self.until && !active {
                    self.mode = TapeMode::Still;
                    self.target = 0.0;
                }
            }
            _ => {
                if t >= self.until {
                    self.play(t);
                }
            }
        }
        self.speed += (self.target - self.speed) * (1.0 - (1.0 - Self::EASE).powf(dt * 60.0));
        if self.target == 0.0 && self.speed.abs() < 2.0 {
            self.speed = 0.0;
        }
        self.angle = (self.angle + self.speed * dt).rem_euclid(360.0);
        if t - self.shown_at >= 1.0 / reel_fps.min(fps.max(self.speed.abs() / Self::STEP)) - 0.004 {
            self.shown = (self.angle * 2.0).round() / 2.0;
            self.shown_at = t;
        }
        self.shown
    }
}

/// The reels' angle in a snapshot.
const SNAPSHOT_REELS: f64 = 37.5;

pub struct Deck {
    b: Base,
    label: String,
    tape: Tape,
    angle: f64,
    centres: Vec<(f64, f64, f64)>,
    size: f64,
    /// FPS and REEL_FPS.
    fps: (f64, f64),
}

impl Deck {
    fn new(name: &str, t: DeckSettings) -> Self {
        Deck { b: Base::new(name), label: t.label, tape: Tape::new(), angle: 0.0, centres: vec![], size: 0.0, fps: (30.0, 60.0) }
    }
    /// The deck's (left, top, scale) in its card.
    fn frame(&self, th: &Theme) -> Option<(f64, f64, f64)> {
        let Area { x, y, w, h, scale } = self.b.area?;
        let fit = th.tw.deck_fit;
        Some((x + (w - 300.0 * fit * scale) / 2.0, y + (h - 156.0 * fit * scale) / 2.0, fit * scale))
    }
    /// Each reel's (centre x, centre y, phase in degrees), and their diameter.
    fn reels(&self, th: &Theme) -> (Vec<(f64, f64, f64)>, f64) {
        let Some((x, y, scale)) = self.frame(th) else { return (vec![], 0.0) };
        (vec![(x + 70.0 * scale, y + 70.0 * scale, 0.0), (x + 230.0 * scale, y + 70.0 * scale, 60.0)], 120.0 * scale)
    }
    /// Map a point of the 66 x 20 tape-and-head path onto 153 x 56 at (73, 102) in deck units.
    fn at(frame: (f64, f64, f64), px: f64, py: f64) -> (f64, f64) {
        let (x, y, scale) = frame;
        (x + (73.0 + (px - 143.0) * 153.0 / 66.0) * scale, y + (102.0 + (py - 136.0) * 56.0 / 20.0) * scale)
    }
}

impl Widget for Deck {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn stack(&self) -> i32 {
        0
    }
    fn label(&self) -> Option<&str> {
        Some(self.label.as_str()).filter(|l| !l.is_empty())
    }
    fn lines(&self, th: &Theme, c: &Context) {
        let Some(frame) = self.frame(th) else { return };
        c.set_line_width(1f64.max(2.5 * frame.2));
        c.set_line_join(LineJoin::Miter);
        for (k, (px, py)) in [(146.2, 145.0), (165.0, 153.0), (187.0, 153.0), (205.8, 145.0)].into_iter().enumerate() {
            let (x, y) = Self::at(frame, px, py);
            if k > 0 { c.line_to(x, y) } else { c.move_to(x, y) }
        }
        let _ = c.stroke();
        let ((hx, hy), (hx1, hy1)) = (Self::at(frame, 166.0, 139.0), Self::at(frame, 186.0, 151.0));
        c.rectangle(hx, hy, hx1 - hx, hy1 - hy);
        let _ = c.stroke();
    }
    fn fill(&self, th: &Theme, c: &Context) {
        let Some(frame) = self.frame(th) else { return };
        let ((hx, hy), (hx1, hy1)) = (Self::at(frame, 166.0, 139.0), Self::at(frame, 186.0, 151.0));
        let (colour, a) = fill_of(th.tw.spool_fill);
        rgba(c, colour, a);
        c.rectangle(hx, hy, hx1 - hx, hy1 - hy);
        let _ = c.fill();
    }
    fn rest(&self, th: &Theme, c: &Context) {
        let (centres, size) = self.reels(th);
        let (sc, sa) = fill_of(th.tw.spool_fill);
        for (cx, cy, phase) in centres {
            rgba(c, sc, sa);
            spool(th, c, cx, cy, size, phase);
            rgba(c, th.tw.deck_colour.rgb(), th.tw.deck_alpha);
            reel(th, c, cx, cy, size, phase);
        }
    }
    fn textures(&mut self, th: &Theme, up: &mut Uploads, sprites: &mut SpriteTex) {
        let size = 4.0 * self.reels(th).1.ceil();
        for (kind, spool_part) in [(SpriteKind::Reel, false), (SpriteKind::Spool, true)] {
            let tex = &mut sprites[kind as usize];
            if tex.is_none() {
                *tex = Some(up.add(reel_sprite(th, size, spool_part), Kind::Sprite));
            }
        }
    }
    fn start(&mut self, g: &Geo, _w: &World, _s: &Shared) {
        self.tape = Tape::new();
        (self.centres, self.size) = self.reels(&g.th);
        self.fps = (g.th.tw.fps, g.th.tw.reel_fps);
    }
    fn state(&mut self, w: &World) -> u64 {
        self.angle = if w.frozen.is_some() { SNAPSHOT_REELS } else { self.tape.at(w.clock_real, w, self.fps.0, self.fps.1) };
        self.b.state = self.angle.to_bits();
        self.b.state
    }
    fn interval_needed(&self) -> Option<f64> {
        let (fps, reel_fps) = self.fps;
        Some(1.0 / if self.tape.speed.abs() / Tape::STEP > fps { reel_fps } else { fps })
    }
    fn sprites(&self, th: &Theme, p: f64, out: &mut Sprites) {
        let size = self.size * th.tw.reel_pad;
        let (sc, sa) = fill_of(th.tw.spool_fill);
        let (deck, alpha) = (th.tw.deck_colour.rgb(), th.tw.deck_alpha);
        for &(cx, cy, phase) in &self.centres {
            let at = Sprite { cx, cy, size, angle: self.angle + phase, rgb: sc, alpha: sa * p };
            out[SpriteKind::Spool as usize].push(at);
            out[SpriteKind::Reel as usize].push(Sprite { rgb: deck, alpha: alpha * p, ..at });
        }
    }
}
