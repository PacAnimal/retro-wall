//! Windows 10 and 11: Win32 and WGL for the display, the system's own counters for the machine, the
//! desktop wallpaper through SystemParametersInfo and the registry.

pub mod desktop;
pub mod display;
pub mod font;
pub mod system;

use crate::platform::{Desktop, Power};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Windows;

impl crate::platform::Platform for Windows {
    type Display = display::Win;

    fn open() -> Result<display::Win, String> {
        display::Win::open()
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
        Some(Box::new(desktop::Wallpaper))
    }

    fn desktop_named(name: &str) -> Option<Box<dyn Desktop>> {
        (name == desktop::NAME).then(|| Box::new(desktop::Wallpaper) as Box<dyn Desktop>)
    }

    /// Windows names a process by its executable only.
    fn name_process(_: &str) {}
}

/// Make the font file at `path` the one cairo draws DejaVu Sans Mono with (its regular or its bold,
/// as the file says).
pub fn register_font(path: &Path) -> Result<(), String> {
    font::register(path)
}

/// A string as a NUL-terminated UTF-16 one, for Win32.
pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_ref().encode_wide().chain([0]).collect()
}

/// A UTF-16 buffer up to its first NUL.
pub fn unwide(w: &[u16]) -> String {
    String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
}

/// %LOCALAPPDATA%\retro-wall: the program, its themes, its cache and its log.
pub fn local_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(|v| PathBuf::from(v).join("retro-wall"))
}

/// Standard output and error for a GUI program. Started from a console or with its output redirected,
/// it writes there; started with neither (from Explorer or the Task Scheduler), it appends to
/// %LOCALAPPDATA%\retro-wall\retro-wall.log, kept under a megabyte (the previous one as .log.old).
pub fn attach_output() {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    unsafe extern "C" {
        fn _set_error_mode(mode: i32) -> i32;
    }
    // a failed C assertion to standard error, not a message box that would hold the wall up
    // SAFETY: sets a C runtime flag.
    unsafe { _set_error_mode(2) };
    let usable = |h: HANDLE| !h.is_null() && h != INVALID_HANDLE_VALUE;
    // SAFETY: GetStdHandle only reads the process's handle table.
    let (out, err) = unsafe { (GetStdHandle(STD_OUTPUT_HANDLE), GetStdHandle(STD_ERROR_HANDLE)) };
    if usable(out) && usable(err) {
        return; // redirected by whoever started us
    }
    let set = |h: HANDLE| {
        // SAFETY: h is an open handle owned by the process for its lifetime.
        unsafe {
            if !usable(out) {
                SetStdHandle(STD_OUTPUT_HANDLE, h);
            }
            if !usable(err) {
                SetStdHandle(STD_ERROR_HANDLE, h);
            }
        }
    };
    // SAFETY: plain calls; the name is NUL-terminated and outlives the call.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            let name = wide("CONOUT$");
            let h = CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if usable(h) {
                set(h);
                return;
            }
        }
    }
    let Some(dir) = local_dir() else { return };
    let path = dir.join("retro-wall.log");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::os::windows::io::IntoRawHandle;
        set(f.into_raw_handle());
    }
}

/// Give the allocator's free memory back to the system: the C runtime's heap already gives back a
/// large block when it is freed.
pub fn release_free_memory() {}
