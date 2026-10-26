//! What the theme tests share: themes by path, scratch themes, and drawing a theme every way that
//! needs no display.

#![allow(dead_code)]

#[cfg(unix)]
pub mod procs;
#[cfg(target_os = "linux")]
pub mod x;

use retro_wall::config::{self, Config, Loaded, Theme};
use retro_wall::elements;
use retro_wall::gfx::Uploads;
use retro_wall::live::Frozen;
use retro_wall::scene::{Geo, Header, Net, Scene, World, moves};
use retro_wall::sources::Traffic;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub fn themes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("themes")
}

/// A bundled theme, by path.
pub fn bundled(name: &str) -> String {
    themes_dir().join(name).display().to_string()
}

pub fn resolve(spec: &str) -> Result<Loaded, String> {
    config::resolve(spec, &Config::default(), elements::kind_info, true).map_err(|e| e.0)
}

/// A scratch directory of the test's own (`tag`), with these themes: name -> theme.toml. "BUNDLED/x"
/// in a theme stands for the bundled theme x's path.
pub fn scratch(tag: &str, themes: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("retro-wall-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in themes {
        std::fs::create_dir_all(dir.join(name)).unwrap();
        let text = text.replace("BUNDLED/", &format!("{}/", themes_dir().display()));
        std::fs::write(dir.join(name).join("theme.toml"), text).unwrap();
    }
    dir
}

/// A scratch tag of its own for each case: `kind` and a hash of `parts`.
pub fn tag(kind: &str, parts: &[&str]) -> String {
    let h = parts.iter().flat_map(|p| p.bytes().chain([0])).fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{kind}-{h:016x}")
}

/// A theme that extends a bundled one with `more` (TOML), resolved; written to a scratch theme
/// named `tag`.
pub fn variant(tag: &str, base: &str, more: &str) -> Result<Loaded, String> {
    let text = format!("extends = \"BUNDLED/{base}\"\n{more}");
    let dir = scratch(tag, &[("variant", &text)]);
    resolve(&dir.join("variant").display().to_string())
}

/// The world a snapshot sees: fixed readings and clock.
pub fn world(th: &Theme) -> World {
    let at = 7.25;
    let mut traffic = Traffic::new(th.tw.mountains[1], th.tw.mountain_tiers.clone());
    traffic.sample(at, true);
    traffic.held = true;
    World {
        clock: at,
        clock_real: at,
        frozen: Some(Frozen { at, mains: true }),
        demo: true,
        zoom: 1.0,
        traffic,
        network: Net { online: Some(true), address: Some("192.168.1.37".into()), router: None, public: Some("203.0.113.37".into()) },
        readings: Arc::new(Mutex::new(HashMap::new())),
    }
}

/// A loaded theme, every look: its trial, its still picture at w x h, and its live parts started,
/// their pictures made and their state taken, as on a monitor (all but the GPU's drawing).
pub fn exercise(l: &Loaded, w: i32, h: i32) -> Result<(), String> {
    for th in l.looks.iter().flat_map(|looks| looks.values()).chain([&l.plain]) {
        elements::trial(th)?;
        let g = Geo::new(w, h, "retro-test", retro_wall::scene::Edges::NONE, Header::ALL, None, th.clone());
        let mut pic = retro_wall::still::draw(&g)?;
        assert_eq!((pic.width(), pic.height()), (w, h));
        // nothing else holds it: its pixels can be read in place (the wallpaper's file name)
        assert!(retro_wall::util::view(&mut pic).is_some(), "the still picture is held elsewhere");
        let mut scene = Scene::new(g.clone());
        let mut up = Uploads::default();
        let _plan = scene.compile(&mut up);
        let mut shared = std::mem::take(&mut scene.shared);
        let mut world = world(th);
        let mut parts = scene.parts;
        let live: Vec<usize> = (0..parts.len()).filter(|&i| moves(parts[i].as_ref())).collect();
        for &i in &live {
            parts[i].start(&g, &mut world, &mut shared);
        }
        for &i in &live {
            parts[i].textures(&g, &shared, &mut up);
        }
        for &i in &live {
            parts[i].state(&g, &mut world);
            let _ = parts[i].interval();
            let _ = parts[i].top(&g);
        }
        for p in parts.iter_mut() {
            p.stop();
        }
    }
    Ok(())
}

/// Run `job` over `items` on every core, each item caught if it panics: the failures, each with its
/// item's name.
pub fn each_parallel<T: Sync>(items: &[T], name: impl Fn(&T) -> String + Sync, job: impl Fn(&T) -> Result<(), String> + Sync) -> Vec<String> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failed = Mutex::new(vec![]);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(item)));
                    let why = match r {
                        Ok(Ok(())) => continue,
                        Ok(Err(e)) => e,
                        Err(p) => format!(
                            "PANIC {}",
                            p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
                        ),
                    };
                    failed.lock().unwrap().push(format!("{}: {why}", name(item)));
                }
            });
        }
    });
    failed.into_inner().unwrap()
}
