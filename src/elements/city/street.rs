//! The street along the quay, its lamps and its traffic.

use super::*;

// --- the street ------------------------------------------------------------------------------------

/// The waterfront: a quay, lamp posts, and water mirroring the skylines.
pub struct Street {
    pub(super) colour: Rgb,
    pub(super) sheen: Rgb,
    pub(super) mirror: Vec<String>,
    pub(super) reflect: f64,
    pub(super) moon: Rgb,
    pub(super) glimmer: f64,
    pub(super) quay: f64,
    pub(super) stone: Rgb,
    pub(super) lamps: Rgb,
    pub(super) spacing: f64,
    pub(super) sand: Rgb,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StreetSettings {
    pub(super) colour: Colour,
    pub(super) sheen: Colour,
    pub(super) mirror: Vec<String>,
    pub(super) reflect: f64,
    pub(super) moon: Colour,
    pub(super) glimmer: f64,
    pub(super) quay: f64,
    pub(super) stone: Colour,
    pub(super) lamps: Colour,
    pub(super) spacing: f64,
    pub(super) sand: Colour,
}

impl Validate for StreetSettings {
    fn validate(&self) -> Result<(), String> {
        within("reflect", self.reflect, 0.0, 1.0)?;
        within("glimmer", self.glimmer, 0.0, 1.0)?;
        within("quay", self.quay, 0.01, 0.5)?;
        within("spacing", self.spacing, 0.01, 10.0)
    }
}

impl Street {
    pub(super) fn new(st: StreetSettings) -> Self {
        Street {
            colour: st.colour.rgb(),
            sheen: st.sheen.rgb(),
            mirror: st.mirror,
            reflect: st.reflect,
            moon: st.moon.rgb(),
            glimmer: st.glimmer,
            quay: st.quay,
            stone: st.stone.rgb(),
            lamps: st.lamps.rgb(),
            spacing: st.spacing,
            sand: st.sand.rgb(),
        }
    }

    pub(super) fn waterline(&self, g: &Geo, rng: &mut Rng, wl: f64) -> Vec<(f64, f64)> {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let k = (u / 1080.0).max(1.0);
        let phases: Vec<f64> = (0..3).map(|_| rng.range(0.0, 2.0 * PI)).collect();
        let (mut points, mut x) = (vec![], 0.0);
        while x <= w + 4.0 {
            let mut y =
                wl + k * (1.1 * (x / (0.07 * u) + phases[0]).sin() + 0.6 * (x / (0.017 * u) + phases[1]).sin() + 0.35 * (x / (0.006 * u) + phases[2]).sin());
            if rng.f64() < 0.06 {
                y -= k * rng.range(0.8, 1.8);
            }
            points.push((x, y));
            x += (u / 400.0).max(2.0);
        }
        points
    }

    pub(super) fn lamps_at(&self, g: &Geo) -> Vec<f64> {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let mut rng = Rng::new(29);
        let (mut xs, mut x) = (vec![], 0.0);
        loop {
            x += self.spacing * u * rng.range(0.8, 1.2);
            if x > w {
                return xs;
            }
            xs.push(x.round());
        }
    }

    pub(super) fn shoreline(&self, g: &Geo, c: &Context, rng: &mut Rng, face: f64, foot: &[(f64, f64)]) {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let k = (u / 1080.0).max(1.0);
        let lamps = self.lamps_at(g);
        let step_x = foot[1].0 - foot[0].0;
        let at = |x: f64| foot[((x / step_x) as i64).max(0).min(foot.len() as i64 - 1) as usize].1;
        let warmth = |x: f64| {
            let mut best: Option<f64> = None;
            for &lx in &lamps {
                let v = (-((x - lx) / (0.028 * u)).powf(2.0)).exp();
                if best.is_none_or(|b| v > b) {
                    best = Some(v);
                }
            }
            best.unwrap_or(0.0)
        };
        let mut x = rng.range(0.0, 0.05) * u;
        while x < w {
            let kind = rng.weighted(&[0.5, 0.2, 0.3]);
            let length = rng.range(0.02, if kind == 2 { 0.07 } else { 0.2 }) * u;
            if kind == 0 {
                self.rocks(c, rng, x, (w + 8.0).min(x + length), &at, &warmth, k);
            } else if kind == 1 {
                self.beach(c, rng, x, (w + 8.0).min(x + length), &at, &warmth, k, face);
            }
            x += length;
        }
    }

    pub(super) fn rocks(&self, c: &Context, rng: &mut Rng, a: f64, b: f64, at: &dyn Fn(f64) -> f64, warmth: &dyn Fn(f64) -> f64, k: f64) {
        let mut cx = a;
        while cx < b {
            let taper = (PI * (cx - a) / (b - a)).sin().powf(0.5);
            let w = rng.range(4.0, 10.0) * k * (0.4 + 0.6 * taper);
            let mut piles = vec![(cx, at(cx) + rng.range(0.0, 1.0) * k, w)];
            if taper > 0.4 && rng.f64() < 0.45 {
                let px = cx + rng.range(-0.3, 0.3) * w;
                let base = at(cx) - 0.45 * w;
                let pw = w * rng.range(0.5, 0.75);
                piles.push((px, base, pw));
            }
            for (px, base, pw) in piles {
                let h = pw * rng.range(0.45, 0.8);
                let n = rng.int(6, 8);
                let mut pts = vec![];
                for i in 0..n {
                    let t = 2.0 * PI * i as f64 / n as f64 + rng.range(-0.25, 0.25);
                    let r = rng.range(0.75, 1.0);
                    pts.push((px + t.cos() * pw / 2.0 * r, base.min(base - h / 2.0 + t.sin() * h / 2.0 * r)));
                }
                let lit = warmth(px);
                let body = mixed(mixed(self.stone, BLACK, rng.range(0.25, 0.6)), self.lamps, 0.12 * lit);
                let rim = mixed(mixed(self.stone, self.sheen, 0.5), self.lamps, 0.35 * lit);
                for (dy, colour) in [(0.0, rim), ((0.8 * k).max(1.0), body)] {
                    for (i, &(qx, qy)) in pts.iter().enumerate() {
                        let y = base.min(qy + dy);
                        if i > 0 { c.line_to(qx, y) } else { c.move_to(qx, y) }
                    }
                    c.close_path();
                    c.set_source_rgb(colour.0, colour.1, colour.2);
                    let _ = c.fill();
                }
            }
            cx += w * rng.range(0.55, 0.85);
        }
    }

    pub(super) fn beach(&self, c: &Context, rng: &mut Rng, a: f64, b: f64, at: &dyn Fn(f64) -> f64, warmth: &dyn Fn(f64) -> f64, k: f64, face: f64) {
        let tall = face * rng.range(0.3, 0.55);
        let phase = rng.range(0.0, 2.0 * PI);
        let xs: Vec<f64> = (0..41).map(|i| a + (b - a) * i as f64 / 40.0).collect();
        let crest = |x: f64| {
            let f = (x - a) / (b - a);
            at(x) - tall * (PI * f).sin().powf(0.7) * (1.0 + 0.12 * (9.0 * f + phase).sin())
        };
        for (i, &x) in xs.iter().enumerate() {
            if i > 0 { c.line_to(x, crest(x)) } else { c.move_to(x, crest(x)) }
        }
        for &x in xs.iter().rev() {
            c.line_to(x, at(x) + 1.0);
        }
        c.close_path();
        let top = xs.iter().map(|&x| crest(x)).fold(f64::INFINITY, f64::min);
        let bottom = xs.iter().map(|&x| at(x)).fold(f64::NEG_INFINITY, f64::max) + 1.0;
        let gr = LinearGradient::new(0.0, top, 0.0, bottom);
        let s0 = mixed(self.sand, self.lamps, 0.3 * warmth((a + b) / 2.0));
        gr.add_color_stop_rgb(0.0, s0.0, s0.1, s0.2);
        let s1 = mixed(self.sand, BLACK, 0.35);
        gr.add_color_stop_rgb(0.65, s1.0, s1.1, s1.2);
        let s2 = mixed(self.sand, BLACK, 0.6);
        gr.add_color_stop_rgb(1.0, s2.0, s2.1, s2.2);
        let _ = c.save();
        c.clip_preserve();
        let _ = c.set_source(&gr);
        let _ = c.fill();
        let n = ((b - a) * tall / (6.0 * k * k)).round() as i64;
        for _ in 0..n {
            let x = rng.range(a, b);
            let y = rng.range(crest(x), at(x));
            let light = rng.f64() < 0.5;
            c.rectangle(x.round(), y.round(), 1.0, 1.0);
            let col = if light { mixed(mixed(self.sand, WHITE, 0.1), self.lamps, 0.25 * warmth(x)) } else { mixed(self.sand, BLACK, 0.5) };
            c.set_source_rgba(col.0, col.1, col.2, rng.range(0.2, 0.45));
            let _ = c.fill();
        }
        let _ = c.restore();
        for (i, &x) in xs.iter().enumerate() {
            if i > 0 { c.line_to(x, at(x) - 0.5) } else { c.move_to(x, at(x) - 0.5) }
        }
        c.set_line_width(1.0);
        let e = mixed(self.sheen, self.lamps, 0.5 * warmth((a + b) / 2.0));
        c.set_source_rgba(e.0, e.1, e.2, 0.6);
        let _ = c.stroke();
    }

    pub(super) fn lamplight(&self, g: &Geo, c: &Context, rng: &mut Rng, wl: f64) {
        let (h, u) = (g.h as f64, unit(g.w, g.h));
        let lamp = self.lamps;
        for x in self.lamps_at(g) {
            let length = (0.09 + 0.05 * rng.f64()) * u;
            let w = 0.005 * u;
            let column = LinearGradient::new(0.0, wl, 0.0, wl + length);
            column.add_color_stop_rgba(0.0, lamp.0, lamp.1, lamp.2, 0.24);
            column.add_color_stop_rgba(1.0, lamp.0, lamp.1, lamp.2, 0.0);
            let soft = LinearGradient::new(x - w, 0.0, x + w, 0.0);
            soft.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            soft.add_color_stop_rgba(0.5, 0.0, 0.0, 0.0, 1.0);
            soft.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
            let _ = c.save();
            c.rectangle(x - w, wl, 2.0 * w, length);
            c.clip();
            let _ = c.set_source(&column);
            let _ = c.mask(&soft);
            let _ = c.restore();
            let mut y = wl + 1.0;
            while y < h.min(wl + length) {
                let d = (y - wl) / length;
                let mut wide = (0.002 + 0.007 * d) * u * rng.range(0.35, 1.3);
                if rng.f64() < 0.12 {
                    wide *= 2.2;
                }
                let cx = x + rng.normal(0.0, 0.6 + 2.2 * d);
                let tall = if rng.f64() < 0.75 { 1.0 } else { 2.0 };
                let gr = LinearGradient::new(cx - wide / 2.0, 0.0, cx + wide / 2.0, 0.0);
                gr.add_color_stop_rgba(0.0, lamp.0, lamp.1, lamp.2, 0.0);
                let m = mixed(self.lamps, WHITE, 0.25 * (1.0 - d));
                gr.add_color_stop_rgba(0.5, m.0, m.1, m.2, 0.8 * (1.0 - d).powf(1.4) * rng.range(0.55, 1.0));
                gr.add_color_stop_rgba(1.0, lamp.0, lamp.1, lamp.2, 0.0);
                let _ = c.set_source(&gr);
                c.rectangle(cx - wide / 2.0, y.round(), wide, tall);
                let _ = c.fill();
                y += tall + (u / 540.0).max(1.0) * *rng.pick(&[1.0, 1.0, 1.5, 2.0, 3.0]) * (1.0 + 1.5 * d);
            }
        }
    }

    pub(super) fn quayside(&self, g: &Geo, c: &Context, s: &mut Shared, rng: &mut Rng, hz: f64, wl: f64) {
        let (w, u) = (g.w as f64, unit(g.w, g.h));
        let top = hz.floor();
        let coping = (u / 360.0).round().max(2.0);
        let foot = self.waterline(g, rng, wl);
        s.put(&SHORE, foot.clone());
        let face = LinearGradient::new(0.0, top, 0.0, wl);
        let st = self.stone;
        face.add_color_stop_rgb(0.0, st.0, st.1, st.2);
        let m = mixed(self.stone, BLACK, 0.3);
        face.add_color_stop_rgb(1.0, m.0, m.1, m.2);
        c.move_to(0.0, top);
        c.line_to(w, top);
        for &(x, y) in foot.iter().rev() {
            c.line_to(x, y);
        }
        c.close_path();
        let _ = c.save();
        c.clip_preserve();
        let _ = c.set_source(&face);
        let _ = c.fill();
        let m = mixed(self.stone, BLACK, 0.45);
        c.set_source_rgba(m.0, m.1, m.2, 1.0);
        let mut x = rng.range(0.0, 0.03) * u;
        while x < w {
            c.rectangle(x.round(), top + coping, 1.0, wl + 4.0 - top - coping);
            x += 0.03 * u * rng.range(0.7, 1.3);
        }
        let _ = c.fill();
        let _ = c.restore();
        c.rectangle(0.0, top, w, coping);
        let m = mixed(self.stone, self.sheen, 0.45);
        c.set_source_rgb(m.0, m.1, m.2);
        let _ = c.fill();
        let l = self.lamps;
        for x in self.lamps_at(g) {
            let spot = LinearGradient::new(x - 0.03 * u, 0.0, x + 0.03 * u, 0.0);
            spot.add_color_stop_rgba(0.0, l.0, l.1, l.2, 0.0);
            spot.add_color_stop_rgba(0.5, l.0, l.1, l.2, 0.45);
            spot.add_color_stop_rgba(1.0, l.0, l.1, l.2, 0.0);
            c.rectangle(x - 0.03 * u, top, 0.06 * u, coping);
            let _ = c.set_source(&spot);
            let _ = c.fill();
            let _ = c.save();
            c.translate(x, top + coping);
            c.scale(0.03 * u, (wl - top - coping) * 0.9);
            let wash = RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
            wash.add_color_stop_rgba(0.0, l.0, l.1, l.2, 0.2);
            wash.add_color_stop_rgba(1.0, l.0, l.1, l.2, 0.0);
            c.rectangle(-1.0, 0.0, 2.0, 1.0);
            let _ = c.set_source(&wash);
            let _ = c.fill();
            let _ = c.restore();
        }
        c.rectangle(0.0, top, w, 1.0);
        let m = mixed(self.sheen, WHITE, 0.45);
        c.set_source_rgba(m.0, m.1, m.2, 0.8);
        let _ = c.fill();
        for (k, &(x, y)) in foot.iter().enumerate() {
            if k > 0 { c.line_to(x, y) } else { c.move_to(x, y) }
        }
        c.set_line_width(1.0);
        let m = mixed(self.sheen, WHITE, 0.2);
        c.set_source_rgba(m.0, m.1, m.2, 0.7);
        let _ = c.stroke();
        self.shoreline(g, c, rng, wl - top, &foot);
        let (height, r) = (0.024 * u, (0.0022 * u).max(1.0));
        let thick = (u / 540.0).round().max(2.0);
        for x in self.lamps_at(g) {
            let gr = LinearGradient::new(0.0, top - height, 0.0, top);
            let m = mixed(self.stone, self.lamps, 0.55);
            gr.add_color_stop_rgb(0.0, m.0, m.1, m.2);
            let m = mixed(self.stone, self.lamps, 0.25);
            gr.add_color_stop_rgb(0.35, m.0, m.1, m.2);
            let m = mixed(self.stone, self.sheen, 0.35);
            gr.add_color_stop_rgb(1.0, m.0, m.1, m.2);
            c.rectangle(x - thick / 2.0, top - height + r, thick, height - r);
            let _ = c.set_source(&gr);
            let _ = c.fill();
            c.rectangle(x - 2.2 * r, top - height - 1.6 * r, 4.4 * r, (0.7 * r).max(1.0));
            let m = mixed(self.stone, self.lamps, 0.4);
            c.set_source_rgb(m.0, m.1, m.2);
            let _ = c.fill();
            let glow = RadialGradient::new(x, top - height, 0.0, x, top - height, 9.0 * r);
            glow.add_color_stop_rgba(0.0, l.0, l.1, l.2, 0.55);
            glow.add_color_stop_rgba(0.3, l.0, l.1, l.2, 0.18);
            glow.add_color_stop_rgba(1.0, l.0, l.1, l.2, 0.0);
            c.rectangle(x - 9.0 * r - 1.0, top - height - 9.0 * r - 1.0, 18.0 * r + 2.0, 18.0 * r + 2.0);
            let _ = c.set_source(&glow);
            let _ = c.fill();
            c.arc(x, top - height, r, 0.0, 2.0 * PI);
            let m = mixed(self.lamps, WHITE, 0.5);
            c.set_source_rgb(m.0, m.1, m.2);
            let _ = c.fill();
        }
    }
}

impl Part for Street {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::City)
    }
    fn area(&self, g: &Geo) -> Option<Vec<Rect>> {
        let u = unit(g.w, g.h);
        Some(vec![(0.0, (g.th.horizon_y(g.h) - 0.04 * u).floor(), g.w as f64, g.h as f64)])
    }
    fn paint(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let (w, h, u) = (g.w as f64, g.h as f64, unit(g.w, g.h));
        let hz = g.th.horizon_y(g.h);
        let wl = (hz + self.quay * u).round();
        s.put(&WATERLINE, wl);
        let gr = LinearGradient::new(0.0, wl, 0.0, h);
        let sh = self.sheen;
        gr.add_color_stop_rgb(0.0, sh.0, sh.1, sh.2);
        let m = mixed(self.sheen, self.colour, 0.7);
        gr.add_color_stop_rgb(0.18, m.0, m.1, m.2);
        gr.add_color_stop_rgb(1.0, self.colour.0, self.colour.1, self.colour.2);
        c.rectangle(0.0, wl - 4.0, w, h - wl + 4.0);
        let _ = c.set_source(&gr);
        let _ = c.fill();
        let mirror = surface(g.w, g.h);
        {
            let mc = ctx(&mirror);
            for name in &self.mirror {
                if let Some(data) = s.get_named::<SkylineData>(name) {
                    let _ = mc.save();
                    mc.translate(0.0, 2.0 * wl);
                    mc.scale(1.0, -1.0);
                    let _ = mc.set_source_surface(&data.surface, 0.0, 0.0);
                    let _ = mc.paint();
                    let _ = mc.restore();
                }
            }
            mc.rectangle(0.0, wl, w, wl - hz);
            mc.set_source_rgb(self.stone.0, self.stone.1, self.stone.2);
            let _ = mc.fill();
        }
        let smear = resampled(&resampled(&mirror, 1.0 / 6.0, Filter::Good), 6.0, Filter::Bilinear);
        let rippled = surface(g.w, g.h);
        let mut rng = Rng::new(11);
        {
            let rc = ctx(&rippled);
            let band = (u / 360.0).round().max(2.0);
            let mut y = wl;
            while y < h {
                let shift = ((y * 0.21).sin() * 1.5 + rng.range(-1.5, 1.5) * (y - wl) / (h - wl)).round();
                rc.rectangle(0.0, y, w, band);
                let _ = rc.set_source_surface(&smear, shift, 0.0);
                let _ = rc.fill();
                y += band;
            }
        }
        let fade = LinearGradient::new(0.0, wl, 0.0, h);
        fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, self.reflect);
        fade.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, self.reflect * 0.35);
        fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        let _ = c.save();
        c.rectangle(0.0, wl, w, h - wl);
        c.clip();
        let _ = c.set_source_surface(&rippled, 0.0, 0.0);
        let _ = c.mask(&fade);
        self.lamplight(g, c, &mut rng, wl);
        let (sx, sr) = sun(&g.th);
        let (mx, r) = (sx * w, sr * u);
        let mut y = wl + 3.0;
        let mo = self.moon;
        while y < h {
            let d = (y - wl) / (h - wl);
            for _ in 0..rng.int(1, 3) {
                let ww = r * (0.08 + 0.35 * d) * rng.range(0.3, 1.0);
                let x = mx + rng.normal(0.0, 0.22 + 0.4 * d) * r;
                let a = self.glimmer * (1.0 - d).powf(2.0) * rng.range(0.2, 1.0) * (-((x - mx) / (0.6 * r)).powf(2.0)).exp();
                let gr = LinearGradient::new(x - ww / 2.0, 0.0, x + ww / 2.0, 0.0);
                gr.add_color_stop_rgba(0.0, mo.0, mo.1, mo.2, 0.0);
                gr.add_color_stop_rgba(0.5, mo.0, mo.1, mo.2, a);
                gr.add_color_stop_rgba(1.0, mo.0, mo.1, mo.2, 0.0);
                let _ = c.set_source(&gr);
                c.rectangle(x - ww / 2.0, y.round(), ww, (u / 900.0).round().max(1.0));
                let _ = c.fill();
            }
            y += (u / 260.0 * (1.0 + 3.0 * d)).max(2.0) * rng.range(0.6, 1.8);
        }
        let _ = c.restore();
        self.quayside(g, c, s, &mut rng, hz, wl);
    }
}
