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
        wall_on(&self.display, home)
    }
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The binary on `display`, with a home of its own (no user's config), the bundled themes and software GL.
fn wall_on(display: &str, home: &Path) -> Command {
    std::fs::create_dir_all(home).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_retro-wall"));
    c.env("DISPLAY", display)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("RETRO_GL")
        .env("RETRO_THEMES", Path::new(env!("CARGO_MANIFEST_DIR")).join("themes"))
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .stdin(Stdio::null());
    c
}

/// Where a monitor is on the desktop: its output (DUMMY0 to DUMMY3), x, y, width, height.
pub type Output = (&'static str, i32, i32, i32, i32);

/// The outputs `layout` turns on and off.
const OUTPUTS: [&str; 4] = ["DUMMY0", "DUMMY1", "DUMMY2", "DUMMY3"];

/// A virtual X server of several monitors, each a RandR output as a graphics card has: Xorg on its dummy
/// driver, run as the test's user. Xvfb has no outputs, so it is always one monitor. Stopped when dropped.
pub struct Monitors {
    child: Child,
    pub display: String,
    dir: std::path::PathBuf,
}

impl Monitors {
    /// The server, its outputs laid out as `outputs`. None, said on stderr, when Xorg, its dummy
    /// driver, xrandr or xwd is missing: the test is then skipped, unless RETRO_REQUIRE_XVFB is set (as
    /// in CI), when it fails.
    pub fn start(test: &str, outputs: &[Output]) -> Option<Monitors> {
        let xorg = ["/usr/lib/xorg/Xorg", "/usr/libexec/Xorg", "/usr/bin/Xorg"].into_iter().find(|p| Path::new(p).is_file());
        let dummy = ["/usr/lib/xorg/modules/drivers/dummy_drv.so", "/usr/lib64/xorg/modules/drivers/dummy_drv.so"].iter().any(|p| Path::new(p).is_file());
        let tools = ["xrandr", "xwd"].iter().all(|t| Command::new(t).arg("-help").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok());
        let Some(xorg) = xorg.filter(|_| dummy && tools) else {
            assert!(std::env::var_os("RETRO_REQUIRE_XVFB").is_none(), "{test}: Xorg's dummy driver, xrandr and xwd are required here");
            let _ = writeln!(
                std::io::stderr(),
                "{test}: SKIPPED: no Xorg dummy driver, xrandr or xwd; install them (xserver-xorg-video-dummy, x11-xserver-utils, x11-apps) to run this test"
            );
            return None;
        };
        let dir = std::env::temp_dir().join(format!("retro-wall-test-xorg-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conf = dir.join("xorg.conf");
        std::fs::write(&conf, DUMMY_CONF).unwrap();
        let spawned = Command::new(xorg)
            .args(["-displayfd", "1", "-noreset", "-nolisten", "tcp", "-config"])
            .arg(&conf)
            .arg("-logfile")
            .arg(dir.join("xorg.log"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = spawned.unwrap_or_else(|e| panic!("{test}: Xorg will not start: {e}"));
        let mut line = String::new();
        let _ = BufReader::new(child.stdout.take().expect("Xorg's stdout")).read_line(&mut line);
        let n: u32 = line.trim().parse().unwrap_or_else(|_| panic!("{test}: Xorg gave no display number (its log: {})", dir.join("xorg.log").display()));
        let m = Monitors { child, display: format!(":{n}"), dir };
        m.layout(outputs);
        Some(m)
    }

    /// Lay the monitors out as `outputs`, as plugging, unplugging and moving them does; the others off.
    pub fn layout(&self, outputs: &[Output]) {
        for &(out, _, _, w, h) in outputs {
            let mode = format!("{w}x{h}");
            // a mode for the size; made again for another output, which xrandr refuses harmlessly
            let (hs, he, ht, vs, ve, vt) = (w + 48, w + 80, w + 160, h + 3, h + 8, h + 40);
            let clock = format!("{:.2}", (ht * vt) as f64 * 60.0 / 1e6);
            let nums: Vec<String> = [w, hs, he, ht, h, vs, ve, vt].iter().map(|v| v.to_string()).collect();
            let _ = self.xrandr(&[&["--newmode", &mode, &clock], &nums.iter().map(String::as_str).collect::<Vec<_>>()[..], &["-hsync", "+vsync"]].concat());
            let _ = self.xrandr(&["--addmode", out, &mode]);
        }
        let mut args: Vec<String> = vec![];
        for out in OUTPUTS {
            args.extend(["--output".into(), out.into()]);
            match outputs.iter().find(|o| o.0 == out) {
                Some(&(_, x, y, w, h)) => args.extend(["--mode".into(), format!("{w}x{h}"), "--pos".into(), format!("{x}x{y}")]),
                None => args.push("--off".into()),
            }
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert!(self.xrandr(&args), "xrandr {}: refused", args.join(" "));
    }

    fn xrandr(&self, args: &[&str]) -> bool {
        Command::new("xrandr").env("DISPLAY", &self.display).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }

    pub fn retro_wall(&self, home: &Path) -> Command {
        wall_on(&self.display, home)
    }

    /// What the whole desktop shows now.
    pub fn dump(&self) -> Dump {
        let out = Command::new("xwd").env("DISPLAY", &self.display).args(["-root", "-silent"]).output().expect("xwd runs");
        assert!(out.status.success(), "xwd failed");
        Dump::from_xwd(&out.stdout)
    }
}

impl Drop for Monitors {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The dummy driver's card: four outputs; a desktop up to 4096 x 2048.
const DUMMY_CONF: &str = r#"
Section "ServerFlags"
    Option "AutoAddDevices" "false"
    Option "AutoAddGPU" "false"
    Option "AutoBindGPU" "false"
EndSection
Section "Device"
    Identifier "card"
    Driver "dummy"
    VideoRam 65536
EndSection
Section "Screen"
    Identifier "screen"
    Device "card"
    DefaultDepth 24
    SubSection "Display"
        Depth 24
        Virtual 4096 2048
    EndSubSection
EndSection
"#;

/// A screen dump: width, height, and (r, g, b) by row.
pub struct Dump {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 3]>,
}

impl Dump {
    /// From xwd's output: a ZPixmap of 24 or 32 bits a pixel, true colour.
    pub fn from_xwd(data: &[u8]) -> Dump {
        let field = |i: usize| -> usize { u32::from_be_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()) as usize };
        let (header, format, w, h) = (field(0), field(2), field(4), field(5));
        let (little, bpp, line) = (field(7) == 0, field(11), field(12));
        let masks = [field(14) as u32, field(15) as u32, field(16) as u32];
        assert!(format == 2 && (bpp == 24 || bpp == 32), "xwd: format {format}, {bpp} bits a pixel: only ZPixmap of 24 or 32");
        let start = header + field(19) * 12;
        let step = bpp / 8;
        let mut px = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let b = &data[start + y * line + x * step..][..step];
                let v = (0..step).fold(0u32, |v, k| v << 8 | if little { b[step - 1 - k] } else { b[k] } as u32);
                px.push(masks.map(|m| ((v & m) >> m.trailing_zeros()) as u8));
            }
        }
        Dump { w, h, px }
    }

    /// Written as a PNG, to be looked at when a test fails.
    pub fn save(&self, path: &Path) {
        let mut s = cairo::ImageSurface::create(cairo::Format::Rgb24, self.w as i32, self.h as i32).unwrap();
        let stride = s.stride() as usize;
        {
            let mut d = s.data().unwrap();
            for (i, [r, g, b]) in self.px.iter().copied().enumerate() {
                let at = (i / self.w) * stride + (i % self.w) * 4;
                d[at..at + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        s.write_to_png(&mut std::fs::File::create(path).unwrap()).unwrap();
    }

    /// Of the pixels in the rectangle (x, y, w, h), the share that differ from `other`'s by more than
    /// 24 in a channel.
    pub fn differ(&self, other: &Dump, (x, y, w, h): (i32, i32, i32, i32)) -> f64 {
        let mut n = 0usize;
        for r in y as usize..(y + h) as usize {
            for c in x as usize..(x + w) as usize {
                let (a, b) = (self.px[r * self.w + c], other.px[r * other.w + c]);
                n += a.iter().zip(b).any(|(p, q)| p.abs_diff(q) > 24) as usize;
            }
        }
        n as f64 / (w * h) as f64
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
