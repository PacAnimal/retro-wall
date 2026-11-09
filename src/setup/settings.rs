//! The settings the setup program shows, and the files they are kept in, in the user's settings
//! directory (`config::config_dir`, `%APPDATA%\retro` on Windows):
//!
//! - `retro-wall.conf`: the wall's options, on the line for `CONF_KEY`. The scheduled task starts the
//!   wall with none, and it reads them there.
//! - `retro.toml`: where each piece of the header shows (`[monitors]`), and which instruments
//!   (`[[group]]`, written only when one is hidden).
//! - a theme of the user's own, `setup-NAME`, that extends a theme to change what its scene can: blocky's
//!   interface and hand. Written only when they differ from the theme's.
//!
//! Each is changed only where the settings say: other lines, keys, comments and options it does not
//! know are kept as they were.

use crate::config::{Direction, Group};
use std::path::Path;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

/// The key of the Windows desktop's line in `retro-wall.conf`.
pub const CONF_KEY: &str = "windows";

/// The prefix of the themes the setup program writes for a theme's extras.
pub const OWN_THEME: &str = "setup-";

/// Which monitors the wall covers.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Screens {
    /// The main (primary) monitor.
    #[default]
    Main,
    /// Every monitor, followed as they come and go.
    All,
    /// One monitor, by its name (DISPLAY2).
    One(String),
}

/// Where a piece of the header shows, among the monitors (`[monitors]` in retro.toml).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Place {
    /// As the theme has it.
    #[default]
    Theme,
    Rightmost,
    Leftmost,
    Primary,
    All,
    None,
    /// These monitors, by name.
    Named(Vec<String>),
}

impl Place {
    /// Its value in retro.toml; None for `Theme` (no key).
    fn item(&self) -> Option<Item> {
        Some(match self {
            Place::Theme => return None,
            Place::Rightmost => value("rightmost"),
            Place::Leftmost => value("leftmost"),
            Place::Primary => value("primary"),
            Place::All => value("all"),
            Place::None => value("none"),
            Place::Named(names) => value(names.iter().collect::<Array>()),
        })
    }
    fn of(item: Option<&Item>) -> Place {
        let Some(item) = item else { return Place::Theme };
        if let Some(a) = item.as_array() {
            return Place::Named(a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        }
        match item.as_str() {
            Some("rightmost") => Place::Rightmost,
            Some("leftmost") => Place::Leftmost,
            Some("primary") => Place::Primary,
            Some("all") => Place::All,
            Some("none") => Place::None,
            _ => Place::Theme,
        }
    }
}

/// What the wall does on battery power.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum OnBattery {
    /// Pauses (the wall's own default).
    #[default]
    Pause,
    /// Pauses only at this charge, in percent, or less.
    Below(u8),
    /// Keeps animating.
    Keep,
}

/// The instruments the setup program can hide, as the wall names them.
/// The battery's meter is not one: the battery setting shows it (`--battery`).
pub const INSTRUMENTS: [&str; 5] = ["cpu", "ram", "swap", "deck", "status"];

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Settings {
    /// The theme; None for the wall's default (phosphor).
    pub theme: Option<String>,
    pub screens: Screens,
    /// The name shown; None for this machine's.
    pub host: Option<String>,
    /// A BAT meter.
    pub battery: bool,
    pub on_battery: OnBattery,
    pub pause_screen_off: bool,
    /// Pause while a remote desktop client is connected to this port (3389 for Windows' own).
    pub pause_rdp: Option<u16>,
    /// Pause while a VNC viewer is connected to this port.
    pub pause_vnc: Option<u16>,
    /// Count these network interfaces' traffic; none for the physical ones'.
    pub interfaces: Vec<String>,
    /// Where to ask for the public address; None for the wall's default.
    pub ip_url: Option<String>,
    /// Animate on a software renderer too.
    pub software: bool,
    /// Options it does not show, kept as they are.
    pub extra: Vec<String>,
    /// Where the hostname, the instruments, the traffic graph and the status line show.
    pub title: Place,
    pub widgets: Place,
    pub graph: Place,
    pub status: Place,
    /// Instruments hidden (`INSTRUMENTS`).
    pub hidden: Vec<String>,
    /// Blocky's interface and hand; None as the theme has them.
    pub ui: Option<bool>,
    pub hand: Option<bool>,
    /// The user lays out the instruments in retro.toml with widgets of their own: the setup program
    /// leaves their `[[group]]` as it is, and hides none.
    pub custom_layout: bool,
}

/// Whether retro.toml's `[[group]]` is one the setup program can manage: every widget in it its own.
fn managed_groups(groups: &ArrayOfTables) -> bool {
    groups
        .iter()
        .filter_map(|g| g.get("widgets").and_then(Item::as_array))
        .flatten()
        .all(|v| v.as_str().is_some_and(|w| w == "bat" || INSTRUMENTS.contains(&w)))
}

/// Options the wall takes a value with that the settings do not show: kept with their value.
fn takes_value(opt: &str) -> bool {
    matches!(opt, "--config" | "--at" | "--fps" | "--frames" | "--snapshot") || opt.starts_with("--reserve-")
}

/// A time of day as `--demo` takes one: HH:mm or HH:mm:ss.
fn time_shaped(v: &str) -> bool {
    let parts: Vec<&str> = v.split(':').collect();
    (2..=3).contains(&parts.len()) && parts.iter().all(|p| (1..=2).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()))
}

/// The keys of `Settings::set` and `Settings::pairs`, the setup program's command line.
pub const KEYS: [&str; 18] = [
    "theme",
    "screens",
    "host",
    "battery",
    "on-battery",
    "screen-off",
    "rdp",
    "vnc",
    "interfaces",
    "ip-url",
    "software",
    "title",
    "widgets",
    "graph",
    "status",
    "hide",
    "ui",
    "hand",
];

fn on_off(v: &str) -> Result<bool, String> {
    match v {
        "on" | "yes" | "true" => Ok(true),
        "off" | "no" | "false" => Ok(false),
        _ => Err(format!("{v:?}: on or off")),
    }
}

fn maybe(v: Option<bool>) -> String {
    v.map_or("theme".into(), |b| if b { "on".into() } else { "off".into() })
}

fn list(v: &str) -> Vec<String> {
    v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

impl Place {
    fn parse(v: &str) -> Place {
        match v {
            "theme" => Place::Theme,
            "rightmost" => Place::Rightmost,
            "leftmost" => Place::Leftmost,
            "primary" => Place::Primary,
            "all" => Place::All,
            "none" => Place::None,
            names => Place::Named(list(names)),
        }
    }
    fn name(&self) -> String {
        match self {
            Place::Theme => "theme".into(),
            Place::Rightmost => "rightmost".into(),
            Place::Leftmost => "leftmost".into(),
            Place::Primary => "primary".into(),
            Place::All => "all".into(),
            Place::None => "none".into(),
            Place::Named(n) => n.join(","),
        }
    }
}

impl Settings {
    /// Set one setting from its key and a word: the setup program's `--set KEY=VALUE`.
    pub fn set(&mut self, key: &str, v: &str) -> Result<(), String> {
        let port = |v: &str| if v == "off" { Ok(None) } else { v.parse::<u16>().map(Some).map_err(|_| format!("{v:?}: a port, or off")) };
        if v.contains('"') {
            return Err(format!("{key}: no double quotes in a value"));
        }
        let opt = |v: &str| if v.is_empty() || v == "default" { None } else { Some(v.to_string()) };
        let extra = |v: &str| match v {
            "theme" => Ok(None),
            v => on_off(v).map(Some),
        };
        match key {
            "theme" => self.theme = opt(v),
            "screens" => {
                self.screens = match v {
                    "main" => Screens::Main,
                    "all" => Screens::All,
                    m if !m.is_empty() => Screens::One(m.to_string()),
                    _ => return Err("screens: main, all, or a monitor's name".into()),
                }
            }
            "host" => self.host = opt(v),
            "battery" => self.battery = on_off(v)?,
            "on-battery" => {
                self.on_battery = match v {
                    "pause" => OnBattery::Pause,
                    "keep" => OnBattery::Keep,
                    n => OnBattery::Below(
                        n.trim_end_matches('%')
                            .parse::<u8>()
                            .ok()
                            .filter(|n| (1..=100).contains(n))
                            .ok_or(format!("on-battery: pause, keep, or a charge 1 to 100, not {n:?}"))?,
                    ),
                }
            }
            "screen-off" => self.pause_screen_off = on_off(v)?,
            "rdp" => self.pause_rdp = port(v)?,
            "vnc" => self.pause_vnc = port(v)?,
            "interfaces" => self.interfaces = list(v),
            "ip-url" => self.ip_url = opt(v),
            "software" => self.software = on_off(v)?,
            "title" => self.title = Place::parse(v),
            "widgets" => self.widgets = Place::parse(v),
            "graph" => self.graph = Place::parse(v),
            "status" => self.status = Place::parse(v),
            "hide" => {
                let hide = list(v);
                if let Some(bad) = hide.iter().find(|h| !INSTRUMENTS.contains(&h.as_str())) {
                    return Err(format!("hide: {bad:?} is none of {}", INSTRUMENTS.join(", ")));
                }
                self.hidden = INSTRUMENTS.iter().filter(|i| hide.iter().any(|h| h == *i)).map(|i| i.to_string()).collect();
            }
            "ui" => self.ui = extra(v)?,
            "hand" => self.hand = extra(v)?,
            _ => return Err(format!("no setting {key:?}: {}", KEYS.join(", "))),
        }
        Ok(())
    }

    /// Every setting as its key and word, as `set` takes them.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        let port = |p: Option<u16>| p.map_or("off".into(), |p| p.to_string());
        let on = |b: bool| if b { "on".to_string() } else { "off".to_string() };
        vec![
            ("theme", self.theme.clone().unwrap_or_else(|| "default".into())),
            (
                "screens",
                match &self.screens {
                    Screens::Main => "main".into(),
                    Screens::All => "all".into(),
                    Screens::One(m) => m.clone(),
                },
            ),
            ("host", self.host.clone().unwrap_or_else(|| "default".into())),
            ("battery", on(self.battery)),
            (
                "on-battery",
                match self.on_battery {
                    OnBattery::Pause => "pause".into(),
                    OnBattery::Keep => "keep".into(),
                    OnBattery::Below(n) => n.to_string(),
                },
            ),
            ("screen-off", on(self.pause_screen_off)),
            ("rdp", port(self.pause_rdp)),
            ("vnc", port(self.pause_vnc)),
            ("interfaces", self.interfaces.join(",")),
            ("ip-url", self.ip_url.clone().unwrap_or_else(|| "default".into())),
            ("software", on(self.software)),
            ("title", self.title.name()),
            ("widgets", self.widgets.name()),
            ("graph", self.graph.name()),
            ("status", self.status.name()),
            ("hide", self.hidden.join(",")),
            ("ui", maybe(self.ui)),
            ("hand", maybe(self.hand)),
        ]
    }

    /// The options part of the settings, from the wall's options.
    pub fn from_args(args: &[String]) -> Settings {
        let mut s = Settings::default();
        let mut it = args.iter().peekable();
        while let Some(a) = it.next() {
            let mut take = || it.next().cloned();
            match a.as_str() {
                "--theme" => s.theme = take(),
                "--all-monitors" => s.screens = Screens::All,
                "--host" => s.host = take(),
                "--battery" => s.battery = true,
                "--pause-screen-off" => s.pause_screen_off = true,
                "--pause-below" => {
                    s.on_battery = match take().and_then(|v| v.parse::<u8>().ok()) {
                        Some(0) => OnBattery::Keep,
                        Some(n) => OnBattery::Below(n.min(100)),
                        None => OnBattery::Pause,
                    }
                }
                "--pause-rdp" => s.pause_rdp = take().and_then(|v| v.parse().ok()),
                "--pause-vnc" => s.pause_vnc = take().and_then(|v| v.parse().ok()),
                "--interface" => s.interfaces.extend(take()),
                "--ip-url" => s.ip_url = take(),
                "--software" => s.software = true,
                "--demo" => {
                    s.extra.push(a.clone());
                    if it.peek().is_some_and(|v| time_shaped(v)) {
                        s.extra.extend(it.next().cloned());
                    }
                }
                o if takes_value(o) => {
                    s.extra.push(a.clone());
                    s.extra.extend(take());
                }
                o if !o.starts_with('-') => s.screens = Screens::One(o.to_string()),
                _ => s.extra.push(a.clone()),
            }
        }
        s
    }

    /// The wall's options for these settings: what `from_args` reads back.
    pub fn args(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let mut push = |o: &str, v: Option<String>| {
            out.push(o.into());
            out.extend(v);
        };
        if let Some(t) = &self.theme {
            push("--theme", Some(t.clone()));
        }
        if let Some(h) = &self.host {
            push("--host", Some(h.clone()));
        }
        if self.battery {
            push("--battery", None);
        }
        match self.on_battery {
            OnBattery::Pause => {}
            OnBattery::Below(n) => push("--pause-below", Some(n.to_string())),
            OnBattery::Keep => push("--pause-below", Some("0".into())),
        }
        if self.pause_screen_off {
            push("--pause-screen-off", None);
        }
        if let Some(p) = self.pause_rdp {
            push("--pause-rdp", Some(p.to_string()));
        }
        if let Some(p) = self.pause_vnc {
            push("--pause-vnc", Some(p.to_string()));
        }
        for i in &self.interfaces {
            push("--interface", Some(i.clone()));
        }
        if let Some(u) = &self.ip_url {
            push("--ip-url", Some(u.clone()));
        }
        if self.software {
            push("--software", None);
        }
        out.extend(self.extra.iter().cloned());
        match &self.screens {
            Screens::Main => {}
            Screens::All => out.push("--all-monitors".into()),
            Screens::One(m) => out.push(m.clone()),
        }
        out
    }

    /// The settings as kept in `dir`: what `write` wrote, and what the user wrote by hand.
    pub fn read(dir: &Path) -> Settings {
        let conf = std::fs::read_to_string(dir.join("retro-wall.conf")).unwrap_or_default();
        let mut s = Settings::from_args(&conf_args(&conf));
        let doc: DocumentMut = std::fs::read_to_string(dir.join("retro.toml")).unwrap_or_default().parse().unwrap_or_default();
        if let Some(m) = doc.get("monitors").and_then(Item::as_table) {
            (s.title, s.widgets, s.graph, s.status) =
                (Place::of(m.get("title")), Place::of(m.get("widgets")), Place::of(m.get("graph")), Place::of(m.get("status")));
        }
        if doc.get("group").and_then(Item::as_array_of_tables).is_some_and(|g| !managed_groups(g)) {
            s.custom_layout = true;
        } else if let Some(groups) = doc.get("group").and_then(Item::as_array_of_tables) {
            let shown: Vec<String> =
                groups.iter().filter_map(|g| g.get("widgets").and_then(Item::as_array)).flatten().filter_map(|v| v.as_str().map(String::from)).collect();
            s.hidden = INSTRUMENTS.iter().filter(|i| !shown.iter().any(|w| w == *i)).map(|i| i.to_string()).collect();
        }
        // a theme of the setup program's own: the theme it extends, and what it changes
        if let Some(own) = s.theme.clone().filter(|t| t.starts_with(OWN_THEME)) {
            let text = std::fs::read_to_string(dir.join("themes").join(&own).join("theme.toml")).unwrap_or_default();
            let t: DocumentMut = text.parse().unwrap_or_default();
            s.theme = t.get("extends").and_then(Item::as_str).map(String::from).or(Some(own.trim_start_matches(OWN_THEME).to_string()));
            let world = t.get("element").and_then(|e| e.get(crate::elements::blocks::KIND));
            s.ui = world.and_then(|w| w.get("ui")).and_then(Item::as_bool);
            s.hand = world.and_then(|w| w.get("hand")).and_then(Item::as_bool);
        }
        s
    }

    /// Keep the settings in `dir`. `layout` is a theme's instruments as it has them, for writing them
    /// with the hidden ones left out.
    pub fn write(&self, dir: &Path, layout: &dyn Fn(&str) -> Vec<Group>) -> Result<(), String> {
        // retro.toml read first: one the user broke changes nothing at all
        let toml_path = dir.join("retro.toml");
        let text = std::fs::read_to_string(&toml_path).unwrap_or_default();
        let mut doc: DocumentMut = text.parse().map_err(|e| format!("{}: {e}: nothing changed; mend it, or move it aside", toml_path.display()))?;
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let theme = self.theme.clone().unwrap_or_else(|| "phosphor".into());
        // a theme's extras: a theme of its own that extends it, only where they differ from its own
        let extras = self.ui.is_some() || self.hand.is_some();
        let own = format!("{OWN_THEME}{theme}");
        let own_dir = dir.join("themes").join(&own);
        let mut opts = self.clone();
        if extras {
            let mut world = Table::new();
            if let Some(v) = self.ui {
                world["ui"] = value(v);
            }
            if let Some(v) = self.hand {
                world["hand"] = value(v);
            }
            let mut t = DocumentMut::new();
            // no name of its own: the theme it extends keeps its name, in the log and the window
            t["extends"] = value(theme.as_str());
            let mut element = Table::new();
            element.set_implicit(true);
            element[crate::elements::blocks::KIND] = Item::Table(world);
            t["element"] = Item::Table(element);
            let head = "# Written by retro-wall's setup program: the theme it extends, with your settings. Change them there.\n";
            std::fs::create_dir_all(&own_dir).map_err(|e| format!("{}: {e}", own_dir.display()))?;
            write(&own_dir.join("theme.toml"), &format!("{head}{t}"))?;
            opts.theme = Some(own.clone());
        }
        // its own themes for other themes, or for this one when it has no extras now: gone
        for e in std::fs::read_dir(dir.join("themes")).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(OWN_THEME) && (name != own || !extras) {
                std::fs::remove_dir_all(e.path()).map_err(|err| format!("{}: {err}", e.path().display()))?;
            }
        }
        // the options, on this desktop's line; every other line as it was
        let path = dir.join("retro-wall.conf");
        let conf = std::fs::read_to_string(&path).unwrap_or_default();
        write(&path, &with_conf_line(&conf, &opts.args()))?;
        // retro.toml: [monitors] and the setup program's [[group]], everything else as it was
        let path = toml_path;
        let places = [("title", &self.title), ("widgets", &self.widgets), ("graph", &self.graph), ("status", &self.status)];
        if places.iter().all(|(_, p)| **p == Place::Theme) {
            doc.remove("monitors");
        } else {
            let m = doc.entry("monitors").or_insert(Item::Table(Table::new()));
            if let Some(m) = m.as_table_mut() {
                for (key, place) in places {
                    match place.item() {
                        Some(item) => m[key] = item,
                        None => {
                            m.remove(key);
                        }
                    }
                }
            }
        }
        let managed = doc.get("group").and_then(Item::as_array_of_tables).is_none_or(managed_groups);
        if !managed || self.custom_layout {
            // the user's own layout: left as it is
        } else if self.hidden.is_empty() {
            doc.remove("group");
        } else {
            let mut groups = ArrayOfTables::new();
            for mut g in layout(&theme) {
                // the battery's meter where the wall puts it, after SWAP, written out: with SWAP hidden
                // the wall would have nowhere to put it, and it would not show
                if self.battery
                    && !g.widgets.iter().any(|w| w == "bat")
                    && let Some(i) = g.widgets.iter().position(|w| w == "swap")
                {
                    g.widgets.insert(i + 1, "bat".into());
                }
                let widgets: Vec<&String> = g.widgets.iter().filter(|w| !self.hidden.contains(w)).collect();
                if widgets.is_empty() {
                    continue;
                }
                let mut t = Table::new();
                t["anchor"] = value(g.anchor.name());
                if g.direction == Direction::Column {
                    t["direction"] = value("column");
                }
                t["widgets"] = value(widgets.into_iter().collect::<Array>());
                if !g.edges {
                    t["edges"] = value(false);
                }
                groups.push(t);
            }
            doc["group"] = Item::ArrayOfTables(groups);
        }
        write(&path, &doc.to_string())
    }
}

/// Take out of `dir` what the setup program wrote: the Windows desktop's line of retro-wall.conf, its
/// own themes, `[monitors]` and its `[[group]]` in retro.toml. The user's own lines, themes, sources
/// and layout stay; a file or directory left with nothing of theirs goes.
pub fn forget(dir: &Path) -> Result<(), String> {
    let conf_path = dir.join("retro-wall.conf");
    if let Ok(conf) = std::fs::read_to_string(&conf_path) {
        let kept: Vec<&str> = conf.lines().filter(|l| words(l).first().map(String::as_str) != Some(CONF_KEY)).collect();
        if kept.iter().all(|l| words(l).is_empty()) {
            std::fs::remove_file(&conf_path).map_err(|e| format!("{}: {e}", conf_path.display()))?;
        } else {
            write(&conf_path, &(kept.join("\n") + "\n"))?;
        }
    }
    let themes = dir.join("themes");
    for e in std::fs::read_dir(&themes).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(OWN_THEME) {
            std::fs::remove_dir_all(e.path()).map_err(|err| format!("{}: {err}", e.path().display()))?;
        }
    }
    let _ = std::fs::remove_dir(&themes);
    let toml_path = dir.join("retro.toml");
    if let Ok(text) = std::fs::read_to_string(&toml_path)
        && let Ok(mut doc) = text.parse::<DocumentMut>()
    {
        doc.remove("monitors");
        if doc.get("group").and_then(Item::as_array_of_tables).is_some_and(managed_groups) {
            doc.remove("group");
        }
        if doc.iter().next().is_none() && doc.to_string().lines().all(|l| l.trim().is_empty() || l.trim_start().starts_with('#')) {
            std::fs::remove_file(&toml_path).map_err(|e| format!("{}: {e}", toml_path.display()))?;
        } else {
            write(&toml_path, &doc.to_string())?;
        }
    }
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

/// A line of retro-wall.conf as words: split at spaces, but a word in double quotes kept whole (a
/// network interface named "Ethernet 2"); a `#` outside quotes starts a comment.
pub fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted, mut any) = (vec![], String::new(), false, false);
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '#' if !quoted => break,
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

/// A word as `words` reads it back: in double quotes if it has a space, a `#` or nothing in it.
fn quoted(word: &str) -> String {
    if word.is_empty() || word.contains(|c: char| c.is_whitespace() || c == '#') { format!("\"{word}\"") } else { word.to_string() }
}

/// The wall's options on the Windows desktop's line of retro-wall.conf.
pub fn conf_args(conf: &str) -> Vec<String> {
    for line in conf.lines() {
        let w = words(line);
        if w.first().map(String::as_str) == Some(CONF_KEY) {
            return w[1..].to_vec();
        }
    }
    vec![]
}

/// retro-wall.conf with the Windows desktop's line set to `args`, every other line as it was.
fn with_conf_line(conf: &str, args: &[String]) -> String {
    let line = format!("{CONF_KEY}  {}", args.iter().map(|a| quoted(a)).collect::<Vec<_>>().join(" ")).trim_end().to_string();
    let mut out = vec![];
    let mut done = false;
    for l in conf.lines() {
        if words(l).first().map(String::as_str) == Some(CONF_KEY) {
            if !done {
                out.push(line.clone());
                done = true;
            }
        } else {
            out.push(l.to_string());
        }
    }
    if !done {
        if out.is_empty() {
            out.push("# retro-wall's options, one line per desktop: the Windows desktop's is \"windows\". The setup program writes it.".into());
        }
        out.push(line);
    }
    out.join("\n") + "\n"
}

/// Write a file whole: to a temporary beside it, on the disk, then renamed over it, so a reader never
/// sees half, and a power cut leaves the old file or the new one (renamed before its data was on the
/// disk, a file can come back as zeros).
fn write(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("tmp-setup");
    std::fs::File::create(&tmp).and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all())).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;

    fn every_setting() -> Settings {
        Settings {
            theme: Some("blocky".into()),
            screens: Screens::One("DISPLAY2".into()),
            host: Some("office pc".into()),
            battery: true,
            on_battery: OnBattery::Below(25),
            pause_screen_off: true,
            pause_rdp: Some(3389),
            pause_vnc: Some(5900),
            interfaces: vec!["Ethernet".into(), "Wi-Fi".into()],
            ip_url: Some("https://example.org/ip".into()),
            software: true,
            extra: vec!["--reserve-bottom".into(), "48".into()],
            title: Place::Named(vec!["DISPLAY1".into()]),
            widgets: Place::Primary,
            graph: Place::None,
            status: Place::Theme,
            hidden: vec!["swap".into(), "deck".into()],
            ui: Some(false),
            hand: Some(true),
            custom_layout: false,
        }
    }

    #[test]
    fn every_option_the_settings_write_is_one_the_wall_takes() {
        let mut s = every_setting();
        for screens in [Screens::Main, Screens::All, Screens::One("DISPLAY2".into())] {
            for on_battery in [OnBattery::Pause, OnBattery::Below(40), OnBattery::Keep] {
                s.screens = screens.clone();
                s.on_battery = on_battery.clone();
                let args = s.args();
                cli::parse(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
                // and they read back as they were, through the file's line
                let line = with_conf_line("", &args);
                assert_eq!(conf_args(&line), args, "{line}");
                assert_eq!(Settings::from_args(&conf_args(&line)).args(), args);
            }
        }
        assert_eq!(Settings::default().args(), Vec::<String>::new(), "the defaults are no options at all");
    }

    #[test]
    fn a_line_splits_at_spaces_but_not_in_quotes() {
        assert_eq!(words(r##"windows  --interface "Ethernet 2" --host "#a b" # a comment"##), ["windows", "--interface", "Ethernet 2", "--host", "#a b"]);
        assert_eq!(words(":0.0 --theme winter"), [":0.0", "--theme", "winter"]);
        assert_eq!(words(r#"x """#), ["x", ""]);
        assert!(words("   # nothing").is_empty());
    }

    #[test]
    fn every_setting_reads_back_through_its_key() {
        let s = every_setting();
        let pairs = s.pairs();
        assert_eq!(pairs.iter().map(|p| p.0).collect::<Vec<_>>(), KEYS, "every key, in order");
        let mut back = Settings { extra: s.extra.clone(), ..Settings::default() };
        for (k, v) in &pairs {
            back.set(k, v).unwrap_or_else(|e| panic!("{k}={v}: {e}"));
        }
        assert_eq!(back, s);
        let mut d = Settings::default();
        for (k, v) in Settings::default().pairs() {
            d.set(k, &v).unwrap();
        }
        assert_eq!(d, Settings::default());
        for (k, v) in [
            ("host", "a\"b"),
            ("battery", "maybe"),
            ("rdp", "x"),
            ("on-battery", "0"),
            ("on-battery", "101"),
            ("hide", "bat"),
            ("ui", "x"),
            ("nope", "1"),
            ("screens", ""),
        ] {
            assert!(Settings::default().set(k, v).is_err(), "{k}={v}");
        }
    }

    #[test]
    fn options_it_does_not_show_are_kept() {
        let args: Vec<String> = ["--theme", "winter", "--reserve-top", "DISPLAY1=40", "--demo", "12:00", "--config", "C:/x.toml", "--at", "120", "--weird"]
            .map(String::from)
            .to_vec();
        let s = Settings::from_args(&args);
        assert_eq!(s.theme.as_deref(), Some("winter"));
        assert_eq!(s.screens, Screens::Main, "--at's value is not a monitor");
        assert_eq!(s.extra, ["--reserve-top", "DISPLAY1=40", "--demo", "12:00", "--config", "C:/x.toml", "--at", "120", "--weird"]);
        // --demo takes a time only: a monitor after it is the monitor
        assert_eq!(Settings::from_args(&["--demo".to_string(), "07:05:09".to_string()]).extra, ["--demo", "07:05:09"]);
        let d = Settings::from_args(&["--demo".to_string(), "DISPLAY2".to_string()]);
        assert_eq!((d.extra, d.screens), (vec!["--demo".to_string()], Screens::One("DISPLAY2".into())));
        assert!(s.args().ends_with(&s.extra));
    }

    fn layout(_: &str) -> Vec<Group> {
        crate::config::default_layout()
    }

    #[test]
    fn the_settings_written_read_back_and_the_users_own_lines_are_kept() {
        let dir = std::env::temp_dir().join(format!("retro-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // what the user had: another desktop's line, and retro.toml with a comment, a source and a widget
        std::fs::write(dir.join("retro-wall.conf"), ":0.0  --theme matrix\nwindows  --theme old\n").unwrap();
        std::fs::write(dir.join("retro.toml"), "# mine\n[source.users]\ncommand = \"who | wc -l\"\n\n[widget.users]\ntype = \"meter\"\nsource = \"users\"\n")
            .unwrap();
        let s = every_setting();
        s.write(&dir, &layout).unwrap();
        let back = Settings::read(&dir);
        assert_eq!(back, s);
        let conf = std::fs::read_to_string(dir.join("retro-wall.conf")).unwrap();
        assert!(conf.starts_with(":0.0  --theme matrix\n"), "{conf}");
        assert_eq!(conf.lines().filter(|l| l.starts_with("windows")).count(), 1);
        assert!(conf.contains("--theme setup-blocky"), "blocky's extras through a theme of its own: {conf}");
        let toml = std::fs::read_to_string(dir.join("retro.toml")).unwrap();
        for kept in ["# mine", "[source.users]", "[widget.users]"] {
            assert!(toml.contains(kept), "{kept} in {toml}");
        }
        // the theme it wrote is a theme the wall loads, extending blocky
        let own = std::fs::read_to_string(dir.join("themes/setup-blocky/theme.toml")).unwrap();
        assert!(own.contains("extends = \"blocky\"") && own.contains("hand = true") && own.contains("ui = false"), "{own}");
        // back to the theme's own: the files lose what the settings wrote, the user's lines stay
        // another theme: blocky's own theme is not left behind
        let other = Settings { theme: Some("winter".into()), hand: Some(true), ..Settings::default() };
        other.write(&dir, &layout).unwrap();
        assert!(!dir.join("themes/setup-blocky").exists() && dir.join("themes/setup-winter").exists());
        let plain = Settings { theme: Some("blocky".into()), ..Settings::default() };
        plain.write(&dir, &layout).unwrap();
        assert_eq!(Settings::read(&dir), plain);
        assert!(!dir.join("themes/setup-blocky").exists() && !dir.join("themes/setup-winter").exists());
        let toml = std::fs::read_to_string(dir.join("retro.toml")).unwrap();
        assert!(!toml.contains("[monitors]") && !toml.contains("[[group]]") && toml.contains("[widget.users]"), "{toml}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_users_own_layout_is_left_as_it_is() {
        let dir = std::env::temp_dir().join(format!("retro-setup-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mine = "[[group]]\nanchor = \"top-left\"\nwidgets = [\"cpu\", \"users\"]\n";
        std::fs::write(dir.join("retro.toml"), mine).unwrap();
        let mut s = Settings::read(&dir);
        assert!(s.custom_layout && s.hidden.is_empty());
        s.hidden = vec!["cpu".into()];
        s.graph = Place::None;
        s.write(&dir, &layout).unwrap();
        let toml = std::fs::read_to_string(dir.join("retro.toml")).unwrap();
        assert!(toml.contains("\"users\"") && toml.contains("\"cpu\"") && toml.contains("graph = \"none\""), "{toml}");
        // forgetting what the setup program wrote keeps it too
        forget(&dir).unwrap();
        let toml = std::fs::read_to_string(dir.join("retro.toml")).unwrap();
        assert!(toml.contains("\"users\"") && !toml.contains("[monitors]"), "{toml}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn forgetting_takes_out_only_what_the_setup_program_wrote() {
        let dir = std::env::temp_dir().join(format!("retro-setup-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes/teal")).unwrap();
        std::fs::write(dir.join("themes/teal/theme.toml"), "extends = \"phosphor\"\n").unwrap();
        std::fs::write(dir.join("retro-wall.conf"), ":0.0  --theme matrix\n").unwrap();
        std::fs::write(dir.join("retro.toml"), "[source.users]\ncommand = \"who\"\n").unwrap();
        every_setting().write(&dir, &layout).unwrap();
        forget(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("retro-wall.conf")).unwrap(), ":0.0  --theme matrix\n");
        let toml = std::fs::read_to_string(dir.join("retro.toml")).unwrap();
        assert!(toml.contains("[source.users]") && !toml.contains("[monitors]") && !toml.contains("[[group]]"), "{toml}");
        assert!(dir.join("themes/teal").is_dir() && !dir.join("themes/setup-blocky").exists());
        // with nothing of the user's, nothing is left
        let bare = std::env::temp_dir().join(format!("retro-setup-bare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bare);
        every_setting().write(&bare, &layout).unwrap();
        forget(&bare).unwrap();
        assert!(!bare.exists(), "{:?}", std::fs::read_dir(&bare).map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_broken_retro_toml_changes_nothing() {
        let dir = std::env::temp_dir().join(format!("retro-setup-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("retro.toml"), "[monitors\n").unwrap();
        assert!(every_setting().write(&dir, &layout).is_err());
        assert!(!dir.join("retro-wall.conf").exists() && !dir.join("themes").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_hidden_instrument_is_left_out_of_the_themes_own_layout() {
        let dir = std::env::temp_dir().join(format!("retro-setup-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = Settings { hidden: vec!["ram".into(), "status".into()], ..Settings::default() };
        s.write(&dir, &layout).unwrap();
        let cfg = crate::config::read_config(Some(dir.join("retro.toml").to_str().unwrap())).unwrap();
        assert!(cfg.problems.is_empty(), "{:?}", cfg.problems);
        let groups = cfg.groups.expect("a layout");
        let widgets: Vec<&str> = groups.iter().flat_map(|g| g.widgets.iter().map(String::as_str)).collect();
        assert_eq!(widgets, ["cpu", "swap", "deck"], "the status line's group is empty and left out");
        assert_eq!(Settings::read(&dir).hidden, ["ram", "status"]);
        // with the battery's meter on and SWAP hidden, BAT is written where SWAP was, so it still shows
        let s = Settings { battery: true, hidden: vec!["swap".into()], ..Settings::default() };
        s.write(&dir, &layout).unwrap();
        let cfg = crate::config::read_config(Some(dir.join("retro.toml").to_str().unwrap())).unwrap();
        let widgets: Vec<String> = cfg.groups.unwrap().iter().flat_map(|g| g.widgets.clone()).collect();
        assert_eq!(widgets, ["cpu", "ram", "bat", "deck", "status"]);
        assert_eq!(Settings::read(&dir), s);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
