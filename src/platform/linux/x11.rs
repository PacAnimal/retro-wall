//! X11 and GLX: wallpaper windows (desktop type, below all others, on every workspace, off the taskbar,
//! click-through), each with a GL context sharing the first one's objects, and what the display knows:
//! monitors, panel struts, DPMS, and which windows cover the desktop.

use crate::platform::{Edges, Rect, Screen, WindowId};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong};
use std::ptr::null_mut;
use std::time::Duration;
use x11_dl::{dpms, glx, xfixes, xlib, xrandr};

/// The atoms used, interned in one round trip when the display opens.
struct Atoms {
    atom: c_ulong,
    cardinal: c_ulong,
    stacking: c_ulong,
    clients: c_ulong,
    strut_partial: c_ulong,
    strut: c_ulong,
    frame_extents: c_ulong,
    wm_type: c_ulong,
    wm_state: c_ulong,
    wm_desktop: c_ulong,
    motif_hints: c_ulong,
    type_desktop: c_ulong,
    state_hidden: c_ulong,
    state_below: c_ulong,
    state_sticky: c_ulong,
    skip_taskbar: c_ulong,
    skip_pager: c_ulong,
    compositor: c_ulong,
    /// Window types that never hide the desktop: the desktop's own, popups, tooltips, notifications.
    passing: [c_ulong; 6],
}

impl Atoms {
    fn intern(xl: &xlib::Xlib, dpy: *mut xlib::Display, screen: c_int) -> Atoms {
        let compositor = format!("_NET_WM_CM_S{screen}");
        let names = [
            "ATOM",
            "CARDINAL",
            "_NET_CLIENT_LIST_STACKING",
            "_NET_CLIENT_LIST",
            "_NET_WM_STRUT_PARTIAL",
            "_NET_WM_STRUT",
            "_NET_FRAME_EXTENTS",
            "_NET_WM_WINDOW_TYPE",
            "_NET_WM_STATE",
            "_NET_WM_DESKTOP",
            "_MOTIF_WM_HINTS",
            "_NET_WM_WINDOW_TYPE_DESKTOP",
            "_NET_WM_STATE_HIDDEN",
            "_NET_WM_STATE_BELOW",
            "_NET_WM_STATE_STICKY",
            "_NET_WM_STATE_SKIP_TASKBAR",
            "_NET_WM_STATE_SKIP_PAGER",
            &compositor,
            "_NET_WM_WINDOW_TYPE_NOTIFICATION",
            "_NET_WM_WINDOW_TYPE_TOOLTIP",
            "_NET_WM_WINDOW_TYPE_DND",
            "_NET_WM_WINDOW_TYPE_POPUP_MENU",
            "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU",
        ];
        let owned: Vec<CString> = names.iter().map(|n| CString::new(*n).unwrap_or_default()).collect();
        let mut ptrs: Vec<*mut c_char> = owned.iter().map(|c| c.as_ptr() as *mut c_char).collect();
        let mut a = vec![0 as c_ulong; names.len()];
        // SAFETY: an open display; `ptrs` holds names.len() NUL-terminated names that outlive the call, `a` room for as many atoms.
        unsafe { (xl.XInternAtoms)(dpy, ptrs.as_mut_ptr(), names.len() as c_int, 0, a.as_mut_ptr()) };
        Atoms {
            atom: a[0],
            cardinal: a[1],
            stacking: a[2],
            clients: a[3],
            strut_partial: a[4],
            strut: a[5],
            frame_extents: a[6],
            wm_type: a[7],
            wm_state: a[8],
            wm_desktop: a[9],
            motif_hints: a[10],
            type_desktop: a[11],
            state_hidden: a[12],
            state_below: a[13],
            state_sticky: a[14],
            skip_taskbar: a[15],
            skip_pager: a[16],
            compositor: a[17],
            passing: [a[11], a[18], a[19], a[20], a[21], a[22]],
        }
    }
}

struct Win {
    ctx: glx::GLXContext,
    core: bool,
    place: (i32, i32),
    moved_back: u32,
}

pub struct X {
    xl: xlib::Xlib,
    glx: glx::Glx,
    xf: xfixes::Xlib,
    dp: dpms::Xext,
    rr: xrandr::Xrandr,
    dpy: *mut xlib::Display,
    root: c_ulong,
    screen: c_int,
    atoms: Atoms,
    wins: HashMap<WindowId, Win>,
    /// A live context whose objects new ones share.
    share: glx::GLXContext,
    gl_loaded: bool,
}

impl X {
    pub fn open() -> Result<X, String> {
        let xl = xlib::Xlib::open().map_err(|e| format!("libX11: {e}"))?;
        let glx = glx::Glx::open().map_err(|e| format!("libGL: {e}"))?;
        let xf = xfixes::Xlib::open().map_err(|e| format!("libXfixes: {e}"))?;
        let dp = dpms::Xext::open().map_err(|e| format!("libXext: {e}"))?;
        let rr = xrandr::Xrandr::open().map_err(|e| format!("libXrandr: {e}"))?;
        // SAFETY: the libraries are loaded; each call is on the display just opened, checked for null.
        unsafe {
            let dpy = (xl.XOpenDisplay)(std::ptr::null());
            if dpy.is_null() {
                return Err("no X display".into());
            }
            unsafe extern "C" fn quiet(_: *mut xlib::Display, _: *mut xlib::XErrorEvent) -> c_int {
                0
            }
            (xl.XSetErrorHandler)(Some(quiet));
            let root = (xl.XDefaultRootWindow)(dpy);
            let screen = (xl.XDefaultScreen)(dpy);
            let atoms = Atoms::intern(&xl, dpy, screen);
            Ok(X { xl, glx, xf, dp, rr, dpy, root, screen, atoms, wins: HashMap::new(), share: null_mut(), gl_loaded: false })
        }
    }

    fn longs(&self, win: c_ulong, atom: c_ulong, count: c_long) -> Option<Vec<c_ulong>> {
        // SAFETY: an open display; XGetWindowProperty's data, checked for null, holds `n` longs and is freed with XFree.
        unsafe {
            let (mut kind, mut form, mut n, mut after, mut data) = (0, 0, 0, 0, null_mut::<c_uchar>());
            if (self.xl.XGetWindowProperty)(self.dpy, win, atom, 0, count, 0, 0, &mut kind, &mut form, &mut n, &mut after, &mut data) != 0 || data.is_null() {
                return None;
            }
            let out = (form == 32).then(|| std::slice::from_raw_parts(data as *const c_ulong, n as usize).to_vec());
            (self.xl.XFree)(data as *mut _);
            out
        }
    }

    fn screen_size(&self) -> (i32, i32) {
        // SAFETY: an open display and its default screen.
        unsafe { ((self.xl.XDisplayWidth)(self.dpy, self.screen), (self.xl.XDisplayHeight)(self.dpy, self.screen)) }
    }

    fn clients(&self) -> Option<Vec<c_ulong>> {
        self.longs(self.root, self.atoms.stacking, 4096).or_else(|| self.longs(self.root, self.atoms.clients, 4096))
    }

    /// Whether DPMS has the screen in standby, suspend or off.
    fn dpms_off(&self) -> bool {
        // SAFETY: an open display; out-pointers to locals.
        unsafe {
            let (mut a, mut b) = (0, 0);
            if (self.dp.DPMSQueryExtension)(self.dpy, &mut a, &mut b) == 0 {
                return false;
            }
            let (mut level, mut enabled) = (0u16, 0u8);
            if (self.dp.DPMSInfo)(self.dpy, &mut level, &mut enabled) == 0 {
                return false;
            }
            enabled != 0 && level != 0
        }
    }

    /// Whether a compositing manager runs on the screen.
    fn composited(&self) -> bool {
        // SAFETY: an open display and an atom of it.
        unsafe { (self.xl.XGetSelectionOwner)(self.dpy, self.atoms.compositor) != 0 }
    }

    /// Whether the screen's GLX offers `name`. glXGetProcAddressARB answers for any name, so an
    /// extension's function is looked up only once this says it is there.
    fn glx_extension(&self, name: &str) -> bool {
        // SAFETY: an open display and its screen; the answer is null or a NUL-terminated string GLX keeps.
        let list = unsafe { (self.glx.glXQueryExtensionsString)(self.dpy, self.screen) };
        // SAFETY: checked for null; NUL-terminated, as above.
        !list.is_null() && unsafe { CStr::from_ptr(list) }.to_bytes().split(|&b| b == b' ').any(|e| e == name.as_bytes())
    }

    /// A GL context for the visual, sharing objects with `self.share`: 3.3 core where the driver offers
    /// it, else legacy (2.1+). RETRO_GL=2.1 forces legacy, for tests.
    ///
    /// # Safety
    /// `vi` is a visual this display gave, not yet freed.
    unsafe fn context(&self, vi: *mut xlib::XVisualInfo) -> Option<(glx::GLXContext, bool)> {
        // SAFETY: an open display and a live visual of it (the caller's promise); the FBConfig list is
        // read within its count and freed once; the attribute list ends in 0; `create` is
        // glXCreateContextAttribsARB, which the screen offers, of the type its specification gives.
        unsafe {
            let legacy = || {
                let c = (self.glx.glXCreateContext)(self.dpy, vi, self.share, 1);
                (!c.is_null()).then_some((c, false))
            };
            if std::env::var("RETRO_GL").is_ok_and(|v| v == "2.1") || !self.glx_extension("GLX_ARB_create_context_profile") {
                return legacy();
            }
            let Some(create) = (self.glx.glXGetProcAddressARB)(c"glXCreateContextAttribsARB".as_ptr() as *const u8) else {
                return legacy();
            };
            type Create = unsafe extern "C" fn(*mut xlib::Display, glx::GLXFBConfig, glx::GLXContext, c_int, *const c_int) -> glx::GLXContext;
            let create: Create = std::mem::transmute(create);
            // the FBConfig matching the window's visual
            let mut n = 0;
            let configs = (self.glx.glXGetFBConfigs)(self.dpy, self.screen, &mut n);
            let mut chosen = None;
            for i in 0..if configs.is_null() { 0 } else { n.max(0) as usize } {
                let c = *configs.add(i);
                let mut id = 0;
                (self.glx.glXGetFBConfigAttrib)(self.dpy, c, glx::GLX_VISUAL_ID, &mut id);
                if id as c_ulong == (*vi).visualid {
                    chosen = Some(c);
                    break;
                }
            }
            if !configs.is_null() {
                (self.xl.XFree)(configs as *mut _);
            }
            let Some(config) = chosen else { return legacy() };
            const MAJOR: c_int = 0x2091;
            const MINOR: c_int = 0x2092;
            const PROFILE: c_int = 0x9126;
            const CORE: c_int = 0x1;
            let attribs = [MAJOR, 3, MINOR, 3, PROFILE, CORE, 0];
            let ctx = create(self.dpy, config, self.share, 1, attribs.as_ptr());
            (self.xl.XSync)(self.dpy, 0);
            if ctx.is_null() { legacy() } else { Some((ctx, true)) }
        }
    }

    fn set_property(&self, win: c_ulong, name: c_ulong, kind: c_ulong, vals: &[c_ulong]) {
        // SAFETY: an open display; `vals` holds the count of 32-bit items said.
        unsafe { (self.xl.XChangeProperty)(self.dpy, win, name, kind, 32, xlib::PropModeReplace, vals.as_ptr() as *const c_uchar, vals.len() as c_int) };
    }
}

impl crate::platform::Display for X {
    /// The monitors, by their RandR names.
    fn screens(&self) -> Vec<Screen> {
        // SAFETY: an open display; the monitors' list, checked for null, is read within its count and freed with XRRFreeMonitors.
        unsafe {
            let mut n = 0;
            let m = (self.rr.XRRGetMonitors)(self.dpy, self.root, 1, &mut n);
            let mut out = vec![];
            if m.is_null() {
                return out;
            }
            for i in 0..n.max(0) as usize {
                let mi = &*m.add(i);
                let name = (self.xl.XGetAtomName)(self.dpy, mi.name);
                if name.is_null() {
                    continue;
                }
                out.push(Screen {
                    name: CStr::from_ptr(name).to_string_lossy().into_owned(),
                    x: mi.x,
                    y: mi.y,
                    w: mi.width,
                    h: mi.height,
                    primary: mi.primary != 0,
                });
                (self.xl.XFree)(name as *mut _);
            }
            (self.rr.XRRFreeMonitors)(m);
            out
        }
    }

    /// The panels' struts, where they reach into the monitor.
    fn work_edges(&self, s: &Screen) -> Edges {
        let Some(wins) = self.clients() else { return Edges::NONE };
        let (sw, sh) = self.screen_size();
        let (mut top, mut right, mut bottom, mut left) = (0i64, 0i64, 0i64, 0i64);
        for w in wins {
            let strut = match self.longs(w, self.atoms.strut_partial, 12) {
                Some(v) if v.len() >= 12 => v.iter().map(|&x| x as i64).collect::<Vec<_>>(),
                _ => match self.longs(w, self.atoms.strut, 4) {
                    Some(v) if v.len() == 4 => {
                        let mut o: Vec<i64> = v.iter().map(|&x| x as i64).collect();
                        for _ in 0..4 {
                            o.extend([0, 1 << 30]);
                        }
                        o
                    }
                    _ => continue,
                },
            };
            if strut[..4].iter().all(|&x| x == 0) {
                continue;
            }
            let (x, y, w, h) = (s.x as i64, s.y as i64, s.w as i64, s.h as i64);
            if strut[4] < y + h && strut[5] >= y {
                left = left.max(strut[0] - x);
            }
            if strut[6] < y + h && strut[7] >= y {
                right = right.max(strut[1] - (sw as i64 - x - w));
            }
            if strut[8] < x + w && strut[9] >= x {
                top = top.max(strut[2] - y);
            }
            if strut[10] < x + w && strut[11] >= x {
                bottom = bottom.max(strut[3] - (sh as i64 - y - h));
            }
        }
        let c = |v: i64, size: i32| v.clamp(0, size as i64) as i32;
        Edges { top: c(top, s.h), right: c(right, s.w), bottom: c(bottom, s.h), left: c(left, s.w) }
    }

    fn window(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<WindowId, String> {
        if w <= 0 || h <= 0 {
            return Err(format!("a window of {w}x{h}"));
        }
        // SAFETY: an open display; the visual is checked for null, read while alive and freed once; the
        // context is made current on the window just made; the property, name and hint data outlive the
        // calls that read them; the hint structures are Xlib's, checked for null and freed with XFree;
        // glXSwapIntervalMESA is called only where the screen offers it, with its specified type.
        unsafe {
            let mut attrs = [glx::GLX_RGBA, glx::GLX_DOUBLEBUFFER, glx::GLX_RED_SIZE, 8, glx::GLX_GREEN_SIZE, 8, glx::GLX_BLUE_SIZE, 8, 0];
            let vi = (self.glx.glXChooseVisual)(self.dpy, self.screen, attrs.as_mut_ptr());
            if vi.is_null() {
                return Err("no GLX visual with 8-bit colour and double buffering".into());
            }
            let Some((ctx, core)) = self.context(vi) else {
                (self.xl.XFree)(vi as *mut _);
                return Err("the driver made no OpenGL context".into());
            };
            let cmap = (self.xl.XCreateColormap)(self.dpy, self.root, (*vi).visual, xlib::AllocNone);
            let mut swa: xlib::XSetWindowAttributes = std::mem::zeroed();
            swa.colormap = cmap;
            swa.border_pixel = 0;
            swa.event_mask = xlib::ExposureMask | xlib::StructureNotifyMask;
            let win = (self.xl.XCreateWindow)(
                self.dpy,
                self.root,
                x,
                y,
                w as c_uint,
                h as c_uint,
                0,
                (*vi).depth,
                xlib::InputOutput as c_uint,
                (*vi).visual,
                xlib::CWColormap | xlib::CWBorderPixel | xlib::CWEventMask,
                &mut swa,
            );
            (self.xl.XFree)(vi as *mut _);
            let a = &self.atoms;
            self.set_property(win, a.wm_type, a.atom, &[a.type_desktop]);
            self.set_property(win, a.wm_state, a.atom, &[a.state_below, a.state_sticky, a.skip_taskbar, a.skip_pager]);
            self.set_property(win, a.wm_desktop, a.cardinal, &[0xFFFF_FFFF]);
            self.set_property(win, a.motif_hints, a.motif_hints, &[2, 0, 0, 0, 0]);
            let name = c"retro-wall";
            (self.xl.XStoreName)(self.dpy, win, name.as_ptr());
            let mut class = xlib::XClassHint { res_name: name.as_ptr() as *mut _, res_class: name.as_ptr() as *mut _ };
            (self.xl.XSetClassHint)(self.dpy, win, &mut class);
            let hints = (self.xl.XAllocWMHints)();
            if !hints.is_null() {
                (*hints).flags = xlib::InputHint;
                (*hints).input = 0;
                (self.xl.XSetWMHints)(self.dpy, win, hints);
                (self.xl.XFree)(hints as *mut _);
            }
            let size = (self.xl.XAllocSizeHints)();
            if !size.is_null() {
                (*size).flags = xlib::USPosition | xlib::PPosition | xlib::PMinSize | xlib::PMaxSize;
                (*size).x = x;
                (*size).y = y;
                (*size).min_width = w;
                (*size).max_width = w;
                (*size).min_height = h;
                (*size).max_height = h;
                (self.xl.XSetWMNormalHints)(self.dpy, win, size);
                (self.xl.XFree)(size as *mut _);
            }
            let region = (self.xf.XFixesCreateRegion)(self.dpy, null_mut(), 0);
            (self.xf.XFixesSetWindowShapeRegion)(self.dpy, win, 2 /* ShapeInput */, 0, 0, region);
            (self.xf.XFixesDestroyRegion)(self.dpy, region);
            (self.xl.XMapWindow)(self.dpy, win);
            (self.xl.XMoveWindow)(self.dpy, win, x, y);
            (self.glx.glXMakeCurrent)(self.dpy, win, ctx);
            if !self.gl_loaded {
                gl::load_with(|s| {
                    let c = CString::new(s).unwrap_or_default();
                    (self.glx.glXGetProcAddressARB)(c.as_ptr() as *const u8).map_or(std::ptr::null(), |f| f as *const _)
                });
                self.gl_loaded = true;
            }
            // under a compositor it paces the frames: no vsync wait; without one, vsync, or frames tear
            if self.glx_extension("GLX_MESA_swap_control")
                && let Some(f) = (self.glx.glXGetProcAddressARB)(c"glXSwapIntervalMESA".as_ptr() as *const u8)
            {
                let f: unsafe extern "C" fn(c_uint) -> c_int = std::mem::transmute(f);
                f(if self.composited() { 0 } else { 1 });
            }
            (self.xl.XSync)(self.dpy, 0);
            if self.share.is_null() {
                self.share = ctx;
            }
            let id = WindowId(win as u64);
            self.wins.insert(id, Win { ctx, core, place: (x, y), moved_back: 0 });
            Ok(id)
        }
    }

    fn core(&self, w: WindowId) -> bool {
        self.wins.get(&w).is_some_and(|w| w.core)
    }

    fn make_current(&mut self, w: WindowId) {
        if let Some(win) = self.wins.get(&w) {
            // SAFETY: an open display, a window of ours and its context.
            unsafe { (self.glx.glXMakeCurrent)(self.dpy, w.0 as c_ulong, win.ctx) };
        }
    }

    fn swap(&self, w: WindowId) {
        // SAFETY: an open display and a window of ours.
        unsafe {
            (self.glx.glXSwapBuffers)(self.dpy, w.0 as c_ulong);
            (self.xl.XFlush)(self.dpy);
        }
    }

    fn raise(&self, w: WindowId) {
        // SAFETY: an open display and a window of ours.
        unsafe { (self.xl.XRaiseWindow)(self.dpy, w.0 as c_ulong) };
    }

    fn close(&mut self, w: WindowId) {
        let Some(win) = self.wins.remove(&w) else { return };
        // SAFETY: an open display; the window and its context are ours, removed from the table so used no more.
        unsafe {
            (self.glx.glXMakeCurrent)(self.dpy, 0, null_mut());
            (self.glx.glXDestroyContext)(self.dpy, win.ctx);
            (self.xl.XDestroyWindow)(self.dpy, w.0 as c_ulong);
            (self.xl.XFlush)(self.dpy);
        }
        if self.share == win.ctx {
            self.share = self.wins.values().next().map_or(null_mut(), |w| w.ctx);
        }
    }

    fn renderer(&self) -> String {
        // SAFETY: a GL context is current; GetString gives null or a NUL-terminated string the driver keeps.
        unsafe {
            let p = gl::GetString(gl::RENDERER);
            if p.is_null() { String::new() } else { CStr::from_ptr(p as *const _).to_string_lossy().into_owned() }
        }
    }

    /// A window moved by something else is put back, at most three times.
    fn events(&mut self) -> Vec<WindowId> {
        let mut exposed = vec![];
        // SAFETY: an open display; XEvent is plain data, filled in by XNextEvent and read as the variant its type says.
        unsafe {
            while (self.xl.XPending)(self.dpy) > 0 {
                let mut e: xlib::XEvent = std::mem::zeroed();
                (self.xl.XNextEvent)(self.dpy, &mut e);
                let id = WindowId(e.any.window as u64);
                let Some(w) = self.wins.get_mut(&id) else { continue };
                match e.get_type() {
                    xlib::Expose => {
                        if !exposed.contains(&id) {
                            exposed.push(id);
                        }
                    }
                    xlib::ConfigureNotify => {
                        let (mut rx, mut ry, mut child) = (0, 0, 0);
                        (self.xl.XTranslateCoordinates)(self.dpy, id.0 as c_ulong, self.root, 0, 0, &mut rx, &mut ry, &mut child);
                        if (rx, ry) != w.place && w.moved_back < 3 {
                            w.moved_back += 1;
                            crate::outln!("window moved to {rx},{ry}: back to {},{}", w.place.0, w.place.1);
                            (self.xl.XMoveWindow)(self.dpy, id.0 as c_ulong, w.place.0, w.place.1);
                        }
                    }
                    _ => {}
                }
            }
        }
        exposed
    }

    fn wait(&self, timeout: Duration) {
        // SAFETY: an open display; ppoll on its connection's descriptor, with a local pollfd and timespec.
        unsafe {
            (self.xl.XFlush)(self.dpy);
            if (self.xl.XPending)(self.dpy) > 0 {
                return;
            }
            let mut fd = libc::pollfd { fd: (self.xl.XConnectionNumber)(self.dpy), events: libc::POLLIN, revents: 0 };
            let ts = libc::timespec { tv_sec: timeout.as_secs() as libc::time_t, tv_nsec: timeout.subsec_nanos() as _ };
            libc::ppoll(&mut fd, 1, &ts, std::ptr::null());
        }
    }

    /// Windows over the desktop: mapped, opaque, not hidden, not the desktop or a popup, tooltip or notification.
    fn covering(&self) -> Option<Vec<Rect>> {
        let wins = self.clients()?;
        let mut rects = vec![];
        for w in wins {
            // SAFETY: an open display; XWindowAttributes is plain data, filled in by the call.
            unsafe {
                let mut a: xlib::XWindowAttributes = std::mem::zeroed();
                if (self.xl.XGetWindowAttributes)(self.dpy, w, &mut a) == 0 || a.map_state != 2 || a.depth == 32 {
                    continue;
                }
                if self.longs(w, self.atoms.wm_type, 64).unwrap_or_default().iter().any(|t| self.atoms.passing.contains(t))
                    || self.longs(w, self.atoms.wm_state, 64).unwrap_or_default().contains(&self.atoms.state_hidden)
                {
                    continue;
                }
                let (mut rx, mut ry, mut child) = (0, 0, 0);
                if (self.xl.XTranslateCoordinates)(self.dpy, w, self.root, 0, 0, &mut rx, &mut ry, &mut child) == 0 {
                    continue;
                }
                let e = self.longs(w, self.atoms.frame_extents, 4).filter(|e| e.len() >= 4).unwrap_or(vec![0; 4]);
                let (l, r, t, b) = (e[0] as i64, e[1] as i64, e[2] as i64, e[3] as i64);
                rects.push(Rect::new(rx as i64 - l, ry as i64 - t, rx as i64 + a.width as i64 + r, ry as i64 + a.height as i64 + b));
            }
        }
        Some(rects)
    }

    /// DPMS off, or the lid closed while only the laptop's own panel is connected.
    fn screen_off(&self) -> bool {
        if self.dpms_off() {
            return true;
        }
        let screens = self.screens();
        !screens.is_empty() && screens.iter().all(|s| builtin_panel(&s.name)) && super::system::lid_closed()
    }
}

/// Whether a RandR output name is a laptop's own panel.
pub fn builtin_panel(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|b| name.starts_with(b))
}

impl Drop for X {
    fn drop(&mut self) {
        let ids: Vec<WindowId> = self.wins.keys().copied().collect();
        for id in ids {
            crate::platform::Display::close(self, id);
        }
        // SAFETY: the display is open until here and used no more.
        unsafe { (self.xl.XCloseDisplay)(self.dpy) };
    }
}
