//! The live picture as the GPU draws it: one frame (--snapshot) of every bundled theme under Xvfb and
//! Mesa's llvmpipe, at 640x360 with the time held at 13:37, against the pictures in tests/reference/;
//! and the OpenGL 2.1 path against the 3.3 one.
//!
//! RETRO_BLESS=1 writes the references again (look at them before committing them).
//!
//! The comparison has a tolerance. The frame is fixed (readings, clock, time of day, host name, and
//! the font, which is carried in the binary), but cairo, pixman and FreeType rasterise the edges of
//! letters a little differently from release to release, and llvmpipe its shaders: a CI image is not
//! the machine the references were made on. Ubuntu 24.04 against Debian 13 differs only in lettering,
//! by up to 0.4% of the 4x4 blocks. A needle moved by a fraction of a second of its easing is 2.4%; a
//! layer, a colour or a part gone is far more.

#![cfg(target_os = "linux")]

mod common;

use common::procs::scratch;
use common::x::{Xvfb, difference};
use std::path::{Path, PathBuf};

const THEMES: &[&str] = &["phosphor", "neon_rain", "night_city", "aurora", "winter", "matrix", "guybrush"];
const SIZE: (i32, i32) = (640, 360);
/// A block of pixels differs when its average in a channel is more than this apart.
const LEVEL: f64 = 8.0;
/// At most this share of the blocks may differ.
const SHARE: f64 = 0.01;

fn reference(theme: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reference").join(format!("{theme}.png"))
}

/// Each theme's snapshot on `x`, into `dir`, with these extra environment variables, drawn with
/// `profile`; the failures.
fn snapshots(x: &Xvfb, dir: &Path, env: &[(&str, &str)], profile: &str) -> Vec<String> {
    let failed = std::sync::Mutex::new(vec![]);
    std::thread::scope(|s| {
        for theme in THEMES {
            let failed = &failed;
            s.spawn(move || {
                let out = dir.join(format!("{theme}.png"));
                let theme_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes").join(theme);
                let mut c = x.retro_wall(&dir.join(format!("home-{theme}")));
                c.args(["--snapshot", out.to_str().unwrap(), "--demo", "13:37", "--host", "testhost", "--theme", theme_dir.to_str().unwrap()]);
                c.envs(env.iter().copied());
                let run = c.output().expect("the binary runs");
                let said = String::from_utf8_lossy(&run.stderr);
                if !run.status.success() || !out.is_file() {
                    failed.lock().unwrap().push(format!("{theme}: no snapshot ({}): {said}", run.status));
                } else if said.lines().any(|l| !l.trim().is_empty()) {
                    failed.lock().unwrap().push(format!("{theme}: complained: {said}"));
                } else if !String::from_utf8_lossy(&run.stdout).contains(profile) {
                    failed.lock().unwrap().push(format!("{theme}: not drawn with {profile}"));
                }
            });
        }
    });
    failed.into_inner().unwrap()
}

#[test]
fn every_theme_snapshots_as_its_reference() {
    let Some(x) = Xvfb::start("every_theme_snapshots_as_its_reference", SIZE.0, SIZE.1) else { return };
    let dir = scratch("snapshots");
    let failed = snapshots(&x, &dir, &[], "OpenGL 3.3");
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let bless = std::env::var_os("RETRO_BLESS").is_some();
    let mut wrong = vec![];
    for theme in THEMES {
        let (got, want) = (dir.join(format!("{theme}.png")), reference(theme));
        if bless {
            std::fs::create_dir_all(want.parent().unwrap()).unwrap();
            std::fs::copy(&got, &want).unwrap();
            continue;
        }
        assert!(want.is_file(), "{}: no reference; RETRO_BLESS=1 makes it", want.display());
        let d = difference(&got, &want, LEVEL);
        eprintln!("{theme}: {:.3}% of blocks differ, a pixel by at most {}", d.share * 100.0, d.worst);
        if d.share > SHARE {
            wrong.push(format!("{theme}: {:.3}% of its blocks differ (at most {:.3}%): {}", d.share * 100.0, SHARE * 100.0, got.display()));
        }
    }
    assert!(wrong.is_empty(), "not as the reference:\n{}", wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_gl_2_1_path_draws_as_the_3_3_path() {
    let Some(x) = Xvfb::start("the_gl_2_1_path_draws_as_the_3_3_path", SIZE.0, SIZE.1) else { return };
    let (core, old) = (scratch("gl33"), scratch("gl21"));
    let mut failed = snapshots(&x, &core, &[], "OpenGL 3.3");
    failed.extend(snapshots(&x, &old, &[("RETRO_GL", "2.1")], "OpenGL 2.1"));
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let mut wrong = vec![];
    for theme in THEMES {
        let name = format!("{theme}.png");
        let d = difference(&core.join(&name), &old.join(&name), LEVEL);
        eprintln!("{theme}: 2.1 against 3.3: {:.3}% of blocks differ, a pixel by at most {}", d.share * 100.0, d.worst);
        if d.share > SHARE {
            wrong.push(format!("{theme}: {:.3}% of its blocks differ", d.share * 100.0));
        }
    }
    assert!(wrong.is_empty(), "2.1 and 3.3 differ:\n{}", wrong.join("\n"));
    let _ = std::fs::remove_dir_all(&core);
    let _ = std::fs::remove_dir_all(&old);
}
