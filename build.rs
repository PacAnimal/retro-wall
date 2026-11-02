//! On Windows, the program's manifest (src/platform/windows/retro-wall.manifest) is linked into the exe.
//! Without it Windows runs the program as if for Vista, and a Windows 11 24H2 desktop refuses the wall's
//! windows. Every other target needs nothing here.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR")).join("src/platform/windows");
    let manifest = dir.join("retro-wall.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
        return;
    }
    // GNU: windres makes a COFF object of the resource script, linked in as any object is
    let rc = dir.join("retro-wall.rc");
    println!("cargo:rerun-if-changed={}", rc.display());
    println!("cargo:rerun-if-env-changed=WINDRES");
    let windres = env::var("WINDRES").unwrap_or_else(|_| {
        let cross = env::var("HOST").ok() != env::var("TARGET").ok();
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        if cross { format!("{arch}-w64-mingw32-windres") } else { "windres".into() }
    });
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("retro-wall-manifest.o");
    let status = Command::new(&windres)
        .arg("--input")
        .arg(&rc)
        .arg("--include-dir")
        .arg(&dir)
        .args(["--output-format", "coff", "--output"])
        .arg(&out)
        .status()
        .unwrap_or_else(|e| panic!("{windres}: {e}: it makes the Windows manifest's resource (set WINDRES to another)"));
    assert!(status.success(), "{windres} failed on {}", rc.display());
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
