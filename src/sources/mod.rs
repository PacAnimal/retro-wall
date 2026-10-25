//! The machine's readings. Built-in sources are a table (`BUILTINS`), one file each, read on one
//! thread; plugin sources (plugins.rs) run as programs. Each publishes its last value; the frame
//! loop never waits on them.

mod bat;
mod cpu;
mod disk;
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
use std::sync::atomic::{AtomicBool, Ordering};
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

pub const BUILTINS: &[Builtin] = &[cpu::SOURCE, ram::SOURCE, swap::SOURCE, bat::SOURCE, disk::SOURCE, space::USED, space::FREE];

/// The built-in a source name means (any case), and its path.
pub fn builtin(name: &str) -> Option<(&'static Builtin, Option<&str>)> {
    let (base, path) = match name.split_once(':') {
        Some((b, p)) => (b, Some(p)),
        None => (name, None),
    };
    let b = BUILTINS.iter().find(|b| b.name.eq_ignore_ascii_case(base))?;
    (path.is_none() || b.path).then_some((b, path))
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

/// Seconds on the made-up readings' clock.
pub(crate) fn demo_time() -> f64 {
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() + 1000.0
}

/// The sampler: every quarter second, reads each built-in source a widget asked for, at its pace.
/// Made-up readings with `demo`.
pub fn spawn_sampler(readings: Arc<Mutex<HashMap<String, Reading>>>, running: Arc<AtomicBool>, demo: bool) {
    let spawned = std::thread::Builder::new().name("sources".into()).spawn(move || {
        let mut sources: HashMap<String, (Box<dyn Source>, Duration, Instant)> = HashMap::new();
        let mut others: HashSet<String> = HashSet::new();
        let lock = || readings.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            std::thread::sleep(Duration::from_millis(250));
            if !running.load(Ordering::Relaxed) {
                continue;
            }
            let asked = lock().len();
            if asked != sources.len() + others.len() {
                let wanted: Vec<String> = lock().keys().filter(|k| !sources.contains_key(*k) && !others.contains(*k)).cloned().collect();
                for name in wanted {
                    match builtin(&name) {
                        Some((b, path)) => match (b.make)(path, demo) {
                            Some(s) => {
                                sources.insert(name, (s, crate::util::seconds(b.every), Instant::now()));
                            }
                            None => {
                                crate::errln!("source {name}: cannot be read");
                                lock().entry(name.clone()).or_default().stale = true;
                                others.insert(name);
                            }
                        },
                        None => {
                            others.insert(name); // a plugin's, read on its own thread
                        }
                    }
                }
            }
            let now = Instant::now();
            for (name, (source, every, due)) in sources.iter_mut() {
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
    });
    if let Err(e) = spawned {
        crate::errln!("sources: {e}");
    }
}
