//! Every tweak and setting a theme gives is checked when the theme loads: out of its range or of the
//! wrong shape, it is an error; at the edges of its range, and anywhere a hostile value gets through,
//! the theme draws without a panic.

mod common;

use common::*;
use retro_wall::config::core;
use retro_wall::tweaks::{LIMITS, Ranges};
use toml::Value;

/// The numbers in a value, in order.
fn count(v: &Value) -> usize {
    match v {
        Value::Float(_) | Value::Integer(_) => 1,
        Value::Array(a) => a.iter().map(count).sum(),
        _ => 0,
    }
}

/// `v` with its number `i` (in reading order) replaced by `n`, kept an integer where it was one.
fn with_number(v: &Value, i: usize, n: f64) -> Value {
    fn walk(v: &Value, i: &mut isize, n: f64) -> Value {
        match v {
            Value::Float(_) | Value::Integer(_) => {
                *i -= 1;
                if *i != -1 {
                    return v.clone();
                }
                match v {
                    Value::Integer(_) if n.is_finite() && n.fract() == 0.0 => Value::Integer(n as i64),
                    _ => Value::Float(n),
                }
            }
            Value::Array(a) => Value::Array(a.iter().map(|x| walk(x, i, n)).collect()),
            _ => v.clone(),
        }
    }
    walk(v, &mut (i as isize), n)
}

fn tweak(key: &str, v: &Value) -> String {
    format!("[tweaks]\n{key} = {v}\n")
}

/// The bases that take most of the time: a painting, or many textures, or full HD.
const SLOW: &[&str] = &["neon_rain", "winter", "phosphor@full", "copperworks"];

/// Whether a case of a slow base is run: each one in `one_in`, picked by its name so that the slow
/// bases share out the cases between them; every one with RETRO_TEST_FULL set (as the nightly CI run
/// has). The fast bases run every case.
fn sampled(base: &str, case: &str, one_in: u64) -> bool {
    let Some(k) = SLOW.iter().position(|s| *s == base) else { return true };
    std::env::var_os("RETRO_TEST_FULL").is_some()
        || tag("", &[case]).bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)) % one_in == k as u64 % one_in
}

fn tweaks() -> &'static toml::Table {
    core()["tweaks"].as_table().unwrap()
}

#[test]
fn every_tweak_with_a_number_has_a_limit() {
    for (k, v) in tweaks() {
        if count(v) > 0 {
            assert!(LIMITS.iter().any(|(n, _)| n == k), "tweaks.{k} has numbers and no limit");
        }
    }
    for (n, _) in LIMITS {
        assert!(tweaks().contains_key(*n), "a limit for {n}, which is not a tweak");
    }
}

#[test]
fn the_defaults_are_within_their_limits() {
    for (k, v) in tweaks() {
        retro_wall::tweaks::limit(k, v).unwrap_or_else(|e| panic!("tweaks.{k}{e}"));
    }
}

/// Every number of every tweak just past its range, not a number, or endless; and every tweak given a
/// value of another shape.
#[test]
fn tweaks_out_of_range_or_misshapen_are_errors() {
    let mut cases: Vec<(String, Value)> = vec![];
    for (key, limit) in LIMITS {
        let d = &tweaks()[*key];
        let rows = match (limit.rows, d) {
            (0, _) => None,
            (_, Value::Array(a)) => a.first().cloned(),
            _ => None,
        };
        let n = rows.as_ref().map_or_else(|| count(d), count);
        for i in 0..n {
            let (lo, hi) = limit.ranges.of(i);
            let span = (hi - lo).max(1.0);
            for bad in [lo - 0.01 * span - 1e-9, hi + 0.01 * span + 1e-9, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                // past an integer range by a whole one
                let bad = if matches!(d, Value::Integer(_)) && bad.is_finite() { if bad < lo { lo - 1.0 } else { hi + 1.0 } } else { bad };
                let v = match &rows {
                    Some(row) => Value::Array(vec![with_number(row, i, bad)]),
                    None => with_number(d, i, bad),
                };
                cases.push((key.to_string(), v));
            }
        }
        if limit.rows > 0
            && let Some(row) = &rows
        {
            cases.push((key.to_string(), Value::Array(vec![row.clone(); limit.rows + 1])));
        }
        cases.push((key.to_string(), Value::String("x".into())));
        cases.push((key.to_string(), Value::Array(vec![])));
        cases.push((key.to_string(), Value::Array(vec![d.clone()])));
        cases.push((key.to_string(), Value::Boolean(true)));
    }
    assert!(cases.len() > 500);
    let failed = each_parallel(
        &cases,
        |(k, v)| format!("tweaks.{k} = {v}"),
        |(k, v)| match variant(&tag("bad", &[k, &v.to_string()]), "phosphor", &tweak(k, v)) {
            Err(_) => Ok(()),
            Ok(_) => Err("loaded".into()),
        },
    );
    assert!(failed.is_empty(), "{} not refused:\n{}", failed.len(), failed.join("\n"));
}

/// Every number of every tweak at each end of its range draws, in themes that use the tweaks in
/// different ways, on screens small, full HD and portrait (the slow ones a share of the cases: `sampled`).
#[test]
fn tweaks_at_their_limits_draw() {
    let mut cases: Vec<(&str, String, Value)> = vec![];
    for (key, limit) in LIMITS {
        let d = &tweaks()[*key];
        let row = match d {
            Value::Array(a) if limit.rows > 0 => a.first().cloned(),
            _ => None,
        };
        let n = row.as_ref().map_or_else(|| count(d), count);
        for i in 0..n {
            let (lo, hi) = limit.ranges.of(i);
            for edge in [lo, hi] {
                let v = match &row {
                    Some(r) => Value::Array(vec![with_number(r, i, edge)]),
                    None => with_number(d, i, edge),
                };
                for base in ["phosphor", "neon_rain", "winter", "guybrush", "matrix", "copperworks", "phosphor@full", "night_city@portrait"] {
                    if sampled(base, &format!("{key}={v}"), 8) {
                        cases.push((base, key.to_string(), v.clone()));
                    }
                }
            }
        }
        if let (Some(row), Ranges::At(_)) = (&row, limit.ranges) {
            let most = Value::Array(vec![row.clone(); limit.rows]);
            cases.push(("phosphor", key.to_string(), most));
        }
    }
    let shown = "[monitors]\ntitle = \"all\"\nwidgets = \"all\"\ngraph = \"all\"\n";
    let failed = each_parallel(
        &cases,
        |(b, k, v)| format!("{b}: tweaks.{k} = {v}"),
        |(base, k, v)| {
            let (theme, (w, h)) = match base.split_once('@') {
                Some((t, "full")) => (t, (1920, 1080)),
                Some((t, _)) => (t, (540, 960)),
                None => (*base, (480, 270)),
            };
            match variant(&tag("edge", &[base, k, &v.to_string()]), theme, &format!("{}{shown}", tweak(k, v))) {
                Ok(l) => exercise(&l, w, h),
                Err(_) => Ok(()), // refused by a relation between tweaks (day, the mountains' tiers)
            }
        },
    );
    assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n"));
}

/// Broken themes of the kinds that would stop the wall while it draws are errors when they load.
#[test]
fn themes_that_would_stop_the_wall_are_errors() {
    for (name, more) in [
        ("reel-short", "[tweaks]\nreel = \"M16\"\n"),
        ("reel-no-windows", "[tweaks]\nreel = \"M0,0 L1,1 Z\"\n"),
        ("reel-letter", "[tweaks]\nreel = \"M1,2 L3 Z\"\n"),
        ("reel-arc", "[tweaks]\nreel = \"M0,0 A1,1 0 0 1 2,2\"\n"),
        ("reel-endless", "[tweaks]\nreel = \"M1e999,0 L1,1\"\n"),
        ("title", "[tweaks]\ntitle = 0.0\n"),
        ("supersample", "[tweaks]\nsupersample = 0\n"),
        ("deck-glow", "[tweaks]\ndeck_glow = [[0, 0.2]]\n"),
        ("tick", "[tweaks]\ntick_seconds = 1e-300\n"),
        ("scanline", "[tweaks]\nscanline = [0.5, 0.06]\n"),
    ] {
        assert!(variant(&format!("crashed-{name}"), "phosphor", more).is_err(), "{name} loaded");
    }
}

/// A scene that starts with a live part, or one only some monitors show, fails its trial; and a
/// scene that is built anyway starts black.
#[test]
fn a_scene_must_start_still_on_every_monitor() {
    for (name, scene) in [("live-first", "[\"rain\", \"sky\"]"), ("header-first", "[\"title\", \"sky\"]")] {
        let l = variant(&format!("first-{name}"), "phosphor", &format!("scene = {scene}\n")).unwrap();
        assert!(retro_wall::elements::trial(&l.plain).is_err(), "{name}");
        let g = retro_wall::scene::Geo::new(64, 36, "x", retro_wall::scene::Edges::NONE, retro_wall::scene::Header::ALL, None, l.plain.clone());
        let pic = retro_wall::still::draw(&g).expect("a picture");
        assert_eq!(pic.width(), 64);
    }
}

/// A matrix theme whose lettering file is cut short, or has a command no path has, fails its trial
/// with the file's name.
#[test]
fn broken_letterings_fail_the_trial() {
    let text = std::fs::read_to_string(themes_dir().join("matrix/MatrixCode.json")).unwrap();
    let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let glyphs = file["glyphs"].as_object_mut().unwrap();
    let first = glyphs.keys().next().unwrap().clone();
    let d = glyphs[&first][1].as_str().unwrap().to_string();
    for (name, broken) in [("cut", format!("{d} L1")), ("arc", format!("{d} A1,1 0 0 1 2,2")), ("number", format!("{d} 5")), ("empty-glyphs", String::new())] {
        let mut f = file.clone();
        if name == "empty-glyphs" {
            f["units_per_em"] = serde_json::json!(0);
        } else {
            f["glyphs"][&first][1] = serde_json::Value::String(broken);
        }
        let dir = scratch(&format!("lettering-{name}"), &[("variant", "extends = \"BUNDLED/matrix\"\n")]);
        std::fs::write(dir.join("variant/MatrixCode.json"), f.to_string()).unwrap();
        let l = resolve(&dir.join("variant").display().to_string()).unwrap();
        let e = retro_wall::elements::trial(&l.plain).expect_err(name);
        assert!(e.contains("MatrixCode.json"), "{name}: {e}");
    }
    let dir = scratch("lettering-missing", &[("variant", "extends = \"BUNDLED/matrix\"\n")]);
    std::fs::write(dir.join("variant/Matrix.json"), "{\"units_per_em\": 1000").unwrap();
    let l = resolve(&dir.join("variant").display().to_string()).unwrap();
    assert!(retro_wall::elements::trial(&l.plain).is_err());
}

/// The numbers of every element's settings, in every bundled theme's scene, given hostile values:
/// refused, or drawn without a panic (the slow bases' elements a share of the cases: `sampled`).
#[test]
fn hostile_element_settings_are_refused_or_draw() {
    let mut cases: Vec<(String, String, String, Value)> = vec![];
    let mut seen = std::collections::HashSet::new();
    for base in ["phosphor", "neon_rain", "night_city", "winter", "guybrush", "matrix", "aurora", "copperworks"] {
        let l = resolve(&bundled(base)).unwrap();
        for e in &l.plain.scene {
            if !seen.insert(e.kind.clone()) {
                continue;
            }
            for (k, v) in &e.settings {
                for i in 0..count(v) {
                    for bad in [-1e9, -1.0, 0.0, 1e-9, 1e9, f64::NAN, f64::INFINITY] {
                        if sampled(base, &format!("{}.{k}.{i}={bad}", e.name), 4) {
                            cases.push((base.to_string(), e.name.clone(), k.clone(), with_number(v, i, bad)));
                        }
                    }
                }
                if let Value::Array(a) = v
                    && let Some(first) = a.first()
                {
                    cases.push((base.to_string(), e.name.clone(), k.clone(), Value::Array(vec![first.clone(); 300])));
                }
            }
        }
    }
    assert!(cases.len() > 500, "{}", cases.len());
    let shown = "[monitors]\ntitle = \"all\"\nwidgets = \"all\"\ngraph = \"all\"\n";
    let failed = each_parallel(
        &cases,
        |(b, e, k, v)| format!("{b}: element.{e}.{k} = {v}"),
        |(base, e, k, v)| {
            let more = format!("{shown}[element.{e}]\n{k} = {v}\n");
            match variant(&tag("setting", &[base, e, k, &v.to_string()]), base, &more) {
                Ok(l) => exercise(&l, 320, 180),
                Err(_) => Ok(()),
            }
        },
    );
    assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n"));
}

/// Every built-in widget type's numbers given hostile values: refused, or drawn without a panic.
#[test]
fn hostile_widget_settings_are_refused_or_draw() {
    let numbers: &[(&str, &[&str])] = &[
        ("ring", &["tau", "deadband", "step", "sweep", "thick"]),
        ("bar", &["tau", "deadband", "step", "segments"]),
        ("spark", &["tau", "deadband", "step", "span", "every"]),
        ("battery", &["tau", "deadband", "step"]),
        ("cells", &["tau", "deadband", "step", "cells", "high", "cap", "glow", "width", "bright", "space"]),
        ("ticker", &["speed"]),
    ];
    let mut cases = vec![];
    for (kind, keys) in numbers {
        for key in *keys {
            for bad in ["-1e9", "-1", "0", "1e-9", "1e9", "nan", "inf", "3", "\"x\""] {
                cases.push((kind.to_string(), key.to_string(), bad.to_string()));
            }
        }
    }
    let failed = each_parallel(
        &cases,
        |(t, k, v)| format!("{t}.{k} = {v}"),
        |(kind, key, v)| {
            let more = format!("[[group]]\nanchor = \"top-left\"\nwidgets = [\"w\"]\n[widget.w]\ntype = \"{kind}\"\n{key} = {v}\n");
            match variant(&tag("widget", &[kind, key, v]), "phosphor", &more) {
                Ok(l) => exercise(&l, 320, 180),
                Err(_) => Ok(()),
            }
        },
    );
    assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n"));
}

#[test]
fn a_huge_number_is_echoed_short() {
    let (k, _) = LIMITS.iter().find(|(k, l)| l.rows == 0 && count(&tweaks()[*k]) == 1).unwrap();
    let e = retro_wall::tweaks::limit(k, &Value::Float(1e308)).unwrap_err();
    assert!(e.contains("is 1e308") && e.len() < 80, "{e}");
}
