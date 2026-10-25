//! The one typeface everything is lettered in, DejaVu Sans Mono, carried in the binary: written once to
//! the cache directory and registered with the system's font machinery for this process, so the
//! picture is the same whether or not the system has the font.

use std::path::{Path, PathBuf};

/// The family name every drawing selects.
pub const FAMILY: &str = "DejaVu Sans Mono";

/// The embedded faces: file name, contents.
pub const FACES: &[(&str, &[u8])] =
    &[("DejaVuSansMono.ttf", include_bytes!("../fonts/DejaVuSansMono.ttf")), ("DejaVuSansMono-Bold.ttf", include_bytes!("../fonts/DejaVuSansMono-Bold.ttf"))];

/// Where the faces are written: the cache's `fonts` directory, else a directory in the system's
/// temporary directory.
fn place() -> PathBuf {
    crate::svg::cache_dir().map(|r| r.join("fonts")).unwrap_or_else(|| std::env::temp_dir().join(format!("retro-wall-fonts-{}", whoami())))
}

#[cfg(unix)]
fn whoami() -> u32 {
    // SAFETY: getuid cannot fail and touches no memory.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn whoami() -> u32 {
    0
}

/// `bytes` as a file in `dir`, named for its contents: written unless a file of that name and size
/// is there already.
pub fn materialise(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, "ttf"));
    let path = dir.join(format!("{stem}-{:08x}.{ext}", crate::svg::fnv(bytes) as u32));
    if path.metadata().is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Ok(path);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let part = path.with_extension(format!("{}.part", std::process::id()));
    std::fs::write(&part, bytes).and_then(|_| std::fs::rename(&part, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {e}", path.display())
    })?;
    Ok(path)
}

/// Make the embedded faces available to this process. Said, not fatal, when it fails: the system's
/// own DejaVu Sans Mono, or its nearest, is used instead. Once a process, and before the first lookup
/// of the family: cairo keeps a lookup that failed, and a face registered after it is not seen.
pub fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = place();
        for (name, bytes) in FACES {
            match materialise(&dir, name, bytes).and_then(|p| crate::platform::register_font(&p).map(|()| p)) {
                Ok(p) => REGISTERED.lock().unwrap_or_else(|e| e.into_inner()).push(p),
                Err(e) => crate::errln!("font {name}: {e}"),
            }
        }
    });
}

static REGISTERED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// The embedded faces' files the system took, for `--list`: the text is drawn from these.
pub fn registered() -> Vec<PathBuf> {
    REGISTERED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_family_name_has_no_nul() {
        // cairo-rs panics on one (util::select_mono passes it as it is)
        assert!(!super::FAMILY.contains('\0'));
    }
}
