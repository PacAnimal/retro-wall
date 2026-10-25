//! Themes and retro.toml. A theme is a directory with theme.toml and its pictures; it may extend another,
//! set the palette's roles (or turn its hue), tweak the core and name its scene's elements, bottom to top.
//! Everything is checked when it loads: a broken theme or config is reported, never a crash.

use crate::tweaks::{Colour, Tweaks};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use toml::{Table, Value};

pub const DEFAULT_THEME: &str = "phosphor";

#[derive(Debug)]
pub struct ThemeError(pub String);

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

type R<T> = Result<T, ThemeError>;

fn err<T>(s: impl Into<String>) -> R<T> {
    Err(ThemeError(s.into()))
}

/// A table deserialised into T, its errors as ThemeErrors at `at`.
pub fn typed<T: serde::de::DeserializeOwned>(t: &Table, at: &str) -> R<T> {
    Value::Table(t.clone()).try_into().map_err(|e: toml::de::Error| ThemeError(format!("{at}: {}", e.message())))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    UnderTitle,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    #[default]
    Row,
    Column,
}

fn yes() -> bool {
    true
}

/// A group of the instruments' layout.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub anchor: Anchor,
    #[serde(default)]
    pub direction: Direction,
    pub widgets: Vec<String>,
    /// Whether the work area's edges (panels) move it.
    #[serde(default = "yes")]
    pub edges: bool,
}

/// Which monitors show a part of the header.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Choice {
    Named(Vec<String>),
    Rule(Rule),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rule {
    Rightmost,
    Leftmost,
    Primary,
    All,
    None,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Monitors {
    #[serde(default = "rightmost")]
    pub title: Choice,
    #[serde(default = "rightmost")]
    pub widgets: Choice,
    #[serde(default = "rightmost")]
    pub graph: Choice,
}

fn rightmost() -> Choice {
    Choice::Rule(Rule::Rightmost)
}

impl Default for Monitors {
    fn default() -> Self {
        Monitors { title: rightmost(), widgets: rightmost(), graph: rightmost() }
    }
}

/// The palette: each role a colour; the ramp six, dark to bright.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub base: Colour,
    pub shadow: Colour,
    pub deep: Colour,
    pub ramp: [Colour; 6],
    pub sun_top: Colour,
    pub sun_bottom: Colour,
    pub title_top: Colour,
    pub title_light: Colour,
    pub title_bottom: Colour,
    pub glow: Colour,
    pub caption: Colour,
    pub bracket: Colour,
    pub ridge: Colour,
}

/// An element of the scene: its name, its kind, its settings (the kind's defaults, the theme's over them).
#[derive(Clone, Debug)]
pub struct Element {
    pub name: String,
    pub kind: String,
    pub settings: Table,
}

/// The lettering's glyphs: each a list of strokes on a 4 x 6 grid.
pub type Strokes = Vec<Vec<(f64, f64)>>;

/// One look of a theme, resolved.
pub struct Theme {
    pub name: String,
    pub tw: Tweaks,
    pub palette: Palette,
    pub glyphs: &'static HashMap<char, Strokes>,
    pub scene: Vec<Element>,
    pub groups: Vec<Group>,
    /// [monitor.NAME] groups from retro.toml: that monitor's own layout.
    pub monitor_groups: BTreeMap<String, Vec<Group>>,
    /// retro.toml's layout or the default one, never the theme's.
    pub layout: Vec<Group>,
    /// Every widget's settings by name, its `type` among them.
    pub widgets: BTreeMap<String, Table>,
    pub monitors: Monitors,
    /// The chain's directories, the theme's own first: where pictures are looked up.
    pub dirs: Vec<PathBuf>,
    /// [source.NAME] tables from retro.toml.
    pub sources: Table,
    /// "day" or "night" for a theme with looks.
    pub phase: Option<String>,
    /// The lettering's measurements, made as they are needed.
    pub measures: crate::title::Measures,
}

/// A theme resolved: its one look, or its day and night looks.
pub struct Loaded {
    pub looks: Option<HashMap<String, Arc<Theme>>>,
    pub plain: Arc<Theme>,
    /// The user's settings left out of it, each with the reason.
    pub problems: Vec<String>,
}

impl Loaded {
    /// A look: "day", "night", or None (or a theme without looks) for the plain one.
    pub fn look(&self, phase: Option<&str>) -> Arc<Theme> {
        match (phase, &self.looks) {
            (Some(p), Some(looks)) => looks.get(p).unwrap_or(&self.plain).clone(),
            _ => self.plain.clone(),
        }
    }
}

pub fn core() -> &'static Table {
    static CORE: OnceLock<Table> = OnceLock::new();
    CORE.get_or_init(|| include_str!("core.toml").parse().unwrap_or_else(|e| unreachable!("core.toml: {e}")))
}

/// The lettering's glyphs, from core.toml.
pub fn glyphs() -> &'static HashMap<char, Strokes> {
    static GLYPHS: OnceLock<HashMap<char, Strokes>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        let n = |v: &Value| v.as_float().or(v.as_integer().map(|i| i as f64)).unwrap_or_else(|| unreachable!("core.toml glyphs: {v}"));
        let list = |v: &Value| v.as_array().cloned().unwrap_or_else(|| unreachable!("core.toml glyphs: {v}"));
        core()["glyphs"]
            .as_table()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| {
                let strokes = list(v)
                    .iter()
                    .map(|s| {
                        list(s)
                            .iter()
                            .map(|p| {
                                let p = list(p);
                                (n(&p[0]), n(&p[1]))
                            })
                            .collect()
                    })
                    .collect();
                Some((k.chars().next()?, strokes))
            })
            .collect()
    })
}

// --- finding and loading ---------------------------------------------------------------------------

/// The user's home directory: $HOME, or on Windows %USERPROFILE%.
pub fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    PathBuf::from(std::env::var_os(var).unwrap_or_default())
}

/// ~/.config/retro ($XDG_CONFIG_HOME/retro); on Windows %APPDATA%\retro.
pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        return PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("retro");
    }
    std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".config")).join("retro")
}

/// A path without Windows' verbatim prefix (\\?\C:\...), which canonicalize gives and people do not
/// write.
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// Theme search directories, the user's first.
pub fn theme_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![config_dir().join("themes")];
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).and_then(|e| e.parent().map(|p| plain(p).join("themes"))) {
        dirs.push(d);
    }
    if let Ok(extra) = std::env::var("RETRO_THEMES") {
        dirs.insert(0, PathBuf::from(extra));
    }
    dirs
}

pub fn find_theme(spec: &str) -> R<PathBuf> {
    let candidates: Vec<PathBuf> = if spec.contains(std::path::is_separator) || spec.ends_with(".toml") {
        vec![PathBuf::from(spec), Path::new(spec).join("theme.toml")]
    } else {
        theme_dirs().iter().flat_map(|d| [d.join(format!("{spec}.toml")), d.join(spec).join("theme.toml")]).collect()
    };
    for p in candidates {
        if p.is_file() {
            return Ok(p.canonicalize().map_or(p, |c| plain(&c)));
        }
    }
    err(format!("no theme {spec:?} (looked in {})", theme_dirs().iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(" or ")))
}

/// A theme's name, from its file: the directory of a theme.toml, else the file's stem.
fn theme_name(path: &Path) -> String {
    let named = if path.file_name().is_some_and(|f| f == "theme.toml") { path.parent() } else { Some(path) };
    named.and_then(Path::file_stem).map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn read_toml(path: &Path) -> R<Table> {
    let text = crate::files::read_text(path, crate::files::Kind::Settings).map_err(|e| ThemeError(format!("{}: {e}", path.display())))?;
    let mut t: Table = text.parse().map_err(|e: toml::de::Error| ThemeError(format!("{}: {}", path.display(), e.message())))?;
    clean_table(&mut t);
    Ok(t)
}

/// A table's keys and strings without NULs, which no setting has a use for (and cairo cannot draw).
fn clean_table(t: &mut Table) {
    if t.keys().any(|k| k.contains('\0')) {
        *t = std::mem::take(t).into_iter().map(|(k, v)| (k.replace('\0', ""), v)).collect();
    }
    t.iter_mut().for_each(|(_, v)| clean_value(v));
}

fn clean_value(v: &mut Value) {
    match v {
        Value::String(s) if s.contains('\0') => s.retain(|c| c != '\0'),
        Value::Array(a) => a.iter_mut().for_each(clean_value),
        Value::Table(t) => clean_table(t),
        _ => {}
    }
}

/// A theme and every theme it extends, the furthest first. A theme it extends is looked for beside it
/// first.
fn load_chain(spec: &str, beside: Option<&Path>, seen: &mut Vec<PathBuf>) -> R<Vec<(PathBuf, Table)>> {
    let sibling = beside.filter(|_| !spec.contains('/')).map(|d| d.join(spec).join("theme.toml")).filter(|p| p.is_file());
    let path = match sibling {
        Some(p) => p.canonicalize().map_or(p, |c| plain(&c)),
        None => find_theme(spec)?,
    };
    if let Some(i) = seen.iter().position(|p| *p == path) {
        return err(match &seen[i..] {
            [_] => format!("{}: extends itself", path.display()),
            circle => {
                let names: Vec<String> = circle.iter().chain([&path]).map(|p| theme_name(p)).collect();
                format!("{}: extends in a circle: {}", path.display(), names.join(" -> "))
            }
        });
    }
    let data = read_toml(&path)?;
    let known = ["name", "extends", "palette", "tweaks", "scene", "element", "day", "night", "group", "widget", "monitors"];
    if let Some(k) = data.keys().find(|k| !known.contains(&k.as_str())) {
        return err(format!("{}: unknown {k} (a theme has {})", path.display(), known.join(", ")));
    }
    seen.push(path.clone());
    let mut chain = match data.get("extends") {
        Some(Value::String(parent)) => load_chain(parent, path.parent().and_then(Path::parent), seen)?,
        Some(_) => return err(format!("{}: extends is a theme's name", path.display())),
        None => vec![],
    };
    chain.push((path, data));
    Ok(chain)
}

fn table<'a>(data: &'a Table, key: &str, at: &str) -> R<Option<&'a Table>> {
    match data.get(key) {
        None => Ok(None),
        Some(Value::Table(t)) => Ok(Some(t)),
        Some(_) => err(format!("{at}: {key} is a table: [{key}]")),
    }
}

fn groups_of(v: &Value, at: &str) -> R<Vec<Group>> {
    let Value::Array(a) = v else { return err(format!("{at}: group is a list of tables: [[group]]")) };
    a.iter()
        .enumerate()
        .map(|(i, g)| match g {
            Value::Table(t) => typed::<Group>(t, &format!("{at}: group {}", i + 1)),
            _ => err(format!("{at}: group {} is a table", i + 1)),
        })
        .collect()
}

// --- checking ----------------------------------------------------------------------------------------

/// `value` checked against the default it replaces: the same kind, a list of the same length (any
/// non-empty length with `vary`), a colour where the default is one.
pub fn fits(value: &Value, default: &Value, at: &str, vary: bool) -> R<Value> {
    match default {
        Value::String(d) if d.starts_with('#') => match value.as_str() {
            Some(s) if Colour::parse(s).is_some() => Ok(Value::String(s.to_lowercase())),
            _ => err(format!("{at} is {value}: it takes a colour, #rrggbb")),
        },
        Value::Boolean(_) => value.as_bool().map(Value::Boolean).ok_or_else(|| ThemeError(format!("{at} is {value}: it takes true or false"))),
        Value::Integer(_) => match value {
            Value::Integer(_) => Ok(value.clone()),
            _ => err(format!("{at} is {value}: it takes a whole number")),
        },
        Value::Float(_) => match value {
            Value::Integer(i) => Ok(Value::Float(*i as f64)),
            Value::Float(f) if f.is_finite() => Ok(value.clone()),
            _ => err(format!("{at} is {value}: it takes a number")),
        },
        Value::Array(d) => {
            let Some(v) = value.as_array() else { return err(format!("{at} is {value}: it takes a list like {default}")) };
            if d.is_empty() {
                return Ok(value.clone()); // an empty default takes any list
            }
            if v.is_empty() || (v.len() != d.len() && !vary) {
                return err(format!("{at} is {value}: it takes a list like {default}{}", if vary { ", of any length" } else { "" }));
            }
            v.iter()
                .enumerate()
                .map(|(i, item)| fits(item, if v.len() == d.len() { &d[i] } else { &d[0] }, &format!("{at}[{i}]"), false))
                .collect::<R<Vec<_>>>()
                .map(Value::Array)
        }
        Value::String(_) => value.as_str().map(|s| Value::String(s.into())).ok_or_else(|| ThemeError(format!("{at} is {value}: it takes a string"))),
        _ => Ok(value.clone()),
    }
}

// --- the palette -------------------------------------------------------------------------------------

fn oklab(c: Colour) -> (f64, f64, f64) {
    let lin = [c.0, c.1, c.2].map(|c| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) });
    let rows = [[0.4122214708, 0.5363325363, 0.0514459929], [0.2119034982, 0.6806995451, 0.1073969566], [0.0883024619, 0.2817188376, 0.6299787005]];
    let [l, m, s] = rows.map(|row| (row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]).cbrt());
    (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )
}

/// A colour from OKLab, its chroma brought in until the screen can show it.
fn from_oklab(ll: f64, mut a: f64, mut b: f64) -> Colour {
    let mut lin = [0.0; 3];
    for _ in 0..60 {
        let c = |v: f64| v.powi(3);
        let (l, m, s) = (c(ll + 0.3963377774 * a + 0.2158037573 * b), c(ll - 0.1055613458 * a - 0.0638541728 * b), c(ll - 0.0894841775 * a - 1.2914855480 * b));
        lin = [
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        ];
        if lin.iter().all(|&c| (-0.0005..=1.0005).contains(&c)) {
            break;
        }
        a *= 0.95;
        b *= 0.95;
    }
    let [r, g, b] = lin.map(|c| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s.clamp(0.0, 1.0) * 255.0).round() / 255.0
    });
    Colour(r, g, b)
}

/// Every colour turned about the hue wheel so that the ramp's fifth lands on `hue` degrees, its chroma
/// times `chroma`; lightness kept.
fn turned(palette: &Table, hue: Option<f64>, chroma: f64) -> R<Table> {
    let c = |v: &Value| v.as_str().and_then(Colour::parse).ok_or_else(|| ThemeError(format!("palette: {v} is not a colour")));
    let ramp4 = palette["ramp"].as_array().map(|a| c(&a[4])).ok_or_else(|| ThemeError("palette: no ramp".into()))??;
    let (_, a, b) = oklab(ramp4);
    let turn = hue.map_or(0.0, |h| h.to_radians() - b.atan2(a));
    let one = |v: &Value| -> R<Value> {
        let (l, a, b) = oklab(c(v)?);
        let (ch, h) = (a.hypot(b) * chroma, b.atan2(a) + turn);
        Ok(Value::String(from_oklab(l, ch * h.cos(), ch * h.sin()).hex()))
    };
    palette
        .iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                match v {
                    Value::Array(a) => Value::Array(a.iter().map(one).collect::<R<_>>()?),
                    _ => one(v)?,
                },
            ))
        })
        .collect()
}

/// Set a value at a path in the tweaks: ["name", i, j].
fn put(tweaks: &mut Table, path: &[Value], value: Value) {
    let Some(mut v) = path[0].as_str().and_then(|n| tweaks.get_mut(n)) else { return };
    for i in &path[1..] {
        match (v, i.as_integer()) {
            (Value::Array(a), Some(i)) if (i as usize) < a.len() => v = &mut a[i as usize],
            _ => return,
        }
    }
    *v = value;
}

fn role(palette: &Table, role: &Value) -> Value {
    match role {
        Value::String(s) => palette[s.as_str()].clone(),
        Value::Array(a) => palette[a[0].as_str().unwrap()].as_array().unwrap()[a[1].as_integer().unwrap() as usize].clone(),
        _ => unreachable!("core.toml roles"),
    }
}

// --- the config ---------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Config {
    pub theme: Option<String>,
    pub host: Option<String>,
    pub scanlines: Option<bool>,
    pub groups: Option<Vec<Group>>,
    pub widgets: BTreeMap<String, Table>,
    pub monitors: Option<Table>,
    pub sources: Table,
    pub monitor_groups: BTreeMap<String, Vec<Group>>,
    /// What was wrong in it, each item left out.
    pub problems: Vec<String>,
    pub path: Option<PathBuf>,
}

impl Config {
    /// Where it was read from, for messages.
    pub fn at(&self) -> String {
        self.path.as_ref().map_or_else(|| "retro.toml".into(), |p| p.display().to_string())
    }
}

pub fn default_layout() -> Vec<Group> {
    let g = |anchor, w: &[&str]| Group { anchor, direction: Direction::Row, widgets: w.iter().map(|s| s.to_string()).collect(), edges: true };
    vec![g(Anchor::UnderTitle, &["cpu", "ram", "swap", "deck"]), g(Anchor::Status, &["status"])]
}

fn builtin_widgets() -> BTreeMap<String, Table> {
    let mut out = BTreeMap::new();
    for (name, label) in [("cpu", "CPU"), ("ram", "RAM"), ("swap", "SWAP"), ("bat", "BAT")] {
        let mut w = Table::new();
        w.insert("type".into(), "meter".into());
        w.insert("source".into(), name.into());
        w.insert("label".into(), label.into());
        out.insert(name.to_string(), w);
    }
    for name in ["deck", "status"] {
        let mut w = Table::new();
        w.insert("type".into(), name.into());
        out.insert(name.to_string(), w);
    }
    out
}

fn widgets_of(t: Option<&Table>, at: &str, into: &mut BTreeMap<String, Table>) -> R<()> {
    for (name, w) in t.into_iter().flatten() {
        let Value::Table(w) = w else { return err(format!("{at}: widget.{name} is a table")) };
        into.entry(name.clone()).or_default().extend(w.clone());
    }
    Ok(())
}

/// A user's file: each item checked on its own, a bad one reported and left out.
struct Lenient<'a> {
    at: &'a str,
    problems: Vec<String>,
}

impl Lenient<'_> {
    fn note(&mut self, e: impl std::fmt::Display) {
        self.problems.push(format!("{e}: left out"));
    }
    fn table<'t>(&mut self, data: &'t Table, key: &str) -> Option<&'t Table> {
        table(data, key, self.at).unwrap_or_else(|e| {
            self.note(e);
            None
        })
    }
    /// The groups that check, each bad one reported.
    fn groups(&mut self, v: &Value, at: &str) -> Option<Vec<Group>> {
        let Value::Array(a) = v else {
            self.note(format!("{at}: group is a list of tables: [[group]]"));
            return None;
        };
        let mut out = vec![];
        for (i, g) in a.iter().enumerate() {
            let at = format!("{at}: group {}", i + 1);
            match g {
                Value::Table(t) => match typed::<Group>(t, &at) {
                    Ok(g) => out.push(g),
                    Err(e) => self.note(e),
                },
                _ => self.note(format!("{at} is a table")),
            }
        }
        Some(out)
    }
}

/// Read retro.toml (or `path`): theme, host, layout, widgets, sources. Only a file that cannot be read
/// as TOML is an error; anything wrong inside is in `problems`, and left out.
pub fn read_config(path: Option<&str>) -> R<Config> {
    let path = path.map(PathBuf::from).unwrap_or_else(|| config_dir().join("retro.toml"));
    if !path.is_file() {
        return Ok(Config::default());
    }
    let d = read_toml(&path)?;
    let at = path.display().to_string();
    let mut l = Lenient { at: &at, problems: vec![] };
    let known = ["theme", "host", "scanlines", "group", "widget", "monitors", "monitor", "source"];
    for k in d.keys().filter(|k| !known.contains(&k.as_str())) {
        l.note(format!("{at}: unknown {k} (it has {})", known.join(", ")));
    }
    let mut string = |k: &str| match d.get(k) {
        None => None,
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(v) => {
            l.note(format!("{at}: {k} is {v}: it takes a name"));
            None
        }
    };
    let (theme, host) = (string("theme"), string("host"));
    let scanlines = match d.get("scanlines") {
        None => None,
        Some(Value::Boolean(b)) => Some(*b),
        Some(v) => {
            l.note(format!("{at}: scanlines is {v}: true or false"));
            None
        }
    };
    let mut widgets = BTreeMap::new();
    for (name, w) in l.table(&d, "widget").into_iter().flatten() {
        match w {
            Value::Table(w) => {
                widgets.insert(name.clone(), w.clone());
            }
            _ => l.note(format!("{at}: widget.{name} is a table")),
        }
    }
    let mut monitor_groups = BTreeMap::new();
    for (name, t) in l.table(&d, "monitor").into_iter().flatten() {
        let at = format!("{at}: monitor.{name}");
        let Value::Table(t) = t else {
            l.note(format!("{at} is a table: [[monitor.{name}.group]]"));
            continue;
        };
        for k in t.keys().filter(|k| *k != "group") {
            l.note(format!("{at}: unknown {k} (it has group)"));
        }
        if let Some(groups) = t.get("group").and_then(|g| l.groups(g, &at)) {
            monitor_groups.insert(name.clone(), groups);
        }
    }
    let mut sources = Table::new();
    for (name, s) in l.table(&d, "source").into_iter().flatten() {
        match crate::plugins::check_source(name, s) {
            Ok(()) => {
                sources.insert(name.clone(), s.clone());
            }
            Err(e) => l.note(format!("{at}: source.{name}: {e}")),
        }
    }
    let mut monitors = Table::new();
    for (k, v) in l.table(&d, "monitors").into_iter().flatten() {
        let one = Table::from_iter([(k.clone(), v.clone())]);
        match typed::<Monitors>(&one, &format!("{at}: monitors")) {
            Ok(_) => {
                monitors.insert(k.clone(), v.clone());
            }
            Err(_) if ["title", "widgets", "graph"].contains(&k.as_str()) => {
                l.note(format!("{at}: monitors.{k} is {v}: it takes rightmost, leftmost, primary, all, none, or a list of monitors' names"))
            }
            Err(e) => l.note(e),
        }
    }
    let groups = d.get("group").and_then(|g| l.groups(g, &at));
    Ok(Config {
        theme,
        host,
        scanlines,
        groups,
        widgets,
        monitors: (!monitors.is_empty()).then_some(monitors),
        sources,
        monitor_groups,
        problems: l.problems,
        path: Some(path.clone()),
    })
}

// --- resolving -----------------------------------------------------------------------------------------

/// What the theme loader asks of the element library: a kind's defaults, its list settings of any
/// length, and a check of its resolved settings.
pub struct KindInfo {
    pub defaults: &'static Table,
    pub varies: &'static [&'static str],
    pub check: fn(&Table) -> Result<(), String>,
}

pub type Kinds = fn(&str) -> Option<KindInfo>;

/// Load a theme and everything it extends, check it all, and resolve its looks.
pub fn resolve(spec: &str, cfg: &Config, kinds: Kinds, battery: bool) -> R<Loaded> {
    let chain = load_chain(spec, None, &mut vec![])?;
    let core = core();
    let mut palette = core["palette"].as_table().unwrap().clone();
    let mut tweaks = core["tweaks"].as_table().unwrap().clone();
    let meta = core["meta"].as_table().unwrap();
    let strings = |k: &str| meta[k].as_array().unwrap().iter().filter_map(|n| n.as_str().map(String::from)).collect::<Vec<String>>();
    let vary = strings("vary");
    let slots = core["roles"]["slots"].as_array().unwrap().clone();
    let mut looks: BTreeMap<&str, (Table, Table)> = BTreeMap::from([("day", Default::default()), ("night", Default::default())]);
    let (mut scene, mut settings): (Option<Vec<String>>, BTreeMap<String, Table>) = (None, BTreeMap::new());
    let (mut layout, mut widgets, mut monitors) = (None, builtin_widgets(), Table::new());
    let mut name = spec.to_string();

    // a tweak's value checked against the one it replaces
    let tweak = |tweaks: &Table, k: &str, v: &Value, at: &str| -> R<Value> {
        let Some(d) = tweaks.get(k) else {
            let mut all: Vec<&String> = tweaks.keys().collect();
            all.sort();
            return err(format!("{at}: tweaks.{k}: not a tweak (they are {})", all.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        };
        let f = fits(v, d, &format!("{at}: tweaks.{k}"), vary.iter().any(|n| n == k))?;
        crate::tweaks::limit(k, &f).map_err(|e| ThemeError(format!("{at}: tweaks.{k}{e}")))?;
        Ok(f)
    };
    let colour_of = |palette: &Table, k: &str, v: &Value, at: &str| -> R<Value> {
        let Some(d) = palette.get(k) else {
            return err(format!("{at}: palette.{k}: no such role (they are {}, and hue and chroma)", palette.keys().cloned().collect::<Vec<_>>().join(", ")));
        };
        fits(v, d, &format!("{at}: palette.{k}"), false)
    };

    for (path, data) in &chain {
        let at = path.display().to_string();
        for phase in ["day", "night"] {
            let Some(look) = table(data, phase, &at)? else { continue };
            if let Some(k) = look.keys().find(|k| !["palette", "tweaks"].contains(&k.as_str())) {
                return err(format!("{at}: {phase}.{k}: a look has [{phase}.palette] and [{phase}.tweaks]"));
            }
            let entry = looks.get_mut(phase).unwrap();
            entry.0.extend(table(look, "palette", &at)?.cloned().unwrap_or_default());
            entry.1.extend(table(look, "tweaks", &at)?.cloned().unwrap_or_default());
        }
        let pal = table(data, "palette", &at)?.cloned().unwrap_or_default();
        let number = |k: &str| -> R<Option<f64>> {
            match pal.get(k) {
                None => Ok(None),
                Some(Value::Float(f)) => Ok(Some(*f)),
                Some(Value::Integer(i)) => Ok(Some(*i as f64)),
                Some(v) => err(format!("{at}: palette.{k} is {v}: a number")),
            }
        };
        if pal.contains_key("hue") || pal.contains_key("chroma") {
            palette = turned(&palette, number("hue")?, number("chroma")?.unwrap_or(1.0))?;
        }
        for (k, v) in pal.iter().filter(|(k, _)| !["hue", "chroma"].contains(&k.as_str())) {
            let f = colour_of(&palette, k, v, &at)?;
            palette.insert(k.clone(), f);
        }
        for (k, v) in table(data, "tweaks", &at)?.into_iter().flatten() {
            let f = tweak(&tweaks, k, v, &at)?;
            tweaks.insert(k.clone(), f);
        }
        widgets_of(table(data, "widget", &at)?, &at, &mut widgets)?;
        monitors.extend(table(data, "monitors", &at)?.cloned().unwrap_or_default());
        if let Some(g) = data.get("group") {
            layout = Some(groups_of(g, &at)?);
        }
        match data.get("scene") {
            None => {}
            Some(Value::Array(a)) if !a.is_empty() && a.iter().all(|n| n.is_str()) => scene = Some(a.iter().map(|n| n.as_str().unwrap().to_string()).collect()),
            Some(_) => return err(format!("{at}: scene is a list of the elements' names, bottom to top")),
        }
        for (n, t) in table(data, "element", &at)?.into_iter().flatten() {
            let Value::Table(t) = t else { return err(format!("{at}: element.{n} is a table of its settings")) };
            settings.entry(n.clone()).or_default().extend(t.clone());
        }
        match data.get("name") {
            Some(Value::String(n)) => name = n.clone(),
            Some(v) => return err(format!("{at}: name is {v}: a string")),
            None => {}
        }
    }
    if let Some(s) = cfg.scanlines {
        tweaks.insert("scanlines".into(), Value::Boolean(s));
    }

    // the scene: each element against its kind's settings
    let scene = scene.unwrap_or_else(|| ["sky", "mountains", "title", "instruments", "finish", "ruler"].iter().map(|s| s.to_string()).collect());
    let mut made = vec![];
    for n in &scene {
        let mut t = settings.get(n).cloned().unwrap_or_default();
        let kind = match t.remove("type") {
            None => n.clone(),
            Some(Value::String(k)) => k,
            Some(v) => return err(format!("{spec}: element.{n}.type is {v}: a kind's name")),
        };
        let Some(info) = kinds(&kind) else {
            return err(format!("{spec}: the scene's {n:?} is a {kind:?}, and there is no such element (retro-wall --list)"));
        };
        let mut merged = info.defaults.clone();
        for (k, v) in &t {
            let Some(d) = info.defaults.get(k) else {
                return err(format!("{spec}: element.{n}: no setting {k:?} (it has {})", info.defaults.keys().cloned().collect::<Vec<_>>().join(", ")));
            };
            merged.insert(k.clone(), fits(v, d, &format!("{spec}: element.{n}.{k}"), info.varies.contains(&k.as_str()))?);
        }
        (info.check)(&merged).map_err(|e| ThemeError(format!("{spec}: element.{n}: {e}")))?;
        made.push(Element { name: n.clone(), kind, settings: merged });
    }
    for (n, t) in &settings {
        if !scene.contains(n) {
            let kind = t.get("type").and_then(|t| t.as_str()).unwrap_or(n);
            if kinds(kind).is_none() {
                return err(format!("{spec}: element.{n}: the scene has no {n:?}, and there is no such element"));
            }
        }
    }

    // the layout: retro.toml's, else the theme's, else the default; --battery's BAT meter after SWAP
    let theme_widgets = widgets.clone();
    for (k, v) in &cfg.widgets {
        widgets.entry(k.clone()).or_default().extend(v.clone());
    }
    let source = |widgets: &BTreeMap<String, Table>, w: &str| widgets.get(w).and_then(|t| t.get("source")).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let kind_of = |widgets: &BTreeMap<String, Table>, w: &str| widgets.get(w).and_then(|t| t.get("type")).and_then(|s| s.as_str()).unwrap_or(w).to_string();
    let battery_widget = |widgets: &BTreeMap<String, Table>, w: &str| source(widgets, w) == "bat" || kind_of(widgets, w) == "battery";
    let user_layout = cfg.groups.is_some();
    let mut groups = match (&cfg.groups, layout) {
        (Some(g), _) => g.clone(),
        (None, Some(mut theme_groups)) => {
            for g in theme_groups.iter_mut() {
                g.widgets.retain(|w| battery || !battery_widget(&widgets, w));
            }
            theme_groups
        }
        (None, None) => default_layout(),
    };
    if battery && !groups.iter().any(|g| g.widgets.iter().any(|w| battery_widget(&widgets, w))) {
        for g in groups.iter_mut() {
            if let Some(i) = g.widgets.iter().position(|w| w == "swap") {
                g.widgets.insert(i + 1, "bat".into());
            }
        }
    }
    let mut monitor_groups = cfg.monitor_groups.clone();
    let mut user_layout_groups = cfg.groups.clone().unwrap_or_else(default_layout);

    // every widget shown, checked: the user's settings that break one are left out (the theme's kept),
    // a widget only the user names that does not check is left out; the theme's own is an error
    let faces = crate::plugins::face_plugins();
    let check = |widgets: &BTreeMap<String, Table>, w: &str| -> Result<(), String> {
        let kind = kind_of(widgets, w);
        if faces.contains_key(&kind) {
            return Ok(());
        }
        crate::elements::instruments::check_widget(&kind, &widgets.get(w).cloned().unwrap_or_default())
    };
    let named_by_user = |w: &str| {
        (user_layout && groups.iter().any(|g| g.widgets.iter().any(|n| n == w))) || monitor_groups.values().flatten().any(|g| g.widgets.iter().any(|n| n == w))
    };
    let mut used: Vec<String> = groups.iter().chain(monitor_groups.values().flatten()).flat_map(|g| g.widgets.iter().cloned()).collect();
    used.sort();
    used.dedup();
    let mut problems = vec![];
    let mut dropped = vec![];
    for w in &used {
        let Err(e) = check(&widgets, w) else { continue };
        if let (true, Some(t)) = (cfg.widgets.contains_key(w), theme_widgets.get(w)) {
            let mut theirs = widgets.clone();
            theirs.insert(w.clone(), t.clone());
            if check(&theirs, w).is_ok() {
                problems.push(format!("widget {w}: {e}: your settings for it left out"));
                widgets = theirs;
                continue;
            }
        }
        if cfg.widgets.contains_key(w) || named_by_user(w) {
            problems.push(format!("widget {w}: {e}: left out"));
            dropped.push(w.clone());
        } else {
            return err(format!("widget {w}: {e}"));
        }
    }
    // the status anchor holds the status line alone; a widget is in one place (on each monitor)
    let place = |gs: &mut Vec<Group>, user: bool, problems: &mut Vec<String>| -> R<()> {
        let mut seen: Vec<String> = vec![];
        for g in gs.iter_mut() {
            let mut kept = vec![];
            for w in std::mem::take(&mut g.widgets) {
                let why = if dropped.contains(&w) {
                    continue;
                } else if g.anchor == Anchor::Status && kind_of(&widgets, &w) != "status" {
                    Some(format!("widget {w}: the status anchor is for the status line alone"))
                } else if seen.contains(&w) {
                    Some(format!("widget {w} is in two places"))
                } else {
                    None
                };
                match why {
                    None => {
                        seen.push(w.clone());
                        kept.push(w);
                    }
                    Some(why) if user || cfg.widgets.contains_key(&w) => problems.push(format!("{why}: left out")),
                    Some(why) => return err(why),
                }
            }
            g.widgets = kept;
        }
        Ok(())
    };
    place(&mut groups, user_layout, &mut problems)?;
    place(&mut user_layout_groups, user_layout, &mut problems)?;
    for gs in monitor_groups.values_mut() {
        place(gs, true, &mut problems)?;
    }
    let mut said = vec![];
    for p in problems {
        let p = format!("{}: {p}", cfg.at());
        if !said.contains(&p) {
            said.push(p);
        }
    }
    let problems = said;
    if let Some(m) = &cfg.monitors {
        monitors.extend(m.clone());
    }
    let monitors: Monitors = typed(&monitors, "monitors")?;
    let dirs: Vec<PathBuf> = chain.iter().rev().map(|(p, _)| p.parent().unwrap_or(Path::new(".")).to_path_buf()).collect();
    let build = |tw: &Table, pal: &Table, phase: Option<&str>| -> R<Arc<Theme>> {
        let mut tw = tw.clone();
        for s in &slots {
            let s = s.as_array().unwrap();
            put(&mut tw, s[0].as_array().unwrap(), role(pal, &s[1]));
        }
        let at = format!("{spec}{}", phase.map(|p| format!(" ({p})")).unwrap_or_default());
        let tw: Tweaks = typed(&tw, &format!("{at}: tweaks"))?;
        tw.check().map_err(|e| ThemeError(format!("{at}: tweaks: {e}")))?;
        Ok(Arc::new(Theme {
            name: name.clone(),
            tw,
            palette: typed(pal, &format!("{at}: palette"))?,
            glyphs: glyphs(),
            scene: made.clone(),
            groups: groups.clone(),
            monitor_groups: monitor_groups.clone(),
            layout: user_layout_groups.clone(),
            widgets: widgets.clone(),
            monitors: monitors.clone(),
            dirs: dirs.clone(),
            sources: cfg.sources.clone(),
            phase: phase.map(String::from),
            measures: Default::default(),
        }))
    };
    let plain = build(&tweaks, &palette, None)?;
    let looks = if looks.values().any(|(p, t)| !p.is_empty() || !t.is_empty()) {
        let mut out = HashMap::new();
        for (phase, (lp, lt)) in &looks {
            let (mut p, mut t) = (palette.clone(), tweaks.clone());
            for (k, v) in lp {
                p.insert(k.clone(), colour_of(&palette, k, v, &format!("{spec}: {phase}"))?);
            }
            for (k, v) in lt {
                if k == "day" {
                    return err(format!("{spec}: {phase}.tweaks.day: the day is the same by day and night"));
                }
                t.insert(k.clone(), tweak(&tweaks, k, v, &format!("{spec}: {phase}"))?);
            }
            out.insert(phase.to_string(), build(&t, &p, Some(phase))?);
        }
        Some(out)
    } else {
        None
    };
    Ok(Loaded { looks, plain, problems })
}

/// The default theme as it was installed, passing over a user's theme of the same name.
pub fn bundled_default() -> Option<PathBuf> {
    let user = config_dir().join("themes");
    theme_dirs()
        .into_iter()
        .filter(|d| *d != user)
        .flat_map(|d| [d.join(format!("{DEFAULT_THEME}.toml")), d.join(DEFAULT_THEME).join("theme.toml")])
        .find(|p| p.is_file())
}

/// A theme resolved and tried by building its scene once: a theme that cannot draw fails here, not
/// when it draws.
pub fn load(spec: &str, cfg: &Config, kinds: Kinds, battery: bool) -> R<Loaded> {
    let l = resolve(spec, cfg, kinds, battery)?;
    for th in l.looks.iter().flat_map(|looks| looks.values()).chain([&l.plain]) {
        crate::elements::trial(th).map_err(|e| ThemeError(format!("{spec}: {e}")))?;
    }
    Ok(l)
}

/// The theme `spec`, or the default if none is named or it fails (with the reason), and the user's
/// settings it leaves out said.
pub fn apply(spec: Option<&str>, cfg: &Config, kinds: Kinds, battery: bool) -> Arc<Loaded> {
    let first = spec.or(cfg.theme.as_deref()).unwrap_or(DEFAULT_THEME).to_string();
    let mut tried = vec![first.clone()];
    let fallback = bundled_default().map_or_else(|| DEFAULT_THEME.to_string(), |p| p.display().to_string());
    if fallback != first && find_theme(&first).ok().map(|p| p.display().to_string()) != Some(fallback.clone()) {
        tried.push(fallback);
    }
    for (i, s) in tried.iter().enumerate() {
        match load(s, cfg, kinds, battery) {
            Ok(l) => {
                for p in &l.problems {
                    crate::errln!("config: {p}");
                }
                return Arc::new(l);
            }
            Err(e) if i + 1 < tried.len() => crate::errln!("theme: {e}: the default theme, {DEFAULT_THEME}, instead"),
            Err(e) => crate::errln!("theme: {e}"),
        }
    }
    crate::errln!("retro-wall: no theme could be loaded");
    std::process::exit(1);
}

impl Theme {
    pub fn glyph(&self, ch: char) -> Option<&Strokes> {
        self.glyphs.get(&ch)
    }
    /// A picture of the theme's, from its own directory or one it extends.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.dirs.iter().map(|d| d.join(rel)).find(|p| p.exists()).unwrap_or_else(|| self.dirs[0].join(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nuls_are_cleaned_from_keys_and_strings() {
        let mut t: Table = "label = \"C\\u0000PU\"\n[\"w\\u0000x\"]\nlist = [\"a\\u0000\", 1, { s = \"\\u0000b\" }]\n".parse().unwrap();
        clean_table(&mut t);
        assert_eq!(t["label"].as_str(), Some("CPU"));
        let list = t["wx"]["list"].as_array().unwrap();
        assert_eq!((list[0].as_str(), list[1].as_integer(), list[2]["s"].as_str()), (Some("a"), Some(1), Some("b")));
    }

    #[test]
    fn a_theme_is_named_by_its_directory_or_file() {
        assert_eq!(theme_name(Path::new("/x/themes/aurora/theme.toml")), "aurora");
        assert_eq!(theme_name(Path::new("/x/themes/mine.toml")), "mine");
    }
}
