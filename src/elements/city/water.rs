//! The water: the waves at the quay and the city's reflections.

use super::*;

// --- the waves at the quay -------------------------------------------------------------------------

/// Waves at the quay (live): swell crests along the edge and foam flecks. A seamless loop of `loop`
/// seconds at `rate` steps a second.
pub struct Shore {
    pub(super) loop_: f64,
    pub(super) rate: f64,
    pub(super) alpha: f64,
    pub(super) foam: f64,
    pub(super) colour: Rgb,
    pub(super) steps: i64,
    pub(super) flecks: Vec<(f64, i64, f64, f64)>,
    pub(super) shore: Vec<(f64, f64)>,
    pub(super) width: f64,
    pub(super) step: Option<i64>,
    pub(super) cache: HashMap<i64, Vec<f32>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ShoreSettings {
    #[serde(rename = "loop")]
    pub(super) loop_: f64,
    pub(super) rate: f64,
    pub(super) alpha: f64,
    pub(super) foam: f64,
    pub(super) colour: Colour,
}

impl Validate for ShoreSettings {
    fn validate(&self) -> Result<(), String> {
        within("loop", self.loop_, 0.5, 120.0)?;
        within("rate", self.rate, 1.0, 60.0)?;
        within("loop x rate", self.loop_ * self.rate, 1.0, 3600.0)?;
        within("alpha", self.alpha, 0.0, 1.0)?;
        within("foam", self.foam, 0.0, 1000.0)
    }
}

impl Shore {
    pub(super) fn new(st: ShoreSettings) -> Self {
        Shore {
            loop_: st.loop_,
            rate: st.rate,
            alpha: st.alpha,
            foam: st.foam,
            colour: st.colour.rgb(),
            steps: 1,
            flecks: vec![],
            shore: vec![],
            width: 0.0,
            step: None,
            cache: HashMap::new(),
        }
    }

    pub(super) fn make(&mut self, g: &Geo, s: &Shared) {
        self.width = g.w as f64;
        self.shore = s.get(&SHORE).cloned().unwrap_or_default();
        self.steps = ((self.loop_ * self.rate).round() as i64).max(1);
        let mut rng = Rng::new(31);
        self.flecks.clear();
        for _ in 0..(self.foam * self.width / 1920.0).round() as i64 {
            let drift = rng.range(2.0, 6.0);
            let x = rng.range(drift, self.width - drift - 3.0);
            let size = *rng.pick(&[1, 2]);
            let phase = rng.f64();
            self.flecks.push((x, size, phase, drift));
        }
    }

    pub(super) fn swell(&self, x: f64, f: f64) -> f64 {
        (2.0 * PI * (x / (0.11 * self.width) - 2.0 * f)).sin()
    }

    pub(super) fn crests(&self, step: i64) -> Vec<(f64, f64, f64, f64)> {
        let f = step as f64 / self.steps as f64;
        let mut runs: Vec<(f64, f64, f64, f64)> = vec![];
        for p in self.shore.windows(2) {
            let ((x0, y0), (x1, _)) = (p[0], p[1]);
            let swell = self.swell(x0, f);
            if swell <= 0.3 {
                continue;
            }
            let y = (y0 + 1.0 - swell).round();
            let alpha = round2(self.alpha * ((swell - 0.3) / 0.7).powf(2.0));
            if alpha <= 0.0 {
                continue;
            }
            let (x0, x1) = (x0.round(), x1.round());
            match runs.last_mut() {
                Some(r) if r.1 == x0 && r.2 == y && r.3 == alpha => r.1 = x1,
                _ => runs.push((x0, x1, y, alpha)),
            }
        }
        runs
    }

    pub(super) fn foamed(&self, step: i64) -> Vec<(f64, f64, i64, f64)> {
        if self.shore.is_empty() {
            return vec![];
        }
        let f = step as f64 / self.steps as f64;
        let mut out = vec![];
        for &(where_, size, phase, drift) in &self.flecks {
            let x = (where_ + drift * (2.0 * PI * (f + phase)).sin()).round();
            let i = ((x / (self.shore[1].0 - self.shore[0].0)) as i64).min(self.shore.len() as i64 - 1);
            let y = self.shore[i as usize].1;
            let glow = (2.0 * PI * (f * 2.0 + phase)).sin().max(0.0);
            let y = (y + 1.0 - self.swell(x, f)).round();
            if glow > 0.05 {
                out.push((x, y, size, round2((1.5 * self.alpha).min(1.0) * glow)));
            }
        }
        out
    }

    pub(super) fn placed(&self, step: i64) -> Vec<f32> {
        let mut d = vec![];
        for (x0, x1, y, a) in self.crests(step) {
            d.extend(fill_quad(x0, y, x1, y + 1.0, self.colour, a).map(|v| v as f32));
        }
        for (x, y, size, a) in self.foamed(step) {
            d.extend(fill_quad(x, y, x + 1.0 + size as f64, y + 1.0, self.colour, a).map(|v| v as f32));
        }
        d
    }
}

impl Part for Shore {
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.make(g, s);
    }
    fn top(&self, g: &Geo) -> i32 {
        let m = self.shore.iter().map(|p| p.1).fold(None, |a: Option<f64>, y| Some(a.map_or(y, |a| a.min(y))));
        (m.unwrap_or(g.h as f64 + 3.0).floor() - 3.0) as i32
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.step = (!self.shore.is_empty()).then(|| ((w.clock * self.rate) as i64).rem_euclid(self.steps));
        let mut h = 0xcbf2_9ce4_8422_2325;
        opt(&mut h, self.step);
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        if let Some(step) = self.step {
            if !self.cache.contains_key(&step) {
                let d = self.placed(step);
                self.cache.insert(step, d);
            }
            gfx.draw(None, &self.cache[&step], Blend::Alpha);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        self.make(g, s);
        let d: Vec<f64> = self.placed(0).iter().map(|&v| v as f64).collect();
        paint_fills(c, &d);
    }
}

// --- the reflections -------------------------------------------------------------------------------

/// Lit windows mirrored on the water (live): a soft streak under each, `stretch` times as tall.
pub struct Reflections {
    pub(super) windows: String,
    pub(super) stretch: f64,
    pub(super) alpha: f64,
    pub(super) smear: Option<ImageSurface>,
    pub(super) tex: Option<Tex>,
    pub(super) sources: Option<Vec<(Vec<Option<[f64; 32]>>, Rc<Bundle>)>>,
    pub(super) streaks: Option<Vec<Lit>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReflectionsSettings {
    pub(super) windows: String,
    pub(super) stretch: f64,
    pub(super) alpha: f64,
}

impl Validate for ReflectionsSettings {
    fn validate(&self) -> Result<(), String> {
        within("stretch", self.stretch, 0.0, 100.0)?;
        within("alpha", self.alpha, 0.0, 1.0)
    }
}

impl Reflections {
    pub(super) fn new(st: ReflectionsSettings) -> Self {
        Reflections { windows: st.windows, stretch: st.stretch, alpha: st.alpha, smear: None, tex: None, sources: None, streaks: None }
    }

    pub(super) fn smear(&mut self) -> ImageSurface {
        if self.smear.is_none() {
            let (w, h) = (32, 128);
            let s = surface(w, h);
            {
                let c = ctx(&s);
                let mut rng = Rng::new(5);
                for y in 0..h {
                    let t = y as f64 / h as f64;
                    if rng.f64() < 0.18 * t {
                        continue;
                    }
                    let across = LinearGradient::new(0.0, 0.0, w as f64, 0.0);
                    for k in 0..9 {
                        let x = k as f64 / 8.0;
                        across.add_color_stop_rgba(x, 1.0, 1.0, 1.0, (-((x - 0.5) / 0.22).powf(2.0)).exp() * (1.0 - t).powf(1.5));
                    }
                    c.rectangle(0.0, y as f64, w as f64, 1.0);
                    let _ = c.set_source(&across);
                    let _ = c.fill();
                }
            }
            self.smear = Some(s);
        }
        self.smear.clone().unwrap()
    }

    pub(super) fn sources(&mut self, g: &Geo, s: &mut Shared) {
        if self.sources.is_some() {
            return;
        }
        let u = unit(g.w, g.h);
        let h = g.h as f64;
        let hz = s.get(&WATERLINE).copied().unwrap_or_else(|| g.th.horizon_y(g.h));
        let Some(b) = bundle(&self.windows, g, s) else {
            self.sources = Some(vec![]);
            return;
        };
        let mut quads = vec![];
        for w in &b.windows {
            let Some(span) = w.mirrored else {
                quads.push(None);
                continue;
            };
            let m0 = hz + (hz - w.y1 as f64);
            let length = (w.y1 - w.y0) as f64 * self.stretch + 0.012 * u;
            let ww = (2.4 * (span.1 - span.0) as f64).max(3.0);
            let cx = (span.0 + span.1) as f64 / 2.0 + (m0 * 0.13).sin() * 1.5;
            let fade = (1.0 - (m0 - hz) / (h - hz)).max(0.0);
            quads.push(Some(image_quad(cx - ww / 2.0, m0, cx + ww / 2.0, m0 + length, w.colour, self.alpha * fade, (0.0, 0.0, 1.0, 1.0))));
        }
        self.sources = Some(vec![(quads, b)]);
    }
}

impl Part for Reflections {
    fn live(&self) -> bool {
        true
    }
    fn start(&mut self, g: &Geo, _w: &mut World, s: &mut Shared) {
        self.sources(g, s);
    }
    fn textures(&mut self, _g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.tex = Some(up.add(self.smear(), Kind::Sprite));
    }
    fn top(&self, g: &Geo) -> i32 {
        g.th.horizon_y(g.h).floor() as i32
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        let c = w.clock;
        let mut h = 0xcbf2_9ce4_8422_2325;
        for (_, b) in self.sources.as_ref().unwrap() {
            let l = b.lights.borrow();
            mix(&mut h, l.version);
            opt(&mut h, l.dimming(c).then(|| (c * FPS).round() as i64));
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, w: &World, _p: f64) {
        let sources = self.sources.as_ref().unwrap();
        let streaks = self.streaks.get_or_insert_with(|| sources.iter().map(|(q, _)| Lit::new(q, true)).collect());
        let tex = self.tex;
        for (lit, (_, b)) in streaks.iter_mut().zip(sources.iter()) {
            let data = lit.current(&b.lights.borrow(), w.clock);
            gfx.draw(tex, data, Blend::Premultiplied);
        }
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        self.sources(g, s);
        let smear = self.smear();
        for (quads, b) in self.sources.as_ref().unwrap() {
            let mut l = b.lights.borrow_mut();
            l.at(0.0);
            let mut data: Vec<f32> = vec![];
            for (q, &on) in quads.iter().zip(l.lit.iter()) {
                if let (true, Some(q)) = (on, q) {
                    data.extend(q.map(|v| v as f32));
                }
            }
            paint_vertices(c, &smear, &data);
            l.c = None;
        }
    }
}
