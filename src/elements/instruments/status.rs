//! The network status line: a lamp, ONLINE or OFFLINE, and the local, gateway and public addresses.
//! Not in a card: under the network's graph or the horizon, or at its group's anchor.

use super::draw::*;
use super::{Base, Widget, WidgetType};
use crate::config::{Anchor, Theme};
use crate::gfx::{Blend, Gfx};
use crate::scene::{Geo, GraphPlace, Net, Shared, World};
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, ImageSurface};
use std::f64::consts::PI;

/// Set the status line's font. Returns (tracking, capital height, gap between groups).
pub fn status_font(th: &Theme, c: &Context, scale: f64) -> (f64, f64, f64) {
    mono(c, false);
    c.set_font_size((7f64.max(13.0 * scale)).round());
    let (cap, x_advance) = text_extents(c, "X").map_or((0.0, 0.0), |x| (-x.y_bearing(), x.x_advance()));
    ((1f64.max(3.0 * scale)).round(), cap, (th.tw.status_space * x_advance).round())
}

type Rgba = (f64, f64, f64, f64);

/// A run of the status line.
enum Run {
    /// The lamp, and the room it takes.
    Lamp(f64),
    /// Text, its colour, and whether it is tracked.
    Text(String, Rgba, bool),
}

/// The status line's colours: keys, values, and the lit lamp.
fn status_colours(th: &Theme) -> (Rgba, Rgba, Rgba) {
    let t = &th.tw;
    let [keys, values] = t.status_colours;
    let with = |c: Colour, a: f64| {
        let (r, g, b) = c.rgb();
        (r, g, b, a)
    };
    let key = keys.0.map_or(with(t.deck_colour, t.deck_alpha), |k| with(k, 0.95));
    let value = with(values.0.unwrap_or(t.caption), 0.95);
    let bright = with(values.0.unwrap_or(t.needle_colour), 0.95);
    (key, value, bright)
}

fn status_groups(th: &Theme, c: &Context, scale: f64, online: bool, fields: &[(&str, Option<&str>)]) -> (Vec<Vec<Run>>, Vec<f64>) {
    let (track, cap, _) = status_font(th, c, scale);
    let lamp = (cap * 1.1).round();
    let (key, value, bright) = status_colours(th);
    let word = if online { "ONLINE" } else { "OFFLINE" };
    let mut groups = vec![vec![Run::Lamp(lamp + (cap * 0.6).round()), Run::Text(word.into(), if online { key } else { bright }, true)]];
    for (k, v) in fields {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            groups.push(vec![Run::Text(k.to_string(), key, true), Run::Text(format!(" {v}"), value, false)]);
        }
    }
    let width = |r: &Run| -> f64 {
        match r {
            Run::Lamp(room) => *room,
            Run::Text(text, _, tracked) => {
                let t = if *tracked { track } else { 0.0 };
                text.chars().map(|ch| advance(c, ch) + t).sum::<f64>() - t
            }
        }
    };
    let widths = groups.iter().map(|g| g.iter().map(width).sum()).collect();
    (groups, widths)
}

/// The status line's widest possible width: OFFLINE and every address at full length.
fn status_room(th: &Theme, scale: f64) -> f64 {
    let c = probe();
    let widest = Some("255.255.255.255");
    let (groups, widths) = status_groups(th, &c, scale, false, &[("IP", widest), ("GW", widest), ("WAN", widest)]);
    widths.iter().sum::<f64>() + (groups.len() - 1) as f64 * status_font(th, &c, scale).2
}

/// The x of the mountains' scale, in line with the right corner brackets.
pub fn mountain_scale_x(w: i32, h: i32) -> f64 {
    (w as f64 - 0.03 * unit(w, h)).round()
}

/// The status line's area: under the horizon, right-aligned with the mountains' "bit/s"; or under
/// the graph at `under`.
pub fn status_area(th: &Theme, w: i32, h: i32, zoom: f64, under: Option<GraphPlace>) -> Area {
    let base = card_scale(th, w, h);
    let scale = base * zoom * th.tw.status_zoom;
    let hh = ((7f64.max(13.0 * scale)).round() * 1.8).round();
    let gap = ((7f64.max(13.0 * base)).round() * 1.8).round() / 2.0;
    let cap = status_font(th, &probe(), scale).1;
    let mut below = (th.horizon_y(h)).round() - 1.0 + 1.5f64.max(unit(w, h) / 500.0);
    let label = label_size(w, h) as f64;
    let mut right = mountain_scale_x(w, h) - (label * 0.8).round() - (label * 0.4).round();
    if let Some(GraphPlace { left, right: end, base: b, corner }) = under {
        below = b;
        right = if corner { end.round() } else { (end - 0.05 * (end - left)).round() };
    }
    let y = (below + gap - (hh / 2.0 - cap / 2.0)).round();
    let room = status_room(th, scale);
    Area { x: right - room, y, w: room, h: hh, scale }
}

/// The y where the status line's capitals end, under a graph whose base is `line`.
pub fn status_bottom(th: &Theme, w: i32, h: i32, zoom: f64, line: f64) -> f64 {
    let a = status_area(th, w, h, zoom, Some(GraphPlace { left: 0.0, right: w as f64, base: line, corner: false }));
    let cap = status_font(th, &probe(), a.scale).1;
    a.y + a.h / 2.0 + cap / 2.0
}

fn status_surface(th: &Theme, w: i32, h: i32, scale: f64, online: bool, fields: &[(&str, Option<&str>)]) -> ImageSurface {
    let out = surface(w, h);
    {
        let c = ctx(&out);
        let (groups, widths) = status_groups(th, &c, scale, online, fields);
        let (track, cap, space) = status_font(th, &c, scale);
        let lamp = (cap * 1.1).round();
        let (key, _, bright) = status_colours(th);
        let base = (h as f64 / 2.0 + cap / 2.0).round();
        let mut x = w as f64 - widths.iter().sum::<f64>() - space * (groups.len() - 1) as f64;
        for (g, gw) in groups.iter().zip(widths.iter()) {
            let mut cx = x.round();
            for run in g {
                match run {
                    Run::Lamp(room) => {
                        let r = lamp / 2.0 - 0.5;
                        c.arc(cx + lamp / 2.0, base - cap / 2.0, r, 0.0, 2.0 * PI);
                        if online {
                            c.set_source_rgba(bright.0, bright.1, bright.2, bright.3);
                            let _ = c.fill();
                        } else {
                            c.set_line_width(1f64.max(scale * 1.5));
                            c.set_source_rgba(key.0, key.1, key.2, key.3);
                            let _ = c.stroke();
                        }
                        cx += room;
                    }
                    Run::Text(text, col, tracked) => {
                        c.set_source_rgba(col.0, col.1, col.2, col.3);
                        let t = if *tracked { track } else { 0.0 };
                        cx = super::draw::tracked(&c, text, cx, base, t);
                        if *tracked {
                            cx -= track;
                        }
                    }
                }
            }
            x += gw + space;
        }
    }
    out.flush();
    drop_shadow(out, &th.tw.status_shadow, scale, None)
}

pub struct Status {
    b: Base,
    shown: Net,
}

pub const TYPE: WidgetType = WidgetType {
    name: "status",
    check: crate::elements::check::<StatusSettings>,
    make: |name, _| Box::new(Status { b: Base::new(name), shown: Net::default() }),
};

super::settings!(StatusSettings {});
impl crate::elements::Validate for StatusSettings {}

impl Status {
    /// Under the graph or the horizon, or at its group's anchor: left- or right-aligned to the group,
    /// top- or bottom-aligned.
    fn place(&self, g: &Geo, placed: Option<Area>, zoom: f64, s: &Shared) -> Area {
        let area = status_area(&g.th, g.w, g.h, zoom, s.graph);
        let Some(p) = placed.filter(|_| self.b.anchor != Anchor::Status) else { return area };
        let top = if matches!(self.b.anchor, Anchor::UnderTitle | Anchor::TopLeft | Anchor::TopRight) { p.y } else { p.y + p.h - area.h };
        let mut right = p.x + p.w;
        if self.b.anchor == Anchor::UnderTitle {
            right -= 2.0 * g.th.title_unit(g.w, g.h, &g.th.title_text(&g.host));
        }
        Area { x: if matches!(self.b.anchor, Anchor::TopLeft | Anchor::BottomLeft) { p.x } else { right - area.w }, y: top, ..area }
    }
}

impl Widget for Status {
    fn base(&self) -> &Base {
        &self.b
    }
    fn base_mut(&mut self) -> &mut Base {
        &mut self.b
    }
    fn card(&self) -> bool {
        false
    }
    fn room(&self, g: &Geo, cw: i32, gap: i32) -> Option<(i32, i32)> {
        if self.b.anchor == Anchor::Status {
            return None;
        }
        let w = status_area(&g.th, g.w, g.h, 1.0, None).w;
        Some((1.max(((w + gap as f64) / (cw + gap) as f64).ceil() as i32), 1))
    }
    fn start(&mut self, g: &Geo, w: &World, s: &Shared) {
        self.b.area = Some(self.place(g, self.b.area, w.zoom, s));
    }
    fn state(&mut self, w: &World) -> u64 {
        let net = &w.network;
        let mut h = match net.online {
            None => 1,
            Some(false) => 2,
            Some(true) => 3,
        };
        for s in [&net.address, &net.router, &net.public] {
            h = hash_bytes(hash_bytes(h, s.as_deref().map_or(&[0xff][..], str::as_bytes)), &[0xfe]);
        }
        if h != self.b.state || self.b.baked.is_none() {
            self.shown = w.network.clone(); // only when it changed
        }
        self.b.state = h;
        h
    }
    fn over(&mut self, th: &Theme, gfx: &mut Gfx, p: f64) {
        let Some(a) = self.b.area else { return };
        if self.b.baked != Some(self.b.state) {
            let n = &self.shown;
            let fields = [("IP", n.address.as_deref()), ("GW", n.router.as_deref()), ("WAN", n.public.as_deref())];
            let s = status_surface(th, a.w as i32, a.h as i32, a.scale, n.online == Some(true), &fields);
            self.b.put(gfx, &s);
            self.b.baked = Some(self.b.state);
        }
        if let Some(t) = self.b.tex {
            gfx.image(t, a.x, a.y, a.w, a.h, p, Blend::Premultiplied);
        }
    }
}
