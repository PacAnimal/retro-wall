//! The skylines: rows of towers, far to near.

use super::*;

// --- the skylines ----------------------------------------------------------------------------------

pub(super) type Tier = (i64, i64, i64);

#[derive(Clone)]
pub struct Building {
    pub(super) tiers: Vec<Tier>,
    pub(super) crown: &'static str,
    pub(super) shade: f64,
}

/// A window of a skyline: (x0, y0, x1, y1, brightness, floor, building).
pub(super) type Pane = (i64, i64, i64, i64, f64, i64, usize);

/// What a skyline passes to the street, windows and beacons.
#[derive(Clone)]
pub struct SkylineData {
    pub(super) surface: ImageSurface,
    pub(super) windows: Vec<Pane>,
    pub(super) buildings: Vec<Building>,
    pub(super) haze: f64,
    pub(super) tips: Vec<(f64, f64)>,
}

/// A row of buildings on the horizon, across the screen.
pub struct Skyline {
    pub(super) name: String,
    pub(super) seed: i64,
    pub(super) heights: (f64, f64),
    pub(super) breadth: (f64, f64),
    pub(super) downtown: f64,
    pub(super) colour: Rgb,
    pub(super) haze: f64,
    pub(super) fog: Rgb,
    pub(super) rim: Rgb,
    pub(super) window: f64,
    pub(super) glass: Rgb,
    pub(super) dark: f64,
    pub(super) planned: OnceCell<(Vec<Building>, i64)>,
}

pub(super) const FACADES: [&str; 6] = ["punched", "punched", "punched", "curtain", "ribbon", "paired"];

pub(super) fn facade_k(f: &str) -> (f64, f64, f64, f64) {
    // gaps: between bays, between floors, window height inset, window width inset
    match f {
        "punched" => (0.9, 1.3, 0.8, 0.9),
        "curtain" => (0.25, 0.6, 0.15, 0.25),
        "ribbon" => (0.25, 1.6, 0.9, 0.25),
        _ => (0.5, 1.2, 0.7, 0.5),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SkylineSettings {
    pub(super) seed: i64,
    pub(super) heights: [f64; 2],
    pub(super) breadth: [f64; 2],
    pub(super) downtown: f64,
    pub(super) colour: Colour,
    pub(super) haze: f64,
    pub(super) fog: Colour,
    pub(super) rim: Colour,
    pub(super) window: f64,
    pub(super) glass: Colour,
    pub(super) dark: f64,
}

impl Validate for SkylineSettings {
    fn validate(&self) -> Result<(), String> {
        for h in self.heights {
            within("heights", h, 0.0, 1.0)?;
        }
        positive_pair("breadth", self.breadth)?;
        each("breadth", &self.breadth, 0.002, 1.0)?;
        within("downtown", self.downtown, -10.0, 10.0)?;
        within("window", self.window, 0.0001, 0.1)?;
        within("haze", self.haze, 0.0, 1.0)?;
        within("dark", self.dark, 0.0, 1.0)
    }
}

impl Skyline {
    pub(super) fn new(name: &str, st: SkylineSettings) -> Self {
        Skyline {
            name: name.to_string(),
            seed: st.seed,
            heights: (st.heights[0], st.heights[1]),
            breadth: (st.breadth[0], st.breadth[1]),
            downtown: st.downtown,
            colour: st.colour.rgb(),
            haze: st.haze,
            fog: st.fog.rgb(),
            rim: st.rim.rgb(),
            window: st.window,
            glass: st.glass.rgb(),
            dark: st.dark,
            planned: OnceCell::new(),
        }
    }

    pub(super) fn plan(&self, g: &Geo) -> &(Vec<Building>, i64) {
        self.planned.get_or_init(|| {
            let (ww, u) = (g.w as f64, unit(g.w, g.h));
            let mut rng = Rng::new(self.seed);
            let hz = g.th.horizon_y(g.h);
            let mut buildings = vec![];
            let mut x = -rng.range(0.0, self.breadth.1 * u);
            while x < ww {
                let w = rng.range(self.breadth.0, self.breadth.1) * u;
                let near = (-((x + w / 2.0) / ww - self.downtown).powf(2.0) / 0.045).exp();
                let mut h = (self.heights.0 + (self.heights.1 - self.heights.0) * (0.25 + 0.75 * near) * rng.f64().powf(0.8)) * u;
                if rng.f64() < 0.08 {
                    h *= 1.35;
                }
                let (x0, x1, top) = (x.round() as i64, (x + w).round() as i64, (hz - h).round() as i64);
                let mut tiers = vec![(x0, x1, top)];
                let n = *rng.pick(&[0, 0, 1, 1, 2]);
                for _ in 0..n {
                    let (a, b, t) = *tiers.last().unwrap();
                    let inset = rng.range(0.1, 0.22) * (b - a) as f64;
                    if (b - a) as f64 - 2.0 * inset < 0.012 * u {
                        break;
                    }
                    let na = (a as f64 + inset).round() as i64;
                    let nb = (b as f64 - inset).round() as i64;
                    let nt = (t as f64 - rng.range(0.04, 0.12) * h).round() as i64;
                    tiers.push((na, nb, nt));
                }
                let last = *tiers.last().unwrap();
                let slender = ((last.1 - last.0) as f64) < 0.6 * (hz - last.2 as f64);
                let crown = if slender {
                    *rng.pick(&["flat", "flat", "mast", "mast", "slant", "tank", "box", "spire", "spire"])
                } else {
                    *rng.pick(&["flat", "flat", "mast", "mast", "slant", "tank", "box"])
                };
                let shade = rng.f64();
                buildings.push(Building { tiers, crown, shade });
                x += w * rng.range(0.82, 1.0);
            }
            let top = buildings.iter().flat_map(|b| b.tiers.iter().map(|t| t.2)).min().unwrap_or(hz as i64) - (0.06 * u).round() as i64;
            (buildings, top)
        })
    }

    /// A building's windows, its glass drawn unlit: [(x0, y0, x1, y1, brightness, floor)].
    pub(super) fn glazing(&self, g: &Geo, c: &Context, tiers: &[Tier], rng: &mut Rng, later: &[Tier], face: Rgb) -> Vec<(i64, i64, i64, i64, f64, i64)> {
        let (u, hz) = (unit(g.w, g.h), g.th.horizon_y(g.h));
        let size = ((self.window * u).round() as i64).max(1);
        let facade = *rng.pick(&FACADES);
        let (kb, kf, kh, kw) = facade_k(facade);
        let s = size as f64;
        let bay = size + ((s * kb).round() as i64).max(1);
        let floor = size + ((s * kf).round() as i64).max(1);
        let height = floor - ((s * kh).round() as i64).max(1);
        let width = bay - ((s * kw).round() as i64).max(1);
        let lobby = ((1.6 * floor as f64).round() as i64).max(2);
        let parapet = ((0.6 * floor as f64).round() as i64).max(1);
        let (a0, b0, _) = tiers[0];
        let centre = (a0 + b0) as f64 / 2.0;
        let glass = mixed(mixed(self.glass, self.fog, self.haze), face, 0.0);
        let dark = rng.f64() < self.dark;
        let bright = if rng.f64() < 0.35 { rng.range(0.76, 0.84) } else { 1.0 };
        let (mut out, mut unlit) = (vec![], vec![]);
        let (mut k, mut y1) = (0i64, (hz - lobby as f64).round() as i64);
        loop {
            let y0 = y1 - height;
            let here: Vec<&Tier> = tiers.iter().filter(|t| t.2 + parapet <= y0).collect();
            if here.is_empty() {
                break;
            }
            let mut best = here[0];
            for t in &here[1..] {
                if t.1 - t.0 < best.1 - best.0 {
                    best = t;
                }
            }
            let (a, b, _) = *best;
            let margin = ((0.5 * bay as f64).round() as i64).max(1);
            let first = (((a + margin) as f64 - centre) / bay as f64 - 0.5).ceil() as i64;
            let last = (((b - margin) as f64 - centre) / bay as f64 - 0.5).floor() as i64;
            for cc in first..=last {
                if facade == "paired" && cc.rem_euclid(3) == 2 {
                    continue;
                }
                let x0 = (centre + (cc as f64 + 0.5) * bay as f64 - width as f64 / 2.0).round() as i64;
                let x1 = x0 + width;
                if later.iter().any(|&(ta, tb, tt)| ta < x1 && x0 < tb && tt < y1) {
                    continue;
                }
                unlit.push((x0, y0, x1, y1));
                if !dark {
                    out.push((x0, y0, x1, y1, bright * (0.96 + 0.08 * rng.f64()), k));
                }
            }
            k += 1;
            y1 -= floor;
        }
        c.set_source_rgb(glass.0, glass.1, glass.2);
        for &(x0, y0, x1, y1) in &unlit {
            c.rectangle(x0 as f64, y0 as f64, (x1 - x0) as f64, (y1 - y0) as f64);
        }
        let _ = c.fill();
        out
    }
}

impl Part for Skyline {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::City)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let top = self.plan(g).1;
        Some(vec![(0.0, (top - 4).max(0) as f64, g.w as f64, g.th.horizon_y(g.h).ceil() + 2.0)])
    }
    fn paint(&mut self, g: &Geo, layer: &Context, s: &mut Shared) {
        let (ww, u) = (g.w as f64, unit(g.w, g.h));
        let hz = g.th.horizon_y(g.h);
        let (buildings, self_top) = self.plan(g).clone();
        let own = surface(g.w, g.h);
        let c = ctx(&own);
        let body = mixed(self.colour, self.fog, self.haze);
        let rim = self.rim;
        let mut rng = Rng::new(self.seed * 31 + 5);
        let (mut windows, mut tips): (Vec<Pane>, Vec<(f64, f64)>) = (vec![], vec![]);
        let sun_x = sun(&g.th).0 * ww;
        for (n, bld) in buildings.iter().enumerate() {
            let k = 0.85 + 0.3 * bld.shade;
            let face = (body.0 * k, body.1 * k, body.2 * k);
            c.set_source_rgb(face.0, face.1, face.2);
            for &(a, b, t) in &bld.tiers {
                c.rectangle(a as f64, t as f64, (b - a) as f64, hz - t as f64 + 1.0);
            }
            let _ = c.fill();
            let (a, b, t) = *bld.tiers.last().unwrap();
            let (af, bf, tf) = (a as f64, b as f64, t as f64);
            let (mid, span) = ((a + b) as f64 / 2.0, (b - a) as f64);
            c.set_line_width((u / 900.0).max(1.0));
            match bld.crown {
                "mast" => {
                    let tip = tf - rng.range(0.03, 0.07) * u;
                    c.move_to(mid, tf);
                    c.line_to(mid, tip);
                    let _ = c.stroke();
                    tips.push((mid, tip));
                }
                "spire" => {
                    c.move_to(af + 0.3 * span, tf);
                    c.line_to(mid, tf - rng.range(0.5, 0.9) * span);
                    c.line_to(bf - 0.3 * span, tf);
                    let _ = c.fill();
                    let tip = tf - rng.range(1.0, 1.6) * span;
                    c.move_to(mid, tf - 0.5 * span);
                    c.line_to(mid, tip);
                    let _ = c.stroke();
                    tips.push((mid, tip));
                }
                "slant" => {
                    c.move_to(af, tf);
                    c.line_to(bf, tf - rng.range(0.25, 0.5) * span);
                    c.line_to(bf, tf);
                    let _ = c.fill();
                }
                "tank" => {
                    let tw = 0.25 * span;
                    let tx = rng.range(af + 2.0, bf - tw - 2.0);
                    c.rectangle(tx, tf - 0.6 * tw, tw, 0.45 * tw);
                    c.rectangle(tx + 0.15 * tw, tf - 0.15 * tw, 0.1 * tw, 0.15 * tw);
                    c.rectangle(tx + 0.75 * tw, tf - 0.15 * tw, 0.1 * tw, 0.15 * tw);
                    let _ = c.fill();
                }
                "box" => {
                    c.rectangle(af + 0.15 * span, tf - 0.08 * span, 0.35 * span, 0.08 * span);
                    let _ = c.fill();
                }
                _ => {}
            }
            // pilasters
            let step = ((rng.range(0.004, 0.009) * u).round() as i64).max(3);
            c.set_source_rgba(rim.0, rim.1, rim.2, 0.035 + 0.03 * (1.0 - self.haze));
            for &(ta, tb, tt) in &bld.tiers {
                let mut px = ta + step;
                while px < tb - 1 {
                    c.rectangle(px as f64, (tt + 2) as f64, 1.0, hz - tt as f64 - 2.0);
                    px += step;
                }
            }
            let _ = c.fill();
            // the edge the moon is on
            let side = if (a + b) as f64 / 2.0 > sun_x { a } else { b - 1 };
            c.set_source_rgba(rim.0, rim.1, rim.2, 0.10 + 0.12 * (1.0 - self.haze));
            for &(ta, tb, tt) in &bld.tiers {
                c.rectangle(if side == a { ta } else { tb - 1 } as f64, tt as f64, 1.0, hz - tt as f64);
                c.rectangle(ta as f64, tt as f64, (tb - ta) as f64, 1.0);
            }
            let _ = c.fill();
            let later: Vec<Tier> = buildings[n + 1..(n + 4).min(buildings.len())].iter().flat_map(|b| b.tiers.iter().cloned()).collect();
            for w in self.glazing(g, &c, &bld.tiers, &mut rng, &later, face) {
                windows.push((w.0, w.1, w.2, w.3, w.4, w.5, n));
            }
        }
        let fog = LinearGradient::new(0.0, self_top as f64, 0.0, hz);
        let f = self.fog;
        fog.add_color_stop_rgba(0.0, f.0, f.1, f.2, 0.0);
        fog.add_color_stop_rgba(0.55, f.0, f.1, f.2, 0.05 + 0.25 * self.haze);
        fog.add_color_stop_rgba(1.0, f.0, f.1, f.2, 0.45 + 0.3 * self.haze);
        c.set_operator(Operator::Atop);
        let _ = c.set_source(&fog);
        let _ = c.paint();
        c.set_operator(Operator::Over);
        drop(c);
        s.put_named(&self.name, SkylineData { surface: own.clone(), windows, buildings, haze: self.haze, tips });
        let _ = layer.set_source_surface(&own, 0.0, 0.0);
        let _ = layer.paint();
    }
}
