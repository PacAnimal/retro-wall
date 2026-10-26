//! The host side: the command line, the loop's sleeping, monitors and their header, the desktop's
//! settings, the machine's readings.

use retro_wall::cli::{self, Command, Reserve};
use retro_wall::config::{Choice, Monitors, Rule};
use retro_wall::live::{self, Timers};
#[cfg(target_os = "linux")]
use retro_wall::platform::linux::{desktop, system};
#[cfg(target_os = "linux")]
use retro_wall::platform::{self, Route};
use retro_wall::platform::{Edges, Screen};
use retro_wall::scene::{Header, Piece};
use retro_wall::sources;
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::path::Path;
use std::process::Command as Run;
use std::time::Duration;

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn screen(name: &str, x: i32, y: i32, primary: bool) -> Screen {
    Screen { name: name.into(), x, y, w: 1920, h: 1080, primary }
}

/// The binary, with the bundled themes and no display; (exit code, standard error).
fn retro_wall(a: &[&str]) -> (i32, String) {
    let out = Run::new(env!("CARGO_BIN_EXE_retro-wall"))
        .args(a)
        .env_remove("DISPLAY")
        .env("RETRO_THEMES", Path::new(env!("CARGO_MANIFEST_DIR")).join("themes"))
        .env("XDG_CONFIG_HOME", std::env::temp_dir().join("retro-wall-host-test-config"))
        .output()
        .expect("the binary runs");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn paused_loop_sleeps() {
    let mut t = Timers::new();
    let now = 1000.0;
    let due = t.tick(now);
    assert!(due.watch && due.look && due.restack);
    // nothing draws (paused and faded, or covered): asleep until the next check, not spinning
    assert!(t.sleep(now, [None, None]) >= retro_wall::util::seconds(Timers::LOOK - 1e-9));
    // checks that are due move on even when their work is skipped
    let later = now + 1.5;
    let due = t.tick(later);
    assert!(due.look && !due.watch);
    assert!(t.sleep(later, [None]) > Duration::from_millis(400));
    // drawing: to the next frame
    assert_eq!(t.sleep(later, [Some(later + 0.02)]), retro_wall::util::seconds(0.02));
    // a frame already late: at once
    assert_eq!(t.sleep(later, [Some(later - 1.0)]), Duration::ZERO);
    // never longer than the nap
    let far = Timers { watch: 1e9, look: 1e9, restack: 1e9 };
    assert_eq!(far.sleep(0.0, []), retro_wall::util::seconds(Timers::NAP));
    // a time that is not a number does not stop the wall: it is a nap
    assert_eq!(far.sleep(0.0, [Some(f64::NAN)]), retro_wall::util::seconds(Timers::NAP));
    assert_eq!(far.sleep(f64::NAN, []), retro_wall::util::seconds(Timers::NAP));
}

#[test]
fn command_lines() {
    let (c, a) = cli::parse(&args(
        "--theme winter --battery --pause-vnc 5900 --interface wg0 --interface eth1 --reserve-top 30 --reserve-left DP-1=40 --demo 12:30 HDMI-1",
    ))
    .unwrap();
    assert_eq!(c, Command::Live);
    assert_eq!((a.theme.as_deref(), a.battery, a.pause_vnc), (Some("winter"), true, Some(5900)));
    assert_eq!(a.interfaces, ["wg0", "eth1"]);
    assert_eq!((a.demo, a.demo_time.as_deref(), a.monitor.as_deref()), (true, Some("12:30"), Some("HDMI-1")));
    assert_eq!(cli::parse(&args("--demo HDMI-1")).unwrap().1.monitor.as_deref(), Some("HDMI-1"));
    let (c, a) = cli::parse(&args("--preview out.png 640x360 --battery -")).unwrap();
    assert_eq!((c, a.host.as_deref()), (Command::Preview("out.png".into(), 640, 360), Some("-")));
    assert_eq!(cli::parse(&args("--snapshot x.png --at 3")).unwrap().0, Command::Snapshot("x.png".into()));
    assert!(cli::parse(&args("--all-monitors")).unwrap().1.all_monitors);
    for bad in [
        "--preview x.png 0x0",
        "--preview x.png 100000x100000",
        "--preview x.png 64x",
        "--preview x.png",
        "--pause-vnc http",
        "--pause-vnc 0",
        "--pause-below 150",
        "--at nan",
        "--at -1",
        "--reserve-middle 3",
        "--reserve-top -4",
        "--reserve-top x=",
        "--all-monitors DP-1",
        "DP-1 DP-2",
        "--snapshot x.png DP-1",
        "--frobnicate",
        "--theme",
    ] {
        assert!(cli::parse(&args(bad)).is_err(), "{bad}");
    }
    assert!(!cli::plain_name("../x") && !cli::plain_name("") && !cli::plain_name("a/b") && cli::plain_name("load"));
}

#[test]
fn reserved_edges() {
    let mut r = Reserve::default();
    r.add("top", "30").unwrap();
    r.add("left", "DP-1=40").unwrap();
    r.add("bottom", "99999").unwrap();
    let found = Edges { top: 5, right: 6, bottom: 7, left: 8 };
    assert_eq!(r.apply(&screen("DP-1", 0, 0, false), found), Edges { top: 30, right: 6, bottom: 1080, left: 40 });
    assert_eq!(r.apply(&screen("HDMI-1", 0, 0, false), found), Edges { top: 30, right: 6, bottom: 1080, left: 8 });
}

/// Output to a reader that has gone (`retro-wall --list | head -1`): no panic, the command finishes.
#[test]
fn output_to_a_closed_pipe_is_no_panic() {
    for a in [&["--list"][..], &["--version"], &["--settings", "phosphor"], &["--bogus"]] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let out = Run::new(env!("CARGO_BIN_EXE_retro-wall"))
            .args(a)
            .env_remove("DISPLAY")
            .env("RETRO_THEMES", Path::new(env!("CARGO_MANIFEST_DIR")).join("themes"))
            .env("XDG_CONFIG_HOME", std::env::temp_dir().join("retro-wall-host-test-config"))
            .stdout(writer.try_clone().unwrap())
            .stderr(writer)
            .output()
            .expect("the binary runs");
        let code = out.status.code();
        assert!(matches!(code, Some(0 | 2)), "{a:?}: {:?}", out.status);
    }
}

/// A widget's label with a NUL in retro.toml: the still picture is drawn, the label without it.
#[test]
fn a_label_with_a_nul_is_drawn() {
    let dir = std::env::temp_dir().join(format!("retro-wall-host-test-nul-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (config, out) = (dir.join("retro.toml"), dir.join("out.png"));
    std::fs::write(&config, "[widget.cpu]\nlabel = \"C\\u0000PU\"\n[widget.\"r\\u0000am\"]\ntype = \"meter\"\nsource = \"ram\"\n").unwrap();
    let (code, err) = retro_wall(&["--preview", out.to_str().unwrap(), "640x360", "--config", config.to_str().unwrap(), "--theme", "phosphor"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.is_file(), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_input_exits_2_without_a_panic() {
    let dir = std::env::temp_dir().join(format!("retro-wall-host-test-{}", std::process::id()));
    let unwritable = dir.join("no/such/dir/out.png");
    let u = unwritable.to_str().unwrap();
    for a in [
        &["--preview", u, "64x36"][..],
        &["--preview", "/tmp/x.png", "0x0"],
        &["--preview", "/tmp/x.png", "100000x100000"],
        &["--snapshot", u],
        &["--pause-vnc", "x"],
        &["--at", "inf", "--snapshot", "/tmp/x.png"],
        &["--new", "source", "../escape"],
        &["--desktop", "frob"],
        &["--new", "theme"],
    ] {
        let (code, err) = retro_wall(a);
        assert_eq!(code, 2, "{a:?}: {err}");
        assert!(!err.contains("panicked"), "{a:?}: {err}");
    }
    // no X display (or no X libraries): an error, exit 1. macOS and Windows have their display anyway.
    if cfg!(target_os = "linux") {
        let (code, err) = retro_wall(&["--snapshot", "/tmp/retro-wall-host-test.png"]);
        assert_eq!(code, 1, "{err}");
        assert!(err.starts_with("retro-wall: ") && !err.contains("panicked"), "{err}");
    }
}

#[test]
fn header_placement() {
    let all = [screen("DP-1", 0, 0, true), screen("HDMI-1", 1920, 0, false)];
    let rules = Monitors::default();
    let every = Header::ALL;
    // one monitor driven: the whole picture, primary or not, rightmost or not
    assert_eq!(live::header(&rules, "DP-1", &all, true), every);
    // every monitor: the rule places it
    assert_eq!(live::header(&rules, "HDMI-1", &all, false), every);
    assert_eq!(live::header(&rules, "DP-1", &all, false), Header::NONE);
    let primary = Monitors { title: Choice::Rule(Rule::Primary), widgets: Choice::Named(vec!["HDMI-1".into()]), graph: Choice::Rule(Rule::None) };
    let h = live::header(&primary, "DP-1", &all, false);
    assert!(h.contains(Piece::Title) && !h.contains(Piece::Widgets) && !h.contains(Piece::Graph));
    assert!(!live::header(&primary, "DP-1", &all, true).contains(Piece::Graph));
    assert_eq!(live::pick(&all, None).unwrap().name, "DP-1");
    assert_eq!(live::pick(&all, Some("HDMI-1")).unwrap().name, "HDMI-1");
    assert!(live::pick(&all, Some("VGA-1")).is_none());
}

#[test]
#[cfg(target_os = "linux")]
fn desktop_detection() {
    let running = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
    assert_eq!(desktop::detect("XFCE", &running(&["xfdesktop", "xfwm4"])), Some("xfce"));
    assert_eq!(desktop::detect("ubuntu:GNOME", &running(&["gnome-shell"])), Some("gnome"));
    assert_eq!(desktop::detect("X-Cinnamon", &running(&["cinnamon", "caja"])), Some("cinnamon"));
    assert_eq!(desktop::detect("KDE", &running(&["plasmashell"])), Some("kde"));
    assert_eq!(desktop::detect("", &running(&["pcmanfm"])), Some("lxde"));
    assert_eq!(desktop::detect("XFCE", &running(&["i3"])), None);
    for name in ["xfce", "gnome", "cinnamon", "mate", "lxde", "kde"] {
        assert_eq!(desktop::named(name).map(|d| d.name()), Some(name));
    }
    assert!(desktop::named("cde").is_none());
}

#[test]
#[cfg(target_os = "linux")]
fn desktop_settings_text() {
    let p = desktop::properties("({'/backdrop/b': <uint32 5>, '/backdrop/a': <'it\\'s, <odd>'>, '/backdrop/c': <[<0.5>, <1.0>]>},)").unwrap();
    assert_eq!(
        p,
        [
            ("/backdrop/a".to_string(), "'it\\'s, <odd>'".to_string()),
            ("/backdrop/b".into(), "uint32 5".into()),
            ("/backdrop/c".into(), "[<0.5>, <1.0>]".into())
        ]
    );
    assert_eq!(desktop::properties("(@a{sv} {},)").unwrap(), []);
    assert!(desktop::properties("garbage").is_none());
    assert_eq!(desktop::dbus_send_string("method return time=1\n   string \"[{\"a\":1}]\"").as_deref(), Some("[{\"a\":1}]"));
    assert_eq!(cli::saved_desktop("desktop\tgnome\nvia\tdconf\n"), ("gnome", "via\tdconf\n"));
    assert_eq!(cli::saved_desktop("desktop\t\n"), ("", ""));
    assert_eq!(cli::saved_desktop("/backdrop/x\t1\n"), ("xfce", "/backdrop/x\t1\n"));
}

#[test]
fn source_registry() {
    for b in sources::BUILTINS {
        assert!(sources::builtin(b.name).is_some(), "{}", b.name);
        assert!(b.every > 0.0 && (0.0..=100.0).contains(&b.fixed));
        let mut s = (b.make)(None, true).expect("a made-up reader");
        for _ in 0..2 {
            if let Some((v, _)) = s.read() {
                assert!((0.0..=100.0).contains(&v), "{}: {v}", b.name);
            }
        }
    }
    assert_eq!(sources::builtin("used:/home").map(|(b, p)| (b.name, p)), Some(("used", Some("/home"))));
    assert_eq!(sources::builtin("CPU").map(|(b, _)| b.name), Some("cpu"));
    assert!(sources::builtin("cpu:/x").is_none(), "cpu takes no path");
    assert!(sources::builtin("gpu").is_none());
    // a path that cannot be a path: no reader, no panic
    let (used, path) = sources::builtin("used:/a\0b").unwrap();
    assert!((used.make)(path, false).is_none());
    assert_eq!(live::Frozen { at: 0.0, mains: true }.value("FREE"), 36.0);
    assert_eq!(live::Frozen { at: 0.0, mains: true }.value("mine"), 50.0);
}

#[test]
#[cfg(target_os = "linux")]
fn machine_tables() {
    let tcp = "  sl  local_address rem_address   st\n   0: 0100007F:170C 0100007F:9C40 01 0\n   1: 00000000:0016 00000000:0000 0A 0\n";
    assert!(system::established_to(tcp, 5900));
    assert!(!system::established_to(tcp, 22), "listening is not connected");
    let routes = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
        wg0\t00000000\t00000000\t0001\t0\t0\t50\t00000000\n\
        eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\n\
        eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n";
    let r = system::default_routes(routes);
    assert_eq!(r, [Route { gateway: None, metric: 50 }, Route { gateway: Some("192.168.1.1".into()), metric: 100 }]);
    // through a tunnel: no router shown
    assert_eq!(platform::gateway(&r), None);
    assert_eq!(platform::gateway(&r[1..]), Some("192.168.1.1".into()));
    assert_eq!(platform::gateway(&[]), None);
}

#[test]
fn time_of_day() {
    use retro_wall::day;
    assert_eq!(day::parse("13:37"), Some(13 * 3600 + 37 * 60));
    assert_eq!(day::parse("23:59:59"), Some(86399));
    for bad in ["24:00", "12:60", "1:2", "12:00:99999999999999999999", "12::00", "-1:00", "12:00:00:00"] {
        assert_eq!(day::parse(bad), None, "{bad}");
    }
    // a zone file cut short or lying about its counts: no offset, no panic
    if let Ok(real) = std::fs::read("/etc/localtime") {
        for n in [0, 4, 44, 60, real.len() / 2, real.len()] {
            let _ = day::tz_offset(&real[..n.min(real.len())], 1_700_000_000);
        }
    }
    let mut lying = b"TZif2".to_vec();
    lying.resize(44, 0xff);
    assert_eq!(day::tz_offset(&lying, 0), None);
}

#[test]
fn software_renderers_are_told_from_gpus() {
    for r in ["llvmpipe (LLVM 19.1.7, 128 bits)", "softpipe", "Apple Software Renderer"] {
        assert!(live::software(r), "{r}");
    }
    for r in ["Mesa Intel(R) HD Graphics 500 (APL 2)", "Apple M5 Pro", "NVIDIA GeForce RTX 3080/PCIe/SSE2"] {
        assert!(!live::software(r), "{r}");
    }
}
