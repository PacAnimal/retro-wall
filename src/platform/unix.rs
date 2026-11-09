//! What Linux and macOS read of the machine alike, through POSIX.

/// A drive's space at `path`: (used, free), each a percentage of its size.
pub fn drive_space(path: &str) -> Option<(f64, f64)> {
    let c = std::ffi::CString::new(path).ok()?;
    // SAFETY: statvfs is plain data: zeroed is a valid value.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a NUL-terminated path and a valid out-pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 || st.f_blocks == 0 {
        return None;
    }
    let (total, free) = (st.f_blocks as f64 * st.f_frsize as f64, st.f_bavail as f64 * st.f_frsize as f64);
    Some((100.0 * (total - free) / total, 100.0 * free / total))
}
