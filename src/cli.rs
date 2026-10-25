//! The command line: parsing it into a command and its options, and the commands that are not the
//! live wall: --new (a theme, source or face to start from), --settings (what a theme's scene can
//! set), --list (what is available), --preview (a still picture to a file) and --desktop (the
//! desktop's wallpaper settings, for the installer).

use crate::config::{self, Theme};
use crate::platform::{Edges, Native, Platform, Screen};
use crate::scene::{Geo, Header};
use std::path::Path;
use std::sync::Arc;

pub const USAGE: &str = "usage: retro-wall [--theme NAME] [--config FILE] [--host NAME] [--battery]
                  [--pause-screen-off] [--pause-below PERCENT] [--pause-vnc PORT]
                  [--interface NAME]... [--ip-url URL] [--reserve-EDGE [MONITOR=]PX]...
                  [--software] [--demo [HH:mm]] [--all-monitors | MONITOR]
           the live desktop
       retro-wall --snapshot FILE [--at SECONDS] [--demo HH:mm] [--theme NAME]
                  [--host NAME|-] [--battery] [--reserve-EDGE PX]...
           one frame of it, to a PNG
       retro-wall --preview FILE WIDTHxHEIGHT [--theme NAME] [--battery] [--demo HH:mm]
                  [--reserve-EDGE PX]... [HOST|-]
           the still picture, to a PNG
       retro-wall --wallpaper [--all-monitors | MONITOR]
           set the desktop's still picture, and exit
       retro-wall --new theme NAME [EXTENDS] | --new source NAME | --new face NAME
           start a theme, a measurement or an instrument
       retro-wall --settings THEME          what a theme's scene can set
       retro-wall --list                    the themes, kinds, sources and widgets there are
       retro-wall --desktop which | save FILE | restore FILE | show
           the desktop, and its wallpaper settings
       retro-wall --version

  --all-monitors      every monitor, followed as they come and go; the header (hostname,
                      instruments, traffic) where the theme's [monitors] says. One monitor
                      alone shows all of it
  --battery           a BAT meter, the battery's charge
  --pause-screen-off  pause while the screen is off, or a laptop's lid is closed on its panel
  --pause-below PCT   on battery, pause only at PCT or less (by default, at any charge)
  --pause-vnc PORT    pause while a VNC viewer is connected to the server on PORT
  --interface NAME    count this interface's traffic, not the physical ones'; repeat for more
  --ip-url URL        where to ask for the public address (plain-text IPv4)
  --reserve-EDGE [MONITOR=]PX
                      EDGE is top, right, bottom or left: keep PX clear there, on every
                      monitor or the one named, instead of what the desktop's panels keep
  --software          animate on a software OpenGL renderer too (without it, on Linux, a
                      software-only display is drawn through VirtualGL's vglrun if installed)
  --demo [HH:mm]      made-up readings, and the time of day held at HH:mm
  --host NAME         the name shown, instead of this machine's; - shows no header";

/// What to do.
#[derive(Debug, PartialEq)]
pub enum Command {
    Live,
    Wallpaper,
    Snapshot(String),
    Preview(String, i32, i32),
}

/// --reserve-EDGE [MONITOR=]PX, each: (edge: 0 top, 1 right, 2 bottom, 3 left; the monitor, or None
/// for every one; pixels).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reserve(Vec<(usize, Option<String>, i32)>);

const EDGES: [&str; 4] = ["top", "right", "bottom", "left"];

impl Reserve {
    pub fn add(&mut self, edge: &str, value: &str) -> Result<(), String> {
        let i = EDGES.iter().position(|e| *e == edge).ok_or(format!("--reserve-{edge}: the edge is top, right, bottom or left"))?;
        let (monitor, px) = match value.rsplit_once('=') {
            Some((m, px)) if !m.is_empty() => (Some(m.to_string()), px),
            _ => (None, value),
        };
        let px = px.parse::<i32>().ok().filter(|p| (0..=100_000).contains(p)).ok_or(format!("--reserve-{edge} {value}: [MONITOR=]PIXELS"))?;
        self.0.push((i, monitor, px));
        Ok(())
    }

    /// What is kept along each side of `s`: what was said for it, else for every monitor, else `found`.
    pub fn apply(&self, s: &Screen, found: Edges) -> Edges {
        let mut e = [found.top, found.right, found.bottom, found.left];
        for (i, v) in e.iter_mut().enumerate() {
            let said = |named: bool| {
                self.0.iter().rev().find(|(edge, m, _)| *edge == i && m.as_deref().is_some() == named && m.as_deref().is_none_or(|m| m == s.name))
            };
            if let Some((_, _, px)) = said(true).or_else(|| said(false)) {
                *v = *px;
            }
            *v = (*v).clamp(0, if i % 2 == 0 { s.h } else { s.w });
        }
        Edges { top: e[0], right: e[1], bottom: e[2], left: e[3] }
    }
}

/// Every option, as parsed.
#[derive(Debug, PartialEq)]
pub struct Args {
    pub theme: Option<String>,
    pub config: Option<String>,
    pub host: Option<String>,
    pub battery: bool,
    pub demo: bool,
    /// --demo HH:mm: the time of day held.
    pub demo_time: Option<String>,
    pub monitor: Option<String>,
    pub all_monitors: bool,
    pub pause_screen_off: bool,
    pub pause_below: f64,
    pub pause_vnc: Option<u16>,
    pub interfaces: Vec<String>,
    pub ip_url: String,
    pub software: bool,
    pub reserve: Reserve,
    pub at: f64,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            theme: None,
            config: None,
            host: None,
            battery: false,
            demo: false,
            demo_time: None,
            monitor: None,
            all_monitors: false,
            pause_screen_off: false,
            pause_below: 100.0,
            pause_vnc: None,
            interfaces: vec![],
            ip_url: "https://ip.c-net.org/".into(),
            software: false,
            reserve: Reserve::default(),
            at: 7.25,
        }
    }
}

/// WIDTHxHEIGHT, each 1 to 16384.
pub fn size(text: &str) -> Result<(i32, i32), String> {
    let wh = text.split_once('x').and_then(|(w, h)| Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?)));
    match wh {
        Some((w, h)) if (1..=16384).contains(&w) && (1..=16384).contains(&h) => Ok((w, h)),
        _ => Err(format!("{text}: WIDTHxHEIGHT, each 1 to 16384")),
    }
}

/// Digits and colons, as HH:mm or HH:mm:ss: meant as a time, whether or not it is one.
fn time_shaped(v: &str) -> bool {
    let parts: Vec<&str> = v.split(':').collect();
    (2..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn number(v: &str, what: &str, lo: f64, hi: f64) -> Result<f64, String> {
    v.parse::<f64>().ok().filter(|n| n.is_finite() && (lo..=hi).contains(n)).ok_or(format!("{what} {v}: a number, {lo} to {hi}"))
}

/// The command and its options. Err: what is wrong, for the usage message.
pub fn parse(args: &[String]) -> Result<(Command, Args), String> {
    let mut a = Args::default();
    let mut it = args.iter().map(String::as_str).peekable();
    let command = match it.peek().copied() {
        Some("--preview") => {
            it.next();
            let file = it.next().ok_or("--preview FILE WIDTHxHEIGHT")?;
            let (w, h) = size(it.next().ok_or("--preview FILE WIDTHxHEIGHT")?)?;
            Command::Preview(file.to_string(), w, h)
        }
        Some("--wallpaper") => {
            it.next();
            Command::Wallpaper
        }
        _ => Command::Live,
    };
    let mut command = command;
    let mut positional: Option<String> = None;
    while let Some(arg) = it.next() {
        let mut value = |what: &str| it.next().map(str::to_string).ok_or(format!("{arg} {what}"));
        match arg {
            "--theme" => a.theme = Some(value("NAME")?),
            "--config" => a.config = Some(value("FILE")?),
            "--host" => a.host = Some(value("NAME")?),
            "--battery" => a.battery = true,
            "--software" => a.software = true,
            "--all-monitors" => a.all_monitors = true,
            "--pause-screen-off" => a.pause_screen_off = true,
            "--pause-below" => a.pause_below = number(&value("PERCENT")?, arg, 0.0, 100.0)?,
            "--pause-vnc" => {
                let v = value("PORT")?;
                a.pause_vnc = Some(v.parse::<u16>().ok().filter(|p| *p > 0).ok_or(format!("--pause-vnc {v}: a TCP port"))?);
            }
            "--interface" => a.interfaces.push(value("NAME")?),
            "--ip-url" => a.ip_url = value("URL")?,
            "--at" => a.at = number(&value("SECONDS")?, arg, 0.0, 1e9)?,
            "--snapshot" if command == Command::Live => command = Command::Snapshot(value("FILE")?),
            "--demo" => {
                a.demo = true;
                if let Some(t) = it.next_if(|v| time_shaped(v)) {
                    crate::day::parse(t).ok_or(format!("--demo {t}: HH:mm, a time of day"))?;
                    a.demo_time = Some(t.to_string());
                }
            }
            r if r.starts_with("--reserve-") => {
                let v = value("[MONITOR=]PX")?;
                a.reserve.add(&r["--reserve-".len()..], &v)?;
            }
            s if s.starts_with('-') && s != "-" => return Err(format!("{s}: not an option here")),
            s => {
                if positional.is_some() {
                    return Err(format!("{s}: one {} only", if matches!(command, Command::Preview(..)) { "HOST" } else { "MONITOR" }));
                }
                positional = Some(s.to_string());
            }
        }
    }
    match (&command, positional) {
        (Command::Preview(..), Some(h)) => a.host = Some(h),
        (Command::Snapshot(_), Some(p)) => return Err(format!("{p}: a snapshot is of the primary monitor")),
        (_, Some(m)) if a.all_monitors => return Err(format!("{m}: --all-monitors or a MONITOR, not both")),
        (_, Some(m)) => a.monitor = Some(m),
        (_, None) => {}
    }
    Ok((command, a))
}

/// A name for something new: a file name, not a path.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\0']) && !name.starts_with('-')
}

const THEME: &str = r###"# NAME: a theme of your own. It starts as a copy of BASE in everything it does not say.
# See it:   retro-wall --preview /tmp/NAME.png 1920x1080 --theme NAME
#           retro-wall --snapshot /tmp/NAME.png --theme NAME       (the live parts too)
# Use it:   theme = "NAME" in ~/.config/retro/retro.toml, or retro-wall --theme NAME
# What can be set: retro-wall --settings NAME
name = "NAME"
extends = "BASE"
# The scene, bottom to top: BASE's, unless you name your own (retro-wall --settings BASE shows BASE's).
# scene = ["room", "mountains", "title", "instruments", "finish"]

[palette]                   # the colours, by role; or hue = 200 to turn them all about the colour wheel
# hue = 200
# base = "#07050f"          # the sky
# ramp = ["#2a1450", "#3a1d6a", "#55288a", "#7b3ab0", "#ff4fb0", "#ff86cf"]   # dark to bright
# caption = "#4fb6d6"       # the lettering of the instruments

[tweaks]                    # the core's shapes and sizes
# vignette = 0.5            # how dark the corners get
# title_corner = "bottom-left"
# scanlines = false

# A picture of your own is a painting (with the scene named above):
# [element.room]
# type = "painting"
# image = "room.png"        # beside this file; set canvas = [width, height] in [tweaks] to its size

# Motion of your own, on the GPU: a fragment shader beside this file (shaders/glow.frag is an example).
# Add "glow" to the scene where it should be drawn, between what it goes over and what goes over it.
# [element.glow]
# type = "shader"
# fragment = "shaders/glow.frag"
# area = [0.0, 0.0, 1.0, 1.0]   # left, top, right, bottom: fractions of the screen
# readings = ["cpu"]            # readings.x: the CPU, 0 to 1
"###;

const SHADER: &str = r###"// A soft light that breathes with the CPU. Given: time (seconds), resolution, area, readings (0 to 1),
// tex0..tex3 (the pictures named in `textures`), and uv (0 to 1 over the area). Premultiplied output.
void main() {
    vec2 p = uv - vec2(0.5, 0.6);
    float glow = exp(-dot(p, p) * 12.0) * (0.15 + 0.25 * readings.x) * (0.8 + 0.2 * sin(time * 0.7));
    gl_FragColor = vec4(vec3(1.0, 0.4, 0.8) * glow, glow);
}
"###;

const SOURCE: &str = r###"#!/bin/sh
# NAME: a measurement of your own, for retro-wall. It prints its reading, 0 to 100, and exits; retro-wall
# runs it every 2 seconds (set `every` in a [source.NAME] table in retro.toml for another pace).
# It may print JSON instead: {"value": 42, "indicator": true} (a meter's light), or {"text": "..."}.
# A program that keeps running and prints a line whenever it has something new: print {"stream": true}
# first. Any language will do.
#
# Show it: in ~/.config/retro/retro.toml
#
#   [widget.NAME]
#   type = "meter"            # or ring, bar, spark, cells, or a face of your own
#   source = "NAME"
#   label = "NAME"
#
# and put "NAME" in a [[group]]'s widgets. Test it by running it: it should print one number.

# the load average, per core, as a percentage (/proc on Linux, sysctl on macOS and the BSDs)
cores=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
if [ -r /proc/loadavg ]; then load=$(cut -d' ' -f1 /proc/loadavg); else load=$(sysctl -n vm.loadavg | awk '{ print $2 }'); fi
awk -v l="$load" -v n="$cores" 'BEGIN { v = 100 * l / n; if (v > 100) v = 100; printf "%.1f\n", v }'
"###;

const FACE: &str = r###"#!/bin/sh
# NAME: an instrument of your own, for retro-wall. It keeps running: for each picture retro-wall needs
# (only when what it shows changes), it reads a line of JSON and answers with a line: a list of
# drawing operations, which retro-wall draws as it draws its own instruments.
#
# This one ignores what it is asked and draws the same frame and caption each time. The request (the
# picture's size, the reading, the theme's colours) and every operation are in EXTENDING.md, under
# "An instrument"; a face that reads them is best written in a language with a JSON parser.
#
# Use it: in ~/.config/retro/retro.toml
#
#   [widget.mine]
#   type = "NAME"
#   source = "cpu"            # any source: cpu, ram, swap, bat, disk, used, free, or your own
#   label = "CPU"
#   size = [1, 1]             # in cards: columns, rows
#
# and put "mine" in a [[group]]'s widgets.

while read -r _request; do
    echo '[{"op": "rect", "x": 10, "y": 10, "w": 150, "h": 70}, {"op": "stroke", "colour": "#4fb6d6", "width": 1},' \
         '{"op": "glow"}, {"op": "text", "text": "NAME", "x": 85, "y": 105, "align": "centre", "bold": true}]'
done
"###;

/// On Windows a plugin is a .cmd (it has no execute bit); it starts PowerShell on the script beside it.
#[cfg(windows)]
const LAUNCHER: &str = "@powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dpn0.ps1\"\r\n";

#[cfg(windows)]
const SOURCE_PS: &str = r###"# NAME: a measurement of your own, for retro-wall. NAME.cmd beside it starts it. It prints its reading,
# 0 to 100, and exits; retro-wall runs it every 2 seconds (set `every` in a [source.NAME] table in
# retro.toml for another pace). It may print JSON instead: {"value": 42, "indicator": true}, or
# {"text": "..."}. A program that keeps running prints {"stream": true} first, then a line whenever it
# has something new. See EXTENDING.md.
#
# Show it: in retro.toml (%APPDATA%\retro\retro.toml)
#
#   [widget.NAME]
#   type = "meter"            # or ring, bar, spark, cells, or a face of your own
#   source = "NAME"
#   label = "NAME"
#
# and put "NAME" in a [[group]]'s widgets. Test it by running NAME.cmd: it should print one number.

# the processors' load, in percent
$load = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
[Console]::Out.WriteLine(([Math]::Round([double]$load, 1)).ToString([Globalization.CultureInfo]::InvariantCulture))
"###;

#[cfg(windows)]
const FACE_PS: &str = r###"# NAME: an instrument of your own, for retro-wall. NAME.cmd beside it starts it. It keeps running:
# for each picture retro-wall needs, it reads a line of JSON and answers with a line, a list of
# drawing operations. This one ignores what it is asked and draws the same frame and caption each
# time; the request and the operations are in EXTENDING.md, under "An instrument"
# (ConvertFrom-Json reads a request).
#
# Use it: in retro.toml, a [widget.mine] with type = "NAME" and a source, and "mine" in a [[group]]'s
# widgets.

while ($null -ne ($request = [Console]::In.ReadLine())) {
    [Console]::Out.WriteLine('[{"op": "rect", "x": 10, "y": 10, "w": 150, "h": 70}, {"op": "stroke", "colour": "#4fb6d6", "width": 1}, {"op": "glow"}, {"op": "text", "text": "NAME", "x": 85, "y": 105, "align": "centre", "bold": true}]')
    [Console]::Out.Flush()
}
"###;

fn write_new(path: &Path, text: &str, exec: bool) -> bool {
    if path.exists() {
        crate::errln!("{} is there already: left as it is", path.display());
        return false;
    }
    let made = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(path, text)).and_then(|_| executable(path, exec));
    if let Err(e) = made {
        crate::errln!("{}: {e}", path.display());
        std::process::exit(1);
    }
    crate::outln!("wrote {}", path.display());
    true
}

/// Mark a file executable when `exec`; on Windows its extension makes it so.
fn executable(path: &Path, exec: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    if exec {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    let _ = (path, exec);
    Ok(())
}

/// --new theme NAME [BASE] | --new source NAME | --new face NAME
pub fn new(what: &str, name: &str, base: Option<&str>) {
    if !plain_name(name) || base.is_some_and(|b| b.is_empty()) {
        crate::errln!("--new {what} {name:?}: a name, not a path");
        std::process::exit(2);
    }
    let dir = config::config_dir();
    match what {
        "theme" => {
            let base = base.unwrap_or(config::DEFAULT_THEME);
            let t = dir.join("themes").join(name);
            write_new(&t.join("theme.toml"), &THEME.replace("NAME", name).replace("BASE", base), false);
            write_new(&t.join("shaders/glow.frag"), SHADER, false);
        }
        "source" | "face" => {
            let (place, script) = if what == "source" { ("sources", SOURCE) } else { ("faces", FACE) };
            #[cfg(windows)]
            let script = {
                let _ = script;
                write_new(&dir.join(place).join(format!("{name}.cmd")), LAUNCHER, true);
                if what == "source" { SOURCE_PS } else { FACE_PS }
            };
            #[cfg(windows)]
            let file = dir.join(place).join(format!("{name}.ps1"));
            #[cfg(not(windows))]
            let file = dir.join(place).join(name);
            write_new(&file, &script.replace("NAME", name), true);
        }
        _ => {
            crate::errln!("--new theme NAME [EXTENDS] | --new source NAME | --new face NAME");
            std::process::exit(2);
        }
    }
}

/// --settings THEME: every element of its scene with each of its settings, as the theme has it.
pub fn settings(spec: &str) {
    let cfg = config::Config::default();
    if let Err(e) = config::resolve(spec, &cfg, crate::elements::kind_info, false).map(|l| {
        for e in &l.plain.scene {
            crate::outln!("[element.{}]   # type = \"{}\"", e.name, e.kind);
            for (k, v) in &e.settings {
                crate::outln!("{k} = {v}");
            }
            crate::outln!();
        }
        crate::outln!("[tweaks]   # every tweak and its default: retro-wall's core.toml");
        for (k, v) in config::core()["tweaks"].as_table().into_iter().flatten() {
            crate::outln!("{k} = {v}");
        }
    }) {
        crate::errln!("{e}");
        std::process::exit(1);
    }
}

/// --list: the themes, element kinds, sources and widget types.
pub fn list() {
    match crate::font::registered() {
        files if files.is_empty() => crate::outln!("font: the system's {}, or its nearest (the embedded faces were not taken)", crate::font::FAMILY),
        files => {
            crate::outln!("font: {}, embedded, from {}", crate::font::FAMILY, files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(" and "))
        }
    }
    crate::outln!("themes:");
    for d in config::theme_dirs() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut names: Vec<String> =
                rd.flatten().filter(|e| e.path().join("theme.toml").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            crate::outln!("  {}: {}", d.display(), names.join(", "));
        }
    }
    let mut kinds: Vec<&str> = crate::elements::all().map(|k| k.name).collect();
    kinds.sort();
    crate::outln!("element kinds: {}", kinds.join(", "));
    let mut plugins: Vec<String> =
        crate::plugins::source_specs(&config::read_config(None).map(|c| c.sources).unwrap_or_default()).into_iter().map(|s| s.name).collect();
    plugins.sort();
    crate::outln!(
        "sources: {}{}",
        crate::sources::builtin_names().join(", "),
        if plugins.is_empty() { String::new() } else { format!("; yours: {}", plugins.join(", ")) }
    );
    let mut faces: Vec<String> = crate::plugins::face_plugins().into_keys().collect();
    faces.sort();
    crate::outln!(
        "widget types: {}{}",
        crate::elements::instruments::type_names().collect::<Vec<_>>().join(", "),
        if faces.is_empty() { String::new() } else { format!("; yours: {}", faces.join(", ")) }
    );
}

/// --preview FILE WxH [HOST|-]: the still picture, at that size, to a file.
pub fn preview(th: Arc<Theme>, path: &str, w: i32, h: i32, host: &str, header: bool, reserve: &Reserve) {
    let mut file = std::fs::File::create(path).unwrap_or_else(|e| {
        crate::errln!("--preview {path}: {e}");
        std::process::exit(2)
    });
    let header = if header { Header::ALL } else { Header::NONE };
    let screen = Screen { name: String::new(), x: 0, y: 0, w, h, primary: true };
    let g = Geo::new(w, h, host, reserve.apply(&screen, Edges::NONE), header, None, th);
    let written = crate::still::draw(&g).and_then(|pic| pic.write_to_png(&mut file).map_err(|e| e.to_string()));
    if let Err(e) = written {
        crate::errln!("--preview {path}: {e}");
        std::process::exit(2);
    }
    crate::outln!("{path}: {w}x{h}");
}

/// --desktop which | save FILE | restore FILE | show: the desktop that draws the wallpaper, and its
/// wallpaper settings, saved to a file and put back from it exactly.
pub fn desktop(args: &[String]) {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let saved = || {
        let d = Native::desktop();
        let body = d.as_ref().map_or(Ok(String::new()), |d| d.save())?;
        Ok::<String, String>(format!("desktop\t{}\n{body}", d.map_or("", |d| d.name())))
    };
    let result = match words.as_slice() {
        ["which"] => {
            crate::outln!("{}", Native::desktop().map_or("", |d| d.name()));
            Ok(())
        }
        ["show"] => saved().map(|s| crate::out!("{s}")),
        ["save", file] => saved().and_then(|s| std::fs::write(file, s).map_err(|e| format!("{file}: {e}"))),
        ["restore", file] => crate::files::read_text(Path::new(file), crate::files::Kind::Settings).map_err(|e| format!("{file}: {e}")).and_then(|text| {
            let (name, body) = saved_desktop(&text);
            match name {
                "" => Ok(()),
                n => Native::desktop_named(n).ok_or(format!("{file}: {n} is not a desktop retro-wall knows"))?.restore(body),
            }
        }),
        _ => {
            crate::errln!("--desktop which | save FILE | restore FILE | show");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        crate::errln!("retro-wall: {e}");
        std::process::exit(1);
    }
}

/// A saved file's desktop and settings. A file with no desktop line is XFCE's.
pub fn saved_desktop(text: &str) -> (&str, &str) {
    match text.strip_prefix("desktop\t") {
        Some(rest) => rest.split_once('\n').map_or((rest.trim(), ""), |(n, body)| (n.trim(), body)),
        None => ("xfce", text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Result<(Command, Args), String> {
        parse(&line.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn demo_takes_a_time_when_one_follows() {
        let (_, a) = parsed("--demo 13:37").unwrap();
        assert!(a.demo && a.demo_time.as_deref() == Some("13:37") && a.monitor.is_none());
        let (_, a) = parsed("--demo 07:05:09").unwrap();
        assert_eq!(a.demo_time.as_deref(), Some("07:05:09"));
        // a monitor after --demo is the monitor
        let (_, a) = parsed("--demo HDMI-1").unwrap();
        assert!(a.demo && a.demo_time.is_none());
        assert_eq!(a.monitor.as_deref(), Some("HDMI-1"));
    }

    #[test]
    fn demo_with_a_time_that_is_not_one_is_an_error() {
        for t in ["99:99", "24:00", "12:60", "1:2:99"] {
            let e = parsed(&format!("--demo {t}")).err();
            assert!(e.as_deref().is_some_and(|e| e.contains("a time of day")), "{t}: {e:?}");
        }
        assert!(time_shaped("99:99") && time_shaped("1:2:3") && !time_shaped("1:2:3:4"));
        assert!(!time_shaped("DP-1") && !time_shaped(":0") && !time_shaped("12:") && !time_shaped("1a:00"));
    }

    #[test]
    fn sizes_and_numbers_are_checked() {
        assert_eq!(size("640x360"), Ok((640, 360)));
        for bad in ["0x10", "10x0", "16385x10", "x", "10x", "axb", "10*10", "-1x5"] {
            assert!(size(bad).is_err(), "{bad}");
        }
        assert_eq!(number("50", "--pause-below", 0.0, 100.0), Ok(50.0));
        for bad in ["inf", "NaN", "-1", "101", "", "5%"] {
            assert!(number(bad, "--pause-below", 0.0, 100.0).is_err(), "{bad}");
        }
    }

    #[test]
    fn options_and_positionals() {
        let (c, a) = parsed("--preview out.png 64x36 --theme aurora -").unwrap();
        assert_eq!(c, Command::Preview("out.png".into(), 64, 36));
        assert_eq!((a.theme.as_deref(), a.host.as_deref()), (Some("aurora"), Some("-")));
        assert_eq!(parsed("--snapshot x.png").unwrap().0, Command::Snapshot("x.png".into()));
        assert!(parsed("--snapshot x.png DP-1").is_err());
        assert!(parsed("DP-1 HDMI-1").is_err());
        assert!(parsed("--all-monitors DP-1").is_err());
        assert!(parsed("--frob").is_err());
        assert!(parsed("--theme").is_err());
        assert_eq!(parsed("--pause-vnc 5900").unwrap().1.pause_vnc, Some(5900));
        assert!(parsed("--pause-vnc 0").is_err());
    }

    #[test]
    fn reserved_edges_take_an_optional_monitor() {
        let mut r = Reserve::default();
        assert!(r.add("top", "30").is_ok());
        assert!(r.add("left", "DP-1=40").is_ok());
        assert!(r.add("middle", "1").is_err());
        assert!(r.add("top", "-1").is_err());
        assert!(r.add("top", "DP-1=").is_err());
        let s = Screen { name: "DP-1".into(), x: 0, y: 0, w: 100, h: 50, primary: true };
        let e = r.apply(&s, Edges { top: 1, right: 2, bottom: 3, left: 4 });
        assert_eq!((e.top, e.right, e.bottom, e.left), (30, 2, 3, 40));
    }

    #[test]
    fn plain_names_and_saved_desktops() {
        assert!(plain_name("mine") && !plain_name("../x") && !plain_name("a/b") && !plain_name(""));
        assert_eq!(saved_desktop("desktop\tkde\nbody\n"), ("kde", "body\n"));
        assert_eq!(saved_desktop("a\tb\n"), ("xfce", "a\tb\n"));
    }
}
