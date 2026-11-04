//! The heads-up display, in the pixels of a block game's interface, each `scale` screen pixels: the
//! hotbar with its tools and blocks, and over it the hearts, the food and the experience
//! bar with its level, which follow readings of the machine.

use super::textures::{self, Textures};
use cairo::Context;

/// A picture as rows of characters, each a colour in its palette ('.' clear).
struct Art<'a> {
    rows: &'a [&'a str],
    palette: &'a [(char, u32, f64)],
}

impl Art<'_> {
    fn paint(&self, c: &Context, x: f64, y: f64, s: f64) {
        for (j, row) in self.rows.iter().enumerate() {
            for (i, ch) in row.chars().enumerate() {
                let Some(&(_, rgb, a)) = self.palette.iter().find(|p| p.0 == ch) else { continue };
                c.set_source_rgba(((rgb >> 16) & 255) as f64 / 255.0, ((rgb >> 8) & 255) as f64 / 255.0, (rgb & 255) as f64 / 255.0, a);
                c.rectangle(x + i as f64 * s, y + j as f64 * s, s, s);
                let _ = c.fill();
            }
        }
    }
}

const HEART: &[&str] = &[".KK...KK.", "KrrK.KrrK", "KrwrKrrrK", "KrrrrrrrK", "KrrrrrrdK", ".KrrrrdK.", "..KrrdK..", "...KdK...", "....K...."];
const HEART_LEFT: &[&str] = &[".KK......", "KrrK.....", "KrwrK....", "KrrrrK...", "KrrrrK...", ".KrrrK...", "..KrrK...", "...KdK...", "....K...."];
const HEART_EMPTY: &[&str] = &[".KK...KK.", "KeeK.KeeK", "KeeeKeeeK", "KeeeeeeeK", "KeeeeeeeK", ".KeeeeeK.", "..KeeeK..", "...KeK...", "....K...."];
const HEART_COLOURS: &[(char, u32, f64)] = &[('K', 0x000000, 1.0), ('r', 0xe01414, 1.0), ('d', 0x9c0a0a, 1.0), ('w', 0xffb4b4, 1.0), ('e', 0x2a1010, 0.75)];

/// A shank, as the game draws it: the meat at the top left, its bone out at the bottom right; the
/// right half of it, for a half; and its dark outline when eaten.
const FOOD: &[&str] = &["..KK.....", ".KrRK....", "KrprbK...", "KRrbmbK..", ".KdDmmK..", "..KdDDK..", "...KKKwK.", "......KWK", "......KK."];
const FOOD_RIGHT: &[&str] = &[".........", "....K....", "....bK...", "....mbK..", "....mmK..", "....DDK..", "....KKwK.", "......KWK", "......KK."];
const FOOD_EMPTY: &[&str] = &["..KK.....", ".KeeK....", "KeeeeK...", "KeeeeeK..", ".KeeeeK..", "..KeeeK..", "...KKKeK.", "......KeK", "......KK."];
const FOOD_COLOURS: &[(char, u32, f64)] = &[
    ('K', 0x000000, 1.0),
    ('r', 0xd42a2a, 1.0),
    ('R', 0xb21818, 1.0),
    ('p', 0xdfb18f, 1.0),
    ('b', 0xb88458, 1.0),
    ('m', 0x9d6d43, 1.0),
    ('d', 0x613c1b, 1.0),
    ('D', 0x7b512d, 1.0),
    ('w', 0xe2d5aa, 1.0),
    ('W', 0xfff7dc, 1.0),
    ('e', 0x282828, 0.9),
];

const SWORD: &[&str] = &[
    "............KKK.",
    "...........KwlK.",
    "..........KwlmK.",
    ".........KwlmK..",
    "........KwlmK...",
    ".......KwlmK....",
    "..KK..KwlmK.....",
    "..KdK.KlmK......",
    "...KdKlmK.......",
    "....KdmK........",
    "....KhKdK.......",
    "...KhK.KdK......",
    "..KhK...KK......",
    ".KbK............",
    ".KK.............",
    "................",
];
const PICKAXE: &[&str] = &[
    "................",
    "....KKKKKK......",
    "...KwllllmK.....",
    "....KKKKlmmK....",
    "........KhmK....",
    ".......KhKmmK...",
    "......KhK.KmK...",
    ".....KhK..KmK...",
    "....KhK....KK...",
    "...KhK..........",
    "..KhK...........",
    ".KbK............",
    ".KK.............",
    "................",
    "................",
    "................",
];
const AXE: &[&str] = &[
    "................",
    "......KKK.......",
    ".....KwllK......",
    "....KwlmmK......",
    "....KlmKhK......",
    ".....KKhKmK.....",
    "......KhK.K.....",
    ".....KhK........",
    "....KhK.........",
    "...KhK..........",
    "..KhK...........",
    ".KbK............",
    ".KK.............",
    "................",
    "................",
    "................",
];
const SHOVEL: &[&str] = &[
    "................",
    "..........KKK...",
    ".........KwllK..",
    "........KwlmmK..",
    "........KlmmK...",
    ".......KKmmK....",
    "......KhKKK.....",
    ".....KhK........",
    "....KhK.........",
    "...KhK..........",
    "..KhK...........",
    ".KbK............",
    ".KK.............",
    "................",
    "................",
    "................",
];
/// Iron's colours, and the stick's.
const TOOL_COLOURS: &[(char, u32, f64)] =
    &[('K', 0x1e1e1e, 1.0), ('w', 0xffffff, 1.0), ('l', 0xd8d8d8, 1.0), ('m', 0x9a9a9a, 1.0), ('d', 0x4a3a1a, 1.0), ('h', 0x896727, 1.0), ('b', 0x5c4418, 1.0)];
const DIAMOND_COLOURS: &[(char, u32, f64)] =
    &[('K', 0x0c2a28, 1.0), ('w', 0xd6fff6, 1.0), ('l', 0x4aeddb, 1.0), ('m', 0x1ea595, 1.0), ('d', 0x4a3a1a, 1.0), ('h', 0x896727, 1.0), ('b', 0x5c4418, 1.0)];
const TORCH: &[&str] = &[
    "................",
    ".......yy.......",
    "......yooy......",
    "......yOOy......",
    ".......hh.......",
    ".......hb.......",
    ".......hb.......",
    ".......hb.......",
    ".......hb.......",
    ".......hb.......",
    ".......hb.......",
    ".......hb.......",
    ".......hb.......",
    ".......bb.......",
    "................",
    "................",
];
const TORCH_COLOURS: &[(char, u32, f64)] = &[('y', 0xffd84a, 1.0), ('o', 0xfff3a0, 1.0), ('O', 0xffffff, 1.0), ('h', 0x896727, 1.0), ('b', 0x5c4418, 1.0)];
const BREAD: &[&str] = &[
    "................",
    "................",
    "................",
    "..........KKK...",
    ".......KKKlllK..",
    "....KKKlllllmK..",
    "..KKlllllmmlmK..",
    ".KlllmmlllmmmK..",
    ".KllmmmlmmmmK...",
    "KlmmmmmmmmdK....",
    "KmmmmmmmddK.....",
    "KmmmmddddK......",
    ".KddddKKK.......",
    "..KKKK..........",
    "................",
    "................",
];
const BREAD_COLOURS: &[(char, u32, f64)] = &[('K', 0x3a2408, 1.0), ('l', 0xe8b860, 1.0), ('m', 0xc08a34, 1.0), ('d', 0x8a5a1c, 1.0)];

/// The interface's digits, five pixels by seven.
const DIGITS: [[&str; 7]; 10] = [
    [".###.", "#...#", "#..##", "#.#.#", "##..#", "#...#", ".###."],
    ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", "#####"],
    [".###.", "#...#", "....#", "..##.", ".#...", "#...#", "#####"],
    [".###.", "#...#", "....#", "..##.", "....#", "#...#", ".###."],
    ["...##", "..#.#", ".#..#", "#...#", "#####", "....#", "....#"],
    ["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
    ["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
    ["#####", "#...#", "....#", "...#.", "..#..", "..#..", "..#.."],
    [".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
    [".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
];

/// The width of a number in interface pixels.
fn number_width(n: u32) -> f64 {
    n.to_string().len() as f64 * 6.0 - 1.0
}

/// A number, its glyphs filled with `rgb`, at (x, y) its top left.
fn number(c: &Context, n: u32, x: f64, y: f64, s: f64, rgb: u32) {
    for (k, ch) in n.to_string().chars().enumerate() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        let palette = [('#', rgb, 1.0)];
        Art { rows: &DIGITS[d], palette: &palette }.paint(c, x + k as f64 * 6.0 * s, y, s);
    }
}

/// A stack's count, as the interface shows it: white, with its shadow down and right.
fn count(c: &Context, n: u32, x: f64, y: f64, s: f64) {
    number(c, n, x + s, y + s, s, 0x3f3f3f);
    number(c, n, x, y, s, 0xffffff);
}

/// Where the HUD's pieces are, in screen pixels.
#[derive(Clone, Copy)]
pub struct Layout {
    pub s: f64,
    /// The hotbar's top left.
    pub bar: (f64, f64),
    /// The rows over the hotbar that change (hearts, food, the bar and its level): top left, size.
    pub status: (f64, f64, f64, f64),
    /// The hotbar's picture with its frame: top left, size.
    pub hotbar: (f64, f64, f64, f64),
}

impl Layout {
    /// On a screen w by h, `bottom` pixels of it at the foot kept by a panel: the HUD stands on the
    /// panel, not under it. Its scale is the whole screen's.
    pub fn new(w: f64, h: f64, bottom: f64, scale: f64) -> Layout {
        let s = if scale > 0.0 { scale } else { (h / 360.0).round().max(1.0) };
        let foot = h - bottom.clamp(0.0, h / 2.0).round();
        let bar = ((w / s / 2.0 - 91.0).floor() * s, foot - 22.0 * s);
        Layout { s, bar, status: (bar.0 - s, foot - 42.0 * s, 184.0 * s, 20.0 * s), hotbar: (bar.0 - s, bar.1 - s, 185.0 * s, 24.0 * s) }
    }
}

/// What the status rows show: health and food in half icons (0 to 20), the bar's fill (0 to 1) and the
/// level.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Status {
    pub health: u32,
    pub food: u32,
    pub xp: f64,
    pub level: u32,
}

/// The hotbar with its items, which never changes, into a picture of its own the size of
/// `Layout::hotbar`, at its top left.
pub fn paint_hotbar(c: &Context, l: &Layout, tex: &Textures) {
    let s = l.s;
    c.translate(-l.hotbar.0, -l.hotbar.1);
    // the hotbar: nine slots in a black frame, each lit at its top and left edges, a grey line within
    // that and a shadow within that, its middle a faint dark glass over the world
    let (x, y) = l.bar;
    let fill = |r: f64, a: f64, x0: f64, y0: f64, w: f64, h: f64| {
        c.set_source_rgba(r, r, r, a);
        c.rectangle(x0, y0, w, h);
        let _ = c.fill();
    };
    fill(0.01, 0.95, x, y, 182.0 * s, s);
    fill(0.01, 0.95, x, y + 21.0 * s, 182.0 * s, s);
    fill(0.01, 0.95, x, y, s, 22.0 * s);
    fill(0.01, 0.95, x + 181.0 * s, y, s, 22.0 * s);
    for k in 0..9 {
        let sx = x + (1.0 + 20.0 * k as f64) * s;
        fill(0.0, 0.28, sx + 3.0 * s, y + 4.0 * s, 15.0 * s, 15.0 * s);
        fill(0.62, 0.95, sx, y + s, 20.0 * s, s);
        fill(0.6, 0.95, sx, y + s, s, 20.0 * s);
        fill(0.36, 0.95, sx + s, y + 2.0 * s, 19.0 * s, s);
        fill(0.36, 0.95, sx + s, y + 2.0 * s, s, 19.0 * s);
        fill(0.05, 0.85, sx + 2.0 * s, y + 3.0 * s, 17.0 * s, s);
        fill(0.05, 0.85, sx + 2.0 * s, y + 3.0 * s, s, 16.0 * s);
        fill(0.49, 0.95, sx + 2.0 * s, y + 19.0 * s, 17.0 * s, s);
        fill(0.49, 0.95, sx + 18.0 * s, y + 3.0 * s, s, 17.0 * s);
        fill(0.37, 0.95, sx + s, y + 20.0 * s, 19.0 * s, s);
        fill(0.37, 0.95, sx + 19.0 * s, y + 2.0 * s, s, 19.0 * s);
    }
    // the items: a diamond sword, iron tools, torches, bread and blocks
    let slot = |k: usize| (x + (3.0 + 20.0 * k as f64) * s, y + 3.0 * s);
    let tools: [(&[&str], &[(char, u32, f64)]); 4] = [(SWORD, DIAMOND_COLOURS), (PICKAXE, DIAMOND_COLOURS), (AXE, TOOL_COLOURS), (SHOVEL, TOOL_COLOURS)];
    for (k, (rows, palette)) in tools.into_iter().enumerate() {
        let (ix, iy) = slot(k);
        Art { rows, palette }.paint(c, ix, iy, s);
    }
    let (ix, iy) = slot(4);
    Art { rows: TORCH, palette: TORCH_COLOURS }.paint(c, ix, iy, s);
    count(c, 64, ix + (17.0 - number_width(64)) * s, iy + 9.0 * s, s);
    let (ix, iy) = slot(5);
    Art { rows: BREAD, palette: BREAD_COLOURS }.paint(c, ix, iy, s);
    count(c, 23, ix + (17.0 - number_width(23)) * s, iy + 9.0 * s, s);
    for (k, faces, n) in [(6, [textures::GRASS_TOP, textures::GRASS_SIDE], 64), (7, [textures::COBBLE; 2], 37)] {
        let (ix, iy) = slot(k);
        cube(c, tex, faces, ix, iy, s);
        count(c, n, ix + (17.0 - number_width(n)) * s, iy + 9.0 * s, s);
    }
    // the selected slot: the empty hand's, framed white
    let sel = 8.0;
    let (fx, fy) = (x + (sel * 20.0 - 1.0) * s, y - s);
    c.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    frame(c, fx, fy, 24.0 * s, 24.0 * s, s);
    c.set_source_rgb(1.0, 1.0, 1.0);
    frame(c, fx + s, fy + s, 22.0 * s, 22.0 * s, s);
    c.set_source_rgb(0.75, 0.75, 0.75);
    frame(c, fx + 2.0 * s, fy + 2.0 * s, 20.0 * s, 20.0 * s, s);
}

/// A frame `t` thick inside the rectangle.
fn frame(c: &Context, x: f64, y: f64, w: f64, h: f64, t: f64) {
    c.rectangle(x, y, w, t);
    c.rectangle(x, y + h - t, w, t);
    c.rectangle(x, y, t, h);
    c.rectangle(x + w - t, y, t, h);
    let _ = c.fill();
}

/// A block as the hotbar shows it: a cube seen from above and to the side, 16 interface pixels across,
/// its top and its two sides each shaded, drawn pixel by pixel on the screen.
fn cube(c: &Context, tex: &Textures, faces: [usize; 2], x: f64, y: f64, s: f64) {
    let size = (16.0 * s).round() as i32;
    let (cx, top) = (size as f64 / 2.0, 1.0 * s);
    // the cube's corners: its top a rhombus a quarter as tall as wide
    let half = size as f64 * 0.44;
    let rise = half * 0.5;
    let side = size as f64 * 0.5;
    for py in 0..size {
        for px in 0..size {
            let (fx, fy) = (px as f64 + 0.5 - cx, py as f64 + 0.5 - top - rise);
            // the top: u along the left edge, v along the right
            let (a, b) = ((fx / half + fy / rise) * 0.5, (-fx / half + fy / rise) * 0.5);
            let (u, v, face, k) = if (-1.0..=0.0).contains(&a) && (-1.0..=0.0).contains(&b) {
                ((a + 1.0) as f32, (b + 1.0) as f32, faces[0], 1.0f32)
            } else if fx < 0.0 && fx >= -half {
                // the left side: across from its outer edge, down from its top edge
                let u = (fx + half) / half;
                let edge = fy - (fx + half) / half * rise;
                let v = edge / side;
                if !(0.0..1.0).contains(&v) {
                    continue;
                }
                (u as f32, v as f32, faces[1], 0.62)
            } else if fx >= 0.0 && fx <= half {
                let u = fx / half;
                let edge = fy - (1.0 - fx / half) * rise;
                let v = edge / side;
                if !(0.0..1.0).contains(&v) {
                    continue;
                }
                (u as f32, v as f32, faces[1], 0.8)
            } else {
                continue;
            };
            let t = tex.all[face].at(u.clamp(0.0, 0.999), v.clamp(0.0, 0.999), 0);
            c.set_source_rgb((t[0] * k) as f64, (t[1] * k) as f64, (t[2] * k) as f64);
            c.rectangle(x + px as f64, y + py as f64, 1.0, 1.0);
            let _ = c.fill();
        }
    }
}

/// The status rows, into a picture of their own the size of `Layout::status`, at its top left.
pub fn paint_status(c: &Context, l: &Layout, st: &Status) {
    let s = l.s;
    // in the rows' own picture: the hotbar's left edge is one interface pixel in
    let x0 = s;
    // hearts on the left, food on the right, both over the bar
    for k in 0..10u32 {
        let hx = x0 + k as f64 * 8.0 * s;
        let rows = if st.health >= 2 * k + 2 { HEART } else { HEART_EMPTY };
        Art { rows, palette: HEART_COLOURS }.paint(c, hx, 3.0 * s, s);
        if st.health == 2 * k + 1 {
            Art { rows: HEART_LEFT, palette: HEART_COLOURS }.paint(c, hx, 3.0 * s, s);
        }
        let fx = x0 + (182.0 - 9.0 - k as f64 * 8.0) * s;
        let rows = if st.food >= 2 * k + 2 { FOOD } else { FOOD_EMPTY };
        Art { rows, palette: FOOD_COLOURS }.paint(c, fx, 3.0 * s, s);
        if st.food == 2 * k + 1 {
            Art { rows: FOOD_RIGHT, palette: FOOD_COLOURS }.paint(c, fx, 3.0 * s, s);
        }
    }
    // the experience bar
    let (bx, by) = (x0, 13.0 * s);
    c.set_source_rgba(0.0, 0.0, 0.0, 0.9);
    c.rectangle(bx, by, 182.0 * s, 5.0 * s);
    let _ = c.fill();
    c.set_source_rgba(0.18, 0.22, 0.16, 0.95);
    c.rectangle(bx + s, by + s, 180.0 * s, 3.0 * s);
    let _ = c.fill();
    let fill = (st.xp.clamp(0.0, 1.0) * 180.0).round();
    if fill > 0.0 {
        c.set_source_rgb(0.5, 1.0, 0.13);
        c.rectangle(bx + s, by + s, fill * s, 3.0 * s);
        let _ = c.fill();
        c.set_source_rgb(0.72, 1.0, 0.45);
        c.rectangle(bx + s, by + s, fill * s, s);
        let _ = c.fill();
        c.set_source_rgb(0.3, 0.7, 0.05);
        c.rectangle(bx + s, by + 3.0 * s, fill * s, s);
        let _ = c.fill();
    }
    // notches every eighteen pixels along it
    c.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    for k in 1..10 {
        c.rectangle(bx + (1.0 + k as f64 * 18.0) * s, by + s, s, 3.0 * s);
    }
    let _ = c.fill();
    // the level over the bar's middle: green, outlined in black
    if st.level > 0 {
        let lx = x0 + ((182.0 - number_width(st.level)) / 2.0).floor() * s;
        let ly = 7.0 * s;
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            number(c, st.level, lx + dx * s, ly + dy * s, s, 0x000000);
        }
        number(c, st.level, lx, ly, s, 0x80ff20);
    }
}
