//! The still picture: the theme with everything at rest. The desktop shows it under the live window
//! and when retro-wall is not running; --preview writes it.

use crate::config::Theme;
use crate::platform::{Desktop, Screen};
use crate::scene::{Geo, Scene};
use cairo::{Context, ImageSurface};

/// Where the still pictures go, beside the installed program: ~/.local/share/retro; on macOS
/// ~/Library/Application Support/retro-wall; on Windows %LOCALAPPDATA%\retro-wall\wallpaper.
pub fn out_dir() -> std::path::PathBuf {
    if cfg!(windows) {
        return std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("retro-wall").join("wallpaper");
    }
    let dir = if cfg!(target_os = "macos") { "Library/Application Support/retro-wall" } else { ".local/share/retro" };
    crate::config::home().join(dir)
}

/// The still picture at width x height.
pub fn draw(g: &Geo) -> Result<ImageSurface, String> {
    let s = crate::util::surface_rgb(g.w, g.h);
    {
        let c = Context::new(&s).map_err(|e| e.to_string())?;
        Scene::new(g.clone()).compose(&c);
    }
    crate::elements::built();
    Ok(s)
}

/// Write a monitor's still picture, named by its content (a desktop may cache pictures by path), and
/// have the desktop show it, `th`'s base colour round it. Older pictures for that monitor are removed.
pub fn set_wallpaper(th: &Theme, desktop: &dyn Desktop, s: &Screen, picture: &mut ImageSurface) {
    let dir = out_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for chunk in crate::util::view(picture).as_deref().unwrap_or_default().chunks(8) {
        let mut b = [0u8; 8];
        b[..chunk.len()].copy_from_slice(chunk);
        h = (h ^ u64::from_le_bytes(b)).wrapping_mul(0x100000001b3);
    }
    let name = &s.name;
    let path = dir.join(format!("wallpaper-{name}-{h:016x}.png"));
    if !path.exists() {
        let tmp = path.with_extension("tmp");
        if let Ok(mut f) = std::fs::File::create(&tmp)
            && picture.write_to_png(&mut f).is_ok()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    let p = path.to_string_lossy().into_owned();
    desktop.show(s, &p, &th.tw.base.hex());
    crate::outln!("{name}: {}x{} -> {p}", picture.width(), picture.height());
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&format!("wallpaper-{name}-")) && n.ends_with(".png") && e.path() != path {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
