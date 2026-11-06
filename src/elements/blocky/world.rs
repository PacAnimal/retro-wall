//! The world: a grid of blocks about the viewer, laid out from a seed as a block game's terrain
//! generator would: a wide valley of grassland, woods of oak and birch about it, a river across it and
//! mountains beyond, their cliffs of bare stone with ore in them and snow on their peaks, spruce on the
//! high ground. A farm is built in the valley (`farm`), and the viewer looks down on it from on high. Then
//! the light: the sky's, let down each column and spread sideways under what overhangs, and the
//! torches', spread from each, both a level less a block.

use crate::rng::Rng;

/// The world reaches this far either way across from the viewer, in blocks.
pub const R: i32 = 208;
/// The grid's side across, and its lowest and highest rows.
pub const S: i32 = 2 * R;
pub const Y0: i32 = 36;
pub const Y1: i32 = 164;
const H: i32 = Y1 - Y0;
/// The water's level: water fills what is below it.
pub const SEA: i32 = 63;
/// The valley floor: the row over its top blocks.
pub const GROUND: i32 = 64;

// the blocks
pub const AIR: u8 = 0;
pub const GRASS: u8 = 1;
pub const DIRT: u8 = 2;
pub const STONE: u8 = 3;
pub const SAND: u8 = 4;
pub const GRAVEL: u8 = 5;
pub const OAK_LOG: u8 = 6;
pub const OAK_LEAVES: u8 = 7;
pub const BIRCH_LOG: u8 = 8;
pub const BIRCH_LEAVES: u8 = 9;
pub const SPRUCE_LOG: u8 = 10;
pub const SPRUCE_LEAVES: u8 = 11;
pub const WATER: u8 = 12;
pub const SNOW: u8 = 13;
pub const SNOWY_GRASS: u8 = 14;
pub const COAL: u8 = 15;
pub const IRON: u8 = 16;
pub const TALL_GRASS: u8 = 17;
pub const POPPY: u8 = 18;
pub const DANDELION: u8 = 19;
pub const CORNFLOWER: u8 = 20;
pub const CANE: u8 = 21;
pub const PUMPKIN: u8 = 22;
pub const CLAY: u8 = 23;
pub const FERN: u8 = 24;
pub const COBBLE: u8 = 25;
pub const SNOWY_STONE: u8 = 26;
pub const PLANKS: u8 = 27;
pub const SPRUCE_PLANKS: u8 = 28;
pub const GLASS: u8 = 29;
pub const DOOR_LOW: u8 = 30;
pub const DOOR_HIGH: u8 = 31;
pub const FARMLAND: u8 = 32;
pub const PATH: u8 = 33;
pub const WHEAT: u8 = 34;
pub const WHEAT_YOUNG: u8 = 35;
pub const CARROTS: u8 = 36;
pub const POTATOES: u8 = 37;
pub const BEETROOTS: u8 = 38;
pub const FENCE: u8 = 39;
pub const TORCH: u8 = 40;
pub const HAY: u8 = 41;
pub const CRAFTING: u8 = 42;
pub const OAK_SLAB: u8 = 43;
/// Roof stairs, each rising towards +x, -x, +z or -z.
pub const STAIRS_PX: u8 = 44;
pub const STAIRS_NX: u8 = 45;
pub const STAIRS_PZ: u8 = 46;
pub const STAIRS_NZ: u8 = 47;
pub const ROOF_SLAB: u8 = 48;
pub const LOG_X: u8 = 49;
pub const LOG_Z: u8 = 50;
pub const BARREL: u8 = 51;
/// An open cell an animal or a villager stands in: the tracer asks the mobs there.
pub const MOB: u8 = 52;
pub const RAIL: u8 = 53;
pub const LANTERN: u8 = 54;
pub const LILY: u8 = 55;
pub const CAMPFIRE: u8 = 56;
pub const COBBLE_SLAB: u8 = 57;
/// Rails: `RAIL` runs along x, `RAIL_Z` along z.
pub const RAIL_Z: u8 = 58;
/// A chimney's top: cobblestone, open at the top.
pub const CHIMNEY_TOP: u8 = 59;

/// How a block is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Air,
    /// A whole cube, nothing seen through it.
    Cube,
    /// A cube with holes in its faces (leaves, glass).
    Cutout,
    /// Two pictures crossed on its diagonals (a plant).
    Cross,
    /// Four pictures in a square, a crop's rows.
    Crop,
    Water,
    /// Boxes within the cell, in sixteenths: slabs, stairs, fences, a torch, farmland.
    Boxes,
    Mob,
}

/// A block's shape and its faces' pictures: top, sides, bottom; whether its top is turned at random
/// from block to block, so a field of it shows no repeat; the light it gives, 0 to 15.
#[derive(Clone, Copy)]
pub struct Block {
    pub shape: Shape,
    pub faces: [usize; 3],
    pub turned: bool,
    pub glow: u8,
    /// A log lying along x (0) or z (2): its ends ringed, its bark's grain along it. None standing.
    pub lying: Option<usize>,
}

pub fn block(b: u8) -> Block {
    use super::textures as t;
    use Shape::*;
    let (shape, faces, turned) = match b {
        GRASS => (Cube, [t::GRASS_TOP, t::GRASS_SIDE, t::DIRT], true),
        DIRT => (Cube, [t::DIRT; 3], true),
        STONE => (Cube, [t::STONE; 3], false),
        SAND => (Cube, [t::SAND; 3], true),
        GRAVEL => (Cube, [t::GRAVEL; 3], true),
        OAK_LOG => (Cube, [t::OAK_TOP, t::OAK_LOG, t::OAK_TOP], false),
        OAK_LEAVES => (Cutout, [t::OAK_LEAVES; 3], true),
        BIRCH_LOG => (Cube, [t::BIRCH_TOP, t::BIRCH_LOG, t::BIRCH_TOP], false),
        BIRCH_LEAVES => (Cutout, [t::BIRCH_LEAVES; 3], true),
        SPRUCE_LOG => (Cube, [t::SPRUCE_TOP, t::SPRUCE_LOG, t::SPRUCE_TOP], false),
        SPRUCE_LEAVES => (Cutout, [t::SPRUCE_LEAVES; 3], true),
        WATER => (Water, [t::WATER; 3], false),
        SNOW => (Cube, [t::SNOW; 3], true),
        SNOWY_GRASS => (Cube, [t::SNOW, t::SNOW_SIDE, t::DIRT], true),
        COAL => (Cube, [t::COAL; 3], false),
        IRON => (Cube, [t::IRON; 3], false),
        TALL_GRASS => (Cross, [t::TALL_GRASS; 3], false),
        POPPY => (Cross, [t::POPPY; 3], false),
        DANDELION => (Cross, [t::DANDELION; 3], false),
        CORNFLOWER => (Cross, [t::CORNFLOWER; 3], false),
        CANE => (Cross, [t::CANE; 3], false),
        PUMPKIN => (Cube, [t::PUMPKIN_TOP, t::PUMPKIN_SIDE, t::PUMPKIN_SIDE], false),
        CLAY => (Cube, [t::CLAY; 3], true),
        FERN => (Cross, [t::FERN; 3], false),
        COBBLE => (Cube, [t::COBBLE; 3], false),
        SNOWY_STONE => (Cube, [t::SNOW, t::SNOW_STONE, t::STONE], true),
        PLANKS => (Cube, [t::PLANKS; 3], false),
        SPRUCE_PLANKS => (Cube, [t::SPRUCE_PLANKS; 3], false),
        GLASS => (Cutout, [t::GLASS; 3], false),
        DOOR_LOW => (Cube, [t::PLANKS, t::DOOR_LOW, t::PLANKS], false),
        DOOR_HIGH => (Cube, [t::PLANKS, t::DOOR_HIGH, t::PLANKS], false),
        FARMLAND => (Boxes, [t::FARMLAND, t::DIRT, t::DIRT], false),
        PATH => (Boxes, [t::PATH, t::PATH_SIDE, t::DIRT], true),
        WHEAT => (Crop, [t::WHEAT; 3], false),
        WHEAT_YOUNG => (Crop, [t::WHEAT_YOUNG; 3], false),
        CARROTS => (Crop, [t::CARROTS; 3], false),
        POTATOES => (Crop, [t::POTATOES; 3], false),
        BEETROOTS => (Crop, [t::BEETROOTS; 3], false),
        FENCE => (Boxes, [t::PLANKS; 3], false),
        TORCH => (Boxes, [t::TORCH_TOP, t::TORCH, t::TORCH], false),
        HAY => (Cube, [t::HAY_TOP, t::HAY_SIDE, t::HAY_TOP], false),
        CRAFTING => (Cube, [t::CRAFTING_TOP, t::CRAFTING_SIDE, t::PLANKS], false),
        OAK_SLAB => (Boxes, [t::PLANKS; 3], false),
        STAIRS_PX | STAIRS_NX | STAIRS_PZ | STAIRS_NZ | ROOF_SLAB => (Boxes, [t::SPRUCE_PLANKS; 3], false),
        LOG_X | LOG_Z => (Cube, [t::OAK_TOP, t::OAK_LOG, t::OAK_LYING], false),
        BARREL => (Cube, [t::BARREL_TOP, t::BARREL_SIDE, t::BARREL_TOP], false),
        MOB => (Mob, [0; 3], false),
        RAIL => (Boxes, [t::RAIL_X; 3], false),
        RAIL_Z => (Boxes, [t::RAIL; 3], false),
        LANTERN => (Boxes, [t::LANTERN_TOP, t::LANTERN, t::LANTERN_TOP], false),
        LILY => (Boxes, [t::LILY, t::LILY, t::LILY], true),
        CAMPFIRE => (Cross, [t::CAMPFIRE; 3], false),
        COBBLE_SLAB => (Boxes, [t::COBBLE; 3], false),
        CHIMNEY_TOP => (Cube, [t::CHIMNEY_TOP, t::COBBLE, t::COBBLE], false),
        _ => (Air, [0; 3], false),
    };
    let glow = match b {
        TORCH => 14,
        LANTERN | CAMPFIRE => 15,
        _ => 0,
    };
    let lying = match b {
        LOG_X => Some(0),
        LOG_Z => Some(2),
        _ => None,
    };
    Block { shape, faces, turned, glow, lying }
}

/// A box in a cell, in sixteenths: x0, y0, z0, x1, y1, z1.
pub type Box16 = [u8; 6];

/// Whether a fence reaches out to a neighbour: another fence, or a whole block.
fn joins(b: u8) -> bool {
    b == FENCE || block(b).shape == Shape::Cube
}

/// The boxes a block of `Shape::Boxes` is made of, at (x, y, z): a fence's rails reach the neighbours
/// it joins.
pub fn boxes(w: &World, b: u8, x: i32, y: i32, z: i32) -> ([Box16; 5], usize) {
    let mut out = [[0u8; 6]; 5];
    let mut n = 0;
    let mut push = |bx: Box16| {
        out[n] = bx;
        n += 1;
    };
    match b {
        FARMLAND | PATH => push([0, 0, 0, 16, 15, 16]),
        OAK_SLAB | ROOF_SLAB | COBBLE_SLAB => push([0, 0, 0, 16, 8, 16]),
        STAIRS_PX => {
            push([0, 0, 0, 16, 8, 16]);
            push([8, 8, 0, 16, 16, 16]);
        }
        STAIRS_NX => {
            push([0, 0, 0, 16, 8, 16]);
            push([0, 8, 0, 8, 16, 16]);
        }
        STAIRS_PZ => {
            push([0, 0, 0, 16, 8, 16]);
            push([0, 8, 8, 16, 16, 16]);
        }
        STAIRS_NZ => {
            push([0, 0, 0, 16, 8, 16]);
            push([0, 8, 0, 16, 16, 8]);
        }
        TORCH => push([7, 0, 7, 9, 10, 9]),
        LANTERN => {
            // the cage, its cap and the handle's loop over that
            push([5, 0, 5, 11, 7, 11]);
            push([6, 7, 6, 10, 9, 10]);
            push([6, 9, 7, 10, 11, 9]);
        }
        RAIL | RAIL_Z | LILY => push([0, 0, 0, 16, 1, 16]),
        FENCE => {
            push([6, 0, 6, 10, 16, 10]);
            // two rails towards each neighbour it joins, as one box each side from the post
            let (px, nx, pz, nz) = (joins(w.get(x + 1, y, z)), joins(w.get(x - 1, y, z)), joins(w.get(x, y, z + 1)), joins(w.get(x, y, z - 1)));
            if px || nx {
                push([if nx { 0 } else { 6 }, 6, 7, if px { 16 } else { 10 }, 15, 9]);
            }
            if pz || nz {
                push([7, 6, if nz { 0 } else { 6 }, 9, 15, if pz { 16 } else { 10 }]);
            }
        }
        _ => push([0, 0, 0, 16, 16, 16]),
    }
    (out, n)
}

/// Smooth noise over the plane: a lattice of random heights, eased between.
pub struct Noise {
    seed: u64,
}

impl Noise {
    pub fn new(seed: i64, salt: u64) -> Noise {
        Noise { seed: (seed as u64) ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15) }
    }
    fn lattice(&self, x: i64, z: i64) -> f64 {
        let mut h = self.seed ^ (x as u64).wrapping_mul(0xd1b5_4a32_d192_ed03) ^ (z as u64).wrapping_mul(0xabc9_8388_fb8f_ac03);
        h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 31;
        (h >> 11) as f64 / (1u64 << 53) as f64
    }
    /// 0 to 1.
    pub fn at(&self, x: f64, z: f64) -> f64 {
        let (x0, z0) = (x.floor(), z.floor());
        let (tx, tz) = (x - x0, z - z0);
        let (tx, tz) = (tx * tx * tx * (tx * (tx * 6.0 - 15.0) + 10.0), tz * tz * tz * (tz * (tz * 6.0 - 15.0) + 10.0));
        let (xi, zi) = (x0 as i64, z0 as i64);
        let a = self.lattice(xi, zi) + (self.lattice(xi + 1, zi) - self.lattice(xi, zi)) * tx;
        let b = self.lattice(xi, zi + 1) + (self.lattice(xi + 1, zi + 1) - self.lattice(xi, zi + 1)) * tx;
        a + (b - a) * tz
    }
    /// Octaves of it, -1 to 1 or so.
    fn fbm(&self, x: f64, z: f64, octaves: u32) -> f64 {
        let (mut sum, mut amp, mut f, mut norm) = (0.0, 1.0, 1.0, 0.0);
        for k in 0..octaves {
            // each octave turned, so the lattice's rows do not line up
            let (c, s) = ((k as f64 * 1.7).cos(), (k as f64 * 1.7).sin());
            sum += amp * (self.at((x * c - z * s) * f + k as f64 * 31.7, (x * s + z * c) * f) * 2.0 - 1.0);
            norm += amp;
            amp *= 0.5;
            f *= 2.03;
        }
        sum / norm
    }
    /// Ridges: sharp crests where the noise crosses its middle, 0 to 1.
    fn ridged(&self, x: f64, z: f64, octaves: u32) -> f64 {
        let (mut sum, mut amp, mut f, mut norm) = (0.0, 1.0, 1.0, 0.0);
        for k in 0..octaves {
            let n = 1.0 - (self.at(x * f + k as f64 * 17.3, z * f) * 2.0 - 1.0).abs();
            sum += amp * n * n;
            norm += amp;
            amp *= 0.5;
            f *= 2.1;
        }
        sum / norm
    }
}

pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where the viewer looks across the ground: a unit vector, x then z.
pub type Ahead = (f64, f64);

/// The blocks, the light in each (the sky's in the low four bits, the torches' in the high), and the
/// mobs.
pub struct World {
    pub blocks: Vec<u8>,
    pub light: Vec<u8>,
    /// Each column's highest block that is not air, as a row above Y0 (0: none).
    pub tops: Vec<u8>,
    /// The highest of `tops` in each square of COARSE columns, plus one.
    pub coarse: Vec<u8>,
    pub mobs: super::mobs::Mobs,
    /// Where smoke rises from, and whether it is a chimney (which is lit only now and then), and where
    /// fireflies dance at night (a middle and a reach), in the world.
    pub smoke: Vec<([f64; 3], bool)>,
    pub glades: Vec<([f64; 3], f64)>,
    /// The pens' insides and the ponds, x0, z0, x1, z1 inclusive: where the animals wander.
    pub pens: Vec<[i32; 4]>,
    pub ponds: Vec<[i32; 4]>,
    /// The viewer's eye.
    pub eye: [f64; 3],
    /// Whether `light` is worked out (`light_up`).
    lit: bool,
}

/// The side of the squares of columns the tracer skips whole.
pub const COARSE: i32 = 8;
pub const CS: i32 = S / COARSE;

#[inline]
pub fn index(x: i32, y: i32, z: i32) -> usize {
    (((x + R) * S + (z + R)) * H + (y - Y0)) as usize
}

/// How far the farm's middle is ahead of the viewer, across the ground.
pub const FARM_AHEAD: f64 = 38.0;
/// The rocky knoll beyond the river that the mine is cut into: its middle, across the ground.
pub fn knoll(ahead: Ahead) -> (f64, f64) {
    let (along, across) = (FARM_AHEAD + 40.0, 58.0);
    (ahead.0 * along + ahead.1 * across, ahead.1 * along - ahead.0 * across)
}

/// Where the small cart tunnel is sought, in the mountains on the left: across the ground.
pub fn cart_tunnel_at(ahead: Ahead) -> (f64, f64) {
    let (along, across) = (FARM_AHEAD + 40.0, 70.0);
    (ahead.0 * along + ahead.1 * across, ahead.1 * along - ahead.0 * across)
}

/// Where the mineshaft is sought, in the foot of the grey mountain on the left, close behind the farm:
/// across the ground.
pub fn shaft_at(ahead: Ahead) -> (f64, f64) {
    let (along, across) = (FARM_AHEAD + 14.0, 64.0);
    (ahead.0 * along + ahead.1 * across, ahead.1 * along - ahead.0 * across)
}

/// Where the cliff dwelling is cut into the mountains on the left, left of the mine: across the ground.
pub fn cliff_at(ahead: Ahead) -> (f64, f64) {
    let (along, across) = (FARM_AHEAD + 36.0, 76.0);
    (ahead.0 * along + ahead.1 * across, ahead.1 * along - ahead.0 * across)
}

/// How much (x, z) lies in the mountains on the left of the view, close behind the farm: 0 to 1.
fn mountains_left(ahead: Ahead, x: f64, z: f64) -> f64 {
    let along = x * ahead.0 + z * ahead.1;
    let across = x * ahead.1 - z * ahead.0;
    smoothstep(44.0, 72.0, across) * smoothstep(5.0, 35.0, along)
}

/// How high the viewer's eye is over the valley floor, in blocks: at the edge of something tall, out
/// of sight below.
const EYE: f64 = 9.6;

impl World {
    #[inline]
    pub fn inside(x: i32, y: i32, z: i32) -> bool {
        (-R..R).contains(&x) && (-R..R).contains(&z) && (Y0..Y1).contains(&y)
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if !World::inside(x, y, z) {
            return if y < Y0 { STONE } else { AIR };
        }
        self.blocks[index(x, y, z)]
    }
    /// The sky's light and the torches', 0 to 15 each.
    #[inline]
    pub fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !World::inside(x, y, z) {
            return (if y < Y0 { 0 } else { 15 }, 0);
        }
        let l = self.light[index(x, y, z)];
        (l & 15, l >> 4)
    }
    pub fn set(&mut self, x: i32, y: i32, z: i32, b: u8) {
        if World::inside(x, y, z) {
            self.blocks[index(x, y, z)] = b;
        }
    }

    #[cfg(test)]
    pub fn new(seed: i64, ahead: Ahead) -> World {
        let mut w = World::unlit(seed, ahead);
        w.light_up();
        w
    }

    /// The world without its light, for what does not trace it: the animals, the smoke, a picture read
    /// from the cache. `light_up` lights it.
    pub fn unlit(seed: i64, ahead: Ahead) -> World {
        let n = (S * S * H) as usize;
        let mut w = World {
            blocks: vec![AIR; n],
            light: vec![0; n],
            tops: vec![0; (S * S) as usize],
            coarse: vec![0; (CS * CS) as usize],
            mobs: Default::default(),
            smoke: vec![],
            glades: vec![],
            pens: vec![],
            ponds: vec![],
            eye: [0.5, 80.0, 0.5],
            lit: false,
        };
        let farm = ((ahead.0 * FARM_AHEAD).round() as i32, (ahead.1 * FARM_AHEAD).round() as i32);
        let heights = Terrain::new(seed, ahead, farm);
        let surface = heights.columns();
        w.fill(seed, &heights, &surface);
        w.plant(seed, ahead, farm, &surface);
        super::farm::build(&mut w, seed, farm, ahead);
        w.eye = [0.5, GROUND as f64 + EYE, 0.5];
        w.measure();
        w
    }

    /// The highest row over a whole block at (x, z): where something stands.
    pub fn ground(&self, x: i32, z: i32) -> i32 {
        (Y0..Y1).rev().find(|&y| block(self.get(x, y, z)).shape == Shape::Cube).map_or(Y0, |y| y + 1)
    }

    /// Each column's top and each coarse square's.
    fn measure(&mut self) {
        for x in -R..R {
            for z in -R..R {
                let col = index(x, Y0, z);
                let top = (0..H as usize).rev().find(|&y| self.blocks[col + y] != AIR).map_or(0, |y| y + 1);
                self.tops[((x + R) * S + (z + R)) as usize] = top as u8;
                let c = (((x + R) / COARSE) * CS + (z + R) / COARSE) as usize;
                self.coarse[c] = self.coarse[c].max(top as u8);
            }
        }
    }

    /// The ground and the water: stone, then dirt under grass, or sand along the water, or bare stone
    /// on steep and high ground and snow on the peaks; ore in the stone; the river.
    fn fill(&mut self, seed: i64, t: &Terrain, cols: &Columns) {
        let mut rng = Rng::new(seed ^ 0x5eed_0f0a);
        let patch = Noise::new(seed, 41);
        for x in -R..R {
            for z in -R..R {
                let (h, steep) = cols.at(x, z);
                let wet = t.wet(x as f64, z as f64);
                let (fx, fz) = (x as f64, z as f64);
                let snow_line = 128.0 + 6.0 * patch.fbm(fx / 23.0, fz / 23.0, 2);
                let stone_line = 104.0 + 8.0 * patch.fbm(fx / 31.0 + 9.0, fz / 31.0, 2);
                let top_y = h - 1;
                let under_water = top_y < SEA - 1;
                let beach = !under_water && top_y <= SEA + 1 && wet < 3.0;
                let (cap, layer, depth) = if under_water {
                    let p = patch.at(fx / 9.0, fz / 9.0);
                    let deep = SEA - 1 - top_y;
                    if p > 0.7 {
                        (CLAY, CLAY, 2)
                    } else if deep > 2 && p < 0.45 {
                        (GRAVEL, GRAVEL, 2)
                    } else {
                        (SAND, SAND, 3)
                    }
                } else if beach {
                    (SAND, SAND, 3)
                } else if top_y as f64 > snow_line && steep < 3 {
                    (SNOW, STONE, 1)
                } else if top_y as f64 > snow_line - 4.0 {
                    (SNOWY_STONE, STONE, 0)
                } else if steep >= 3 || top_y as f64 > stone_line {
                    if steep < 2 && (top_y as f64) < stone_line + 12.0 && patch.at(fx / 7.0, fz / 7.0) > 0.55 { (GRAVEL, STONE, 1) } else { (STONE, STONE, 0) }
                } else if top_y > 96 && top_y as f64 > snow_line - 14.0 && patch.at(fx / 5.0, fz / 5.0) > 0.6 {
                    (SNOWY_GRASS, DIRT, 3)
                } else {
                    (GRASS, DIRT, 3 + (patch.at(fx / 4.0, fz / 4.0) * 2.0) as i32)
                };
                for y in Y0..h.min(Y1) {
                    let b = if y == top_y {
                        cap
                    } else if y >= top_y - depth {
                        layer
                    } else {
                        let r = rng.f64();
                        if r < 0.012 {
                            COAL
                        } else if r < 0.017 && y < 90 {
                            IRON
                        } else {
                            STONE
                        }
                    };
                    self.blocks[index(x, y, z)] = b;
                }
                for y in h.max(Y0)..SEA {
                    self.blocks[index(x, y, z)] = WATER;
                }
            }
        }
        // ore shows in clusters, not single blocks: grow each ore block a little into the stone
        let mut seeds = vec![];
        for x in -R..R {
            for z in -R..R {
                let top = cols.at(x, z).0;
                for y in Y0.max(top - 12)..top {
                    let b = self.blocks[index(x, y, z)];
                    if b == COAL || b == IRON {
                        seeds.push((x, y, z, b));
                    }
                }
            }
        }
        for (x, y, z, b) in seeds {
            for _ in 0..rng.int(1, 4) {
                let (dx, dy, dz) = (rng.int(-1, 1) as i32, rng.int(-1, 1) as i32, rng.int(-1, 1) as i32);
                if World::inside(x + dx, y + dy, z + dz) && self.blocks[index(x + dx, y + dy, z + dz)] == STONE {
                    self.blocks[index(x + dx, y + dy, z + dz)] = b;
                }
            }
        }
    }

    /// Trees, plants and the odd pumpkin, on the ground that takes them, outside the farm.
    fn plant(&mut self, seed: i64, ahead: Ahead, farm: (i32, i32), cols: &Columns) {
        let mut rng = Rng::new(seed ^ 0x7ee5);
        let wood = Noise::new(seed, 77);
        let kind = Noise::new(seed, 78);
        let flowers = Noise::new(seed, 79);
        let in_farm = |x: i32, z: i32, pad: i32| (x - farm.0).abs() <= super::farm::HALF + pad && (z - farm.1).abs() <= super::farm::HALF + pad;
        // trees: one at most in each cell of a grid, where the woods are thick enough
        const CELL: i32 = 5;
        for cx in (-R / CELL)..(R / CELL) {
            for cz in (-R / CELL)..(R / CELL) {
                let (x, z) = (cx * CELL + rng.int(0, CELL as i64 - 1) as i32, cz * CELL + rng.int(0, CELL as i64 - 1) as i32);
                let (fx, fz) = (x as f64, z as f64);
                let thick = wood.at(fx / 48.0, fz / 48.0);
                if rng.f64() > (thick - 0.36) * 2.2 || in_farm(x, z, 4) {
                    continue;
                }
                // the view down onto the farm is kept open
                let along = fx * ahead.0 + fz * ahead.1;
                let across = fx * ahead.1 - fz * ahead.0;
                let k = knoll(ahead);
                // the mountains on the left are bare but for a spruce here and there
                let bare = mountains_left(ahead, fx, fz) > 0.25 && rng.f64() < 0.85;
                if bare || fx.hypot(fz) < 6.0 || (along > 0.0 && along < FARM_AHEAD && across.abs() < 6.0 + along * 0.8) || (fx - k.0).hypot(fz - k.1) < 30.0 {
                    continue;
                }
                let (h, steep) = cols.at(x, z);
                let ground = self.get(x, h - 1, z);
                if steep > 1 || !(ground == GRASS || ground == DIRT || ground == SNOWY_GRASS) {
                    continue;
                }
                if h > 92 || ground == SNOWY_GRASS {
                    self.spruce(&mut rng, x, h, z);
                } else if kind.at(fx / 40.0, fz / 40.0) > 0.62 {
                    self.broadleaf(&mut rng, x, h, z, BIRCH_LOG, BIRCH_LEAVES, (5, 7));
                } else {
                    self.broadleaf(&mut rng, x, h, z, OAK_LOG, OAK_LEAVES, (4, 6));
                }
            }
        }
        // the ground's cover
        for x in -R..R {
            for z in -R..R {
                let (h, _) = cols.at(x, z);
                if !World::inside(x, h, z) || self.get(x, h, z) != AIR || in_farm(x, z, 0) {
                    continue;
                }
                let ground = self.get(x, h - 1, z);
                let (fx, fz) = (x as f64, z as f64);
                let r = rng.f64();
                if ground == GRASS {
                    let field = flowers.at(fx / 14.0, fz / 14.0);
                    let woods = wood.at(fx / 48.0, fz / 48.0);
                    if field > 0.72 && r < 0.3 {
                        let pick = flowers.at(fx / 6.0 + 50.0, fz / 6.0);
                        self.set(
                            x,
                            h,
                            z,
                            if pick < 0.4 {
                                POPPY
                            } else if pick < 0.75 {
                                DANDELION
                            } else {
                                CORNFLOWER
                            },
                        );
                    } else if r < 0.2 + 0.25 * woods {
                        self.set(x, h, z, if woods > 0.6 && rng.f64() < 0.35 { FERN } else { TALL_GRASS });
                    } else if r > 0.9993 {
                        self.set(x, h, z, PUMPKIN);
                    }
                } else if (ground == SAND || ground == GRASS) && h == SEA && r < 0.35 && self.beside_water(x, h - 1, z) {
                    for y in h..h + rng.int(1, 3) as i32 {
                        self.set(x, y, z, CANE);
                    }
                }
            }
        }
    }

    fn beside_water(&self, x: i32, y: i32, z: i32) -> bool {
        [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dz)| self.get(x + dx, y, z + dz) == WATER)
    }

    fn leaf(&mut self, x: i32, y: i32, z: i32, b: u8) {
        if World::inside(x, y, z) && matches!(self.get(x, y, z), AIR | TALL_GRASS | FERN | POPPY | DANDELION | CORNFLOWER) {
            self.set(x, y, z, b);
        }
    }

    /// An oak or a birch: a trunk, two wide layers of leaves about its top and two narrow ones over
    /// them, their corners left out at random.
    pub fn broadleaf(&mut self, rng: &mut Rng, x: i32, h: i32, z: i32, log: u8, leaves: u8, tall: (i64, i64)) {
        let t = rng.int(tall.0, tall.1) as i32;
        for dy in -3..=0 {
            let y = h + t + dy;
            let r: i32 = if dy >= -1 { 1 } else { 2 };
            for dx in -r..=r {
                for dz in -r..=r {
                    let corner = dx.abs() == r && dz.abs() == r;
                    if corner && (dy == 0 || rng.f64() < 0.5) {
                        continue;
                    }
                    self.leaf(x + dx, y, z + dz, leaves);
                }
            }
        }
        for y in h..h + t {
            self.set(x, y, z, log);
        }
        self.set(x, h - 1, z, DIRT);
    }

    /// A spruce: a tall trunk in a cone of leaves, rings wide and narrow by turns, a tip on top.
    fn spruce(&mut self, rng: &mut Rng, x: i32, h: i32, z: i32) {
        let t = rng.int(6, 10) as i32;
        let bare = rng.int(1, 2) as i32;
        let mut r: i32 = 0;
        let mut widest = 1;
        for y in (h + bare..=h + t + 1).rev() {
            for dx in -r..=r {
                for dz in -r..=r {
                    if r > 0 && dx.abs() == r && dz.abs() == r {
                        continue;
                    }
                    self.leaf(x + dx, y, z + dz, SPRUCE_LEAVES);
                }
            }
            r = if r >= widest {
                widest = (widest + 1).min(3);
                (r - 1).max(0).max(widest - 2)
            } else {
                r + 1
            };
        }
        for y in h..h + t {
            self.set(x, y, z, SPRUCE_LOG);
        }
        self.set(x, h - 1, z, DIRT);
    }

    /// The light: the sky's 15 down each column until something stops it (a level less through each
    /// leaf or water block), the torches' from each; both spread sideways and down a level less a
    /// block.
    pub fn light_up(&mut self) {
        if std::mem::replace(&mut self.lit, true) {
            return;
        }
        let h = H as usize;
        let mut sky: Vec<Vec<(i32, i32, i32)>> = vec![vec![]; 16];
        let mut torch: Vec<Vec<(i32, i32, i32)>> = vec![vec![]; 16];
        for x in -R..R {
            for z in -R..R {
                let col = index(x, Y0, z);
                let mut l = 15i32;
                for y in (0..h).rev() {
                    let b = block(self.blocks[col + y]);
                    if b.glow > 0 {
                        self.light[col + y] |= b.glow << 4;
                        torch[b.glow as usize].push((x, Y0 + y as i32, z));
                    }
                    if l == 0 || b.shape == Shape::Cube {
                        l = 0;
                        continue;
                    }
                    if matches!(b.shape, Shape::Cutout | Shape::Water) {
                        l = (l - 1).max(0);
                    }
                    self.light[col + y] |= l as u8;
                }
            }
        }
        // the sky's spreads from where a lit cell meets a darker open one
        for x in -R..R {
            for z in -R..R {
                let col = index(x, Y0, z);
                let top = self.tops[((x + R) * S + (z + R)) as usize] as i32;
                for y in 0..(top + 1).min(H) {
                    let l = self.light[col + y as usize] & 15;
                    if l < 2 {
                        continue;
                    }
                    let darker = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)].iter().any(|&(dx, dy, dz)| {
                        let (nx, ny, nz) = (x + dx, Y0 + y + dy, z + dz);
                        World::inside(nx, ny, nz) && (self.light[index(nx, ny, nz)] & 15) + 1 < l && block(self.blocks[index(nx, ny, nz)]).shape != Shape::Cube
                    });
                    if darker {
                        sky[l as usize].push((x, Y0 + y, z));
                    }
                }
            }
        }
        self.spread(sky, 0);
        self.spread(torch, 4);
    }

    /// Spread one kind of light (in the bits from `shift`) out from the cells queued by level.
    fn spread(&mut self, mut queue: Vec<Vec<(i32, i32, i32)>>, shift: u8) {
        let get = |l: u8| (l >> shift) & 15;
        for l in (2..=15usize).rev() {
            while let Some((x, y, z)) = queue[l].pop() {
                if get(self.light[index(x, y, z)]) as usize != l {
                    continue;
                }
                for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0), (0, 1, 0)] {
                    let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                    if !World::inside(nx, ny, nz) {
                        continue;
                    }
                    let i = index(nx, ny, nz);
                    let shape = block(self.blocks[i]).shape;
                    if shape == Shape::Cube {
                        continue;
                    }
                    let cost = if matches!(shape, Shape::Cutout | Shape::Water) && shift == 0 { 2 } else { 1 };
                    let nl = l.saturating_sub(cost);
                    if nl > get(self.light[i]) as usize {
                        self.light[i] = (self.light[i] & !(15 << shift)) | ((nl as u8) << shift);
                        if nl >= 2 {
                            queue[nl].push((nx, ny, nz));
                        }
                    }
                }
            }
        }
    }
}

/// The ground's height over the world, and where the water is.
struct Terrain {
    hills: Noise,
    detail: Noise,
    peaks: Noise,
    shore: Noise,
    ahead: Ahead,
    farm: (f64, f64),
}

/// Each column's ground height (the row over its top block) and how steep it is there.
pub struct Columns {
    h: Vec<i32>,
    steep: Vec<i32>,
}

impl Columns {
    pub fn at(&self, x: i32, z: i32) -> (i32, i32) {
        let i = ((x.clamp(-R, R - 1) + R) * S + (z.clamp(-R, R - 1) + R)) as usize;
        (self.h[i], self.steep[i])
    }
}

impl Terrain {
    fn new(seed: i64, ahead: Ahead, farm: (i32, i32)) -> Terrain {
        Terrain {
            hills: Noise::new(seed, 1),
            detail: Noise::new(seed, 2),
            peaks: Noise::new(seed, 3),
            shore: Noise::new(seed, 4),
            ahead,
            farm: (farm.0 as f64, farm.1 as f64),
        }
    }

    /// How far (x, z) is from the river's edge: under 0 in it. The river runs across the valley
    /// beyond the farm, winding.
    fn wet(&self, x: f64, z: f64) -> f64 {
        let along = x * self.ahead.0 + z * self.ahead.1;
        let across = x * self.ahead.1 - z * self.ahead.0;
        let course = FARM_AHEAD + 46.0 + 12.0 * (across / 37.0).sin() + 8.0 * self.shore.fbm(across / 70.0, 3.0, 2);
        let width = 4.0 + 1.5 * self.shore.at(across / 30.0, 7.0);
        (along - course).abs() - width
    }

    /// How far (x, z) is inside the farm's flat ground: over 0 inside, falling off outside.
    fn farmland(&self, x: f64, z: f64) -> f64 {
        let half = super::farm::HALF as f64 + 3.0;
        let d = ((x - self.farm.0).abs() - half).max((z - self.farm.1).abs() - half);
        1.0 - smoothstep(0.0, 14.0, d)
    }

    fn height(&self, x: f64, z: f64) -> f64 {
        let r = x.hypot(z);
        let far = x * self.ahead.0 + z * self.ahead.1;
        let mut h = GROUND as f64 + 1.0 + 6.0 * self.hills.fbm(x / 70.0, z / 70.0, 4) + 2.0 * self.detail.fbm(x / 18.0, z / 18.0, 3);
        // mountains rise beyond the river, and all about further off
        let rise = smoothstep(110.0, 175.0, far.max(r * 0.8));
        let ridge = self.peaks.ridged(x / 95.0, z / 95.0, 5);
        h += rise * (16.0 + 70.0 * ridge.powf(1.5)) + rise * 6.0 * self.detail.fbm(x / 9.0, z / 9.0, 2);
        // and on the left, close behind the farm: the mine's mountains
        let left = mountains_left(self.ahead, x, z);
        let crags = self.peaks.ridged(x / 55.0 + 11.0, z / 55.0, 4);
        h += left * (12.0 + 48.0 * crags.powf(1.3)) + left * 4.0 * self.detail.fbm(x / 7.0, z / 7.0, 2);
        // the knoll, on a shelf above the valley, steep on every side
        let k = knoll(self.ahead);
        let near = (x - k.0).hypot(z - k.1);
        let shelf = 1.0 - smoothstep(22.0, 32.0, near);
        if shelf > 0.0 {
            h = h.max(GROUND as f64 + 7.0 * shelf + 12.0 * (1.0 - smoothstep(8.0, 16.0, near)) + 2.0 * self.detail.fbm(x / 6.0, z / 6.0, 2) * shelf);
        }
        // the farm's ground is level
        h += (GROUND as f64 - h) * self.farmland(x, z);
        // the river: the ground falls to below its level, its banks sloping up
        let wet = self.wet(x, z);
        let bed = SEA as f64 - 2.0 - 2.5 * smoothstep(0.0, -5.0, wet);
        let bank = smoothstep(-2.0, 6.0, wet);
        if wet < 14.0 {
            let level = SEA as f64 + 0.6 + 2.0 * smoothstep(0.0, 10.0, wet);
            h = h.min(level + (h - level).max(0.0) * smoothstep(2.0, 14.0, wet));
        }
        h = bed + (h - bed) * bank;
        h.clamp(Y0 as f64 + 4.0, Y1 as f64 - 24.0)
    }

    fn columns(&self) -> Columns {
        let mut h = vec![0; (S * S) as usize];
        for x in -R..R {
            for z in -R..R {
                h[((x + R) * S + (z + R)) as usize] = self.height(x as f64, z as f64).round() as i32;
            }
        }
        let at = |x: i32, z: i32| h[((x.clamp(-R, R - 1) + R) * S + (z.clamp(-R, R - 1) + R)) as usize];
        let mut steep = vec![0; (S * S) as usize];
        for x in -R..R {
            for z in -R..R {
                let c = at(x, z);
                steep[((x + R) * S + (z + R)) as usize] =
                    [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|&(dx, dz)| (c - at(x + dx, z + dz)).abs()).max().unwrap_or(0);
            }
        }
        Columns { h, steep }
    }
}
