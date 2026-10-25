//! The tweaks: every shape, size and colour of the core a theme can set (`[tweaks]` in theme.toml,
//! lower case). Defaults are in core.toml; a theme's values are checked against `LIMITS`, then by
//! deserialising into `Tweaks` and `Tweaks::check`.

use crate::path::PathData;
use serde::Deserialize;
use serde::de::{self, Deserializer};

/// An sRGB colour, 0 to 1 a channel, from "#rrggbb".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour(pub f64, pub f64, pub f64);

impl Colour {
    pub fn parse(s: &str) -> Option<Colour> {
        let b = s.as_bytes();
        if b.len() != 7 || b[0] != b'#' || !b[1..].iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap() as f64 / 255.0;
        Some(Colour(p(1), p(3), p(5)))
    }
    pub fn rgb(self) -> (f64, f64, f64) {
        (self.0, self.1, self.2)
    }
    pub fn hex(self) -> String {
        let c = |v: f64| (v * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", c(self.0), c(self.1), c(self.2))
    }
    /// Toward white by k (0 to 1).
    pub fn lighter(self, k: f64) -> Colour {
        Colour(self.0 + (1.0 - self.0) * k, self.1 + (1.0 - self.1) * k, self.2 + (1.0 - self.2) * k)
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Colour::parse(&s).ok_or_else(|| de::Error::custom(format!("{s:?} is not a colour (#rrggbb)")))
    }
}

/// A colour, or "" for the default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaybeColour(pub Option<Colour>);

impl<'de> Deserialize<'de> for MaybeColour {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.is_empty() {
            return Ok(MaybeColour(None));
        }
        Colour::parse(&s).map(|c| MaybeColour(Some(c))).ok_or_else(|| de::Error::custom(format!("{s:?} is not a colour (#rrggbb, or \"\")")))
    }
}

/// A drop shadow: offset, blur (pixels at scale 1), strength (0: none; over 1: laid twice), colour.
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub dx: f64,
    pub dy: f64,
    pub blur: f64,
    pub strength: f64,
    pub colour: Colour,
}

impl<'de> Deserialize<'de> for Shadow {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let (dx, dy, blur, strength, colour) = <(f64, f64, f64, f64, Colour)>::deserialize(d)?;
        Ok(Shadow { dx, dy, blur, strength, colour })
    }
}

/// The tape reel: SVG path data on a 32 x 32 box centred on (16, 16), filled even-odd; and its two
/// windows, the smallest of its subpaths (by box) off the hub on the left and on the right, which the
/// spool's fill leaves open.
#[derive(Clone, Debug)]
pub struct Reel {
    pub path: PathData,
    pub windows: [PathData; 2],
}

impl Reel {
    pub fn parse(d: &str) -> Result<Reel, String> {
        let path = PathData::parse(d).map_err(|e| format!("reel: {e}"))?;
        let probe = cairo::ImageSurface::create(cairo::Format::A8, 1, 1).and_then(|s| cairo::Context::new(&s)).map_err(|e| e.to_string())?;
        // (box area, right of the hub, its text, its path) of every subpath off the hub
        let mut off: Vec<(f64, bool, String, PathData)> = vec![];
        for part in d.split('M').filter(|p| !p.trim().is_empty()) {
            let sub = format!("M{part}");
            let p = PathData::parse(&sub).map_err(|e| format!("reel: {e}"))?;
            probe.new_path();
            p.trace(&probe);
            let (x0, y0, x1, y1) = probe.path_extents().map_err(|e| e.to_string())?;
            if ((x0 + x1) / 2.0 - 16.0).hypot((y0 + y1) / 2.0 - 16.0) > 3.0 {
                off.push(((x1 - x0) * (y1 - y0), (x0 + x1) / 2.0 > 16.0, sub, p));
            }
        }
        let window = |side: bool| {
            off.iter().filter(|p| p.1 == side).min_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2))).map(|p| p.3.clone()).ok_or_else(|| {
                format!("reel: no window {} of the hub (a subpath, M..., whose box is centred off (16, 16))", if side { "right" } else { "left" })
            })
        };
        Ok(Reel { windows: [window(false)?, window(true)?], path })
    }
}

impl<'de> Deserialize<'de> for Reel {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Reel::parse(&String::deserialize(d)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TitleCorner {
    TopRight,
    BottomLeft,
}

/// A glow's passes: (blur radius, alpha).
pub type Glow = Vec<(f64, f64)>;
/// A range's look: (fill colour, its alpha, ridge colour, its alpha).
pub type RangeLook = (Colour, f64, Colour, f64);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tweaks {
    // a painted theme's canvas
    pub canvas: [f64; 2],
    pub canvas_source: [f64; 2],
    pub canvas_scale: [f64; 2],
    pub crop_centre: [f64; 2],

    // the picture
    pub base: Colour,
    pub horizon: Colour,
    pub horizon_y: f64,
    pub sun: [f64; 2],
    pub turn_gap: f64,
    pub vignette: f64,
    pub scanline: (f64, f64),
    pub scanlines: bool,
    pub day: [f64; 2],
    pub supersample: i64,
    pub fps: f64,
    pub reel_fps: f64,
    pub needle_fps: f64,

    // the hostname
    pub title: f64,
    pub title_top: f64,
    pub title_corner: TitleCorner,
    pub margin: f64,
    pub corner: f64,
    pub rule: bool,
    pub ruler_end: f64,
    pub slant: f64,
    pub track: f64,
    pub title_colours: Vec<(f64, Colour)>,
    pub extrude: Vec<(f64, Colour)>,
    pub slices: Vec<[f64; 2]>,
    pub title_glow: Glow,
    pub title_shadow: Shadow,
    pub tick_seconds: f64,
    pub caption: Colour,
    pub tube: [f64; 2],
    pub tube_glow: Glow,
    pub tube_unlit: f64,
    pub glyph_width: f64,
    pub glyph_miter: f64,
    pub kern_depth: f64,
    pub kern_min: f64,
    pub kern_short: f64,
    pub kern_run: f64,

    // the instruments
    pub deck: f64,
    pub deck_colour: Colour,
    pub deck_alpha: f64,
    pub deck_fill: (Colour, f64),
    pub deck_glow: Glow,
    pub spool_fill: (Colour, f64),
    pub deck_label: String,
    pub deck_fit: f64,
    pub card_frame: bool,
    pub card_shadow: Shadow,
    pub card_h: f64,
    pub instrument_gap: f64,
    pub needle_colour: Colour,
    pub sweep: f64,
    pub pivot: [f64; 2],
    pub scale_r: f64,
    pub cap_r: f64,
    pub cap_drop: f64,
    pub overload: f64,
    pub reel_pad: f64,
    pub reel: Reel,
    pub bolt: f64,
    pub status_zoom: f64,
    pub status_shadow: Shadow,
    pub status_colours: [MaybeColour; 2],
    pub status_space: f64,

    // the network's mountains
    pub mountains: [f64; 2],
    pub mountain_fade: [f64; 2],
    pub mountain_floor: i64,
    pub mountain_tiers: Vec<i64>,
    pub download: RangeLook,
    pub upload: RangeLook,
    pub phi: f64,

    // the synthwave scenery (sun, rays, shimmer, floor, bloom, stripes, brackets)
    pub angle: f64,
    pub bloom: [f64; 3],
    pub bloom_colours: [Colour; 2],
    pub bracket: Colour,
    pub fade: [f64; 2],
    pub gap: f64,
    pub glow: Glow,
    pub grid: Colour,
    pub grid_step: f64,
    pub miter: f64,
    pub portrait_climb: f64,
    pub rays: i64,
    pub ray_step: f64,
    pub round: f64,
    pub shimmer: (f64, f64, [f64; 2], [f64; 2]),
    pub stripe: f64,
    pub stripes: [Colour; 6],
    pub sun_alpha: f64,
    pub sun_colours: Vec<(f64, Colour)>,
    pub sun_glow: Glow,
    pub turn_y: f64,
}

// --- limits ----------------------------------------------------------------------------------------

/// The ranges of a tweak's numbers, read in order with colours and names skipped: one for all, or
/// each by position.
#[derive(Clone, Copy)]
pub enum Ranges {
    All(f64, f64),
    At(&'static [(f64, f64)]),
}

impl Ranges {
    pub fn of(self, i: usize) -> (f64, f64) {
        match self {
            Ranges::All(lo, hi) => (lo, hi),
            Ranges::At(each) => each.get(i).copied().unwrap_or((f64::MIN, f64::MAX)),
        }
    }
}

/// A tweak's valid values. With `rows`, the tweak is a list of at most `rows` entries, and the
/// ranges apply within each entry. Numbers are always finite.
pub struct Limit {
    pub ranges: Ranges,
    pub rows: usize,
}

const fn all(lo: f64, hi: f64) -> Limit {
    Limit { ranges: Ranges::All(lo, hi), rows: 0 }
}

const fn at(each: &'static [(f64, f64)]) -> Limit {
    Limit { ranges: Ranges::At(each), rows: 0 }
}

const fn rows(rows: usize, each: &'static [(f64, f64)]) -> Limit {
    Limit { ranges: Ranges::At(each), rows }
}

const ALPHA: (f64, f64) = (0.0, 1.0);
/// A glow pass: its blur (pixels) and alpha.
const GLOW: &[(f64, f64)] = &[(1.0, 64.0), ALPHA];
/// A shadow: offset, blur, strength.
const SHADOW: &[(f64, f64)] = &[(-50.0, 50.0), (-50.0, 50.0), (0.0, 50.0), (0.0, 2.0)];
/// A gradient stop: where it stands.
const STOP: &[(f64, f64)] = &[ALPHA];

/// Every tweak with a number in it, and its valid values. A tweak with none (a switch, a colour, the
/// reel's path, a corner) is checked by its type alone.
pub const LIMITS: &[(&str, Limit)] = &[
    ("canvas", all(256.0, 16384.0)),
    ("canvas_source", all(1.0, 16384.0)),
    ("canvas_scale", all(0.1, 10.0)),
    ("crop_centre", all(0.0, 1.0)),
    ("horizon_y", all(0.05, 0.95)),
    ("sun", at(&[(0.0, 1.0), (0.01, 0.5)])),
    ("turn_gap", all(0.0, 0.5)),
    ("vignette", all(0.0, 1.0)),
    ("scanline", at(&[(1.0, 64.0), ALPHA])),
    ("day", all(0.0, 24.0)),
    ("supersample", all(1.0, 4.0)),
    ("fps", all(1.0, 240.0)),
    ("reel_fps", all(1.0, 240.0)),
    ("needle_fps", all(1.0, 240.0)),
    ("title", all(0.01, 0.5)),
    ("title_top", all(0.0, 0.9)),
    ("margin", all(0.0, 0.3)),
    ("corner", all(0.0, 0.3)),
    ("ruler_end", all(0.0, 1.0)),
    ("slant", all(-0.5, 1.0)),
    ("track", all(-1.0, 8.0)),
    ("title_colours", rows(16, STOP)),
    ("extrude", rows(2, &[(0.0, 40.0)])),
    ("slices", rows(32, &[ALPHA, ALPHA])),
    ("title_glow", rows(8, GLOW)),
    ("title_shadow", at(SHADOW)),
    ("tick_seconds", all(0.05, 600.0)),
    ("tube", all(0.0, 2.0)),
    ("tube_glow", rows(8, GLOW)),
    ("tube_unlit", all(0.0, 1.0)),
    ("glyph_width", all(0.1, 2.0)),
    ("glyph_miter", all(1.0, 10.0)),
    ("kern_depth", all(0.0, 6.0)),
    ("kern_min", all(0.0, 1.0)),
    ("kern_short", all(0.0, 1.0)),
    ("kern_run", all(0.0, 1.0)),
    ("deck", all(0.02, 0.5)),
    ("deck_alpha", all(0.0, 1.0)),
    ("deck_fill", at(&[ALPHA])),
    ("deck_glow", rows(8, GLOW)),
    ("spool_fill", at(&[ALPHA])),
    ("deck_fit", all(0.5, 1.0)),
    ("card_shadow", at(SHADOW)),
    ("card_h", all(100.0, 400.0)),
    ("instrument_gap", all(0.0, 100.0)),
    ("sweep", all(0.0, 180.0)),
    ("pivot", at(&[(0.0, 300.0), (0.0, 400.0)])),
    ("scale_r", all(10.0, 300.0)),
    ("cap_r", all(0.0, 100.0)),
    ("cap_drop", all(-50.0, 100.0)),
    ("overload", all(0.0, 100.0)),
    ("reel_pad", all(1.0, 1.5)),
    ("bolt", all(0.0, 100.0)),
    ("status_zoom", all(0.25, 4.0)),
    ("status_shadow", at(SHADOW)),
    ("status_space", all(0.0, 10.0)),
    ("mountains", at(&[(0.01, 0.5), (5.0, 3600.0)])),
    ("mountain_fade", at(&[(0.0, 10.0), ALPHA])),
    ("mountain_floor", all(0.0, 10.0)),
    ("mountain_tiers", all(1.0, 15.0)),
    ("download", at(&[ALPHA, ALPHA])),
    ("upload", at(&[ALPHA, ALPHA])),
    ("phi", all(0.0, 10.0)),
    ("angle", all(5.0, 85.0)),
    ("bloom", at(&[(0.01, 4.0), (0.01, 2.0), ALPHA])),
    ("fade", all(0.0, 1.0)),
    ("gap", all(0.0, 0.1)),
    ("glow", rows(8, GLOW)),
    ("grid_step", all(0.05, 4.0)),
    ("miter", all(0.0, 1.0)),
    ("portrait_climb", all(0.0, 2.0)),
    ("rays", all(0.0, 200.0)),
    ("ray_step", all(0.0, 1.0)),
    ("round", all(0.0, 4.0)),
    ("shimmer", at(&[(0.0, 0.05), (0.0, 2.0), (0.001, 1.0), (0.1, 100.0), (0.001, 1.0), (0.1, 100.0)])),
    ("stripe", all(0.001, 0.1)),
    ("sun_alpha", all(0.0, 1.0)),
    ("sun_colours", rows(16, STOP)),
    ("sun_glow", rows(8, GLOW)),
    ("turn_y", all(0.0, 1.0)),
];

/// The numbers in a value, in order.
fn numbers(v: &toml::Value, out: &mut Vec<f64>) {
    match v {
        toml::Value::Float(f) => out.push(*f),
        toml::Value::Integer(i) => out.push(*i as f64),
        toml::Value::Array(a) => a.iter().for_each(|v| numbers(v, out)),
        _ => {}
    }
}

/// A tweak's value within its limits, or why not (to follow the tweak's name).
pub fn limit(name: &str, v: &toml::Value) -> Result<(), String> {
    let Some((_, l)) = LIMITS.iter().find(|(n, _)| *n == name) else { return Ok(()) };
    let entries: Vec<&toml::Value> = match (l.rows, v) {
        (0, _) => vec![v],
        (most, toml::Value::Array(a)) if a.len() <= most => a.iter().collect(),
        (most, _) => return Err(format!(" has more than {most} entries: at most {most}")),
    };
    for e in entries {
        let mut ns = vec![];
        numbers(e, &mut ns);
        for (i, n) in ns.iter().enumerate() {
            let (lo, hi) = l.ranges.of(i);
            if !n.is_finite() || *n < lo || *n > hi {
                let which = if ns.len() > 1 { format!("'s number {}", i + 1) } else { String::new() };
                return Err(format!("{which} is {}: it is {} to {}", crate::util::num(*n), crate::util::num(lo), crate::util::num(hi)));
            }
        }
    }
    Ok(())
}

impl Tweaks {
    /// What the limits of single tweaks cannot say: how tweaks stand to each other.
    pub fn check(&self) -> Result<(), String> {
        let [rise, set] = self.day;
        if !(rise < set && (1.0..=23.0).contains(&(set - rise))) {
            return Err("day is [rise, set], hours of one day, rise first, the day 1 to 23 hours long".into());
        }
        let tiers = &self.mountain_tiers;
        if tiers.windows(2).any(|w| w[0] >= w[1]) || tiers.first().is_some_and(|&t| t <= self.mountain_floor) {
            return Err("mountain_tiers rise, every one over mountain_floor".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_hash_and_six_hex_digits() {
        assert_eq!(Colour::parse("#ff0080"), Some(Colour(1.0, 0.0, 128.0 / 255.0)));
        assert_eq!(Colour::parse("#FFFFFF"), Some(Colour(1.0, 1.0, 1.0)));
        for bad in ["", "#", "ff0080", "#ff008", "#ff00800", "#gg0000", "#ff 080", "#ff00\u{e9}", "\u{0}#ff0080"] {
            assert_eq!(Colour::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_colour_goes_back_to_its_hex() {
        for hex in ["#000000", "#ff4fb0", "#4fb6d6", "#ffffff"] {
            assert_eq!(Colour::parse(hex).unwrap().hex(), hex);
        }
        assert_eq!(Colour(0.0, 0.5, 1.0).lighter(1.0), Colour(1.0, 1.0, 1.0));
        assert_eq!(Colour(0.2, 0.4, 0.6).lighter(0.0), Colour(0.2, 0.4, 0.6));
    }
}
