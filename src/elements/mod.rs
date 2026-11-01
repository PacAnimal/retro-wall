//! The element kinds a scene is made of. A kind declares its settings (TOML defaults), checks them into
//! a typed struct, and builds its parts for a monitor. A theme names it: `[element.NAME] type = "KIND"`.

pub mod aurora;
pub mod core;
pub mod guybrush;
pub mod instruments;
pub mod machine;
pub mod matrix;
pub mod mountains;
pub mod night_city;
pub mod painted;
pub mod phosphor;
pub mod shader;
pub mod winter;

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

/// A setting that must lie in [lo, hi].
pub fn within(name: &str, v: f64, lo: f64, hi: f64) -> Result<(), String> {
    if (lo..=hi).contains(&v) { Ok(()) } else { Err(format!("{name} is {}: it is {} to {}", crate::util::num(v), crate::util::num(lo), crate::util::num(hi))) }
}

/// Every element kind, with its defaults parsed.
fn registry() -> &'static [(Kind, Table)] {
    static ALL: OnceLock<Vec<(Kind, Table)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v = vec![];
        v.extend(core::kinds());
        v.extend(painted::kinds());
        v.extend(machine::kinds());
        v.extend(phosphor::kinds());
        v.extend(night_city::kinds());
        v.extend(mountains::kinds());
        v.extend(instruments::kinds());
        v.extend(shader::kinds());
        v.extend(aurora::kinds());
        v.extend(guybrush::kinds());
        v.extend(winter::kinds());
        v.extend(matrix::kinds());
        v.into_iter()
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
    guybrush::built();
}

/// The scene's parts for a monitor, bottom to top: the header's parts only where it shows them. An
/// element that cannot be made is said and left out.
pub fn build(g: &Geo) -> Vec<Box<dyn Part>> {
    let mut parts: Vec<Box<dyn Part>> = vec![];
    for e in &g.th.scene {
        let Some((k, _)) = find(&e.kind) else { continue };
        match (k.make)(&e.name, &e.settings, g) {
            Ok(made) => parts.extend(made.into_iter().filter(|p| p.shows().is_none_or(|s| g.header.contains(s)))),
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
            first = parts.first().map(|p| p.shows().is_none() && p.passes().first() == Some(&Pass::Still(0)));
        }
    }
    built();
    match first {
        Some(true) => Ok(()),
        _ => Err("its scene must start with a still element (a sky, a painting)".into()),
    }
}
