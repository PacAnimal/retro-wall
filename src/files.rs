//! Reading the files a theme or the config names. Each is opened through `open`: a regular file only
//! (a FIFO, a device or a directory is refused before anything is read from it), and no larger than its
//! kind's limit.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// What a file is, for its size limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// theme.toml, retro.toml, retro-wall.conf, a saved desktop
    Settings,
    /// a shader's GLSL
    Shader,
    /// a lettering's JSON
    Lettering,
    /// a PNG
    Picture,
    /// an SVG or SVGZ, before it is unzipped
    Svg,
}

impl Kind {
    /// The most bytes a file of this kind may have.
    pub const fn limit(self) -> u64 {
        match self {
            Kind::Settings => 4 << 20,
            Kind::Shader => 1 << 20,
            Kind::Lettering => 16 << 20,
            Kind::Picture => 256 << 20,
            Kind::Svg => 64 << 20,
        }
    }
}

fn refused(what: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// A regular file, open for reading, no larger than `kind` allows; reads past the limit end there.
pub fn open(path: &Path, kind: Kind) -> io::Result<io::Take<File>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // a FIFO opened without O_NONBLOCK waits for a writer; it is refused below, unread
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let f = options.open(path)?;
    let meta = f.metadata()?;
    if !meta.is_file() {
        return Err(refused("not a regular file".into()));
    }
    let limit = kind.limit();
    if meta.len() > limit {
        return Err(refused(format!("{} bytes: the most is {limit}", meta.len())));
    }
    // a file that grows after it is opened is cut at one byte past the limit, and refused by `read`
    Ok(f.take(limit + 1))
}

/// A file's bytes (see `open`).
pub fn read(path: &Path, kind: Kind) -> io::Result<Vec<u8>> {
    let mut f = open(path, kind)?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)?;
    if out.len() as u64 > kind.limit() {
        return Err(refused(format!("more than {} bytes", kind.limit())));
    }
    Ok(out)
}

/// A file's text (see `open`).
pub fn read_text(path: &Path, kind: Kind) -> io::Result<String> {
    String::from_utf8(read(path, kind)?).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8 text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("retro-wall-files-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_a_regular_file() {
        let d = scratch("regular");
        std::fs::write(d.join("a.toml"), "x = 1\n").unwrap();
        assert_eq!(read_text(&d.join("a.toml"), Kind::Settings).unwrap(), "x = 1\n");
    }

    #[test]
    fn refuses_a_directory_and_what_is_missing() {
        let d = scratch("dir");
        assert!(read(&d, Kind::Settings).is_err());
        assert!(read(&d.join("none"), Kind::Settings).is_err());
    }

    #[test]
    fn refuses_a_file_over_its_limit() {
        let d = scratch("large");
        let p = d.join("big.frag");
        std::fs::write(&p, vec![b' '; Kind::Shader.limit() as usize + 1]).unwrap();
        let e = read(&p, Kind::Shader).unwrap_err();
        assert!(e.to_string().contains("the most is"), "{e}");
        std::fs::write(&p, vec![b' '; Kind::Shader.limit() as usize]).unwrap();
        assert!(read(&p, Kind::Shader).is_ok());
    }

    #[test]
    fn refuses_text_that_is_not_utf8() {
        let d = scratch("utf8");
        std::fs::write(d.join("x"), [0xff, 0xfe]).unwrap();
        assert!(read_text(&d.join("x"), Kind::Settings).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_device_and_a_fifo_without_waiting() {
        assert!(read(Path::new("/dev/zero"), Kind::Shader).unwrap_err().to_string().contains("not a regular file"));
        let d = scratch("fifo");
        let p = d.join("pipe");
        let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let t = std::time::Instant::now();
        assert!(read(&p, Kind::Settings).unwrap_err().to_string().contains("not a regular file"));
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
    }
}
