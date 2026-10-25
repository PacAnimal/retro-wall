//! The instruments: widgets laid out in groups at anchors, in rows or columns, sized in cards. One
//! part in four passes over all widgets: cards (still), moving parts (sprites, live), covers (still),
//! and readouts drawn on top (live, redrawn when their value changes). Every widget's state is taken
//! once a frame, before either live pass draws.
//!
//! A widget type is a file here and a line in `TYPES`; a face plugin (plugins.rs) is a type of its
//! own, found by name at run time.

mod bar;
mod battery;
mod cells;
mod deck;
mod draw;
mod face;
mod layout;
mod meter;
mod ring;
mod spark;
mod status;
mod ticker;

pub use draw::{Area, card_scale, label_size};
pub use status::{mountain_scale_x, status_area, status_bottom};

use crate::config::{Anchor, Group, Theme};
use crate::elements::Validate;
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::util::{ctx, drop_shadow, glowing, surface};
use cairo::{Context, ImageSurface};
use draw::*;
use serde::Deserialize;
use std::collections::HashMap;
use toml::Table;

/// A built-in widget type: its name, a check of its settings, and how one is made from settings that
/// passed the check.
pub struct WidgetType {
    pub name: &'static str,
    pub check: fn(&Table) -> Result<(), String>,
    pub make: fn(&str, &Table) -> Box<dyn Widget>,
}

/// The built-in widget types.
pub const TYPES: &[WidgetType] = &[bar::TYPE, battery::TYPE, cells::TYPE, deck::TYPE, meter::TYPE, ring::TYPE, spark::TYPE, status::TYPE, ticker::TYPE];

/// A widget type's settings: `type` and its own, each with its default.
macro_rules! settings {
    ($name:ident { $($field:ident: $t:ty = $default:expr),* $(,)? }) => {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields, default)]
        pub struct $name {
            #[serde(rename = "type")]
            #[allow(dead_code)]
            kind: Option<String>,
            $(pub $field: $t),*
        }
        impl Default for $name {
            fn default() -> Self {
                $name { kind: None, $($field: $default),* }
            }
        }
    };
}
pub(crate) use settings;

/// A reading widget's settings: the common ones (source, label, easing) and its own.
macro_rules! reading_settings {
    ($name:ident { $($field:ident: $t:ty = $default:expr),* $(,)? }, $source:expr, $label:expr) => {
        super::settings!($name {
            source: String = $source.into(), label: String = $label.into(), tau: f64 = 0.8, deadband: f64 = 1.5, step: f64 = 1.0,
            $($field: $t = $default),*
        });
        impl $name {
            pub fn reading(&self) -> super::draw::ReadingSettings {
                super::draw::ReadingSettings { source: self.source.clone(), label: self.label.clone(), tau: self.tau, deadband: self.deadband, step: self.step }
            }
        }
    };
}
pub(crate) use reading_settings;

/// The methods every reading widget has the same.
macro_rules! reading_widget {
    () => {
        fn base(&self) -> &Base {
            &self.b
        }
        fn base_mut(&mut self) -> &mut Base {
            &mut self.b
        }
        fn label(&self) -> Option<&str> {
            self.r.label()
        }
        fn reading(&self) -> bool {
            true
        }
        fn draws(&self) -> bool {
            true
        }
    };
}
pub(crate) use reading_widget;

/// State common to every widget.
pub struct Base {
    pub name: String,
    pub anchor: Anchor,
    pub area: Option<Area>,
    /// Its readout's texture, once uploaded.
    pub tex: Option<Tex>,
    /// The state its readout shows.
    pub baked: Option<u64>,
    pub failed: u32,
    pub error: Option<String>,
    pub stale: bool,
    pub state: u64,
}

impl Base {
    pub fn new(name: &str) -> Self {
        Base { name: name.to_string(), anchor: Anchor::UnderTitle, area: None, tex: None, baked: None, failed: 0, error: None, stale: false, state: 0 }
    }
    /// The readout's new picture: uploaded the first time, replaced after.
    pub fn put(&mut self, gfx: &mut Gfx, s: &ImageSurface) -> Tex {
        match self.tex {
            Some(t) => {
                gfx.replace(t, s, Kind::Label);
                t
            }
            None => *self.tex.insert(gfx.upload(s, Kind::Label)),
        }
    }
}

/// The sprite pictures, by kind: the first widget that gives one sets it.
pub type SpriteTex = [Option<Tex>; 3];

/// A widget drawn in the area the layout gives it.
pub trait Widget {
    fn base(&self) -> &Base;
    fn base_mut(&mut self) -> &mut Base;
    /// Size in cards: (columns, rows).
    fn size(&self) -> (i32, i32) {
        (1, 1)
    }
    /// Z order of its sprites at rest.
    fn stack(&self) -> i32 {
        1
    }
    /// Drawn in a card with outline and label.
    fn card(&self) -> bool {
        true
    }
    fn label(&self) -> Option<&str> {
        None
    }
    /// A reading widget (without a frame it draws its own label).
    fn reading(&self) -> bool {
        false
    }
    /// Size in cards for a widget outside a card; None if it places itself.
    fn room(&self, _g: &Geo, _cw: i32, _gap: i32) -> Option<(i32, i32)> {
        None
    }
    fn set_area(&mut self, a: Option<Area>) {
        self.base_mut().area = a;
    }
    fn fill(&self, _th: &Theme, _c: &Context) {}
    fn lines(&self, _th: &Theme, _c: &Context) {}
    fn text(&self, _th: &Theme, _c: &Context) {}
    fn cover(&self, _th: &Theme, _c: &Context) {}
    fn rest(&self, _th: &Theme, _c: &Context) {}
    /// Its pictures added to `up`: its own kept, the shared sprites set in `sprites` if not yet.
    fn textures(&mut self, _th: &Theme, _up: &mut Uploads, _sprites: &mut SpriteTex) {}
    fn start(&mut self, _g: &Geo, _w: &World, _s: &Shared) {}
    /// Stop anything it keeps running.
    fn stop(&mut self) {}
    /// Update what it shows; returns a value that changes whenever that does.
    fn state(&mut self, w: &World) -> u64;
    /// Whether the first answer to something it asked is still to come.
    fn waiting(&self) -> bool {
        false
    }
    /// Rotating parts drawn under the covers, added to `out` by kind.
    fn sprites(&self, _th: &Theme, _p: f64, _out: &mut Sprites) {}
    /// Whether the default over() should show draw().
    fn draws(&self) -> bool {
        false
    }
    /// Whether it has anything to show.
    fn has_value(&self) -> bool {
        true
    }
    /// Draw on transparency, in the area's own pixel coordinates.
    fn draw(&mut self, _th: &Theme, _c: &Context, _w: i32, _h: i32, _scale: f64) {}
    /// Draw over the covers. Default: draw() cached as an image until the state changes; ERR on error;
    /// dimmed while stale.
    fn over(&mut self, th: &Theme, gfx: &mut Gfx, p: f64) {
        if !self.draws() && self.base().error.is_none() {
            return;
        }
        let Some(a) = self.base().area else { return };
        let strength = p * if self.base().stale { 0.5 } else { 1.0 };
        if self.base().baked != Some(self.base().state) {
            let (wi, hi) = (a.w as i32, a.h as i32);
            let s = surface(wi, hi);
            {
                let c = ctx(&s);
                if self.base().error.is_some() || self.base().failed >= 3 {
                    card_error(&c, th, a.scale);
                } else if self.has_value() {
                    self.draw(th, &c, wi, hi, a.scale);
                    self.base_mut().failed = 0;
                }
            }
            s.flush();
            let b = self.base_mut();
            b.put(gfx, &drop_shadow(s, &th.tw.card_shadow, a.scale, None));
            b.baked = Some(b.state);
        }
        if let Some(t) = self.base().tex {
            gfx.image(t, a.x, a.y, a.w, a.h, strength, Blend::Premultiplied);
        }
    }
    fn top(&self) -> Option<f64> {
        self.base().area.map(|a| a.y)
    }
    /// Seconds until it may next change, if sooner than the scene's frame.
    fn interval_needed(&self) -> Option<f64> {
        None
    }
}

const WIDGET_TYPE_NAMES: &str = "bar, battery, cells, deck, meter, ring, spark, status, ticker";

fn widget_type(kind: &str) -> Result<&'static WidgetType, String> {
    TYPES
        .iter()
        .find(|t| t.name == kind)
        .ok_or_else(|| format!("no widget type {kind:?} (there are {WIDGET_TYPE_NAMES}; faces of your own live in {}/)", crate::plugins::faces_dir().display()))
}

/// The built-in widget types' names.
pub fn type_names() -> impl Iterator<Item = &'static str> {
    TYPES.iter().map(|t| t.name)
}

/// Check a widget's settings, for the theme loader: `kind` is its type, not a face plugin's.
pub fn check_widget(kind: &str, spec: &Table) -> Result<(), String> {
    (widget_type(kind)?.check)(spec)
}

/// Build a widget of type `kind` from its settings: a face plugin of that name, else a built-in type.
fn widget(th: &Theme, name: &str, kind: &str, spec: &Table, faces: &HashMap<String, std::path::PathBuf>) -> Result<Box<dyn Widget>, String> {
    if let Some(path) = faces.get(kind) {
        return Ok(Box::new(face::PluginFace::new(th, name, path.clone(), spec)));
    }
    let t = widget_type(kind)?;
    (t.check)(spec)?;
    Ok((t.make)(name, spec))
}

// --- the element -----------------------------------------------------------------------------------

/// The instruments element: all widgets, laid out when it is made, drawn in passes.
struct Instruments {
    widgets: Vec<Box<dyn Widget>>,
    /// The widgets in cards, laid out; and every widget laid out or placing itself.
    cards: Vec<usize>,
    shown: Vec<usize>,
    /// The cards' rects, padded for their glow.
    area: Vec<Rect>,
    sprite_tex: SpriteTex,
    /// Each frame's sprites and vertices.
    sprites: Sprites,
    verts: Vec<f32>,
    fps: f64,
}

/// Its passes: cards, moving parts, covers, readouts.
const PASSES: &[Pass] = &[Pass::Still(0), Pass::Live(0), Pass::Still(1), Pass::Live(1)];

impl Instruments {
    fn new(g: &Geo) -> Self {
        let th = &g.th;
        let faces = crate::plugins::face_plugins();
        let mut widgets: Vec<Box<dyn Widget>> = vec![];
        let groups: &[Group] = g.monitor.as_deref().and_then(|m| th.monitor_groups.get(m)).unwrap_or(&th.groups);
        for gr in groups {
            for n in &gr.widgets {
                let spec = th.widgets.get(n);
                let kind = spec.and_then(|t| t.get("type")).and_then(|t| t.as_str()).unwrap_or(n);
                match widget(th, n, kind, spec.unwrap_or(&Table::new()), &faces) {
                    Ok(mut w) => {
                        w.base_mut().anchor = gr.anchor;
                        widgets.push(w);
                    }
                    Err(e) => crate::errln!("widget {n}: {e}: left out"), // checked at load; a face gone since
                }
            }
        }
        // laid out once, for the monitor's size
        let (cw, _, gap) = card_size(th, g.w, g.h);
        let mut sizes = HashMap::new();
        for w in &widgets {
            let s = if w.card() { Some(w.size()) } else { w.room(g, cw, gap) };
            if let Some(s) = s {
                sizes.insert(w.base().name.clone(), s);
            }
        }
        let areas = layout::layout(g, groups, &sizes);
        for w in widgets.iter_mut() {
            let a = areas.get(&w.base().name).copied().or(w.base().area);
            w.set_area(a);
        }
        let cards: Vec<usize> = (0..widgets.len()).filter(|&i| widgets[i].card() && widgets[i].base().area.is_some()).collect();
        let reach = 3.0 * th.tw.deck_glow.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) + 2.0;
        let area: Vec<Rect> = cards
            .iter()
            .filter_map(|&i| widgets[i].base().area)
            .map(|a| (0f64.max(a.x - reach), 0f64.max(a.y - reach), a.x + a.w + reach, a.y + a.h + reach))
            .collect();
        Instruments {
            widgets,
            cards,
            shown: vec![],
            area: if area.is_empty() { vec![(0.0, 0.0, 0.0, 0.0)] } else { area },
            sprite_tex: [None; 3],
            sprites: Default::default(),
            verts: vec![],
            fps: th.tw.fps,
        }
    }

    /// Each card's box, outline, face, glowing lines, lettering and label.
    fn paint_cards(&mut self, g: &Geo, c: &Context) {
        let th = &g.th;
        let alpha = th.tw.deck_alpha;
        let deck = th.tw.deck_colour.rgb();
        let lines = surface(g.w, g.h);
        {
            let l = ctx(&lines);
            if th.tw.card_frame {
                let (fill, fa) = fill_of(th.tw.deck_fill);
                for a in self.cards.iter().filter_map(|&i| self.widgets[i].base().area) {
                    rgba(c, fill, fa);
                    c.rectangle(a.x, a.y, a.w, a.h);
                    let _ = c.fill();
                    rgba(&l, deck, alpha);
                    l.rectangle(a.x + 0.5, a.y + 0.5, a.w - 1.0, a.h - 1.0);
                    l.set_line_width(1.0);
                    let _ = l.stroke();
                }
            }
            for w in self.widgets.iter().filter(|w| w.base().area.is_some()) {
                w.fill(th, c);
                l.save().ok();
                rgba(&l, deck, alpha);
                w.lines(th, &l);
                l.restore().ok();
            }
        }
        lines.flush();
        glowing(c, &lines, &th.tw.deck_glow);
        for w in self.widgets.iter().filter(|w| w.base().area.is_some()) {
            w.text(th, c);
        }
        for &i in &self.cards {
            let w = &self.widgets[i];
            if let (Some(label), Some(a)) = (w.label(), w.base().area)
                && (th.tw.card_frame || !w.reading())
            {
                card_label(c, th, label, a);
            }
        }
    }

    /// Over the moving parts: the meters' caps.
    fn paint_covers(&mut self, g: &Geo, c: &Context) {
        for w in self.widgets.iter().filter(|w| w.base().area.is_some()) {
            w.cover(&g.th, c);
        }
    }

    /// All widgets' sprites, one draw per kind, in stack order.
    fn draw_moving(&mut self, g: &Geo, gfx: &mut Gfx, p: f64) {
        for s in self.sprites.iter_mut() {
            s.clear();
        }
        for &i in &self.shown {
            self.widgets[i].sprites(&g.th, p, &mut self.sprites);
        }
        for (placed, tex) in self.sprites.iter().zip(self.sprite_tex.iter()) {
            if let (Some(tex), false) = (tex, placed.is_empty()) {
                self.verts.clear();
                sprite_vertices(placed, &mut self.verts);
                gfx.draw(Some(*tex), &self.verts, Blend::Premultiplied);
            }
        }
    }

    /// What each widget draws over the covers.
    fn draw_readouts(&mut self, g: &Geo, gfx: &mut Gfx, p: f64) {
        for &i in &self.shown {
            self.widgets[i].over(&g.th, gfx, p);
        }
    }
}

fn least(ys: impl Iterator<Item = f64>) -> Option<f64> {
    ys.fold(None, |m: Option<f64>, y| Some(m.map_or(y, |m| m.min(y))))
}

impl Part for Instruments {
    fn shows(&self) -> Option<Piece> {
        Some(Piece::Widgets)
    }
    fn passes(&self) -> &'static [Pass] {
        PASSES
    }
    fn fades(&self) -> bool {
        true
    }
    fn area(&self, _g: &Geo) -> Option<Vec<Rect>> {
        Some(self.area.clone())
    }
    fn paint_pass(&mut self, k: u8, g: &Geo, c: &Context, _s: &mut Shared) {
        if k == 0 { self.paint_cards(g, c) } else { self.paint_covers(g, c) }
    }
    fn top(&self, g: &Geo) -> i32 {
        let cards = least(self.cards.iter().filter_map(|&i| self.widgets[i].base().area).map(|a| a.y));
        let readouts = least(self.widgets.iter().filter_map(|w| w.top()));
        let top = |y: Option<f64>| y.map_or(g.h, |y| y as i32);
        top(cards).min(top(readouts))
    }
    fn start(&mut self, g: &Geo, w: &mut World, s: &mut Shared) {
        for wd in self.widgets.iter_mut() {
            if wd.base().area.is_some() || !wd.card() {
                wd.start(g, w, s);
            }
        }
        self.shown = (0..self.widgets.len()).filter(|&i| self.widgets[i].base().area.is_some()).collect();
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        for &i in &self.cards {
            self.widgets[i].textures(&g.th, up, &mut self.sprite_tex);
        }
    }
    fn stop(&mut self) {
        for w in self.widgets.iter_mut() {
            w.stop();
        }
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let mut h = 7u64;
        for &i in &self.shown {
            h = (h ^ self.widgets[i].state(w)).wrapping_mul(0x100000001b3);
        }
        h
    }
    fn waiting(&self) -> bool {
        self.shown.iter().any(|&i| self.widgets[i].waiting())
    }
    fn interval(&self) -> f64 {
        self.cards.iter().filter_map(|&i| self.widgets[i].interval_needed()).fold(1.0 / self.fps, f64::min)
    }
    fn draw_pass(&mut self, k: u8, g: &Geo, gfx: &mut Gfx, _w: &World, p: f64) {
        if k == 0 { self.draw_moving(g, gfx, p) } else { self.draw_readouts(g, gfx, p) }
    }
    fn rest_pass(&mut self, k: u8, g: &Geo, c: &Context, _s: &mut Shared) {
        if k != 0 {
            return;
        }
        let s = surface(g.w, g.h);
        {
            let parts = ctx(&s);
            let mut cards = self.cards.clone();
            cards.sort_by_key(|&i| self.widgets[i].stack()); // stacked as they are live
            for i in cards {
                self.widgets[i].rest(&g.th, &parts);
            }
        }
        s.flush();
        if c.set_source_surface(&s, 0.0, 0.0).is_ok() {
            let _ = c.paint();
        }
    }
}

/// The element has no settings of its own: its widgets are the layout's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstrumentsSettings {}

impl Validate for InstrumentsSettings {}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "instruments",
        defaults: "",
        varies: &[],
        check: crate::elements::check::<InstrumentsSettings>,
        make: |_, _, g| Ok(vec![Box::new(Instruments::new(g))]),
    }]
}
