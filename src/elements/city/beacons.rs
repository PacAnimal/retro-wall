//! The beacons blinking on the towers' tops.

use super::*;

// --- the beacons -----------------------------------------------------------------------------------

/// Aircraft-warning lights (live): slow red blinks on the tallest spires.
pub struct Beacons {
    pub(super) skylines: Vec<String>,
    pub(super) count: i64,
    pub(super) colour: Rgb,
    pub(super) period: f64,
    pub(super) lit: f64,
    pub(super) beacons: Vec<(f64, f64, f64)>,
    pub(super) size: f64,
    pub(super) on: Vec<bool>,
    // each beacon's sprite quad, and the lit ones' this frame
    pub(super) quads: Vec<Vec<f32>>,
    pub(super) data: Vec<f32>,
    pub(super) tex: Option<Tex>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BeaconsSettings {
    pub(super) skylines: Vec<String>,
    pub(super) count: i64,
    pub(super) colour: Colour,
    pub(super) period: f64,
    pub(super) lit: f64,
}

impl Validate for BeaconsSettings {
    fn validate(&self) -> Result<(), String> {
        within("count", self.count as f64, 0.0, 1000.0)?;
        within("period", self.period, 0.05, 3600.0)?;
        within("lit", self.lit, 0.0, 1.0)
    }
}

impl Beacons {
    pub(super) fn new(st: BeaconsSettings) -> Self {
        Beacons {
            skylines: st.skylines,
            count: st.count,
            colour: st.colour.rgb(),
            period: st.period,
            lit: st.lit,
            beacons: vec![],
            size: 0.0,
            on: vec![],
            quads: vec![],
            data: vec![],
            tex: None,
        }
    }

    pub(super) fn lights(&self, s: &Shared) -> Vec<(f64, f64, f64)> {
        let mut tips: Vec<(f64, f64)> = vec![];
        for name in &self.skylines {
            if let Some(d) = s.get_named::<SkylineData>(name) {
                tips.extend(d.tips.iter().cloned());
            }
        }
        tips.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut rng = Rng::new(41);
        tips.into_iter().take(self.count.max(0) as usize).map(|(x, y)| (x, y, rng.f64())).collect()
    }

    pub(super) fn make(&mut self, g: &Geo, s: &Shared) -> ImageSurface {
        let dot = soft_dot(32, &SOFT_DOT);
        self.beacons = self.lights(s);
        self.size = (0.012 * unit(g.w, g.h)).max(6.0);
        self.quads = self.beacons.iter().map(|&(x, y, _)| sprite_vertices(&[(x, y, self.size, 0.0, self.colour, 1.0)])).collect();
        dot
    }

    pub(super) fn on_at(&self, c: f64) -> Vec<bool> {
        self.beacons.iter().map(|&(_, _, phase)| self.lit_at(phase, c)).collect()
    }

    pub(super) fn lit_at(&self, phase: f64, c: f64) -> bool {
        (c / self.period + phase).rem_euclid(1.0) < self.lit
    }

    pub(super) fn placed(&self, on: &[bool]) -> Vec<f32> {
        let p: Vec<_> = self.beacons.iter().zip(on).filter(|(_, l)| **l).map(|(&(x, y, _), _)| (x, y, self.size, 0.0, self.colour, 1.0)).collect();
        sprite_vertices(&p)
    }
}

impl Part for Beacons {
    fn live(&self) -> bool {
        true
    }
    fn textures(&mut self, g: &Geo, s: &Shared, up: &mut Uploads) {
        self.tex = Some(up.add(self.make(g, s), Kind::Sprite));
    }
    fn top(&self, g: &Geo) -> i32 {
        let m = self.beacons.iter().map(|b| b.1).fold(g.h as f64, f64::min);
        (m - 0.02 * unit(g.w, g.h)).floor() as i32
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.on.resize(self.beacons.len(), false);
        for i in 0..self.beacons.len() {
            self.on[i] = self.lit_at(self.beacons[i].2, w.clock);
        }
        let mut h = 0xcbf2_9ce4_8422_2325;
        for &l in &self.on {
            mix(&mut h, l as u64);
        }
        h
    }
    fn draw(&mut self, _g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        self.data.clear();
        for (q, _) in self.quads.iter().zip(&self.on).filter(|(_, on)| **on) {
            self.data.extend_from_slice(q);
        }
        gfx.draw(self.tex, &self.data, Blend::Premultiplied);
    }
    fn rest(&mut self, g: &Geo, c: &Context, s: &mut Shared) {
        let dot = self.make(g, s);
        let d = self.placed(&self.on_at(0.0));
        paint_vertices(c, &dot, &d);
    }
}
