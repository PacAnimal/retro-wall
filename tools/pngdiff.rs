//! pngdiff A.png B.png [DIFF.png]: how two pictures differ. Prints the count of differing pixels, the
//! largest channel difference, and a grid of 64-pixel cells that differ; writes an amplified diff picture.

fn load(p: &str) -> cairo::ImageSurface {
    cairo::ImageSurface::create_from_png(&mut std::fs::File::open(p).unwrap()).unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (mut x, mut y) = (load(&a[1]), load(&a[2]));
    let (w, h) = (x.width(), x.height());
    assert_eq!((w, h), (y.width(), y.height()), "sizes differ");
    let (sx, sy) = (x.stride() as usize, y.stride() as usize);
    let (dx, dy) = (x.data().unwrap().to_vec(), y.data().unwrap().to_vec());
    let mut out = cairo::ImageSurface::create(cairo::Format::Rgb24, w, h).unwrap();
    let so = out.stride() as usize;
    let (mut n, mut worst, mut sum) = (0u64, 0u8, 0u64);
    let mut hist = [0u64; 256];
    let mut cells = std::collections::BTreeMap::new();
    {
        let mut o = out.data().unwrap();
        for r in 0..h as usize {
            for c in 0..w as usize {
                let (p, q) = (&dx[r * sx + c * 4..r * sx + c * 4 + 3], &dy[r * sy + c * 4..r * sy + c * 4 + 3]);
                let d = (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap();
                if d > 0 {
                    n += 1;
                    sum += d as u64;
                    hist[d as usize] += 1;
                    worst = worst.max(d);
                    *cells.entry((r / 64, c / 64)).or_insert(0u64) += 1;
                }
                let v = (d as u32 * 32).min(255) as u8;
                o[r * so + c * 4..r * so + c * 4 + 3].copy_from_slice(&[v, v, v]);
            }
        }
    }
    println!(
        "{n} of {} pixels differ ({:.4} %), largest {worst}, mean {:.2}",
        w * h,
        100.0 * n as f64 / (w * h) as f64,
        if n > 0 { sum as f64 / n as f64 } else { 0.0 }
    );
    let spread: Vec<String> = (1..=worst as usize).filter(|&d| hist[d] > 0).map(|d| format!("{d}:{}", hist[d])).collect();
    println!("by size: {}", spread.join(" "));
    let mut top: Vec<_> = cells.into_iter().collect();
    top.sort_by_key(|c| std::cmp::Reverse(c.1));
    for ((r, c), k) in top.iter().take(12) {
        println!("  cell x {}..{} y {}..{}: {k}", c * 64, c * 64 + 64, r * 64, r * 64 + 64);
    }
    if let Some(p) = a.get(3) {
        out.write_to_png(&mut std::fs::File::create(p).unwrap()).unwrap();
    }
}
