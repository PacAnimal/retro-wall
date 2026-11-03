//! XComposite and XDamage, loaded at run time (x11-dl binds neither): what the desktop's own window
//! draws (its icons over the wallpaper) read from the pixmap the server keeps of it, and told when it
//! changes, whatever covers it.

use std::ffi::{CStr, c_void};
use std::os::raw::{c_int, c_ulong};
use x11_dl::xlib;

/// The server keeps the window's contents in a pixmap of its own and still draws it on the screen.
const REDIRECT_AUTOMATIC: c_int = 0;
/// A damage event for each rectangle drawn on that was not already waiting to be told.
const REPORT_DELTA: c_int = 1;

type Query = unsafe extern "C" fn(*mut xlib::Display, *mut c_int, *mut c_int) -> c_int;
type Redirect = unsafe extern "C" fn(*mut xlib::Display, c_ulong, c_int);
type NamePixmap = unsafe extern "C" fn(*mut xlib::Display, c_ulong) -> c_ulong;
type DamageCreate = unsafe extern "C" fn(*mut xlib::Display, c_ulong, c_int) -> c_ulong;
type DamageDestroy = unsafe extern "C" fn(*mut xlib::Display, c_ulong);
type DamageSubtract = unsafe extern "C" fn(*mut xlib::Display, c_ulong, c_ulong, c_ulong);

pub struct Composite {
    redirect: Redirect,
    name_pixmap: NamePixmap,
    damage_create: DamageCreate,
    damage_destroy: DamageDestroy,
    damage_subtract: DamageSubtract,
    /// The type of a damage event on this display.
    pub damage_event: c_int,
}

/// XDamageNotifyEvent.
#[repr(C)]
pub struct DamageNotify {
    pub kind: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut xlib::Display,
    pub drawable: c_ulong,
    pub damage: c_ulong,
    pub level: c_int,
    pub more: c_int,
    pub timestamp: c_ulong,
    pub area: xlib::XRectangle,
    pub geometry: xlib::XRectangle,
}

fn library(name: &CStr) -> Option<*mut c_void> {
    // SAFETY: dlopen with a NUL-terminated name; the handle is never closed, so what it gives stays loaded.
    let h = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    (!h.is_null()).then_some(h)
}

fn symbol(lib: *mut c_void, name: &CStr) -> Option<*mut c_void> {
    // SAFETY: a handle dlopen gave and a NUL-terminated name.
    let f = unsafe { libc::dlsym(lib, name.as_ptr()) };
    (!f.is_null()).then_some(f)
}

impl Composite {
    /// Both libraries, and both extensions on the display; None where either is missing.
    ///
    /// # Safety
    /// An open display.
    pub unsafe fn open(dpy: *mut xlib::Display) -> Option<Composite> {
        let (comp, damage) = (library(c"libXcomposite.so.1")?, library(c"libXdamage.so.1")?);
        // SAFETY: each symbol is the function of that name in its library, of the type its header gives;
        // the display is the caller's promise.
        unsafe {
            let comp_query = std::mem::transmute::<*mut c_void, Query>(symbol(comp, c"XCompositeQueryExtension")?);
            let damage_query = std::mem::transmute::<*mut c_void, Query>(symbol(damage, c"XDamageQueryExtension")?);
            let (mut event, mut error) = (0, 0);
            if comp_query(dpy, &mut event, &mut error) == 0 {
                return None;
            }
            let (mut damage_event, mut error) = (0, 0);
            if damage_query(dpy, &mut damage_event, &mut error) == 0 {
                return None;
            }
            Some(Composite {
                redirect: std::mem::transmute::<*mut c_void, Redirect>(symbol(comp, c"XCompositeRedirectWindow")?),
                name_pixmap: std::mem::transmute::<*mut c_void, NamePixmap>(symbol(comp, c"XCompositeNameWindowPixmap")?),
                damage_create: std::mem::transmute::<*mut c_void, DamageCreate>(symbol(damage, c"XDamageCreate")?),
                damage_destroy: std::mem::transmute::<*mut c_void, DamageDestroy>(symbol(damage, c"XDamageDestroy")?),
                damage_subtract: std::mem::transmute::<*mut c_void, DamageSubtract>(symbol(damage, c"XDamageSubtract")?),
                damage_event,
            })
        }
    }

    /// Have the server keep `win`'s contents (a compositing manager may already), and tell of each
    /// change to them: the damage that does.
    ///
    /// # Safety
    /// An open display and a window of it.
    pub unsafe fn follow(&self, dpy: *mut xlib::Display, win: c_ulong) -> c_ulong {
        // SAFETY: the caller's promise; a redirect another client already made is an error the quiet
        // handler drops, and the window's contents are kept either way.
        unsafe {
            (self.redirect)(dpy, win, REDIRECT_AUTOMATIC);
            (self.damage_create)(dpy, win, REPORT_DELTA)
        }
    }

    /// The pixmap holding `win`'s contents now (named anew after it is mapped or resized); 0 if none.
    ///
    /// # Safety
    /// An open display and a window of it that `follow` was called for; the pixmap is freed by the caller.
    pub unsafe fn pixmap(&self, dpy: *mut xlib::Display, win: c_ulong) -> c_ulong {
        // SAFETY: the caller's promise.
        unsafe { (self.name_pixmap)(dpy, win) }
    }

    /// Ready to tell of the next change.
    ///
    /// # Safety
    /// An open display and a damage of it.
    pub unsafe fn repaired(&self, dpy: *mut xlib::Display, damage: c_ulong) {
        // SAFETY: the caller's promise; no regions: all of it repaired.
        unsafe { (self.damage_subtract)(dpy, damage, 0, 0) }
    }

    /// # Safety
    /// An open display and a damage of it, used no more.
    pub unsafe fn forget(&self, dpy: *mut xlib::Display, damage: c_ulong) {
        // SAFETY: the caller's promise.
        unsafe { (self.damage_destroy)(dpy, damage) }
    }
}
