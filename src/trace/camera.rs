//! The camera: an eye, where it looks and its lens; the ray through each pixel, and where a point of
//! the world falls on the screen.

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> V3 {
    mul(a, 1.0 / dot(a, a).sqrt())
}

/// The eye, where it looks, and its lens.
#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: V3,
    pub forward: V3,
    pub right: V3,
    pub up: V3,
    /// tan of half the vertical field of view, and the screen's width over its height.
    pub tan: f64,
    pub aspect: f64,
    pub w: usize,
    pub h: usize,
}

impl Camera {
    pub fn new(eye: V3, yaw: f64, pitch: f64, fov: f64, w: usize, h: usize) -> Camera {
        let forward = [pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()];
        let right = norm(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        Camera { eye, forward, right, up, tan: (fov.to_radians() / 2.0).tan(), aspect: w as f64 / h as f64, w, h }
    }
    /// The ray through a point of the screen, in pixels from its top left.
    pub fn ray(&self, px: f64, py: f64) -> V3 {
        Self::through(self.column(px), self.row(py))
    }
    /// What the rays through the pixels at `px` across share (`ray`, in parts, the same to the bit).
    pub fn column(&self, px: f64) -> V3 {
        add(self.forward, mul(self.right, (2.0 * px / self.w as f64 - 1.0) * self.tan * self.aspect))
    }
    /// What the rays through the pixels at `py` down share.
    pub fn row(&self, py: f64) -> V3 {
        mul(self.up, (1.0 - 2.0 * py / self.h as f64) * self.tan)
    }
    /// The ray of a column and a row.
    pub fn through(column: V3, row: V3) -> V3 {
        norm(add(column, row))
    }
    /// The angle one pixel spans.
    pub fn pixel(&self) -> f64 {
        2.0 * self.tan / self.h as f64
    }
    /// Where a point of the world falls on the screen, in pixels from its top left, and how many pixels a
    /// block spans there; None behind the eye.
    pub fn project(&self, p: V3) -> Option<(f64, f64, f64)> {
        let d = add(p, mul(self.eye, -1.0));
        let z = dot(d, self.forward);
        if z < 0.2 {
            return None;
        }
        let (x, y) = (dot(d, self.right) / z, dot(d, self.up) / z);
        let (w, h) = (self.w as f64, self.h as f64);
        Some(((x / (self.tan * self.aspect) + 1.0) * w / 2.0, (1.0 - y / self.tan) * h / 2.0, h / (2.0 * self.tan * z)))
    }
}

/// A rectangle of the screen, in pixels: left, top, right, bottom (exclusive).
pub type Px = (i32, i32, i32, i32);

/// Where some points of the world fall on the screen: the rectangle about them (left, top, right,
/// bottom), None if all are behind the eye; and whether all of them are in front of it. Only then does
/// the rectangle hold all that lies between them.
pub fn bounds(cam: &Camera, points: impl IntoIterator<Item = V3>) -> (Option<(f64, f64, f64, f64)>, bool) {
    let mut rect = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut all = true;
    for p in points {
        match cam.project(p) {
            Some((sx, sy, _)) => rect = (rect.0.min(sx), rect.1.min(sy), rect.2.max(sx), rect.3.max(sy)),
            None => all = false,
        }
    }
    ((rect.0 <= rect.2).then_some(rect), all)
}

/// The pixels a rectangle of the screen touches, and one more all round: whatever a ray through a
/// pixel's middle meets inside it, rounding included.
pub fn pixels(r: (f64, f64, f64, f64)) -> Px {
    (r.0.floor() as i32 - 1, r.1.floor() as i32 - 1, r.2.ceil() as i32 + 1, r.3.ceil() as i32 + 1)
}

/// The pixels whose middles lie in a rectangle of the screen: those a ray through a pixel's middle can
/// meet anything inside it through. Give it the rectangle of a box grown a little (`boxes::PAD`), so
/// rounding never leaves out a pixel that meets the box itself.
pub fn centres(r: (f64, f64, f64, f64)) -> Px {
    const E: f64 = 1e-6;
    ((r.0 - 0.5 - E).ceil() as i32, (r.1 - 0.5 - E).ceil() as i32, (r.2 - 0.5 + E).floor() as i32 + 1, (r.3 - 0.5 + E).floor() as i32 + 1)
}

/// The outline of points of the screen: their convex hull, counter-clockwise.
pub fn hull(mut points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let turn = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut out: Vec<(f64, f64)> = vec![];
    for pass in 0..2 {
        let start = out.len();
        let pts: Vec<(f64, f64)> = if pass == 0 { points.clone() } else { points.iter().rev().copied().collect() };
        for p in pts {
            while out.len() >= start + 2 && turn(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// The pixels of row `y` whose middles lie in an outline (`hull`), a little over: left, right
/// (exclusive); None where none do.
pub fn span(outline: &[(f64, f64)], y: i32) -> Option<(i32, i32)> {
    const E: f64 = 1e-6;
    let yc = y as f64 + 0.5;
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        if (a.1 - yc) * (b.1 - yc) > 0.0 {
            continue;
        }
        let x = if (b.1 - a.1).abs() < 1e-12 { a.0.min(b.0) } else { a.0 + (yc - a.1) * (b.0 - a.0) / (b.1 - a.1) };
        let x2 = if (b.1 - a.1).abs() < 1e-12 { a.0.max(b.0) } else { x };
        lo = lo.min(x);
        hi = hi.max(x2);
    }
    let (x0, x1) = ((lo - 0.5 - E).ceil() as i32, (hi - 0.5 + E).floor() as i32 + 1);
    (lo <= hi && x0 < x1).then_some((x0, x1))
}

/// Where two rectangles of pixels overlap; None where they do not.
pub fn overlap(a: Px, b: Px) -> Option<Px> {
    let r = (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3));
    (r.0 < r.2 && r.1 < r.3).then_some(r)
}
