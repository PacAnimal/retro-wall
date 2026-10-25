//! macOS: Cocoa and NSOpenGL for the display, Mach, sysctl and IOKit for the machine, NSWorkspace for
//! the desktop picture.

pub mod cocoa;
pub mod desktop;
pub mod system;

use crate::platform::{Desktop, Power};
use std::collections::HashMap;

pub struct Mac;

impl crate::platform::Platform for Mac {
    type Display = cocoa::Quartz;

    fn open() -> Result<cocoa::Quartz, String> {
        cocoa::Quartz::open()
    }

    fn power() -> Power {
        system::power()
    }

    fn vnc_connected(port: u16) -> bool {
        system::vnc_connected(port)
    }

    fn cpu_times() -> Option<(u64, u64)> {
        system::cpu_times()
    }

    fn memory() -> Option<(f64, f64)> {
        system::memory()
    }

    fn disk_ios() -> u64 {
        system::disk_ios()
    }

    fn drive_space(path: &str) -> Option<(f64, f64)> {
        system::drive_space(path)
    }

    fn net_counts(only: &[String]) -> HashMap<String, (u64, u64)> {
        system::net_counts(only)
    }

    fn addresses() -> (Option<String>, Option<String>) {
        system::addresses()
    }

    fn hostname() -> String {
        system::hostname()
    }

    /// NSWorkspace's per-screen picture, unless a profile forces it: then the wall shows its own.
    fn desktop() -> Option<Box<dyn Desktop>> {
        (!desktop::managed()).then(|| Box::new(desktop::Workspace) as Box<dyn Desktop>)
    }

    fn desktop_named(name: &str) -> Option<Box<dyn Desktop>> {
        (name == "macos").then(|| Box::new(desktop::Workspace) as Box<dyn Desktop>)
    }

    /// ps shows the command line, which cannot be changed here.
    fn name_process(_name: &str) {}
}

/// Give the allocator's free memory back to the system: a large block freed stays in the process's
/// footprint until then.
pub fn release_free_memory() {
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: a null zone means every zone; a goal of 0 means as much as can be.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

/// Make the font file at `path` known to this process, for cairo's Quartz fonts to find by name. A
/// font already known (installed for the user or the system) is fine as it is.
pub fn register_font(path: &std::path::Path) -> Result<(), String> {
    use objc2_core_foundation::{CFError, CFRetained, CFURL};
    use objc2_core_text::{CTFontManagerRegisterFontsForURL, CTFontManagerScope};
    const ALREADY_REGISTERED: isize = 105;
    let url = CFURL::from_file_path(path).ok_or(format!("{}: not a file path", path.display()))?;
    let mut error: *mut CFError = std::ptr::null_mut();
    // SAFETY: a valid URL, and a place for CoreText to put a +1 error.
    if unsafe { CTFontManagerRegisterFontsForURL(&url, CTFontManagerScope::Process, &mut error) } {
        return Ok(());
    }
    // SAFETY: on failure `error` is null or a +1 CFError we now own.
    let error = std::ptr::NonNull::new(error).map(|e| unsafe { CFRetained::from_raw(e) });
    match error.map(|e| (e.code(), e.description().map(|d| d.to_string()))) {
        Some((ALREADY_REGISTERED, _)) => Ok(()),
        Some((code, d)) => Err(format!("{}: {}", path.display(), d.unwrap_or_else(|| format!("CoreText error {code}")))),
        None => Err(format!("{}: CoreText did not take the font", path.display())),
    }
}
