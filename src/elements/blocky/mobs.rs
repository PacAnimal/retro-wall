//! The animals and villagers: each a few boxes, measured in the block game's sixteenths of a block,
//! standing where the farm puts them and turned about their feet. Their faces are pixel art; their
//! coats a texel's noise, a cow's patches, a cat's stripes.

use super::world::{AIR, MOB, RAIL, RAIL_Z, Shape, WATER, World, block};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Sheep,
    Cow,
    Pig,
    Chicken,
    Cat,
    Villager,
    Farmer,
    Duck,
    Duckling,
    Fish,
    Cart,
}

impl Kind {
    /// Whether it wanders (`herd`), rather than standing where it was put, baked into the picture.
    pub fn roams(self) -> bool {
        matches!(self, Kind::Sheep | Kind::Cow | Kind::Pig | Kind::Chicken | Kind::Duck | Kind::Duckling | Kind::Fish)
    }
    /// Its size about its feet, in blocks, for keeping clear of others: and its pace, blocks a second.
    pub fn girth(self) -> (f64, f64) {
        match self {
            Kind::Cow => (0.75, 0.5),
            Kind::Sheep | Kind::Pig => (0.6, 0.6),
            Kind::Chicken => (0.3, 0.9),
            Kind::Duck => (0.35, 0.4),
            Kind::Duckling => (0.2, 0.5),
            Kind::Fish => (0.3, 0.6),
            _ => (0.5, 0.0),
        }
    }
}

/// How a box's faces are painted: a colour with a darker one sprinkled in, in patches, or in stripes;
/// a face on its front.
#[derive(Clone, Copy)]
enum Coat {
    Fur(u32, u32),
    Patches(u32, u32),
    Stripes(u32, u32),
    Face(u32, u32, &'static [&'static str], &'static [(char, u32)]),
}

/// A box of a model: its corners in sixteenths (x across, y up, z forward from the feet) and its coat.
struct Part {
    lo: [f64; 3],
    hi: [f64; 3],
    coat: Coat,
}

const fn part(lo: [f64; 3], hi: [f64; 3], coat: Coat) -> Part {
    Part { lo, hi, coat }
}

const EYES: &[(char, u32)] = &[('w', 0xffffff), ('k', 0x1a1a1a), ('p', 0xe8a0a0), ('n', 0x6a3a2a), ('g', 0x2a8a3a), ('b', 0x3a2a1a), ('y', 0xd8c040)];

const SHEEP_FACE: &[&str] = &["......", "......", "wk..kw", "......", "..pp..", "......", "......"];
const PIG_FACE: &[&str] = &["........", "........", "........", "wk....kw", "........", "........", "........", "........"];
const SNOUT: &[&str] = &["....", ".nn.", "...."];
const COW_FACE: &[&str] = &["........", "........", "........", "kw....wk", "........", "........", "........", "........"];
const MUZZLE: &[&str] = &["....", "n..n", "...."];
const CHICKEN_FACE: &[&str] = &["....", "k..k", "....", "...."];
const DUCK_FACE: &[&str] = &["...", "k.k", "...", "..."];
const DUCKLING_FACE: &[&str] = &["..", "kk", ".."];
const FISH_FACE: &[&str] = &["..", "k.", ".."];
const CAT_FACE: &[&str] = &[".....", ".....", "gk.kg", ".....", "..p.."];
const VILLAGER_FACE: &[&str] = &["........", "........", "........", "........", ".bbbbbb.", "wg....gw", "........", "........", "........", "........"];

fn model(kind: Kind) -> &'static [Part] {
    use Coat::*;
    const SHEEP: &[Part] = &[
        part([-4.0, 0.0, -6.0], [-1.0, 6.0, -3.0], Fur(0xcdb49c, 0xbca189)),
        part([1.0, 0.0, -6.0], [4.0, 6.0, -3.0], Fur(0xcdb49c, 0xbca189)),
        part([-4.0, 0.0, 3.0], [-1.0, 6.0, 6.0], Fur(0xcdb49c, 0xbca189)),
        part([1.0, 0.0, 3.0], [4.0, 6.0, 6.0], Fur(0xcdb49c, 0xbca189)),
        part([-5.0, 6.0, -8.0], [5.0, 15.0, 8.0], Fur(0xeaeaea, 0xd2d2d2)),
        part([-3.0, 11.0, 7.0], [3.0, 18.0, 13.0], Face(0xd9b79e, 0xc8a68c, SHEEP_FACE, EYES)),
        part([-3.5, 16.0, 6.5], [3.5, 18.5, 11.0], Fur(0xeaeaea, 0xd2d2d2)),
    ];
    const COW: &[Part] = &[
        part([-6.0, 0.0, -8.0], [-2.0, 12.0, -4.0], Patches(0x4a3626, 0xe8e6de)),
        part([2.0, 0.0, -8.0], [6.0, 12.0, -4.0], Patches(0x4a3626, 0xe8e6de)),
        part([-6.0, 0.0, 4.0], [-2.0, 12.0, 8.0], Patches(0x4a3626, 0xe8e6de)),
        part([2.0, 0.0, 4.0], [6.0, 12.0, 8.0], Patches(0x4a3626, 0xe8e6de)),
        part([-6.0, 12.0, -9.0], [6.0, 22.0, 9.0], Patches(0x4a3626, 0xe8e6de)),
        part([-4.0, 15.0, 9.0], [4.0, 23.0, 15.0], Face(0x4a3626, 0x3e2c1e, COW_FACE, EYES)),
        part([-2.0, 15.0, 15.0], [2.0, 18.0, 16.0], Face(0xc8a890, 0xb89880, MUZZLE, EYES)),
        part([-6.0, 21.0, 11.0], [-4.0, 24.0, 13.0], Fur(0xddd5c4, 0xc8bfae)),
        part([4.0, 21.0, 11.0], [6.0, 24.0, 13.0], Fur(0xddd5c4, 0xc8bfae)),
    ];
    const PIG: &[Part] = &[
        part([-4.0, 0.0, -6.0], [-1.0, 6.0, -3.0], Fur(0xeea09e, 0xe08e8c)),
        part([1.0, 0.0, -6.0], [4.0, 6.0, -3.0], Fur(0xeea09e, 0xe08e8c)),
        part([-4.0, 0.0, 3.0], [-1.0, 6.0, 6.0], Fur(0xeea09e, 0xe08e8c)),
        part([1.0, 0.0, 3.0], [4.0, 6.0, 6.0], Fur(0xeea09e, 0xe08e8c)),
        part([-5.0, 6.0, -8.0], [5.0, 14.0, 8.0], Fur(0xf2a8a6, 0xe69896)),
        part([-4.0, 7.0, 8.0], [4.0, 15.0, 15.0], Face(0xf2a8a6, 0xe69896, PIG_FACE, EYES)),
        part([-2.0, 8.0, 15.0], [2.0, 11.0, 16.0], Face(0xe88c8c, 0xd87c7c, SNOUT, EYES)),
    ];
    const CHICKEN: &[Part] = &[
        part([-2.0, 0.0, -0.5], [-1.0, 5.0, 0.5], Fur(0xe89a2a, 0xd08020)),
        part([1.0, 0.0, -0.5], [2.0, 5.0, 0.5], Fur(0xe89a2a, 0xd08020)),
        part([-3.0, 5.0, -4.0], [3.0, 11.0, 4.0], Fur(0xf6f6f6, 0xe2e2e2)),
        part([-4.0, 7.0, -3.0], [-3.0, 11.0, 3.0], Fur(0xeeeeee, 0xdadada)),
        part([3.0, 7.0, -3.0], [4.0, 11.0, 3.0], Fur(0xeeeeee, 0xdadada)),
        part([-2.0, 9.0, 3.0], [2.0, 15.0, 6.0], Face(0xf6f6f6, 0xe2e2e2, CHICKEN_FACE, EYES)),
        part([-2.0, 11.0, 6.0], [2.0, 13.0, 8.0], Fur(0xf0b020, 0xe0a010)),
        part([-1.0, 9.0, 6.0], [1.0, 11.0, 7.0], Fur(0xd02020, 0xb01010)),
    ];
    const CAT: &[Part] = &[
        part([-2.0, 0.0, -6.0], [-0.4, 6.0, -4.0], Stripes(0xe3a050, 0xb06a28)),
        part([0.4, 0.0, -6.0], [2.0, 6.0, -4.0], Stripes(0xe3a050, 0xb06a28)),
        part([-2.0, 0.0, 4.0], [-0.4, 6.0, 6.0], Stripes(0xe3a050, 0xb06a28)),
        part([0.4, 0.0, 4.0], [2.0, 6.0, 6.0], Stripes(0xe3a050, 0xb06a28)),
        part([-2.0, 5.0, -8.0], [2.0, 10.0, 8.0], Stripes(0xe3a050, 0xb06a28)),
        part([-2.5, 7.0, 7.0], [2.5, 12.0, 12.0], Face(0xe3a050, 0xd08c40, CAT_FACE, EYES)),
        part([-2.5, 12.0, 9.0], [-1.0, 13.5, 10.5], Fur(0xe3a050, 0xb06a28)),
        part([1.0, 12.0, 9.0], [2.5, 13.5, 10.5], Fur(0xe3a050, 0xb06a28)),
        part([-0.5, 9.0, -16.0], [0.5, 10.0, -8.0], Stripes(0xe3a050, 0xb06a28)),
    ];
    const VILLAGER: &[Part] = &[
        part([-4.0, 0.0, -3.0], [4.0, 20.0, 3.0], Fur(0x7a5634, 0x6a4a2c)),
        part([-4.0, 13.0, 3.0], [4.0, 17.0, 7.0], Fur(0x7a5634, 0x6a4a2c)),
        part([-4.0, 20.0, -4.0], [4.0, 30.0, 4.0], Face(0xbd8b72, 0xae7e66, VILLAGER_FACE, EYES)),
        part([-1.0, 19.0, 4.0], [1.0, 23.0, 6.0], Fur(0xb07e66, 0xa0705a)),
    ];
    const FARMER: &[Part] = &[
        part([-4.0, 0.0, -3.0], [4.0, 20.0, 3.0], Fur(0x6a7a3a, 0x5a6a30)),
        part([-4.0, 13.0, 3.0], [4.0, 17.0, 7.0], Fur(0x6a7a3a, 0x5a6a30)),
        part([-4.0, 20.0, -4.0], [4.0, 30.0, 4.0], Face(0xbd8b72, 0xae7e66, VILLAGER_FACE, EYES)),
        part([-1.0, 19.0, 4.0], [1.0, 23.0, 6.0], Fur(0xb07e66, 0xa0705a)),
        part([-6.0, 29.0, -6.0], [6.0, 30.0, 6.0], Fur(0xd8b860, 0xc0a048)),
        part([-4.0, 30.0, -4.0], [4.0, 32.0, 4.0], Fur(0xd8b860, 0xc0a048)),
    ];
    // a mallard: a grey back, a brown breast, a green head, an orange bill; it floats
    const DUCK: &[Part] = &[
        part([-2.5, 0.0, -4.0], [2.5, 4.0, 3.0], Fur(0xbab4a6, 0xa49e90)),
        part([-2.5, 0.0, 2.0], [2.5, 3.5, 4.0], Fur(0x7a4a2a, 0x6a3e22)),
        part([-1.5, 2.0, -5.5], [1.5, 4.0, -4.0], Fur(0x2a2a2a, 0x1a1a1a)),
        part([-1.5, 3.5, 2.5], [1.5, 7.0, 5.5], Face(0x1f6b2a, 0x195a22, DUCK_FACE, EYES)),
        part([-1.6, 3.4, 2.4], [1.6, 4.0, 5.6], Fur(0xf0f0f0, 0xe0e0e0)),
        part([-1.0, 4.0, 5.5], [1.0, 5.0, 7.5], Fur(0xe8a030, 0xd89020)),
    ];
    const DUCKLING: &[Part] = &[
        part([-1.5, 0.0, -2.0], [1.5, 2.5, 2.0], Fur(0xf2d850, 0xe2c840)),
        part([-1.0, 2.0, 1.0], [1.0, 4.0, 3.0], Face(0xf2d850, 0xe2c840, DUCKLING_FACE, EYES)),
        part([-0.5, 2.5, 3.0], [0.5, 3.0, 4.0], Fur(0xe89a2a, 0xd08020)),
    ];
    // a cod, under the water
    const FISH: &[Part] = &[
        part([-1.0, 0.0, -3.0], [1.0, 3.0, 4.0], Fur(0x9c8058, 0x8a6e48)),
        part([-1.0, 0.5, 4.0], [1.0, 2.5, 6.0], Face(0x8a6e48, 0x7a5e3a, FISH_FACE, EYES)),
        part([-0.3, 0.0, -5.5], [0.3, 3.0, -3.0], Fur(0x7a5e3a, 0x6a4e2e)),
    ];
    // a cart: an iron floor and four low walls, open on top, on the rails
    const CART: &[Part] = &[
        part([-6.0, 1.0, -8.0], [6.0, 3.0, 8.0], Fur(0x5a5a5a, 0x4a4a4a)),
        part([-6.0, 3.0, -8.0], [-5.0, 9.0, 8.0], Fur(0x8a8a8a, 0x7a7a7a)),
        part([5.0, 3.0, -8.0], [6.0, 9.0, 8.0], Fur(0x8a8a8a, 0x7a7a7a)),
        part([-5.0, 3.0, -8.0], [5.0, 9.0, -7.0], Fur(0x8a8a8a, 0x7a7a7a)),
        part([-5.0, 3.0, 7.0], [5.0, 9.0, 8.0], Fur(0x8a8a8a, 0x7a7a7a)),
    ];
    match kind {
        Kind::Cart => CART,
        Kind::Duck => DUCK,
        Kind::Duckling => DUCKLING,
        Kind::Fish => FISH,
        Kind::Sheep => SHEEP,
        Kind::Cow => COW,
        Kind::Pig => PIG,
        Kind::Chicken => CHICKEN,
        Kind::Cat => CAT,
        Kind::Villager => VILLAGER,
        Kind::Farmer => FARMER,
    }
}

fn rgb(h: u32) -> [f32; 3] {
    [((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0]
}

fn hash(a: i32, b: i32, c: i32) -> u32 {
    let mut h = (a as u32).wrapping_mul(0x8da6_b343) ^ (b as u32).wrapping_mul(0xd816_3841) ^ (c as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

struct Mob {
    kind: Kind,
    /// Its feet, in blocks.
    at: [f64; 3],
    /// Its turn about its feet.
    yaw: f64,
}

/// What a ray met of a mob: how far, its colour and the face's normal in the world.
pub struct MobHit {
    pub t: f64,
    pub colour: [f32; 3],
    pub normal: [f64; 3],
    /// The cell its light is read from.
    pub cell: [i32; 3],
}

#[derive(Default)]
pub struct Mobs {
    list: Vec<Mob>,
    /// The mobs in each cell they reach into.
    cells: HashMap<[i32; 3], Vec<u16>>,
}

impl Mobs {
    pub fn add(&mut self, kind: Kind, at: [f64; 3], yaw: f64) {
        self.list.push(Mob { kind, at, yaw });
    }

    /// Mark the cells each mob reaches into, so the tracer looks for it there; a plant in its way is
    /// trodden down.
    pub fn place(&mut self, w: &mut World) {
        for (i, m) in self.list.iter().enumerate().filter(|(_, m)| !m.kind.roams()) {
            let (reach, tall) = reach(m.kind);
            for x in (m.at[0] - reach).floor() as i32..=(m.at[0] + reach).floor() as i32 {
                for z in (m.at[2] - reach).floor() as i32..=(m.at[2] + reach).floor() as i32 {
                    for y in m.at[1].floor() as i32..=(m.at[1] + tall).floor() as i32 {
                        let b = w.get(x, y, z);
                        // in the open the cell is marked; in water it stays water, and a cart's rail
                        // stays rail, so the track runs on under it: the tracer asks the mobs there as
                        // it passes through
                        if b == AIR || b == MOB || b == WATER || b == RAIL || b == RAIL_Z || block(b).shape == Shape::Cross {
                            if !matches!(b, WATER | RAIL | RAIL_Z) {
                                w.set(x, y, z, MOB);
                            }
                            self.cells.entry([x, y, z]).or_default().push(i as u16);
                        }
                    }
                }
            }
        }
    }

    /// The nearest mob a ray meets between t0 and t1 in a cell.
    pub fn hit(&self, cell: [i32; 3], o: [f64; 3], d: [f64; 3], t0: f64, t1: f64) -> Option<MobHit> {
        let mut best: Option<MobHit> = None;
        for &i in self.cells.get(&cell)? {
            let m = &self.list[i as usize];
            if let Some(h) = hit_one(m.kind, m.at, m.yaw, o, d)
                && h.t >= t0 - 1e-6
                && h.t <= t1 + 1e-6
                && best.as_ref().is_none_or(|b| h.t < b.t)
            {
                best = Some(h);
            }
        }
        best
    }

    /// The mobs that wander: each one's kind, feet and turn.
    pub fn roaming(&self) -> Vec<(Kind, [f64; 3], f64)> {
        self.list.iter().filter(|m| m.kind.roams()).map(|m| (m.kind, m.at, m.yaw)).collect()
    }
}

/// How an animal's legs are in its walk, as the block game's four- and two-legged animals walk: each leg
/// swings about its hip, ahead and back, by cos(`phase` × 0.6662 + its own) × 1.4 × `amount` radians.
/// `phase` grows as it walks; `amount` is 0 standing and eases up while walking. Diagonal legs of four
/// swing together, the two pairs opposed; a bird's two legs are opposed.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Walk {
    pub phase: f64,
    pub amount: f64,
}

impl Walk {
    pub const STILL: Walk = Walk { phase: 0.0, amount: 0.0 };
}

/// Its legs: each one's part in `model` and its phase in the walk; and how far they swing, of the block
/// game's stride.
fn legs(kind: Kind) -> (&'static [(usize, f64)], f64) {
    use std::f64::consts::PI;
    // hind left and fore right together, hind right and fore left half a stride on
    const FOUR: &[(usize, f64)] = &[(0, 0.0), (1, PI), (2, PI), (3, 0.0)];
    match kind {
        Kind::Sheep | Kind::Pig => (FOUR, 1.0),
        // a cow's legs are long and close: a leg and the one before it on its side swing towards each
        // other, and at a full stride their feet met. Kept short of that
        Kind::Cow => (FOUR, 0.44),
        Kind::Chicken => (&[(0, 0.0), (1, PI)], 1.0),
        _ => (&[], 1.0),
    }
}

/// How far leg part `pi` of a kind is turned about its hip at `walk`, in radians.
fn swing(kind: Kind, pi: usize, walk: Walk) -> f64 {
    let (legs, stride) = legs(kind);
    legs.iter().find(|l| l.0 == pi).map_or(0.0, |&(_, own)| (walk.phase * 0.6662 + own).cos() * 1.4 * walk.amount * stride)
}

/// Where a ray meets a mob of a kind standing at `at`, turned by `yaw`: the nearest of its boxes.
pub fn hit_one(kind: Kind, at: [f64; 3], yaw: f64, o: [f64; 3], d: [f64; 3]) -> Option<MobHit> {
    hit_walking(kind, at, yaw, Walk::STILL, o, d)
}

/// The same, its legs as they are at `walk`.
pub fn hit_walking(kind: Kind, at: [f64; 3], yaw: f64, walk: Walk, o: [f64; 3], d: [f64; 3]) -> Option<MobHit> {
    let (c, s) = (yaw.cos(), yaw.sin());
    // the ray in the mob's own frame, in sixteenths: turned back by its turn about its feet
    let back = |v: [f64; 3]| [v[0] * c - v[2] * s, v[1], v[0] * s + v[2] * c];
    let lo = back([o[0] - at[0], o[1] - at[1], o[2] - at[2]]).map(|v| v * 16.0);
    let ld = back(d).map(|v| v * 16.0);
    let mut best: Option<MobHit> = None;
    for (pi, p) in model(kind).iter().enumerate() {
        // a leg in its swing: the ray turned back about its hip (the top of the leg, across the body)
        let swing = swing(kind, pi, walk);
        let hip = [0.0, p.hi[1], (p.lo[2] + p.hi[2]) * 0.5];
        let (sc, ss) = (swing.cos(), swing.sin());
        // about the sideways axis: by -swing into the leg's frame, by +swing out of it
        let turn = |v: [f64; 3], k: f64| [v[0], v[1] * sc + v[2] * ss * k, -v[1] * ss * k + v[2] * sc];
        let (ro, rd) = if swing == 0.0 {
            (lo, ld)
        } else {
            let rel = turn([lo[0], lo[1] - hip[1], lo[2] - hip[2]], 1.0);
            ([rel[0], rel[1] + hip[1], rel[2] + hip[2]], turn(ld, 1.0))
        };
        let Some((t, axis, sign)) = slab(ro, rd, p.lo, p.hi) else { continue };
        if best.as_ref().is_some_and(|b| b.t <= t) {
            continue;
        }
        let local: [f64; 3] = std::array::from_fn(|k| ro[k] + rd[k] * t);
        let colour = paint(p, pi, axis, sign, local);
        let mut n = [0.0; 3];
        n[axis] = sign;
        // the leg's normal turned out of its swing, then out into the world
        let n = if swing == 0.0 { n } else { turn(n, -1.0) };
        let normal = [n[0] * c + n[2] * s, n[1], -n[0] * s + n[2] * c];
        let cell = [at[0].floor() as i32, (at[1] + 0.3).floor() as i32, at[2].floor() as i32];
        best = Some(MobHit { t, colour, normal, cell });
    }
    best
}

/// How far a mob of a kind reaches about its feet, and how tall it is, in blocks.
pub fn reach(kind: Kind) -> (f64, f64) {
    let parts = model(kind);
    let reach = parts.iter().flat_map(|p| [p.lo, p.hi]).map(|c| c[0].hypot(c[2])).fold(0.0, f64::max) / 16.0;
    let tall = parts.iter().map(|p| p.hi[1]).fold(0.0, f64::max) / 16.0;
    (reach, tall)
}

/// Where a ray meets a box from outside: how far, and the face's axis and side.
pub fn slab(o: [f64; 3], d: [f64; 3], lo: [f64; 3], hi: [f64; 3]) -> Option<(f64, usize, f64)> {
    let (mut t0, mut t1, mut axis, mut sign) = (f64::MIN, f64::MAX, 0, 1.0);
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i] < lo[i] || o[i] > hi[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[i] - o[i]) / d[i], (hi[i] - o[i]) / d[i]);
        let (near, far) = if a < b { (a, b) } else { (b, a) };
        if near > t0 {
            t0 = near;
            axis = i;
            sign = if d[i] > 0.0 { -1.0 } else { 1.0 };
        }
        t1 = t1.min(far);
    }
    if t0 > t1 || t1 < 0.0 || t0 < 0.0 {
        return None;
    }
    Some((t0, axis, sign))
}

/// A box's colour at a point of its face, in sixteenths.
fn paint(p: &Part, pi: usize, axis: usize, sign: f64, at: [f64; 3]) -> [f32; 3] {
    // the point on its face's plane, a hair inside the box: met by a ray it lies on the box's edge, give
    // or take, and the patches and stripes are chosen by where it lies through the box too. Where that
    // edge is on a patch's border (a cow's legs, at whole multiples of four), the point fell either side
    // of it from pixel to pixel, more so on a leg turned in its stride, and the leg was speckled
    let mut at = at;
    at[axis] = if sign > 0.0 { p.hi[axis] - 1e-3 } else { p.lo[axis] + 1e-3 };
    // the texel: across and down the face
    let (u, v) = match axis {
        0 => (at[2], at[1]),
        1 => (at[0], at[2]),
        _ => (at[0], at[1]),
    };
    let (tu, tv) = (u.floor() as i32, v.floor() as i32);
    let noise = hash(pi as i32 * 7 + axis as i32 * 2 + (sign > 0.0) as i32, tu, tv);
    let fur = |a: u32, b: u32| rgb(if noise.is_multiple_of(4) { b } else { a });
    match p.coat {
        Coat::Fur(a, b) => fur(a, b),
        Coat::Patches(a, b) => {
            // patches a few texels across, the same wherever on the mob they fall
            let (px, py, pz) = ((at[0] / 4.0).floor() as i32, (at[1] / 4.0).floor() as i32, (at[2] / 4.0).floor() as i32);
            if hash(px, py, pz).is_multiple_of(3) { rgb(b) } else { fur(a, a) }
        }
        Coat::Stripes(a, b) => {
            if (at[2].floor() as i32).rem_euclid(3) == 0 {
                rgb(b)
            } else {
                fur(a, a)
            }
        }
        Coat::Face(a, b, rows, palette) => {
            // the face's art on the front, read from the viewer's left (the model's +x) and its top
            if axis == 2 && sign > 0.0 {
                let col = (p.hi[0] - at[0]).floor() as usize;
                let row = (p.hi[1] - at[1]).floor() as usize;
                if let Some(ch) = rows.get(row).and_then(|r| r.chars().nth(col))
                    && let Some(&(_, c)) = palette.iter().find(|q| q.0 == ch)
                {
                    return rgb(c);
                }
            }
            fur(a, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether a ray along +x at height `y` and depth `z` (sixteenths, the cow's own frame) meets it.
    fn meets(walk: Walk, y: f64, z: f64) -> bool {
        hit_walking(Kind::Cow, [0.0; 3], 0.0, walk, [-2.0, y / 16.0, z / 16.0], [1.0, 0.0, 0.0]).is_some()
    }

    #[test]
    fn a_face_is_painted_the_same_wherever_a_ray_meets_it() {
        // a cow's hind leg's back face lies on a patch border (z = -8): met a hair either side of it, as
        // a ray turned in the leg's stride meets it, it is the same colour
        let leg = &model(Kind::Cow)[0];
        for (y, x) in [(3.5, -5.5), (7.5, -3.5), (10.5, -2.5)] {
            let a = paint(leg, 0, 2, -1.0, [x, y, -8.0 + 1e-9]);
            let b = paint(leg, 0, 2, -1.0, [x, y, -8.0 - 1e-9]);
            assert_eq!(a, b, "at {x}, {y}");
        }
    }

    #[test]
    fn a_cow_never_steps_on_its_own_feet() {
        // a hind leg and the fore leg on its side, at every point of a walking stride: the hind foot's
        // front stays behind the fore foot's back (the feet go furthest; the hips stay put)
        let foot_z = |kind: Kind, pi: usize, walk: Walk, edge: f64| {
            let p = &model(kind)[pi];
            let (hy, hz) = (p.hi[1], (p.lo[2] + p.hi[2]) * 0.5);
            let (y, z) = (p.lo[1] - hy, edge - hz);
            let a = swing(kind, pi, walk);
            hz + y * a.sin() + z * a.cos()
        };
        {
            let kind = Kind::Cow;
            for k in 0..200 {
                let walk = Walk { phase: k as f64 * 0.05, amount: 0.5 };
                for (hind, fore) in [(0, 2), (1, 3)] {
                    let (h, f) = (&model(kind)[hind], &model(kind)[fore]);
                    let gap = foot_z(kind, fore, walk, f.lo[2]) - foot_z(kind, hind, walk, h.hi[2]);
                    assert!(gap > 0.25, "{kind:?} legs {hind} and {fore} meet at phase {}: {gap}", walk.phase);
                }
            }
        }
    }

    #[test]
    fn a_walking_animal_swings_its_legs_about_its_hips() {
        // through the hind legs' feet: standing, they are there
        assert!(meets(Walk::STILL, 0.5, -6.0));
        // at the top of its swing a foot is half a block ahead or behind: the ray goes under the body
        let stride = Walk { phase: 0.0, amount: 0.5 };
        assert!(!meets(stride, 0.5, -6.0), "the hind feet have swung away");
        // the hips stay put: through the top of the legs, under the body, it still meets them
        assert!(meets(stride, 11.5, -6.0));
        // diagonal legs together, the pairs opposed, as the block game's four-legged animals walk
        let leg = |pi: usize| swing(Kind::Cow, pi, stride);
        assert_eq!(leg(0), leg(3));
        assert_eq!(leg(1), leg(2));
        assert_eq!(leg(0), -leg(1));
        // standing, as before there were legs that walk
        assert_eq!(
            hit_one(Kind::Cow, [0.0; 3], 0.4, [-2.0, 0.5, 0.1], [1.0, 0.0, 0.0]).map(|h| h.t),
            hit_walking(Kind::Cow, [0.0; 3], 0.4, Walk::STILL, [-2.0, 0.5, 0.1], [1.0, 0.0, 0.0]).map(|h| h.t)
        );
    }
}
