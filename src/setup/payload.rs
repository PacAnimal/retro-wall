//! The files the setup program carries: the wall's program and its themes, appended to the setup
//! program's own file, so that one download is all a user needs.
//!
//! The setup program's bytes, then the files deflated as one archive, then a trailer: `MAGIC` and the
//! archive's length (8 bytes, little-endian). In the archive, each file is its path (4-byte length,
//! UTF-8, `/` between parts) and its bytes (8-byte length); a path of length 0 ends it. Windows runs a
//! program with data after its last section as it would without (the data is never mapped).

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{Read, Write};
use std::path::{Component, Path};

/// What ends a payload, before its length.
pub const MAGIC: &[u8; 8] = b"RWSETUP1";

/// A file of the payload: its path under the install directory (`/` between parts) and its bytes.
pub type File = (String, Vec<u8>);

/// The files as a payload: archive and trailer, to append to the setup program.
pub fn pack(files: &[File]) -> Result<Vec<u8>, String> {
    let mut z = DeflateEncoder::new(Vec::new(), Compression::best());
    for (path, data) in files {
        check(path)?;
        let p = path.as_bytes();
        let n = u32::try_from(p.len()).map_err(|_| format!("{path}: too long a path"))?;
        z.write_all(&n.to_le_bytes()).and_then(|_| z.write_all(p)).map_err(|e| e.to_string())?;
        z.write_all(&(data.len() as u64).to_le_bytes()).and_then(|_| z.write_all(data)).map_err(|e| e.to_string())?;
    }
    z.write_all(&0u32.to_le_bytes()).map_err(|e| e.to_string())?;
    let mut out = z.finish().map_err(|e| e.to_string())?;
    let len = out.len() as u64;
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&len.to_le_bytes());
    Ok(out)
}

/// A program without its payload: the setup program alone (what an uninstall's helper runs as).
pub fn strip(program: &[u8]) -> &[u8] {
    let Some(t) = program.len().checked_sub(16) else { return program };
    if &program[t..t + 8] != MAGIC {
        return program;
    }
    let len = program[t + 8..].try_into().map(u64::from_le_bytes).unwrap_or(u64::MAX);
    usize::try_from(len).ok().and_then(|l| t.checked_sub(l)).map_or(program, |start| &program[..start])
}

/// The files in a program with a payload; None if it has none.
pub fn read(program: &[u8]) -> Option<Result<Vec<File>, String>> {
    let t = program.len().checked_sub(16)?;
    if &program[t..t + 8] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(program[t + 8..].try_into().ok()?);
    let Some(start) = usize::try_from(len).ok().and_then(|l| t.checked_sub(l)) else {
        return Some(Err("the payload is longer than the program: damaged".into()));
    };
    Some(unpack(&program[start..t]))
}

fn unpack(archive: &[u8]) -> Result<Vec<File>, String> {
    let mut z = DeflateDecoder::new(archive);
    let mut files = vec![];
    let damaged = |e: std::io::Error| format!("the payload is damaged: {e}");
    loop {
        let mut n = [0u8; 4];
        z.read_exact(&mut n).map_err(damaged)?;
        let n = u32::from_le_bytes(n) as usize;
        if n == 0 {
            return Ok(files);
        }
        let mut p = vec![0u8; n];
        z.read_exact(&mut p).map_err(damaged)?;
        let path = String::from_utf8(p).map_err(|_| "the payload is damaged: a path is not UTF-8".to_string())?;
        check(&path)?;
        let mut len = [0u8; 8];
        z.read_exact(&mut len).map_err(damaged)?;
        let mut data = vec![];
        (&mut z).take(u64::from_le_bytes(len)).read_to_end(&mut data).map_err(damaged)?;
        if data.len() as u64 != u64::from_le_bytes(len) {
            return Err(format!("the payload is damaged: {path} is cut short"));
        }
        files.push((path, data));
    }
}

/// A payload's path is plain parts under the install directory: never absolute, a drive, or `..`.
fn check(path: &str) -> Result<(), String> {
    let bad = path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || Path::new(path).components().any(|c| !matches!(c, Component::Normal(_)))
        || path.split('/').any(|p| p.is_empty() || p == "." || p == "..");
    if bad { Err(format!("{path:?}: not a plain relative path")) } else { Ok(()) }
}

/// The files under `dir`, as paths under `prefix`, sorted; for packing the themes.
pub fn collect(dir: &Path, prefix: &str) -> Result<Vec<File>, String> {
    let mut out = vec![];
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}/{name}");
        let ft = e.file_type().map_err(|err| format!("{}: {err}", e.path().display()))?;
        if ft.is_dir() {
            out.extend(collect(&e.path(), &path)?);
        } else if ft.is_file() {
            out.push((path, std::fs::read(e.path()).map_err(|err| format!("{}: {err}", e.path().display()))?));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_appended_to_a_program_comes_back_whole() {
        let files: Vec<File> = vec![
            ("retro-wall.exe".into(), (0..70_000u32).map(|i| (i * 7 % 251) as u8).collect()),
            ("themes/blocky/theme.toml".into(), b"name = \"Blocky\"\n".to_vec()),
            ("themes/empty".into(), vec![]),
        ];
        let program = b"MZ the setup program itself".to_vec();
        let mut whole = program.clone();
        whole.extend(pack(&files).unwrap());
        assert_eq!(read(&whole).unwrap().unwrap(), files);
        assert!(read(&program).is_none(), "a program without one has none");
        assert_eq!(strip(&whole), &program[..], "the program alone, without it");
        assert_eq!(strip(&program), &program[..]);
    }

    #[test]
    fn a_damaged_or_crafted_payload_is_refused() {
        let mut whole = b"MZ".to_vec();
        whole.extend(pack(&[("a".into(), vec![1, 2, 3])]).unwrap());
        // cut short in the middle of its archive
        let mut cut = whole.clone();
        cut.drain(2..6);
        assert!(read(&cut).unwrap().is_err());
        // a length longer than the file
        let mut long = whole.clone();
        let n = long.len();
        long[n - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(read(&long).unwrap().is_err());
        // paths that would leave the install directory are refused, packing and unpacking
        for bad in ["../evil.exe", "/abs", "C:/x", "a\\b", "a//b", "./a", ""] {
            assert!(pack(&[(bad.into(), vec![])]).is_err(), "{bad:?}");
        }
    }
}
