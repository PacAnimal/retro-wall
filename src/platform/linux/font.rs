//! Fonts of our own, through fontconfig: the library cairo already uses, loaded by its name so the
//! build needs none of its headers.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

type AddFile = unsafe extern "C" fn(config: *mut c_void, file: *const u8) -> c_int;

/// fontconfig's FcConfigAppFontAddFile, found once.
fn add_file() -> Result<AddFile, String> {
    static FOUND: std::sync::OnceLock<Result<usize, String>> = std::sync::OnceLock::new();
    // SAFETY: dlopen and dlsym take NUL-terminated names; dlerror's message is read before any other dl call.
    let found = FOUND.get_or_init(|| unsafe {
        let lib = libc::dlopen(c"libfontconfig.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if lib.is_null() {
            let why = libc::dlerror();
            return Err(if why.is_null() {
                "libfontconfig.so.1: not found".into()
            } else {
                CStr::from_ptr(why as *const c_char).to_string_lossy().into_owned()
            });
        }
        let f = libc::dlsym(lib, c"FcConfigAppFontAddFile".as_ptr());
        if f.is_null() { Err("libfontconfig.so.1 has no FcConfigAppFontAddFile".into()) } else { Ok(f as usize) }
    });
    // SAFETY: the symbol is fontconfig's FcConfigAppFontAddFile, of this signature
    found.clone().map(|f| unsafe { std::mem::transmute::<usize, AddFile>(f) })
}

/// Add the font file at `path` to fontconfig's current configuration, for this process.
pub fn register_font(path: &Path) -> Result<(), String> {
    let add = add_file()?;
    let file = CString::new(path.as_os_str().as_bytes()).map_err(|_| format!("{}: a NUL in the path", path.display()))?;
    // a null configuration is the current one: cairo's
    // SAFETY: `add` is fontconfig's FcConfigAppFontAddFile; a null configuration is the current one, the path NUL-terminated.
    if unsafe { add(std::ptr::null_mut(), file.as_ptr() as *const u8) } == 0 {
        return Err(format!("{}: fontconfig would not take it", path.display()));
    }
    Ok(())
}
