//! The lettering on the towers: every line of text, every block of small print and every little
//! diagram a face can show, drawn once into one picture of coverage (a byte a texel) with its
//! smaller copies, and read back filtered at whatever size a face shows it.

use crate::rng::Rng;
use crate::util::{ctx, select_mono, show_text, surface, text_extents, view};
use cairo::Context;

/// A line's height in the picture, at full size.
pub const LINE: usize = 32;
/// Entries start on multiples of this, so each smaller copy keeps them apart.
const ALIGN: usize = 32;
const WIDTH: usize = 2048;
/// Smaller copies, each half the last.
const LEVELS: usize = 6;
/// Small diagrams drawn: each kind several times over, each one different.
const DIAGRAMS: usize = 84;

/// The names of directories, as a large company's machine would hold them.
const NAMES: &[&str] = &[
    "ACCOUNTS PAYABLE",
    "ACCOUNTS RECEIV.",
    "ANNUAL REPORTS",
    "AUDIT TRAILS",
    "BACKUP SETS",
    "BALLAST TABLES",
    "BILLING",
    "BOARD MINUTES",
    "BUDGET FORECAST",
    "CARGO MANIFESTS",
    "CLAIMS",
    "COMPLIANCE",
    "CONTRACTS",
    "CORE DUMPS",
    "CREW ROSTERS",
    "CUSTOMS FILINGS",
    "DRILL LOGS",
    "EMPLOYEE FILES",
    "ENGINEERING",
    "ESCROW",
    "EXPORT LICENCES",
    "FLEET STATUS",
    "FREIGHT RATES",
    "FUEL CONTRACTS",
    "GARBAGE",
    "GEOLOGIC SURVEYS",
    "HARBOUR FEES",
    "INSURANCE",
    "INVENTORY",
    "INVOICES",
    "KERNEL",
    "LEDGERS",
    "LEGAL",
    "LOAD SCHEDULES",
    "MAIL SPOOL",
    "MAINTENANCE",
    "MARINE CHARTS",
    "OFFSHORE ACCTS.",
    "OPERATIONS",
    "PAYROLL",
    "PENSIONS",
    "PERMITS",
    "PIPELINE MAPS",
    "PORT AUTHORITY",
    "PROCUREMENT",
    "PROJECTIONS",
    "QUARTERLY",
    "R & D",
    "REFINERY OUTPUT",
    "RESEARCH",
    "ROUTING TABLES",
    "SAFETY RECORDS",
    "SALES",
    "SECURITY",
    "SEISMIC DATA",
    "SHIPPING",
    "SPARE PARTS",
    "STOCK OPTIONS",
    "SUBSIDIARIES",
    "SYSTEM",
    "TANKER STATUS",
    "TAX RETURNS",
    "TELEX ARCHIVE",
    "TENDERS",
    "TRANSFERS",
    "TRAVEL",
    "USER ACCOUNTS",
    "VENDORS",
    "WAREHOUSE",
    "WIRE TRANSFERS",
    // and what every machine like this has somewhere
    "XXX",
    "PORN",
    "NUDE",
    "GIBSON",
    "ZERO COOL",
    "CRASH OVERRIDE",
    "ACID BURN",
    "CEREAL KILLER",
    "LORD NIKON",
    "PHANTOM PHREAK",
    "THE PLAGUE",
    "DA VINCI",
    "ZORG INDUSTRIES",
    "MULTIPASS",
    "FHLOSTON PARADISE",
    "MONDOSHAWAN",
    "WINTERMUTE",
    "TESSIER-ASHPOOL",
    "STRAYLIGHT",
    "CHIBA CITY",
    "FLATLINE",
    "MINITRUE",
    "ROOM 101",
    "THOUGHTCRIME",
    "BIG BROTHER",
    "ZION",
    "NEBUCHADNEZZAR",
    "RED PILL",
    "WHITE RABBIT",
    "TYRELL CORP",
    "NEXUS 6",
    "VOIGHT-KAMPFF",
    "TEARS IN RAIN",
    "SKYNET",
    "CYBERDYNE",
    "T-800",
    "JUDGMENT DAY",
    "HAL 9000",
    "WOPR",
    "JOSHUA",
    "MCP",
    "ENCOM",
    "WEYLAND-YUTANI",
    "NOSTROMO",
    "RIPLEY",
    "LV-426",
    "MU-TH-UR 6000",
    "SPECIAL ORDER 937",
    "SULACO",
    "HADLEY'S HOPE",
];

/// Short words for the columns of figures.
const WORDS: &[&str] = &["DEFAULT", "NULL", "EOF", "ROOT", "TMP", "SWAP", "LOCK", "SYNC", "INIT", "ACK", "VOID", "OK"];

/// The kinds of entry: what a panel of a face shows a row of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Set {
    /// Directory names, each with a triangle after it, in the wide capitals.
    Names,
    /// Figures and short words.
    Figures,
    /// Blocks of small print, square.
    Print,
    /// Small diagrams, square.
    Diagrams,
}

/// Where an entry is in the picture, in texels at full size.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub struct Atlas {
    /// The picture and its smaller copies, a byte a texel.
    levels: Vec<Vec<u8>>,
    widths: Vec<usize>,
    pub names: Vec<Entry>,
    pub figures: Vec<Entry>,
    pub print: Vec<Entry>,
    pub diagrams: Vec<Entry>,
    /// Each set's mean coverage over its entries: what a row of it looks like from far off.
    pub mean: [f64; 4],
}

/// The wide capitals: the typeface stretched across, as the lettering on a big computer's front.
const STRETCH: f64 = 1.45;

fn figures(rng: &mut Rng) -> Vec<String> {
    let mut out = vec![];
    for i in 0..72 {
        let s = match i % 6 {
            0 | 1 => {
                let n = rng.int(4, 8) as usize;
                (0..n).map(|_| *rng.pick(&['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F'])).collect()
            }
            2 => format!("{}", rng.int(100, 9_999_999)),
            3 => rng.pick(WORDS).to_string(),
            4 => "0".repeat(rng.int(5, 8) as usize),
            _ => format!("{:02}:{:02}:{:02}", rng.int(0, 23), rng.int(0, 59), rng.int(0, 59)),
        };
        out.push(s);
    }
    out
}

impl Atlas {
    pub fn new() -> Atlas {
        let mut rng = Rng::new(1995);
        let figs = figures(&mut rng);
        // lay the entries out: lines first, then the squares
        let probe = surface(8, 8);
        let widths_of = |texts: &[String], c: &Context| -> Vec<usize> {
            texts.iter().map(|t| (text_extents(c, t).map(|e| e.x_advance()).unwrap_or(0.0) * STRETCH).ceil() as usize + 8).collect()
        };
        let (name_w, fig_w) = {
            let c = ctx(&probe);
            select_mono(&c, true);
            c.set_font_size(LINE as f64 * 0.78);
            let names: Vec<String> = NAMES.iter().map(|n| n.to_string()).collect();
            (widths_of(&names, &c).into_iter().map(|w| w + LINE).collect::<Vec<_>>(), widths_of(&figs, &c))
        };
        let mut pen = (0usize, 0usize, 0usize); // x, y, the row's height
        let mut place = |w: usize, h: usize| -> Entry {
            let w = w.min(WIDTH);
            if pen.0 + w > WIDTH {
                pen = (0, pen.1 + pen.2, 0);
            }
            let e = Entry { x: pen.0, y: pen.1, w, h };
            pen.0 += w.div_ceil(ALIGN) * ALIGN;
            pen.2 = pen.2.max(h.div_ceil(ALIGN) * ALIGN);
            e
        };
        let names: Vec<Entry> = name_w.iter().map(|&w| place(w, LINE)).collect();
        let figures: Vec<Entry> = fig_w.iter().map(|&w| place(w, LINE)).collect();
        let print: Vec<Entry> = (0..16).map(|_| place(256, 256)).collect();
        let diagrams: Vec<Entry> = (0..DIAGRAMS).map(|_| place(128, 128)).collect();
        let height = (pen.1 + pen.2).div_ceil(ALIGN) * ALIGN;

        let mut pic = surface(WIDTH as i32, height as i32);
        {
            let c = ctx(&pic);
            c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            select_mono(&c, true);
            c.set_font_size(LINE as f64 * 0.78);
            for (e, text) in names.iter().zip(NAMES) {
                let _ = c.save();
                c.translate(e.x as f64 + 2.0, e.y as f64 + LINE as f64 * 0.82);
                c.scale(STRETCH, 1.0);
                c.move_to(0.0, 0.0);
                let _ = show_text(&c, text);
                let _ = c.restore();
                // the triangle after the name
                let (tx, ty, s) = ((e.x + e.w) as f64 - LINE as f64 * 0.7, e.y as f64 + LINE as f64 * 0.52, LINE as f64 * 0.22);
                c.move_to(tx, ty - s);
                c.line_to(tx + s * 1.3, ty);
                c.line_to(tx, ty + s);
                c.close_path();
                let _ = c.fill();
            }
            for (e, text) in figures.iter().zip(&figs) {
                let _ = c.save();
                c.translate(e.x as f64 + 2.0, e.y as f64 + LINE as f64 * 0.82);
                c.scale(STRETCH, 1.0);
                c.move_to(0.0, 0.0);
                let _ = show_text(&c, text);
                let _ = c.restore();
            }
            for e in &print {
                small_print(&c, &mut rng, *e);
            }
            for (i, e) in diagrams.iter().enumerate() {
                diagram(&c, &mut rng, *e, i);
            }
        }
        let base: Vec<u8> = match view(&mut pic) {
            Some(d) => d.chunks(4).map(|p| p[3]).collect(),
            None => vec![0; WIDTH * height],
        };
        let mut levels = vec![base];
        let mut widths = vec![WIDTH];
        let mut h = height;
        for _ in 1..LEVELS {
            let (pw, prev) = (*widths.last().unwrap(), levels.last().unwrap());
            let (nw, nh) = (pw / 2, h / 2);
            let mut next = vec![0u8; nw * nh];
            for y in 0..nh {
                for x in 0..nw {
                    let at = |dx: usize, dy: usize| prev[(2 * y + dy) * pw + 2 * x + dx] as u32;
                    next[y * nw + x] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
                }
            }
            levels.push(next);
            widths.push(nw);
            h = nh;
        }
        let mut a = Atlas { levels, widths, names, figures, print, diagrams, mean: [0.0; 4] };
        a.mean = [Set::Names, Set::Figures, Set::Print, Set::Diagrams].map(|s| {
            let es = a.set(s);
            es.iter().map(|e| a.average(*e)).sum::<f64>() / es.len().max(1) as f64
        });
        a
    }

    pub fn set(&self, s: Set) -> &[Entry] {
        match s {
            Set::Names => &self.names,
            Set::Figures => &self.figures,
            Set::Print => &self.print,
            Set::Diagrams => &self.diagrams,
        }
    }

    /// An entry's mean coverage, 0 to 1.
    fn average(&self, e: Entry) -> f64 {
        let mut sum = 0u64;
        for y in e.y..e.y + e.h {
            sum += self.levels[0][y * WIDTH + e.x..y * WIDTH + e.x + e.w].iter().map(|&v| v as u64).sum::<u64>();
        }
        sum as f64 / (255.0 * (e.w * e.h).max(1) as f64)
    }

    /// The coverage of an entry at (s, t), each 0 to 1 across it, seen `texels` texels to a pixel:
    /// filtered between the two nearest sizes.
    pub fn sample(&self, e: Entry, s: f64, t: f64, texels: f64) -> f64 {
        let l = texels.max(1.0).log2().min((LEVELS - 1) as f64);
        let l0 = l as i64 as usize; // l is never below 0: truncating is flooring, without a call
        let f = l - l0 as f64;
        let a = self.bilinear(e, s, t, l0);
        if f < 0.02 || l0 + 1 >= LEVELS { a } else { a + (self.bilinear(e, s, t, l0 + 1) - a) * f }
    }

    fn bilinear(&self, e: Entry, s: f64, t: f64, l: usize) -> f64 {
        let k = (1usize << l) as f64;
        let (x0, y0, w, h) = (e.x as f64 / k, e.y as f64 / k, (e.w as f64 / k).max(1.0), (e.h as f64 / k).max(1.0));
        let x = (x0 + s.clamp(0.0, 1.0) * w - 0.5).clamp(x0, x0 + w - 1.0);
        let y = (y0 + t.clamp(0.0, 1.0) * h - 0.5).clamp(y0, y0 + h - 1.0);
        // x and y are never below 0: truncating is flooring, without a call (no SSE4.1 assumed)
        let (xi, yi) = (x as i64 as usize, y as i64 as usize);
        let (fx, fy) = (x - xi as f64, y - yi as f64);
        let pw = self.widths[l];
        let rows = self.levels[l].len() / pw;
        let xj = (xi + 1).min((x0 + w - 1.0).max(x0) as i64 as usize).min(pw - 1);
        let yj = (yi + 1).min((y0 + h - 1.0).max(y0) as i64 as usize).min(rows - 1);
        let px = |x: usize, y: usize| self.levels[l][y.min(rows - 1) * pw + x.min(pw - 1)] as f64;
        let top = px(xi, yi) + (px(xj, yi) - px(xi, yi)) * fx;
        let bottom = px(xi, yj) + (px(xj, yj) - px(xi, yj)) * fx;
        (top + (bottom - top) * fy) / 255.0
    }
}

/// A block of small print: lines of figures and words, some short, in columns.
fn small_print(c: &Context, rng: &mut Rng, e: Entry) {
    let _ = c.save();
    c.rectangle(e.x as f64, e.y as f64, e.w as f64, e.h as f64);
    c.clip();
    select_mono(c, false);
    c.set_font_size(9.0);
    let cols = rng.int(1, 3);
    let cw = e.w as f64 / cols as f64;
    for col in 0..cols {
        let mut y = e.y as f64 + 9.0;
        while y < (e.y + e.h) as f64 {
            if rng.f64() < 0.12 {
                y += 10.0; // a blank line between paragraphs
                continue;
            }
            let n = (rng.range(0.3, 1.0) * cw / 6.0) as usize;
            let line: String =
                (0..n).map(|_| if rng.f64() < 0.15 { ' ' } else { *rng.pick(&['0', '1', '7', 'A', 'E', 'F', 'R', 'S', 'X', '.', ':', '/']) }).collect();
            c.move_to(e.x as f64 + col as f64 * cw + 2.0, y);
            let _ = show_text(c, &line);
            y += 10.0;
        }
    }
    let _ = c.restore();
}

/// One of the small diagrams, of kind `i` modulo their number, each drawn differently: a file's
/// icon, bars, a tree, a gauge, a framed button, a grid of cells, a trace, a pie, a scatter of points,
/// a network of nodes, a radar, a stack of progress bars, a column of framed files, a barcode.
fn diagram(c: &Context, rng: &mut Rng, e: Entry, i: usize) {
    use std::f64::consts::TAU;
    let (x, y, s) = (e.x as f64, e.y as f64, e.w as f64);
    let _ = c.save();
    c.rectangle(x, y, s, s);
    c.clip();
    c.set_line_width(rng.range(2.0, 4.0));
    match i % 14 {
        0 => {
            // a file: a page with its corner folded and lines of text
            let (l, r) = (rng.range(18.0, 36.0), rng.range(92.0, 110.0));
            c.move_to(x + l, y + 10.0);
            c.line_to(x + r - 18.0, y + 10.0);
            c.line_to(x + r, y + 28.0);
            c.line_to(x + r, y + 118.0);
            c.line_to(x + l, y + 118.0);
            c.close_path();
            let _ = c.stroke();
            for k in 0..rng.int(5, 9) {
                c.rectangle(x + l + 8.0, y + 36.0 + k as f64 * 10.0, rng.range(15.0, r - l - 16.0), 4.0);
            }
            let _ = c.fill();
        }
        1 => {
            // bars, on an axis
            let n = rng.int(5, 14);
            let w = 116.0 / n as f64;
            for k in 0..n {
                let h = rng.range(10.0, 110.0);
                c.rectangle(x + 6.0 + k as f64 * w, y + 120.0 - h, w * rng.range(0.4, 0.8), h);
            }
            let _ = c.fill();
            c.move_to(x + 4.0, y + 122.0);
            c.line_to(x + 124.0, y + 122.0);
            let _ = c.stroke();
        }
        2 => {
            // a tree of directories
            c.move_to(x + 20.0, y + 10.0);
            c.line_to(x + 20.0, y + 116.0);
            let n = rng.int(4, 8);
            for k in 0..n {
                let yy = y + 14.0 + k as f64 * 100.0 / n as f64;
                let indent = if rng.f64() < 0.3 { 18.0 } else { 0.0 };
                c.move_to(x + 20.0, yy);
                c.line_to(x + 40.0 + indent, yy);
                c.rectangle(x + 44.0 + indent, yy - 5.0, rng.range(25.0, 70.0 - indent), 10.0);
            }
            let _ = c.stroke();
        }
        3 => {
            // a gauge, its scale ticked
            c.arc(x + 64.0, y + 64.0, 50.0, 0.0, TAU);
            let _ = c.stroke();
            for k in 0..12 {
                let a = k as f64 / 12.0 * TAU;
                c.move_to(x + 64.0 + 44.0 * a.cos(), y + 64.0 + 44.0 * a.sin());
                c.line_to(x + 64.0 + 50.0 * a.cos(), y + 64.0 + 50.0 * a.sin());
            }
            let _ = c.stroke();
            c.arc(x + 64.0, y + 64.0, 34.0, -2.4, rng.range(-2.0, 0.9));
            c.set_line_width(10.0);
            let _ = c.stroke();
        }
        4 => {
            // a framed button with a triangle
            let h = rng.range(30.0, 60.0);
            c.rectangle(x + 8.0, y + 64.0 - h / 2.0, 112.0, h);
            let _ = c.stroke();
            let (cx, t) = (x + rng.range(40.0, 88.0), h * 0.25);
            c.move_to(cx - t, y + 64.0 + t);
            c.line_to(cx, y + 64.0 - t);
            c.line_to(cx + t, y + 64.0 + t);
            c.close_path();
            let _ = c.fill();
        }
        5 => {
            // a grid of cells, some filled
            let n = rng.int(4, 8);
            let step = 112.0 / n as f64;
            let fill = rng.range(0.2, 0.6);
            for r in 0..n {
                for k in 0..n {
                    c.rectangle(x + 8.0 + k as f64 * step, y + 8.0 + r as f64 * step, step * 0.72, step * 0.72);
                    if rng.f64() < fill {
                        let _ = c.fill();
                    } else {
                        c.set_line_width(2.0);
                        let _ = c.stroke();
                    }
                }
            }
        }
        6 => {
            // a trace: a waveform over a grid
            c.set_line_width(1.0);
            for k in 0..5 {
                let g = y + 14.0 + k as f64 * 25.0;
                c.move_to(x + 4.0, g);
                c.line_to(x + 124.0, g);
            }
            let _ = c.stroke();
            c.set_line_width(3.0);
            let (f, f2, amp) = (rng.range(0.05, 0.25), rng.range(0.2, 0.6), rng.range(15.0, 40.0));
            for k in 0..=60 {
                let px = x + 4.0 + k as f64 * 2.0;
                let py = y + 64.0 + amp * (k as f64 * f).sin() + amp * 0.35 * (k as f64 * f2).sin();
                if k == 0 { c.move_to(px, py) } else { c.line_to(px, py) }
            }
            let _ = c.stroke();
        }
        7 => {
            // a pie: a whole circle cut into two or three slices, one clear, one white, and the third
            // half white
            let (cx, cy, r) = (x + 64.0, y + 64.0, 48.0);
            let n = rng.int(2, 3) as usize;
            let mut cuts: Vec<f64> = (0..n).map(|_| rng.range(0.25, 1.0)).collect();
            let total: f64 = cuts.iter().sum();
            cuts.iter_mut().for_each(|c| *c *= TAU / total);
            let mut a0 = rng.range(0.0, TAU);
            for (k, sweep) in cuts.into_iter().enumerate() {
                let fill = [0.0, 1.0, 0.5][k];
                if fill > 0.0 {
                    c.set_source_rgba(1.0, 1.0, 1.0, fill);
                    c.move_to(cx, cy);
                    c.arc(cx, cy, r, a0, a0 + sweep);
                    c.close_path();
                    let _ = c.fill();
                    c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                }
                c.move_to(cx, cy);
                c.line_to(cx + r * a0.cos(), cy + r * a0.sin());
                a0 += sweep;
            }
            let _ = c.stroke();
            c.new_sub_path();
            c.arc(cx, cy, r, 0.0, TAU);
            let _ = c.stroke();
        }
        8 => {
            // a scatter of points, with its axes and a fitted line
            c.move_to(x + 8.0, y + 6.0);
            c.line_to(x + 8.0, y + 120.0);
            c.line_to(x + 124.0, y + 120.0);
            let _ = c.stroke();
            let slope = rng.range(-0.8, 0.8);
            for _ in 0..rng.int(15, 40) {
                let px = rng.range(14.0, 120.0);
                let py = (64.0 - slope * (px - 64.0) + rng.normal(0.0, 14.0)).clamp(8.0, 114.0);
                c.rectangle(x + px - 2.0, y + py - 2.0, 4.0, 4.0);
            }
            let _ = c.fill();
            c.set_line_width(1.5);
            c.move_to(x + 12.0, y + 64.0 + slope * 52.0);
            c.line_to(x + 122.0, y + 64.0 - slope * 58.0);
            let _ = c.stroke();
        }
        9 => {
            // a network: nodes, and links between them
            let n = rng.int(5, 10) as usize;
            let nodes: Vec<(f64, f64)> = (0..n).map(|_| (x + rng.range(14.0, 114.0), y + rng.range(14.0, 114.0))).collect();
            c.set_line_width(1.5);
            for k in 1..n {
                let j = rng.below(k as u64) as usize;
                c.move_to(nodes[k].0, nodes[k].1);
                c.line_to(nodes[j].0, nodes[j].1);
            }
            let _ = c.stroke();
            for &(nx, ny) in &nodes {
                c.rectangle(nx - 5.0, ny - 5.0, 10.0, 10.0);
            }
            let _ = c.fill();
        }
        10 => {
            // a radar: rings, a sweep, blips
            c.set_line_width(1.5);
            for r in [16.0, 32.0, 48.0] {
                c.arc(x + 64.0, y + 64.0, r, 0.0, TAU);
                let _ = c.stroke();
            }
            let a = rng.range(0.0, TAU);
            c.set_line_width(3.0);
            c.move_to(x + 64.0, y + 64.0);
            c.line_to(x + 64.0 + 56.0 * a.cos(), y + 64.0 + 56.0 * a.sin());
            let _ = c.stroke();
            for _ in 0..rng.int(2, 6) {
                let (b, r) = (rng.range(0.0, TAU), rng.range(10.0, 50.0));
                c.arc(x + 64.0 + r * b.cos(), y + 64.0 + r * b.sin(), 3.5, 0.0, TAU);
                let _ = c.fill();
            }
        }
        11 => {
            // progress bars, framed, part full
            let n = rng.int(3, 6);
            let h = 100.0 / n as f64;
            for k in 0..n {
                let yy = y + 12.0 + k as f64 * h;
                c.set_line_width(2.0);
                c.rectangle(x + 8.0, yy, 112.0, h * 0.55);
                let _ = c.stroke();
                c.rectangle(x + 11.0, yy + 3.0, rng.range(5.0, 106.0), h * 0.55 - 6.0);
                let _ = c.fill();
            }
        }
        12 => {
            // a column of framed files, waiting
            let n = rng.int(3, 5);
            let h = 112.0 / n as f64;
            for k in 0..n {
                let yy = y + 8.0 + k as f64 * h;
                c.set_line_width(2.5);
                c.rectangle(x + 14.0, yy, 100.0, h * 0.5);
                let _ = c.stroke();
                c.rectangle(x + 30.0, yy + h * 0.18, rng.range(30.0, 68.0), h * 0.16);
                c.rectangle(x + 24.0, yy + h * 0.62, rng.range(40.0, 80.0), h * 0.12);
                let _ = c.fill();
            }
        }
        _ => {
            // a barcode
            let mut px = x + 8.0;
            while px < x + 118.0 {
                let w = rng.range(1.5, 6.0);
                c.rectangle(px, y + 24.0, w, 70.0);
                px += w + rng.range(1.5, 5.0);
            }
            let _ = c.fill();
            for k in 0..8 {
                c.rectangle(x + 10.0 + k as f64 * 14.0, y + 100.0, 9.0, 10.0);
            }
            let _ = c.fill();
        }
    }
    let _ = c.restore();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_has_lettering_and_sampling_stays_in_range() {
        crate::font::register();
        let a = Atlas::new();
        assert_eq!(a.names.len(), NAMES.len());
        for s in [Set::Names, Set::Figures, Set::Print, Set::Diagrams] {
            for e in a.set(s) {
                assert!(a.average(*e) > 0.01, "{s:?} {e:?} is blank");
                for texels in [0.5, 1.0, 3.0, 40.0] {
                    let v = a.sample(*e, 0.37, 0.61, texels);
                    assert!((0.0..=1.0).contains(&v));
                }
            }
        }
    }
}
