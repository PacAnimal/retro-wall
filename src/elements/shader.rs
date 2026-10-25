//! A theme's own fragment shader, drawn over its `area` every frame. The theme names a GLSL file;
//! the shader gets:
//!
//! - `time`: the scene's clock, in seconds (it slows to a stop when the wall pauses)
//! - `resolution`: the screen's size in pixels
//! - `area`: its rectangle, in the screen's pixels (x, y, width, height)
//! - `readings`: up to four readings, 0 to 1 (`readings = ["cpu", "ram"]`: readings.x, readings.y)
//! - `tex0` .. `tex3`: the pictures it names (`textures = ["fog.png"]`), filtered and repeating
//! - `uv`: where the fragment is in its area, 0 to 1 from the top left
//!
//! It writes `gl_FragColor`, premultiplied (rgb already times alpha) unless `blend = "alpha"`.
//! The setting `area` is [x0, y0, x1, y1], fractions of the screen.

use crate::elements::{Validate, check, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::scene::*;
use crate::util::*;
use gl::types::{GLint, GLuint};
use serde::Deserialize;
use std::path::PathBuf;
use toml::Table;

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Blending {
    Premultiplied,
    Alpha,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    fragment: String,
    area: [f64; 4],
    readings: Vec<String>,
    textures: Vec<String>,
    blend: Blending,
    fps: f64,
}

impl Validate for Settings {
    fn validate(&self) -> Result<(), String> {
        if self.fragment.is_empty() {
            return Err("fragment: it names the shader's GLSL file".into());
        }
        let [x0, y0, x1, y1] = self.area;
        if !(0.0 <= x0 && x0 < x1 && x1 <= 1.0 && 0.0 <= y0 && y0 < y1 && y1 <= 1.0) {
            return Err(format!("area is {:?}: it is [x0, y0, x1, y1], fractions of the screen, x0 < x1 and y0 < y1", self.area));
        }
        if self.readings.len() > 4 {
            return Err(format!("readings has {}: a shader takes up to four", self.readings.len()));
        }
        if self.textures.len() > 4 {
            return Err(format!("textures has {}: a shader takes up to four", self.textures.len()));
        }
        within("fps", self.fps, 1.0, 240.0)
    }
}

/// The compiled program and where its inputs are.
struct Program {
    prog: GLuint,
    time: GLint,
    resolution: GLint,
    area: GLint,
    readings: GLint,
    /// Each texture's handle with its sampler's location.
    samplers: Vec<(GLint, Tex)>,
}

pub struct Shader {
    fragment: PathBuf,
    area: [f64; 4],
    readings: Vec<String>,
    textures: Vec<String>,
    blend: Blend,
    fps: f64,
    /// Compiled at its first draw; None after, if it failed.
    program: Option<Option<Program>>,
    bound: Vec<Tex>,
    t: f64,
    values: [f32; 4],
}

impl Shader {
    fn new(g: &Geo, t: &Table) -> Self {
        let s: Settings = settings(t);
        Shader {
            fragment: g.th.path(&s.fragment),
            area: s.area,
            readings: s.readings,
            textures: s.textures,
            blend: if s.blend == Blending::Alpha { Blend::Alpha } else { Blend::Premultiplied },
            fps: s.fps.max(1.0),
            program: None,
            bound: vec![],
            t: 0.0,
            values: [0.0; 4],
        }
    }
    /// The program, compiled once; one that fails is said, with the driver's log, and draws nothing.
    fn compile(gfx: &Gfx, path: &std::path::Path, bound: &[Tex]) -> Option<Program> {
        match crate::files::read_text(path, crate::files::Kind::Shader).map_err(|e| e.to_string()).and_then(|src| gfx.compile(&src)) {
            Ok(prog) => Some(Program {
                prog,
                time: gfx.uniform(prog, "time"),
                resolution: gfx.uniform(prog, "resolution"),
                area: gfx.uniform(prog, "area"),
                readings: gfx.uniform(prog, "readings"),
                samplers: bound.iter().zip(["tex0", "tex1", "tex2", "tex3"]).map(|(&t, n)| (gfx.uniform(prog, n), t)).collect(),
            }),
            Err(e) => {
                crate::errln!("shader {}: {e}: left out", path.display());
                None
            }
        }
    }
    fn rect(&self, g: &Geo) -> (f64, f64, f64, f64) {
        let (w, h) = (g.w as f64, g.h as f64);
        ((self.area[0] * w).floor(), (self.area[1] * h).floor(), (self.area[2] * w).ceil(), (self.area[3] * h).ceil())
    }
}

impl Part for Shader {
    fn live(&self) -> bool {
        true
    }
    fn top(&self, g: &Geo) -> i32 {
        self.rect(g).1 as i32
    }
    fn textures(&mut self, g: &Geo, _s: &Shared, up: &mut Uploads) {
        self.bound = self.textures.iter().map(|name| up.add(load_png(g.th.path(name)), Kind::Tile)).collect();
    }
    fn state(&mut self, _g: &Geo, w: &mut World) -> u64 {
        self.t = (w.clock * self.fps).round() / self.fps;
        let mut h = self.t.to_bits();
        for (i, r) in self.readings.iter().enumerate() {
            self.values[i] = (w.reading(r).0 / 100.0) as f32;
            hash_f(&mut h, self.values[i] as f64);
        }
        h
    }
    fn interval(&self) -> f64 {
        1.0 / self.fps
    }
    fn draw(&mut self, g: &Geo, gfx: &mut Gfx, _w: &World, _p: f64) {
        let (x0, y0, x1, y1) = self.rect(g);
        let program = self.program.get_or_insert_with(|| Self::compile(gfx, &self.fragment, &self.bound));
        let Some(p) = program else { return };
        let all = [
            (p.time, 1, [self.t as f32, 0.0, 0.0, 0.0]),
            (p.resolution, 2, [g.w as f32, g.h as f32, 0.0, 0.0]),
            (p.area, 4, [x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32]),
            (p.readings, 4, self.values),
        ];
        gfx.custom(p.prog, &all, &p.samplers, x0, y0, x1, y1, self.blend);
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    vec![crate::elements::Kind {
        name: "shader",
        defaults: r#"fragment = ""
area = [0.0, 0.0, 1.0, 1.0]
readings = []
textures = []
blend = "premultiplied"
fps = 30.0"#,
        varies: &[],
        check: check::<Settings>,
        make: |_, t, g| Ok(vec![Box::new(Shader::new(g, t))]),
    }]
}
