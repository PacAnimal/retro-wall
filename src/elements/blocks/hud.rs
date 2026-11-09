//! The heads-up display, in the pixels of a block game's interface, each `scale` screen pixels: the
//! hotbar with its tools, and over it the hearts, the food and the experience bar with its level, which
//! follow readings of the machine.

use cairo::Context;

/// A picture as rows of characters, each a colour in its palette ('.' clear).
struct Art<'a> {
    rows: &'a [&'a str],
    palette: &'a [(char, u32, f64)],
}

impl Art<'_> {
    fn paint(&self, c: &Context, x: f64, y: f64, s: f64) {
        self.paint_columns(c, x, y, s, 0..usize::MAX);
    }

    /// Only the columns in `cols`: half an icon.
    fn paint_columns(&self, c: &Context, x: f64, y: f64, s: f64, cols: std::ops::Range<usize>) {
        for (j, row) in self.rows.iter().enumerate() {
            for (i, ch) in row.chars().enumerate().filter(|(i, _)| cols.contains(i)) {
                let Some(&(_, rgb, a)) = self.palette.iter().find(|p| p.0 == ch) else { continue };
                c.set_source_rgba(((rgb >> 16) & 255) as f64 / 255.0, ((rgb >> 8) & 255) as f64 / 255.0, (rgb & 255) as f64 / 255.0, a);
                c.rectangle(x + i as f64 * s, y + j as f64 * s, s, s);
                let _ = c.fill();
            }
        }
    }
}

/// A heart, a glint at its top left and its lower edges shaded; empty, its outline round a dark middle.
const HEART: &[&str] = &[".KK...KK.", "KrpK.KrrK", "KpwrKrrrK", "KrrrrrrrK", "KdrrrrrdK", ".KdrrrdK.", "..KdrdK..", "...KdK...", "....K...."];
const HEART_COLOURS: &[(char, u32, f64)] = &[('K', 0x2c0404, 1.0), ('r', 0xee3a2a, 1.0), ('d', 0xb01c12, 1.0), ('p', 0xff9a8c, 1.0), ('w', 0xffffff, 1.0)];
const HEART_EMPTY: &[(char, u32, f64)] = &[('K', 0x2c0404, 1.0), ('r', 0x3a1414, 0.75), ('d', 0x3a1414, 0.75), ('p', 0x3a1414, 0.75), ('w', 0x3a1414, 0.75)];

/// A drumstick: the meat at the top left, red with a glint over cooked brown, its bone out at the bottom
/// right.
const FOOD: &[&str] = &["..KKK....", ".KRpRK...", "KRpwRRK..", "KRRRRttK.", "KRRRtttK.", ".KRttdK..", "..KKKKWK.", "......KWK", ".......K."];
const FOOD_COLOURS: &[(char, u32, f64)] =
    &[('K', 0x140804, 1.0), ('R', 0xc83828, 1.0), ('p', 0xf07a62, 1.0), ('w', 0xffc4b0, 1.0), ('t', 0xb47a4c, 1.0), ('d', 0x8a5634, 1.0), ('W', 0xe8e4d8, 1.0)];
const FOOD_EMPTY: &[(char, u32, f64)] =
    &[('K', 0x140804, 1.0), ('R', 0x282828, 0.9), ('p', 0x282828, 0.9), ('w', 0x282828, 0.9), ('t', 0x282828, 0.9), ('d', 0x282828, 0.9), ('W', 0x282828, 0.9)];

// The tools, 16 pixels square, lit from the top left: each filled pixel gets the tool's dark outline on
// its open sides when drawn (`item`), so the grids hold the fill alone.
const SWORD: &[&str] = &[
    "................",
    ".............wl.",
    "............wllm",
    "...........wllm.",
    "..........wllm..",
    ".........wllm...",
    "........wllm....",
    ".......wllm.....",
    "..G...wllm......",
    "...G.wllm.......",
    "....Gllm........",
    "....hGG.........",
    "...hb..G........",
    "..hb............",
    ".gG.............",
    ".G..............",
];
const SHOVEL: &[&str] = &[
    "................",
    "...........wl...",
    "..........wllm..",
    "..........llmmn.",
    ".........lllmmn.",
    ".........llmmn..",
    ".........hmnn...",
    "........hb......",
    ".......hb.......",
    "......hb........",
    ".....hb.........",
    "....hb..........",
    "...hb...........",
    "..hb............",
    ".hb.............",
    "................",
];
const PICKAXE: &[&str] = &[
    "................",
    "................",
    "....wwwwwl......",
    "....nnmmmmll....",
    "..........nml...",
    ".........hbnml..",
    "........hb..nml.",
    ".......hb....ml.",
    "......hb.....ml.",
    ".....hb......nm.",
    "....hb.......n..",
    "...hb...........",
    "..hb............",
    ".hb.............",
    "................",
    "................",
];
const AXE: &[&str] = &[
    "................",
    ".........wl.....",
    "........wllm....",
    ".......wlllmhb..",
    "........llmhb...",
    ".......wlmhb....",
    "........nhb.....",
    ".......hbn......",
    "......hb........",
    ".....hb.........",
    "....hb..........",
    "...hb...........",
    "..hb............",
    ".hb.............",
    "................",
    "................",
];
const HOE: &[&str] = &[
    "................",
    "................",
    ".......wwwll....",
    ".......nnmmml...",
    "...........hml..",
    "..........hb.n..",
    ".........hb.....",
    "........hb......",
    ".......hb.......",
    "......hb........",
    ".....hb.........",
    "....hb..........",
    "...hb...........",
    "..hb............",
    "................",
    "................",
];
/// Diamond: the head's four shades, the stick's two, a sword's guard and pommel.
const DIAMOND: &[(char, u32, f64)] = &[
    ('w', 0xeafffb, 1.0),
    ('l', 0x63f0dd, 1.0),
    ('m', 0x25b4a2, 1.0),
    ('n', 0x137468, 1.0),
    ('h', 0xb07a34, 1.0),
    ('b', 0x6a4618, 1.0),
    ('g', 0x2fc4b0, 1.0),
    ('G', 0x137468, 1.0),
];

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
pub fn paint_hotbar(c: &Context, l: &Layout) {
    let s = l.s;
    c.translate(-l.hotbar.0, -l.hotbar.1);
    // the hotbar: nine slots in a black frame, each lit at its top and left edges, a grey line within
    // that and a shadow within that, its middle a dark glass over the world
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
        fill(0.0, 0.55, sx + 3.0 * s, y + 4.0 * s, 15.0 * s, 15.0 * s);
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
    // the tools, all diamond, the rest of the slots empty; none of them selected
    for (k, rows) in [SWORD, SHOVEL, PICKAXE, AXE, HOE].into_iter().enumerate() {
        let (ix, iy) = (x + (3.0 + 20.0 * k as f64) * s, y + 3.0 * s);
        item(c, Art { rows, palette: DIAMOND }, 0x082220, ix, iy, s);
    }
}

/// An item: its picture over an outline in `edge`, one pixel round its filled pixels on their open
/// sides, so it stands out of the world behind the slot's glass.
fn item(c: &Context, art: Art, edge: u32, x: f64, y: f64, s: f64) {
    let filled = |i: i32, j: i32| i >= 0 && j >= 0 && art.rows.get(j as usize).and_then(|r| r.as_bytes().get(i as usize)).is_some_and(|&b| b != b'.');
    c.set_source_rgb(((edge >> 16) & 255) as f64 / 255.0, ((edge >> 8) & 255) as f64 / 255.0, (edge & 255) as f64 / 255.0);
    for j in 0..16 {
        for i in 0..16 {
            if !filled(i, j) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(di, dj)| filled(i + di, j + dj)) {
                c.rectangle(x + i as f64 * s, y + j as f64 * s, s, s);
            }
        }
    }
    let _ = c.fill();
    art.paint(c, x, y, s);
}

/// The status rows, into a picture of their own the size of `Layout::status`, at its top left.
pub fn paint_status(c: &Context, l: &Layout, st: &Status) {
    let s = l.s;
    // in the rows' own picture: the hotbar's left edge is one interface pixel in
    let x0 = s;
    // hearts on the left, food on the right, both over the bar
    for k in 0..10u32 {
        let hx = x0 + k as f64 * 8.0 * s;
        let palette = if st.health >= 2 * k + 2 { HEART_COLOURS } else { HEART_EMPTY };
        Art { rows: HEART, palette }.paint(c, hx, 3.0 * s, s);
        if st.health == 2 * k + 1 {
            Art { rows: HEART, palette: HEART_COLOURS }.paint_columns(c, hx, 3.0 * s, s, 0..5);
        }
        let fx = x0 + (182.0 - 9.0 - k as f64 * 8.0) * s;
        let palette = if st.food >= 2 * k + 2 { FOOD_COLOURS } else { FOOD_EMPTY };
        Art { rows: FOOD, palette }.paint(c, fx, 3.0 * s, s);
        if st.food == 2 * k + 1 {
            Art { rows: FOOD, palette: FOOD_COLOURS }.paint_columns(c, fx, 3.0 * s, s, 4..9);
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
