//! Files at work: a copy is a bar scanning down a list on one tower to the file, the file chosen (in
//! purple), a pulse of light carrying it along a trace of the floor (or a beam across the street) to
//! another tower, whose list is written again from the top with the file in it, flashing. Between
//! them, pulses run along the floor's traces on errands of their own. What happens is drawn from the
//! clock and the work's seed, as busy as the readings ask.

use super::city::{City, HALF, Show};
use super::render::{MarkMap, Marks, Rgb3};
use crate::trace::V3;

const SCAN: f64 = 1.3;
const PICK: f64 = 0.5;
const REDRAW: f64 = 0.9;
/// The lists are written with this start's names over this many seconds after it.
const SHUFFLE: f64 = 20.0;
/// A fetched file blinks this long before it goes.
const BLINK: f64 = 1.0;
/// A row scrolls every this many seconds.
const SCROLL: f64 = 0.09;
const FLASH: f64 = 1.8;
/// How fast a file travels, along the floor and through the air (units a second).
const FLOOR_SPEED: f64 = 11.0;
const AIR_SPEED: f64 = 16.0;
/// A copy may start every this many seconds, an errand every this many.
const SLOT: f64 = 0.4;
const ERRAND: f64 = 0.28;
/// A packet may start along the bus every this many seconds.
const PACKET: f64 = 2.0;
/// A streak may fly in every this many seconds, a far beam flash every this many.
const FLIGHT: f64 = 2.0;
const FAR_BEAM: f64 = 0.72;
/// How far back the work is followed at the first frame, so it starts busy.
const HISTORY: f64 = 25.0;

#[derive(Clone, Copy, Debug)]
pub enum Way {
    /// Along a route of the floor, from its first point (true) or its last.
    Floor { trace: usize, forward: bool },
    /// Straight through the air between two points.
    Air { from: V3, to: V3 },
}

#[derive(Clone, Copy, Debug)]
pub struct Copy {
    pub start: f64,
    /// Face, panel, row.
    pub from: (usize, usize, usize),
    pub to: (usize, usize, usize),
    pub way: Way,
    pub travel: f64,
    /// A fetch: a signal coming in to the list first, its way and how long it takes; then the file,
    /// found, blinks before it goes.
    pub inbound: Option<(Way, f64)>,
}

impl Copy {
    /// When the bar starts down the list: when the signal that asked for the file has come in.
    fn scans(&self) -> f64 {
        self.start + self.inbound.map_or(0.0, |i| i.1)
    }
    fn leaves(&self) -> f64 {
        self.scans() + SCAN + if self.inbound.is_some() { BLINK } else { PICK }
    }
    fn arrives(&self) -> f64 {
        self.leaves() + self.travel
    }
    fn ends(&self) -> f64 {
        self.arrives() + REDRAW + FLASH
    }
}

/// A pulse on an errand: along its way over the board, wire to wire, starting then.
#[derive(Clone, Debug)]
pub struct Errand {
    pub path: Path,
    pub len: f64,
    pub start: f64,
    pub speed: f64,
    /// Which kind of signal: its colour and the length of its trail.
    pub kind: u8,
    /// Along the whole bus, end to end.
    pub bus: bool,
}

/// A way over the board: points on the floor, from where a signal starts to where it ends.
pub type Path = std::sync::Arc<[(f64, f64)]>;

/// A wire's points, from its start or its end.
fn oriented(city: &City, trace: usize, forward: bool) -> Path {
    let mut p = city.traces[trace].points.clone();
    if !forward {
        p.reverse();
    }
    p.into()
}

/// A pulse as it is drawn now: along a floor trace, or in the air.
#[derive(Clone, Debug)]
pub enum Pulse {
    Floor {
        path: Path,
        head: f64,
        strength: f64,
        file: bool,
        kind: u8,
    },
    /// Through the air: its head `head` of the way along, its bright tail that much of it, and the
    /// whole way drawn faint (`line`) or not.
    Air {
        from: V3,
        to: V3,
        head: f64,
        strength: f64,
        tail: f64,
        line: bool,
    },
}

/// A streak of light through the air: from the far end of the street overhead toward the eye, or a
/// beam flashing across the street far off.
#[derive(Clone, Copy, Debug)]
pub struct Flight {
    pub start: f64,
    pub from: V3,
    pub to: V3,
    pub time: f64,
    pub tail: f64,
    pub line: bool,
}

pub struct Work {
    pub copies: Vec<Copy>,
    pub errands: Vec<Errand>,
    pub flights: Vec<Flight>,
    next_flight: i64,
    next_far: i64,
    /// The next slots not yet decided.
    next_copy: i64,
    next_errand: i64,
    next_packet: i64,
    seed: u64,
    /// The faces' list panels on the street, (face, panel), and the traces errands may take.
    lists: Vec<(usize, usize)>,
    lanes: Vec<usize>,
    near: Vec<usize>,
    /// Every panel that shows, and what each changed one shows now: its entries' seed and scroll.
    panels: Vec<(usize, usize)>,
    state: std::collections::HashMap<(usize, usize), (u64, i64), super::render::Quick>,
    changes: Vec<Change>,
    next_change: i64,
}

/// A panel changing by itself: a list scrolling, written again, figures or a graph updated.
#[derive(Clone, Copy, Debug)]
pub struct Change {
    pub start: f64,
    pub panel: (usize, usize),
    /// Rows to scroll (negative: down), one every SCROLL seconds; or 0 and a new seed, written again
    /// from the top over REDRAW (`redraw`) or at once.
    pub scroll: i64,
    pub seed: u64,
    pub redraw: bool,
}

impl Change {
    fn time(&self) -> f64 {
        if self.scroll != 0 {
            self.scroll.unsigned_abs() as f64 * SCROLL
        } else if self.redraw {
            REDRAW
        } else {
            0.0
        }
    }
}

fn hash(a: u64, b: u64) -> u64 {
    let mut h = a ^ b.wrapping_mul(0x9E3779B97F4A7C15);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 29;
    h = h.wrapping_mul(0x94D049BB133111EB);
    h ^ (h >> 32)
}

fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

impl Work {
    /// `seen` says whether a point of the world is on the screen, and a row that high there big enough
    /// to see.
    /// With `shuffle`, every list in sight is written again in the first moments with names of this
    /// start's own: the street is traced once and kept, its lists as the layout has them, and each start
    /// shows them in another order.
    pub fn new(city: &City, seed: u64, seen: impl Fn(V3, f64) -> bool, front: impl Fn(V3) -> bool, shuffle: bool) -> Work {
        // a panel all in front of the eye: one partly behind it is never found on the screen
        let whole = |face: &super::city::Face, p: &super::city::Panel| {
            [(p.u0, p.v0), (p.u1, p.v0), (p.u0, p.v1), (p.u1, p.v1)].iter().all(|&(u, v)| front(face.point(u, v)))
        };
        let mut lists = vec![];
        for &f in &city.street_faces {
            let face = &city.faces[f];
            for (pi, p) in face.panels.iter().enumerate() {
                // a list whose middle shows, its rows tall enough to see
                if matches!(p.show, Show::Rows { set: super::atlas::Set::Names, .. })
                    && p.rows() >= 4
                    && seen(face.point((p.u0 + p.u1) / 2.0, (p.v0 + p.v1) / 2.0), p.row)
                    && whole(face, p)
                {
                    lists.push((f, pi));
                }
            }
        }
        let mut panels = vec![];
        for (f, face) in city.faces.iter().enumerate() {
            for (pi, p) in face.panels.iter().enumerate() {
                if p.rows() >= 1 && seen(face.point((p.u0 + p.u1) / 2.0, (p.v0 + p.v1) / 2.0), p.row * 0.8) && whole(face, p) {
                    panels.push((f, pi));
                }
            }
        }
        let lanes: Vec<usize> = (0..city.traces.len())
            .filter(|&t| {
                // every wire of the board, either way; the main lines and the wires leaving them are taken
                // as one network (`way`), and a part's printed outline is no wire
                !city.wires.contains(&t) && !city.joins.iter().any(|j| j.0 == t) && city.traces[t].half > 0.02
            })
            .collect();
        // the near stretch, where a signal shows best: the street's wires there, and those under the near towers
        let near = lanes.iter().copied().filter(|&t: &usize| city.traces[t].points.iter().any(|p| p.1 < 35.0 && p.0.abs() < HALF + 6.0)).collect();
        let changes = if shuffle {
            panels
                .iter()
                .enumerate()
                .filter(|&(_, &(f, p))| matches!(city.faces[f].panels[p].show, Show::Rows { set: super::atlas::Set::Names, .. }))
                .map(|(i, &panel)| Change {
                    start: 0.5 + SHUFFLE * unit(hash(seed ^ 0x5A5A, i as u64)),
                    panel,
                    scroll: 0,
                    seed: hash(seed ^ 0xA5A5, i as u64) | 1,
                    redraw: true,
                })
                .collect()
        } else {
            vec![]
        };
        Work {
            copies: vec![],
            errands: vec![],
            flights: vec![],
            next_flight: i64::MIN,
            next_far: i64::MIN,
            next_copy: i64::MIN,
            next_errand: i64::MIN,
            next_packet: i64::MIN,
            seed,
            lists,
            lanes,
            near,
            panels,
            state: Default::default(),
            changes,
            next_change: i64::MIN,
        }
    }

    /// The work up to time `t`: copies begun and ended, at `files` (0 to 1) of the most, and errands
    /// at `traffic`.
    pub fn advance(&mut self, city: &City, t: f64, files: f64, traffic: f64) {
        self.copies.retain(|c| c.ends() > t);
        self.errands.retain(|e| e.start + e.len / e.speed + 1.0 > t);
        let first = ((t - HISTORY) / SLOT).floor() as i64;
        let mut k = self.next_copy.max(first);
        while (k as f64) * SLOT <= t {
            let h = hash(self.seed, k as u64);
            // at rest a copy every few seconds, busy several a second
            let p = 0.55 * (0.25 + 0.7 * files.clamp(0.0, 1.0));
            if unit(h) < p
                && self.copies.len() < 7
                && let Some(c) = (0..6).find_map(|i| self.copy(city, k as f64 * SLOT, hash(h, 1 + i)))
                && c.ends() > t
            {
                self.copies.push(c);
            }
            k += 1;
        }
        self.next_copy = k;
        // panels changing by themselves, anywhere in sight: the rest of the files' work
        let mut done = vec![];
        self.changes.retain(|c| {
            let over = c.start + c.time() <= t;
            if over {
                done.push(*c);
            }
            !over
        });
        for c in done {
            self.settle(c);
        }
        let first = ((t - HISTORY) / SLOT).floor() as i64;
        let mut k = self.next_change.max(first);
        while (k as f64) * SLOT <= t {
            let h = hash(self.seed ^ 0x3333, k as u64);
            if unit(h) < 0.45 * (0.25 + 0.7 * files.clamp(0.0, 1.0)) && !self.panels.is_empty() {
                let panel = self.panels[(hash(h, 1) % self.panels.len() as u64) as usize];
                let busy =
                    self.changes.iter().any(|c| c.panel == panel) || self.copies.iter().any(|c| (c.from.0, c.from.1) == panel || (c.to.0, c.to.1) == panel);
                if !busy {
                    let p = &city.faces[panel.0].panels[panel.1];
                    let r = unit(hash(h, 2));
                    let rows = p.rows().max(1) as i64;
                    let steps = 1 + (hash(h, 3) % rows.clamp(1, 8) as u64) as i64;
                    let dir = if hash(h, 4) & 1 == 0 { 1 } else { -1 };
                    let (scroll, redraw) = match p.show {
                        // lists scroll, or are written again from the top
                        Show::Rows { set: super::atlas::Set::Names, .. } => {
                            if r < 0.55 {
                                (steps * dir, false)
                            } else {
                                (0, true)
                            }
                        }
                        // figures scroll, are written again, or change where they stand
                        Show::Rows { .. } => {
                            if r < 0.35 {
                                (steps * dir, false)
                            } else {
                                (0, r < 0.6)
                            }
                        }
                        // a graph or a block of print updates
                        Show::Tiles { .. } => (0, false),
                    };
                    let c = Change { start: k as f64 * SLOT, panel, scroll, seed: hash(h, 5) | 1, redraw };
                    if c.start + c.time() <= t {
                        self.settle(c);
                    } else {
                        self.changes.push(c);
                    }
                }
            }
            k += 1;
        }
        self.next_change = k;
        let first = ((t - 12.0) / ERRAND).floor() as i64;
        let mut k = self.next_errand.max(first);
        while (k as f64) * ERRAND <= t {
            let h = hash(self.seed ^ 0x5555, k as u64);
            let p = 0.2 + 0.6 * traffic.clamp(0.0, 1.0);
            if unit(h) < p
                && let Some(path) = self.way(city, h)
            {
                let len = crate::elements::data_city::city::length(&path);
                let (speed, kind, start) = (5.0 + 12.0 * unit(hash(h, 4)), (hash(h, 6) % 4) as u8, k as f64 * ERRAND);
                let e = Errand { path, len, start, speed, kind, bus: false };
                if e.start + e.len / e.speed > t {
                    self.errands.push(e);
                }
                // now and then a second pulse follows a little behind, from the same place; it chooses its own
                // way at the junctions, so the two may part
                if unit(hash(h, 40)) < 0.2
                    && let Some(path) = self.way_from(city, h, 1)
                {
                    let len = crate::elements::data_city::city::length(&path);
                    let e = Errand { path, len, start: start + (0.6 + 0.6 * unit(hash(h, 41))) / speed, speed, kind, bus: false };
                    if e.start + e.len / e.speed > t {
                        self.errands.push(e);
                    }
                }
            }
            k += 1;
        }
        self.next_errand = k;
        // the bus: packets of signals side by side, running a stretch of the street, both ways
        let first = ((t - 12.0) / PACKET).floor() as i64;
        let mut k = self.next_packet.max(first);
        while (k as f64) * PACKET <= t {
            let h = hash(self.seed ^ 0xBB55, k as u64);
            let bus = &city.bus;
            if unit(h) < 0.3 + 0.3 * traffic.clamp(0.0, 1.0) && !bus.is_empty() {
                let r = |i: u64| unit(hash(h, i));
                // now and then side by side on two or three of its wires; mostly one alone
                let wide = if r(1) < 0.8 { 1 } else { 2 + (hash(h, 1) % 2) as usize }.min(bus.len());
                let lane0 = (hash(h, 2) % (bus.len() - wide + 1) as u64) as usize;
                let forward = r(3) < 0.5;
                // the whole length of the bus, from one end to the other
                let (speed, kind) = (18.0 + 14.0 * r(6), (hash(h, 7) % 4) as u8);
                for l in 0..wide {
                    let path = oriented(city, bus[lane0 + l], forward);
                    let len = crate::elements::data_city::city::length(&path);
                    let e = Errand { path, len, start: k as f64 * PACKET, speed, kind, bus: true };
                    if e.start + len / speed > t {
                        self.errands.push(e);
                    }
                }
            }
            k += 1;
        }
        self.next_packet = k;
        // the bus is never empty: with no signal on it, one sets off from one of its ends now
        if !city.bus.is_empty() && !self.errands.iter().any(|e| e.bus && e.start <= t && t < e.start + e.len / e.speed) {
            let h = hash(self.seed ^ 0xB0B, (t * 10.0) as u64);
            let wire = city.bus[(hash(h, 1) % city.bus.len() as u64) as usize];
            let path = oriented(city, wire, unit(hash(h, 2)) < 0.6);
            let len = crate::elements::data_city::city::length(&path);
            self.errands.push(Errand { path, len, start: t, speed: 18.0 + 14.0 * unit(hash(h, 3)), kind: (hash(h, 4) % 4) as u8, bus: true });
        }
        // the far end of the street is never still: streaks fly in from it overhead, with the processor's
        // work, and beams flash across it, with the disk's
        self.flights.retain(|f| f.start + f.time + 0.3 > t);
        let first = ((t - 8.0) / FLIGHT).floor() as i64;
        let mut k = self.next_flight.max(first);
        while (k as f64) * FLIGHT <= t {
            let h = hash(self.seed ^ 0x7777, k as u64);
            if unit(h) < 0.35 + 0.5 * traffic.clamp(0.0, 1.0) {
                let r = |i: u64| unit(hash(h, i));
                let from = [-2.2 + 4.4 * r(1), 5.0 + 11.0 * r(2), 185.0];
                let to = [-2.4 + 4.8 * r(3), 9.0 + 14.0 * r(4), -10.0];
                // one in three the other way: from behind the eye off into the far end
                let (from, to) = if r(6) < 0.33 { (to, from) } else { (from, to) };
                let f = Flight { start: k as f64 * FLIGHT, from, to, time: 195.0 / (28.0 + 22.0 * r(5)), tail: 0.12, line: false };
                if f.start + f.time > t {
                    self.flights.push(f);
                }
            }
            k += 1;
        }
        self.next_flight = k;
        let first = ((t - 2.0) / FAR_BEAM).floor() as i64;
        let mut k = self.next_far.max(first);
        while (k as f64) * FAR_BEAM <= t {
            let h = hash(self.seed ^ 0x9999, k as u64);
            if unit(h) < 0.3 + 0.5 * files.clamp(0.0, 1.0) {
                let r = |i: u64| unit(hash(h, i));
                let (z, y) = (45.0 + 115.0 * r(1), 1.5 + 16.0 * r(2) * r(2));
                let side = if r(3) < 0.5 { -1.0 } else { 1.0 };
                let (from, to) = ([side * HALF, y, z], [-side * HALF, y + 1.5 * (r(4) - 0.5), z + 3.0 * (r(5) - 0.5)]);
                let f = Flight { start: k as f64 * FAR_BEAM, from, to, time: 0.35 + 0.4 * r(6), tail: 0.5, line: true };
                if f.start + f.time > t {
                    self.flights.push(f);
                }
            }
            k += 1;
        }
        self.next_far = k;
    }

    /// The way an errand takes over the board. In from a side street (or up from a dead end) to a main
    /// line, along it, and off down another wire or on to its end; or from a main line's end, along it and
    /// off; or along a wire of its own, end to end. Never back the way it came.
    fn way(&self, city: &City, h: u64) -> Option<Path> {
        self.way_from(city, h, 0)
    }

    /// `way`, its choices at the junctions made by `branch`: the same start, and perhaps another end.
    fn way_from(&self, city: &City, h: u64, branch: u64) -> Option<Path> {
        use crate::elements::data_city::city::{along, length};
        let r = |i: u64| unit(hash(h, i));
        let pick_near = |list: &[usize], near: &[usize], i: u64| -> Option<usize> {
            let pool = if r(i) < 0.75 && !near.is_empty() { near } else { list };
            (!pool.is_empty()).then(|| pool[(hash(h, i + 1) % pool.len() as u64) as usize])
        };
        // along main line `wire` from `s`, one way: off down the wire of a join ahead, or to its end
        let ride = |path: &mut Vec<(f64, f64)>, wire: usize, s: f64, dir: f64, salt: u64| {
            let pts = &city.traces[wire].points;
            let mut ahead: Vec<&(usize, usize, f64)> = city.joins.iter().filter(|j| j.1 == wire && (j.2 - s) * dir > 0.5).collect();
            ahead.sort_by(|a, b| (a.2 - s).abs().total_cmp(&(b.2 - s).abs()));
            let hb = hash(h, salt.wrapping_add(branch.wrapping_mul(0x9E37)));
            if !ahead.is_empty() && unit(hb) < 0.6 {
                let j = ahead[(hash(hb, 1) % ahead.len().min(3) as u64) as usize];
                path.extend(along(pts, s, j.2));
                path.extend(city.traces[j.0].points.iter().copied());
            } else {
                path.extend(along(pts, s, if dir > 0.0 { length(pts) } else { 0.0 }));
            }
        };
        let k = r(10);
        let mut path: Vec<(f64, f64)> = vec![];
        if k < 0.45 && !city.joins.is_empty() {
            let all: Vec<usize> = (0..city.joins.len()).collect();
            let near: Vec<usize> = all.iter().copied().filter(|&i| city.traces[city.joins[i].0].points[0].1 < 45.0).collect();
            let j = city.joins[pick_near(&all, &near, 11)?];
            path.extend(city.traces[j.0].points.iter().rev().copied());
            ride(&mut path, j.1, j.2, if r(13) < 0.5 { 1.0 } else { -1.0 }, 14);
        } else if k < 0.65 && !city.wires.is_empty() {
            let wire = city.wires[(hash(h, 20) % city.wires.len() as u64) as usize];
            let len = length(&city.traces[wire].points);
            let (s, dir) = if r(21) < 0.65 { (0.0, 1.0) } else { (len, -1.0) };
            ride(&mut path, wire, s, dir, 22);
        } else {
            let t = pick_near(&self.lanes, &self.near, 30)?;
            path.extend(oriented(city, t, r(32) < 0.5).iter().copied());
        }
        (path.len() >= 2).then(|| path.into())
    }

    /// A copy starting at `start`, if one can: from a list to another, neither busy.
    fn copy(&self, city: &City, start: f64, h: u64) -> Option<Copy> {
        if self.lists.len() < 2 {
            return None;
        }
        let busy = |fp: (usize, usize)| self.copies.iter().any(|c| (c.from.0, c.from.1) == fp || (c.to.0, c.to.1) == fp);
        let a = self.lists[(hash(h, 1) % self.lists.len() as u64) as usize];
        if busy(a) {
            return None;
        }
        let fa = &city.faces[a.0];
        let pa = &fa.panels[a.1];
        let row_a = (hash(h, 2) % pa.rows() as u64) as usize;
        let ua = (pa.u0 + pa.u1) / 2.0;
        let za = fa.point(ua, 0.0)[2];
        // by the floor when a route leaves near this list, else through the air
        let route = city.routes.iter().copied().filter_map(|r| {
            let (p, q) = city.traces[r].ends?;
            let near = |(f, u): (usize, f64)| f == a.0 && (u - ua).abs() < (pa.u1 - pa.u0) / 2.0 + 0.6;
            if near(p) {
                Some((r, true, q))
            } else if near(q) {
                Some((r, false, p))
            } else {
                None
            }
        });
        // a list on the far end's face, nearest above where the route arrives
        let by_floor = route.into_iter().find_map(|(r, forward, (fb, ub))| {
            let face = &city.faces[fb];
            let mid = |x: &(usize, usize)| ((face.panels[x.1].u0 + face.panels[x.1].u1) / 2.0 - ub).abs();
            let b = self.lists.iter().copied().filter(|&(f, p)| f == fb && !busy((f, p))).min_by(|x, y| mid(x).total_cmp(&mid(y)))?;
            Some((r, forward, b))
        });
        let (way, b) = match by_floor {
            Some((r, forward, b)) if !hash(h, 3).is_multiple_of(3) => (Way::Floor { trace: r, forward }, b),
            _ => {
                // across the street, to a list on the other side not far off
                let others: Vec<(usize, usize)> = self
                    .lists
                    .iter()
                    .copied()
                    .filter(|&(f, p)| {
                        let fb = &city.faces[f];
                        fb.normal[0] * fa.normal[0] < 0.0 && !busy((f, p)) && {
                            let pb = &fb.panels[p];
                            (fb.point((pb.u0 + pb.u1) / 2.0, 0.0)[2] - za).abs() < 22.0
                        }
                    })
                    .collect();
                if others.is_empty() {
                    return None;
                }
                let b = others[(hash(h, 4) % others.len() as u64) as usize];
                let fb = &city.faces[b.0];
                let pb = &fb.panels[b.1];
                let from = fa.point(ua, pa.v1 - (row_a as f64 + 0.5) * pa.row);
                let row_b0 = (hash(h, 5) % pb.rows() as u64) as usize;
                let to = fb.point((pb.u0 + pb.u1) / 2.0, pb.v1 - (row_b0 as f64 + 0.5) * pb.row);
                (Way::Air { from, to }, b)
            }
        };
        let pb = &city.faces[b.0].panels[b.1];
        let row_b = match way {
            Way::Air { to, .. } => ((pb.v1 - (to[1] - city.faces[b.0].origin[1])) / pb.row).floor().max(0.0) as usize,
            _ => (hash(h, 5) % pb.rows() as u64) as usize,
        }
        .min(pb.rows() - 1);
        let travel = match way {
            Way::Floor { trace, .. } => city.traces[trace].length() / FLOOR_SPEED,
            Way::Air { from, to } => {
                let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() / AIR_SPEED
            }
        };
        // two in five are fetches: a signal comes in first, along another route to this list or
        // through the air from across the street
        let inbound = (hash(h, 7) % 5 < 2).then(|| {
            let other = city.routes.iter().copied().find_map(|r| {
                let (p, q) = city.traces[r].ends?;
                let near = |(f, u): (usize, f64)| f == a.0 && (u - ua).abs() < (pa.u1 - pa.u0) / 2.0 + 0.6;
                let used = matches!(way, Way::Floor { trace, .. } if trace == r);
                // arriving at the list: from the far end toward the near one
                (!used && (near(p) || near(q))).then_some(Way::Floor { trace: r, forward: !near(p) })
            });
            let w = match other {
                Some(w) if hash(h, 8) & 1 == 0 => w,
                _ => {
                    let to = fa.point(ua, pa.v1 - (row_a as f64 + 0.5) * pa.row);
                    let from = [-to[0], 1.0 + 12.0 * unit(hash(h, 9)), to[2] + 20.0 * (unit(hash(h, 10)) - 0.5)];
                    Way::Air { from, to }
                }
            };
            let time = match w {
                Way::Floor { trace, .. } => city.traces[trace].length() / FLOOR_SPEED,
                Way::Air { from, to } => ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)).sqrt() / AIR_SPEED,
            };
            (w, time)
        });
        Some(Copy { start, from: (a.0, a.1, row_a), to: (b.0, b.1, row_b), way, travel, inbound })
    }

    /// What the panels show at `t`, from the copies under way.
    /// A change done: what its panel shows from now on.
    fn settle(&mut self, c: Change) {
        let s = self.state.entry(c.panel).or_insert((0, 0));
        if c.scroll != 0 {
            s.1 += c.scroll;
        } else {
            *s = (c.seed, 0);
        }
    }

    pub fn marks(&self, city: &City, t: f64, select: Rgb3, fresh: Rgb3) -> MarkMap {
        let mut m = MarkMap::default();
        for (&k, &(seed, scroll)) in &self.state {
            m.insert(k, Marks { seed, scroll, ..Default::default() });
        }
        for c in &self.changes {
            let s = t - c.start;
            if s < 0.0 {
                continue;
            }
            let e = m.entry(c.panel).or_default();
            if c.scroll != 0 {
                e.scroll += c.scroll.signum() * ((s / SCROLL).floor() as i64).min(c.scroll.abs());
            } else {
                let rows = city.faces[c.panel.0].panels[c.panel.1].rows() as f64;
                (e.seed, e.scroll) = (c.seed, 0);
                if c.redraw {
                    e.shown = Some((s / REDRAW * rows).floor().min(rows));
                }
            }
        }
        for c in &self.copies {
            let s = t - c.scans();
            if s < 0.0 {
                continue;
            }
            let from = (c.from.0, c.from.1);
            if s < SCAN {
                // the bar runs down the list to the file
                let row = c.from.2 as f64 * (s / SCAN).min(1.0);
                let e = m.entry(from).or_default();
                e.bar = Some((row, 1.0));
            } else if t < c.arrives() + 0.4 {
                // a fetched file blinks, then stays chosen while it goes
                let k = if c.inbound.is_some() && s < SCAN + BLINK {
                    if ((s - SCAN) * 7.0).floor() as i64 % 2 == 0 { 1.0 } else { 0.15 }
                } else if s < SCAN + 0.15 {
                    0.6
                } else {
                    1.0
                };
                let e = m.entry(from).or_default();
                (e.bar, e.row) = (Some((c.from.2 as f64, 0.35)), Some((c.from.2, select, k)));
            }
            let a = t - c.arrives();
            if a >= 0.0 {
                let rows = city.faces[c.to.0].panels[c.to.1].rows() as f64;
                let shown = (a / REDRAW * rows).min(rows);
                let flash = if a < REDRAW { 1.0 } else { 1.0 - (a - REDRAW) / FLASH };
                let row = (shown > c.to.2 as f64).then_some((c.to.2, fresh, flash.clamp(0.0, 1.0)));
                let e = m.entry((c.to.0, c.to.1)).or_default();
                (e.row, e.shown) = (row, (shown < rows).then_some(shown.floor()));
            }
        }
        m
    }

    /// The pulses now.
    pub fn pulses(&self, city: &City, t: f64) -> Vec<Pulse> {
        let mut out = vec![];
        // the signals asking for files, on their way in
        for c in &self.copies {
            let Some((way, time)) = c.inbound else { continue };
            let s = t - c.start;
            if s < 0.0 || s > time + 0.3 {
                continue;
            }
            let k = (s / time.max(1e-3)).min(1.0);
            let fade = if s > time { 1.0 - (s - time) / 0.3 } else { 1.0 };
            match way {
                Way::Floor { trace, forward } => {
                    out.push(Pulse::Floor { path: oriented(city, trace, forward), head: k * city.traces[trace].length(), strength: fade, file: true, kind: 1 });
                }
                Way::Air { from, to } => out.push(Pulse::Air { from, to, head: k, strength: fade, tail: 0.45, line: true }),
            }
        }
        for c in &self.copies {
            let s = t - c.leaves();
            if s < 0.0 || s > c.travel + 0.3 {
                continue;
            }
            let k = (s / c.travel.max(1e-3)).min(1.0);
            let fade = if s > c.travel { 1.0 - (s - c.travel) / 0.3 } else { 1.0 };
            match c.way {
                Way::Floor { trace, forward } => {
                    out.push(Pulse::Floor { path: oriented(city, trace, forward), head: k * city.traces[trace].length(), strength: fade, file: true, kind: 0 });
                }
                Way::Air { from, to } => out.push(Pulse::Air { from, to, head: k, strength: fade, tail: 0.45, line: true }),
            }
        }
        for f in &self.flights {
            let s = t - f.start;
            if s >= 0.0 && s <= f.time + 0.3 {
                let fade = if s > f.time { 1.0 - (s - f.time) / 0.3 } else { 1.0 };
                out.push(Pulse::Air { from: f.from, to: f.to, head: (s / f.time).min(1.0), strength: fade, tail: f.tail, line: f.line });
            }
        }
        for e in &self.errands {
            let s = t - e.start;
            if s >= 0.0 {
                out.push(Pulse::Floor {
                    path: e.path.clone(),
                    head: s * e.speed,
                    strength: if s * e.speed > e.len + 2.0 { 0.0 } else { 0.85 },
                    file: false,
                    kind: e.kind,
                });
            }
        }
        out
    }
}
