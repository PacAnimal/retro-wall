//! screen: what the screen showed, from xwd's dumps of the root window (`xwd -root -silent >FILE`), or
//! grim's PNGs of a Wayland compositor's screen, for the Linux install test. Each prints a share, 0 to 1:
//!
//!   screen near DUMP #rrggbb     of the pixels, those within 24 of that colour in every channel
//!   screen like DUMP PNG         of the pixels, those within 24 of the PNG's at the same place
//!   screen differ DUMP DUMP      of the pixels, those that differ between the two
//!   screen kept REF BASE DUMP [OVER]   what the desktop drew: the pixels where REF differs from BASE (a
//!                                dump, or #rrggbb), and so do their four neighbours (over its own
//!                                wallpaper or before a menu opened, without its edges, which blend with
//!                                what is under them); with OVER, the same desktop over another
//!                                background, only those it shows alike there too (what it draws opaque:
//!                                not a shadow or a see-through label, which show what is under them).
//!                                Of each part of it (pixels touching one another, MIN_PART or more), the
//!                                share DUMP shows as REF does: the least of them. On stderr, each part,
//!                                and how much of the screen it all is
//!                                Less than MIN_DRAWN of it: no answer, the test would prove nothing
//!   screen drawn-as REF BASE #rrggbb   of all the pixels kept looks at, those within 24 of that colour
//!                                (a menu that drew nothing but black)
//!   screen clicks                of the nine parts of $DISPLAY's screen, three by three, those where a
//!                                click at the middle goes past retro-wall (and any frame it is in) to
//!                                what is under it

const TOLERANCE: u8 = 24;
/// The least share of the screen `kept` looks at.
const MIN_DRAWN: f64 = 0.002;
/// The least part, in pixels, `kept` tells of on its own.
const MIN_PART: usize = 200;

/// A picture: width, height, and (r, g, b) by row.
struct Picture {
    w: usize,
    h: usize,
    px: Vec<[u8; 3]>,
}

impl Picture {
    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        self.px[y * self.w + x]
    }
}

fn fail(why: String) -> ! {
    eprintln!("screen: {why}");
    std::process::exit(2)
}

/// A screen dump: grim's PNG (a compositor's own screen, as on Hyprland), else xwd's.
fn load(path: &str) -> Picture {
    let mut head = [0u8; 8];
    let png_file = std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && head == *b"\x89PNG\r\n\x1a\n";
    if png_file { png(path) } else { xwd(path) }
}

/// An xwd dump: a ZPixmap of 24 or 32 bits a pixel, true colour.
fn xwd(path: &str) -> Picture {
    let data = std::fs::read(path).unwrap_or_else(|e| fail(format!("{path}: {e}")));
    let field = |i: usize| -> usize {
        let b = data.get(i * 4..i * 4 + 4).unwrap_or_else(|| fail(format!("{path}: cut short")));
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize
    };
    let (header, format, w, h) = (field(0), field(2), field(4), field(5));
    let (little, bpp, line) = (field(7) == 0, field(11), field(12));
    let masks = [field(14) as u32, field(15) as u32, field(16) as u32];
    let colours = field(19);
    if format != 2 || !(bpp == 24 || bpp == 32) {
        fail(format!("{path}: format {format}, {bpp} bits a pixel: only ZPixmap of 24 or 32"));
    }
    let start = header + colours * 12;
    let step = bpp / 8;
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let at = start + y * line + x * step;
            let b = data.get(at..at + step).unwrap_or_else(|| fail(format!("{path}: cut short")));
            let mut v = 0u32;
            for k in 0..step {
                let byte = if little { b[step - 1 - k] } else { b[k] };
                v = v << 8 | byte as u32;
            }
            px.push(masks.map(|m| ((v & m) >> m.trailing_zeros()) as u8));
        }
    }
    Picture { w, h, px }
}

fn png(path: &str) -> Picture {
    let mut f = std::fs::File::open(path).unwrap_or_else(|e| fail(format!("{path}: {e}")));
    let mut s = cairo::ImageSurface::create_from_png(&mut f).unwrap_or_else(|e| fail(format!("{path}: {e}")));
    let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
    let d = s.data().unwrap_or_else(|e| fail(format!("{path}: {e}")));
    let px = (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).map(|(y, x)| {
        let i = y * stride + x * 4;
        [d[i + 2], d[i + 1], d[i]]
    });
    Picture { w, h, px: px.collect() }
}

/// What the desktop drew: the pixels where `r` differs from `base` (a dump, or #rrggbb), and so do their
/// four neighbours (without its edges, which blend with what is under them). Too few of them to tell
/// anything by ends the program.
fn footprint(r: &Picture, base: &str, over: Option<&Picture>) -> Vec<bool> {
    let (flat, under) = match base.starts_with('#') {
        true => (Some(colour(base)), None),
        false => (None, Some(load(base))),
    };
    if under.iter().chain(over).any(|b| (b.w, b.h) != (r.w, r.h)) {
        fail("the dumps differ in size".into());
    }
    let differs = |x: usize, y: usize| !near(r.at(x, y), flat.unwrap_or_else(|| under.as_ref().map_or([0; 3], |b| b.at(x, y))));
    let drawn = |x: usize, y: usize| {
        x > 0 && y > 0 && x + 1 < r.w && y + 1 < r.h && [(x, y), (x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)].iter().all(|&(a, b)| differs(a, b))
    };
    let opaque = |x: usize, y: usize| over.is_none_or(|o| near(r.at(x, y), o.at(x, y)));
    let mask: Vec<bool> = (0..r.h).flat_map(|y| (0..r.w).map(move |x| (x, y))).map(|(x, y)| drawn(x, y) && opaque(x, y)).collect();
    let area = mask.iter().filter(|&&d| d).count() as f64 / mask.len().max(1) as f64;
    eprintln!("the desktop drew {:.2}% of the screen", area * 100.0);
    if area < MIN_DRAWN {
        fail(format!("the desktop drew {:.3}% of the screen there: too little to tell", area * 100.0));
    }
    mask
}

/// The parts of a footprint, each a set of pixels touching one another; those under MIN_PART pixels (a
/// letter, a clock's digit) left out.
fn parts(mask: &[bool], w: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut out = vec![];
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let (mut part, mut todo) = (vec![], vec![start]);
        seen[start] = true;
        while let Some(i) = todo.pop() {
            part.push(i);
            let (x, y) = (i % w, i / w);
            let next = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (i + w < mask.len()).then(|| i + w)];
            for j in next.into_iter().flatten() {
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    todo.push(j);
                }
            }
        }
        if part.len() >= MIN_PART {
            out.push(part);
        }
    }
    out
}

/// "#rrggbb" as its three bytes.
fn colour(text: &str) -> [u8; 3] {
    let hex = |i: usize| text.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or_else(|| fail(format!("{text}: not #rrggbb")));
    [hex(1), hex(3), hex(5)]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(p, q)| p.abs_diff(q) <= TOLERANCE)
}

/// The share of the points of a grid, every `step` pixels over w by h, that pass `test`.
fn share(w: usize, h: usize, step: usize, test: impl Fn(usize, usize) -> bool) -> f64 {
    let (mut hits, mut all) = (0usize, 0usize);
    for y in (0..h).step_by(step) {
        for x in (0..w).step_by(step) {
            all += 1;
            hits += test(x, y) as usize;
        }
    }
    hits as f64 / all.max(1) as f64
}

/// Where a click would go at the middles of the nine parts of the screen (three by three), asked of the
/// X server: the pointer put on each, and the window under it followed down from the root. The server
/// answers with each window's input region applied, as it does for a real click. A point fails if that
/// window is a retro-wall window or one it is inside (a window manager's frame). Each point is told on
/// stderr; at least one must lie on a retro-wall window, or the test proves nothing.
#[cfg(target_os = "linux")]
fn clicks() -> f64 {
    use std::os::raw::{c_int, c_uint, c_ulong};
    use x11_dl::xlib;
    let xl = xlib::Xlib::open().unwrap_or_else(|e| fail(format!("no Xlib: {e}")));
    // SAFETY: a display opened here and used only here; XQueryTree's and XFetchName's data are Xlib's,
    // checked for null and freed with XFree; out-pointers are locals.
    unsafe {
        let dpy = (xl.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            fail("no display".into());
        }
        let root = (xl.XDefaultRootWindow)(dpy);
        let kids = |w: c_ulong| -> (c_ulong, Vec<c_ulong>) {
            let (mut r, mut parent, mut list, mut n) = (0, 0, std::ptr::null_mut(), 0 as c_uint);
            if (xl.XQueryTree)(dpy, w, &mut r, &mut parent, &mut list, &mut n) == 0 {
                return (0, vec![]);
            }
            let v = if list.is_null() { vec![] } else { std::slice::from_raw_parts(list, n as usize).to_vec() };
            if !list.is_null() {
                (xl.XFree)(list as *mut _);
            }
            (parent, v)
        };
        let named = |w: c_ulong| {
            let mut name = std::ptr::null_mut();
            if (xl.XFetchName)(dpy, w, &mut name) == 0 || name.is_null() {
                return false;
            }
            let ours = std::ffi::CStr::from_ptr(name).to_bytes() == b"retro-wall";
            (xl.XFree)(name as *mut _);
            ours
        };
        // the mapped retro-wall windows: where each is on the screen, and the windows it is inside
        let mut walls: Vec<((c_int, c_int, c_int, c_int), Vec<c_ulong>)> = vec![];
        let mut todo = vec![root];
        while let Some(w) = todo.pop() {
            let mut a: xlib::XWindowAttributes = std::mem::zeroed();
            if (xl.XGetWindowAttributes)(dpy, w, &mut a) == 0 || (w != root && a.map_state != xlib::IsViewable) {
                continue;
            }
            if w != root && named(w) {
                let (mut x, mut y, mut child) = (0, 0, 0);
                (xl.XTranslateCoordinates)(dpy, w, root, 0, 0, &mut x, &mut y, &mut child);
                let mut up = vec![w];
                loop {
                    let (parent, _) = kids(*up.last().unwrap_or(&root));
                    if parent == 0 || parent == root {
                        break;
                    }
                    up.push(parent);
                }
                walls.push(((x, y, a.width, a.height), up));
            }
            todo.extend(kids(w).1);
        }
        let screen = (xl.XDefaultScreen)(dpy);
        let (sw, sh) = ((xl.XDisplayWidth)(dpy, screen), (xl.XDisplayHeight)(dpy, screen));
        let (mut back_x, mut back_y, mut wx, mut wy, mut mask, mut r, mut c) = (0, 0, 0, 0, 0 as c_uint, 0, 0);
        (xl.XQueryPointer)(dpy, root, &mut r, &mut c, &mut back_x, &mut back_y, &mut wx, &mut wy, &mut mask);
        let (mut passed, mut on_wall) = (0, 0);
        for j in 0..3 {
            for i in 0..3 {
                let (px, py) = (sw * (2 * i + 1) / 6, sh * (2 * j + 1) / 6);
                (xl.XWarpPointer)(dpy, 0, root, 0, 0, 0, 0, px, py);
                (xl.XSync)(dpy, 0);
                let mut under = root;
                loop {
                    let (mut rr, mut ch, mut rx, mut ry, mut x, mut y, mut m) = (0, 0, 0, 0, 0, 0, 0 as c_uint);
                    if (xl.XQueryPointer)(dpy, under, &mut rr, &mut ch, &mut rx, &mut ry, &mut x, &mut y, &mut m) == 0 || ch == 0 {
                        break;
                    }
                    under = ch;
                }
                let over = walls.iter().any(|((x, y, w, h), _)| px >= *x && py >= *y && px < x + w && py < y + h);
                let taken = walls.iter().find(|(_, up)| up.contains(&under));
                on_wall += over as usize;
                let what = match taken {
                    Some((_, up)) if up[0] == under => "retro-wall: FAIL",
                    Some(_) => "the frame retro-wall is in: FAIL",
                    None => "past retro-wall: ok",
                };
                passed += taken.is_none() as usize;
                let place = if over { "on retro-wall" } else { "off retro-wall" };
                eprintln!("clicks: part {} at {px},{py}, {place}, goes to 0x{under:x}, {what}", j * 3 + i + 1);
            }
        }
        (xl.XWarpPointer)(dpy, 0, root, 0, 0, 0, 0, back_x, back_y);
        (xl.XCloseDisplay)(dpy);
        if on_wall == 0 {
            fail("no part of the screen has a retro-wall window under its middle".into());
        }
        passed as f64 / 9.0
    }
}

#[cfg(not(target_os = "linux"))]
fn clicks() -> f64 {
    fail("clicks: X11 only".into())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let v = match a.as_slice() {
        ["near", dump, c] => {
            let (p, c) = (load(dump), colour(c));
            share(p.w, p.h, 4, |x, y| near(p.at(x, y), c))
        }
        ["like", dump, picture] => {
            let (p, q) = (load(dump), png(picture));
            share(p.w, p.h, 4, |x, y| x < q.w && y < q.h && near(p.at(x, y), q.at(x, y)))
        }
        ["differ", one, two] => {
            let (p, q) = (load(one), load(two));
            if (p.w, p.h) != (q.w, q.h) {
                fail("the dumps differ in size".into());
            }
            share(p.w, p.h, 2, |x, y| p.at(x, y) != q.at(x, y))
        }
        ["kept", reference, base, dump, over @ ..] if over.len() <= 1 => {
            let (r, d, over) = (load(reference), load(dump), over.first().map(|o| load(o)));
            if (r.w, r.h) != (d.w, d.h) {
                fail("the dumps differ in size".into());
            }
            // each part on its own: a large one (a panel) must not hide a small one (an icon) under it
            let parts = parts(&footprint(&r, base, over.as_ref()), r.w);
            if parts.is_empty() {
                fail(format!("no part of {MIN_PART} pixels or more drawn: nothing to tell by"));
            }
            let worst = parts.iter().map(|p| {
                let kept = p.iter().filter(|&&i| near(r.px[i], d.px[i])).count() as f64 / p.len() as f64;
                let (x, y) = (p.iter().map(|i| i % r.w).min().unwrap_or(0), p.iter().map(|i| i / r.w).min().unwrap_or(0));
                eprintln!("  a part of {} pixels at {x},{y}: {kept:.3} kept", p.len());
                kept
            });
            worst.fold(f64::NAN, f64::min)
        }
        ["drawn-as", reference, base, c] => {
            let (r, c) = (load(reference), colour(c));
            let mask = footprint(&r, base, None);
            let all = mask.iter().filter(|&&m| m).count();
            mask.iter().enumerate().filter(|&(i, &m)| m && near(r.px[i], c)).count() as f64 / all.max(1) as f64
        }
        ["clicks"] => clicks(),
        _ => fail("near DUMP #rrggbb | like DUMP PNG | differ DUMP DUMP | kept REF BASE DUMP [OVER] | drawn-as REF BASE #rrggbb | clicks".into()),
    };
    println!("{v}");
}
