//! The renderer, on OpenGL 3.3 core or 2.1 (GLSL 330 or 120). Parts append quads (four corners of
//! u, v, r, g, b, a, x, y) to one vertex stream; a frame uploads it once, indexed, and makes one draw per
//! run of quads sharing a texture, blend, clip and mask. A masked run multiplies each pixel by the alpha
//! of a second texture, placed on the picture by a mapping of its own (a tank's window).

use cairo::{Format, ImageSurface};
use gl::types::*;
use std::ffi::CString;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Blend {
    Opaque,
    Alpha,
    Premultiplied,
    /// Lightens: 1 - (1 - source)(1 - destination), the source premultiplied.
    Screen,
    /// Adds the premultiplied source.
    Add,
}

/// A mask for a run: the texture whose alpha it multiplies by, and where it stands, as
/// (x scale, y scale, x offset, y offset) from picture pixels to the texture's u and v.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mask {
    pub tex: Tex,
    pub map: [f32; 4],
}

impl Mask {
    /// The texture's piece `uv` (u0, v0, u1, v1) laid over the picture's rectangle (x0, y0)-(x1, y1).
    pub fn over(tex: Tex, rect: (f64, f64, f64, f64), uv: (f64, f64, f64, f64)) -> Mask {
        let (x0, y0, x1, y1) = rect;
        let (u0, v0, u1, v1) = uv;
        let (sx, sy) = ((u1 - u0) / (x1 - x0), (v1 - v0) / (y1 - y0));
        Mask { tex, map: [sx as f32, sy as f32, (u0 - x0 * sx) as f32, (v0 - y0 * sy) as f32] }
    }
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
    /// Pixel art: each texel a sharp square however large it is drawn; mipmapped when drawn smaller.
    Pixel,
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
pub struct Uploads(Vec<(ImageSurface, Kind, Option<Region>, bool)>);

impl Uploads {
    /// Add a picture; its handle.
    pub fn add(&mut self, s: ImageSurface, kind: Kind) -> Tex {
        self.0.push((s, kind, None, false));
        Tex(self.0.len() as u32 - 1)
    }
    /// Add a picture-sized picture of which only `region` shows: only that part is kept (a Layer).
    pub fn add_layer(&mut self, s: ImageSurface, region: Region) -> Tex {
        self.0.push((s, Kind::Layer, Some(region), false));
        Tex(self.0.len() as u32 - 1)
    }
    /// Add a small picture, filtered and drawn scaled (Smooth), to go up packed with others into one
    /// texture: drawn one after another, they are one draw, where each of its own would be one each,
    /// and a draw costs a small processor's driver far more than its pixels do. Drawn with `draw` only.
    pub fn add_packed(&mut self, s: ImageSurface) -> Tex {
        self.0.push((s, Kind::Smooth, None, true));
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
    /// Its vec4 array uniform, if it has one (`uniform_array`).
    array: Option<(GLint, Vec<f32>)>,
}

struct Cmd {
    /// The render target drawn into (`Gfx::target`); None for the window.
    target: Option<Tex>,
    custom: Option<usize>,
    tex: Option<Tex>,
    mask: Option<Mask>,
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
    /// The framebuffer drawing into it, for a render target (`Gfx::render_target`); 0 for none.
    fbo: GLuint,
    /// Its storage as last made: width, height, format and kind; new pixels of the same go into it.
    stored: Option<(i32, i32, u32, Kind)>,
    /// A packed picture's page (`Uploads::add_packed`) and its place there: u, v of its top left, and
    /// its width and height, as fractions of the page.
    packed: Option<(Tex, f32, f32, f32, f32)>,
}

/// How wide a page of packed pictures is, and how tall it may grow.
const PAGE: (i32, i32) = (2048, 4096);

/// Pictures packed onto pages, in shelves: each picture's place, its page and top left there, and the
/// pages. Each picture has a pixel round it of its own edge's pixels, so a filtered texture reads at
/// its edge what it would alone, clamped. A picture that does not fit a page is not packed.
fn pack(pictures: &[(usize, &ImageSurface)]) -> (Vec<(usize, usize, i32, i32)>, Vec<ImageSurface>) {
    let mut order: Vec<&(usize, &ImageSurface)> = pictures.iter().filter(|(_, s)| s.width() + 2 <= PAGE.0 && s.height() + 2 <= PAGE.1).collect();
    order.sort_by_key(|(_, s)| std::cmp::Reverse(s.height()));
    // shelves: each picture at the right of the last on its shelf, a new shelf when it does not fit
    let (mut places, mut sizes) = (vec![], vec![(0i32, 0i32)]);
    let (mut x, mut y, mut shelf) = (0, 0, 0);
    for (i, s) in order {
        let (w, h) = (s.width() + 2, s.height() + 2);
        if x + w > PAGE.0 {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if y + h > PAGE.1 {
            sizes.push((0, 0));
            (x, y, shelf) = (0, 0, 0);
        }
        let page = sizes.len() - 1;
        places.push((*i, page, x, y));
        (x, shelf) = (x + w, shelf.max(h));
        sizes[page] = (sizes[page].0.max(x), sizes[page].1.max(y + shelf));
    }
    let mut pages: Vec<ImageSurface> = sizes.iter().map(|&(w, h)| crate::util::surface(w.max(1), h.max(1))).collect();
    for &(i, page, px, py) in &places {
        let Some(&(_, s)) = pictures.iter().find(|p| p.0 == i) else { continue };
        let (w, h, sstride) = (s.width(), s.height(), s.stride() as usize);
        let mut src = vec![];
        let _ = s.with_data(|d| src = d.to_vec());
        let dst = &mut pages[page];
        let dstride = dst.stride() as usize;
        if let Some(mut d) = crate::util::view(dst) {
            for row in -1..=h {
                let sr = row.clamp(0, h - 1) as usize;
                for col in -1..=w {
                    let sc = col.clamp(0, w - 1) as usize;
                    let at = ((py + 1 + row) as usize) * dstride + ((px + 1 + col) as usize) * 4;
                    d[at..at + 4].copy_from_slice(&src[sr * sstride + sc * 4..sr * sstride + sc * 4 + 4]);
                }
            }
        }
    }
    (places, pages)
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
    /// Where draws go now: a render target, or the window.
    target: Option<Tex>,
    /// Whether the driver has framebuffer objects, for render targets.
    fbos: bool,
    customs: Vec<Custom>,
    uniforms: Vec<Uniform>,
    samplers: Vec<(GLint, Tex)>,
    prog: GLuint,
    /// The masked program, its `textured` and `retro_mask` uniforms.
    masked: (GLuint, GLint, GLint),
    vbo: GLuint,
    ibo: GLuint,
    indexed: usize, // quads the index buffer holds
    textured: GLint,
    core: bool,
    mipmaps: bool,
    /// Not Send or Sync: every GL call relies on its window's context being current, which it is on
    /// the thread that made it only.
    _thread: std::marker::PhantomData<*const ()>,
    /// A vec4 array for the next custom draw (`uniform_array`).
    next_array: Option<(GLint, Vec<f32>)>,
}

// names a theme's fragment shader shares with it start with retro_, which themes leave alone
const VS: &str = "uniform vec4 retro_view; // ortho projection: x scale, x offset, y scale, y offset
ATTR vec2 uv; ATTR vec4 col; ATTR vec2 pos;
VARY vec2 retro_t; VARY vec4 retro_c;
void main() {
  retro_t = uv; retro_c = col;
  gl_Position = vec4(retro_view.x * pos.x + retro_view.y, retro_view.z * pos.y + retro_view.w, 0.0, 1.0);
}";

// the masked program's: the mask's texture coordinates from the picture's
const VS_MASKED: &str = "uniform vec4 retro_view; uniform vec4 retro_mask;
ATTR vec2 uv; ATTR vec4 col; ATTR vec2 pos;
VARY vec2 retro_t; VARY vec4 retro_c; VARY vec2 retro_m;
void main() {
  retro_t = uv; retro_c = col; retro_m = pos * retro_mask.xy + retro_mask.zw;
  gl_Position = vec4(retro_view.x * pos.x + retro_view.y, retro_view.z * pos.y + retro_view.w, 0.0, 1.0);
}";

fn vs_of(core: bool, src: &str) -> String {
    let (head, inn, out) = if core { ("#version 330 core\n", "in", "out") } else { ("#version 120\n", "attribute", "varying") };
    format!("{head}{src}").replace("ATTR", inn).replace("VARY", out)
}

fn vs(core: bool) -> String {
    vs_of(core, VS)
}

fn fs_masked(core: bool) -> &'static str {
    if core {
        "#version 330 core\nuniform sampler2D tex; uniform sampler2D retro_mask_tex; uniform float textured;\n\
         in vec2 retro_t; in vec4 retro_c; in vec2 retro_m; out vec4 colour;\n\
         void main() { colour = (textured > 0.5 ? texture(tex, retro_t) * retro_c : retro_c) * texture(retro_mask_tex, retro_m).a; }"
    } else {
        "#version 120\nuniform sampler2D tex; uniform sampler2D retro_mask_tex; uniform float textured;\n\
         varying vec2 retro_t; varying vec4 retro_c; varying vec2 retro_m;\n\
         void main() { gl_FragColor = (textured > 0.5 ? texture2D(tex, retro_t) * retro_c : retro_c) * texture2D(retro_mask_tex, retro_m).a; }"
    }
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
        // render targets need the core names of the framebuffer calls: GL 3, or the ARB extension
        let fbos = core || major >= 3 || ext.contains("GL_ARB_framebuffer_object");
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
            let mprog = program(&vs_of(core, VS_MASKED), fs_masked(core)).unwrap_or_else(|e| panic!("the renderer's masked shaders: {e}"));
            set_view(mprog, rect);
            gl::Uniform1i(gl::GetUniformLocation(mprog, c"tex".as_ptr()), 0);
            gl::Uniform1i(gl::GetUniformLocation(mprog, c"retro_mask_tex".as_ptr()), 1);
            let masked = (mprog, gl::GetUniformLocation(mprog, c"textured".as_ptr()), gl::GetUniformLocation(mprog, c"retro_mask".as_ptr()));
            gl::UseProgram(prog);
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
                target: None,
                fbos,
                customs: vec![],
                uniforms: vec![],
                samplers: vec![],
                prog,
                masked,
                vbo,
                ibo,
                indexed: 0,
                textured,
                core,
                mipmaps,
                _thread: std::marker::PhantomData,
                next_array: None,
            };
            // the packed pictures onto pages, which go up after every other picture: page k is texture
            // n + k
            let n = up.0.len();
            let packed: Vec<(usize, &ImageSurface)> =
                up.0.iter().enumerate().filter(|(_, e)| e.3 && e.0.format() == Format::ARgb32).map(|(i, e)| (i, &e.0)).collect();
            let (places, pages) = pack(&packed);
            for (i, (s, kind, region, _)) in up.0.iter().enumerate() {
                match places.iter().find(|p| p.0 == i) {
                    Some(&(_, page, x, y)) => {
                        let (pw, ph) = (pages[page].width() as f32, pages[page].height() as f32);
                        let (w, h) = (s.width(), s.height());
                        let at = ((x + 1) as f32 / pw, (y + 1) as f32 / ph, w as f32 / pw, h as f32 / ph);
                        let packed = Some((Tex((n + page) as u32), at.0, at.1, at.2, at.3));
                        gfx.textures.push(Texture { id: 0, w, h, place: (0.0, 0.0, w as f64, h as f64), fbo: 0, stored: None, packed });
                    }
                    None => {
                        gfx.upload_region(s, *kind, *region);
                    }
                }
            }
            for page in &pages {
                gfx.upload(page, Kind::Smooth);
            }
            let ids: Vec<GLuint> = gfx.textures.iter().skip(n).map(|t| t.id).collect();
            for t in gfx.textures.iter_mut().take(n) {
                if let Some((page, ..)) = t.packed {
                    t.id = ids.get(page.0 as usize - n).copied().unwrap_or(0);
                }
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
        self.textures.push(Texture { id, w: 0, h: 0, place: (0.0, 0.0, 0.0, 0.0), fbo: 0, stored: None, packed: None });
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
        let mipmaps = matches!(kind, Kind::Sprite | Kind::Tile | Kind::Pixel);
        let internal = if src.format() == Format::Rgb24 { gl::RGB } else { gl::RGBA };
        // the same size and kind as before: the pixels go into the storage it has, not new storage, which
        // costs a driver a great deal more, every frame for a texture drawn again each frame
        let again = tx.stored == Some((w, h, internal, kind));
        tx.stored = Some((w, h, internal, kind));
        // SAFETY: a GL context is current, and no pixel unpack buffer is bound, so GL copies from
        // `data` before each call returns. `data` starts `src`'s stride x height bytes (`pixel_start`),
        // which `src` keeps alive and unchanged meanwhile (see `pixel_start`); every pointer handed to GL
        // is a row of the rectangle (x, y, w, h), which lies within the surface, and into a texture whose
        // storage is that size (`again`).
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tx.id);
            if again {
                gl::PixelStorei(gl::UNPACK_ROW_LENGTH, src.stride() / 4);
                // in strips of a megabyte or so, as a first upload is
                let rows = (UPLOAD_BYTES / (4 * w as usize)).max(1) as i32;
                for r in (0..h).step_by(rows as usize) {
                    let at = data.add((y + r) as usize * stride + x as usize * 4).cast();
                    gl::TexSubImage2D(gl::TEXTURE_2D, 0, 0, r, w, rows.min(h - r), gl::BGRA, gl::UNSIGNED_BYTE, at);
                }
                if mipmaps && self.mipmaps {
                    gl::GenerateMipmap(gl::TEXTURE_2D);
                }
                gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
                return;
            }
            let (min, mag, wrap) = match kind {
                Kind::Sprite => (gl::LINEAR_MIPMAP_LINEAR, gl::LINEAR, gl::CLAMP_TO_EDGE),
                Kind::Tile => (gl::LINEAR_MIPMAP_LINEAR, gl::LINEAR, gl::REPEAT),
                Kind::Smooth => (gl::LINEAR, gl::LINEAR, gl::CLAMP_TO_EDGE),
                Kind::Pixel => (gl::LINEAR_MIPMAP_NEAREST, gl::NEAREST, gl::CLAMP_TO_EDGE),
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

    /// Pixels into part of a texture, its top left at (x, y) of it: `w` x `h` of them, B, G, R, A, rows
    /// packed; straight from the bytes, no picture made of them first.
    pub fn replace_bytes(&mut self, t: Tex, data: &[u8], x: i32, y: i32, w: i32, h: i32) {
        let tx = &self.textures[t.0 as usize];
        if w <= 0 || h <= 0 || x < 0 || y < 0 || x + w > tx.w || y + h > tx.h || data.len() < (w * h * 4) as usize {
            return;
        }
        // SAFETY: a GL context is current, and no pixel unpack buffer is bound, so GL copies the w x h
        // pixels, rows packed (whole pixels, so the default unpack alignment holds), from `data`, which
        // holds that many (checked), before the call returns; the region lies within the texture
        // (checked).
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tx.id);
            gl::TexSubImage2D(gl::TEXTURE_2D, 0, x, y, w, h, gl::BGRA, gl::UNSIGNED_BYTE, data.as_ptr().cast());
        }
    }

    /// Reload columns x0..x1 of a texture.
    /// A picture into part of a texture, its top left at (x, y) of it.
    pub fn replace_at(&mut self, t: Tex, s: &ImageSurface, x: i32, y: i32) {
        let Some(data) = pixel_start(s).filter(|_| s.width() > 0 && s.height() > 0) else { return };
        let tx = &self.textures[t.0 as usize];
        if x < 0 || y < 0 || x + s.width() > tx.w || y + s.height() > tx.h {
            return;
        }
        // SAFETY: a GL context is current, and no pixel unpack buffer is bound, so GL copies the
        // width x height pixels from `data`, rows `stride` bytes apart, which lie within the surface's
        // stride x height bytes, before the call returns; the region lies within the texture (checked).
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tx.id);
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, s.stride() / 4);
            gl::TexSubImage2D(gl::TEXTURE_2D, 0, x, y, s.width(), s.height(), gl::BGRA, gl::UNSIGNED_BYTE, data.cast());
            gl::PixelStorei(gl::UNPACK_ROW_LENGTH, 0);
        }
    }

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

    /// A texture `w` x `h` that draws can go into (`target`), for a picture drawn small and shown large
    /// through a shader of its own: its texels sampled sharp, or `smooth`, between them. None where the
    /// driver has no framebuffer objects.
    pub fn render_target(&mut self, w: i32, h: i32, smooth: bool) -> Option<Tex> {
        if !self.fbos || w <= 0 || h <= 0 {
            return None;
        }
        // SAFETY: its window's context is current; the texture and framebuffer are this call's own, the
        // texture's storage made with no data; both are deleted if the framebuffer is not complete.
        unsafe {
            let (mut id, mut fbo) = (0, 0);
            gl::GenTextures(1, &mut id);
            gl::BindTexture(gl::TEXTURE_2D, id);
            gl::TexImage2D(gl::TEXTURE_2D, 0, gl::RGBA as i32, w, h, 0, gl::BGRA, gl::UNSIGNED_BYTE, std::ptr::null());
            let filter = if smooth { gl::LINEAR } else { gl::NEAREST };
            for (k, v) in [
                (gl::TEXTURE_MIN_FILTER, filter),
                (gl::TEXTURE_MAG_FILTER, filter),
                (gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE),
                (gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE),
            ] {
                gl::TexParameteri(gl::TEXTURE_2D, k, v as i32);
            }
            gl::GenFramebuffers(1, &mut fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
            gl::FramebufferTexture2D(gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, id, 0);
            let complete = gl::CheckFramebufferStatus(gl::FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE;
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            if !complete {
                gl::DeleteFramebuffers(1, &fbo);
                gl::DeleteTextures(1, &id);
                return None;
            }
            self.textures.push(Texture { id, w, h, place: (0.0, 0.0, w as f64, h as f64), fbo, stored: None, packed: None });
            Some(Tex(self.textures.len() as u32 - 1))
        }
    }

    /// Draw into a render target (`render_target`) from now on: the pane, scaled to fill it; or into
    /// the window again (None).
    pub fn target(&mut self, t: Option<Tex>) {
        self.target = t.filter(|t| self.textures.get(t.0 as usize).is_some_and(|tx| tx.fbo != 0));
    }

    /// Clip to a rectangle in GL window coordinates (from the bottom), or no clip.
    pub fn scissor(&mut self, rect: Option<(i32, i32, i32, i32)>) {
        self.scissor = rect;
    }

    /// Open (or extend) a run for `quads` quads that the caller appends next.
    fn run(&mut self, tex: Option<Tex>, blend: Blend, quads: usize) {
        self.run_masked(tex, None, blend, quads)
    }

    fn run_masked(&mut self, tex: Option<Tex>, mask: Option<Mask>, blend: Blend, quads: usize) {
        let first = self.verts.len() / 32;
        if let Some(last) = self.cmds.last_mut()
            && last.custom.is_none()
            && last.tex == tex
            && last.mask == mask
            && last.blend == blend
            && last.scissor == self.scissor
            && last.target == self.target
            && last.first + last.count == first
        {
            last.count += quads;
            return;
        }
        self.cmds.push(Cmd { target: self.target, custom: None, tex, mask, blend, scissor: self.scissor, first, count: quads });
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
        // a packed picture: drawn from its page, its corners' u, v moved to its place there
        if let Some((page, u0, v0, su, sv)) = tex.and_then(|t| self.textures[t.0 as usize].packed) {
            self.run(Some(page), blend, n);
            for corner in data[..n * 32].as_chunks::<8>().0 {
                self.verts.extend_from_slice(&[u0 + corner[0] * su, v0 + corner[1] * sv]);
                self.verts.extend_from_slice(&corner[2..]);
            }
            return;
        }
        self.run(tex, blend, n);
        self.verts.extend_from_slice(&data[..n * 32]);
    }

    /// Quads as `draw` makes them, each pixel times the alpha of `mask` there.
    pub fn draw_masked(&mut self, tex: Option<Tex>, mask: Mask, data: &[f32], blend: Blend) {
        let n = data.len() / 32;
        if n == 0 {
            return;
        }
        self.run_masked(tex, Some(mask), blend, n);
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
        self.layer_lit(tex, area, strength, 1.0, blend);
    }

    /// The same, its colour darkened by `light` (0 to 1) as it stays as solid: its alpha is kept.
    pub fn layer_lit(&mut self, tex: Tex, area: Option<&[(f64, f64, f64, f64)]>, strength: f64, light: f64, blend: Blend) {
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
        let l = k * light.clamp(0.0, 1.0) as f32;
        let c = [l, l, l, k];
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

    /// A vec4 array uniform (`data` four floats an element) for the next `custom` draw: a table a shader
    /// reads every frame goes up with its uniforms this way, which costs a driver far less than a
    /// texture changed each frame. Check `uniform_room` first.
    pub fn uniform_array(&mut self, loc: GLint, data: Vec<f32>) {
        self.next_array = Some((loc, data));
    }

    /// How many vec4 uniforms a fragment shader may have here.
    pub fn uniform_room(&self) -> i32 {
        let mut n = 0;
        // SAFETY: its window's context is current; one integer is written to `n`.
        unsafe {
            gl::GetIntegerv(0x8B49 /* GL_MAX_FRAGMENT_UNIFORM_COMPONENTS */, &mut n)
        };
        n / 4
    }

    /// One quad over (x0, y0)-(x1, y1), drawn by a theme's shader with these inputs.
    pub fn custom(&mut self, prog: GLuint, uniforms: &[Uniform], textures: &[(GLint, Tex)], x0: f64, y0: f64, x1: f64, y1: f64, blend: Blend) {
        let u = self.uniforms.len();
        self.uniforms.extend(uniforms.iter().filter(|x| x.0 >= 0));
        let t = self.samplers.len();
        self.samplers.extend_from_slice(textures);
        let custom = Custom { prog, uniforms: u..self.uniforms.len(), textures: t..self.samplers.len(), array: self.next_array.take() };
        let first = self.verts.len() / 32;
        let c = [1.0; 4];
        self.corner(0.0, 0.0, c, x0, y0);
        self.corner(1.0, 0.0, c, x1, y0);
        self.corner(1.0, 1.0, c, x1, y1);
        self.corner(0.0, 1.0, c, x0, y1);
        self.customs.push(custom);
        self.cmds.push(Cmd { target: self.target, custom: Some(self.customs.len() - 1), tex: None, mask: None, blend, scissor: self.scissor, first, count: 1 });
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
            let (mut blend, mut tex, mut sc, mut into) = (None, None, None, None);
            for c in &self.cmds {
                if into != Some(c.target) {
                    // the pane maps onto the whole of what is drawn into, so a target's viewport is all of it
                    match c.target.map(|t| &self.textures[t.0 as usize]) {
                        Some(tx) => {
                            gl::BindFramebuffer(gl::FRAMEBUFFER, tx.fbo);
                            gl::Viewport(0, 0, tx.w, tx.h);
                        }
                        None => {
                            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                            gl::Viewport(0, 0, self.rect.2, self.rect.3);
                        }
                    }
                    into = Some(c.target);
                }
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
                        Blend::Screen => {
                            gl::Enable(gl::BLEND);
                            gl::BlendFunc(gl::ONE, gl::ONE_MINUS_SRC_COLOR);
                        }
                        Blend::Add => {
                            gl::Enable(gl::BLEND);
                            gl::BlendFunc(gl::ONE, gl::ONE);
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
                    if let Some((loc, data)) = &cu.array {
                        gl::Uniform4fv(*loc, (data.len() / 4) as i32, data.as_ptr());
                    }
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
                if let Some(m) = c.mask {
                    let (mprog, textured, map) = self.masked;
                    gl::UseProgram(mprog);
                    gl::Uniform4f(map, m.map[0], m.map[1], m.map[2], m.map[3]);
                    gl::Uniform1f(textured, if c.tex.is_some() { 1.0 } else { 0.0 });
                    gl::ActiveTexture(gl::TEXTURE1);
                    gl::BindTexture(gl::TEXTURE_2D, self.textures[m.tex.0 as usize].id);
                    gl::ActiveTexture(gl::TEXTURE0);
                    if let Some(t) = c.tex {
                        gl::BindTexture(gl::TEXTURE_2D, self.textures[t.0 as usize].id);
                    }
                    gl::DrawElements(gl::TRIANGLES, (c.count * 6) as i32, gl::UNSIGNED_INT, at);
                    gl::UseProgram(self.prog);
                    tex = None; // unit 0 was bound for the masked draw
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
            if into.flatten().is_some() {
                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                gl::Viewport(0, 0, self.rect.2, self.rect.3);
            }
        }
        self.verts.clear();
        self.cmds.clear();
        self.customs.clear();
        self.uniforms.clear();
        self.samplers.clear();
        self.scissor = None;
        self.target = None;
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

#[cfg(test)]
mod pack_tests {
    use super::*;

    #[test]
    fn packed_pictures_are_whole_on_their_pages_each_with_its_own_edge_round_it() {
        let mut rng = crate::rng::Rng::new(5);
        let pictures: Vec<ImageSurface> = (0..120)
            .map(|k| {
                let (w, h) = (rng.int(1, 300) as i32, rng.int(1, 300) as i32);
                let mut s = crate::util::surface(w, h);
                let stride = s.stride() as usize;
                if let Some(mut d) = crate::util::view(&mut s) {
                    for y in 0..h as usize {
                        for x in 0..w as usize {
                            d[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&[(x % 256) as u8, (y % 256) as u8, k as u8, 255]);
                        }
                    }
                }
                s
            })
            .collect();
        let list: Vec<(usize, &ImageSurface)> = pictures.iter().enumerate().collect();
        let (places, pages) = pack(&list);
        assert_eq!(places.len(), pictures.len(), "all fit");
        let data: Vec<(Vec<u8>, usize)> = pages
            .iter()
            .map(|p| {
                let mut v = vec![];
                let _ = p.with_data(|d| v = d.to_vec());
                (v, p.stride() as usize)
            })
            .collect();
        let mut taken: Vec<(usize, i32, i32, i32, i32)> = vec![];
        for &(i, page, px, py) in &places {
            let s = &pictures[i];
            let (w, h) = (s.width(), s.height());
            let (d, stride) = &data[page];
            for row in -1..=h {
                for col in -1..=w {
                    let at = (py + 1 + row) as usize * stride + (px + 1 + col) as usize * 4;
                    let (x, y) = (col.clamp(0, w - 1) as usize, row.clamp(0, h - 1) as usize);
                    assert_eq!(&d[at..at + 4], &[(x % 256) as u8, (y % 256) as u8, i as u8, 255], "picture {i} at {col}, {row}");
                }
            }
            for &(p, x0, y0, x1, y1) in &taken {
                assert!(p != page || px + w + 2 <= x0 || x1 <= px || py + h + 2 <= y0 || y1 <= py, "picture {i} overlaps another");
            }
            taken.push((page, px, py, px + w + 2, py + h + 2));
        }
    }
}
