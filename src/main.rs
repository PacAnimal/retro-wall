//! retro-wall: a live desktop wallpaper. A theme is a directory with theme.toml (settings and scene)
//! and the pictures it names.

// on Windows a GUI program: no console window of its own
#![cfg_attr(windows, windows_subsystem = "windows")]
// output goes through outln!/errln!: println! and eprintln! panic when the reader has gone (EPIPE)
#![warn(clippy::print_stdout, clippy::print_stderr)]

use retro_wall::cli::{self, Command, USAGE};
use retro_wall::platform::{Native, Platform};
use retro_wall::{config, day, elements, font, live, sources, util};

/// This display's arguments from retro-wall.conf (in the settings directory): lines of display, then
/// arguments, a word with a space in double quotes. On Windows the display is `windows`, the line the
/// setup program writes.
fn configured() -> Vec<String> {
    use retro_wall::setup::settings::{CONF_KEY, words};
    let key = |d: &str| {
        if cfg!(windows) {
            return d.to_string();
        }
        let (host, rest) = d.rsplit_once(':').unwrap_or(("", d));
        format!("{host}:{}", if rest.contains('.') { rest.to_string() } else { format!("{rest}.0") })
    };
    let display = if cfg!(windows) { CONF_KEY.to_string() } else { key(&std::env::var("DISPLAY").unwrap_or_default()) };
    let text = retro_wall::files::read_text(&config::config_dir().join("retro-wall.conf"), retro_wall::files::Kind::Settings).unwrap_or_default();
    for line in text.lines() {
        let w = words(line);
        if !w.is_empty() && key(&w[0]) == display {
            return w[1..].to_vec();
        }
    }
    vec![]
}

fn usage(why: &str) -> ! {
    retro_wall::errln!("retro-wall: {why}\n{USAGE}");
    std::process::exit(2)
}

fn main() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    retro_wall::platform::linux::map_large_allocations();
    #[cfg(windows)]
    retro_wall::platform::windows::attach_output();
    font::register();
    util::settle_fonts();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--new") if (3..=4).contains(&args.len()) => return cli::new(&args[1], &args[2], args.get(3).map(String::as_str)),
        Some("--new") => usage("--new theme NAME [EXTENDS] | --new source NAME | --new face NAME"),
        Some("--settings") if args.len() == 2 => return cli::settings(&args[1]),
        Some("--settings") => usage("--settings THEME"),
        Some("--list") => return cli::list(),
        Some("--desktop") => return cli::desktop(&args[1..]),
        Some("--version") => {
            retro_wall::outln!("retro-wall {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("-h") | Some("--help") => {
            retro_wall::outln!("{USAGE}");
            return;
        }
        _ => {}
    }
    if args.is_empty() || args == ["--wallpaper"] {
        args.extend(configured()); // this display's options, for the autostarted wall
    }
    let (command, a) = cli::parse(&args).unwrap_or_else(|e| usage(&e));
    if let Some(t) = &a.demo_time {
        day::hold(t);
    }
    if matches!(command, Command::Snapshot(_)) && !day::held() {
        day::hold("13:37"); // a snapshot is repeatable: hold the time of day too
    }
    sources::count_interfaces(a.interfaces.clone());
    let cfg = config::read_config(a.config.as_deref()).unwrap_or_else(|e| {
        retro_wall::errln!("config: {e}: the default layout instead");
        config::Config::default()
    });
    for p in &cfg.problems {
        retro_wall::errln!("config: {p}");
    }
    let loaded = config::apply(a.theme.as_deref(), &cfg, elements::kind_info, a.battery || a.demo || matches!(command, Command::Snapshot(_)));
    let header = a.host.as_deref() != Some("-");
    let host = match &a.host {
        Some(h) if h != "-" => h.clone(),
        _ => cfg.host.clone().unwrap_or_else(Native::hostname),
    };
    let options = |a: cli::Args| live::Options {
        host: host.clone(),
        monitor: a.monitor,
        all_monitors: a.all_monitors,
        pause_screen_off: a.pause_screen_off,
        pause_below: a.pause_below,
        pause_vnc: a.pause_vnc,
        pause_rdp: a.pause_rdp,
        ip_url: a.ip_url,
        software: a.software,
        demo: a.demo,
        reserve: a.reserve,
    };
    match command {
        Command::Preview(file, w, h) => cli::preview(loaded.look(day::lighting_phase(&loaded).as_deref()), &file, w, h, &host, header, &a.reserve),
        Command::Snapshot(file) => {
            let shot = a.frames.map_or(live::Shot::Still, |count| live::Shot::Frames { count, fps: a.fps });
            live::snapshot(&loaded, &file, &host, header, a.at, &a.reserve, shot)
        }
        Command::Wallpaper => live::wallpapers(&loaded, &options(a)),
        Command::Live => live::run(&loaded, options(a)),
    }
}
