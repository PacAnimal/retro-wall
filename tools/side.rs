//! side A.png B.png OUT.png: A over B at half size, for looking at, and how far apart they are in
//! structure: the mean difference of 16-pixel blocks' averages (where scattered random details land
//! does not count; a layout, a colour or a light that differs does), and the worst block.

fn load(p: &str) -> cairo::ImageSurface {
    cairo::ImageSurface::create_from_png(&mut std::fs::File::open(p).unwrap()).unwrap()
}

fn blocks(s: &mut cairo::ImageSurface, k: usize) -> Vec<[f64; 3]> {
    let (w, h, st) = (s.width() as usize, s.height() as usize, s.stride() as usize);
    let d = s.data().unwrap();
    let mut out = vec![];
    for by in 0..h / k {
        for bx in 0..w / k {
            let mut acc = [0.0; 3];
            for y in by * k..by * k + k {
                for x in bx * k..bx * k + k {
                    for c in 0..3 {
                        acc[c] += d[y * st + x * 4 + c] as f64;
                    }
                }
            }
            out.push(acc.map(|v| v / (k * k) as f64));
        }
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut x, mut y) = (load(&a[1]), load(&a[2]));
    let (w, h) = (x.width(), x.height());
    let (bx, by) = (blocks(&mut x, 16), blocks(&mut y, 16));
    let diffs: Vec<f64> = bx.iter().zip(&by).map(|(p, q)| (0..3).map(|c| (p[c] - q[c]).abs()).sum::<f64>() / 3.0).collect();
    let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
    let worst = diffs.iter().cloned().fold(0.0, f64::max);
    let (i, _) = diffs.iter().enumerate().fold((0, 0.0), |b, (i, &d)| if d > b.1 { (i, d) } else { b });
    let cols = w as usize / 16;
    println!("structure: mean {mean:.2} levels, worst block {worst:.1} at x {} y {}", (i % cols) * 16, (i / cols) * 16);
    let out = cairo::ImageSurface::create(cairo::Format::Rgb24, w / 2, h).unwrap();
    {
        let c = cairo::Context::new(&out).unwrap();
        c.scale(0.5, 0.5);
        c.set_source_surface(&x, 0.0, 0.0).unwrap();
        c.paint().unwrap();
        c.set_source_surface(&y, 0.0, h as f64).unwrap();
        c.paint().unwrap();
    }
    out.write_to_png(&mut std::fs::File::create(&a[3]).unwrap()).unwrap();
}
