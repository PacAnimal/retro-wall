//! bat: the battery's charge; its indicator lit on mains.

use super::{Builtin, Source};
use crate::platform::{Native, Platform};

pub const SOURCE: Builtin = Builtin { name: "bat", path: false, every: 2.0, fixed: 87.0, make };

fn make(_: Option<&str>, demo: bool) -> Option<Box<dyn Source>> {
    Some(Box::new(Bat { demo }))
}

struct Bat {
    demo: bool,
}

impl Source for Bat {
    fn read(&mut self) -> Option<(f64, bool)> {
        if self.demo {
            return Some((87.0, true));
        }
        let power = Native::power();
        Some((power.charge, !power.on_battery))
    }
}
