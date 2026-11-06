//! The blocks' faces, 16 x 16 pixel art made here from a seed: dirt, grass, stone, ores, logs, leaves,
//! water and the plants, each as a block game paints them, with its smaller levels (mipmaps) for the
//! distance.

use crate::rng::Rng;

/// A face's side, in texels.
pub const N: usize = 16;

pub type Rgba = [f32; 4];

/// A face's picture and its smaller levels: 16, 8, 4, 2, 1 texels across.
pub struct Texture {
    pub levels: Vec<Vec<Rgba>>,
}

impl Texture {
    fn new(px: Vec<Rgba>) -> Texture {
        let mut levels = vec![px];
        let mut n = N;
        while n > 1 {
            let prev = levels.last().unwrap();
            let h = n / 2;
            let mut next = vec![[0.0; 4]; h * h];
            for y in 0..h {
                for x in 0..h {
                    // alpha-weighted, so a leaf's holes do not darken its leaves
                    let mut s = [0.0f32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let p = prev[(2 * y + dy) * n + 2 * x + dx];
                        for (c, v) in s.iter_mut().zip([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]) {
                            *c += v;
                        }
                    }
                    let a = s[3] / 4.0;
                    next[y * h + x] = if s[3] > 0.0 { [s[0] / s[3], s[1] / s[3], s[2] / s[3], a] } else { [0.0; 4] };
                }
            }
            levels.push(next);
            n = h;
        }
        Texture { levels }
    }

    /// The texel at (u, v), 0 to 1 from the top left, on the level `lod` (0: the full picture).
    #[inline]
    pub fn at(&self, u: f32, v: f32, lod: usize) -> Rgba {
        let lod = lod.min(self.levels.len() - 1);
        let n = N >> lod;
        let x = ((u * n as f32) as isize).clamp(0, n as isize - 1) as usize;
        let y = ((v * n as f32) as isize).clamp(0, n as isize - 1) as usize;
        self.levels[lod][y * n + x]
    }

    /// The texel's alpha on the full picture: whether a cutout face has something there.
    #[inline]
    pub fn solid(&self, u: f32, v: f32) -> bool {
        self.at(u, v, 0)[3] > 0.5
    }
}

/// The faces, by index into `Textures::all`.
pub const DIRT: usize = 0;
pub const GRASS_TOP: usize = 1;
pub const GRASS_SIDE: usize = 2;
pub const STONE: usize = 3;
pub const COBBLE: usize = 4;
pub const SAND: usize = 5;
pub const GRAVEL: usize = 6;
pub const OAK_LOG: usize = 7;
pub const OAK_TOP: usize = 8;
pub const BIRCH_LOG: usize = 9;
pub const BIRCH_TOP: usize = 10;
pub const SPRUCE_LOG: usize = 11;
pub const SPRUCE_TOP: usize = 12;
pub const OAK_LEAVES: usize = 13;
pub const BIRCH_LEAVES: usize = 14;
pub const SPRUCE_LEAVES: usize = 15;
pub const WATER: usize = 16;
pub const SNOW: usize = 17;
pub const SNOW_SIDE: usize = 18;
pub const COAL: usize = 19;
pub const IRON: usize = 20;
pub const TALL_GRASS: usize = 21;
pub const POPPY: usize = 22;
pub const DANDELION: usize = 23;
pub const CORNFLOWER: usize = 24;
pub const CANE: usize = 25;
pub const PUMPKIN_SIDE: usize = 26;
pub const PUMPKIN_TOP: usize = 27;
pub const CLAY: usize = 28;
pub const FERN: usize = 29;
pub const ARM: usize = 30;
pub const SNOW_STONE: usize = 31;
pub const PLANKS: usize = 32;
pub const SPRUCE_PLANKS: usize = 33;
pub const GLASS: usize = 34;
pub const DOOR_LOW: usize = 35;
pub const DOOR_HIGH: usize = 36;
pub const FARMLAND: usize = 37;
pub const PATH: usize = 38;
pub const PATH_SIDE: usize = 39;
pub const WHEAT: usize = 40;
pub const WHEAT_YOUNG: usize = 41;
pub const CARROTS: usize = 42;
pub const POTATOES: usize = 43;
pub const BEETROOTS: usize = 44;
pub const TORCH: usize = 45;
pub const HAY_TOP: usize = 46;
pub const HAY_SIDE: usize = 47;
pub const CRAFTING_TOP: usize = 48;
pub const CRAFTING_SIDE: usize = 49;
pub const BARREL_TOP: usize = 50;
pub const BARREL_SIDE: usize = 51;
pub const RAIL: usize = 52;
pub const LANTERN: usize = 53;
pub const LILY: usize = 54;
pub const CAMPFIRE: usize = 55;
pub const OAK_LYING: usize = 56;
/// Rails running along x; `RAIL` runs along z.
pub const RAIL_X: usize = 57;
pub const TORCH_TOP: usize = 58;
pub const LANTERN_TOP: usize = 59;
pub const CHIMNEY_TOP: usize = 60;

pub struct Textures {
    pub all: Vec<Texture>,
}

fn hex(h: u32) -> [f32; 3] {
    [((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0]
}

fn opaque(c: [f32; 3]) -> Rgba {
    [c[0], c[1], c[2], 1.0]
}

fn scaled(c: [f32; 3], k: f32) -> [f32; 3] {
    [(c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0)]
}

/// Smooth noise that tiles over the face, `cells` across.
fn lattice(rng: &mut Rng, cells: usize) -> impl Fn(f32, f32) -> f32 + use<> {
    let g: Vec<f32> = (0..cells * cells).map(|_| rng.f64() as f32).collect();
    move |x: f32, y: f32| {
        let (fx, fy) = (x * cells as f32 / N as f32, y * cells as f32 / N as f32);
        let (x0, y0) = (fx.floor() as usize % cells, fy.floor() as usize % cells);
        let (x1, y1) = ((x0 + 1) % cells, (y0 + 1) % cells);
        let (tx, ty) = (fx.fract(), fy.fract());
        let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
        let a = g[y0 * cells + x0] + (g[y0 * cells + x1] - g[y0 * cells + x0]) * tx;
        let b = g[y1 * cells + x0] + (g[y1 * cells + x1] - g[y1 * cells + x0]) * tx;
        a + (b - a) * ty
    }
}

/// A face of pixels, each one of `shades` (dark to light) by a noise that clusters them a little.
fn speckled(rng: &mut Rng, shades: &[u32], cluster: f32) -> Vec<Rgba> {
    let smooth = lattice(rng, 4);
    let fine = lattice(rng, 8);
    (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32, (i / N) as f32);
            let v = cluster * (0.6 * smooth(x, y) + 0.4 * fine(x, y)) + (1.0 - cluster) * rng.f64() as f32;
            let k = ((v * shades.len() as f32) as usize).min(shades.len() - 1);
            opaque(hex(shades[k]))
        })
        .collect()
}

fn dirt(rng: &mut Rng) -> Vec<Rgba> {
    let mut px = speckled(rng, &[0x6a4a30, 0x79553a, 0x866043, 0x8f6845, 0x9b734d], 0.35);
    // a few dark pebbles and light grains
    for _ in 0..9 {
        let (x, y) = (rng.below(N as u64) as usize, rng.below(N as u64) as usize);
        px[y * N + x] = opaque(hex(if rng.f64() < 0.6 { 0x593d29 } else { 0xb9855c }));
    }
    px
}

const GRASS: [u32; 6] = [0x4f7d2a, 0x5b8c31, 0x679a38, 0x72a63f, 0x7eb147, 0x8abb50];

fn grass_top(rng: &mut Rng) -> Vec<Rgba> {
    let mut px = speckled(rng, &GRASS, 0.3);
    // a few blades' tips catch the light
    for _ in 0..14 {
        let (x, y) = (rng.below(N as u64) as usize, rng.below(N as u64) as usize);
        px[y * N + x] = opaque(hex(if rng.f64() < 0.5 { 0x96c45a } else { 0x46722a }));
    }
    px
}

/// Dirt under a fringe of `top`'s colour that hangs a few pixels down, unevenly.
fn fringed(rng: &mut Rng, dirt: &[Rgba], top: &[Rgba], deep: (u64, u64)) -> Vec<Rgba> {
    let mut px = dirt.to_vec();
    for x in 0..N {
        let hang = 2 + rng.int(deep.0 as i64, deep.1 as i64) as usize;
        for y in 0..hang.min(N) {
            px[y * N + x] = top[y * N + x];
        }
        // the fringe's lowest pixel darker, as the shade under the turf
        if hang < N {
            px[hang * N + x] = opaque(scaled([px[hang * N + x][0], px[hang * N + x][1], px[hang * N + x][2]], 0.82));
        }
    }
    px
}

fn stone(rng: &mut Rng) -> Vec<Rgba> {
    let shades = [0x686868, 0x747474, 0x7f7f7f, 0x888888, 0x949494];
    let streak = lattice(rng, 4);
    let mut px: Vec<Rgba> = (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32, (i / N) as f32);
            // smudges drawn out across, as the face of a cut stone
            let v = 0.55 * streak(x * 0.5, y * 1.6) + 0.45 * rng.f64() as f32;
            opaque(hex(shades[((v * 5.0) as usize).min(4)]))
        })
        .collect();
    for _ in 0..6 {
        let (x, y) = (rng.below(N as u64) as usize, rng.below(N as u64) as usize);
        let len = 2 + rng.below(3) as usize;
        for k in 0..len {
            px[y * N + (x + k) % N] = opaque(hex(0x5f5f5f));
        }
    }
    px
}

/// Stone with clusters of an ore in it.
fn ore(rng: &mut Rng, base: &[Rgba], colours: &[u32], clusters: usize) -> Vec<Rgba> {
    let mut px = base.to_vec();
    for _ in 0..clusters {
        let (cx, cy) = (2 + rng.below(12) as i32, 2 + rng.below(12) as i32);
        for _ in 0..5 {
            let (x, y) = (cx + rng.int(-1, 1) as i32, cy + rng.int(-1, 1) as i32);
            let c = hex(*rng.pick(colours));
            px[(y as usize) * N + x as usize] = opaque(c);
        }
    }
    px
}

fn cobble(rng: &mut Rng) -> Vec<Rgba> {
    // stones are the cells about scattered points, each lit from its top left, the mortar dark
    let pts: Vec<(f32, f32, f32)> = (0..11).map(|_| (rng.f64() as f32 * 16.0, rng.f64() as f32 * 16.0, rng.f64() as f32)).collect();
    (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32 + 0.5, (i / N) as f32 + 0.5);
            let mut d = [(f32::MAX, 0usize); 2];
            for (k, p) in pts.iter().enumerate() {
                for (ox, oy) in [(0.0, 0.0), (16.0, 0.0), (-16.0, 0.0), (0.0, 16.0), (0.0, -16.0), (16.0, 16.0), (-16.0, -16.0), (16.0, -16.0), (-16.0, 16.0)] {
                    let (dx, dy) = (x - p.0 - ox, y - p.1 - oy);
                    let dd = (dx * dx + dy * dy).sqrt();
                    if dd < d[0].0 {
                        d[1] = d[0];
                        d[0] = (dd, k);
                    } else if dd < d[1].0 {
                        d[1] = (dd, k);
                    }
                }
            }
            let edge = d[1].0 - d[0].0;
            let tone = pts[d[0].1].2;
            let c = if edge < 1.0 {
                0x4a4a4a
            } else if edge < 2.0 {
                0x6b6b6b
            } else if tone < 0.33 {
                0x7a7a7a
            } else if tone < 0.66 {
                0x8a8a8a
            } else {
                0x9c9c9c
            };
            opaque(scaled(hex(c), 0.94 + 0.12 * rng.f64() as f32))
        })
        .collect()
}

fn sand(rng: &mut Rng) -> Vec<Rgba> {
    speckled(rng, &[0xcfc28a, 0xd5c792, 0xdbd3a0, 0xe0d8a8, 0xe7e0b6], 0.25)
}

fn gravel(rng: &mut Rng) -> Vec<Rgba> {
    let mut px = speckled(rng, &[0x5e5856, 0x77716e, 0x857f7c, 0x948e8a, 0xa8a29e], 0.1);
    for _ in 0..10 {
        let (x, y) = (rng.below(15) as usize, rng.below(15) as usize);
        let c = opaque(hex(*rng.pick(&[0x6b5e55, 0x9a8f8a, 0x504a48])));
        px[y * N + x] = c;
        px[y * N + x + 1] = c;
        px[(y + 1) * N + x] = c;
    }
    px
}

/// Bark: ridges down the log with dark grooves between them, each groove wandering a pixel now and
/// then, the ridge's edge beside it lit; a knot or two.
fn ridged(rng: &mut Rng, shades: &[u32]) -> Vec<Rgba> {
    let mut px: Vec<Rgba> = (0..N * N).map(|_| opaque(hex(shades[[1, 2, 2, 2, 3][rng.below(5) as usize].min(shades.len() - 1)]))).collect();
    for start in [1usize, 5, 9, 13] {
        let mut x = start as i64;
        for y in 0..N {
            if rng.f64() < 0.18 {
                x += rng.int(-1, 1);
            }
            let gx = x.rem_euclid(N as i64) as usize;
            px[y * N + gx] = opaque(hex(shades[0]));
            px[y * N + (gx + 1) % N] = opaque(hex(shades[shades.len() - 1]));
            px[y * N + (gx + N - 1) % N] = opaque(hex(shades[1]));
        }
    }
    for _ in 0..2 {
        let (x, y) = (rng.below(14) as usize, rng.below(14) as usize);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            px[(y + dy) * N + x + dx] = opaque(hex(shades[0]));
        }
        px[y * N + x] = opaque(hex(shades[1]));
    }
    px
}

/// The same picture turned a quarter, for a log lying down: its grain along the log.
fn turned(px: &[Rgba]) -> Vec<Rgba> {
    (0..N * N).map(|i| px[(i % N) * N + (N - 1 - i / N)]).collect()
}

/// Bark: runs of colour down the log, each column with its own tone, broken now and then.
fn bark(rng: &mut Rng, shades: &[u32], marks: Option<(&[u32], usize)>) -> Vec<Rgba> {
    if marks.is_none() {
        return ridged(rng, shades);
    }
    let mut px = vec![[0.0; 4]; N * N];
    for x in 0..N {
        let mut k = rng.below(shades.len() as u64) as usize;
        let mut y = 0;
        while y < N {
            let run = 2 + rng.below(6) as usize;
            for yy in y..(y + run).min(N) {
                px[yy * N + x] = opaque(hex(shades[k]));
            }
            y += run;
            k = (k as i64 + rng.int(-1, 1)).clamp(0, shades.len() as i64 - 1) as usize;
        }
    }
    if let Some((colours, count)) = marks {
        // a birch's dark marks: short dashes across
        for _ in 0..count {
            let (x, y) = (rng.below(N as u64) as usize, rng.below(N as u64) as usize);
            let len = 1 + rng.below(4) as usize;
            for k in 0..len {
                px[y * N + (x + k) % N] = opaque(hex(*rng.pick(colours)));
            }
        }
    }
    px
}

/// A log's end: rings about its middle, the bark round its edge.
fn rings(rng: &mut Rng, rings: &[u32], bark: &[Rgba]) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if x == 0 || y == 0 || x == N - 1 || y == N - 1 {
                return bark[i];
            }
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let r = dx.abs().max(dy.abs()) * 0.7 + (dx * dx + dy * dy).sqrt() * 0.3;
            let k = ((r + 0.3 * rng.f64() as f32) as usize / 2) % rings.len();
            opaque(hex(rings[k]))
        })
        .collect()
}

/// Leaves: clusters of light and dark, gaps between them through which the light shows.
fn leaves(rng: &mut Rng, shades: &[u32], holes: f64) -> Vec<Rgba> {
    let blob = lattice(rng, 4);
    (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32, (i / N) as f32);
            if rng.f64() < holes {
                return [0.0; 4];
            }
            let v = 0.5 * blob(x, y) + 0.5 * rng.f64() as f32;
            opaque(hex(shades[((v * shades.len() as f32) as usize).min(shades.len() - 1)]))
        })
        .collect()
}

fn water(rng: &mut Rng) -> Vec<Rgba> {
    let wave = lattice(rng, 4);
    (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32, (i / N) as f32);
            let v = wave(x * 1.3, y * 0.6) * 0.7 + 0.3 * rng.f64() as f32;
            let c = if v > 0.72 {
                0x4f86e8
            } else if v > 0.5 {
                0x3f76e4
            } else if v > 0.3 {
                0x3768cf
            } else {
                0x2f5bbd
            };
            [hex(c)[0], hex(c)[1], hex(c)[2], 1.0]
        })
        .collect()
}

fn snow(rng: &mut Rng) -> Vec<Rgba> {
    speckled(rng, &[0xdde8ea, 0xe6eff0, 0xedf4f5, 0xf4f9f9, 0xfbfdfd], 0.4)
}

/// A plant seen flat: blades or a stem from the foot, transparent about them.
fn blades(rng: &mut Rng, shades: &[u32], count: usize, tall: (usize, usize)) -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    for _ in 0..count {
        let mut x = 1.0 + rng.f64() as f32 * 14.0;
        let lean = (rng.f64() as f32 - 0.5) * 0.5;
        let h = rng.int(tall.0 as i64, tall.1 as i64) as usize;
        for k in 0..h {
            let y = N - 1 - k;
            let xi = (x.round() as usize).min(N - 1);
            // lighter towards the tip
            let s = ((k as f32 / h as f32 * shades.len() as f32) as usize).min(shades.len() - 1);
            px[y * N + xi] = opaque(hex(shades[s]));
            x = (x + lean).clamp(0.0, 15.0);
        }
    }
    px
}

/// A flower: a stem with two leaves, its head a few pixels of `petals` about a `heart`.
fn flower(rng: &mut Rng, petals: &[u32], heart: u32, head_at: usize) -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    let stem = [0x3d7a1c, 0x4c8c26];
    for y in head_at..N {
        px[y * N + 7] = opaque(hex(stem[y % 2]));
    }
    for (x, y) in [(6, 12), (5, 11), (8, 10), (9, 9), (6, 13)] {
        px[y * N + x] = opaque(hex(0x5c9e32));
    }
    let c = head_at as i32 - 2;
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            if dx.abs() + dy.abs() > 3 || (dx.abs() == 2 && dy.abs() == 2) {
                continue;
            }
            let (x, y) = ((7 + dx) as usize, (c + dy) as usize);
            px[y * N + x] = opaque(hex(*rng.pick(petals)));
        }
    }
    px[c as usize * N + 7] = opaque(hex(heart));
    px
}

fn cane(rng: &mut Rng) -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    for &x in &[2usize, 7, 12] {
        let joint = rng.below(4) as usize;
        for y in 0..N {
            let j = (y + joint).is_multiple_of(5);
            for (dx, c) in [(0, 0x8bc35a), (1, 0xa6d36e)] {
                px[y * N + x + dx] = opaque(scaled(hex(c), if j { 0.78 } else { 1.0 }));
            }
        }
        // a leaf off a joint
        let y = 4 + rng.below(6) as usize;
        px[y * N + x + 2] = opaque(hex(0x76a843));
        px[(y - 1) * N + x + 3] = opaque(hex(0x76a843));
    }
    px
}

fn pumpkin_side(rng: &mut Rng) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let x = i % N;
            // ribs: a darker groove every few columns
            let groove = matches!(x, 0 | 4 | 5 | 10 | 15);
            let c = if groove { 0xb5600c } else { *rng.pick(&[0xe38a1d, 0xd77e17, 0xe9952a]) };
            opaque(hex(c))
        })
        .collect()
}

fn pumpkin_top(rng: &mut Rng) -> Vec<Rgba> {
    let side = pumpkin_side(rng);
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if (6..=9).contains(&x) && (6..=9).contains(&y) { opaque(hex(if (x + y) % 2 == 0 { 0x5a6b1f } else { 0x4a5a18 })) } else { side[i] }
        })
        .collect()
}

fn clay(rng: &mut Rng) -> Vec<Rgba> {
    speckled(rng, &[0x9096a4, 0x9aa0ad, 0xa1a7b4, 0xa9aebb], 0.5)
}

fn arm(rng: &mut Rng) -> Vec<Rgba> {
    speckled(rng, &[0xb07a62, 0xb98670, 0xc4917a, 0xc99a84], 0.6)
}

/// Boards across the face, their seams dark and their joints staggered.
fn planks(rng: &mut Rng, shades: [u32; 4], seam: u32, joint: u32) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if y % 4 == 3 {
                return opaque(hex(seam));
            }
            let board = y / 4;
            if x == [3, 11, 7, 14][board] {
                return opaque(hex(joint));
            }
            opaque(hex(shades[(rng.below(4) as usize + board) % 4]))
        })
        .collect()
}

/// Glass: a pale frame, clear inside but for a few streaks of light.
fn glass() -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if x == 0 || y == 0 || x == N - 1 || y == N - 1 {
                return [0.86, 0.94, 0.96, 1.0];
            }
            if (x + y == 6 || x + y == 7 || x + y == 20) && (2..14).contains(&x) {
                return [0.95, 0.98, 1.0, 1.0];
            }
            [0.0; 4]
        })
        .collect()
}

/// A door's half: a frame of dark boards about lighter panels; the upper half with two small dark
/// panes.
fn door(rng: &mut Rng, upper: bool) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            let frame = x <= 1 || x >= 14 || (upper && y <= 1) || (!upper && y >= 14) || y == 7 || y == 8;
            if upper && (3..7).contains(&y) && ((3..7).contains(&x) || (9..13).contains(&x)) {
                return opaque(hex(0x2b2d33));
            }
            if frame { opaque(hex(0x6b4f2c)) } else { opaque(scaled(hex(0x9c7a48), 0.92 + 0.12 * rng.f64() as f32)) }
        })
        .collect()
}

fn farmland(rng: &mut Rng) -> Vec<Rgba> {
    let base = speckled(rng, &[0x3a2818, 0x44301e, 0x4e3824, 0x58402a], 0.3);
    // furrows across
    base.into_iter().enumerate().map(|(i, c)| if (i / N).is_multiple_of(4) { opaque(scaled([c[0], c[1], c[2]], 0.72)) } else { c }).collect()
}

fn path_top(rng: &mut Rng) -> Vec<Rgba> {
    speckled(rng, &[0x7e6236, 0x8c6c3e, 0x9a7a48, 0xa88a54], 0.35)
}

/// A crop: stalks from the foot, and its fruit or heads in `accent` about rows `at`.
fn crop(rng: &mut Rng, stalks: &[u32], tall: (usize, usize), accent: &[u32], at: (usize, usize), count: usize) -> Vec<Rgba> {
    let mut px = blades(rng, stalks, 11, tall);
    for _ in 0..count {
        let (x, y) = (1 + rng.below(14) as usize, rng.int(at.0 as i64, at.1 as i64) as usize);
        let c = opaque(hex(*rng.pick(accent)));
        px[y * N + x] = c;
        if rng.f64() < 0.6 {
            px[(y + 1).min(N - 1) * N + x] = c;
        }
    }
    px
}

/// Pixel art from rows of characters, each a colour in its palette ('.' clear), at its top left.
fn drawn(px: &mut [Rgba], x0: usize, y0: usize, rows: &[&str], palette: &[(char, u32)]) {
    for (j, row) in rows.iter().enumerate() {
        for (i, ch) in row.chars().enumerate() {
            if let Some(&(_, c)) = palette.iter().find(|p| p.0 == ch) {
                px[(y0 + j) * N + x0 + i] = opaque(hex(c));
            }
        }
    }
}

/// A torch's sides: a stick two pixels square, lit on its left, and its head at the top: two rows of
/// flame, white-hot over yellow, and an ember's row of orange under them.
fn torch() -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    let rows = ["Wy", "yo", "oe", "hb", "hb", "hb", "hb", "hb", "hb", "db"];
    drawn(&mut px, 7, 6, &rows, &[('W', 0xfff4a8), ('y', 0xffd848), ('o', 0xffa21c), ('e', 0xe4641a), ('h', 0x8f6a2e), ('b', 0x5e4419), ('d', 0x765627)]);
    px
}

/// Its top: the flame seen from above, hottest in a corner.
fn torch_top() -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    drawn(&mut px, 7, 7, &["Wy", "yo"], &[('W', 0xfff4a8), ('y', 0xffd848), ('o', 0xffb02a)]);
    px
}

fn hay(rng: &mut Rng, top: bool) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if !top && (y == 3 || y == 12) {
                return opaque(hex(if x % 2 == 0 { 0x8a5a2a } else { 0x7a4c22 }));
            }
            if top && (x == 0 || y == 0 || x == N - 1 || y == N - 1) {
                return opaque(hex(0xa88a2a));
            }
            let c = *rng.pick(&[0xc8a83a, 0xd4b444, 0xbc9c30, 0xdcc050]);
            // straw runs along the face
            opaque(scaled(hex(c), if (if top { x + y } else { y }) % 3 == 0 { 0.9 } else { 1.0 }))
        })
        .collect()
}

fn crafting(top: bool, plank: &[Rgba]) -> Vec<Rgba> {
    let mut px = plank.to_vec();
    for (i, p) in px.iter_mut().enumerate() {
        let (x, y) = (i % N, i / N);
        if x == 0 || y == 0 || x == N - 1 || y == N - 1 {
            *p = opaque(hex(0x5a4326));
        } else if top && (x == 5 || x == 10 || y == 5 || y == 10) {
            *p = opaque(hex(0x7a5c34));
        }
    }
    if !top {
        // a saw and a hammer hung on the side
        for (x, y) in [(3, 3), (4, 4), (5, 5), (6, 6), (3, 4), (4, 5), (5, 6)] {
            px[y * N + x] = opaque(hex(0xb0b0b0));
        }
        for (x, y) in [(10, 3), (11, 3), (12, 3), (11, 4), (11, 5), (11, 6), (11, 7)] {
            px[y * N + x] = opaque(hex(if y == 3 { 0x8a8a8a } else { 0x6b4f2c }));
        }
    }
    px
}

fn barrel(rng: &mut Rng, top: bool) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if top {
                let r = ((x as f32 - 7.5).abs()).max((y as f32 - 7.5).abs());
                return opaque(hex(if r > 6.5 {
                    0x3e2a16
                } else if r > 5.5 {
                    0x5a4024
                } else {
                    *rng.pick(&[0x6e4d2c, 0x7a5632])
                }));
            }
            if y == 2 || y == 13 {
                return opaque(hex(0x404040));
            }
            if x % 4 == 0 {
                return opaque(hex(0x4a3219));
            }
            opaque(hex(*rng.pick(&[0x654628, 0x6e4d2c, 0x7a5632])))
        })
        .collect()
}

fn rail() -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            if x == 2 || x == 3 || x == 12 || x == 13 {
                opaque(hex(if x == 2 || x == 12 { 0xa8a8a8 } else { 0x7a7a7a }))
            } else if y % 4 == 1 && (1..15).contains(&x) {
                opaque(hex(if y % 8 == 1 { 0x6b4f2c } else { 0x5a4024 }))
            } else {
                [0.0; 4]
            }
        })
        .collect()
}

/// The lantern's iron, dark and its lit edges, and its light, hot in the middle.
const LANTERN_COLOURS: [(char, u32); 7] =
    [('K', 0x24242c), ('k', 0x484854), ('o', 0xe8862a), ('y', 0xffc850), ('W', 0xffeca0), ('d', 0xc86420), ('g', 0x34343e)];

/// A lantern's sides (`world::boxes`): its loop of a handle, a cap, and under it a cage of dark iron, six
/// pixels by seven, its light through its panes.
fn lantern() -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    let rows = [".KKKK.", ".K..K.", ".kkkk.", ".KKKK.", "KkkkkK", "KoyyoK", "KyWWyK", "KyWWyK", "KoyyoK", "KdoodK", "KKggKK"];
    drawn(&mut px, 5, 5, &rows, &LANTERN_COLOURS);
    px
}

/// Its top and bottom: the iron of the cage, the cap within it and the handle across that.
/// A chimney's top: a rim of its cobblestone `stone` about a dark opening, sooty at its edge. The rim
/// a pixel wide: seen from the side, the top is a texel or two high, and a wider rim greyed it out.
fn chimney_top(stone: &[Rgba]) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            let ring = x.min(y).min(N - 1 - x).min(N - 1 - y);
            match ring {
                0 => stone[i],
                1 => opaque(hex(0x2b2b2b)),
                _ => opaque(hex(0x0e0e0e)),
            }
        })
        .collect()
}

fn lantern_top() -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    let rows = ["KKKKKK", "KkkkkK", "KKKKKK", "KKKKKK", "KkkkkK", "KKKKKK"];
    drawn(&mut px, 5, 5, &rows, &LANTERN_COLOURS);
    px
}

fn lily(rng: &mut Rng) -> Vec<Rgba> {
    (0..N * N)
        .map(|i| {
            let (x, y) = (i % N, i / N);
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let r = (dx * dx + dy * dy).sqrt();
            // a round pad with a notch cut to its middle
            if r > 7.0 || (dx > 0.0 && dy.abs() < dx * 0.35) {
                return [0.0; 4];
            }
            opaque(hex(*rng.pick(&[0x2a6a1a, 0x327a20, 0x3c8a28])))
        })
        .collect()
}

/// A campfire's logs; its flames are drawn live, at night only (`life::flames`).
fn campfire() -> Vec<Rgba> {
    let mut px = vec![[0.0; 4]; N * N];
    for y in 12..N {
        for x in 0..N {
            px[y * N + x] = opaque(hex(if (x / 3 + y) % 2 == 0 { 0x5a4024 } else { 0x3a2a16 }));
        }
    }
    px
}

impl Textures {
    pub fn new(seed: i64) -> Textures {
        let mut rng = Rng::new(seed ^ 0x7e57_ab1e);
        let r = &mut rng;
        let dirt_px = dirt(r);
        let grass_px = grass_top(r);
        let stone_px = stone(r);
        let snow_px = snow(r);
        let oak_planks = planks(r, [0x9c7f4e, 0xa8875a, 0xb08f5f, 0xbc9862], 0x6e5531, 0x7d6038);
        let path_px = path_top(r);
        let oak_bark = bark(r, &[0x4c3a22, 0x5a4428, 0x67502f, 0x725a36], None);
        let birch_bark = bark(r, &[0xc9c5bb, 0xd7d4cc, 0xe3e0d8, 0xe9e7e1], Some((&[0x2e2e2e, 0x4a4a44, 0x5d5a52], 9)));
        let spruce_bark = bark(r, &[0x34230f, 0x3e2a14, 0x4a3219, 0x553a1e], None);
        let all = vec![
            Texture::new(dirt_px.clone()),
            Texture::new(grass_px.clone()),
            Texture::new(fringed(r, &dirt_px, &grass_px, (0, 3))),
            Texture::new(stone_px.clone()),
            Texture::new(cobble(r)),
            Texture::new(sand(r)),
            Texture::new(gravel(r)),
            Texture::new(oak_bark.clone()),
            Texture::new(rings(r, &[0xb8945f, 0xa58350, 0x977546, 0xae8c58], &oak_bark)),
            Texture::new(birch_bark.clone()),
            Texture::new(rings(r, &[0xd6c48c, 0xc6b27a, 0xbba56f], &birch_bark)),
            Texture::new(spruce_bark.clone()),
            Texture::new(rings(r, &[0x7a5a34, 0x6b4d2b, 0x5e4225], &spruce_bark)),
            Texture::new(leaves(r, &[0x2f5e12, 0x3a6e18, 0x467e1f, 0x528c26, 0x5f9a2e], 0.22)),
            Texture::new(leaves(r, &[0x4d6e2c, 0x5a7d34, 0x67893c, 0x759646, 0x82a24f], 0.22)),
            Texture::new(leaves(r, &[0x24402a, 0x2c4c31, 0x355a39, 0x3f6742, 0x4a734b], 0.18)),
            Texture::new(water(r)),
            Texture::new(snow_px.clone()),
            Texture::new(fringed(r, &dirt_px, &snow_px, (0, 2))),
            Texture::new(ore(r, &stone_px, &[0x1f1f1f, 0x2e2e2e, 0x3b3b3b], 5)),
            Texture::new(ore(r, &stone_px, &[0xd8af93, 0xc49a7e, 0xe8c8b0, 0xaf8a6e], 4)),
            Texture::new(blades(r, &[0x3f6e1e, 0x4d8226, 0x5c9330, 0x6ca33a, 0x7db246], 22, (5, 13))),
            Texture::new(flower(r, &[0xd12a1f, 0xe6382c, 0xb81c14], 0x231a0e, 6)),
            Texture::new(flower(r, &[0xf2d31b, 0xf8e34a, 0xe0b80f], 0xf8a815, 7)),
            Texture::new(flower(r, &[0x4f6fe0, 0x6b88f0, 0x3d58c4], 0xe8e8ff, 6)),
            Texture::new(cane(r)),
            Texture::new(pumpkin_side(r)),
            Texture::new(pumpkin_top(r)),
            Texture::new(clay(r)),
            Texture::new(blades(r, &[0x2f5f1d, 0x3c7226, 0x4a8430, 0x58943a], 9, (6, 12))),
            Texture::new(arm(r)),
            Texture::new(fringed(r, &stone_px, &snow_px, (0, 2))),
            Texture::new(oak_planks.clone()),
            Texture::new(planks(r, [0x5a3d22, 0x654628, 0x6e4d2c, 0x7a5632], 0x3e2a16, 0x4a3219)),
            Texture::new(glass()),
            Texture::new(door(r, false)),
            Texture::new(door(r, true)),
            Texture::new(farmland(r)),
            Texture::new(path_px.clone()),
            Texture::new(fringed(r, &dirt_px, &path_px, (0, 0))),
            Texture::new(crop(r, &[0x8a7a2a, 0xa8902e, 0xc0a63a, 0xd4bc4a], (9, 14), &[0xa08020, 0xc8a038, 0x8a6a1a], (2, 6), 26)),
            Texture::new(crop(r, &[0x3f7a1e, 0x4d8a26, 0x5c9a30], (4, 8), &[0x6cab3a], (8, 11), 6)),
            Texture::new(crop(r, &[0x3f7a1e, 0x4d8a26, 0x5c9a30, 0x6cab3a], (5, 9), &[0xe8771a, 0xd0620e], (13, 14), 10)),
            Texture::new(crop(r, &[0x3a7020, 0x4a8228, 0x5a9230, 0x6aa23a], (5, 9), &[0xd8c070, 0xc8a858], (13, 14), 8)),
            Texture::new(crop(r, &[0x3a7020, 0x4a8228, 0x5a9230], (5, 9), &[0xa0202a, 0x801828, 0xb83040], (11, 14), 12)),
            Texture::new(torch()),
            Texture::new(hay(r, true)),
            Texture::new(hay(r, false)),
            Texture::new(crafting(true, &oak_planks)),
            Texture::new(crafting(false, &oak_planks)),
            Texture::new(barrel(r, true)),
            Texture::new(barrel(r, false)),
            Texture::new(rail()),
            Texture::new(lantern()),
            Texture::new(lily(r)),
            Texture::new(campfire()),
            Texture::new(turned(&oak_bark)),
            Texture::new(turned(&rail())),
            Texture::new(torch_top()),
            Texture::new(lantern_top()),
        ];
        let mut all = all;
        all.push(Texture::new(chimney_top(&all[COBBLE].levels[0])));
        Textures { all }
    }
}
