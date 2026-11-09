//! The night sky and the moon.

use super::*;

// --- the sky ---------------------------------------------------------------------------------------

/// The sky over a city at night: near black overhead, glowing toward the horizon, a few faint stars.
pub struct NightSky {
    pub(super) zenith: Rgb,
    pub(super) middle: Rgb,
    pub(super) horizon: Rgb,
    pub(super) glow: f64,
    pub(super) stars: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NightSkySettings {
    pub(super) zenith: Colour,
    pub(super) middle: Colour,
    pub(super) horizon: Colour,
    pub(super) glow: f64,
    pub(super) stars: f64,
}

impl Validate for NightSkySettings {
    fn validate(&self) -> Result<(), String> {
        within("glow", self.glow, 0.0, 1.0)?;
        within("stars", self.stars, 0.0, 100000.0)
    }
}

impl NightSky {
    pub(super) fn new(st: NightSkySettings) -> Self {
        NightSky { zenith: st.zenith.rgb(), middle: st.middle.rgb(), horizon: st.horizon.rgb(), glow: st.glow, stars: st.stars }
    }
}

impl Part for NightSky {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, c: &Context, _s: &mut Shared) {
        let (w, h, hz) = (g.w as f64, g.h as f64, g.th.horizon_y(g.h));
        let gr = LinearGradient::new(0.0, 0.0, 0.0, hz);
        gr.add_color_stop_rgb(0.0, self.zenith.0, self.zenith.1, self.zenith.2);
        gr.add_color_stop_rgb(1.0 - self.glow, self.middle.0, self.middle.1, self.middle.2);
        gr.add_color_stop_rgb(1.0, self.horizon.0, self.horizon.1, self.horizon.2);
        let _ = c.set_source(&gr);
        let _ = c.paint();
        let mut rng = Rng::new(7);
        let u = unit(g.w, g.h);
        let n = (self.stars * w * h / (1920.0 * 1080.0)).round() as i64;
        let star = mixed(WHITE, self.middle, 0.2);
        for _ in 0..n {
            let x = rng.range(0.0, w);
            let y = hz * rng.range(0.0, 0.55).powf(1.4);
            let a = rng.range(0.15, 0.55) * (1.0 - y / (0.6 * hz));
            c.set_source_rgba(star.0, star.1, star.2, a);
            let r = (u / 1400.0).max(0.6) * *rng.pick(&[1.0, 1.0, 1.0, 1.6]);
            c.arc(x, y, r, 0.0, 2.0 * PI);
            let _ = c.fill();
        }
    }
}

// --- the moon --------------------------------------------------------------------------------------

/// A big low moon in a wide halo, with seas and wisps of cloud.
pub struct Moon {
    pub(super) colour: Rgb,
    pub(super) shade: Rgb,
    pub(super) halo: f64,
    pub(super) rise: f64,
    pub(super) clouds: Rgb,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MoonSettings {
    pub(super) colour: Colour,
    pub(super) shade: Colour,
    pub(super) halo: f64,
    pub(super) rise: f64,
    pub(super) clouds: Colour,
}

impl Validate for MoonSettings {
    fn validate(&self) -> Result<(), String> {
        within("halo", self.halo, 0.0, 1.0)?;
        within("rise", self.rise, -4.0, 4.0)
    }
}

impl Moon {
    pub(super) fn new(st: MoonSettings) -> Self {
        Moon { colour: st.colour.rgb(), shade: st.shade.rgb(), halo: st.halo, rise: st.rise, clouds: st.clouds.rgb() }
    }
}

impl Part for Moon {
    fn still(&self) -> bool {
        true
    }
    fn layer(&self) -> Option<Layer> {
        Some(Layer::Back)
    }
    fn paint(&mut self, g: &Geo, ctx_: &Context, _s: &mut Shared) {
        let (w, h, u) = (g.w, g.h, unit(g.w, g.h));
        let (sx, sr) = sun(&g.th);
        let (cx, r) = (sx * w as f64, sr * u);
        let cy = g.th.horizon_y(h) - self.rise * r;
        let halo = RadialGradient::new(cx, cy, r * 0.9, cx, cy, r * 3.2);
        let col = self.colour;
        for k in 0..9 {
            let t = k as f64 / 8.0;
            halo.add_color_stop_rgba(t, col.0, col.1, col.2, self.halo * (1.0 - t).powf(2.2));
        }
        let _ = ctx_.set_source(&halo);
        let _ = ctx_.paint();
        let (shade, colour) = (self.shade, self.colour);
        let disc = |c: &Context| {
            let face = RadialGradient::new(cx - 0.35 * r, cy - 0.4 * r, 0.1 * r, cx, cy, r);
            face.add_color_stop_rgb(0.0, colour.0, colour.1, colour.2);
            let m = mixed(colour, shade, 0.35);
            face.add_color_stop_rgb(0.75, m.0, m.1, m.2);
            let m = mixed(colour, shade, 0.8);
            face.add_color_stop_rgb(1.0, m.0, m.1, m.2);
            c.arc(cx, cy, r, 0.0, 2.0 * PI);
            let _ = c.set_source(&face);
            let _ = c.fill_preserve();
            let _ = c.save();
            c.clip();
            let mut rng = Rng::new(3);
            for _ in 0..9 {
                let mx = cx + rng.range(-0.6, 0.6) * r;
                let my = cy + rng.range(-0.6, 0.5) * r;
                let _ = c.save();
                c.translate(mx, my);
                let sx = rng.range(0.12, 0.3) * r;
                let sy = rng.range(0.08, 0.22) * r;
                c.scale(sx, sy);
                c.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI);
                let _ = c.restore();
                c.set_source_rgba(shade.0, shade.1, shade.2, rng.range(0.10, 0.2));
                let _ = c.fill();
            }
            let _ = c.restore();
        };
        let bbox = traced(|c| c.arc(cx, cy, r, 0.0, 2.0 * PI));
        let _ = ctx_.set_source_surface(supersampled(w, h, g.scale, disc, Some(bbox)), 0.0, 0.0);
        let _ = ctx_.paint();
        let k = 8;
        let wisps = surface((w / k).max(1), (h / k).max(1));
        {
            let wc = ctx(&wisps);
            wc.scale(1.0 / k as f64, 1.0 / k as f64);
            let mut rng = Rng::new(9);
            for i in 0..4 {
                let y = cy - (0.75 - 0.3 * i as f64) * r + rng.range(-0.08, 0.08) * r;
                let x0 = cx - rng.range(1.4, 2.6) * r;
                let x1 = cx + rng.range(1.2, 2.4) * r;
                let thick = rng.range(0.05, 0.11) * r;
                wc.move_to(x0, y);
                wc.curve_to(cx - r, y - thick, cx + r, y + thick, x1, y - 0.5 * thick);
                wc.curve_to(cx + r, y + 2.2 * thick, cx - r, y + 1.2 * thick, x0, y + 0.6 * thick);
                wc.close_path();
                let cl = self.clouds;
                wc.set_source_rgba(cl.0, cl.1, cl.2, rng.range(0.3, 0.5));
                let _ = wc.fill();
            }
        }
        let _ = ctx_.set_source_surface(resampled(&wisps, k as f64, Filter::Bilinear), 0.0, 0.0);
        let _ = ctx_.paint();
    }
}
