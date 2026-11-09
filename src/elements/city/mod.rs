//! Night city elements: sky, moon, skylines with windows that light and go dark, the waterfront,
//! waves, window reflections and aircraft-warning beacons. Seeded, so identical on every monitor and
//! run; live elements follow the scene clock.

use crate::config::Theme;
use crate::elements::{Validate, check, each, positive, settings, within};
use crate::gfx::{Blend, Gfx, Kind, Tex, Uploads};
use crate::rng::Rng;
use crate::scene::*;
use crate::tweaks::Colour;
use crate::util::*;
use cairo::{Context, Filter, ImageSurface, LinearGradient, Operator, RadialGradient};
use serde::Deserialize;
use std::cell::{OnceCell, RefCell};
use std::collections::{BinaryHeap, HashMap};
use std::f64::consts::PI;
use std::rc::Rc;

mod beacons;
mod lights;
mod sky;
mod skyline;
mod street;
mod water;

use beacons::*;
use lights::*;
use sky::*;
use skyline::*;
use street::*;
use water::*;

const FPS: f64 = 30.0;
const WHITE: Rgb = (1.0, 1.0, 1.0);
const BLACK: Rgb = (0.0, 0.0, 0.0);

/// The street's water line, for the reflections.
const WATERLINE: Key<f64> = Key::new("waterline");
/// The quay's foot along the water, for the waves.
const SHORE: Key<Vec<(f64, f64)>> = Key::new("shore");

fn sun(th: &Theme) -> (f64, f64) {
    let [x, r] = th.tw.sun;
    (x, r)
}

/// Both ends of a range over 0.
fn positive_pair(name: &str, v: [f64; 2]) -> Result<(), String> {
    positive(name, v[0])?;
    positive(name, v[1])
}

fn mix(h: &mut u64, v: u64) {
    *h = (*h ^ v).wrapping_mul(0x100000001b3);
}

/// Hashes an optional frame number, keeping `None` distinct from any value.
fn opt(h: &mut u64, v: Option<i64>) {
    match v {
        Some(v) => {
            mix(h, 1);
            mix(h, v as u64);
        }
        None => mix(h, 0),
    }
}

pub fn kinds() -> Vec<crate::elements::Kind> {
    use crate::elements::Kind;
    vec![
        Kind {
            name: "night-sky",
            defaults: r##"zenith = "#04030a"
middle = "#120a26"
horizon = "#4a1a5e"
glow = 0.55
stars = 60"##,
            varies: &[],
            check: check::<NightSkySettings>,
            make: |_, t, _g| Ok(vec![Box::new(NightSky::new(settings(t)))]),
        },
        Kind {
            name: "moon",
            defaults: r##"colour = "#f3ecff"
shade = "#b9a6e6"
halo = 0.30
rise = 0.62
clouds = "#2a1a4a""##,
            varies: &[],
            check: check::<MoonSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Moon::new(settings(t)))]),
        },
        Kind {
            name: "skyline",
            defaults: r##"seed = 1
heights = [0.08, 0.32]
breadth = [0.025, 0.07]
downtown = 0.62
colour = "#0d0a19"
haze = 0.0
fog = "#3a1650"
rim = "#7a5cc8"
window = 0.0042
glass = "#2a2340"
dark = 0.12"##,
            varies: &[],
            check: check::<SkylineSettings>,
            make: |n, t, _g| Ok(vec![Box::new(Skyline::new(n, settings(t)))]),
        },
        Kind {
            name: "windows",
            defaults: r##"skylines = ["skyline"]
lit = 0.35
period = [120.0, 900.0]
colours = ["#ffd38a", "#ffe9c4", "#bfe6ff", "#ff7ad9", "#6ff3ff"]
alpha = 0.9"##,
            varies: &["skylines", "colours"],
            check: check::<WindowsSettings>,
            make: |n, _, _g| Ok(vec![Box::new(Windows { name: n.into(), bundle: None, glass: None })]),
        },
        Kind {
            name: "street",
            defaults: r##"colour = "#06050c"
sheen = "#3a1650"
mirror = ["skyline-far", "skyline-near"]
reflect = 0.55
moon = "#e6dcff"
glimmer = 0.75
quay = 0.016
stone = "#262038"
lamps = "#ffc978"
spacing = 0.07
sand = "#4a3c48""##,
            varies: &["mirror"],
            check: check::<StreetSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Street::new(settings(t)))]),
        },
        Kind {
            name: "shore",
            defaults: r##"loop = 8.0
rate = 15
alpha = 0.2
foam = 10
colour = "#d8c8ff""##,
            varies: &[],
            check: check::<ShoreSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Shore::new(settings(t)))]),
        },
        Kind {
            name: "reflections",
            defaults: r##"windows = "windows"
stretch = 5.0
alpha = 0.42"##,
            varies: &[],
            check: check::<ReflectionsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Reflections::new(settings(t)))]),
        },
        Kind {
            name: "beacons",
            defaults: r##"skylines = ["skyline-far", "skyline-mid", "skyline-near"]
count = 7
colour = "#ff2a2a"
period = 2.4
lit = 0.25"##,
            varies: &["skylines"],
            check: check::<BeaconsSettings>,
            make: |_, t, _g| Ok(vec![Box::new(Beacons::new(settings(t)))]),
        },
    ]
}
