//! The projection, the hills, the road ahead row by row, and the drive: the road's bends and the car's
//! place as functions of time.

use super::*;

// --- the projection -------------------------------------------------------------------------------

// A camera that adds up: it is a viewer's height (VIEWER, in metres) over the road, right behind the
// car, looking along the road; lengths are in those heights. The car is Z_CAR of them ahead of it, so
// it is seen from atan(1 / Z_CAR) = 6 degrees above, the height its middle view is drawn from; the
// lens (FOCAL) keeps the road as wide on the screen as it was with a larger car; the horizon is low on
// the screen.

pub(super) const CANVAS: (f64, f64) = (1440.0, 900.0);
/// The horizon's row on the canvas.
pub(super) const HORIZON: f64 = 591.0;
/// The lens: canvas pixels for a thing one viewer's height across, one viewer's height away.
pub(super) const FOCAL: f64 = 1888.0;
/// The viewer's height over the road, in metres: just over the car's roof.
pub(super) const VIEWER: f64 = 1.5;
/// The car's rear wheels' depth.
pub(super) const Z_CAR: f64 = 9.51;
/// The car's width, a real one's 1.97 metres.
pub(super) const CAR_W: f64 = 1.97 / VIEWER;
/// The road's half width: one lane, the car under two thirds of it.
pub(super) const ROAD_HALF: f64 = CAR_W / 0.65;
/// The curb's outer edge, in half widths.
pub(super) const CURB: f64 = 1.055;
/// The nearest depth anything is placed at, under the canvas's foot, and the farthest.
pub(super) const NEAR: f64 = 4.0;
pub(super) const FAR: f64 = 110.0;
/// The farthest the road is walked.
pub(super) const Z_END: f64 = 400.0;

/// Where the road vanishes, across the canvas: right above the car.
pub(super) const VANISH: f64 = 720.0;
/// Where the viewer is across the road, in half widths from its centre: over the right-hand lane.
pub(super) const VIEW: f64 = 0.5;

/// Canvas pixels to the road's half width, at depth z: sprites' sizes are in these.
pub(super) fn scale(z: f64) -> f64 {
    FOCAL * ROAD_HALF / z
}

/// Where a point `x` road half widths from the straight line ahead (right positive) is across the
/// canvas, at depth z.
pub(super) fn road_x(z: f64, x: f64) -> f64 {
    VANISH + (x - VIEW) * scale(z)
}

/// The row on the canvas of a point at depth z standing `dh` above the level of the road at the car (in
/// the viewer's heights).
pub(super) fn row_of(z: f64, dh: f64) -> f64 {
    HORIZON + FOCAL * (1.0 - dh) / z
}

/// How near a depth is, as the level road's rows go: 1 at the canvas's foot, 0 at the horizon. For
/// what fades with distance.
pub(super) fn nearness(z: f64) -> f64 {
    (FOCAL / z / (CANVAS.1 - HORIZON)).clamp(0.0, 1.5)
}

/// The car's width on the canvas.
pub(super) const CAR_WIDTH: f64 = FOCAL * CAR_W / Z_CAR;
/// Its length, a real one's 4.36 metres: its nose is this far ahead of its rear wheels' depth.
pub(super) const NOSE: f64 = 4.36 / VIEWER;
/// Its headlamps' distance either side of its middle, in road half widths: four tenths of its width.
pub(super) const LAMPS: f64 = 0.4 * CAR_W / ROAD_HALF;

// --- the hills --------------------------------------------------------------------------------------

/// Each stretch of road this long holds one hill or dip, or none.
pub(super) const HILL_CELL: f64 = 220.0;

/// The hill or dip in stretch k: where it starts, its length, and its height (in the viewer's heights
/// over the road; a dip below 0): high and long, so the road is seen climbing and dropping far ahead.
/// None for a level stretch.
pub(super) fn hill(seed: i64, k: i64) -> Option<(f64, f64, f64)> {
    if k < 1 || hash(seed, k, 81) > 0.75 {
        return None;
    }
    let len = 130.0 + 70.0 * hash(seed, k, 82);
    let at = k as f64 * HILL_CELL + (HILL_CELL - len) * hash(seed, k, 83);
    let high = 2.2 + 2.6 * hash(seed, k, 84);
    Some((at, len, if hash(seed, k, 85) < 0.65 { high } else { -0.75 * high }))
}

/// The road's height at distance s, and its slope: smooth hills and dips, each a raised cosine.
pub(super) fn height(seed: i64, s: f64) -> (f64, f64) {
    height_on(s, hill(seed, (s / HILL_CELL).floor() as i64))
}

/// The road's height at s, on the hill of its cell, as `height_on` has it, without its slope.
pub(super) fn height_only(s: f64, hill: Option<(f64, f64, f64)>) -> f64 {
    match hill {
        Some((at, len, high)) if (at..at + len).contains(&s) => high * (1.0 - (2.0 * PI * ((s - at) / len)).cos()) / 2.0,
        _ => 0.0,
    }
}

/// The road's height and slope at s, on the hill of its cell (`hill`).
pub(super) fn height_on(s: f64, hill: Option<(f64, f64, f64)>) -> (f64, f64) {
    match hill {
        Some((at, len, high)) if (at..at + len).contains(&s) => {
            let u = (s - at) / len;
            (high * (1.0 - (2.0 * PI * u).cos()) / 2.0, high * PI / len * (2.0 * PI * u).sin())
        }
        _ => (0.0, 0.0),
    }
}

// --- the road ahead, row by row ------------------------------------------------------------------

/// How many rows the road's table has, over the canvas from ROWS_TOP to its foot: finer than the
/// coarsest grid of pixels.
pub(super) const ROWS: usize = 512;
/// The highest the road can be seen: a hill ahead rises over the horizon.
pub(super) const ROWS_TOP: f64 = HORIZON - 260.0;

/// The road's table as the road's shader reads it: two rows a vec4 of uniforms, each its depth and
/// offset codes as whole numbers; or, where a driver has too few uniforms for that (`TABLE_ROOM`, with
/// the shader's others), a texture a row high, each row's codes in its four bytes.
pub(super) const TABLE_UNIFORMS: (&str, &str) = ("uniform vec4 table[256];", "vec4 q = table[int(r) / 2]; vec2 e2 = mod(r, 2.0) < 0.5 ? q.xy : q.zw;");
pub(super) const TABLE_TEXTURE: (&str, &str) =
    ("", "vec4 e = floor(texture2D(tex0, vec2((r + 0.5) / rows.z, 0.5)) * 255.0 + 0.5); vec2 e2 = vec2(e.r * 256.0 + e.g, e.b * 256.0 + e.a);");
pub(super) const TABLE_ROOM: i32 = 256 + 64;
const _: () = assert!(ROWS == 2 * 256, "TABLE_UNIFORMS holds two rows a vec4");

pub(super) fn row_height() -> f64 {
    (CANVAS.1 - ROWS_TOP) / ROWS as f64
}

/// What a row of the screen shows: sky (nothing), the land beyond the road's end, or the road at a
/// depth, with its offset across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Row {
    Sky,
    Beyond,
    Road { z: f64, x: f64 },
}

/// The road walked from the car to the horizon: each step's depth, row, offset across and the row
/// above which nearer road hides it (a crest), and what each row of the screen shows.
pub(super) struct Ahead {
    pub(super) steps: Vec<(f64, f64, f64, f64)>,
    pub(super) rows: Vec<Row>,
    /// The car's slope against the viewer's, and its turn: what picks its view.
    pub(super) pitch: f64,
    pub(super) yaw: f64,
    /// The backdrop's rise on the canvas: the car climbing tilts the view up, and the horizon down.
    pub(super) rise: f64,
}

impl Ahead {
    pub(super) fn level() -> Ahead {
        Ahead { steps: vec![], rows: vec![Row::Sky; ROWS], pitch: 0.0, yaw: 0.0, rise: 0.0 }
    }

    /// Where a thing at depth z stands: its row, its offset across, and the row below which the road
    /// in front of it hides it. None beyond the road walked.
    pub(super) fn at(&self, z: f64) -> Option<(f64, f64, f64)> {
        let i = self.steps.partition_point(|s| s.0 < z);
        if i == 0 || i >= self.steps.len() {
            return None;
        }
        let (a, b) = (self.steps[i - 1], self.steps[i]);
        let t = (z - a.0) / (b.0 - a.0);
        Some((a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t, a.3))
    }
}
/// A number in [0, 1) for a key: the same on every start and every machine.
pub(super) fn hash(seed: i64, k: i64, salt: u64) -> f64 {
    crate::rng::keyed(seed, k, salt)
}

/// A picture `period` canvas pixels wide, repeating, moved `offset` across, over the canvas's width:
/// its pieces as (x0, x1, u0, u1), each within one repeat of the picture.
pub(super) fn wrapped(period: f64, offset: f64) -> Vec<(f64, f64, f64, f64)> {
    let mut out = vec![];
    let mut x = 0.0;
    while x < CANVAS.0 {
        // rem_euclid of a tiny negative gives the period itself: that is the start of a repeat
        let at = Some((x - offset).rem_euclid(period)).filter(|&a| a < period).unwrap_or(0.0);
        let end = CANVAS.0.min(x + period - at).max(x + 1e-6);
        out.push((x, end, at / period, (at + end - x) / period));
        x = end;
    }
    out
}

/// Smooth value noise in [-1, 1] over x.
pub(super) fn noise(seed: i64, salt: u64, x: f64) -> f64 {
    let (i, f) = (x.floor() as i64, x - x.floor());
    let (a, b) = (hash(seed, i, salt), hash(seed, i + 1, salt));
    2.0 * (a + (b - a) * smooth(f)) - 1.0
}

// --- the drive: the road's bends and the car's place, as functions of time ------------------------

/// The length of each piece of road, in units of depth, and of the ease from one piece's curvature
/// into the next, at the piece's start.
pub(super) const PIECE: f64 = 60.0;
pub(super) const EASE: f64 = 25.0;
/// How far the backdrop turns across, in canvas pixels, per unit of the road's turn (its curvature
/// summed along it): as far as the angle it turns, through the lens. A right-hand bend carries the sun,
/// the hills and the clouds to the left, while the car is in it.
pub(super) const TURN: f64 = FOCAL * ROAD_HALF;
/// How far the backdrop may be turned either way: the sun and the moon stay on the canvas. A bend that
/// would carry it further is not taken; the bends keep it within this, the next going back as often as
/// on, so two right-hand bends can follow a left-hand one.
pub(super) const TURNED: f64 = 380.0;

/// The curvature `tau` into a piece easing from `from` to `to`, and its sum over that length.
pub(super) fn piece_at(from: f64, to: f64, tau: f64) -> (f64, f64) {
    let e = EASE;
    if tau >= e {
        (to, from * e + (to - from) * e / 2.0 + to * (tau - e))
    } else {
        // the smoothstep's sum: u^3 - u^4 / 2
        let u = tau / e;
        (from + (to - from) * smooth(u), from * tau + (to - from) * e * (u * u * u - u * u * u * u / 2.0))
    }
}

/// The road, piece by piece along its length: each piece's curvature (right positive), and the
/// backdrop's turn at its end. A bend lies on the road: it is seen far ahead, comes nearer as the car
/// drives on, and turns the backdrop only while the car is in it. A piece's curvature is drawn from its
/// seed and from where the backdrop stands: about half the pieces straight, the rest gently left or
/// right, back towards the middle more often the further it is turned, and never so far as to carry it
/// past TURNED. A piece often runs on as the one before it, so bends and straights are of many lengths.
/// Each piece follows from the one before it, so the road is made once, as far as it has been seen,
/// and kept.
pub(super) struct Course {
    pub(super) seed: i64,
    pub(super) pieces: Vec<(f64, f64)>,
}

impl Course {
    pub(super) fn new(seed: i64) -> Course {
        Course { seed, pieces: vec![(0.0, 0.0)] }
    }

    /// Piece i: its curvature, and the backdrop's turn at its end (in canvas pixels, right positive).
    pub(super) fn piece(&mut self, i: usize) -> (f64, f64) {
        while self.pieces.len() <= i {
            let n = self.pieces.len() as i64;
            let (from, turned) = *self.pieces.last().unwrap_or(&(0.0, 0.0));
            let h = |salt| hash(self.seed, n, salt);
            let mut to = if h(11) < 0.35 {
                from
            } else if h(12) < 0.45 {
                0.0
            } else {
                // right with an even chance in the middle, more likely back when turned: the backdrop
                // carried left (by right-hand bends) asks for a left-hand bend
                let right = 0.5 + 0.45 * (turned / TURNED).clamp(-1.0, 1.0);
                let size = 0.0005 + 0.0007 * h(13);
                if h(14) < right { size } else { -size }
            };
            // the turn this piece would leave, and the most the next can add while it eases out of it
            let end = |to: f64| turned - TURN * (piece_at(from, to, PIECE).1 + to * EASE / 2.0);
            if end(to).abs() > TURNED {
                to = 0.0;
            }
            let after = turned - TURN * piece_at(from, to, PIECE).1;
            self.pieces.push((to, after));
        }
        self.pieces[i]
    }

    /// At distance s along the road: the curvature alone, as `at` has it.
    pub(super) fn curvature(&mut self, s: f64) -> f64 {
        let s = s.max(0.0);
        let i = (s / PIECE).floor() as usize;
        let from = if i == 0 { 0.0 } else { self.piece(i - 1).0 };
        let to = self.piece(i).0;
        let tau = s - i as f64 * PIECE;
        if tau >= EASE { to } else { from + (to - from) * smooth(tau / EASE) }
    }

    /// At distance s along the road: the curvature, and the backdrop's turn.
    pub(super) fn at(&mut self, s: f64) -> (f64, f64) {
        let s = s.max(0.0);
        let i = (s / PIECE).floor() as usize;
        let (from, turned) = if i == 0 { (0.0, 0.0) } else { self.piece(i - 1) };
        let (to, _) = self.piece(i);
        let (c, sum) = piece_at(from, to, s - i as f64 * PIECE);
        (c, turned - TURN * sum)
    }
}

/// The car's place across the road at time t, in half widths from the centre line: the middle of the
/// right-hand lane, wandering slowly a little to either side, never near its edges.
pub(super) fn lane_at(seed: i64, t: f64) -> f64 {
    VIEW + 0.06 * noise(seed, 31, t / 4.7) + 0.025 * noise(seed, 32, t / 1.9)
}

/// The car's bump at road distance s, in canvas pixels, up: a little, as an old racer's car jiggles.
pub(super) fn bump_at(seed: i64, s: f64) -> f64 {
    let fine = noise(seed, 41, s * 0.68);
    let coarse = noise(seed, 42, s * 0.18);
    // now and then a pothole: a jolt that dies away within a few tenths of a second
    let (i, f) = ((s * 0.24).floor() as i64, s * 0.24 - (s * 0.24).floor());
    let jolt = if hash(seed, i, 43) < 0.18 { (-f * 10.0).exp() * (f * 34.0).sin() } else { 0.0 };
    0.9 * fine + 0.7 * coarse.abs() + 1.4 * jolt
}
