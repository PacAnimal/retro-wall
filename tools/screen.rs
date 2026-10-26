//! screen: what the screen showed, from xwd's dumps of the root window (`xwd -root -silent >FILE`), for
//! the Linux install test. Each prints a share, 0 to 1:
//!
//!   screen near DUMP #rrggbb     of the pixels, those within 24 of that colour in every channel
//!   screen like DUMP PNG         of the pixels, those within 24 of the PNG's at the same place
//!   screen differ DUMP DUMP      of the pixels, those that differ between the two

const TOLERANCE: u8 = 24;

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

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let v = match a.as_slice() {
        ["near", dump, colour] => {
            let p = xwd(dump);
            let hex = |i: usize| colour.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).unwrap_or_else(|| fail(format!("{colour}: not #rrggbb")));
            let c = [hex(1), hex(3), hex(5)];
            share(p.w, p.h, 4, |x, y| near(p.at(x, y), c))
        }
        ["like", dump, picture] => {
            let (p, q) = (xwd(dump), png(picture));
            share(p.w, p.h, 4, |x, y| x < q.w && y < q.h && near(p.at(x, y), q.at(x, y)))
        }
        ["differ", one, two] => {
            let (p, q) = (xwd(one), xwd(two));
            if (p.w, p.h) != (q.w, q.h) {
                fail("the dumps differ in size".into());
            }
            share(p.w, p.h, 2, |x, y| p.at(x, y) != q.at(x, y))
        }
        _ => fail("near DUMP #rrggbb | like DUMP PNG | differ DUMP DUMP".into()),
    };
    println!("{v}");
}
