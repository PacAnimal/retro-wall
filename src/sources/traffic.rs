//! Network traffic for the mountains: (down, up) bit/s each half second.

use super::{SAMPLE, jitter, wave};
use crate::platform::{Native, Platform};
use std::collections::HashMap;
use std::sync::OnceLock;

static INTERFACES: OnceLock<Vec<String>> = OnceLock::new();

/// Count these interfaces' traffic instead of the physical ones'.
pub fn count_interfaces(names: Vec<String>) {
    let _ = INTERFACES.set(names);
}

/// Made-up traffic at time t, for --demo.
pub fn demo_rate(t: f64) -> (f64, f64) {
    let n = (t / SAMPLE) as i64;
    let burst = wave(t, &[(41.0, 0.3), (17.0, 2.1), (7.3, 4.0)]).max(0.0);
    let down = 10f64.powf(5.3 + 0.6 * jitter(n, 1) + 3.2 * burst.powf(1.5));
    let up = down * (0.06 + 0.1 * jitter(n, 2)) + 10f64.powf(4.6 + 0.8 * jitter(n, 3));
    (down, up)
}

pub fn net_counts() -> HashMap<String, (u64, u64)> {
    Native::net_counts(INTERFACES.get().map_or(&[], |v| v.as_slice()))
}

/// The samples, kept for 1.5 spans.
pub struct Traffic {
    pub columns: i64,
    pub rates: Vec<(f64, f64)>,
    pub counts: Option<HashMap<String, (u64, u64)>>,
    pub sampled: Option<i64>,
    pub version: u64,
    pub newest: Option<i64>,
    pub goal: f64,
    pub held: bool,
    tiers: Vec<i64>,
}

impl Traffic {
    pub fn new(span: f64, tiers: Vec<i64>) -> Traffic {
        let columns = (span / SAMPLE).round() as i64;
        let goal = tiers.first().copied().unwrap_or(6) as f64;
        Traffic { columns, rates: vec![], counts: None, sampled: None, version: 0, newest: None, goal, held: false, tiers }
    }

    fn set_goal(&mut self) {
        let peak = self.rates.iter().map(|r| r.0.max(r.1)).fold(f64::NEG_INFINITY, f64::max);
        let top = self.tiers.last().copied().unwrap_or(6);
        self.goal = self.tiers.iter().copied().find(|&t| 10f64.powi(t as i32) >= peak).unwrap_or(top) as f64;
    }

    /// Take this half second's sample, if not yet taken. Missed samples count as 0.
    pub fn sample(&mut self, t: f64, demo: bool) {
        if self.held {
            return;
        }
        let n = (t / SAMPLE) as i64;
        if let Some(s) = self.sampled
            && n <= s
        {
            return;
        }
        let keep = (1.5 * self.columns as f64).round() as usize;
        let counts = if demo { HashMap::new() } else { net_counts() };
        if let (Some(old), Some(s)) = (&self.counts, self.sampled) {
            let missed = n - s;
            let dt = missed as f64 * SAMPLE;
            let rate = if demo {
                demo_rate(n as f64 * SAMPLE)
            } else {
                let mut sums = [0i128; 2];
                for (k, v) in &counts {
                    if let Some(o) = old.get(k) {
                        sums[0] += v.0 as i128 - o.0 as i128;
                        sums[1] += v.1 as i128 - o.1 as i128;
                    }
                }
                (8.0 * sums[0].max(0) as f64 / dt, 8.0 * sums[1].max(0) as f64 / dt)
            };
            if missed == 1 {
                self.rates.push(rate);
            } else {
                self.rates.extend(std::iter::repeat_n((0.0, 0.0), (missed as usize).min(keep)));
            }
            if self.rates.len() > keep {
                self.rates.drain(..self.rates.len() - keep);
            }
            self.set_goal();
            self.version += 1;
            self.newest = Some(n);
        } else if demo {
            self.rates = (0..keep).map(|i| demo_rate((n - keep as i64 + 1 + i as i64) as f64 * SAMPLE)).collect();
            self.set_goal();
            self.version += 1;
            self.newest = Some(n);
        }
        self.counts = Some(counts);
        self.sampled = Some(n);
    }

    pub fn rate(&self, n: i64) -> Option<(f64, f64)> {
        let i = n - self.newest? - 1;
        let len = self.rates.len() as i64;
        (-len <= i && i < 0).then(|| self.rates[(len + i) as usize])
    }
}
