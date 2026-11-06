//! The animals that wander: sheep, cows and pigs about their pens, chickens about the run and the yards,
//! ducks and their ducklings over the pond, fish under it. Each has the ground it keeps to, walks to a
//! spot on it, turns and walks again, grazes a while; it keeps clear of the fences and of every other
//! animal, stopping and choosing elsewhere where one is in the way. They step ten times a second on the
//! scene's clock, so a moment is always the same.
//!
//! They are left out of the baked picture. For each ground the patch of the screen it can be seen in is
//! kept as baked, with how far off each of its pixels is; each frame they move, the animals are traced
//! into a copy of the patch, before what is behind them and behind what is before them, and the patch
//! goes up in place of the baked one. They are lit by the time of day alone, as if in the open, so they
//! keep their colour as they walk in and out of shadows.

use super::mobs::{self, Kind};
use super::render::{self, Camera, Frame, SIDE, mob_shade};
use super::world::*;
use crate::rng::Rng;
use crate::trace::camera::{Px, bounds, overlap, pixels};
use crate::trace::rig::Pose;
use crate::trace::sprites::{Backdrop, Changes, merge};
use std::collections::HashMap;

/// A step of the animals, in seconds.
const STEP: f64 = 0.1;

/// Where an animal may stand: a grid of cells, open or not, and the height its feet are at.
struct Ground {
    x0: i32,
    z0: i32,
    w: i32,
    h: i32,
    open: Vec<bool>,
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
    /// Each ground's patch of the screen, as baked: the pictures what is seen, its light and the sun.
    patches: Vec<Backdrop>,
    /// Where the haze starts and where it hides everything (`render::haze`).
    fog: (f64, f64),
    /// What has changed since the animals were last painted (`pose_key`).
    shown: Changes<[u64; 5]>,
    /// What each animal looked like in each patch when last traced whole there, by its pose: its pixels
    /// to lay again, so an animal standing still under one that moves is not traced again.
    kept: Kept,
    /// The time stepped to.
    t: f64,
}

/// Each animal's pixels in each patch (by their places in the herd), as last traced whole there, and the
/// pose they were traced in (`pose_key`).
type Kept = HashMap<(usize, usize), ([u64; 5], Vec<Frag>)>;

/// A pixel of an animal as traced: where, how far, and what it paints: what is seen, and for all but
/// a fish the light and the sun there (a fish shows through the water and keeps the water's).
#[derive(Clone, Copy)]
struct Frag {
    x: i32,
    y: i32,
    t: f32,
    base: [u8; 4],
    lit: Option<([u8; 4], [u8; 4])>,
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
    /// `depth` is how far off a pixel of the screen is (`Tracer::depth`), asked for the patches' pixels
    /// in turn, row by row (`depths` gives them back in that order); `fog` is the tracer's.
    pub fn new(w: &World, fog: (f64, f64), frame: &Frame, cam: &Camera, seed: i64, mut depth: impl FnMut(i32, i32) -> f32) -> Herd {
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
        merge(&mut rects);
        let patches = rects.into_iter().map(|r| Backdrop::take(r, cam.w, &[&frame.base, &frame.light, &frame.sun], &mut depth)).collect();
        let shown = Changes::new(animals.len());
        Herd { animals, grounds: grounds.into_iter().map(|(g, _, _)| g).collect(), patches, fog, shown, kept: Default::default(), t: 0.0 }
    }

    /// How far off each of the patches' pixels is, in the order `new` asks for them.
    pub fn depths(&self) -> impl Iterator<Item = f32> + '_ {
        self.patches.iter().flat_map(|p| p.depth.iter().copied())
    }

    /// Step the animals on to the scene's clock at `t`; a number that changes when any of them has moved.
    pub fn advance(&mut self, t: f64) -> u64 {
        while self.t + STEP <= t {
            self.step();
            self.t += STEP;
        }
        // of everything each one's picture depends on, exactly, so no move goes unpainted
        self.animals.iter().flat_map(pose_key).fold(0xcbf2_9ce4_8422_2325u64, |h, v| (h ^ v).wrapping_mul(0x0100_0000_01b3))
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

    /// What has changed of the patches since the last call, with the animals traced in as they are now:
    /// each region's place and its three pictures. An animal that has not moved since is left as it was
    /// painted; where one has moved, the region it left and the one it is in now are painted again, with
    /// every animal over them. A pixel's colour is the baked one and the animals over it, so a region
    /// painted again is what painting the whole patch would make there.
    pub fn paint(&mut self, cam: &Camera) -> Vec<(i32, i32, i32, i32, [Vec<u8>; 3])> {
        let seen: Vec<Option<Seen>> = self.animals.iter().map(|a| Seen::new(a, cam)).collect();
        // an animal hidden where it is (behind a house) changes nothing it moves
        let patches = &self.patches;
        let visible = |s: &Seen| !patches.iter().all(|p| p.hides(s.pixels, s.pose.near() as f32));
        let dirty = self.shown.since(self.animals.iter().zip(&seen).map(|(a, s)| (pose_key(a), s.as_ref().filter(|s| visible(s)).map(|s| s.pixels))));
        let mut out = vec![];
        let mut kept = std::mem::take(&mut self.kept);
        for (pi, p) in self.patches.iter().enumerate() {
            for clip in dirty.iter().filter_map(|&d| overlap(d, p.at)) {
                out.push((clip.0, clip.1, clip.2 - clip.0, clip.3 - clip.1, self.paint_region(pi, p, clip, &seen, cam, &mut kept)));
            }
        }
        self.kept = kept;
        out
    }

    /// The region `clip` of a patch, baked, with the animals traced into it.
    /// `kept`: each animal's pixels in each patch as last traced whole there, by its pose.
    fn paint_region(&self, pi: usize, p: &Backdrop, clip: Px, seen: &[Option<Seen>], cam: &Camera, kept: &mut Kept) -> [Vec<u8>; 3] {
        let rw = (clip.2 - clip.0) as usize;
        let (planes, mut near) = p.crop(clip);
        let [mut base, mut light, mut sun]: [Vec<u8>; 3] = planes.try_into().unwrap_or_default();
        for (ai, (a, s)) in self.animals.iter().zip(seen).enumerate() {
            let Some(Seen { rect, pixels, pose }) = s else { continue };
            // an animal is traced into a patch its reach's rectangle is on
            if rect.2 < p.at.0 as f64 || rect.3 < p.at.1 as f64 || rect.0 > p.at.2 as f64 || rect.1 > p.at.3 as f64 {
                continue;
            }
            let Some(area) = overlap(*pixels, clip) else { continue };
            let key = pose_key(a);
            // its pixels as traced in this pose before, if they were all traced; else traced now, and
            // kept where this region holds all of it in this patch
            let frags: Vec<Frag> = match kept.get(&(ai, pi)) {
                Some((k, frags)) if *k == key => frags.iter().filter(|f| f.x >= area.0 && f.x < area.2 && f.y >= area.1 && f.y < area.3).copied().collect(),
                _ => {
                    let whole = overlap(*pixels, p.at) == Some(area);
                    let frags = self.trace_frags(a, pose, p, area, cam);
                    if whole {
                        kept.insert((ai, pi), (key, frags.clone()));
                    }
                    frags
                }
            };
            for f in frags {
                let k = (f.y - clip.1) as usize * rw + (f.x - clip.0) as usize;
                if f.t >= near[k] {
                    continue;
                }
                near[k] = f.t;
                let i = k * 4;
                base[i..i + 4].copy_from_slice(&f.base);
                if let Some((l, u)) = f.lit {
                    light[i..i + 4].copy_from_slice(&l);
                    sun[i..i + 4].copy_from_slice(&u);
                }
            }
        }
        [base, light, sun]
    }

    /// An animal traced over `area` of a patch: each pixel it is seen in, with what it paints there.
    fn trace_frags(&self, a: &Animal, pose: &Pose, p: &Backdrop, area: Px, cam: &Camera) -> Vec<Frag> {
        let baked = &p.planes[0];
        let mut out = vec![];
        // each face's shade, facing and material: the same wherever a ray meets it in this pose
        let mut faces: Vec<Option<(f32, u8, u8)>> = vec![];
        pose.trace(
            cam,
            area,
            |x, y| p.depth[p.index(x, y)],
            |sx, sy, d, h| {
                // a duck shows only above the water, a fish through it
                let hy = cam.eye[1] + d[1] * h.t;
                if matches!(a.kind, Kind::Duck | Kind::Duckling) && hy < SURFACE {
                    return;
                }
                let f = h.part * 6 + h.axis * 2 + (h.sign > 0.0) as usize;
                if faces.len() <= f {
                    faces.resize(f + 1, None);
                }
                let (shade, g, mat) = *faces[f].get_or_insert_with(|| {
                    let (shade, facing) = mob_shade(h.normal);
                    // a side, turned any way: its normal's angle, so the sun on it turns as the animal
                    // does, not in a jump where its nearest axis changes
                    let (g, mat) = if h.normal[1].abs() < 0.5 { (super::sky::side_angle(h.normal), SIDE * 40) } else { (facing * 32, 0) };
                    (shade, g, mat)
                });
                let c = mobs::coat(a.kind, &h).map(|v| (v * shade).clamp(0.0, 1.0));
                let byte = |v: f32| (v * 255.0 + 0.5) as u8;
                let t = h.t as f32;
                if a.kind == Kind::Fish {
                    let b = p.index(sx, sy) * 4;
                    let w = [baked[b + 2], baked[b + 1], baked[b]].map(|v| v as f32 / 255.0);
                    let tint = [c[0] * 0.75, c[1] * 0.88, c[2]];
                    let mixed: [f32; 3] = std::array::from_fn(|j| tint[j] + (w[j] - tint[j]) * 0.55);
                    out.push(Frag { x: sx, y: sy, t, base: [byte(mixed[2]), byte(mixed[1]), byte(mixed[0]), 255], lit: None });
                    return;
                }
                // lit by the time of day alone: the open sky's light, no torch's, and the sun at every
                // place on its arc. Lit by the ground under it, a shadow it walked into (a fence's, a
                // house's) changed its colour at once; the haze by its distance
                let haze = byte(render::haze(self.fog, d, h.t));
                out.push(Frag { x: sx, y: sy, t, base: [byte(c[2]), byte(c[1]), byte(c[0]), 255], lit: Some(([haze, 0, 255, 255], [255, g, mat, 255])) });
            },
        );
        out
    }

    /// The animals as they are now, painted into whole pictures of the screen (`w` pixels across).
    pub fn paint_into(&mut self, cam: &Camera, frame: &mut Frame, w: usize) {
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

/// An animal as the screen sees it this frame: the rectangle of the box its reach makes about its feet
/// (left, top, right, bottom), which says which patches it is traced into; the pixels it can be in,
/// those of the box about its parts as they stand where all of that is in front of the eye; and its
/// pose.
struct Seen {
    rect: (f64, f64, f64, f64),
    pixels: Px,
    pose: Pose,
}

impl Seen {
    /// None where all of it is behind the eye.
    fn new(a: &Animal, cam: &Camera) -> Option<Seen> {
        let (reach, tall) = mobs::reach(a.kind);
        let feet = [a.at[0], a.y, a.at[1]];
        const UNITS: [(f64, f64, f64); 8] =
            [(-1.0, 0.0, -1.0), (1.0, 0.0, -1.0), (-1.0, 0.0, 1.0), (1.0, 0.0, 1.0), (-1.0, 1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, 1.0, 1.0), (1.0, 1.0, 1.0)];
        let rect = bounds(cam, UNITS.map(|(ox, oy, oz)| [feet[0] + ox * reach, feet[1] + oy * tall, feet[2] + oz * reach])).0?;
        let pose = mobs::pose(a.kind, feet, a.yaw, a.walk, cam.eye);
        let all = pixels(rect);
        let pixels = pose.pixels(cam).and_then(|p| overlap(p, all)).unwrap_or(all);
        Some(Seen { rect, pixels, pose })
    }
}

/// Everything an animal's picture depends on, exactly: where it is, its turn and its legs.
fn pose_key(a: &Animal) -> [u64; 5] {
    [a.at[0], a.at[1], a.yaw, a.walk.phase, a.walk.amount].map(f64::to_bits)
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
    Ground { x0, z0, w: gw, h: gh, open }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::blocky::render::Tracer;

    #[test]
    fn animals_keep_to_their_ground_and_clear_of_each_other() {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let tex = super::super::textures::Textures::new(7);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 96, 54);
        let tracer = Tracer::new(&w, &tex, cam, (130.0, 190.0), super::super::sky::arcs(&cam), false);
        let frame = tracer.render(1);
        let mut herd = Herd::new(&w, tracer.fog, &frame, &cam, 7, |x, y| tracer.depth(x as f64 + 0.5, y as f64 + 0.5));
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

    #[test]
    fn painting_what_changed_makes_what_painting_everything_does() {
        let ahead = (35f64.to_radians().sin(), 35f64.to_radians().cos());
        let w = World::new(7, ahead);
        let tex = super::super::textures::Textures::new(7);
        let cam = Camera::new(w.eye, 35f64.to_radians(), -13f64.to_radians(), 78.0, 480, 270);
        let tracer = Tracer::new(&w, &tex, cam, (130.0, 190.0), super::super::sky::arcs(&cam), false);
        let bare = tracer.render(1);
        let depth = |x: i32, y: i32| tracer.depth(x as f64 + 0.5, y as f64 + 0.5);
        let (mut herd, mut fresh) = (Herd::new(&w, tracer.fog, &bare, &cam, 7, depth), Herd::new(&w, tracer.fog, &bare, &cam, 7, depth));
        let lay = |into: &mut Frame, h: &mut Herd| {
            for (x, y, pw, ph, pics) in h.paint(&cam) {
                for (pic, to) in pics.iter().zip([&mut into.base, &mut into.light, &mut into.sun]) {
                    for row in 0..ph as usize {
                        let at = ((y as usize + row) * cam.w + x as usize) * 4;
                        to[at..at + pw as usize * 4].copy_from_slice(&pic[row * pw as usize * 4..(row + 1) * pw as usize * 4]);
                    }
                }
            }
        };
        let copy = |f: &Frame| Frame { base: f.base.clone(), light: f.light.clone(), sun: f.sun.clone() };
        let mut kept = copy(&bare);
        let mut changed = 0;
        for k in 1..=300 {
            herd.advance(k as f64 * STEP);
            fresh.advance(k as f64 * STEP);
            lay(&mut kept, &mut herd);
            // everything painted again, over the picture without the animals
            fresh.shown = Changes::new(fresh.animals.len());
            let mut all = copy(&bare);
            lay(&mut all, &mut fresh);
            assert!(kept.base == all.base && kept.light == all.light && kept.sun == all.sun, "step {k}");
            changed += (all.base != bare.base) as usize;
        }
        assert!(changed > 250, "the animals are seen: {changed}");
    }
}
