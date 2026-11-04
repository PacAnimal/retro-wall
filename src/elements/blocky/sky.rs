//! The time of day: the sun and the moon on their arcs, rising to the left of the view and setting to
//! its right, so dawn and dusk are seen; the sky's colours, its glow about the sun low down and the
//! stars at night; and the light, which lights the baked picture each frame: the sky's all about, the
//! sun's (or the moon's) on what faces it and is not in the shadows baked for its place, golden low
//! down, and the torches' (flickering) where those are less. Each is a shader, and the same in Rust for
//! the still picture.

use super::render::{Camera, HELD, PLANT, SUNS, V3, add, cross, dot, mul, norm, normal};
use crate::util::Rgb;

/// The day at a moment: where the sun and the moon are, how light it is (0 night, 1 day), how near
/// the sun is to the horizon (1 on it), and the colours that follow.
#[derive(Clone, Copy)]
pub struct Moment {
    pub sun: V3,
    pub moon: V3,
    pub day: f64,
    pub dusk: f64,
    pub top: Rgb,
    pub haze: Rgb,
    /// The sky's light on the world, from all about.
    pub light: Rgb,
    /// The sun's and the moon's light where they reach, and their places along the arc (0 to 1).
    pub sunc: Rgb,
    pub moonc: Rgb,
    pub sun_at: f64,
    pub moon_at: f64,
    pub glow: Rgb,
    pub stars: f64,
    pub cloud: Rgb,
}

const NIGHT_TOP: Rgb = (0.012, 0.016, 0.045);
const NIGHT_HAZE: Rgb = (0.035, 0.045, 0.095);
const NIGHT_LIGHT: Rgb = (0.17, 0.19, 0.32);
const DUSK: Rgb = (1.0, 0.5, 0.22);
/// The torches' light, at its brightest.
const TORCH: Rgb = (1.3, 0.98, 0.6);

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

fn times(a: Rgb, b: Rgb) -> Rgb {
    (a.0 * b.0, a.1 * b.1, a.2 * b.2)
}

/// The places along the arc the shadows are baked for.
pub fn arcs(cam: &Camera) -> [V3; SUNS] {
    std::array::from_fn(|k| arc(cam, k as f64 / (SUNS - 1) as f64))
}

/// A point on the arc the sun and moon follow, at `p` along it (0 rising, 1 setting): from 45 degrees
/// left of the view to 45 right, 55 degrees up at its highest.
pub fn arc(cam: &Camera, p: f64) -> V3 {
    let f = norm([cam.forward[0], 0.0, cam.forward[2]]);
    let r = cam.right;
    let az = (-45.0 + 90.0 * p).to_radians();
    let el = (55.0f64.to_radians()) * (std::f64::consts::PI * p).sin();
    add(mul(add(mul(f, az.cos()), mul(r, az.sin())), el.cos()), [0.0, el.sin(), 0.0])
}

impl Moment {
    /// The moment at fraction `f` of the day, the sun up from `rise` to `set` (hours).
    pub fn at(f: f64, rise: f64, set: f64, cam: &Camera, top: Rgb, haze: Rgb) -> Moment {
        let (sun, moon, _, day, dusk) = crate::day::at(rise, set, f);
        let stars = (1.0 - day) * (1.0 - day);
        let top = mix(NIGHT_TOP, top, day);
        let haze = mix(mix(NIGHT_HAZE, haze, day), (0.98, 0.62, 0.42), dusk * 0.55);
        // the sky's light from all about: cool, the shade's; the sun's warm, golden low down
        let light = mix(times(NIGHT_LIGHT, (0.75, 0.75, 0.75)), (0.56, 0.61, 0.72), day);
        let (sun_dir, moon_dir) = (arc(cam, sun), arc(cam, moon));
        let low = 1.0 - ((sun_dir[1] - 0.02) / 0.6).clamp(0.0, 1.0);
        let light = mix(light, times(light, (1.0, 0.84, 0.74)), (dusk * 0.5).max(low * 0.4 * day));
        let up = |d: V3| ((d[1] + 0.02) / 0.14).clamp(0.0, 1.0);
        // golden as it sinks: the lower, the warmer
        let gold = 1.0 - ((sun_dir[1] - 0.02) / 0.6).clamp(0.0, 1.0);
        let k = up(sun_dir) * day.max(0.15);
        let sunc = times(mix((0.68, 0.62, 0.5), (1.15, 0.62, 0.28), gold.powf(1.4)), (k, k, k));
        let k = up(moon_dir) * (1.0 - day);
        let moonc = times((0.13, 0.15, 0.24), (k, k, k));
        let cloud = mix(mix((0.1, 0.11, 0.17), (1.0, 1.0, 1.0), day), (1.0, 0.68, 0.52), dusk * 0.5 * day.max(0.3));
        Moment { sun: sun_dir, moon: moon_dir, day, dusk, top, haze, light, sunc, moonc, sun_at: sun, moon_at: moon, glow: DUSK, stars, cloud }
    }

    /// The uniforms the sky's shader takes, in the order of `SKY_UNIFORMS`.
    pub fn uniforms(&self) -> [[f32; 4]; 6] {
        let v = |c: Rgb, w: f64| [c.0 as f32, c.1 as f32, c.2 as f32, w as f32];
        let d = |p: V3, w: f64| [p[0] as f32, p[1] as f32, p[2] as f32, w as f32];
        [v(self.top, self.day), v(self.haze, self.dusk), v(self.glow, self.stars), d(self.sun, 0.0), d(self.moon, 0.0), v(self.cloud, 0.0)]
    }
}

/// A star's chance and brightness from a cell of the sky: the shaders' own hash.
fn hash3(c: [f32; 3]) -> f32 {
    ((c[0] * 12.9898 + c[1] * 78.233 + c[2] * 37.719).sin() * 43758.547).fract().abs()
}

/// A square of light about `dir` along `d`, `size` across (tangent), its glow out to `glow`: core, glow.
fn square(d: V3, dir: V3, size: f64, glow: f64) -> (f64, f64) {
    let toward = dot(d, dir);
    if toward <= 0.0 {
        return (0.0, 0.0);
    }
    let su = norm(cross(dir, [0.0, 1.0, 0.0]));
    let sv = cross(su, dir);
    let m = (dot(d, su) / toward).abs().max((dot(d, sv) / toward).abs());
    (if m < size { 1.0 } else { 0.0 }, (1.0 - (m - size) / glow).clamp(0.0, 1.0).powi(2))
}

/// The sky's colour along a ray, as `SKY` draws it.
pub fn sky_at(m: &Moment, d: V3) -> Rgb {
    let e = d[1].max(0.0);
    let mut out = mix(m.haze, m.top, (e * 2.4).min(1.0).powf(0.8));
    // the glow about the sun low down, along the horizon
    let flat = norm([d[0], 0.0, d[2]]);
    let sflat = norm([m.sun[0], 0.0, m.sun[2] + 1e-9]);
    let along = dot(flat, sflat).max(0.0);
    let g = m.dusk * along.powi(6) * (1.0 - (e / 0.45).min(1.0)).powi(2);
    out = mix(out, m.glow, g * 0.85);
    // the sun, above the horizon; the moon, pale, at night
    if m.sun[1] > -0.12 && d[1] > -0.03 {
        let (core, glow) = square(d, m.sun, 0.045, 0.08);
        let k = core.max(glow * 0.35);
        out = ((out.0 + k).min(1.0), (out.1 + k * 0.96).min(1.0), (out.2 + k * 0.82).min(1.0));
    }
    if m.moon[1] > -0.12 && d[1] > -0.03 {
        let (core, glow) = square(d, m.moon, 0.032, 0.05);
        let k = (core * 0.85).max(glow * 0.12) * (1.0 - m.day);
        out = ((out.0 + k * 0.9).min(1.0), (out.1 + k * 0.92).min(1.0), (out.2 + k).min(1.0));
    }
    // the stars: a few cells of the sky lit, small and white
    if m.stars > 0.01 && d[1] > 0.0 {
        let cell = [(d[0] * 260.0).floor() as f32, (d[1] * 260.0).floor() as f32, (d[2] * 260.0).floor() as f32];
        let h = hash3(cell);
        if h > 0.9965 {
            let k = m.stars * ((h - 0.9965) / 0.0035) as f64 * (d[1] * 8.0).min(1.0);
            out = ((out.0 + k).min(1.0), (out.1 + k).min(1.0), (out.2 + k).min(1.0));
        }
    }
    out
}

/// How lit the torches are by the daylight (0 night, 1 day): out by full day, lit through dusk, the
/// night and dawn.
pub fn lamps(day: f64) -> f64 {
    1.0 - crate::util::smooth_ab(0.6, 0.97, day)
}

/// How bright a torch is at a pixel at a moment: a slight, slow waver, as a flame's. Its phase changes
/// only over a large part of the screen, so a lamp's pool of light wavers as one and no band of light
/// runs through it; lamps far apart waver each in their own time.
pub fn flicker(px: f64, py: f64, t: f64) -> f64 {
    let ph = 3.0 * (px * 0.0021).sin() + 3.0 * (py * 0.0027).cos();
    0.97 + 0.03 * (0.6 * (t * 1.9 + ph).sin() + 0.4 * (t * 3.3 + 1.7 * ph).sin())
}

/// How much of the sun at `p` along its arc reaches a point, from the bits baked for its places: eased
/// between the two nearest.
fn reach(bits: u8, p: f64) -> f64 {
    if !(0.0..=1.0).contains(&p) {
        return 0.0;
    }
    let f = p * (SUNS - 1) as f64;
    let k = (f.floor() as usize).min(SUNS - 2);
    let bit = |k: usize| ((bits >> k) & 1) as f64;
    bit(k) + (bit(k + 1) - bit(k)) * (f - k as f64)
}

/// A side's normal (horizontal) as its angle about the upright, a whole turn in 256 (`render::SIDE`).
pub fn side_angle(n: V3) -> u8 {
    ((n[2].atan2(n[0]) / std::f64::consts::TAU).rem_euclid(1.0) * 256.0).round() as u32 as u8
}

/// How much light from `dir` falls on a side at angle `a` (`side_angle`).
fn side_falls(a: u8, dir: V3) -> f64 {
    let t = a as f64 / 256.0 * std::f64::consts::TAU;
    (t.cos() * dir[0] + t.sin() * dir[2]).max(0.0)
}

/// How much light from `dir` falls on something facing `n`.
fn falls(n: u8, dir: V3) -> f64 {
    match n {
        PLANT | HELD => 0.75 * dir[1].max(0.0) + 0.25,
        _ => dot(normal(n), dir).max(0.0),
    }
}

/// The baked picture lit, as `LAND` lights it: what is seen (premultiplied); the sky's light on it,
/// the torches' and the haze; its facing and the sun's reach. Premultiplied.
pub fn lit(m: &Moment, base: [f64; 4], light: [f64; 3], sun: (u8, u8), flick: f64) -> [f64; 4] {
    lit_by(m, base, light, sun.0, |dir| falls(sun.1, dir), flick)
}

/// `lit`, the light from a direction falling on it by `falls`.
fn lit_by(m: &Moment, base: [f64; 4], light: [f64; 3], bits: u8, falls: impl Fn(V3) -> f64, flick: f64) -> [f64; 4] {
    let a = base[3];
    let torch = light[1] * flick * lamps(m.day);
    let (s, mo) = (reach(bits, m.sun_at) * falls(m.sun), reach(bits, m.moon_at) * falls(m.moon));
    let sky = (m.light.0 + m.sunc.0 * s + m.moonc.0 * mo, m.light.1 + m.sunc.1 * s + m.moonc.1 * mo, m.light.2 + m.sunc.2 * s + m.moonc.2 * mo);
    let l = [sky.0 * light[0], sky.1 * light[0], sky.2 * light[0]];
    let l = [l[0].max(TORCH.0 * torch) + 0.012, l[1].max(TORCH.1 * torch) + 0.012, l[2].max(TORCH.2 * torch) + 0.016];
    let f = light[2];
    let haze = [m.haze.0, m.haze.1, m.haze.2];
    std::array::from_fn(|i| if i == 3 { a } else { base[i] * l[i] * (1.0 - f) + haze[i] * f * a })
}

/// How bright the hostname shows by night, of by day.
const TITLE_NIGHT: f64 = 0.35;

/// How bright the hostname shows at a moment: fully by day, darkened by night, so it does not glare
/// over the dark farm; between, as the daylight goes. It stays solid.
pub fn title_light(m: &Moment) -> f64 {
    TITLE_NIGHT + (1.0 - TITLE_NIGHT) * m.day.clamp(0.0, 1.0)
}

/// How bright a light's own flame or glass shows, over its picture's colour.
pub const FLAME: f64 = 1.15;

/// A pixel as `LAND` shows it, from the sun texture's reach, facing byte and material: `lit` (an
/// animal's side by its angle, `render::SIDE`), and where it is a light's flame or glass (`GLOW`), that
/// shining with its own colour while the lamps are lit.
pub fn shown(m: &Moment, base: [f64; 4], light: [f64; 3], sun: (u8, u8), mat: u8, flick: f64) -> [f64; 4] {
    let (bits, g) = sun;
    let out =
        if mat == super::render::SIDE { lit_by(m, base, light, bits, |dir| side_falls(g, dir), flick) } else { lit(m, base, light, (bits, g / 32), flick) };
    if mat != super::render::GLOW {
        return out;
    }
    let (on, f, a) = (lamps(m.day), light[2], base[3]);
    let haze = [m.haze.0, m.haze.1, m.haze.2];
    std::array::from_fn(|i| if i == 3 { a } else { out[i] + (base[i] * FLAME * flick * (1.0 - f) + haze[i] * f * a - out[i]) * on })
}

/// What turns a pixel into a ray, for the shaders: the camera's frame and lens.
pub const RAY: &str = "uniform vec3 eye; uniform vec3 fwd; uniform vec3 rgt; uniform vec3 upv; uniform vec4 lens;
vec3 ray(vec2 px) {
  vec2 n = vec2((2.0 * px.x / lens.z - 1.0) * lens.x * lens.y, (1.0 - 2.0 * px.y / lens.w) * lens.x);
  return normalize(fwd + rgt * n.x + upv * n.y);
}
";

/// The sky: `sky_at` for each pixel.
pub const SKY: &str = "uniform vec4 top; uniform vec4 haze; uniform vec4 glow; uniform vec4 sun; uniform vec4 moon;
float hash3(vec3 c) { return abs(fract(sin(c.x * 12.9898 + c.y * 78.233 + c.z * 37.719) * 43758.547)); }
vec2 square(vec3 d, vec3 dir, float size, float g) {
  float toward = dot(d, dir);
  if (toward <= 0.0) return vec2(0.0);
  vec3 su = normalize(cross(dir, vec3(0.0, 1.0, 0.0)));
  vec3 sv = cross(su, dir);
  float m = max(abs(dot(d, su) / toward), abs(dot(d, sv) / toward));
  float x = clamp(1.0 - (m - size) / g, 0.0, 1.0);
  return vec2(m < size ? 1.0 : 0.0, x * x);
}
void main() {
  vec2 px = area.xy + uv * area.zw;
  vec3 d = ray(px);
  float e = max(d.y, 0.0);
  vec3 c = mix(haze.rgb, top.rgb, pow(min(e * 2.4, 1.0), 0.8));
  vec3 level = normalize(vec3(d.x, 0.0, d.z));
  vec3 sflat = normalize(vec3(sun.x, 0.0, sun.z + 1e-9));
  float along = max(dot(level, sflat), 0.0);
  float g = haze.w * pow(along, 6.0) * pow(1.0 - min(e / 0.45, 1.0), 2.0);
  c = mix(c, glow.rgb, g * 0.85);
  if (sun.y > -0.12 && d.y > -0.03) {
    vec2 s = square(d, sun.xyz, 0.045, 0.08);
    float k = max(s.x, s.y * 0.35);
    c = min(c + k * vec3(1.0, 0.96, 0.82), 1.0);
  }
  if (moon.y > -0.12 && d.y > -0.03) {
    vec2 s = square(d, moon.xyz, 0.032, 0.05);
    float k = max(s.x * 0.85, s.y * 0.12) * (1.0 - top.w);
    c = min(c + k * vec3(0.9, 0.92, 1.0), 1.0);
  }
  if (glow.w > 0.01 && d.y > 0.0) {
    float h = hash3(floor(d * 260.0));
    if (h > 0.9965) c = min(c + glow.w * (h - 0.9965) / 0.0035 * min(d.y * 8.0, 1.0), 1.0);
  }
  gl_FragColor = vec4(c, 1.0);
}";
pub const SKY_UNIFORMS: [&str; 6] = ["top", "haze", "glow", "sun", "moon", "cloud"];

/// The land: the baked picture (`tex0`) lit by the light baked with it (`tex1`, `tex2`) at the moment,
/// the torches flickering, the water rippling and falling, the leaves and plants swaying.
pub const LAND: &str = "uniform vec4 light; uniform vec4 hazec; uniform float clock;
uniform vec4 sunc; uniform vec4 moonc; uniform vec4 sund; uniform vec4 moond;
float bit(float v, float k) { return mod(floor(v / exp2(k) + 0.001), 2.0); }
float reach(float bits, float p) {
  if (p < 0.0 || p > 1.0) return 0.0;
  float f = p * SUNS1;
  float k = min(floor(f), SUNS1 - 1.0);
  return mix(bit(bits, k), bit(bits, k + 1.0), f - k);
}
vec3 normal(float n) {
  if (n < 0.5) return vec3(0.0, 1.0, 0.0);
  if (n < 1.5) return vec3(0.0, -1.0, 0.0);
  if (n < 2.5) return vec3(1.0, 0.0, 0.0);
  if (n < 3.5) return vec3(-1.0, 0.0, 0.0);
  if (n < 4.5) return vec3(0.0, 0.0, 1.0);
  return vec3(0.0, 0.0, -1.0);
}
float falls(float n, vec3 dir) { return n > 5.5 ? 0.75 * max(dir.y, 0.0) + 0.25 : max(dot(normal(n), dir), 0.0); }
float flicker(vec2 p, float t) {
  float ph = 3.0 * sin(p.x * 0.0021) + 3.0 * cos(p.y * 0.0027);
  return 0.97 + 0.03 * (0.6 * sin(t * 1.9 + ph) + 0.4 * sin(t * 3.3 + 1.7 * ph));
}
void main() {
  vec2 px = area.xy + uv * area.zw;
  vec2 at = uv;
  vec4 sn = floor(texture2D(tex2, at) * 255.0 + 0.5);
  float mat = floor(sn.r / 40.0 + 0.5);
  float k = lens.w / 1080.0;
  // leaves and plants sway in gusts that pass across: each takes its colour from a pixel or so aside,
  // where that is the same stuff
  if (mat > 2.5 && mat < 4.5) {
    float gust = 0.55 + 0.45 * sin(clock * 0.35 + px.x * 0.0025 - px.y * 0.001);
    float amp = (mat > 3.5 ? 1.6 : 1.0) * k * gust;
    vec2 off = vec2(sin(clock * 1.7 + px.x * 0.013 + px.y * 0.007) * amp, 0.0);
    vec2 aside = uv + off / area.zw;
    vec4 so = floor(texture2D(tex2, aside) * 255.0 + 0.5);
    if (abs(floor(so.r / 40.0 + 0.5) - mat) < 0.5) { at = aside; sn = so; }
  }
  vec4 b = texture2D(tex0, at);
  if (b.a <= 0.0) discard;
  vec4 l = texture2D(tex1, at);
  float n = floor(sn.g / 32.0 + 0.5);
  // an animal's side, turned any way: its normal by its angle, a whole turn in 256
  bool side = abs(mat - 6.0) < 0.5;
  vec3 sideways = vec3(cos(sn.g * 0.02454369), 0.0, sin(sn.g * 0.02454369));
  float s = reach(sn.b, sunc.w) * (side ? max(dot(sideways, sund.xyz), 0.0) : falls(n, sund.xyz));
  float mo = reach(sn.b, moonc.w) * (side ? max(dot(sideways, moond.xyz), 0.0) : falls(n, moond.xyz));
  float torch = l.g * flicker(px, clock) * (1.0 - smoothstep(0.6, 0.97, light.w));
  float sky = l.r;
  float f = l.b;
  vec3 lit = max((light.rgb + sunc.rgb * s + moonc.rgb * mo) * sky, vec3(1.3, 0.98, 0.6) * torch) + vec3(0.012, 0.012, 0.016);
  vec3 c = b.rgb * lit * (1.0 - f) + hazec.rgb * f * b.a;
  if (abs(mat - 5.0) < 0.5) {
    // a light's own flame or glass shines with its own colour while the lamps are lit
    float on = 1.0 - smoothstep(0.6, 0.97, light.w);
    c = mix(c, b.rgb * FLAME * flicker(px, clock) * (1.0 - f) + hazec.rgb * f * b.a, on);
  }
  if (abs(mat - 1.0) < 0.5) {
    // the water's surface: ripples over it, where the sun reaches it glints on their crests
    vec3 d = ray(px);
    vec2 w = eye.xz + d.xz * (63.8 - eye.y) / min(d.y, -1e-3);
    float r = (sin(w.x * 1.9 + clock * 1.3) + sin(w.y * 1.6 - clock * 1.1) + sin((w.x + w.y) * 1.2 + clock * 0.8)) / 3.0;
    c *= 1.0 + 0.08 * r;
    c += sunc.rgb * s * pow(max(r, 0.0), 6.0) * 0.6 * b.a * (1.0 - f);
  } else if (abs(mat - 2.0) < 0.5) {
    // falling water: light runs down it, each column in its own time
    float col = fract(sin(floor(px.x / (3.0 * k)) * 12.9898) * 43758.547);
    float run = fract(px.y / (5.0 * k) - clock * 2.2 + col);
    c *= run < 0.25 ? 1.3 : 0.94;
  }
  gl_FragColor = vec4(c, b.a);
}";
pub const LAND_UNIFORMS: [&str; 10] = ["light", "hazec", "clock", "sunc", "moonc", "sund", "moond", "tex0", "tex1", "tex2"];
