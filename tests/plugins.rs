//! Plugin processes as the wall runs them: sources that hang, time out, print garbage, die or stream
//! (and are stopped and resumed with the wall), and faces that answer huge numbers, endless output or
//! no JSON. Each is said, marked stale or contained; none holds up its caller. And the examples that
//! `--new` writes work.

#![cfg(unix)]

mod common;

use common::procs::{eventually, group, scratch, state};
use retro_wall::plugins::{self, Face, FaceLook, SourceSpec};
use retro_wall::sources::Reading;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Readings = Arc<Mutex<HashMap<String, Reading>>>;

fn executable(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A source run from a script, as the wall runs it; its readings and its switch. Starting it must not
/// wait for the script.
fn source(tag: &str, body: &str, every: f64) -> (PathBuf, Readings, Arc<AtomicBool>) {
    let dir = scratch(&format!("source-{tag}"));
    let p = executable(&dir, tag, &body.replace("DIR", &dir.display().to_string()));
    let (readings, running): (Readings, _) = (Arc::default(), Arc::new(AtomicBool::new(true)));
    let t0 = Instant::now();
    plugins::spawn_source(SourceSpec { name: tag.into(), command: vec![p.display().to_string()], every, stream: false }, readings.clone(), running.clone());
    assert!(t0.elapsed() < Duration::from_millis(100), "{tag}: starting the source waited for it");
    (dir, readings, running)
}

fn reading(r: &Readings, name: &str) -> Option<Reading> {
    r.lock().unwrap().get(name).cloned()
}

fn pid_in(dir: &Path) -> Option<u32> {
    std::fs::read_to_string(dir.join("pid")).ok()?.trim().parse().ok()
}

#[test]
fn a_source_that_hangs_is_stale_and_killed() {
    let (dir, r, _on) = source("hangs", "echo $$ >DIR/pid; exec sleep 1000", 1.0);
    assert!(eventually(9.0, || reading(&r, "hangs").is_some_and(|x| x.stale)), "not marked stale");
    let pid = pid_in(&dir).expect("it ran");
    assert!(eventually(2.0, || state(pid).is_none()), "left running");
}

#[test]
fn a_source_that_answers_too_late_is_stale() {
    let (dir, r, _on) = source("late", "echo $$ >DIR/pid; sleep 7; echo 50", 1.0);
    assert!(eventually(9.0, || reading(&r, "late").is_some_and(|x| x.stale)), "not marked stale");
    assert_ne!(reading(&r, "late").unwrap().value, 50.0, "a reading past its deadline was taken");
    let pid = pid_in(&dir).expect("it ran");
    assert!(eventually(2.0, || state(pid).is_none()), "left running");
}

#[test]
fn a_source_that_prints_garbage_keeps_its_last_value_dimmed() {
    // a good reading first, then garbage
    let (_dir, r, _on) =
        source("garbage", "if [ -e DIR/once ]; then echo 'no number here'; printf '{\"value\": [}\\n\\001\\377\\n'; else : >DIR/once; echo 42; fi", 1.0);
    assert!(eventually(3.0, || reading(&r, "garbage").is_some_and(|x| x.value == 42.0 && !x.stale)), "the good reading not taken");
    assert!(eventually(3.0, || reading(&r, "garbage").is_some_and(|x| x.stale)), "garbage not marked stale");
    assert_eq!(reading(&r, "garbage").unwrap().value, 42.0, "the last good value not kept");
}

#[test]
fn a_source_that_dies_is_stale() {
    let (_dir, r, _on) = source("dies", "kill -KILL $$", 0.2);
    assert!(eventually(3.0, || reading(&r, "dies").is_some_and(|x| x.stale)), "not marked stale");
}

#[test]
fn a_source_that_is_not_there_is_stale() {
    let (readings, running): (Readings, _) = (Arc::default(), Arc::new(AtomicBool::new(true)));
    plugins::spawn_source(
        SourceSpec { name: "gone".into(), command: vec!["/nonexistent/retro-wall-source".into()], every: 1.0, stream: false },
        readings.clone(),
        running,
    );
    assert!(eventually(3.0, || reading(&readings, "gone").is_some_and(|x| x.stale)), "not marked stale");
}

#[test]
fn a_streaming_source_is_stopped_and_resumed_with_the_wall() {
    let (dir, r, on) = source("stream", "echo $$ >DIR/pid; echo '{\"stream\": true}'; i=0; while :; do i=$((i % 90 + 1)); echo $i; sleep 0.05; done", 2.0);
    let value = || reading(&r, "stream").map(|x| x.value);
    assert!(eventually(3.0, || value().is_some()), "no reading");
    let pid = pid_in(&dir).expect("it ran");
    on.store(false, Ordering::Relaxed);
    // stopped: every process of its group (the shell itself waits in D when stopped while it starts a
    // program, until that program is let go on)
    let stopped = || {
        let g = group(pid);
        !g.is_empty() && g.iter().all(|&p| matches!(state(p), Some('T' | 't' | 'D' | 'U')))
    };
    assert!(eventually(3.0, stopped), "not stopped with the wall: {:?}", group(pid).iter().map(|&p| state(p)).collect::<Vec<_>>());
    let held = value();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(value(), held, "readings went on while stopped");
    on.store(true, Ordering::Relaxed);
    assert!(eventually(3.0, || !group(pid).iter().any(|&p| state(p) == Some('T')) && value() != held), "not resumed with the wall");
}

fn look() -> FaceLook {
    let th = retro_wall::config::resolve(&common::bundled("phosphor"), &retro_wall::config::Config::default(), retro_wall::elements::kind_info, false)
        .unwrap()
        .plain;
    FaceLook::of(&th)
}

/// A face from a script, asked for one picture; its answer, within `secs`. Asking must not wait.
fn face(tag: &str, body: &str, secs: f64) -> (PathBuf, Result<plugins::Picture, String>) {
    let dir = scratch(&format!("face-{tag}"));
    let p = executable(&dir, tag, &body.replace("DIR", &dir.display().to_string()));
    let mut f = Face::new(p, look());
    let t0 = Instant::now();
    let id = f.ask(json!({"width": 120, "height": 90, "scale": 1.0, "value": 40.0, "settings": {}}));
    assert!(t0.elapsed() < Duration::from_millis(50), "{tag}: asking waited for the face");
    let end = Instant::now() + retro_wall::util::seconds(secs);
    loop {
        if let Some((n, answer)) = f.answer() {
            assert_eq!(n, id);
            return (dir, answer);
        }
        assert!(Instant::now() < end, "{tag}: no answer, not even a failure, in {secs} s");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_face_with_huge_numbers_draws_within_its_picture() {
    let (_, answer) = face(
        "huge",
        r##"read -r _; echo '[{"op": "rect", "x": -50, "y": -50, "w": 1e308, "h": 1e308}, {"op": "fill", "colour": "#ff0000"}, {"op": "arc", "x": 1e300, "y": 0, "r": 1e300, "from": 0, "to": 1e300}, {"op": "stroke", "width": 1e300}, {"op": "text", "text": "x", "size": 1e300, "x": 1e300, "y": -1e300}]'; read -r _"##,
        5.0,
    );
    let pic = answer.expect("drawn");
    assert_eq!((pic.w, pic.h), (120, 90));
    // the clamped rectangle covers the picture: red throughout (premultiplied BGRA)
    let px = &pic.data[(45 * pic.stride + 100 * 4) as usize..][..4];
    assert_eq!(px, [0, 0, 255, 255]);
}

#[test]
fn a_face_with_endless_output_is_contained() {
    let (dir, answer) = face("endless", "echo $$ >DIR/pid; read -r _; yes x | tr -d '\\n'", 5.0);
    assert!(answer.is_err(), "endless output taken as an answer");
    let pid = pid_in(&dir).expect("it ran");
    assert!(eventually(2.0, || group(pid).is_empty()), "what it started was left running: {:?}", group(pid));
}

#[test]
fn a_face_that_answers_no_json_is_said() {
    let (_, answer) = face("notjson", "read -r _; echo '{[}'; read -r _", 5.0);
    let e = answer.err().expect("refused");
    assert!(e.contains("not a list of operations"), "{e}");
}

#[test]
fn a_face_that_never_answers_fails_in_two_seconds() {
    let t0 = Instant::now();
    let (dir, answer) = face("silent", "echo $$ >DIR/pid; exec sleep 1000", 5.0);
    assert!(answer.is_err());
    assert!(t0.elapsed() < Duration::from_secs(4));
    let pid = pid_in(&dir).expect("it ran");
    assert!(eventually(2.0, || state(pid).is_none()), "left running");
}

#[test]
fn a_face_that_exits_is_said() {
    let (_, answer) = face("exits", "exit 0", 5.0);
    assert!(answer.is_err());
}

#[test]
fn a_source_text_with_a_nul_is_taken_without_it() {
    let (_dir, r, _on) = source("nul", r#"printf '%s\n' '{"value": 5, "text": "news\u0000here\u0007"}'"#, 1.0);
    assert!(eventually(3.0, || reading(&r, "nul").is_some()), "no reading");
    let got = reading(&r, "nul").unwrap();
    assert_eq!((got.value, got.text.as_deref()), (5.0, Some("newshere")));
    assert_eq!(plugins::parse_reading(r#""a\u0000b""#).and_then(|x| x.text).as_deref(), Some("ab"));
}

#[test]
fn a_face_text_with_a_nul_is_drawn() {
    let (_, answer) = face(
        "nul",
        r##"read -r _; printf '%s\n' '[{"op": "text", "text": "\u0000x", "x": 4, "y": 40, "size": 30, "colour": "#ffffff"}, {"op": "text", "text": "\u0000", "align": "right"}]'; read -r _"##,
        5.0,
    );
    let pic = answer.expect("drawn");
    assert!(pic.data.chunks(4).any(|px| px[3] != 0), "the x after the NUL was not drawn");
}

/// `--new source` and `--new face` write programs that work as they are.
#[test]
fn the_new_examples_work() {
    let home = scratch("new");
    let run = |what: &str| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_retro-wall"))
            .args(["--new", what, "mine"])
            .env("HOME", &home)
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .unwrap();
        assert!(out.status.success(), "--new {what}: {}", String::from_utf8_lossy(&out.stderr));
    };
    run("source");
    run("face");
    let conf = home.join(".config/retro");
    let printed = std::process::Command::new(conf.join("sources/mine")).output().unwrap();
    let line = String::from_utf8_lossy(&printed.stdout);
    assert!(printed.status.success() && plugins::parse_reading(line.trim()).is_some(), "the source printed {line:?}");
    let mut f = Face::new(conf.join("faces/mine"), look());
    f.ask(json!({"width": 172, "height": 117, "scale": 0.57, "value": 46.0, "settings": {"label": "CPU"}}));
    assert!(eventually(5.0, || match f.answer() {
        Some((_, a)) => a.map(|p| p.w == 172).expect("the face drew"),
        None => false,
    }));
}
