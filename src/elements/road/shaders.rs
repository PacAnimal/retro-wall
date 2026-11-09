//! The GLSL of the sky, the road and the CRT, and their uniforms.

pub(super) const SKY_UNIFORMS: [&str; 8] = ["view", "top", "mid", "low", "sun", "glow", "band", "area"];
pub(super) const SKY_SIZES: [usize; 8] = [4, 3, 3, 3, 4, 3, 4, 4];
/// How many street lamps' light the road's shader takes: as many as are near enough to show.
pub(super) const POOLS: usize = 8;
pub(super) const ROAD_UNIFORMS: [&str; 15] =
    ["view", "motion", "grass", "grass2", "asphalt", "curb", "cream", "dash", "haze", "tint", "beam", "sheen", "glow", "rows", "area"];
pub(super) const ROAD_SIZES: [usize; 15] = [4, 4, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 3, 4, 4];
pub(super) const CRT_UNIFORMS: [&str; 4] = ["src", "mask", "area", "tex0"];

/// What every shader of this element shares: the grid of pixels and the colours. `cell` is the grid's
/// pixel a screen position is in; `centre` that pixel's middle; `bayer` its threshold in a 4 x 4
/// ordered dither, 0 to 1; `quant` a colour brought to `levels` a channel, dithered by it.
pub(super) const DOS: &str = "float b2(vec2 v) { v = mod(v, 2.0); return mod(2.0 * v.x + 3.0 * v.y, 4.0); }
float bayer(vec2 cell) { return (4.0 * b2(cell) + b2(floor(cell / 2.0)) + 0.5) / 16.0; }
vec2 cell(vec2 p, float pix) { return floor(p / max(pix, 1.0)); }
vec2 centre(vec2 p, float pix) { return pix > 1.0 ? (cell(p, pix) + 0.5) * pix : p; }
vec3 quant(vec3 c, float levels, float b) { float l = levels - 1.0; return clamp(floor(c * l + b) / l, 0.0, 1.0); }
";

/// The sky: three colours from the top to the horizon, the glow about the sun, a warm band along the
/// horizon at dawn and dusk, brightest under the sun. On the grid, as an old game's sky: its gradient
/// and its glow in steps, each step's edge dithered.
pub(super) const SKY: &str = "uniform vec4 view; uniform vec3 top; uniform vec3 mid; uniform vec3 low;
uniform vec4 sun; uniform vec3 glow; uniform vec4 band;
void main() {
  vec2 raw = area.xy + uv * area.zw;
  vec2 px = centre(raw, band.z);
  vec2 c = (px - view.yz) / view.x;
  float b = bayer(cell(raw, band.z));
  bool grid = band.z > 1.0;
  float h = clamp(c.y / view.w, 0.0, 1.0);
  if (grid) h = floor(h * 18.0 + b) / 18.0;
  vec3 col = mix(top, mid, smoothstep(0.0, 0.62, h));
  col = mix(col, low, smoothstep(0.5, 1.0, h));
  vec2 d = c - sun.xy;
  d.y *= 1.35;
  float r = length(d) / sun.z;
  float g = sun.w * (0.55 * exp(-r * r * 1.6) + 0.45 * exp(-r * 1.4));
  if (grid) g = floor(g * 9.0 + b) / 9.0;
  col = 1.0 - (1.0 - col) * (1.0 - glow * clamp(g, 0.0, 1.0));
  float az = exp(-pow((c.x - sun.x) / band.y, 2.0));
  if (grid) az = floor(az * 6.0 + b) / 6.0;
  col = mix(col, glow, band.x * smoothstep(0.5, 1.0, h) * smoothstep(0.5, 1.0, h) * (0.3 + 0.7 * az));
  gl_FragColor = vec4(grid ? quant(col, band.w, b) : col, 1.0);
}";

/// The ground and the road, row by row from the table (`tex0`): the land in bands, the road's curbs,
/// centre line and surface, all moving towards the viewer, beyond the road's end the land in the haze,
/// and the sky left alone. The far bands are filtered over a pixel of the grid, so they dither rather
/// than shimmer; the edges are sharp, as on the grid they would be. The scene's light over them, the
/// headlights' beams on the road at night, the haze at the horizon, the glare of a low sun.
pub(super) const ROAD_SHADER: &str = "uniform vec4 view; uniform vec4 motion; uniform vec3 grass; uniform vec3 grass2;
uniform vec3 asphalt; uniform vec3 curb; uniform vec3 cream; uniform vec3 dash; uniform vec3 haze;
uniform vec3 tint; uniform vec3 beam; uniform vec4 sheen; uniform vec3 glow; uniform vec4 rows;
uniform vec3 lampc; uniform vec4 pools[POOLS]; TABLE_DECL
float rise(float x, float duty) { return floor(x) * duty + min(fract(x), duty); }
float pulse(float p, float duty, float fw) {
  fw = max(fw, 1e-4);
  return clamp((rise(p + 0.5 * fw, duty) - rise(p - 0.5 * fw, duty)) / fw, 0.0, 1.0);
}
float inside(float d, float edge, float fw) { return fw <= 0.0 ? step(d, edge) : 1.0 - smoothstep(edge - 0.5 * fw, edge + 0.5 * fw, d); }
// the road or land at a screen position, and whether there is any (0: sky); `ps` the screen pixels a
// fragment covers
vec4 shade(vec2 p, float ps) {
  vec2 c = (p - view.yz) / view.x;
  float r = floor((c.y - rows.x) / rows.y);
  if (r < 0.0 || r >= rows.z) return vec4(0.0);
  TABLE_READ
  float zc = e2.x;
  // sky only above the horizon: under it, whatever the table says, it is the land beyond the road
  if (zc < 0.5) { if (c.y < rows.w) return vec4(0.0); zc = 65535.0; }
  vec3 col;
  if (zc > 65534.5) {
    col = mix(mix(grass, grass2, 0.5) * tint, haze, motion.w);
  } else {
    float z = zc / 65533.0 * 128.0;
    float off = e2.y / 65535.0 * 512.0 - 256.0;
    float q = clamp(FOCAL / z / (900.0 - HORIZON), 0.0, 1.5);
    float d = (c.x - VANISH) * z / (FOCAL * ROAD_HALF) + VIEW - off;
    float s = z + motion.x;
    float ad = abs(d);
    // how far the road runs, and across it, in what a fragment covers: the bands' and edges' filter
    float fz = z * z * ps / view.x / FOCAL;
    float fw = z * ps / (view.x * FOCAL * ROAD_HALF);
    col = mix(grass, grass2, pulse(s / 6.0, 0.42, fz / 6.0));
    col = mix(col, mix(cream, curb, pulse(s / 1.9, 0.5, fz / 1.9)), inside(ad, 1.055, fw));
    vec3 road = mix(asphalt, asphalt * 1.06, pulse(s / 6.0, 0.42, fz / 6.0));
    col = mix(col, road, inside(ad, 1.0, fw));
    col = mix(col, dash, inside(ad, 0.008, fw) * pulse(s / 3.4, 0.45, fz / 3.4));
    col *= tint;
    // the headlights: two cones from the car's nose, one from each lamp, each spreading thirty degrees,
    // fifteen to a side, fading far off; nothing beside or behind the nose
    float ahead = z - Z_CAR - NOSE;
    float qq = max(q, 0.0);
    float lit = 0.0;
    if (ahead > 0.0) {
      float half_w = (0.12 + 0.268 * ahead) / ROAD_HALF;
      for (int k = 0; k < 2; k++) {
        float lamp = motion.y + (float(k) * 2.0 - 1.0) * LAMPS;
        lit += 1.0 - smoothstep(0.5 * half_w, half_w, abs(d - lamp));
      }
      lit = min(lit, 1.0) * smoothstep(0.0, 1.5, ahead) * (1.0 - smoothstep(25.0, 60.0, ahead)) * motion.z;
    }
    col += beam * lit * 0.55 / (1.0 + 0.05 * max(ahead, 0.0)) * (0.35 + 0.65 * inside(ad, 1.055, fw));
    // the street lamps' light: about the ground under each lamp's head, by its true distance there,
    // longer along the road than across it, so that from this low it looks round; the road takes it,
    // the grass much less
    float pl = 0.0;
    for (int k = 0; k < POOLS; k++) {
      vec4 P = pools[k];
      float dz = (z - P.x) / 1.8;
      float dx = (d - P.y) * ROAD_HALF;
      pl += P.z * exp(-(dx * dx + dz * dz) / 5.8);
    }
    col += lampc * pl * 0.34 * mix(0.3, 1.0, inside(ad, 1.055, fw));
    col = mix(col, haze, motion.w * exp(-qq * 14.0));
    float under = exp(-pow((c.x - sheen.x) / (120.0 + 900.0 * qq), 2.0));
    col = 1.0 - (1.0 - col) * (1.0 - glow * sheen.y * under * exp(-qq * 5.0) * (0.35 + 0.65 * inside(ad, 1.0, fw)));
  }
  return vec4(col, 1.0);
}
void main() {
  vec2 raw = area.xy + uv * area.zw;
  // a fragment's height on the screen: on the low frame a whole pixel of it; its edge with the sky
  // taken in four rows, so the horizon and the crests glide rather than step a pixel at a time
  float ps = max(fwidth(raw.y), 1.0);
  vec3 sum = vec3(0.0);
  float n = 0.0;
  for (int k = 0; k < 4; k++) {
    vec4 v = shade(vec2(raw.x, raw.y + ps * ((float(k) + 0.5) / 4.0 - 0.5)), ps);
    sum += v.rgb * v.a;
    n += v.a;
  }
  if (n < 0.5) discard;
  float a = n / 4.0;
  gl_FragColor = vec4(sum / 4.0, a);
}";

/// The CRT: the low frame (`tex0`, `src.xy` pixels) shown as a tube would show it, after crt-lottes
/// (Timothy Lottes, public domain): each row a beam, bright at its middle and dark between rows
/// (Gaussian, `src.z`), each pixel spread a little into its neighbours along the row (`src.w`), the
/// rows above and below lending their light, then the phosphors' mask, a stretched VGA shadow mask
/// (`mask.x` dark, `mask.y` light), and the light the mask and the rows took made back (`mask.z`). In
/// a near linear light (squared), back by the square root.
pub(super) const CRT: &str = "vec3 fetch(vec2 pos, vec2 off) {
  vec2 p = (floor(pos * src.xy + off) + 0.5) / src.xy;
  vec3 c = texture2D(tex0, vec2(p.x, 1.0 - p.y)).rgb;
  return c * c;
}
vec2 dist(vec2 pos) { pos *= src.xy; return -((pos - floor(pos)) - 0.5); }
vec3 horz3(vec2 pos, float off) {
  vec3 b = fetch(pos, vec2(-1.0, off));
  vec3 c = fetch(pos, vec2(0.0, off));
  vec3 d = fetch(pos, vec2(1.0, off));
  float dst = dist(pos).x;
  float wb = gaus(dst - 1.0, src.w);
  float wc = gaus(dst, src.w);
  float wd = gaus(dst + 1.0, src.w);
  return (b * wb + c * wc + d * wd) / (wb + wc + wd);
}
float scan(vec2 pos, float off) { return gaus(dist(pos).y + off, src.z); }
vec3 tri(vec2 pos) {
  return horz3(pos, -1.0) * scan(pos, -1.0) + horz3(pos, 0.0) * scan(pos, 0.0) + horz3(pos, 1.0) * scan(pos, 1.0);
}
void main() {
  vec3 c = tri(uv) * shadow(floor(gl_FragCoord.xy)) * mask.z;
  gl_FragColor = vec4(sqrt(max(c, 0.0)), 1.0);
}";

/// How much finer than the low frame the fine frame is, each way (`Road::fine`): even, for RESOLVE.
pub(super) const FINE: i32 = 4;
/// The beam's spread along a row (crt-lottes's hardPix), over the fine frame's smooth signal: as sharp
/// as the CRT's -3 over whole pixels (four fifths of a pixel's light its own at its middle).
pub(super) const HARD_PIX: f64 = -4.75;

/// The fine frame's rows, as the CRT's beams take them (`Road::fine`): each of the low frame's rows
/// the mean of its FINE fine ones (two at a time, between texels), spread along the row as the beam
/// spreads it, in linear light (`src`: the fine frame's size, the spread). Read smooth along the row.
pub(super) const RESOLVE: &str = "uniform vec4 src;
void main() {
  float col = (floor(uv.x * src.x) + 0.5) / src.x;
  float row = floor(uv.y * src.y / FINE) * FINE;
  vec3 sum = vec3(0.0);
  float ws = 0.0;
  for (int j = -REACH; j <= REACH; j++) {
    float d = float(j) / FINE;
    float w = exp2(src.z * d * d);
    float x = col + float(j) / src.x;
    vec3 c = vec3(0.0);
    for (int k = 1; k < int(FINE); k += 2) {
      vec3 t = texture2D(tex0, vec2(x, 1.0 - (row + float(k)) / src.y)).rgb;
      c += t * t;
    }
    sum += w * c;
    ws += w;
  }
  gl_FragColor = vec4(sqrt(sum / (ws * FINE * 0.5)), 1.0);
}";

/// What both CRTs share: the beam's profile (`gaus`) and the phosphors' mask (`shadow`).
pub(super) const CRT_COMMON: &str = "uniform vec4 src; uniform vec4 mask;
float gaus(float p, float s) { return exp2(s * p * p); }
vec3 shadow(vec2 p) {
  vec3 m = vec3(mask.x);
  p.x += p.y * 3.0;
  float f = fract(p.x / 6.0);
  if (f < 0.333) m.r = mask.y; else if (f < 0.666) m.g = mask.y; else m.b = mask.y;
  return m;
}
";

/// The CRT over the fine frame's rows (`RESOLVE`): as `CRT`, each row read smooth along it, already
/// spread as the beam spreads it.
pub(super) const CRT_FINE: &str = "vec3 row(vec2 pos, float off) {
  float r = (floor(pos.y * src.y + off) + 0.5) / src.y;
  vec3 c = texture2D(tex0, vec2(pos.x, 1.0 - r)).rgb;
  return c * c;
}
float scan(vec2 pos, float off) { float d = -((pos.y * src.y - floor(pos.y * src.y)) - 0.5); return gaus(d + off, src.z); }
void main() {
  vec3 c = row(uv, -1.0) * scan(uv, -1.0) + row(uv, 0.0) * scan(uv, 0.0) + row(uv, 1.0) * scan(uv, 1.0);
  c *= shadow(floor(gl_FragCoord.xy)) * mask.z;
  gl_FragColor = vec4(sqrt(max(c, 0.0)), 1.0);
}";
