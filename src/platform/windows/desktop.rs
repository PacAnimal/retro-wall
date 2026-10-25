//! The Windows desktop's wallpaper: the picture through SystemParametersInfo, its placement in
//! HKCU\Control Panel\Desktop (WallpaperStyle, TileWallpaper), the colour round it in
//! HKCU\Control Panel\Colors (Background) and the session's system colours. One picture serves every
//! monitor. A slideshow or Spotlight is not saved: restoring sets the picture it showed when saved.

use super::{unwide, wide};
use crate::platform::{Desktop, Screen};
use windows_sys::Win32::Graphics::Gdi::{COLOR_DESKTOP, SetSysColors};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW};

pub const NAME: &str = "windows";

const DESKTOP: &str = r"Control Panel\Desktop";
const COLOURS: &str = r"Control Panel\Colors";

pub struct Wallpaper;

/// A string value under HKCU; None when it is not there.
fn get(key: &str, value: &str) -> Option<String> {
    let (k, v) = (wide(key), wide(value));
    let mut buf = vec![0u16; 1024];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: NUL-terminated names and a buffer of `bytes` bytes.
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut bytes) };
    (r == 0).then(|| unwide(&buf))
}

fn set(key: &str, value: &str, data: &str) -> Result<(), String> {
    let (k, v, d) = (wide(key), wide(value), wide(data));
    // SAFETY: NUL-terminated names; the data is d.len() UTF-16 units with its NUL.
    let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), REG_SZ, d.as_ptr().cast(), (d.len() * 2) as u32) };
    if r == 0 { Ok(()) } else { Err(format!(r"HKCU\{key}\{value}: error {r}")) }
}

/// The picture's path as the system has it; empty for none.
fn picture() -> String {
    let mut buf = [0u16; 1024];
    // SAFETY: a buffer of the length given, in characters.
    unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, buf.as_mut_ptr().cast(), 0) };
    unwide(&buf)
}

/// Show `path` (empty: no picture), saved to the user's profile and announced to every window.
fn set_picture(path: &str) -> Result<(), String> {
    let mut p = wide(path);
    // SAFETY: a NUL-terminated path, read during the call.
    if unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, p.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) } == 0 {
        return Err(format!("SPI_SETDESKWALLPAPER {path}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// The desktop colour, "R G B" as the registry holds it, for this session too.
fn set_colour(rgb: &str) -> Result<(), String> {
    let c: Vec<u32> = rgb.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    if c.len() == 3 {
        let colour = c[0].min(255) | c[1].min(255) << 8 | c[2].min(255) << 16;
        // SAFETY: one element and one colour.
        unsafe { SetSysColors(1, &COLOR_DESKTOP, &colour) };
    }
    set(COLOURS, "Background", rgb)
}

impl Desktop for Wallpaper {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Centred, the colour round it.
    fn show(&self, _: &Screen, path: &str, colour: &str) {
        let byte = |i: usize| colour.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(0);
        let rgb = format!("{} {} {}", byte(1), byte(3), byte(5));
        let placed = get(DESKTOP, "WallpaperStyle").as_deref() == Some("0") && get(DESKTOP, "TileWallpaper").as_deref() == Some("0");
        if picture().eq_ignore_ascii_case(path) && placed && get(COLOURS, "Background").as_deref() == Some(rgb.as_str()) {
            return; // shown already: setting it again redraws the desktop
        }
        let done =
            set(DESKTOP, "WallpaperStyle", "0").and_then(|_| set(DESKTOP, "TileWallpaper", "0")).and_then(|_| set_colour(&rgb)).and_then(|_| set_picture(path));
        if let Err(e) = done {
            crate::errln!("wallpaper: {e}");
        }
    }

    /// Lines of setting, tab, value: the picture, its style and tiling, the colour round it.
    fn save(&self) -> Result<String, String> {
        let mut out = format!("picture\t{}\n", picture());
        for (name, key, value) in [("style", DESKTOP, "WallpaperStyle"), ("tile", DESKTOP, "TileWallpaper"), ("colour", COLOURS, "Background")] {
            if let Some(v) = get(key, value) {
                out += &format!("{name}\t{v}\n");
            }
        }
        Ok(out)
    }

    fn restore(&self, saved: &str) -> Result<(), String> {
        let value = |name: &str| saved.lines().find_map(|l| l.split_once('\t').filter(|(k, _)| *k == name).map(|(_, v)| v.to_string()));
        if let Some(v) = value("style") {
            set(DESKTOP, "WallpaperStyle", &v)?;
        }
        if let Some(v) = value("tile") {
            set(DESKTOP, "TileWallpaper", &v)?;
        }
        if let Some(v) = value("colour") {
            set_colour(&v)?;
        }
        set_picture(&value("picture").unwrap_or_default())
    }
}
