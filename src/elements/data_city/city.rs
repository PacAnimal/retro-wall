//! The city laid out: towers of glass along a street, each face covered in panels of lettering, and
//! a floor of circuit traces, some of them routes from one tower's foot to another's that files
//! travel along. All of it from a seed: the same city every start.

use super::atlas::{Atlas, Set};
use crate::rng::Rng;
use crate::trace::V3;

/// Half the street's width: the inner rows' faces stand at x = -HALF and x = HALF.
pub const HALF: f64 = 3.0;
/// The bus's half width: its lanes are those this near the middle.
pub const BUS: f64 = 0.4;
/// Where the city ends, ahead.
pub const FAR: f64 = 190.0;
/// The floor's cells, for finding what is near a point, are this wide.
pub const CELL: f64 = 2.0;
pub const X_MAX: f64 = 26.0;
pub const Z_MIN: f64 = -30.0;

/// How a tower's glass looks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Glass {
    /// Clear glass glowing from within, cyan.
    Lit,
    /// Deeper, bluer glass.
    Blue,
    /// Smoked glass: dark panels, the lettering bright on them.
    Smoked,
}

pub struct Tower {
    pub min: V3,
    pub max: V3,
    pub glass: Glass,
    /// Its faces' indices into `City::faces`: -x, +x, -z, +z, top.
    pub faces: [usize; 5],
}

/// What a panel shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Show {
    /// Rows of a set's entries, one to a row, aligned left or right.
    Rows { set: Set, right: bool },
    /// One square entry (small print or a diagram), repeated down the panel.
    Tiles { set: Set },
}

/// A rectangle of a face, in its own units (u across from its left as seen from outside, v up),
/// with what it shows.
#[derive(Clone, Debug)]
pub struct Panel {
    pub u0: f64,
    pub u1: f64,
    pub v0: f64,
    pub v1: f64,
    pub show: Show,
    /// A row's height (the rows), or a tile's side.
    pub row: f64,
    /// Picks the entries.
    pub seed: u64,
    /// How bright its lettering is, and whether it stands on a dark backing.
    pub bright: f64,
    pub backed: bool,
}

impl Panel {
    pub fn rows(&self) -> usize {
        ((self.v1 - self.v0) / self.row).floor().max(0.0) as usize
    }
}

/// A face: where it stands, its axes, and its panels in columns.
pub struct Face {
    pub origin: V3,
    pub u_axis: V3,
    pub v_axis: V3,
    pub normal: V3,
    pub width: f64,
    pub height: f64,
    /// Columns, left to right: (u0, u1, the panels in it from the bottom).
    pub columns: Vec<(f64, f64, Vec<usize>)>,
    pub panels: Vec<Panel>,
}

impl Face {
    /// The panel at (u, v), if any.
    pub fn panel_at(&self, u: f64, v: f64) -> Option<usize> {
        let ci = self.columns.partition_point(|c| c.1 < u);
        let (u0, _, ps) = self.columns.get(ci)?;
        if u < *u0 {
            return None;
        }
        let k = ps.partition_point(|&p| self.panels[p].v1 < v);
        let &p = ps.get(k)?;
        (v >= self.panels[p].v0).then_some(p)
    }
    /// A point of the face, in the world.
    pub fn point(&self, u: f64, v: f64) -> V3 {
        [
            self.origin[0] + self.u_axis[0] * u + self.v_axis[0] * v,
            self.origin[1] + self.u_axis[1] * u + self.v_axis[1] * v,
            self.origin[2] + self.u_axis[2] * u + self.v_axis[2] * v,
        ]
    }
}

/// A trace on the floor: a run of straight pieces, (x, z) points, and its half width.
pub struct Trace {
    pub points: Vec<(f64, f64)>,
    pub half: f64,
    /// A route between two towers' feet: the faces at its ends, and where along them (u).
    pub ends: Option<((usize, f64), (usize, f64))>,
}

impl Trace {
    pub fn length(&self) -> f64 {
        length(&self.points)
    }
}

/// A run of straight pieces' length.
pub fn length(points: &[(f64, f64)]) -> f64 {
    points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum()
}

/// How far along a run of pieces the point of it nearest `p` is.
pub fn locate(points: &[(f64, f64)], p: (f64, f64)) -> f64 {
    let (mut best, mut at, mut s) = (f64::MAX, 0.0, 0.0);
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (vx, vz) = (b.0 - a.0, b.1 - a.1);
        let l2 = vx * vx + vz * vz;
        let h = if l2 > 0.0 { (((p.0 - a.0) * vx + (p.1 - a.1) * vz) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (p.0 - a.0 - vx * h).hypot(p.1 - a.1 - vz * h);
        if d < best {
            (best, at) = (d, s + h * l2.sqrt());
        }
        s += l2.sqrt();
    }
    at
}

/// The part of a run of pieces from `s0` along it to `s1`, either way.
pub fn along(points: &[(f64, f64)], s0: f64, s1: f64) -> Vec<(f64, f64)> {
    let (lo, hi) = (s0.min(s1), s0.max(s1));
    let mut out = vec![at(points, lo).0];
    let mut s = 0.0;
    for w in points.windows(2) {
        s += (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
        if s > lo && s < hi {
            out.push(w[1]);
        }
    }
    out.push(at(points, hi).0);
    if s0 > s1 {
        out.reverse();
    }
    out
}

/// The point `s` along a run of pieces, and the direction there.
pub fn at(points: &[(f64, f64)], s: f64) -> ((f64, f64), (f64, f64)) {
    {
        let mut left = s.max(0.0);
        for w in points.windows(2) {
            let l = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            let d = if l > 0.0 { ((w[1].0 - w[0].0) / l, (w[1].1 - w[0].1) / l) } else { (0.0, 1.0) };
            if left <= l {
                return ((w[0].0 + d.0 * left, w[0].1 + d.1 * left), d);
            }
            left -= l;
        }
        let n = points.len();
        let (a, b) = (points[n.saturating_sub(2)], points[n - 1]);
        let l = (b.0 - a.0).hypot(b.1 - a.1).max(1e-9);
        (b, ((b.0 - a.0) / l, (b.1 - a.1) / l))
    }
}

/// A pad of copper on the floor, drilled through: round where wires join, square at a dead end, where a
/// part's leg would be soldered. Its centre and half its width.
pub struct Pad {
    pub x: f64,
    pub z: f64,
    pub half: f64,
    pub round: bool,
}

pub struct City {
    pub towers: Vec<Tower>,
    pub faces: Vec<Face>,
    pub traces: Vec<Trace>,
    pub pads: Vec<Pad>,
    /// The floor's cells, (x, z) from (-X_MAX, Z_MIN): the towers over each.
    pub tower_cells: Vec<Vec<u16>>,
    /// The traces' pieces (trace, piece) and the pads near each cell.
    pub trace_cells: Vec<Vec<(u16, u16)>>,
    pub pad_cells: Vec<Vec<u16>>,
    pub nx: usize,
    pub nz: usize,
    pub tallest: f64,
    /// The faces on the street: where files are read and written.
    pub street_faces: Vec<usize>,
    /// The routes among the traces.
    pub routes: Vec<usize>,
    /// The bus down the middle of the street: its traces, left to right.
    pub bus: Vec<usize>,
    /// The main lines: the bus's, and two beside it that turn into a side street.
    pub wires: Vec<usize>,
    /// Where a wire leaves a main line: the wire (it starts there), the main line, how far along it.
    pub joins: Vec<(usize, usize, f64)>,
}

pub fn cell_of(x: f64, z: f64) -> Option<(usize, usize)> {
    let (cx, cz) = (((x + X_MAX) / CELL).floor(), ((z - Z_MIN) / CELL).floor());
    let (nx, nz) = (cells_x(), cells_z());
    (cx >= 0.0 && cz >= 0.0 && (cx as usize) < nx && (cz as usize) < nz).then_some((cx as usize, cz as usize))
}
pub fn cells_x() -> usize {
    (2.0 * X_MAX / CELL).ceil() as usize
}
pub fn cells_z() -> usize {
    ((FAR - Z_MIN) / CELL).ceil() as usize
}

impl City {
    pub fn new(seed: i64, atlas: &Atlas) -> City {
        let mut rng = Rng::new(seed);
        let mut towers = vec![];
        // rows of towers on each side: the street's walls, then rows behind, beyond alleys
        for side in [-1.0f64, 1.0] {
            let mut x_in = HALF;
            for row in 0..4 {
                let depth_x = rng.range(2.6, 3.8);
                // the street's right wall (as the eye sees it, -x) starts nearer, beside the street's mouth
                let mut z = if side < 0.0 && row == 0 { -3.0 } else { rng.range(0.0, 2.0) };
                while z < FAR {
                    let len = rng.range(2.4, 4.2);
                    // side streets between the towers, now and then a wide one
                    let gap = if rng.f64() < 0.15 { rng.range(3.5, 5.0) } else { rng.range(1.6, 2.6) };
                    // the street's walls are tall near the eye, and the rows behind taller still, so the
                    // city has no edge in sight
                    let h = match row {
                        0 => rng.range(9.0, 30.0),
                        _ => rng.range(14.0, 42.0),
                    };
                    let (inset, setback) = if row == 0 { (0.0, rng.range(0.0, 0.25)) } else { (rng.range(-0.3, 0.3), 0.0) };
                    let (a, b) = (x_in + setback + inset, x_in + setback + inset + depth_x);
                    let (x0, x1) = if side < 0.0 { (-b, -a) } else { (a, b) };
                    let glass = match rng.f64() {
                        v if v < 0.5 => Glass::Lit,
                        v if v < 0.72 => Glass::Blue,
                        _ => Glass::Smoked,
                    };
                    towers.push(Tower { min: [x0, 0.0, z], max: [x1, h, z + len], glass, faces: [0; 5] });
                    z += len + gap;
                }
                x_in += depth_x + rng.range(1.2, 2.2);
            }
        }
        // on the left (as the eye sees it, +x) a tower nearer still, past a wide side street: it hides
        // only the outer third of the face of the first tower behind it
        if let Some(front) = towers.iter().filter(|t| t.min[0] > 0.0 && t.min[0] < HALF + 0.5).min_by(|a, b| a.min[2].total_cmp(&b.min[2])) {
            let (x0, x1, z) = (front.min[0], front.max[0], front.min[2] - 2.55);
            towers.push(Tower { min: [x0, 0.0, z - 2.6], max: [x1, 26.0, z], glass: Glass::Lit, faces: [0; 5] });
        }
        let tallest = towers.iter().map(|t| t.max[1]).fold(0.0, f64::max);
        // the faces, and their panels
        let mut faces = vec![];
        for t in towers.iter_mut() {
            let (a, b) = (t.min, t.max);
            let (w, h, d) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
            // each face's axes: u to the right as seen from outside (the camera's handedness), v up
            let sides: [(V3, V3, V3, f64, f64); 5] = [
                ([a[0], 0.0, a[2]], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], d, h),  // -x: seen from -x
                ([b[0], 0.0, b[2]], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], d, h),  // +x
                ([b[0], 0.0, a[2]], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], w, h), // -z: facing the eye
                ([a[0], 0.0, b[2]], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], w, h),   // +z
                ([a[0], h, a[2]], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], w, d),     // top: v along z
            ];
            for (k, &(origin, u_axis, normal, width, height)) in sides.iter().enumerate() {
                let v_axis = if k == 4 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] };
                let mut f = Face { origin, u_axis, v_axis, normal, width, height, columns: vec![], panels: vec![] };
                lay_out(&mut f, &mut rng, t.glass, k == 4, atlas);
                t.faces[k] = faces.len();
                faces.push(f);
            }
        }
        // the street's faces: the inner rows' faces toward the street
        let street_faces: Vec<usize> =
            towers.iter().filter(|t| t.min[0].abs().min(t.max[0].abs()) < HALF + 0.5).map(|t| if t.min[0] > 0.0 { t.faces[0] } else { t.faces[1] }).collect();
        let mut city = City {
            towers,
            faces,
            traces: vec![],
            pads: vec![],
            tower_cells: vec![],
            trace_cells: vec![],
            pad_cells: vec![],
            nx: cells_x(),
            nz: cells_z(),
            tallest,
            street_faces,
            routes: vec![],
            bus: vec![],
            wires: vec![],
            joins: vec![],
        };
        city.lay_traces(&mut rng);
        city.index();
        city
    }

    /// The traces: routes from the street's towers' feet along lanes in the street, and runs of
    /// lines in the alleys and under the towers, with pads at the feet.
    fn lay_traces(&mut self, rng: &mut Rng) {
        // lanes across the street, each holding stretches already taken
        let lanes: Vec<f64> = (0..13).map(|i| -2.4 + i as f64 * 0.4).collect();
        let lane = |x: f64| lanes.iter().position(|&l| (l - x).abs() < 0.01);
        // each lane's stretches already taken, and where wires kink, so that neighbours never kink together
        let mut taken: Vec<Vec<(f64, f64)>> = vec![vec![]; lanes.len()];
        let is_free = |taken: &[Vec<(f64, f64)>], li: usize, a: f64, b: f64| !taken[li].iter().any(|&(s, e)| a - 0.6 < e && s < b + 0.6);
        let mut kinks: Vec<(usize, f64)> = vec![];
        let calm = |kinks: &[(usize, f64)], li: usize, z: f64| !kinks.iter().any(|&(l, kz)| l.abs_diff(li) <= 1 && (kz - z).abs() < 4.0);

        // the side streets, each side's in order: the places across each where a wire can turn into it,
        // from the near corner, and how far out the wires run
        let mut walls: Vec<(f64, f64, f64)> =
            self.towers.iter().filter(|t| t.min[0].abs().min(t.max[0].abs()) < HALF + 0.5).map(|t| (t.min[0].signum(), t.min[2], t.max[2])).collect();
        walls.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        struct Side {
            side: f64,
            slots: Vec<f64>,
            next: usize,
            out: f64,
        }
        let mut streets: Vec<Side> = vec![];
        for w in walls.windows(2) {
            let (side, end, start) = (w[0].0, w[0].2, w[1].1);
            let gap = start - end;
            if side != w[1].0 || end > 130.0 || gap < 1.4 {
                continue;
            }
            let n = ((gap - 0.5) / 0.4).floor().max(1.0) as usize;
            let first = end + (gap - (n - 1) as f64 * 0.4) / 2.0;
            streets.push(Side { side, slots: (0..n).map(|j| first + j as f64 * 0.4).collect(), next: 0, out: side * (HALF + rng.range(5.0, 12.0)) });
        }

        // two main lines up the middle from the near end, beside the bus on the right (as the eye sees
        // it, -x), that turn right together into a side street a few blocks up, the outer one first
        let mut mains_until = Z_MIN;
        let mut main_bend: Option<(usize, f64, f64)> = None;
        if let Some(st) = streets.iter_mut().filter(|st| st.side < 0.0 && st.slots.len() >= 2 && st.slots[0] > 12.0).nth(1) {
            for (j, d) in [BUS + 0.8, BUS + 0.4].into_iter().enumerate() {
                let (x, z) = (-d, st.slots[j]);
                let Some(li) = lane(x) else { continue };
                taken[li].push((f64::NEG_INFINITY, z + 0.3));
                for lj in (0..lanes.len()).filter(|&lj| lanes[lj] < x - 0.01) {
                    taken[lj].push((z - 0.3, z + 0.3));
                }
                let mut points = vec![(x, Z_MIN + 1.0)];
                // the outer one steps out round something beside the second tower on the right, where
                // room for it is kept
                if j == 0
                    && let Some(&(_, t0, t1)) = walls.iter().filter(|w| w.0 < 0.0).nth(1)
                    && t1 < z - 3.0
                {
                    let (k0, len) = (t0 + (t1 - t0) * 0.25, ((t1 - t0) * 0.45).clamp(1.2, 3.0));
                    let xs = x - 0.4;
                    points.extend([(x, k0), (xs, k0 + 0.4), (xs, k0 + 0.4 + len), (x, k0 + 0.8 + len)]);
                    if let Some(out) = lane(xs) {
                        taken[out].push((k0 - 0.2, k0 + 1.0 + len));
                    }
                    kinks.push((li, k0 + 0.4 + len / 2.0));
                    main_bend = Some((self.traces.len(), k0 - 0.5, k0 + 1.3 + len));
                }
                points.extend([(x, z - 0.3), (x - 0.3, z), (st.out, z)]);
                self.wires.push(self.traces.len());
                self.traces.push(Trace { points, half: 0.045, ends: None });
            }
            st.next = 2;
            mains_until = st.slots[1];
        }

        // the bus: three wires down the middle, end to end. Each outer one steps out once round something
        // on the board and back, a single wire at a time
        let mut bends: Vec<(usize, f64, f64)> = main_bend.into_iter().collect(); // wire, and the stretch where it is not where it was
        for x in [-BUS, 0.0, BUS] {
            let Some(li) = lane(x) else { continue };
            let mut points = vec![(x, Z_MIN + 1.0)];
            let id = self.traces.len();
            if x != 0.0 {
                let z = if x < 0.0 { rng.range(mains_until + 3.0, mains_until + 14.0) } else { rng.range(30.0, 50.0) };
                let xs = x + x.signum() * 0.2;
                points.extend([(x, z), (xs, z + 0.2), (xs, z + 1.6), (x, z + 1.8)]);
                kinks.push((li, z));
                bends.push((id, z - 0.5, z + 2.3));
                if let Some(out) = lane(x + x.signum() * 0.4) {
                    taken[out].push((z - 0.6, z + 2.4));
                }
                self.pads.push(Pad { x, z: z + 0.9, half: 0.12, round: false });
            }
            points.push((x, FAR - 4.0));
            self.bus.push(id);
            self.wires.push(id);
            self.traces.push(Trace { points, half: 0.045, ends: None });
            taken[li].push((f64::NEG_INFINITY, f64::INFINITY));
        }
        // the main line a wire leaving at x on `side` and z starts from: the bus's edge, or below where the
        // two turn off on the right, the outer of those
        let n_mains = self.wires.len() - self.bus.len();
        let source = |side: f64, z: f64, bus: &[usize]| -> Option<(usize, f64)> {
            if side < 0.0 && z < mains_until - 0.5 && n_mains == 2 {
                Some((self.wires[0], -(BUS + 0.8)))
            } else if side < 0.0 {
                Some((bus[0], -BUS))
            } else {
                Some((bus[2], BUS))
            }
        };

        // the wires laid from here on may kink where they find room (the main lines keep straight)
        let laid = self.traces.len();
        // every side street is fed from a main line: a bundle cuts across at 45 degrees to the lanes by the
        // wall, runs along them, and turns in. The outermost leaves first and turns first, so none crosses
        let bus = self.bus.clone();
        for st in streets.iter_mut() {
            let side = st.side;
            let k = rng.int(1, 3).min((st.slots.len() - st.next) as i64).max(0) as usize;
            let run = rng.range(2.0, 7.0);
            for j in 0..k {
                let Some(li) = lane(side * (HALF - 0.6 - 0.4 * j as f64)) else { continue };
                let Some(&zj) = st.slots.get(st.next) else { break };
                let x = lanes[li];
                let Some((wire, src)) = source(side, zj - run - 3.0, &bus) else { continue };
                let across = (x - src).abs();
                if across < 0.39 {
                    continue;
                }
                let from = zj - run - 0.4 * (k - 1 - j) as f64 - across;
                let crossed: Vec<(usize, f64)> = lanes
                    .iter()
                    .enumerate()
                    .filter(|&(_, &xx)| (xx - src) * side > 0.01 && (xx - x) * side < -0.01)
                    .map(|(lj, &xx)| (lj, from + (xx - src).abs()))
                    .collect();
                let bent = bends.iter().any(|&(w, a, b)| w == wire && from > a && from < b);
                if bent || !is_free(&taken, li, from + across, zj) || crossed.iter().any(|&(lj, at)| !is_free(&taken, lj, at, at)) {
                    continue;
                }
                taken[li].push((from + across, zj));
                for (lj, at) in crossed {
                    taken[lj].push((at - 0.3, at + 0.3));
                }
                st.next += 1;
                let id = self.traces.len();
                self.traces.push(Trace {
                    points: vec![(src, from), (x, from + across), (x, zj - 0.3), (x + side * 0.3, zj), (st.out, zj)],
                    half: 0.035,
                    ends: None,
                });
                self.joins.push((id, wire, locate(&self.traces[wire].points, (src, from))));
            }
        }

        // short wires off the main lines that end at a part's leg: a dead end, a square pad
        for _ in 0..22 {
            let side = if rng.f64() < 0.5 { -1.0 } else { 1.0 };
            let z = rng.range(-2.0, 95.0);
            let Some((wire, src)) = source(side, z, &bus) else { continue };
            let Some(li) = lane(src + side * 0.4) else { continue };
            let len = rng.range(0.8, 4.0);
            let bent = bends.iter().any(|&(w, a, b)| w == wire && z > a && z < b);
            if bent || !is_free(&taken, li, z + 0.4, z + 0.4 + len) {
                continue;
            }
            taken[li].push((z + 0.1, z + 0.4 + len));
            let x = lanes[li];
            let id = self.traces.len();
            self.traces.push(Trace { points: vec![(src, z), (x, z + 0.4), (x, z + 0.4 + len)], half: 0.03, ends: None });
            self.joins.push((id, wire, locate(&self.traces[wire].points, (src, z))));
            self.pads.push(Pad { x, z: z + 0.4 + len, half: 0.12, round: false });
        }

        // wires that come out of one side street, run along the street and turn into another on the same
        // side further on, into each from its far corner so they never cross what feeds it. The near ones
        // always get some
        let mut by_side: Vec<Vec<usize>> = vec![vec![], vec![]];
        for (i, st) in streets.iter().enumerate() {
            by_side[if st.side < 0.0 { 0 } else { 1 }].push(i);
        }
        let mut pairs: Vec<(usize, usize)> = vec![];
        for ids in &by_side {
            for n in 0..ids.len().min(5) {
                for m in 1..=2 {
                    if let Some(&b) = ids.get(n + m) {
                        pairs.push((ids[n], b));
                    }
                }
            }
        }
        let mut near_done = 0;
        let mut extra = vec![];
        for ids in &by_side {
            for (n, &a) in ids.iter().enumerate() {
                if rng.f64() < 0.6
                    && let Some(&b) = ids.get(n + rng.int(1, 3) as usize)
                {
                    extra.push((a, b));
                }
            }
        }
        for (pass, (a, b)) in pairs.into_iter().map(|p| (0, p)).chain(extra.into_iter().map(|p| (1, p))) {
            if pass == 0 && near_done >= 3 {
                continue;
            }
            let side = streets[a].side;
            if streets[a].next >= streets[a].slots.len() || streets[b].next >= streets[b].slots.len() {
                continue;
            }
            let (Some(&za), Some(&zb)) = (streets[a].slots.last(), streets[b].slots.last()) else { continue };
            // the lane nearest the bus that is free all the way, and the lanes outside it where it turns
            let candidates: Vec<usize> = [0.8, 1.2, 1.6, 2.0].iter().filter_map(|&d| lane(side * d)).collect();
            let Some(li) = candidates.into_iter().find(|&li| {
                let x = lanes[li];
                is_free(&taken, li, za, zb)
                    && (0..lanes.len()).filter(|&lj| (lanes[lj] - x) * side > 0.01).all(|lj| is_free(&taken, lj, za, za) && is_free(&taken, lj, zb, zb))
            }) else {
                continue;
            };
            let x = lanes[li];
            taken[li].push((za, zb));
            for lj in (0..lanes.len()).filter(|&lj| (lanes[lj] - x) * side > 0.01) {
                taken[lj].push((za - 0.3, za + 0.3));
                taken[lj].push((zb - 0.3, zb + 0.3));
            }
            streets[a].slots.pop();
            streets[b].slots.pop();
            let (oa, ob) = (streets[a].out, streets[b].out);
            let mut points = vec![(oa, za), (x + side * 0.3, za), (x, za + 0.3)];
            // now and then a kink toward the bus and back, never beside another
            let m = za + (zb - za) * rng.range(0.3, 0.6);
            if zb - za > 6.0 && rng.f64() < 0.5 && calm(&kinks, li, m) {
                let j = -side * 0.2;
                points.extend([(x, m), (x + j, m + 0.2), (x + j, m + 1.4), (x, m + 1.6)]);
                kinks.push((li, m));
            }
            points.extend([(x, zb - 0.3), (x + side * 0.3, zb), (ob, zb)]);
            self.traces.push(Trace { points, half: if rng.f64() < 0.4 { 0.045 } else { 0.035 }, ends: None });
            if pass == 0 {
                near_done += 1;
            }
        }

        // the footprints of parts with two rows of legs, in the street between the bus and the walls: square
        // pad for the first leg, round for the rest, and wires from a few legs to dead ends
        for _ in 0..10 {
            let side = if rng.f64() < 0.5 { -1.0 } else { 1.0 };
            let (Some(l0), Some(l1)) = (lane(side * 1.6), lane(side * 2.0)) else { continue };
            let n = rng.int(4, 8);
            let z0 = rng.range(0.0, 80.0);
            let z1 = z0 + (n - 1) as f64 * 0.25;
            if !is_free(&taken, l0, z0 - 0.4, z1 + 0.4) || !is_free(&taken, l1, z0 - 0.4, z1 + 0.4) {
                continue;
            }
            for (col, li) in [l0, l1].into_iter().enumerate() {
                let x = lanes[li];
                for k in 0..n {
                    self.pads.push(Pad { x, z: z0 + k as f64 * 0.25, half: 0.085, round: !(col == 0 && k == 0) });
                }
                // a wire on from the end leg, along the lane to a dead end
                let (zs, dir) = if col == 0 { (z1, 1.0) } else { (z0, -1.0) };
                let len = rng.range(1.0, 4.0);
                let ze = zs + dir * len;
                if is_free(&taken, li, zs.min(ze) - 0.2, zs.max(ze) + 0.2) {
                    self.traces.push(Trace { points: vec![(x, zs), (x, ze)], half: 0.03, ends: None });
                    self.pads.push(Pad { x, z: ze, half: 0.12, round: false });
                    taken[li].push((zs.min(ze), zs.max(ze)));
                }
                taken[li].push((z0 - 0.4, z1 + 0.4));
            }
            // its outline, printed faint
            let (xa, xb) = (side * 1.45, side * 2.15);
            self.traces.push(Trace { points: vec![(xa, z0 - 0.2), (xb, z0 - 0.2), (xb, z1 + 0.2), (xa, z1 + 0.2), (xa, z0 - 0.2)], half: 0.008, ends: None });
        }
        let feet: Vec<(usize, f64, f64, f64)> = self
            .street_faces
            .iter()
            .flat_map(|&f| {
                let face = &self.faces[f];
                let side = if face.normal[0] < 0.0 { 1.0 } else { -1.0 }; // which side of the street the tower is on
                let n = (face.width / 0.9).floor().max(1.0) as usize;
                (0..n).map(move |k| {
                    let u = (k as f64 + 0.5) * face.width / n as f64;
                    let z = face.origin[2] + face.u_axis[2] * u;
                    (f, u, z, side)
                })
            })
            .filter(|&(_, _, z, _)| z < 120.0)
            .collect();
        // routes from one tower's foot to another's, for the files: out from the foot, a corner cut at 45
        // degrees onto a free lane, along it, and back
        let mut tries = 0;
        while self.routes.len() < 14 && tries < 4000 {
            tries += 1;
            let a = feet[rng.below(feet.len() as u64) as usize];
            let b = feet[rng.below(feet.len() as u64) as usize];
            if (a.2 - b.2).abs() < 5.0 || (a.2 - b.2).abs() > 50.0 || a.3 != b.3 {
                continue;
            }
            let (a, b) = if a.2 < b.2 { (a, b) } else { (b, a) };
            let Some(li) = lane(a.3 * (HALF - 0.6 - 0.4 * rng.below(3) as f64)) else { continue };
            let x = lanes[li];
            let (z0, z1) = (a.2 + 0.3, b.2 - 0.3);
            let outer: Vec<usize> = (0..lanes.len()).filter(|&lj| (lanes[lj] - x) * a.3 > 0.01).collect();
            if !is_free(&taken, li, z0, z1) || outer.iter().any(|&lj| !is_free(&taken, lj, a.2, a.2) || !is_free(&taken, lj, b.2, b.2)) {
                continue;
            }
            // the feet a route leaves from are each its own
            if self
                .traces
                .iter()
                .filter_map(|t| t.ends)
                .any(|(p, q)| [p, q].iter().any(|&(f, u)| (f == a.0 && (u - a.1).abs() < 0.3) || (f == b.0 && (u - b.1).abs() < 0.3)))
            {
                continue;
            }
            taken[li].push((z0, z1));
            for &lj in &outer {
                taken[lj].push((a.2 - 0.3, a.2 + 0.3));
                taken[lj].push((b.2 - 0.3, b.2 + 0.3));
            }
            let points = vec![(a.3 * HALF, a.2), (x + a.3 * 0.3, a.2), (x, z0), (x, z1), (x + b.3 * 0.3, b.2), (b.3 * HALF, b.2)];
            self.routes.push(self.traces.len());
            self.traces.push(Trace { points, half: 0.035, ends: None });
            let r = self.routes.len() - 1;
            self.traces[self.routes[r]].ends = Some(((a.0, a.1), (b.0, b.1)));
        }

        // what is left of the lanes: short wires between dead ends, signals starting and stopping at them,
        // now and then one kinking aside and back, never beside another that kinks there
        for (li, &x) in lanes.iter().enumerate() {
            let mut z = Z_MIN + 1.0 + rng.range(0.0, 3.0);
            while z < FAR {
                let len = rng.range(4.0, 18.0);
                if is_free(&taken, li, z, z + len) && rng.f64() < 0.6 {
                    let mut pts = vec![(x, z)];
                    let side = if rng.f64() < 0.5 { -0.2 } else { 0.2 };
                    let m = z + len * 0.4;
                    if len > 6.0 && rng.f64() < 0.5 && (x + side).abs() < HALF - 0.4 && calm(&kinks, li, m) {
                        pts.extend([(x, m), (x + side, m + 0.2), (x + side, m + 1.4), (x, m + 1.6)]);
                        kinks.push((li, m));
                    }
                    pts.push((x, z + len));
                    self.traces.push(Trace { points: pts, half: if rng.f64() < 0.3 { 0.05 } else { 0.03 }, ends: None });
                    for zz in [z, z + len] {
                        self.pads.push(Pad { x, z: zz, half: 0.12, round: false });
                    }
                    taken[li].push((z, z + len));
                }
                z += len + rng.range(0.8, 4.0);
            }
        }
        // where a wire runs straight beside an empty lane, it tends to step across into it and back: a
        // kink where there is room for one, never beside another
        let mains: Vec<usize> = self.wires.iter().copied().filter(|w| !self.bus.contains(w)).collect();
        for t in mains.iter().copied().chain(laid..self.traces.len()) {
            if self.bus.contains(&t) || (t >= laid && self.wires.contains(&t)) || self.traces[t].half < 0.02 {
                continue;
            }
            let mut i = 0;
            while i + 1 < self.traces[t].points.len() {
                let (a, b) = (self.traces[t].points[i], self.traces[t].points[i + 1]);
                let (lo, hi) = (a.1.min(b.1), a.1.max(b.1));
                let Some(li) = lane(a.0) else {
                    i += 1;
                    continue;
                };
                if (a.0 - b.0).abs() > 1e-9 || hi - lo < 3.6 || a.0.abs() > HALF - 0.4 {
                    i += 1;
                    continue;
                }
                // the longest empty stretch of a lane beside it, toward the bus or the wall
                let mut best: Option<(f64, f64, f64)> = None; // side, from, to
                for dir in [-0.4, 0.4] {
                    let Some(l2) = lane(a.0 + dir) else { continue };
                    if (a.0 + dir).abs() > HALF - 0.5 || self.bus.iter().any(|&w| (self.traces[w].points[0].0 - (a.0 + dir)).abs() < 0.01) {
                        continue;
                    }
                    let (mut z, mut run) = (lo + 0.8, None::<f64>);
                    while z <= hi - 0.8 {
                        if is_free(&taken, l2, z, z) {
                            let start = *run.get_or_insert(z);
                            if best.is_none_or(|b| z - start > b.2 - b.1) {
                                best = Some((dir, start, z));
                            }
                        } else {
                            run = None;
                        }
                        z += 0.2;
                    }
                }
                let Some((dir, from, to)) = best else {
                    i += 1;
                    continue;
                };
                let room = to - from;
                let mid = (from + to) / 2.0;
                // a main line kinks wherever it has room, clear of the wires leaving it
                let main = mains.contains(&t);
                let clear = |z0: f64, z1: f64| {
                    !self.joins.iter().any(|j| j.1 == t && self.traces[j.0].points[0].1 > z0 - 0.8 && self.traces[j.0].points[0].1 < z1 + 0.8)
                };
                if room < 2.4 || (!main && rng.f64() > 0.8) || !calm(&kinks, li, mid) || (main && !clear(from, to)) {
                    i += 1;
                    continue;
                }
                let len = rng.range(1.0, (room - 0.8).clamp(1.0, 5.0));
                let z0 = mid - len / 2.0 - 0.4;
                let x = a.0;
                let mut jog = vec![(x, z0), (x + dir, z0 + 0.4), (x + dir, z0 + 0.4 + len), (x, z0 + 0.8 + len)];
                if b.1 < a.1 {
                    jog.reverse();
                }
                if let Some(l2) = lane(x + dir) {
                    taken[l2].push((z0, z0 + 0.8 + len));
                }
                kinks.push((li, mid));
                let n = jog.len();
                self.traces[t].points.splice(i + 1..i + 1, jog);
                i += n + 1;
            }
        }

        // where the wires leaving the main lines start along them, now that they may have kinked
        for j in self.joins.iter_mut() {
            let p = self.traces[j.0].points[0];
            j.2 = locate(&self.traces[j.1].points, p);
        }

        // pads at every route's ends, and rows of pins at the street's feet
        for &r in &self.routes {
            let t = &self.traces[r];
            for &(x, z) in [t.points[0], *t.points.last().unwrap()].iter() {
                self.pads.push(Pad { x: x - x.signum() * 0.1, z, half: 0.12, round: false });
            }
        }
        // under and between the towers away from the street: runs at right angles with 45 degree turns
        for _ in 0..1200 {
            let side = if rng.f64() < 0.5 { -1.0 } else { 1.0 };
            let x = side * rng.range(HALF + 0.2, X_MAX - 1.0);
            let z = rng.range(Z_MIN + 1.0, FAR - 10.0);
            let mut pts = vec![(x, z)];
            let (mut px, mut pz) = (x, z);
            for _ in 0..rng.int(2, 5) {
                let along = rng.range(0.6, 4.0);
                match rng.below(4) {
                    0 => pz += along,
                    1 => px += along * if rng.f64() < 0.5 { -1.0 } else { 1.0 },
                    2 => {
                        px += along * 0.5;
                        pz += along * 0.5;
                    }
                    _ => {
                        px -= along * 0.5;
                        pz += along * 0.5;
                    }
                }
                // never into the street
                if px.abs() < HALF + 0.1 {
                    px = side * (HALF + 0.1);
                }
                pts.push((px, pz));
            }
            self.traces.push(Trace { points: pts, half: rng.range(0.02, 0.045), ends: None });
            if rng.f64() < 0.5 {
                self.pads.push(Pad { x: px, z: pz, half: 0.11, round: rng.f64() < 0.5 });
            }
        }
        // cross traces at the alleys' mouths
        let mut alleys = vec![];
        for t in &self.towers {
            if t.min[0].abs().min(t.max[0].abs()) < HALF + 0.5 {
                alleys.push((t.min[0].signum(), t.max[2]));
            }
        }
        for (side, z) in alleys {
            if rng.f64() < 0.5 {
                let x0 = side * (HALF + 0.05);
                self.traces.push(Trace { points: vec![(x0, z + 0.3), (side * (HALF + 4.0), z + 0.3)], half: 0.03, ends: None });
            }
        }
    }

    /// Which towers, trace pieces and pads are near each cell of the floor.
    fn index(&mut self) {
        let n = self.nx * self.nz;
        self.tower_cells = vec![vec![]; n];
        self.trace_cells = vec![vec![]; n];
        self.pad_cells = vec![vec![]; n];
        let (nx, nz) = (self.nx, self.nz);
        let cover = |x0: f64, z0: f64, x1: f64, z1: f64, f: &mut dyn FnMut(usize)| {
            let cx0 = (((x0 + X_MAX) / CELL).floor().max(0.0) as usize).min(nx - 1);
            let cx1 = (((x1 + X_MAX) / CELL).floor().max(0.0) as usize).min(nx - 1);
            let cz0 = (((z0 - Z_MIN) / CELL).floor().max(0.0) as usize).min(nz - 1);
            let cz1 = (((z1 - Z_MIN) / CELL).floor().max(0.0) as usize).min(nz - 1);
            for cz in cz0..=cz1 {
                for cx in cx0..=cx1 {
                    f(cz * nx + cx);
                }
            }
        };
        for (i, t) in self.towers.iter().enumerate() {
            cover(t.min[0], t.min[2], t.max[0], t.max[2], &mut |c| self.tower_cells[c].push(i as u16));
        }
        // a trace reaches past its line by its glow
        const REACH: f64 = 0.35;
        for (i, t) in self.traces.iter().enumerate() {
            for (k, w) in t.points.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                cover(a.0.min(b.0) - REACH, a.1.min(b.1) - REACH, a.0.max(b.0) + REACH, a.1.max(b.1) + REACH, &mut |c| {
                    self.trace_cells[c].push((i as u16, k as u16))
                });
            }
        }
        for (i, p) in self.pads.iter().enumerate() {
            cover(p.x - p.half - REACH, p.z - p.half - REACH, p.x + p.half + REACH, p.z + p.half + REACH, &mut |c| self.pad_cells[c].push(i as u16));
        }
    }
}

/// A face's panels: columns of lists, figures, small print and diagrams, with gaps between.
fn lay_out(f: &mut Face, rng: &mut Rng, glass: Glass, top: bool, atlas: &Atlas) {
    let margin = 0.12;
    let usable = f.width - 2.0 * margin;
    if usable <= 0.2 {
        return;
    }
    let n = ((usable / rng.range(0.9, 1.5)).round() as usize).clamp(1, 4);
    let cw = usable / n as f64;
    let names_w = atlas.names.iter().map(|e| e.w).max().unwrap_or(1) as f64 / super::atlas::LINE as f64;
    for c in 0..n {
        let (u0, u1) = (margin + c as f64 * cw + 0.04, margin + (c + 1) as f64 * cw - 0.04);
        let mut ps = vec![];
        let mut v = if top { margin } else { rng.range(0.25, 0.8) };
        let end = f.height - margin;
        while v < end - 0.3 {
            let h = rng.range(0.6, 3.4).min(end - v);
            let kind = rng.f64();
            let show = match kind {
                k if k < 0.45 => Show::Rows { set: Set::Names, right: rng.f64() < 0.7 },
                k if k < 0.7 => Show::Rows { set: Set::Figures, right: rng.f64() < 0.3 },
                k if k < 0.9 => Show::Tiles { set: Set::Print },
                _ => Show::Tiles { set: Set::Diagrams },
            };
            // a row's height: the names fill the column's width at most
            let row = match show {
                Show::Rows { set: Set::Names, .. } => ((u1 - u0) / names_w).clamp(0.1, 0.2),
                Show::Rows { .. } => rng.range(0.1, 0.17),
                Show::Tiles { set: Set::Print } => (u1 - u0).min(rng.range(0.6, 1.2)),
                Show::Tiles { .. } => (u1 - u0).min(rng.range(0.35, 0.6)),
            };
            if h >= row {
                let backed = glass == Glass::Smoked || rng.f64() < 0.14;
                let bright = rng.range(0.55, 1.0) * if backed { 1.15 } else { 1.0 };
                ps.push(f.panels.len());
                f.panels.push(Panel { u0, u1, v0: v, v1: v + h, show, row, seed: rng.next_u64(), bright, backed });
            }
            v += h + if rng.f64() < 0.25 { rng.range(0.4, 2.0) } else { rng.range(0.08, 0.25) };
        }
        f.columns.push((u0, u1, ps));
    }
}
