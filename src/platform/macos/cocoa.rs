//! Cocoa and NSOpenGL: wallpaper windows (borderless, just over the desktop picture and under the
//! desktop icons, on every Space, letting clicks through), each with a GL context sharing the first
//! one's objects, drawn at the display's backing pixels; and what the window server knows: the
//! screens, the menu bar and Dock, the windows over the desktop, and whether the displays sleep.
//!
//! Positions are in pixels from the top left of the main screen. A screen's origin is its point
//! origin times the largest backing scale of all screens (so no two screens' pixels overlap) and its
//! size is its own backing pixels.

// OpenGL is deprecated on macOS, and it is what the renderer draws with.
#![allow(deprecated)]

use crate::platform::{Edges, Rect, Screen, WindowId};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask, NSOpenGLContext, NSOpenGLContextParameter, NSOpenGLPixelFormat,
    NSOpenGLPixelFormatAttribute, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey, CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{NSArray, NSDate, NSDefaultRunLoopMode, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::ptr::NonNull;
use std::time::Duration;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

/// A screen as Cocoa has it, and as we give it.
pub struct Place {
    pub screen: Screen,
    pub native: Retained<NSScreen>,
    /// Its frame in points, from the main screen's top left: (x, y, w, h).
    pub points: (f64, f64, f64, f64),
    pub scale: f64,
}

/// The screens: the first (the one with the menu bar) is the primary.
pub fn places(mtm: MainThreadMarker) -> Vec<Place> {
    let screens = NSScreen::screens(mtm);
    let Some(main) = screens.firstObject() else { return vec![] };
    let top = main.frame().origin.y + main.frame().size.height;
    let most = screens.iter().map(|s| s.backingScaleFactor()).fold(1.0, f64::max);
    screens
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (f, k) = (s.frame(), s.backingScaleFactor());
            let points = (f.origin.x, top - f.origin.y - f.size.height, f.size.width, f.size.height);
            let screen = Screen {
                name: s.localizedName().to_string(),
                x: (points.0 * most).round() as i32,
                y: (points.1 * most).round() as i32,
                w: (points.2 * k).round() as i32,
                h: (points.3 * k).round() as i32,
                primary: i == 0,
            };
            Place { screen, native: s, points, scale: k }
        })
        .collect()
}

struct Win {
    window: Retained<NSWindow>,
    ctx: Retained<NSOpenGLContext>,
    core: bool,
    visible: bool,
}

pub struct Quartz {
    mtm: MainThreadMarker,
    app: Retained<NSApplication>,
    wins: HashMap<WindowId, Win>,
    /// The pixel format of every context (they share objects, so one), and whether it is 4.1 core.
    format: Option<(Retained<NSOpenGLPixelFormat>, bool)>,
    /// A live context whose objects new ones share.
    share: Option<Retained<NSOpenGLContext>>,
    gl_loaded: bool,
    /// Windows made since the last events(): they need drawing.
    fresh: Vec<WindowId>,
}

impl Quartz {
    pub fn open() -> Result<Quartz, String> {
        let mtm = MainThreadMarker::new().ok_or("the display is used from the main thread only")?;
        let app = NSApplication::sharedApplication(mtm);
        // no Dock icon, no menu bar, not in Cmd-Tab
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        Ok(Quartz { mtm, app, wins: HashMap::new(), format: None, share: None, gl_loaded: false, fresh: vec![] })
    }

    /// The pixel format: 4.1 core where it is made, else legacy (2.1); a GPU's, else any (a virtual
    /// machine's software renderer, which the loop then turns down). RETRO_GL=2.1 forces legacy.
    fn pixel_format(&mut self) -> Result<(Retained<NSOpenGLPixelFormat>, bool), String> {
        if let Some(f) = &self.format {
            return Ok(f.clone());
        }
        use objc2_app_kit::{
            NSOpenGLPFAAccelerated, NSOpenGLPFAAlphaSize, NSOpenGLPFAColorSize, NSOpenGLPFADoubleBuffer, NSOpenGLPFAOpenGLProfile,
            NSOpenGLProfileVersion4_1Core, NSOpenGLProfileVersionLegacy,
        };
        let legacy_only = std::env::var("RETRO_GL").is_ok_and(|v| v == "2.1");
        let profiles: &[(u32, bool)] = if legacy_only {
            &[(NSOpenGLProfileVersionLegacy, false)]
        } else {
            &[(NSOpenGLProfileVersion4_1Core, true), (NSOpenGLProfileVersionLegacy, false)]
        };
        for accelerated in [true, false] {
            for &(profile, core) in profiles {
                let mut a: Vec<NSOpenGLPixelFormatAttribute> =
                    vec![NSOpenGLPFAOpenGLProfile, profile, NSOpenGLPFADoubleBuffer, NSOpenGLPFAColorSize, 24, NSOpenGLPFAAlphaSize, 8];
                if accelerated {
                    a.push(NSOpenGLPFAAccelerated);
                }
                a.push(0);
                let ptr = NonNull::new(a.as_mut_ptr()).ok_or("no attributes")?;
                // SAFETY: `a` is a zero-terminated attribute list that outlives the call.
                if let Some(f) = unsafe { NSOpenGLPixelFormat::initWithAttributes(self.mtm.alloc(), ptr) } {
                    self.format = Some((f.clone(), core));
                    return Ok((f, core));
                }
            }
        }
        Err("no OpenGL pixel format".into())
    }

    fn load_gl(&mut self) {
        if self.gl_loaded {
            return;
        }
        // SAFETY: a framework path, NUL-terminated; the handle is kept for the life of the process.
        let lib = unsafe { libc::dlopen(c"/System/Library/Frameworks/OpenGL.framework/OpenGL".as_ptr(), libc::RTLD_LAZY) };
        gl::load_with(|name| {
            let c = CString::new(name).unwrap_or_default();
            // SAFETY: dlsym on a valid handle with a NUL-terminated name.
            if lib.is_null() { std::ptr::null() } else { unsafe { libc::dlsym(lib, c.as_ptr()) as *const c_void } }
        });
        self.gl_loaded = true;
    }

    /// The screen holding pixel (x, y).
    fn place_at(&self, x: i32, y: i32) -> Option<Place> {
        places(self.mtm).into_iter().find(|p| {
            let s = &p.screen;
            (s.x..s.x + s.w).contains(&x) && (s.y..s.y + s.h).contains(&y)
        })
    }
}

/// A dictionary's value for `key` as a dictionary.
fn dict(d: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<Retained<NSDictionary<NSString, AnyObject>>> {
    d.objectForKey(&NSString::from_str(key)).and_then(super::system::dictionary)
}

impl crate::platform::Display for Quartz {
    fn screens(&self) -> Vec<Screen> {
        places(self.mtm).into_iter().map(|p| p.screen).collect()
    }

    /// The menu bar and the Dock: the screen's frame less its visible frame.
    fn work_edges(&self, s: &Screen) -> Edges {
        let Some(p) = places(self.mtm).into_iter().find(|p| p.screen == *s) else { return Edges::NONE };
        let (f, v, k) = (p.native.frame(), p.native.visibleFrame(), p.scale);
        let px = |points: f64, size: i32| ((points * k).round() as i32).clamp(0, size);
        Edges {
            top: px(f.origin.y + f.size.height - (v.origin.y + v.size.height), s.h),
            right: px(f.origin.x + f.size.width - (v.origin.x + v.size.width), s.w),
            bottom: px(v.origin.y - f.origin.y, s.h),
            left: px(v.origin.x - f.origin.x, s.w),
        }
    }

    fn window(&mut self, x: i32, y: i32, w: i32, h: i32) -> Result<WindowId, String> {
        if w <= 0 || h <= 0 {
            return Err(format!("a window of {w}x{h}"));
        }
        let p = self.place_at(x, y).ok_or(format!("no screen at {x},{y}"))?;
        let k = p.scale;
        let f = p.native.frame();
        let size = NSSize::new(w as f64 / k, h as f64 / k);
        let top = (y - p.screen.y) as f64 / k;
        let origin = NSPoint::new(f.origin.x + (x - p.screen.x) as f64 / k, f.origin.y + f.size.height - top - size.height);
        let frame = NSRect::new(origin, size);
        let (format, core) = self.pixel_format()?;
        let mtm = self.mtm;
        // SAFETY: a fresh allocation; the window is kept (not released when closed) for as long as we hold it.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false)
        };
        // SAFETY: we hold a Retained reference, so it must not release itself on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setLevel(CGWindowLevelForKey(CGWindowLevelKey::DesktopWindowLevelKey) as isize + 1);
        window.setIgnoresMouseEvents(true);
        window.setOpaque(true);
        window.setHasShadow(false);
        window.setBackgroundColor(Some(&NSColor::blackColor()));
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle,
        );
        let view = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::new(0.0, 0.0), size));
        view.setWantsBestResolutionOpenGLSurface(true);
        window.setContentView(Some(&view));
        window.orderFrontRegardless();
        window.setFrame_display(frame, false);
        let ctx = NSOpenGLContext::initWithFormat_shareContext(NSOpenGLContext::alloc(), &format, self.share.as_deref())
            .ok_or("the driver made no OpenGL context")?;
        ctx.setView(Some(&view), mtm);
        let interval: i32 = 0; // the window server composes and paces the frames
        // SAFETY: setValues:forParameter: reads one GLint for the swap interval.
        let _: () = unsafe { msg_send![&*ctx, setValues: NonNull::from(&interval), forParameter: NSOpenGLContextParameter::SwapInterval] };
        ctx.makeCurrentContext();
        self.load_gl();
        if self.share.is_none() {
            self.share = Some(ctx.clone());
        }
        let id = WindowId(window.windowNumber() as u64);
        self.wins.insert(id, Win { window, ctx, core, visible: true });
        self.fresh.push(id);
        Ok(id)
    }

    fn core(&self, w: WindowId) -> bool {
        self.wins.get(&w).is_some_and(|w| w.core)
    }

    fn make_current(&mut self, w: WindowId) {
        if let Some(win) = self.wins.get(&w) {
            win.ctx.makeCurrentContext();
        }
    }

    fn swap(&self, w: WindowId) {
        if let Some(win) = self.wins.get(&w) {
            win.ctx.flushBuffer();
        }
    }

    fn raise(&self, w: WindowId) {
        if let Some(win) = self.wins.get(&w) {
            win.window.orderFrontRegardless();
        }
    }

    fn close(&mut self, w: WindowId) {
        let Some(win) = self.wins.remove(&w) else { return };
        NSOpenGLContext::clearCurrentContext();
        win.ctx.clearDrawable();
        win.window.orderOut(None);
        win.window.close();
        if self.share.as_ref().is_some_and(|s| std::ptr::eq(&**s, &*win.ctx)) {
            self.share = self.wins.values().next().map(|w| w.ctx.clone());
        }
        self.fresh.retain(|f| *f != w);
    }

    fn renderer(&self) -> String {
        // SAFETY: a context is current; GetString returns a static NUL-terminated string or null.
        unsafe {
            let p = gl::GetString(gl::RENDERER);
            if p.is_null() { String::new() } else { CStr::from_ptr(p as *const _).to_string_lossy().into_owned() }
        }
    }

    /// Cocoa's events handed on; the windows made since, and those that came back into view.
    fn events(&mut self) -> Vec<WindowId> {
        let past = NSDate::distantPast();
        // SAFETY: an immutable constant string.
        let mode = unsafe { NSDefaultRunLoopMode };
        while let Some(e) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&past), mode, true) {
            self.app.sendEvent(&e);
        }
        let mut out = std::mem::take(&mut self.fresh);
        for (id, w) in self.wins.iter_mut() {
            let visible = w.window.occlusionState().contains(NSWindowOcclusionState::Visible);
            if visible && !w.visible && !out.contains(id) {
                out.push(*id);
            }
            w.visible = visible;
        }
        out
    }

    fn wait(&self, timeout: Duration) {
        if timeout.is_zero() {
            return;
        }
        let until = NSDate::dateWithTimeIntervalSinceNow(timeout.as_secs_f64());
        // SAFETY: an immutable constant string.
        let mode = unsafe { NSDefaultRunLoopMode };
        if let Some(e) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&until), mode, true) {
            self.app.sendEvent(&e);
        }
    }

    /// Other applications' normal windows on screen, each cut to the screens it lies on.
    fn covering(&self) -> Option<Vec<Rect>> {
        let list = CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements, 0)?;
        // SAFETY: CGWindowListCopyWindowInfo gives a CFArray of CFDictionary (toll-free bridged to
        // NSArray and NSDictionary), owned by `list`, which outlives the borrow.
        let list: &NSArray<NSDictionary<NSString, AnyObject>> = unsafe { &*(&*list as *const _ as *const NSArray<NSDictionary<NSString, AnyObject>>) };
        let places = places(self.mtm);
        let me = std::process::id() as f64;
        let mut rects = vec![];
        for d in list.iter() {
            let n = |k: &str| super::system::number(&d, k);
            if n("kCGWindowLayer") != Some(0.0) || n("kCGWindowOwnerPID") == Some(me) || n("kCGWindowAlpha").is_some_and(|a| a <= 0.0) {
                continue;
            }
            let Some(b) = dict(&d, "kCGWindowBounds") else { continue };
            let bn = |k: &str| super::system::number(&b, k).unwrap_or(0.0);
            let (x0, y0) = (bn("X"), bn("Y"));
            let (x1, y1) = (x0 + bn("Width"), y0 + bn("Height"));
            for p in &places {
                let (px, py, pw, ph) = p.points;
                let (ix0, iy0, ix1, iy1) = (x0.max(px), y0.max(py), x1.min(px + pw), y1.min(py + ph));
                if ix1 <= ix0 || iy1 <= iy0 {
                    continue;
                }
                let at = |v: f64, from: f64, origin: i32| origin as i64 + ((v - from) * p.scale).round() as i64;
                rects.push(Rect::new(at(ix0, px, p.screen.x), at(iy0, py, p.screen.y), at(ix1, px, p.screen.x), at(iy1, py, p.screen.y)));
            }
        }
        Some(rects)
    }

    /// Every active display asleep.
    fn screen_off(&self) -> bool {
        let mut ids = [0u32; 32];
        let mut n = 0u32;
        // SAFETY: room for 32 display IDs, and the count written.
        if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) } != 0 || n == 0 {
            return false;
        }
        // SAFETY: CGDisplayIsAsleep takes any display ID and touches no memory of ours.
        ids[..(n as usize).min(ids.len())].iter().all(|&d| unsafe { CGDisplayIsAsleep(d) } != 0)
    }
}

impl Drop for Quartz {
    fn drop(&mut self) {
        let ids: Vec<WindowId> = self.wins.keys().copied().collect();
        for id in ids {
            crate::platform::Display::close(self, id);
        }
    }
}
