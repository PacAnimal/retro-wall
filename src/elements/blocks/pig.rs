//! The pig that `pig = true` lets loose on the farm: a pig that keeps to no ground and no physics, but
//! never goes through anything. It walks in a straight line over whatever is in its way: up a wall,
//! upside down under the eaves, round their edge and over the roof, down the far side; along the
//! bottom of the stream and up the other bank. It climbs any house's walls from any side, wanders
//! between the houses, treks up to the lodge in the mountains, swims in the pond, jumps from roof to
//! roof. It runs in circles, does the zoomies, flips, headstands, naps, begs and stares at the viewer,
//! and peeks up into their face. It flies many ways. It goes up in a puff of smoke and comes down in
//! another somewhere else. It tumbles down the mountains. Most of all, it eats the crops (`DOINGS`).
//!
//! Whatever it does off the faces it walks on is checked against the blocks first, the whole of its
//! body all the way (`body_clear`): a hop, a flight, a way of holding itself. What would go through
//! something is not done; a step that would put it in a block anyway drops it straight down instead.
//!
//! It is one of the herd's moving things (`herd`), traced over the picture the same way, and it steps
//! on the scene's clock, so a moment is always the same. What it walks on is the world's blocks about
//! the farm, kept as what it can stand on (`Map`); where it is put down, the places the camera sees.

use super::mobs::{self, Kind, Walk};
use super::world::*;
use crate::rng::Rng;
use crate::trace::camera::{Camera, V3, add, bounds, cross, dot, mul, norm, overlap, pixels};
use crate::trace::rig::Pose;
use std::f64::consts::{PI, TAU};

/// A step of the pig, in seconds: it is quick, so it steps more often than the herd.
pub const STEP: f64 = 1.0 / 30.0;
/// How far about the farm's middle it knows the world, across the ground, in blocks: out to the
/// lodge in the mountains on the left.
const REACH: i32 = 96;
/// Half its height: the middle it turns head over heels about.
const MIDDLE: f64 = 0.45;
/// How fast it falls in a hop, blocks a second a second: its hops float a little.
const GRAVITY: f64 = 20.0;
/// How long a puff of smoke lasts, in seconds.
pub const PUFF: f64 = 1.3;
/// Its pace walking on anything, and running in circles, in blocks a second.
const CRAWL: f64 = 1.6;
const RUN: f64 = 2.4;
/// Its fastest across the ground in a hop, blocks a second.
const HOP: f64 = 3.0;
/// How high over where it comes down it drops out of the sky from, in blocks.
const FALL: f64 = 16.0;
/// How often it comes down on a roof, when it comes down in smoke other than to eat.
const ROOF: f64 = 0.25;
/// Its pace flying by, blocks a second.
const FLY: f64 = 4.0;
/// Its fastest tumbling down a slope, blocks a second.
const ROLL: f64 = 4.5;
/// How near the eye, across the ground, it is put down at the nearest, in blocks.
const NEAREST: f64 = 14.0;
/// Half its body's width, and its height, in blocks.
const HALF_W: f64 = 5.0 / 16.0;
const TALL: f64 = 15.0 / 16.0;

/// What a cell is to the pig: open air (or a plant, a torch, a rail), something to walk on, water, or
/// a crop to eat.
const OPEN: u8 = 0;
const SOLID: u8 = 1;
const WET: u8 = 2;
const CROP: u8 = 3;

fn class(b: u8) -> u8 {
    match block(b).shape {
        Shape::Cube | Shape::Cutout => SOLID,
        Shape::Boxes if !matches!(b, TORCH | LANTERN | RAIL | RAIL_Z | LILY) => SOLID,
        Shape::Water => WET,
        Shape::Crop => CROP,
        _ => OPEN,
    }
}

/// The world about the farm as the pig knows it: each cell's class, and each column's top (the row
/// over its highest solid block).
pub struct Map {
    x0: i32,
    z0: i32,
    n: i32,
    cells: Vec<u8>,
    tops: Vec<i32>,
}

const ROWS: i32 = Y1 - Y0;

impl Map {
    fn new(w: &World, centre: (i32, i32)) -> Map {
        Map::from_fn(centre.0 - REACH, centre.1 - REACH, 2 * REACH + 1, |x, y, z| class(w.get(x, y, z)))
    }

    /// `n` columns by `n` from (x0, z0), each cell's class from `at`.
    fn from_fn(x0: i32, z0: i32, n: i32, at: impl Fn(i32, i32, i32) -> u8) -> Map {
        let mut cells = vec![OPEN; (n * n * ROWS) as usize];
        let mut tops = vec![Y0; (n * n) as usize];
        for cx in 0..n {
            for cz in 0..n {
                let col = (cx * n + cz) as usize;
                for y in Y0..Y1 {
                    let c = at(x0 + cx, y, z0 + cz);
                    cells[col * ROWS as usize + (y - Y0) as usize] = c;
                    if c == SOLID {
                        tops[col] = y + 1;
                    }
                }
            }
        }
        Map { x0, z0, n, cells, tops }
    }

    fn column(&self, x: i32, z: i32) -> Option<usize> {
        let (cx, cz) = (x - self.x0, z - self.z0);
        (cx >= 0 && cz >= 0 && cx < self.n && cz < self.n).then_some((cx * self.n + cz) as usize)
    }

    fn at(&self, c: [i32; 3]) -> u8 {
        if c[1] < Y0 {
            return SOLID;
        }
        match self.column(c[0], c[2]) {
            Some(col) if c[1] < Y1 => self.cells[col * ROWS as usize + (c[1] - Y0) as usize],
            _ => OPEN,
        }
    }

    fn solid(&self, c: [i32; 3]) -> bool {
        self.at(c) == SOLID
    }

    /// Whether a cell is one it may walk into: known, and clear of the world's floor and ceiling.
    fn within(&self, c: [i32; 3]) -> bool {
        self.column(c[0], c[2]).is_some() && c[1] > Y0 && c[1] < Y1 - 2
    }

    /// The row over the highest solid block of the column under a point.
    fn top(&self, x: f64, z: f64) -> i32 {
        self.column(x.floor() as i32, z.floor() as i32).map_or(GROUND, |c| self.tops[c])
    }
}

/// Walking on whatever is there: the open cell its body is in, the way out of the face it walks on
/// (`up`), the way it walks (`fwd`), and how far it is along the face from the cell's middle, -0.5 to
/// 0.5; each a whole block's way along an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Crawl {
    cell: [i32; 3],
    up: [i32; 3],
    fwd: [i32; 3],
    along: f64,
}

fn iadd(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn ineg(a: [i32; 3]) -> [i32; 3] {
    [-a[0], -a[1], -a[2]]
}
fn f3(a: [i32; 3]) -> V3 {
    a.map(f64::from)
}

impl Crawl {
    /// `dist` blocks further on, keeping to the surface: into a wall, it turns up it; off an edge, it
    /// turns down round it; at the edge of what it knows, it turns back.
    fn on(mut self, map: &Map, dist: f64) -> Crawl {
        self.along += dist;
        // a few cells a step at most: a step is a fraction of a block
        for _ in 0..8 {
            if self.along < 0.5 {
                break;
            }
            let ahead = iadd(self.cell, self.fwd);
            let under = iadd(ahead, ineg(self.up));
            if self.at_the_end(map) {
                // back the way it came, from the cell's edge
                self.fwd = ineg(self.fwd);
                self.along -= 1.0;
            } else if map.solid(ahead) {
                // a wall before it: up it, the floor now behind it
                (self.up, self.fwd) = (ineg(self.fwd), self.up);
                self.along -= 1.0;
            } else if !map.solid(under) {
                // an edge: round it and down its side
                self.cell = under;
                (self.up, self.fwd) = (self.fwd, ineg(self.up));
                self.along -= 1.0;
            } else {
                self.cell = ahead;
                self.along -= 1.0;
            }
        }
        self
    }

    /// Where its feet are: on the face, under its body.
    fn feet(&self) -> V3 {
        let c = add(f3(self.cell), [0.5; 3]);
        add(add(c, mul(f3(self.up), -0.5)), mul(f3(self.fwd), self.along))
    }

    /// Whether the next cell on is past what it knows, so it turns back there.
    fn at_the_end(&self, map: &Map) -> bool {
        let ahead = iadd(self.cell, self.fwd);
        !map.within(ahead) || !map.within(iadd(ahead, ineg(self.up)))
    }

    /// The same place, facing the other way.
    fn back(self) -> Crawl {
        Crawl { fwd: ineg(self.fwd), along: -self.along, ..self }
    }

    /// How its body lies with its feet here: its feet, the way out of its back and the way it points.
    /// The body is stiff and longer than a block, so it cannot bend round a corner with its feet: its
    /// hind end is on the way it walks, and it points to where that way is a body's length off in a
    /// straight line, then is lifted off whatever it walks on that would be inside it. Into a corner it
    /// slides up the wall like a ladder; over an edge it lies across it and tips over. Round a block
    /// shorter than itself, under the eaves, the way never gets a body's length off before it turns
    /// back: there it points to as far along as one edge would take it, and swings round the block.
    fn lie(&self, map: &Map) -> (V3, V3, V3) {
        const BIT: f64 = 1.0 / 32.0;
        const SPARE: f64 = 0.5;
        const FARTHEST: f64 = LENGTH * std::f64::consts::SQRT_2;
        let hind = self.back().on(map, HIND).back();
        let p0 = hind.feet();
        let far = |p: V3| (0..3).map(|i| (p[i] - p0[i]).powi(2)).sum::<f64>().sqrt();
        // along the way from a little behind its hind end to a little past its snout: what it walks on,
        // and where its snout points
        let mut c = hind.back().on(map, SPARE).back();
        let (mut s, mut last, mut front, mut end) = (-SPARE, c.feet(), None, FARTHEST + SPARE);
        let mut under: Vec<[i32; 3]> = vec![];
        while s < end {
            if c.at_the_end(map) {
                // it turns back here, at the edge of what it knows: the way folds over on itself
                return (self.feet(), f3(self.up), f3(self.fwd));
            }
            let cell = iadd(c.cell, ineg(c.up));
            if !under.contains(&cell) {
                under.push(cell);
            }
            let p = c.feet();
            if front.is_none() && s > 0.0 {
                let (r0, r) = (far(last), far(p));
                if r >= LENGTH || s >= FARTHEST {
                    let k = if r >= LENGTH && r - r0 > 1e-9 { (LENGTH - r0) / (r - r0) } else { 1.0 };
                    front = Some(add(last, mul(add(p, mul(last, -1.0)), k)));
                    end = s + SPARE;
                }
            }
            last = p;
            c = c.on(map, BIT);
            s += BIT;
        }
        let front = front.unwrap_or(last);
        let reach = far(front);
        let d = mul(add(front, mul(p0, -1.0)), 1.0 / reach);
        let n = cross(d, f3(icross(self.up, self.fwd)));
        let lo = dot(p0, d);
        let rest = under.iter().fold(dot(p0, n), |h, &cell| h.max(highest(cell, d, n, lo + 1e-6, lo + reach - 1e-6)));
        (add(add(p0, mul(n, rest - dot(p0, n))), mul(d, HIND)), n, d)
    }
}

/// How far its body reaches behind its feet, and its whole length to the tip of its snout.
const HIND: f64 = 0.5;
const LENGTH: f64 = 1.5;

/// Whether its body, posed so (its feet, the way out of its back and the way it points), is clear of
/// every block: points through the whole of it, a little in from its skin. Nothing it does off the
/// faces it walks on may put it inside one.
fn body_clear(map: &Map, at: V3, up: V3, fwd: V3) -> bool {
    let across = cross(up, fwd);
    for i in 0..7 {
        let z = -HIND + 0.04 + (LENGTH - 0.08) * i as f64 / 6.0;
        for j in 0..4 {
            let y = 0.05 + (TALL - 0.1) * j as f64 / 3.0;
            for x in [-HALF_W + 0.04, 0.0, HALF_W - 0.04] {
                let p = add(add(add(at, mul(fwd, z)), mul(up, y)), mul(across, x));
                if map.solid(p.map(|v| v.floor() as i32)) {
                    return false;
                }
            }
        }
    }
    true
}

/// `v` turned about the unit axis `k` by `a`.
fn turn(v: V3, k: V3, a: f64) -> V3 {
    let (c, s) = (a.cos(), a.sin());
    add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)))
}

/// What it does in a hop besides rising and falling: whole turns head over heels (`flips`, forwards),
/// rolled over sideways (`rolls`) and spun about the upright (`spins`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Trick {
    flips: f64,
    rolls: f64,
    spins: f64,
}

impl Trick {
    const NONE: Trick = Trick { flips: 0.0, rolls: 0.0, spins: 0.0 };
    fn flips(n: f64) -> Trick {
        Trick { flips: n, ..Trick::NONE }
    }
}

/// How it holds itself, standing still a while.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hold {
    /// Standing, looking: at the viewer, when it was turned to.
    Stare,
    /// On its haunches, its front up.
    Sit,
    /// Up on its hind legs.
    Beg,
    /// On its snout, its back end in the air.
    Headstand,
    /// Its head down to the viewer.
    Bow,
    /// Lying on its side, breathing slowly.
    Sleep,
    /// On its back, its legs kicking.
    Back,
    /// Shaking itself off, rocking fast from side to side.
    Shake,
    /// Its head going up and down, fast.
    Headbang,
    /// Rocking and turning slowly, giddy.
    Dizzy,
    /// Turning round on the spot.
    Spin,
}

/// The ways it flies.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Flight {
    Across,
    Swoop,
    Overhead,
    Orbit,
    Looping,
    Corkscrew,
    TakeOff,
    RoofLanding,
    Slalom,
    PondDive,
}

const FLIGHTS: [Flight; 10] = [
    Flight::Across,
    Flight::Swoop,
    Flight::Overhead,
    Flight::Orbit,
    Flight::Looping,
    Flight::Corkscrew,
    Flight::TakeOff,
    Flight::RoofLanding,
    Flight::Slalom,
    Flight::PondDive,
];

/// A point on a smooth curve through `pts` (Catmull-Rom, by the lengths between them), `k` of the way
/// along it, 0 to 1, and the way it goes there; and the curve's whole length.
fn curve(pts: &[V3], k: f64) -> (V3, V3, f64) {
    let n = pts.len();
    if n < 2 {
        return (pts.first().copied().unwrap_or([0.0; 3]), [0.0, 0.0, 1.0], 0.0);
    }
    let chords: Vec<f64> = pts.windows(2).map(|w| (0..3).map(|i| (w[1][i] - w[0][i]).powi(2)).sum::<f64>().sqrt().max(1e-6)).collect();
    let total: f64 = chords.iter().sum();
    let mut want = k.clamp(0.0, 1.0) * total;
    let mut j = 0;
    while j + 1 < chords.len() && want > chords[j] {
        want -= chords[j];
        j += 1;
    }
    let u = (want / chords[j]).clamp(0.0, 1.0);
    let p1 = pts[j];
    let p2 = pts[j + 1];
    let p0 = if j > 0 { pts[j - 1] } else { add(p1, mul(add(p2, mul(p1, -1.0)), -1.0)) };
    let p3 = if j + 2 < n { pts[j + 2] } else { add(p2, add(p2, mul(p1, -1.0))) };
    let (u2, u3) = (u * u, u * u * u);
    let at: V3 = std::array::from_fn(|i| {
        0.5 * (2.0 * p1[i] + (-p0[i] + p2[i]) * u + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * u2 + (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * u3)
    });
    let d: V3 = std::array::from_fn(|i| {
        0.5 * ((-p0[i] + p2[i]) + 2.0 * (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * u + 3.0 * (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * u2)
    });
    let d = if dot(d, d) < 1e-12 { add(p2, mul(p1, -1.0)) } else { d };
    (at, norm(d), total)
}

fn icross(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// How far along `n` the block at `cell` reaches, of its part between `lo` and `hi` along `d`; far
/// below anything when no part of it is there.
fn highest(cell: [i32; 3], d: V3, n: V3, lo: f64, hi: f64) -> f64 {
    let corner = |k: usize| [0, 1, 2].map(|i| f64::from(cell[i] + (k >> i & 1) as i32));
    let mut h = f64::MIN;
    for k in 0..8 {
        let a = corner(k);
        let da = dot(a, d);
        if (lo..=hi).contains(&da) {
            h = h.max(dot(a, n));
        }
        // where its edges cross the ends of the body
        for i in 0..3 {
            if k >> i & 1 == 1 {
                continue;
            }
            let b = corner(k | 1 << i);
            let db = dot(b, d);
            for bound in [lo, hi] {
                if (da - bound) * (db - bound) < 0.0 {
                    let t = (bound - da) / (db - da);
                    h = h.max(dot(add(a, mul(add(b, mul(a, -1.0)), t)), n));
                }
            }
        }
    }
    h
}

/// What it is doing.
enum Act {
    /// Choosing what next.
    Next,
    /// One thing after another: each started when the one before it is done.
    Seq(std::collections::VecDeque<Act>),
    Crawl {
        c: Crawl,
        /// Seconds left; past them it goes on until it is on its feet again, never stopping on a wall
        /// or under a roof.
        left: f64,
    },
    /// Running round `centre`, `turn` radians a second.
    Circle {
        centre: [f64; 2],
        r: f64,
        angle: f64,
        turn: f64,
        left: f64,
    },
    /// A hop from one place to another, up at `v` blocks a second and falling at `g` blocks a second a
    /// second, `time` seconds long, `s` into it, doing `trick` on the way. `more` hops after it, towards
    /// a place if there is one, then `then`.
    Hop {
        from: V3,
        to: V3,
        v: f64,
        g: f64,
        time: f64,
        s: f64,
        trick: Trick,
        more: u32,
        toward: Option<[i32; 3]>,
        then: Box<Act>,
    },
    /// Gone in a puff of smoke, to come down at `to`, then `then`: in another puff, or with `fall`,
    /// dropping out of the sky onto it.
    Gone {
        left: f64,
        to: [i32; 3],
        then: Box<Act>,
        fall: bool,
    },
    Eat {
        left: f64,
        /// Until it moves on to the next plant.
        shuffle: f64,
        /// Where it is turning to look, while it looks about; none while it does not.
        look: Option<V3>,
    },
    /// Turning on the spot to face `face`, at its turning pace (`Pig::turn_rate`).
    Turn {
        face: V3,
    },
    /// Rolling down a slope, `roll` radians turned so far; `flat` seconds on level ground.
    Tumble {
        dir: [f64; 2],
        speed: f64,
        roll: f64,
        flat: f64,
        left: f64,
    },
    Stand {
        left: f64,
    },
    /// Flying along a curve through `pts`, `s` seconds of `time`; rolled over `rolls` times on the way;
    /// `up` the way out of its back so far, carried along the curve. At its end, `then`; with none, it
    /// is off the screen, and comes down somewhere a little later.
    Fly {
        pts: Vec<V3>,
        s: f64,
        time: f64,
        rolls: f64,
        bob: bool,
        up: V3,
        then: Option<Box<Act>>,
    },
    /// Holding itself some way (`Hold`) where it stands, its feet at `feet`, facing `face`.
    Held {
        hold: Hold,
        left: f64,
        t: f64,
        feet: V3,
        face: V3,
    },
    /// Across the ground through `pts`, `speed` blocks a second, backwards with `back`, `pause`
    /// seconds at each (sniffing about). With `climb` it gets over what is in its way, hopping up or
    /// walking up it; without, it goes on to the next point. Given up after `left` seconds, or when it
    /// gets no nearer for a long while.
    Walk {
        pts: Vec<[f64; 2]>,
        i: usize,
        speed: f64,
        back: bool,
        pause: f64,
        wait: f64,
        climb: bool,
        left: f64,
        best: f64,
        since: f64,
    },
    /// Swimming about on the water through `pts`, its back just out of it.
    Swim {
        pts: Vec<[f64; 2]>,
        i: usize,
        left: f64,
    },
    /// Falling straight down, `v` blocks a second, onto whatever is under it.
    Drop {
        v: f64,
    },
    /// Up into the viewer's face: gone in a puff of smoke, then up from below the bottom of the
    /// screen, right in front of the eye, `d` blocks off and `h` across, looking at the viewer for
    /// `hold` seconds, and back down out of sight. `s` seconds into it.
    Peek {
        s: f64,
        d: f64,
        h: f64,
        hold: f64,
    },
    /// Started when it comes to them in a `Seq`, from wherever it is then: a hop to `to`; a way of
    /// holding itself (turned to the viewer with `face`); up in smoke and down at a spot; off along the
    /// ground the way `fwd` points, walking on whatever is there.
    Leap {
        to: V3,
        rise: f64,
        trick: Trick,
    },
    Pose {
        hold: Hold,
        secs: f64,
        face: bool,
    },
    Poof {
        to: [i32; 3],
        stay: f64,
    },
    Wander {
        fwd: [i32; 3],
        left: f64,
    },
}

/// A house of the farm as the pig knows it: the spots on its roof it sees, and the box they make
/// across the ground (lowest and highest x and z).
struct House {
    roof: Vec<[i32; 3]>,
    lo: [i32; 2],
    hi: [i32; 2],
}

impl House {
    fn middle(&self) -> [f64; 2] {
        [(self.lo[0] + self.hi[0]) as f64 / 2.0 + 0.5, (self.lo[1] + self.hi[1]) as f64 / 2.0 + 0.5]
    }
}

/// How fast it turns on the spot, radians a second, and how often: at the herd's pace mostly, a little
/// faster now and then, and now and then swivelling round at once.
const TURNS: [(f64, f64); 3] = [(3.0, 0.7), (5.0, 0.2), (f64::INFINITY, 0.1)];

/// The things it does, each a name (`Pig::doing`) and how likely it is chosen.
const DOINGS: &[(&str, f64)] = &[
    ("eat", 45.0),
    ("crawl", 6.0),
    ("climb a wall", 7.0),
    ("run in circles", 4.0),
    ("hop about", 5.0),
    ("pop over in smoke", 2.0),
    ("tumble down a mountain", 2.5),
    ("fly", 3.0),
    ("drop out of the sky", 1.5),
    ("peek at the viewer", 1.2),
    ("wander between the houses", 4.0),
    ("trip to the lodge", 1.5),
    ("swim in the pond", 2.5),
    ("jump between the roofs", 2.5),
    ("laps round a house", 1.5),
    ("sit", 1.5),
    ("beg", 1.2),
    ("headstand", 0.8),
    ("nap", 1.5),
    ("kick on its back", 1.2),
    ("bow", 0.8),
    ("stare at the viewer", 1.2),
    ("shake", 0.8),
    ("headbang", 0.8),
    ("spin round", 0.8),
    ("chase its tail", 1.2),
    ("zoomies", 1.2),
    ("figure eight", 0.8),
    ("zigzag", 0.8),
    ("moonwalk", 0.8),
    ("tiptoe", 0.8),
    ("sniff about", 1.5),
    ("pace", 0.8),
    ("pogo", 1.2),
    ("hopscotch", 0.8),
    ("backflip", 0.8),
    ("barrel roll", 0.8),
    ("spin jump", 0.8),
    ("triple flip", 0.5),
    ("rocket", 0.6),
    ("cannonball", 1.0),
    ("smoke trail", 0.5),
    ("bump into a wall", 0.8),
    ("nap on a roof", 0.8),
    ("king of the hill", 0.8),
    ("ridge walk", 0.8),
    ("dizzy", 0.4),
    ("climb a mountain", 0.8),
];

pub struct Pig {
    map: Map,
    rng: Rng,
    cam: Camera,
    /// The places it is put down: open cells over the ground the camera sees, crops it sees, and high
    /// places it sees on a slope.
    spots: Vec<[i32; 3]>,
    /// Those of the spots that are on a roof, which it likes to come down on, and those that are not.
    roofs: Vec<[i32; 3]>,
    plain: Vec<[i32; 3]>,
    crops: Vec<[i32; 3]>,
    peaks: Vec<[i32; 3]>,
    /// Water it sees (the cell over the bed), and of it the pond: open water, not the narrow stream.
    wet: Vec<[i32; 3]>,
    pond: Vec<[i32; 3]>,
    /// The feet of walls it sees on level ground, and the way into each: houses' walls, the mountains'.
    walls: Vec<([i32; 3], [i32; 3])>,
    houses: Vec<House>,
    /// The terrace of the lodge in the mountains on the left.
    lodge: Vec<[i32; 3]>,
    centre: [f64; 2],
    /// The camera's eye, the way it looks across the ground, and how far off the sides of the screen
    /// are a block ahead: where it flies by.
    eye: V3,
    ahead: V3,
    wide: f64,
    t: f64,
    act: Act,
    /// What it chose to do last (`DOINGS`).
    doing: &'static str,
    /// How fast it turns on the spot, radians a second, chosen with each thing it does (`TURNS`):
    /// infinite swivels at once.
    turn_rate: f64,
    /// Its feet, the way out of its back and the way its snout points: where its doings put it.
    at: V3,
    up: V3,
    fwd: V3,
    /// The same as shown, following those: it rounds a corner and lands softly rather than snapping.
    shown: [V3; 3],
    walk: Walk,
    nod: f64,
    seen: bool,
    /// Where puffs of smoke went up, and when.
    puffs: Vec<(V3, f64)>,
}

const Y: V3 = [0.0, 1.0, 0.0];

/// `up` and `fwd` turned head over heels by `a`, forwards.
fn heels(up: V3, fwd: V3, a: f64) -> (V3, V3) {
    let (c, s) = (a.cos(), a.sin());
    (add(mul(up, c), mul(fwd, s)), add(mul(fwd, c), mul(up, -s)))
}

/// A way across the ground as a direction.
fn level(d: [f64; 2]) -> V3 {
    let k = d[0].hypot(d[1]);
    if k < 1e-9 { [0.0, 0.0, 1.0] } else { [d[0] / k, 0.0, d[1] / k] }
}

fn spot_feet(c: [i32; 3]) -> V3 {
    [c[0] as f64 + 0.5, c[1] as f64, c[2] as f64 + 0.5]
}

fn across_ground(a: V3, b: V3) -> f64 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// The spots near each other gathered into groups: each spot with those within a block of it, across
/// the ground.
fn groups(spots: &[[i32; 3]]) -> Vec<Vec<[i32; 3]>> {
    let mut left: Vec<[i32; 3]> = spots.to_vec();
    let mut out = vec![];
    while let Some(first) = left.pop() {
        let mut group = vec![first];
        let mut k = 0;
        while k < group.len() {
            let c = group[k];
            let (near, far): (Vec<[i32; 3]>, Vec<[i32; 3]>) = left.into_iter().partition(|o| (o[0] - c[0]).abs() <= 1 && (o[2] - c[2]).abs() <= 1);
            left = far;
            group.extend(near);
            k += 1;
        }
        out.push(group);
    }
    out
}

impl Pig {
    /// The pig, in a world seen by `cam`; `seen` is whether a point of the world shows on the screen,
    /// not hidden behind anything.
    pub fn new(w: &World, cam: &Camera, seed: i64, seen: impl Fn(V3) -> bool) -> Pig {
        let ahead = level([cam.forward[0], cam.forward[2]]);
        let centre = ((ahead[0] * FARM_AHEAD).round() as i32, (ahead[2] * FARM_AHEAD).round() as i32);
        let map = Map::new(w, centre);
        let (mut spots, mut crops, mut peaks, mut roofs, mut wet) = (vec![], vec![], vec![], vec![], vec![]);
        for x in centre.0 - REACH + 2..=centre.0 + REACH - 2 {
            for z in centre.1 - REACH + 2..=centre.1 + REACH - 2 {
                let top = map.top(x as f64, z as f64);
                let c = [x, top, z];
                // not right under the eye, where it would fill the screen
                let p = add(spot_feet(c), [0.0, 0.5, 0.0]);
                let near = (p[0] - cam.eye[0]).hypot(p[2] - cam.eye[2]) < NEAREST;
                if top <= Y0 + 1 || near || !map.within(c) || !seen(p) {
                    continue;
                }
                match map.at(c) {
                    CROP => crops.push(c),
                    WET => wet.push(c),
                    OPEN => {
                        spots.push(c);
                        if matches!(w.get(x, top - 1, z), STAIRS_PX | STAIRS_NX | STAIRS_PZ | STAIRS_NZ | ROOF_SLAB) {
                            roofs.push(c);
                        }
                        let lower = [(3, 0), (-3, 0), (0, 3), (0, -3)].iter().any(|&(dx, dz)| map.top((x + dx) as f64, (z + dz) as f64) <= top - 3);
                        if top >= GROUND + 10 && lower {
                            peaks.push(c);
                        }
                    }
                    _ => {}
                }
            }
        }
        let plain: Vec<[i32; 3]> = spots.iter().copied().filter(|c| !roofs.contains(c)).collect();
        // the pond: water with water all about it, not the narrow stream
        let pond: Vec<[i32; 3]> = wet
            .iter()
            .copied()
            .filter(|c| (-2..=2).flat_map(|dx| (-2..=2).map(move |dz| (dx, dz))).filter(|&(dx, dz)| map.at([c[0] + dx, c[1], c[2] + dz]) == WET).count() >= 18)
            .collect();
        // the feet of walls on level ground: a column three or more higher beside a plain spot
        let mut walls = vec![];
        for &c in &plain {
            for d in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
                let n = iadd(c, d);
                if map.solid(n) && map.top(n[0] as f64, n[2] as f64) >= c[1] + 3 {
                    walls.push((c, d));
                }
            }
        }
        // the houses: their roofs' spots in groups
        let mut houses: Vec<House> = groups(&roofs)
            .into_iter()
            .filter(|g| g.len() >= 6)
            .map(|roof| {
                let lo = [roof.iter().map(|c| c[0]).min().unwrap_or(0), roof.iter().map(|c| c[2]).min().unwrap_or(0)];
                let hi = [roof.iter().map(|c| c[0]).max().unwrap_or(0), roof.iter().map(|c| c[2]).max().unwrap_or(0)];
                House { roof, lo, hi }
            })
            .collect();
        houses.sort_by_key(|h| std::cmp::Reverse(h.roof.len()));
        // the lodge's terrace: the commonest height of the ground about the cliff dwelling, which is
        // its level
        let lodge = {
            let (lx, lz) = cliff_at((ahead[0], ahead[2]));
            let (lx, lz) = (lx.round() as i32, lz.round() as i32);
            let near: Vec<[i32; 3]> = (-9..=9)
                .flat_map(|dx| (-9..=9).map(move |dz| (dx, dz)))
                .map(|(dx, dz)| [lx + dx, map.top((lx + dx) as f64, (lz + dz) as f64), lz + dz])
                .filter(|c| map.within(*c) && map.at(*c) == OPEN && c[1] >= GROUND + 6)
                .collect();
            let mut counts: std::collections::HashMap<i32, usize> = Default::default();
            for c in &near {
                *counts.entry(c[1]).or_default() += 1;
            }
            let level = counts.into_iter().max_by_key(|&(y, n)| (n, y)).map(|(y, _)| y);
            near.into_iter().filter(|c| Some(c[1]) == level).collect()
        };
        let mut rng = Rng::new(seed ^ 0x0050_4947);
        let start = if crops.is_empty() { spots.first().copied() } else { Some(*rng.pick(&crops)) };
        let start = start.unwrap_or([centre.0, map.top(centre.0 as f64, centre.1 as f64), centre.1]);
        let at = spot_feet(start);
        let fwd = level([rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)]);
        Pig {
            map,
            rng,
            cam: *cam,
            spots,
            plain,
            roofs,
            crops,
            peaks,
            wet,
            pond,
            walls,
            houses,
            lodge,
            centre: [centre.0 as f64 + 0.5, centre.1 as f64 + 0.5],
            eye: cam.eye,
            ahead,
            wide: cam.tan * cam.aspect,
            t: 0.0,
            act: Act::Eat { left: 20.0, shuffle: 4.0, look: None },
            doing: "eat",
            turn_rate: TURNS[0].0,
            at,
            up: Y,
            fwd,
            shown: [at, Y, fwd],
            walk: Walk::STILL,
            nod: 0.0,
            seen: true,
            puffs: vec![],
        }
    }

    /// Step on to the scene's clock at `t`.
    pub fn advance(&mut self, t: f64) {
        while self.t + STEP <= t {
            self.step();
            self.t += STEP;
        }
    }

    /// As it is shown, posed, seen from `eye`: None while it is gone.
    pub fn pose(&self, eye: V3) -> Option<Pose> {
        let [at, up, fwd] = self.shown;
        self.seen.then(|| mobs::pose_turned(Kind::Pig, at, [cross(up, fwd), up, fwd], self.walk, self.nod, eye))
    }

    /// The rectangle of the screen it can be in, turned any way: a box about its middle.
    pub fn rect(&self, cam: &Camera) -> Option<(f64, f64, f64, f64)> {
        let [at, up, _] = self.shown;
        let mid = add(at, mul(up, MIDDLE));
        let r = 1.1;
        bounds(cam, (0..8).map(|k| [0, 1, 2].map(|i| mid[i] + if k >> i & 1 == 0 { -r } else { r }))).0
    }

    /// The pixels it can be in, and its pose; None while it is gone or behind the eye.
    pub fn seen(&self, cam: &Camera) -> Option<((f64, f64, f64, f64), crate::trace::camera::Px, Pose)> {
        let rect = self.rect(cam)?;
        let pose = self.pose(cam.eye)?;
        let all = pixels(rect);
        let px = pose.pixels(cam).and_then(|p| overlap(p, all)).unwrap_or(all);
        Some((rect, px, pose))
    }

    /// Everything its picture depends on, exactly.
    pub fn key(&self) -> [u64; 5] {
        let [at, up, fwd] = self.shown;
        let mix = |vs: &[f64]| vs.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, v| (h ^ v.to_bits()).wrapping_mul(0x0100_0000_01b3));
        [
            at[0].to_bits(),
            at[1].to_bits(),
            at[2].to_bits(),
            mix(&[up[0], up[1], up[2], fwd[0], fwd[1], fwd[2]]),
            mix(&[self.walk.phase, self.walk.amount, self.nod, self.seen as u8 as f64]),
        ]
    }

    /// Whether a point of the world is under water.
    pub fn wet(&self, p: V3) -> bool {
        self.map.at(p.map(|v| v.floor() as i32)) == WET
    }

    /// The puffs of smoke in the air: where each went up, and how long ago.
    pub fn puffs(&self) -> impl Iterator<Item = (V3, f64)> + '_ {
        self.puffs.iter().map(|&(p, t)| (p, self.t - t)).filter(|&(_, age)| (0.0..PUFF).contains(&age))
    }

    /// What it is doing at this moment: the first of a `Seq`.
    fn now(&self) -> &Act {
        let mut a = &self.act;
        while let Act::Seq(q) = a {
            match q.front() {
                Some(f) => a = f,
                None => break,
            }
        }
        a
    }

    fn step(&mut self) {
        let before = self.at;
        let was = (self.at, self.up, self.fwd);
        let crawled = matches!(self.now(), Act::Crawl { .. });
        let dropping = matches!(self.now(), Act::Drop { .. });
        let act = std::mem::replace(&mut self.act, Act::Next);
        self.act = self.act(act);
        // whatever it does off the faces it walks on, never into a block: where a step would put it in
        // one, it stays where it was and falls straight down from there instead
        let crawling = crawled || matches!(self.now(), Act::Crawl { .. });
        if self.seen && !crawling && !body_clear(&self.map, self.at, self.up, self.fwd) {
            (self.at, self.up, self.fwd) = was;
            self.act = if !dropping && body_clear(&self.map, was.0, Y, level([was.2[0], was.2[2]])) {
                (self.up, self.fwd) = (Y, level([was.2[0], was.2[2]]));
                Act::Drop { v: 0.0 }
            } else {
                let to = self.landing();
                self.gone(to, Act::Stand { left: 0.5 })
            };
        }
        self.puffs.retain(|&(_, t)| self.t - t < PUFF);
        // its legs: they go with the ground walked, flail in the air, paddle in water, kick on its back,
        // rest while it eats
        let moved = (0..3).map(|i| (self.at[i] - before[i]).powi(2)).sum::<f64>().sqrt();
        let (amount, flail) = match self.now() {
            Act::Crawl { .. } | Act::Circle { .. } | Act::Walk { .. } | Act::Wander { .. } => (0.9, 0.0),
            Act::Fly { .. } => (0.8, 8.0 * STEP),
            Act::Swim { .. } => (0.8, 7.0 * STEP),
            Act::Tumble { .. } => (1.0, 0.0),
            Act::Hop { .. } | Act::Drop { .. } => (0.7, 14.0 * STEP),
            Act::Held { hold: Hold::Back, .. } => (1.0, 16.0 * STEP),
            Act::Held { hold: Hold::Spin, .. } => (0.6, 6.0 * STEP),
            _ => (0.0, 0.0),
        };
        self.walk.phase += if flail > 0.0 { flail } else { moved.min(1.0) * 8.0 };
        self.walk.amount += (amount - self.walk.amount) * 0.4;
        if self.walk.amount < 0.01 && amount == 0.0 {
            self.walk.amount = 0.0;
        }
        let nod = match self.now() {
            Act::Eat { .. } => 0.5 + 0.3 * (self.t * 11.0).sin(),
            Act::Held { hold: Hold::Headbang, t, .. } => 0.35 + 0.55 * (t * 13.0).sin(),
            Act::Held { hold: Hold::Bow, .. } => 0.45,
            Act::Held { hold: Hold::Sleep, .. } => 0.3,
            Act::Walk { wait, .. } if *wait > 0.0 => 0.45 + 0.25 * (self.t * 9.0).sin(),
            Act::Peek { s, .. } => 0.18 * (s * 3.1).sin(),
            _ => 0.0,
        };
        self.nod += (nod - self.nod) * 0.5;
        if self.nod.abs() < 1e-3 && nod == 0.0 {
            self.nod = 0.0;
        }
        self.follow();
    }

    /// What is shown, moved on towards where it is.
    fn follow(&mut self) {
        const K: f64 = 0.45;
        let [at, up, fwd] = self.shown;
        let toward = |from: V3, to: V3, side: V3| {
            // turned right round, the way through `side`, so it does not pass through nothing
            let to = if dot(from, to) < -0.7 { norm(add(to, side)) } else { to };
            add(from, mul(add(to, mul(from, -1.0)), K))
        };
        let at = add(at, mul(add(self.at, mul(at, -1.0)), K));
        let up2 = toward(up, self.up, fwd);
        let up2 = if dot(up2, up2) < 1e-6 { self.up } else { norm(up2) };
        let f = toward(fwd, self.fwd, up);
        let f = add(f, mul(up2, -dot(f, up2)));
        let f = if dot(f, f) < 1e-6 {
            let g = add(self.fwd, mul(up2, -dot(self.fwd, up2)));
            // square to its back: across the world's axis least along it
            let axis = if up2[0].abs() < 0.6 { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
            if dot(g, g) < 1e-6 { cross(up2, axis) } else { g }
        } else {
            f
        };
        self.shown = [at, up2, norm(f)];
    }

    fn puff(&mut self) {
        let [at, up, _] = self.shown;
        self.puffs.push((add(at, mul(up, MIDDLE)), self.t));
    }

    /// Put down at a spot, standing, facing a way its body is clear of everything.
    fn put(&mut self, c: [i32; 3]) {
        self.at = spot_feet(c);
        self.up = Y;
        self.fwd = level([self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0)]);
        for k in 0..8 {
            if body_clear(&self.map, self.at, Y, self.fwd) {
                break;
            }
            let a = k as f64 * PI / 4.0;
            self.fwd = level([a.cos(), a.sin()]);
        }
        self.shown = [self.at, self.up, self.fwd];
    }

    /// A step of what it is doing; what it is doing then.
    fn act(&mut self, act: Act) -> Act {
        match act {
            Act::Next => self.choose(),
            Act::Seq(mut q) => {
                let Some(first) = q.pop_front() else { return Act::Next };
                match self.act(first) {
                    Act::Next if q.is_empty() => Act::Next,
                    Act::Next => Act::Seq(q),
                    a => {
                        q.push_front(a);
                        Act::Seq(q)
                    }
                }
            }
            Act::Stand { left } => {
                if left <= 0.0 {
                    Act::Next
                } else {
                    Act::Stand { left: left - STEP }
                }
            }
            Act::Crawl { c, left } => {
                let c = c.on(&self.map, CRAWL * STEP);
                (self.at, self.up, self.fwd) = c.lie(&self.map);
                // done only on its feet: off a wall or out from under a roof it goes on walking
                let footed = c.up == [0, 1, 0] && self.up[1] > 0.99;
                if left <= 0.0 && footed {
                    Act::Next
                } else if left < -40.0 {
                    let to = self.landing();
                    self.gone(to, Act::Stand { left: 0.5 })
                } else {
                    Act::Crawl { c, left: left - STEP }
                }
            }
            Act::Circle { centre, r, angle, turn, left } => {
                let angle = angle + turn * STEP;
                let (x, z) = (centre[0] + r * angle.cos(), centre[1] + r * angle.sin());
                if self.map.column(x.floor() as i32, z.floor() as i32).is_none() {
                    return Act::Next;
                }
                let top = self.map.top(x, z) as f64;
                // a wall in its way: over it in a hop
                if top > self.at[1] + 1.01 {
                    return self.hop(0, None, Act::Next);
                }
                // up a step or down one; off a drop it runs on through the air
                let y = if (top - self.at[1]).abs() <= 1.01 { top } else { self.at[1] };
                self.at = [x, y, z];
                self.fwd = level([-angle.sin() * turn.signum(), angle.cos() * turn.signum()]);
                self.up = Y;
                if left <= 0.0 {
                    Act::Next
                } else {
                    Act::Circle { centre, r, angle, turn: if self.rng.f64() < 0.004 { -turn } else { turn }, left: left - STEP }
                }
            }
            Act::Hop { from, to, v, g, time, s, trick, more, toward, then } => {
                let s = (s + STEP).min(time);
                let fwd0 = self.hop_way(from, to);
                (self.at, self.up, self.fwd) = hop_pose(from, to, v, g, s, time, trick, fwd0);
                if s < time {
                    return Act::Hop { from, to, v, g, time, s, trick, more, toward, then };
                }
                let fwd = turn(fwd0, Y, trick.spins * TAU);
                (self.at, self.up, self.fwd) = (to, Y, level([fwd[0], fwd[2]]));
                if more > 0 { self.hop(more - 1, toward, *then) } else { *then }
            }
            Act::Gone { left, to, then, fall } => {
                if left > 0.0 {
                    return Act::Gone { left: left - STEP, to, then, fall };
                }
                self.put(to);
                self.seen = true;
                if !fall {
                    self.puff();
                    return *then;
                }
                // out of the sky, high over where it comes down, and down onto it
                let feet = self.at;
                self.at[1] += FALL;
                self.shown = [self.at, self.up, self.fwd];
                // out of a puff up there, so it does not show out of nothing
                self.puff();
                let flips = if self.rng.f64() < 0.4 { 1.0 } else { 0.0 };
                self.leap(feet, 0.2, flips, 0, None, *then)
            }
            Act::Eat { left, shuffle, look } => {
                if left <= 0.0 {
                    return Act::Next;
                }
                let mut look = look;
                if look.is_none() && self.rng.f64() < 0.01 {
                    // it looks up, and about it, turning on the spot at its pace
                    let f = level([self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0)]);
                    if body_clear(&self.map, self.at, Y, f) {
                        look = Some(f);
                    }
                }
                if let Some(f) = look
                    && self.turn_toward(f)
                {
                    look = None;
                }
                if shuffle > 0.0 || look.is_some() {
                    return Act::Eat { left: left - STEP, shuffle: shuffle - STEP, look };
                }
                // on to the next plant: a crop next to it, a little hop over
                let here = self.at;
                let next: Vec<[i32; 3]> = self
                    .crops
                    .iter()
                    .copied()
                    .filter(|c| {
                        let d = across_ground(spot_feet(*c), here);
                        d > 0.5 && d < 2.5
                    })
                    .collect();
                let then = Act::Eat { left, shuffle: self.rng.range(3.0, 7.0), look: None };
                if next.is_empty() {
                    return then;
                }
                let c = *self.rng.pick(&next);
                if self.clear_leap(here, spot_feet(c), 0.5, Trick::NONE) { self.leap(spot_feet(c), 0.5, 0.0, 0, None, then) } else { then }
            }
            Act::Tumble { dir, speed, roll, flat, left } => self.tumble(dir, speed, roll, flat, left),
            Act::Fly { pts, s, time, rolls, bob, up, then } => {
                let s = s + STEP;
                let k = (s / time).min(1.0);
                let (p, d, _) = curve(&pts, k);
                // the way out of its back carried along the curve, square to it: through a loop too
                let u = add(up, mul(d, -dot(up, d)));
                let u = if dot(u, u) < 1e-6 { norm(add(Y, mul(d, -dot(Y, d)))) } else { norm(u) };
                let wobble = if bob { (s * 2.6).sin() * 0.3 } else { 0.0 };
                self.up = turn(u, d, rolls * TAU * k);
                self.at = add(add(p, [0.0, wobble, 0.0]), mul(self.up, -MIDDLE));
                self.fwd = d;
                if s < time {
                    return Act::Fly { pts, s, time, rolls, bob, up: u, then };
                }
                if let Some(then) = then {
                    let last = pts[pts.len() - 1];
                    (self.at, self.up, self.fwd) = (add(last, mul(Y, -MIDDLE)), Y, level([d[0], d[2]]));
                    return *then;
                }
                // off the screen: down somewhere a little later
                self.seen = false;
                let to = self.landing();
                let stand = Act::Stand { left: self.rng.range(0.5, 2.0) };
                Act::Gone { left: self.rng.range(1.0, 3.0), to, then: Box::new(stand), fall: false }
            }
            Act::Held { hold, left, t, feet, face } => {
                if left <= 0.0 {
                    (self.at, self.up, self.fwd) = (feet, Y, face);
                    return Act::Next;
                }
                (self.at, self.up, self.fwd) = held(hold, t, feet, face);
                Act::Held { hold, left: left - STEP, t: t + STEP, feet, face }
            }
            Act::Walk { pts, i, speed, back, pause, wait, climb, left, best, since } => {
                self.walk_on(pts, i, speed, back, pause, wait, climb, left, best, since)
            }
            Act::Swim { pts, i, left } => self.swim(pts, i, left),
            Act::Drop { v } => {
                let v = v + GRAVITY * STEP;
                let (x, z) = (self.at[0], self.at[2]);
                let mut floor = Y0 + 1;
                for y in (Y0..=(self.at[1].floor() as i32)).rev() {
                    if self.map.solid([x.floor() as i32, y, z.floor() as i32]) {
                        floor = y + 1;
                        break;
                    }
                }
                let ny = self.at[1] - v * STEP;
                (self.up, self.fwd) = (Y, level([self.fwd[0], self.fwd[2]]));
                if ny <= floor as f64 {
                    self.at[1] = floor as f64;
                    return Act::Stand { left: 0.3 };
                }
                self.at[1] = ny;
                Act::Drop { v }
            }
            Act::Peek { s, d, h, hold } => self.peek_on(s, d, h, hold),
            Act::Leap { to, rise, trick } => {
                if self.clear_leap(self.at, to, rise, trick) {
                    self.leap_with(to, rise, trick, 0, None, Act::Next)
                } else {
                    Act::Next
                }
            }
            Act::Pose { hold, secs, face } => match self.pose_act(hold, secs, face) {
                Some(a) => a,
                None => Act::Next,
            },
            Act::Poof { to, stay } => self.gone(to, Act::Stand { left: stay }),
            Act::Turn { face } => {
                if self.turn_toward(face) {
                    Act::Next
                } else {
                    Act::Turn { face }
                }
            }
            Act::Wander { fwd, left } => {
                if self.upright() && self.off(f3(fwd)).abs() > 0.05 {
                    return Act::Seq(vec![Act::Turn { face: f3(fwd) }, Act::Wander { fwd, left }].into());
                }
                let cell = [self.at[0].floor() as i32, (self.at[1] + 0.5).floor() as i32, self.at[2].floor() as i32];
                if self.map.solid(cell) || !self.map.solid(iadd(cell, [0, -1, 0])) || self.up[1] < 0.99 {
                    return Act::Next;
                }
                let middle = add(f3(cell), [0.5, 0.0, 0.5]);
                let along = dot(add(self.at, mul(middle, -1.0)), f3(fwd)).clamp(-0.49, 0.49);
                Act::Crawl { c: Crawl { cell, up: [0, 1, 0], fwd, along }, left }
            }
        }
    }

    /// How far round `face` is from the way it faces, about the upright: -PI to PI.
    fn off(&self, face: V3) -> f64 {
        let want = face[0].atan2(face[2]);
        let now = self.fwd[0].atan2(self.fwd[2]);
        (want - now + PI).rem_euclid(TAU) - PI
    }

    /// A step of turning on the spot towards `face`, at its pace; whether it faces it now.
    fn turn_toward(&mut self, face: V3) -> bool {
        let face = level([face[0], face[2]]);
        let off = self.off(face);
        let most = self.turn_rate * STEP;
        if off.abs() <= most {
            self.fwd = face;
            return true;
        }
        self.fwd = turn(level([self.fwd[0], self.fwd[2]]), Y, off.signum() * most);
        false
    }

    fn upright(&self) -> bool {
        self.up[1] > 0.99
    }

    /// What it does next: mostly eat; all manner of other things now and then (`DOINGS`).
    fn choose(&mut self) -> Act {
        if !self.upright() {
            // off whatever it was on, down onto its feet
            return Act::Drop { v: 0.0 };
        }
        // how fast it turns on the spot while it does it: mostly at the herd's pace
        let paces: Vec<f64> = TURNS.iter().map(|t| t.1).collect();
        self.turn_rate = TURNS[self.rng.weighted(&paces)].0;
        let weights: Vec<f64> = DOINGS.iter().map(|d| d.1).collect();
        for _ in 0..4 {
            let i = self.rng.weighted(&weights);
            if let Some(a) = self.start(i) {
                self.doing = DOINGS[i].0;
                return a;
            }
        }
        self.doing = "hop about";
        let n = self.rng.int(3, 9) as u32;
        self.hop(n, None, Act::Next)
    }

    /// The `i`th of the things it does (`DOINGS`), from where it is; None where it cannot here.
    fn start(&mut self, i: usize) -> Option<Act> {
        let name = DOINGS.get(i)?.0;
        let seq = |v: Vec<Act>| Act::Seq(v.into());
        let here = self.at;
        match name {
            "eat" => {
                let c = *self.pick(&self.crops.clone())?;
                let eat = Act::Eat { left: self.rng.range(12.0, 35.0), shuffle: self.rng.range(3.0, 7.0), look: None };
                let far = across_ground(spot_feet(c), here);
                Some(if far < 40.0 { self.hop(14, Some(c), eat) } else { self.gone(c, eat) })
            }
            "crawl" => Some(self.crawl()),
            "climb a wall" => {
                // a wall's foot it sees, any house's or the mountains', from any side
                let (c, d) = *self.pick(&self.walls.clone())?;
                let climb = Act::Wander { fwd: d, left: self.rng.range(6.0, 16.0) };
                Some(self.travel(c, climb))
            }
            "run in circles" => {
                let r = self.rng.range(0.8, 2.0);
                let v0 = self.rng.range(3.0, 9.0);
                Some(self.circle(r, RUN, v0))
            }
            "hop about" => {
                let n = self.rng.int(3, 9) as u32;
                Some(self.hop(n, None, Act::Next))
            }
            "pop over in smoke" => {
                if self.spots.is_empty() {
                    return None;
                }
                let to = self.landing();
                let stand = Act::Stand { left: self.rng.range(0.5, 2.0) };
                Some(self.gone(to, stand))
            }
            "tumble down a mountain" => {
                let to = *self.pick(&self.peaks.clone())?;
                let tumble = Act::Tumble { dir: [0.0, 0.0], speed: 0.0, roll: 0.0, flat: 0.0, left: 14.0 };
                Some(self.gone(to, tumble))
            }
            "fly" => {
                if self.spots.is_empty() {
                    return None;
                }
                self.fly()
            }
            "drop out of the sky" => {
                if self.spots.is_empty() {
                    return None;
                }
                // up in smoke, and a few seconds later down out of the sky
                let to = self.landing();
                let stand = Act::Stand { left: self.rng.range(0.5, 2.0) };
                self.puff();
                self.seen = false;
                Some(Act::Gone { left: self.rng.range(4.0, 6.0), to, then: Box::new(stand), fall: true })
            }
            "peek at the viewer" => self.peek(),
            "wander between the houses" => {
                if self.houses.len() < 2 {
                    return None;
                }
                // to the side of one house after another, sniffing about at each
                let n = self.rng.int(2, self.houses.len().min(4) as i64) as usize;
                let mut order: Vec<usize> = (0..self.houses.len()).collect();
                self.rng.shuffle(&mut order);
                let mut pts = vec![];
                for &k in order.iter().take(n) {
                    let side = self.rng.int_below(0, 4);
                    let h = &self.houses[k];
                    let (lo, hi) = ([h.lo[0] as f64 - 1.5, h.lo[1] as f64 - 1.5], [h.hi[0] as f64 + 2.5, h.hi[1] as f64 + 2.5]);
                    let m = h.middle();
                    pts.push(match side {
                        0 => [lo[0], m[1]],
                        1 => [hi[0], m[1]],
                        2 => [m[0], lo[1]],
                        _ => [m[0], hi[1]],
                    });
                }
                Some(self.walk(pts, CRAWL * 1.3, false, 1.5, true, 80.0))
            }
            "trip to the lodge" => {
                let c = *self.pick(&self.lodge.clone())?;
                let sit = Act::Pose { hold: Hold::Sit, secs: self.rng.range(5.0, 9.0), face: true };
                let down = Act::Tumble { dir: [0.0, 0.0], speed: 0.0, roll: 0.0, flat: 0.0, left: 14.0 };
                let walk = self.walk(vec![[c[0] as f64 + 0.5, c[2] as f64 + 0.5]], CRAWL * 1.4, false, 0.0, true, 120.0);
                Some(seq(vec![walk, Act::Poof { to: c, stay: 0.2 }, sit, down]))
            }
            "swim in the pond" => {
                let c = *self.pick(&self.pond.clone())?;
                let v0 = self.rng.range(10.0, 20.0);
                let swim = self.swim_about(c, v0);
                let out = self.out_of_water(c);
                let shake = Act::Pose { hold: Hold::Shake, secs: 1.5, face: false };
                let dive = self.dive_in(c);
                let near = self.near_spot(spot_feet(c), 3.0, 9.0);
                let get_there = match near {
                    Some(n) => self.travel(n, Act::Next),
                    None => Act::Next,
                };
                Some(seq(vec![get_there, dive, swim, out, shake]))
            }
            "jump between the roofs" => {
                if self.houses.len() < 2 {
                    return None;
                }
                let mut order: Vec<usize> = (0..self.houses.len()).collect();
                self.rng.shuffle(&mut order);
                let first = *self.pick(&self.houses[order[0]].roof.clone())?;
                let mut acts = vec![Act::Poof { to: first, stay: 0.4 }];
                for k in order.iter().skip(1) {
                    let to = *self.pick(&self.houses[*k].roof.clone())?;
                    let flips = if self.rng.f64() < 0.3 { 1.0 } else { 0.0 };
                    acts.push(Act::Leap { to: spot_feet(to), rise: self.rng.range(3.0, 6.0), trick: Trick::flips(flips) });
                    acts.push(Act::Stand { left: self.rng.range(0.3, 1.0) });
                }
                acts.push(Act::Pose { hold: Hold::Stare, secs: 2.0, face: true });
                Some(seq(acts))
            }
            "laps round a house" => {
                let h = self.pick(&self.houses.iter().map(|h| (h.lo, h.hi)).collect::<Vec<_>>())?.to_owned();
                let (lo, hi) = ([h.0[0] as f64 - 1.5, h.0[1] as f64 - 1.5], [h.1[0] as f64 + 2.5, h.1[1] as f64 + 2.5]);
                let corners = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]];
                let laps = self.rng.int(1, 3) as usize;
                let mut pts: Vec<[f64; 2]> = vec![];
                let start = self.rng.int_below(0, 4) as usize;
                for k in 0..4 * laps + 1 {
                    pts.push(corners[(start + k) % 4]);
                }
                let get_there = self.walk(vec![pts[0]], CRAWL * 1.3, false, 0.0, true, 40.0);
                let laps = self.walk(pts, RUN, false, 0.0, false, 60.0);
                Some(seq(vec![get_there, laps]))
            }
            "sit" => {
                let v0 = self.rng.range(4.0, 9.0);
                let v1 = self.rng.f64();
                self.pose_act(Hold::Sit, v0, v1 < 0.6)
            }
            "beg" => {
                let v0 = self.rng.range(2.5, 5.0);
                self.pose_act(Hold::Beg, v0, true)
            }
            "headstand" => {
                let v0 = self.rng.range(2.0, 4.0);
                self.pose_act(Hold::Headstand, v0, false)
            }
            "nap" => {
                let v0 = self.rng.range(10.0, 22.0);
                self.pose_act(Hold::Sleep, v0, false)
            }
            "kick on its back" => {
                let v0 = self.rng.range(3.0, 6.0);
                self.pose_act(Hold::Back, v0, false)
            }
            "bow" => {
                let v0 = self.rng.range(1.5, 2.5);
                self.pose_act(Hold::Bow, v0, true)
            }
            "stare at the viewer" => {
                let v0 = self.rng.range(3.0, 6.0);
                self.pose_act(Hold::Stare, v0, true)
            }
            "shake" => {
                let v0 = self.rng.range(1.0, 2.0);
                self.pose_act(Hold::Shake, v0, false)
            }
            "headbang" => {
                let v0 = self.rng.range(2.0, 4.0);
                self.pose_act(Hold::Headbang, v0, true)
            }
            "spin round" => {
                let v0 = self.rng.range(2.0, 4.0);
                self.pose_act(Hold::Spin, v0, false)
            }
            "dizzy" => {
                let v0 = self.rng.range(2.0, 4.0);
                self.pose_act(Hold::Dizzy, v0, false)
            }
            "chase its tail" => {
                let v0 = self.rng.range(2.5, 5.0);
                let tail = self.circle(0.35, RUN * 1.3, v0);
                Some(seq(vec![tail, Act::Pose { hold: Hold::Dizzy, secs: 2.0, face: false }]))
            }
            "zoomies" => {
                let mut acts = vec![];
                for k in 0..self.rng.int(3, 6) {
                    let a = self.rng.range(0.0, TAU);
                    let far = self.rng.range(3.0, 7.0);
                    acts.push(self.walk(vec![[here[0] + a.cos() * far, here[2] + a.sin() * far], [here[0], here[2]]], RUN * 1.5, false, 0.0, false, 8.0));
                    if k % 2 == 1 {
                        acts.push(Act::Leap { to: here, rise: 1.5, trick: Trick { spins: 1.0, ..Trick::NONE } });
                    }
                }
                Some(seq(acts))
            }
            "figure eight" => {
                let r = self.rng.range(1.5, 2.5);
                let a0 = self.rng.range(0.0, TAU);
                let (ax, az) = (a0.cos(), a0.sin());
                let pts: Vec<[f64; 2]> = (0..=32)
                    .map(|k| {
                        let t = k as f64 / 32.0 * TAU;
                        let (u, v) = (r * 2.0 * t.sin(), r * (2.0 * t).sin());
                        [here[0] + u * ax - v * az, here[2] + u * az + v * ax]
                    })
                    .collect();
                Some(self.walk(pts, RUN, false, 0.0, false, 30.0))
            }
            "zigzag" => {
                let a = self.rng.range(0.0, TAU);
                let (dx, dz) = (a.cos(), a.sin());
                let pts: Vec<[f64; 2]> = (1..=6)
                    .map(|k| {
                        let side = if k % 2 == 0 { 1.2 } else { -1.2 };
                        [here[0] + dx * k as f64 * 1.5 - dz * side, here[2] + dz * k as f64 * 1.5 + dx * side]
                    })
                    .collect();
                Some(self.walk(pts, RUN * 1.4, false, 0.0, false, 15.0))
            }
            "moonwalk" => {
                let f = self.fwd;
                let far = self.rng.range(3.0, 6.0);
                Some(self.walk(vec![[here[0] - f[0] * far, here[2] - f[2] * far]], 1.0, true, 0.0, false, 10.0))
            }
            "tiptoe" => {
                let a = self.rng.range(0.0, TAU);
                let far = self.rng.range(3.0, 5.0);
                Some(self.walk(vec![[here[0] + a.cos() * far, here[2] + a.sin() * far]], 0.55, false, 0.0, false, 12.0))
            }
            "sniff about" => {
                let pts: Vec<[f64; 2]> = (0..self.rng.int(3, 6))
                    .map(|_| {
                        let a = self.rng.range(0.0, TAU);
                        let far = self.rng.range(0.8, 2.5);
                        [here[0] + a.cos() * far, here[2] + a.sin() * far]
                    })
                    .collect();
                Some(self.walk(pts, CRAWL * 0.7, false, 1.4, false, 25.0))
            }
            "pace" => {
                let a = self.rng.range(0.0, TAU);
                let far = self.rng.range(2.5, 4.0);
                let (p, q) = ([here[0] + a.cos() * far, here[2] + a.sin() * far], [here[0], here[2]]);
                Some(self.walk(vec![p, q, p, q, p, q], CRAWL, false, 0.4, false, 25.0))
            }
            "pogo" => {
                let n = self.rng.int(5, 12);
                Some(seq((0..n).map(|_| Act::Leap { to: here, rise: self.rng.range(1.2, 3.0), trick: Trick::NONE }).collect()))
            }
            "hopscotch" => {
                let f = self.fwd;
                Some(seq((1..=6)
                    .map(|k| Act::Leap { to: [here[0] + f[0] * k as f64, 0.0, here[2] + f[2] * k as f64], rise: 0.7, trick: Trick::NONE })
                    .collect()))
            }
            "backflip" => Some(Act::Leap { to: here, rise: 3.0, trick: Trick::flips(-1.0) }),
            "barrel roll" => {
                let f = self.fwd;
                Some(Act::Leap { to: [here[0] + f[0] * 3.0, 0.0, here[2] + f[2] * 3.0], rise: 2.5, trick: Trick { rolls: 1.0, ..Trick::NONE } })
            }
            "spin jump" => Some(Act::Leap { to: here, rise: 2.8, trick: Trick { spins: self.rng.int(1, 3) as f64, ..Trick::NONE } }),
            "triple flip" => Some(Act::Leap { to: here, rise: 8.5, trick: Trick::flips(3.0) }),
            "rocket" => {
                // straight up out of the top of the screen, and back down
                let trick = Trick { flips: 2.0, spins: 1.0, ..Trick::NONE };
                Some(seq(vec![Act::Leap { to: here, rise: 24.0, trick }, Act::Pose { hold: Hold::Dizzy, secs: 1.5, face: false }]))
            }
            "cannonball" => {
                let near: Vec<[i32; 3]> = self.wet.iter().copied().filter(|c| across_ground(spot_feet(*c), here) < 10.0).collect();
                let c = *self.pick(&near)?;
                let dive = self.dive_in(c);
                let out = self.out_of_water(c);
                let shake = Act::Pose { hold: Hold::Shake, secs: 1.5, face: false };
                let v0 = self.rng.range(4.0, 8.0);
                let swim = if self.pond.contains(&c) { self.swim_about(c, v0) } else { Act::Stand { left: 1.0 } };
                Some(seq(vec![dive, swim, out, shake]))
            }
            "smoke trail" => {
                if self.spots.is_empty() {
                    return None;
                }
                let n = self.rng.int(4, 7);
                let mut acts = vec![];
                for _ in 0..n {
                    let to = self.landing();
                    acts.push(Act::Poof { to, stay: self.rng.range(0.3, 0.7) });
                }
                Some(seq(acts))
            }
            "bump into a wall" => {
                let near: Vec<([i32; 3], [i32; 3])> = self.walls.iter().copied().filter(|(c, _)| across_ground(spot_feet(*c), here) < 12.0).collect();
                let (c, d) = *self.pick(&near)?;
                let foot = spot_feet(c);
                // its snout against the wall's face, then knocked back, giddy
                let stop = [foot[0] - d[0] as f64 * 0.5, foot[2] - d[2] as f64 * 0.5];
                let run = self.walk(vec![stop], RUN * 1.5, false, 0.0, false, 12.0);
                let back = [foot[0] - d[0] as f64 * 2.5, 0.0, foot[2] - d[2] as f64 * 2.5];
                Some(seq(vec![run, Act::Leap { to: back, rise: 0.9, trick: Trick::NONE }, Act::Pose { hold: Hold::Dizzy, secs: 2.5, face: false }]))
            }
            "nap on a roof" => {
                let c = *self.pick(&self.roofs.clone())?;
                Some(seq(vec![Act::Poof { to: c, stay: 0.3 }, Act::Pose { hold: Hold::Sleep, secs: self.rng.range(8.0, 16.0), face: false }]))
            }
            "king of the hill" => {
                let c = *self.pick(&self.peaks.clone())?;
                let down = Act::Tumble { dir: [0.0, 0.0], speed: 0.0, roll: 0.0, flat: 0.0, left: 14.0 };
                Some(seq(vec![Act::Poof { to: c, stay: 0.3 }, Act::Pose { hold: Hold::Beg, secs: 3.5, face: true }, down]))
            }
            "ridge walk" => {
                let k = self.rng.int_below(0, self.houses.len().max(1) as i64) as usize;
                let h = self.houses.get(k)?;
                // along the house's length, from its highest spot: the ridge
                let top = *h.roof.iter().max_by_key(|c| c[1])?;
                let long = if h.hi[0] - h.lo[0] >= h.hi[1] - h.lo[1] { [1, 0, 0] } else { [0, 0, 1] };
                let fwd = if self.rng.f64() < 0.5 { long } else { ineg(long) };
                Some(seq(vec![Act::Poof { to: top, stay: 0.3 }, Act::Wander { fwd, left: self.rng.range(3.0, 8.0) }]))
            }
            "climb a mountain" => {
                let c = *self.pick(&self.peaks.clone())?;
                let walk = self.walk(vec![[c[0] as f64 + 0.5, c[2] as f64 + 0.5]], CRAWL * 1.4, false, 0.0, true, 90.0);
                Some(seq(vec![walk, Act::Pose { hold: Hold::Stare, secs: 3.0, face: true }]))
            }
            _ => None,
        }
    }

    /// One of `from`, at random; None of none.
    fn pick<'a, T>(&mut self, from: &'a [T]) -> Option<&'a T> {
        (!from.is_empty()).then(|| self.rng.pick(from))
    }

    /// Round and round a circle of `r` beside it, `speed` blocks a second, for `left` seconds.
    fn circle(&mut self, r: f64, speed: f64, left: f64) -> Act {
        let turn = speed / r * if self.rng.f64() < 0.5 { 1.0 } else { -1.0 };
        // the centre on the side that sets it off the way it faces, not swung round on the spot
        let side = mul(cross(self.up, self.fwd), -turn.signum());
        let centre = [self.at[0] + side[0] * r, self.at[2] + side[2] * r];
        let angle = (self.at[2] - centre[1]).atan2(self.at[0] - centre[0]);
        Act::Circle { centre, r, angle, turn, left }
    }

    /// A walk through `pts` (`Act::Walk`).
    fn walk(&mut self, pts: Vec<[f64; 2]>, speed: f64, back: bool, pause: f64, climb: bool, left: f64) -> Act {
        Act::Walk { pts, i: 0, speed, back, pause, wait: 0.0, climb, left, best: f64::MAX, since: 0.0 }
    }

    /// To a spot, then `then`: on foot over the ground when it is near, else in a puff of smoke.
    fn travel(&mut self, c: [i32; 3], then: Act) -> Act {
        let far = across_ground(spot_feet(c), self.at);
        if far < 14.0 && (spot_feet(c)[1] - self.at[1]).abs() < 3.0 {
            let walk = self.walk(vec![[c[0] as f64 + 0.5, c[2] as f64 + 0.5]], CRAWL * 1.3, false, 0.0, true, 30.0);
            Act::Seq(vec![walk, then].into())
        } else {
            Act::Seq(vec![Act::Poof { to: c, stay: 0.3 }, then].into())
        }
    }

    /// A plain spot `lo` to `hi` blocks across the ground from a point.
    fn near_spot(&mut self, p: V3, lo: f64, hi: f64) -> Option<[i32; 3]> {
        let near: Vec<[i32; 3]> = self.plain.iter().copied().filter(|c| (lo..=hi).contains(&across_ground(spot_feet(*c), p))).collect();
        self.pick(&near).copied()
    }

    /// The water's surface over a cell of water: the bottom of the first cell up that is not water.
    fn surface(&self, c: [i32; 3]) -> f64 {
        let mut y = c[1];
        while self.map.at([c[0], y, c[2]]) == WET && y < Y1 {
            y += 1;
        }
        y as f64
    }

    /// Where it floats in water at a cell: its back just out of it.
    fn afloat(&self, c: [i32; 3]) -> V3 {
        let s = self.surface(c);
        [c[0] as f64 + 0.5, (s - 0.65).max(c[1] as f64), c[2] as f64 + 0.5]
    }

    /// A dive into the water at a cell: a high hop, head first.
    fn dive_in(&mut self, c: [i32; 3]) -> Act {
        Act::Leap { to: self.afloat(c), rise: self.rng.range(3.0, 5.0), trick: Trick::flips(if self.rng.f64() < 0.4 { 0.5 } else { 0.0 }) }
    }

    /// Out of the water near a cell, onto the bank, in a hop; in smoke where it cannot hop out.
    fn out_of_water(&mut self, c: [i32; 3]) -> Act {
        match self.near_spot(spot_feet(c), 1.5, 6.0) {
            Some(n) => Act::Seq(vec![Act::Leap { to: spot_feet(n), rise: 1.5, trick: Trick::NONE }].into()),
            None => match self.near_spot(spot_feet(c), 0.0, 30.0) {
                Some(n) => Act::Poof { to: n, stay: 0.3 },
                None => Act::Next,
            },
        }
    }

    /// Swimming about the pond from a cell of it, `left` seconds.
    fn swim_about(&mut self, c: [i32; 3], left: f64) -> Act {
        let near: Vec<[i32; 3]> = self.pond.iter().copied().filter(|o| across_ground(spot_feet(*o), spot_feet(c)) < 8.0).collect();
        let pts: Vec<[f64; 2]> = (0..8).filter_map(|_| self.pick(&near).copied()).map(|o| [o[0] as f64 + 0.5, o[2] as f64 + 0.5]).collect();
        Act::Swim { pts, i: 0, left }
    }

    /// A step of swimming: on to the next point over water, its back just out of it, its legs
    /// paddling; past water it does not go.
    fn swim(&mut self, pts: Vec<[f64; 2]>, i: usize, left: f64) -> Act {
        if left <= 0.0 || i >= pts.len() || self.map.at(self.at.map(|v| v.floor() as i32)) != WET {
            return Act::Next;
        }
        let to = pts[i];
        let d = [to[0] - self.at[0], to[1] - self.at[2]];
        let far = d[0].hypot(d[1]);
        if far < 0.2 {
            return Act::Swim { pts, i: i + 1, left: left - STEP };
        }
        let f = level(d);
        let k = (0.9 * STEP).min(far);
        let (nx, nz) = (self.at[0] + f[0] * k, self.at[2] + f[2] * k);
        let cell = [nx.floor() as i32, self.at[1].floor() as i32, nz.floor() as i32];
        if self.map.at(cell) != WET {
            return Act::Swim { pts, i: i + 1, left: left - STEP };
        }
        let y = self.afloat(cell)[1] + (self.t * 2.4).sin() * 0.04;
        (self.at, self.up, self.fwd) = ([nx, y, nz], Y, f);
        Act::Swim { pts, i, left: left - STEP }
    }

    /// A step of walking across the ground through points (`Act::Walk`).
    #[allow(clippy::too_many_arguments)]
    fn walk_on(&mut self, pts: Vec<[f64; 2]>, i: usize, speed: f64, back: bool, pause: f64, wait: f64, climb: bool, left: f64, best: f64, since: f64) -> Act {
        let rest = |pts: Vec<[f64; 2]>, i, wait, best, since| Act::Walk { pts, i, speed, back, pause, wait, climb, left: left - STEP, best, since };
        if i >= pts.len() {
            return Act::Next;
        }
        if wait > 0.0 {
            return rest(pts, i, wait - STEP, best, since);
        }
        let to = pts[i];
        let (x, y, z) = (self.at[0], self.at[1], self.at[2]);
        let d = [to[0] - x, to[1] - z];
        let far = d[0].hypot(d[1]);
        // given up: no nearer for a long while, or too long at it; it is a pig
        let (best, since) = if far < best - 0.5 { (far, 0.0) } else { (best, since + STEP) };
        if left <= 0.0 || since > 12.0 {
            if climb && i + 1 == pts.len() {
                let c = [to[0].floor() as i32, self.map.top(to[0], to[1]), to[1].floor() as i32];
                if self.map.within(c) && self.map.at(c) != SOLID {
                    return Act::Poof { to: c, stay: 0.3 };
                }
            }
            return Act::Next;
        }
        if far < 0.15 {
            return rest(pts, i + 1, pause, f64::MAX, 0.0);
        }
        let f = level(d);
        let k = (speed * STEP).min(far);
        let (nx, nz) = (x + f[0] * k, z + f[2] * k);
        if self.map.column(nx.floor() as i32, nz.floor() as i32).is_none() {
            return rest(pts, i + 1, 0.0, f64::MAX, 0.0);
        }
        let face = if back { mul(f, -1.0) } else { f };
        if self.off(face).abs() > 0.6 && self.turn_rate.is_finite() {
            // far round from where it faces: it turns on the spot before it sets off
            self.turn_toward(face);
            return rest(pts, i, 0.0, best, since);
        }
        let top = self.map.top(nx, nz) as f64;
        if top > y + 1.01 {
            if !climb {
                // in its way: on to the next point
                return rest(pts, i + 1, 0.0, f64::MAX, 0.0);
            }
            // a step up it can hop, a wall it walks up
            let cell = [nx.floor() as i32, top as i32, nz.floor() as i32];
            let onto = spot_feet(cell);
            let more = rest(pts, i, 0.0, best, since);
            if top - y <= 3.5 && self.clear_leap(self.at, onto, 1.0, Trick::NONE) {
                return Act::Seq(vec![self.leap_with(onto, 1.0, Trick::NONE, 0, None, Act::Next), more].into());
            }
            let axis = if f[0].abs() > f[2].abs() { [f[0].signum() as i32, 0, 0] } else { [0, 0, f[2].signum() as i32] };
            return Act::Seq(vec![Act::Wander { fwd: axis, left: 1.0 }, more].into());
        }
        if top < y - 1.01 {
            // off an edge: a step out over it, and down
            self.at = [nx, y, nz];
            self.fwd = face;
            return Act::Seq(vec![Act::Drop { v: 0.0 }, rest(pts, i, 0.0, best, since)].into());
        }
        (self.at, self.up, self.fwd) = ([nx, top, nz], Y, face);
        rest(pts, i, 0.0, best, since)
    }

    /// Holding itself some way where it stands, `secs` seconds: turned to the viewer with `face`. None
    /// where its body would not be clear, held so.
    fn pose_act(&mut self, hold: Hold, secs: f64, face: bool) -> Option<Act> {
        if !self.upright() {
            return None;
        }
        let feet = self.at;
        let f = if face { level([self.eye[0] - feet[0], self.eye[2] - feet[2]]) } else { level([self.fwd[0], self.fwd[2]]) };
        let clear = (0..=20).all(|k| {
            let (a, u, w) = held(hold, secs * k as f64 / 20.0, feet, f);
            body_clear(&self.map, a, u, w)
        });
        let held = Act::Held { hold, left: secs, t: 0.0, feet, face: f };
        if !clear {
            return None;
        }
        Some(if self.off(f).abs() > 0.05 { Act::Seq(vec![Act::Turn { face: f }, held].into()) } else { held })
    }

    /// Up into the viewer's face (`Act::Peek`); None where it would be inside something there.
    fn peek(&mut self) -> Option<Act> {
        let hold = self.rng.range(2.5, 4.5);
        let spread = self.rng.range(-0.35, 0.35);
        for d in [2.6, 3.0, 3.4, 2.3, 3.8] {
            let h = spread * d * self.cam.tan * self.cam.aspect;
            let clear = (0..=24).all(|k| {
                let (a, u, f) = self.peek_pose(d, h, k as f64 / 24.0, 0.0);
                body_clear(&self.map, a, u, f)
            });
            if clear {
                return Some(Act::Peek { s: 0.0, d, h, hold });
            }
        }
        None
    }

    /// Its pose peeking: `k` of the way up from out of sight below the bottom of the screen (0) to
    /// where it looks at the viewer (1), its head tilted by `tilt`.
    fn peek_pose(&self, d: f64, h: f64, k: f64, tilt: f64) -> (V3, V3, V3) {
        let cam = &self.cam;
        let below = -(d * cam.tan + 1.4);
        let shown = -d * cam.tan * 0.42;
        let v = below + (shown - below) * k;
        let mid = add(add(add(cam.eye, mul(cam.forward, d)), mul(cam.right, h)), mul(cam.up, v));
        let fwd = mul(cam.forward, -1.0);
        let up = turn(cam.up, fwd, tilt);
        (add(add(mid, mul(up, -MIDDLE)), mul(fwd, -0.25)), up, fwd)
    }

    /// A step of peeking (`Act::Peek`).
    fn peek_on(&mut self, s: f64, d: f64, h: f64, hold: f64) -> Act {
        const HIDE: f64 = 0.8;
        const RISE: f64 = 0.8;
        const SINK: f64 = 0.7;
        if s == 0.0 {
            self.puff();
            self.seen = false;
        }
        let s2 = s + STEP;
        if s2 < HIDE {
            return Act::Peek { s: s2, d, h, hold };
        }
        let t = s2 - HIDE;
        let ease = |x: f64| x * x * (3.0 - 2.0 * x);
        let (k, tilt) = if t < RISE {
            (ease(t / RISE), 0.0)
        } else if t < RISE + hold {
            // looking at the viewer, its head tilted one way and the other
            (1.0, 0.22 * ((t - RISE) * 1.7).sin())
        } else if t < RISE + hold + SINK {
            (1.0 - ease((t - RISE - hold) / SINK), 0.0)
        } else {
            self.seen = false;
            let to = self.landing();
            return Act::Gone { left: 0.6, to, then: Box::new(Act::Stand { left: 1.0 }), fall: false };
        };
        let pose = self.peek_pose(d, h, k, tilt);
        if !self.seen {
            self.seen = true;
            self.shown = [pose.0, pose.1, pose.2];
        }
        (self.at, self.up, self.fwd) = pose;
        Act::Peek { s: s2, d, h, hold }
    }

    /// Where it comes down in a puff of smoke, when not to eat: on a roof one time in four (`ROOF`).
    fn landing(&mut self) -> [i32; 3] {
        let roof = !self.roofs.is_empty() && (self.plain.is_empty() || self.rng.f64() < ROOF);
        let from = if roof { &self.roofs } else { &self.plain };
        if from.is_empty() {
            // nowhere seen to come down: the farm's middle
            let (x, z) = (self.centre[0], self.centre[1]);
            return [x.floor() as i32, self.map.top(x, z), z.floor() as i32];
        }
        *self.rng.pick(from)
    }

    /// A point past the side of the screen, `far` ahead and `height` up: on the left with `side` -1.
    fn off_side(&self, far: f64, height: f64, side: f64) -> V3 {
        let right = [self.ahead[2], 0.0, -self.ahead[0]];
        let middle = add(self.eye, mul(self.ahead, far));
        add(middle, add(mul(right, (far * self.wide + 3.0) * side), [0.0, height, 0.0]))
    }

    /// Pigs can fly, many ways (`Flight`): mostly in from one side of the screen, across the sky and
    /// out of the other; round the farm; up off the ground and away; down onto a roof or into the
    /// pond. None where no way of them is clear of everything.
    fn fly(&mut self) -> Option<Act> {
        let mut ways = FLIGHTS.to_vec();
        self.rng.shuffle(&mut ways);
        // the plain way across comes first more often than the others
        if self.rng.f64() < 0.3 {
            ways.insert(0, Flight::Across);
        }
        for way in ways {
            let side = if self.rng.f64() < 0.5 { 1.0 } else { -1.0 };
            let far = self.rng.range(22.0, 45.0);
            let height = self.rng.range(1.0, 6.0);
            let mut pts: Vec<V3>;
            let (mut rolls, mut bob, mut then, mut start_seen) = (0.0, false, None, false);
            match way {
                Flight::Across => {
                    let drift = self.rng.range(-1.5, 1.5);
                    pts = vec![self.off_side(far, height, -side), self.off_side(far, height + drift, side)];
                    bob = true;
                }
                Flight::Swoop => {
                    let low = self.map.top(self.centre[0], self.centre[1]) as f64 + 2.5;
                    let mid = add(self.eye, mul(self.ahead, far));
                    pts = vec![self.off_side(far, height + 8.0, -side), [mid[0], low, mid[2]], self.off_side(far, height + 8.0, side)];
                }
                Flight::Overhead => {
                    let start = add(add(self.eye, mul(self.ahead, 45.0)), [self.rng.range(-6.0, 6.0), 4.0, 0.0]);
                    pts = vec![start, add(self.eye, [0.0, 2.5, 0.0]), add(add(self.eye, mul(self.ahead, -12.0)), [0.0, 2.0, 0.0])];
                }
                Flight::Orbit => {
                    let r = self.rng.range(12.0, 20.0);
                    let y = self.map.top(self.centre[0], self.centre[1]) as f64 + self.rng.range(6.0, 9.0);
                    let a0 = self.rng.range(0.0, TAU);
                    pts =
                        (0..=20).map(|k| a0 + side * k as f64 / 16.0 * TAU).map(|a| [self.centre[0] + r * a.cos(), y, self.centre[1] + r * a.sin()]).collect();
                    pts.push(add(pts[pts.len() - 1], [0.0, 30.0, 0.0]));
                    start_seen = true;
                }
                Flight::Looping => {
                    let mid = add(self.eye, mul(self.ahead, far));
                    let right = [self.ahead[2] * side, 0.0, -self.ahead[0] * side];
                    let c = add(mid, [0.0, height + 3.0, 0.0]);
                    pts = vec![self.off_side(far, height, -side)];
                    for k in 0..=8 {
                        let a = -PI / 2.0 + k as f64 / 8.0 * TAU;
                        pts.push(add(add(c, mul(right, a.cos() * 3.0 + k as f64 * 0.2)), [0.0, a.sin() * 3.0, 0.0]));
                    }
                    pts.push(self.off_side(far, height, side));
                }
                Flight::Corkscrew => {
                    pts = vec![self.off_side(far, height, -side), self.off_side(far, height + 1.0, side)];
                    rolls = self.rng.int(2, 4) as f64;
                }
                Flight::TakeOff => {
                    if !self.upright() {
                        continue;
                    }
                    let f = self.fwd;
                    let at = self.at;
                    let p = add(at, mul(Y, MIDDLE));
                    pts = vec![
                        p,
                        add(add(p, mul(f, 2.5)), [0.0, 0.6, 0.0]),
                        add(add(p, mul(f, 7.0)), [0.0, 4.0, 0.0]),
                        add(self.off_side(far, height + 10.0, side), [0.0, 0.0, 0.0]),
                    ];
                    start_seen = true;
                }
                Flight::RoofLanding => {
                    let Some(&c) = self.pick(&self.roofs.clone()) else { continue };
                    let roof = add(spot_feet(c), mul(Y, MIDDLE));
                    pts = vec![self.off_side(far, height + 6.0, -side), add(roof, [0.0, 5.0, 0.0]), add(roof, [0.0, 1.0, 0.0]), roof];
                    then = Some(Box::new(Act::Stand { left: self.rng.range(1.0, 2.5) }));
                }
                Flight::Slalom => {
                    if self.houses.len() < 2 {
                        continue;
                    }
                    let y = self.map.top(self.centre[0], self.centre[1]) as f64 + 6.0;
                    let mut ms: Vec<[f64; 2]> = self.houses.iter().map(House::middle).collect();
                    let right = [self.ahead[2], -self.ahead[0]];
                    ms.sort_by(|a, b| (side * (a[0] * right[0] + a[1] * right[1])).total_cmp(&(side * (b[0] * right[0] + b[1] * right[1]))));
                    pts = vec![self.off_side(far, height + 4.0, -side)];
                    for (k, m) in ms.iter().enumerate() {
                        let off = if k % 2 == 0 { 5.0 } else { -5.0 };
                        pts.push([m[0] + self.ahead[0] * off, y, m[1] + self.ahead[2] * off]);
                    }
                    pts.push(self.off_side(far, height + 4.0, side));
                }
                Flight::PondDive => {
                    let Some(&c) = self.pick(&self.pond.clone()) else { continue };
                    let water = add(self.afloat(c), mul(Y, MIDDLE));
                    pts = vec![self.off_side(far, height + 8.0, -side), add(water, [0.0, 6.0, 0.0]), add(water, [0.0, 2.0, 0.0]), water];
                    let v0 = self.rng.range(6.0, 12.0);
                    let swim = self.swim_about(c, v0);
                    let out = self.out_of_water(c);
                    then = Some(Box::new(Act::Seq(vec![swim, out, Act::Pose { hold: Hold::Shake, secs: 1.5, face: false }].into())));
                }
            }
            let (_, _, length) = curve(&pts, 0.0);
            let time = length / FLY;
            if !self.fly_clear(&pts, time, rolls, bob) {
                continue;
            }
            // gone in a puff from where it was, unless it takes off from there; and out of one where it
            // starts in sight
            if way != Flight::TakeOff {
                self.puff();
            }
            let (p, d, _) = curve(&pts, 0.0);
            self.at = add(p, mul(Y, -MIDDLE));
            self.up = Y;
            self.fwd = d;
            self.shown = [self.at, self.up, self.fwd];
            if start_seen && way != Flight::TakeOff {
                self.puff();
            }
            self.seen = true;
            return Some(Act::Fly { pts, s: 0.0, time, rolls, bob, up: Y, then });
        }
        None
    }

    /// Whether a flight through `pts`, `time` seconds long, keeps the whole of its body clear of
    /// everything: flown step by step as `Act::Fly` flies it.
    fn fly_clear(&self, pts: &[V3], time: f64, rolls: f64, bob: bool) -> bool {
        let mut up = Y;
        let mut s = 0.0;
        while s < time {
            s += STEP;
            let k = (s / time).min(1.0);
            let (p, d, _) = curve(pts, k);
            let u = add(up, mul(d, -dot(up, d)));
            let u = if dot(u, u) < 1e-6 { norm(add(Y, mul(d, -dot(Y, d)))) } else { norm(u) };
            up = u;
            let wobble = if bob { (s * 2.6).sin() * 0.3 } else { 0.0 };
            let r = turn(u, d, rolls * TAU * k);
            if !body_clear(&self.map, add(add(p, [0.0, wobble, 0.0]), mul(r, -MIDDLE)), r, d) {
                return false;
            }
        }
        true
    }

    /// Up in a puff of smoke, and down in another at `to` a moment later.
    fn gone(&mut self, to: [i32; 3], then: Act) -> Act {
        self.puff();
        self.seen = false;
        Act::Gone { left: self.rng.range(0.4, 1.1), to, then: Box::new(then), fall: false }
    }

    /// Off in a straight line, any of the four ways.
    fn crawl(&mut self) -> Act {
        let ways = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
        let fwd = *self.rng.pick(&ways);
        let left = self.rng.range(5.0, 14.0);
        let cell = [self.at[0].floor() as i32, (self.at[1] + 0.5).floor() as i32, self.at[2].floor() as i32];
        if self.map.solid(cell) || !self.map.solid(iadd(cell, [0, -1, 0])) {
            return self.hop(2, None, Act::Next);
        }
        Act::Wander { fwd, left }
    }

    /// A hop from where it is: towards `toward` (onto it, near enough), or to a place nearby; `more`
    /// after it, then `then`. Only where the whole of the hop is clear of everything: else, towards a
    /// place, it goes in smoke; to a place nearby, it tries others, and stays where it is if none is.
    fn hop(&mut self, more: u32, toward: Option<[i32; 3]>, then: Act) -> Act {
        let here = self.at;
        // most hops low, some nearly as high as the roofs; now and then head over heels
        let rise = if self.rng.f64() < 0.65 { self.rng.range(1.0, 3.0) } else { self.rng.range(4.0, 7.5) };
        let flips = match self.rng.f64() {
            f if f < 0.15 => 1.0,
            f if f < 0.2 => -1.0,
            f if f < 0.24 && rise > 4.0 => 2.0,
            _ => 0.0,
        };
        match toward {
            Some(c) if across_ground(spot_feet(c), here) <= 6.0 => {
                let big = self.rng.f64() < 0.3;
                let rise = if big { 4.0 } else { 1.2 };
                if self.clear_leap(here, spot_feet(c), rise, Trick::NONE) {
                    return self.leap(spot_feet(c), rise, 0.0, 0, None, then);
                }
                self.gone(c, then)
            }
            Some(c) => {
                let d = level([spot_feet(c)[0] - here[0], spot_feet(c)[2] - here[2]]);
                for _ in 0..4 {
                    let k = self.rng.range(3.0, 6.0);
                    let (x, z) = (here[0] + d[0] * k, here[2] + d[2] * k);
                    let to = [x, self.map.top(x, z) as f64, z];
                    if self.map.column(x.floor() as i32, z.floor() as i32).is_some() && self.clear_leap(here, to, rise, Trick::flips(flips)) {
                        return self.leap(to, rise, flips, more, toward, then);
                    }
                }
                self.gone(c, then)
            }
            None => {
                let near: Vec<[i32; 3]> = self.spots.iter().copied().filter(|c| (2.0..=7.0).contains(&across_ground(spot_feet(*c), here))).collect();
                for _ in 0..6 {
                    let to = match near.is_empty() {
                        false => spot_feet(*self.rng.pick(&near)),
                        // nowhere seen near: back towards the farm
                        true => {
                            let d = level([self.centre[0] - here[0], self.centre[1] - here[2]]);
                            let (x, z) = (here[0] + d[0] * 3.0, here[2] + d[2] * 3.0);
                            [x, self.map.top(x, z) as f64, z]
                        }
                    };
                    if self.clear_leap(here, to, rise, Trick::flips(flips)) {
                        return self.leap(to, rise, flips, more, toward, then);
                    }
                }
                then
            }
        }
    }

    /// The way a hop from `from` to `to` faces: along it, or the way it faces for one straight up.
    fn hop_way(&self, from: V3, to: V3) -> V3 {
        if across_ground(from, to) < 1e-6 { level([self.fwd[0], self.fwd[2]]) } else { level([to[0] - from[0], to[2] - from[2]]) }
    }

    /// Whether a hop from `from` to `to` (its height there the ground's where it is 0), `rise` high,
    /// doing `trick`, keeps the whole of its body clear of everything all the way.
    fn clear_leap(&self, from: V3, to: V3, rise: f64, trick: Trick) -> bool {
        let to = self.grounded(to);
        let (v, g, time) = plan(from, to, rise);
        let fwd0 = self.hop_way(from, to);
        let n = (time / 0.03).ceil().max(2.0) as usize;
        (0..=n).all(|k| {
            let (a, u, f) = hop_pose(from, to, v, g, time * k as f64 / n as f64, time, trick, fwd0);
            body_clear(&self.map, a, u, f)
        })
    }

    /// A place to hop to, on the ground where its height is not given (0).
    fn grounded(&self, to: V3) -> V3 {
        if to[1] == 0.0 { [to[0], self.map.top(to[0], to[2]) as f64, to[2]] } else { to }
    }

    /// A hop to `to`, `rise` blocks over the higher of its ends: never quicker across the ground than
    /// `HOP`, floating the longer for it.
    fn leap(&mut self, to: V3, rise: f64, flips: f64, more: u32, toward: Option<[i32; 3]>, then: Act) -> Act {
        self.leap_with(to, rise, Trick::flips(flips), more, toward, then)
    }

    fn leap_with(&mut self, to: V3, rise: f64, trick: Trick, more: u32, toward: Option<[i32; 3]>, then: Act) -> Act {
        let from = self.at;
        let to = self.grounded(to);
        let (v, g, time) = plan(from, to, rise);
        let hop = Act::Hop { from, to, v, g, time, s: 0.0, trick, more, toward, then: Box::new(then) };
        // standing, it turns on the spot to the way it hops first
        let way = self.hop_way(from, to);
        if self.upright() && self.off(way).abs() > 0.05 && self.turn_rate.is_finite() {
            return Act::Seq(vec![Act::Turn { face: way }, hop].into());
        }
        hop
    }

    /// A step of rolling downhill: the steepest way down, gathering speed, slowing where it levels out.
    fn tumble(&mut self, dir: [f64; 2], speed: f64, roll: f64, flat: f64, left: f64) -> Act {
        let (x, z) = (self.at[0], self.at[2]);
        let here = self.map.top(x, z);
        let mut best = (0, dir);
        for k in 0..8 {
            let a = k as f64 * PI / 4.0;
            let (dx, dz) = (a.cos(), a.sin());
            let drop = here - self.map.top(x + dx * 1.5, z + dz * 1.5);
            if drop > best.0 {
                best = (drop, [dx, dz]);
            }
        }
        let want = best.1;
        let dir = if dir == [0.0, 0.0] { want } else { [dir[0] * 0.85 + want[0] * 0.15, dir[1] * 0.85 + want[1] * 0.15] };
        let d = level(dir);
        let speed = if best.0 > 0 { (speed + 6.0 * STEP).min(ROLL) } else { (speed - 5.0 * STEP).max(0.0) };
        let (nx, nz) = (x + d[0] * speed * STEP, z + d[2] * speed * STEP);
        // never up a wall, nor off what it knows: it stops against it
        // (its body turning about its middle reaches half a block about it)
        let edge = (0..4).any(|k| {
            let (ex, ez) = (nx + [0.5, -0.5, 0.0, 0.0][k], nz + [0.0, 0.0, 0.5, -0.5][k]);
            self.map.column(ex.floor() as i32, ez.floor() as i32).is_none()
        });
        let stop = edge || self.map.top(nx, nz) > here + 1;
        let (nx, nz) = if stop { (x, z) } else { (nx, nz) };
        let ground = self.map.top(nx, nz) as f64;
        // off a ledge higher than a step: it falls, rather than dropping through the air at once
        if !stop && ground < here as f64 - 1.01 {
            (self.at, self.up, self.fwd) = ([nx, here as f64, nz], Y, d);
            return Act::Seq(vec![Act::Drop { v: speed * 0.3 }, Act::Pose { hold: Hold::Dizzy, secs: 2.0, face: false }].into());
        }
        let roll = roll + speed * STEP / MIDDLE;
        let y = ground + roll.sin().abs() * 0.25 * (speed / ROLL);
        let (up, fwd) = heels(Y, d, roll);
        // turning about its middle, lifted by as much as its lowest corner would be under the ground:
        // a box rolling, its ends knocking it up as it goes over them
        let low =
            [-HIND, LENGTH - HIND].iter().flat_map(|&z| [0.0, TALL].map(move |y| MIDDLE + (y - MIDDLE) * roll.cos() - z * roll.sin())).fold(f64::MAX, f64::min);
        let lift = (-low).max(0.0) + 0.02;
        self.at = add(add([nx, y + lift, nz], mul(Y, MIDDLE)), mul(up, -MIDDLE));
        (self.up, self.fwd) = (up, fwd);
        let flat = if best.0 > 0 { 0.0 } else { flat + STEP };
        let wet = self.wet([nx, ground + 0.2, nz]);
        if left <= 0.0 || wet || stop || (flat > 0.6 && speed < 0.3) {
            // back on its feet, dizzy: round and round
            (self.at, self.up, self.fwd) = ([nx, ground, nz], Y, d);
            let r = 1.0;
            let side = cross(Y, d);
            let centre = [nx + side[0] * r, nz + side[2] * r];
            return Act::Circle { centre, r, angle: (nz - centre[1]).atan2(nx - centre[0]), turn: RUN / r, left: 2.5 };
        }
        Act::Tumble { dir: [d[0], d[2]], speed, roll, flat, left: left - STEP }
    }
}

/// A hop from `from` to `to` rising `rise` over the higher end: its speed up, its pull down and how
/// long it takes. Never quicker across the ground than `HOP`; floating the longer for it.
fn plan(from: V3, to: V3, rise: f64) -> (f64, f64, f64) {
    let peak = from[1].max(to[1]) + rise;
    let timed = |g: f64| ((2.0 * g * (peak - from[1])).sqrt(), (2.0 * (peak - from[1]) / g).sqrt() + (2.0 * (peak - to[1]) / g).sqrt());
    let (_, quick) = timed(GRAVITY);
    let least = across_ground(from, to) / HOP;
    // the time goes as one over the root of the fall
    let g = if quick < least { GRAVITY * (quick / least).powi(2) } else { GRAVITY };
    let (v, time) = timed(g);
    (v, g, time)
}

/// Its pose `s` seconds into a hop (`Act::Hop`): turning about its middle as the trick goes, which
/// starts and ends upright.
#[allow(clippy::too_many_arguments)]
fn hop_pose(from: V3, to: V3, v: f64, g: f64, s: f64, time: f64, trick: Trick, fwd0: V3) -> (V3, V3, V3) {
    let k = (s / time).clamp(0.0, 1.0);
    let p = [from[0] + (to[0] - from[0]) * k, from[1] + v * s - g * s * s / 2.0, from[2] + (to[2] - from[2]) * k];
    let f = turn(fwd0, Y, trick.spins * TAU * k);
    let u = turn(Y, f, trick.rolls * TAU * k);
    let (up, fwd) = heels(u, f, trick.flips * TAU * k);
    (add(add(p, mul(Y, MIDDLE)), mul(up, -MIDDLE)), up, fwd)
}

/// Its pose holding itself some way (`Hold`), `t` seconds into it, its feet at `feet`, facing `f`.
fn held(hold: Hold, t: f64, feet: V3, f: V3) -> (V3, V3, V3) {
    // tipped up about its hind end, or down about its snout, by `a`
    let back_up = |a: f64| {
        let (u, w) = heels(Y, f, -a);
        (add(add(feet, mul(f, -HIND)), mul(w, HIND)), u, w)
    };
    let nose_down = |a: f64| {
        let front = LENGTH - HIND;
        let (u, w) = heels(Y, f, a);
        (add(add(feet, mul(f, front)), mul(w, -front)), u, w)
    };
    // rolled over sideways by `r` about its middle, lifted so its sides stay out of the ground
    let rolled = |r: f64, f: V3| {
        let u = turn(Y, f, r);
        let lift = HALF_W * r.sin().abs() + (1.0 - r.cos()) * MIDDLE * 0.2 + 0.01;
        (add(add(add(feet, mul(Y, MIDDLE + lift)), mul(u, -MIDDLE)), [0.0; 3]), u, f)
    };
    match hold {
        Hold::Stare | Hold::Headbang => (feet, Y, f),
        Hold::Sit => back_up(0.75 * (t * 3.0).min(1.0)),
        Hold::Beg => back_up(1.2 * (t * 2.5).min(1.0) + 0.05 * (t * 2.0).sin()),
        Hold::Headstand => nose_down(1.45 * (t * 2.0).min(1.0)),
        Hold::Bow => nose_down(0.35 * (t * 4.0).min(1.0)),
        Hold::Sleep => {
            let u = cross(f, Y);
            let breath = 0.02 * (t * 1.6).sin();
            (add(feet, [0.0, HALF_W + 0.01 + breath, 0.0]), u, f)
        }
        Hold::Back => (add(feet, [0.0, TALL + 0.01, 0.0]), mul(Y, -1.0), f),
        Hold::Shake => rolled(0.35 * (t * 26.0).sin(), f),
        Hold::Dizzy => {
            let f2 = turn(f, Y, t * 0.9);
            rolled(0.22 * (t * 4.0).sin(), f2)
        }
        Hold::Spin => (feet, Y, turn(f, Y, t * 5.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(a: V3, b: V3) -> f64 {
        (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
    }

    /// A floor at GROUND, and on it a house: walls from x 10 to 13, three high, its roof a block wider
    /// all round.
    fn yard() -> Map {
        Map::from_fn(0, 0, 32, |x, y, z| {
            let wall = (10..=13).contains(&x) && (2..=6).contains(&z) && (GROUND..GROUND + 3).contains(&y);
            let roof = (9..=14).contains(&x) && (1..=7).contains(&z) && y == GROUND + 3;
            if y < GROUND || wall || roof { SOLID } else { OPEN }
        })
    }

    #[test]
    fn it_walks_straight_up_a_wall_under_the_eaves_over_the_roof_and_down() {
        let map = yard();
        let mut c = Crawl { cell: [2, GROUND, 4], up: [0, 1, 0], fwd: [1, 0, 0], along: 0.0 };
        let (mut ups, mut highest, mut last) = (vec![], 0, c.feet());
        for _ in 0..400 {
            c = c.on(&map, 0.1);
            let f = c.feet();
            // never through anything: its body in the open, its feet on a face
            assert!(!map.solid(c.cell), "inside a block at {:?}", c.cell);
            assert!(map.solid(iadd(c.cell, ineg(c.up))), "nothing under it at {c:?}");
            assert!((0..3).map(|i| (f[i] - last[i]).abs()).sum::<f64>() < 0.25, "a jump at {c:?}");
            assert_eq!(f[2], 4.5, "it keeps to its line");
            last = f;
            if ups.last() != Some(&c.up) {
                ups.push(c.up);
            }
            highest = highest.max(c.cell[1]);
            if c.cell[0] > 18 {
                break;
            }
        }
        assert!(c.cell[0] > 18 && c.up == [0, 1, 0] && c.cell[1] == GROUND, "on across the yard: {c:?}");
        assert_eq!(highest, GROUND + 4, "over the roof");
        // up the wall, upside down under the eaves, round their edge, over the roof, down round its
        // far edge, under the far eaves and down the wall
        assert_eq!(ups, [[0, 1, 0], [-1, 0, 0], [0, -1, 0], [-1, 0, 0], [0, 1, 0], [1, 0, 0], [0, -1, 0], [1, 0, 0], [0, 1, 0]]);
    }

    /// A house as the farm builds them: walls from x 10 to 16 and z 4 to 8 up to `top`, a roof of
    /// steps rising to a ridge along x, a block wider all round, and the gable ends filled in.
    fn cottage() -> Map {
        let (x0, x1, z0, z1, top) = (10, 16, 4, 8, GROUND + 3);
        Map::from_fn(0, 0, 32, move |x, y, z| {
            let wall = (x == x0 || x == x1 || z == z0 || z == z1) && (x0..=x1).contains(&x) && (z0..=z1).contains(&z) && (GROUND..=top).contains(&y);
            let k = y - top;
            let (a, b) = (z0 - 1 + k, z1 + 1 - k);
            let roof = k >= 0 && a <= b && (x0 - 1..=x1 + 1).contains(&x) && (z == a || z == b);
            let gable = k >= 1 && (x == x0 || x == x1) && z > a && z < b;
            if y < GROUND || wall || roof || gable { SOLID } else { OPEN }
        })
    }

    /// Whether any of its body, posed so, is inside a block.
    fn inside(map: &Map, (at, up, fwd): (V3, V3, V3)) -> Option<V3> {
        let (len, tall) = (24, 15);
        for i in 0..=len {
            for j in 0..=tall {
                let z = -HIND + 0.02 + (LENGTH - 0.04) * i as f64 / len as f64;
                let y = 0.02 + (15.0 / 16.0 - 0.04) * j as f64 / tall as f64;
                let p = add(add(at, mul(fwd, z)), mul(up, y));
                if map.solid(p.map(|v| v.floor() as i32)) {
                    return Some(p);
                }
            }
        }
        None
    }

    #[test]
    fn its_body_keeps_out_of_walls_eaves_and_roofs() {
        let mut lines = vec![];
        {
            let out = &mut lines;
            for z in 2..11 {
                out.push(Crawl { cell: [3, GROUND, z], up: [0, 1, 0], fwd: [1, 0, 0], along: 0.0 });
            }
            for x in 8..19 {
                out.push(Crawl { cell: [x, GROUND, 0], up: [0, 1, 0], fwd: [0, 0, 1], along: 0.0 });
            }
        }
        for (map, name) in [(yard(), "shed"), (cottage(), "house")] {
            let map = &map;
            for &c in &lines {
                let mut c: Crawl = c;
                let mut was = c.lie(map);
                // on over the house and across the yard, short of where it turns back
                while c.cell[0].max(c.cell[2]) < 26 {
                    c = c.on(map, CRAWL * STEP);
                    let lie = c.lie(map);
                    if let Some(p) = inside(map, lie) {
                        panic!("{name}: its body in a block at {p:?}, feet {c:?}");
                    }
                    // smoothly: no jumps, and no sudden turns
                    assert!(dist(lie.0, was.0) < 0.3, "{name}: a jump at {c:?}");
                    assert!(dot(lie.1, was.1) > 0.8 && dot(lie.2, was.2) > 0.8, "{name}: a snap at {c:?}");
                    was = lie;
                }
            }
        }
    }

    /// Pictures of it crawling over the house, to look at: `RETRO_PIG_FRAMES=<dir> cargo test --release
    /// pig_frames -- --ignored`. Each way over the house is one sheet of moments along it; each moment
    /// is the house cut through along its line and seen from the side, the pig placed by its feet alone
    /// on the left, lying stiff along the way on the right. Whatever of it is inside a block, the block
    /// hides.
    #[test]
    #[ignore]
    fn pig_frames() {
        let Ok(dir) = std::env::var("RETRO_PIG_FRAMES") else { return };
        let map = cottage();
        let ways = [
            ("gable-ridge", Crawl { cell: [3, GROUND, 6], up: [0, 1, 0], fwd: [1, 0, 0], along: 0.0 }),
            ("gable-slope", Crawl { cell: [3, GROUND, 5], up: [0, 1, 0], fwd: [1, 0, 0], along: 0.0 }),
            ("long-side", Crawl { cell: [13, GROUND, 0], up: [0, 1, 0], fwd: [0, 0, 1], along: 0.0 }),
        ];
        let (w, h, cols) = (200usize, 150usize, 5usize);
        for (name, start) in ways {
            // the moments: where lying stiff differs from standing on its feet, at the corners, every
            // so far along, until it is down off the far side
            let mut c = start;
            let mut moments = vec![];
            let mut gap = f64::MAX;
            while c.cell[0].max(c.cell[2]) < 24 {
                let lie = c.lie(&map);
                let same = dist(lie.0, c.feet()) < 1e-6 && dot(lie.1, f3(c.up)) > 1.0 - 1e-9;
                if !same && gap >= 0.3 {
                    moments.push(c);
                    gap = 0.0;
                }
                c = c.on(&map, 0.05);
                gap += 0.05;
            }
            let rows = moments.len().div_ceil(cols);
            // seen square across the line, from the side whose cells are cut away
            let across: V3 = if start.fwd[0] != 0 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
            let line = if start.fwd[0] != 0 { start.cell[2] } else { start.cell[0] };
            let yaw = across[0].atan2(across[2]);
            let cut = |cell: [i32; 3]| if across[2] != 0.0 { cell[2] < line } else { cell[0] < line };
            let (sw, sh) = (cols * (2 * w + 6), rows * (h + 6));
            let mut sheet = vec![0u32; sw * sh];
            for (k, m) in moments.iter().enumerate() {
                let old = (m.feet(), f3(m.up), f3(m.fwd));
                // close by, on the line, looking at where its feet are
                let feet = m.feet();
                let mid = if across[2] != 0.0 { [feet[0], feet[1] + 0.3, line as f64] } else { [line as f64, feet[1] + 0.3, feet[2]] };
                let eye = add(mid, mul(across, -6.0));
                let cam = Camera::new(eye, yaw, 0.0, 40.0, w, h);
                for (side, lie) in [old, m.lie(&map)].into_iter().enumerate() {
                    let pose = mobs::pose_turned(Kind::Pig, lie.0, [cross(lie.1, lie.2), lie.1, lie.2], Walk::STILL, 0.0, eye);
                    let (ox, oy) = ((k % cols) * (2 * w + 6) + side * w, (k / cols) * (h + 6));
                    for py in 0..h {
                        for px in 0..w {
                            let d = cam.ray(px as f64 + 0.5, py as f64 + 0.5);
                            let block = cast(&map, eye, d, &cut);
                            let pig = mobs::seen(Kind::Pig, lie.0, &pose, d);
                            let rgb = match (block, pig) {
                                (Some((t, _, _)), Some(p)) if p.t < t => shade(p.colour.map(f64::from), p.normal),
                                (_, Some(p)) if block.is_none() => shade(p.colour.map(f64::from), p.normal),
                                (Some((_, cell, n)), _) => {
                                    let base = if cell[1] < GROUND {
                                        [0.36, 0.6, 0.3]
                                    } else if cut(iadd(cell, [across[0] as i32, 0, across[2] as i32].map(|v| -v))) {
                                        [0.72, 0.56, 0.36]
                                    } else {
                                        [0.45, 0.35, 0.24]
                                    };
                                    shade(base, n)
                                }
                                _ => [0.55, 0.75, 0.95],
                            };
                            sheet[(oy + py) * sw + ox + px] = rgb.iter().fold(0, |a, v| a << 8 | (v.clamp(0.0, 1.0) * 255.0) as u32);
                        }
                    }
                }
            }
            let mut bytes: Vec<u8> = sheet.iter().flat_map(|v| v.to_ne_bytes()).collect();
            let stride = sw as i32 * 4;
            let surface = cairo::ImageSurface::create_for_data(std::mem::take(&mut bytes), cairo::Format::Rgb24, sw as i32, sh as i32, stride).unwrap();
            let mut f = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
            surface.write_to_png(&mut f).unwrap();
        }
    }

    /// Light by which way a face looks: tops brightest, bottoms darkest.
    fn shade(c: V3, n: V3) -> V3 {
        let k = 0.55 + 0.3 * n[1] + 0.15 * n[0].abs();
        c.map(|v| v * k)
    }

    /// The first block a ray from `o` along `d` meets, of those not cut away: how far, which, and the
    /// way out of the face it meets.
    fn cast(map: &Map, o: V3, d: V3, cut: &impl Fn([i32; 3]) -> bool) -> Option<(f64, [i32; 3], V3)> {
        let mut cell = o.map(|v| v.floor() as i32);
        let step = d.map(|v| if v > 0.0 { 1 } else { -1 });
        let delta = d.map(|v| if v == 0.0 { f64::MAX } else { 1.0 / v.abs() });
        let mut next = [0, 1, 2].map(|i| {
            let edge = if d[i] > 0.0 { cell[i] as f64 + 1.0 - o[i] } else { o[i] - cell[i] as f64 };
            if d[i] == 0.0 { f64::MAX } else { edge * delta[i] }
        });
        let mut normal = [0.0; 3];
        let mut t = 0.0;
        while t < 60.0 {
            if !cut(cell) && map.solid(cell) {
                return Some((t, cell, normal));
            }
            let i = if next[0] < next[1] && next[0] < next[2] {
                0
            } else if next[1] < next[2] {
                1
            } else {
                2
            };
            t = next[i];
            next[i] += delta[i];
            cell[i] += step[i];
            normal = [0.0; 3];
            normal[i] = -step[i] as f64;
        }
        None
    }

    #[test]
    fn at_the_edge_of_what_it_knows_it_turns_back() {
        let map = yard();
        let mut c = Crawl { cell: [28, GROUND, 20], up: [0, 1, 0], fwd: [1, 0, 0], along: 0.0 };
        for _ in 0..100 {
            c = c.on(&map, 0.1);
            assert!(map.within(c.cell));
        }
        assert_eq!(c.fwd, [-1, 0, 0]);
    }

    #[test]
    fn a_hop_lands_where_it_was_aimed() {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 96, 54);
        let mut pig = Pig::new(&w, &cam, 7, |_| true);
        let to = [pig.at[0] + 4.0, pig.at[1] + 1.0, pig.at[2] - 2.0];
        pig.act = pig.leap(to, 5.0, 1.0, 0, None, Act::Stand { left: 100.0 });
        let mut highest = f64::MIN;
        for _ in 0..200 {
            pig.step();
            pig.t += STEP;
            highest = highest.max(pig.at[1]);
        }
        assert!(matches!(pig.now(), Act::Stand { .. }));
        assert_eq!(pig.at, to);
        assert!(highest > to[1] + 4.5, "it rises: {highest}");
        assert!((pig.shown[0][0] - to[0]).abs() < 1e-3 && pig.shown[1][1] > 0.999, "shown standing where it landed");
    }

    #[test]
    fn seeing_nowhere_to_stand_it_keeps_going() {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 96, 54);
        let mut pig = Pig::new(&w, &cam, 7, |_| false);
        assert!(pig.spots.is_empty() && pig.crops.is_empty());
        for k in 0..(30.0 * 60.0 / STEP) as usize {
            pig.advance(k as f64 * STEP);
            assert!(pig.at.iter().all(|v| v.is_finite()));
        }
        let _ = pig.landing();
    }

    /// The farm's world, its camera and the pig in it, seeing anywhere in front of the camera.
    fn farm() -> (World, Camera) {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 192, 108);
        (w, cam)
    }

    fn farm_pig(w: &World, cam: &Camera, seed: i64) -> Pig {
        let c = *cam;
        Pig::new(w, cam, seed, move |p| c.project(p).is_some_and(|(x, y, _)| x > 0.0 && y > 0.0 && x < 192.0 && y < 108.0))
    }

    #[test]
    fn every_doing_runs_to_its_end_clear_of_everything() {
        let (w, cam) = farm();
        let mut started = vec![];
        for (i, &(name, _)) in DOINGS.iter().enumerate() {
            let mut pig = farm_pig(&w, &cam, 11 + i as i64);
            // standing on plain ground near the farm's middle
            let centre = spot_feet([pig.centre[0] as i32, 0, pig.centre[1] as i32]);
            let spot = *pig.plain.iter().min_by(|a, b| across_ground(spot_feet(**a), centre).total_cmp(&across_ground(spot_feet(**b), centre))).unwrap();
            pig.put(spot);
            let Some(a) = pig.start(i) else { continue };
            pig.act = a;
            let mut ended = false;
            for _ in 0..(300.0 / STEP) as usize {
                pig.step();
                pig.t += STEP;
                assert!(pig.at.iter().chain(pig.shown.iter().flatten()).all(|v| v.is_finite()), "{name}");
                if pig.seen && !matches!(pig.now(), Act::Crawl { .. }) {
                    assert!(body_clear(&pig.map, pig.at, pig.up, pig.fwd), "{name}: inside a block at {:?} {:?} {:?}", pig.at, pig.up, pig.fwd);
                }
                if matches!(pig.act, Act::Next) {
                    ended = true;
                    break;
                }
            }
            assert!(ended, "{name} ends");
            started.push(name);
        }
        assert!(started.len() + 3 >= DOINGS.len(), "nearly all can be done there: {} of {}: {started:?}", started.len(), DOINGS.len());
    }

    #[test]
    fn a_crawl_out_of_time_under_the_eaves_goes_on_to_its_feet() {
        let (w, cam) = farm();
        let mut pig = farm_pig(&w, &cam, 7);
        pig.map = yard();
        // up the shed's wall and under its eaves when its time is up, not hopping off through the roof
        pig.at = [2.5, GROUND as f64, 4.5];
        pig.act = Act::Crawl { c: Crawl { cell: [2, GROUND, 4], up: [0, 1, 0], fwd: [1, 0, 0], along: 0.0 }, left: 6.6 };
        // its time runs out on the wall or under the eaves, whichever it is on then
        let mut out_of_time_off_its_feet = false;
        for _ in 0..(60.0 / STEP) as usize {
            pig.step();
            pig.t += STEP;
            if let Act::Crawl { c, left } = pig.now() {
                out_of_time_off_its_feet |= *left <= 0.0 && c.up != [0, 1, 0];
            }
            if !matches!(pig.act, Act::Crawl { .. }) {
                break;
            }
        }
        assert!(out_of_time_off_its_feet, "its time ran out off its feet");
        assert!(pig.up[1] > 0.99, "it stopped on its feet: {:?}", pig.up);
        assert!(body_clear(&pig.map, pig.at, pig.up, pig.fwd));
    }

    #[test]
    fn it_turns_on_the_spot_at_its_pace() {
        let (w, cam) = farm();
        let mut pig = farm_pig(&w, &cam, 5);
        pig.fwd = [0.0, 0.0, 1.0];
        pig.turn_rate = 3.0;
        // half a turn at three radians a second: a little over a second, each step no more than its pace
        let face = [0.0, 0.0, -1.0];
        let mut steps = 0;
        while !pig.turn_toward(face) {
            steps += 1;
            assert!(steps < 100);
        }
        assert!((steps as f64 * STEP - PI / 3.0).abs() < 0.1, "{steps} steps");
        // and the other way round, the short way
        pig.fwd = [1.0, 0.0, 0.0];
        assert!(!pig.turn_toward([0.0, 0.0, 1.0]));
        assert!(pig.fwd[2] > 0.0 && pig.fwd[0] < 1.0, "towards it: {:?}", pig.fwd);
        // swivelling: at once
        pig.turn_rate = f64::INFINITY;
        assert!(pig.turn_toward([-1.0, 0.0, 0.0]) && pig.fwd == [-1.0, 0.0, 0.0]);
        // mostly at the herd's pace, now and then faster, now and then a swivel
        let total: f64 = TURNS.iter().map(|t| t.1).sum();
        assert!((TURNS[0].1 / total - 0.7).abs() < 1e-9 && (TURNS[1].1 / total - 0.2).abs() < 1e-9 && (TURNS[2].1 / total - 0.1).abs() < 1e-9);
    }

    #[test]
    fn it_peeks_up_into_the_viewers_face() {
        let (w, cam) = farm();
        let mut pig = farm_pig(&w, &cam, 3);
        let i = DOINGS.iter().position(|d| d.0 == "peek at the viewer").unwrap();
        pig.act = pig.start(i).expect("room before the eye");
        let (mut close, mut hid) = (0, false);
        for _ in 0..(12.0 / STEP) as usize {
            pig.step();
            pig.t += STEP;
            if !pig.seen {
                hid = true;
                continue;
            }
            if let Act::Peek { .. } = pig.now() {
                let mid = add(pig.at, mul(pig.up, MIDDLE));
                let d = dist(mid, cam.eye);
                if let Some((x, y, _)) = cam.project(mid)
                    && d < 4.5
                    && dot(pig.fwd, cam.forward) < -0.9
                    && x > 192.0 * 0.15
                    && x < 192.0 * 0.85
                    && y > 108.0 * 0.5
                    && y < 108.0
                {
                    close += 1;
                }
            }
        }
        assert!(hid, "gone in smoke first");
        assert!(close as f64 * STEP > 2.0, "in the lower middle of the screen, close, looking at the viewer: {close} steps");
        assert!(!matches!(pig.now(), Act::Peek { .. }), "and gone again");
    }

    #[test]
    fn it_does_everything_and_eats_most() {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 192, 108);
        // seen: anywhere in front of the camera
        let mut pig = Pig::new(&w, &cam, 7, |p| cam.project(p).is_some_and(|(x, y, _)| x > 0.0 && y > 0.0 && x < 192.0 && y < 108.0));
        assert!(
            pig.crops.len() > 50 && pig.spots.len() > 500 && !pig.peaks.is_empty() && pig.roofs.len() > 20,
            "{} {} {} {}",
            pig.crops.len(),
            pig.spots.len(),
            pig.peaks.len(),
            pig.roofs.len()
        );
        let mut did = [0usize; 10];
        let mut doings = std::collections::HashSet::new();
        let mut puffs = 0;
        let mut last = vec![];
        let (mut gone, mut puffs_up, mut was_seen, mut landed, mut on_roof, mut fell) = (0, 0, true, 0, 0, 0);
        let steps = (40.0 * 60.0 / STEP) as usize;
        for k in 0..steps {
            pig.advance(k as f64 * STEP);
            let i = match pig.now() {
                Act::Next => 0,
                Act::Crawl { .. } => 1,
                Act::Circle { .. } => 2,
                Act::Hop { .. } => 3,
                Act::Gone { .. } => 4,
                Act::Eat { .. } => 5,
                Act::Tumble { .. } => 6,
                Act::Stand { .. } => 7,
                Act::Fly { .. } => 8,
                _ => 9,
            };
            doings.insert(pig.doing);
            // never inside anything, but where it walks on the faces of blocks (`Crawl` keeps it out)
            if pig.seen && !matches!(pig.now(), Act::Crawl { .. }) {
                assert!(body_clear(&pig.map, pig.at, pig.up, pig.fwd), "inside a block at {:?} {:?} {:?}, {}", pig.at, pig.up, pig.fwd, pig.doing);
            }
            did[i] += 1;
            gone += !pig.seen as usize;
            fell += (matches!(pig.now(), Act::Hop { .. }) && pig.at[1] > pig.map.top(pig.at[0], pig.at[2]) as f64 + FALL - 2.0) as usize;
            if !was_seen && pig.seen {
                let c = [pig.at[0].floor() as i32, pig.at[1].round() as i32, pig.at[2].floor() as i32];
                landed += 1;
                on_roof += pig.roofs.contains(&c) as usize;
            }
            puffs_up += (was_seen && !pig.seen) as usize;
            was_seen = pig.seen;
            if last.last() != Some(&i) {
                last.push(i);
            }
            puffs = puffs.max(pig.puffs().count());
            assert!(pig.at.iter().chain(pig.shown.iter().flatten()).all(|v| v.is_finite()));
            assert!(
                matches!(pig.now(), Act::Fly { .. } | Act::Peek { .. }) || pig.map.column(pig.at[0].floor() as i32, pig.at[2].floor() as i32).is_some(),
                "it stays about the farm: {:?} after {:?}",
                pig.at,
                &last[last.len().saturating_sub(6)..]
            );
            assert!((dot(pig.shown[1], pig.shown[1]) - 1.0).abs() < 1e-6 && dot(pig.shown[1], pig.shown[2]).abs() < 1e-6, "shown square");
        }
        assert!(did[1..10].iter().all(|&n| n > 0), "every kind of doing: {did:?}");
        assert!(doings.len() >= 25, "all manner of things: {} {doings:?}", doings.len());
        // always there, but for the moments it is gone in a puff of smoke; eating most of all
        assert!((gone as f64) < steps as f64 * 0.05, "gone {gone} steps of {steps}");
        assert!(did[5] as f64 > steps as f64 * 0.4, "most of all it eats: {did:?}");
        assert!(on_roof > 0 && landed > 10, "it comes down on roofs: {on_roof} of {landed} times");
        // other than to eat, on a roof one time in four
        let roofs = (0..4000)
            .filter(|_| {
                let c = pig.landing();
                pig.roofs.contains(&c)
            })
            .count();
        assert!((900..=1100).contains(&roofs), "on a roof {roofs} of 4000 landings");
        assert!(puffs_up < 110, "it goes up in smoke only now and then: {puffs_up} times in 40 minutes");
        assert!(fell > 0, "now and then it drops out of the sky");
        assert!(puffs > 0);
    }
}
