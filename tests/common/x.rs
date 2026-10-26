//! A virtual X server for the tests that need a display, and what they share: running the binary on
//! it, comparing pictures, a process's CPU time.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// An Xvfb of the test's own, stopped when dropped.
pub struct Xvfb {
    child: Child,
    pub display: String,
}

impl Xvfb {
    /// A screen w by h, with GLX (Mesa's llvmpipe). None, said on stderr, when there is no Xvfb: the
    /// test is then skipped, unless RETRO_REQUIRE_XVFB is set (as in CI), when it fails.
    pub fn start(test: &str, w: i32, h: i32) -> Option<Xvfb> {
        let spawned = Command::new("Xvfb")
            .args(["-displayfd", "1", "-screen", "0", &format!("{w}x{h}x24"), "-nolisten", "tcp", "+extension", "GLX", "-noreset"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                assert!(std::env::var_os("RETRO_REQUIRE_XVFB").is_none(), "{test}: Xvfb is required here, and will not start: {e}");
                // straight to stderr, past the test harness's capture: a skip should be seen
                let _ = writeln!(std::io::stderr(), "{test}: SKIPPED: no Xvfb ({e}); install it (xvfb) to run this test");
                return None;
            }
        };
        let mut line = String::new();
        let _ = BufReader::new(child.stdout.take().expect("Xvfb's stdout")).read_line(&mut line);
        let n: u32 = line.trim().parse().unwrap_or_else(|_| panic!("{test}: Xvfb gave no display number"));
        Some(Xvfb { child, display: format!(":{n}") })
    }

    /// The binary on this display, with a home of its own (no user's config), the bundled themes and
    /// software GL.
    pub fn retro_wall(&self, home: &Path) -> Command {
        std::fs::create_dir_all(home).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_retro-wall"));
        c.env("DISPLAY", &self.display)
            .env("HOME", home)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("RETRO_GL")
            .env("RETRO_THEMES", Path::new(env!("CARGO_MANIFEST_DIR")).join("themes"))
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            .stdin(Stdio::null());
        c
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A PNG's size and its pixels, 4 bytes each (B, G, R, X), rows packed.
pub fn pixels(path: &Path) -> (i32, i32, Vec<u8>) {
    let mut f = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut s = cairo::ImageSurface::create_from_png(&mut f).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (w, h, stride) = (s.width(), s.height(), s.stride() as usize);
    let d = s.data().unwrap();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for r in 0..h as usize {
        out.extend_from_slice(&d[r * stride..r * stride + w as usize * 4]);
    }
    (w, h, out)
}

/// How far apart two pictures of one size are.
#[derive(Debug)]
pub struct Difference {
    /// Of the blocks of BLOCK by BLOCK pixels, the share whose average differs by more than the
    /// level asked for, in some channel.
    pub share: f64,
    /// The largest channel difference of any one pixel.
    pub worst: u8,
}

/// The side of the blocks `difference` averages over.
pub const BLOCK: usize = 4;

/// Compared in blocks, not pixel by pixel: the edges of letters, rasterised by another FreeType, move
/// a level or a pixel and average out over a block; a needle, a layer or a colour that moved does not.
pub fn difference(a: &Path, b: &Path, level: f64) -> Difference {
    let (wa, ha, pa) = pixels(a);
    let (wb, hb, pb) = pixels(b);
    assert_eq!((wa, ha), (wb, hb), "{} and {}: sizes differ", a.display(), b.display());
    let (w, h) = (wa as usize, ha as usize);
    let worst = pa.as_chunks::<4>().0.iter().zip(pb.as_chunks::<4>().0).flat_map(|(p, q)| (0..3).map(move |k| p[k].abs_diff(q[k]))).max().unwrap_or(0);
    let (mut over, mut all) = (0usize, 0usize);
    for by in 0..h / BLOCK {
        for bx in 0..w / BLOCK {
            let mut sum = [0i64; 3];
            for y in by * BLOCK..(by + 1) * BLOCK {
                for x in bx * BLOCK..(bx + 1) * BLOCK {
                    let i = (y * w + x) * 4;
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += pa[i + k] as i64 - pb[i + k] as i64;
                    }
                }
            }
            all += 1;
            over += sum.iter().any(|s| (*s as f64 / (BLOCK * BLOCK) as f64).abs() > level) as usize;
        }
    }
    Difference { share: over as f64 / all.max(1) as f64, worst }
}

/// A process's CPU time so far, user and system, in clock ticks.
pub fn ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let rest = stat.rsplit_once(')').map_or("", |(_, r)| r);
    let f: Vec<u64> = rest.split_whitespace().map(|v| v.parse().unwrap_or(0)).collect();
    // after the name: state, ppid, pgrp, session, tty, tpgid, flags, minflt, cminflt, majflt, cmajflt, utime, stime
    f.get(11).copied().unwrap_or(0) + f.get(12).copied().unwrap_or(0)
}

/// Clock ticks a second.
pub fn hz() -> u64 {
    // SAFETY: sysconf only reads a system value.
    (unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).max(1) as u64
}
