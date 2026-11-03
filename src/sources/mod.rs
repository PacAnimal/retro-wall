//! The machine's readings. Built-in sources are a table (`BUILTINS`), one file each, read on one
//! thread; plugin sources (plugins.rs) run as programs. Each publishes its last value; the frame
//! loop never waits on them.

mod bat;
mod cpu;
mod disk;
mod net;
mod network;
mod ram;
mod space;
mod swap;
mod traffic;

pub use disk::{demo_disk_ios, disk_ios};
pub use network::{demo as network_demo, spawn_network};
pub use traffic::{Traffic, count_interfaces, demo_rate, net_counts};

use crate::rng::Rng;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const SAMPLE: f64 = 0.5;

/// A source's last reading: 0 to 100 with its indicator (the battery's bolt), or a text. Stale when
/// the last read failed; the value is kept and shown dimmed.
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub value: f64,
    pub indicator: bool,
    pub text: Option<std::sync::Arc<str>>,
    pub stale: bool,
}

/// A built-in measurement's reader.
pub trait Source: Send {
    /// (value, 0 to 100; indicator), or None to keep the last reading.
    fn read(&mut self) -> Option<(f64, bool)>;
}

/// A built-in measurement: one line of `BUILTINS`.
pub struct Builtin {
    pub name: &'static str,
    /// Whether it takes a path after a colon: `used:/home`.
    pub path: bool,
    /// Seconds between readings.
    pub every: f64,
    /// Its value in a snapshot.
    pub fixed: f64,
    /// Its reader, of the machine or made up (--demo), for the path if it takes one; None when that
    /// cannot be read.
    pub make: fn(Option<&str>, bool) -> Option<Box<dyn Source>>,
}

pub const BUILTINS: &[Builtin] = &[cpu::SOURCE, ram::SOURCE, swap::SOURCE, bat::SOURCE, disk::SOURCE, space::USED, space::FREE, net::DOWN, net::UP];

/// The built-in a source name means (any case), and its path.
pub fn builtin(name: &str) -> Option<(&'static Builtin, Option<&str>)> {
    let (base, path) = match name.split_once(':') {
        Some((b, p)) => (b, Some(p)),
        None => (name, None),
    };
    let b = BUILTINS.iter().find(|b| b.name.eq_ignore_ascii_case(base))?;
    (path.is_none() || b.path).then_some((b, path))
}

/// Whether a source can be read here: the battery's charge only where there is one (always with
/// made-up readings); every other source is taken to be.
pub fn present(name: &str, demo: bool) -> bool {
    match builtin(name) {
        Some((b, _)) if b.name == bat::SOURCE.name => demo || bat::present(),
        _ => true,
    }
}

/// The first of `|`-separated alternatives that `present` finds ("bat|swap"); the last if none is.
pub fn first_present(spec: &str, demo: bool) -> &str {
    let mut last = spec;
    for alt in spec.split('|').map(str::trim) {
        if present(alt, demo) {
            return alt;
        }
        last = alt;
    }
    last
}

/// The built-in names, as --list shows them.
pub fn builtin_names() -> Vec<String> {
    BUILTINS.iter().map(|b| if b.path { format!("{}[:PATH]", b.name) } else { b.name.to_string() }).collect()
}

/// A smooth wander between -1 and 1: sines of unrelated periods, (period, phase) each.
pub(crate) fn wave(t: f64, terms: &[(f64, f64)]) -> f64 {
    terms.iter().map(|&(period, phase)| (t * 2.0 * std::f64::consts::PI / period + phase).sin()).sum::<f64>() / terms.len().max(1) as f64
}

/// A repeatable random number in [0, 1) for sample n.
pub(crate) fn jitter(n: i64, seed: i64) -> f64 {
    Rng::new(n * 7919 + seed).f64()
}

static T0: OnceLock<Instant> = OnceLock::new();
/// The made-up readings' clock when held (`hold_demo_time`), as f64 bits; NaN while it runs.
static HELD: AtomicU64 = AtomicU64::new(0x7ff8_0000_0000_0000);

/// Seconds on the made-up readings' clock.
pub(crate) fn demo_time() -> f64 {
    let held = f64::from_bits(HELD.load(Ordering::Relaxed));
    if held.is_nan() { T0.get_or_init(Instant::now).elapsed().as_secs_f64() + 1000.0 } else { held }
}

/// Hold the made-up readings' clock at t, for frames drawn on a clock of their own.
pub fn hold_demo_time(t: f64) {
    HELD.store(t.to_bits(), Ordering::Relaxed);
}

/// Seconds on the clock a source measures its rates by: the made-up readings' own with `demo`.
pub(crate) fn rate_time(demo: bool) -> f64 {
    if demo { demo_time() } else { T0.get_or_init(Instant::now).elapsed().as_secs_f64() + 1000.0 }
}

/// Reads each built-in source a widget asked for, at its pace; made-up readings with `demo`.
pub struct Sampler {
    sources: HashMap<String, (Box<dyn Source>, f64, f64)>,
    others: HashSet<String>,
    demo: bool,
}

impl Sampler {
    pub fn new(demo: bool) -> Sampler {
        Sampler { sources: HashMap::new(), others: HashSet::new(), demo }
    }

    /// Read the sources due at `now` (seconds), first making those newly asked for.
    pub fn step(&mut self, readings: &Mutex<HashMap<String, Reading>>, now: f64) {
        let lock = || readings.lock().unwrap_or_else(|e| e.into_inner());
        let asked = lock().len();
        if asked != self.sources.len() + self.others.len() {
            let wanted: Vec<String> = lock().keys().filter(|k| !self.sources.contains_key(*k) && !self.others.contains(*k)).cloned().collect();
            for name in wanted {
                match builtin(&name) {
                    Some((b, path)) => match (b.make)(path, self.demo) {
                        Some(s) => {
                            self.sources.insert(name, (s, b.every, now));
                        }
                        None => {
                            crate::errln!("source {name}: cannot be read");
                            lock().entry(name.clone()).or_default().stale = true;
                            self.others.insert(name);
                        }
                    },
                    None => {
                        self.others.insert(name); // a plugin's, read on its own thread
                    }
                }
            }
        }
        for (name, (source, every, due)) in self.sources.iter_mut() {
            if now < *due {
                continue;
            }
            *due = now + *every;
            if let Some((value, indicator)) = source.read()
                && let Some(r) = lock().get_mut(name)
            {
                *r = Reading { value: value.clamp(0.0, 100.0), indicator, text: None, stale: false };
            }
        }
    }
}

/// The sampler on a thread of its own: every quarter second while `running`.
pub fn spawn_sampler(readings: Arc<Mutex<HashMap<String, Reading>>>, running: Arc<AtomicBool>, demo: bool) {
    let spawned = std::thread::Builder::new().name("sources".into()).spawn(move || {
        let mut sampler = Sampler::new(demo);
        let t0 = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            if running.load(Ordering::Relaxed) {
                sampler.step(&readings, t0.elapsed().as_secs_f64());
            }
        }
    });
    if let Err(e) = spawned {
        crate::errln!("sources: {e}");
    }
}
