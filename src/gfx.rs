//! The renderer, on OpenGL 3.3 core or 2.1 (GLSL 330 or 120). Parts append quads (four corners of
//! u, v, r, g, b, a, x, y) to one vertex stream; a frame uploads it once, indexed, and makes one draw per
//! run of quads sharing a texture, blend and clip.

use cairo::{Format, ImageSurface};
use gl::types::*;
use std::ffi::CString;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Blend {
    Opaque,
    Alpha,
    Premultiplied,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Picture-sized, drawn pixel for pixel.
    Layer,
    /// Drawn rotated and scaled: mipmapped.
    Sprite,
    /// A sprite repeating past its edges.
    Tile,
    /// Drawn pixel for pixel, whole.
    Label,
    /// Filtered, drawn scaled.
    Smooth,
}

/// The most bytes handed to the driver in one upload.
const UPLOAD_BYTES: usize = 1 << 20;

/// A texture of this renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Tex(u32);

/// A rectangle in picture pixels: x0, y0, x1, y1.
pub type Region = (i32, i32, i32, i32);

/// Pictures to upload once the renderer exists, each with the handle it will have.
#[derive(Default)]
pub struct Uploads(Vec<(ImageSurface, Kind, Option<Region>)>);

impl Uploads {
    /// Add a picture; its handle.
    pub fn add(&mut self, s: ImageSurface, kind: Kind) -> Tex {
        self.0.push((s, kind, None));
        Tex(self.0.len() as u32 - 1)
    }
    /// Add a picture-sized picture of which only `region` shows: only that part is kept (a Layer).
    pub fn add_layer(&mut self, s: ImageSurface, region: Region) -> Tex {
        self.0.push((s, Kind::Layer, Some(region)));
        Tex(self.0.len() as u32 - 1)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A piece of a texture: its pixels (sx0, sy0)-(sx1, sy1) placed onto (x0, y0)-(x1, y1).
#[derive(Clone, Copy, Debug, Default)]
pub struct Patch {
    pub sx0: f64,
    pub sy0: f64,
    pub sx1: f64,
    pub sy1: f64,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// A uniform of a theme's shader: location, components, values.
pub type Uniform = (GLint, usize, [f32; 4]);

/// A custom draw: its program and its ranges in the frame's uniform and texture lists.
struct Custom {
    prog: GLuint,
    uniforms: std::ops::Range<usize>,
    textures: std::ops::Range<usize>,
}

struct Cmd {
    custom: Option<usize>,
    tex: Option<Tex>,
    blend: Blend,
    scissor: Option<(i32, i32, i32, i32)>,
    first: usize, // in quads
    count: usize,
}

struct Texture {
    id: GLuint,
    w: i32,
    h: i32,
    /// Where a Layer's pixels stand in the picture: x, y, w, h.
    place: (f64, f64, f64, f64),
}

/// Where a surface's pixels start, flushed, for GL to copy from: stride x height bytes; None for a
/// surface without them (finished, or in error). The pointer is good while `s` is alive and nothing draws
/// on it: an ImageSurface is not Send, and its pixels go to another thread only when nothing else holds
/// it (`ImageSurface::take_data`), so with `s` borrowed here nothing can draw on it until this thread does.
fn pixel_start(s: &ImageSurface) -> Option<*const u8> {
    s.flush();
    // SAFETY: `s` is a live image surface; this reads its data pointer, nothing more.
    let data = unsafe { cairo::ffi::cairo_image_surface_get_data(s.to_raw_none()) };
    (!data.is_null()).then_some(data.cast_const())
}

/// One window's renderer. Its methods are called with that window's GL context current, on the thread
/// that made it; the contexts share their objects.
pub struct Gfx {
    /// The pane, in picture pixels.
    pub rect: (i32, i32, i32, i32),
    textures: Vec<Texture>,
    verts: Vec<f32>,
    cmds: Vec<Cmd>,
    scissor: Option<(i32, i32, i32, i32)>,
    customs: Vec<Custom>,
    uniforms: Vec<Uniform>,
    samplers: Vec<(GLint, Tex)>,
    prog: GLuint,
    vbo: GLuint,
    ibo: GLuint,
    indexed: usize, // quads the index buffer holds
    textured: GLint,
    core: bool,
    mipmaps: bool,
    /// Not Send or Sync: every GL call relies on its window's context being current, which it is on
    /// the thread that made it only.
    _thread: std::marker::PhantomData<*const ()>,
}

// names a theme's fragment shader shares with it start with retro_, which themes leave alone
const VS: &str = "uniform vec4 retro_view; // ortho projection: x scale, x offset, y scale, y offset
ATTR vec2 uv; ATTR vec4 col; ATTR vec2 pos;
VARY vec2 retro_t; VARY vec4 retro_c;
void main() {
  retro_t = uv; retro_c = col;
  gl_Position = vec4(retro_view.x * pos.x + retro_view.y, retro_view.z * pos.y + retro_view.w, 0.0, 1.0);
}";

fn vs(core: bool) -> String {
    let (head, inn, out) = if core { ("#version 330 core\n", "in", "out") } else { ("#version 120\n", "attribute", "varying") };
    format!("{head}{VS}").replace("ATTR", inn).replace("VARY", out)
}

fn fs(core: bool) -> &'static str {
    if core {
        "#version 330 core\nuniform sampler2D tex; uniform float textured;\nin vec2 retro_t; in vec4 retro_c; out vec4 colour;\n\
         void main() { colour = textured > 0.5 ? texture(tex, retro_t) * retro_c : retro_c; }"
    } else {
        "#version 120\nuniform sampler2D tex; uniform float textured;\nvarying vec2 retro_t; varying vec4 retro_c;\n\
         void main() { gl_FragColor = textured > 0.5 ? texture2D(tex, retro_t) * retro_c : retro_c; }"
    }
}

fn gl_string(name: GLenum) -> String {
    // SAFETY: called by Gfx::new, with the context current; GetString gives null or a NUL-terminated
    // string the driver keeps.
    unsafe {
        let p = gl::GetString(name);
        if p.is_null() { String::new() } else { std::ffi::CStr::from_ptr(p as *const _).to_string_lossy().into_owned() }
    }
}

/// A compiled shader, or the driver's message.
fn shader(kind: GLenum, src: &str) -> Result<GLuint, String> {
    // SAFETY: called with a context current (by Gfx::new and Gfx::compile); the source is one
    // NUL-terminated string that outlives the call; the log is written within the 4096 bytes said.
    unsafe {
        let s = gl::CreateShader(kind);
        let c = CString::new(src).map_err(|e| e.to_string())?;
        gl::ShaderSource(s, 1, &c.as_ptr(), std::ptr::null());
        gl::CompileShader(s);
        let mut ok = 0;
        gl::GetShaderiv(s, gl::COMPILE_STATUS, &mut ok);
        if ok != 0 {
            return Ok(s);
        }
        let mut buf = vec![0u8; 4096];
        let mut n = 0;
        gl::GetShaderInfoLog(s, 4096, &mut n, buf.as_mut_ptr() as *mut _);
        gl::DeleteShader(s);
        Err(String::from_utf8_lossy(&buf[..(n.max(0) as usize).min(buf.len())]).trim().to_string())
    }
}

/// A linked program of two shaders (deleted once linked), or the driver's message.
fn program(vertex: &str, fragment: &str) -> Result<GLuint, String> {
    // SAFETY: called with a context current; the shaders are this function's own, deleted once linked
    // or on failure; the attribute names are NUL-terminated.
    unsafe {
        let v = shader(gl::VERTEX_SHADER, vertex)?;
        let f = shader(gl::FRAGMENT_SHADER, fragment).inspect_err(|_| gl::DeleteShader(v))?;
        let p = gl::CreateProgram();
        gl::AttachShader(p, v);
        gl::AttachShader(p, f);
        for (loc, name) in [(0, c"uv"), (1, c"col"), (2, c"pos")] {
            gl::BindAttribLocation(p, loc, name.as_ptr());
        }
        gl::LinkProgram(p);
        gl::DeleteShader(v);
        gl::DeleteShader(f);
        let mut ok = 0;
        gl::GetProgramiv(p, gl::LINK_STATUS, &mut ok);
        if ok != 0 {
            return Ok(p);
        }
        let mut buf = vec![0u8; 4096];
        let mut n = 0;
        gl::GetProgramInfoLog(p, 4096, &mut n, buf.as_mut_ptr() as *mut _);
        gl::DeleteProgram(p);
        Err(String::from_utf8_lossy(&buf[..(n.max(0) as usize).min(buf.len())]).trim().to_string())
    }
}

fn set_view(prog: GLuint, rect: (i32, i32, i32, i32)) {
    let (l, r, b, t) = (rect.0 as f32, (rect.0 + rect.2) as f32, (rect.1 + rect.3) as f32, rect.1 as f32);
    // SAFETY: called with a context current; `prog` is a linked program; the name is NUL-terminated.
    unsafe {
        gl::UseProgram(prog);
        gl::Uniform4f(gl::GetUniformLocation(prog, c"retro_view".as_ptr()), 2.0 / (r - l), -(r + l) / (r - l), 2.0 / (t - b), -(t + b) / (t - b));
    }
}

fn clip(r: &(f64, f64, f64, f64), pane: (f64, f64, f64, f64)) -> Option<(f64, f64, f64, f64)> {
    let (x, y, w, h) = pane;
    let c = (x.max(r.0), y.max(r.1), (x + w).min(r.2), (y + h).min(r.3));
    (c.2 > c.0 && c.3 > c.1).then_some(c)
}

impl Gfx {
    /// Made with the window's GL context current; `up` uploaded, so their handles are good.
    pub fn new(rect: (i32, i32, i32, i32), core: bool, up: Uploads) -> Gfx {
        let major: u32 = gl_string(gl::VERSION).split(|c: char| !c.is_ascii_digit()).next().and_then(|m| m.parse().ok()).unwrap_or(1);
        let ext = if core { String::new() } else { gl_string(gl::EXTENSIONS) };
        let mipmaps = core || major >= 3 || ext.contains("GL_ARB_framebuffer_object") || ext.contains("GL_EXT_framebuffer_object");
        // SAFETY: made with its window's context current (the caller's duty, as for every method); the
        // attribute offsets lie within a 32-byte vertex, read from the bound vertex buffer.
        unsafe {
            if core {
                let mut vao = 0;
                gl::GenVertexArrays(1, &mut vao);
                gl::BindVertexArray(vao);
            }
            let prog = program(&vs(core), fs(core)).unwrap_or_else(|e| panic!("the renderer's shaders: {e}"));
            set_view(prog, rect);
            gl::Uniform1i(gl::GetUniformLocation(prog, c"tex".as_ptr()), 0);
            let textured = gl::GetUniformLocation(prog, c"textured".as_ptr());
            gl::Viewport(0, 0, rect.2, rect.3);
            let (mut vbo, mut ibo) = (0, 0);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ibo);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ibo);
            for (l, n, off) in [(0u32, 2, 0usize), (1, 4, 8), (2, 2, 24)] {
                gl::EnableVertexAttribArray(l);
                gl::VertexAttribPointer(l, n, gl::FLOAT, gl::FALSE, 32, off as *const _);
            }
            let mut gfx = Gfx {
                rect,
                textures: vec![],
                verts: Vec::with_capacity(1 << 16),
                cmds: vec![],
                scissor: None,
                customs: vec![],
                uniforms: vec![],
                samplers: vec![],
                prog,
                vbo,
                ibo,
                indexed: 0,
                textured,
                core,
                mipmaps,
                _thread: std::marker::PhantomData,
            };
            for (s, kind, region) in up.0 {
                gfx.upload_region(&s, kind, region);
            }
            gfx
        }
    }

    pub fn core(&self) -> bool {
        self.core
    }

    pub fn size(&self, t: Tex) -> (i32, i32) {
        let tx = &self.textures[t.0 as usize];
        (tx.w, tx.h)
    }

    /// Upload a surface as a new texture. A Layer is the pane's rectangle of a picture-sized surface.
    pub fn upload(&mut self, s: &ImageSurface, kind: Kind) -> Tex {
        self.upload_region(s, kind, None)
    }

    fn upload_region(&mut self, s: &ImageSurface, kind: Kind, region: Option<Region>) -> Tex {
        let mut id = 0;
        // SAFETY: its window's context is current; one name written to `id`.
        unsafe { gl::GenTextures(1, &mut id) };
        let t = Tex(self.textures.len() as u32);
        self.textures.push(Texture { id, w: 0, h: 0, place: (0.0, 0.0, 0.0, 0.0) });
        self.replace_region(t, s, kind, region);
        t
    }

    /// New pixels for a texture.
    pub fn replace(&mut self, t: Tex, s: &ImageSurface, kind: Kind) {
        self.replace_region(t, s, kind, None)
    }

    /// New pixels for a texture; a Layer keeps only the pane's part of `region` (all the pane without one).
    fn replace_region(&mut self, t: Tex, s: &ImageSurface, kind: Kind, region: Option<Region>) {
        let (x, y, w, h) = if kind == Kind::Layer {
            let (px, py, pw, ph) = self.rect;
            let (x0, y0, x1, y1) = region.unwrap_or((px, py, px + pw, py + ph));
            let (x0, y0) = (x0.max(px).max(0), y0.max(py).max(0));
            let (x1, y1) = (x1.min(px + pw).min(s.width()), y1.min(py + ph).min(s.height()));
            (x0, y0, x1 - x0, y1 - y0)
        } else {
            (0, 0, s.width(), s.height())
        };
        let tx = &mut self.textures[t.0 as usize];
        tx.w = s.width();
        tx.h = s.height();
        tx.place = (x as f64, y as f64, w.max(0) as f64, h.max(0) as f64);
        // nothing of it shows: one clear pixel
        let (x, y, w, h) = if w <= 0 || h <= 0 { (0, 0, 1, 1) } else { (x, y, w, h) };
        let blank;
        let (src, data, x, y, w, h) = match pixel_start(s) {
            Some(data) if x + w <= s.width() && y + h <= s.height() => (s, data, x, y, w, h),
            _ => {
                blank = crate::util::surface(1, 1);
                let Some(data) = pixel_start(&blank) else { return };
                (&blank, data, 0, 0, 1, 1)
            }
        };
        let stride = src.stride() as usize;
        let mipmaps = matches!(kind, Kind::Sprite | Kind::Tile);
        // SAFETY: a GL context is current, and no pixel unpack buffer is bound, so GL copies from
        // `data` before each call returns. `data` starts `src`'s stride x height bytes (`pixel_start`),
        // which `src` keeps alive and unchanged meanwhile (see `pixel_start`); every pointer handed to GL
        // is a row of the rectangle (x, y, w, h), which lies within the surface.
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tx.id);
            let (min, mag, wrap) = match kind {
                Kind::Sprite => (gl::LINEAR_MIPMAP_LINEAR, gl::LINEAR, gl::CLAMP_TO_EDGE),
                Kind::Tile => (gl::LINEAR_MIPMAP_LINEAR, gl::LINEAR, gl::REPEAT),
                Kind::Smooth => (gl::LINEAR, gl::LINEAR, gl::CLAMP_TO_EDGE),
                Kind::Layer | Kind::Label => (gl::NEAREST, gl::NEAREST, gl::CLAMP_TO_EDGE),
            };
            if mipmaps && !self.mipmaps {
                gl::TexParameteri(gl::TEXTURE_2D, 0x8191 /* GL_GENERATE_MIPMAP */, 1);
            }
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, min as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, mag as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, wrap as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, wrap as i32);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, src.stride() / 4);
            let internal = if src.format() == Format::Rgb24 { gl::RGB } else { gl::RGBA };
            let at = |row: i32| data.add(row as usize * stride + x as usize * 4).cast();
            // in strips of a megabyte or so: a driver may keep a staging buffer as large as one upload
            let rows = (UPLOAD_BYTES / (4 * w as usize)).max(1) as i32;
            if h <= rows {
                gl::TexImage2D(gl::TEXTURE_2D, 0, internal as i32, w, h, 0, gl::BGRA, gl::UNSIGNED_BYTE, at(y));
            } else {
                gl::TexImage2D(gl::TEXTURE_2D, 0, internal as i32, w, h, 0, gl::BGRA, gl::UNSIGNED_BYTE, std::ptr::null());
                for r in (0..h).step_by(rows as usize) {
                    gl::TexSubImage2D(gl::TEXTURE_2D, 0, 0, r, w, rows.min(h - r), gl::BGRA, gl::UNSIGNED_BYTE, at(y + r));
                }
            }
            if mipmaps && self.mipmaps {
                gl::GenerateMipmap(gl::TEXTURE_2D);
            }
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
        }
    }

    /// Reload columns x0..x1 of a texture.
    pub fn replace_columns(&mut self, t: Tex, s: &ImageSurface, x0: i32, x1: i32) {
        let (x0, x1) = (x0.max(0), x1.min(s.width()));
        let Some(data) = pixel_start(s).filter(|_| x0 < x1) else { return };
        // SAFETY: as in `replace_region`: GL copies the columns x0..x1 of every row, which lie within
        // the stride x height bytes from `data`, before the call returns.
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.textures[t.0 as usize].id);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, s.stride() / 4);
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, x0);
            gl::TexSubImage2D(gl::TEXTURE_2D, 0, x0, 0, x1 - x0, s.height(), gl::BGRA, gl::UNSIGNED_BYTE, data.cast());
            gl::PixelStorei(gl::UNPACK_SKIP_PIXELS, 0);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
        }
    }

    /// Clip to a rectangle in GL window coordinates (from the bottom), or no clip.
    pub fn scissor(&mut self, rect: Option<(i32, i32, i32, i32)>) {
        self.scissor = rect;
    }

    /// Open (or extend) a run for `quads` quads that the caller appends next.
    fn run(&mut self, tex: Option<Tex>, blend: Blend, quads: usize) {
        let first = self.verts.len() / 32;
        if let Some(last) = self.cmds.last_mut()
            && last.custom.is_none()
            && last.tex == tex
            && last.blend == blend
            && last.scissor == self.scissor
            && last.first + last.count == first
        {
            last.count += quads;
            return;
        }
        self.cmds.push(Cmd { custom: None, tex, blend, scissor: self.scissor, first, count: quads });
    }

    fn corner(&mut self, u: f64, v: f64, rgba: [f32; 4], x: f64, y: f64) {
        self.verts.extend_from_slice(&[u as f32, v as f32, rgba[0], rgba[1], rgba[2], rgba[3], x as f32, y as f32]);
    }

    /// Where a picture-sized texture's pixels stand: x, y, w, h.
    fn place(&self, t: Tex) -> (f64, f64, f64, f64) {
        self.textures[t.0 as usize].place
    }

    fn pane(&self) -> (f64, f64, f64, f64) {
        let (x, y, w, h) = self.rect;
        (x as f64, y as f64, w as f64, h as f64)
    }

    /// Quads, 32 floats each (four corners of u, v, r, g, b, a, x, y).
    pub fn draw(&mut self, tex: Option<Tex>, data: &[f32], blend: Blend) {
        let n = data.len() / 32;
        if n == 0 {
            return;
        }
        self.run(tex, blend, n);
        self.verts.extend_from_slice(&data[..n * 32]);
    }

    /// Rectangles (x0, y0, x1, y1, alpha) of a picture-sized texture, or filled in `rgb` with no texture.
    pub fn rects(&mut self, tex: Option<Tex>, rects: &[(f64, f64, f64, f64, f64)], rgb: (f64, f64, f64), blend: Blend) {
        if rects.is_empty() {
            return;
        }
        let place = tex.map_or(self.pane(), |t| self.place(t));
        let (x, y, w, h) = place;
        let n = if tex.is_some() { rects.iter().filter(|r| clip(&(r.0, r.1, r.2, r.3), place).is_some()).count() } else { rects.len() };
        self.run(tex, blend, n);
        for &(x0, y0, x1, y1, a) in rects {
            let (x0, y0, x1, y1) = match tex {
                Some(_) => match clip(&(x0, y0, x1, y1), place) {
                    Some(r) => r,
                    None => continue,
                },
                None => (x0, y0, x1, y1),
            };
            let c = [rgb.0 as f32, rgb.1 as f32, rgb.2 as f32, a as f32];
            let (u0, v0, u1, v1) = ((x0 - x) / w, (y0 - y) / h, (x1 - x) / w, (y1 - y) / h);
            self.corner(u0, v0, c, x0, y0);
            self.corner(u1, v0, c, x1, y0);
            self.corner(u1, v1, c, x1, y1);
            self.corner(u0, v1, c, x0, y1);
        }
    }

    /// A picture-sized texture over `area` (or all it holds), at `strength`.
    pub fn layer(&mut self, tex: Tex, area: Option<&[(f64, f64, f64, f64)]>, strength: f64, blend: Blend) {
        if strength <= 0.0 {
            return;
        }
        let place = self.place(tex);
        let whole = [(place.0, place.1, place.0 + place.2, place.1 + place.3)];
        let area = area.unwrap_or(&whole);
        let n = area.iter().filter(|r| clip(r, place).is_some()).count();
        if n == 0 {
            return;
        }
        self.run(Some(tex), blend, n);
        let k = strength.min(1.0) as f32;
        let c = [k; 4];
        let (x, y, w, h) = place;
        for (x0, y0, x1, y1) in area.iter().filter_map(|r| clip(r, place)) {
            let (u0, v0, u1, v1) = ((x0 - x) / w, (y0 - y) / h, (x1 - x) / w, (y1 - y) / h);
            self.corner(u0, v0, c, x0, y0);
            self.corner(u1, v0, c, x1, y0);
            self.corner(u1, v1, c, x1, y1);
            self.corner(u0, v1, c, x0, y1);
        }
    }

    /// The whole texture at (x, y), w by h, at `strength`.
    pub fn image(&mut self, tex: Tex, x: f64, y: f64, w: f64, h: f64, strength: f64, blend: Blend) {
        self.spans(tex, x, y, w, h, &[(x, x + w, strength, strength)], blend);
    }

    /// Columns of the texture placed at (x, y), w by h: each (x0, x1, a0, a1) fading a0 to a1.
    pub fn spans(&mut self, tex: Tex, x: f64, y: f64, w: f64, h: f64, spans: &[(f64, f64, f64, f64)], blend: Blend) {
        self.run(Some(tex), blend, spans.len());
        for &(x0, x1, a0, a1) in spans {
            let (u0, u1) = ((x0 - x) / w, (x1 - x) / w);
            let (c0, c1) = ([a0 as f32; 4], [a1 as f32; 4]);
            self.corner(u0, 0.0, c0, x0, y);
            self.corner(u1, 0.0, c1, x1, y);
            self.corner(u1, 1.0, c1, x1, y + h);
            self.corner(u0, 1.0, c0, x0, y + h);
        }
    }

    /// Pieces of a texture, premultiplied, at k.
    pub fn patches(&mut self, tex: Tex, placed: &[Patch], k: f64) {
        let (tw, th) = self.size(tex);
        let (tw, th) = (tw as f64, th as f64);
        self.run(Some(tex), Blend::Premultiplied, placed.len());
        let c = [k as f32; 4];
        for &Patch { sx0, sy0, sx1, sy1, x0, y0, x1, y1 } in placed {
            let (u0, v0, u1, v1) = (sx0 / tw, sy0 / th, sx1 / tw, sy1 / th);
            self.corner(u0, v0, c, x0, y0);
            self.corner(u1, v0, c, x1, y0);
            self.corner(u1, v1, c, x1, y1);
            self.corner(u0, v1, c, x0, y1);
        }
    }

    /// A picture-sized texture over (x0, y0)-(x1, y1), alpha a0 at x0 to a1 at x1.
    pub fn fade(&mut self, tex: Tex, x0: f64, y0: f64, x1: f64, y1: f64, a0: f64, a1: f64, blend: Blend) {
        let (x, y, w, h) = self.place(tex);
        self.run(Some(tex), blend, 1);
        let (c0, c1) = ([1.0, 1.0, 1.0, a0 as f32], [1.0, 1.0, 1.0, a1 as f32]);
        self.corner((x0 - x) / w, (y0 - y) / h, c0, x0, y0);
        self.corner((x1 - x) / w, (y0 - y) / h, c1, x1, y0);
        self.corner((x1 - x) / w, (y1 - y) / h, c1, x1, y1);
        self.corner((x0 - x) / w, (y1 - y) / h, c0, x0, y1);
    }

    /// Compile a theme's fragment shader, written in GLSL 1.20 against the inputs `time`,
    /// `resolution`, `area`, `readings`, `tex0`..`tex3` and `uv`. On the core profile, `gl_FragColor`
    /// and `texture2D` are defined to their GLSL 330 forms.
    pub fn compile(&self, fragment: &str) -> Result<GLuint, String> {
        let inputs = "uniform float time; uniform vec2 resolution; uniform vec4 area; uniform vec4 readings;\n\
            uniform sampler2D tex0; uniform sampler2D tex1; uniform sampler2D tex2; uniform sampler2D tex3;\n";
        let src = if self.core {
            format!(
                "#version 330 core\nout vec4 retro_colour;\n#define gl_FragColor retro_colour\n#define texture2D texture\n{inputs}in vec2 retro_t; in vec4 retro_c;\n#define uv retro_t\n#line 1\n{fragment}"
            )
        } else {
            format!("#version 120\n{inputs}varying vec2 retro_t; varying vec4 retro_c;\n#define uv retro_t\n#line 1\n{fragment}")
        };
        let p = program(&vs(self.core), &src)?;
        set_view(p, self.rect);
        // SAFETY: its window's context is current; `self.prog` is the renderer's linked program.
        unsafe { gl::UseProgram(self.prog) };
        Ok(p)
    }

    pub fn uniform(&self, prog: GLuint, name: &str) -> GLint {
        let n = CString::new(name).unwrap();
        // SAFETY: its window's context is current; the name is NUL-terminated and outlives the call.
        unsafe { gl::GetUniformLocation(prog, n.as_ptr()) }
    }

    /// One quad over (x0, y0)-(x1, y1), drawn by a theme's shader with these inputs.
    pub fn custom(&mut self, prog: GLuint, uniforms: &[Uniform], textures: &[(GLint, Tex)], x0: f64, y0: f64, x1: f64, y1: f64, blend: Blend) {
        let u = self.uniforms.len();
        self.uniforms.extend(uniforms.iter().filter(|x| x.0 >= 0));
        let t = self.samplers.len();
        self.samplers.extend_from_slice(textures);
        let custom = Custom { prog, uniforms: u..self.uniforms.len(), textures: t..self.samplers.len() };
        let first = self.verts.len() / 32;
        let c = [1.0; 4];
        self.corner(0.0, 0.0, c, x0, y0);
        self.corner(1.0, 0.0, c, x1, y0);
        self.corner(1.0, 1.0, c, x1, y1);
        self.corner(0.0, 1.0, c, x0, y1);
        self.customs.push(custom);
        self.cmds.push(Cmd { custom: Some(self.customs.len() - 1), tex: None, blend, scissor: self.scissor, first, count: 1 });
    }

    /// Indices for at least `quads` quads: corners (0, 1, 3) and (1, 2, 3) of each.
    fn index(&mut self, quads: usize) {
        if quads <= self.indexed {
            return;
        }
        let n = quads.next_power_of_two().max(1024);
        let idx: Vec<u32> = (0..n as u32)
            .flat_map(|q| {
                let b = q * 4;
                [b, b + 1, b + 3, b + 1, b + 2, b + 3]
            })
            .collect();
        // SAFETY: its window's context is current; the index buffer is handed `idx`'s own bytes.
        unsafe {
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ibo);
            gl::BufferData(gl::ELEMENT_ARRAY_BUFFER, (idx.len() * 4) as isize, idx.as_ptr() as *const _, gl::STATIC_DRAW);
        }
        self.indexed = n;
    }

    /// Draw what was recorded, then forget it.
    pub fn flush(&mut self) {
        self.index(self.verts.len() / 32);
        // SAFETY: its window's context is current; the vertex buffer is handed `verts`'s own bytes; every
        // draw's range lies within the indices `index` made for the quads in `verts`.
        unsafe {
            gl::Disable(gl::SCISSOR_TEST);
            gl::ClearColor(0.0, 0.0, 0.0, 1.0); // the back buffer is undefined after a swap
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::UseProgram(self.prog);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ibo);
            gl::BufferData(gl::ARRAY_BUFFER, (self.verts.len() * 4) as isize, self.verts.as_ptr() as *const _, gl::STREAM_DRAW);
            let (mut blend, mut tex, mut sc) = (None, None, None);
            for c in &self.cmds {
                if blend != Some(c.blend) {
                    match c.blend {
                        Blend::Opaque => gl::Disable(gl::BLEND),
                        Blend::Alpha => {
                            gl::Enable(gl::BLEND);
                            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                        }
                        Blend::Premultiplied => {
                            gl::Enable(gl::BLEND);
                            gl::BlendFunc(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);
                        }
                    }
                    blend = Some(c.blend);
                }
                if sc != Some(c.scissor) {
                    match c.scissor {
                        Some((x, y, w, h)) => {
                            gl::Enable(gl::SCISSOR_TEST);
                            gl::Scissor(x, y, w, h);
                        }
                        None => gl::Disable(gl::SCISSOR_TEST),
                    }
                    sc = Some(c.scissor);
                }
                let at = (c.first * 6 * 4) as *const _;
                if let Some(k) = c.custom {
                    let cu = &self.customs[k];
                    gl::UseProgram(cu.prog);
                    for (loc, n, v) in &self.uniforms[cu.uniforms.clone()] {
                        match n {
                            1 => gl::Uniform1f(*loc, v[0]),
                            2 => gl::Uniform2f(*loc, v[0], v[1]),
                            3 => gl::Uniform3f(*loc, v[0], v[1], v[2]),
                            _ => gl::Uniform4f(*loc, v[0], v[1], v[2], v[3]),
                        }
                    }
                    for (unit, (loc, t)) in self.samplers[cu.textures.clone()].iter().enumerate() {
                        gl::ActiveTexture(gl::TEXTURE1 + unit as u32);
                        gl::BindTexture(gl::TEXTURE_2D, self.textures[t.0 as usize].id);
                        gl::Uniform1i(*loc, 1 + unit as i32);
                    }
                    gl::ActiveTexture(gl::TEXTURE0);
                    gl::DrawElements(gl::TRIANGLES, (c.count * 6) as i32, gl::UNSIGNED_INT, at);
                    gl::UseProgram(self.prog);
                    tex = None; // the texture unit's binding is not ours to trust after a custom draw
                    continue;
                }
                if tex != Some(c.tex) {
                    match c.tex {
                        Some(t) => {
                            gl::BindTexture(gl::TEXTURE_2D, self.textures[t.0 as usize].id);
                            gl::Uniform1f(self.textured, 1.0);
                        }
                        None => gl::Uniform1f(self.textured, 0.0),
                    }
                    tex = Some(c.tex);
                }
                gl::DrawElements(gl::TRIANGLES, (c.count * 6) as i32, gl::UNSIGNED_INT, at);
            }
            gl::Disable(gl::SCISSOR_TEST);
        }
        self.verts.clear();
        self.cmds.clear();
        self.customs.clear();
        self.uniforms.clear();
        self.samplers.clear();
        self.scissor = None;
    }

    /// The back buffer's pixels, top row first, BGRA.
    pub fn read(&self) -> Vec<u8> {
        let (_, _, w, h) = self.rect;
        let mut px = vec![0u8; (w * h * 4) as usize];
        // SAFETY: its window's context is current and no pack buffer is bound; `px` holds w x h pixels of
        // 4 bytes, rows packed at 4.
        unsafe {
            gl::Finish();
            gl::ReadBuffer(gl::BACK);
            gl::PixelStorei(gl::PACK_ALIGNMENT, 4);
            gl::ReadPixels(0, 0, w, h, gl::BGRA, gl::UNSIGNED_BYTE, px.as_mut_ptr() as *mut _);
        }
        let row = (w * 4) as usize;
        let mut out = vec![0u8; px.len()];
        for r in 0..h as usize {
            out[r * row..(r + 1) * row].copy_from_slice(&px[(h as usize - 1 - r) * row..(h as usize - r) * row]);
        }
        out
    }
}
