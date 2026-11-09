//! The pig that `pig = true` lets loose on the farm: a pig that keeps to no ground and no physics. It
//! walks in a straight line over whatever is in its way: up a wall, upside down under the eaves, round
//! their edge and over the roof, down the far side; along the bottom of the stream and up the other
//! bank. It runs in circles. It bounces about in hops nearly as high as the roofs, now and then head
//! over heels. It goes up in a puff of smoke and comes down in another somewhere else. It tumbles down
//! the mountains. Most of all, it eats the crops.
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
/// How far about the farm's middle it knows the world, across the ground, in blocks.
const REACH: i32 = 72;
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
    Crawl {
        c: Crawl,
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
    /// second, `time` seconds long, `s` into it; turning head over heels `flips` times. `more` hops after it, towards a place if there is one,
    /// then `then`.
    Hop {
        from: V3,
        to: V3,
        v: f64,
        g: f64,
        time: f64,
        s: f64,
        flips: f64,
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
    /// Flying by across the sky before the camera, `s` seconds of `time` from one side to the other.
    Fly {
        from: V3,
        to: V3,
        s: f64,
        time: f64,
    },
}

pub struct Pig {
    map: Map,
    rng: Rng,
    /// The places it is put down: open cells over the ground the camera sees, crops it sees, and high
    /// places it sees on a slope.
    spots: Vec<[i32; 3]>,
    /// Those of the spots that are on a roof, which it likes to come down on, and those that are not.
    roofs: Vec<[i32; 3]>,
    plain: Vec<[i32; 3]>,
    crops: Vec<[i32; 3]>,
    peaks: Vec<[i32; 3]>,
    centre: [f64; 2],
    /// The camera's eye, the way it looks across the ground, and how far off the sides of the screen
    /// are a block ahead: where it flies by.
    eye: V3,
    ahead: V3,
    wide: f64,
    t: f64,
    act: Act,
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

impl Pig {
    /// The pig, in a world seen by `cam`; `seen` is whether a point of the world shows on the screen,
    /// not hidden behind anything.
    pub fn new(w: &World, cam: &Camera, seed: i64, seen: impl Fn(V3) -> bool) -> Pig {
        let ahead = level([cam.forward[0], cam.forward[2]]);
        let centre = ((ahead[0] * FARM_AHEAD).round() as i32, (ahead[2] * FARM_AHEAD).round() as i32);
        let map = Map::new(w, centre);
        let (mut spots, mut crops, mut peaks, mut roofs) = (vec![], vec![], vec![], vec![]);
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
        let plain = spots.iter().copied().filter(|c| !roofs.contains(c)).collect();
        let mut rng = Rng::new(seed ^ 0x0050_4947);
        let start = if crops.is_empty() { spots.first().copied() } else { Some(*rng.pick(&crops)) };
        let start = start.unwrap_or([centre.0, map.top(centre.0 as f64, centre.1 as f64), centre.1]);
        let at = spot_feet(start);
        let fwd = level([rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)]);
        Pig {
            map,
            rng,
            spots,
            plain,
            roofs,
            crops,
            peaks,
            centre: [centre.0 as f64 + 0.5, centre.1 as f64 + 0.5],
            eye: cam.eye,
            ahead,
            wide: cam.tan * cam.aspect,
            t: 0.0,
            act: Act::Eat { left: 20.0, shuffle: 4.0 },
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

    fn step(&mut self) {
        let before = self.at;
        let act = std::mem::replace(&mut self.act, Act::Next);
        self.act = self.act(act);
        self.puffs.retain(|&(_, t)| self.t - t < PUFF);
        // its legs: they go with the ground walked, flail in the air, rest while it eats
        let moved = (0..3).map(|i| (self.at[i] - before[i]).powi(2)).sum::<f64>().sqrt();
        let (amount, flail) = match self.act {
            Act::Crawl { .. } | Act::Circle { .. } => (0.9, 0.0),
            Act::Fly { .. } => (0.8, 8.0 * STEP),
            Act::Tumble { .. } => (1.0, 0.0),
            Act::Hop { .. } => (0.7, 14.0 * STEP),
            _ => (0.0, 0.0),
        };
        self.walk.phase += if flail > 0.0 { flail } else { moved.min(1.0) * 8.0 };
        self.walk.amount += (amount - self.walk.amount) * 0.4;
        if self.walk.amount < 0.01 && amount == 0.0 {
            self.walk.amount = 0.0;
        }
        let nod = if matches!(self.act, Act::Eat { .. }) { 0.5 + 0.3 * (self.t * 11.0).sin() } else { 0.0 };
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

    /// Put down at a spot, standing.
    fn put(&mut self, c: [i32; 3]) {
        self.at = spot_feet(c);
        self.up = Y;
        self.fwd = level([self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0)]);
        self.shown = [self.at, self.up, self.fwd];
    }

    /// A step of what it is doing; what it is doing then.
    fn act(&mut self, act: Act) -> Act {
        match act {
            Act::Next => self.choose(),
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
                if left <= 0.0 { Act::Next } else { Act::Crawl { c, left: left - STEP } }
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
            Act::Hop { from, to, v, g, time, s, flips, more, toward, then } => {
                let s = (s + STEP).min(time);
                let k = s / time;
                let p = [from[0] + (to[0] - from[0]) * k, from[1] + v * s - g * s * s / 2.0, from[2] + (to[2] - from[2]) * k];
                let fwd = level([to[0] - from[0], to[2] - from[2]]);
                let fwd = if (to[0] - from[0]).hypot(to[2] - from[2]) < 1e-6 { level([self.fwd[0], self.fwd[2]]) } else { fwd };
                let (up, f) = heels(Y, fwd, flips * TAU * k);
                self.at = add(add(p, mul(Y, MIDDLE)), mul(up, -MIDDLE));
                (self.up, self.fwd) = (up, f);
                if s < time {
                    return Act::Hop { from, to, v, g, time, s, flips, more, toward, then };
                }
                (self.at, self.up, self.fwd) = (to, Y, fwd);
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
            Act::Eat { left, shuffle } => {
                if left <= 0.0 {
                    return Act::Next;
                }
                if self.rng.f64() < 0.01 {
                    // it looks up, and about it
                    self.fwd = level([self.rng.range(-1.0, 1.0), self.rng.range(-1.0, 1.0)]);
                }
                if shuffle > 0.0 {
                    return Act::Eat { left: left - STEP, shuffle: shuffle - STEP };
                }
                // on to the next plant: a crop next to it, a little hop over
                let here = self.at;
                let next = self.crops.iter().copied().filter(|c| {
                    let d = (spot_feet(*c)[0] - here[0]).hypot(spot_feet(*c)[2] - here[2]);
                    d > 0.5 && d < 2.5
                });
                let next: Vec<[i32; 3]> = next.collect();
                let then = Act::Eat { left, shuffle: self.rng.range(3.0, 7.0) };
                match next.is_empty() {
                    true => then,
                    false => {
                        let c = *self.rng.pick(&next);
                        self.leap(spot_feet(c), 0.5, 0.0, 0, None, then)
                    }
                }
            }
            Act::Tumble { dir, speed, roll, flat, left } => self.tumble(dir, speed, roll, flat, left),
            Act::Fly { from, to, s, time } => {
                let s = s + STEP;
                let k = (s / time).min(1.0);
                // level, bobbing a little as it paddles the air
                let bob = (s * 2.6).sin() * 0.3;
                self.at = [from[0] + (to[0] - from[0]) * k, from[1] + (to[1] - from[1]) * k + bob, from[2] + (to[2] - from[2]) * k];
                self.fwd = level([to[0] - from[0], to[2] - from[2]]);
                self.up = Y;
                if s < time {
                    return Act::Fly { from, to, s, time };
                }
                // off the screen's side: down somewhere a little later
                self.seen = false;
                let to = self.landing();
                let stand = Act::Stand { left: self.rng.range(0.5, 2.0) };
                Act::Gone { left: self.rng.range(1.0, 3.0), to, then: Box::new(stand), fall: false }
            }
        }
    }

    fn upright(&self) -> bool {
        self.up[1] > 0.99
    }

    /// What it does next: mostly eat; it goes up in smoke to be somewhere else only now and then.
    fn choose(&mut self) -> Act {
        if !self.upright() {
            // off the wall, or down from under the roof
            return self.hop(0, None, Act::Next);
        }
        match self.rng.weighted(&[34.0, 22.0, 14.0, 10.0, 2.0, 4.0, 3.0, 2.0]) {
            0 if !self.crops.is_empty() => {
                let c = *self.rng.pick(&self.crops);
                let eat = Act::Eat { left: self.rng.range(12.0, 35.0), shuffle: self.rng.range(3.0, 7.0) };
                let far = (spot_feet(c)[0] - self.at[0]).hypot(spot_feet(c)[2] - self.at[2]);
                if far < 40.0 { self.hop(14, Some(c), eat) } else { self.gone(c, eat) }
            }
            1 => self.crawl(),
            3 => {
                let r = self.rng.range(0.8, 2.0);
                let side = cross(self.up, self.fwd);
                let centre = [self.at[0] + side[0] * r, self.at[2] + side[2] * r];
                let angle = (self.at[2] - centre[1]).atan2(self.at[0] - centre[0]);
                let turn = RUN / r * if self.rng.f64() < 0.5 { 1.0 } else { -1.0 };
                Act::Circle { centre, r, angle, turn, left: self.rng.range(3.0, 9.0) }
            }
            4 if !self.spots.is_empty() => {
                let to = self.landing();
                let stand = Act::Stand { left: self.rng.range(0.5, 2.0) };
                self.gone(to, stand)
            }
            5 if !self.peaks.is_empty() => {
                let to = *self.rng.pick(&self.peaks);
                let tumble = Act::Tumble { dir: [0.0, 0.0], speed: 0.0, roll: 0.0, flat: 0.0, left: 14.0 };
                self.gone(to, tumble)
            }
            6 if !self.spots.is_empty() => self.fly(),
            7 if !self.spots.is_empty() => {
                // up in smoke, and a few seconds later down out of the sky
                let to = self.landing();
                let stand = Act::Stand { left: self.rng.range(0.5, 2.0) };
                self.puff();
                self.seen = false;
                Act::Gone { left: self.rng.range(4.0, 6.0), to, then: Box::new(stand), fall: true }
            }
            _ => {
                let n = self.rng.int(3, 9) as u32;
                self.hop(n, None, Act::Next)
            }
        }
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

    /// Pigs can fly: up in a puff of smoke, and in from one side of the screen, across the sky over the
    /// roofs and out of the other, some way off.
    fn fly(&mut self) -> Act {
        self.puff();
        let far = self.rng.range(22.0, 45.0);
        let right = [self.ahead[2], 0.0, -self.ahead[0]];
        let middle = add(self.eye, mul(self.ahead, far));
        // from past the screen's side to past the other
        let half = far * self.wide + 3.0;
        let side = if self.rng.f64() < 0.5 { 1.0 } else { -1.0 };
        let height = self.rng.range(1.0, 6.0);
        let from = add(middle, add(mul(right, -half * side), [0.0, height, 0.0]));
        let to = add(middle, add(mul(right, half * side), [0.0, height + self.rng.range(-1.5, 1.5), 0.0]));
        let time = 2.0 * half / FLY;
        self.at = from;
        self.up = Y;
        self.fwd = level([to[0] - from[0], to[2] - from[2]]);
        self.shown = [self.at, self.up, self.fwd];
        self.seen = true;
        Act::Fly { from, to, s: 0.0, time }
    }

    /// Up in a puff of smoke, and down in another at `to` a moment later.
    fn gone(&mut self, to: [i32; 3], then: Act) -> Act {
        self.puff();
        self.seen = false;
        Act::Gone { left: self.rng.range(0.4, 1.1), to, then: Box::new(then), fall: false }
    }

    /// Off in a straight line, mostly back towards the farm's middle.
    fn crawl(&mut self) -> Act {
        let cell = [self.at[0].floor() as i32, (self.at[1] + 0.5).floor() as i32, self.at[2].floor() as i32];
        if self.map.solid(cell) || !self.map.solid(iadd(cell, [0, -1, 0])) {
            return self.hop(2, None, Act::Next);
        }
        let ways = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
        let home = [self.centre[0] - self.at[0], 0.0, self.centre[1] - self.at[2]];
        let fwd = if self.rng.f64() < 0.5 {
            *ways.iter().max_by(|a, b| dot(f3(**a), home).total_cmp(&dot(f3(**b), home))).unwrap_or(&ways[0])
        } else {
            *self.rng.pick(&ways)
        };
        let middle = add(f3(cell), [0.5, 0.0, 0.5]);
        let along = dot(add(self.at, mul(middle, -1.0)), f3(fwd)).clamp(-0.49, 0.49);
        Act::Crawl { c: Crawl { cell, up: [0, 1, 0], fwd, along }, left: self.rng.range(5.0, 14.0) }
    }

    /// A hop from where it is: towards `toward` (onto it, near enough), or to a place nearby; `more`
    /// after it, then `then`.
    fn hop(&mut self, more: u32, toward: Option<[i32; 3]>, then: Act) -> Act {
        let here = self.at;
        let to = match toward {
            Some(c) if (spot_feet(c)[0] - here[0]).hypot(spot_feet(c)[2] - here[2]) <= 6.0 => {
                let big = self.rng.f64() < 0.3;
                return self.leap(spot_feet(c), if big { 4.0 } else { 1.2 }, 0.0, 0, None, then);
            }
            Some(c) => {
                let d = level([spot_feet(c)[0] - here[0], spot_feet(c)[2] - here[2]]);
                let k = self.rng.range(3.0, 6.0);
                let (x, z) = (here[0] + d[0] * k, here[2] + d[2] * k);
                [x, self.map.top(x, z) as f64, z]
            }
            None => {
                let near: Vec<[i32; 3]> =
                    self.spots.iter().copied().filter(|c| (2.0..=7.0).contains(&(spot_feet(*c)[0] - here[0]).hypot(spot_feet(*c)[2] - here[2]))).collect();
                match near.is_empty() {
                    false => spot_feet(*self.rng.pick(&near)),
                    // nowhere seen near: back towards the farm
                    true => {
                        let d = level([self.centre[0] - here[0], self.centre[1] - here[2]]);
                        let (x, z) = (here[0] + d[0] * 3.0, here[2] + d[2] * 3.0);
                        [x, self.map.top(x, z) as f64, z]
                    }
                }
            }
        };
        // most hops low, some nearly as high as the roofs; now and then head over heels
        let rise = if self.rng.f64() < 0.65 { self.rng.range(1.0, 3.0) } else { self.rng.range(4.0, 7.5) };
        let flips = match self.rng.f64() {
            f if f < 0.15 => 1.0,
            f if f < 0.2 => -1.0,
            f if f < 0.24 && rise > 4.0 => 2.0,
            _ => 0.0,
        };
        self.leap(to, rise, flips, more, toward, then)
    }

    /// A hop to `to`, `rise` blocks over the higher of its ends: never quicker across the ground than
    /// `HOP`, floating the longer for it.
    fn leap(&mut self, to: V3, rise: f64, flips: f64, more: u32, toward: Option<[i32; 3]>, then: Act) -> Act {
        let from = self.at;
        let peak = from[1].max(to[1]) + rise;
        let timed = |g: f64| ((2.0 * g * (peak - from[1])).sqrt(), (2.0 * (peak - from[1]) / g).sqrt() + (2.0 * (peak - to[1]) / g).sqrt());
        let (_, quick) = timed(GRAVITY);
        let least = (to[0] - from[0]).hypot(to[2] - from[2]) / HOP;
        // the time goes as one over the root of the fall
        let g = if quick < least { GRAVITY * (quick / least).powi(2) } else { GRAVITY };
        let (v, time) = timed(g);
        Act::Hop { from, to, v, g, time, s: 0.0, flips, more, toward, then: Box::new(then) }
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
        let roll = roll + speed * STEP / MIDDLE;
        let ground = self.map.top(nx, nz) as f64;
        let y = ground + roll.sin().abs() * 0.25 * (speed / ROLL);
        let (up, fwd) = heels(Y, d, roll);
        self.at = add(add([nx, y, nz], mul(Y, MIDDLE)), mul(up, -MIDDLE));
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
        assert!(matches!(pig.act, Act::Stand { .. }));
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
        let mut did = [0usize; 9];
        let mut puffs = 0;
        let mut last = vec![];
        let (mut gone, mut puffs_up, mut was_seen, mut landed, mut on_roof, mut fell) = (0, 0, true, 0, 0, 0);
        let steps = (40.0 * 60.0 / STEP) as usize;
        for k in 0..steps {
            pig.advance(k as f64 * STEP);
            let i = match pig.act {
                Act::Next => 0,
                Act::Crawl { .. } => 1,
                Act::Circle { .. } => 2,
                Act::Hop { .. } => 3,
                Act::Gone { .. } => 4,
                Act::Eat { .. } => 5,
                Act::Tumble { .. } => 6,
                Act::Stand { .. } => 7,
                Act::Fly { .. } => 8,
            };
            did[i] += 1;
            gone += !pig.seen as usize;
            fell += (matches!(pig.act, Act::Hop { .. }) && pig.at[1] > pig.map.top(pig.at[0], pig.at[2]) as f64 + FALL - 2.0) as usize;
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
                matches!(pig.act, Act::Fly { .. }) || pig.map.column(pig.at[0].floor() as i32, pig.at[2].floor() as i32).is_some(),
                "it stays about the farm: {:?} after {:?}",
                pig.at,
                &last[last.len().saturating_sub(6)..]
            );
            assert!((dot(pig.shown[1], pig.shown[1]) - 1.0).abs() < 1e-6 && dot(pig.shown[1], pig.shown[2]).abs() < 1e-6, "shown square");
        }
        assert!(did[1..9].iter().all(|&n| n > 0), "every doing: {did:?}");
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
        assert!(puffs_up < 75, "it goes up in smoke only now and then: {puffs_up} times in 40 minutes");
        assert!(fell > 0, "now and then it drops out of the sky");
        assert!(puffs > 0);
    }
}
