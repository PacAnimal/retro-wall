//! Win32 and WGL: wallpaper windows behind the desktop icons, each with a GL context sharing the
//! first one's objects, and what the system knows: monitors, work areas, the display's power, which
//! windows cover the desktop.
//!
//! The windows are children of the window Explorer keeps between the wallpaper and the icons: the
//! WorkerW it makes when Progman is sent 0x052C, the one following the window that holds the icons
//! (SHELLDLL_DefView). From Windows 11 24H2 that WorkerW is a child of Progman, below the icons; there
//! the windows are children of Progman, placed just below the icons, and layered, which that desktop
//! needs to compose them. Without either, Progman itself.
//!
//! One window per monitor, each over its monitor's part of the host window (which spans the virtual
//! screen): every monitor paces, pauses and stops when covered on its own, a swap presents one
//! monitor and not the whole desktop with its gaps, and one window never straddles monitors of
//! different DPI or refresh rates.
//!
//! Explorer restarting (TaskbarCreated, or the host window gone) takes the windows with it: they are
//! made again in the new host, each keeping its GL context and so everything uploaded, and drawn again.

use super::wide;
use crate::platform::{Edges, Rect, Screen, WindowId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, EndPaint, EnumDisplayMonitors, GetDC, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, PAINTSTRUCT, ReleaseDC,
};
use windows_sys::Win32::Graphics::OpenGL::{
    ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE, PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR, SetPixelFormat,
    SwapBuffers, wglCreateContext, wglDeleteContext, wglGetProcAddress, wglMakeCurrent, wglShareLists,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Power::{POWERBROADCAST_SETTING, RegisterPowerSettingNotification};
use windows_sys::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};
use windows_sys::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    /// Windows Windows asked to paint since the last look.
    static EXPOSED: RefCell<Vec<HWND>> = const { RefCell::new(vec![]) };
    /// Whether the console display is on (dimmed counts as on).
    static DISPLAY_ON: Cell<bool> = const { Cell::new(true) };
    static LOCKED: Cell<bool> = const { Cell::new(false) };
    /// Explorer has started again.
    static RESHELL: Cell<bool> = const { Cell::new(false) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

const PANE_CLASS: &str = "retro-wall";
const WATCH_CLASS: &str = "retro-wall-watch";

/// A wallpaper window: (x, y, w, h) on the virtual screen.
struct Pane {
    hwnd: HWND,
    dc: HDC,
    ctx: HGLRC,
    core: bool,
    rect: (i32, i32, i32, i32),
}

/// The window the panes are children of.
#[derive(Clone, Copy)]
struct Host {
    hwnd: HWND,
    /// 24H2's layout: the icons' window, which the panes stay just below.
    icons: Option<HWND>,
    /// Its client area's origin on the virtual screen.
    origin: (i32, i32),
}

type CreateContextAttribs = unsafe extern "system" fn(HDC, HGLRC, *const i32) -> HGLRC;
type SwapInterval = unsafe extern "system" fn(i32) -> i32;

pub struct Win {
    instance: HMODULE,
    watch: HWND,
    host: Option<Host>,
    /// When the host was last looked for and not found.
    host_missed: Option<Instant>,
    panes: HashMap<WindowId, Pane>,
    next_id: u64,
    /// A live context whose objects new ones share.
    share: HGLRC,
    /// The pixel format of the first window, which every later one (and every remade one) takes.
    format: i32,
    opengl32: HMODULE,
    create_attribs: Option<CreateContextAttribs>,
    swap_interval: Option<SwapInterval>,
    wgl_looked: bool,
    gl_loaded: bool,
}

unsafe extern "system" fn pane_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by Windows on this thread, for our window, with its own arguments; the paint
    // structure is filled by BeginPaint and handed back to EndPaint. EXPOSED is never borrowed while
    // messages are dispatched, so the borrow here is the only one.
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = zeroed();
                BeginPaint(hwnd, &mut ps);
                EndPaint(hwnd, &ps);
                EXPOSED.with_borrow_mut(|e| e.push(hwnd));
                0
            }
            WM_ERASEBKGND => 1,
            WM_NCHITTEST => HTTRANSPARENT as LRESULT,
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The hidden top-level window: it gets the broadcasts child windows do not.
unsafe extern "system" fn watch_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.get() {
        RESHELL.set(true);
        return 0;
    }
    match msg {
        WM_POWERBROADCAST if wparam == PBT_POWERSETTINGCHANGE as usize => {
            // SAFETY: for this event lparam points at a POWERBROADCAST_SETTING followed by DataLength
            // bytes of data; the data is read only as the console display state, a DWORD, once its
            // setting and length say it is one.
            let state = unsafe {
                let s = lparam as *const POWERBROADCAST_SETTING;
                let (g, want) = ((*s).PowerSetting, GUID_CONSOLE_DISPLAY_STATE);
                let ours = (g.data1, g.data2, g.data3, g.data4) == (want.data1, want.data2, want.data3, want.data4);
                (ours && (*s).DataLength >= 4).then(|| ((&raw const (*s).Data) as *const u32).read_unaligned())
            };
            if let Some(state) = state {
                DISPLAY_ON.set(state != 0);
            }
            1
        }
        WM_WTSSESSION_CHANGE => {
            match wparam as u32 {
                WTS_SESSION_LOCK => LOCKED.set(true),
                WTS_SESSION_UNLOCK => LOCKED.set(false),
                _ => {}
            }
            0
        }
        // SAFETY: Windows' own arguments, passed on.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// A window's class name.
fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: a buffer of the length given.
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn find(parent: HWND, after: HWND, class: &str) -> HWND {
    let c = wide(class);
    // SAFETY: a NUL-terminated class name; null handles mean the desktop and the first child.
    unsafe { FindWindowExW(parent, after, c.as_ptr(), null()) }
}

/// The window to draw in, behind the desktop icons; None while Explorer has no desktop.
fn find_host() -> Option<Host> {
    let progman = find(null_mut(), null_mut(), "Progman");
    if progman.is_null() {
        return None;
    }
    // SAFETY: a message Progman takes; at most a second's wait for a hung Explorer.
    unsafe { SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, null_mut()) };
    let inner = find(progman, null_mut(), "WorkerW");
    let icons = find(progman, null_mut(), "SHELLDLL_DefView");
    let (hwnd, icons) = if !inner.is_null() && !icons.is_null() {
        (progman, Some(icons))
    } else {
        unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> i32 {
            if !find(hwnd, null_mut(), "SHELLDLL_DefView").is_null() {
                let next = find(null_mut(), hwnd, "WorkerW");
                if !next.is_null() {
                    // SAFETY: found is the address of `worker` below, which outlives EnumWindows, and
                    // nothing else touches it during the enumeration.
                    unsafe { *(found as *mut HWND) = next };
                    return 0;
                }
            }
            1
        }
        let mut worker: HWND = null_mut();
        // SAFETY: the callback writes only through the pointer it is given, during the call.
        unsafe { EnumWindows(Some(each), &raw mut worker as LPARAM) };
        (if worker.is_null() { progman } else { worker }, None)
    };
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: a valid window and point.
    unsafe { ClientToScreen(hwnd, &mut p) };
    Some(Host { hwnd, icons, origin: (p.x, p.y) })
}

/// Every monitor: its handle and its MONITORINFOEXW.
fn monitors() -> Vec<(HMONITOR, MONITORINFOEXW)> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> i32 {
        // SAFETY: out is the address of the Vec below, which outlives the enumeration and is not
        // otherwise touched during it; the info's size is set before GetMonitorInfoW fills it.
        unsafe {
            let mut info: MONITORINFOEXW = zeroed();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(m, &raw mut info as *mut MONITORINFO) != 0 {
                (*(out as *mut Vec<(HMONITOR, MONITORINFOEXW)>)).push((m, info));
            }
        }
        1
    }
    let mut out: Vec<(HMONITOR, MONITORINFOEXW)> = vec![];
    // SAFETY: the callback writes only to `out`, during the call.
    unsafe { EnumDisplayMonitors(null_mut(), null(), Some(each), &raw mut out as LPARAM) };
    out
}

impl Win {
    pub fn open() -> Result<Win, String> {
        // SAFETY: plain Win32 calls with NUL-terminated strings that outlive them; the classes'
        // procedures are the functions above.
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(null());
            let (pane_name, watch_name) = (wide(PANE_CLASS), wide(WATCH_CLASS));
            for (name, proc, style) in [(&pane_name, pane_proc as WndProc, CS_OWNDC), (&watch_name, watch_proc as WndProc, 0)] {
                let mut wc: WNDCLASSEXW = zeroed();
                wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
                wc.style = style;
                wc.lpfnWndProc = Some(proc);
                wc.hInstance = instance;
                wc.lpszClassName = name.as_ptr();
                RegisterClassExW(&wc);
            }
            let title = wide("retro-wall");
            let watch = CreateWindowExW(WS_EX_TOOLWINDOW, watch_name.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), instance, null());
            if watch.is_null() {
                return Err(format!("a window: {}", std::io::Error::last_os_error()));
            }
            TASKBAR_CREATED.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
            RegisterPowerSettingNotification(watch, &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
            WTSRegisterSessionNotification(watch, NOTIFY_FOR_THIS_SESSION);
            let opengl32 = LoadLibraryW(wide("opengl32.dll").as_ptr());
            Ok(Win {
                instance,
                watch,
                host: None,
                host_missed: None,
                panes: HashMap::new(),
                next_id: 1,
                share: null_mut(),
                format: 0,
                opengl32,
                create_attribs: None,
                swap_interval: None,
                wgl_looked: false,
                gl_loaded: false,
            })
        }
    }

    /// The host window, looked for again when it is gone (at most once a second while not found).
    fn host(&mut self) -> Option<Host> {
        // SAFETY: IsWindow takes any value.
        if let Some(h) = self.host.filter(|h| unsafe { IsWindow(h.hwnd) } != 0) {
            return Some(h);
        }
        if self.host_missed.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return None;
        }
        self.host = find_host();
        self.host_missed = self.host.is_none().then(Instant::now);
        if let Some(h) = self.host {
            crate::outln!("desktop window: {} {:#x}{}", class_of(h.hwnd), h.hwnd as usize, if h.icons.is_some() { ", below the icons" } else { "" });
        }
        self.host
    }

    /// A child window of the host over (x, y, w, h), its DC with the pixel format set. Without a host
    /// (no Explorer, or not yet), a top-level window at the bottom of the stack, made a child again
    /// when Explorer comes.
    fn child(&mut self, rect: (i32, i32, i32, i32)) -> Result<(HWND, HDC), String> {
        let host = self.host();
        let (x, y, w, h) = rect;
        let layered = if host.is_some_and(|h| h.icons.is_some()) { WS_EX_LAYERED } else { 0 };
        let (style, ex, parent, origin) = match host {
            Some(h) => (WS_CHILD, WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | layered, h.hwnd, h.origin),
            None => (WS_POPUP, WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW, null_mut(), (0, 0)),
        };
        let (class, title) = (wide(PANE_CLASS), wide("retro-wall"));
        // SAFETY: Win32 calls on windows and DCs made here, with NUL-terminated strings and a filled-in
        // PIXELFORMATDESCRIPTOR.
        unsafe {
            let hwnd = CreateWindowExW(
                ex,
                class.as_ptr(),
                title.as_ptr(),
                style | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                x - origin.0,
                y - origin.1,
                w,
                h,
                parent,
                null_mut(),
                self.instance,
                null(),
            );
            if hwnd.is_null() {
                return Err(format!("a window: {}", std::io::Error::last_os_error()));
            }
            if layered != 0 {
                SetLayeredWindowAttributes(hwnd, 0 as COLORREF, 255, LWA_ALPHA);
            }
            self.restack(hwnd, host);
            let dc = GetDC(hwnd);
            let mut pfd: PIXELFORMATDESCRIPTOR = zeroed();
            pfd.nSize = size_of::<PIXELFORMATDESCRIPTOR>() as u16;
            pfd.nVersion = 1;
            pfd.dwFlags = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
            pfd.iPixelType = PFD_TYPE_RGBA;
            pfd.cColorBits = 24;
            pfd.iLayerType = PFD_MAIN_PLANE as u8;
            if self.format == 0 {
                self.format = ChoosePixelFormat(dc, &pfd);
            }
            if self.format == 0 || SetPixelFormat(dc, self.format, &pfd) == 0 {
                let e = std::io::Error::last_os_error();
                ReleaseDC(hwnd, dc);
                DestroyWindow(hwnd);
                return Err(format!("no OpenGL pixel format with 8-bit colour and double buffering: {e}"));
            }
            Ok((hwnd, dc))
        }
    }

    /// To the top of the host's children; in 24H2's layout, just below the icons; without a host, to
    /// the bottom of every window.
    fn restack(&self, hwnd: HWND, host: Option<Host>) {
        let after = match host {
            Some(h) => h.icons.unwrap_or(HWND_TOP),
            None => HWND_BOTTOM,
        };
        // SAFETY: valid windows; no move, size or activation.
        unsafe { SetWindowPos(hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    /// The WGL extension functions.
    ///
    /// # Safety
    /// A context is current on this thread.
    unsafe fn wgl_functions(&mut self) {
        if self.wgl_looked {
            return;
        }
        self.wgl_looked = true;
        // SAFETY: a context is current (the caller's promise); `wgl_proc` gives only real addresses;
        // the functions have these types by the extensions' specifications.
        unsafe {
            self.create_attribs = wgl_proc(c"wglCreateContextAttribsARB").map(|f| std::mem::transmute::<_, CreateContextAttribs>(f));
            self.swap_interval = wgl_proc(c"wglSwapIntervalEXT").map(|f| std::mem::transmute::<_, SwapInterval>(f));
        }
    }

    /// A GL context for `dc`, sharing objects with `self.share`: 3.3 core where the driver offers it,
    /// else legacy (2.1+). RETRO_GL=2.1 forces legacy, for tests. No context is current after.
    ///
    /// # Safety
    /// `dc` is a window's DC with its pixel format set.
    unsafe fn context(&mut self, dc: HDC) -> Option<(HGLRC, bool)> {
        // SAFETY: dc is as the caller promises; the legacy context is current while the extension
        // functions are looked up and called; the attribute list ends in 0; contexts made here are
        // deleted when not kept.
        unsafe {
            let legacy = wglCreateContext(dc);
            if legacy.is_null() {
                return None;
            }
            wglMakeCurrent(dc, legacy);
            self.wgl_functions();
            if !std::env::var("RETRO_GL").is_ok_and(|v| v == "2.1")
                && let Some(create) = self.create_attribs
            {
                const MAJOR: i32 = 0x2091;
                const MINOR: i32 = 0x2092;
                const PROFILE: i32 = 0x9126;
                const CORE: i32 = 0x1;
                let attribs = [MAJOR, 3, MINOR, 3, PROFILE, CORE, 0];
                let ctx = create(dc, self.share, attribs.as_ptr());
                if !ctx.is_null() {
                    wglMakeCurrent(null_mut(), null_mut());
                    wglDeleteContext(legacy);
                    return Some((ctx, true));
                }
            }
            wglMakeCurrent(null_mut(), null_mut());
            if !self.share.is_null() && wglShareLists(self.share, legacy) == 0 {
                wglDeleteContext(legacy);
                return None;
            }
            Some((legacy, false))
        }
    }

    fn load_gl(&mut self) {
        if self.gl_loaded {
            return;
        }
        self.gl_loaded = true;
        let opengl32 = self.opengl32;
        gl::load_with(|s| {
            let c = CString::new(s).unwrap_or_default();
            // SAFETY: a context is current (load_gl runs after one is made current); the name is
            // NUL-terminated. opengl32 has the OpenGL 1.1 functions wglGetProcAddress does not answer for.
            unsafe {
                match wgl_proc(&c) {
                    Some(f) => f as *const c_void,
                    None => GetProcAddress(opengl32, c.as_ptr().cast()).map_or(null(), |f| f as *const c_void),
                }
            }
        });
    }

    /// The panes made again in a new host (Explorer started or restarted), keeping their contexts.
    fn rehost(&mut self) -> Vec<WindowId> {
        // SAFETY: no context current while windows change.
        unsafe { wglMakeCurrent(null_mut(), null_mut()) };
        let ids: Vec<WindowId> = self.panes.keys().copied().collect();
        let mut remade = vec![];
        for id in ids {
            let rect = self.panes[&id].rect;
            let (old_hwnd, old_dc) = (self.panes[&id].hwnd, self.panes[&id].dc);
            // SAFETY: the old window, if Explorer has not taken it already.
            unsafe {
                if IsWindow(old_hwnd) != 0 {
                    ReleaseDC(old_hwnd, old_dc);
                    DestroyWindow(old_hwnd);
                }
            }
            match self.child(rect) {
                Ok((hwnd, dc)) => {
                    let p = self.panes.get_mut(&id).expect("listed above");
                    p.hwnd = hwnd;
                    p.dc = dc;
                    remade.push(id);
                }
                Err(e) => crate::errln!("a window made again: {e}"),
            }
        }
        crate::outln!("{} window(s) made again in the desktop window", remade.len());
        remade
    }

    /// The panes moved back over their monitors when the host's origin moved (a monitor added left
    /// of or above the others).
    fn follow_host(&mut self) {
        let Some(host) = self.host else { return };
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: a valid window and point.
        if unsafe { ClientToScreen(host.hwnd, &mut p) } == 0 || (p.x, p.y) == host.origin {
            return;
        }
        self.host = Some(Host { origin: (p.x, p.y), ..host });
        for pane in self.panes.values() {
            let (x, y, w, h) = pane.rect;
            // SAFETY: our own window; no z-order change or activation.
            unsafe { SetWindowPos(pane.hwnd, null_mut(), x - p.x, y - p.y, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
        }
    }
}

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// wglGetProcAddress's answer, None where it has none: some drivers answer 1, 2, 3 or -1 for none.
///
/// # Safety
/// A context is current on this thread.
unsafe fn wgl_proc(name: &CStr) -> Option<unsafe extern "system" fn() -> isize> {
    // SAFETY: a context is current (the caller's promise); the name is NUL-terminated.
    unsafe { wglGetProcAddress(name.as_ptr().cast()) }.filter(|&f| !matches!(f as usize, 1 | 2 | 3 | usize::MAX))
}

impl crate::platform::Display for Win {
    /// The monitors, named as their devices (DISPLAY1), in physical pixels.
    fn screens(&self) -> Vec<Screen> {
        monitors()
            .into_iter()
            .map(|(_, m)| {
                let r = m.monitorInfo.rcMonitor;
                let name = super::unwide(&m.szDevice);
                Screen {
                    name: name.trim_start_matches(r"\\.\").to_string(),
                    x: r.left,
                    y: r.top,
                    w: r.right - r.left,
                    h: r.bottom - r.top,
                    primary: m.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                }
            })
            .collect()
    }

    /// Where the monitor's work area stops short of it: the taskbar, and docked toolbars.
    fn work_edges(&self, s: &Screen) -> Edges {
        for (_, m) in monitors() {
            let (r, w) = (m.monitorInfo.rcMonitor, m.monitorInfo.rcWork);
            if (r.left, r.top, r.right - r.left, r.bottom - r.top) == (s.x, s.y, s.w, s.h) {
                let c = |v: i32, size: i32| v.clamp(0, size);
                return Edges {
                    top: c(w.top - r.top, s.h),
                    right: c(r.right - w.right, s.w),
                    bottom: c(r.bottom - w.bottom, s.h),
                    left: c(w.left - r.left, s.w),
                };
            }
        }
        Edges::NONE
    }

    fn window(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<WindowId, String> {
        if w <= 0 || h <= 0 {
            return Err(format!("a window of {w}x{h}"));
        }
        let (hwnd, dc) = self.child((x, y, w, h))?;
        // SAFETY: dc has its pixel format; the new context is current for the GL and swap interval
        // calls; on failure the context, window and DC made above are released.
        let (ctx, core) = unsafe {
            let Some((ctx, core)) = self.context(dc) else {
                ReleaseDC(hwnd, dc);
                DestroyWindow(hwnd);
                return Err("the driver made no OpenGL context".into());
            };
            wglMakeCurrent(dc, ctx);
            self.load_gl();
            if !core {
                let version = gl::GetString(gl::VERSION);
                let v = if version.is_null() { String::new() } else { CStr::from_ptr(version.cast()).to_string_lossy().into_owned() };
                let mut nums = v.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u32>().ok());
                if (nums.next().unwrap_or(0), nums.next().unwrap_or(0)) < (2, 1) {
                    let renderer = crate::platform::Display::renderer(self);
                    wglMakeCurrent(null_mut(), null_mut());
                    wglDeleteContext(ctx);
                    ReleaseDC(hwnd, dc);
                    DestroyWindow(hwnd);
                    return Err(format!("OpenGL {v} ({renderer}): 2.1 or newer needed"));
                }
            }
            // DWM composes every window: a swap need not wait for the display
            if let Some(f) = self.swap_interval {
                f(0);
            }
            if self.share.is_null() {
                self.share = ctx;
            }
            (ctx, core)
        };
        let id = WindowId(self.next_id);
        self.next_id += 1;
        self.panes.insert(id, Pane { hwnd, dc, ctx, core, rect: (x, y, w, h) });
        Ok(id)
    }

    fn core(&self, w: WindowId) -> bool {
        self.panes.get(&w).is_some_and(|p| p.core)
    }

    fn make_current(&mut self, w: WindowId) {
        if let Some(p) = self.panes.get(&w) {
            // SAFETY: the pane's own DC and context.
            unsafe { wglMakeCurrent(p.dc, p.ctx) };
        }
    }

    fn swap(&self, w: WindowId) {
        if let Some(p) = self.panes.get(&w) {
            // SAFETY: the pane's own DC.
            unsafe { SwapBuffers(p.dc) };
        }
    }

    fn raise(&self, w: WindowId) {
        if let Some(p) = self.panes.get(&w) {
            self.restack(p.hwnd, self.host);
        }
    }

    fn close(&mut self, w: WindowId) {
        let Some(p) = self.panes.remove(&w) else { return };
        // SAFETY: the pane's own context, DC and window, not used again.
        unsafe {
            wglMakeCurrent(null_mut(), null_mut());
            wglDeleteContext(p.ctx);
            if IsWindow(p.hwnd) != 0 {
                ReleaseDC(p.hwnd, p.dc);
                DestroyWindow(p.hwnd);
            }
        }
        if self.share == p.ctx {
            self.share = self.panes.values().next().map_or(null_mut(), |p| p.ctx);
        }
    }

    fn renderer(&self) -> String {
        // SAFETY: a context is current; GL returns a static string or null.
        unsafe {
            let p = gl::GetString(gl::RENDERER);
            if p.is_null() { String::new() } else { CStr::from_ptr(p.cast()).to_string_lossy().into_owned() }
        }
    }

    /// Windows' messages handled; the panes that need drawing again, all of them after Explorer
    /// restarted.
    fn events(&mut self) -> Vec<WindowId> {
        // SAFETY: the message loop on this thread, with a valid MSG.
        unsafe {
            let mut msg: MSG = zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let gone = self.host.is_some_and(|h| {
            // SAFETY: IsWindow takes any value.
            unsafe { IsWindow(h.hwnd) == 0 || self.panes.values().any(|p| IsWindow(p.hwnd) == 0) }
        });
        if RESHELL.replace(false) || gone {
            self.host = None;
            self.host_missed = None;
        }
        if self.host.is_none() && !self.panes.is_empty() && self.host().is_some() {
            return self.rehost();
        }
        self.follow_host();
        let exposed = EXPOSED.with_borrow_mut(std::mem::take);
        let mut out: Vec<WindowId> = vec![];
        for hwnd in exposed {
            if let Some((id, _)) = self.panes.iter().find(|(_, p)| p.hwnd == hwnd)
                && !out.contains(id)
            {
                out.push(*id);
            }
        }
        out
    }

    fn wait(&self, timeout: Duration) {
        // SAFETY: no handles; wakes on any message.
        unsafe { MsgWaitForMultipleObjectsEx(0, null(), timeout.as_millis().min(u32::MAX as u128 - 1) as u32, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
    }

    /// Top-level windows over the desktop: shown, not minimised, not cloaked (on another virtual
    /// desktop, or a Start menu kept ready), not the desktop's own, not one clicks pass through nor a
    /// layered one short of opaque. Their bounds as drawn, without the invisible resize borders.
    fn covering(&self) -> Option<Vec<Rect>> {
        unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> i32 {
            // SAFETY: out is the address of the Vec below, which outlives the enumeration and is not
            // otherwise touched during it; the rest are queries on hwnd with out-pointers of the sizes
            // given.
            unsafe {
                if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                    return 1;
                }
                let mut cloaked = 0u32;
                if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&raw mut cloaked).cast(), 4) == 0 && cloaked != 0 {
                    return 1;
                }
                if matches!(class_of(hwnd).as_str(), "Progman" | "WorkerW") {
                    return 1;
                }
                let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                if ex & WS_EX_TRANSPARENT != 0 {
                    return 1;
                }
                if ex & WS_EX_LAYERED != 0 {
                    let (mut key, mut alpha, mut flags) = (0u32, 0u8, 0u32);
                    if GetLayeredWindowAttributes(hwnd, &mut key, &mut alpha, &mut flags) == 0 || flags & LWA_ALPHA == 0 || alpha < 255 {
                        return 1;
                    }
                }
                let mut r: RECT = zeroed();
                if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS as u32, (&raw mut r).cast(), size_of::<RECT>() as u32) != 0 {
                    GetWindowRect(hwnd, &mut r);
                }
                if r.right > r.left && r.bottom > r.top {
                    (*(out as *mut Vec<Rect>)).push(Rect::new(r.left as i64, r.top as i64, r.right as i64, r.bottom as i64));
                }
                1
            }
        }
        let mut rects: Vec<Rect> = vec![];
        // SAFETY: the callback writes only to `rects`, during the call.
        let ok = unsafe { EnumWindows(Some(each), &raw mut rects as LPARAM) };
        (ok != 0).then_some(rects)
    }

    /// The console display powered down, or the session locked.
    fn screen_off(&self) -> bool {
        !DISPLAY_ON.get() || LOCKED.get()
    }
}

impl Drop for Win {
    fn drop(&mut self) {
        let ids: Vec<WindowId> = self.panes.keys().copied().collect();
        for id in ids {
            crate::platform::Display::close(self, id);
        }
        // SAFETY: our own hidden window.
        unsafe { DestroyWindow(self.watch) };
    }
}
