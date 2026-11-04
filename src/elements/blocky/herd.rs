//! The animals that wander: sheep, cows and pigs about their pens, chickens about the run and the yards,
//! ducks and their ducklings over the pond, fish under it. Each has the ground it keeps to, walks to a
//! spot on it, turns and walks again, grazes a while; it keeps clear of the fences and of every other
//! animal, stopping and choosing elsewhere where one is in the way. They step ten times a second on the
//! scene's clock, so a moment is always the same.
//!
//! They are left out of the baked picture. For each ground the patch of the screen it can be seen in is
//! kept as baked, with how far off each of its pixels is; each frame they move, the animals are traced
//! into a copy of the patch, before what is behind them and behind what is before them, and the patch
//! goes up in place of the baked one, lit as the land is.

use super::mobs::{self, Kind};
use super::render::{Camera, Frame, SIDE, Tracer, mob_shade};
use super::world::*;
use crate::rng::Rng;

/// A step of the animals, in seconds.
const STEP: f64 = 0.1;

/// Where an animal may stand: a grid of cells, open or not, and the height its feet are at.
struct Ground {
    x0: i32,
    z0: i32,
    w: i32,
    h: i32,
    open: Vec<bool>,
    /// The sun's reach in each cell at an animal's middle, at each of its places (`Tracer::sunlit`):
    /// what the ground's light says of the ground, not of an animal standing over a fence's shadow.
    sun: Vec<u8>,
}

impl Ground {
    fn open(&self, x: f64, z: f64) -> bool {
        let (cx, cz) = (x.floor() as i32 - self.x0, z.floor() as i32 - self.z0);
        cx >= 0 && cz >= 0 && cx < self.w && cz < self.h && self.open[(cz * self.w + cx) as usize]
    }
    /// A spot to walk to: an open cell's middle, give or take; where it is if none is found.
    fn pick(&self, rng: &mut Rng, from: [f64; 2]) -> [f64; 2] {
        for _ in 0..24 {
            let (cx, cz) = (rng.below(self.w as u64) as i32, rng.below(self.h as u64) as i32);
            if self.open[(cz * self.w + cx) as usize] {
                return [(self.x0 + cx) as f64 + rng.range(0.3, 0.7), (self.z0 + cz) as f64 + rng.range(0.3, 0.7)];
            }
        }
        from
    }
}

/// A patch of the screen: its rectangle, the baked pictures there (B, G, R, A rows) and how far off each
/// of its pixels is.
struct Patch {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    base: Vec<u8>,
    light: Vec<u8>,
    sun: Vec<u8>,
    depth: Vec<f32>,
}

struct Animal {
    kind: Kind,
    ground: usize,
    at: [f64; 2],
    y: f64,
    yaw: f64,
    to: [f64; 2],
    wait: f64,
    /// Its legs in their walk (`mobs::Walk`).
    walk: mobs::Walk,
    rng: Rng,
}

pub struct Herd {
    animals: Vec<Animal>,
    grounds: Vec<Ground>,
    /// Each ground's patch of the screen.
    patches: Vec<Patch>,
    /// The time stepped to.
    t: f64,
}

/// How far a walking animal's legs swing (`mobs::Walk::amount`): ±40 degrees or so, a calm stride.
const WALKING: f64 = 0.5;

/// The water's surface in the pond, which a duck sits in.
const SURFACE: f64 = GROUND as f64 - 0.125;

fn swims(kind: Kind) -> bool {
    matches!(kind, Kind::Duck | Kind::Duckling | Kind::Fish)
}

impl Herd {
    /// The animals that wander in the world, the patches they are seen in kept from the baked picture.
    pub fn new(w: &World, tracer: &Tracer, frame: &Frame, cam: &Camera, seed: i64) -> Herd {
        let mut grounds: Vec<(Ground, [i32; 4], bool)> = vec![];
        let mut animals = vec![];
        for (i, (kind, at, yaw)) in w.mobs.roaming().into_iter().enumerate() {
            let (x, z) = (at[0].floor() as i32, at[2].floor() as i32);
            let inside = |r: &[i32; 4]| (r[0]..=r[2]).contains(&x) && (r[1]..=r[3]).contains(&z);
            let area = if swims(kind) { w.ponds.iter().find(|r| inside(r)) } else { w.pens.iter().find(|r| inside(r)) };
            let area = area.copied().unwrap_or([x - 4, z - 4, x + 4, z + 4]);
            let fish = kind == Kind::Fish;
            let g = match grounds.iter().position(|(_, r, f)| *r == area && *f == fish) {
                Some(g) => g,
                None => {
                    grounds.push((ground(w, area, kind, at[1]), area, fish));
                    grounds.len() - 1
                }
            };
            // one put down on closed ground steps onto the nearest open cell
            let gr = &grounds[g].0;
            let start = (0..6)
                .flat_map(|r: i32| (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (dx, dz))))
                .map(|(dx, dz)| [at[0] + dx as f64, at[2] + dz as f64])
                .find(|p| gr.open(p[0], p[1]))
                .map_or([at[0], at[2]], |p| [p[0].floor() + 0.5, p[1].floor() + 0.5]);
            animals.push(Animal {
                kind,
                ground: g,
                at: start,
                y: at[1],
                yaw,
                to: start,
                wait: 0.0,
                walk: mobs::Walk::STILL,
                rng: Rng::new(seed ^ (i as i64 * 7919 + 13)),
            });
        }
        // the sun's reach at an animal's middle over each ground, so a shadow on the ground under it (a
        // fence's) shades it only if it reaches that high
        for (g, (gr, _, _)) in grounds.iter_mut().enumerate() {
            let Some(a) = animals.iter().find(|a| a.ground == g) else { continue };
            let middle = a.y + mobs::reach(a.kind).1 * 0.6;
            gr.sun = (0..gr.h)
                .flat_map(|cz| (0..gr.w).map(move |cx| (cx, cz)))
                .map(|(cx, cz)| tracer.sunlit([(gr.x0 + cx) as f64 + 0.5, middle, (gr.z0 + cz) as f64 + 0.5], 0))
                .collect();
        }
        // each ground's rectangle on the screen; those that overlap made one, so no patch goes up over
        // another's animals
        let mut rects: Vec<(i32, i32, i32, i32)> = grounds
            .iter()
            .enumerate()
            .map(|(g, (gr, _, _))| {
                let y = animals.iter().find(|a| a.ground == g).map_or(GROUND as f64, |a| a.y);
                rect(gr, y, cam)
            })
            .filter(|r| r.2 > r.0 && r.3 > r.1)
            .collect();
        let mut merged = true;
        while merged {
            merged = false;
            'pairs: for i in 0..rects.len() {
                for j in i + 1..rects.len() {
                    let (a, b) = (rects[i], rects[j]);
                    if a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3 {
                        rects[i] = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
                        rects.swap_remove(j);
                        merged = true;
                        break 'pairs;
                    }
                }
            }
        }
        let patches = rects.into_iter().map(|r| patch(r, frame, tracer, cam)).collect();
        Herd { animals, grounds: grounds.into_iter().map(|(g, _, _)| g).collect(), patches, t: 0.0 }
    }

    /// Step the animals on to the scene's clock at `t`; a number that changes when any of them has moved.
    pub fn advance(&mut self, t: f64) -> u64 {
        while self.t + STEP <= t {
            self.step();
            self.t += STEP;
        }
        self.animals.iter().fold(0u64, |h, a| {
            let legs = ((a.walk.phase * 64.0) as i64).wrapping_mul(0x9e37) ^ ((a.walk.amount * 1024.0) as i64) << 8;
            h.wrapping_mul(31).wrapping_add(((a.at[0] * 64.0) as i64 ^ ((a.at[1] * 64.0) as i64) << 20 ^ ((a.yaw * 64.0) as i64) << 40 ^ legs) as u64)
        })
    }

    fn step(&mut self) {
        let before: Vec<[f64; 2]> = self.animals.iter().map(|a| a.at).collect();
        self.walk();
        // the legs: their swing grows with the ground walked, and eases up while walking, down standing.
        // Twice the block game's swing a block, so a slow walker is still seen to step
        for (a, b) in self.animals.iter_mut().zip(before) {
            let moved = (a.at[0] - b[0]).hypot(a.at[1] - b[1]);
            if swims(a.kind) {
                continue;
            }
            a.walk.phase += moved * 8.0;
            let to = if moved > 1e-6 { WALKING } else { 0.0 };
            a.walk.amount += (to - a.walk.amount) * 0.5;
            if a.walk.amount < 0.01 && to == 0.0 {
                a.walk.amount = 0.0;
            }
        }
    }

    fn walk(&mut self) {
        for i in 0..self.animals.len() {
            let others: Vec<([f64; 2], f64, f64)> =
                self.animals.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, b)| (b.at, b.y, b.kind.girth().0)).collect();
            let a = &mut self.animals[i];
            let ground = &self.grounds[a.ground];
            let (girth, pace) = a.kind.girth();
            if a.wait > 0.0 {
                a.wait -= STEP;
                continue;
            }
            let (dx, dz) = (a.to[0] - a.at[0], a.to[1] - a.at[1]);
            let dist = dx.hypot(dz);
            if dist < 0.15 {
                // there: graze a while, then choose somewhere else
                a.wait = a.rng.range(2.0, 9.0);
                a.to = ground.pick(&mut a.rng, a.at);
                continue;
            }
            // turn towards it, walking only once nearly facing it
            let want = dx.atan2(dz);
            let off = (want - a.yaw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            a.yaw += off.clamp(-3.0 * STEP, 3.0 * STEP);
            if off.abs() > 0.6 {
                continue;
            }
            let step = (pace * STEP).min(dist);
            let next = [a.at[0] + dx / dist * step, a.at[1] + dz / dist * step];
            // its body stays on open ground, clear of everyone else (or getting further from them)
            let clear_ground = [(0.0, 0.0), (girth, 0.0), (-girth, 0.0), (0.0, girth), (0.0, -girth)]
                .iter()
                .all(|&(ox, oz)| ground.open(next[0] + ox * 0.7, next[1] + oz * 0.7));
            let clear_others = others.iter().all(|&(o, oy, og)| {
                let near = (next[0] - o[0]).hypot(next[1] - o[1]);
                (oy - a.y).abs() > 0.5 || near >= girth + og || near > (a.at[0] - o[0]).hypot(a.at[1] - o[1])
            });
            if clear_ground && clear_others {
                a.at = next;
            } else {
                a.wait = a.rng.range(0.5, 1.5);
                a.to = ground.pick(&mut a.rng, a.at);
            }
        }
    }

    /// Each patch with its animals traced into it, as it is now: its place and its three pictures.
    pub fn paint(&self, cam: &Camera) -> Vec<(i32, i32, i32, i32, [Vec<u8>; 3])> {
        self.patches.iter().map(|p| (p.x, p.y, p.w, p.h, self.paint_patch(p, cam))).collect()
    }

    fn paint_patch(&self, p: &Patch, cam: &Camera) -> [Vec<u8>; 3] {
        let (mut base, mut light, mut sun, mut near) = (p.base.clone(), p.light.clone(), p.sun.clone(), p.depth.clone());
        for a in &self.animals {
            let feet = [a.at[0], a.y, a.at[1]];
            let (reach, tall) = mobs::reach(a.kind);
            // its rectangle on the screen
            let mut rect = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (ox, oy, oz) in
                [(-1.0, 0.0, -1.0), (1.0, 0.0, -1.0), (-1.0, 0.0, 1.0), (1.0, 0.0, 1.0), (-1.0, 1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, 1.0, 1.0), (1.0, 1.0, 1.0)]
            {
                if let Some((sx, sy, _)) = cam.project([feet[0] + ox * reach, feet[1] + oy * tall, feet[2] + oz * reach]) {
                    rect = (rect.0.min(sx), rect.1.min(sy), rect.2.max(sx), rect.3.max(sy));
                }
            }
            let (ground_light, ground_sun) = ground_under(p, &self.grounds[a.ground], cam, feet, reach);
            if rect.2 < p.x as f64 || rect.3 < p.y as f64 || rect.0 > (p.x + p.w) as f64 || rect.1 > (p.y + p.h) as f64 {
                continue;
            }
            let x0 = (rect.0.floor() as i32 - 1).max(p.x);
            let y0 = (rect.1.floor() as i32 - 1).max(p.y);
            let x1 = (rect.2.ceil() as i32 + 1).min(p.x + p.w);
            let y1 = (rect.3.ceil() as i32 + 1).min(p.y + p.h);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let d = cam.ray(sx as f64 + 0.5, sy as f64 + 0.5);
                    let Some(h) = mobs::hit_walking(a.kind, feet, a.yaw, a.walk, cam.eye, d) else { continue };
                    let k = ((sy - p.y) * p.w + (sx - p.x)) as usize;
                    if h.t as f32 >= near[k] {
                        continue;
                    }
                    // a duck shows only above the water, a fish through it
                    let hy = cam.eye[1] + d[1] * h.t;
                    if matches!(a.kind, Kind::Duck | Kind::Duckling) && hy < SURFACE {
                        continue;
                    }
                    near[k] = h.t as f32;
                    let (shade, facing) = mob_shade(h.normal);
                    let c = h.colour.map(|v| (v * shade).clamp(0.0, 1.0));
                    let i = k * 4;
                    let byte = |v: f32| (v * 255.0 + 0.5) as u8;
                    if a.kind == Kind::Fish {
                        let w = [p.base[i + 2], p.base[i + 1], p.base[i]].map(|v| v as f32 / 255.0);
                        let tint = [c[0] * 0.75, c[1] * 0.88, c[2]];
                        let mixed: [f32; 3] = std::array::from_fn(|j| tint[j] + (w[j] - tint[j]) * 0.55);
                        base[i..i + 4].copy_from_slice(&[byte(mixed[2]), byte(mixed[1]), byte(mixed[0]), 255]);
                        continue;
                    }
                    base[i..i + 4].copy_from_slice(&[byte(c[2]), byte(c[1]), byte(c[0]), 255]);
                    light[i..i + 4].copy_from_slice(&[ground_light[0], ground_light[1], ground_light[2], 255]);
                    // a side, turned any way: its normal's angle, so the sun on it turns as the animal does,
                    // not in a jump where its nearest axis changes
                    let (g, mat) = if h.normal[1].abs() < 0.5 { (super::sky::side_angle(h.normal), SIDE * 40) } else { (facing * 32, 0) };
                    sun[i..i + 4].copy_from_slice(&[ground_sun, g, mat, 255]);
                }
            }
        }
        [base, light, sun]
    }

    /// The animals as they are now, painted into whole pictures of the screen (`w` pixels across).
    pub fn paint_into(&self, cam: &Camera, frame: &mut Frame, w: usize) {
        for (x, y, pw, ph, pics) in self.paint(cam) {
            for (pic, into) in pics.iter().zip([&mut frame.base, &mut frame.light, &mut frame.sun]) {
                for row in 0..ph as usize {
                    let from = row * pw as usize * 4;
                    let to = ((y as usize + row) * w + x as usize) * 4;
                    into[to..to + pw as usize * 4].copy_from_slice(&pic[from..from + pw as usize * 4]);
                }
            }
        }
    }
}

/// The light on an animal, over its whole footprint: the sky's, the torches' and the haze from the
/// ground's in the picture without it, averaged; the sun's reach at each of its places by the most of
/// the cells under it, at its middle (`Ground::sun`). Read at one point of the ground under its feet, a
/// thin shadow there (a fence's, a blade of grass) flipped the whole animal between sunlit and shaded as
/// it walked.
fn ground_under(p: &Patch, gr: &Ground, cam: &Camera, feet: [f64; 3], reach: f64) -> ([u8; 3], u8) {
    const N: usize = 5;
    let at = |sx: f64, sy: f64| ((sy as i32 - p.y).clamp(0, p.h - 1) * p.w + (sx as i32 - p.x).clamp(0, p.w - 1)) as usize * 4;
    let (mut sum, mut seen, mut votes, mut cells) = ([0u32; 3], 0u32, [0u32; 8], 0u32);
    for i in 0..N {
        for j in 0..N {
            let [ox, oz] = [i, j].map(|k| (k as f64 / (N - 1) as f64 * 2.0 - 1.0) * reach);
            let (x, z) = (feet[0] + ox, feet[2] + oz);
            let (cx, cz) = (x.floor() as i32 - gr.x0, z.floor() as i32 - gr.z0);
            if cx >= 0 && cz >= 0 && cx < gr.w && cz < gr.h {
                let bits = gr.sun[(cz * gr.w + cx) as usize];
                for (b, v) in votes.iter_mut().enumerate() {
                    *v += ((bits >> b) & 1) as u32;
                }
                cells += 1;
            }
            let Some((sx, sy, _)) = cam.project([x, feet[1], z]) else { continue };
            if sx < p.x as f64 || sy < p.y as f64 || sx >= (p.x + p.w) as f64 || sy >= (p.y + p.h) as f64 {
                continue;
            }
            let q = at(sx, sy);
            for (s, v) in sum.iter_mut().zip(&p.light[q..q + 3]) {
                *s += *v as u32;
            }
            seen += 1;
        }
    }
    // none of it on the patch, or off its ground: the point under its feet, kept to the patch
    let q = cam.project(feet).map_or(0, |(sx, sy, _)| at(sx, sy));
    let light = if seen == 0 { [p.light[q], p.light[q + 1], p.light[q + 2]] } else { sum.map(|s| ((s + seen / 2) / seen) as u8) };
    let sun = if cells == 0 { p.sun[q] } else { votes.iter().enumerate().fold(0u8, |bits, (b, &v)| if v * 2 > cells { bits | (1 << b) } else { bits }) };
    (light, sun)
}

/// The ground an animal of a kind keeps to within `area`: for one that walks, open cells on firm ground
/// (not a fence, a crop, a trough or someone standing); for a duck, water open to the sky; for a fish,
/// water below the surface.
fn ground(w: &World, area: [i32; 4], kind: Kind, y: f64) -> Ground {
    let (x0, z0, x1, z1) = (area[0], area[1], area[2], area[3]);
    let (gw, gh) = (x1 - x0 + 1, z1 - z0 + 1);
    let feet = y.floor() as i32;
    let walkable = |b: u8| b == AIR || block(b).shape == Shape::Cross;
    let open = (0..gh)
        .flat_map(|cz| (0..gw).map(move |cx| (x0 + cx, z0 + cz)))
        .map(|(x, z)| match kind {
            Kind::Fish => w.get(x, feet, z) == WATER && w.get(x, feet + 1, z) == WATER,
            Kind::Duck | Kind::Duckling => w.get(x, feet, z) == WATER && matches!(w.get(x, feet + 1, z), AIR | LILY),
            _ => walkable(w.get(x, feet, z)) && walkable(w.get(x, feet + 1, z)) && matches!(block(w.get(x, feet - 1, z)).shape, Shape::Cube | Shape::Boxes),
        })
        .collect();
    Ground { x0, z0, w: gw, h: gh, open, sun: vec![255; (gw * gh) as usize] }
}

/// The rectangle of the screen a ground can be seen in, animals two blocks tall on it: left, top, right,
/// bottom.
fn rect(g: &Ground, y: f64, cam: &Camera) -> (i32, i32, i32, i32) {
    let (sw, sh) = (cam.w as i32, cam.h as i32);
    let mut rect = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, z) in [(g.x0, g.z0), (g.x0 + g.w, g.z0), (g.x0, g.z0 + g.h), (g.x0 + g.w, g.z0 + g.h)] {
        for dy in [-0.5, 2.2] {
            if let Some((sx, sy, _)) = cam.project([x as f64, y + dy, z as f64]) {
                rect = (rect.0.min(sx), rect.1.min(sy), rect.2.max(sx), rect.3.max(sy));
            }
        }
    }
    if rect.0 > rect.2 {
        return (0, 0, 0, 0);
    }
    (
        (rect.0.floor() as i32 - 4).clamp(0, sw),
        (rect.1.floor() as i32 - 4).clamp(0, sh),
        (rect.2.ceil() as i32 + 4).clamp(0, sw),
        (rect.3.ceil() as i32 + 4).clamp(0, sh),
    )
}

/// A patch of the screen kept as baked, with how far off each pixel is.
fn patch(r: (i32, i32, i32, i32), frame: &Frame, tracer: &Tracer, cam: &Camera) -> Patch {
    let sw = cam.w as i32;
    let (x, yy, w, h) = (r.0, r.1, r.2 - r.0, r.3 - r.1);
    let take = |pic: &[u8]| -> Vec<u8> {
        (0..h as usize)
            .flat_map(|r| {
                let from = ((yy as usize + r) * sw as usize + x as usize) * 4;
                pic[from..from + w as usize * 4].iter().copied()
            })
            .collect()
    };
    let depth = (0..h).flat_map(|r| (0..w).map(move |c| (c, r))).map(|(c, r)| tracer.depth((x + c) as f64 + 0.5, (yy + r) as f64 + 0.5)).collect();
    Patch { x, y: yy, w, h, base: take(&frame.base), light: take(&frame.light), sun: take(&frame.sun), depth }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_animal_is_lit_by_the_sun_at_its_middle_and_the_light_about_it() {
        let cam = Camera::new([0.0, 74.0, 0.0], 0.0, -0.4, 70.0, 320, 180);
        let feet = [0.5, 64.0, 12.0];
        let (fx, fy, _) = cam.project(feet).unwrap();
        let (w, h) = (320, 180);
        // the ground's picture: a thin shadow across under its feet, the light brighter there than about
        let mut sun = vec![255u8; w * h * 4];
        let mut light = vec![200u8; w * h * 4];
        for x in 0..w {
            sun[(fy as usize * w + x) * 4] = 0;
            light[(fy as usize * w + x) * 4] = 0;
        }
        let p = Patch { x: 0, y: 0, w: w as i32, h: h as i32, base: vec![0; w * h * 4], light, sun, depth: vec![0.0; w * h] };
        assert_eq!(p.sun[(fy as usize * w + fx as usize) * 4], 0, "the shadow is under its feet");
        let open = Ground { x0: -4, z0: 8, w: 9, h: 9, open: vec![true; 81], sun: vec![255; 81] };
        let (light, bits) = ground_under(&p, &open, &cam, feet, 0.45);
        assert_eq!(bits, 255, "in the sun at its middle, over the ground's shadow");
        assert!(light[0] > 150 && light[1] == 200, "the light about it, the thin dark row outvoted: {light:?}");
        // where the sun does not reach its middle, it is shaded
        let shaded = Ground { sun: vec![0; 81], ..open };
        assert_eq!(ground_under(&p, &shaded, &cam, feet, 0.45).1, 0);
    }

    #[test]
    fn animals_keep_to_their_ground_and_clear_of_each_other() {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let tex = super::super::textures::Textures::new(7);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 96, 54);
        let tracer = Tracer::new(&w, &tex, cam, (130.0, 190.0), super::super::sky::arcs(&cam), false);
        let frame = tracer.render(1);
        let mut herd = Herd::new(&w, &tracer, &frame, &cam, 7);
        assert!(herd.animals.len() > 20);
        let start: Vec<[f64; 2]> = herd.animals.iter().map(|a| a.at).collect();
        let n = herd.animals.len();
        let gap = |h: &Herd, i: usize, j: usize| (h.animals[i].at[0] - h.animals[j].at[0]).hypot(h.animals[i].at[1] - h.animals[j].at[1]);
        let mut was: Vec<f64> = (0..n * n).map(|k| gap(&herd, k / n, k % n)).collect();
        for k in 1..=6000 {
            herd.advance(k as f64 * STEP);
            for i in 0..n {
                let a = &herd.animals[i];
                assert!(herd.grounds[a.ground].open(a.at[0], a.at[1]), "{:?} off its ground at {:?}", a.kind, a.at);
                for j in i + 1..n {
                    let b = &herd.animals[j];
                    let (now, girth) = (gap(&herd, i, j), a.kind.girth().0 + b.kind.girth().0);
                    // apart they stay apart; two put down too close may only draw apart
                    if (a.y - b.y).abs() < 0.5 && now < girth - 1e-9 {
                        assert!(now >= was[i * n + j] - 1e-9, "{:?} and {:?} walked into each other: {now:.2} apart", a.kind, b.kind);
                    }
                    was[i * n + j] = now;
                }
            }
        }
        let moved = herd.animals.iter().zip(&start).filter(|(a, s)| (a.at[0] - s[0]).hypot(a.at[1] - s[1]) > 1.0).count();
        assert!(moved > herd.animals.len() / 2, "they wander: {moved} of {}", herd.animals.len());
    }
}
