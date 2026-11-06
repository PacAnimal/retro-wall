//! The blocky theme's element: a block game's world as a player would screenshot it, looking down from
//! a hill over a farm: timber houses about a well, fields, pens of animals, a stream, a mine in the
//! hillside beyond, woods and mountains; the same each start, and following the time of day. A small
//! renderer of its own draws it once, on the CPU: the world laid out in blocks (`world`, `farm`,
//! `mobs`), their faces painted as pixel art (`textures`), and every pixel traced through the grid
//! (`render`) into what is seen and the light on it. Each frame the GPU lights that for the moment
//! (`sky`): the sky with its sun, moon and stars, the clouds drifting over, the torches flickering at
//! night. Over everything, the interface (`hud`): its hearts, food and experience follow readings of the
//! machine.

mod farm;
mod herd;
mod hud;
mod life;
mod mobs;
mod render;
mod sky;
mod textures;
mod world;

use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, ImageSurface};
use gl::types::{GLint, GLuint};
use render::{Camera, Tracer, V3};
use serde::Deserialize;
use sky::Moment;
use toml::Table;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    seed: i64,
    yaw: f64,
    pitch: f64,
    fov: f64,
    samples: f64,
    distance: f64,
    hand: bool,
    ui: bool,
    gui_scale: f64,
    clouds: f64,
    average: f64,
    sky: Colour,
    haze: Colour,
    health: String,
    food: String,
    experience: String,
    level: String,
}

impl Validate for Settings {
    fn validate(&self) -> Result<(), String> {
        within("yaw", self.yaw, -360.0, 360.0)?;
        within("pitch", self.pitch, -60.0, 60.0)?;
        within("fov", self.fov, 30.0, 110.0)?;
        within("samples", self.samples, 1.0, 4.0)?;
        within("distance", self.distance, 32.0, (world::R - 8) as f64)?;
        within("gui_scale", self.gui_scale, 0.0, 16.0)?;
        within("clouds", self.clouds, 0.0, 20.0)?;
        within("average", self.average, 0.0, 3600.0)
    }
}

/// A still pass under everything, then the sky, the clouds (and birds), the land, what moves over it
/// (smoke, fireflies) and the interface, each drawn live.
const PASSES: &[Pass] = &[Pass::Still(0), Pass::Live(0), Pass::Live(1), Pass::Live(2), Pass::Live(3), Pass::Live(4)];

/// The clouds: a grid of cells over the world, each filled or clear, a slab of them at a height.
const CLOUD_Y: f64 = 152.0;
const CLOUD_THICK: f64 = 4.0;
const CLOUD_CELL: f64 = 12.0;
const CLOUD_MAP: usize = 64;
/// How far off they fade out, across the ground, in blocks.
const CLOUD_FADE: (f64, f64) = (260.0, 900.0);

/// The cloud grid: which cells are filled, in blobs, repeating every CLOUD_MAP cells.
fn cloud_map(seed: i64) -> Vec<bool> {
    let mut rng = crate::rng::Rng::new(seed ^ 0xc10d);
    let n = CLOUD_MAP;
    let lattice: Vec<f64> = (0..64).map(|_| rng.f64()).collect();
    let smooth = |x: f64, y: f64| {
        let (cells, k) = (8usize, 8.0);
        let (fx, fy) = (x / k, y / k);
        let (x0, y0) = (fx.floor() as usize % cells, fy.floor() as usize % cells);
        let (x1, y1) = ((x0 + 1) % cells, (y0 + 1) % cells);
        let (tx, ty) = (smooth(fx.fract()), smooth(fy.fract()));
        let a = lattice[y0 * cells + x0] + (lattice[y0 * cells + x1] - lattice[y0 * cells + x0]) * tx;
        let b = lattice[y1 * cells + x0] + (lattice[y1 * cells + x1] - lattice[y1 * cells + x0]) * tx;
        a + (b - a) * ty
    };
    (0..n * n).map(|i| smooth((i % n) as f64, (i / n) as f64) * 0.75 + rng.f64() * 0.25 > 0.56).collect()
}

/// A ray's cloud, as the shader finds it: stepped through the grid's cells in the slab, the first
/// filled one, shaded by the face it was entered through and lit by the moment, faded with distance.
/// Premultiplied.
fn cloud_at(map: &[bool], eye: V3, d: V3, shift: f64, m: &Moment) -> [f64; 4] {
    if d[1] < 1e-3 {
        return [0.0; 4];
    }
    let t0 = (CLOUD_Y - eye[1]) / d[1];
    let t1 = (CLOUD_Y + CLOUD_THICK - eye[1]) / d[1];
    let p = [eye[0] + d[0] * t0 + shift, eye[2] + d[2] * t0];
    let mut cell = [(p[0] / CLOUD_CELL).floor(), (p[1] / CLOUD_CELL).floor()];
    let dd = [d[0], d[2]];
    let step = dd.map(|v| if v >= 0.0 { 1.0 } else { -1.0 });
    let span = dd.map(|v| if v.abs() < 1e-9 { 1e12 } else { CLOUD_CELL / v.abs() });
    let mut next: [f64; 2] = std::array::from_fn(|i| {
        if dd[i].abs() < 1e-9 {
            return 1e12;
        }
        let edge = (cell[i] + if step[i] > 0.0 { 1.0 } else { 0.0 }) * CLOUD_CELL;
        t0 + (edge - p[i]) / dd[i]
    });
    let (mut t, mut face) = (t0, 1usize);
    for _ in 0..48 {
        if t > t1 {
            break;
        }
        let (mx, my) = ((cell[0] as i64).rem_euclid(CLOUD_MAP as i64) as usize, (cell[1] as i64).rem_euclid(CLOUD_MAP as i64) as usize);
        if map[my * CLOUD_MAP + mx] {
            let k = [0.86, 0.74, 0.92][face];
            let far = t * dd[0].hypot(dd[1]);
            let fade = 1.0 - smooth_ab(CLOUD_FADE.0, CLOUD_FADE.1, far);
            let mist = 0.55 * smooth_ab(CLOUD_FADE.0 * 0.4, CLOUD_FADE.1, far);
            let a = 0.82 * fade;
            let lit = [k * m.cloud.0, k * m.cloud.1, k * m.cloud.2];
            let haze = [m.haze.0, m.haze.1, m.haze.2];
            return std::array::from_fn(|i| if i == 3 { a } else { (lit[i] + (haze[i] - lit[i]) * mist) * a });
        }
        if next[0] < next[1] {
            t = next[0];
            next[0] += span[0];
            cell[0] += step[0];
            face = 0;
        } else {
            t = next[1];
            next[1] += span[1];
            cell[1] += step[1];
            face = 2;
        }
    }
    [0.0; 4]
}

/// The clouds' shader: `cloud_at` for each of four rays through the pixel.
const CLOUDS: &str = "uniform vec4 cloud; uniform vec4 hazec; uniform vec4 lightc;
float filled(vec2 c) { vec2 m = mod(c, MAP); return texture2D(tex0, (m + 0.5) / MAP).a; }
vec4 at(vec2 px) {
  vec3 d = ray(px);
  if (d.y < 1e-3) return vec4(0.0);
  float t0 = (cloud.x - eye.y) / d.y;
  float t1 = (cloud.x + cloud.y - eye.y) / d.y;
  vec2 p = vec2(eye.x + d.x * t0 + cloud.w, eye.z + d.z * t0);
  vec2 cell = floor(p / cloud.z);
  vec2 dd = d.xz;
  vec2 stp = vec2(dd.x >= 0.0 ? 1.0 : -1.0, dd.y >= 0.0 ? 1.0 : -1.0);
  vec2 span = vec2(abs(dd.x) < 1e-9 ? 1e12 : cloud.z / abs(dd.x), abs(dd.y) < 1e-9 ? 1e12 : cloud.z / abs(dd.y));
  vec2 next;
  next.x = abs(dd.x) < 1e-9 ? 1e12 : t0 + ((cell.x + (stp.x > 0.0 ? 1.0 : 0.0)) * cloud.z - p.x) / dd.x;
  next.y = abs(dd.y) < 1e-9 ? 1e12 : t0 + ((cell.y + (stp.y > 0.0 ? 1.0 : 0.0)) * cloud.z - p.y) / dd.y;
  float t = t0;
  float k = 0.74;
  for (int i = 0; i < 48; i++) {
    if (t > t1) break;
    if (filled(cell) > 0.5) {
      float far = t * length(dd);
      float a = 0.82 * (1.0 - smoothstep(FADE0, FADE1, far));
      float mist = 0.55 * smoothstep(FADE0 * 0.4, FADE1, far);
      return vec4(mix(k * lightc.rgb, hazec.rgb, mist) * a, a);
    }
    if (next.x < next.y) { t = next.x; next.x += span.x; cell.x += stp.x; k = 0.86; }
    else { t = next.y; next.y += span.y; cell.y += stp.y; k = 0.92; }
  }
  return vec4(0.0);
}
void main() {
  vec2 px = area.xy + uv * area.zw;
  vec4 s = at(px + vec2(-0.25, -0.25)) + at(px + vec2(0.25, -0.25)) + at(px + vec2(-0.25, 0.25)) + at(px + vec2(0.25, 0.25));
  gl_FragColor = s / 4.0;
}";
const CLOUD_UNIFORMS: [&str; 4] = ["cloud", "hazec", "lightc", "tex0"];
/// The camera's uniforms, which every shader here takes first, and the area.
const RAY_UNIFORMS: [&str; 6] = ["eye", "fwd", "rgt", "upv", "lens", "area"];

/// A shader of this element, and its uniforms' places: the camera's first, then its own.
struct Program {
    prog: GLuint,
    at: Vec<GLint>,
}

impl Program {
    fn compile(gfx: &Gfx, src: &str, names: &[&str]) -> Option<Program> {
        match gfx.compile(&format!("{}{src}", sky::RAY)) {
            Ok(prog) => Some(Program { prog, at: RAY_UNIFORMS.iter().chain(names).map(|n| gfx.uniform(prog, n)).collect() }),
            Err(e) => {
                crate::errln!("blocky: a shader does not compile: {e}");
                None
            }
        }
    }
}

pub struct Blocky {
    s: Settings,
    seed: i64,
    /// The hours the sun is up.
    day: [f64; 2],
    cam: Option<Camera>,
    /// The traced pictures, until they are uploaded.
    frame: Option<render::Frame>,
    land: Option<[Tex; 3]>,
    /// Where smoke rises and fireflies dance, from the world.
    smoke: Vec<(V3, bool)>,
    /// The animals that wander, and how they stood when their patches last went up.
    herd: Option<herd::Herd>,
    herd_now: u64,
    herd_shown: Option<u64>,
    glades: Vec<(V3, f64)>,
    /// The fires the camera sees, for their flames at night (`life::flames`).
    fires: Vec<V3>,
    clouds: Vec<bool>,
    cloud_tex: Option<Tex>,
    programs: [Option<Option<Program>>; 3],
    layout: Option<hud::Layout>,
    hotbar: Option<ImageSurface>,
    hotbar_tex: Option<Tex>,
    status: Option<hud::Status>,
    status_tex: Option<Tex>,
    shown: Option<hud::Status>,
    readings: Mean,
    /// The scene's clock, and the time of day as a fraction of it.
    t: f64,
    f: f64,
}

/// The world's seed: the same farm, hills and woods every start.
const WORLD: i64 = 7;

impl Blocky {
    fn new(t: &Table, g: &Geo) -> Blocky {
        let s: Settings = settings(t);
        let seed = if s.seed != 0 { s.seed } else { WORLD };
        Blocky {
            seed,
            day: g.th.tw.day,
            clouds: cloud_map(seed),
            s,
            cam: None,
            frame: None,
            land: None,
            smoke: vec![],
            glades: vec![],
            fires: vec![],
            herd: None,
            herd_now: 0,
            herd_shown: None,
            cloud_tex: None,
            programs: [None, None, None],
            layout: None,
            hotbar: None,
            hotbar_tex: None,
            status: None,
            status_tex: None,
            shown: None,
            readings: Mean::default(),
            t: 0.0,
            f: crate::day::daytime(),
        }
    }

    /// The world laid out and traced, once, for this screen.
    fn render(&mut self, g: &Geo) {
        if self.cam.is_some() {
            return;
        }
        let yaw = self.s.yaw.to_radians();
        let s = &self.s;
        let tag = format!("{} {} {} {} {} {} {}", self.seed, s.yaw, s.pitch, s.fov, s.samples, s.distance, s.hand);
        let theme = g.th.dirs.first().map(|d| d.join("theme.toml"));
        // the world made (lit only to be traced) while what the cache keeps is read
        let seed = self.seed;
        let (mut w, kept) = std::thread::scope(|sc| {
            let world = sc.spawn(move || world::World::unlit(seed, (yaw.sin(), yaw.cos())));
            let kept = theme.as_ref().and_then(|t| crate::svg::kept_read(t, g.w, g.h, "blocky", &tag));
            (world.join().unwrap_or_else(|e| std::panic::resume_unwind(e)), kept)
        });
        let tex = textures::Textures::new(self.seed);
        let cam = Camera::new(w.eye, yaw, self.s.pitch.to_radians(), self.s.fov, g.w.max(1) as usize, g.h.max(1) as usize);
        self.cam = Some(cam);
        let fog = (self.s.distance * 0.7, self.s.distance);
        // the traced picture and the depths under the animals, kept in the theme's cache: tracing them
        // takes seconds on a small machine, and they are the same each start
        let trace = |w: &mut world::World| {
            w.light_up();
            let tracer = Tracer::new(w, &tex, cam, fog, sky::arcs(&cam), self.s.hand);
            let frame = tracer.render(self.s.samples.round() as usize);
            let herd = herd::Herd::new(w, fog, &frame, &cam, self.seed, |x, y| tracer.depth(x as f64 + 0.5, y as f64 + 0.5));
            let mut bytes = [frame.base, frame.light, frame.sun].concat();
            bytes.extend(herd.depths().flat_map(f32::to_le_bytes));
            bytes
        };
        let (mut frame, herd) = match kept.and_then(|bytes| self.unpack(&w, fog, &cam, &bytes)) {
            Some(both) => both,
            None => {
                let bytes = trace(&mut w);
                if let Some(t) = &theme {
                    crate::svg::keep(t, g.w, g.h, "blocky", &tag, &bytes);
                }
                self.unpack(&w, fog, &cam, &bytes).expect("a fresh trace unpacks")
            }
        };
        // the wandering animals, where they start, into the still picture
        let mut herd = herd;
        herd.paint_into(&cam, &mut frame, cam.w);
        self.herd = Some(herd);
        self.frame = Some(frame);
        (self.smoke, self.glades) = (w.smoke.clone(), w.glades.clone());
        self.fires = w.smoke.iter().filter(|&&(p, chimney)| !chimney && seen(&w, cam.eye, p)).map(|&(p, _)| [p[0], p[1] - 0.6, p[2]]).collect();
        if self.s.ui {
            // the HUD over a panel at the screen's foot, never under it; the arm, in the scene, stays put
            let layout = hud::Layout::new(g.w as f64, g.h as f64, g.edges.bottom as f64, self.s.gui_scale);
            let bar = surface(layout.hotbar.2.ceil() as i32, layout.hotbar.3.ceil() as i32);
            hud::paint_hotbar(&ctx(&bar), &layout);
            self.layout = Some(layout);
            self.hotbar = Some(bar);
        }
    }

    /// The picture and the herd from `trace`'s bytes: the three pictures, then the depths the herd asks
    /// for; None where they are not as many as this screen and world need.
    fn unpack(&self, w: &world::World, fog: (f64, f64), cam: &Camera, bytes: &[u8]) -> Option<(render::Frame, herd::Herd)> {
        let n = cam.w * cam.h * 4;
        if bytes.len() < n * 3 {
            return None;
        }
        let frame = render::Frame { base: bytes[..n].to_vec(), light: bytes[n..n * 2].to_vec(), sun: bytes[n * 2..n * 3].to_vec() };
        let (chunks, rest) = bytes[n * 3..].as_chunks::<4>();
        if !rest.is_empty() {
            return None;
        }
        let mut depths = chunks.iter().map(|&b| f32::from_le_bytes(b));
        let mut short = false;
        let herd = herd::Herd::new(w, fog, &frame, cam, self.seed, |_, _| {
            depths.next().unwrap_or_else(|| {
                short = true;
                f32::INFINITY
            })
        });
        (!short && depths.next().is_none()).then_some((frame, herd))
    }

    fn moment(&self) -> Option<Moment> {
        let cam = self.cam?;
        Some(Moment::at(self.f, self.day[0], self.day[1], &cam, self.s.sky.rgb(), self.s.haze.rgb()))
    }

    /// The rows the clouds can show in: from the top to just under the horizon.
    fn cloud_rows(&self, g: &Geo) -> f64 {
        let Some(cam) = self.cam else { return g.h as f64 };
        // where a level ray meets the screen's middle column
        let yn = -cam.forward[1] / cam.up[1].max(1e-6);
        ((1.0 - yn / cam.tan) * g.h as f64 / 2.0 + 2.0).clamp(0.0, g.h as f64)
    }

    fn shift(&self) -> f64 {
        -self.t * self.s.clouds
    }

    /// The HUD's readings, each the mean of the last `average` seconds, so it shows how the machine has
    /// been rather than every spike.
    fn read(&mut self, w: &World) -> hud::Status {
        let now = [&self.s.health, &self.s.food, &self.s.experience, &self.s.level].map(|src| w.reading(src).0);
        let [health, food, xp, level] = self.readings.add(w.clock, now, self.s.average);
        let half = |v: f64| ((v / 100.0).clamp(0.0, 1.0) * 20.0).round() as u32;
        let xp = (xp / 100.0).clamp(0.0, 1.0);
        hud::Status { health: half(100.0 - health), food: half(100.0 - food), xp: (xp * 180.0).round() / 180.0, level: level.clamp(0.0, 999.0).round() as u32 }
    }

    fn status_picture(&self) -> Option<ImageSurface> {
        let (l, st) = (self.layout?, self.status?);
        let s = surface(l.status.2.ceil() as i32, l.status.3.ceil() as i32);
        hud::paint_status(&ctx(&s), &l, &st);
        Some(s)
    }

    /// The camera's uniforms, and the area a shader draws over.
    fn ray_uniforms(cam: &Camera, g: &Geo, rows: f64) -> [(usize, [f32; 4]); 6] {
        let v3 = |v: V3| [v[0] as f32, v[1] as f32, v[2] as f32, 0.0];
        [
            (3, v3(cam.eye)),
            (3, v3(cam.forward)),
            (3, v3(cam.right)),
            (3, v3(cam.up)),
            (4, [cam.tan as f32, cam.aspect as f32, g.w as f32, g.h as f32]),
            (4, [0.0, 0.0, g.w as f32, rows as f32]),
        ]
    }

    /// Draw with program k over the top `rows` of the screen: the camera's uniforms, then `own` (each
    /// its size and value), then the textures.
    fn shade(&mut self, k: usize, src: &str, names: &[&str], g: &Geo, gfx: &mut Gfx, rows: f64, own: &[(usize, [f32; 4])], textures: &[Tex], blend: Blend) {
        let Some(cam) = self.cam else { return };
        let program = self.programs[k].get_or_insert_with(|| Program::compile(gfx, src, names));
        let Some(p) = program else { return };
        let all: Vec<_> = Self::ray_uniforms(&cam, g, rows).iter().chain(own).zip(&p.at).map(|(&(n, v), &at)| (at, n, v)).collect();
        let samplers: Vec<_> = textures.iter().enumerate().map(|(i, &t)| (p.at[RAY_UNIFORMS.len() + own.len() + i], t)).collect();
        gfx.custom(p.prog, &all, &samplers, 0.0, 0.0, g.w as f64, rows, blend);
    }
}

/// A running mean of readings over a window of time, sampled once a second.
#[derive(Default)]
struct Mean {
    samples: std::collections::VecDeque<(f64, [f64; 4])>,
}

impl Mean {
    /// Take the readings at `t` (seconds) and give the mean over the last `window` seconds; with no
    /// window, the readings as they are.
    fn add(&mut self, t: f64, now: [f64; 4], window: f64) -> [f64; 4] {
        if window <= 0.0 {
            return now;
        }
        if self.samples.back().is_none_or(|&(last, _)| t - last >= 1.0 || t < last) {
            self.samples.push_back((t, now));
        }
        while self.samples.front().is_some_and(|&(first, _)| t - first > window || first > t) {
            self.samples.pop_front();
        }
        let n = self.samples.len().max(1) as f64;
        std::array::from_fn(|i| self.samples.iter().map(|s| s.1[i]).sum::<f64>() / n)
    }
}

impl Part for Blocky {
    fn passes(&self) -> &'static [Pass] {
        PASSES
    }
    fn still(&self) -> bool {
        true
    }
    fn live(&self) -> bool {
        true
    }
    fn area(&self, _g: &Geo) -> Option<Vec<Rect>> {
        Some(vec![]) // painted only under the live picture, never shown in it
    }
    fn top(&self, _g: &Geo) -> i32 {
        0
    }
    fn paint_pass(&mut self, _k: u8, g: &Geo, _c: &Context, _s: &mut Shared) {
        self.render(g);
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.render(g);
        let n = CLOUD_MAP as i32;
        let mut map = surface(n, n);
        if let Some(mut d) = view(&mut map) {
            for (i, &f) in self.clouds.iter().enumerate() {
                if f {
                    d[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
        self.cloud_tex = Some(up.add(map, Kind::Label));
        if let Some(frame) = self.frame.take() {
            self.land = Some([frame.base, frame.light, frame.sun].map(|bytes| up.add(picture(&bytes, g.w, g.h), Kind::Label)));
        }
        if let Some(bar) = self.hotbar.take() {
            self.hotbar_tex = Some(up.add(bar, Kind::Label));
        }
        if let Some(s) = self.status_picture() {
            self.status_tex = Some(up.add(s, Kind::Label));
            self.shown = self.status;
        }
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        // the clouds move and the torches flicker a little a frame: fifteen frames a second are plenty
        self.t = (w.clock * 15.0).round() / 15.0;
        // the time of day, in steps of twenty seconds
        self.f = (crate::day::daytime() * 4320.0).round() / 4320.0;
        if let Some(m) = self.moment() {
            w.title_light = sky::title_light(&m);
        }
        let mut h = self.t.to_bits();
        hash_f(&mut h, self.f);
        if let Some(herd) = &mut self.herd {
            self.herd_now = herd.advance(w.clock);
            h ^= self.herd_now;
        }
        if self.s.ui {
            let st = self.read(w);
            self.status = Some(st);
            for v in [st.health as f64, st.food as f64, st.xp, st.level as f64] {
                hash_f(&mut h, v);
            }
        }
        h
    }
    fn interval(&self) -> f64 {
        1.0 / 15.0
    }
    fn draw_pass(&mut self, k: u8, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let Some(m) = self.moment() else { return };
        let v = |c: Rgb| [c.0 as f32, c.1 as f32, c.2 as f32, 0.0];
        match k {
            0 => {
                let u = m.uniforms();
                let own: Vec<(usize, [f32; 4])> = u.iter().map(|&x| (4, x)).collect();
                self.shade(0, sky::SKY, &sky::SKY_UNIFORMS, g, gfx, g.h as f64, &own, &[], Blend::Opaque);
            }
            1 => {
                let Some(map) = self.cloud_tex else { return };
                let rows = self.cloud_rows(g).ceil();
                if rows <= 0.0 {
                    return;
                }
                let src = CLOUDS
                    .replace("MAP", &format!("{CLOUD_MAP:.1}"))
                    .replace("FADE0", &format!("{:.1}", CLOUD_FADE.0))
                    .replace("FADE1", &format!("{:.1}", CLOUD_FADE.1));
                let own = [(4, [CLOUD_Y as f32, CLOUD_THICK as f32, CLOUD_CELL as f32, self.shift() as f32]), (4, v(m.haze)), (4, v(m.cloud))];
                self.shade(1, &src, &CLOUD_UNIFORMS, g, gfx, rows, &own, &[map], Blend::Premultiplied);
                if let Some(cam) = self.cam {
                    gfx.rects(None, &life::birds(&cam, self.t, &m), (0.13, 0.13, 0.15), Blend::Alpha);
                }
            }
            2 => {
                let Some(land) = self.land else { return };
                // the animals' patches, where they have moved
                if self.herd_shown != Some(self.herd_now)
                    && let (Some(herd), Some(cam)) = (&mut self.herd, self.cam)
                {
                    for (x, y, w, h, pics) in herd.paint(&cam) {
                        for (pic, tex) in pics.into_iter().zip(land) {
                            gfx.replace_bytes(tex, &pic, x, y, w, h);
                        }
                    }
                    self.herd_shown = Some(self.herd_now);
                }
                let at = |c: Rgb, p: f64| [c.0 as f32, c.1 as f32, c.2 as f32, p as f32];
                let dir = |d: V3| [d[0] as f32, d[1] as f32, d[2] as f32, 0.0];
                let own = [
                    (4, at(m.light, m.day)),
                    (4, v(m.haze)),
                    (1, [self.t as f32, 0.0, 0.0, 0.0]),
                    (4, at(m.sunc, m.sun_at)),
                    (4, at(m.moonc, m.moon_at)),
                    (4, dir(m.sun)),
                    (4, dir(m.moon)),
                ];
                let src = sky::LAND.replace("SUNS1", &format!("{:.1}", render::SUNS - 1)).replace("FLAME", &format!("{:.3}", sky::FLAME));
                self.shade(2, &src, &sky::LAND_UNIFORMS, g, gfx, g.h as f64, &own, &land, Blend::Premultiplied);
            }
            3 => {
                let Some(cam) = self.cam else { return };
                let (outer, inner) = life::flames(&cam, &self.fires, self.t, &m);
                gfx.rects(None, &outer, (0.95, 0.45, 0.1), Blend::Alpha);
                gfx.rects(None, &inner, (1.0, 0.85, 0.35), Blend::Alpha);
                for (puffs, grey) in life::smoke(&cam, &self.smoke, self.t, &m) {
                    gfx.rects(None, &puffs, grey, Blend::Alpha);
                }
                let (glow, core) = life::fireflies(&cam, &self.glades, self.t, &m);
                gfx.rects(None, &glow, (0.8, 1.0, 0.4), Blend::Alpha);
                gfx.rects(None, &core, (0.92, 1.0, 0.6), Blend::Alpha);
            }
            _ => {
                let Some(l) = self.layout else { return };
                if let Some(tex) = self.hotbar_tex {
                    gfx.image(tex, l.hotbar.0, l.hotbar.1, l.hotbar.2.ceil(), l.hotbar.3.ceil(), 1.0, Blend::Premultiplied);
                }
                if self.status != self.shown
                    && let (Some(tex), Some(s)) = (self.status_tex, self.status_picture())
                {
                    gfx.replace(tex, &s, Kind::Label);
                    self.shown = self.status;
                }
                if let Some(tex) = self.status_tex {
                    gfx.image(tex, l.status.0, l.status.1, l.status.2.ceil(), l.status.3.ceil(), 1.0, Blend::Premultiplied);
                }
            }
        }
    }
    fn rest_pass(&mut self, k: u8, g: &Geo, c: &Context, _s: &mut Shared) {
        self.render(g);
        let (Some(cam), Some(m)) = (self.cam, self.moment()) else { return };
        let (w, h) = (g.w.max(1) as usize, g.h.max(1) as usize);
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut pic = surface(w as i32, h as i32);
        let stride = pic.stride() as usize;
        // where the land is opaque it is all that shows: the sky and the clouds there are not worked out
        let cover = self.frame.as_ref().map(|f| &f.base);
        let under = |x: usize, y: usize| cover.is_some_and(|b| b[(y * w + x) * 4 + 3] == 255);
        // each pass's picture, pixel by pixel, as its shader draws it, its rows on every core
        let mut paint = |fill: &(dyn Fn(usize, usize) -> Option<[u8; 4]> + Sync)| {
            if let Some(mut d) = view(&mut pic) {
                crate::trace::rows([&mut d[..]], stride, |y, [row]| {
                    for x in 0..w {
                        if let Some(px) = fill(x, y) {
                            row[x * 4..x * 4 + 4].copy_from_slice(&px);
                        }
                    }
                });
            }
        };
        match k {
            0 => paint(&|x, y| {
                if under(x, y) {
                    return None;
                }
                let s = sky::sky_at(&m, cam.ray(x as f64 + 0.5, y as f64 + 0.5));
                Some([byte(s.2), byte(s.1), byte(s.0), 255])
            }),
            1 => {
                let rows = (self.cloud_rows(g).ceil() as usize).min(h);
                let (shift, clouds) = (self.shift(), &self.clouds);
                paint(&|x, y| {
                    if y >= rows || under(x, y) {
                        return None;
                    }
                    let mut sum = [0.0; 4];
                    for (ox, oy) in [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)] {
                        let c = cloud_at(clouds, cam.eye, cam.ray(x as f64 + 0.5 + ox, y as f64 + 0.5 + oy), shift, &m);
                        for i in 0..4 {
                            sum[i] += c[i] / 4.0;
                        }
                    }
                    Some([byte(sum[2]), byte(sum[1]), byte(sum[0]), byte(sum[3])])
                });
            }
            2 => {
                let Some(frame) = &self.frame else { return };
                let t = self.t;
                paint(&|x, y| {
                    let i = (y * w + x) * 4;
                    let a = frame.base[i + 3];
                    if a == 0 {
                        return None;
                    }
                    let f = |v: u8| v as f64 / 255.0;
                    let base = [f(frame.base[i + 2]), f(frame.base[i + 1]), f(frame.base[i]), f(a)];
                    let light = [f(frame.light[i + 2]), f(frame.light[i + 1]), f(frame.light[i])];
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let sun = (frame.sun[i], frame.sun[i + 1]);
                    let mat = ((frame.sun[i + 2] as f64) / 40.0).round() as u8;
                    let o = sky::shown(&m, base, light, sun, mat, sky::flicker(px, py, t));
                    Some([byte(o[2]), byte(o[1]), byte(o[0]), a])
                });
            }
            // what moves is left out of the still picture
            3 => return,
            _ => {
                let Some(l) = self.layout else { return };
                if let Some(bar) = &self.hotbar
                    && c.set_source_surface(bar, l.hotbar.0, l.hotbar.1).is_ok()
                {
                    let _ = c.paint();
                }
                if self.status.is_none() {
                    self.status = Some(hud::Status { health: 20, food: 20, xp: 0.45, level: 27 });
                }
                if let Some(s) = self.status_picture()
                    && c.set_source_surface(&s, l.status.0, l.status.1).is_ok()
                {
                    let _ = c.paint();
                }
                return;
            }
        }
        if c.set_source_surface(&pic, 0.0, 0.0).is_ok() {
            let _ = c.paint();
        }
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "blocky",
        defaults: r##"seed = 0
yaw = 35.0
pitch = -13.0
fov = 78.0
samples = 2.0
distance = 196.0
hand = false
ui = true
gui_scale = 0.0
clouds = 0.6
average = 60.0
sky = "#79a6ff"
haze = "#c3d9ff"
health = "cpu"
food = "ram"
experience = "disk"
level = "used""##,
        varies: &[],
        check: check::<Settings>,
        make: |_, t, g| Ok(vec![Box::new(Blocky::new(t, g))]),
    }]
}

/// Whether the camera at `eye` sees the point `p` of the world: no whole block between them past the
/// one `p` is in. Flames are drawn over the picture, so a fire behind a house must not show.
fn seen(w: &world::World, eye: V3, p: V3) -> bool {
    let d = [eye[0] - p[0], eye[1] - p[1], eye[2] - p[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let mut s = 0.75;
    while s < len {
        let q = [p[0] + d[0] / len * s, p[1] + d[1] / len * s, p[2] + d[2] / len * s];
        let b = w.get(q[0].floor() as i32, q[1].floor() as i32, q[2].floor() as i32);
        // whole blocks only: a lantern, a fence or a slab fills little of its cell
        if world::block(b).shape == world::Shape::Cube {
            return false;
        }
        s += 0.05;
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> world::World {
        world::World::new(7, (35f64.to_radians().sin(), 35f64.to_radians().cos()))
    }

    fn ahead() -> (f64, f64) {
        (35f64.to_radians().sin(), 35f64.to_radians().cos())
    }

    #[test]
    fn the_viewer_looks_down_on_the_farm_ahead() {
        let w = world();
        let [x, y, z] = w.eye.map(|v| v.floor() as i32);
        assert!((y - 1..=y).all(|y| w.get(x, y, z) == world::AIR), "open air about and below the eye");
        let below = w.ground(x, z);
        assert!((8..=11).contains(&(y - below)), "looking down from on high: {} blocks over the ground", y - below);
        // the farm: houses' walls, a field, torches and animals about its middle
        let (cx, cz) = ((ahead().0 * world::FARM_AHEAD).round() as i32, (ahead().1 * world::FARM_AHEAD).round() as i32);
        let count = |b: u8| {
            (-30..=30)
                .flat_map(|dx| (-30..=30).map(move |dz| (dx, dz)))
                .flat_map(|(dx, dz)| (60..80).map(move |y| (dx, y, dz)))
                .filter(|&(dx, y, dz)| w.get(cx + dx, y, cz + dz) == b)
                .count()
        };
        assert!(count(world::PLANKS) > 100, "houses");
        assert!(count(world::FARMLAND) > 100 && count(world::WHEAT) > 20, "fields");
        assert!(count(world::TORCH) >= 10 && count(world::LANTERN) >= 4, "lights for the night");
        assert!(count(world::MOB) > 20, "animals and villagers");
    }

    #[test]
    fn the_mine_is_lit_and_open() {
        let w = world();
        let k = world::knoll(ahead());
        let (kx, kz) = (k.0.round() as i32, k.1.round() as i32);
        let near = |b: u8| (-30..=30).any(|dx| (-30..=30).any(|dz| (60..100).any(|y| w.get(kx + dx, y, kz + dz) == b)));
        assert!(near(world::RAIL) || near(world::RAIL_Z), "rails run into it");
        assert!(near(world::TORCH) && near(world::LANTERN) && near(world::CAMPFIRE), "it is lit, a fire before it");
    }

    #[test]
    fn the_cart_tunnel_has_rails_out_of_it_and_two_torches() {
        let w = world();
        let k = world::cart_tunnel_at(ahead());
        let (kx, kz) = (k.0.round() as i32, k.1.round() as i32);
        let count = |b: u8| {
            (-30..=30)
                .flat_map(|dx| (-30..=30).map(move |dz| (dx, dz)))
                .flat_map(|(dx, dz)| (60..100).map(move |y| (dx, y, dz)))
                .filter(|&(dx, y, dz)| w.get(kx + dx, y, kz + dz) == b)
                .count()
        };
        assert!(count(world::RAIL) + count(world::RAIL_Z) >= 10, "rails");
        assert!(count(world::TORCH) >= 2, "a torch at its mouth, one inside");
        assert!(count(world::MOB) >= 1, "a cart on the rails");
    }

    #[test]
    fn the_mountain_on_the_left_has_a_lit_shaft() {
        let w = world();
        let k = world::shaft_at(ahead());
        let (kx, kz) = (k.0.round() as i32, k.1.round() as i32);
        let count = |b: u8| {
            (-40..=40)
                .flat_map(|dx| (-40..=40).map(move |dz| (dx, dz)))
                .flat_map(|(dx, dz)| (60..100).map(move |y| (dx, y, dz)))
                .filter(|&(dx, y, dz)| w.get(kx + dx, y, kz + dz) == b)
                .count()
        };
        assert!(count(world::RAIL) + count(world::RAIL_Z) >= 12, "rails run into it");
        assert!(count(world::TORCH) >= 7, "torches on its frame, inside and about its mouth: {}", count(world::TORCH));
    }

    #[test]
    fn the_cliff_cottage_stands_on_its_plateau_with_a_waterfall() {
        let w = world();
        let k = world::cliff_at(ahead());
        let (kx, kz) = (k.0.round() as i32, k.1.round() as i32);
        let count = |b: u8| {
            (-12..=12)
                .flat_map(|dx| (-12..=12).map(move |dz| (dx, dz)))
                .flat_map(|(dx, dz)| (60..130).map(move |y| (dx, y, dz)))
                .filter(|&(dx, y, dz)| w.get(kx + dx, y, kz + dz) == b)
                .count()
        };
        assert!(count(world::GLASS) >= 2 && count(world::DOOR_LOW) == 1, "a cottage with a door and windows");
        assert!(count(world::LANTERN) >= 2 && count(world::TORCH) >= 1, "lit at night");
        assert!(count(world::WATER) >= 9, "a waterfall and its pool");
    }

    #[test]
    fn torches_light_their_surroundings() {
        let w = world();
        let (cx, cz) = ((ahead().0 * world::FARM_AHEAD).round() as i32, (ahead().1 * world::FARM_AHEAD).round() as i32);
        // a lamp post at (4, 4) from the middle: its torch two blocks over the ground
        assert_eq!(w.get(cx + 4, world::GROUND + 2, cz + 4), world::TORCH);
        assert_eq!(w.light(cx + 4, world::GROUND + 2, cz + 4).1, 14);
        assert_eq!(w.light(cx + 6, world::GROUND + 2, cz + 4).1, 12, "a level less a block");
        assert_eq!(w.light(cx + 4, world::GROUND + 30, cz + 4).0, 15, "the open sky");
    }

    #[test]
    fn the_sky_is_clear_and_the_ground_is_not() {
        let w = world();
        let tex = textures::Textures::new(7);
        let cam = Camera::new(w.eye, 0.0, -0.2, 70.0, 64, 36);
        let frame = Tracer::new(&w, &tex, cam, (120.0, 190.0), sky::arcs(&cam), false).render(1);
        let alpha = |y: usize, x: usize| frame.base[(y * 64 + x) * 4 + 3];
        assert_eq!(alpha(0, 32), 0, "the sky overhead shows through the land's picture");
        assert_eq!(alpha(35, 32), 255, "the ground underfoot covers it");
        assert!(frame.light[(35 * 64 + 32) * 4 + 2] >= 250, "the sky's full light on open ground");
    }

    #[test]
    fn night_is_darker_than_day_and_torches_are_warm() {
        let cam = Camera::new([0.0, 80.0, 0.0], 0.0, -0.2, 70.0, 64, 36);
        let [day, night] = [13.0, 23.5].map(|h| Moment::at(h / 24.0, 6.5, 19.5, &cam, (0.47, 0.65, 1.0), (0.76, 0.85, 1.0)));
        assert!(day.light.0 + day.sunc.0 > 0.9 && night.light.0 + night.moonc.0 < 0.3, "the sky and the sun, by day and by night");
        assert!(day.stars < 0.01 && night.stars > 0.9);
        let base = [0.5, 0.5, 0.5, 1.0];
        let lamp = sky::lit(&night, base, [0.2, 0.9, 0.0], (255, 0), 1.0);
        assert!(lamp[0] > lamp[2] * 1.5, "torchlight is warm: {lamp:?}");
        assert!(sky::lit(&day, base, [1.0, 0.9, 0.0], (255, 0), 1.0)[2] > lamp[2], "by day the sky outshines it");
        // the torches are out by day: under a roof, lit only by one, it is dark
        let roofed = |m: &Moment| sky::lit(m, base, [0.0, 0.9, 0.0], (0, 0), 1.0)[0];
        assert!(roofed(&day) < 0.02 && roofed(&night) > 0.3, "{} {}", roofed(&day), roofed(&night));
        let dusk = Moment::at(19.4 / 24.0, 6.5, 19.5, &cam, (0.47, 0.65, 1.0), (0.76, 0.85, 1.0));
        assert!(roofed(&dusk) > 0.2, "lit at dusk: {}", roofed(&dusk));
        // in the sun's light a top face is brighter than in shadow, and a face turned away from it
        // darker than one facing it
        let (sunlit, shade) = (sky::lit(&day, base, [1.0, 0.0, 0.0], (255, 0), 1.0), sky::lit(&day, base, [1.0, 0.0, 0.0], (0, 0), 1.0));
        assert!(sunlit[0] > shade[0] * 1.5, "{sunlit:?} {shade:?}");
        // an animal's side takes the sun by its true facing: turned a little, a little more or less,
        // never a jump where its nearest axis changes
        let grey = [0.5, 0.5, 0.5, 1.0];
        let side = |deg: f64| {
            let n = [deg.to_radians().cos(), 0.0, deg.to_radians().sin()];
            sky::shown(&day, grey, [1.0, 0.0, 0.0], (255, sky::side_angle(n)), render::SIDE, 1.0)[0]
        };
        let steps: Vec<f64> = (0..=36).map(|k| side(k as f64 * 10.0)).collect();
        assert!(steps.windows(2).all(|w| (w[0] - w[1]).abs() < 0.06), "{steps:?}");
        assert!(steps.iter().cloned().fold(0.0, f64::max) - steps.iter().cloned().fold(1.0, f64::min) > 0.05, "it does turn: {steps:?}");
        // the hostname shows fully by day, dampened by night
        assert!(sky::title_light(&day) > 0.99, "{}", sky::title_light(&day));
        assert!(sky::title_light(&night) < 0.5, "{}", sky::title_light(&night));
        // a torch's flame shines by night whatever light falls on it, and is lit as anything else by day
        let flame = [0.9, 0.6, 0.2, 1.0];
        let glow = |m: &Moment, mat: u8| sky::shown(m, flame, [0.0, 0.3, 0.0], (0, 0), mat, 1.0)[0];
        assert!(glow(&night, render::GLOW) > 1.0 && glow(&night, render::STILL) < 0.5, "{} {}", glow(&night, render::GLOW), glow(&night, render::STILL));
        assert_eq!(glow(&day, render::GLOW), glow(&day, render::STILL));
        // it wavers, a little: within a few percent of full
        let f: Vec<f64> = (0..400).map(|i| sky::flicker(100.0, 200.0, i as f64 * 0.07)).collect();
        assert!(f.iter().all(|&v| (0.94..=1.0).contains(&v)) && f.iter().any(|&v| v < 0.955) && f.iter().any(|&v| v > 0.985));
        // and a lamp's pool of light, a hundred pixels or so across, wavers as one
        let spread = |t: f64| {
            let v: Vec<f64> = (0..=10).map(|k| sky::flicker(500.0 + 15.0 * k as f64, 300.0 + 10.0 * k as f64, t)).collect();
            v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
        };
        assert!((0..100).all(|i| spread(i as f64 * 0.1) < 0.012), "a band of light runs through a lamp's pool");
    }

    #[test]
    fn clouds_are_overhead_only() {
        let map = cloud_map(3);
        assert!(map.iter().any(|&f| f) && map.iter().any(|&f| !f), "some cells filled, some clear");
        let eye = [0.5, 80.0, 0.5];
        let cam = Camera::new(eye, 0.0, 0.0, 70.0, 64, 36);
        let m = Moment::at(0.5, 6.5, 19.5, &cam, (0.47, 0.65, 1.0), (0.76, 0.85, 1.0));
        assert_eq!(cloud_at(&map, eye, [0.0, -1.0, 0.0], 0.0, &m), [0.0; 4]);
        // looking up through the slab at a filled cell's middle meets it
        let i = map.iter().position(|&f| f).unwrap();
        let (cx, cz) = (((i % CLOUD_MAP) as f64 + 0.5) * CLOUD_CELL, ((i / CLOUD_MAP) as f64 + 0.5) * CLOUD_CELL);
        let d = render::norm([cx - eye[0], CLOUD_Y + 0.1 - eye[1], cz - eye[2]]);
        assert!(cloud_at(&map, eye, d, 0.0, &m)[3] > 0.5);
    }

    #[test]
    fn the_hud_shows_the_last_minute_not_the_moment() {
        let mut m = Mean::default();
        for t in 0..60 {
            m.add(t as f64, [10.0; 4], 60.0);
        }
        // a spike moves the mean a little, not all the way
        let spiked = m.add(60.0, [100.0; 4], 60.0);
        assert!(spiked[0] > 10.0 && spiked[0] < 15.0, "{spiked:?}");
        // readings within the same second are not counted twice
        let n = m.samples.len();
        m.add(60.5, [100.0; 4], 60.0);
        assert!(m.samples.len() <= n);
        // a minute later the spike has gone from the window
        for t in 61..=121 {
            m.add(t as f64, [10.0; 4], 60.0);
        }
        assert_eq!(m.add(121.0, [10.0; 4], 60.0), [10.0; 4]);
        assert_eq!(Mean::default().add(5.0, [42.0; 4], 0.0), [42.0; 4], "no window: as it is");
    }

    #[test]
    fn the_hotbar_is_centred_on_the_screen_at_a_whole_scale() {
        let l = hud::Layout::new(1920.0, 1080.0, 0.0, 0.0);
        assert_eq!(l.s, 3.0);
        assert!((l.bar.0 + 91.0 * l.s - 960.0).abs() <= l.s);
        assert_eq!(l.bar.1 + 22.0 * l.s, 1080.0);
        // over a panel at the foot: on it, all of it moved up, at the same scale
        let p = hud::Layout::new(1920.0, 1080.0, 40.0, 0.0);
        assert_eq!((p.s, p.bar.0), (l.s, l.bar.0));
        assert_eq!(p.bar.1 + 22.0 * p.s, 1040.0);
        assert_eq!((p.status.1, p.hotbar.1), (l.status.1 - 40.0, l.hotbar.1 - 40.0));
    }
}
