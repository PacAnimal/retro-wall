//! Every bundled theme loads, builds and draws; broken themes and configs are reported, not panics.
//! Themes are named by path, so no test changes the process's environment.

use retro_wall::config::{self, Config, Loaded};
use retro_wall::elements;
use retro_wall::scene::{Geo, Header};
use std::path::{Path, PathBuf};

fn themes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("themes")
}

/// A theme by path: the bundled one of that name.
fn bundled(name: &str) -> String {
    themes_dir().join(name).display().to_string()
}

fn resolve(spec: &str) -> Result<Loaded, String> {
    config::resolve(spec, &Config::default(), elements::kind_info, true).map_err(|e| e.0)
}

/// A scratch directory of its own with theme.toml files: name -> content. "PHOSPHOR" in a content is
/// the bundled phosphor's path.
fn scratch(tag: &str, themes: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("retro-wall-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in themes {
        std::fs::create_dir_all(dir.join(name)).unwrap();
        std::fs::write(dir.join(name).join("theme.toml"), text.replace("PHOSPHOR", &bundled("phosphor"))).unwrap();
    }
    dir
}

/// A bundled theme: it resolves, passes the trial, and every look draws its still picture.
fn draws(name: &str) {
    let l = resolve(&bundled(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(l.plain.name.to_lowercase().replace(' ', "_"), name.to_lowercase());
    for th in l.looks.iter().flat_map(|looks| looks.values()).chain([&l.plain]) {
        elements::trial(th).unwrap_or_else(|e| panic!("{name}: {e}"));
        let pic = retro_wall::still::draw(&Geo::new(480, 270, "retro", retro_wall::scene::Edges::NONE, Header::ALL, None, th.clone())).expect("a picture");
        assert_eq!((pic.width(), pic.height()), (480, 270));
    }
}

#[test]
fn aurora() {
    draws("aurora");
}

#[test]
fn guybrush() {
    draws("guybrush");
}

#[test]
fn matrix() {
    draws("matrix");
}

#[test]
fn neon_rain() {
    draws("neon_rain");
}

#[test]
fn night_city() {
    draws("night_city");
}

#[test]
fn phosphor() {
    draws("phosphor");
}

#[test]
fn winter() {
    draws("winter");
}

#[test]
fn every_bundled_theme_has_a_test() {
    let mut names: Vec<String> = std::fs::read_dir(themes_dir())
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("theme.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["aurora", "guybrush", "matrix", "neon_rain", "night_city", "phosphor", "winter"]);
}

#[test]
fn broken_themes_are_errors_that_say_why() {
    let themes = [
        ("bad-tweak", "extends = \"PHOSPHOR\"\n[tweaks]\nvignete = 0.3\n"),
        ("bad-type", "extends = \"PHOSPHOR\"\n[tweaks]\nvignette = \"dark\"\n"),
        ("bad-colour", "extends = \"PHOSPHOR\"\n[palette]\nbase = \"#12345\"\n"),
        ("bad-role", "extends = \"PHOSPHOR\"\n[day.palette]\nsky = \"#123456\"\n"),
        ("bad-look", "extends = \"PHOSPHOR\"\n[night.tweaks]\nvignette = true\n"),
        ("bad-group", "extends = \"PHOSPHOR\"\n[[group]]\nanchor = \"top-left\"\n"),
        ("bad-anchor", "extends = \"PHOSPHOR\"\n[[group]]\nanchor = \"middle\"\nwidgets = [\"cpu\"]\n"),
        ("bad-widget", "extends = \"PHOSPHOR\"\n[[group]]\nanchor = \"top-left\"\nwidgets = [\"gpu\"]\n"),
        ("bad-style", "extends = \"PHOSPHOR\"\n[element.title]\nstyle = \"neon\"\n"),
        ("bad-kind", "extends = \"PHOSPHOR\"\nscene = [\"sky\", \"nebula\"]\n"),
        ("bad-setting", "extends = \"PHOSPHOR\"\n[element.title]\nflicker = 3\n"),
        ("bad-toml", "this is not toml\n"),
    ];
    let dir = scratch("broken", &themes);
    for (name, _) in themes {
        let e = resolve(&dir.join(name).display().to_string()).err().unwrap_or_else(|| panic!("{name} loaded"));
        assert!(!e.is_empty(), "{name}");
    }
    let own = scratch("loop", &[("bad-extends", "")]).join("bad-extends");
    std::fs::write(own.join("theme.toml"), format!("extends = {:?}\n", own.display().to_string())).unwrap();
    let e = resolve(&own.display().to_string()).err().expect("a theme that extends itself loaded");
    assert!(e.ends_with("extends itself"), "{e}");
    // a circle of two is named as one, not as a theme that extends itself
    let dir = scratch("circle", &[("cyc1", "extends = \"cyc2\"\n"), ("cyc2", "extends = \"cyc1\"\n")]);
    let e = resolve(&dir.join("cyc1").display().to_string()).err().expect("a circle of themes loaded");
    assert!(e.ends_with("extends in a circle: cyc1 -> cyc2 -> cyc1"), "{e}");
}

/// A theme file too large, or one that is a FIFO: errors that say why, at once.
#[cfg(unix)]
#[test]
fn a_theme_file_too_large_or_not_regular_is_refused() {
    let big = scratch("big", &[("big", "")]).join("big");
    std::fs::write(big.join("theme.toml"), format!("# {}\nextends = \"PHOSPHOR\"\n", "x".repeat(retro_wall::files::Kind::Settings.limit() as usize))).unwrap();
    let e = resolve(&big.display().to_string()).err().expect("an oversized theme.toml loaded");
    assert!(e.contains("the most is"), "{e}");
    let dir = scratch("fifo-extends", &[]);
    let parent = dir.join("parent.toml");
    let c = std::ffi::CString::new(parent.to_str().unwrap()).unwrap();
    // SAFETY: a NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    std::fs::create_dir_all(dir.join("child")).unwrap();
    std::fs::write(dir.join("child/theme.toml"), format!("extends = {:?}\n", parent.display().to_string())).unwrap();
    let t0 = std::time::Instant::now();
    assert!(resolve(&dir.join("child").display().to_string()).is_err(), "a FIFO for a theme loaded");
    assert!(t0.elapsed() < std::time::Duration::from_secs(2), "a FIFO for a theme was waited for");
}

#[test]
fn a_scene_that_does_not_start_still_on_every_monitor_fails_its_trial() {
    let dir = scratch(
        "first",
        &[("live-first", "extends = \"PHOSPHOR\"\nscene = [\"rain\", \"sky\"]\n"), ("header-first", "extends = \"PHOSPHOR\"\nscene = [\"title\", \"sky\"]\n")],
    );
    for name in ["live-first", "header-first"] {
        let l = resolve(&dir.join(name).display().to_string()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(elements::trial(&l.plain).is_err(), "{name}");
    }
}

/// A retro.toml with something wrong in nearly every item, and something right beside each.
const BROKEN_CONFIG: &str = r#"
colour = 1
scanlines = "yes"
host = 3
widget.plain = 5
[[group]]
anchor = "top-left"
widgets = ["cpu", "news", "gpu", "ram", "cpu"]
[[group]]
anchor = "middle"
widgets = ["swap"]
[[group]]
anchor = "status"
widgets = ["status", "ram"]
[widget.news]
type = "ticker"
size = [2, 1]
[widget.cpu]
label = 5
[source.gpu]
command = []
[source.load]
command = ["echo", "5"]
[monitors]
title = "middle"
widgets = "all"
[monitor.left]
group = 3
"#;

fn broken_config(tag: &str) -> Config {
    let path = scratch(tag, &[]).join("retro.toml");
    std::fs::write(&path, BROKEN_CONFIG).unwrap();
    config::read_config(Some(path.to_str().unwrap())).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_config_keeps_what_checks_and_says_what_it_left_out() {
    let cfg = broken_config("config-items");
    let said = cfg.problems.join("\n");
    for what in ["unknown colour", "scanlines", "host", "widget.plain", "source.gpu", "monitors.title", "group 2", "monitor.left"] {
        assert!(said.contains(what), "{what} not said in:\n{said}");
    }
    assert!(cfg.problems.iter().all(|p| p.ends_with("left out")), "{said}");
    assert_eq!(cfg.sources.keys().collect::<Vec<_>>(), ["load"]);
    let groups = cfg.groups.as_ref().expect("the groups that check");
    assert_eq!(groups.len(), 2);
    assert_eq!((cfg.scanlines, cfg.host.as_deref()), (None, None));
    let monitors = cfg.monitors.as_ref().expect("the monitors that check");
    assert_eq!(monitors.keys().collect::<Vec<_>>(), ["widgets"]);
    assert!(cfg.widgets.contains_key("news") && !cfg.widgets.contains_key("plain"));
}

#[test]
fn a_config_that_is_not_toml_is_an_error() {
    let bad = scratch("config", &[]).join("retro.toml");
    std::fs::write(&bad, "this is not toml\n").unwrap();
    assert!(config::read_config(Some(bad.to_str().unwrap())).is_err());
}

#[test]
fn every_bundled_theme_loads_under_a_broken_config() {
    let cfg = broken_config("config-themes");
    for name in ["aurora", "guybrush", "matrix", "neon_rain", "night_city", "phosphor", "winter"] {
        let l = config::load(&bundled(name), &cfg, elements::kind_info, false).unwrap_or_else(|e| panic!("{name}: {e}"));
        let said = l.problems.join("\n");
        for what in
            ["widget news: unknown field `size`", "widget gpu: no widget type", "widget cpu", "widget ram: the status anchor", "widget cpu is in two places"]
        {
            assert!(said.contains(what), "{name}: {what} not said in:\n{said}");
        }
        for th in l.looks.iter().flat_map(|looks| looks.values()).chain([&l.plain]) {
            let shown: Vec<Vec<&str>> = th.groups.iter().map(|g| g.widgets.iter().map(String::as_str).collect()).collect();
            assert_eq!(shown, [vec!["cpu", "ram"], vec!["status"]], "{name}");
            assert_eq!(th.layout.len(), 2, "{name}");
            // the user's settings that broke the meter are left out; the theme's are kept
            assert!(th.widgets["cpu"].get("label").is_some_and(|l| l.is_str()), "{name}");
        }
    }
}

#[test]
fn a_users_settings_that_check_are_kept() {
    let path = scratch("config-good", &[]).join("retro.toml");
    std::fs::write(
        &path,
        "[widget.cpu]\nlabel = \"LOAD\"\n[[group]]\nanchor = \"top-right\"\nwidgets = [\"cpu\", \"news\"]\n[widget.news]\ntype = \"ticker\"\n",
    )
    .unwrap();
    let cfg = config::read_config(Some(path.to_str().unwrap())).unwrap();
    assert!(cfg.problems.is_empty(), "{:?}", cfg.problems);
    let l = config::load(&bundled("phosphor"), &cfg, elements::kind_info, false).unwrap();
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert_eq!(l.plain.groups[0].widgets, ["cpu", "news"]);
    assert_eq!(l.plain.widgets["cpu"]["label"].as_str(), Some("LOAD"));
}

/// The binary, with this scratch directory as its config home; (exit code, standard error).
fn run_with_config(home: &Path, themes: Option<&Path>, exe: &Path, a: &[&str]) -> (i32, String) {
    let mut run = std::process::Command::new(exe);
    run.args(a).env_remove("DISPLAY").env_remove("RETRO_THEMES").env("XDG_CONFIG_HOME", home).env("APPDATA", home);
    if let Some(t) = themes {
        run.env("RETRO_THEMES", t);
    }
    let out = run.output().expect("the binary runs");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn a_broken_user_theme_and_config_still_draw() {
    let home = scratch("user-theme", &[]);
    let user = home.join("retro");
    std::fs::create_dir_all(user.join("themes").join("mine")).unwrap();
    std::fs::write(user.join("themes").join("mine").join("theme.toml"), "extends = \"aurora\"\n[tweaks]\nvignete = 2\n").unwrap();
    std::fs::write(user.join("retro.toml"), BROKEN_CONFIG).unwrap();
    let out = home.join("out.png");
    let exe = Path::new(env!("CARGO_BIN_EXE_retro-wall"));
    let (code, err) = run_with_config(&home, Some(&themes_dir()), exe, &["--preview", out.to_str().unwrap(), "320x180", "--theme", "mine"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("vignete") && err.contains("the default theme"), "{err}");
    assert!(err.contains("widget news") && err.contains("left out"), "{err}");
    assert!(out.is_file());
}

/// A user's theme named as the default, and broken: the installed default draws.
#[cfg(unix)]
#[test]
fn a_broken_user_theme_in_the_defaults_name_still_draws() {
    let home = scratch("user-default", &[]);
    let user = home.join("retro").join("themes").join("phosphor");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(user.join("theme.toml"), "scene = [\"sky\", \"nebula\"]\n").unwrap();
    let installed = home.join("installed");
    std::fs::create_dir_all(&installed).unwrap();
    let exe = installed.join("retro-wall");
    std::fs::copy(env!("CARGO_BIN_EXE_retro-wall"), &exe).unwrap();
    let _ = std::os::unix::fs::symlink(themes_dir(), installed.join("themes"));
    let out = home.join("out.png");
    let (code, err) = run_with_config(&home, None, &exe, &["--preview", out.to_str().unwrap(), "320x180"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("nebula") && err.contains("the default theme"), "{err}");
    assert!(out.is_file());
}
