//! The pieces that need no theme: config checks, readings, plugin protocol, geometry, random numbers.

use retro_wall::{config, platform, plugins, rng::Rng, scene};
use toml::Value;

#[test]
fn fits_checks_against_the_default() {
    let v = |s: &str| s.parse::<toml::Table>().unwrap()["v"].clone();
    assert!(config::fits(&v("v = 0.5"), &v("v = 1.0"), "t", false).is_ok());
    assert!(config::fits(&v("v = 1"), &v("v = 1.0"), "t", false).is_ok());
    assert!(config::fits(&v("v = \"x\""), &v("v = 1.0"), "t", false).is_err());
    assert!(config::fits(&v("v = \"#abcdef\""), &v("v = \"#000000\""), "t", false).is_ok());
    assert!(config::fits(&v("v = \"#abcde\""), &v("v = \"#000000\""), "t", false).is_err());
    assert!(config::fits(&v("v = [1.0, 2.0, 3.0]"), &v("v = [1.0, 2.0]"), "t", false).is_err());
    assert!(config::fits(&v("v = [1.0, 2.0, 3.0]"), &v("v = [1.0, 2.0]"), "t", true).is_ok());
    assert!(config::fits(&v("v = []"), &v("v = [1.0]"), "t", true).is_err());
}

#[test]
fn readings() {
    assert_eq!(plugins::parse_reading("42").unwrap().value, 42.0);
    assert_eq!(plugins::parse_reading("250").unwrap().value, 100.0);
    assert!(plugins::parse_reading("nan").is_none());
    let r = plugins::parse_reading(r#"{"value": 7, "indicator": true}"#).unwrap();
    assert!(r.indicator && r.value == 7.0);
    assert_eq!(plugins::parse_reading(r#"{"text": "hello"}"#).unwrap().text.as_deref(), Some("hello"));
    assert!(plugins::parse_reading("garbage").is_none());
}

#[test]
fn source_tables() {
    let t = |s: &str| s.parse::<toml::Table>().unwrap()["s"].clone();
    assert!(plugins::check_source("s", &t("s = { command = [\"true\"] }")).is_ok());
    assert!(plugins::check_source("s", &t("s = { command = \"echo 3\", every = 1 }")).is_ok());
    assert!(plugins::check_source("s", &t("s = { command = [] }")).is_err());
    assert!(plugins::check_source("s", &t("s = { command = [\"x\"], every = 0 }")).is_err());
    assert!(plugins::check_source("s", &t("s = { command = [\"x\"], every = \"5\" }")).is_err());
    assert!(plugins::check_source("s", &t("s = { command = [\"x\"], colour = 1 }")).is_err());
    assert!(plugins::check_source("s", &Value::Integer(3)).is_err());
}

/// A face's picture's pixel at (x, y): premultiplied B, G, R, A.
fn pixel(s: &mut cairo::ImageSurface, x: i32, y: i32) -> [u8; 4] {
    let d = retro_wall::util::pixels(s);
    let i = (y * s.stride() + x * 4) as usize;
    [d[i], d[i + 1], d[i + 2], d[i + 3]]
}

#[test]
fn hostile_face_operations_do_not_crash() {
    let th = config::resolve(&format!("{}/themes/phosphor", env!("CARGO_MANIFEST_DIR")), &config::Config::default(), retro_wall::elements::kind_info, false)
        .unwrap()
        .plain;
    let look = plugins::FaceLook::of(&th);
    let ops = |text: &str| -> Vec<serde_json::Value> { serde_json::from_str(text).unwrap() };
    let draw = |text: &str| plugins::face_ops(&ops(text), 64, 64, 1.0, &look);

    // nonsense of every kind: unbalanced restores, numbers out of reach or of the wrong type, a bad
    // colour, unknown operations, values that are not operations
    let pic = draw(
        r##"[{"op": "restore"}, {"op": "restore"}, {"op": "save"},
        {"op": "rect", "x": 1e308, "y": -1e308, "w": "x", "h": null}, {"op": "fill", "colour": "#zzzzzz"},
        {"op": "arc", "x": 0, "y": 0, "r": -5, "from": 0, "to": 1e9}, {"op": "stroke", "width": 1e12},
        {"op": "unknown"}, 17, null]"##,
    )
    .expect("drawn, the nonsense ignored");
    assert_eq!((pic.width(), pic.height()), (64, 64));

    // a rectangle of endless size is clamped to one that covers the picture; a bad colour is white
    let mut pic = draw(r##"[{"op": "rect", "x": -50, "y": -50, "w": 1e308, "h": 1e308}, {"op": "fill", "colour": "#zzzzzz"}]"##).unwrap();
    for (x, y) in [(0, 0), (63, 63), (32, 10)] {
        assert_eq!(pixel(&mut pic, x, y), [255, 255, 255, 255], "at {x},{y}");
    }

    // a stroke's width is held to 100: a line along the top leaves the bottom clear
    let mut pic =
        draw(r##"[{"op": "move", "x": -10, "y": -40}, {"op": "line", "x": 80, "y": -40}, {"op": "stroke", "colour": "#ff0000", "width": 1e12}]"##).unwrap();
    assert_eq!(pixel(&mut pic, 32, 0), [0, 0, 255, 255], "the stroke reaches 50 below its line");
    assert_eq!(pixel(&mut pic, 32, 63), [0, 0, 0, 0], "the stroke is wider than 100");

    // more restores than saves are ignored: the extra one cannot undo the state of the drawing before
    // it, and does not put cairo in error (which would fail the picture)
    assert!(draw(r#"[{"op": "save"}, {"op": "restore"}, {"op": "restore"}, {"op": "restore"}]"#).is_ok());
    // saves without restores are fine, even many
    let saves = format!("[{}]", vec![r#"{"op": "save"}"#; 5000].join(","));
    assert!(draw(&saves).is_ok());
    // a sweep of an arc of endless degrees is held to one turn; a negative radius is its size
    let mut pic = draw(r##"[{"op": "arc", "x": 32, "y": 32, "r": -20, "from": 0, "to": 1e300}, {"op": "fill", "colour": "#00ff00"}]"##).unwrap();
    assert_eq!(pixel(&mut pic, 32, 32), [0, 255, 0, 255]);
    assert_eq!(pixel(&mut pic, 2, 2), [0, 0, 0, 0]);
    // text of an endless size is held to 500 px, and drawn
    assert!(draw(r#"[{"op": "text", "text": "W", "size": 1e300, "x": -100, "y": 400}]"#).is_ok());
    // only the first 10,000 operations are run
    let many = format!(
        "[{}, {}]",
        vec![r#"{"op": "move", "x": 0, "y": 0}"#; 10_000].join(","),
        r##"{"op": "rect", "x": 0, "y": 0, "w": 64, "h": 64}, {"op": "fill", "colour": "#ff0000"}"##
    );
    assert_eq!(pixel(&mut draw(&many).unwrap(), 10, 10), [0, 0, 0, 0]);
}

#[test]
fn disjoint_rectangles_cover_the_same() {
    let r = scene::disjoint(&[(0.0, 0.0, 10.0, 10.0), (5.0, 5.0, 15.0, 15.0)]);
    let area: f64 = r.iter().map(|r| (r.2 - r.0) * (r.3 - r.1)).sum();
    assert_eq!(area, 175.0);
    let r = platform::Rect::new;
    assert!(platform::covers(&[r(0, 0, 10, 10), r(10, 0, 20, 10)], r(0, 0, 20, 10)));
    assert!(!platform::covers(&[r(0, 0, 10, 10)], r(0, 0, 20, 10)));
}

#[test]
fn random_numbers_repeat_and_stay_in_range() {
    let (mut a, mut b) = (Rng::new(7), Rng::new(7));
    for _ in 0..1000 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
    let mut r = Rng::new(1);
    for _ in 0..10_000 {
        let f = r.f64();
        assert!((0.0..1.0).contains(&f));
        assert!((3..=5).contains(&r.int(3, 5)));
        assert!(r.below(7) < 7);
    }
}

#[test]
fn the_svg_cache_lives_in_the_users_cache_directory() {
    use retro_wall::svg::cache_root;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    assert_eq!(cache_root(Some(OsStr::new("/x/cache")), Some(OsStr::new("/home/u"))), Some(PathBuf::from("/x/cache/retro-wall")));
    assert_eq!(cache_root(Some(OsStr::new("relative")), Some(OsStr::new("/home/u"))), Some(PathBuf::from("/home/u/.cache/retro-wall")));
    assert_eq!(cache_root(None, Some(OsStr::new("/home/u"))), Some(PathBuf::from("/home/u/.cache/retro-wall")));
    assert_eq!(cache_root(None, None), None);
}

#[test]
fn paths_parse_or_say_why() {
    use retro_wall::path::{PathData, Seg};
    let p = PathData::parse("M1,2 l3 4 h1 v-1 Q0,0 2,2 T4,4 C1 1 2 2 3 3 S5 5 6 6 z m1 1").unwrap();
    assert_eq!(p.0[0], Seg::Move(1.0, 2.0));
    assert_eq!(p.0[1], Seg::Line(4.0, 6.0));
    assert_eq!(p.0.last(), Some(&Seg::Move(2.0, 3.0)));
    assert!(PathData::parse("").unwrap().0.is_empty());
    for bad in ["M16", "M1,2 L3 Z", "L1,2", "5 5", "M0,0 A1,1 0 0 1 2,2", "M1e999,0", "M0,0 Z 1", "M0,0 C1,1 2,2"] {
        let e = PathData::parse(bad).expect_err(bad);
        assert!(!e.what.is_empty(), "{bad}");
    }
    // what came before the fault is kept, for drawings that show as much as they can
    assert_eq!(PathData::parse("M0,0 L1,1 L2").unwrap_err().done.0.len(), 2);
}

#[test]
fn a_layer_keeps_only_where_it_has_ink() {
    let mut s = retro_wall::util::surface(64, 48);
    assert_eq!(retro_wall::util::ink_box(&s, 0), None);
    {
        let c = cairo::Context::new(&s).unwrap();
        c.rectangle(10.0, 5.0, 20.0, 7.0);
        c.set_source_rgba(1.0, 1.0, 1.0, 0.01);
        let _ = c.fill();
    }
    assert_eq!(retro_wall::util::ink_box(&s, 0), Some((10, 5, 30, 12)));
    assert_eq!(retro_wall::util::ink_box(&s, 12), Some((0, 0, 42, 24)));
    // a shared surface (a cached layer) is measured as well
    let shared = s.clone();
    assert_eq!(retro_wall::util::ink_box(&shared, 0), Some((10, 5, 30, 12)));
    // while anything else holds it, its pixels are not lent out to be read in place
    assert!(retro_wall::util::view(&mut s).is_none());
    drop(shared);
    assert!(retro_wall::util::view(&mut s).is_some());
}

#[test]
fn a_quad_at_rest_shows_its_own_piece_of_the_texture() {
    // a strip of two cells: the left one clear, the right one ink
    let strip = retro_wall::util::surface(20, 10);
    {
        let c = cairo::Context::new(&strip).unwrap();
        c.rectangle(10.0, 0.0, 10.0, 10.0);
        c.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        let _ = c.fill();
    }
    let paint = |u0: f64, u1: f64| {
        let mut out = retro_wall::util::surface(40, 40);
        let q = retro_wall::util::image_quad(0.0, 0.0, 40.0, 40.0, (1.0, 1.0, 1.0), 1.0, (u0, 0.0, u1, 1.0));
        let data: Vec<f32> = q.iter().map(|&v| v as f32).collect();
        retro_wall::util::paint_vertices(&cairo::Context::new(&out).unwrap(), &strip, &data);
        (pixel(&mut out, 5, 20)[3], pixel(&mut out, 35, 20)[3])
    };
    assert_eq!(paint(0.5, 1.0), (255, 255), "the ink cell fills the quad");
    assert_eq!(paint(0.0, 0.5), (0, 0), "the clear cell leaves it clear");
    assert_eq!(paint(0.0, 1.0), (0, 255), "the whole strip, stretched");
}
