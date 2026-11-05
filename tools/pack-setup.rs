//! The setup program with what it installs carried in it: the one file a Windows user downloads.
//!
//!   cargo run --release --example pack-setup -- SETUP.exe RETRO-WALL.exe THEMES-DIR OUT.exe
//!
//! SETUP.exe and RETRO-WALL.exe are the Windows builds (target/x86_64-pc-windows-gnu/release); it runs
//! on any system. The payload's format: `retro_wall::setup::payload`.

use retro_wall::setup::payload;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [setup, wall, themes, out] = args.as_slice() else {
        retro_wall::errln!("usage: pack-setup SETUP.exe RETRO-WALL.exe THEMES-DIR OUT.exe");
        std::process::exit(2);
    };
    let fail = |e: String| -> ! {
        retro_wall::errln!("pack-setup: {e}");
        std::process::exit(1)
    };
    let mut program = std::fs::read(setup).unwrap_or_else(|e| fail(format!("{setup}: {e}")));
    if payload::read(&program).is_some() {
        fail(format!("{setup} carries a payload already"));
    }
    let mut files = vec![("retro-wall.exe".to_string(), std::fs::read(wall).unwrap_or_else(|e| fail(format!("{wall}: {e}"))))];
    files.extend(payload::collect(Path::new(themes), "themes").unwrap_or_else(|e| fail(e)));
    let n = files.len();
    program.extend(payload::pack(&files).unwrap_or_else(|e| fail(e)));
    std::fs::write(out, &program).unwrap_or_else(|e| fail(format!("{out}: {e}")));
    // read back, so a payload it cannot read is never shipped
    match payload::read(&std::fs::read(out).unwrap_or_default()) {
        Some(Ok(back)) if back == files => retro_wall::outln!("{out}: {n} files, {} bytes", program.len()),
        _ => fail(format!("{out}: the payload did not read back")),
    }
}
