//! A face plugin (plugins.rs) that answers drawing operations for each new reading. It runs on its own
//! thread: a reading is asked for and the answer picked up on a later frame. Shows the last good
//! picture, ERR when the face failed; a failed face is asked again after a while.

use super::{Base, Widget};
use crate::config::Theme;
use crate::scene::{World, hash_f};
use cairo::{Context, ImageSurface};
use toml::Table;

use super::draw::hash_bytes;

pub struct PluginFace {
    b: Base,
    source: String,
    label: String,
    size: (i32, i32),
    settings: Table,
    face: Option<crate::plugins::Face>,
    /// The reading last asked about (its hash), and when.
    asked: Option<u64>,
    asked_at: f64,
    /// Answers picked up so far: a new one redraws.
    answers: u64,
    picture: Option<ImageSurface>,
    /// What every request carries of the theme.
    look: crate::plugins::FaceLook,
}

impl PluginFace {
    /// Seconds before a failed face is asked again.
    const RETRY: f64 = 10.0;

    /// A face's settings are its own (they reach the plugin); only source, label and size are read here,
    /// each its default when it is not what it should be.
    pub fn new(th: &Theme, name: &str, path: std::path::PathBuf, t: &Table) -> Self {
        let text = |k: &str, d: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or(d).to_string();
        let cards = |v: &toml::Value| v.as_integer().filter(|n| (1..=8).contains(n)).map(|n| n as i32);
        let size = match t.get("size").and_then(|s| s.as_array()).map(|a| a.as_slice()) {
            Some([c, r]) => cards(c).zip(cards(r)),
            _ => None,
        };
        let look = crate::plugins::FaceLook::of(th);
        PluginFace {
            b: Base::new(name),
            source: text("source", "cpu"),
            label: text("label", ""),
            size: size.unwrap_or((1, 1)),
            settings: t.clone(),
            face: Some(crate::plugins::Face::new(path, look.clone())),
            asked: None,
            asked_at: 0.0,
            answers: 0,
            picture: None,
            look,
        }
    }
}

impl Widget for PluginFace {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn size(&self) -> (i32, i32) {
        self.size
    }
    fn label(&self) -> Option<&str> {
        (!self.label.is_empty()).then_some(self.label.as_str())
    }
    fn draws(&self) -> bool {
        true
    }
    fn stop(&mut self) {
        self.face = None;
    }
    fn state(&mut self, w: &World) -> u64 {
        let (Some(face), Some(a)) = (self.face.as_mut(), self.b.area) else { return self.b.state };
        if let Some((_, answer)) = face.answer() {
            self.answers += 1;
            match answer {
                Ok(picture) => {
                    self.picture = Some(picture.surface());
                    self.b.error = None;
                }
                Err(e) => self.b.error = Some(e),
            }
        }
        let (value, indicator) = w.reading(&self.source);
        let stale = w.stale(&self.source);
        let text = w.text(&self.source);
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        hash_f(&mut h, (value * 10.0).round());
        hash_f(&mut h, indicator as u8 as f64);
        hash_f(&mut h, stale as u8 as f64);
        h = hash_bytes(h, text.as_deref().unwrap_or("").as_bytes());
        let retry = self.b.error.is_some() && w.clock_real - self.asked_at >= Self::RETRY;
        if self.asked != Some(h) || retry {
            let reading = crate::sources::Reading { value, indicator, text, stale };
            face.ask(crate::plugins::face_request(&self.look, a.w as i32, a.h as i32, a.scale, &reading, &self.settings));
            self.asked = Some(h);
            self.asked_at = w.clock_real;
        }
        self.b.stale = stale;
        let mut s = self.answers;
        hash_f(&mut s, self.b.error.is_some() as u8 as f64);
        hash_f(&mut s, stale as u8 as f64);
        self.b.state = s;
        s
    }
    fn waiting(&self) -> bool {
        self.face.is_some() && self.asked.is_some() && self.answers == 0
    }
    fn draw(&mut self, _th: &Theme, c: &Context, _w: i32, _h: i32, _scale: f64) {
        if let Some(picture) = &self.picture
            && c.set_source_surface(picture, 0.0, 0.0).is_ok()
        {
            let _ = c.paint();
        }
    }
}
