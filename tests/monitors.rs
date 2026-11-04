//! Several monitors: each piece of the header (the hostname, the widgets, the graph, the status line)
//! on the monitors its theme's [monitors] chooses, and the scenery on every one. Worked out for each
//! bundled theme, drawn as still pictures, and run live on a virtual X server of real RandR outputs:
//! monitors of different sizes and heights, plugged in and out while the wall runs.

mod common;

use common::{bundled, resolve, themes_dir, variant};
use retro_wall::config::Theme;
use retro_wall::live;
use retro_wall::platform::{Edges, Screen};
use retro_wall::scene::{Geo, Header, Piece};
use std::sync::Arc;

/// Two monitors side by side, of different sizes, the left one lower: as in the live tests.
fn pair() -> [Screen; 2] {
    [
        Screen { name: "DUMMY0".into(), x: 0, y: 60, w: 800, h: 450, primary: false, dpi: 96, mm: (0, 0) },
        Screen { name: "DUMMY1".into(), x: 800, y: 0, w: 960, h: 540, primary: true, dpi: 96, mm: (0, 0) },
    ]
}

/// The pieces named.
fn of(ps: &[Piece]) -> Header {
    ps.iter().fold(Header::NONE, |h, &p| h.with(p))
}

fn union(a: Header, b: Header) -> Header {
    of(&Piece::ALL.into_iter().filter(|&p| a.contains(p) || b.contains(p)).collect::<Vec<_>>())
}

/// What each bundled theme shows on the left monitor and on the right one.
fn expected(theme: &str) -> (Header, Header) {
    match theme {
        // the whole header on the left; the right one is the painting alone
        "neon_rain" => (Header::ALL, Header::NONE),
        // the hostname and the status line on the left; widgets and graph (it has none) on the right
        "copperworks" => (of(&[Piece::Title, Piece::Status]), of(&[Piece::Widgets, Piece::Graph])),
        // pure wallpapers: no header anywhere
        "blocky" | "forest" | "matrix" | "racing" => (Header::NONE, Header::NONE),
        _ => (Header::NONE, Header::ALL),
    }
}

fn bundled_themes() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(themes_dir())
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("theme.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn theme(name: &str) -> Arc<Theme> {
    resolve(&bundled(name)).unwrap_or_else(|e| panic!("{name}: {e}")).plain
}

#[test]
fn each_bundled_theme_places_its_header() {
    let names = bundled_themes();
    for n in ["phosphor", "neon_rain", "copperworks", "matrix"] {
        assert!(names.iter().any(|t| t == n), "no bundled theme {n}");
    }
    for name in &names {
        let m = &theme(name).monitors;
        let all = pair();
        let got = (live::header(m, "DUMMY0", &all, false), live::header(m, "DUMMY1", &all, false));
        assert_eq!(got, expected(name), "{name}: (left, right)");
        // a third monitor between them shows none of it
        let three = [
            all[0].clone(),
            Screen { name: "MID".into(), x: 800, y: 0, w: 640, h: 480, primary: false, dpi: 96, mm: (0, 0) },
            Screen { x: 1440, ..all[1].clone() },
        ];
        assert_eq!(live::header(m, "MID", &three, false), Header::NONE, "{name}: the middle of three");
        assert_eq!(live::header(m, "DUMMY0", &three, false), got.0, "{name}: the left of three");
        assert_eq!(live::header(m, "DUMMY1", &three, false), got.1, "{name}: the right of three");
        // stacked: both leftmost and rightmost is the upper one, which shows both sides' pieces
        let stacked = [
            Screen { name: "TOP".into(), x: 0, y: 0, w: 1920, h: 1080, primary: false, dpi: 96, mm: (0, 0) },
            Screen { name: "LOW".into(), x: 0, y: 1080, w: 1920, h: 1080, primary: false, dpi: 96, mm: (0, 0) },
        ];
        assert_eq!(live::header(m, "TOP", &stacked, false), union(got.0, got.1), "{name}: the upper of two stacked");
        assert_eq!(live::header(m, "LOW", &stacked, false), Header::NONE, "{name}: the lower of two stacked");
        // one monitor driven alone shows every piece the theme does not turn off
        let alone = live::header(m, "DUMMY1", &all, true);
        assert_eq!(alone, union(got.0, got.1), "{name}: alone");
    }
}

#[test]
fn a_theme_can_keep_pieces_on_every_monitor() {
    let pair = pair();
    let three = [
        pair[0].clone(),
        Screen { name: "MID".into(), x: 800, y: 0, w: 640, h: 480, primary: false, dpi: 96, mm: (0, 0) },
        Screen { x: 1440, ..pair[1].clone() },
    ];
    let stacked = [
        Screen { name: "TOP".into(), x: 0, y: 0, w: 1920, h: 1080, primary: false, dpi: 96, mm: (0, 0) },
        Screen { name: "LOW".into(), x: 0, y: 1080, w: 1920, h: 1080, primary: false, dpi: 96, mm: (0, 0) },
    ];
    let all = "[monitors]\ntitle = \"all\"\nwidgets = \"all\"\ngraph = \"all\"\nstatus = \"all\"\n";
    for base in bundled_themes() {
        // every piece everywhere, whatever the theme placed: on every monitor of every layout, and alone
        let m = &variant(&format!("monitors-all-{base}"), &base, all).unwrap().plain.monitors;
        for layout in [&pair[..], &three[..], &stacked[..]] {
            for s in layout {
                assert_eq!(live::header(m, &s.name, layout, false), Header::ALL, "{base}: {} of {}", s.name, layout.len());
            }
        }
        assert_eq!(live::header(m, "DUMMY0", &pair, true), Header::ALL, "{base}: alone");
    }
    // one piece everywhere, the others where the theme put them: copperworks' hostname on every monitor
    let m = &variant("monitors-title-all", "copperworks", "[monitors]\ntitle = \"all\"\n").unwrap().plain.monitors;
    assert_eq!(live::header(m, "DUMMY0", &three, false), of(&[Piece::Title, Piece::Status]));
    assert_eq!(live::header(m, "MID", &three, false), Header::of(Piece::Title));
    assert_eq!(live::header(m, "DUMMY1", &three, false), of(&[Piece::Title, Piece::Widgets, Piece::Graph]));
    // and drawn on each: the still hostname in every monitor's own picture, at its own size
    let th = variant("monitors-title-all-drawn", "copperworks", "[monitors]\ntitle = \"all\"\n").unwrap().plain;
    for s in &three {
        let h = live::header(&th.monitors, &s.name, &three, false);
        assert!(differ(&still(&th, s, h), &still(&th, s, Header::NONE), s, BOTTOM_LEFT) > 0.005, "copperworks: no hostname on {}", s.name);
    }
}

/// The scene's parts for a monitor showing `h`: (the scenery's count, the header parts' count).
fn parts(th: &Arc<Theme>, s: &Screen, h: Header) -> (usize, usize) {
    let g = Geo::new(s.w, s.h, "testhost", Edges::NONE, h, Some(s.name.clone()), th.clone());
    let made = retro_wall::elements::build(&g);
    retro_wall::elements::built();
    let scenery = made.iter().filter(|p| p.shows() == Header::NONE).count();
    (scenery, made.len() - scenery)
}

#[test]
fn the_scenery_is_on_every_monitor() {
    for name in bundled_themes() {
        let th = theme(&name);
        let [l, r] = pair();
        let (el, er) = expected(&name);
        let (full, header) = parts(&th, &r, Header::ALL);
        assert!(full > 0, "{name}: no scenery");
        let (left, left_header) = parts(&th, &l, el);
        let (right, right_header) = parts(&th, &r, er);
        // copperworks' gears, gauges, tanks, lamps and vents included: they are the painting
        assert_eq!((left, right), (full, full), "{name}: the scenery on the left and the right");
        assert!(left_header <= header && right_header <= header, "{name}");
        assert_eq!(left_header == 0, el == Header::NONE, "{name}: the left monitor's header parts");
        assert_eq!(right_header == 0, er == Header::NONE, "{name}: the right monitor's header parts");
    }
}

/// The still picture of `th` on monitor `s` showing `h`, as (r, g, b) by row.
fn still(th: &Arc<Theme>, s: &Screen, h: Header) -> Vec<[u8; 3]> {
    let g = Geo::new(s.w, s.h, "testhost", Edges::NONE, h, Some(s.name.clone()), th.clone());
    let mut pic = retro_wall::still::draw(&g).expect("a still picture");
    let (w, h, stride) = (pic.width() as usize, pic.height() as usize, pic.stride() as usize);
    let d = pic.data().unwrap();
    let at = |y: usize, x: usize| y * stride + x * 4;
    (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).map(|(y, x)| [d[at(y, x) + 2], d[at(y, x) + 1], d[at(y, x)]]).collect()
}

/// Of the pixels in the part (x0, y0, x1, y1) of the picture (fractions of its size), the share that
/// differ by more than 24 in a channel.
fn differ(a: &[[u8; 3]], b: &[[u8; 3]], s: &Screen, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> f64 {
    let (w, h) = (s.w as f64, s.h as f64);
    let (xs, ys) = ((x0 * w) as usize..(x1 * w) as usize, (y0 * h) as usize..(y1 * h) as usize);
    let n = xs.len() * ys.len();
    let d = ys.flat_map(|y| xs.clone().map(move |x| y * s.w as usize + x)).filter(|&i| a[i].iter().zip(b[i]).any(|(p, q)| p.abs_diff(q) > 24)).count();
    d as f64 / n as f64
}

/// Parts of a picture, as fractions of its size: (x0, y0, x1, y1).
type Part = (f64, f64, f64, f64);
const WHOLE: Part = (0.0, 0.0, 1.0, 1.0);
/// Where a bottom-left hostname stands.
const BOTTOM_LEFT: Part = (0.0, 0.6, 0.5, 1.0);

#[test]
fn each_piece_is_drawn_only_where_it_is_placed() {
    let [l, r] = pair();
    // copperworks: the hostname on the left monitor; the right one is the bare machine, pixel for pixel
    let th = theme("copperworks");
    let (el, er) = expected("copperworks");
    assert_eq!(differ(&still(&th, &r, er), &still(&th, &r, Header::NONE), &r, WHOLE), 0.0, "copperworks: the right monitor is not the bare machine");
    assert!(differ(&still(&th, &l, el), &still(&th, &l, Header::NONE), &l, BOTTOM_LEFT) > 0.005, "copperworks: no hostname on the left monitor");

    // neon rain: everything on the left, and all of it live (the tube lettering flickers); the still
    // picture is the painting alone on both. live_on_two_monitors sees it drawn
    let th = theme("neon_rain");
    let (el, er) = expected("neon_rain");
    assert_eq!(differ(&still(&th, &r, er), &still(&th, &r, Header::NONE), &r, WHOLE), 0.0, "neon rain: the right monitor is not the painting alone");
    assert_eq!(differ(&still(&th, &l, el), &still(&th, &l, Header::NONE), &l, WHOLE), 0.0, "neon rain: the left still picture is not the painting alone");

    // phosphor: everything on the right
    let th = theme("phosphor");
    let (el, er) = expected("phosphor");
    assert_eq!(differ(&still(&th, &l, el), &still(&th, &l, Header::NONE), &l, WHOLE), 0.0, "phosphor: the left monitor is not the bare scene");
    assert!(differ(&still(&th, &r, er), &still(&th, &r, Header::NONE), &r, WHOLE) > 0.01, "phosphor: no header on the right monitor");

    // the status line apart from the widgets: phosphor's on the left, its other pieces on the right. It
    // is live only: neither still picture changes for it
    let th = variant("monitors-status-apart", "phosphor", "[monitors]\nstatus = \"leftmost\"\n").unwrap().plain;
    let all = pair();
    let (hl, hr) = (live::header(&th.monitors, "DUMMY0", &all, false), live::header(&th.monitors, "DUMMY1", &all, false));
    assert_eq!((hl, hr), (Header::of(Piece::Status), of(&[Piece::Title, Piece::Widgets, Piece::Graph])));
    assert_eq!(differ(&still(&th, &l, hl), &still(&th, &l, Header::NONE), &l, WHOLE), 0.0, "phosphor: the status line in the left still picture");
    assert_eq!(differ(&still(&th, &r, hr), &still(&th, &r, Header::ALL), &r, WHOLE), 0.0, "phosphor: the right still picture changed");
}

#[cfg(target_os = "linux")]
mod live_x {
    use super::*;
    use common::procs::scratch;
    use common::x::{Dump, Monitors, Output};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    /// Where a status line in the bottom right corner stands, clear of a bottom-left hostname.
    const CORNER: Part = (0.8, 0.8, 1.0, 1.0);
    /// Where widgets down the left from the top stand.
    const TOP_LEFT: Part = (0.0, 0.0, 0.3, 0.5);
    /// Where a top-right hostname and the widgets under it stand.
    const TOP_RIGHT: Part = (0.5, 0.0, 1.0, 0.4);

    const LEFT: Output = ("DUMMY0", 0, 60, 800, 450);
    const RIGHT: Output = ("DUMMY1", 800, 0, 960, 540);

    /// A part of a monitor, in the desktop's pixels.
    fn rect(o: Output, (x0, y0, x1, y1): Part) -> (i32, i32, i32, i32) {
        let (w, h) = (o.3 as f64, o.4 as f64);
        (o.1 + (x0 * w) as i32, o.2 + (y0 * h) as i32, ((x1 - x0) * w) as i32, ((y1 - y0) * h) as i32)
    }

    /// A wall on every monitor, its output lines on a channel; killed when dropped.
    struct Wall {
        child: Child,
        said: Receiver<String>,
        seen: Vec<String>,
    }

    impl Wall {
        fn start(x: &Monitors, home: &std::path::Path, args: &[&str]) -> Wall {
            let mut c = x.retro_wall(home);
            c.args(["--software", "--all-monitors", "--demo", "12:00", "--ip-url", "http://127.0.0.1:9/", "--host", "testhost"]).args(args);
            let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("the binary runs");
            let (tx, said) = channel();
            for out in [Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>, Box::new(child.stderr.take().unwrap())] {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                });
            }
            Wall { child, said, seen: vec![] }
        }

        /// Wait up to `secs` for a line that starts with `start` and holds `holds`, not taken before;
        /// the line, taken.
        fn take(&mut self, start: &str, holds: &str, secs: f64) -> String {
            let end = Instant::now() + retro_wall::util::seconds(secs);
            loop {
                while let Ok(l) = self.said.try_recv() {
                    self.seen.push(l);
                }
                if let Some(i) = self.seen.iter().position(|l| l.starts_with(start) && l.contains(holds)) {
                    return self.seen.remove(i);
                }
                assert!(Instant::now() < end, "no line {start:?} with {holds:?} in {secs} s; said:\n{}", self.seen.join("\n"));
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        fn closed(&mut self, output: &str) {
            self.take(&format!("{output}: "), "changed or gone: closed", 20.0);
        }

        /// The header a monitor was made with, from its line: "title, status" or "no header".
        fn made(&mut self, output: &str) -> String {
            let line = self.take(&format!("{output}: "), ", showing ", 60.0);
            let showing = line.split_once(", showing ").and_then(|(_, r)| r.split_once(';')).map(|(h, _)| h.to_string());
            showing.unwrap_or_else(|| panic!("{output}: no header in {line:?}"))
        }
    }

    impl Drop for Wall {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// A theme of `base`'s with its scene cut to `scene` (a still picture and the header's parts, so
    /// that only the header moves) and `more` added, written to `dir`; and the same bare, every piece
    /// of the header off.
    fn trimmed(dir: &std::path::Path, base: &str, scene: &str, more: &str) -> (String, String) {
        let write = |name: &str, extra: &str| {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("theme.toml"), format!("extends = \"{}\"\nscene = {scene}\n{extra}", bundled(base))).unwrap();
            d.display().to_string()
        };
        let off = "[monitors]\ntitle = \"none\"\nwidgets = \"none\"\ngraph = \"none\"\nstatus = \"none\"\n";
        (write("with", more), write("bare", off))
    }

    /// The desktop once both monitors are made and settled.
    fn shown(x: &Monitors, home: &std::path::Path, theme: &str, made: &mut Vec<(String, String)>) -> Dump {
        let mut wall = Wall::start(x, home, &["--theme", theme]);
        let mut got = vec![];
        for o in [LEFT, RIGHT] {
            got.push((o.0.to_string(), wall.made(o.0)));
        }
        *made = got;
        // the instruments fade in; then what the header draws stands still enough to compare
        std::thread::sleep(Duration::from_secs(4));
        x.dump()
    }

    #[test]
    fn live_on_two_monitors() {
        let Some(x) = Monitors::start("live_on_two_monitors", &[LEFT, RIGHT]) else { return };
        let home = scratch("monitors-live");
        let themes = scratch("monitors-themes");
        let phosphor = r#"["sky", "mountains", "title", "instruments"]"#;
        let apart = "[monitors]\nstatus = \"leftmost\"\n";
        let everywhere = "[monitors]\ntitle = \"all\"\nwidgets = \"all\"\ngraph = \"all\"\nstatus = \"all\"\n";
        // a theme, its scene, more of it; what each monitor is made with; and for parts of monitors,
        // whether the header draws there (against the same scene with no header)
        type Case = (&'static str, &'static str, &'static str, (Header, Header), Vec<(Output, Part, bool)>);
        let cases: [Case; 7] = [
            ("phosphor", phosphor, "", expected("phosphor"), vec![(LEFT, WHOLE, false), (RIGHT, TOP_RIGHT, true), (RIGHT, WHOLE, true)]),
            (
                "neon_rain",
                r#"["room", "mountains", "title", "instruments"]"#,
                "",
                expected("neon_rain"),
                vec![(LEFT, BOTTOM_LEFT, true), (LEFT, TOP_LEFT, true), (LEFT, CORNER, true), (RIGHT, WHOLE, false)],
            ),
            // the status line in the left monitor's corner, the hostname in its other; the widgets and the
            // graph on the right, where it has none to draw
            (
                "copperworks",
                r#"["plate", "title", "instruments"]"#,
                "",
                expected("copperworks"),
                vec![(LEFT, BOTTOM_LEFT, true), (LEFT, CORNER, true), (RIGHT, WHOLE, false)],
            ),
            // the status line apart from the rest: alone on the left
            (
                "phosphor",
                phosphor,
                apart,
                (Header::of(Piece::Status), of(&[Piece::Title, Piece::Widgets, Piece::Graph])),
                vec![(LEFT, WHOLE, true), (LEFT, TOP_RIGHT, false), (RIGHT, TOP_RIGHT, true)],
            ),
            // the whole header on every monitor
            ("phosphor", phosphor, everywhere, (Header::ALL, Header::ALL), vec![(LEFT, TOP_RIGHT, true), (RIGHT, TOP_RIGHT, true)]),
            // copperworks' hostname and status line kept on every monitor
            (
                "copperworks",
                r#"["plate", "title", "instruments"]"#,
                "[monitors]\ntitle = \"all\"\nstatus = \"all\"\n",
                (of(&[Piece::Title, Piece::Status]), Header::ALL),
                vec![(LEFT, BOTTOM_LEFT, true), (LEFT, CORNER, true), (RIGHT, BOTTOM_LEFT, true), (RIGHT, CORNER, true)],
            ),
            // one piece on every monitor, the rest on one: neon rain's hostname on both, its widgets,
            // graph and status line on the left only
            (
                "neon_rain",
                r#"["room", "mountains", "title", "instruments"]"#,
                "[monitors]\ntitle = \"all\"\nwidgets = \"leftmost\"\ngraph = \"leftmost\"\nstatus = \"leftmost\"\n",
                (Header::ALL, Header::of(Piece::Title)),
                vec![(LEFT, TOP_LEFT, true), (LEFT, CORNER, true), (RIGHT, BOTTOM_LEFT, true), (RIGHT, TOP_LEFT, false), (RIGHT, CORNER, false)],
            ),
        ];
        for (i, (base, scene, more, (want_l, want_r), checks)) in cases.into_iter().enumerate() {
            let (with, bare) = trimmed(&themes.join(i.to_string()), base, scene, more);
            let mut made = vec![];
            let on = shown(&x, &home, &with, &mut made);
            let want = vec![("DUMMY0".to_string(), want_l.to_string()), ("DUMMY1".to_string(), want_r.to_string())];
            assert_eq!(made, want, "{base} {more}: the header of each monitor");
            let off = shown(&x, &home, &bare, &mut made);
            assert!(made.iter().all(|(_, h)| h == "no header"), "{base}: bare: {made:?}");
            let (on_png, off_png) = (themes.join(format!("{i}-{base}-on.png")), themes.join(format!("{i}-{base}-off.png")));
            on.save(&on_png);
            off.save(&off_png);
            let look = format!("the desktop: {} and, with no header, {}", on_png.display(), off_png.display());
            for (o, part, draws) in checks {
                let d = on.differ(&off, rect(o, part));
                if draws {
                    assert!(d > 0.0005, "{base} {more}: {} {part:?}: no header drawn ({d} of it differs from the bare scene); {look}", o.0);
                } else {
                    assert!(d < 0.0001, "{base} {more}: {} {part:?}: a header drawn ({d} of it differs from the bare scene); {look}", o.0);
                }
            }
        }
    }

    #[test]
    fn the_header_follows_monitors_plugged_in_and_out() {
        let Some(x) = Monitors::start("the_header_follows_monitors_plugged_in_and_out", &[LEFT, RIGHT]) else { return };
        let home = scratch("monitors-plug");
        // neon rain's header is on the leftmost: unplugged, it moves to the one left; plugged in, back
        let mut wall = Wall::start(&x, &home, &["--theme", "neon_rain"]);
        assert_eq!([wall.made("DUMMY0"), wall.made("DUMMY1")], ["title, widgets, graph, status", "no header"]);
        x.layout(&[RIGHT]);
        wall.closed("DUMMY0");
        wall.closed("DUMMY1");
        assert_eq!(wall.made("DUMMY1"), "title, widgets, graph, status", "the header to the monitor left");
        x.layout(&[LEFT, RIGHT]);
        wall.closed("DUMMY1");
        assert_eq!([wall.made("DUMMY0"), wall.made("DUMMY1")], ["title, widgets, graph, status", "no header"], "the header back on the left");
        drop(wall);
        // copperworks: a monitor plugged in on the right takes the widgets and the graph; its machine
        // runs there too, as on every monitor
        let mut wall = Wall::start(&x, &home, &["--theme", "copperworks"]);
        assert_eq!([wall.made("DUMMY0"), wall.made("DUMMY1")], ["title, status", "widgets, graph"]);
        let third: Output = ("DUMMY2", 1760, 30, 640, 480);
        x.layout(&[LEFT, RIGHT, third]);
        wall.closed("DUMMY1");
        assert_eq!([wall.made("DUMMY1"), wall.made("DUMMY2")], ["no header", "widgets, graph"]);
        std::thread::sleep(Duration::from_secs(3));
        let (a, b) = (x.dump(), {
            std::thread::sleep(Duration::from_secs(2));
            x.dump()
        });
        for o in [LEFT, RIGHT, third] {
            let d = a.differ(&b, rect(o, WHOLE));
            assert!(d > 0.0005, "copperworks: the machine stands still on {} ({d} of it moved in 2 s)", o.0);
        }
    }
}
