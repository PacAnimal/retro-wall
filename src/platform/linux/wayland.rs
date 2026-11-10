//! Wayland: a layer-shell surface on each output's background layer (wlr-layer-shell, which wlroots
//! compositors such as sway and Hyprland, KDE's KWin, labwc, niri and COSMIC offer), drawn with EGL.
//! Below every window and on every workspace by the protocol itself; an empty input region lets clicks
//! through; no taskbar entry. A compositor without layer-shell (GNOME's) is left to the X11 path,
//! through XWayland.
//!
//! What X11 tells by looking at windows, Wayland does not tell a client. A surface the compositor does
//! not show (covered by a window, on a monitor that is off) gets no frame callbacks: a window whose
//! callback is overdue counts as covered, so the frame loop pauses it as it pauses a covered X11 wall.
//!
//! libwayland-client and libEGL are opened at run time: a system without them runs on X11.

use crate::platform::{Edges, Rect, Screen, WindowId};
use khronos_egl as egl;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_callback, wl_compositor, wl_output, wl_region, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

/// How long a frame callback may be outstanding before its window counts as covered.
const OVERDUE: Duration = Duration::from_millis(1500);
/// How often a window that draws nothing asks again whether it is shown.
const ASK_EVERY: Duration = Duration::from_secs(1);

type Egl = egl::DynamicInstance<egl::EGL1_4>;
/// EGL_PLATFORM_WAYLAND_KHR (EGL_KHR_platform_wayland), which khronos-egl does not name.
const PLATFORM_WAYLAND: egl::Enum = 0x31D8;

/// An output as its events have told it.
#[derive(Default)]
struct Output {
    global: u32,
    wl: Option<wl_output::WlOutput>,
    xdg: Option<zxdg_output_v1::ZxdgOutputV1>,
    name: String,
    /// The logical position (xdg-output), where the compositor places it in the desktop.
    at: (i32, i32),
    /// Its current mode, in pixels, turned as the output is turned.
    pixels: (i32, i32),
    /// Its size in the desktop's units (xdg-output): the pixels over it are its scale, fractional too.
    logical: (i32, i32),
    turned: bool,
    scale: i32,
    mm: (u32, u32),
    /// All of its description has come (wl_output.done): until then it is not reported.
    ready: bool,
    /// The compositor closed the wallpaper on it: not reported until that window is closed, so the frame
    /// loop drops the monitor and makes it again.
    lost: bool,
}

impl Output {
    fn screen(&self, primary: bool) -> Screen {
        let (w, h) = if self.turned { (self.pixels.1, self.pixels.0) } else { self.pixels };
        let dpi = if self.logical.0 > 0 { (96 * w as i64 / self.logical.0 as i64) as u32 } else { 96 * self.scale.max(1) as u32 };
        Screen { name: self.name.clone(), x: self.at.0, y: self.at.1, w, h, primary, dpi, mm: self.mm }
    }
}

/// One wallpaper surface and its GL side.
struct Win {
    surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    /// The output it is on (its registry name).
    output: u32,
    viewport: wp_viewport::WpViewport,
    egl_window: wayland_egl::WlEglSurface,
    egl_surface: egl::Surface,
    ctx: egl::Context,
    core: bool,
    /// Where it is, as the screens report it: what covering() names when it is not shown.
    area: Rect,
}

/// What the event handlers change.
#[derive(Default)]
struct State {
    /// To ask each output for its logical position and name, where the compositor offers it.
    xdg_outputs: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    outputs: Vec<Output>,
    /// Layer surfaces by window: the size the compositor gave them, once configured.
    configured: HashMap<u64, (u32, u32)>,
    /// Windows the compositor asked to draw again (a first or new configure).
    exposed: Vec<WindowId>,
    /// Windows whose layer surface the compositor closed (its output went).
    closed: Vec<WindowId>,
    /// Per window: when its outstanding frame callback was asked for, if one is.
    asked: HashMap<u64, Instant>,
    /// Per window: when its last frame callback came.
    shown: HashMap<u64, Instant>,
}

struct Globals {
    compositor: wl_compositor::WlCompositor,
    layer_shell: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    viewporter: wp_viewporter::WpViewporter,
}

pub struct Wayland {
    conn: Connection,
    queue: RefCell<EventQueue<State>>,
    qh: QueueHandle<State>,
    state: RefCell<State>,
    globals: Globals,
    egl: Egl,
    display: egl::Display,
    config: egl::Config,
    wins: HashMap<WindowId, Win>,
    next: u64,
    gl_loaded: bool,
}

impl Wayland {
    /// The compositor of WAYLAND_DISPLAY, if it offers layer-shell and viewporter (every compositor with
    /// layer-shell has it); else why not.
    pub fn open() -> Result<Wayland, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland compositor: {e}"))?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("Wayland registry: {e}"))?;
        let qh = queue.handle();
        let layer_shell = globals.bind(&qh, 1..=4, ()).map_err(|_| "the compositor has no layer-shell (wlr-layer-shell)".to_string())?;
        let compositor = globals.bind(&qh, 4..=6, ()).map_err(|e| format!("wl_compositor: {e}"))?;
        let viewporter = globals.bind(&qh, 1..=1, ()).map_err(|_| "the compositor has no viewporter (wp_viewporter)".to_string())?;
        let mut state = State { xdg_outputs: globals.bind(&qh, 1..=3, ()).ok(), ..State::default() };
        let found: Vec<(u32, u32)> =
            globals.contents().with_list(|l| l.iter().filter(|g| g.interface == wl_output::WlOutput::interface().name).map(|g| (g.name, g.version)).collect());
        for (name, version) in found {
            state.add_output(globals.registry(), &qh, name, version);
        }
        let g = Globals { compositor, layer_shell, viewporter };
        // the outputs' and xdg-outputs' first events
        queue.roundtrip(&mut state).map_err(|e| format!("Wayland: {e}"))?;
        queue.roundtrip(&mut state).map_err(|e| format!("Wayland: {e}"))?;
        // SAFETY: libEGL is opened by name; the instance keeps it loaded while the backend lives.
        let egl = unsafe { Egl::load_required() }.map_err(|e| format!("no libEGL: {e}"))?;
        let ptr = conn.backend().display_ptr() as *mut std::ffi::c_void;
        // the display asked for by platform (EGL 1.5), else by the pointer alone, which EGL has to guess at
        let display = match egl.upcast::<egl::EGL1_5>() {
            // SAFETY: the connection's own wl_display, alive as long as `conn`, which the backend holds.
            Some(e15) => unsafe { e15.get_platform_display(PLATFORM_WAYLAND, ptr, &[egl::ATTRIB_NONE]) }.ok(),
            // SAFETY: as above.
            None => unsafe { egl.get_display(ptr as egl::NativeDisplayType) },
        }
        .ok_or("EGL has no display for Wayland")?;
        egl.initialize(display).map_err(|e| format!("EGL: {e}"))?;
        egl.bind_api(egl::OPENGL_API).map_err(|e| format!("EGL has no desktop OpenGL: {e}"))?;
        // opaque: a buffer with alpha would let what is under the wallpaper through where a frame left it below 1
        let attrs =
            [egl::SURFACE_TYPE, egl::WINDOW_BIT, egl::RENDERABLE_TYPE, egl::OPENGL_BIT, egl::RED_SIZE, 8, egl::GREEN_SIZE, 8, egl::BLUE_SIZE, 8, egl::NONE];
        let mut configs = Vec::with_capacity(64);
        egl.choose_config(display, &attrs, &mut configs).map_err(|e| format!("EGL: {e}"))?;
        let alpha = |c: &egl::Config| egl.get_config_attrib(display, *c, egl::ALPHA_SIZE).unwrap_or(0);
        let config = configs.iter().copied().min_by_key(alpha).ok_or("no EGL config with 8-bit colour for OpenGL")?;
        Ok(Wayland {
            conn,
            queue: RefCell::new(queue),
            qh,
            state: RefCell::new(state),
            globals: g,
            egl,
            display,
            config,
            wins: HashMap::new(),
            next: 1,
            gl_loaded: false,
        })
    }

    /// Events already read and queued, handled; and what is waiting on the socket.
    fn dispatch(&self) {
        let mut q = self.queue.borrow_mut();
        let mut st = self.state.borrow_mut();
        alive(q.dispatch_pending(&mut st));
        if let Some(guard) = q.prepare_read() {
            alive(guard.read()); // the socket does not block: nothing there, nothing read
        }
        alive(q.dispatch_pending(&mut st));
    }

    fn flush(&self) {
        alive(self.conn.flush());
    }

    /// A 3.3 core context, else any OpenGL the driver gives (2.1 or more), sharing objects with `share`.
    fn context(&self, share: Option<egl::Context>) -> Option<(egl::Context, bool)> {
        use egl::{CONTEXT_MAJOR_VERSION, CONTEXT_MINOR_VERSION, CONTEXT_OPENGL_CORE_PROFILE_BIT, CONTEXT_OPENGL_PROFILE_MASK, NONE};
        let core = [CONTEXT_MAJOR_VERSION, 3, CONTEXT_MINOR_VERSION, 3, CONTEXT_OPENGL_PROFILE_MASK, CONTEXT_OPENGL_CORE_PROFILE_BIT, NONE];
        if std::env::var("RETRO_GL").as_deref() != Ok("2.1")
            && let Ok(c) = self.egl.create_context(self.display, self.config, share, &core)
        {
            return Some((c, true));
        }
        self.egl.create_context(self.display, self.config, share, &[NONE]).ok().map(|c| (c, false))
    }

    /// Asks for a frame callback on the window and commits: it comes when the compositor shows it.
    fn ask(&self, id: WindowId, w: &Win) {
        w.surface.frame(&self.qh, id.0);
        self.state.borrow_mut().asked.entry(id.0).or_insert_with(Instant::now);
    }

    /// The output a screen at (x, y) is: one surface for each whole output, found by the corner its
    /// screen reports. A screen's corner is the output's logical position, and its size the output's
    /// pixels: on a scaled output the two differ in kind, so outputs are matched by corner, not by area.
    /// Outputs that mirror one another share a corner: each gets one wallpaper, the first without.
    fn output_at(&self, x: i32, y: i32) -> Option<(u32, wl_output::WlOutput)> {
        let free = |o: &&Output| !self.wins.values().any(|w| w.output == o.global);
        self.state.borrow().outputs.iter().filter(|o| o.ready && o.at == (x, y)).find(free).and_then(|o| Some((o.global, o.wl.clone()?)))
    }

    /// The GL side of a configured surface, `lw` by `lh` in the output's units, drawn at w by h pixels:
    /// a viewport scales the buffer to the surface. On an error what it made is undone.
    fn attach(&mut self, surface: &wl_surface::WlSurface, (lw, lh): (u32, u32), w: i32, h: i32) -> Result<GlSide, String> {
        let viewport = self.globals.viewporter.get_viewport(surface, &self.qh, ());
        viewport.set_destination(lw as i32, lh as i32);
        let undo = |v: &wp_viewport::WpViewport, e: String| {
            v.destroy();
            Err(e)
        };
        let egl_window = match wayland_egl::WlEglSurface::new(surface.id(), w, h) {
            Ok(e) => e,
            Err(e) => return undo(&viewport, format!("wayland-egl: {e}")),
        };
        // SAFETY: the wl_egl_window pointer is valid while egl_window lives, which the Win holds as long
        // as the EGL surface made from it.
        let made = unsafe { self.egl.create_window_surface(self.display, self.config, egl_window.ptr() as egl::NativeWindowType, None) };
        let egl_surface = match made {
            Ok(s) => s,
            Err(e) => return undo(&viewport, format!("EGL surface: {e}")),
        };
        let share = self.wins.values().next().map(|w| w.ctx);
        let Some((ctx, core)) = self.context(share) else {
            let _ = self.egl.destroy_surface(self.display, egl_surface);
            return undo(&viewport, "the driver made no OpenGL context".into());
        };
        if let Err(e) = self.egl.make_current(self.display, Some(egl_surface), Some(egl_surface), Some(ctx)) {
            let _ = self.egl.destroy_context(self.display, ctx);
            let _ = self.egl.destroy_surface(self.display, egl_surface);
            return undo(&viewport, format!("EGL: {e}"));
        }
        if !self.gl_loaded {
            gl::load_with(|s| self.egl.get_proc_address(s).map_or(std::ptr::null(), |f| f as *const _));
            self.gl_loaded = true;
        }
        // never wait for the compositor in a swap: a hidden surface's swap would wait for ever
        let _ = self.egl.swap_interval(self.display, 0);
        Ok(GlSide { viewport, egl_window, egl_surface, ctx, core })
    }
}

/// A surface's GL side, as `attach` makes it.
struct GlSide {
    viewport: wp_viewport::WpViewport,
    egl_window: wayland_egl::WlEglSurface,
    egl_surface: egl::Surface,
    ctx: egl::Context,
    core: bool,
}

/// A Wayland call's error that ends the connection, or only a full socket buffer (WouldBlock).
trait Lost: std::fmt::Display {
    fn would_block(&self) -> bool;
}

impl Lost for wayland_client::backend::WaylandError {
    fn would_block(&self) -> bool {
        matches!(self, Self::Io(e) if e.kind() == std::io::ErrorKind::WouldBlock)
    }
}

impl Lost for wayland_client::DispatchError {
    fn would_block(&self) -> bool {
        matches!(self, Self::Backend(e) if e.would_block())
    }
}

/// A Wayland call's result; the connection gone (the compositor ended, or a protocol error) ends the
/// program, as Xlib ends it when its display goes: nothing is left to draw on.
fn alive<T: Default, E: Lost>(r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) if e.would_block() => T::default(),
        Err(e) => {
            crate::errln!("Wayland: the connection to the compositor is gone: {e}");
            std::process::exit(1)
        }
    }
}

impl State {
    /// An output, bound with its xdg-output: at open, and when one is plugged in.
    fn add_output(&mut self, registry: &wl_registry::WlRegistry, qh: &QueueHandle<State>, name: u32, version: u32) {
        let wl: wl_output::WlOutput = registry.bind(name, version.min(4), qh, name);
        let xdg = self.xdg_outputs.as_ref().map(|m| m.get_xdg_output(&wl, qh, name));
        self.outputs.push(Output { global: name, wl: Some(wl), xdg, scale: 1, ..Output::default() });
    }
}

impl crate::platform::Display for Wayland {
    fn screens(&self) -> Vec<Screen> {
        self.dispatch();
        let st = self.state.borrow();
        st.outputs.iter().filter(|o| o.ready && !o.lost && o.pixels.0 > 0).enumerate().map(|(i, o)| o.screen(i == 0)).collect()
    }

    /// Panels on Wayland reserve their room only from surfaces that keep to it, which a wallpaper does
    /// not (it lies under them): their edges are not told. `--reserve-*` sets them.
    fn work_edges(&self, _s: &Screen) -> Edges {
        Edges::NONE
    }

    /// A layer surface over the whole output whose screen is at (x, y), drawn at w by h pixels.
    fn window(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<WindowId, String> {
        use zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity};
        if w <= 0 || h <= 0 {
            return Err(format!("a window of {w}x{h}"));
        }
        let (global, output) = self.output_at(x, y).ok_or(format!("no output at {x},{y}"))?;
        let id = WindowId(self.next);
        self.next += 1;
        let g = &self.globals;
        let surface = g.compositor.create_surface(&self.qh, ());
        let layer = g.layer_shell.get_layer_surface(&surface, Some(&output), zwlr_layer_shell_v1::Layer::Background, "wallpaper".into(), &self.qh, id.0);
        layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
        layer.set_size(0, 0); // the output's size, as the compositor configures it
        layer.set_exclusive_zone(-1); // under the panels too
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        let region = g.compositor.create_region(&self.qh, ());
        surface.set_input_region(Some(&region)); // empty: clicks go past it
        region.add(0, 0, i32::MAX, i32::MAX);
        surface.set_opaque_region(Some(&region)); // all of it: nothing under it is drawn
        region.destroy();
        surface.commit();
        // the first configure says the surface's logical size
        let mut logical = None;
        for _ in 0..50 {
            alive(self.queue.borrow_mut().roundtrip(&mut self.state.borrow_mut()));
            logical = self.state.borrow().configured.get(&id.0).copied();
            if logical.is_some() || self.state.borrow().closed.contains(&id) {
                break;
            }
        }
        let side = match logical.filter(|(a, b)| *a > 0 && *b > 0) {
            Some(size) => self.attach(&surface, size, w, h),
            None => Err("the compositor gave the wallpaper no size".into()),
        };
        let gl = match side {
            Ok(gl) => gl,
            Err(e) => {
                layer.destroy();
                surface.destroy();
                self.state.borrow_mut().configured.remove(&id.0);
                self.flush();
                return Err(e);
            }
        };
        let area = Rect::new(x as i64, y as i64, (x + w) as i64, (y + h) as i64);
        let GlSide { viewport, egl_window, egl_surface, ctx, core } = gl;
        let win = Win { surface, layer, output: global, viewport, egl_window, egl_surface, ctx, core, area };
        self.ask(id, &win);
        self.wins.insert(id, win);
        self.flush();
        Ok(id)
    }

    fn core(&self, w: WindowId) -> bool {
        self.wins.get(&w).is_some_and(|w| w.core)
    }

    fn make_current(&mut self, w: WindowId) {
        if let Some(win) = self.wins.get(&w) {
            let _ = self.egl.make_current(self.display, Some(win.egl_surface), Some(win.egl_surface), Some(win.ctx));
        }
    }

    fn swap(&self, w: WindowId) {
        let Some(win) = self.wins.get(&w) else { return };
        if !self.state.borrow().asked.contains_key(&w.0) {
            self.ask(w, win);
        }
        let _ = self.egl.swap_buffers(self.display, win.egl_surface);
        self.flush();
    }

    /// The background layer keeps its surfaces below every window by itself.
    fn raise(&self, _w: WindowId) {}

    fn close(&mut self, w: WindowId) {
        let Some(win) = self.wins.remove(&w) else { return };
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, win.egl_surface);
        let _ = self.egl.destroy_context(self.display, win.ctx);
        drop(win.egl_window);
        win.viewport.destroy();
        win.layer.destroy();
        win.surface.destroy();
        let st = &mut *self.state.borrow_mut();
        st.asked.remove(&w.0);
        st.shown.remove(&w.0);
        st.configured.remove(&w.0);
        // its output, if the compositor had closed the wallpaper on it, is reported again: made anew
        if let Some(o) = st.outputs.iter_mut().find(|o| o.global == win.output) {
            o.lost = false;
        }
        self.flush();
    }

    fn renderer(&self) -> String {
        crate::gfx::renderer()
    }

    /// A wallpaper the compositor closed takes its output out of the screens until the frame loop has
    /// closed that window: the loop drops the monitor, and makes it again when it is back.
    fn events(&mut self) -> Vec<WindowId> {
        self.dispatch();
        let mut st = self.state.borrow_mut();
        for id in std::mem::take(&mut st.closed) {
            let Some(global) = self.wins.get(&id).map(|w| w.output) else { continue };
            if let Some(o) = st.outputs.iter_mut().find(|o| o.global == global) {
                o.lost = true;
            }
        }
        std::mem::take(&mut st.exposed)
    }

    fn wait(&self, timeout: Duration) {
        self.flush();
        let mut q = self.queue.borrow_mut();
        alive(q.dispatch_pending(&mut self.state.borrow_mut()));
        let Some(guard) = q.prepare_read() else { return };
        let fd = std::os::fd::AsRawFd::as_raw_fd(&guard.connection_fd());
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let ts = libc::timespec { tv_sec: timeout.as_secs() as libc::time_t, tv_nsec: timeout.subsec_nanos() as _ };
        // SAFETY: ppoll on the connection's descriptor, with a local pollfd and timespec.
        if unsafe { libc::ppoll(&mut pfd, 1, &ts, std::ptr::null()) } > 0 {
            alive(guard.read());
        }
        alive(q.dispatch_pending(&mut self.state.borrow_mut()));
    }

    /// The windows the compositor does not show: their frame callback overdue. A window that has none
    /// outstanding asks for one, so one shown again is seen within a second.
    fn covering(&self) -> Option<Vec<Rect>> {
        self.dispatch();
        let now = Instant::now();
        let mut out = vec![];
        for (id, w) in &self.wins {
            let (asked, shown) = {
                let st = self.state.borrow();
                (st.asked.get(&id.0).copied(), st.shown.get(&id.0).copied())
            };
            match asked {
                Some(at) if now - at > OVERDUE => out.push(w.area),
                Some(_) => {}
                None if shown.is_none_or(|s| now - s > ASK_EVERY) => {
                    self.ask(*id, w);
                    w.surface.commit();
                }
                None => {}
            }
        }
        self.flush();
        Some(out)
    }

    /// No compositor shows a client's picture as its wallpaper while the client runs: the wall draws all
    /// of each monitor itself, the still part too.
    fn wallpaper_hidden(&self) -> bool {
        true
    }

    /// An output that is off shows nothing, so its wallpaper's frame callbacks stop and it counts as
    /// covered (above): the frame loop pauses it either way.
    fn screen_off(&self) -> bool {
        false
    }
}

impl Drop for Wayland {
    fn drop(&mut self) {
        let ids: Vec<WindowId> = self.wins.keys().copied().collect();
        for id in ids {
            crate::platform::Display::close(self, id);
        }
        let _ = self.egl.terminate(self.display);
    }
}

// --- events ------------------------------------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(st: &mut State, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &GlobalListContents, _: &Connection, qh: &QueueHandle<State>) {
        match event {
            // an output plugged in: bound now; its xdg-output, if any, from the manager bound at open
            wl_registry::Event::Global { name, interface, version } if interface == wl_output::WlOutput::interface().name => {
                st.add_output(registry, qh, name, version)
            }
            wl_registry::Event::GlobalRemove { name } => st.outputs.retain(|o| {
                if o.global != name {
                    return true;
                }
                if let Some(x) = &o.xdg {
                    x.destroy();
                }
                if let Some(w) = o.wl.as_ref().filter(|w| w.version() >= 3) {
                    w.release();
                }
                false
            }),
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(st: &mut State, _: &wl_output::WlOutput, event: wl_output::Event, global: &u32, _: &Connection, _: &QueueHandle<State>) {
        let Some(o) = st.outputs.iter_mut().find(|o| o.global == *global) else { return };
        match event {
            wl_output::Event::Geometry { x, y, physical_width, physical_height, transform, .. } => {
                if o.xdg.is_none() {
                    o.at = (x, y);
                }
                o.mm = (physical_width.max(0) as u32, physical_height.max(0) as u32);
                use wl_output::Transform as T;
                o.turned = matches!(transform.into_result(), Ok(T::_90 | T::_270 | T::Flipped90 | T::Flipped270));
            }
            wl_output::Event::Mode { flags, width, height, .. } if flags.into_result().is_ok_and(|f| f.contains(wl_output::Mode::Current)) => {
                o.pixels = (width, height);
                // an output of version 1 sends no done: its mode is all there is to wait for
                o.ready |= o.wl.as_ref().is_some_and(|w| w.version() < 2);
            }
            // with xdg-output, its logical place too: a first done can come before it
            wl_output::Event::Done => o.ready = o.xdg.is_none() || o.logical.0 > 0,
            wl_output::Event::Scale { factor } => o.scale = factor,
            wl_output::Event::Name { name } => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for State {
    fn event(st: &mut State, _: &zxdg_output_v1::ZxdgOutputV1, event: zxdg_output_v1::Event, global: &u32, _: &Connection, _: &QueueHandle<State>) {
        let Some(o) = st.outputs.iter_mut().find(|o| o.global == *global) else { return };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => o.at = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } => o.logical = (width, height),
            // xdg-output 1 and 2 end their description with a done of their own; 3 with wl_output's
            zxdg_output_v1::Event::Done => o.ready = o.pixels.0 > 0,
            zxdg_output_v1::Event::Name { name } if o.name.is_empty() => o.name = name,
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u64> for State {
    fn event(
        st: &mut State,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                if st.configured.insert(*id, (width, height)) != Some((width, height)) {
                    st.exposed.push(WindowId(*id));
                }
            }
            zwlr_layer_surface_v1::Event::Closed => st.closed.push(WindowId(*id)),
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, u64> for State {
    fn event(st: &mut State, _: &wl_callback::WlCallback, event: wl_callback::Event, id: &u64, _: &Connection, _: &QueueHandle<State>) {
        if let wl_callback::Event::Done { .. } = event {
            st.asked.remove(id);
            st.shown.insert(*id, Instant::now());
        }
    }
}

delegate_noop!(State: wl_compositor::WlCompositor);
delegate_noop!(State: wl_region::WlRegion);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(State: wp_viewporter::WpViewporter);
delegate_noop!(State: wp_viewport::WpViewport);
delegate_noop!(State: zxdg_output_manager_v1::ZxdgOutputManagerV1);
