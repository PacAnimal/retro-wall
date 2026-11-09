//! The element kinds a scene is made of. A kind declares its settings (TOML defaults), checks them into
//! a typed struct, and builds its parts for a monitor. A theme names it: `[element.NAME] type = "KIND"`.

pub mod arctic;
pub mod blocks;
pub mod city;
pub mod code_rain;
pub mod core;
pub mod instruments;
pub mod island;
pub mod machine;
pub mod mountains;
pub mod painted;
pub mod road;
pub mod shader;
pub mod snowfield;
pub mod synthwave;
pub mod woodland;
pub mod woods;

use crate::config::{KindInfo, Theme};
use crate::scene::{Edges, Geo, Header, Part, Pass};
use serde::de::DeserializeOwned;
use std::sync::{Arc, OnceLock};
use toml::Table;

pub struct Kind {
    pub name: &'static str,
    /// Its settings and their defaults, as TOML.
    pub defaults: &'static str,
    /// Its list settings that may be of any length.
    pub varies: &'static [&'static str],
    /// Its resolved settings checked: typed, in range.
    pub check: fn(&Table) -> Result<(), String>,
    /// Its parts, from settings `check` passed; or why it cannot be made (a theme file it reads).
    pub make: fn(&str, &Table, &Geo) -> Result<Vec<Box<dyn Part>>, String>,
}

/// Settings a kind checks beyond their types (ranges, relations).
pub trait Validate {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Settings as their typed struct.
pub fn parse<T: DeserializeOwned>(t: &Table) -> Result<T, String> {
    toml::Value::Table(t.clone()).try_into().map_err(|e: toml::de::Error| e.message().to_string())
}

/// A kind's `check`: its settings parse and validate.
pub fn check<T: DeserializeOwned + Validate>(t: &Table) -> Result<(), String> {
    parse::<T>(t)?.validate()
}

/// Settings a kind's `make` reads: they passed `check` when the theme loaded.
pub fn settings<T: DeserializeOwned>(t: &Table) -> T {
    parse(t).unwrap_or_else(|e| unreachable!("settings checked at load: {e}"))
}

/// A setting that must be over 0 (and finite).
pub fn positive(name: &str, v: f64) -> Result<(), String> {
    if v > 0.0 && v.is_finite() { Ok(()) } else { Err(format!("{name} is {v}: it must be over 0")) }
}

/// A list setting each of whose numbers must lie in [lo, hi].
pub fn each(name: &str, vs: &[f64], lo: f64, hi: f64) -> Result<(), String> {
    vs.iter().try_for_each(|&v| within(name, v, lo, hi))
}

/// A `band` setting: [top, bottom], fractions of the picture's height, top first, at least 0.02 apart.
pub fn band(v: [f64; 2]) -> Result<(), String> {
    let [top, bottom] = v;
    if !(0.0 <= top && top < bottom && bottom <= 1.0) {
        return Err("band is [top, bottom], fractions of the picture's height, top first".into());
    }
    if bottom - top < 0.02 {
        return Err("band is at least 0.02 of the picture's height".into());
    }
    Ok(())
}

/// A setting naming a picture of the theme's: it must name one.
pub fn named(name: &str, v: &str) -> Result<(), String> {
    if v.is_empty() { Err(format!("{name}: it names a picture of the theme's")) } else { Ok(()) }
}

/// A setting that must lie in [lo, hi].
pub fn within(name: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
    if (lo..=hi).contains(&v) { Ok(()) } else { Err(format!("{name} is {}: it is {} to {}", crate::util::num(v), crate::util::num(lo), crate::util::num(hi))) }
}

/// A module of element kinds: its kinds, and what it lets go of once a scene is built (caches its
/// parts kept only while the scene was made). A new module is one line in `MODULES`.
struct Module {
    kinds: fn() -> Vec<Kind>,
    built: Option<fn()>,
}

const fn module(kinds: fn() -> Vec<Kind>) -> Module {
    Module { kinds, built: None }
}

const MODULES: &[Module] = &[
    module(core::kinds),
    module(painted::kinds),
    module(machine::kinds),
    module(synthwave::kinds),
    module(city::kinds),
    module(mountains::kinds),
    module(instruments::kinds),
    module(shader::kinds),
    module(arctic::kinds),
    Module { kinds: island::kinds, built: Some(island::built) },
    module(snowfield::kinds),
    module(code_rain::kinds),
    module(road::kinds),
    module(woodland::kinds),
    module(blocks::kinds),
];

/// Every element kind, with its defaults parsed.
fn registry() -> &'static [(Kind, Table)] {
    static ALL: OnceLock<Vec<(Kind, Table)>> = OnceLock::new();
    ALL.get_or_init(|| {
        MODULES
            .iter()
            .flat_map(|m| (m.kinds)())
            .map(|k| {
                let defaults = k.defaults.parse::<Table>().unwrap_or_else(|e| unreachable!("kind {}'s defaults: {e}", k.name));
                (k, defaults)
            })
            .collect()
    })
}

/// Every element kind.
pub fn all() -> impl Iterator<Item = &'static Kind> {
    registry().iter().map(|(k, _)| k)
}

fn find(name: &str) -> Option<&'static (Kind, Table)> {
    registry().iter().find(|(k, _)| k.name == name)
}

/// A kind, for the theme loader.
pub fn kind_info(name: &str) -> Option<KindInfo> {
    let (k, defaults) = find(name)?;
    Some(KindInfo { defaults, varies: k.varies, check: k.check })
}

/// A scene is built: what its elements kept only while building it is let go.
pub fn built() {
    MODULES.iter().filter_map(|m| m.built).for_each(|f| f());
}

/// The scene's parts for a monitor, bottom to top: the header's parts only where it shows them. An
/// element that cannot be made is said and left out.
pub fn build(g: &Geo) -> Vec<Box<dyn Part>> {
    let mut parts: Vec<Box<dyn Part>> = vec![];
    for e in &g.th.scene {
        let Some((k, _)) = find(&e.kind) else { continue };
        match (k.make)(&e.name, &e.settings, g) {
            Ok(made) => parts.extend(made.into_iter().filter(|p| p.shows() == Header::NONE || p.shows().meets(g.header))),
            Err(why) => crate::errln!("element.{}: {why}: left out", e.name),
        }
    }
    parts
}

/// The scene built once, for a full-HD monitor showing the whole header: every element made, and the
/// scene starting with a still element that every monitor shows.
pub fn trial(th: &Arc<Theme>) -> Result<(), String> {
    let g = Geo::new(1920, 1080, "retro", Edges::NONE, Header::ALL, None, th.clone());
    let mut first = None;
    for e in &th.scene {
        let Some((k, _)) = find(&e.kind) else { return Err(format!("element.{}: no kind {:?}", e.name, e.kind)) };
        let parts = (k.make)(&e.name, &e.settings, &g).map_err(|why| format!("element.{}: {why}", e.name))?;
        if first.is_none() {
            first = parts.first().map(|p| p.shows() == Header::NONE && p.passes().first() == Some(&Pass::Still(0)));
        }
    }
    built();
    match first {
        Some(true) => Ok(()),
        _ => Err("its scene must start with a still element (a sky, a painting)".into()),
    }
}
