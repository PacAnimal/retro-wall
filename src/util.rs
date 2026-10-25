//! Drawing helpers shared by the elements: surfaces, resampling, supersampling, seamless tiles,
//! glows, drop shadows, SVG paths, vertex arrays and mountain ranges.

use cairo::{Context, Filter, Format, ImageSurface, LinearGradient, SurfacePattern};

/// "#rrggbb" as 0..1 RGB; black if it is not one.
pub fn rgb(hex: &str) -> (f64, f64, f64) {
    crate::tweaks::Colour::parse(hex).map_or((0.0, 0.0, 0.0), |c| c.rgb())
}

pub fn unit(width: i32, height: i32) -> f64 {
    width.min(height) as f64
}

/// The largest side cairo makes a surface.
pub const MAX_SIDE: i32 = 32767;

/// A surface of `format`, its sides brought within 1 to MAX_SIDE.
pub fn surface_of(format: Format, w: i32, h: i32) -> ImageSurface {
    let (w, h) = (w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE));
    #[cfg(target_os = "macos")]
    if let Some(s) = mapped::surface(format, w, h) {
        return s;
    }
    let made = ImageSurface::create(format, w, h).or_else(|_| ImageSurface::create(format, 1, 1));
    made.unwrap_or_else(|e| panic!("cairo cannot make a 1x1 surface: {e}"))
}

/// Large pictures in memory mapped for each: macOS's allocator keeps a large block it is given back,
/// dirty, in the process's footprint; a mapping of our own goes back to the system when the picture
/// does.
#[cfg(target_os = "macos")]
mod mapped {
    use cairo::{Format, ImageSurface};

    /// The least a picture takes to be mapped.
    const LEAST: usize = 1 << 20;

    struct Pixels {
        ptr: *mut u8,
        len: usize,
    }

    // SAFETY: the mapping belongs to this value alone (cairo reaches it only through the surface that
    // owns this value), and mapping memory is not tied to a thread: it may be used and unmapped on any.
    unsafe impl Send for Pixels {}

    impl AsMut<[u8]> for Pixels {
        fn as_mut(&mut self) -> &mut [u8] {
            // SAFETY: ptr is a live read-write mapping of len bytes, owned by self; the slice borrows self
            // mutably, so it is the only one made through self.
            unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
        }
    }

    impl Drop for Pixels {
        fn drop(&mut self) {
            // SAFETY: ptr and len are the mapping made in `surface`; dropped once, so unmapped once.
            unsafe { libc::munmap(self.ptr.cast(), self.len) };
        }
    }

    /// A picture w by h in a mapping of its own (zeroed, as cairo's own are); None if it is small.
    pub fn surface(format: Format, w: i32, h: i32) -> Option<ImageSurface> {
        let stride = format.stride_for_width(w as u32).ok()?;
        let len = stride as usize * h as usize;
        if len < LEAST {
            return None;
        }
        // SAFETY: an anonymous private mapping, no file and no fixed address: it aliases nothing.
        let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0) };
        if ptr == libc::MAP_FAILED {
            return None;
        }
        ImageSurface::create_for_data(Pixels { ptr: ptr.cast(), len }, format, w, h, stride).ok()
    }
}

pub fn surface(w: i32, h: i32) -> ImageSurface {
    surface_of(Format::ARgb32, w, h)
}

pub fn surface_rgb(w: i32, h: i32) -> ImageSurface {
    surface_of(Format::Rgb24, w, h)
}

pub fn ctx(s: &ImageSurface) -> Context {
    Context::new(s).unwrap_or_else(|e| panic!("cairo cannot draw on a surface it made: {e}"))
}

/// `source` scaled by `factor` with the given filter.
pub fn resampled(source: &ImageSurface, factor: f64, filter: Filter) -> ImageSurface {
    let (w, h) = (source.width(), source.height());
    let out = surface(((w as f64 * factor).round() as i32).max(1), ((h as f64 * factor).round() as i32).max(1));
    {
        let c = ctx(&out);
        c.scale(out.width() as f64 / w as f64, out.height() as f64 / h as f64);
        let p = SurfacePattern::create(source);
        p.set_filter(filter);
        let _ = c.set_source(&p);
        let _ = c.paint();
    }
    out
}

/// Draw at `scale` times the size and filter down. Only `bbox` is drawn large, if given.
pub fn supersampled(width: i32, height: i32, scale: i32, draw: impl FnOnce(&Context), bbox: Option<(f64, f64, f64, f64)>) -> ImageSurface {
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, width, height);
    if let Some(b) = bbox
        && scale > 1
    {
        x0 = 0.max(b.0.floor() as i32 - 2);
        y0 = 0.max(b.1.floor() as i32 - 2);
        x1 = width.min(b.2.ceil() as i32 + 2);
        y1 = height.min(b.3.ceil() as i32 + 2);
        if x1 <= x0 || y1 <= y0 {
            return surface(width, height);
        }
    }
    let big = surface((x1 - x0) * scale, (y1 - y0) * scale);
    {
        let c = ctx(&big);
        c.scale(scale as f64, scale as f64);
        c.translate(-x0 as f64, -y0 as f64);
        draw(&c);
    }
    let small = resampled(&big, 1.0 / scale as f64, Filter::Good);
    if (x0, y0, x1, y1) == (0, 0, width, height) {
        return small;
    }
    let out = surface(width, height);
    {
        let c = ctx(&out);
        let _ = c.set_source_surface(&small, x0 as f64, y0 as f64);
        let _ = c.paint();
    }
    out
}

/// A tile that repeats without seams. `reach` is how far a stroke may cross an edge.
pub fn tile(width: i32, height: i32, paint: impl FnOnce(&Context), reach: Option<f64>) -> ImageSurface {
    let m = width.min(height).min(reach.unwrap_or(width.max(height) as f64 / 2.0).ceil() as i32);
    let canvas = surface(width + 2 * m, height + 2 * m);
    {
        let c = ctx(&canvas);
        c.translate(m as f64, m as f64);
        paint(&c);
    }
    let out = surface(width, height);
    {
        let c = ctx(&out);
        for dx in [-width, 0, width] {
            for dy in [-height, 0, height] {
                let _ = c.set_source_surface(&canvas, (-m - dx) as f64, (-m - dy) as f64);
                let _ = c.paint();
            }
        }
    }
    out
}

/// Paint `surface` with a blurred halo under it.
pub fn glowing(c: &Context, s: &ImageSurface, passes: &[(f64, f64)]) {
    for &(blur, alpha) in passes {
        let small = resampled(s, 1.0 / blur, Filter::Good);
        let halo = resampled(&small, blur, Filter::Bilinear);
        let _ = c.set_source_surface(&halo, 0.0, 0.0);
        let _ = c.paint_with_alpha(alpha);
    }
    let _ = c.set_source_surface(s, 0.0, 0.0);
    let _ = c.paint();
}

pub use crate::tweaks::Shadow;

/// `surface` with a drop shadow, cast by `shape` if given.
pub fn drop_shadow(s: ImageSurface, sh: &Shadow, scale: f64, shape: Option<&ImageSurface>) -> ImageSurface {
    if sh.strength <= 0.0 {
        return s;
    }
    let (w, h) = (s.width(), s.height());
    let k = (sh.blur * scale).max(1.0);
    let caster = shape.unwrap_or(&s);
    let blurred;
    let mask: &ImageSurface = if k > 1.0 {
        let small = resampled(caster, 1.0 / k, Filter::Good);
        let m = surface(w, h);
        {
            let c = ctx(&m);
            c.scale(w as f64 / small.width() as f64, h as f64 / small.height() as f64);
            let p = SurfacePattern::create(&small);
            p.set_filter(Filter::Bilinear);
            let _ = c.set_source(&p);
            let _ = c.paint();
        }
        blurred = m;
        &blurred
    } else {
        caster
    };
    let out = surface(w, h);
    {
        let c = ctx(&out);
        let (r, g, b) = sh.colour.rgb();
        let (ox, oy) = ((sh.dx * scale).round(), (sh.dy * scale).round());
        c.set_source_rgba(r, g, b, sh.strength.min(1.0));
        let _ = c.mask_surface(mask, ox, oy);
        if sh.strength > 1.0 {
            c.set_source_rgba(r, g, b, (sh.strength - 1.0).min(1.0));
            let _ = c.mask_surface(mask, ox, oy);
        }
        let _ = c.set_source_surface(&s, 0.0, 0.0);
        let _ = c.paint();
    }
    out
}

pub fn gradient(y0: f64, y1: f64, stops: &[(f64, crate::tweaks::Colour)]) -> LinearGradient {
    let g = LinearGradient::new(0.0, y0, 0.0, y1);
    for &(at, c) in stops {
        g.add_color_stop_rgb(at, c.0, c.1, c.2);
    }
    g
}

/// A 1x1 A8 context for measuring text.
pub fn probe() -> Context {
    ctx(&surface_of(Format::A8, 1, 1))
}

pub fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A surface's pixels, read in place: stride x height bytes, flushed. None while anything else holds
/// the surface (a context, a pattern, another handle), so nothing can draw on it while they are read.
pub fn view(s: &mut ImageSurface) -> Option<cairo::ImageSurfaceData<'_>> {
    s.data().ok()
}

/// A surface's pixels, copied (empty while anything else holds the surface: see `view`).
pub fn pixels(s: &mut ImageSurface) -> Vec<u8> {
    view(s).map_or_else(Vec::new, |d| d.to_vec())
}

/// A PNG; a missing or broken one is said once and drawn as nothing.
pub fn load_png(path: impl AsRef<std::path::Path>) -> ImageSurface {
    let path = path.as_ref();
    let read = || crate::files::open(path, crate::files::Kind::Picture).map_err(|e| e.to_string());
    match read().and_then(|mut f| ImageSurface::create_from_png(&mut f).map_err(|e| e.to_string())) {
        Ok(s) => s,
        Err(e) => {
            static SAID: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());
            let mut said = SAID.lock().unwrap();
            if !said.iter().any(|p| p == path) {
                crate::errln!("{}: {e}: drawn as nothing", path.display());
                said.push(path.to_path_buf());
            }
            surface(1, 1)
        }
    }
}

/// A stable number in [0, 1) from the given integers. Same keys, same result, every run.
pub fn chance(keys: &[i64]) -> f64 {
    let mut h: u64 = 0x9E3779B97F4A7C15;
    for &k in keys {
        h = (h ^ k as u64).wrapping_mul(0xBF58476D1CE4E5B9);
        h ^= h >> 31;
    }
    (h & 0xFFFFFFFF) as f64 / 4294967296.0
}

/// A round light with a glow, white on transparency, `size` across. `stops` are
/// (fraction of radius, alpha).
pub fn soft_dot(size: i32, stops: &[(f64, f64)]) -> ImageSurface {
    let dot = surface(size, size);
    {
        let c = ctx(&dot);
        let s = size as f64;
        let g = cairo::RadialGradient::new(s / 2.0, s / 2.0, 0.0, s / 2.0, s / 2.0, s / 2.0);
        for &(at, alpha) in stops {
            g.add_color_stop_rgba(at, 1.0, 1.0, 1.0, alpha);
        }
        let _ = c.set_source(&g);
        let _ = c.paint();
    }
    dot
}

/// A tinted textured quad as a vertex array's 32 numbers. `uv` is the texture part (u0, v0, u1, v1).
pub fn image_quad(x0: f64, y0: f64, x1: f64, y1: f64, colour: (f64, f64, f64), alpha: f64, uv: (f64, f64, f64, f64)) -> [f64; 32] {
    let (r, g, b) = (colour.0 * alpha, colour.1 * alpha, colour.2 * alpha);
    let (u0, v0, u1, v1) = uv;
    [u0, v0, r, g, b, alpha, x0, y0, u1, v0, r, g, b, alpha, x1, y0, u1, v1, r, g, b, alpha, x1, y1, u0, v1, r, g, b, alpha, x0, y1]
}

/// Paint textured, tinted quads with cairo, blended premultiplied as Gfx does. Used to draw a
/// live part at rest.
pub fn paint_vertices(c: &Context, s: &ImageSurface, data: &[f32]) {
    let (w, h) = (s.width() as f64, s.height() as f64);
    for q in data.as_chunks::<32>().0 {
        let q: Vec<f64> = q.iter().map(|&v| v as f64).collect();
        let alpha = q[5];
        if alpha <= 0.0 {
            continue;
        }
        let ((x0, y0), (x1, y1), (x2, y2), (x3, y3)) = ((q[6], q[7]), (q[14], q[15]), (q[22], q[23]), (q[30], q[31]));
        // the quad's own piece of the texture: (u0, v0) at its first corner, u along to the second, v to the fourth
        let (u0, v0, du, dv) = (q[0], q[1], q[8] - q[0], q[25] - q[1]);
        if du == 0.0 || dv == 0.0 {
            continue;
        }
        let _ = c.save();
        if (u0, v0, du, dv) != (0.0, 0.0, 1.0, 1.0) {
            c.move_to(x0, y0);
            c.line_to(x1, y1);
            c.line_to(x2, y2);
            c.line_to(x3, y3);
            c.close_path();
            c.clip();
        }
        let (xx, yx, xy, yy) = ((x1 - x0) / (du * w), (y1 - y0) / (du * w), (x3 - x0) / (dv * h), (y3 - y0) / (dv * h));
        c.transform(cairo::Matrix::new(xx, yx, xy, yy, x0 - u0 * w * xx - v0 * h * xy, y0 - u0 * w * yx - v0 * h * yy));
        c.set_source_rgba(q[2] / alpha, q[3] / alpha, q[4] / alpha, alpha);
        let _ = c.mask_surface(s, 0.0, 0.0);
        let _ = c.restore();
    }
}

// --- helpers for the phosphor theme and the title ------------------------------------------------

impl crate::config::Theme {
    /// The horizon line, a fraction HORIZON_Y down the screen.
    pub fn horizon_y(&self, height: i32) -> f64 {
        self.tw.horizon_y * height as f64
    }

    /// How a picture of `size` (w, h) covers the screen, cropped about CROP_CENTRE: (scale, x, y),
    /// where (x, y) is its top left.
    pub fn covering(&self, width: i32, height: i32, size: (f64, f64)) -> (f64, f64, f64) {
        let (iw, ih) = size;
        let crop = self.tw.crop_centre;
        let scale = (width as f64 / iw).max(height as f64 / ih);
        (scale, (width as f64 - iw * scale) * crop[0], (height as f64 - ih * scale) * crop[1])
    }
}

/// The fill extents (x0, y0, x1, y1) of the shape `trace` draws.
pub fn traced(trace: impl FnOnce(&Context)) -> (f64, f64, f64, f64) {
    let c = probe();
    trace(&c);
    c.fill_extents().unwrap_or((0.0, 0.0, 0.0, 0.0))
}

/// The pixels a span from a to b covers, as (first, last + 1, coverage). Partly covered pixels
/// are split off, so edges between pixels are antialiased.
pub fn spans(a: f64, b: f64) -> Vec<(f64, f64, f64)> {
    let (fa, fb) = (a.floor(), b.floor());
    if fa == fb {
        return if b > a { vec![(fa, fa + 1.0, b - a)] } else { vec![] };
    }
    let mut out = if a > fa { vec![(fa, fa + 1.0, fa + 1.0 - a)] } else { vec![] };
    let start = if a > fa { fa + 1.0 } else { fa };
    if fb > start {
        out.push((start, fb, 1.0));
    }
    if b > fb {
        out.push((fb, fb + 1.0, b - fb));
    }
    out
}

/// A rectangle with antialiased edges at `alpha`, as pixel-aligned pieces (x0, y0, x1, y1, alpha).
pub fn box_(x0: f64, y0: f64, x1: f64, y1: f64, alpha: f64) -> Vec<(f64, f64, f64, f64, f64)> {
    let mut out = vec![];
    for (ya, yb, cy) in spans(y0, y1) {
        for (xa, xb, cx) in spans(x0, x1) {
            out.push((xa, ya, xb, yb, alpha * cx * cy));
        }
    }
    out
}

pub type Rgb = (f64, f64, f64);

/// Mix colour a toward b by t (RGB, 0..1).
pub fn mixed(a: Rgb, b: Rgb, t: f64) -> Rgb {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

pub fn polygon(c: &Context, pts: &[(f64, f64)]) {
    c.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        c.line_to(p.0, p.1);
    }
    c.close_path();
}

/// A line from a to b, roughened by midpoint displacement of at most `rough` pixels.
pub fn jagged(rng: &mut crate::rng::Rng, ends: &[(f64, f64)], rough: f64) -> Vec<(f64, f64)> {
    let mut pts = ends.to_vec();
    let mut rough = rough;
    for _ in 0..4 {
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            let dx = rng.range(-rough, rough) * 0.3;
            let dy = rng.range(-rough, rough);
            out.push(((x0 + x1) / 2.0 + dx, (y0 + y1) / 2.0 + dy));
            out.push((x1, y1));
        }
        pts = out;
        rough *= 0.55;
    }
    pts
}

/// The colours of a range: lit face, snow edge, shade and bare rock.
pub struct Light {
    pub lit: Rgb,
    pub edge: Rgb,
    pub shade: Rgb,
    pub rock: Rgb,
}

/// One row of peaks for peaks(). `rise` is a fraction of the unit, `count` is peaks across a
/// 1920 width, `snowy` and `haze` are 0 to 1.
pub struct Range {
    pub colour: Rgb,
    pub rise: f64,
    pub count: i64,
    pub snowy: f64,
    pub haze: f64,
    pub rough: f64,
    pub shoulders: bool,
    pub spread: (f64, f64),
}

/// A peak's detail: lesser ridges down its faces, slices a shade apart, and fine streaks of snow
/// and rock.
pub fn facets(
    c: &Context,
    rng: &mut crate::rng::Rng,
    l: &Light,
    x: f64,
    apex: f64,
    left: f64,
    right: f64,
    hz: f64,
    tall: f64,
    outline: &[(f64, f64)],
    haze: f64,
) {
    let _ = c.save();
    polygon(c, outline);
    c.clip();
    let strength = 1.0 - haze;
    for _ in 0..rng.int(2, 4) {
        let top = (x + rng.range(-0.4, 0.4) * left.min(right), apex + tall * rng.range(0.1, 0.45));
        let foot = (top.0 + rng.range(-0.6, 0.6) * tall, hz + 1.0);
        let side = *rng.pick(&[-1.0, 1.0]) * tall * rng.range(0.3, 0.7);
        polygon(c, &[top, foot, (foot.0 + side, hz + 1.0)]);
        let col = if rng.f64() < 0.5 { l.shade } else { l.edge };
        c.set_source_rgba(col.0, col.1, col.2, 0.07 * strength);
        let _ = c.fill();
    }
    for _ in 0..(tall / 2.0).round() as i64 {
        let sx = x + rng.range(-left, right) * 0.8;
        let sy = apex + (hz - apex) * rng.range(0.05, 0.8);
        let length = tall * rng.range(0.08, 0.3);
        let lean = (sx - x) / tall.max(1.0) * 0.5;
        c.move_to(sx, sy);
        c.line_to(sx + lean * length, sy + length);
        c.set_line_width((tall / 120.0).max(0.5));
        let col = if rng.f64() < 0.5 { l.edge } else { l.rock };
        c.set_source_rgba(col.0, col.1, col.2, 0.18 * strength);
        let _ = c.stroke();
    }
    let _ = c.restore();
}

/// Mountain ranges on the horizon `hz`, far ranges first. Each peak is lit on the left and shaded
/// on the right. Lower peaks are drawn first, so higher ones stand in front.
pub fn peaks(c: &Context, rng: &mut crate::rng::Rng, width: i32, hz: f64, u: f64, ranges: &[Range], l: &Light) {
    let w = width as f64;
    for r in ranges {
        let colour = r.colour;
        let (wide0, wide1) = r.spread;
        let mut peaks = vec![];
        for _ in 0..((r.count * width as i64) as f64 / 1920.0).round() as i64 {
            let x = rng.range(-0.05, 1.05) * w;
            let tall = r.rise * u * rng.range(0.25, 1.0).powf(1.5);
            let left = tall * rng.range(wide0, wide1);
            let right = tall * rng.range(wide0, wide1);
            peaks.push((tall, x, left, right));
        }
        peaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for (tall, x, left, right) in peaks {
            let apex = hz - tall;
            let mut lit_ends = vec![(x - left, hz + 1.0), (x, apex)];
            let mut dark_ends = vec![(x, apex), (x + right, hz + 1.0)];
            if r.shoulders {
                for (ends, side, reach) in [(&mut lit_ends, -1.0, left), (&mut dark_ends, 1.0, right)] {
                    let f = rng.range(0.35, 0.6);
                    let px = x + side * reach * f * rng.range(0.7, 0.9);
                    let py = apex + tall * f * rng.range(0.45, 0.75);
                    ends.insert(1, (px, py));
                }
            }
            let lit_face = jagged(rng, &lit_ends, tall * r.rough);
            let dark_face = jagged(rng, &dark_ends, tall * r.rough);
            let fog = mixed(colour, l.lit, 0.25 + 0.5 * r.haze);
            let foot = (x + rng.range(-0.3, 0.6) * left.min(right), hz + 1.0);
            let ridge_line = jagged(rng, &[(x, apex), foot], tall * 0.05);
            let mut lit_side = lit_face.clone();
            lit_side.extend_from_slice(&ridge_line[1..]);
            let mut dark_side = ridge_line.clone();
            dark_side.extend(dark_face.iter().rev().take(dark_face.len() - 1));
            for (face, shade) in [(&lit_side, mixed(colour, l.edge, 0.12)), (&dark_side, mixed(colour, l.shade, 0.35))] {
                polygon(c, face);
                let g = LinearGradient::new(0.0, apex, 0.0, hz);
                g.add_color_stop_rgb(0.0, shade.0, shade.1, shade.2);
                let f = mixed(shade, fog, 0.7);
                g.add_color_stop_rgb(1.0, f.0, f.1, f.2);
                let _ = c.set_source(&g);
                let _ = c.fill();
            }
            let mut outline = lit_side.clone();
            outline.extend_from_slice(&dark_side);
            facets(c, rng, l, x, apex, left, right, hz, tall, &outline, r.haze);
            if r.snowy != 0.0 && tall > 0.35 * r.rise * u {
                let line = apex + tall * rng.range(0.25, 0.5) * r.snowy;
                let _ = c.save();
                let mut whole = lit_face.clone();
                whole.extend_from_slice(&dark_face[1..]);
                whole.push((x, hz + 1.0));
                polygon(c, &whole);
                c.clip();
                let mut edge = vec![(x - left, line)];
                for i in 1..12 {
                    let px = x - left + (left + right) * i as f64 / 12.0;
                    let dy = tall * rng.range(-0.08, 0.12);
                    let notch = if rng.f64() < 0.2 { tall * 0.25 } else { 0.0 };
                    edge.push((px, line + dy + notch));
                }
                edge.push((x + right, line));
                let k = 0.15 + 0.3 * r.haze;
                for (side, face) in [(mixed(mixed(l.lit, l.edge, 0.3), colour, k), &lit_side), (mixed(mixed(l.lit, l.shade, 0.55), colour, k), &dark_side)] {
                    let _ = c.save();
                    polygon(c, face);
                    c.clip();
                    let mut cap = vec![(x - left, apex - 2.0), (x + right, apex - 2.0)];
                    cap.extend(edge.iter().rev());
                    polygon(c, &cap);
                    c.set_source_rgb(side.0, side.1, side.2);
                    let _ = c.fill();
                    let _ = c.restore();
                }
                let _ = c.restore();
            }
        }
    }
}

pub const SOFT_DOT: [(f64, f64); 4] = [(0.0, 1.0), (0.12, 0.9), (0.35, 0.25), (1.0, 0.0)];

/// Sprites as one vertex array: (cx, cy, size, angle in degrees, rgb, alpha) each, premultiplied.
pub fn sprite_vertices(placed: &[(f64, f64, f64, f64, Rgb, f64)]) -> Vec<f32> {
    let mut data = Vec::with_capacity(placed.len() * 32);
    for &(cx, cy, size, angle, rgb, alpha) in placed {
        let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
        let h = size / 2.0;
        let (r, g, b) = (rgb.0 * alpha, rgb.1 * alpha, rgb.2 * alpha);
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let (px, py) = ((2.0 * u - 1.0) * h, (2.0 * v - 1.0) * h);
            for x in [u, v, r, g, b, alpha, cx + px * c - py * s, cy + px * s + py * c] {
                data.push(x as f32);
            }
        }
    }
    data
}

/// A filled rectangle as a vertex array's 32 numbers.
pub fn fill_quad(x0: f64, y0: f64, x1: f64, y1: f64, colour: Rgb, alpha: f64) -> [f64; 32] {
    let (r, g, b) = colour;
    [0.0, 0.0, r, g, b, alpha, x0, y0, 0.0, 0.0, r, g, b, alpha, x1, y0, 0.0, 0.0, r, g, b, alpha, x1, y1, 0.0, 0.0, r, g, b, alpha, x0, y1]
}

/// A rectangle shaded from `top` at y0 to `bottom` at y1, with alpha (top, bottom).
pub fn shade_quad(x0: f64, y0: f64, x1: f64, y1: f64, top: Rgb, bottom: Rgb, alpha: (f64, f64)) -> [f64; 32] {
    let ((r0, g0, b0), (r1, g1, b1), (a0, a1)) = (top, bottom, alpha);
    [0.0, 0.0, r0, g0, b0, a0, x0, y0, 0.0, 0.0, r0, g0, b0, a0, x1, y0, 0.0, 0.0, r1, g1, b1, a1, x1, y1, 0.0, 0.0, r1, g1, b1, a1, x0, y1]
}

/// Paint untextured quads with cairo. Used to draw a live part at rest.
pub fn paint_fills(c: &Context, data: &[f64]) {
    for q in data.as_chunks::<32>().0 {
        c.set_source_rgba(q[2], q[3], q[4], q[5]);
        c.rectangle(q[6], q[7], q[14] - q[6], q[23] - q[7]);
        let _ = c.fill();
    }
}

/// The longest `seconds` gives: a day.
pub const MAX_SECONDS: f64 = 86_400.0;

/// `s` seconds as a Duration, within 0 and a day: never a panic, whatever the number (NaN is 0).
#[allow(clippy::disallowed_methods)] // in range here
pub fn seconds(s: f64) -> std::time::Duration {
    std::time::Duration::from_secs_f64(if s > 0.0 { s.min(MAX_SECONDS) } else { 0.0 })
}

/// `x` for a message: `{:e}` when very large or very small, so 1e308 is not 309 digits.
pub fn num(x: f64) -> String {
    let a = x.abs();
    if a.is_finite() && a != 0.0 && !(1e-4..1e9).contains(&a) { format!("{x:e}") } else { format!("{x}") }
}

/// `x` rounded to two decimals.
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// DejaVu Sans Mono, the one typeface, as `c`'s font. On Windows cairo looks a family up in the
/// system's fonts only, so a registered face of ours is set directly.
pub fn select_mono(c: &Context, bold: bool) {
    #[cfg(windows)]
    if let Some(face) = crate::platform::windows::font::face(bold) {
        c.set_font_face(&face);
        return;
    }
    #[allow(clippy::disallowed_methods)] // a constant family name, without a NUL (font::tests)
    c.select_font_face(crate::font::FAMILY, cairo::FontSlant::Normal, if bold { cairo::FontWeight::Bold } else { cairo::FontWeight::Normal });
}

// --- text: cairo-rs panics on a NUL in text, so all text is drawn and measured through these -------

/// `text` as cairo can take it: without NULs.
fn nul_free(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains('\0') { text.replace('\0', "").into() } else { text.into() }
}

/// Draw `text` at the current point.
#[allow(clippy::disallowed_methods)]
pub fn show_text(c: &Context, text: &str) -> Result<(), cairo::Error> {
    c.show_text(&nul_free(text))
}

/// `text`'s extents in the current font.
#[allow(clippy::disallowed_methods)]
pub fn text_extents(c: &Context, text: &str) -> Result<cairo::TextExtents, cairo::Error> {
    c.text_extents(&nul_free(text))
}

/// `text`'s outline added to the path.
#[allow(clippy::disallowed_methods)]
pub fn text_path(c: &Context, text: &str) {
    c.text_path(&nul_free(text))
}

/// Text from outside (a plugin, a source) as it is shown: without control characters.
pub fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Resolve the fonts once before anything draws. cairo keeps the font options of a face's first
/// use, so text could otherwise measure a pixel differently depending on what drew first.
pub fn settle_fonts() {
    crate::font::register(); // the faces first: a lookup before them would be kept
    let c = ctx(&surface(1, 1));
    for bold in [false, true] {
        select_mono(&c, bold);
        let _ = text_extents(&c, "X");
    }
}

/// Smoothstep: 0 below a, 1 above b.
pub fn smooth_ab(a: f64, b: f64, x: f64) -> f64 {
    let t = 1.0f64.min(0.0f64.max((x - a) / (b - a)));
    t * t * (3.0 - 2.0 * t)
}

impl Range {
    pub fn new(colour: Rgb, rise: f64, count: f64, snowy: f64, haze: f64) -> Self {
        Range { colour, rise, count: count as i64, snowy, haze, rough: 0.06, shoulders: false, spread: (1.6, 3.2) }
    }
}

/// Write a surface raw (format, size, premultiplied pixels), deflated. Loads back exactly, and
/// faster than a PNG.
pub fn write_raw(s: &mut ImageSurface, path: &std::path::Path) -> bool {
    use std::io::Write;
    let head = [if s.format() == Format::Rgb24 { 1u32 } else { 0 }, s.width() as u32, s.height() as u32, s.stride() as u32];
    let Some(data) = view(s) else { return false };
    let Ok(f) = std::fs::File::create(path) else { return false };
    let mut z = flate2::write::DeflateEncoder::new(std::io::BufWriter::new(f), flate2::Compression::fast());
    let ok = head.iter().all(|v| z.write_all(&v.to_le_bytes()).is_ok()) && z.write_all(&data).is_ok();
    ok && z.finish().is_ok()
}

pub fn read_raw(path: &std::path::Path) -> Option<ImageSurface> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut z = flate2::read::DeflateDecoder::new(std::io::BufReader::new(f));
    let mut head = [0u8; 16];
    z.read_exact(&mut head).ok()?;
    let v = |i: usize| u32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap()) as i32;
    let format = if v(0) == 1 { Format::Rgb24 } else { Format::ARgb32 };
    let mut s = ImageSurface::create(format, v(1), v(2)).ok()?;
    if s.stride() != v(3) {
        return None;
    }
    {
        let mut d = s.data().ok()?;
        z.read_exact(&mut d).ok()?;
    }
    s.mark_dirty();
    Some(s)
}

/// Where a surface has anything drawn: (x0, y0, x1, y1), `pad` pixels round it; None if nothing. The
/// surface may be shared (a cached layer): its pixels are read through cairo-rs's `with_data`.
pub fn ink_box(s: &ImageSurface, pad: i32) -> Option<(i32, i32, i32, i32)> {
    let (stride, w, h) = (s.stride() as usize, s.width(), s.height());
    let mut found = Some((0, 0, w, h));
    let _ = s.with_data(|data| found = ink_in(data, stride, w, h, pad));
    found
}

fn ink_in(data: &[u8], stride: usize, w: i32, h: i32, pad: i32) -> Option<(i32, i32, i32, i32)> {
    let (mut x0, mut x1, mut y0, mut y1) = (w, 0, None, 0);
    for y in 0..h as usize {
        let row = &data[y * stride..y * stride + 4 * w as usize];
        let alpha = |x: usize| row[4 * x + 3];
        let Some(left) = (0..w as usize).find(|&x| alpha(x) != 0) else { continue };
        let right = (0..w as usize).rev().find(|&x| alpha(x) != 0).unwrap_or(left) + 1;
        if y0.is_none() {
            y0 = Some(y as i32);
        }
        y1 = y as i32;
        x0 = x0.min(left as i32);
        x1 = x1.max(right as i32);
    }
    let y0 = y0?;
    Some((0.max(x0 - pad), 0.max(y0 - pad), w.min(x1 + pad), h.min(y1 + 1 + pad)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn seconds_never_panic() {
        assert_eq!(seconds(1.5), Duration::from_millis(1500));
        assert_eq!(seconds(0.0), Duration::ZERO);
        for zero in [-1.0, f64::NAN, f64::NEG_INFINITY, -0.0] {
            assert_eq!(seconds(zero), Duration::ZERO, "{zero}");
        }
        for long in [1e30, f64::INFINITY, f64::MAX, MAX_SECONDS + 1.0] {
            assert_eq!(seconds(long), Duration::from_secs(MAX_SECONDS as u64), "{long}");
        }
    }

    #[test]
    fn numbers_in_messages_are_short() {
        let cases = [(1e308, "1e308"), (-2.5e-7, "-2.5e-7"), (0.0, "0"), (86400.0, "86400"), (0.25, "0.25"), (f64::INFINITY, "inf")];
        for (x, text) in cases {
            assert_eq!(num(x), text);
        }
    }

    #[test]
    fn text_with_a_nul_is_measured_and_drawn_without_it() {
        let c = probe();
        select_mono(&c, false);
        c.set_font_size(20.0);
        let plain = text_extents(&c, "news").unwrap().x_advance();
        assert_eq!(text_extents(&c, "ne\0ws\0").unwrap().x_advance(), plain);
        assert_eq!(text_extents(&c, "\0").unwrap().x_advance(), 0.0);
        assert!(show_text(&c, "\0x\0").is_ok());
        text_path(&c, "a\0b");
        assert_eq!(nul_free("plain"), std::borrow::Cow::Borrowed("plain"));
        assert_eq!(printable("a\0b\u{7}c\n\td\u{1b}[0m"), "abcd[0m");
    }

    #[test]
    fn view_lends_the_pixels_only_to_their_one_holder() {
        let mut s = surface(4, 2);
        {
            let c = ctx(&s);
            c.set_source_rgb(1.0, 0.0, 0.0);
            let _ = c.paint();
            assert!(view(&mut s).is_none(), "lent while a context draws on it");
        }
        let d = view(&mut s).expect("held by nothing else");
        assert_eq!(d.len(), 4 * 4 * 2);
        assert_eq!(&d[..4], [0, 0, 255, 255]);
    }
}
