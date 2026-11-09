//! The pixel art: pictures of lines made into the low frame's pixels, tinted, halved and lit.

use super::*;

/// A pixel of a picture of lines (`pixel_art_with`), from its straight colours `px`: its colour in
/// `levels`, and there as much as the picture covers it, over an outline as much as its fullest
/// neighbour covers its own, `edge` of it. A pixel either there or not made a line one, two or three
/// pixels wide by where it fell, so its width stepped as it grew; covered in part, it grows smoothly.
pub(super) fn lined(px: &[[f64; 4]], w: usize, h: usize, x: i64, y: i64, levels: f64, edge: f64) -> [f64; 4] {
    let at = |x: i64, y: i64| (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h).then(|| px[y as usize * w + x as usize]);
    let [r, g, b, a] = at(x, y).unwrap_or_default();
    let l = 0.299 * r + 0.587 * g + 0.114 * b;
    let f = if l > 1e-6 { ((l * levels).round() / levels).clamp(0.0, 1.0) / l } else { 0.0 };
    let c = [(r * f).min(1.0), (g * f).min(1.0), (b * f).min(1.0)];
    let n = [(0, -1), (-1, 0), (1, 0), (0, 1)].iter().filter_map(|(dx, dy)| at(x + dx, y + dy)).fold([0.0; 4], |m, p| if p[3] > m[3] { p } else { m });
    let o = n[3] * edge * (1.0 - a);
    let total = a + o;
    if total <= 0.0 {
        return [0.0; 4];
    }
    // straight colour of the shape over the outline (the neighbour's colour, deep)
    std::array::from_fn(|j| if j == 3 { total } else { (c[j] * a + n[j] * 0.3 * o) / total })
}

/// Where a picture's main lines start, as parts of its width and height: the left edge of its fullest
/// column's line and the top edge of its fullest row's, each where its coverage falls under half.
pub(super) fn line_edges(s: &ImageSurface) -> (f64, f64) {
    let (w, h) = (s.width().max(1) as usize, s.height().max(1) as usize);
    let (mut cols, mut rows) = (vec![0.0f64; w], vec![0.0f64; h]);
    let s = s.clone();
    let _ = s.with_data(|d| {
        let stride = d.len() / h;
        for y in 0..h {
            for x in 0..w {
                let a = d[y * stride + x * 4 + 3] as f64 / 255.0;
                cols[x] += a;
                rows[y] += a;
            }
        }
    });
    // from the fullest, back over the line's full ones; the one before it partly covered
    let start = |v: &[f64]| {
        let (top, peak) = v.iter().enumerate().fold((0, 0.0), |b, (i, &x)| if x > b.1 { (i, x) } else { b });
        if peak <= 0.0 {
            return 0.0;
        }
        let mut i = top;
        while i > 0 && v[i - 1] >= 0.5 * peak {
            i -= 1;
        }
        let part = if i > 0 { (v[i - 1] / peak).min(1.0) } else { 0.0 };
        (i as f64 - part) / v.len() as f64
    };
    (start(&cols), start(&rows))
}

/// A picture at half its size, each pixel the mean of four.
pub(super) fn halved(s: &ImageSurface) -> ImageSurface {
    let (w, h) = ((s.width() / 2).max(1), (s.height() / 2).max(1));
    let out = surface(w, h);
    {
        let c = ctx(&out);
        c.scale(w as f64 / s.width().max(1) as f64, h as f64 / s.height().max(1) as f64);
        if c.set_source_surface(s, 0.0, 0.0).is_ok() {
            c.source().set_filter(cairo::Filter::Good);
            let _ = c.paint();
        }
    }
    out
}

/// The largest a sprite's pixel art is made, in the low frame's pixels.
pub(super) const ART_MAX: f64 = 4096.0;

/// A sprite's picture tinted as the GPU tints it: each texel (premultiplied) times the tint, which goes
/// from `top` (colour, alpha) at row `v0` of the picture to `foot` at row `v1` (fractions of its height).
pub(super) fn tinted(s: &ImageSurface, top: (Rgb, f64), foot: (Rgb, f64), v0: f64, v1: f64) -> ImageSurface {
    let (w, h) = (s.width(), s.height());
    let mut out = surface(w, h);
    let (stride, ostride) = (s.stride() as usize, out.stride() as usize);
    let mut src = vec![];
    let _ = s.with_data(|d| src = d.to_vec());
    let pre = |(c, a): (Rgb, f64)| [c.2 * a, c.1 * a, c.0 * a, a];
    let (t, f) = (pre(top), pre(foot));
    if let Some(mut d) = view(&mut out) {
        for y in 0..h as usize {
            let k = (((y as f64 + 0.5) / h as f64 - v0) / (v1 - v0)).clamp(0.0, 1.0);
            let tint: [f64; 4] = std::array::from_fn(|i| t[i] + (f[i] - t[i]) * k);
            for x in 0..w as usize {
                let (i, o) = (y * stride + x * 4, y * ostride + x * 4);
                for ch in 0..4 {
                    d[o + ch] = (src[i + ch] as f64 * tint[ch]).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

/// The low frame as the CRT shows it, near enough for a still: each of its pixels `pix` square on the
/// screen, its rows a beam, bright at their middle and dark between, as bright overall.
pub(super) fn beams(low: &ImageSurface, s: &mut ImageSurface, pix: usize) {
    let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
    let lstride = low.stride() as usize;
    let mut src = vec![];
    let _ = low.with_data(|d| src = d.to_vec());
    let Some(mut d) = view(s) else { return };
    let beam: Vec<f64> = (0..pix).map(|y| (-8.0 * ((y as f64 + 0.5) / pix as f64 - 0.5).powi(2)).exp2()).collect();
    let mean = beam.iter().sum::<f64>() / pix as f64;
    for cy in 0..h.div_ceil(pix) {
        for cx in 0..w.div_ceil(pix) {
            let at = cy * lstride + cx * 4;
            let px = [src[at], src[at + 1], src[at + 2], src[at + 3]];
            for y in cy * pix..((cy + 1) * pix).min(h) {
                let k = beam[y - cy * pix] / mean;
                let row = [0, 1, 2].map(|i| (px[i] as f64 * k).round().min(255.0) as u8);
                for x in cx * pix..((cx + 1) * pix).min(w) {
                    d[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&[row[0], row[1], row[2], px[3]]);
                }
            }
        }
    }
}

/// A picture made pixel art, `w` x `h`: drawn down to that size, its edges hard; lit from the top left
/// (its edges there a step lighter, at the bottom right a step darker), its light in `levels` steps,
/// a dark outline round it; leaves in clusters of lighter and darker pixels.
pub(super) fn pixel_art(src: &ImageSurface, w: i32, h: i32, levels: f64, leaves: bool, car: bool) -> ImageSurface {
    let clusters = ((w.max(1) / 2) as f64, (h.max(1) / 2) as f64);
    pixel_art_with(src, w, h, levels, leaves.then_some(clusters), car, None, cairo::Filter::Best)
}

/// As `pixel_art`, drawn down with `filter` (Good is as fine from a picture at most twice the size), its
/// leaves' clusters `leaves` across and down: the same clusters, in the same places, at every size.
/// `lines`: a picture of lines (a pole), moved by that many pixels (`Sprite::rung_size`), each pixel as
/// much of it as the picture covers there (`lined`), and the height its art is made for.
pub(super) fn pixel_art_with(
    src: &ImageSurface,
    w: i32,
    h: i32,
    levels: f64,
    leaves: Option<(f64, f64)>,
    car: bool,
    lines: Option<((f64, f64), f64)>,
    filter: cairo::Filter,
) -> ImageSurface {
    let (w, h) = (w.max(1), h.max(1));
    let mut small = surface(w, h);
    {
        let c = ctx(&small);
        if let Some(((dx, dy), _)) = lines {
            c.translate(dx, dy);
        }
        c.scale(w as f64 / src.width().max(1) as f64, h as f64 / src.height().max(1) as f64);
        if c.set_source_surface(src, 0.0, 0.0).is_ok() {
            c.source().set_filter(filter);
            let _ = c.paint();
        }
    }
    let (wu, hu, stride) = (w as usize, h as usize, small.stride() as usize);
    let Some(mut d) = view(&mut small) else { return small };
    // straight colours, 0 to 1, and whether each pixel is there
    let px: Vec<[f64; 4]> = (0..wu * hu)
        .map(|i| {
            let at = (i / wu) * stride + (i % wu) * 4;
            let a = d[at + 3] as f64 / 255.0;
            let un = |v: u8| if a > 0.0 { (v as f64 / 255.0 / a).min(1.0) } else { 0.0 };
            [un(d[at + 2]), un(d[at + 1]), un(d[at]), a]
        })
        .collect();
    // a pixel is there where it is half covered
    let there: Vec<bool> = px.iter().map(|p| p[3] >= 0.5).collect();
    let solid = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < wu && (y as usize) < hu && there[y as usize * wu + x as usize];
    // the outline, a pixel thick; about a picture of lines, as thick for its size as at the height its
    // art is made for (`near`), a pixel there: a pixel at every size made a far pole a post, fattest
    // for its size halfway, that slimmed as it neared
    let edge = lines.map_or(1.0, |(_, near)| (h as f64 / near.max(1.0)).min(1.0));
    for y in 0..hu as i64 {
        for x in 0..wu as i64 {
            let i = y as usize * wu + x as usize;
            let out = if lines.is_some() {
                lined(&px, wu, hu, x, y, levels, edge)
            } else if solid(x, y) {
                let mut k = 1.0;
                // leaves: here and there a cluster a level darker, the same at every size of the picture,
                // whatever light the cluster's pixels have there
                let mut step = 0.0;
                if let Some((across, down)) = leaves {
                    let cell = |p: i64, n: i32, k: f64| ((p as f64 + 0.5) / n as f64 * k).floor() as i64;
                    let v = hash(0, cell(x, w, across) * 7919 + cell(y, h, down) * 104_729, 91);
                    step = if v < 0.12 { -1.0 } else { 0.0 };
                }
                // lit from straight above, shaded below: the same on both sides
                if !solid(x, y - 1) {
                    k *= 1.22;
                } else if !solid(x, y + 1) {
                    k *= 0.75;
                }
                let [r, g, b, _] = px[i];
                let l = 0.299 * r + 0.587 * g + 0.114 * b;
                let lq = (((l * k * levels).round() + step) / levels).clamp(0.0, 1.0);
                let f = if l > 1e-6 { lq / l } else { 0.0 };
                [(r * f).min(1.0), (g * f).min(1.0), (b * f).min(1.0), 1.0]
            } else if let Some(n) = [(0, -1), (-1, 0), (1, 0), (0, 1)]
                .iter()
                .filter(|(dx, dy)| solid(x + dx, y + dy))
                .map(|(dx, dy)| px[(y + dy) as usize * wu + (x + dx) as usize])
                .reduce(|a, b| if a[0] + a[1] + a[2] < b[0] + b[1] + b[2] { a } else { b })
            {
                // the outline: the darkest neighbour's colour, deep
                [n[0] * 0.3, n[1] * 0.3, n[2] * 0.3, edge]
            } else {
                [0.0; 4]
            };
            let at = y as usize * stride + x as usize * 4;
            let a = out[3];
            d[at..at + 4].copy_from_slice(&[
                (out[2] * a * 255.0).round() as u8,
                (out[1] * a * 255.0).round() as u8,
                (out[0] * a * 255.0).round() as u8,
                (a * 255.0).round() as u8,
            ]);
        }
    }
    if car {
        underside(&mut d, wu, hu, stride);
    }
    drop(d);
    small
}

/// A car's underside made clean: between its tyres, from its bumper's lower edge down, a flat dark band
/// with a straight foot, its tail pipes' bright rims kept; below it nothing, so its shadow shows. The
/// parts that hang under a car, drawn this small, are ragged teeth that look to scrape the road.
pub(super) fn underside(d: &mut [u8], w: usize, h: usize, stride: usize) {
    let at = |x: usize, y: usize| y * stride + x * 4;
    let solid = |d: &[u8], x: usize, y: usize| d[at(x, y) + 3] > 127;
    // the tyres' foot: the lowest row with anything in it
    let Some(foot) = (0..h).rev().find(|&y| (0..w).any(|x| solid(d, x, y))) else { return };
    // the tyres: the outermost runs two rows above their foot; between them, the underside
    let row = foot.saturating_sub(2);
    let runs: Vec<(usize, usize)> = {
        let mut v = vec![];
        let mut x = 0;
        while x < w {
            if solid(d, x, row) {
                let s = x;
                while x < w && solid(d, x, row) {
                    x += 1;
                }
                v.push((s, x));
            }
            x += 1;
        }
        v
    };
    if runs.len() < 2 {
        return;
    }
    let (left, right) = (runs[0].1, runs[runs.len() - 1].0);
    if right <= left + 2 {
        return;
    }
    // the bumper's lower edge: in the inner half between the tyres, the lowest row that is still red
    let red = |d: &[u8], x: usize, y: usize| {
        let i = at(x, y);
        let (b, g, r) = (d[i] as f64, d[i + 1] as f64, d[i + 2] as f64);
        solid(d, x, y) && r > 60.0 && r > 1.7 * g && r > 1.4 * b
    };
    let inner = left + (right - left) / 4..right - (right - left) / 4;
    let mut edges: Vec<usize> = inner.clone().filter_map(|x| (0..foot).rev().find(|&y| red(d, x, y))).collect();
    if edges.is_empty() {
        return;
    }
    edges.sort();
    let edge = edges[edges.len() / 2];
    let band_foot = foot.saturating_sub(1);
    for y in edge + 1..=foot {
        for x in left..right {
            let i = at(x, y);
            let (b, g, r) = (d[i] as f64, d[i + 1] as f64, d[i + 2] as f64);
            let chrome = solid(d, x, y) && (r + g + b) / 3.0 > 120.0 && (r - b).abs() < 40.0;
            let px = if y > band_foot {
                [0, 0, 0, 0]
            } else if chrome {
                [d[i], d[i + 1], d[i + 2], 255]
            } else {
                [26, 22, 22, 255]
            };
            d[i..i + 4].copy_from_slice(&px);
        }
    }
}
