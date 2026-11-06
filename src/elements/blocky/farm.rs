//! The farm: a village of timber houses about a well, on paths of trodden earth, its fields of wheat,
//! carrots, potatoes and beetroot watered by channels, pens of sheep, cows and pigs, chickens about the
//! yards, a cat at a door and villagers at work; lamp posts along the paths and a torch in every
//! house, which light it at night.

use super::mobs::Kind;
use super::world::*;
use crate::rng::Rng;

/// The farm reaches this far from its middle either way, in blocks.
pub const HALF: i32 = 46;

/// The valley floor's top blocks, and the row things stand on.
const FLOOR: i32 = GROUND - 1;
const ON: i32 = GROUND;

struct Farm<'a> {
    w: &'a mut World,
    /// Its middle.
    c: (i32, i32),
    rng: Rng,
}

impl Farm<'_> {
    fn put(&mut self, dx: i32, y: i32, dz: i32, b: u8) {
        self.w.set(self.c.0 + dx, y, self.c.1 + dz, b);
    }
    fn get(&self, dx: i32, y: i32, dz: i32) -> u8 {
        self.w.get(self.c.0 + dx, y, self.c.1 + dz)
    }
    fn mob(&mut self, kind: Kind, dx: f64, dz: f64, yaw: f64) {
        self.mob_at(kind, dx, ON as f64, dz, yaw);
    }
    /// A mob with its feet at height `y`: a duck afloat, a fish under the water.
    fn mob_at(&mut self, kind: Kind, dx: f64, y: f64, dz: f64, yaw: f64) {
        let p = [self.c.0 as f64 + dx + 0.5, y, self.c.1 as f64 + dz + 0.5];
        self.w.mobs.add(kind, p, yaw);
    }

    /// The ground cleared and levelled: grass, a few tufts, nothing standing. Only the valley floor:
    /// the viewer's hill and the river are left as they are.
    fn clear(&mut self) {
        for dx in -HALF..=HALF {
            for dz in -HALF..=HALF {
                if self.w.ground(self.c.0 + dx, self.c.1 + dz) != ON {
                    continue;
                }
                for y in ON..ON + 12 {
                    self.put(dx, y, dz, AIR);
                }
                self.put(dx, FLOOR, dz, GRASS);
                for y in FLOOR - 3..FLOOR {
                    self.put(dx, y, dz, DIRT);
                }
                let r = self.rng.f64();
                if r < 0.1 {
                    self.put(dx, ON, dz, TALL_GRASS);
                } else if r < 0.115 {
                    let f = *self.rng.pick(&[POPPY, DANDELION, CORNFLOWER]);
                    self.put(dx, ON, dz, f);
                }
            }
        }
    }

    /// A path of trodden earth from one corner to the other, inclusive, its edges kept straight.
    fn path(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) {
        for dx in x0.min(x1)..=x0.max(x1) {
            for dz in z0.min(z1)..=z0.max(z1) {
                self.put(dx, FLOOR, dz, PATH);
                self.put(dx, ON, dz, AIR);
            }
        }
    }

    /// A house over x0..=x1, z0..=z1: a cobblestone footing, logs at its corners and along its eaves,
    /// plank walls with windows, a door in the middle of the side `door` faces (0: -z, 1: +z, 2: -x,
    /// 3: +x), a roof of stairs rising to a ridge along its longer side, a torch inside.
    fn house(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, door: u8, walls: i32) {
        let top = ON + walls;
        let (dmid_x, dmid_z) = ((x0 + x1) / 2, (z0 + z1) / 2);
        let door_at = match door {
            0 => (dmid_x, z0),
            1 => (dmid_x, z1),
            2 => (x0, dmid_z),
            _ => (x1, dmid_z),
        };
        for dx in x0..=x1 {
            for dz in z0..=z1 {
                let wall = dx == x0 || dx == x1 || dz == z0 || dz == z1;
                let corner = (dx == x0 || dx == x1) && (dz == z0 || dz == z1);
                self.put(dx, FLOOR, dz, if wall { COBBLE } else { PLANKS });
                for y in ON..=top {
                    let b = if !wall {
                        AIR
                    } else if corner {
                        OAK_LOG
                    } else if y == top {
                        if dz == z0 || dz == z1 { LOG_X } else { LOG_Z }
                    } else if (dx, dz) == door_at && y < ON + 2 {
                        if y == ON { DOOR_LOW } else { DOOR_HIGH }
                    } else if y == ON + 1 && self.window(dx, dz, x0, z0, x1, z1, door_at) {
                        GLASS
                    } else {
                        PLANKS
                    };
                    self.put(dx, y, dz, b);
                }
            }
        }
        // the step before the door, and the lamp beside it
        let out = match door {
            0 => (0, -1),
            1 => (0, 1),
            2 => (-1, 0),
            _ => (1, 0),
        };
        self.put(door_at.0 + out.0, FLOOR, door_at.1 + out.1, PATH);
        self.put(door_at.0 + out.0, ON, door_at.1 + out.1, AIR);
        self.put(dmid_x, ON, dmid_z, TORCH);
        // lanterns in its far corners, so its windows glow at night
        self.put(x1 - 1, ON, z1 - 1, LANTERN);
        self.put(x0 + 1, ON, z1 - 1, LANTERN);
        self.put(x1 - 1, ON, z0 + 1, LANTERN);
        // a lantern on a post beside the door
        let (sx, sz) = if door < 2 { (2, 0) } else { (0, 2) };
        let (px, pz) = (door_at.0 + out.0 + sx, door_at.1 + out.1 + sz);
        self.put(px, ON, pz, FENCE);
        self.put(px, ON + 1, pz, LANTERN);
        // the roof: along x when the house is wider than deep
        let along_x = x1 - x0 >= z1 - z0;
        let (lo, hi) = if along_x { (z0 - 1, z1 + 1) } else { (x0 - 1, x1 + 1) };
        let (from, to) = if along_x { (x0 - 1, x1 + 1) } else { (z0 - 1, z1 + 1) };
        let mut k = 0;
        loop {
            let (a, b) = (lo + k, hi - k);
            let y = top + k;
            if a > b {
                break;
            }
            for s in from..=to {
                let at = |q: i32| if along_x { (s, q) } else { (q, s) };
                if a == b {
                    let (x, z) = at(a);
                    self.put(x, y, z, ROOF_SLAB);
                    continue;
                }
                let (x, z) = at(a);
                self.put(x, y, z, if along_x { STAIRS_PZ } else { STAIRS_PX });
                let (x, z) = at(b);
                self.put(x, y, z, if along_x { STAIRS_NZ } else { STAIRS_NX });
                // the gable ends, between the slopes
                if (s == from + 1 || s == to - 1) && k >= 1 {
                    for q in a + 1..b {
                        let (x, z) = at(q);
                        self.put(x, y, z, PLANKS);
                    }
                }
            }
            if a == b || a + 1 == b {
                break;
            }
            k += 1;
        }
    }

    /// Whether a wall block takes a window: every other block along each side, away from the corners
    /// and the door.
    fn window(&self, dx: i32, dz: i32, x0: i32, z0: i32, x1: i32, z1: i32, door: (i32, i32)) -> bool {
        if (dx - door.0).abs() + (dz - door.1).abs() <= 1 {
            return false;
        }
        let along = if dz == z0 || dz == z1 { dx - x0 } else { dz - z0 };
        let len = if dz == z0 || dz == z1 { x1 - x0 } else { z1 - z0 };
        along >= 2 && along <= len - 2 && along % 2 == 0
    }

    /// A field over x0..=x1, z0..=z1: tilled earth in rows of a crop each, a channel of water down its
    /// middle, framed in logs.
    fn field(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, crops: &[u8]) {
        let along_x = x1 - x0 >= z1 - z0;
        let mid = if along_x { (z0 + z1) / 2 } else { (x0 + x1) / 2 };
        for dx in x0..=x1 {
            for dz in z0..=z1 {
                let frame = dx == x0 || dx == x1 || dz == z0 || dz == z1;
                self.put(dx, ON, dz, AIR);
                if frame {
                    self.put(dx, FLOOR, dz, if along_x == (dz == z0 || dz == z1) { LOG_X } else { LOG_Z });
                    continue;
                }
                let across = if along_x { dz } else { dx };
                if across == mid {
                    self.put(dx, FLOOR, dz, WATER);
                    continue;
                }
                self.put(dx, FLOOR, dz, FARMLAND);
                // a crop to each pair of rows; here and there one not yet grown
                let row = ((across - mid).abs() - 1) / 2;
                let crop = crops[row as usize % crops.len()];
                let crop = if crop == WHEAT && self.rng.f64() < 0.12 { WHEAT_YOUNG } else { crop };
                if self.rng.f64() < 0.96 {
                    self.put(dx, ON, dz, crop);
                }
            }
        }
    }

    /// A fence round x0..=x1, z0..=z1, with a gap for a gate at (gx, gz).
    fn pen(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, gate: (i32, i32)) {
        let inside = [self.c.0 + x0 + 1, self.c.1 + z0 + 1, self.c.0 + x1 - 1, self.c.1 + z1 - 1];
        self.w.pens.push(inside);
        for dx in x0..=x1 {
            for dz in z0..=z1 {
                let edge = dx == x0 || dx == x1 || dz == z0 || dz == z1;
                if edge && (dx, dz) != gate {
                    self.put(dx, ON, dz, FENCE);
                } else if !edge && self.get(dx, ON, dz) != AIR && self.rng.f64() < 0.7 {
                    // grazed short
                    self.put(dx, ON, dz, AIR);
                }
            }
        }
    }

    /// A lamp post: a fence post two high and a torch on it.
    fn lamp(&mut self, dx: i32, dz: i32) {
        self.put(dx, ON, dz, FENCE);
        self.put(dx, ON + 1, dz, FENCE);
        self.put(dx, ON + 2, dz, TORCH);
    }

    /// The well: a ring of cobblestone about water, posts at its corners, a roof of slabs.
    fn well(&mut self) {
        for dx in -1..=1 {
            for dz in -1..=1 {
                for y in FLOOR - 4..FLOOR {
                    self.put(dx, y, dz, COBBLE);
                }
                self.put(dx, FLOOR, dz, COBBLE);
                self.put(dx, ON, dz, if dx == 0 && dz == 0 { AIR } else { COBBLE });
                self.put(dx, ON + 3, dz, OAK_SLAB);
            }
        }
        for y in FLOOR - 3..=FLOOR {
            self.put(0, y, 0, WATER);
        }
        for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
            self.put(dx, ON + 1, dz, FENCE);
            self.put(dx, ON + 2, dz, FENCE);
        }
    }
}

impl Farm<'_> {
    /// Water in a cell of the stream: its bed below, a lily pad on it now and then, cane on its banks.
    fn water(&mut self, dx: i32, dz: i32, deep: i32) {
        // cane on what was the bank goes with it, all of it
        for y in ON + 1..ON + 4 {
            if self.get(dx, y, dz) == CANE {
                self.put(dx, y, dz, AIR);
            }
        }
        for d in 0..deep {
            self.put(dx, FLOOR - d, dz, WATER);
        }
        let bed = if self.rng.f64() < 0.5 { SAND } else { GRAVEL };
        self.put(dx, FLOOR - deep, dz, bed);
        let top = if self.rng.f64() < 0.1 { LILY } else { AIR };
        self.put(dx, ON, dz, top);
        for (bx, bz) in [(dx + 1, dz), (dx - 1, dz), (dx, dz + 1), (dx, dz - 1)] {
            if self.get(bx, FLOOR, bz) == GRASS && self.get(bx, ON, bz) != CANE && self.rng.f64() < 0.06 {
                self.put(bx, ON, bz, CANE);
                self.put(bx, ON + 1, bz, CANE);
            }
        }
    }

    /// The stream: across the farm between the houses and the pens, winding a little; at the farm's
    /// end it turns back towards the hill round the fields, and ends in a pond before the hill. A deck
    /// of planks where a path crosses it.
    fn stream(&mut self) {
        let mut cells = vec![];
        for dx in -31..=30 {
            let dz = 14 - ((dx as f64 / 5.0).sin().abs() * 1.4).round().min(1.0) as i32;
            cells.extend([(dx, dz), (dx, dz + 1)]);
        }
        for dz in -14..=14 {
            let wiggle = ((dz as f64 / 6.0).sin() * 0.8).round() as i32;
            cells.extend([(-29 + wiggle, dz), (-28 + wiggle, dz)]);
        }
        for dx in -29..=-13 {
            let dz = -14 + ((dx as f64 / 4.0).sin() * 0.6).round() as i32;
            cells.extend([(dx, dz), (dx, dz + 1)]);
            // widening into the pond, so it opens cleanly
            if dx >= -19 {
                cells.extend([(dx, dz - 1), (dx, dz + 2)]);
            }
        }
        for (dx, dz) in cells {
            let path = self.get(dx, FLOOR, dz) == PATH;
            self.water(dx, dz, 1);
            if path {
                // a deck of planks in the water, level with the path, so no water shows between them
                self.put(dx, FLOOR, dz, PLANKS);
                self.put(dx, ON, dz, AIR);
            }
        }
        self.pond(-12, -16);
    }

    /// The duck pond: a round of water two deep, its edge uneven, lily pads, cane about it.
    fn pond(&mut self, cx: i32, cz: i32) {
        let (wx, wz) = (self.c.0 + cx, self.c.1 + cz);
        self.w.ponds.push([wx - 6, wz - 5, wx + 6, wz + 5]);
        for dx in -6..=6 {
            for dz in -5..=5 {
                let r = (dx as f64 / 4.6).powi(2) + (dz as f64 / 3.3).powi(2);
                let edge = 1.0 + 0.18 * ((dx * 3 + dz * 5) as f64).sin();
                if r < edge {
                    self.water(cx + dx, cz + dz, if r < edge * 0.5 { 2 } else { 1 });
                }
            }
        }
        // its edge smoothed: grass with water on three sides of four is a notch, and goes under
        loop {
            let notches: Vec<(i32, i32)> = (-7..=7)
                .flat_map(|dx| (-6..=6).map(move |dz| (cx + dx, cz + dz)))
                .filter(|&(x, z)| {
                    self.get(x, FLOOR, z) == GRASS
                        && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter(|&&(ox, oz)| self.get(x + ox, FLOOR, z + oz) == WATER).count() >= 3
                })
                .collect();
            if notches.is_empty() {
                break;
            }
            for (x, z) in notches {
                self.water(x, z, 1);
            }
        }
    }

    /// A rail line along x0..=x1 at z, on planks where it crosses water.
    fn rails(&mut self, x0: i32, x1: i32, dz: i32) {
        for dx in x0..=x1 {
            if self.get(dx, FLOOR, dz) == WATER {
                self.put(dx, FLOOR, dz, PLANKS);
            } else if self.get(dx, FLOOR, dz) != PLANKS {
                self.put(dx, FLOOR, dz, GRAVEL);
            }
            self.put(dx, ON, dz, RAIL);
        }
    }

    /// A trough of water in the cow pen, rimmed in planks, hay beside it.
    fn trough(&mut self, x0: i32, x1: i32, z: i32) {
        for dx in x0 - 1..=x1 + 1 {
            for dz in z - 1..=z + 1 {
                let rim = dx < x0 || dx > x1 || dz != z;
                self.put(dx, ON, dz, if rim { SPRUCE_PLANKS } else { WATER });
                self.put(dx, FLOOR, dz, PLANKS);
            }
        }
        self.put(x1 + 2, ON, z, HAY);
        self.put(x1 + 2, ON, z - 1, HAY);
    }

    /// A chicken coop: a little hut of planks with a slab roof, a fenced run before its door, straw and a
    /// barrel of feed beside it.
    fn coop(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, run: (i32, i32, i32, i32)) {
        for dx in x0..=x1 {
            for dz in z0..=z1 {
                let wall = dx == x0 || dx == x1 || dz == z0 || dz == z1;
                let corner = (dx == x0 || dx == x1) && (dz == z0 || dz == z1);
                self.put(dx, FLOOR, dz, PLANKS);
                for y in ON..ON + 2 {
                    self.put(
                        dx,
                        y,
                        dz,
                        if !wall {
                            AIR
                        } else if corner {
                            OAK_LOG
                        } else {
                            PLANKS
                        },
                    );
                }
                self.put(dx, ON + 2, dz, OAK_SLAB);
            }
        }
        // its door: a gap onto the run, the run's fence round the rest
        let door = (x1, (z0 + z1) / 2);
        self.put(door.0, ON, door.1, AIR);
        self.put(door.0, ON + 1, door.1, AIR);
        self.put((x0 + x1) / 2, ON, (z0 + z1) / 2, TORCH);
        let (rx0, rz0, rx1, rz1) = run;
        self.pen(rx0, rz0, rx1, rz1, (rx0, door.1));
        self.put(x0 - 1, ON, z0, HAY);
        self.put(x0 - 1, ON, z0 + 1, HAY);
        self.put(x0 - 1, ON + 1, z0, HAY);
        self.put(x0 - 1, ON, z1, BARREL);
    }

    /// A scarecrow: a post, crossed arms of fence, a straw body and a pumpkin for a head.
    fn scarecrow(&mut self, dx: i32, dz: i32) {
        self.put(dx, ON, dz, FENCE);
        self.put(dx, ON + 1, dz, HAY);
        self.put(dx - 1, ON + 1, dz, FENCE);
        self.put(dx + 1, ON + 1, dz, FENCE);
        self.put(dx, ON + 2, dz, PUMPKIN);
    }

    /// A campfire with log benches about it, a lantern on a post beside them.
    /// A point of the farm in the world, at height y.
    fn world(&self, dx: f64, y: f64, dz: f64) -> [f64; 3] {
        [self.c.0 as f64 + dx + 0.5, y, self.c.1 as f64 + dz + 0.5]
    }

    /// A chimney of cobblestone up from (dx, dz) inside a house, through its roof and a block past it,
    /// open at its top; smoke rises from it.
    fn chimney(&mut self, dx: i32, dz: i32) {
        let mut y = ON;
        let mut roofed = false;
        loop {
            let b = self.get(dx, y, dz);
            roofed |= b != AIR && y > ON + 2;
            self.put(dx, y, dz, COBBLE);
            y += 1;
            if (roofed && self.get(dx, y, dz) == AIR) || y > ON + 16 {
                self.put(dx, y, dz, CHIMNEY_TOP);
                break;
            }
        }
        let top = self.world(dx as f64, y as f64 + 1.0, dz as f64);
        self.w.smoke.push((top, true));
    }

    fn fire(&mut self, dx: i32, dz: i32) {
        for (x, z) in [(dx - 1, dz - 1), (dx, dz - 1), (dx + 1, dz - 1), (dx - 1, dz), (dx + 1, dz), (dx - 1, dz + 1), (dx, dz + 1), (dx + 1, dz + 1)] {
            self.put(x, FLOOR, z, PATH);
            self.put(x, ON, z, AIR);
        }
        self.put(dx, FLOOR, dz, COBBLE);
        self.put(dx, ON, dz, CAMPFIRE);
        let smoke = self.world(dx as f64, ON as f64 + 0.6, dz as f64);
        self.w.smoke.push((smoke, false));
        self.put(dx - 2, ON, dz, LOG_Z);
        self.put(dx - 2, ON, dz + 1, LOG_Z);
        self.put(dx + 2, ON, dz - 1, LOG_Z);
        self.put(dx + 2, ON, dz, LOG_Z);
        self.put(dx, ON, dz + 2, LOG_X);
        self.put(dx + 1, ON, dz + 2, LOG_X);
        self.put(dx - 2, ON, dz - 2, FENCE);
        self.put(dx - 2, ON + 1, dz - 2, LANTERN);
    }
}

/// The mine: an opening in the knoll's face towards the viewer: a frame of logs and planks, rails
/// running in, torches inside, lanterns at the door.
fn mine(w: &mut World, ahead: (f64, f64)) {
    // into the hill along the axis nearest the line from the viewer to it, from its foot on that side
    let k = knoll(ahead);
    let (ax, az) = if k.0.abs() > k.1.abs() { (k.0.signum() as i32, 0) } else { (0, k.1.signum() as i32) };
    let (sx, sz) = (-az, ax);
    let (kx, kz) = (k.0.round() as i32, k.1.round() as i32);
    let Some((mx, mz, floor)) = (-30..0).find_map(|d| {
        let (x, z) = (kx + d * ax, kz + d * az);
        let before = w.ground(x - 6 * ax, z - 6 * az);
        // the knoll's own face, from the shelf it stands on
        (w.ground(x, z) >= before + 5 && before >= GROUND + 5).then_some((x, z, before))
    }) else {
        return;
    };
    let at = |along: i32, side: i32| (mx + along * ax + side * sx, mz + along * az + side * sz);
    // the hillside cut back to a face before it, wider further out, its floor levelled; the tunnel
    for along in -6..16 {
        for side in -5..=5 {
            let (x, z) = at(along, side);
            let cut = along < 0 && side.abs() <= 2 - along / 2;
            let tunnel = along >= 0 && side.abs() <= 1;
            if !(cut || tunnel) {
                continue;
            }
            for y in floor..floor + if tunnel { 3 } else { 30 } {
                w.set(x, y, z, AIR);
            }
            w.set(
                x,
                floor - 1,
                z,
                if tunnel {
                    STONE
                } else if side.abs() <= 2 {
                    GRAVEL
                } else {
                    GRASS
                },
            );
        }
    }
    // the frame at its mouth and every four blocks in
    for along in [0, 4, 8, 12] {
        for side in [-2, 2] {
            let (x, z) = at(along, side);
            for y in floor..floor + 3 {
                w.set(x, y, z, OAK_LOG);
            }
        }
        for side in -2..=2 {
            let (x, z) = at(along, side);
            w.set(x, floor + 3, z, PLANKS);
        }
    }
    for along in -4..16 {
        let (x, z) = at(along, 0);
        w.set(x, floor, z, if ax != 0 { RAIL } else { RAIL_Z });
    }
    for along in [2, 6, 10, 14] {
        let (x, z) = at(along, 1);
        w.set(x, floor, z, TORCH);
    }
    for side in [-3, 3] {
        let (x, z) = at(-1, side);
        w.set(x, floor, z, FENCE);
        w.set(x, floor + 1, z, LANTERN);
    }
    let (x, z) = at(-2, -2);
    w.set(x, floor, z, BARREL);
    let (x, z) = at(-3, -2);
    w.set(x, floor, z, COBBLE);
    w.set(x, floor + 1, z, COAL);
    // torches over its mouth, on the frame, and along the cut either side, so it shows from afar at night
    for side in [-2, 2] {
        let (x, z) = at(0, side);
        w.set(x, floor + 4, z, TORCH);
    }
    for (along, side) in [(-3, -4), (-3, 4), (-6, -5), (-6, 5)] {
        let (x, z) = at(along, side);
        w.set(x, floor, z, FENCE);
        w.set(x, floor + 1, z, TORCH);
    }
    // a fire pit before it, in a ring of cobblestone
    let (cx, cz) = at(-5, 2);
    for (dx, dz) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
        w.set(cx + dx, floor - 1, cz + dz, COBBLE);
        w.set(cx + dx, floor, cz + dz, COBBLE_SLAB);
    }
    w.set(cx, floor - 1, cz, COBBLE);
    w.set(cx, floor, cz, CAMPFIRE);
    w.smoke.push(([cx as f64 + 0.5, floor as f64 + 0.6, cz as f64 + 0.5], false));
}

/// Build the farm about (x, z), and the mine beyond it along `ahead`.
pub fn build(w: &mut World, seed: i64, c: (i32, i32), ahead: (f64, f64)) {
    let mut f = Farm { w: &mut *w, c, rng: Rng::new(seed ^ 0xfa53) };
    f.clear();
    // the paths: two crossing at the well, a square about it
    f.path(-28, -1, 28, 1);
    f.path(-1, -28, 1, 28);
    // the road runs on over the stream at the farm's end, straight, as far as the ground stays level
    for dx in (-HALF..=-29).rev() {
        if (-1..=1).any(|dz| f.w.ground(f.c.0 + dx, f.c.1 + dz) != ON) {
            break;
        }
        f.path(dx, -1, dx, 1);
    }
    // a path from the road straight to the fields beyond the stream, over it on slabs as the road
    f.path(-16, 2, -14, 36);
    f.path(-3, -3, 3, 3);
    f.well();
    // the houses about the crossing, their doors on the paths
    f.house(-13, -12, -5, -6, 1, 3);
    f.path(-9, -5, -9, -2);
    f.house(5, -13, 11, -6, 1, 3);
    f.path(8, -5, 8, -2);
    f.house(-12, 6, -5, 12, 0, 3);
    f.path(-8, 2, -8, 5);
    f.house(5, 6, 14, 12, 0, 4);
    f.path(9, 2, 9, 5);
    f.house(17, -10, 22, -5, 2, 3);
    f.path(13, -7, 16, -7);
    f.path(13, -7, 13, -2);
    // the fields beyond the houses, either side
    f.field(-25, -10, -17, -3, &[WHEAT, CARROTS, WHEAT, POTATOES]);
    f.field(-25, 3, -17, 12, &[WHEAT, BEETROOTS, POTATOES, WHEAT]);
    f.field(17, 3, 28, 12, &[CARROTS, WHEAT, WHEAT]);
    f.field(25, -22, 35, -11, &[WHEAT, POTATOES, WHEAT, CARROTS]);
    // and two beyond the stream, the path crossing to them
    f.field(-31, 18, -19, 26, &[BEETROOTS, WHEAT, CARROTS]);
    f.field(-31, 28, -19, 36, &[WHEAT, POTATOES, WHEAT]);
    // the pens, behind the houses
    f.pen(-13, 16, -2, 26, (-7, 16));
    f.pen(3, 17, 17, 27, (9, 17));
    // and a pasture on the near side, by the road
    f.pen(3, -29, 15, -17, (3, -23));
    f.path(2, -23, 2, -23);
    f.path(-7, 14, -7, 15);
    f.path(9, 15, 9, 16);
    f.chimney(-12, -11);
    f.chimney(13, 7);
    f.chimney(6, -12);
    f.chimney(-11, 7);
    f.chimney(18, -9);
    f.stream();
    // a rail line from the pond along the stream, on straight where it turns, a cart waiting on it
    f.rails(-46, -16, -11);
    f.mob_at(Kind::Cart, -26.5, ON as f64 + 0.07, -11.0, std::f64::consts::FRAC_PI_2);
    // on the near side: a chicken coop with its run, and a kitchen garden with a scarecrow
    f.coop(3, -36, 5, -34, (6, -37, 10, -32));
    f.field(-5, -32, 2, -29, &[CARROTS, POTATOES, BEETROOTS]);
    f.scarecrow(-1, -30);
    // and a large field right below the viewer
    f.field(-27, -33, -11, -20, &[WHEAT, CARROTS, WHEAT, POTATOES, WHEAT]);
    f.trough(11, 14, 25);
    f.trough(9, 11, -27);
    f.fire(-5, -19);
    // where fireflies dance at night: over the pond, the fields and the garden
    for (dx, dz, reach) in [(-12.0, -16.0, 6.0), (-19.0, -26.0, 7.0), (-1.0, -30.0, 4.0), (-21.0, -6.0, 5.0), (8.0, -23.0, 5.0)] {
        let at = f.world(dx, ON as f64, dz);
        f.w.glades.push((at, reach));
    }
    // lanterns on the pens' corner posts
    for (dx, dz) in [(-13, 16), (-2, 16), (3, 17), (17, 17), (3, -17), (15, -17), (3, -29), (15, -29)] {
        f.put(dx, ON + 1, dz, LANTERN);
    }
    // flowers either side of each door
    for (dx, dz) in [(-10, -4), (-8, -4), (7, -4), (9, -4), (-9, 4), (-7, 4), (8, 4), (10, 4)] {
        if f.get(dx, FLOOR, dz) == GRASS {
            let flower = *f.rng.pick(&[POPPY, DANDELION, CORNFLOWER]);
            f.put(dx, ON, dz, flower);
        }
    }
    // a well-kept yard: hay, a crafting table, a barrel, pumpkins
    for (dx, y, dz) in [(16, ON, 10), (16, ON, 11), (17, ON, 10), (16, ON + 1, 10), (17, ON, 11)] {
        f.put(dx, y, dz, HAY);
    }
    f.put(12, ON, -4, CRAFTING);
    f.put(13, ON, -4, BARREL);
    f.put(-14, ON, -4, BARREL);
    for (dx, dz) in [(19, -15), (21, -14), (20, -16), (23, -15)] {
        f.put(dx, ON, dz, PUMPKIN);
    }
    // lamps along the paths, each beside one, never in it
    for (dx, dz) in [(-4, -4), (4, 4), (-4, 4), (4, -4), (-17, 2), (16, -2), (2, -17), (-2, 17), (-27, -2), (27, 2)] {
        f.lamp(dx, dz);
    }
    // a few oaks for shade
    let mut rng = Rng::new(seed ^ 0x0a4);
    for (dx, dz) in [(-20, 20), (24, 21), (-38, -6), (23, -24)] {
        let (x, z) = (c.0 + dx, c.1 + dz);
        f.w.broadleaf(&mut rng, x, ON, z, OAK_LOG, OAK_LEAVES, (4, 6));
    }
    // the animals in their pens, the chickens about the yards, a cat at a door, villagers at work
    for (dx, dz, yaw) in [(-10.0, 19.0, 0.4), (-6.0, 21.5, 2.2), (-9.5, 23.5, -1.0), (-4.5, 18.5, 3.6), (-11.5, 24.5, 1.2)] {
        f.mob(Kind::Sheep, dx, dz, yaw);
    }
    for (dx, dz, yaw) in [(6.0, 20.0, 1.1), (11.0, 23.5, -0.6), (14.5, 20.5, 2.6)] {
        f.mob(Kind::Cow, dx, dz, yaw);
    }
    for (dx, dz, yaw) in [(8.5, 25.0, 0.2), (13.0, 25.5, 4.0), (5.5, 23.5, 2.9)] {
        f.mob(Kind::Pig, dx, dz, yaw);
    }
    for (dx, dz, yaw) in [(2.6, -6.5, 0.3), (3.4, -8.0, 2.1), (-3.0, 9.5, 4.4), (15.0, 1.0 + 2.5, 1.6), (-15.5, 0.0 + 3.0, 5.2)] {
        f.mob(Kind::Chicken, dx, dz, yaw);
    }
    // and about the campfire on the near side, most turned towards the hill
    for (dx, dz, yaw) in [(8.0, -22.0, 3.9), (11.5, -19.5, 3.0), (5.5, -25.5, 4.4), (13.0, -26.5, 3.6)] {
        f.mob(Kind::Sheep, dx, dz, yaw);
    }
    for (dx, dz, yaw) in [(6.0, -20.0, 3.3), (12.5, -23.5, 2.2)] {
        f.mob(Kind::Cow, dx, dz, yaw);
    }
    for (dx, dz, yaw) in [(9.5, -18.5, 4.0), (5.0, -27.5, 3.1)] {
        f.mob(Kind::Pig, dx, dz, yaw);
    }
    for (dx, dz, yaw) in [(-2.5, -16.5, 3.0), (-3.5, -23.0, 4.6), (-15.0, -10.5, 3.8)] {
        f.mob(Kind::Chicken, dx, dz, yaw);
    }
    f.mob(Kind::Cat, -3.2, -20.5, 3.9);
    for (dx, dz, yaw) in [(7.0, -35.5, 3.4), (8.5, -33.5, 4.1), (7.5, -33.0, 2.6), (9.0, -35.0, 5.0)] {
        f.mob(Kind::Chicken, dx, dz, yaw);
    }
    // ducks on the pond, ducklings after one; fish under the water
    let float = FLOOR as f64 + 0.68;
    for (dx, dz, yaw) in [(-14.0, -16.0, 3.6), (-9.5, -14.8, 2.4)] {
        f.mob_at(Kind::Duck, dx, float, dz, yaw);
    }
    for (dx, dz) in [(-14.4, -17.3), (-15.0, -18.0), (-15.6, -17.1)] {
        f.mob_at(Kind::Duckling, dx, float + 0.04, dz, 3.4);
    }
    for (dx, dz, yaw) in [(-12.5, -16.8, 1.2), (-10.0, -16.2, 4.0)] {
        f.mob_at(Kind::Fish, dx, FLOOR as f64 - 0.75, dz, yaw);
    }
    f.mob(Kind::Villager, -5.0, -21.6, 0.0);
    f.mob(Kind::Cat, -10.2, -4.2, 0.9);
    f.mob(Kind::Villager, 0.0, -7.0, 0.5);
    f.mob(Kind::Farmer, -22.0, -0.2, -1.4);
    f.mob(Kind::Villager, 9.0, 3.2, 3.4);
    cart_tunnel(f.w, ahead);
    shaft(f.w, ahead);
    cliff_house(f.w, ahead);
    let mut mobs = std::mem::take(&mut f.w.mobs);
    mobs.place(f.w);
    f.w.mobs = mobs;
    mine(w, ahead);
}

/// A dwelling in the mountains left of the mine: a plateau levelled out of the slope, a cottage of logs
/// and planks at its front, its door and windows to the viewer, a slab roof, lanterns either side of
/// the door and a torch inside; a spruce, a bench and flowers about it, and a waterfall down the rock
/// behind into a pool.
fn cliff_house(w: &mut World, ahead: (f64, f64)) {
    let p = cliff_at(ahead);
    let (ax, az) = if p.0.abs() > p.1.abs() { (p.0.signum() as i32, 0) } else { (0, p.1.signum() as i32) };
    let (sx, sz) = (-az, ax);
    let (cx, cz) = (p.0.round() as i32, p.1.round() as i32);
    let at = |along: i32, side: i32| (cx + along * ax + side * sx, cz + along * az + side * sz);
    // the plateau: cut down into the slope as low as it can be and still stand clear of the slope before
    // it, so the house is seen over its edge; built up out over the slope below, its top grass
    let front = at(-9, 0);
    let level = (w.ground(cx, cz) - 8).max(w.ground(front.0, front.1) + 2);
    for along in -4..=3 {
        for side in -8..=8i32 {
            let (x, z) = at(along, side);
            if along == 3 {
                // the rock behind stands as it is
                continue;
            }
            for y in level..level + 40 {
                w.set(x, y, z, AIR);
            }
            for y in level - 24..level {
                if block(w.get(x, y, z)).shape != Shape::Cube {
                    w.set(x, y, z, STONE);
                }
            }
            w.set(x, level - 1, z, if along >= 1 && side.abs() <= 1 { PATH } else { GRASS });
        }
    }
    // the slope before it cut down to its level too, out to where the ground falls away below it, so
    // the cottage is seen over the terrace
    for along in -18..=-5 {
        for side in -8..=8 {
            let (x, z) = at(along, side);
            if w.ground(x, z) < level {
                continue;
            }
            for y in level..level + 40 {
                w.set(x, y, z, AIR);
            }
            w.set(x, level - 1, z, GRASS);
        }
    }
    // the rock wall at its back
    for side in -8..=8 {
        let (x, z) = at(3, side);
        for y in level..level + 7 {
            w.set(x, y, z, STONE);
        }
    }
    // the cottage at the plateau's front, its door and windows to the viewer: logs at its corners,
    // plank walls, a slab roof overhanging, a torch inside, lanterns either side of the door
    for along in -3..=1 {
        for side in -3..=3i32 {
            let (x, z) = at(along, side);
            let wall = along == -3 || along == 1 || side.abs() == 3;
            let corner = (along == -3 || along == 1) && side.abs() == 3;
            w.set(x, level - 1, z, PLANKS);
            for y in level..level + 4 {
                let b = if !wall {
                    AIR
                } else if corner || y == level + 3 {
                    OAK_LOG
                } else if along == -3 && side == 0 && y < level + 2 {
                    if y == level { DOOR_LOW } else { DOOR_HIGH }
                } else if y == level + 1 && ((along == -3 && side.abs() == 2) || (side.abs() == 3 && along == -1)) {
                    GLASS
                } else {
                    PLANKS
                };
                w.set(x, y, z, b);
            }
        }
    }
    for along in -4..=2 {
        for side in -4..=4 {
            let (x, z) = at(along, side);
            w.set(x, level + 4, z, ROOF_SLAB);
        }
    }
    let (x, z) = at(-1, 0);
    w.set(x, level, z, TORCH);
    for side in [-2, 2] {
        let (x, z) = at(-4, side);
        w.set(x, level, z, FENCE);
        w.set(x, level + 1, z, LANTERN);
    }
    // the plateau's furniture: a spruce, a bench, flowers
    let mut rng = Rng::new(0xc1ff);
    let (x, z) = at(-1, -6);
    w.broadleaf(&mut rng, x, level, z, SPRUCE_LOG, SPRUCE_LEAVES, (4, 5));
    for side in [5, 6] {
        let (x, z) = at(-3, side);
        w.set(x, level, z, if ax != 0 { LOG_Z } else { LOG_X });
    }
    for (along, side, f) in [(-4, -4, POPPY), (-4, 4, DANDELION), (-2, 5, CORNFLOWER), (0, -5, POPPY), (-4, -6, DANDELION)] {
        let (x, z) = at(along, side);
        w.set(x, level, z, f);
    }
    // the waterfall down the rock beside the house, into its pool
    for y in level..level + 9 {
        let (x, z) = at(3, 6);
        w.set(x, y, z, WATER);
    }
    for along in 1..=2 {
        for side in 5..=7 {
            let (x, z) = at(along, side);
            w.set(x, level - 1, z, WATER);
            w.set(x, level - 2, z, STONE);
        }
    }
}

/// A mineshaft in the foot of the grey mountain on the left, its mouth to the farm: a timber frame at
/// the mouth and every four blocks in, rails running in from a gravel apron, torches on the frame,
/// every twelve blocks inside, and on posts along the mountain's foot either side, so the foot shows at
/// night. It runs on into the dark past its last torch, so its end is not seen.
fn shaft(w: &mut World, ahead: (f64, f64)) {
    const DEEP: i32 = 44;
    let p = shaft_at(ahead);
    // into the mountain, along the axis nearest the line across from the farm to it
    let into = (ahead.1, -ahead.0);
    let (ax, az) = if into.0.abs() > into.1.abs() { (into.0.signum() as i32, 0) } else { (0, into.1.signum() as i32) };
    let (sx, sz) = (-az, ax);
    let (px, pz) = (p.0.round() as i32, p.1.round() as i32);
    // the foot of the mountain's face, coming from the farm's side
    let Some((mx, mz, floor)) = (-30..0).find_map(|d| {
        let (x, z) = (px + d * ax, pz + d * az);
        let before = w.ground(x - 4 * ax, z - 4 * az);
        (w.ground(x, z) >= before + 5 && before >= GROUND).then_some((x, z, before))
    }) else {
        return;
    };
    let at = |along: i32, side: i32| (mx + along * ax + side * sx, mz + along * az + side * sz);
    // no tree between the viewer and it, so its mouth is seen: each tree whose trunk stands near the
    // line of sight taken out whole. A trunk is logs crowned by leaves: the farm's buildings have log
    // corners too
    let log = |b: u8| matches!(b, OAK_LOG | BIRCH_LOG | SPRUCE_LOG);
    let leaves = |b: u8| matches!(b, OAK_LEAVES | BIRCH_LEAVES | SPRUCE_LEAVES);
    let (ex, ez) = (w.eye[0], w.eye[2]);
    let (tx, tz) = (mx as f64 - 2.0 * ax as f64, mz as f64 - 2.0 * az as f64);
    let reach = (tx - ex).hypot(tz - ez);
    let mut trunks = vec![];
    for (x, z) in (mx - 60..mx + 60).flat_map(|x| (mz - 60..mz + 60).map(move |z| (x, z))) {
        let (vx, vz) = (x as f64 + 0.5 - ex, z as f64 + 0.5 - ez);
        let u = (vx * (tx - ex) + vz * (tz - ez)) / (reach * reach);
        let off = (vx - u * (tx - ex)).hypot(vz - u * (tz - ez));
        if !(0.4..=1.0).contains(&u) || off > 6.0 {
            continue;
        }
        let Some(foot) = (floor - 3..floor + 14).find(|&y| log(w.get(x, y, z))) else { continue };
        let top = (foot..foot + 12).find(|&y| !log(w.get(x, y, z))).unwrap_or(foot);
        if leaves(w.get(x, top, z)) {
            trunks.push((x, z, foot, top));
        }
    }
    for (x, z, foot, top) in trunks {
        for t in foot..top {
            w.set(x, t, z, AIR);
        }
        for (dx, dz, y) in (-3..=3).flat_map(|dx| (-3..=3).flat_map(move |dz| (foot..top + 4).map(move |y| (dx, dz, y)))) {
            if leaves(w.get(x + dx, y, z + dz)) {
                w.set(x + dx, y, z + dz, AIR);
            }
        }
    }
    // the shaft runs deep, so its end is never seen, past its last torch into the dark; but stops short
    // where the rock over it grows thin, so it never opens out of the mountain's side
    // (the face's own slope is thin at the mouth: measured from four blocks in)
    let deep = (4..DEEP).find(|&along| {
        let (x, z) = at(along, 0);
        (-2..=2).any(|side| {
            let (x, z) = (x + side * sx, z + side * sz);
            w.ground(x, z) < floor + 5
        })
    });
    let deep = deep.map_or(DEEP, |d| (d - 3).max(8));
    // the face cut back a little before it, and the shaft, three wide and three high
    for along in -3..deep {
        for side in -3..=3i32 {
            let tunnel = along >= 0 && side.abs() <= 1;
            let cut = along < 0 && side.abs() <= 3;
            if !(tunnel || cut) {
                continue;
            }
            let (x, z) = at(along, side);
            for y in floor..floor + if tunnel { 3 } else { 24 } {
                w.set(x, y, z, AIR);
            }
            w.set(x, floor - 1, z, if tunnel { STONE } else { GRAVEL });
        }
    }
    // the frame at its mouth and every four blocks in
    for along in (0..deep - 1).step_by(4) {
        for side in [-2, 2] {
            let (x, z) = at(along, side);
            for y in floor..floor + 3 {
                w.set(x, y, z, OAK_LOG);
            }
        }
        for side in -2..=2 {
            let (x, z) = at(along, side);
            w.set(x, floor + 3, z, PLANKS);
        }
    }
    for along in -3..deep {
        let (x, z) = at(along, 0);
        w.set(x, floor, z, if ax != 0 { RAIL } else { RAIL_Z });
    }
    // torches: over the mouth on the frame, every twelve blocks inside to well short of its end, and on
    // posts either side of the mouth
    for side in [-2, 2] {
        let (x, z) = at(0, side);
        w.set(x, floor + 4, z, TORCH);
    }
    for along in (3..deep - 12).step_by(12) {
        let (x, z) = at(along, 1);
        w.set(x, floor, z, TORCH);
    }
    for side in [-3, 3] {
        let (x, z) = at(-2, side);
        w.set(x, floor, z, FENCE);
        w.set(x, floor + 1, z, TORCH);
    }
    let (x, z) = at(-1, -2);
    w.set(x, floor, z, BARREL);
    let (x, z) = at(-1, 2);
    w.set(x, floor, z, COBBLE);
    w.set(x, floor + 1, z, COAL);
    // and on posts along the mountain's foot either side, wherever the ground there is open
    for side in [-12, -8, 8, 12] {
        let (x, z) = at(-2, side);
        let g = w.ground(x, z);
        if (g - floor).abs() <= 2 && w.get(x, g, z) == AIR {
            w.set(x, g, z, FENCE);
            w.set(x, g + 1, z, TORCH);
        }
    }
}

/// A small tunnel low in the mountains on the left, towards the viewer: two wide, two high, rails
/// running out of it onto the slope, a cart on them ready to go; a torch at its mouth and one inside.
fn cart_tunnel(w: &mut World, ahead: (f64, f64)) {
    let p = cart_tunnel_at(ahead);
    let (ax, az) = if p.0.abs() > p.1.abs() { (p.0.signum() as i32, 0) } else { (0, p.1.signum() as i32) };
    let (sx, sz) = (-az, ax);
    let (px, pz) = (p.0.round() as i32, p.1.round() as i32);
    // the foot of the mountain's face, coming from the viewer's side
    let Some((mx, mz, floor)) = (-30..0).find_map(|d| {
        let (x, z) = (px + d * ax, pz + d * az);
        let before = w.ground(x - 4 * ax, z - 4 * az);
        (w.ground(x, z) >= before + 4 && before >= GROUND).then_some((x, z, before))
    }) else {
        return;
    };
    let at = |along: i32, side: i32| (mx + along * ax + side * sx, mz + along * az + side * sz);
    // a yard dug out before it, wide and open to the sky, so its mouth is seen from the side too
    for along in -9..12 {
        for side in -4..=5i32 {
            let tunnel = along >= 0 && (0..=1).contains(&side);
            let yard = along < 0 && side.abs() <= 3 - along / 3;
            if !(tunnel || yard) {
                continue;
            }
            let (x, z) = at(along, side);
            for y in floor..floor + if tunnel { 2 } else { 24 } {
                w.set(x, y, z, AIR);
            }
            w.set(x, floor - 1, z, if tunnel { STONE } else { GRAVEL });
        }
        if along >= -6 {
            let (x, z) = at(along, 0);
            w.set(x, floor, z, if ax != 0 { RAIL } else { RAIL_Z });
        }
    }
    let (x, z) = at(-1, 1);
    w.set(x, floor, z, TORCH);
    let (x, z) = at(6, 1);
    w.set(x, floor, z, TORCH);
    // the cart on the rails before the mouth, along them
    let (x, z) = at(-3, 0);
    let yaw = (ax as f64).atan2(az as f64);
    w.mobs.add(Kind::Cart, [x as f64 + 0.5, floor as f64 + 0.07, z as f64 + 0.5], yaw);
}
