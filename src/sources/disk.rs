//! disk: the disks' activity, reads and writes a second on a log scale. The tape deck's reels follow
//! the same count.

use super::{Builtin, Source, demo_time, wave};
use crate::platform::{Native, Platform};
use std::sync::Mutex;
use std::time::Instant;

pub const SOURCE: Builtin = Builtin { name: "disk", path: false, every: 0.5, fixed: 33.0, make };

fn make(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(Box::new(Disk { demo, last: None }))
}

struct Disk {
    demo: bool,
    last: Option<(Instant, u64)>,
}

impl Source for Disk {
    fn read(&mut self) -> Option<(f64, bool)> {
        let (now, ios) = (Instant::now(), if self.demo { demo_disk_ios() } else { disk_ios() });
        let v = match self.last {
            Some((t0, i0)) if now > t0 => (25.0 * (1.0 + ios.saturating_sub(i0) as f64 / (now - t0).as_secs_f64()).log10()).min(100.0),
            _ => 0.0,
        };
        self.last = Some((now, ios));
        Some((v, false))
    }
}

/// Reads and writes completed so far.
pub fn disk_ios() -> u64 {
    Native::disk_ios()
}

static DEMO_IO: Mutex<u64> = Mutex::new(0);

/// Made-up activity: bursts a few seconds on and a few off.
pub fn demo_disk_ios() -> u64 {
    let mut io = DEMO_IO.lock().unwrap_or_else(|e| e.into_inner());
    if wave(demo_time(), &[(19.0, 0.5), (8.3, 1.9)]) > -0.25 {
        *io += 1;
    }
    *io
}
