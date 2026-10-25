//! cpu: the processors' busy time since the last reading.

use super::{Builtin, Source, demo_time, jitter, wave};
use crate::platform::{Native, Platform};

pub const SOURCE: Builtin = Builtin { name: "cpu", path: false, every: 0.25, fixed: 46.0, make };

fn make(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(if demo { Box::new(Demo) } else { Box::new(Cpu(None)) })
}

struct Cpu(Option<(u64, u64)>);

impl Source for Cpu {
    fn read(&mut self) -> Option<(f64, bool)> {
        let (idle, total) = Native::cpu_times()?;
        let v = match self.0 {
            Some((i0, t0)) if total > t0 => Some(100.0 * (1.0 - idle.saturating_sub(i0) as f64 / (total - t0) as f64)),
            _ => None,
        };
        self.0 = Some((idle, total));
        Some((v?, false))
    }
}

struct Demo;

impl Source for Demo {
    fn read(&mut self) -> Option<(f64, bool)> {
        let t = demo_time();
        let busy = 0.34 + 0.22 * wave(t, &[(13.0, 0.0), (5.1, 1.7), (2.3, 3.1)]) + 0.08 * jitter((t * 4.0) as i64, 4);
        Some((100.0 * busy.clamp(0.03, 0.97), false))
    }
}
