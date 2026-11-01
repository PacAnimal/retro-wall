//! down and up: the network's traffic, received and sent, on a log scale: 0 at a kilobit a second or
//! less, 100 at a gigabit. The interfaces are the mountains' (`count_interfaces`).

use super::traffic::{demo_rate, net_counts};
use super::{Builtin, Source, demo_time};
use std::time::Instant;

pub const DOWN: Builtin = Builtin { name: "down", path: false, every: 0.5, fixed: 62.0, make: down };
pub const UP: Builtin = Builtin { name: "up", path: false, every: 0.5, fixed: 38.0, make: up };

fn down(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(Box::new(Net { sent: false, demo, last: None }))
}

fn up(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(Box::new(Net { sent: true, demo, last: None }))
}

/// Bits a second on the scale, 0 to 100.
pub fn level(bits: f64) -> f64 {
    (100.0 * (bits.max(1.0).log10() - 3.0) / 6.0).clamp(0.0, 100.0)
}

struct Net {
    sent: bool,
    demo: bool,
    last: Option<(Instant, u64)>,
}

impl Source for Net {
    fn read(&mut self) -> Option<(f64, bool)> {
        if self.demo {
            let (down, up) = demo_rate(demo_time());
            return Some((level(if self.sent { up } else { down }), false));
        }
        let now = Instant::now();
        let bytes = net_counts().values().map(|&(r, s)| if self.sent { s } else { r }).fold(0u64, u64::wrapping_add);
        let v = match self.last {
            Some((t0, b0)) if now > t0 => level(8.0 * bytes.saturating_sub(b0) as f64 / (now - t0).as_secs_f64()),
            _ => 0.0,
        };
        self.last = Some((now, bytes));
        Some((v, false))
    }
}

#[cfg(test)]
mod tests {
    use super::level;

    #[test]
    fn the_scale_runs_from_a_kilobit_to_a_gigabit() {
        assert_eq!(level(0.0), 0.0);
        assert_eq!(level(1e3), 0.0);
        assert!((level(1e6) - 50.0).abs() < 1e-9);
        assert_eq!(level(1e9), 100.0);
        assert_eq!(level(1e12), 100.0);
    }
}
