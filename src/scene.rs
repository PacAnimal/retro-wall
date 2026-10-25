//! The scene: a theme's elements as parts, bottom to top. Still parts are baked once into layers;
//! live parts draw each frame they change. Each run of still layers between two live parts is merged
//! into one texture, and empty runs are dropped. The still picture (the wallpaper) is every layer with
//! the live parts drawn at rest between them.

use crate::config::Theme;
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::util::*;
use cairo::{Context, Format, ImageSurface};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

pub type Rect = (f64, f64, f64, f64);
pub use crate::platform::Edges;

/// A piece of the header, which a monitor shows or not ([monitors] in the theme or retro.toml).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Title,
    Widgets,
    Graph,
}

impl Piece {
    pub const ALL: [Piece; 3] = [Piece::Title, Piece::Widgets, Piece::Graph];
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The header pieces a monitor shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header(u8);

impl Header {
    pub const NONE: Header = Header(0);
    pub const ALL: Header = Header(0b111);
    pub fn contains(self, p: Piece) -> bool {
        self.0 & p.bit() != 0
    }
    pub fn with(self, p: Piece) -> Header {
        Header(self.0 | p.bit())
    }
}

/// What every part knows of the picture it is in.
#[derive(Clone)]
pub struct Geo {
    pub w: i32,
    pub h: i32,
    pub host: String,
    pub edges: Edges,
    pub scale: i32, // supersampling factor
    pub header: Header,
    pub monitor: Option<String>,
    /// The look it is drawn in.
    pub th: Arc<Theme>,
}

impl Geo {
    /// A monitor's picture in the look `th`, supersampled as it says.
    pub fn new(w: i32, h: i32, host: &str, edges: Edges, header: Header, monitor: Option<String>, th: Arc<Theme>) -> Geo {
        Geo { w, h, host: host.to_string(), edges, scale: th.tw.supersample as i32, header, monitor, th }
    }
}

/// A typed key for a value one part leaves another: `const RULER: Key<Ruler> = Key::new("ruler");`.
pub struct Key<T>(&'static str, std::marker::PhantomData<fn() -> T>);

impl<T> Key<T> {
    pub const fn new(name: &'static str) -> Self {
        Key(name, std::marker::PhantomData)
    }
}

/// Where the network's graph stands: the status line goes under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphPlace {
    pub left: f64,
    pub right: f64,
    /// Its base line.
    pub base: f64,
    /// In a corner, rather than across the horizon.
    pub corner: bool,
}

/// Values parts leave for each other (the ruler, a skyline's towers, the sun's halos), by typed key,
/// or by type and an element's name; and the graph's place, which the status line reads.
#[derive(Default)]
pub struct Shared {
    pub graph: Option<GraphPlace>,
    /// By type, then by name: a lookup borrows its name.
    map: HashMap<std::any::TypeId, HashMap<String, Box<dyn Any>>>,
}

impl Shared {
    pub fn put<T: Any>(&mut self, key: &Key<T>, v: T) {
        self.put_named(key.0, v);
    }
    pub fn get<T: Any>(&self, key: &Key<T>) -> Option<&T> {
        self.get_named(key.0)
    }
    pub fn put_named<T: Any>(&mut self, name: &str, v: T) {
        self.map.entry(std::any::TypeId::of::<T>()).or_default().insert(name.to_string(), Box::new(v));
    }
    pub fn get_named<T: Any>(&self, name: &str) -> Option<&T> {
        self.map.get(&std::any::TypeId::of::<T>())?.get(name)?.downcast_ref()
    }
}

/// The network's status: whether the internet answers, and the local, gateway and public addresses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Net {
    pub online: Option<bool>,
    pub address: Option<String>,
    pub router: Option<String>,
    pub public: Option<String>,
}

/// The readings live parts see, and the clocks.
pub struct World {
    pub clock: f64,      // scene clock, slows to a stop on pausing
    pub clock_real: f64, // monotonic clock, for what eases in real time (needles)
    pub frozen: Option<crate::live::Frozen>,
    pub demo: bool,
    pub zoom: f64,
    pub traffic: crate::sources::Traffic,
    pub network: Net,
    pub readings: std::sync::Arc<std::sync::Mutex<HashMap<String, crate::sources::Reading>>>,
}

impl World {
    /// The readings, locked; one left poisoned by a panic elsewhere is used as it is.
    fn readings(&self) -> std::sync::MutexGuard<'_, HashMap<String, crate::sources::Reading>> {
        self.readings.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// A source's last value (0 to 100) and its indicator.
    pub fn reading(&self, source: &str) -> (f64, bool) {
        if let Some(f) = &self.frozen {
            return (f.value(source), f.mains);
        }
        let mut r = self.readings();
        match r.get(source) {
            Some(e) => (e.value, e.indicator),
            None => {
                r.insert(source.to_string(), Default::default()); // asked for: the sampler reads it from now on
                (0.0, false)
            }
        }
    }
    /// A source's text (a headline), if any.
    pub fn text(&self, source: &str) -> Option<std::sync::Arc<str>> {
        if self.frozen.is_some() {
            return None;
        }
        self.readings().get(source).and_then(|r| r.text.clone())
    }
    /// Whether a source has failed (its last value is stale).
    pub fn stale(&self, source: &str) -> bool {
        self.frozen.is_none() && self.readings().get(source).is_some_and(|r| r.stale)
    }
    /// Disk reads and writes completed so far. The deck's reels follow them.
    pub fn disk_ios(&self) -> u64 {
        if self.demo {
            return crate::sources::demo_disk_ios();
        }
        crate::sources::disk_ios()
    }
}

/// A layer that adjacent still parts share, painted as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Back,
    Front,
    City,
    Land,
}

/// One of a part's places in the scene: a still layer it paints, or a live draw. Most parts have one
/// of each kind at most; a part with several (the instruments) is told which by its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Still(u8),
    Live(u8),
}

pub trait Part {
    /// The header piece it is, drawn only on monitors that show it. None: scenery, drawn on every
    /// monitor.
    fn shows(&self) -> Option<Piece> {
        None
    }
    /// Its passes, bottom to top. By default a still pass if `still`, then a live one if `live`.
    fn passes(&self) -> &'static [Pass] {
        match (self.still(), self.live()) {
            (true, true) => &[Pass::Still(0), Pass::Live(0)],
            (true, false) => &[Pass::Still(0)],
            (false, true) => &[Pass::Live(0)],
            (false, false) => &[],
        }
    }

    // still side
    fn still(&self) -> bool {
        false
    }
    /// A layer shared with adjacent parts of the same layer; None for a layer of its own.
    fn layer(&self) -> Option<Layer> {
        None
    }
    /// Whether it fades with the instruments on pausing.
    fn fades(&self) -> bool {
        false
    }
    /// Keep its layer as its own texture, never merged with neighbours.
    fn alone(&self) -> bool {
        false
    }
    /// The rectangles it can touch, or None for the whole screen.
    fn area(&self, _g: &Geo) -> Option<Vec<Rect>> {
        None
    }
    fn paint(&mut self, _g: &Geo, _c: &Context, _s: &mut Shared) {}
    /// Paint still pass k.
    fn paint_pass(&mut self, _k: u8, g: &Geo, c: &Context, s: &mut Shared) {
        self.paint(g, c, s)
    }

    // live side
    fn live(&self) -> bool {
        false
    }
    /// Whether it wants the flattened picture under it: to paint with (`set_backdrop`, before it
    /// paints) or as a texture (`set_backdrop_tex`, once the scene is compiled).
    fn backdrop(&self) -> Backdrop {
        Backdrop::None
    }
    fn set_backdrop(&mut self, _picture: &ImageSurface) {}
    fn set_backdrop_tex(&mut self, _tex: Tex) {}
    /// Whether it wants the picture under it with the live parts below drawn at rest (a card's frost).
    /// Given to `set_under` before anything is painted over it.
    fn under(&self) -> bool {
        false
    }
    fn set_under(&mut self, _picture: &ImageSurface) {}
    /// The topmost row it changes.
    fn top(&self, g: &Geo) -> i32 {
        g.h
    }
    fn start(&mut self, _g: &Geo, _w: &mut World, _s: &mut Shared) {}
    /// Its pictures, added to `up`; it keeps the handles. Called once, after the still picture is drawn
    /// (`rest`, never asked again): pictures kept only for that are handed over, not copied.
    fn textures(&mut self, _g: &Geo, _s: &Shared, _up: &mut Uploads) {}
    /// A number that changes when its picture at the world's clock does. Asked once a frame, before
    /// any part draws.
    fn state(&mut self, _g: &Geo, _w: &mut World) -> u64 {
        0
    }
    /// Whether the first answer to something it asked is still to come: a snapshot waits for it.
    fn waiting(&self) -> bool {
        false
    }
    fn draw(&mut self, _g: &Geo, _gfx: &mut Gfx, _w: &World, _p: f64) {}
    /// Draw live pass k.
    fn draw_pass(&mut self, _k: u8, g: &Geo, gfx: &mut Gfx, w: &World, p: f64) {
        self.draw(g, gfx, w, p)
    }
    /// Seconds until it may next change.
    fn interval(&self) -> f64 {
        1.0 / 30.0
    }
    /// Draw itself at rest into the still picture.
    fn rest(&mut self, _g: &Geo, _c: &Context, _s: &mut Shared) {}
    /// Draw live pass k at rest.
    fn rest_pass(&mut self, _k: u8, g: &Geo, c: &Context, s: &mut Shared) {
        self.rest(g, c, s)
    }
    /// Its monitor is gone: stop anything it keeps running.
    fn stop(&mut self) {}
}

/// Whether a part has a live pass.
/// What a part wants of the picture under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    None,
    Picture,
    Texture,
}

pub fn moves(p: &dyn Part) -> bool {
    p.passes().iter().any(|x| matches!(x, Pass::Live(_)))
}

pub fn hash_f(h: &mut u64, v: f64) {
    *h = h.wrapping_mul(0x100000001b3) ^ v.to_bits();
}

/// One entry of a frame's plan: a run of still layers as one texture, or a part's live pass.
pub enum Step {
    Run { tex: Tex, fade: bool, area: Option<Vec<Rect>>, opaque: bool },
    Live(usize, u8),
}

/// A still layer, painted.
struct Painted {
    surface: ImageSurface,
    fades: bool,
    /// The rectangles it covers; None for the whole picture.
    area: Option<Vec<Rect>>,
    alone: bool,
}

enum Entry {
    Layer(Painted),
    Live(usize, u8),
}

pub struct Scene {
    pub geo: Geo,
    pub parts: Vec<Box<dyn Part>>,
    pub shared: Shared,
    entries: Option<Vec<Entry>>,
    backdrops: Vec<(usize, ImageSurface)>,
}

/// Rectangles that cover the same area without overlap.
pub fn disjoint(rects: &[Rect]) -> Vec<Rect> {
    let mut ys: Vec<f64> = rects.iter().flat_map(|r| [r.1, r.3]).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut out: Vec<Rect> = vec![];
    for w in ys.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        let mut spans: Vec<(f64, f64)> = rects.iter().filter(|r| r.1 <= y0 && r.3 >= y1).map(|r| (r.0, r.2)).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut merged: Vec<(f64, f64)> = vec![];
        for (a, b) in spans {
            match merged.last_mut() {
                Some(m) if a <= m.1 => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        for (a, b) in merged {
            match out.last_mut() {
                Some(o) if o.0 == a && o.2 == b && o.3 == y0 => o.3 = y1,
                _ => out.push((a, y0, b, y1)),
            }
        }
    }
    out
}

fn flatten<'a>(c: &Context, layers: impl Iterator<Item = &'a ImageSurface>) {
    for s in layers {
        if c.set_source_surface(s, 0.0, 0.0).is_ok() {
            let _ = c.paint();
        }
    }
}

impl Scene {
    pub fn new(geo: Geo) -> Scene {
        let parts = crate::elements::build(&geo);
        Scene { geo, parts, shared: Shared::default(), entries: None, backdrops: vec![] }
    }

    /// Draw the still layers, bottom to top, with the live parts between.
    fn layers(&mut self) {
        if self.entries.is_some() {
            return;
        }
        let g = self.geo.clone();
        let mut entries: Vec<Entry> = vec![];
        let mut current: Option<Layer> = None;
        for i in 0..self.parts.len() {
            let wants = self.parts[i].backdrop();
            if wants != Backdrop::None {
                let flat = surface_rgb(g.w, g.h);
                flatten(
                    &ctx(&flat),
                    entries.iter().filter_map(|e| match e {
                        Entry::Layer(l) => Some(&l.surface),
                        Entry::Live(..) => None,
                    }),
                );
                match wants {
                    Backdrop::Picture => self.parts[i].set_backdrop(&flat),
                    _ => self.backdrops.push((i, flat)),
                }
            }
            if self.parts[i].under() {
                let picture = surface(g.w, g.h);
                {
                    let c = ctx(&picture);
                    for e in &entries {
                        match e {
                            Entry::Layer(l) => flatten(&c, std::iter::once(&l.surface)),
                            Entry::Live(j, k) => {
                                c.save().ok();
                                self.parts[*j].rest_pass(*k, &g, &c, &mut self.shared);
                                c.restore().ok();
                            }
                        }
                    }
                }
                self.parts[i].set_under(&picture);
            }
            for &pass in self.parts[i].passes() {
                match pass {
                    Pass::Still(k) => {
                        let layer = self.parts[i].layer();
                        let same = layer.is_some() && layer == current && matches!(entries.last(), Some(Entry::Layer(..)));
                        if !same {
                            let s = if entries.is_empty() { surface_rgb(g.w, g.h) } else { surface(g.w, g.h) };
                            let p = &self.parts[i];
                            entries.push(Entry::Layer(Painted { surface: s, fades: p.fades(), area: p.area(&g), alone: p.alone() }));
                        } else if let Some(Entry::Layer(l)) = entries.last_mut()
                            && let Some(area) = l.area.as_mut()
                        {
                            match self.parts[i].area(&g) {
                                None => l.area = None,
                                Some(a) => area.extend(a),
                            }
                        }
                        current = layer;
                        if let Some(Entry::Layer(l)) = entries.last() {
                            let c = ctx(&l.surface);
                            self.parts[i].paint_pass(k, &g, &c, &mut self.shared);
                        }
                    }
                    Pass::Live(k) => {
                        entries.push(Entry::Live(i, k));
                        current = None;
                    }
                }
            }
        }
        if !matches!(entries.first(), Some(Entry::Layer(..))) {
            // a scene starts with a still element (elements::trial); without one, it starts black
            entries.insert(0, Entry::Layer(Painted { surface: surface_rgb(g.w, g.h), fades: false, area: None, alone: false }));
        }
        self.entries = Some(entries);
    }

    /// The still picture: every layer, and every live part at rest, in order.
    pub fn compose(&mut self, c: &Context) {
        self.layers();
        let g = self.geo.clone();
        for e in self.entries.iter().flatten() {
            match e {
                Entry::Layer(l) => flatten(c, std::iter::once(&l.surface)),
                Entry::Live(i, k) => self.parts[*i].rest_pass(*k, &g, c, &mut self.shared),
            }
        }
    }

    /// The frame's plan, its runs' textures added to `up`: each run of layers between live parts
    /// merged into one.
    pub fn compile(&mut self, up: &mut Uploads) -> Vec<Step> {
        self.layers();
        let g = self.geo.clone();
        let entries = self.entries.take().unwrap_or_default();
        let mut plan = vec![];
        let mut run: Vec<Painted> = vec![];
        let close = |run: &mut Vec<Painted>, plan: &mut Vec<Step>, up: &mut Uploads| {
            if run.is_empty() {
                return;
            }
            let opaque = plan.is_empty();
            let s = if run.len() == 1 {
                run[0].surface.clone()
            } else {
                let flat = if opaque { surface_rgb(g.w, g.h) } else { surface(g.w, g.h) };
                flatten(&ctx(&flat), run.iter().map(|l| &l.surface));
                flat
            };
            let area = if run.iter().any(|l| l.area.is_none()) {
                None
            } else {
                Some(disjoint(&run.iter().flat_map(|l| l.area.iter().flatten().copied()).collect::<Vec<_>>()))
            };
            let fade = run[0].fades;
            run.clear();
            // a run over others keeps only where it has anything drawn, within its area
            let tex = if opaque {
                up.add(s, Kind::Layer)
            } else {
                let ink = if s.format() == Format::ARgb32 { ink_box(&s, 0) } else { Some((0, 0, g.w, g.h)) };
                let Some(mut r) = ink else { return };
                if let Some(a) = &area {
                    let bound = a.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, q| {
                        (b.0.min(q.0.floor() as i32), b.1.min(q.1.floor() as i32), b.2.max(q.2.ceil() as i32), b.3.max(q.3.ceil() as i32))
                    });
                    r = (r.0.max(bound.0), r.1.max(bound.1), r.2.min(bound.2), r.3.min(bound.3));
                    if r.2 <= r.0 || r.3 <= r.1 {
                        return;
                    }
                }
                up.add_layer(s, r)
            };
            plan.push(Step::Run { tex, fade, area, opaque });
        };
        for e in entries {
            match e {
                Entry::Live(i, k) => {
                    close(&mut run, &mut plan, up);
                    plan.push(Step::Live(i, k));
                }
                Entry::Layer(l) => {
                    if let Some(last) = run.last()
                        && (last.fades != l.fades || last.alone || l.alone)
                    {
                        close(&mut run, &mut plan, up);
                    }
                    run.push(l);
                }
            }
        }
        close(&mut run, &mut plan, up);
        for (i, flat) in std::mem::take(&mut self.backdrops) {
            let tex = up.add(flat, Kind::Layer);
            self.parts[i].set_backdrop_tex(tex);
        }
        plan
    }
}

/// Draw a frame's plan: runs and live passes in order, fading ones at strength p.
pub fn draw_plan(plan: &[Step], parts: &mut [Box<dyn Part>], g: &Geo, gfx: &mut Gfx, w: &World, presence: f64) {
    let p = presence * presence * (3.0 - 2.0 * presence);
    for step in plan {
        match step {
            Step::Run { tex, fade, area, opaque } => {
                gfx.layer(*tex, area.as_deref(), if *fade { p } else { 1.0 }, if *opaque { Blend::Opaque } else { Blend::Premultiplied });
            }
            Step::Live(i, k) => {
                if !parts[*i].fades() {
                    parts[*i].draw_pass(*k, g, gfx, w, 1.0);
                } else if p > 0.0 {
                    parts[*i].draw_pass(*k, g, gfx, w, p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_values_are_kept_by_type_and_name() {
        let mut s = Shared::default();
        s.put_named("a", 1u32);
        s.put_named("a", "one");
        s.put_named("b", 2u32);
        assert_eq!(s.get_named::<u32>("a"), Some(&1));
        assert_eq!(s.get_named::<&str>("a"), Some(&"one"));
        assert_eq!(s.get_named::<u32>("b"), Some(&2));
        assert_eq!(s.get_named::<u32>("c"), None);
        assert_eq!(s.get_named::<u64>("a"), None);
        s.put_named("a", 3u32);
        assert_eq!(s.get_named::<u32>("a"), Some(&3));
    }
}
