//! Linux: X11 and GLX for the display, /proc and /sys for the machine, the desktop environments'
//! own tools for their wallpaper.

pub mod desktop;
pub mod font;
pub mod system;
pub mod x11;

use crate::platform::{Desktop, Power};
use std::collections::HashMap;

pub use font::register_font;

pub struct Linux;

impl crate::platform::Platform for Linux {
    type Display = x11::X;

    fn open() -> Result<x11::X, String> {
        x11::X::open()
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

    fn desktop() -> Option<Box<dyn Desktop>> {
        desktop::detect(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(), &system::user_programs()).and_then(desktop::named)
    }

    fn desktop_named(name: &str) -> Option<Box<dyn Desktop>> {
        desktop::named(name)
    }

    fn name_process(name: &str) {
        system::name_process(name)
    }
}

/// Fix glibc's mmap threshold. A scene is built from screen-sized pictures made and freed in turn;
/// glibc raises its threshold past the first one freed, so the rest come from the heap and leave it
/// fragmented, held for the life of the process. Mapped, each is given back when it is freed.
#[cfg(target_env = "gnu")]
pub fn map_large_allocations() {
    // SAFETY: glibc's mallopt takes the allocator's lock; it changes a tuning value and no memory.
    unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, 128 << 10) };
}

/// Give the allocator's free memory back to the system.
pub fn release_free_memory() {
    // SAFETY: glibc's malloc_trim takes the allocator's lock and returns only pages no allocation uses.
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::malloc_trim(0);
    }
}
